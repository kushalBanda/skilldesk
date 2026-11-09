//! Sessions of every agent (agent-data.md §8): list, search, resume.
//!
//! Claude sessions come from the incremental transcript read (stage
//! `Transcripts`); the other agents are read in stage `Deep`, their databases
//! opened read-only and immutable (ADR 0011). Prompt text is masked
//! ([`crate::secrets::mask`]) before it is stored.

pub mod deep;

use crate::core::Core;
use crate::error::{AppError, ErrorKind};
use crate::model::AgentKey;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ts_rs::TS;

/// One prompt is cut here before it is indexed.
pub const PROMPT_CAP: usize = 4000;
/// Shown title length.
const TITLE_CAP: usize = 90;

/// Where a session lives. `cursor_editor` chats have no resume command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SessionOrigin {
    Claude,
    Codex,
    Opencode,
    CursorEditor,
    CursorCli,
    Gemini,
}

impl SessionOrigin {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionOrigin::Claude => "claude",
            SessionOrigin::Codex => "codex",
            SessionOrigin::Opencode => "opencode",
            SessionOrigin::CursorEditor => "cursor_editor",
            SessionOrigin::CursorCli => "cursor_cli",
            SessionOrigin::Gemini => "gemini",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "claude" => SessionOrigin::Claude,
            "codex" => SessionOrigin::Codex,
            "opencode" => SessionOrigin::Opencode,
            "cursor_editor" => SessionOrigin::CursorEditor,
            "cursor_cli" => SessionOrigin::CursorCli,
            "gemini" => SessionOrigin::Gemini,
            _ => return None,
        })
    }

    pub fn agent(&self) -> &'static str {
        match self {
            SessionOrigin::Claude => "claude_code",
            SessionOrigin::Codex => "codex",
            SessionOrigin::Opencode => "opencode",
            SessionOrigin::CursorEditor | SessionOrigin::CursorCli => "cursor",
            SessionOrigin::Gemini => "gemini_cli",
        }
    }
}

/// A session as stored (masked text only).
#[derive(Debug, Clone, Default)]
pub struct SessionRow {
    pub native_id: String,
    pub origin: Option<SessionOrigin>,
    /// Transcript file or agent database.
    pub source: String,
    /// The agent's own name for the chat, if any.
    pub title: Option<String>,
    pub first_prompt: Option<String>,
    pub project: Option<String>,
    /// ms since epoch.
    pub started: Option<i64>,
    pub ended: Option<i64>,
    pub prompts: u32,
    /// Masked prompt text for the search index.
    pub text: String,
    /// Secret values in the prompts (memory only; stored as hashes).
    pub found: Vec<crate::secrets::Hit>,
    /// Prompts per UTC quarter hour (`ms / QUARTER_MS`), for the Dashboard.
    pub quarters: std::collections::BTreeMap<i64, u32>,
}

/// Activity bucket. A quarter hour fits every time zone offset.
pub const QUARTER_MS: i64 = 900_000;

impl SessionRow {
    pub fn id(&self) -> String {
        format!("{}:{}", self.origin.map(|o| o.as_str()).unwrap_or(""), self.native_id)
    }

    /// Adds one real prompt (masks it, caps it).
    pub fn push_prompt(&mut self, raw: &str, at: Option<i64>) {
        let p = clip(raw.trim(), PROMPT_CAP);
        if p.is_empty() {
            return;
        }
        self.found.extend(crate::secrets::hits(raw, -1, at));
        let p = crate::secrets::mask(&p);
        if self.first_prompt.is_none() {
            self.first_prompt = Some(one_line(&p, 400));
        }
        if !self.text.is_empty() {
            self.text.push('\n');
        }
        self.text.push_str(&p);
        self.prompts += 1;
        if let Some(t) = at {
            *self.quarters.entry(t.div_euclid(QUARTER_MS)).or_default() += 1;
            self.started = Some(self.started.map_or(t, |s| s.min(t)));
            self.ended = Some(self.ended.map_or(t, |e| e.max(t)));
        }
    }
}

/// A session for the UI.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Session {
    pub id: String,
    pub agent: AgentKey,
    pub origin: SessionOrigin,
    pub title: String,
    pub first_prompt: Option<String>,
    pub project: Option<PathBuf>,
    #[ts(type = "number | null")]
    pub started: Option<i64>,
    #[ts(type = "number | null")]
    pub ended: Option<i64>,
    pub prompts: u32,
    pub source: PathBuf,
    /// Shell command that resumes it, when the agent has one.
    pub resume: Option<String>,
}

/// A search result: the session and a masked excerpt. The matched words sit
/// between U+E000 and U+E001.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SearchHit {
    pub id: String,
    pub snippet: String,
}

impl Session {
    pub fn origin_name(&self) -> &'static str {
        self.origin.as_str()
    }
}

fn clip(s: &str, cap: usize) -> String {
    match s.char_indices().nth(cap) {
        Some((i, _)) => s[..i].to_string(),
        None => s.to_string(),
    }
}

fn one_line(s: &str, cap: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > cap {
        format!("{}…", clip(&flat, cap - 1).trim_end())
    } else {
        flat
    }
}

/// Agent titles that say nothing about the chat.
fn generic(t: &str) -> bool {
    let t = t.trim().to_lowercase();
    t.is_empty() || ["new agent", "new chat", "new session", "untitled", "new composer"].contains(&t.as_str())
}

/// The chat's own name (for Claude: the user's `/rename`), unless it says
/// nothing; else the first real prompt (DESIGN.md update 2026-11-07).
pub fn display_title(title: Option<&str>, first: Option<&str>) -> String {
    let pick = title.filter(|t| !generic(t)).or(first);
    let bare = strip_tags(pick.unwrap_or(""));
    one_line(if bare.trim().is_empty() { "Untitled session" } else { &bare }, TITLE_CAP)
}

/// Drops short markup like `<pasted_content id="…">` so a title starts with words.
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        match tail.find('>') {
            Some(j) if j <= 120 && !tail[..j].contains('\n') && tail[1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '/') => {
                out.push(' ');
                rest = &tail[j + 1..];
            }
            _ => {
                out.push('<');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The real prompt a Claude `user` line holds, or `None` for tool results,
/// meta lines, slash commands, command output and injected context.
pub fn claude_prompt(line: &[u8]) -> Option<(String, Option<i64>)> {
    #[derive(Deserialize)]
    struct Msg {
        content: serde_json::Value,
    }
    #[derive(Deserialize)]
    struct Line {
        #[serde(rename = "type")]
        kind: Option<String>,
        message: Option<Msg>,
        #[serde(rename = "isMeta")]
        is_meta: Option<bool>,
        #[serde(rename = "isCompactSummary")]
        compact: Option<bool>,
        #[serde(rename = "isSidechain")]
        sidechain: Option<bool>,
        timestamp: Option<String>,
    }
    let l: Line = serde_json::from_slice(line).ok()?;
    if l.kind.as_deref() != Some("user") || l.is_meta == Some(true) || l.compact == Some(true) || l.sidechain == Some(true) {
        return None;
    }
    let text = match l.message?.content {
        serde_json::Value::String(s) => s,
        serde_json::Value::Array(blocks) => {
            let mut parts = Vec::new();
            for b in blocks {
                match b.get("type").and_then(|t| t.as_str()) {
                    Some("tool_result") => return None,
                    Some("text") => {
                        if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                            if !t.trim_start().starts_with("<system-reminder>") {
                                parts.push(t.to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
            parts.join("\n")
        }
        _ => return None,
    };
    let t = text.trim();
    const NOT_PROMPTS: &[&str] = &[
        "<command-",
        "<local-command-",
        "<bash-",
        "<task-notification",
        "<user-prompt-submit-hook",
        "<system-reminder>",
        "[Request interrupted",
        "Caveat:",
        "This session is being continued",
    ];
    if t.is_empty() || NOT_PROMPTS.iter().any(|p| t.starts_with(p)) {
        return None;
    }
    let at = l.timestamp.as_deref().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|d| d.timestamp_millis());
    Some((t.to_string(), at))
}

/// `'…'` for zsh, safe for any text.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn safe_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 200 && id.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}

/// The command that resumes a session, run from its project folder.
pub fn resume_command(origin: SessionOrigin, native_id: &str, project: Option<&str>) -> Option<String> {
    if !safe_id(native_id) {
        return None;
    }
    let cmd = match origin {
        SessionOrigin::Claude => format!("claude --resume {native_id}"),
        SessionOrigin::Codex => format!("codex resume {native_id}"),
        SessionOrigin::Opencode => format!("opencode -s {native_id}"),
        SessionOrigin::CursorCli => format!("cursor-agent --resume {native_id}"),
        // Gemini resumes by list index, not id; Cursor editor chats open in Cursor.
        SessionOrigin::Gemini | SessionOrigin::CursorEditor => return None,
    };
    Some(match project.filter(|p| std::path::Path::new(p).is_dir()) {
        Some(p) => format!("cd {} && {cmd}", quote(p)),
        None => cmd,
    })
}

impl Core {
    pub fn sessions(&self) -> Result<Vec<Session>, AppError> {
        self.store.sessions()
    }

    pub fn sessions_search(&self, query: &str) -> Result<Vec<SearchHit>, AppError> {
        self.store.sessions_search(query)
    }

    /// Opens Terminal in a new window that runs the resume command. The
    /// command goes through a generated `.command` file in SkillDesk's data
    /// folder (architecture: External).
    pub fn session_resume(&self, id: &str) -> Result<String, AppError> {
        let s = self
            .store
            .sessions()?
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| AppError::new(ErrorKind::NotFound, "That session is no longer on disk."))?;
        let cmd = s.resume.ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "This agent has no resume command."))?;
        let dir = self.env.data_dir.join("resume");
        std::fs::create_dir_all(&dir).map_err(|e| AppError::new(ErrorKind::Io, e.to_string()))?;
        let file = dir.join(format!("{}.command", id.replace(':', "-")));
        let body = format!("#!/bin/zsh -il\n# Written by SkillDesk to resume a session.\n{cmd}\n");
        std::fs::write(&file, body).map_err(|e| AppError::new(ErrorKind::Io, e.to_string()))?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o700)).map_err(|e| AppError::new(ErrorKind::Io, e.to_string()))?;
        let ok = std::process::Command::new("/usr/bin/open")
            .args(["-a", "Terminal"])
            .arg(&file)
            .status()
            .map_err(|e| AppError::new(ErrorKind::Io, format!("Cannot open Terminal: {e}")))?
            .success();
        if !ok {
            return Err(AppError::new(ErrorKind::Io, "Terminal did not open."));
        }
        Ok(cmd)
    }

    /// Opens a Cursor editor chat's project folder in Cursor.
    pub fn session_open_project(&self, id: &str) -> Result<(), AppError> {
        let s = self
            .store
            .sessions()?
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| AppError::new(ErrorKind::NotFound, "That session is no longer on disk."))?;
        let p = s.project.filter(|p| p.is_dir()).ok_or_else(|| AppError::new(ErrorKind::NotFound, "The project folder is gone."))?;
        std::process::Command::new("/usr/bin/open")
            .args(["-a", "Cursor"])
            .arg(&p)
            .status()
            .map_err(|e| AppError::new(ErrorKind::Io, format!("Cannot open Cursor: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Secret leak + read-only guarantee: prompts holding secrets are stored
    /// masked everywhere (sessions, index, search, IPC payloads), and the
    /// agent database SkillDesk reads is not written (no bytes, no -wal/-shm).
    #[test]
    fn sessions_fts_holds_masked_text_only() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let key = "sk-ant-api03-SECRETsecretSECRETsecret0123456789";
        let pass = "hunter2hunter2hunter2";
        let proj = home.join(".claude/projects/-tmp-demo");
        std::fs::create_dir_all(&proj).unwrap();
        let line = |t: &str| {
            serde_json::json!({"type":"user","message":{"role":"user","content":t},"cwd":"/tmp/demo","timestamp":"2026-11-01T10:00:00Z"}).to_string()
        };
        std::fs::write(
            proj.join("s1.jsonl"),
            format!("{}\n{}\n", line(&format!("deploy with key {key} today")), line(&format!("db is postgres://app:{pass}@db.local/x"))),
        )
        .unwrap();
        let oc = home.join(".local/share/opencode");
        std::fs::create_dir_all(&oc).unwrap();
        let db = oc.join("opencode.db");
        {
            let c = rusqlite::Connection::open(&db).unwrap();
            c.execute_batch(&format!(
                "CREATE TABLE session (id TEXT, title TEXT, directory TEXT, parent_id TEXT, time_created INTEGER, time_updated INTEGER);
                 CREATE TABLE message (id TEXT, session_id TEXT, time_created INTEGER, data TEXT);
                 CREATE TABLE part (id TEXT, message_id TEXT, session_id TEXT, data TEXT);
                 INSERT INTO session VALUES ('ses1', 'Deploy', '/tmp', NULL, 1, 2);
                 INSERT INTO message VALUES ('m1', 'ses1', 1, '{{\"role\":\"user\"}}');
                 INSERT INTO part VALUES ('p1', 'm1', 'ses1', '{{\"type\":\"text\",\"text\":\"use {key} now\"}}');"
            ))
            .unwrap();
        }
        let before = std::fs::read(&db).unwrap();
        let core = crate::Core::open(crate::Env::for_test(&home, &dir.path().join("data"))).unwrap();
        crate::scan::run_scan(&core, &crate::scan::NoSink).unwrap();

        assert_eq!(std::fs::read(&db).unwrap(), before, "agent database changed");
        assert!(!oc.join("opencode.db-wal").exists() && !oc.join("opencode.db-shm").exists());

        let sessions = core.sessions().unwrap();
        assert_eq!(sessions.len(), 2, "{sessions:?}");
        let hits = core.sessions_search("deploy").unwrap();
        assert_eq!(hits.len(), 2);
        let payload = format!("{}{}", serde_json::to_string(&sessions).unwrap(), serde_json::to_string(&hits).unwrap());
        let stored = {
            let c = rusqlite::Connection::open(dir.path().join("data/skilldesk.db")).unwrap();
            let mut st = c.prepare("SELECT text FROM sessions_fts UNION ALL SELECT COALESCE(title,'') || COALESCE(first_prompt,'') FROM sessions").unwrap();
            st.query_map([], |r| r.get::<_, String>(0)).unwrap().map(|r| r.unwrap()).collect::<Vec<_>>().join("\n")
        };
        for text in [&payload, &stored] {
            assert!(!text.contains(key) && !text.contains("SECRETsecret"), "key leaked: {text}");
            assert!(!text.contains(pass), "password leaked: {text}");
        }
        assert!(stored.contains("••••"));
    }

    #[test]
    fn resume_quotes_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let odd = dir.path().join("it's here");
        std::fs::create_dir(&odd).unwrap();
        let c = resume_command(SessionOrigin::Claude, "abc-123", odd.to_str()).unwrap();
        assert!(c.ends_with("&& claude --resume abc-123"));
        assert!(c.contains("'\\''"));
        assert!(resume_command(SessionOrigin::Claude, "x; rm -rf ~", None).is_none());
    }
}
