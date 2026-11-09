//! Claude plugins, marketplaces and plugin cache (agent-data.md §4); other
//! agents' plugins come from `plugins_other` and join the same list.
//!
//! - Plugins: `installed_plugins.json` (v2) + settings `enabledPlugins`; usage
//!   from `~/.claude.json` `pluginUsage` (read only). Used ids that are not
//!   installed are ghosts (`Gone`). `@inline` / `@builtin` ids are Claude's own.
//! - Marketplaces: `known_marketplaces.json`; `Unused` when no installed plugin
//!   comes from it.
//! - Cache: `plugins/cache/<market>/<plugin>/<version>` folders other than the
//!   installed version, and `temp_git_*` leftovers.
//! - Restore steps for ghosts: `/plugin install` alone when the marketplace is
//!   added; else `/plugin marketplace add <source>` first, the source from
//!   settings `extraKnownMarketplaces`, then from transcripts (newest wins).

use crate::env::Env;
use crate::model::{
    AgentKey, CacheFacts, Item, ItemId, ItemKind, ItemStatus, KindFacts, MarketplaceFacts, PluginContents,
    PluginFacts, PluginRestore, SourceOrigin, SourceRef, Usage,
};
use crate::parse::market_sources::{valid_source, MarketSources};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub struct PluginScan {
    pub plugins: Vec<Item>,
    pub marketplaces: Vec<Item>,
    pub cache: Vec<Item>,
}

fn read_json(p: &Path) -> Option<Value> {
    let t = std::fs::read_to_string(p).ok()?;
    serde_json::from_str(&crate::fsx::json_span::strip_comments(&t)).ok()
}

fn claude() -> Vec<AgentKey> {
    vec![AgentKey("claude_code".into())]
}

fn is_internal(id: &str) -> bool {
    id.ends_with("@inline") || id.ends_with("@builtin")
}

/// Total bytes under `p`, not following links.
pub fn dir_size(p: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![p.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(m) = e.path().symlink_metadata() else { continue };
            if m.is_dir() {
                stack.push(e.path());
            } else {
                total += m.len();
            }
        }
    }
    total
}

fn count_entries(dir: &Path, pred: impl Fn(&Path) -> bool) -> u32 {
    std::fs::read_dir(dir).map(|rd| rd.flatten().filter(|e| pred(&e.path())).count() as u32).unwrap_or(0)
}

fn contents(root: &Path) -> PluginContents {
    let md = |p: &Path| p.extension().is_some_and(|x| x == "md");
    let hooks = read_json(&root.join("hooks/hooks.json"))
        .and_then(|v| v.get("hooks").and_then(|h| h.as_object()).map(|o| {
            o.values().filter_map(|g| g.as_array()).map(|a| a.len() as u32).sum()
        }))
        .unwrap_or(0);
    let mcp = read_json(&root.join(".mcp.json"))
        .map(|v| {
            let servers = v.get("mcpServers").unwrap_or(&v);
            servers.as_object().map(|o| o.len() as u32).unwrap_or(0)
        })
        .unwrap_or(0);
    PluginContents {
        skills: count_entries(&root.join("skills"), crate::parse::skills::is_skill_dir),
        commands: count_entries(&root.join("commands"), md),
        agents: count_entries(&root.join("agents"), md),
        hooks,
        mcp_servers: mcp,
    }
}

fn usage(env: &Env) -> BTreeMap<String, Usage> {
    let Some(v) = read_json(&env.home.join(".claude.json")) else { return BTreeMap::new() };
    let Some(pu) = v.get("pluginUsage").and_then(|p| p.as_object()) else { return BTreeMap::new() };
    pu.iter()
        .map(|(id, u)| {
            let count = u.get("usageCount").and_then(|c| c.as_u64()).unwrap_or(0);
            let last = u
                .get("lastUsedAt")
                .and_then(|t| t.as_i64())
                .and_then(chrono::DateTime::from_timestamp_millis)
                .map(|d| d.to_rfc3339());
            (id.clone(), Usage { count, last_used: last, timing: None })
        })
        .collect()
}

/// `owner/repo`, URL or path of a `{"source": {...}}` marketplace entry.
fn source_of(entry: &Value) -> Option<String> {
    let src = entry.get("source")?;
    ["repo", "url", "path"].iter().find_map(|k| src.get(*k).and_then(|v| v.as_str())).map(str::to_string)
}

/// Quoted when it holds a space (a local path), so Claude reads one argument.
fn arg(s: &str) -> String {
    if s.contains(' ') {
        format!("\"{s}\"")
    } else {
        s.to_string()
    }
}

/// The steps that bring back `id` from `market`.
fn restore(
    id: &str,
    market: &str,
    known: &BTreeMap<String, String>,
    settings: &BTreeMap<String, String>,
    history: Option<&MarketSources>,
) -> PluginRestore {
    let install = format!("/plugin install {id}");
    if let Some(s) = known.get(market) {
        return PluginRestore { steps: vec![install], source: Some(s.clone()), origin: Some(SourceOrigin::Added) };
    }
    let found = settings.get(market).map(|s| (s.clone(), SourceOrigin::Settings)).or_else(|| {
        history?
            .iter()
            .filter(|((name, source), _)| name == market && valid_source(source))
            .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0 .1.cmp(&a.0 .1)))
            .map(|((_, source), _)| (source.clone(), SourceOrigin::History))
    });
    match found {
        Some((source, origin)) => PluginRestore {
            steps: vec![format!("/plugin marketplace add {}", arg(&source)), install],
            source: Some(source),
            origin: Some(origin),
        },
        None => PluginRestore { steps: Vec::new(), source: None, origin: None },
    }
}

pub fn scan(env: &Env, history: Option<&MarketSources>) -> PluginScan {
    let plugins_dir = env.home.join(".claude/plugins");
    let settings = read_json(&env.home.join(".claude/settings.json")).unwrap_or(Value::Null);
    let enabled: BTreeMap<String, bool> = settings
        .get("enabledPlugins")
        .and_then(|e| e.as_object())
        .map(|o| o.iter().filter_map(|(k, v)| v.as_bool().map(|b| (k.clone(), b))).collect())
        .unwrap_or_default();
    let usage = usage(env);
    let known_json = read_json(&plugins_dir.join("known_marketplaces.json")).unwrap_or(Value::Null);
    let known_sources: BTreeMap<String, String> = known_json
        .as_object()
        .map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), source_of(v)?))).collect())
        .unwrap_or_default();
    let settings_sources: BTreeMap<String, String> = settings
        .get("extraKnownMarketplaces")
        .and_then(|e| e.as_object())
        .map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), source_of(v)?))).collect())
        .unwrap_or_default();
    let installed_file = plugins_dir.join("installed_plugins.json");
    let installed = read_json(&installed_file).unwrap_or(Value::Null);

    let mut plugins = Vec::new();
    let mut installed_ids: BTreeSet<String> = BTreeSet::new();
    let mut installed_paths: BTreeSet<PathBuf> = BTreeSet::new();
    let mut market_plugins: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if let Some(map) = installed.get("plugins").and_then(|p| p.as_object()) {
        for (id, installs) in map {
            for inst in installs.as_array().into_iter().flatten() {
                let s = |k: &str| inst.get(k).and_then(|v| v.as_str()).map(str::to_string);
                let path = s("installPath").map(PathBuf::from);
                if let Some(p) = &path {
                    installed_paths.insert(p.clone());
                }
                let (name, market) = id.split_once('@').unwrap_or((id, ""));
                market_plugins.entry(market.to_string()).or_default().push(id.clone());
                installed_ids.insert(id.clone());
                let on = enabled.get(id).copied();
                let project = s("projectPath").map(PathBuf::from);
                let status = if path.as_ref().is_some_and(|p| !p.is_dir()) {
                    ItemStatus::Broken
                } else if on == Some(true) || project.is_some() {
                    ItemStatus::Ok
                } else {
                    ItemStatus::Off
                };
                plugins.push(Item {
                    id: ItemId::derive(ItemKind::Plugin, &installed_file, &format!("{id}\0{}", project.as_deref().map(|p| p.display().to_string()).unwrap_or_default())),
                    kind: ItemKind::Plugin,
                    name: name.to_string(),
                    agents: claude(),
                    status,
                    sources: vec![SourceRef { path: installed_file.clone(), line: None }],
                    usage: usage.get(id).cloned(),
                    facts: KindFacts::Plugin(PluginFacts {
                        id: id.clone(),
                        marketplace: market.to_string(),
                        installed: true,
                        enabled: on,
                        version: s("version"),
                        scope: s("scope"),
                        project,
                        contents: path.as_deref().map(contents).unwrap_or_default(),
                        size_bytes: path.as_deref().map(dir_size),
                        install_path: path,
                        installed_at: s("installedAt"),
                        updated_at: s("lastUpdated"),
                        restore: None,
                    }),
                });
            }
        }
    }
    // Ghosts: used before, not installed now.
    for (id, u) in &usage {
        if installed_ids.contains(id) || is_internal(id) || u.count == 0 {
            continue;
        }
        let (name, market) = id.split_once('@').unwrap_or((id, ""));
        plugins.push(Item {
            id: ItemId::derive(ItemKind::Plugin, Path::new(""), &format!("ghost\0{id}")),
            kind: ItemKind::Plugin,
            name: name.to_string(),
            agents: claude(),
            status: ItemStatus::Gone,
            sources: Vec::new(),
            usage: Some(u.clone()),
            facts: KindFacts::Plugin(PluginFacts {
                id: id.clone(),
                marketplace: market.to_string(),
                installed: false,
                enabled: enabled.get(id).copied(),
                version: None,
                scope: None,
                project: None,
                install_path: None,
                installed_at: None,
                updated_at: None,
                contents: PluginContents::default(),
                size_bytes: None,
                restore: Some(restore(id, market, &known_sources, &settings_sources, history)),
            }),
        });
    }

    // Marketplaces.
    let known_file = plugins_dir.join("known_marketplaces.json");
    let extra: BTreeSet<String> = settings
        .get("extraKnownMarketplaces")
        .and_then(|e| e.as_object())
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    let clones = plugins_dir.join("marketplaces");
    let mut marketplaces = Vec::new();
    if let Some(map) = read_json(&known_file).and_then(|v| v.as_object().cloned()) {
        for (name, m) in map {
            let source = source_of(&m).unwrap_or_default();
            let loc = m.get("installLocation").and_then(|v| v.as_str()).map(PathBuf::from);
            let removable = loc.as_ref().is_some_and(|l| l.parent() == Some(clones.as_path()) && l.is_dir());
            let used = market_plugins.get(&name).cloned().unwrap_or_default();
            marketplaces.push(Item {
                id: ItemId::derive(ItemKind::Marketplace, &known_file, &name),
                kind: ItemKind::Marketplace,
                name: name.clone(),
                agents: claude(),
                status: if used.is_empty() { ItemStatus::Unused } else { ItemStatus::Ok },
                sources: vec![SourceRef { path: known_file.clone(), line: None }],
                usage: None,
                facts: KindFacts::Marketplace(MarketplaceFacts {
                    source,
                    size_bytes: removable.then(|| loc.as_deref().map(dir_size)).flatten(),
                    install_location: loc,
                    removable,
                    updated_at: m.get("lastUpdated").and_then(|v| v.as_str()).map(str::to_string),
                    plugins: used,
                    in_settings: extra.contains(&name),
                }),
            });
        }
    }

    // Cache: versions not installed, temp clones.
    let cache_dir = plugins_dir.join("cache");
    let mut cache = Vec::new();
    let mut push_cache = |path: PathBuf, plugin: Option<String>, version: Option<String>, temp: bool| {
        let size = dir_size(&path);
        let name = match (&plugin, &version) {
            (Some(p), Some(v)) => format!("{p} {v}"),
            _ => path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        };
        cache.push(Item {
            id: ItemId::derive(ItemKind::PluginCache, &path, ""),
            kind: ItemKind::PluginCache,
            name,
            agents: claude(),
            status: ItemStatus::Unused,
            sources: vec![SourceRef { path: path.clone(), line: None }],
            usage: None,
            facts: KindFacts::PluginCache(CacheFacts { path, plugin, version, temp, size_bytes: size }),
        });
    };
    for market in std::fs::read_dir(&cache_dir).into_iter().flatten().flatten() {
        let mname = market.file_name().to_string_lossy().into_owned();
        if !market.path().is_dir() {
            continue;
        }
        if mname.starts_with("temp_git_") {
            push_cache(market.path(), None, None, true);
            continue;
        }
        for plugin in std::fs::read_dir(market.path()).into_iter().flatten().flatten() {
            if !plugin.path().is_dir() {
                continue;
            }
            let pid = format!("{}@{mname}", plugin.file_name().to_string_lossy());
            for ver in std::fs::read_dir(plugin.path()).into_iter().flatten().flatten() {
                let p = ver.path();
                if p.is_dir() && !installed_paths.contains(&p) {
                    push_cache(p, Some(pid.clone()), Some(ver.file_name().to_string_lossy().into_owned()), false);
                }
            }
        }
    }
    cache.sort_by(|a, b| a.name.cmp(&b.name));
    plugins.extend(super::plugins_other::scan(env));
    plugins.sort_by(|a, b| a.name.cmp(&b.name));
    PluginScan { plugins, marketplaces, cache }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn restore_installs_alone_when_the_marketplace_is_added() {
        let r = restore("p@m", "m", &map(&[("m", "o/m")]), &BTreeMap::new(), None);
        assert_eq!(r.steps, vec!["/plugin install p@m"]);
        assert_eq!(r.origin, Some(SourceOrigin::Added));
    }

    #[test]
    fn restore_adds_the_marketplace_from_settings_then_history() {
        let mut history = MarketSources::new();
        history.insert(("m".into(), "old/m".into()), Some(1));
        history.insert(("m".into(), "new/m".into()), Some(5));
        history.insert(("other".into(), "x/other".into()), Some(9));
        let r = restore("p@m", "m", &BTreeMap::new(), &BTreeMap::new(), Some(&history));
        assert_eq!(r.steps, vec!["/plugin marketplace add new/m", "/plugin install p@m"]);
        assert_eq!(r.origin, Some(SourceOrigin::History));
        let r = restore("p@m", "m", &BTreeMap::new(), &map(&[("m", "set/m")]), Some(&history));
        assert_eq!(r.source.as_deref(), Some("set/m"));
        assert_eq!(r.origin, Some(SourceOrigin::Settings));
    }

    #[test]
    fn restore_has_no_steps_without_a_source() {
        let r = restore("p@m", "m", &BTreeMap::new(), &BTreeMap::new(), None);
        assert!(r.steps.is_empty());
        assert_eq!(r.source, None);
    }
}
