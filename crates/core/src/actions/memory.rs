//! Memory actions (DESIGN §7.8): edit, forget (Trash), copy to an agent's
//! global rules file, promote several memories into one global rule.
//!
//! Forget and promote move the memory file to the Trash and drop its line
//! from `MEMORY.md`, so Claude stops loading it. Rules go under one
//! "## Rules from memory" heading in each agent's global instructions file.

use super::{new_id, ActionPlan, ActionRequest, FileOp};
use crate::adapters;
use crate::core::Core;
use crate::error::{AppError, ErrorKind};
use crate::model::{AgentKey, Item, ItemId, KindFacts, MemoryFacts};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use ts_rs::TS;

pub const RULES_HEADING: &str = "## Rules from memory";

fn invalid(msg: impl Into<String>) -> AppError {
    AppError::new(ErrorKind::InvalidInput, msg)
}

/// An agent's global instructions file, where a promoted rule goes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RuleTarget {
    pub agent: AgentKey,
    pub display_name: String,
    #[ts(type = "string")]
    pub path: PathBuf,
    pub exists: bool,
}

/// Agents with a plain-text global instructions file, installed on this Mac.
/// Cursor keeps user rules in its database, so it is not offered.
pub fn rule_targets(core: &Core) -> Vec<RuleTarget> {
    let h = &core.env.home;
    let all: [(&str, PathBuf); 4] = [
        ("claude_code", h.join(".claude/CLAUDE.md")),
        ("codex", h.join(".codex/AGENTS.md")),
        ("gemini_cli", h.join(".gemini/GEMINI.md")),
        ("opencode", core.env.resolve(".config/opencode/AGENTS.md")),
    ];
    all.into_iter()
        .filter_map(|(key, path)| {
            let a = adapters::find(key).filter(|a| a.is_installed(&core.env))?;
            Some(RuleTarget { agent: AgentKey(key.into()), display_name: a.display_name.to_string(), exists: path.is_file(), path })
        })
        .collect()
}

fn memory(core: &Core, id: &ItemId) -> Result<(Item, MemoryFacts, PathBuf), AppError> {
    let item = core
        .store
        .item(id)?
        .ok_or_else(|| AppError::new(ErrorKind::NotFound, "This memory is no longer in the inventory. Rescan and try again."))?;
    let KindFacts::Memory(f) = item.facts.clone() else { return Err(invalid("Not a memory.")) };
    let path = item.sources.first().map(|s| s.path.clone()).ok_or_else(|| invalid("This memory has no file."))?;
    Ok((item, f, path))
}

fn read(path: &Path) -> Result<Option<String>, AppError> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(AppError::new(ErrorKind::Io, format!("Cannot read {}: {e}", path.display()))),
    }
}

fn plan(req: ActionRequest, summary: String, agents: Vec<AgentKey>, ops: Vec<FileOp>, needs_confirm: bool) -> ActionPlan {
    ActionPlan { id: new_id(), request: req, summary, agents, ops, needs_confirm, removed: vec![], untouched: vec![] }
}

fn claude() -> AgentKey {
    AgentKey("claude_code".into())
}

pub fn edit(core: &Core, req: ActionRequest, id: &ItemId, text: &str) -> Result<ActionPlan, AppError> {
    let (item, _, path) = memory(core, id)?;
    if text.trim().is_empty() {
        return Err(invalid("A memory cannot be empty. Use Forget to remove it."));
    }
    let before = read(&path)?.ok_or_else(|| invalid("This memory file is gone. Rescan and try again."))?;
    if before == text {
        return Err(invalid("Nothing changed."));
    }
    let op = FileOp::edit_text(path, Some(&before), Some(text));
    let agents = item.agents.clone();
    Ok(plan(req, format!("Edited memory {}", item.name), agents, vec![op], false))
}

pub fn forget(core: &Core, req: ActionRequest, ids: &[ItemId]) -> Result<ActionPlan, AppError> {
    if ids.is_empty() {
        return Err(invalid("No memories chosen."));
    }
    let ops = trash_ops(core, ids)?;
    let summary = if ids.len() == 1 {
        format!("Forgot memory {}", memory(core, &ids[0])?.0.name)
    } else {
        format!("Forgot {} memories", ids.len())
    };
    Ok(plan(req, summary, vec![claude()], ops, false))
}

pub fn copy_to_agent(core: &Core, req: ActionRequest, id: &ItemId, agent: &AgentKey, text: &str) -> Result<ActionPlan, AppError> {
    let (item, _, _) = memory(core, id)?;
    let rule = one_line(text)?;
    let t = target(core, agent)?;
    let op = add_rule(&t.path, &rule)?.ok_or_else(|| invalid(format!("{} already has this rule.", t.display_name)))?;
    Ok(plan(req, format!("Copied memory {} to {}", item.name, t.display_name), vec![agent.clone()], vec![op], true))
}

pub fn promote(core: &Core, req: ActionRequest, ids: &[ItemId], agents: &[AgentKey], text: &str) -> Result<ActionPlan, AppError> {
    if ids.is_empty() {
        return Err(invalid("No memories chosen."));
    }
    if agents.is_empty() {
        return Err(invalid("Choose at least one agent for the rule."));
    }
    let rule = one_line(text)?;
    let mut ops = Vec::new();
    let mut names = Vec::new();
    for a in agents {
        let t = target(core, a)?;
        if let Some(op) = add_rule(&t.path, &rule)? {
            ops.push(op);
        }
        names.push(t.display_name);
    }
    ops.extend(trash_ops(core, ids)?);
    let n = ids.len();
    let what = if n == 1 { "1 memory".to_string() } else { format!("{n} memories") };
    let summary = format!("Merged {what} into one rule for {}", names.join(", "));
    let mut touched = agents.to_vec();
    if !touched.contains(&claude()) {
        touched.push(claude());
    }
    Ok(plan(req, summary, touched, ops, true))
}

fn target(core: &Core, agent: &AgentKey) -> Result<RuleTarget, AppError> {
    rule_targets(core)
        .into_iter()
        .find(|t| &t.agent == agent)
        .ok_or_else(|| invalid("SkillDesk cannot write global rules for that agent."))
}

fn one_line(text: &str) -> Result<String, AppError> {
    let rule = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if rule.is_empty() {
        return Err(invalid("Write the rule first."));
    }
    if rule.len() > 2000 {
        return Err(invalid("Keep the rule under 2,000 characters."));
    }
    Ok(rule)
}

/// The edit that adds `- rule` under `RULES_HEADING`. `None` = already there.
fn add_rule(path: &Path, rule: &str) -> Result<Option<FileOp>, AppError> {
    let before = read(path)?;
    let text = before.clone().unwrap_or_default();
    let bullet = format!("- {rule}");
    if text.lines().any(|l| l.trim_end() == bullet) {
        return Ok(None);
    }
    let after = with_rule(&text, &bullet);
    Ok(Some(FileOp::edit_text(path.to_path_buf(), before.as_deref(), Some(&after))))
}

fn with_rule(text: &str, bullet: &str) -> String {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    if let Some(h) = lines.iter().position(|l| l.trim_end() == RULES_HEADING) {
        // End of the section: the next heading, or the end of the file.
        let end = lines[h + 1..].iter().position(|l| l.starts_with('#')).map_or(lines.len(), |p| h + 1 + p);
        // Insert after the last non-blank line of the section.
        let at = (h + 1..end).rev().find(|&i| !lines[i].trim().is_empty()).map_or(h + 1, |i| i + 1);
        let mut out: String = lines[..at].concat();
        if !out.ends_with('\n') {
            out.push('\n');
        }
        if at == h + 1 {
            out.push('\n');
        }
        out.push_str(bullet);
        out.push('\n');
        out.push_str(&lines[at..].concat());
        return out;
    }
    let mut out = text.to_string();
    if !out.is_empty() {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str(RULES_HEADING);
    out.push_str("\n\n");
    out.push_str(bullet);
    out.push('\n');
    out
}

/// Drop each memory's `MEMORY.md` line, then move the files to the Trash.
fn trash_ops(core: &Core, ids: &[ItemId]) -> Result<Vec<FileOp>, AppError> {
    let mut by_index: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
    let mut moves = Vec::new();
    let mut taken: Vec<PathBuf> = Vec::new();
    for id in ids {
        let (_, f, path) = memory(core, id)?;
        if f.mem_type == crate::model::MemoryType::Instructions {
            return Err(invalid(format!("{} is an agent's instructions file. Edit it instead; SkillDesk does not move it to the Trash.", path.display())));
        }
        if !path.is_file() {
            return Err(invalid(format!("{} is gone. Rescan and try again.", path.display())));
        }
        if let Some(ix) = f.index {
            let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            by_index.entry(ix).or_default().push(file);
        }
        let mut to = super::trash_target(core, &path);
        let mut n = 2;
        while taken.contains(&to) {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            to = core.env.home.join(".Trash").join(format!("{name} {n}"));
            n += 1;
        }
        taken.push(to.clone());
        moves.push(FileOp::MovePath { from: path, to });
    }
    let mut ops = Vec::new();
    for (ix, files) in by_index {
        let Some(before) = read(&ix)? else { continue };
        let after: String = before
            .split_inclusive('\n')
            .filter(|l| !files.iter().any(|f| l.contains(&format!("({f})"))))
            .collect();
        if after != before {
            ops.push(FileOp::edit_text(ix, Some(&before), Some(&after)));
        }
    }
    ops.extend(moves);
    Ok(ops)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::Env;

    fn mem(dir: &Path, file: &str, name: &str) -> PathBuf {
        let p = dir.join(file);
        std::fs::write(&p, format!("---\nname: {name}\ndescription: Never run git commit; user commits\ntype: feedback\n---\n\nNever commit.\n")).unwrap();
        p
    }

    /// Promote = data-touching: two memories go to the Trash, two index lines
    /// and the global rules file change. Undo must give back every byte.
    #[test]
    fn promote_then_undo_restores_every_byte() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().canonicalize().unwrap();
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::create_dir_all(home.join(".Trash")).unwrap();
        let claude_md = home.join(".claude/CLAUDE.md");
        std::fs::write(&claude_md, "# Mine\n\nKeep this.\n").unwrap();
        let mut files = Vec::new();
        for p in ["-a-one", "-a-two"] {
            let dir = home.join(".claude/projects").join(p).join("memory");
            std::fs::create_dir_all(&dir).unwrap();
            let f = mem(&dir, "no-commit.md", "no-commit");
            std::fs::write(dir.join("MEMORY.md"), "- [Other](other.md) — keep\n- [No commit](no-commit.md) — never\n").unwrap();
            files.push(f);
        }
        let core = Core::open(Env::for_test(&home, &home.join("data"))).unwrap();
        crate::scan::run_scan(&core, &crate::scan::NoSink).unwrap();
        let items: Vec<_> = core
            .items(crate::model::ItemKind::Memory)
            .unwrap()
            .into_iter()
            .filter(|i| !matches!(&i.facts, KindFacts::Memory(f) if f.mem_type == crate::model::MemoryType::Instructions))
            .collect();
        assert_eq!(items.len(), 2);
        let ids: Vec<ItemId> = items.iter().map(|i| i.id.clone()).collect();
        let snapshot = |paths: &[PathBuf]| paths.iter().map(|p| std::fs::read(p).ok()).collect::<Vec<_>>();
        let mut watched = files.clone();
        watched.extend(files.iter().map(|f| f.with_file_name("MEMORY.md")));
        watched.push(claude_md.clone());
        let before = snapshot(&watched);

        let req = ActionRequest::MemoryPromote { ids, agents: vec![claude()], text: "Never commit; the user commits.".into() };
        let p = core.action_plan(req).unwrap();
        let rec = core.action_apply(&p.id).unwrap();
        assert!(files.iter().all(|f| !f.exists()), "memories moved out");
        let md = std::fs::read_to_string(&claude_md).unwrap();
        assert_eq!(md, "# Mine\n\nKeep this.\n\n## Rules from memory\n\n- Never commit; the user commits.\n");
        let ix = std::fs::read_to_string(files[0].with_file_name("MEMORY.md")).unwrap();
        assert_eq!(ix, "- [Other](other.md) — keep\n");

        core.action_undo(&rec.id).unwrap();
        assert_eq!(snapshot(&watched), before, "Undo restores every byte");
    }

    #[test]
    fn rule_goes_under_existing_heading() {
        let t = "# A\n\n## Rules from memory\n\n- one\n\n## Next\nx\n";
        assert_eq!(with_rule(t, "- two"), "# A\n\n## Rules from memory\n\n- one\n- two\n\n## Next\nx\n");
    }
}
