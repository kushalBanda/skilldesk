//! Agent adapter registry (ADR 0003).

mod builtin;

use crate::env::Env;
use crate::model::{AgentKey, AgentSummary, AgentTier};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::RwLock;

pub use crate::model::CustomAgent;

pub use crate::model::AgentTier as Tier;

#[derive(Debug, Clone)]
pub struct AgentAdapter {
    pub key: &'static str,
    pub display_name: &'static str,
    pub tier: AgentTier,
    /// Relative to home; the agent's global skills folder.
    pub skills_dir: &'static str,
    /// Relative to home; existence means the agent is installed.
    pub detect_dir: &'static str,
    /// Other folders this agent also reads skills from (discovery only, never written).
    pub extra_scan_dirs: &'static [&'static str],
    /// Project-level skills path when it differs from `skills_dir`.
    pub project_skills_dir: Option<&'static str>,
    /// Walk nested category folders instead of immediate children (Hermes).
    pub recursive_scan: bool,
}

impl AgentAdapter {
    pub fn agent_key(&self) -> AgentKey {
        AgentKey(self.key.to_string())
    }

    pub fn summary(&self) -> AgentSummary {
        AgentSummary {
            key: self.agent_key(),
            display_name: self.display_name.to_string(),
            tier: self.tier,
        }
    }

    pub fn is_installed(&self, env: &Env) -> bool {
        env.resolve(self.detect_dir).exists()
    }

    /// Every folder this agent reads skills from, resolved.
    pub fn skill_dirs(&self, env: &Env) -> Vec<PathBuf> {
        std::iter::once(self.skills_dir)
            .chain(self.extra_scan_dirs.iter().copied())
            .map(|rel| env.resolve(rel))
            .collect()
    }
}

/// Built-ins plus the User's custom agents (set by `configure`).
static TABLE: RwLock<Option<&'static [AgentAdapter]>> = RwLock::new(None);
/// Agents the User switched off in Settings: SkillDesk does not read them.
static OFF: RwLock<Vec<String>> = RwLock::new(Vec::new());

pub fn registry() -> &'static [AgentAdapter] {
    TABLE.read().unwrap_or_else(|p| p.into_inner()).unwrap_or(builtin::BUILTIN)
}

/// Adds the custom agents after the built-ins (a key a built-in has keeps the
/// built-in) and records which agents are off. Called when the core opens and
/// after each change in Settings. The few strings of a custom agent live for
/// the whole process.
pub fn configure(custom: &[CustomAgent], off: &[String]) {
    let leak = |s: &str| -> &'static str { Box::leak(s.to_string().into_boxed_str()) };
    let mut all = builtin::BUILTIN.to_vec();
    for c in custom {
        if all.iter().any(|a| a.key == c.key) {
            continue;
        }
        let dir = leak(&c.skills_dir.to_string_lossy());
        all.push(AgentAdapter {
            key: leak(&c.key),
            display_name: leak(&c.name),
            tier: AgentTier::Skills,
            skills_dir: dir,
            detect_dir: dir,
            extra_scan_dirs: &[],
            project_skills_dir: c.project_dir.as_deref().map(leak),
            recursive_scan: false,
        });
    }
    *TABLE.write().unwrap_or_else(|p| p.into_inner()) = Some(Box::leak(all.into_boxed_slice()));
    *OFF.write().unwrap_or_else(|p| p.into_inner()) = off.to_vec();
}

pub fn is_off(key: &str) -> bool {
    OFF.read().unwrap_or_else(|p| p.into_inner()).iter().any(|k| k == key)
}

/// Agents on this Mac, read or not.
pub fn installed(env: &Env) -> Vec<&'static AgentAdapter> {
    registry().iter().filter(|a| a.is_installed(env)).collect()
}

/// Agents on this Mac that SkillDesk reads.
pub fn detected(env: &Env) -> Vec<&'static AgentAdapter> {
    installed(env).into_iter().filter(|a| !is_off(a.key)).collect()
}

pub fn find(key: &str) -> Option<&'static AgentAdapter> {
    registry().iter().find(|a| a.key == key)
}

/// A skills folder on disk and every detected agent that reads it. Folders are
/// keyed by device + inode, so `~/.agents/skills` read by several agents, a
/// project reached under two spellings (`Github` / `github` on a
/// case-insensitive disk), or home listed as a project are each one entry
/// (REVIEW item 7: shared directories).
#[derive(Debug, Clone)]
pub struct SkillFolder {
    pub path: PathBuf,
    pub agents: Vec<AgentKey>,
    pub recursive: bool,
    /// Set for project-level folders (`<repo>/.claude/skills`).
    pub project: Option<PathBuf>,
}

fn folder_key(dir: &std::path::Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(dir).ok().filter(|m| m.is_dir()).map(|m| (m.dev(), m.ino()))
}

pub fn skill_folders(env: &Env, projects: &[PathBuf]) -> Vec<SkillFolder> {
    let mut order: Vec<(u64, u64)> = Vec::new();
    let mut by_key: BTreeMap<(u64, u64), SkillFolder> = BTreeMap::new();
    let mut add = |dir: PathBuf, agent: &AgentAdapter, project: Option<&PathBuf>| {
        let Some(key) = folder_key(&dir) else { return };
        let entry = by_key.entry(key).or_insert_with(|| {
            order.push(key);
            SkillFolder { path: dir.clone(), agents: Vec::new(), recursive: false, project: project.cloned() }
        });
        let k = agent.agent_key();
        if !entry.agents.contains(&k) {
            entry.agents.push(k);
        }
        entry.recursive |= agent.recursive_scan;
    };
    let agents = detected(env);
    for agent in &agents {
        for dir in agent.skill_dirs(env) {
            add(dir, agent, None);
        }
    }
    for project in projects {
        for agent in &agents {
            if agent.tier == AgentTier::Shared {
                continue;
            }
            let rel = agent.project_skills_dir.unwrap_or(agent.skills_dir);
            add(project.join(rel), agent, Some(project));
        }
    }
    order.into_iter().filter_map(|k| by_key.remove(&k)).collect()
}
