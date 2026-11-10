//! File watching (ADR 0009): agent folders are watched; a burst of changes
//! collapses into one config rescan; SkillDesk's own writes are muted by path.
//! Project config files are polled (they live all over the disk), and
//! transcripts are read incrementally on a slow timer, never watched.

use crate::adapters;
use crate::core::Core;
use crate::error::{AppError, ErrorKind};
use crate::scan::{self, ProgressSink};
use notify::{RecursiveMode, Watcher as _};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

const QUIET: Duration = Duration::from_millis(800);
const MAX_WAIT: Duration = Duration::from_secs(5);
/// `~/.claude.json` changes on nearly every Claude prompt; it only feeds usage
/// counts, so changes to it alone rescan at most this often.
const CLAUDE_JSON_EVERY: Duration = Duration::from_secs(60);
const PROJECT_POLL: Duration = Duration::from_secs(30);
const TRANSCRIPTS_EVERY: Duration = Duration::from_secs(300);
pub const MUTE_FOR: Duration = Duration::from_millis(1500);

const CONFIG_NAMES: &[&str] = &[
    "settings.json", "settings.local.json", "hooks.json", "mcp.json", "mcp-config.json", "config.json",
    "config.toml", "opencode.json", "opencode.jsonc", "tui.json", "installed_plugins.json", "known_marketplaces.json",
    ".claude.json", ".mcp.json", "SKILL.md", "skill.md",
];
const CONTENT_DIRS: &[&str] = &["skills", "skill", "plugins", "hooks", "agents", "commands"];
/// Busy folders that never hold config: transcripts, caches, editor state.
const IGNORED: &[&str] = &[
    ".git", "node_modules", "projects", "file-history", "todos", "shell-snapshots", "statsig", "debug",
    "session-env", "logs", "paste-cache", "telemetry", "usage-data", "ide", "chats", "extensions", "sessions",
];

/// Whether a changed path can change the inventory.
pub fn relevant(p: &Path) -> bool {
    let names: Vec<String> = p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    // Claude memories live inside the transcripts folder: `projects/<p>/memory/<f>.md`.
    let n = names.len();
    if n >= 4 && names[n - 4] == "projects" && names[n - 2] == "memory" && names[n - 1].ends_with(".md") {
        return true;
    }
    if names.iter().any(|n| IGNORED.contains(&n.as_str())) {
        return false;
    }
    let file = names.last().map(String::as_str).unwrap_or("");
    if file.ends_with(".skilldesk-tmp") {
        return false;
    }
    CONFIG_NAMES.contains(&file) || names.iter().any(|n| CONTENT_DIRS.contains(&n.as_str()))
}

pub struct Watcher {
    _inner: notify::RecommendedWatcher,
    stop: Arc<AtomicBool>,
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn roots(core: &Core) -> Vec<(PathBuf, RecursiveMode)> {
    let env = &core.env;
    let mut out: Vec<(PathBuf, RecursiveMode)> = vec![(env.home.clone(), RecursiveMode::NonRecursive)];
    for a in adapters::detected(env) {
        for dir in std::iter::once(env.resolve(a.detect_dir)).chain(a.skill_dirs(env)) {
            let covered = out.iter().any(|(r, m)| *m == RecursiveMode::Recursive && dir.starts_with(r));
            if dir.is_dir() && !covered {
                out.push((dir, RecursiveMode::Recursive));
            }
        }
    }
    out
}

/// (len, mtime) of each project-level config file the scans read.
fn project_stamps(core: &Core) -> HashMap<PathBuf, (u64, i64)> {
    use std::os::unix::fs::MetadataExt;
    let mut files: Vec<PathBuf> = Vec::new();
    for p in crate::parse::claude::project_paths(&core.env) {
        for f in [".claude/settings.json", ".claude/settings.local.json", ".mcp.json", ".cursor/hooks.json", ".cursor/mcp.json", ".gemini/settings.json"] {
            files.push(p.join(f));
        }
    }
    files
        .into_iter()
        .filter_map(|f| std::fs::metadata(&f).ok().map(|m| (f, (m.len(), m.mtime() * 1_000_000_000 + m.mtime_nsec()))))
        .collect()
}

pub fn start(core: Arc<Core>, sink: Arc<dyn ProgressSink>) -> Result<Watcher, AppError> {
    let (tx, rx) = mpsc::channel::<PathBuf>();
    let mute_core = core.clone();
    let mut inner = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        if matches!(ev.kind, notify::EventKind::Access(_)) {
            return;
        }
        for p in ev.paths {
            if relevant(&p) && !mute_core.is_muted(&p) {
                let _ = tx.send(p);
            }
        }
    })
    .map_err(|e| AppError::new(ErrorKind::Io, format!("Cannot watch files: {e}")))?;
    for (root, mode) in roots(&core) {
        if let Err(e) = inner.watch(&root, mode) {
            eprintln!("watch {} failed: {e}", root.display());
        }
    }
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    std::thread::Builder::new()
        .name("skilldesk-watch".into())
        .spawn(move || worker(core, sink, rx, stop2))
        .map_err(|e| AppError::internal(e.to_string()))?;
    Ok(Watcher { _inner: inner, stop })
}

fn worker(core: Arc<Core>, sink: Arc<dyn ProgressSink>, rx: mpsc::Receiver<PathBuf>, stop: Arc<AtomicBool>) {
    let mut first: Option<Instant> = None;
    let mut last_event = Instant::now();
    let mut only_claude_json = true;
    let mut last_claude_json_scan = Instant::now() - CLAUDE_JSON_EVERY;
    let mut last_poll = Instant::now();
    let mut last_transcripts = Instant::now();
    let mut stamps = project_stamps(&core);
    while !stop.load(Ordering::SeqCst) {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(p) => {
                first.get_or_insert_with(Instant::now);
                last_event = Instant::now();
                only_claude_json &= p.file_name().is_some_and(|n| n == ".claude.json");
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let now = Instant::now();
        let mut rescan = false;
        if let Some(f) = first {
            let settled = now - last_event >= QUIET || now - f >= MAX_WAIT;
            if settled && (!only_claude_json || now - last_claude_json_scan >= CLAUDE_JSON_EVERY) {
                rescan = true;
                first = None;
                only_claude_json = true;
            }
        }
        if now - last_poll >= PROJECT_POLL {
            last_poll = now;
            let fresh = project_stamps(&core);
            if fresh != stamps {
                stamps = fresh;
                rescan = true;
            }
        }
        if now - last_transcripts >= TRANSCRIPTS_EVERY {
            last_transcripts = now;
            last_claude_json_scan = now;
            // Incremental: reads only appended lines. Busy (a scan runs) is fine.
            let _ = scan::run_scan(&core, sink.as_ref());
            continue;
        }
        if rescan {
            last_claude_json_scan = now;
            if let Err(e) = scan::rescan_config(&core, sink.as_ref()) {
                eprintln!("rescan after file change failed: {e}");
            }
        }
    }
}
