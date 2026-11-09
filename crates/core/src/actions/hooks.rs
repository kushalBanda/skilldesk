//! Hook on/off (Gate 3 switch table): Codex has a native switch in
//! `config.toml`; Claude, Cursor, Gemini and Copilot entries are removed from
//! their file and kept in the stash; plugin hooks and OpenCode plugin files are
//! refused with the switch that does exist.

use super::stash::{self, StashEntry};
use super::{new_id, ActionPlan, ActionRequest, FileOp};
use crate::adapters;
use crate::core::Core;
use crate::error::{AppError, ErrorKind};
use crate::fsx::json_span::{self, Seg};
use crate::model::{AgentKey, ItemId};
use crate::parse::hooks::{HookPos, RawHook};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

fn invalid(msg: impl Into<String>) -> AppError {
    AppError::new(ErrorKind::InvalidInput, msg)
}

fn display(agent: &str) -> String {
    adapters::find(agent).map(|a| a.display_name.to_string()).unwrap_or_else(|| agent.to_string())
}

fn read_text(path: &PathBuf) -> Result<String, AppError> {
    std::fs::read_to_string(path)
        .map_err(|e| AppError::new(ErrorKind::Io, format!("Cannot read {}: {e}", path.display())))
}

fn event_path(event: &str) -> Vec<Seg> {
    vec![Seg::Key("hooks".into()), Seg::Key(event.into())]
}

fn inner_path(event: &str, group: usize) -> Vec<Seg> {
    vec![Seg::Key("hooks".into()), Seg::Key(event.into()), Seg::Index(group), Seg::Key("hooks".into())]
}

fn edit_failed(path: &Path) -> AppError {
    AppError::new(
        ErrorKind::Parse,
        format!("Could not edit {} without changing other parts of it. Nothing was changed.", path.display()),
    )
}

pub fn build(core: &Core, req: ActionRequest, ids: &[ItemId], on: bool) -> Result<ActionPlan, AppError> {
    if ids.is_empty() {
        return Err(invalid("No hooks chosen."));
    }
    let env = &core.env;
    let (configured, _) = crate::scan::hooks::configured(env);
    let by_id: HashMap<&ItemId, &RawHook> = configured.iter().map(|(id, r)| (id, r)).collect();
    let (stash_text, mut stash_file) = stash::load(env).map_err(|e| {
        AppError::new(ErrorKind::Parse, format!("{} is not valid JSON ({e}). Nothing was changed.", stash::path(env).display()))
    })?;

    let mut agents: Vec<AgentKey> = Vec::new();
    let mut note_agent = |a: &str| {
        let k = AgentKey(a.to_string());
        if !agents.contains(&k) {
            agents.push(k);
        }
    };
    let mut ops: Vec<FileOp> = Vec::new();
    let mut codex_keys: Vec<String> = Vec::new();

    if !on {
        let mut per_file: BTreeMap<PathBuf, Vec<(ItemId, &RawHook)>> = BTreeMap::new();
        for id in ids {
            let raw = by_id.get(id).copied().ok_or_else(|| {
                if stash_file.hooks.iter().any(|e| &e.id == id) {
                    invalid("This hook is already off.")
                } else {
                    AppError::new(ErrorKind::NotFound, "This hook is no longer in its file. Rescan and try again.")
                }
            })?;
            if let Some(p) = &raw.plugin {
                return Err(invalid(format!(
                    "This hook is part of the plugin {}. Turning the plugin off is the only switch.",
                    p.id
                )));
            }
            note_agent(raw.agent);
            match raw.agent {
                "codex" => codex_keys.push(raw.native_key.clone().ok_or_else(|| invalid("Codex hook without a state key."))?),
                "opencode" => return Err(invalid("OpenCode plugin files are not switched here. Remove the file in OpenCode.")),
                _ => per_file.entry(raw.file.clone()).or_default().push((id.clone(), raw)),
            }
        }
        for (file, targets) in per_file {
            let before = read_text(&file)?;
            let value: serde_json::Value =
                serde_json::from_str(&json_span::strip_comments(&before)).map_err(|_| edit_failed(&file))?;
            let (after, entries) = remove_hooks(&before, &value, &file, &targets).ok_or_else(|| edit_failed(&file))?;
            ops.push(FileOp::edit_text(file, Some(&before), Some(&after)));
            stash_file.hooks.extend(entries);
        }
    } else {
        let mut per_file: BTreeMap<PathBuf, Vec<StashEntry>> = BTreeMap::new();
        for id in ids {
            if let Some(i) = stash_file.hooks.iter().position(|e| &e.id == id) {
                let e = stash_file.hooks.remove(i);
                note_agent(&e.agent);
                // A group removed as a whole comes back as a whole.
                if e.whole_group {
                    let same = |o: &StashEntry| {
                        o.whole_group && o.file == e.file && o.pos.event == e.pos.event && o.pos.group == e.pos.group
                            && o.stashed_at == e.stashed_at
                    };
                    let (sib, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut stash_file.hooks).into_iter().partition(same);
                    stash_file.hooks = rest;
                    per_file.entry(e.file.clone()).or_default().extend(sib);
                }
                per_file.entry(e.file.clone()).or_default().push(e);
                continue;
            }
            match by_id.get(id) {
                Some(raw) if raw.agent == "codex" && raw.native_key.is_some() => {
                    note_agent("codex");
                    codex_keys.push(raw.native_key.clone().unwrap_or_default());
                }
                Some(raw) if raw.plugin.is_some() => {
                    return Err(invalid("This hook is part of a plugin. Turn the plugin on to turn it on."))
                }
                Some(_) => return Err(invalid("This hook is already on.")),
                None => return Err(AppError::new(ErrorKind::NotFound, "This hook is no longer known. Rescan and try again.")),
            }
        }
        for (file, mut entries) in per_file {
            let before = read_text(&file)?;
            entries.sort_by(|a, b| (&a.pos.event, a.pos.group, a.pos.hook).cmp(&(&b.pos.event, b.pos.group, b.pos.hook)));
            let mut text = before.clone();
            for e in &entries {
                text = restore_hook(&text, e).ok_or_else(|| edit_failed(&file))?;
            }
            serde_json::from_str::<serde_json::Value>(&json_span::strip_comments(&text)).map_err(|_| edit_failed(&file))?;
            ops.push(FileOp::edit_text(file, Some(&before), Some(&text)));
        }
    }

    if !codex_keys.is_empty() {
        let path = env.home.join(".codex/config.toml");
        let before = read_text(&path)?;
        let mut text = before.clone();
        for k in &codex_keys {
            text = set_codex_enabled(&text, k, on);
        }
        text.parse::<toml::Table>().map_err(|_| edit_failed(&path))?;
        if text != before {
            ops.push(FileOp::edit_text(path, Some(&before), Some(&text)));
        }
    }

    let new_stash = stash::render(&stash_file);
    if new_stash.as_deref() != stash_text.as_deref() && !(stash_text.is_none() && new_stash.is_none()) {
        ops.push(FileOp::edit_text(stash::path(env), stash_text.as_deref(), new_stash.as_deref()));
    }
    if ops.is_empty() {
        return Err(invalid(if on { "These hooks are already on." } else { "These hooks are already off." }));
    }

    let n = ids.len();
    let who = agents.iter().map(|a| display(&a.0)).collect::<Vec<_>>().join(", ");
    let verb = if on { "Turned on" } else { "Turned off" };
    let summary = if n == 1 {
        let name = core.store.item(&ids[0])?.map(|i| i.name).unwrap_or_else(|| "hook".into());
        format!("{verb} {name} for {who}")
    } else {
        format!("{verb} {n} {who} hooks")
    };
    Ok(ActionPlan { id: new_id(), request: req, summary, agents, ops, needs_confirm: false, removed: vec![], untouched: vec![] })
}

/// Removes hooks for good: entries come out of their file (no stash entry),
/// and hooks that are off leave the stash. Undo puts both back.
pub fn remove(core: &Core, req: ActionRequest, ids: &[ItemId]) -> Result<ActionPlan, AppError> {
    if ids.is_empty() {
        return Err(invalid("No hooks chosen."));
    }
    let env = &core.env;
    let (configured, _) = crate::scan::hooks::configured(env);
    let by_id: HashMap<&ItemId, &RawHook> = configured.iter().map(|(id, r)| (id, r)).collect();
    let (stash_text, mut stash_file) = stash::load(env).map_err(|e| {
        AppError::new(ErrorKind::Parse, format!("{} is not valid JSON ({e}). Nothing was changed.", stash::path(env).display()))
    })?;
    let mut agents: Vec<AgentKey> = Vec::new();
    let mut note_agent = |a: &str| {
        let k = AgentKey(a.to_string());
        if !agents.contains(&k) {
            agents.push(k);
        }
    };
    let mut per_file: BTreeMap<PathBuf, Vec<(ItemId, &RawHook)>> = BTreeMap::new();
    for id in ids {
        if let Some(i) = stash_file.hooks.iter().position(|e| &e.id == id) {
            let e = stash_file.hooks.remove(i);
            note_agent(&e.agent);
            // Hooks turned off with their group were stashed as one; they go together.
            if e.whole_group {
                stash_file.hooks.retain(|o| {
                    !(o.whole_group && o.file == e.file && o.pos.event == e.pos.event && o.pos.group == e.pos.group && o.stashed_at == e.stashed_at)
                });
            }
            continue;
        }
        let raw = by_id
            .get(id)
            .copied()
            .ok_or_else(|| AppError::new(ErrorKind::NotFound, "This hook is no longer in its file. Rescan and try again."))?;
        if let Some(p) = &raw.plugin {
            return Err(invalid(format!("This hook is part of the plugin {}. Remove or turn off the plugin instead.", p.id)));
        }
        if raw.agent == "opencode" {
            return Err(invalid("OpenCode hooks live in plugin code. Remove the plugin file in OpenCode."));
        }
        if raw.pos.is_none() {
            return Err(invalid(format!("SkillDesk cannot remove this {} hook from its file safely.", display(raw.agent))));
        }
        note_agent(raw.agent);
        per_file.entry(raw.file.clone()).or_default().push((id.clone(), raw));
    }
    let mut ops: Vec<FileOp> = Vec::new();
    for (file, targets) in per_file {
        let before = read_text(&file)?;
        let value: serde_json::Value = serde_json::from_str(&json_span::strip_comments(&before)).map_err(|_| edit_failed(&file))?;
        let (after, _) = remove_hooks(&before, &value, &file, &targets).ok_or_else(|| edit_failed(&file))?;
        ops.push(FileOp::edit_text(file, Some(&before), Some(&after)));
    }
    let new_stash = stash::render(&stash_file);
    if new_stash.as_deref() != stash_text.as_deref() && !(stash_text.is_none() && new_stash.is_none()) {
        ops.push(FileOp::edit_text(stash::path(env), stash_text.as_deref(), new_stash.as_deref()));
    }
    if ops.is_empty() {
        return Err(invalid("Nothing to remove."));
    }
    let n = ids.len();
    let summary = if n == 1 {
        let name = core.store.item(&ids[0])?.map(|i| i.name).unwrap_or_else(|| "hook".into());
        format!("Removed {name}")
    } else {
        format!("Removed {n} hooks")
    };
    Ok(ActionPlan { id: new_id(), request: req, summary, agents, ops, needs_confirm: true, removed: vec![], untouched: vec![] })
}

/// Removes the targets from `text`, highest index first so earlier indices hold.
/// A nested group whose every hook is targeted goes as a whole.
fn remove_hooks(
    text: &str,
    value: &serde_json::Value,
    file: &Path,
    targets: &[(ItemId, &RawHook)],
) -> Option<(String, Vec<StashEntry>)> {
    // (event, group) → targeted hook indices (None = flat entry).
    type Targets<'a> = Vec<(Option<usize>, &'a ItemId, &'a RawHook)>;
    let mut groups: BTreeMap<(String, usize), Targets> = BTreeMap::new();
    for (id, raw) in targets {
        let pos = raw.pos.as_ref()?;
        groups.entry((pos.event.clone(), pos.group)).or_default().push((pos.hook, id, raw));
    }
    let now = chrono::Utc::now().to_rfc3339();
    let mut out = text.to_string();
    let mut entries = Vec::new();
    for ((event, group), mut hooks) in groups.into_iter().rev() {
        hooks.sort_by_key(|h| std::cmp::Reverse(h.0));
        let inner_len = value
            .pointer(&format!("/hooks/{}/{group}/hooks", event.replace('~', "~0").replace('/', "~1")))
            .and_then(|v| v.as_array())
            .map(|a| a.len());
        let whole = hooks.iter().all(|h| h.0.is_none()) || inner_len == Some(hooks.len());
        let entry = |id: &ItemId, raw: &RawHook, hook: Option<usize>, whole_group: bool, text: String, cut: String| StashEntry {
            id: id.clone(),
            agent: raw.agent.to_string(),
            file: file.to_path_buf(),
            pos: HookPos { event: event.clone(), group, hook },
            whole_group,
            text,
            cut,
            matcher: raw.matcher.clone(),
            command: raw.command.clone(),
            timeout_ms: raw.timeout_ms,
            project: raw.project.clone(),
            stashed_at: now.clone(),
        };
        if whole {
            let (next, removed, cut) = json_span::remove_element(&out, &event_path(&event), group)?;
            out = next;
            // One stash entry per hook; the first carries the group text and
            // restores everything, the rest only record that they are off.
            let mut first = true;
            for (hook, id, raw) in hooks.iter().rev() {
                let (t, c) = if first { (removed.clone(), cut.clone()) } else { (String::new(), String::new()) };
                entries.push(entry(id, raw, *hook, true, t, c));
                first = false;
            }
        } else {
            for (hook, id, raw) in &hooks {
                let (next, removed, cut) = json_span::remove_element(&out, &inner_path(&event, group), (*hook)?)?;
                out = next;
                entries.push(entry(id, raw, *hook, false, removed, cut));
            }
        }
    }
    entries.reverse();
    Some((out, entries))
}

fn restore_hook(text: &str, e: &StashEntry) -> Option<String> {
    let event = &e.pos.event;
    if e.whole_group {
        if e.text.is_empty() {
            return Some(text.to_string()); // restored with its group's first entry
        }
        return json_span::insert_element(text, &event_path(event), e.pos.group, &e.text, Some(&e.cut))
            .or_else(|| json_span::insert_member(text, &[Seg::Key("hooks".into())], event, &format!("[{}]", e.text)));
    }
    let hook = e.pos.hook?;
    let value: serde_json::Value = serde_json::from_str(&json_span::strip_comments(text)).ok()?;
    let group = value.get("hooks")?.get(event).and_then(|v| v.get(e.pos.group));
    let same_group = group.is_some_and(|g| {
        g.get("matcher").and_then(|m| m.as_str()) == e.matcher.as_deref() && g.get("hooks").is_some_and(|h| h.is_array())
    });
    if same_group {
        return json_span::insert_element(text, &inner_path(event, e.pos.group), hook, &e.text, Some(&e.cut));
    }
    // The group is gone or changed: put the hook back in a group of its own.
    let wrapper = match &e.matcher {
        Some(m) => format!("{{\"matcher\": {}, \"hooks\": [{}]}}", serde_json::to_string(m).ok()?, e.text),
        None => format!("{{\"hooks\": [{}]}}", e.text),
    };
    json_span::insert_element(text, &event_path(event), e.pos.group, &wrapper, None)
        .or_else(|| json_span::insert_member(text, &[Seg::Key("hooks".into())], event, &format!("[{wrapper}]")))
}

/// Sets `enabled` under `[hooks.state."<key>"]`, touching only that line, or
/// adds the line or the table when missing.
pub fn set_codex_enabled(text: &str, key: &str, on: bool) -> String {
    set_toml_enabled(text, &format!("[hooks.state.{}]", toml_quote(key)), on)
}

/// Sets `enabled` in the TOML table whose header line is `header`.
pub fn set_toml_enabled(text: &str, header: &str, on: bool) -> String {
    let header = header.to_string();
    let value = if on { "true" } else { "false" };
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    if let Some(h) = lines.iter().position(|l| l.trim() == header) {
        let mut out: String = lines[..=h].concat();
        let mut done = false;
        for l in &lines[h + 1..] {
            let t = l.trim_start();
            if !done && t.starts_with('[') {
                out.push_str(&format!("enabled = {value}\n"));
                done = true;
            }
            if !done && t.starts_with("enabled") && t[7..].trim_start().starts_with('=') {
                let indent = &l[..l.len() - t.len()];
                let nl = if l.ends_with('\n') { "\n" } else { "" };
                out.push_str(&format!("{indent}enabled = {value}{nl}"));
                done = true;
                continue;
            }
            out.push_str(l);
        }
        if !done {
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&format!("enabled = {value}\n"));
        }
        return out;
    }
    let mut out = text.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&format!("\n{header}\nenabled = {value}\n"));
    out
}

pub fn toml_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::Env;

    const CURSOR: &str = "{\n  \"version\": 1,\n  // keep me\n  \"hooks\": {\n    \"beforeSubmitPrompt\": [\n      {\n        \"command\": \"a\"\n      },\n      {\n        \"command\": \"b\", \"timeout\": 5\n      },\n      {\n        \"command\": \"c\"\n      }\n    ],\n    \"stop\": [ { \"command\": \"a\" } ]\n  }\n}\n";
    const CLAUDE: &str = "{\n  \"model\": \"x\",\n  \"hooks\": {\n    \"PreToolUse\": [\n      {\n        \"matcher\": \"Bash\",\n        \"hooks\": [\n          { \"type\": \"command\", \"command\": \"one\" },\n          { \"type\": \"command\", \"command\": \"two\" }\n        ]\n      }\n    ]\n  },\n  \"theme\": \"dark\"\n}\n";

    fn setup() -> (tempfile::TempDir, std::sync::Arc<Core>) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().canonicalize().unwrap();
        std::fs::create_dir_all(home.join(".cursor")).unwrap();
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::write(home.join(".cursor/hooks.json"), CURSOR).unwrap();
        std::fs::write(home.join(".claude/settings.json"), CLAUDE).unwrap();
        let core = Core::open(Env::for_test(&home, &home.join("data"))).unwrap();
        (tmp, core)
    }

    fn ids(core: &Core, file: &str, pick: impl Fn(&RawHook) -> bool) -> Vec<ItemId> {
        crate::scan::hooks::configured(&core.env)
            .0
            .into_iter()
            .filter(|(_, r)| r.file.ends_with(file) && pick(r))
            .map(|(id, _)| id)
            .collect()
    }

    fn run(core: &Core, ids: Vec<ItemId>, on: bool) -> super::super::ActionRecord {
        let plan = core.action_plan(ActionRequest::HookSetEnabled { ids, on }).unwrap();
        core.action_apply(&plan.id).unwrap()
    }

    /// Bytes outside the removed entry (comments, order, spacing) stay as they
    /// were, and Undo gives back the file byte for byte.
    #[test]
    fn apply_json_edit_preserves_bytes_outside_edit() {
        let (_t, core) = setup();
        let path = core.env.home.join(".cursor/hooks.json");
        let b = ids(&core, ".cursor/hooks.json", |r| r.command == "b");
        let rec = run(&core, b, false);
        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            after,
            CURSOR.replace("{\n        \"command\": \"b\", \"timeout\": 5\n      },\n      ", ""),
            "only the entry and its separator are gone"
        );
        core.action_undo(&rec.id).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), CURSOR);
        assert!(!stash::path(&core.env).exists(), "stash removed again on undo");
    }

    /// Off then on (a new action, not Undo) puts each hook back at the same
    /// index with the same JSON, for flat and nested shapes.
    #[test]
    fn stash_roundtrip_restores_exact_json_at_same_index() {
        let (_t, core) = setup();
        let cursor = core.env.home.join(".cursor/hooks.json");
        let claude = core.env.home.join(".claude/settings.json");
        let mut off = ids(&core, ".cursor/hooks.json", |r| r.command != "b");
        off.extend(ids(&core, ".claude/settings.json", |r| r.command == "two"));
        run(&core, off.clone(), false);
        let v: serde_json::Value =
            serde_json::from_str(&json_span::strip_comments(&std::fs::read_to_string(&cursor).unwrap())).unwrap();
        assert_eq!(v["hooks"]["beforeSubmitPrompt"].as_array().unwrap().len(), 1);
        assert_eq!(v["hooks"]["stop"].as_array().unwrap().len(), 0);
        assert_eq!(stash::read(&core.env).len(), 4);

        run(&core, off, true);
        assert_eq!(std::fs::read_to_string(&claude).unwrap(), CLAUDE);
        let restored: serde_json::Value =
            serde_json::from_str(&json_span::strip_comments(&std::fs::read_to_string(&cursor).unwrap())).unwrap();
        let original: serde_json::Value = serde_json::from_str(&CURSOR.replace("  // keep me\n", "")).unwrap();
        assert_eq!(restored, original);
        assert!(stash::read(&core.env).is_empty());
    }

    /// Remove takes the hook out for good (no stash entry); Undo gives the
    /// file back byte for byte.
    #[test]
    fn remove_then_undo_restores_file() {
        let (_t, core) = setup();
        let path = core.env.home.join(".claude/settings.json");
        let two = ids(&core, ".claude/settings.json", |r| r.command == "two");
        let plan = core.action_plan(ActionRequest::HookRemove { ids: two }).unwrap();
        let rec = core.action_apply(&plan.id).unwrap();
        assert!(!std::fs::read_to_string(&path).unwrap().contains("\"two\""));
        assert!(stash::read(&core.env).is_empty(), "removed, not stashed");
        core.action_undo(&rec.id).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), CLAUDE);
    }
}
