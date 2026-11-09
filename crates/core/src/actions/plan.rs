//! Request → plan. Pure apart from reading the cache and stat calls.

use super::{new_id, ActionPlan, ActionRequest, FileOp};
use crate::adapters;
use crate::core::Core;
use crate::error::{AppError, ErrorKind};
use crate::fsx::path_guard::sanitize_name;
use crate::model::{AgentKey, Item, ItemId, KindFacts, LinkState, SkillFacts};
use std::path::{Path, PathBuf};

fn invalid(msg: impl Into<String>) -> AppError {
    AppError::new(ErrorKind::InvalidInput, msg)
}

pub fn build(core: &Core, req: ActionRequest) -> Result<ActionPlan, AppError> {
    match &req {
        ActionRequest::SkillToggle { id, agent, on } => skill_toggle(core, req.clone(), id, agent, *on),
        ActionRequest::HookSetEnabled { ids, on } => super::hooks::build(core, req.clone(), ids, *on),
        ActionRequest::HookRemove { ids } => super::hooks::remove(core, req.clone(), ids),
        ActionRequest::LinksRemoveDangling { ids } => remove_dangling(core, req.clone(), ids, None),
        ActionRequest::LinkRemove { id, path } => remove_dangling(core, req.clone(), std::slice::from_ref(id), Some(path)),
        ActionRequest::PluginSetEnabled { id, on } => super::plugins::set_enabled(core, req.clone(), id, *on),
        ActionRequest::PluginForget { id } => super::plugins::forget(core, req.clone(), id),
        ActionRequest::MarketplaceRemove { ids } => super::plugins::remove_marketplaces(core, req.clone(), ids),
        ActionRequest::CacheClean { ids } => super::plugins::clean_cache(core, req.clone(), ids),
        ActionRequest::McpSetEnabled { id, entry, on } => super::mcp::set_enabled(core, req.clone(), id, entry, *on),
        ActionRequest::McpCopyToAgent { id, agent } => super::mcp::copy_to_agent(core, req.clone(), id, agent),
        ActionRequest::LibraryInstall { from, agents } => super::library::install(core, req.clone(), from, agents),
        ActionRequest::SkillAddToLibrary { id } => super::library::add(core, req.clone(), id),
        ActionRequest::LibraryRemove { id } => super::library::remove(core, req.clone(), id),
        ActionRequest::LibraryAdopt { id, keep } => super::library::adopt(core, req.clone(), id, keep),
        ActionRequest::MemoryEdit { id, text } => super::memory::edit(core, req.clone(), id, text),
        ActionRequest::MemoryForget { ids } => super::memory::forget(core, req.clone(), ids),
        ActionRequest::MemoryPromote { ids, agents, text } => super::memory::promote(core, req.clone(), ids, agents, text),
        ActionRequest::MemoryCopyToAgent { id, agent, text } => super::memory::copy_to_agent(core, req.clone(), id, agent, text),
        ActionRequest::SecretRedact { id } => super::secrets::redact(core, req.clone(), id),
        ActionRequest::StorageTrash { kinds } => super::storage::trash(core, req.clone(), kinds),
    }
}

fn remove_dangling(core: &Core, req: ActionRequest, ids: &[ItemId], only: Option<&PathBuf>) -> Result<ActionPlan, AppError> {
    let mut ops = Vec::new();
    let mut agents: Vec<AgentKey> = Vec::new();
    for id in ids {
        let (_, facts) = skill(core, id)?;
        for loc in facts.locations.iter().filter(|l| only.is_none_or(|p| &l.path == p)) {
            if let LinkState::Dangling(_) = loc.link {
                let Ok(raw) = std::fs::read_link(&loc.path) else { continue };
                ops.push(FileOp::UnlinkDangling { link: loc.path.clone(), to: raw });
                for a in &loc.agents {
                    if !agents.contains(a) {
                        agents.push(a.clone());
                    }
                }
            }
        }
    }
    if ops.is_empty() {
        return Err(invalid("No links that point nowhere are left."));
    }
    let n = ops.len();
    Ok(ActionPlan {
        id: new_id(),
        request: req,
        summary: format!("Removed {n} {} that pointed nowhere", if n == 1 { "link" } else { "links" }),
        agents,
        ops,
        needs_confirm: false,
        removed: vec![],
        untouched: vec![],
    })
}

pub(crate) fn skill(core: &Core, id: &ItemId) -> Result<(Item, SkillFacts), AppError> {
    let item = core
        .store
        .item(id)?
        .ok_or_else(|| AppError::new(ErrorKind::NotFound, "This skill is no longer in the inventory. Rescan and try again."))?;
    match item.facts.clone() {
        KindFacts::Skill(f) => Ok((item, f)),
        _ => Err(invalid("Not a skill.")),
    }
}

/// Every detected agent that reads `dir` (same folder on disk, or same path when
/// the folder does not exist yet). Turning a skill on or off in a shared folder
/// changes it for all of them.
pub(crate) fn agents_reading(core: &Core, dir: &Path) -> Vec<AgentKey> {
    use std::os::unix::fs::MetadataExt;
    let id = |p: &Path| std::fs::metadata(p).ok().map(|m| (m.dev(), m.ino()));
    let target = id(dir);
    let mut out = Vec::new();
    for a in adapters::detected(&core.env) {
        let reads = a.skill_dirs(&core.env).iter().any(|d| match (target, id(d)) {
            (Some(x), Some(y)) => x == y,
            _ => d == dir,
        });
        if reads {
            out.push(a.agent_key());
        }
    }
    out
}

pub(crate) fn display(key: &AgentKey) -> String {
    adapters::find(&key.0).map(|a| a.display_name.to_string()).unwrap_or_else(|| key.0.clone())
}

fn skill_toggle(core: &Core, req: ActionRequest, id: &ItemId, agent: &AgentKey, on: bool) -> Result<ActionPlan, AppError> {
    let (item, facts) = skill(core, id)?;
    let adapter = adapters::find(&agent.0)
        .filter(|a| a.is_installed(&core.env))
        .ok_or_else(|| invalid("That agent is not installed on this Mac."))?;
    let live = |l: &&crate::model::SkillLocation| !matches!(l.link, LinkState::Dangling(_));
    let names = |keys: &[AgentKey]| keys.iter().map(display).collect::<Vec<_>>().join(", ");

    if on {
        if facts.locations.iter().filter(live).any(|l| l.agents.contains(agent)) {
            return Err(invalid(format!("{} is already on for {}.", item.name, adapter.display_name)));
        }
        // Only library skills are linked into other agents (ADR 0015); a skill
        // outside the library toggles only where it is.
        let source: PathBuf = facts
            .library
            .clone()
            .ok_or_else(|| invalid(format!("Add {} to the library first, then turn it on for {}.", item.name, adapter.display_name)))?;
        let dir_name = sanitize_name(&source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| item.name.clone()));
        let agent_dir = core.env.resolve(adapter.skills_dir);
        let link = agent_dir.join(dir_name);
        let mut agents = agents_reading(core, &agent_dir);
        if !agents.contains(agent) {
            agents.insert(0, agent.clone());
        }
        let needs_confirm = agents.len() > 1;
        let summary = format!("{} on for {}", item.name, names(&agents));
        return Ok(ActionPlan { id: new_id(), request: req, summary, agents, ops: vec![FileOp::CreateSymlink { link, to: source }], needs_confirm, removed: vec![], untouched: vec![] });
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
    let mut moves_folder = false;
    for l in here.into_iter().filter(|l| l.plugin.is_none() && l.project.is_none()) {
        match &l.link {
            LinkState::Link(target) => ops.push(FileOp::RemoveSymlink { link: l.path.clone(), to: target.clone() }),
            LinkState::RealDir => {
                moves_folder = true;
                let name = l.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let to = core.env.skilldesk_dir().join("disabled/skills").join(&agent.0).join(sanitize_name(&name));
                ops.push(FileOp::MovePath { from: l.path.clone(), to });
            }
            LinkState::Dangling(_) => {}
        }
        for a in &l.agents {
            if !agents.contains(a) {
                agents.push(a.clone());
            }
        }
    }
    let needs_confirm = moves_folder || agents.len() > 1;
    let summary = format!("{} off for {}", item.name, names(&agents));
    Ok(ActionPlan { id: new_id(), request: req, summary, agents, ops, needs_confirm, removed: vec![], untouched: vec![] })
}
