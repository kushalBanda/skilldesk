//! Library actions (ADR 0015): install, add an existing skill, remove.
//! Every step is a reversible `FileOp`; anything replaced goes to the Trash.

use super::plan::{agents_reading, display, skill};
use super::{new_id, trash_target, ActionPlan, ActionRequest, FileOp, Removal};
use crate::core::Core;
use crate::error::{AppError, ErrorKind};
use crate::fsx::path_guard::sanitize_name;
use crate::fsx::target::{classify_target, TargetState};
use crate::library::{self, install, LibrarySource, Sidecar, SIDECAR};
use crate::model::{AgentKey, ItemId, LinkState};
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

/// Where a link would replace what is at `link`: the ops that clear it.
/// `None` = already linked to `dest`.
fn clear_for_link(core: &Core, link: &Path, dest: &Path, taken: &mut Vec<PathBuf>, removed: &mut Vec<Removal>) -> Result<Option<Vec<FileOp>>, AppError> {
    match classify_target(link, Some(dest)) {
        Ok(TargetState::Absent) => Ok(Some(vec![])),
        Ok(TargetState::LinkToSource) => Ok(None),
        Ok(TargetState::ForeignLink) => {
            let raw = std::fs::read_link(link).unwrap_or_default();
            if link.exists() {
                let to = if raw.is_absolute() { raw } else { link.parent().unwrap_or(Path::new("")).join(raw) };
                Ok(Some(vec![FileOp::RemoveSymlink { link: link.to_path_buf(), to }]))
            } else {
                Ok(Some(vec![FileOp::UnlinkDangling { link: link.to_path_buf(), to: raw }]))
            }
        }
        Ok(TargetState::RealDir) => {
            let (files, bytes) = library::dir_stats(link);
            removed.push(Removal { path: link.to_path_buf(), files, bytes });
            Ok(Some(vec![FileOp::MovePath { from: link.to_path_buf(), to: trash_for(core, link, taken) }]))
        }
        Ok(TargetState::RealFile) => Err(invalid(format!("{} is a file, not a skill folder. Nothing was changed.", link.display()))),
        Err(e) => Err(invalid(format!("Cannot inspect {}: {e}", link.display()))),
    }
}

pub fn install(core: &Core, req: ActionRequest, from: &Path, agents: &[AgentKey]) -> Result<ActionPlan, AppError> {
    let preview = install::inspect(core, from)?;
    let lib = library::checked_library_dir(&core.env)?;
    let dest = lib.join(&preview.name);
    let id = new_id();
    let mut ops = Vec::new();
    let mut removed = Vec::new();
    let mut taken = Vec::new();
    let mut replaced_link = false;

    if std::fs::symlink_metadata(&dest).is_ok() {
        match classify_target(&dest, None) {
            Ok(TargetState::RealDir) => {
                let (files, bytes) = library::dir_stats(&dest);
                removed.push(Removal { path: dest.clone(), files, bytes });
                ops.push(FileOp::MovePath { from: dest.clone(), to: trash_for(core, &dest, &mut taken) });
            }
            _ => return Err(invalid(format!("{} is not a plain folder. Nothing was changed.", dest.display()))),
        }
    }
    let mut linked: Vec<AgentKey> = Vec::new();
    let mut link_ops = Vec::new();
    for key in agents {
        let target = preview
            .targets
            .iter()
            .find(|t| &t.key == key || t.agents.contains(key))
            .ok_or_else(|| invalid(format!("{} is not on this Mac.", display(key))))?;
        if target.agents.iter().all(|a| linked.contains(a)) {
            continue;
        }
        let link = target.dir.join(&preview.name);
        if let Some(clear) = clear_for_link(core, &link, &dest, &mut taken, &mut removed)? {
            replaced_link |= clear.iter().any(|o| !matches!(o, FileOp::MovePath { .. }));
            link_ops.extend(clear);
            link_ops.push(FileOp::CreateSymlink { link, to: dest.clone() });
        }
        add_agents(&mut linked, &target.agents);
    }
    // Staged last: a refusal above leaves nothing behind.
    let staged = install::stage(core, from, &id, &preview.name)?;
    ops.push(FileOp::MovePath { from: staged, to: dest });
    ops.extend(link_ops);

    let shared = linked.len() > agents.len();
    let summary = if linked.is_empty() {
        format!("Installed {} to the library", preview.name)
    } else {
        format!("Installed {} and turned it on for {}", preview.name, names(&linked))
    };
    Ok(ActionPlan { id, request: req, summary, agents: linked, ops, needs_confirm: !removed.is_empty() || replaced_link || shared, removed, untouched: vec![] })
}

pub fn add(core: &Core, req: ActionRequest, id: &ItemId) -> Result<ActionPlan, AppError> {
    let (item, facts) = skill(core, id)?;
    if facts.library.is_some() {
        return Err(invalid(format!("{} is already in the library.", item.name)));
    }
    let src = facts
        .locations
        .iter()
        .find(|l| l.link == LinkState::RealDir && l.plugin.is_none() && l.project.is_none())
        .ok_or_else(|| invalid(format!("{} has no folder of its own outside plugins and projects, so there is nothing to move.", item.name)))?;
    let lib = library::checked_library_dir(&core.env)?;
    let dir_name = sanitize_name(&src.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| item.name.clone()));
    let dest = lib.join(&dir_name);
    if std::fs::symlink_metadata(&dest).is_ok() {
        return Err(invalid(format!("The library already has a folder named {dir_name}. Nothing was changed.")));
    }
    if src.path.join(SIDECAR).exists() {
        return Err(invalid(format!("{} already has a {SIDECAR}. Nothing was changed.", src.path.display())));
    }
    let sidecar = Sidecar::new(LibrarySource::Adopted { from: vec![src.path.clone()] }).render();
    let ops = vec![
        FileOp::MovePath { from: src.path.clone(), to: dest.clone() },
        FileOp::edit_text(dest.join(SIDECAR), None, Some(&sidecar)),
        FileOp::CreateSymlink { link: src.path.clone(), to: dest },
    ];
    let mut agents = src.agents.clone();
    if let Some(parent) = src.path.parent() {
        add_agents(&mut agents, &agents_reading(core, parent));
    }
    let summary = format!("Moved {} into the library; {} link to it", item.name, names(&agents));
    Ok(ActionPlan { id: new_id(), request: req, summary, agents, ops, needs_confirm: true, removed: vec![], untouched: vec![] })
}

pub fn adopt(core: &Core, req: ActionRequest, id: &ItemId, keep: &Path) -> Result<ActionPlan, AppError> {
    let (item, facts) = skill(core, id)?;
    let copies: Vec<_> = library::adopt::own_copies(&facts).into_iter().cloned().collect();
    let keep_is_lib = facts.library.as_deref() == Some(keep);
    if !keep_is_lib && !copies.iter().any(|c| c.path == keep) {
        return Err(invalid("That folder is not a copy of this skill. Rescan and try again."));
    }
    if copies.is_empty() || (copies.len() == 1 && facts.library.is_none()) {
        return Err(invalid(format!("{} has only one copy; use Add to library.", item.name)));
    }
    let lib = library::checked_library_dir(&core.env)?;
    let dest = match &facts.library {
        Some(l) => l.clone(),
        None => lib.join(sanitize_name(&keep.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| item.name.clone()))),
    };
    if facts.library.is_none() && std::fs::symlink_metadata(&dest).is_ok() {
        return Err(invalid(format!("The library already has {}. Nothing was changed.", dest.display())));
    }
    let mut ops = Vec::new();
    let mut removed = Vec::new();
    let mut taken = Vec::new();
    let mut agents: Vec<AgentKey> = Vec::new();
    let mut trash = |p: &Path, ops: &mut Vec<FileOp>, removed: &mut Vec<Removal>| {
        let (files, bytes) = library::dir_stats(p);
        removed.push(Removal { path: p.to_path_buf(), files, bytes });
        ops.push(FileOp::MovePath { from: p.to_path_buf(), to: trash_for(core, p, &mut taken) });
    };
    if !keep_is_lib {
        if facts.library.is_some() {
            trash(&dest, &mut ops, &mut removed);
        }
        ops.push(FileOp::MovePath { from: keep.to_path_buf(), to: dest.clone() });
        if !keep.join(SIDECAR).exists() {
            let from = copies.iter().map(|c| c.path.clone()).collect();
            ops.push(FileOp::edit_text(dest.join(SIDECAR), None, Some(&Sidecar::new(LibrarySource::Adopted { from }).render())));
        }
        ops.push(FileOp::CreateSymlink { link: keep.to_path_buf(), to: dest.clone() });
    }
    for c in &copies {
        add_agents(&mut agents, &c.agents);
        if c.path == keep {
            continue;
        }
        trash(&c.path, &mut ops, &mut removed);
        ops.push(FileOp::CreateSymlink { link: c.path.clone(), to: dest.clone() });
    }
    let n = removed.len();
    let summary = format!("Adopted {} · {n} {} moved to Trash", item.name, if n == 1 { "copy" } else { "copies" });
    Ok(ActionPlan { id: new_id(), request: req, summary, agents, ops, needs_confirm: true, removed, untouched: vec![] })
}

/// Links followed from `link` before reaching a real folder (1 = points straight at it).
fn hops(link: &Path) -> usize {
    let mut p = link.to_path_buf();
    let mut n = 0;
    while let Ok(raw) = std::fs::read_link(&p) {
        n += 1;
        if n >= 10 {
            break;
        }
        p = if raw.is_absolute() { raw } else { p.parent().unwrap_or(Path::new("")).join(raw) };
    }
    n
}

pub fn remove(core: &Core, req: ActionRequest, id: &ItemId) -> Result<ActionPlan, AppError> {
    let (item, facts) = skill(core, id)?;
    let lib_skill = facts.library.clone().ok_or_else(|| invalid(format!("{} is not in the library.", item.name)))?;
    let lib = library::canonical_library_dir(&core.env).ok_or_else(|| invalid("The library folder is missing."))?;
    if !lib_skill.starts_with(&lib) {
        return Err(invalid("This skill's home is not inside the library. Nothing was changed."));
    }
    // Only links that resolve into this library folder; copies stay where they are.
    let mut links: Vec<(usize, FileOp, Vec<AgentKey>)> = Vec::new();
    for l in &facts.locations {
        let LinkState::Link(target) = &l.link else { continue };
        if target != &lib_skill {
            continue;
        }
        let Ok(raw) = std::fs::read_link(&l.path) else { continue };
        let to = if raw.is_absolute() { raw } else { l.path.parent().unwrap_or(Path::new("")).join(raw) };
        links.push((hops(&l.path), FileOp::RemoveSymlink { link: l.path.clone(), to }, l.agents.clone()));
    }
    // A link that others reach the library through goes last.
    links.sort_by_key(|(h, _, _)| std::cmp::Reverse(*h));
    let mut agents = Vec::new();
    let mut ops = Vec::new();
    for (_, op, a) in links {
        add_agents(&mut agents, &a);
        ops.push(op);
    }
    let (files, bytes) = library::dir_stats(&lib_skill);
    let mut taken = Vec::new();
    ops.push(FileOp::MovePath { from: lib_skill.clone(), to: trash_for(core, &lib_skill, &mut taken) });
    let n = ops.len() - 1;
    let summary = if n == 0 {
        format!("Removed {} from the library", item.name)
    } else {
        format!("Removed {} from the library and {n} {}", item.name, if n == 1 { "link" } else { "links" })
    };
    Ok(ActionPlan { id: new_id(), request: req, summary, agents, ops, needs_confirm: false, removed: vec![Removal { path: lib_skill, files, bytes }], untouched: vec![] })
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

    /// Remove undeploys links into the library only: a same-name link to
    /// somewhere else and a real copy in another agent are left alone; Undo
    /// puts every link back.
    #[test]
    fn library_remove_undeploys_only_links_into_library() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().canonicalize().unwrap();
        std::fs::create_dir_all(home.join(".Trash")).unwrap();
        let lib = home.join(".skilldesk/skills/demo");
        skill_dir(&lib, "demo");
        let other = home.join("elsewhere/demo");
        skill_dir(&other, "demo");
        std::fs::create_dir_all(home.join(".claude/skills")).unwrap();
        std::fs::create_dir_all(home.join(".cursor/skills")).unwrap();
        std::fs::create_dir_all(home.join(".codex/skills")).unwrap();
        std::os::unix::fs::symlink(&lib, home.join(".claude/skills/demo")).unwrap();
        std::os::unix::fs::symlink(&other, home.join(".cursor/skills/demo")).unwrap();
        skill_dir(&home.join(".codex/skills/demo"), "demo");

        let core = Core::open(Env::for_test(&home, &home.join("data"))).unwrap();
        crate::scan::run_scan(&core, &crate::scan::NoSink).unwrap();
        let items = core.items(ItemKind::Skill).unwrap();
        let item = items.iter().find(|i| i.name == "demo").cloned().unwrap_or_else(|| panic!("{:?}", items.iter().map(|i| &i.name).collect::<Vec<_>>()));
        let KindFacts::Skill(f) = &item.facts else { panic!() };
        assert_eq!(f.library.as_deref(), Some(lib.as_path()));

        let plan = core.action_plan(ActionRequest::LibraryRemove { id: item.id.clone() }).unwrap();
        let rec = core.action_apply(&plan.id).unwrap();
        assert!(std::fs::symlink_metadata(home.join(".claude/skills/demo")).is_err(), "library link removed");
        assert_eq!(std::fs::read_link(home.join(".cursor/skills/demo")).unwrap(), other, "foreign link kept");
        assert!(home.join(".codex/skills/demo/SKILL.md").is_file(), "real copy kept");
        assert!(other.join("SKILL.md").is_file());
        assert!(!lib.exists(), "library folder moved to the Trash");

        core.action_undo(&rec.id).unwrap();
        assert!(lib.join("SKILL.md").is_file());
        assert_eq!(std::fs::read_link(home.join(".claude/skills/demo")).unwrap(), lib);
    }

    /// Add to library moves the user's folder (never copies then deletes) and
    /// links it back; Undo puts the exact folder back where it was.
    #[test]
    fn library_add_moves_folder_and_undo_restores_it() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().canonicalize().unwrap();
        let own = home.join(".cursor/skills/mine");
        skill_dir(&own, "mine");
        std::fs::write(own.join("notes.txt"), "user edits").unwrap();
        let core = Core::open(Env::for_test(&home, &home.join("data"))).unwrap();
        crate::scan::run_scan(&core, &crate::scan::NoSink).unwrap();
        let item = core.items(ItemKind::Skill).unwrap().into_iter().find(|i| i.name == "mine").unwrap();

        let plan = core.action_plan(ActionRequest::SkillAddToLibrary { id: item.id.clone() }).unwrap();
        let rec = core.action_apply(&plan.id).unwrap();
        let lib = home.join(".skilldesk/skills/mine");
        assert_eq!(std::fs::read_link(&own).unwrap(), lib);
        assert_eq!(std::fs::read_to_string(lib.join("notes.txt")).unwrap(), "user edits");
        assert!(lib.join(SIDECAR).is_file());

        core.action_undo(&rec.id).unwrap();
        assert!(std::fs::symlink_metadata(&own).unwrap().is_dir(), "real folder back");
        assert_eq!(std::fs::read_to_string(own.join("notes.txt")).unwrap(), "user edits");
        assert!(!own.join(SIDECAR).exists(), "sidecar gone with the undo");
        assert!(!lib.exists());
    }

    /// Adopt keeps one copy, trashes the others and links every place; Undo
    /// brings back every original copy with its own content.
    #[test]
    fn library_adopt_undo_restores_all_original_copies() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().canonicalize().unwrap();
        std::fs::create_dir_all(home.join(".Trash")).unwrap();
        let a = home.join(".claude/skills/treg");
        let b = home.join(".cursor/skills/treg");
        let c = home.join(".agents/skills/treg");
        for p in [&a, &b, &c] {
            skill_dir(p, "treg");
        }
        std::fs::write(c.join("SKILL.md"), "---\nname: treg\n---\ndifferent\n").unwrap();
        let core = Core::open(Env::for_test(&home, &home.join("data"))).unwrap();
        crate::scan::run_scan(&core, &crate::scan::NoSink).unwrap();
        let item = core.items(ItemKind::Skill).unwrap().into_iter().find(|i| i.name == "treg").unwrap();
        let v = crate::library::adopt::view(&core, &item.id).unwrap();
        assert_eq!(v.keep, a, "keeps the Claude copy of the majority");
        assert_eq!(v.copies.iter().filter(|c| c.differs > 0).count(), 1);

        let plan = core.action_plan(ActionRequest::LibraryAdopt { id: item.id.clone(), keep: a.clone() }).unwrap();
        assert_eq!(plan.removed.len(), 2);
        let rec = core.action_apply(&plan.id).unwrap();
        let lib = home.join(".skilldesk/skills/treg");
        for p in [&a, &b, &c] {
            assert_eq!(std::fs::read_link(p).unwrap(), lib);
        }
        assert_eq!(std::fs::read_to_string(lib.join("SKILL.md")).unwrap(), "---\nname: treg\n---\nbody\n");

        core.action_undo(&rec.id).unwrap();
        for p in [&a, &b] {
            assert_eq!(std::fs::read_to_string(p.join("SKILL.md")).unwrap(), "---\nname: treg\n---\nbody\n");
        }
        assert_eq!(std::fs::read_to_string(c.join("SKILL.md")).unwrap(), "---\nname: treg\n---\ndifferent\n");
        assert!(!a.join(SIDECAR).exists() && !lib.exists());
    }
}
