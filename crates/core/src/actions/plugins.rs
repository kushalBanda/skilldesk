//! Plugin on/off and forget (`~/.claude/settings.json` `enabledPlugins`),
//! marketplace removal and cache cleaning (folders go to the Trash).
//! `~/.claude.json` is never written: Claude rewrites it while running.

use super::{new_id, ActionPlan, ActionRequest, FileOp};
use crate::core::Core;
use crate::error::{AppError, ErrorKind};
use crate::fsx::json_span::{self, Seg};
use crate::model::{AgentKey, Item, ItemId, ItemStatus, KindFacts};
use std::path::{Path, PathBuf};

fn invalid(msg: impl Into<String>) -> AppError {
    AppError::new(ErrorKind::InvalidInput, msg)
}

fn item(core: &Core, id: &ItemId) -> Result<Item, AppError> {
    core.store
        .item(id)?
        .ok_or_else(|| AppError::new(ErrorKind::NotFound, "This item is no longer in the inventory. Rescan and try again."))
}

fn read(path: &Path) -> Result<Option<String>, AppError> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(AppError::new(ErrorKind::Io, format!("Cannot read {}: {e}", path.display()))),
    }
}

fn unchanged_elsewhere(path: &Path) -> AppError {
    AppError::new(ErrorKind::Parse, format!("Could not edit {} safely. Nothing was changed.", path.display()))
}

fn check_json(path: &Path, text: &str) -> Result<(), AppError> {
    serde_json::from_str::<serde_json::Value>(&json_span::strip_comments(text))
        .map(|_| ())
        .map_err(|_| unchanged_elsewhere(path))
}

/// A free name in `~/.Trash` (Finder style: append the time when taken).
pub fn trash_target(core: &Core, path: &Path) -> PathBuf {
    let trash = core.env.home.join(".Trash");
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "item".into());
    let first = trash.join(&name);
    if std::fs::symlink_metadata(&first).is_err() {
        return first;
    }
    let stamp = chrono::Local::now().format("%H.%M.%S %f");
    trash.join(format!("{name} {stamp}"))
}

fn plan(req: ActionRequest, summary: String, ops: Vec<FileOp>) -> ActionPlan {
    ActionPlan { id: new_id(), request: req, summary, agents: vec![AgentKey("claude_code".into())], ops, needs_confirm: false, removed: vec![], untouched: vec![] }
}

pub fn set_enabled(core: &Core, req: ActionRequest, id: &ItemId, on: bool) -> Result<ActionPlan, AppError> {
    let it = item(core, id)?;
    let KindFacts::Plugin(f) = &it.facts else { return Err(invalid("Not a plugin.")) };
    if !it.agents.iter().any(|a| a.0 == "claude_code") {
        let who = it.agents.first().map(|a| a.0.clone()).unwrap_or_default();
        if f.enabled == Some(on) {
            return Err(invalid(if on { "This plugin is already on." } else { "This plugin is already off." }));
        }
        return match who.as_str() {
            "gemini_cli" => gemini_set(core, req, &it, on),
            "opencode" => opencode_set(core, req, &it, f, on),
            _ => Err(invalid(format!(
                "{} keeps this switch in its own database, which SkillDesk never writes. Turn it on or off in {}.",
                crate::adapters::find(&who).map(|a| a.display_name).unwrap_or(&who),
                crate::adapters::find(&who).map(|a| a.display_name).unwrap_or(&who)
            ))),
        };
    }
    if !f.installed {
        return Err(invalid("This plugin is not installed. Install it in Claude Code first."));
    }
    if f.project.is_some() {
        return Err(invalid("This plugin is installed for one project; switch it in that project's Claude settings."));
    }
    if f.enabled == Some(on) {
        return Err(invalid(if on { "This plugin is already on." } else { "This plugin is already off." }));
    }
    let path = core.env.home.join(".claude/settings.json");
    let before = read(&path)?.unwrap_or_else(|| "{}\n".into());
    let value = if on { "true" } else { "false" };
    let after = json_span::set_member(&before, &[Seg::Key("enabledPlugins".into())], &f.id, value)
        .or_else(|| {
            let obj = format!("{{{}: {value}}}", serde_json::to_string(&f.id).ok()?);
            json_span::set_member(&before, &[], "enabledPlugins", &obj)
        })
        .ok_or_else(|| unchanged_elsewhere(&path))?;
    check_json(&path, &after)?;
    let had = read(&path)?;
    Ok(plan(
        req,
        format!("Turned {} {}", if on { "on" } else { "off" }, it.name),
        vec![FileOp::edit_text(path, had.as_deref(), Some(&after))],
    ))
}

pub fn forget(core: &Core, req: ActionRequest, id: &ItemId) -> Result<ActionPlan, AppError> {
    let it = item(core, id)?;
    let KindFacts::Plugin(f) = &it.facts else { return Err(invalid("Not a plugin.")) };
    if f.installed {
        return Err(invalid("This plugin is installed. Uninstall it in Claude Code to remove it."));
    }
    let path = core.env.home.join(".claude/settings.json");
    let before = read(&path)?.ok_or_else(|| invalid("Claude has no settings file, so there is nothing to forget."))?;
    let after = json_span::remove_member(&before, &[Seg::Key("enabledPlugins".into())], &f.id).ok_or_else(|| {
        invalid("Claude settings no longer list this plugin. Its use count stays in Claude's own records.")
    })?;
    check_json(&path, &after)?;
    Ok(plan(req, format!("Forgot {}", it.name), vec![FileOp::edit_text(path, Some(&before), Some(&after))]))
}

pub fn remove_marketplaces(core: &Core, req: ActionRequest, ids: &[ItemId]) -> Result<ActionPlan, AppError> {
    let plugins = core.env.home.join(".claude/plugins");
    let known = plugins.join("known_marketplaces.json");
    let settings = core.env.home.join(".claude/settings.json");
    let known_before = read(&known)?.ok_or_else(|| invalid("Claude lists no marketplaces."))?;
    let settings_before = read(&settings)?;
    let (mut known_text, mut settings_text) = (known_before.clone(), settings_before.clone());
    let mut moves = Vec::new();
    let mut names = Vec::new();
    for id in ids {
        let it = item(core, id)?;
        let KindFacts::Marketplace(f) = &it.facts else { return Err(invalid("Not a marketplace.")) };
        if it.status == ItemStatus::Ok {
            return Err(invalid(format!("{} still supplies {}. Uninstall those first.", it.name, f.plugins.join(", "))));
        }
        known_text = json_span::remove_member(&known_text, &[], &it.name).ok_or_else(|| unchanged_elsewhere(&known))?;
        if f.in_settings {
            if let Some(t) = settings_text.as_deref() {
                settings_text = Some(
                    json_span::remove_member(t, &[Seg::Key("extraKnownMarketplaces".into())], &it.name)
                        .ok_or_else(|| unchanged_elsewhere(&settings))?,
                );
            }
        }
        // Only clones in Claude's own folder are trashed; a local repo is left alone.
        if let (true, Some(loc)) = (f.removable, &f.install_location) {
            moves.push(FileOp::MovePath { from: loc.clone(), to: trash_target(core, loc) });
        }
        names.push(it.name.clone());
    }
    check_json(&known, &known_text)?;
    let mut ops = vec![FileOp::edit_text(known, Some(&known_before), Some(&known_text))];
    if settings_text != settings_before {
        let t = settings_text.unwrap_or_default();
        check_json(&settings, &t)?;
        ops.push(FileOp::edit_text(settings, settings_before.as_deref(), Some(&t)));
    }
    ops.extend(moves);
    let summary = if names.len() == 1 {
        format!("Removed marketplace {}", names[0])
    } else {
        format!("Removed {} marketplaces", names.len())
    };
    Ok(plan(req, summary, ops))
}

pub fn clean_cache(core: &Core, req: ActionRequest, ids: &[ItemId]) -> Result<ActionPlan, AppError> {
    let mut ops = Vec::new();
    let mut bytes = 0;
    let mut taken: Vec<PathBuf> = Vec::new();
    for id in ids {
        let it = item(core, id)?;
        let KindFacts::PluginCache(f) = &it.facts else { return Err(invalid("Not a plugin cache folder.")) };
        let mut to = trash_target(core, &f.path);
        while taken.contains(&to) {
            to = to.with_file_name(format!("{} {}", to.file_name().unwrap_or_default().to_string_lossy(), taken.len()));
        }
        taken.push(to.clone());
        bytes += f.size_bytes;
        ops.push(FileOp::MovePath { from: f.path.clone(), to });
    }
    if ops.is_empty() {
        return Err(invalid("Nothing to clean."));
    }
    let mb = bytes as f64 / 1_048_576.0;
    Ok(plan(req, format!("Moved {} old plugin folders to the Trash ({mb:.1} MB)", ops.len()), ops))
}

fn plan_for(req: ActionRequest, agent: &str, summary: String, ops: Vec<FileOp>) -> ActionPlan {
    ActionPlan { id: new_id(), request: req, summary, agents: vec![AgentKey(agent.into())], ops, needs_confirm: false, removed: vec![], untouched: vec![] }
}

/// Gemini CLI: `~/.gemini/extensions/extension-enablement.json`, the same
/// entry `gemini extensions enable|disable` writes (user level: `<home>/*`).
fn gemini_set(core: &Core, req: ActionRequest, it: &Item, on: bool) -> Result<ActionPlan, AppError> {
    let path = core.env.home.join(".gemini/extensions/extension-enablement.json");
    let had = read(&path)?;
    let before = had.clone().unwrap_or_else(|| "{}\n".into());
    let scope = format!("{}{}/*", if on { "" } else { "!" }, core.env.home.display());
    let value = format!("{{\"overrides\": [{}]}}", serde_json::to_string(&scope).unwrap_or_default());
    let after = json_span::set_member(&before, &[], &it.name, &value).ok_or_else(|| unchanged_elsewhere(&path))?;
    check_json(&path, &after)?;
    Ok(plan_for(
        req,
        "gemini_cli",
        format!("Turned {} {} for Gemini CLI", if on { "on" } else { "off" }, it.name),
        vec![FileOp::edit_text(path, had.as_deref(), Some(&after))],
    ))
}

/// OpenCode: a code file in `plugins/` has no off switch (OpenCode loads every
/// code file there), so off moves it to the Trash (Undo brings it back); a
/// folder plugin is switched by its entry in the config's `plugin` list.
fn opencode_set(core: &Core, req: ActionRequest, it: &Item, f: &crate::model::PluginFacts, on: bool) -> Result<ActionPlan, AppError> {
    let summary = format!("Turned {} {} for OpenCode", if on { "on" } else { "off" }, it.name);
    let cfg_dir = core.env.home.join(".config/opencode");
    let Some(root) = f.install_path.clone() else {
        return Err(invalid("This OpenCode plugin is an npm package named in opencode.json. Remove it from that list in OpenCode."));
    };
    if root.is_file() {
        if on {
            return Err(invalid("This OpenCode plugin file is already on."));
        }
        let to = trash_target(core, &root);
        let summary = format!("Moved {} to the Trash for OpenCode", it.name);
        let mut p = plan_for(req, "opencode", summary, vec![FileOp::MovePath { from: root, to }]);
        p.needs_confirm = true;
        return Ok(p);
    }
    // Folder plugin: find the entry that points into it, in opencode.json(c) or tui.json.
    let files: Vec<PathBuf> = ["opencode.json", "opencode.jsonc", "tui.json"].iter().map(|n| cfg_dir.join(n)).filter(|p| p.is_file()).collect();
    let path_of = |s: &str| {
        let s = s.trim_start_matches("file://");
        if s.starts_with('/') { PathBuf::from(s) } else { cfg_dir.join(s) }
    };
    if !on {
        for file in &files {
            let text = read(file)?.unwrap_or_default();
            let v: serde_json::Value = serde_json::from_str(&json_span::strip_comments(&text)).map_err(|_| unchanged_elsewhere(file))?;
            let list = v.get("plugin").and_then(|p| p.as_array()).cloned().unwrap_or_default();
            if let Some(i) = list.iter().position(|e| e.as_str().is_some_and(|s| path_of(s).starts_with(&root))) {
                let (after, _, _) = json_span::remove_element(&text, &[Seg::Key("plugin".into())], i).ok_or_else(|| unchanged_elsewhere(file))?;
                check_json(file, &after)?;
                return Ok(plan_for(req, "opencode", summary, vec![FileOp::edit_text(file.clone(), Some(&text), Some(&after))]));
            }
        }
        return Err(invalid("No OpenCode config lists this plugin, so it is already off."));
    }
    // On: list its entry file in opencode.json (package.json `main`, else index or plugin file).
    let main = read_main(&root).ok_or_else(|| invalid("Cannot tell which file starts this plugin. Add it to opencode.json in OpenCode."))?;
    let entry = format!("./{}", main.strip_prefix(&cfg_dir).unwrap_or(&main).display());
    let file = files.iter().find(|p| !p.ends_with("tui.json")).cloned().unwrap_or_else(|| cfg_dir.join("opencode.json"));
    let had = read(&file)?;
    let before = had.clone().unwrap_or_else(|| "{}\n".into());
    let quoted = serde_json::to_string(&entry).unwrap_or_default();
    let n = json_span::array_at(&before, &[Seg::Key("plugin".into())]).map(|a| a.elems.len());
    let after = match n {
        Some(n) => json_span::insert_element(&before, &[Seg::Key("plugin".into())], n, &quoted, None),
        None => json_span::insert_member(&before, &[], "plugin", &format!("[{quoted}]")),
    }
    .ok_or_else(|| unchanged_elsewhere(&file))?;
    check_json(&file, &after)?;
    Ok(plan_for(req, "opencode", summary, vec![FileOp::edit_text(file, had.as_deref(), Some(&after))]))
}

fn read_main(root: &Path) -> Option<PathBuf> {
    let pkg = std::fs::read_to_string(root.join("package.json")).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
    if let Some(m) = pkg.as_ref().and_then(|v| v.get("main")?.as_str().map(str::to_string)) {
        let p = root.join(m.trim_start_matches("./"));
        if p.is_file() {
            return Some(p);
        }
    }
    ["plugin.js", "plugin.ts", "index.js", "index.ts", "src/index.ts", "src/index.js"].iter().map(|n| root.join(n)).find(|p| p.is_file())
}
