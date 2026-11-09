//! The secrets pass over agent history kept as plain text files (Claude
//! transcripts are read with the hook runs, in `parse::claude_runs`). A file
//! is read again only when its size or mtime changed.

use super::{hits, Hit};
use crate::env::Env;
use crate::store::FileHits;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Larger files are not history (and would be slow to rewrite).
const MAX_FILE: u64 = 32 << 20;

/// Every history file with its place key.
/// The agent whose history a `text_files` place belongs to.
pub fn place_agent(place: &str) -> &'static str {
    match place {
        "codex" => "codex",
        "gemini" => "gemini_cli",
        _ => "claude_code",
    }
}

pub fn text_files(env: &Env) -> Vec<(PathBuf, &'static str)> {
    let h = &env.home;
    let mut out = Vec::new();
    walk(&h.join(".claude/file-history"), "file_history", &|_| true, &mut out);
    walk(&h.join(".claude/paste-cache"), "paste_cache", &|_| true, &mut out);
    walk(&h.join(".claude/shell-snapshots"), "shell_snapshot", &|_| true, &mut out);
    for (p, place) in [(h.join(".claude/history.jsonl"), "prompt_history"), (h.join(".codex/history.jsonl"), "codex")] {
        if p.is_file() {
            out.push((p, place));
        }
    }
    if let Ok(dirs) = std::fs::read_dir(h.join(".claude/projects")) {
        for d in dirs.flatten() {
            walk(&d.path().join("memory"), "memory", &|p| p.extension().is_some_and(|e| e == "md"), &mut out);
        }
    }
    walk(&h.join(".codex/sessions"), "codex", &|p| p.extension().is_some_and(|e| e == "jsonl"), &mut out);
    if let Ok(dirs) = std::fs::read_dir(h.join(".gemini/tmp")) {
        for d in dirs.flatten() {
            walk(&d.path().join("chats"), "gemini", &|p| p.extension().is_some_and(|e| e == "json" || e == "jsonl"), &mut out);
            let logs = d.path().join("logs.json");
            if logs.is_file() {
                out.push((logs, "gemini"));
            }
        }
    }
    out.sort();
    out
}

fn walk(dir: &Path, place: &'static str, keep: &dyn Fn(&Path) -> bool, out: &mut Vec<(PathBuf, &'static str)>) {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            let p = e.path();
            if ft.is_dir() {
                stack.push(p);
            } else if ft.is_file() && keep(&p) {
                out.push((p, place));
            }
        }
    }
}

fn stamp(m: &std::fs::Metadata) -> (u64, i64) {
    use std::os::unix::fs::MetadataExt;
    (m.len(), m.mtime() * 1_000_000_000 + m.mtime_nsec())
}

/// Hits of one whole file (UTF-8 text only), seen at its mtime.
pub fn file_hits(path: &Path) -> Vec<Hit> {
    let Ok(meta) = std::fs::metadata(path) else { return Vec::new() };
    if meta.len() > MAX_FILE {
        return Vec::new();
    }
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    let seen = meta.modified().ok().map(|t| chrono::DateTime::<chrono::Utc>::from(t).timestamp_millis());
    hits(&text, 0, seen)
}

/// Files that changed since `stamps`, read; and stamped files that are gone.
pub fn read_changed(files: &[(PathBuf, &'static str)], stamps: &HashMap<String, (u64, i64)>) -> (Vec<FileHits>, Vec<String>) {
    let mut changed = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (p, place) in files {
        let key = p.to_string_lossy().into_owned();
        seen.insert(key.clone());
        let Ok(meta) = std::fs::metadata(p) else { continue };
        let (size, mtime) = stamp(&meta);
        if stamps.get(&key) == Some(&(size, mtime)) {
            continue;
        }
        changed.push(FileHits { path: key, place, size, mtime, hits: file_hits(p) });
    }
    let gone = stamps.keys().filter(|k| !seen.contains(*k)).cloned().collect();
    (changed, gone)
}

/// The place key of a history file SkillDesk may rewrite; `None` for any
/// other path.
pub fn place_of(env: &Env, path: &Path) -> Option<&'static str> {
    if path.starts_with(env.home.join(".claude/projects")) && path.extension().is_some_and(|e| e == "jsonl") {
        return Some("transcript");
    }
    text_files(env).into_iter().find(|(p, _)| p == path).map(|(_, place)| place)
}
