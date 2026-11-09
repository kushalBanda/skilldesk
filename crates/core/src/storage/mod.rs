//! Disk use of every agent's folders, by category, and the cleanup
//! candidates: things the agents make again or no longer read. Measured at
//! the end of a full scan and after each cleanup; kept as one JSON snapshot.
//! Sizes are allocated blocks (as Finder and `du` count them). Links are
//! never followed.

use crate::actions::ActionRequest;
use crate::adapters;
use crate::env::Env;
use crate::model::{AgentKey, Item, ItemStatus, KindFacts};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use ts_rs::TS;

/// Claude transcripts older than this are a candidate.
pub const OLD_DAYS: i64 = 90;
/// The reclaimable-disk rule shows a card from this many bytes.
pub const RULE_MIN_BYTES: u64 = 200 << 20;
/// Paths listed per candidate (the plan rebuilds the full list).
const SHOWN_PATHS: usize = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum StorageCategory {
    History,
    Databases,
    Extensions,
    AppFiles,
    Caches,
    Logs,
    Backups,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CategoryBytes {
    pub category: StorageCategory,
    #[ts(type = "number")]
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AgentStorage {
    pub agent: AgentKey,
    pub name: String,
    #[ts(type = "number")]
    pub bytes: u64,
    /// Largest first.
    pub categories: Vec<CategoryBytes>,
    #[ts(type = "string[]")]
    pub roots: Vec<PathBuf>,
}

/// What a candidate is; the UI words each one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum CandidateKind {
    /// Cursor's copy of its chat database (`state.vscdb.backup`).
    CursorBackup,
    /// Finished Claude background jobs (`~/.claude/jobs/<id>`).
    ClaudeJobs,
    /// Claude transcript folders of projects that are no longer on disk.
    OrphanProjects,
    /// Claude transcripts not written for `OLD_DAYS` days.
    OldTranscripts,
    /// Codex's plugin download area (`~/.codex/.tmp`).
    CodexTmp,
    /// Cursor's logs of earlier app sessions.
    CursorLogs,
    /// Marketplaces that supply nothing in use (Plugins action).
    Marketplaces,
    /// Old plugin versions in Claude's cache (Plugins action).
    PluginCache,
}

impl CandidateKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            CandidateKind::CursorBackup => "cursor_backup",
            CandidateKind::ClaudeJobs => "claude_jobs",
            CandidateKind::OrphanProjects => "orphan_projects",
            CandidateKind::OldTranscripts => "old_transcripts",
            CandidateKind::CodexTmp => "codex_tmp",
            CandidateKind::CursorLogs => "cursor_logs",
            CandidateKind::Marketplaces => "marketplaces",
            CandidateKind::PluginCache => "plugin_cache",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        TRASH_KINDS.iter().copied().find(|k| k.as_str() == s)
    }

    /// Candidates whose files hold history the User may still want.
    pub fn holds_history(&self) -> bool {
        matches!(self, CandidateKind::OrphanProjects | CandidateKind::OldTranscripts)
    }
}

/// Candidates moved to the Trash by `StorageTrash` (the others use the
/// Plugins actions).
pub const TRASH_KINDS: &[CandidateKind] = &[
    CandidateKind::CursorBackup,
    CandidateKind::ClaudeJobs,
    CandidateKind::OrphanProjects,
    CandidateKind::OldTranscripts,
    CandidateKind::CodexTmp,
    CandidateKind::CursorLogs,
];

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Candidate {
    pub kind: CandidateKind,
    pub agent: AgentKey,
    #[ts(type = "number")]
    pub bytes: u64,
    /// Up to 30 of the paths that go to the Trash.
    #[ts(type = "string[]")]
    pub paths: Vec<PathBuf>,
    #[ts(type = "number")]
    pub path_count: usize,
    /// Newest change among the paths (ms).
    #[ts(type = "number | null")]
    pub newest: Option<i64>,
    pub request: ActionRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StorageReport {
    #[ts(type = "number")]
    pub total_bytes: u64,
    /// Largest first.
    pub agents: Vec<AgentStorage>,
    /// Largest first.
    pub candidates: Vec<Candidate>,
    #[ts(type = "number")]
    pub reclaimable_bytes: u64,
    /// What sits in `~/.Trash` now (space comes back only when it is emptied).
    #[ts(type = "number | null")]
    pub trash_bytes: Option<u64>,
    pub measured_at: String,
    #[ts(type = "number")]
    pub measure_ms: u64,
}

/// Every folder (or file) that belongs to an agent, relative to home.
/// The six full agents first, so a folder nested in another agent's root
/// (Antigravity in `~/.gemini`) is counted once, by the outer agent.
fn roots_of(key: &str, detect_dir: &str) -> Vec<String> {
    match key {
        "claude_code" => vec![".claude".into(), ".claude.json".into()],
        "cursor" => vec![".cursor".into(), "Library/Application Support/Cursor".into()],
        "opencode" => vec![".config/opencode".into(), ".local/share/opencode".into()],
        "pi" => vec![".pi".into()],
        "agents_shared" => vec![".agents".into()],
        _ => vec![detect_dir.to_string()],
    }
}

/// (relative path below the root, category), longest match wins. A pattern
/// starting with `*` matches a name suffix at any depth up to 3.
fn rules(key: &str, root: &str) -> &'static [(&'static str, StorageCategory)] {
    use StorageCategory::*;
    match (key, root) {
        ("claude_code", ".claude") => &[
            ("projects", History),
            ("file-history", History),
            ("history.jsonl", History),
            ("paste-cache", History),
            ("shell-snapshots", History),
            ("todos", History),
            ("tasks", History),
            ("session-env", History),
            ("plugins", Extensions),
            ("skills", Extensions),
            ("agents", Extensions),
            ("commands", Extensions),
            ("cache", Caches),
            ("statsig", Caches),
            ("telemetry", Logs),
            ("debug", Logs),
            ("backups", Backups),
        ],
        ("cursor", ".cursor") => &[
            ("extensions", Extensions),
            ("plugins", Extensions),
            ("skills", Extensions),
            ("projects", History),
            ("chats", History),
            ("ai-tracking", History),
        ],
        ("cursor", _) => &[
            ("User/globalStorage/state.vscdb.backup", Backups),
            ("User/globalStorage", Databases),
            ("User/workspaceStorage", Databases),
            ("User/History", History),
            ("logs", Logs),
            ("CachedData", Caches),
            ("CachedExtensionVSIXs", Caches),
            ("CachedProfilesData", Caches),
            ("GPUCache", Caches),
            ("Cache", Caches),
            ("Code Cache", Caches),
            ("DawnGraphiteCache", Caches),
            ("DawnWebGPUCache", Caches),
            ("WebStorage", Caches),
            ("Service Worker", Caches),
            ("Partitions", Caches),
        ],
        ("codex", _) => &[
            ("sessions", History),
            ("archived_sessions", History),
            ("history.jsonl", History),
            ("shell_snapshots", History),
            ("packages", AppFiles),
            ("plugins", Extensions),
            ("skills", Extensions),
            ("cache", Caches),
            (".tmp", Caches),
            ("tmp", Caches),
            ("log", Logs),
            ("*.sqlite", Databases),
            ("*.sqlite-wal", Databases),
            ("*.sqlite-shm", Databases),
        ],
        ("opencode", ".local/share/opencode") => &[
            ("storage", History),
            ("snapshot", History),
            ("*.db", Databases),
            ("*.db-wal", Databases),
            ("*.db-shm", Databases),
            ("log", Logs),
            ("bin", AppFiles),
        ],
        ("opencode", _) => &[("node_modules", Extensions), ("skills", Extensions), ("skill", Extensions)],
        ("gemini_cli", _) => &[("tmp", History), ("extensions", Extensions), ("skills", Extensions)],
        ("github_copilot", _) => &[("session-state", History), ("logs", Logs), ("pkg", AppFiles), ("skills", Extensions)],
        _ => &[("skills", Extensions)],
    }
}

fn category_of(rules: &[(&str, StorageCategory)], rel: &str, name: &str) -> Option<StorageCategory> {
    let mut best: Option<(usize, StorageCategory)> = None;
    for (pat, cat) in rules {
        let hit = match pat.strip_prefix('*') {
            Some(suffix) => name.ends_with(suffix),
            None => rel == *pat,
        };
        if hit && best.is_none_or(|(n, _)| pat.len() > n) {
            best = Some((pat.len(), *cat));
        }
    }
    best.map(|(_, c)| c)
}

fn alloc(m: &std::fs::Metadata) -> u64 {
    m.blocks() * 512
}

/// Allocated bytes of a file or folder tree (links counted as links).
pub fn size_of(path: &Path) -> u64 {
    let Ok(m) = std::fs::symlink_metadata(path) else { return 0 };
    let mut total = alloc(&m);
    if !m.is_dir() {
        return total;
    }
    let mut stack = vec![path.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(m) = e.metadata() else { continue };
            total += alloc(&m);
            if m.is_dir() {
                stack.push(e.path());
            }
        }
    }
    total
}

/// Newest mtime in a tree (ms), stopping at 20,000 entries.
fn newest_of(path: &Path) -> Option<i64> {
    let ms = |m: &std::fs::Metadata| m.mtime() * 1000 + m.mtime_nsec() / 1_000_000;
    let m = std::fs::symlink_metadata(path).ok()?;
    let mut newest = ms(&m);
    if m.is_dir() {
        let mut stack = vec![path.to_path_buf()];
        let mut seen = 0;
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                seen += 1;
                if seen > 20_000 {
                    return Some(newest);
                }
                let Ok(m) = e.metadata() else { continue };
                newest = newest.max(ms(&m));
                if m.is_dir() {
                    stack.push(e.path());
                }
            }
        }
    }
    Some(newest)
}

struct Walk<'a> {
    rules: &'a [(&'static str, StorageCategory)],
    sums: std::collections::BTreeMap<StorageCategory, u64>,
    skip: &'a HashSet<PathBuf>,
}

impl Walk<'_> {
    fn entry(&mut self, path: &Path, rel: &str, depth: usize, cat: StorageCategory) {
        if self.skip.contains(path) {
            return;
        }
        let Ok(m) = std::fs::symlink_metadata(path) else { return };
        let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
        let cat = if depth <= 3 { category_of(self.rules, rel, &name).unwrap_or(cat) } else { cat };
        *self.sums.entry(cat).or_default() += alloc(&m);
        if !m.is_dir() {
            return;
        }
        let Ok(rd) = std::fs::read_dir(path) else { return };
        for e in rd.flatten() {
            let child = e.path();
            if depth >= 3 {
                // Below the rule depth: no names to match, so no strings.
                *self.sums.entry(cat).or_default() += size_of(&child);
                continue;
            }
            let n = e.file_name().to_string_lossy().into_owned();
            let rel = if rel.is_empty() { n } else { format!("{rel}/{n}") };
            self.entry(&child, &rel, depth + 1, cat);
        }
    }
}

/// (agent index, rules, top-level path, its name, nested roots to skip).
type Job = (usize, &'static [(&'static str, StorageCategory)], PathBuf, String, HashSet<PathBuf>);

/// Sizes of every detected agent's folders.
pub fn agents(env: &Env) -> Vec<AgentStorage> {
    let mut claimed: Vec<PathBuf> = Vec::new();
    let mut plans: Vec<(&'static adapters::AgentAdapter, Vec<(String, PathBuf)>)> = Vec::new();
    for a in adapters::registry() {
        let mut mine = Vec::new();
        for rel in roots_of(a.key, a.detect_dir) {
            let p = if rel.starts_with(".config/") { env.resolve(&rel) } else { env.home.join(&rel) };
            if std::fs::symlink_metadata(&p).is_err() || claimed.iter().any(|c| p.starts_with(c)) {
                continue;
            }
            claimed.push(p.clone());
            mine.push((rel, p));
        }
        if !mine.is_empty() {
            plans.push((a, mine));
        }
    }
    // A root that holds a later-claimed root (none today) leaves it out.
    // One job per top-level entry of each root, walked on a few threads
    // (most of the time is spent waiting on the disk).
    let all: Vec<PathBuf> = claimed.clone();
    let mut jobs: Vec<Job> = Vec::new();
    let mut own: Vec<u64> = vec![0; plans.len()];
    for (i, (a, mine)) in plans.iter().enumerate() {
        for (rel, p) in mine {
            let skip: HashSet<PathBuf> = all.iter().filter(|c| c.starts_with(p) && *c != p).cloned().collect();
            let r = rules(a.key, rel);
            let Ok(m) = std::fs::symlink_metadata(p) else { continue };
            own[i] += alloc(&m);
            if !m.is_dir() {
                continue;
            }
            for c in children(p) {
                let name = c.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                jobs.push((i, r, c, name, skip.clone()));
            }
        }
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: std::sync::Mutex<Vec<std::collections::BTreeMap<StorageCategory, u64>>> = std::sync::Mutex::new(vec![Default::default(); plans.len()]);
    std::thread::scope(|s| {
        for _ in 0..8 {
            s.spawn(|| loop {
                let n = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some((i, r, path, name, skip)) = jobs.get(n) else { break };
                let mut w = Walk { rules: r, sums: Default::default(), skip };
                w.entry(path, name, 1, StorageCategory::Other);
                let mut all = results.lock().unwrap_or_else(|p| p.into_inner());
                for (c, b) in w.sums {
                    *all[*i].entry(c).or_insert(0) += b;
                }
            });
        }
    });
    let results = results.into_inner().unwrap_or_else(|p| p.into_inner());
    let mut out = Vec::new();
    for (i, ((a, mine), mut total)) in plans.into_iter().zip(results).enumerate() {
        *total.entry(StorageCategory::Other).or_insert(0) += own[i];
        let mut categories: Vec<CategoryBytes> = total.into_iter().filter(|(_, b)| *b > 0).map(|(category, bytes)| CategoryBytes { category, bytes }).collect();
        categories.sort_by_key(|c| std::cmp::Reverse(c.bytes));
        let bytes = categories.iter().map(|c| c.bytes).sum();
        out.push(AgentStorage { agent: a.agent_key(), name: a.display_name.to_string(), bytes, categories, roots: mine.into_iter().map(|(_, p)| p).collect() });
    }
    out.sort_by_key(|c| std::cmp::Reverse(c.bytes));
    out
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn mtime_ms(p: &Path) -> Option<i64> {
    let m = std::fs::symlink_metadata(p).ok()?;
    Some(m.mtime() * 1000 + m.mtime_nsec() / 1_000_000)
}

fn children(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    v.sort();
    v
}

/// The working folder a Claude project folder's transcripts name.
fn project_cwd(dir: &Path) -> Option<PathBuf> {
    use std::io::BufRead;
    for f in children(dir).into_iter().filter(|p| p.extension().is_some_and(|e| e == "jsonl")).take(5) {
        let Ok(file) = std::fs::File::open(&f) else { continue };
        for line in std::io::BufReader::new(file).lines().take(50).map_while(Result::ok) {
            if !line.contains("\"cwd\"") {
                continue;
            }
            if let Some(cwd) = serde_json::from_str::<serde_json::Value>(&line).ok().and_then(|v| v.get("cwd").and_then(|c| c.as_str()).map(PathBuf::from)) {
                return Some(cwd);
            }
        }
    }
    None
}

/// Claude project folders whose project is no longer on disk.
fn orphan_projects(env: &Env) -> Vec<PathBuf> {
    children(&env.home.join(".claude/projects"))
        .into_iter()
        .filter(|d| d.is_dir())
        .filter(|d| project_cwd(d).is_some_and(|cwd| !cwd.exists()))
        .collect()
}

/// The paths of one Trash candidate, read from disk now. Only places inside
/// the agents' own folders; never a live database.
pub fn trash_paths(env: &Env, kind: CandidateKind) -> Vec<PathBuf> {
    let h = &env.home;
    match kind {
        CandidateKind::CursorBackup => {
            let p = h.join("Library/Application Support/Cursor/User/globalStorage/state.vscdb.backup");
            if p.is_file() {
                vec![p]
            } else {
                vec![]
            }
        }
        CandidateKind::ClaudeJobs => children(&h.join(".claude/jobs"))
            .into_iter()
            .filter(|d| {
                let state = std::fs::read_to_string(d.join("state.json"))
                    .ok()
                    .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                    .and_then(|v| v.get("state").and_then(|s| s.as_str()).map(str::to_string));
                matches!(state.as_deref(), Some("done" | "failed" | "error" | "cancelled" | "canceled" | "stopped"))
            })
            .collect(),
        CandidateKind::OrphanProjects => orphan_projects(env),
        CandidateKind::OldTranscripts => {
            let orphans: HashSet<PathBuf> = orphan_projects(env).into_iter().collect();
            let cutoff = now_ms() - OLD_DAYS * 86_400_000;
            let mut out = Vec::new();
            for d in children(&h.join(".claude/projects")) {
                if !d.is_dir() || orphans.contains(&d) {
                    continue;
                }
                for f in children(&d) {
                    if f.extension().is_none_or(|e| e != "jsonl") || mtime_ms(&f).is_none_or(|t| t >= cutoff) {
                        continue;
                    }
                    // The session's own folder (subagents, tool results) goes with it.
                    let side = f.with_extension("");
                    let side_old = side.is_dir() && newest_of(&side).is_some_and(|t| t < cutoff);
                    out.push(f);
                    if side_old {
                        out.push(side);
                    }
                }
            }
            out
        }
        CandidateKind::CodexTmp => children(&h.join(".codex/.tmp"))
            .into_iter()
            .filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().ends_with(".lock")))
            .collect(),
        CandidateKind::CursorLogs => {
            // Session folders are named by start time; the newest is in use.
            let mut logs: Vec<PathBuf> = children(&h.join("Library/Application Support/Cursor/logs")).into_iter().filter(|p| p.is_dir()).collect();
            logs.pop();
            logs
        }
        CandidateKind::Marketplaces | CandidateKind::PluginCache => vec![],
    }
}

fn agent_of(kind: CandidateKind) -> AgentKey {
    AgentKey(
        match kind {
            CandidateKind::CursorBackup | CandidateKind::CursorLogs => "cursor",
            CandidateKind::CodexTmp => "codex",
            _ => "claude_code",
        }
        .into(),
    )
}

fn candidate(kind: CandidateKind, paths: Vec<PathBuf>, bytes: u64, request: ActionRequest) -> Option<Candidate> {
    if paths.is_empty() || bytes == 0 {
        return None;
    }
    let newest = paths.iter().filter_map(|p| newest_of(p)).max();
    Some(Candidate { kind, agent: agent_of(kind), bytes, path_count: paths.len(), paths: paths.into_iter().take(SHOWN_PATHS).collect(), newest, request })
}

/// Every candidate, largest first. Marketplaces and old plugin versions
/// come from the inventory and use the Plugins actions.
pub fn candidates(env: &Env, marketplaces: &[Item], cache: &[Item]) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = std::thread::scope(|s| {
        let handles: Vec<_> = TRASH_KINDS
            .iter()
            .map(|&k| {
                s.spawn(move || {
                    let paths = trash_paths(env, k);
                    let bytes = paths.iter().map(|p| size_of(p)).sum();
                    candidate(k, paths, bytes, ActionRequest::StorageTrash { kinds: vec![k] })
                })
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok().flatten()).collect()
    });
    let unused: Vec<&Item> = marketplaces.iter().filter(|m| m.status == ItemStatus::Unused).collect();
    let mk: Vec<(PathBuf, u64)> = unused
        .iter()
        .filter_map(|m| match &m.facts {
            KindFacts::Marketplace(f) if f.removable => f.install_location.clone().map(|p| (p, f.size_bytes.unwrap_or(0))),
            _ => None,
        })
        .collect();
    let bytes = mk.iter().map(|(_, b)| b).sum();
    out.extend(candidate(
        CandidateKind::Marketplaces,
        mk.into_iter().map(|(p, _)| p).collect(),
        bytes,
        ActionRequest::MarketplaceRemove { ids: unused.iter().map(|m| m.id.clone()).collect() },
    ));
    let old: Vec<(&Item, PathBuf, u64)> = cache
        .iter()
        .filter_map(|c| match &c.facts {
            KindFacts::PluginCache(f) => Some((c, f.path.clone(), f.size_bytes)),
            _ => None,
        })
        .collect();
    let bytes = old.iter().map(|(_, _, b)| b).sum();
    out.extend(candidate(
        CandidateKind::PluginCache,
        old.iter().map(|(_, p, _)| p.clone()).collect(),
        bytes,
        ActionRequest::CacheClean { ids: old.iter().map(|(c, _, _)| c.id.clone()).collect() },
    ));
    out.sort_by_key(|c| std::cmp::Reverse(c.bytes));
    out
}

/// The whole report: agent sizes, candidates and the Trash.
pub fn measure(env: &Env, marketplaces: &[Item], cache: &[Item]) -> StorageReport {
    let started = std::time::Instant::now();
    let (agents, candidates) = std::thread::scope(|s| {
        let c = s.spawn(|| candidates(env, marketplaces, cache));
        let a = agents(env);
        (a, c.join().unwrap_or_default())
    });
    let trash = env.home.join(".Trash");
    let trash_bytes = std::fs::read_dir(&trash).ok().map(|_| size_of(&trash));
    StorageReport {
        total_bytes: agents.iter().map(|a| a.bytes).sum(),
        reclaimable_bytes: candidates.iter().map(|c| c.bytes).sum(),
        agents,
        candidates,
        trash_bytes,
        measured_at: chrono::Utc::now().to_rfc3339(),
        measure_ms: started.elapsed().as_millis() as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_live_database_is_never_a_candidate() {
        let t = tempfile::tempdir().unwrap();
        let env = Env::for_test(&t.path().join("home"), &t.path().join("data"));
        let g = env.home.join("Library/Application Support/Cursor/User/globalStorage");
        std::fs::create_dir_all(&g).unwrap();
        for f in ["state.vscdb", "state.vscdb-wal", "state.vscdb-shm", "state.vscdb.backup"] {
            std::fs::write(g.join(f), b"x").unwrap();
        }
        let all: Vec<PathBuf> = TRASH_KINDS.iter().flat_map(|&k| trash_paths(&env, k)).collect();
        assert_eq!(all, vec![g.join("state.vscdb.backup")]);
    }
}
