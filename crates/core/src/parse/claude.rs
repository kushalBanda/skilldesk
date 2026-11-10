//! Claude Code files used by the skills scan: `~/.claude.json` usage counters,
//! `~/.claude/plugins/installed_plugins.json`, `~/.claude/settings.json`
//! `enabledPlugins`. Formats: docs/research/agent-data.md §3, §4.

use crate::env::Env;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

/// Claude's own bundled skills and slash commands: counted in `skillUsage` but
/// never on disk, so they must not read as "Gone". Kept short and explicit.
pub const BUILTIN_SKILLS: &[&str] = &[
    "init", "review", "security-review", "simplify", "loop", "schedule", "claude-api",
    "update-config", "keybindings-help", "fewer-permission-prompts", "run", "code-review",
    "claude-in-chrome", "plugin-authoring", "statusline", "pr-comments", "compact", "clear",
    "model", "help", "config", "memory", "cost", "doctor", "login", "logout", "resume",
    "agents", "mcp", "hooks", "permissions", "ultrareview", "artifact-design",
    "artifact-diagramming", "artifact-capabilities", "dataviz",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageCount {
    pub count: u64,
    /// ms since epoch.
    pub last_used_ms: Option<i64>,
}

/// Skill name as recorded (`caveman`, `caveman:caveman`) → bare name after the
/// last colon, as agent-data.md says ("merge by name after the colon").
pub fn bare_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

pub struct ClaudeSkillUsage {
    /// Raw keys in `skillUsage` (the Dashboard's "skills used").
    pub raw_entries: usize,
    /// Merged by bare name.
    pub by_name: HashMap<String, UsageCount>,
}

pub fn skill_usage(env: &Env) -> Option<ClaudeSkillUsage> {
    #[derive(serde::Deserialize)]
    struct Entry {
        #[serde(rename = "usageCount", default)]
        usage_count: u64,
        #[serde(rename = "lastUsedAt")]
        last_used_at: Option<i64>,
    }
    #[derive(serde::Deserialize)]
    struct ClaudeJson {
        #[serde(rename = "skillUsage", default)]
        skill_usage: BTreeMap<String, Entry>,
    }
    let text = std::fs::read_to_string(env.home.join(".claude.json")).ok()?;
    let parsed: ClaudeJson = serde_json::from_str(&text).ok()?;
    let mut by_name: HashMap<String, UsageCount> = HashMap::new();
    for (name, e) in &parsed.skill_usage {
        let u = by_name.entry(bare_name(name).to_string()).or_default();
        u.count += e.usage_count;
        u.last_used_ms = u.last_used_ms.max(e.last_used_at);
    }
    Some(ClaudeSkillUsage {
        raw_entries: parsed.skill_usage.len(),
        by_name,
    })
}

#[derive(Debug, Clone)]
pub struct InstalledPlugin {
    /// `name@marketplace`.
    pub id: String,
    pub install_path: PathBuf,
    pub enabled: bool,
}

pub fn installed_plugins(env: &Env) -> Vec<InstalledPlugin> {
    #[derive(serde::Deserialize)]
    struct Install {
        #[serde(rename = "installPath")]
        install_path: Option<PathBuf>,
        #[serde(rename = "projectPath")]
        project_path: Option<PathBuf>,
    }
    #[derive(serde::Deserialize)]
    struct Installed {
        #[serde(default)]
        plugins: BTreeMap<String, Vec<Install>>,
    }
    #[derive(serde::Deserialize)]
    struct Settings {
        #[serde(rename = "enabledPlugins", default)]
        enabled_plugins: BTreeMap<String, bool>,
    }
    let read = |p: PathBuf| std::fs::read_to_string(p).ok();
    let Some(installed) = read(env.home.join(".claude/plugins/installed_plugins.json"))
        .and_then(|t| serde_json::from_str::<Installed>(&t).ok())
    else {
        return Vec::new();
    };
    let enabled = read(env.home.join(".claude/settings.json"))
        .and_then(|t| serde_json::from_str::<Settings>(&t).ok())
        .map(|s| s.enabled_plugins)
        .unwrap_or_default();
    let mut out = Vec::new();
    for (id, installs) in installed.plugins {
        for inst in installs {
            if let Some(install_path) = inst.install_path {
                out.push(InstalledPlugin {
                    // A project-scope install is on in its project.
                    enabled: inst.project_path.is_some() || enabled.get(&id).copied().unwrap_or(false),
                    id: id.clone(),
                    install_path,
                });
            }
        }
    }
    out
}

/// Project folders Claude has opened (`~/.claude.json` → `projects` keys) that still exist.
pub fn project_paths(env: &Env) -> Vec<PathBuf> {
    project_keys(env).into_iter().filter(|p| p.is_dir()).collect()
}

/// Every project Claude has recorded, including folders that are gone.
pub fn project_keys(env: &Env) -> Vec<PathBuf> {
    #[derive(serde::Deserialize)]
    struct ClaudeJson {
        #[serde(default)]
        projects: BTreeMap<String, serde_json::Value>,
    }
    let Some(parsed) = std::fs::read_to_string(env.home.join(".claude.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<ClaudeJson>(&t).ok())
    else {
        return Vec::new();
    };
    parsed.projects.into_keys().map(PathBuf::from).collect()
}
