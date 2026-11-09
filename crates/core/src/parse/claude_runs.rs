//! Hook runs from Claude transcripts (agent-data.md §5b).
//!
//! Lines `type: attachment` with `attachment.type` in `hook_success`,
//! `hook_non_blocking_error`, `hook_blocking_error`, `hook_cancelled` are one
//! run each, with `hookEvent`, `command`, `exitCode`, `durationMs`. Stop hooks
//! are also listed in `stop_hook_summary`, which would count them twice, so it
//! is not read. Numbers arrive as numbers or strings; both are accepted.

use crate::env::Env;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    Blocked,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy)]
pub struct Run {
    pub ms: u32,
    pub outcome: Outcome,
    /// ms since epoch.
    pub at: Option<i64>,
}

/// (event, command) → runs.
pub type HookRuns = HashMap<(String, String), Vec<Run>>;

/// One MCP server's tool calls: count and last call (ms since epoch).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Calls {
    pub count: u64,
    pub last: Option<i64>,
}

/// Everything one pass over the transcripts collects.
#[derive(Debug, Default)]
pub struct TranscriptStats {
    pub hooks: HookRuns,
    /// Server part of `mcp__<server>__<tool>` (as Claude spells it) → calls.
    pub mcp: HashMap<String, Calls>,
    /// `subagent_type` of each Agent/Task tool call → runs.
    pub subagents: HashMap<String, Calls>,
    /// Slash command typed by the user (without `/`) → runs.
    pub commands: HashMap<String, Calls>,
    /// Marketplace sources seen (restore steps for removed plugins).
    pub markets: super::market_sources::MarketSources,
}

/// Every transcript under `~/.claude/projects`, including subagent files.
pub fn transcript_files(env: &Env) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![env.home.join(".claude/projects")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            let p = e.path();
            if ft.is_dir() {
                stack.push(p);
            } else if ft.is_file() && p.extension().is_some_and(|x| x == "jsonl") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// What one transcript added since `offset` (whole lines only).
#[derive(Debug, Default)]
pub struct FileDelta {
    pub path: String,
    pub size: u64,
    pub mtime: i64,
    /// Byte after the last complete line read.
    pub offset: u64,
    /// The file shrank or was replaced: drop what it gave before.
    pub reset: bool,
    pub hooks: Vec<((String, String), Run)>,
    pub mcp: HashMap<String, Calls>,
    pub subagents: HashMap<String, Calls>,
    pub commands: HashMap<String, Calls>,
    pub markets: super::market_sources::MarketSources,
    /// What this read added to the file's session (main transcripts only).
    pub session: Option<crate::sessions::SessionRow>,
    /// Secret values in the lines read (memory only; stored as hashes).
    pub secrets: Vec<crate::secrets::Hit>,
}

fn mtime_ns(m: &std::fs::Metadata) -> i64 {
    use std::os::unix::fs::MetadataExt;
    m.mtime() * 1_000_000_000 + m.mtime_nsec()
}

/// Reads only what changed since the stamps. Unchanged files are skipped; a
/// file that grew is read from its offset; anything else is read again.
pub fn read_changed(files: &[PathBuf], stamps: &HashMap<String, (u64, i64, u64)>) -> (Vec<FileDelta>, Vec<String>) {
    let mut deltas = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for f in files {
        let key = f.to_string_lossy().into_owned();
        seen.insert(key.clone());
        let Ok(meta) = std::fs::metadata(f) else { continue };
        let (size, mtime) = (meta.len(), mtime_ns(&meta));
        let (start, reset) = match stamps.get(&key) {
            Some(&(s, m, _)) if s == size && m == mtime => continue,
            Some(&(s, _, off)) if size >= s && off <= size => (off, false),
            Some(_) => (0, true),
            None => (0, false),
        };
        let mut d = FileDelta { path: key, size, mtime, offset: start, reset, ..Default::default() };
        let mut stats = TranscriptStats::default();
        let main = f.parent().and_then(|p| p.file_name()).is_some_and(|n| n != "subagents");
        let mut session = main.then(|| crate::sessions::SessionRow {
            native_id: f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            origin: Some(crate::sessions::SessionOrigin::Claude),
            source: d.path.clone(),
            ..Default::default()
        });
        d.offset = read_file(f, start, &mut stats, session.as_mut(), &mut d.secrets);
        d.session = session;
        d.hooks = stats.hooks.into_iter().flat_map(|(k, runs)| runs.into_iter().map(move |r| (k.clone(), r))).collect();
        d.mcp = stats.mcp;
        d.subagents = stats.subagents;
        d.commands = stats.commands;
        d.markets = stats.markets;
        deltas.push(d);
    }
    let gone = stamps.keys().filter(|k| !seen.contains(*k)).cloned().collect();
    (deltas, gone)
}

/// Reads complete lines from `start`; returns the offset after the last one.
fn read_file(
    path: &Path,
    start: u64,
    stats: &mut TranscriptStats,
    mut session: Option<&mut crate::sessions::SessionRow>,
    secrets: &mut Vec<crate::secrets::Hit>,
) -> u64 {
    use std::io::{Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else { return start };
    if file.seek(SeekFrom::Start(start)).is_err() {
        return start;
    }
    let mut pos = start;
    let mut reader = BufReader::with_capacity(1 << 20, file);
    let hook = memchr::memmem::Finder::new(b"\"hook_");
    let mcp = memchr::memmem::Finder::new(b"\"name\":\"mcp__");
    let agent = memchr::memmem::Finder::new(b"\"subagent_type\":\"");
    let command = memchr::memmem::Finder::new(b"\"content\":\"<command-");
    let user = memchr::memmem::Finder::new(b"\"type\":\"user\"");
    let tool_result = memchr::memmem::Finder::new(b"\"tool_use_id\"");
    let custom_title = memchr::memmem::Finder::new(b"\"type\":\"custom-title\"");
    let cwd = memchr::memmem::Finder::new(b"\"cwd\":\"");
    let mut line = Vec::with_capacity(64 * 1024);
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        // A line still being written is read next time.
        if line.last() != Some(&b'\n') {
            break;
        }
        let at = pos;
        pos += line.len() as u64;
        if let Ok(text) = std::str::from_utf8(&line) {
            let found = crate::secrets::hits(text, at as i64, None);
            if !found.is_empty() {
                let seen = line_time(&line);
                secrets.extend(found.into_iter().map(|h| crate::secrets::Hit { seen, ..h }));
            }
        }
        if let Some(row) = session.as_deref_mut() {
            session_line(&line, row, &user, &tool_result, &custom_title, &cwd);
        }
        if hook.find(&line).is_some() {
            if let Some((key, run)) = parse_line(&line) {
                stats.hooks.entry(key).or_default().push(run);
            }
        }
        if mcp.find(&line).is_some() {
            mcp_calls(&line, &mcp, &mut stats.mcp);
        }
        if agent.find(&line).is_some() {
            subagent_runs(&line, &mut stats.subagents);
        }
        if super::market_sources::mentions(&line) {
            super::market_sources::scan_line(&line, line_time(&line), &mut stats.markets);
        }
        if command.find(&line).is_some() {
            if let Some(name) = command_name(&line) {
                let c = stats.commands.entry(name).or_default();
                c.count += 1;
                c.last = c.last.max(line_time(&line));
            }
        }
    }
    pos
}

/// Session facts of one transcript line: span, project, the title the user
/// gave with `/rename` (Claude's own `ai-title` often names the first slash
/// command, so it is not used), real prompts.
fn session_line(
    line: &[u8],
    row: &mut crate::sessions::SessionRow,
    user: &memchr::memmem::Finder,
    tool_result: &memchr::memmem::Finder,
    custom_title: &memchr::memmem::Finder,
    cwd: &memchr::memmem::Finder,
) {
    if let Some(t) = line_time(line) {
        row.started = Some(row.started.map_or(t, |s| s.min(t)));
        row.ended = Some(row.ended.map_or(t, |e| e.max(t)));
    }
    if row.project.is_none() {
        if let Some(i) = cwd.find(line) {
            let rest = &line[i + 7..];
            if let Some(end) = rest.iter().position(|&b| b == b'"') {
                row.project = std::str::from_utf8(&rest[..end]).ok().filter(|s| s.starts_with('/')).map(String::from);
            }
        }
    }
    if custom_title.find(line).is_some() {
        if let Some(t) = serde_json::from_slice::<serde_json::Value>(line).ok().and_then(|v| v.get("customTitle")?.as_str().map(String::from)) {
            row.title = Some(crate::secrets::mask(&t));
        }
    } else if user.find(line).is_some() && tool_result.find(line).is_none() {
        if let Some((text, at)) = crate::sessions::claude_prompt(line) {
            row.push_prompt(&text, at);
        }
    }
}

/// Tool-use blocks naming an MCP tool: `{"type":"tool_use","id":…,"name":"mcp__s__t"`.
/// Only names right after a `tool_use` marker count, not tool lists or prose.
fn mcp_calls(line: &[u8], finder: &memchr::memmem::Finder, out: &mut HashMap<String, Calls>) {
    let tool_use = memchr::memmem::Finder::new(b"\"tool_use\"");
    let mut at = None;
    for start in finder.find_iter(line) {
        let window = &line[start.saturating_sub(120)..start];
        if tool_use.find(window).is_none() {
            continue;
        }
        let rest = &line[start + b"\"name\":\"mcp__".len()..];
        let end = rest.iter().position(|&b| b == b'"').unwrap_or(0);
        let Ok(name) = std::str::from_utf8(&rest[..end]) else { continue };
        let server = match name.find("__") {
            Some(i) => &name[..i],
            None => name,
        };
        if server.is_empty() {
            continue;
        }
        let ts = *at.get_or_insert_with(|| line_time(line));
        let c = out.entry(server.to_string()).or_default();
        c.count += 1;
        c.last = c.last.max(ts);
    }
}

/// Agent or Task tool-use blocks of an assistant line: `input.subagent_type`.
/// Parsed as JSON: the prompt before the type can be any length.
fn subagent_runs(line: &[u8], out: &mut HashMap<String, Calls>) {
    #[derive(serde::Deserialize)]
    struct Block {
        #[serde(rename = "type")]
        kind: Option<String>,
        name: Option<String>,
        input: Option<serde_json::Value>,
    }
    #[derive(serde::Deserialize)]
    struct Msg {
        content: Option<serde_json::Value>,
    }
    #[derive(serde::Deserialize)]
    struct Line {
        #[serde(rename = "type")]
        kind: Option<String>,
        message: Option<Msg>,
    }
    let Ok(l) = serde_json::from_slice::<Line>(line) else { return };
    if l.kind.as_deref() != Some("assistant") {
        return;
    }
    let Some(serde_json::Value::Array(blocks)) = l.message.and_then(|m| m.content) else { return };
    let mut at = None;
    for b in blocks {
        let Ok(b) = serde_json::from_value::<Block>(b) else { continue };
        if b.kind.as_deref() != Some("tool_use") || !matches!(b.name.as_deref(), Some("Agent" | "Task")) {
            continue;
        }
        let Some(name) = b.input.as_ref().and_then(|i| i.get("subagent_type")).and_then(|v| v.as_str()) else { continue };
        if name.is_empty() || name.len() > 120 {
            continue;
        }
        let ts = *at.get_or_insert_with(|| line_time(line));
        let c = out.entry(name.to_string()).or_default();
        c.count += 1;
        c.last = c.last.max(ts);
    }
}

/// A user line whose message is a typed slash command:
/// `"content":"<command-message>…<command-name>/x</command-name>`.
fn command_name(line: &[u8]) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Msg {
        content: serde_json::Value,
    }
    #[derive(serde::Deserialize)]
    struct Line {
        #[serde(rename = "type")]
        kind: Option<String>,
        message: Option<Msg>,
    }
    let l: Line = serde_json::from_slice(line).ok()?;
    if l.kind.as_deref() != Some("user") {
        return None;
    }
    let text = l.message?.content;
    let text = text.as_str()?;
    if !text.starts_with("<command-") {
        return None;
    }
    let open = "<command-name>";
    let i = text.find(open)? + open.len();
    let end = text[i..].find("</command-name>")?;
    let name = text[i..i + end].trim().trim_start_matches('/');
    (!name.is_empty() && name.len() <= 120 && !name.contains(char::is_whitespace)).then(|| name.to_string())
}

fn line_time(line: &[u8]) -> Option<i64> {
    let key = b"\"timestamp\":\"";
    let i = memchr::memmem::rfind(line, key)? + key.len();
    let end = line[i..].iter().position(|&b| b == b'"')?;
    let t = std::str::from_utf8(&line[i..i + end]).ok()?;
    chrono::DateTime::parse_from_rfc3339(t).ok().map(|d| d.timestamp_millis())
}

fn parse_line(line: &[u8]) -> Option<((String, String), Run)> {
    #[derive(serde::Deserialize)]
    struct Attachment {
        #[serde(rename = "type")]
        kind: String,
        #[serde(rename = "hookEvent")]
        event: Option<String>,
        command: Option<String>,
        #[serde(rename = "exitCode")]
        exit_code: Option<serde_json::Value>,
        #[serde(rename = "durationMs")]
        duration_ms: Option<serde_json::Value>,
    }
    #[derive(serde::Deserialize)]
    struct Line {
        #[serde(rename = "type")]
        kind: Option<String>,
        attachment: Option<Attachment>,
        timestamp: Option<String>,
    }
    let l: Line = serde_json::from_slice(line).ok()?;
    if l.kind.as_deref() != Some("attachment") {
        return None;
    }
    let a = l.attachment?;
    let exit = a.exit_code.as_ref().and_then(num);
    let outcome = match a.kind.as_str() {
        "hook_cancelled" => Outcome::Cancelled,
        "hook_blocking_error" => Outcome::Blocked,
        "hook_non_blocking_error" => Outcome::Failed,
        "hook_success" => match exit {
            Some(2) => Outcome::Blocked,
            Some(0) | None => Outcome::Ok,
            Some(_) => Outcome::Failed,
        },
        _ => return None,
    };
    let ms = a.duration_ms.as_ref().and_then(num).unwrap_or(0).clamp(0, u32::MAX as i64) as u32;
    let at = l
        .timestamp
        .as_deref()
        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
        .map(|d| d.timestamp_millis());
    Some(((a.event?, a.command?), Run { ms, outcome, at }))
}

fn num(v: &serde_json::Value) -> Option<i64> {
    match v {
        serde_json::Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// MCP calls in Codex rollouts (`~/.codex/sessions/**/rollout-*.jsonl`):
/// `function_call` items whose name is `mcp__<server>__<tool>`, the same
/// spelling Claude uses, so one server's calls add up across both agents.
pub fn codex_mcp(env: &Env) -> HashMap<String, Calls> {
    let mut out: HashMap<String, Calls> = HashMap::new();
    if crate::adapters::is_off("codex") {
        return out;
    }
    let call = memchr::memmem::Finder::new(b"\"type\":\"function_call\"");
    let name = memchr::memmem::Finder::new(b"\"name\":\"mcp__");
    let mut stack = vec![env.home.join(".codex/sessions")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            if !p.extension().is_some_and(|x| x == "jsonl") {
                continue;
            }
            let Ok(file) = std::fs::File::open(&p) else { continue };
            let mut reader = BufReader::with_capacity(1 << 20, file);
            let mut line = Vec::with_capacity(16 * 1024);
            loop {
                line.clear();
                match reader.read_until(b'\n', &mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                if call.find(&line).is_none() {
                    continue;
                }
                let Some(i) = name.find(&line) else { continue };
                let rest = &line[i + b"\"name\":\"mcp__".len()..];
                let end = rest.iter().position(|&b| b == b'"').unwrap_or(0);
                let Ok(full) = std::str::from_utf8(&rest[..end]) else { continue };
                let server = full.split("__").next().unwrap_or("");
                if server.is_empty() {
                    continue;
                }
                let c = out.entry(server.to_string()).or_default();
                c.count += 1;
                c.last = c.last.max(line_time(&line));
            }
        }
    }
    out
}

/// Claude's calls plus Codex's, per server.
pub fn all_mcp(env: &Env, claude: Option<&HashMap<String, Calls>>) -> HashMap<String, Calls> {
    let mut all = claude.cloned().unwrap_or_default();
    for (k, c) in codex_mcp(env) {
        let e = all.entry(k).or_default();
        e.count += c.count;
        e.last = e.last.max(c.last);
    }
    all
}
