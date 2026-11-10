//! Plugins of agents other than Claude Code (multi-agent Plugins screen).
//! Read only: each agent switches its own plugins.
//!
//! - Codex: `~/.codex/plugins/cache/<market>/<name>/<version>/.codex-plugin/plugin.json`.
//!   Codex keeps no on/off switch on disk; an installed plugin is on.
//! - Cursor: `~/.cursor/plugins/{cache/<market>,local}/<name>/<rev>/.cursor-plugin/plugin.json`
//!   (newest revision). On/off lives in Cursor's own database, which is never
//!   written; an installed plugin shows as on.
//! - OpenCode: files in `~/.config/opencode/plugins/` load by themselves;
//!   folders and npm packages load when `opencode.json` (or `tui.json`, for
//!   TUI plugins) `plugin` lists them.
//! - Gemini CLI: `~/.gemini/extensions/<name>/gemini-extension.json`; off when
//!   `extension-enablement.json` lists only `!` (disabled) path patterns.

use super::plugins::dir_size;
use crate::env::Env;
use crate::model::{AgentKey, Item, ItemId, ItemKind, ItemStatus, KindFacts, PluginContents, PluginFacts, SourceRef};
use serde_json::Value;
use std::path::{Path, PathBuf};

fn read_json(p: &Path) -> Option<Value> {
    let t = std::fs::read_to_string(p).ok()?;
    serde_json::from_str(&crate::fsx::json_span::strip_comments(&t)).ok()
}

fn dirs(p: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    v.sort();
    v
}

/// The most recently changed child folder (the version or revision in use).
fn newest(p: &Path) -> Option<PathBuf> {
    dirs(p)
        .into_iter()
        .filter(|d| !d.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')))
        .max_by_key(|d| std::fs::metadata(d).and_then(|m| m.modified()).ok())
}

fn count(dir: &Path, pred: impl Fn(&Path) -> bool) -> u32 {
    std::fs::read_dir(dir).map(|rd| rd.flatten().filter(|e| pred(&e.path())).count() as u32).unwrap_or(0)
}

fn mcp_count(v: Option<Value>) -> u32 {
    v.map(|v| v.get("mcpServers").unwrap_or(&v).as_object().map(|o| o.len() as u32).unwrap_or(0)).unwrap_or(0)
}

fn hook_count(v: Option<Value>) -> u32 {
    v.and_then(|v| v.get("hooks").and_then(|h| h.as_object()).map(|o| o.values().filter_map(|g| g.as_array()).map(|a| a.len() as u32).sum()))
        .unwrap_or(0)
}

/// Skills, commands, agents, hooks and MCP servers under a plugin root.
fn contents(root: &Path, hooks_file: Option<PathBuf>, mcp: u32) -> PluginContents {
    let md = |p: &Path| p.extension().is_some_and(|x| x == "md" || x == "toml");
    PluginContents {
        skills: count(&root.join("skills"), crate::parse::skills::is_skill_dir),
        commands: count(&root.join("commands"), md),
        agents: count(&root.join("agents"), md),
        hooks: hook_count(hooks_file.and_then(|f| read_json(&f))),
        mcp_servers: mcp,
    }
}

fn mtime(p: &Path) -> Option<String> {
    let t = std::fs::metadata(p).and_then(|m| m.modified()).ok()?;
    Some(chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339())
}

struct Found {
    agent: &'static str,
    name: String,
    market: String,
    version: Option<String>,
    root: PathBuf,
    manifest: PathBuf,
    on: Option<bool>,
    contents: PluginContents,
}

fn item(f: Found) -> Item {
    let status = match f.on {
        Some(false) => ItemStatus::Off,
        _ => ItemStatus::Ok,
    };
    Item {
        id: ItemId::derive(ItemKind::Plugin, &f.manifest, f.agent),
        kind: ItemKind::Plugin,
        name: f.name.clone(),
        agents: vec![AgentKey(f.agent.into())],
        status,
        sources: vec![SourceRef { path: f.manifest.clone(), line: None }],
        usage: None,
        facts: KindFacts::Plugin(PluginFacts {
            id: format!("{}@{}", f.name, f.market),
            marketplace: f.market,
            installed: true,
            enabled: f.on,
            version: f.version,
            scope: Some("user".into()),
            project: None,
            size_bytes: Some(dir_size(&f.root)),
            restore: None,
            installed_at: mtime(&f.root),
            updated_at: None,
            install_path: Some(f.root),
            contents: f.contents,
        }),
    }
}

fn codex(env: &Env, out: &mut Vec<Item>) {
    for market in dirs(&env.home.join(".codex/plugins/cache")) {
        for plugin in dirs(&market) {
            let Some(root) = newest(&plugin) else { continue };
            let manifest = root.join(".codex-plugin/plugin.json");
            let Some(m) = read_json(&manifest) else { continue };
            let s = |k: &str| m.get(k).and_then(|v| v.as_str()).map(str::to_string);
            let name = m.pointer("/interface/displayName").and_then(|v| v.as_str()).map(str::to_string).or_else(|| s("name")).unwrap_or_default();
            let mcp = mcp_count(read_json(&root.join(".mcp.json")));
            let hooks = s("hooks").map(|h| root.join(h)).or_else(|| Some(root.join("hooks/hooks.json")));
            out.push(item(Found {
                agent: "codex",
                name,
                market: market.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                version: s("version"),
                contents: contents(&root, hooks, mcp),
                root,
                manifest,
                on: None,
            }));
        }
    }
}

fn cursor(env: &Env, out: &mut Vec<Item>) {
    let base = env.home.join(".cursor/plugins");
    let mut roots: Vec<(String, PathBuf)> = Vec::new();
    for market in dirs(&base.join("cache")) {
        let mname = market.file_name().unwrap_or_default().to_string_lossy().into_owned();
        for plugin in dirs(&market) {
            if let Some(r) = newest(&plugin) {
                roots.push((mname.clone(), r));
            }
        }
    }
    for local in dirs(&base.join("local")) {
        roots.push(("local".into(), local));
    }
    for (market, root) in roots {
        // Cursor also reads a Claude-format manifest when it has no own one.
        let Some(manifest) = [".cursor-plugin/plugin.json", ".claude-plugin/plugin.json"].iter().map(|f| root.join(f)).find(|p| p.is_file()) else { continue };
        let Some(m) = read_json(&manifest) else { continue };
        let s = |k: &str| m.get(k).and_then(|v| v.as_str()).map(str::to_string);
        let mcp = mcp_count(s("mcpServers").map(|p| root.join(p)).and_then(|p| read_json(&p)).or_else(|| read_json(&root.join(".mcp.json"))));
        let hooks = s("hooks").map(|h| root.join(h)).or_else(|| Some(root.join("hooks/hooks.json")));
        out.push(item(Found {
            agent: "cursor",
            name: s("displayName").or_else(|| s("name")).unwrap_or_default(),
            market,
            version: s("version"),
            contents: contents(&root, hooks, mcp),
            root,
            manifest,
            on: None,
        }));
    }
}

fn opencode(env: &Env, out: &mut Vec<Item>) {
    let cfg_dir = env.home.join(".config/opencode");
    let cfg_file = ["opencode.json", "opencode.jsonc"].iter().map(|f| cfg_dir.join(f)).find(|p| p.is_file());
    let list = |f: Option<&Path>| -> Vec<String> {
        f.and_then(read_json)
            .and_then(|v| v.get("plugin").and_then(|p| p.as_array()).cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|v| v.as_str().map(|s| s.trim_start_matches("file://").to_string()))
            .collect()
    };
    let listed = list(cfg_file.as_deref());
    // TUI plugins are listed in `tui.json`.
    let tui = list(Some(&cfg_dir.join("tui.json")));
    let in_config = |p: &Path| {
        listed.iter().chain(tui.iter()).any(|l| (l.starts_with('.') || l.starts_with('/')) && cfg_dir.join(l).starts_with(p))
    };
    for dir in ["plugins", "plugin"] {
        for e in std::fs::read_dir(cfg_dir.join(dir)).into_iter().flatten().flatten() {
            let p = e.path();
            let fname = e.file_name().to_string_lossy().into_owned();
            if fname.starts_with('.') {
                continue;
            }
            let is_code = p.extension().is_some_and(|x| x == "js" || x == "ts" || x == "mjs");
            if p.is_file() && !is_code {
                continue;
            }
            let pkg = read_json(&p.join("package.json"));
            let name = pkg.as_ref().and_then(|v| v.get("name")?.as_str().map(str::to_string)).unwrap_or_else(|| p.file_stem().unwrap_or_default().to_string_lossy().into_owned());
            // Files load by themselves; a folder loads only when the config points into it.
            let on = p.is_file() || in_config(&p);
            out.push(item(Found {
                agent: "opencode",
                name: name.trim_end_matches("-opencode-plugin").to_string(),
                market: if in_config(&p) { "config".into() } else { format!("{dir}/") },
                version: pkg.as_ref().and_then(|v| v.get("version")?.as_str().map(str::to_string)),
                contents: if p.is_dir() { contents(&p, None, 0) } else { PluginContents::default() },
                manifest: p.clone(),
                root: p,
                on: Some(on),
            }));
        }
    }
    // npm packages named in the config.
    if let Some(file) = cfg_file {
        for l in listed.iter().filter(|l| !l.starts_with('.') && !l.starts_with('/')) {
            let (name, version) = match l.rsplit_once('@') {
                Some((n, v)) if !n.is_empty() => (n.to_string(), Some(v.to_string())),
                _ => (l.clone(), None),
            };
            out.push(Item {
                id: ItemId::derive(ItemKind::Plugin, &file, &format!("opencode\0{l}")),
                kind: ItemKind::Plugin,
                name: name.clone(),
                agents: vec![AgentKey("opencode".into())],
                status: ItemStatus::Ok,
                sources: vec![SourceRef { path: file.clone(), line: None }],
                usage: None,
                facts: KindFacts::Plugin(PluginFacts {
                    id: format!("{name}@npm"),
                    marketplace: "npm".into(),
                    installed: true,
                    enabled: Some(true),
                    version,
                    scope: Some("user".into()),
                    project: None,
                    install_path: None,
                    installed_at: None,
                    updated_at: None,
                    contents: PluginContents::default(),
                    size_bytes: None,
                    restore: None,
                }),
            });
        }
    }
}

fn gemini(env: &Env, out: &mut Vec<Item>) {
    let base = env.home.join(".gemini/extensions");
    let enablement = read_json(&base.join("extension-enablement.json")).unwrap_or(Value::Null);
    for root in dirs(&base) {
        let manifest = root.join("gemini-extension.json");
        let Some(m) = read_json(&manifest) else { continue };
        let s = |k: &str| m.get(k).and_then(|v| v.as_str()).map(str::to_string);
        let name = s("name").unwrap_or_else(|| root.file_name().unwrap_or_default().to_string_lossy().into_owned());
        let overrides: Vec<String> = enablement
            .pointer(&format!("/{}/overrides", name.replace('~', "~0").replace('/', "~1")))
            .and_then(|o| o.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let on = overrides.is_empty() || overrides.iter().any(|o| !o.starts_with('!'));
        let source = read_json(&root.join(".gemini-extension-install.json")).and_then(|v| v.get("source")?.as_str().map(str::to_string));
        let market = source
            .as_deref()
            .map(|s| s.trim_start_matches("https://github.com/").trim_end_matches(".git").to_string())
            .unwrap_or_else(|| "local".into());
        let mcp = mcp_count(m.get("mcpServers").cloned().map(|v| serde_json::json!({ "mcpServers": v })));
        out.push(item(Found {
            agent: "gemini_cli",
            name,
            market,
            version: s("version"),
            contents: contents(&root, Some(root.join("hooks/hooks.json")), mcp),
            root,
            manifest,
            on: Some(on),
        }));
    }
}

/// Every non-Claude agent's plugins, sorted by name.
pub fn scan(env: &Env) -> Vec<Item> {
    let mut out = Vec::new();
    codex(env, &mut out);
    cursor(env, &mut out);
    opencode(env, &mut out);
    gemini(env, &mut out);
    out
}
