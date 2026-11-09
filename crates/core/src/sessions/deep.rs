//! Stage `Deep`: sessions of Codex, OpenCode, Cursor (editor and agent CLI)
//! and Gemini CLI. Agent databases open read-only and immutable (ADR 0011):
//! SkillDesk never writes them, takes no lock and makes no `-wal`/`-shm`.

use super::{SessionOrigin, SessionRow};
use crate::env::Env;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Opens an agent database without any write: `mode=ro&immutable=1`.
pub fn open_ro(path: &Path) -> Option<Connection> {
    if !path.is_file() {
        return None;
    }
    let p = path.to_string_lossy().replace('%', "%25").replace('?', "%3f").replace('#', "%23");
    let uri = format!("file:{p}?mode=ro&immutable=1");
    Connection::open_with_flags(uri, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX).ok()
}

pub fn cursor_db(env: &Env) -> PathBuf {
    env.home.join("Library/Application Support/Cursor/User/globalStorage/state.vscdb")
}

/// Changes when Cursor writes its database (main file or WAL).
pub fn cursor_stamp(env: &Env) -> String {
    let db = cursor_db(env);
    let m = |p: PathBuf| std::fs::metadata(p).ok().map(|m| format!("{}:{:?}", m.len(), m.modified().ok())).unwrap_or_default();
    format!("{}|{}", m(db.clone()), m(PathBuf::from(format!("{}-wal", db.display()))))
}

fn ms(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp_millis()).or_else(|| s.parse().ok()),
        _ => None,
    }
}

fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn files_under(dir: &Path, keep: &dyn Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(p),
                Ok(t) if t.is_file() && keep(&p) => out.push(p),
                _ => {}
            }
        }
    }
    out.sort();
    out
}

/// `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`.
pub fn codex(env: &Env) -> Vec<SessionRow> {
    let files = files_under(&env.home.join(".codex/sessions"), &|p| {
        p.extension().is_some_and(|e| e == "jsonl") && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("rollout-"))
    });
    // Injected context, not typed by the user.
    const INJECTED: &[&str] = &["# AGENTS.md instructions", "<environment_context>", "<user_instructions>", "<permissions", "<user_shell_command>"];
    let mut out = Vec::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let mut row = SessionRow { origin: Some(SessionOrigin::Codex), source: f.to_string_lossy().into_owned(), ..Default::default() };
        let (mut events, mut items) = (Vec::new(), Vec::new());
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
            let at = v.get("timestamp").and_then(ms);
            if let Some(t) = at {
                row.started = Some(row.started.map_or(t, |s| s.min(t)));
                row.ended = Some(row.ended.map_or(t, |e| e.max(t)));
            }
            let p = v.get("payload").cloned().unwrap_or(Value::Null);
            match (v.get("type").and_then(|t| t.as_str()), p.get("type").and_then(|t| t.as_str())) {
                (Some("session_meta"), _) => {
                    row.native_id = p.get("id").and_then(|s| s.as_str()).unwrap_or("").to_string();
                    row.project = p.get("cwd").and_then(|s| s.as_str()).map(String::from);
                }
                (Some("event_msg"), Some("user_message")) => {
                    if let Some(m) = p.get("message").and_then(|m| m.as_str()) {
                        events.push((m.to_string(), at));
                    }
                }
                (Some("response_item"), Some("message")) if p.get("role").and_then(|r| r.as_str()) == Some("user") => {
                    let t = text_of(p.get("content").unwrap_or(&Value::Null));
                    if !INJECTED.iter().any(|i| t.trim_start().starts_with(i)) {
                        items.push((t, at));
                    }
                }
                _ => {}
            }
        }
        // `user_message` events are what the user typed; exec runs have only items.
        for (t, at) in if events.is_empty() { items } else { events } {
            row.push_prompt(&t, at);
        }
        if !row.native_id.is_empty() {
            out.push(row);
        }
    }
    out
}

/// `~/.local/share/opencode/opencode.db`: top-level sessions and their user text parts.
pub fn opencode(env: &Env) -> Vec<SessionRow> {
    let db = env.home.join(".local/share/opencode/opencode.db");
    let Some(conn) = open_ro(&db) else { return Vec::new() };
    let mut rows: Vec<SessionRow> = Vec::new();
    let mut index = std::collections::HashMap::new();
    if let Ok(mut st) = conn.prepare("SELECT id, title, directory, time_created, time_updated FROM session WHERE parent_id IS NULL ORDER BY time_created") {
        let it = st.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, Option<i64>>(3)?, r.get::<_, Option<i64>>(4)?))
        });
        if let Ok(it) = it {
            for (id, title, dir, created, updated) in it.flatten() {
                index.insert(id.clone(), rows.len());
                rows.push(SessionRow {
                    native_id: id,
                    origin: Some(SessionOrigin::Opencode),
                    source: db.to_string_lossy().into_owned(),
                    title: title.map(|t| crate::secrets::mask(&t)),
                    project: dir,
                    started: created,
                    ended: updated,
                    ..Default::default()
                });
            }
        }
    }
    let q = "SELECT p.session_id, p.data, m.time_created FROM part p JOIN message m ON m.id = p.message_id
             WHERE json_extract(m.data, '$.role') = 'user' AND json_extract(p.data, '$.type') = 'text'
               AND COALESCE(json_extract(p.data, '$.synthetic'), 0) = 0
             ORDER BY m.time_created, p.id";
    if let Ok(mut st) = conn.prepare(q) {
        if let Ok(it) = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<i64>>(2)?))) {
            for (sid, data, at) in it.flatten() {
                let Some(&i) = index.get(&sid) else { continue };
                let t = serde_json::from_str::<Value>(&data).ok().and_then(|v| v.get("text").and_then(|t| t.as_str()).map(String::from)).unwrap_or_default();
                let (s, e) = (rows[i].started, rows[i].ended);
                rows[i].push_prompt(&t, at);
                // The session's own times win over message times.
                rows[i].started = s.or(rows[i].started);
                rows[i].ended = e.max(rows[i].ended);
            }
        }
    }
    rows
}

/// Cursor editor chats: `composerData:<id>` rows in `state.vscdb`, user
/// bubbles inline (`conversation`, old builds) or as `bubbleId:<id>:<bubble>`.
pub fn cursor_editor(env: &Env) -> Vec<SessionRow> {
    let db = cursor_db(env);
    let Some(conn) = open_ro(&db) else { return Vec::new() };
    let source = db.to_string_lossy().into_owned();
    // Workspace folders: header path, else the workspace's `workspace.json`.
    let ws_dir = env.home.join("Library/Application Support/Cursor/User/workspaceStorage");
    let folder_of = |id: &str| -> Option<String> {
        let j: Value = serde_json::from_str(&std::fs::read_to_string(ws_dir.join(id).join("workspace.json")).ok()?).ok()?;
        let uri = j.get("folder")?.as_str()?;
        Some(percent_decode(uri.strip_prefix("file://")?))
    };
    let mut heads: std::collections::HashMap<String, (Option<String>, bool, Option<i64>)> = std::collections::HashMap::new();
    if let Ok(mut st) = conn.prepare("SELECT composerId, workspaceId, isSubagent, value FROM composerHeaders") {
        if let Ok(it) = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<i64>>(2)?, r.get::<_, Option<String>>(3)?))) {
            for (cid, ws, sub, value) in it.flatten() {
                let v: Value = value.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
                let path = v.pointer("/workspaceIdentifier/uri/fsPath").and_then(|p| p.as_str()).map(String::from).or_else(|| ws.as_deref().and_then(folder_of));
                let updated = v.get("lastUpdatedAt").and_then(ms);
                heads.insert(cid, (path, sub == Some(1), updated));
            }
        }
    }
    let Ok(mut st) = conn.prepare("SELECT key, value FROM cursorDiskKV WHERE key >= 'composerData:' AND key < 'composerData;'") else { return Vec::new() };
    let Ok(mut bubble) = conn.prepare("SELECT value FROM cursorDiskKV WHERE key = ?1") else { return Vec::new() };
    let Ok(it) = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))) else { return Vec::new() };
    let mut out = Vec::new();
    for (key, value) in it.flatten() {
        let cid = key.trim_start_matches("composerData:").to_string();
        let Some(v) = value.and_then(|s| serde_json::from_str::<Value>(&s).ok()) else { continue };
        let head = heads.get(&cid);
        if head.is_some_and(|h| h.1) {
            continue;
        }
        let created = v.get("createdAt").and_then(ms);
        let mut row = SessionRow {
            native_id: cid.clone(),
            origin: Some(SessionOrigin::CursorEditor),
            source: source.clone(),
            title: v.get("name").and_then(|n| n.as_str()).map(crate::secrets::mask),
            project: head.and_then(|h| h.0.clone()),
            started: created,
            ended: v.get("lastUpdatedAt").and_then(ms).or(head.and_then(|h| h.2)).or(created),
            ..Default::default()
        };
        let mut prompts: Vec<(String, Option<i64>)> = Vec::new();
        if let Some(Value::Array(conv)) = v.get("conversation") {
            for b in conv.iter().filter(|b| b.get("type").and_then(|t| t.as_i64()) == Some(1)) {
                prompts.push((b.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string(), b.get("createdAt").and_then(ms)));
            }
        } else if let Some(Value::Array(hs)) = v.get("fullConversationHeadersOnly") {
            for h in hs.iter().filter(|h| h.get("type").and_then(|t| t.as_i64()) == Some(1)) {
                let Some(bid) = h.get("bubbleId").and_then(|b| b.as_str()) else { continue };
                let raw: Option<String> = bubble.query_row([format!("bubbleId:{cid}:{bid}")], |r| r.get(0)).ok();
                let Some(b) = raw.and_then(|s| serde_json::from_str::<Value>(&s).ok()) else { continue };
                prompts.push((b.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string(), b.get("createdAt").and_then(ms)));
            }
        }
        let (s, e) = (row.started, row.ended);
        for (t, at) in prompts {
            // User bubbles sometimes lack `createdAt`; the chat's own time stands.
            row.push_prompt(&t, at);
        }
        row.started = s.or(row.started);
        row.ended = e.max(row.ended);
        if row.prompts > 0 {
            out.push(row);
        }
    }
    out
}

/// Cursor agent CLI chats: `~/.cursor/chats/<hash>/<agentId>/store.db`.
pub fn cursor_cli(env: &Env) -> Vec<SessionRow> {
    let files = files_under(&env.home.join(".cursor/chats"), &|p| p.file_name().is_some_and(|n| n == "store.db"));
    let mut out = Vec::new();
    for f in files {
        let Some(conn) = open_ro(&f) else { continue };
        let meta: Option<String> = conn.query_row("SELECT value FROM meta WHERE key = '0'", [], |r| r.get(0)).ok();
        let Some(m) = meta.and_then(|h| hex::decode(h.trim()).ok()).and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else { continue };
        let modified = std::fs::metadata(&f).and_then(|m| m.modified()).ok().map(|t| chrono::DateTime::<chrono::Utc>::from(t).timestamp_millis());
        let created = m.get("createdAt").and_then(ms);
        let mut row = SessionRow {
            native_id: m.get("agentId").and_then(|s| s.as_str()).unwrap_or("").to_string(),
            origin: Some(SessionOrigin::CursorCli),
            source: f.to_string_lossy().into_owned(),
            title: m.get("name").and_then(|n| n.as_str()).map(crate::secrets::mask),
            started: created,
            ended: modified.or(created),
            ..Default::default()
        };
        let Ok(mut st) = conn.prepare("SELECT data FROM blobs ORDER BY rowid") else { continue };
        let Ok(it) = st.query_map([], |r| r.get::<_, Vec<u8>>(0)) else { continue };
        for data in it.flatten() {
            if data.first() != Some(&b'{') {
                continue;
            }
            let Ok(v) = serde_json::from_slice::<Value>(&data) else { continue };
            if v.get("role").and_then(|r| r.as_str()) != Some("user") {
                continue;
            }
            let t = text_of(v.get("content").unwrap_or(&Value::Null));
            if row.project.is_none() {
                if let Some(i) = t.find("Workspace Path: ") {
                    let rest = &t[i + 16..];
                    let end = rest.find("  ").or_else(|| rest.find('\n')).unwrap_or(rest.len());
                    row.project = Some(rest[..end].trim().to_string());
                }
            }
            if let (Some(a), Some(b)) = (t.find("<user_query>"), t.find("</user_query>")) {
                if a + 12 <= b {
                    row.push_prompt(&t[a + 12..b], None);
                }
            }
        }
        row.started = created.or(row.started);
        if !row.native_id.is_empty() {
            out.push(row);
        }
    }
    out
}

/// Gemini CLI: `~/.gemini/tmp/<project>/chats/session-*.jsonl` (older builds:
/// one `session-*.json`). Line 1 holds `sessionId` and `startTime`.
pub fn gemini(env: &Env) -> Vec<SessionRow> {
    let files = files_under(&env.home.join(".gemini/tmp"), &|p| {
        p.parent().and_then(|d| d.file_name()).is_some_and(|n| n == "chats")
            && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("session-"))
            && p.extension().is_some_and(|e| e == "jsonl" || e == "json")
    });
    // projects.json: path → slug (the `tmp/<slug>` folder).
    let projects: Vec<(String, String)> = std::fs::read_to_string(env.home.join(".gemini/projects.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.get("projects").cloned().or(Some(v)))
        .and_then(|v| v.as_object().map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect()))
        .unwrap_or_default();
    let mut out = Vec::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let slug = f.parent().and_then(|d| d.parent()).and_then(|d| d.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut row = SessionRow {
            origin: Some(SessionOrigin::Gemini),
            source: f.to_string_lossy().into_owned(),
            project: projects.iter().find(|(_, s)| *s == slug).map(|(p, _)| p.clone()),
            ..Default::default()
        };
        let lines: Vec<Value> = if f.extension().is_some_and(|e| e == "json") {
            let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            let mut l = vec![v.clone()];
            l.extend(v.get("messages").and_then(|m| m.as_array()).cloned().unwrap_or_default());
            l
        } else {
            text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
        };
        for v in lines {
            if let Some(id) = v.get("sessionId").and_then(|s| s.as_str()) {
                if row.native_id.is_empty() {
                    row.native_id = id.to_string();
                    row.started = v.get("startTime").and_then(ms);
                }
            }
            let at = v.get("timestamp").and_then(ms);
            if let Some(t) = at {
                row.ended = Some(row.ended.map_or(t, |e| e.max(t)));
            }
            if v.get("type").and_then(|t| t.as_str()) == Some("user") {
                let t = text_of(v.get("content").unwrap_or(&Value::Null));
                row.push_prompt(&t, at);
            }
        }
        if !row.native_id.is_empty() {
            out.push(row);
        }
    }
    out
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
