//! Hooks, edited in the agent's own files only.
//! - On/off: only where the agent has its own switch (Codex `hooks.state` in
//!   `config.toml`).
//! - Remove: Claude, Cursor, Gemini and Copilot entries come out of their file
//!   (Undo puts them back). These agents have no per-hook off switch.
//! - Plugin hooks and OpenCode plugin files are refused with the switch that
//!   does exist.

use super::{new_id, ActionPlan, ActionRequest, FileOp};
use crate::adapters;
use crate::core::Core;
use crate::error::{AppError, ErrorKind};
use crate::fsx::json_span::{self, Seg};
use crate::model::{AgentKey, ItemId};
use crate::parse::hooks::RawHook;
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
    let mut agents: Vec<AgentKey> = Vec::new();
    let mut codex_keys: Vec<String> = Vec::new();
    for id in ids {
        let raw = by_id
            .get(id)
            .copied()
            .ok_or_else(|| AppError::new(ErrorKind::NotFound, "This hook is no longer in its file. Rescan and try again."))?;
        if let Some(p) = &raw.plugin {
            return Err(invalid(format!("This hook is part of the plugin {}. Turning the plugin on or off is the only switch.", p.id)));
        }
        match raw.agent {
            "codex" => codex_keys.push(raw.native_key.clone().ok_or_else(|| invalid("Codex hook without a state key."))?),
            "opencode" => return Err(invalid("OpenCode hooks live in plugin code. Remove the plugin file in OpenCode.")),
            a => {
                return Err(invalid(format!(
                    "{} has no off switch for one hook. Remove it instead; Undo puts it back.",
                    display(a)
                )))
            }
        }
        let k = AgentKey(raw.agent.to_string());
        if !agents.contains(&k) {
            agents.push(k);
        }
    }
    let path = env.home.join(".codex/config.toml");
    let before = read_text(&path)?;
    let mut text = before.clone();
    for k in &codex_keys {
        text = set_codex_enabled(&text, k, on);
    }
    text.parse::<toml::Table>().map_err(|_| edit_failed(&path))?;
    if text == before {
        return Err(invalid(if on { "These hooks are already on." } else { "These hooks are already off." }));
    }
    let ops = vec![FileOp::edit_text(path, Some(&before), Some(&text))];
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

/// Removes hooks from their file. Undo puts the file back byte for byte.
pub fn remove(core: &Core, req: ActionRequest, ids: &[ItemId]) -> Result<ActionPlan, AppError> {
    if ids.is_empty() {
        return Err(invalid("No hooks chosen."));
    }
    let env = &core.env;
    let (configured, _) = crate::scan::hooks::configured(env);
    let by_id: HashMap<&ItemId, &RawHook> = configured.iter().map(|(id, r)| (id, r)).collect();
    let mut agents: Vec<AgentKey> = Vec::new();
    let mut per_file: BTreeMap<PathBuf, Vec<&RawHook>> = BTreeMap::new();
    for id in ids {
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
        let k = AgentKey(raw.agent.to_string());
        if !agents.contains(&k) {
            agents.push(k);
        }
        per_file.entry(raw.file.clone()).or_default().push(raw);
    }
    let mut ops: Vec<FileOp> = Vec::new();
    for (file, targets) in per_file {
        let before = read_text(&file)?;
        let value: serde_json::Value = serde_json::from_str(&json_span::strip_comments(&before)).map_err(|_| edit_failed(&file))?;
        let after = remove_hooks(&before, &value, &targets).ok_or_else(|| edit_failed(&file))?;
        ops.push(FileOp::edit_text(file, Some(&before), Some(&after)));
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
fn remove_hooks(text: &str, value: &serde_json::Value, targets: &[&RawHook]) -> Option<String> {
    // (event, group) → targeted hook indices (None = flat entry).
    let mut groups: BTreeMap<(String, usize), Vec<Option<usize>>> = BTreeMap::new();
    for raw in targets {
        let pos = raw.pos.as_ref()?;
        groups.entry((pos.event.clone(), pos.group)).or_default().push(pos.hook);
    }
    let mut out = text.to_string();
    for ((event, group), mut hooks) in groups.into_iter().rev() {
        hooks.sort_by_key(|h| std::cmp::Reverse(*h));
        let inner_len = value
            .pointer(&format!("/hooks/{}/{group}/hooks", event.replace('~', "~0").replace('/', "~1")))
            .and_then(|v| v.as_array())
            .map(|a| a.len());
        let whole = hooks.iter().all(|h| h.is_none()) || inner_len == Some(hooks.len());
        if whole {
            out = json_span::remove_element(&out, &event_path(&event), group)?.0;
        } else {
            for hook in &hooks {
                out = json_span::remove_element(&out, &inner_path(&event, group), (*hook)?)?.0;
            }
        }
    }
    Some(out)
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

    fn remove(core: &Core, ids: Vec<ItemId>) -> super::super::ActionRecord {
        let plan = core.action_plan(ActionRequest::HookRemove { ids }).unwrap();
        core.action_apply(&plan.id).unwrap()
    }

    /// Bytes outside the removed entry (comments, order, spacing) stay as they
    /// were, and Undo gives back the file byte for byte.
    #[test]
    fn apply_json_edit_preserves_bytes_outside_edit() {
        let (_t, core) = setup();
        let path = core.env.home.join(".cursor/hooks.json");
        let b = ids(&core, ".cursor/hooks.json", |r| r.command == "b");
        let rec = remove(&core, b);
        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            after,
            CURSOR.replace("{\n        \"command\": \"b\", \"timeout\": 5\n      },\n      ", ""),
            "only the entry and its separator are gone"
        );
        core.action_undo(&rec.id).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), CURSOR);
    }

    /// Agents with no per-hook switch get no "off": nothing is written
    /// outside their own files.
    #[test]
    fn off_without_native_switch_is_refused() {
        let (_t, core) = setup();
        let b = ids(&core, ".cursor/hooks.json", |r| r.command == "b");
        let err = core.action_plan(ActionRequest::HookSetEnabled { ids: b, on: false }).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        assert_eq!(std::fs::read_to_string(core.env.home.join(".cursor/hooks.json")).unwrap(), CURSOR);
    }

    /// Remove takes the hook out of a nested group; Undo gives the
    /// file back byte for byte.
    #[test]
    fn remove_then_undo_restores_file() {
        let (_t, core) = setup();
        let path = core.env.home.join(".claude/settings.json");
        let two = ids(&core, ".claude/settings.json", |r| r.command == "two");
        let plan = core.action_plan(ActionRequest::HookRemove { ids: two }).unwrap();
        let rec = core.action_apply(&plan.id).unwrap();
        assert!(!std::fs::read_to_string(&path).unwrap().contains("\"two\""));
        core.action_undo(&rec.id).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), CLAUDE);
    }
}
