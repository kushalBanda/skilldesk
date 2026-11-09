//! Subagents and slash commands (agent-data.md): definition files for every
//! full-tier agent, one row per bare name, with Claude's runs from transcripts
//! (`subagent_type` of Agent/Task calls, typed `<command-name>`).
//!
//! Read only. Names merge after the last colon, like skills, so
//! `caveman:caveman` and `caveman` are one row.

use crate::adapters;
use crate::env::Env;
use crate::model::{AgentKey, DefFacts, DefLocation, DefOrigin, Item, ItemId, ItemKind, ItemStatus, KindFacts, PluginRef, SourceRef, Usage};
use crate::parse::claude::bare_name;
use crate::parse::claude_runs::Calls;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Claude Code's own subagent types.
const BUILTIN_SUBAGENTS: &[&str] = &["general-purpose", "Explore", "Plan", "statusline-setup", "claude-code-guide", "output-style-setup", "fork"];

/// Claude Code's own slash commands (plus `BUILTIN_SKILLS`). Kept explicit;
/// a missing one shows under "Run before, no file now", never as broken.
const BUILTIN_COMMANDS: &[&str] = &[
    "add-dir", "agents", "artifacts", "bashes", "btw", "bug", "chrome", "clear", "color", "compact", "config", "context",
    "copy", "cost", "desktop", "doctor", "effort", "exit", "export", "extra-usage", "fast", "feedback", "help", "hooks",
    "ide", "init", "insights", "install-github-app", "keybindings", "login", "logout", "mcp", "memory", "mobile", "model",
    "output-style", "permissions", "plan", "plugin", "plugins", "pr-comments", "privacy-settings", "quit", "release-notes",
    "reload-plugins", "remote-control", "rename", "resume", "review", "rewind", "sandbox", "security-review", "skills",
    "stats", "status", "statusline", "stickers", "tasks", "terminal-setup", "theme", "todos", "upgrade", "usage", "vim",
    "voice",
];

pub struct DefScan {
    pub subagents: Vec<Item>,
    pub commands: Vec<Item>,
}

/// Runs from the last transcript read; `None` before the first one.
pub struct Runs<'a> {
    pub subagents: &'a HashMap<String, Calls>,
    pub commands: &'a HashMap<String, Calls>,
}

struct Found {
    loc: DefLocation,
    description: Option<String>,
    model: Option<String>,
}

pub fn scan(env: &Env, runs: Option<Runs>, skill_names: &HashSet<String>) -> DefScan {
    let (agents, commands) = find(env);
    let empty = HashMap::new();
    let sub_runs = merge(runs.as_ref().map_or(&empty, |r| r.subagents));
    let cmd_runs = merge(runs.as_ref().map_or(&empty, |r| r.commands));
    let read = runs.is_some();
    DefScan {
        subagents: rows(ItemKind::Subagent, agents, &sub_runs, read, &|n| BUILTIN_SUBAGENTS.contains(&n), &|_| false),
        commands: rows(
            ItemKind::Command,
            commands,
            &cmd_runs,
            read,
            &|n| BUILTIN_COMMANDS.contains(&n) || crate::parse::claude::BUILTIN_SKILLS.contains(&n),
            &|n| skill_names.contains(n),
        ),
    }
}

fn merge(runs: &HashMap<String, Calls>) -> HashMap<String, Calls> {
    let mut out: HashMap<String, Calls> = HashMap::new();
    for (name, c) in runs {
        let e = out.entry(bare_name(name).to_string()).or_default();
        e.count += c.count;
        e.last = e.last.max(c.last);
    }
    out
}

fn rows(
    kind: ItemKind,
    found: Vec<Found>,
    runs: &HashMap<String, Calls>,
    read: bool,
    builtin: &dyn Fn(&str) -> bool,
    skill: &dyn Fn(&str) -> bool,
) -> Vec<Item> {
    let mut by_name: BTreeMap<String, Vec<Found>> = BTreeMap::new();
    for f in found {
        by_name.entry(bare_name(&f.loc.name).to_string()).or_default().push(f);
    }
    let mut names: Vec<String> = by_name.keys().cloned().collect();
    names.extend(runs.keys().filter(|n| !by_name.contains_key(*n)).cloned());
    let mut out = Vec::new();
    for name in names {
        let found = by_name.remove(&name).unwrap_or_default();
        let origin = if !found.is_empty() {
            DefOrigin::File
        } else if builtin(&name) {
            DefOrigin::BuiltIn
        } else if skill(&name) {
            DefOrigin::Skill
        } else {
            DefOrigin::Unknown
        };
        let used = runs.get(&name);
        let claude_file = found.iter().any(|f| f.loc.agent.0 == "claude_code");
        // Runs are Claude's: shown even when only another agent has a file by this name.
        let claude = claude_file || origin != DefOrigin::File || used.is_some();
        let mut agents: Vec<AgentKey> = found.iter().map(|f| f.loc.agent.clone()).collect();
        if claude {
            agents.push(AgentKey("claude_code".into()));
        }
        agents.sort();
        agents.dedup();
        let all_off = !found.is_empty() && found.iter().all(|f| f.loc.plugin.as_ref().is_some_and(|p| !p.enabled));
        let status = if origin == DefOrigin::Unknown {
            ItemStatus::Gone
        } else if all_off {
            ItemStatus::Off
        } else if read && claude_file && used.is_none() {
            ItemStatus::Unused
        } else {
            ItemStatus::Ok
        };
        let sources = found.iter().map(|f| SourceRef { path: f.loc.path.clone(), line: None }).collect();
        let description = found.iter().find_map(|f| f.description.clone());
        let model = found.iter().find_map(|f| f.model.clone());
        let item_name = name.clone();
        out.push(Item {
            id: ItemId::derive(kind, Path::new(""), &name),
            kind,
            name,
            agents,
            status,
            sources,
            usage: if claude { used.map(|c| Usage { count: c.count, last_used: c.last.and_then(chrono::DateTime::from_timestamp_millis).map(|d| d.to_rfc3339()), timing: None }) } else { None },
            facts: {
                let f = DefFacts { origin, skill: skill(&item_name), description, model, locations: found.into_iter().map(|f| f.loc).collect() };
                if kind == ItemKind::Subagent { KindFacts::Subagent(f) } else { KindFacts::Command(f) }
            },
        });
    }
    out
}

/// Where each agent keeps definitions. `prefix` is put before the file's name
/// (Codex `prompts:`); `ext` is the file type; `cut` is a name suffix to drop.
struct Dir {
    agent: &'static str,
    path: PathBuf,
    ext: &'static str,
    prefix: String,
    cut: &'static str,
    project: Option<PathBuf>,
    plugin: Option<PluginRef>,
}

fn find(env: &Env) -> (Vec<Found>, Vec<Found>) {
    let home = &env.home;
    let on: HashSet<&str> = adapters::detected(env).iter().map(|a| a.key).collect();
    let dir = |agent, path: PathBuf, ext, project: Option<&PathBuf>| Dir { agent, path, ext, prefix: String::new(), cut: "", project: project.cloned(), plugin: None };
    let mut agent_dirs: Vec<Dir> = Vec::new();
    let mut command_dirs: Vec<Dir> = Vec::new();
    let mut add = |global: bool, project: Option<&PathBuf>| {
        let base = project.unwrap_or(home);
        if on.contains("claude_code") {
            agent_dirs.push(dir("claude_code", base.join(".claude/agents"), "md", project));
            command_dirs.push(dir("claude_code", base.join(".claude/commands"), "md", project));
        }
        if on.contains("opencode") {
            let root = if global { home.join(".config/opencode") } else { base.join(".opencode") };
            for d in ["agents", "agent"] {
                agent_dirs.push(dir("opencode", root.join(d), "md", project));
            }
            for d in ["commands", "command"] {
                command_dirs.push(dir("opencode", root.join(d), "md", project));
            }
        }
        if on.contains("cursor") {
            agent_dirs.push(dir("cursor", base.join(".cursor/agents"), "md", project));
            command_dirs.push(dir("cursor", base.join(".cursor/commands"), "md", project));
        }
        if on.contains("gemini_cli") {
            agent_dirs.push(dir("gemini_cli", base.join(".gemini/agents"), "md", project));
            command_dirs.push(dir("gemini_cli", base.join(".gemini/commands"), "toml", project));
        }
        if on.contains("github_copilot") {
            let root = if global { home.join(".copilot") } else { base.join(".github") };
            agent_dirs.push(Dir { cut: ".agent", ..dir("github_copilot", root.join("agents"), "md", project) });
            command_dirs.push(Dir { cut: ".prompt", ..dir("github_copilot", root.join("prompts"), "md", project) });
        }
        if global && on.contains("codex") {
            command_dirs.push(Dir { prefix: "prompts:".into(), ..dir("codex", home.join(".codex/prompts"), "md", None) });
        }
    };
    add(true, None);
    let home_canon = std::fs::canonicalize(home).ok();
    let mut seen = HashSet::new();
    for p in crate::parse::claude::project_paths(env) {
        let canon = std::fs::canonicalize(&p).unwrap_or(p.clone());
        if Some(&canon) == home_canon.as_ref() || !seen.insert(canon) {
            continue;
        }
        add(false, Some(&p));
    }
    if on.contains("claude_code") {
        for plugin in crate::parse::claude::installed_plugins(env) {
            let short = plugin.id.split('@').next().unwrap_or(&plugin.id).to_string();
            let pref = PluginRef { id: plugin.id.clone(), enabled: plugin.enabled };
            for (list, sub) in [(&mut agent_dirs, "agents"), (&mut command_dirs, "commands")] {
                list.push(Dir {
                    prefix: format!("{short}:"),
                    plugin: Some(pref.clone()),
                    ..dir("claude_code", plugin.install_path.join(sub), "md", None)
                });
            }
        }
    }
    let collect = |dirs: Vec<Dir>, subagent: bool| {
        let mut out = Vec::new();
        let mut seen: HashSet<(String, PathBuf)> = HashSet::new();
        for d in dirs {
            for file in files(&d.path, d.ext) {
                let canon = std::fs::canonicalize(&file).unwrap_or(file.clone());
                if !seen.insert((d.agent.to_string(), canon)) {
                    continue;
                }
                let text = std::fs::read_to_string(&file).unwrap_or_default();
                let rel = file.strip_prefix(&d.path).unwrap_or(&file).with_extension("");
                let mut stem = rel.to_string_lossy().replace('/', ":");
                if !d.cut.is_empty() {
                    stem = stem.strip_suffix(d.cut).map(str::to_string).unwrap_or(stem);
                }
                let (fm_name, description, model) = if d.ext == "toml" { toml_meta(&text) } else { md_meta(&text) };
                // A subagent's name is its frontmatter `name`; a command's is its path.
                let base = if subagent { fm_name.unwrap_or(stem) } else { stem };
                out.push(Found {
                    loc: DefLocation {
                        agent: AgentKey(d.agent.to_string()),
                        name: format!("{}{base}", d.prefix),
                        path: file,
                        project: d.project.clone(),
                        plugin: d.plugin.clone(),
                    },
                    description,
                    model,
                });
            }
        }
        out
    };
    (collect(agent_dirs, true), collect(command_dirs, false))
}

/// Definition files under `dir` (recursive, hidden entries skipped, links followed).
fn files(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), 0)];
    while let Some((d, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if e.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            if p.is_dir() {
                if depth < 4 {
                    stack.push((p, depth + 1));
                }
            } else if p.extension().is_some_and(|x| x == ext) && p.is_file() {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn md_meta(text: &str) -> (Option<String>, Option<String>, Option<String>) {
    let meta = crate::parse::skills::parse_frontmatter(text);
    let model = text
        .strip_prefix("---")
        .and_then(|r| r.find("\n---").map(|e| &r[..e]))
        .and_then(|fm| fm.lines().find_map(|l| l.strip_prefix("model:")))
        .map(|v| v.trim().trim_matches(['"', '\'']).to_string())
        .filter(|v| !v.is_empty());
    (meta.name, meta.description, model)
}

fn toml_meta(text: &str) -> (Option<String>, Option<String>, Option<String>) {
    let v: toml::Value = toml::from_str(text).unwrap_or(toml::Value::Table(Default::default()));
    let description = v.get("description").and_then(|d| d.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    (None, description, None)
}
