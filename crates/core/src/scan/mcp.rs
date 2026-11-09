//! MCP servers for the full-tier agents (agent-data.md §6), one row per server
//! name, with Claude's calls from transcripts (`mcp__<server>__<tool>`).
//!
//! Key and header values are never kept: only their names. Command args and
//! URLs are shown with anything key-like masked and query strings dropped.

use crate::adapters;
use crate::env::Env;
use crate::fsx::json_span::strip_comments;
use crate::model::{AgentKey, Item, ItemId, ItemKind, ItemStatus, KindFacts, McpFacts, McpLocation, SourceRef, SwitchKind, Usage};
use crate::parse::claude_runs::Calls;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// How a config file stores servers; decides how to switch and write them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// `~/.claude.json`: Claude rewrites it while running; SkillDesk only reads it.
    ClaudeJson,
    /// Repo `.mcp.json`: switched in `.claude/settings.local.json`.
    McpJson,
    /// `mcpServers` object in a JSON file we may edit (Cursor, Gemini, Copilot).
    Json,
    /// OpenCode `mcp` with its own `enabled`.
    OpenCode,
    /// Codex `config.toml` `[mcp_servers.<name>]`.
    CodexToml,
    /// Inside a Claude plugin.
    Plugin,
}

#[derive(Debug, Clone)]
pub struct RawMcp {
    pub agent: &'static str,
    pub file: PathBuf,
    /// JSON path of the object holding the server (e.g. `["mcpServers"]`).
    pub path: Vec<String>,
    pub name: String,
    pub value: Value,
    pub project: Option<PathBuf>,
    pub plugin: Option<String>,
    pub enabled: bool,
    pub format: Format,
}

impl RawMcp {
    pub fn entry(&self) -> String {
        crate::actions::sha(&format!("{}\0{}\0{}\0{}", self.agent, self.file.display(), self.path.join("\u{1f}"), self.name))
    }

    /// The name Claude uses in tool names.
    pub fn call_name(&self) -> String {
        let norm = normalize(&self.name);
        match &self.plugin {
            Some(id) => format!("plugin_{}_{norm}", normalize(id.split('@').next().unwrap_or(id))),
            None => norm,
        }
    }

    pub fn switch(&self) -> SwitchKind {
        match self.format {
            Format::ClaudeJson => SwitchKind::None,
            Format::Plugin => SwitchKind::Plugin,
            Format::Json => SwitchKind::Stash,
            Format::McpJson | Format::OpenCode | Format::CodexToml => SwitchKind::Native,
        }
    }
}

/// Claude's tool-name form: characters other than letters, digits, `_`, `-` become `_`.
pub fn normalize(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

fn read_json(p: &Path) -> Option<Value> {
    serde_json::from_str(&strip_comments(&std::fs::read_to_string(p).ok()?)).ok()
}

fn servers_at<'a>(v: &'a Value, path: &[&str]) -> Option<&'a serde_json::Map<String, Value>> {
    let mut cur = v;
    for k in path {
        cur = cur.get(*k)?;
    }
    cur.as_object()
}

/// Every configured server entry, plus entries SkillDesk stashed (switched off).
pub fn configured(env: &Env) -> Vec<RawMcp> {
    let on = |key: &str| adapters::find(key).is_some_and(|a| a.is_installed(env));
    let home = &env.home;
    let mut out = Vec::new();
    let push_json = |out: &mut Vec<RawMcp>, agent: &'static str, file: PathBuf, path: &[&str], format: Format, project: Option<PathBuf>, plugin: Option<(String, bool)>, disabled: &HashSet<String>| {
        let Some(v) = read_json(&file) else { return };
        let Some(map) = servers_at(&v, path) else { return };
        for (name, value) in map {
            let enabled = match format {
                Format::OpenCode => value.get("enabled").and_then(|e| e.as_bool()).unwrap_or(true),
                _ => !disabled.contains(name) && plugin.as_ref().is_none_or(|p| p.1),
            };
            out.push(RawMcp {
                agent,
                file: file.clone(),
                path: path.iter().map(|s| s.to_string()).collect(),
                name: name.clone(),
                value: value.clone(),
                project: project.clone(),
                plugin: plugin.as_ref().map(|p| p.0.clone()),
                enabled,
                format,
            });
        }
    };
    let none = HashSet::new();

    if on("claude_code") {
        let cj = home.join(".claude.json");
        push_json(&mut out, "claude_code", cj.clone(), &["mcpServers"], Format::ClaudeJson, None, None, &none);
        let projects: Vec<(String, HashSet<String>)> = read_json(&cj)
            .and_then(|v| v.get("projects").and_then(|p| p.as_object()).cloned())
            .map(|m| {
                m.into_iter()
                    .map(|(k, v)| {
                        let off = v
                            .get("disabledMcpServers")
                            .and_then(|d| d.as_array())
                            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                            .unwrap_or_default();
                        (k, off)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let home_canon = std::fs::canonicalize(home).ok();
        let mut seen_repos = HashSet::new();
        for (proj, off) in &projects {
            let p = PathBuf::from(proj);
            push_json(&mut out, "claude_code", cj.clone(), &["projects", proj, "mcpServers"], Format::ClaudeJson, Some(p.clone()), None, off);
            if !p.is_dir() || std::fs::canonicalize(&p).ok() == home_canon {
                continue;
            }
            if !seen_repos.insert(std::fs::canonicalize(&p).unwrap_or(p.clone())) {
                continue;
            }
            let mut mcpjson_off: HashSet<String> = HashSet::new();
            for f in [".claude/settings.json", ".claude/settings.local.json"] {
                if let Some(v) = read_json(&p.join(f)) {
                    for x in v.get("disabledMcpjsonServers").and_then(|d| d.as_array()).into_iter().flatten() {
                        if let Some(s) = x.as_str() {
                            mcpjson_off.insert(s.to_string());
                        }
                    }
                }
            }
            push_json(&mut out, "claude_code", p.join(".mcp.json"), &["mcpServers"], Format::McpJson, Some(p.clone()), None, &mcpjson_off);
            if on("cursor") {
                push_json(&mut out, "cursor", p.join(".cursor/mcp.json"), &["mcpServers"], Format::Json, Some(p.clone()), None, &none);
            }
        }
        for plugin in crate::parse::claude::installed_plugins(env) {
            let file = plugin.install_path.join(".mcp.json");
            // Plugins write servers under `mcpServers` or at the top level.
            let top = read_json(&file).is_some_and(|v| v.get("mcpServers").is_none());
            let path: &[&str] = if top { &[] } else { &["mcpServers"] };
            push_json(&mut out, "claude_code", file, path, Format::Plugin, None, Some((plugin.id.clone(), plugin.enabled)), &none);
        }
    }
    if on("cursor") {
        push_json(&mut out, "cursor", home.join(".cursor/mcp.json"), &["mcpServers"], Format::Json, None, None, &none);
    }
    if on("gemini_cli") {
        push_json(&mut out, "gemini_cli", home.join(".gemini/settings.json"), &["mcpServers"], Format::Json, None, None, &none);
    }
    if on("github_copilot") {
        for f in [".copilot/mcp-config.json", ".copilot/config.json"] {
            push_json(&mut out, "github_copilot", home.join(f), &["mcpServers"], Format::Json, None, None, &none);
        }
    }
    if on("opencode") {
        for f in [".config/opencode/opencode.json", ".config/opencode/opencode.jsonc"] {
            push_json(&mut out, "opencode", env.resolve(f), &["mcp"], Format::OpenCode, None, None, &none);
        }
    }
    if on("codex") {
        let file = home.join(".codex/config.toml");
        if let Some(t) = std::fs::read_to_string(&file).ok().and_then(|t| t.parse::<toml::Table>().ok()) {
            for (name, v) in t.get("mcp_servers").and_then(|m| m.as_table()).into_iter().flatten() {
                let value = serde_json::to_value(v).unwrap_or(Value::Null);
                out.push(RawMcp {
                    agent: "codex",
                    file: file.clone(),
                    path: vec!["mcp_servers".into()],
                    name: name.clone(),
                    enabled: value.get("enabled").and_then(|e| e.as_bool()).unwrap_or(true),
                    value,
                    project: None,
                    plugin: None,
                    format: Format::CodexToml,
                });
            }
        }
    }
    // Switched off by SkillDesk: kept in the stash.
    for e in crate::actions::stash::read_mcp(env) {
        out.push(e.to_raw());
    }
    out
}

fn looks_secret(s: &str) -> bool {
    let lower = s.to_lowercase();
    if ["key=", "token=", "secret=", "password=", "bearer "].iter().any(|k| lower.contains(k)) {
        return true;
    }
    let run = s.chars().fold((0usize, 0usize), |(best, cur), c| {
        let cur = if c.is_ascii_alphanumeric() || c == '_' || c == '-' { cur + 1 } else { 0 };
        (best.max(cur), cur)
    });
    run.0 >= 32 && s.chars().any(|c| c.is_ascii_digit()) && !s.contains('/')
}

fn mask(s: &str) -> String {
    if looks_secret(s) { "····".into() } else { s.to_string() }
}

pub fn location(r: &RawMcp) -> McpLocation {
    let v = &r.value;
    let strs = |k: &str| -> Vec<String> {
        match v.get(k) {
            Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
            Some(Value::String(s)) => vec![s.clone()],
            _ => Vec::new(),
        }
    };
    let keys = |k: &str| -> Vec<String> {
        v.get(k).and_then(|o| o.as_object()).map(|o| o.keys().cloned().collect()).unwrap_or_default()
    };
    let mut cmd = strs("command");
    cmd.extend(strs("args"));
    let url = ["url", "httpUrl", "serverUrl"]
        .iter()
        .find_map(|k| v.get(*k).and_then(|u| u.as_str()))
        .map(|u| u.split('?').next().unwrap_or(u).to_string());
    let declared = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
    let transport = match (declared, &url) {
        ("sse", _) => "sse",
        ("local" | "stdio", _) => "stdio",
        (_, Some(_)) => "http",
        _ => "stdio",
    };
    let mut env_keys = keys("env");
    env_keys.extend(keys("environment"));
    let mut header_keys = keys("headers");
    header_keys.extend(keys("http_headers"));
    McpLocation {
        entry: r.entry(),
        agent: AgentKey(r.agent.into()),
        name: r.name.clone(),
        file: r.file.clone(),
        project: r.project.clone(),
        plugin: r.plugin.clone(),
        transport: transport.into(),
        command: (!cmd.is_empty()).then(|| cmd.iter().map(|a| mask(a)).collect::<Vec<_>>().join(" ")),
        url: url.map(|u| mask(&u)),
        env_keys,
        header_keys,
        enabled: r.enabled,
        switch: r.switch(),
    }
}

fn usage_of(c: &Calls) -> Usage {
    Usage {
        count: c.count,
        last_used: c.last.and_then(chrono::DateTime::from_timestamp_millis).map(|d| d.to_rfc3339()),
        timing: None,
    }
}

pub fn scan(env: &Env, calls: Option<&HashMap<String, Calls>>) -> Vec<Item> {
    let raws = configured(env);
    let mut rows: BTreeMap<String, (String, Vec<McpLocation>)> = BTreeMap::new();
    for r in &raws {
        let row = rows.entry(r.call_name()).or_insert_with(|| (r.name.clone(), Vec::new()));
        row.1.push(location(r));
    }
    let mut items = Vec::new();
    for (call_name, (name, locations)) in &rows {
        let used = calls.and_then(|c| c.get(call_name));
        // Agents whose history SkillDesk reads for calls: Claude Code and Codex.
        let in_claude = locations.iter().any(|l| l.agent.0 == "claude_code" || l.agent.0 == "codex");
        let any_on = locations.iter().any(|l| l.enabled);
        let status = if !any_on {
            ItemStatus::Off
        } else if calls.is_some() && in_claude && used.is_none() {
            ItemStatus::Unused
        } else {
            ItemStatus::Ok
        };
        let mut agents: Vec<AgentKey> = Vec::new();
        for l in locations {
            if !agents.contains(&l.agent) {
                agents.push(l.agent.clone());
            }
        }
        let mut files: Vec<SourceRef> = Vec::new();
        for l in locations {
            if !files.iter().any(|f| f.path == l.file) {
                files.push(SourceRef { path: l.file.clone(), line: None });
            }
        }
        items.push(Item {
            id: ItemId::derive(ItemKind::McpServer, Path::new(""), call_name),
            kind: ItemKind::McpServer,
            name: name.clone(),
            agents,
            status,
            sources: files,
            usage: used.map(usage_of),
            facts: KindFacts::McpServer(McpFacts {
                call_name: call_name.clone(),
                holds_keys: locations.iter().any(|l| !l.env_keys.is_empty() || !l.header_keys.is_empty()),
                locations: locations.clone(),
                unconfigured: false,
            }),
        });
    }
    // Called, but in no config: built-in servers and claude.ai connectors.
    if let Some(calls) = calls {
        for (call_name, c) in calls {
            if rows.contains_key(call_name) {
                continue;
            }
            let name = call_name.strip_prefix("claude_ai_").map(|n| format!("claude.ai {}", n.replace('_', " "))).unwrap_or_else(|| call_name.clone());
            items.push(Item {
                id: ItemId::derive(ItemKind::McpServer, Path::new(""), call_name),
                kind: ItemKind::McpServer,
                name,
                agents: vec![AgentKey("claude_code".into())],
                status: ItemStatus::Ok,
                sources: Vec::new(),
                usage: Some(usage_of(c)),
                facts: KindFacts::McpServer(McpFacts {
                    call_name: call_name.clone(),
                    locations: Vec::new(),
                    holds_keys: false,
                    unconfigured: true,
                }),
            });
        }
    }
    items.sort_by_key(|i| i.name.to_lowercase());
    items
}
