//! Skill writes, straight into agent skill folders.
//! - Install: a folder, `.zip` or `.skill` is copied into each chosen agent's
//!   own skills folder.
//! - On for an agent: the skill's own folder is copied into that agent's folder.
//! - Off for an agent: a link is removed; a real folder goes to the Trash.
//!
//! Every step is a reversible `FileOp`; anything replaced goes to the Trash.

use super::plan::{agents_reading, display, skill};
use super::{new_id, trash_target, ActionPlan, ActionRequest, FileOp, Removal};
use crate::adapters;
use crate::core::Core;
use crate::error::{AppError, ErrorKind};
use crate::fsx::path_guard::sanitize_name;
use crate::fsx::target::{classify_target, TargetState};
use crate::install::{self, dir_stats};
use crate::model::{AgentKey, ItemId, LinkState, SkillLocation};
use std::path::{Path, PathBuf};

fn invalid(msg: impl Into<String>) -> AppError {
    AppError::new(ErrorKind::InvalidInput, msg)
}

fn names(keys: &[AgentKey]) -> String {
    keys.iter().map(display).collect::<Vec<_>>().join(", ")
}

fn add_agents(into: &mut Vec<AgentKey>, from: &[AgentKey]) {
    for a in from {
        if !into.contains(a) {
            into.push(a.clone());
        }
    }
}

/// A Trash path no other op in this plan uses.
fn trash_for(core: &Core, path: &Path, taken: &mut Vec<PathBuf>) -> PathBuf {
    let mut t = trash_target(core, path);
    let mut n = 2;
    while taken.contains(&t) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        t = core.env.home.join(".Trash").join(format!("{name} {n}"));
        n += 1;
    }
    taken.push(t.clone());
    t
}

/// The ops that clear `dest` before a copy goes there.
fn clear_for_copy(core: &Core, dest: &Path, taken: &mut Vec<PathBuf>, removed: &mut Vec<Removal>) -> Result<Vec<FileOp>, AppError> {
    match classify_target(dest, None) {
        Ok(TargetState::Absent) => Ok(vec![]),
        Ok(TargetState::LinkToSource | TargetState::ForeignLink) => {
            let raw = std::fs::read_link(dest).unwrap_or_default();
            if dest.exists() {
                let to = if raw.is_absolute() { raw } else { dest.parent().unwrap_or(Path::new("")).join(raw) };
                Ok(vec![FileOp::RemoveSymlink { link: dest.to_path_buf(), to }])
            } else {
                Ok(vec![FileOp::UnlinkDangling { link: dest.to_path_buf(), to: raw }])
            }
        }
        Ok(TargetState::RealDir) => {
            let (files, bytes) = dir_stats(dest);
            removed.push(Removal { path: dest.to_path_buf(), files, bytes });
            Ok(vec![FileOp::MovePath { from: dest.to_path_buf(), to: trash_for(core, dest, taken) }])
        }
        Ok(TargetState::RealFile) => Err(invalid(format!("{} is a file, not a skill folder. Nothing was changed.", dest.display()))),
        Err(e) => Err(invalid(format!("Cannot inspect {}: {e}", dest.display()))),
    }
}

/// Copies the skill into each chosen agent's skills folder. A folder of the
/// same name there goes to the Trash first (shown in the plan).
pub fn install(core: &Core, req: ActionRequest, from: &Path, agents: &[AgentKey]) -> Result<ActionPlan, AppError> {
    if agents.is_empty() {
        return Err(invalid("Pick at least one agent to install the skill for."));
    }
    let preview = install::inspect(core, from)?;
    let id = new_id();
    let mut removed = Vec::new();
    let mut taken = Vec::new();
    let mut replaced_link = false;
    let mut covered: Vec<AgentKey> = Vec::new();
    let mut dests: Vec<(PathBuf, Vec<FileOp>)> = Vec::new();
    for key in agents {
        let target = preview
            .targets
            .iter()
            .find(|t| &t.key == key || t.agents.contains(key))
            .ok_or_else(|| invalid(format!("{} is not on this Mac.", display(key))))?;
        if target.agents.iter().all(|a| covered.contains(a)) {
            continue;
        }
        let dest = target.dir.join(&preview.name);
        let clear = clear_for_copy(core, &dest, &mut taken, &mut removed)?;
        replaced_link |= clear.iter().any(|o| !matches!(o, FileOp::MovePath { .. }));
        dests.push((dest, clear));
        add_agents(&mut covered, &target.agents);
    }
    // Staged last: a refusal above leaves nothing behind.
    let mut ops = Vec::new();
    for (slot, (dest, clear)) in dests.into_iter().enumerate() {
        let staged = install::stage(core, from, &id, slot, &preview.name)?;
        ops.extend(clear);
        ops.push(FileOp::MovePath { from: staged, to: dest });
    }
    let shared = covered.len() > agents.len();
    let summary = format!("Installed {} for {}", preview.name, names(&covered));
    Ok(ActionPlan { id, request: req, summary, agents: covered, ops, needs_confirm: !removed.is_empty() || replaced_link || shared, removed, untouched: vec![] })
}

fn live(l: &&SkillLocation) -> bool {
    !matches!(l.link, LinkState::Dangling(_))
}

/// On: copy the skill's own folder into the agent's skills folder.
/// Off: remove the agent's link, or move its folder to the Trash.
pub fn toggle(core: &Core, req: ActionRequest, id: &ItemId, agent: &AgentKey, on: bool) -> Result<ActionPlan, AppError> {
    let (item, facts) = skill(core, id)?;
    let adapter = adapters::find(&agent.0)
        .filter(|a| a.is_installed(&core.env))
        .ok_or_else(|| invalid("That agent is not installed on this Mac."))?;

    if on {
        if facts.locations.iter().filter(live).any(|l| l.agents.contains(agent)) {
            return Err(invalid(format!("{} is already on for {}.", item.name, adapter.display_name)));
        }
        let source = facts
            .locations
            .iter()
            .filter(live)
            .find(|l| l.plugin.is_none())
            .map(|_| facts.home.clone())
            .filter(|h| h.is_dir())
            .ok_or_else(|| invalid(format!("{} has no folder on this Mac to copy from.", item.name)))?;
        let dir_name = sanitize_name(&source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| item.name.clone()));
        let agent_dir = core.env.resolve(adapter.skills_dir);
        let dest = agent_dir.join(&dir_name);
        if std::fs::symlink_metadata(&dest).is_ok() {
            return Err(invalid(format!("{} already exists. Nothing was changed.", dest.display())));
        }
        let mut agents = agents_reading(core, &agent_dir);
        if !agents.contains(agent) {
            agents.insert(0, agent.clone());
        }
        let plan_id = new_id();
        let staged = install::stage(core, &source, &plan_id, 0, &dir_name)?;
        let needs_confirm = agents.len() > 1;
        let summary = format!("Copied {} to {}", item.name, names(&agents));
        return Ok(ActionPlan { id: plan_id, request: req, summary, agents, ops: vec![FileOp::MovePath { from: staged, to: dest }], needs_confirm, removed: vec![], untouched: vec![] });
    }

    let here: Vec<_> = facts.locations.iter().filter(live).filter(|l| l.agents.contains(agent)).collect();
    if here.is_empty() {
        return Err(invalid(format!("{} is already off for {}.", item.name, adapter.display_name)));
    }
    if here.iter().all(|l| l.plugin.is_some()) {
        return Err(invalid(format!("{} comes from a plugin. Turn the plugin off instead.", item.name)));
    }
    if here.iter().all(|l| l.project.is_some()) {
        return Err(invalid(format!("{} lives in a project folder; SkillDesk only switches global skills.", item.name)));
    }
    let mut ops = Vec::new();
    let mut agents: Vec<AgentKey> = Vec::new();
    let mut removed = Vec::new();
    let mut taken = Vec::new();
    for l in here.into_iter().filter(|l| l.plugin.is_none() && l.project.is_none()) {
        match &l.link {
            LinkState::Link(target) => ops.push(FileOp::RemoveSymlink { link: l.path.clone(), to: target.clone() }),
            LinkState::RealDir => {
                let (files, bytes) = dir_stats(&l.path);
                removed.push(Removal { path: l.path.clone(), files, bytes });
                ops.push(FileOp::MovePath { from: l.path.clone(), to: trash_for(core, &l.path, &mut taken) });
            }
            LinkState::Dangling(_) => {}
        }
        add_agents(&mut agents, &l.agents);
    }
    let needs_confirm = !removed.is_empty() || agents.len() > 1;
    let summary = if removed.is_empty() {
        format!("{} off for {}", item.name, names(&agents))
    } else {
        format!("Moved {} to the Trash for {}", item.name, names(&agents))
    };
    Ok(ActionPlan { id: new_id(), request: req, summary, agents, ops, needs_confirm, removed, untouched: vec![] })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::Env;
    use crate::model::{ItemKind, KindFacts};

    fn skill_dir(p: &Path, name: &str) {
        std::fs::create_dir_all(p).unwrap();
        std::fs::write(p.join("SKILL.md"), format!("---\nname: {name}\n---\nbody\n")).unwrap();
    }

    fn setup() -> (tempfile::TempDir, PathBuf, std::sync::Arc<Core>) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().canonicalize().unwrap();
        std::fs::create_dir_all(home.join(".Trash")).unwrap();
        std::fs::create_dir_all(home.join(".claude/skills")).unwrap();
        std::fs::create_dir_all(home.join(".cursor/skills")).unwrap();
        let core = Core::open(Env::for_test(&home, &home.join("data"))).unwrap();
        (tmp, home, core)
    }

    /// Install copies the skill into each agent's own folder; nothing is
    /// written anywhere else, and Undo takes the copies out again.
    #[test]
    fn install_copies_into_each_agent_folder() {
        let (_tmp, home, core) = setup();
        let src = home.join("Downloads/demo");
        skill_dir(&src, "demo");
        let req = ActionRequest::SkillInstall { from: src.clone(), agents: vec![AgentKey("claude_code".into()), AgentKey("cursor".into())] };
        let plan = core.action_plan(req).unwrap();
        let rec = core.action_apply(&plan.id).unwrap();
        for a in [".claude/skills/demo", ".cursor/skills/demo"] {
            let p = home.join(a);
            assert!(std::fs::symlink_metadata(&p).unwrap().is_dir(), "{a} is a real folder");
            assert!(p.join("SKILL.md").is_file());
        }
        assert!(!home.join(".skilldesk").exists(), "no SkillDesk folder in home");
        core.action_undo(&rec.id).unwrap();
        assert!(!home.join(".claude/skills/demo").exists() && !home.join(".cursor/skills/demo").exists());
        assert!(src.join("SKILL.md").is_file(), "source untouched");
    }

    /// On copies the skill's folder to the other agent; off moves a real
    /// folder to the Trash and Undo brings it back.
    #[test]
    fn toggle_copies_on_and_trashes_off() {
        let (_tmp, home, core) = setup();
        let own = home.join(".claude/skills/mine");
        skill_dir(&own, "mine");
        std::fs::write(own.join("notes.txt"), "user edits").unwrap();
        crate::scan::run_scan(&core, &crate::scan::NoSink).unwrap();
        let item = core.items(ItemKind::Skill).unwrap().into_iter().find(|i| i.name == "mine").unwrap();
        let KindFacts::Skill(_) = &item.facts else { panic!() };

        let plan = core.action_plan(ActionRequest::SkillToggle { id: item.id.clone(), agent: AgentKey("cursor".into()), on: true }).unwrap();
        core.action_apply(&plan.id).unwrap();
        let copy = home.join(".cursor/skills/mine");
        assert!(std::fs::symlink_metadata(&copy).unwrap().is_dir(), "a copy, not a link");
        assert_eq!(std::fs::read_to_string(copy.join("notes.txt")).unwrap(), "user edits");

        let plan = core.action_plan(ActionRequest::SkillToggle { id: item.id.clone(), agent: AgentKey("claude_code".into()), on: false }).unwrap();
        assert_eq!(plan.removed.len(), 1);
        let rec = core.action_apply(&plan.id).unwrap();
        assert!(!own.exists(), "moved to the Trash");
        core.action_undo(&rec.id).unwrap();
        assert_eq!(std::fs::read_to_string(own.join("notes.txt")).unwrap(), "user edits");
    }
}
