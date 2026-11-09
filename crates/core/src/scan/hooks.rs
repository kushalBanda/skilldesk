//! Hooks inventory (slice 5): config for Claude (global, project, plugin),
//! Codex, Cursor, Gemini, Copilot, OpenCode; run timing from Claude transcripts.
//!
//! Status: `Off` when switched off (Codex state, plugin off, or in SkillDesk's
//! stash), else `Broken` when a script it runs is missing, `Slow` when p95 > 5 s, `Gone` for hooks
//! that ran but are configured nowhere now.

use crate::adapters;
use crate::env::Env;
use crate::model::{
    AgentKey, HookFacts, HookTotals, Item, ItemId, ItemKind, ItemStatus, KindFacts, PluginRef, SourceRef,
    SwitchKind, Timing, Usage,
};
use crate::parse::claude;
use crate::parse::claude_runs::{HookRuns, Outcome, Run};
use crate::parse::hooks::{self, FileSpec, RawHook, TimeoutUnit, Vars};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub const SLOW_P95_MS: u32 = 5_000;

pub struct HookScan {
    pub items: Vec<Item>,
    pub totals: Option<HookTotals>,
}

/// Every hook entry in agent config files with its item id, plus items for
/// files that cannot be parsed. Shared by the scan and by hook actions.
pub fn configured(env: &Env) -> (Vec<(ItemId, RawHook)>, Vec<Item>) {
    let mut raws: Vec<RawHook> = Vec::new();
    let mut unreadable: Vec<Item> = Vec::new();
    let mut seen_files: HashSet<(u64, u64)> = HashSet::new();
    let mut read = |spec: FileSpec, raws: &mut Vec<RawHook>| {
        if let Some(id) = file_id(&spec.path) {
            if !seen_files.insert(id) {
                return;
            }
        }
        match hooks::read_file(&spec) {
            Ok(list) => raws.extend(list),
            Err(e) => unreadable.push(unreadable_item(spec.agent, &spec.path, &e)),
        }
    };
    let on = |key: &str| adapters::find(key).is_some_and(|a| a.is_installed(env));
    let home = &env.home;
    let spec = |agent: &'static str, path: PathBuf, unit| FileSpec { agent, path, unit, plugin: None, project: None, codex_keys: false };

    if on("claude_code") {
        for f in ["settings.json", "settings.local.json"] {
            read(spec("claude_code", home.join(".claude").join(f), TimeoutUnit::Seconds), &mut raws);
        }
        let home_canon = std::fs::canonicalize(home).ok();
        for proj in claude::project_paths(env) {
            if std::fs::canonicalize(&proj).ok() == home_canon {
                continue;
            }
            for f in [".claude/settings.json", ".claude/settings.local.json"] {
                let mut s = spec("claude_code", proj.join(f), TimeoutUnit::Seconds);
                s.project = Some(proj.clone());
                read(s, &mut raws);
            }
            for (agent, f, unit) in [("cursor", ".cursor/hooks.json", TimeoutUnit::Seconds), ("gemini_cli", ".gemini/settings.json", TimeoutUnit::Millis)] {
                if on(agent) {
                    let mut s = spec(agent, proj.join(f), unit);
                    s.project = Some(proj.clone());
                    read(s, &mut raws);
                }
            }
        }
        for p in claude::installed_plugins(env) {
            let mut s = spec("claude_code", p.install_path.join("hooks/hooks.json"), TimeoutUnit::Seconds);
            s.plugin = Some((PluginRef { id: p.id.clone(), enabled: p.enabled }, p.install_path.clone()));
            read(s, &mut raws);
        }
    }
    if on("codex") {
        let mut s = spec("codex", home.join(".codex/hooks.json"), TimeoutUnit::Seconds);
        s.codex_keys = true;
        read(s, &mut raws);
    }
    if on("cursor") {
        read(spec("cursor", home.join(".cursor/hooks.json"), TimeoutUnit::Seconds), &mut raws);
    }
    if on("gemini_cli") {
        read(spec("gemini_cli", home.join(".gemini/settings.json"), TimeoutUnit::Millis), &mut raws);
    }
    if on("github_copilot") {
        let dir = home.join(".copilot/hooks");
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map(|r| r.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect())
            .unwrap_or_default();
        files.sort();
        for f in files {
            read(spec("github_copilot", f, TimeoutUnit::Seconds), &mut raws);
        }
    }
    if on("opencode") {
        raws.extend(hooks::opencode_plugins(env));
    }

    let mut ids: HashMap<String, u32> = HashMap::new();
    let with_ids = raws
        .into_iter()
        .map(|raw| {
            let base = format!("{}\0{}\0{}\0{}", raw.agent, raw.event, raw.matcher.as_deref().unwrap_or(""), raw.command);
            let n = ids.entry(format!("{}\0{base}", raw.file.display())).or_insert(0);
            *n += 1;
            let key_str = if *n == 1 { base } else { format!("{base}\0{n}") };
            (ItemId::derive(ItemKind::Hook, &raw.file, &key_str), raw)
        })
        .collect();
    (with_ids, unreadable)
}

pub fn scan(env: &Env, runs: Option<&HookRuns>) -> HookScan {
    let home = &env.home;
    let (configured, unreadable) = configured(env);
    // Hooks SkillDesk switched off: kept in the stash, shown as Off.
    let stashed = crate::actions::stash::read(env);
    let all: Vec<(ItemId, RawHook, bool)> = configured
        .into_iter()
        .map(|(id, raw)| (id, raw, false))
        .chain(stashed.into_iter().map(|e| {
            let (id, raw) = e.to_raw();
            (id, raw, true)
        }))
        .collect();
    let codex_off = hooks::codex_disabled(env);
    let mut items = Vec::with_capacity(all.len());
    let mut claimed: HashSet<(String, String)> = HashSet::new();
    for (id, raw, stashed) in &all {
        let vars = Vars { home, plugin_root: raw.plugin_root.as_deref(), project: raw.project.as_deref() };
        let a = hooks::analyze(&raw.command, &vars);
        let switch = match (raw.agent, &raw.plugin) {
            (_, Some(_)) => SwitchKind::Plugin,
            ("codex", _) => SwitchKind::Native,
            ("opencode", _) => SwitchKind::None,
            _ => SwitchKind::Stash,
        };
        let off = *stashed
            || raw.native_key.as_ref().is_some_and(|k| codex_off.contains(k))
            || raw.plugin.as_ref().is_some_and(|p| !p.enabled);
        // Runs are Claude's only; several entries may share (event, command):
        // the first one gets the runs so totals are not counted twice.
        let key = (raw.event.clone(), raw.command.clone());
        let timing_runs = (raw.agent == "claude_code" && claimed.insert(key.clone()))
            .then(|| runs.and_then(|r| r.get(&key)))
            .flatten();
        let usage = timing_runs.map(|r| usage_of(r));
        let slow = usage.as_ref().and_then(|u| u.timing.as_ref()).is_some_and(|t| t.p95_ms > SLOW_P95_MS);
        // Off wins: a switched-off hook does not run, missing script or not.
        let status = if off {
            ItemStatus::Off
        } else if !a.missing.is_empty() {
            ItemStatus::Broken
        } else if slow {
            ItemStatus::Slow
        } else {
            ItemStatus::Ok
        };
        let owner = raw
            .plugin
            .as_ref()
            .map(|p| p.id.split('@').next().unwrap_or(&p.id).to_string())
            .or(a.owner)
            .or_else(|| {
                // OpenCode plugin files are named after their tool: `orca-opencode-status.js`.
                (raw.agent == "opencode")
                    .then(|| raw.file.file_stem().map(|s| s.to_string_lossy().split('-').next().unwrap_or("").to_string()))
                    .flatten()
                    .filter(|s| !s.is_empty())
            });
        items.push(Item {
            id: id.clone(),
            kind: ItemKind::Hook,
            name: a.label,
            agents: vec![AgentKey(raw.agent.into())],
            status,
            sources: vec![SourceRef { path: raw.file.clone(), line: None }],
            usage,
            facts: KindFacts::Hook(HookFacts {
                event: raw.event.clone(),
                matcher: raw.matcher.clone(),
                command: raw.command.clone(),
                owner,
                missing_paths: a.missing,
                switch,
                timeout_ms: raw.timeout_ms,
                plugin: raw.plugin.clone(),
                project: raw.project.clone(),
            }),
        });
    }

    // Hooks that ran but are configured nowhere now.
    if let Some(runs) = runs {
        let mut gone: Vec<(&(String, String), &Vec<Run>)> =
            runs.iter().filter(|(k, _)| !claimed.contains(*k)).collect();
        gone.sort_by(|a, b| a.0.cmp(b.0));
        for ((event, command), r) in gone {
            let vars = Vars { home, plugin_root: None, project: None };
            let a = hooks::analyze(command, &vars);
            items.push(Item {
                id: ItemId::derive(ItemKind::Hook, Path::new(""), &format!("gone\0{event}\0{command}")),
                kind: ItemKind::Hook,
                name: a.label,
                agents: vec![AgentKey("claude_code".into())],
                status: ItemStatus::Gone,
                sources: Vec::new(),
                usage: Some(usage_of(r)),
                facts: KindFacts::Hook(HookFacts {
                    event: event.clone(),
                    matcher: None,
                    command: command.clone(),
                    owner: a.owner,
                    missing_paths: Vec::new(),
                    switch: SwitchKind::None,
                    timeout_ms: None,
                    plugin: None,
                    project: None,
                }),
            });
        }
    }
    items.extend(unreadable);

    let totals = runs.map(|runs| {
        let mut t = HookTotals { runs: 0, total_ms: 0, cancelled: 0, failed: 0, blocked: 0 };
        for r in runs.values().flatten() {
            t.runs += 1;
            t.total_ms += r.ms as u64;
            match r.outcome {
                Outcome::Cancelled => t.cancelled += 1,
                Outcome::Failed => t.failed += 1,
                Outcome::Blocked => t.blocked += 1,
                Outcome::Ok => {}
            }
        }
        t
    });
    HookScan { items, totals }
}

fn usage_of(runs: &[Run]) -> Usage {
    let mut ms: Vec<u32> = runs.iter().map(|r| r.ms).collect();
    ms.sort_unstable();
    let pct = |p: f64| ms.get(((ms.len() as f64 - 1.0) * p).round() as usize).copied().unwrap_or(0);
    let count = |o: Outcome| runs.iter().filter(|r| r.outcome == o).count() as u32;
    Usage {
        count: runs.len() as u64,
        last_used: runs
            .iter()
            .filter_map(|r| r.at)
            .max()
            .and_then(chrono::DateTime::from_timestamp_millis)
            .map(|d| d.to_rfc3339()),
        timing: Some(Timing {
            total_ms: ms.iter().map(|&m| m as u64).sum(),
            p50_ms: pct(0.5),
            p95_ms: pct(0.95),
            max_ms: ms.last().copied().unwrap_or(0),
            cancelled: count(Outcome::Cancelled),
            failed: count(Outcome::Failed),
            blocked: count(Outcome::Blocked),
        }),
    }
}

fn unreadable_item(agent: &str, path: &Path, err: &str) -> Item {
    Item {
        id: ItemId::derive(ItemKind::Hook, path, "unreadable"),
        kind: ItemKind::Hook,
        name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        agents: vec![AgentKey(agent.into())],
        status: ItemStatus::Unreadable(format!("Not valid JSON: {err}")),
        sources: vec![SourceRef { path: path.to_path_buf(), line: None }],
        usage: None,
        facts: KindFacts::None,
    }
}

fn file_id(p: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(p).ok().map(|m| (m.dev(), m.ino()))
}
