//! Request → plan. Pure apart from reading the cache and stat calls.

use super::{new_id, ActionPlan, ActionRequest, FileOp};
use crate::adapters;
use crate::core::Core;
use crate::error::{AppError, ErrorKind};
use crate::model::{AgentKey, Item, ItemId, KindFacts, LinkState, SkillFacts};
use std::path::{Path, PathBuf};

fn invalid(msg: impl Into<String>) -> AppError {
    AppError::new(ErrorKind::InvalidInput, msg)
}

pub fn build(core: &Core, req: ActionRequest) -> Result<ActionPlan, AppError> {
    match &req {
        ActionRequest::SkillToggle { id, agent, on } => super::skills::toggle(core, req.clone(), id, agent, *on),
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
        ActionRequest::McpRemove { id, entry } => super::mcp::remove(core, req.clone(), id, entry),
        ActionRequest::SkillInstall { from, agents } => super::skills::install(core, req.clone(), from, agents),
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
