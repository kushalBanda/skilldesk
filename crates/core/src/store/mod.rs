//! SQLite: cache + history (ADR 0005, 0011): one Mutex<Connection>, WAL, busy_timeout, foreign keys.

mod migrations;
mod secrets;

use crate::error::{AppError, ErrorKind};
use crate::actions::{ActionRecord, ActionStatus};
use crate::model::{Item, ItemId, ItemKind, KindCount};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::Mutex;

pub use migrations::SCHEMA_VERSION;
pub use secrets::FileHits;

pub struct Store {
    conn: Mutex<Connection>,
    /// Random per install; secret values are kept only as hashes with it.
    pub(crate) salt: String,
}

fn db_err(e: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorKind::Database, e.to_string())
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| AppError::new(ErrorKind::Io, format!("Cannot create {}: {e}", dir.display())))?;
        }
        let conn = Connection::open(path).map_err(db_err)?;
        conn.busy_timeout(std::time::Duration::from_secs(5)).map_err(db_err)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(db_err)?;
        migrations::run(&conn)?;
        let salt = secrets::salt(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            salt,
        })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Replace every cached item of `kind` in one transaction.
    /// Replaces every item of `kind`. Returns false (and writes nothing) when
    /// the items are the same as last time, so only real changes reach the UI.
    pub fn replace_kind(&self, kind: ItemKind, items: &[Item]) -> Result<bool, AppError> {
        let bodies: Vec<String> = items
            .iter()
            .map(|i| serde_json::to_string(i).map_err(|e| AppError::internal(e.to_string())))
            .collect::<Result<_, _>>()?;
        let hash = {
            use sha2::{Digest, Sha256};
            let mut h = Sha256::new();
            for b in &bodies {
                h.update(b.as_bytes());
                h.update([0]);
            }
            hex::encode(h.finalize())
        };
        let key = format!("hash.{}", kind.as_str());
        if self.setting(&key)?.as_deref() == Some(hash.as_str()) {
            return Ok(false);
        }
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(db_err)?;
        tx.execute(
            "DELETE FROM item_sources WHERE item_id IN (SELECT id FROM items WHERE kind = ?1)",
            params![kind.as_str()],
        )
        .map_err(db_err)?;
        tx.execute("DELETE FROM items WHERE kind = ?1", params![kind.as_str()])
            .map_err(db_err)?;
        {
            let mut ins = tx
                .prepare("INSERT OR REPLACE INTO items (id, kind, name, status, body) VALUES (?1, ?2, ?3, ?4, ?5)")
                .map_err(db_err)?;
            let mut src = tx
                .prepare("INSERT INTO item_sources (item_id, path, line) VALUES (?1, ?2, ?3)")
                .map_err(db_err)?;
            for (item, body) in items.iter().zip(&bodies) {
                ins.execute(params![item.id.0, kind.as_str(), item.name, item.status.as_str(), body])
                    .map_err(db_err)?;
                for s in &item.sources {
                    src.execute(params![item.id.0, s.path.to_string_lossy(), s.line])
                        .map_err(db_err)?;
                }
            }
            tx.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
                params![key, hash],
            )
            .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;
        Ok(true)
    }

    pub fn items(&self, kind: ItemKind) -> Result<Vec<Item>, AppError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT body FROM items WHERE kind = ?1 ORDER BY name COLLATE NOCASE")
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![kind.as_str()], |r| r.get::<_, String>(0))
            .map_err(db_err)?;
        rows.map(|r| {
            let body = r.map_err(db_err)?;
            serde_json::from_str(&body).map_err(|e| AppError::internal(e.to_string()))
        })
        .collect()
    }

    pub fn item(&self, id: &ItemId) -> Result<Option<Item>, AppError> {
        let conn = self.conn();
        let body: Option<String> = conn
            .query_row("SELECT body FROM items WHERE id = ?1", params![id.0], |r| r.get(0))
            .optional()
            .map_err(db_err)?;
        body.map(|b| serde_json::from_str(&b).map_err(|e| AppError::internal(e.to_string())))
            .transpose()
    }

    /// Counts of real items per kind (folders that are not items excluded).
    pub fn kind_counts(&self) -> Result<Vec<KindCount>, AppError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT kind, COUNT(*) FROM items WHERE status NOT IN ('not_an_item', 'gone') GROUP BY kind")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .map_err(db_err)?;
        let mut out = Vec::new();
        for r in rows {
            let (k, n) = r.map_err(db_err)?;
            if let Some(kind) = ItemKind::parse(&k) {
                out.push(KindCount { kind, count: n as u64 });
            }
        }
        Ok(out)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), AppError> {
        self.conn()
            .execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
                params![key, value],
            )
            .map(|_| ())
            .map_err(db_err)
    }

    /// path → (size, mtime ns, offset) of every transcript read before.
    pub fn transcript_stamps(&self) -> Result<std::collections::HashMap<String, (u64, i64, u64)>, AppError> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT path, size, mtime, offset FROM transcript_files").map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, (r.get::<_, i64>(1)? as u64, r.get::<_, i64>(2)?, r.get::<_, i64>(3)? as u64))))
            .map_err(db_err)?;
        rows.collect::<Result<_, _>>().map_err(db_err)
    }

    /// Stores what each file added since its last offset. `reset` files are
    /// dropped first (they shrank or were replaced); `gone` files are removed.
    pub fn apply_transcripts(&self, deltas: &[crate::parse::claude_runs::FileDelta], gone: &[String]) -> Result<(), AppError> {
        use crate::parse::claude_runs::Outcome;
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(db_err)?;
        {
            let mut del_h = tx.prepare("DELETE FROM t_hook_runs WHERE path = ?1").map_err(db_err)?;
            let mut del_m = tx.prepare("DELETE FROM t_mcp_calls WHERE path = ?1").map_err(db_err)?;
            let mut del_f = tx.prepare("DELETE FROM transcript_files WHERE path = ?1").map_err(db_err)?;
            let mut del_u = tx.prepare("DELETE FROM t_uses WHERE path = ?1").map_err(db_err)?;
            let mut del_ms = tx.prepare("DELETE FROM t_market_sources WHERE path = ?1").map_err(db_err)?;
            let mut del_fts = tx.prepare("DELETE FROM sessions_fts WHERE session_id IN (SELECT id FROM sessions WHERE source = ?1)").map_err(db_err)?;
            let mut del_s = tx.prepare("DELETE FROM sessions WHERE source = ?1").map_err(db_err)?;
            let mut del_q = tx.prepare("DELETE FROM session_quarters WHERE session_id IN (SELECT id FROM sessions WHERE source = ?1)").map_err(db_err)?;
            for p in gone {
                secrets::delete_path(&tx, p)?;
                del_fts.execute(params![p]).map_err(db_err)?;
                del_q.execute(params![p]).map_err(db_err)?;
                del_s.execute(params![p]).map_err(db_err)?;
                del_h.execute(params![p]).map_err(db_err)?;
                del_m.execute(params![p]).map_err(db_err)?;
                del_u.execute(params![p]).map_err(db_err)?;
                del_ms.execute(params![p]).map_err(db_err)?;
                del_f.execute(params![p]).map_err(db_err)?;
            }
            let mut ins_h = tx
                .prepare("INSERT INTO t_hook_runs (path, event, command, ms, outcome, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")
                .map_err(db_err)?;
            let mut up_m = tx
                .prepare(
                    "INSERT INTO t_mcp_calls (path, server, count, last) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(path, server) DO UPDATE SET count = count + ?3, last = MAX(COALESCE(last, 0), COALESCE(?4, 0))",
                )
                .map_err(db_err)?;
            let mut up_u = tx
                .prepare(
                    "INSERT INTO t_uses (path, kind, name, count, last) VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(path, kind, name) DO UPDATE SET count = count + ?4, last = MAX(COALESCE(last, 0), COALESCE(?5, 0))",
                )
                .map_err(db_err)?;
            let mut up_ms = tx
                .prepare(
                    "INSERT INTO t_market_sources (path, name, source, last) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(path, name, source) DO UPDATE SET last = MAX(COALESCE(last, 0), COALESCE(?4, 0))",
                )
                .map_err(db_err)?;
            let mut up_f = tx
                .prepare(
                    "INSERT INTO transcript_files (path, size, mtime, offset) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(path) DO UPDATE SET size = ?2, mtime = ?3, offset = ?4",
                )
                .map_err(db_err)?;
            for d in deltas {
                if d.reset {
                    secrets::delete_path(&tx, &d.path)?;
                    del_fts.execute(params![d.path]).map_err(db_err)?;
                    del_q.execute(params![d.path]).map_err(db_err)?;
                    del_s.execute(params![d.path]).map_err(db_err)?;
                    del_h.execute(params![d.path]).map_err(db_err)?;
                    del_m.execute(params![d.path]).map_err(db_err)?;
                    del_u.execute(params![d.path]).map_err(db_err)?;
                    del_ms.execute(params![d.path]).map_err(db_err)?;
                }
                for ((event, command), run) in &d.hooks {
                    let o = match run.outcome {
                        Outcome::Ok => 0,
                        Outcome::Blocked => 1,
                        Outcome::Failed => 2,
                        Outcome::Cancelled => 3,
                    };
                    ins_h.execute(params![d.path, event, command, run.ms, o, run.at]).map_err(db_err)?;
                }
                for (server, c) in &d.mcp {
                    up_m.execute(params![d.path, server, c.count as i64, c.last]).map_err(db_err)?;
                }
                for (kind, map) in [("subagent", &d.subagents), ("command", &d.commands)] {
                    for (name, c) in map {
                        up_u.execute(params![d.path, kind, name, c.count as i64, c.last]).map_err(db_err)?;
                    }
                }
                for ((name, source), last) in &d.markets {
                    up_ms.execute(params![d.path, name, source, last]).map_err(db_err)?;
                }
                if let Some(row) = &d.session {
                    merge_session(&tx, row)?;
                }
                secrets::put_hits(&tx, &self.salt, &d.path, "transcript", crate::secrets::PlaceStore::TextFile, None, "", &d.secrets)?;
                up_f.execute(params![d.path, d.size as i64, d.mtime, d.offset as i64]).map_err(db_err)?;
            }
        }
        tx.commit().map_err(db_err)
    }

    /// Replaces every session of these origins (stage `Deep`).
    pub fn replace_sessions(&self, origins: &[crate::sessions::SessionOrigin], rows: &[crate::sessions::SessionRow]) -> Result<(), AppError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(db_err)?;
        for o in origins {
            tx.execute("DELETE FROM sessions_fts WHERE session_id IN (SELECT id FROM sessions WHERE origin = ?1)", params![o.as_str()]).map_err(db_err)?;
            tx.execute("DELETE FROM session_quarters WHERE session_id IN (SELECT id FROM sessions WHERE origin = ?1)", params![o.as_str()]).map_err(db_err)?;
            tx.execute("DELETE FROM sessions WHERE origin = ?1", params![o.as_str()]).map_err(db_err)?;
        }
        for r in rows {
            merge_session(&tx, r)?;
        }
        // Secrets in agent databases: listed, never written (ADR 0011).
        use crate::sessions::SessionOrigin as O;
        for o in origins.iter().filter(|o| matches!(o, O::Opencode | O::CursorEditor | O::CursorCli)) {
            tx.execute("DELETE FROM secret_places WHERE origin = ?1 AND store = 'db'", params![o.as_str()]).map_err(db_err)?;
        }
        for r in rows.iter().filter(|r| matches!(r.origin, Some(O::Opencode | O::CursorEditor | O::CursorCli))) {
            let o = r.origin.map(|o| o.as_str()).unwrap_or_default();
            let chat = crate::sessions::display_title(r.title.as_deref(), r.first_prompt.as_deref());
            let chat = if chat.is_empty() { r.native_id.clone() } else { chat };
            secrets::put_hits(&tx, &self.salt, &r.source, o, crate::secrets::PlaceStore::AgentDb, Some(o), &chat, &r.found)?;
        }
        tx.commit().map_err(db_err)
    }

    /// Sessions with at least one real prompt, newest first.
    pub fn sessions(&self) -> Result<Vec<crate::sessions::Session>, AppError> {
        use crate::sessions::{display_title, resume_command, Session, SessionOrigin};
        let conn = self.conn();
        let mut st = conn
            .prepare("SELECT id, origin, native_id, source, title, first_prompt, project, started, ended, prompts FROM sessions WHERE prompts > 0 ORDER BY COALESCE(ended, started) DESC")
            .map_err(db_err)?;
        let rows = st
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, Option<i64>>(7)?,
                    r.get::<_, Option<i64>>(8)?,
                    r.get::<_, i64>(9)?,
                ))
            })
            .map_err(db_err)?;
        let mut out = Vec::new();
        for row in rows {
            let (id, origin, native, source, title, first, project, started, ended, prompts) = row.map_err(db_err)?;
            let Some(o) = SessionOrigin::parse(&origin) else { continue };
            out.push(Session {
                resume: resume_command(o, &native, project.as_deref()),
                title: display_title(title.as_deref(), first.as_deref()),
                id,
                agent: crate::model::AgentKey(o.agent().into()),
                origin: o,
                first_prompt: first,
                project: project.map(std::path::PathBuf::from),
                started,
                ended,
                prompts: prompts as u32,
                source: source.into(),
            });
        }
        Ok(out)
    }

    /// Full-text search over masked prompt text. Every word must match (as a
    /// prefix); best matches first, one hit per session.
    pub fn sessions_search(&self, query: &str) -> Result<Vec<crate::sessions::SearchHit>, AppError> {
        let terms: Vec<String> = query
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .take(12)
            .map(|w| format!("\"{}\"*", w.to_lowercase()))
            .collect();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn();
        let mut st = conn
            .prepare(
                "SELECT session_id, snippet(sessions_fts, 1, char(57344), char(57345), '…', 14) FROM sessions_fts
                 WHERE sessions_fts MATCH ?1 ORDER BY rank LIMIT 500",
            )
            .map_err(db_err)?;
        let rows = st
            .query_map(params![terms.join(" ")], |r| Ok(crate::sessions::SearchHit { id: r.get(0)?, snippet: r.get(1)? }))
            .map_err(db_err)?;
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for h in rows {
            let h = h.map_err(db_err)?;
            if seen.insert(h.id.clone()) {
                out.push(h);
            }
        }
        Ok(out)
    }

    /// Everything stored, aggregated: hook runs by (event, command), MCP calls by server.
    pub fn transcript_stats(&self) -> Result<crate::parse::claude_runs::TranscriptStats, AppError> {
        use crate::parse::claude_runs::{Calls, Outcome, Run, TranscriptStats};
        let conn = self.conn();
        let mut stats = TranscriptStats::default();
        let mut stmt = conn.prepare("SELECT event, command, ms, outcome, at FROM t_hook_runs").map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, Option<i64>>(4)?))
            })
            .map_err(db_err)?;
        for row in rows {
            let (e, c, ms, o, at) = row.map_err(db_err)?;
            let outcome = match o {
                1 => Outcome::Blocked,
                2 => Outcome::Failed,
                3 => Outcome::Cancelled,
                _ => Outcome::Ok,
            };
            stats.hooks.entry((e, c)).or_default().push(Run { ms: ms as u32, outcome, at });
        }
        let mut stmt = conn
            .prepare("SELECT server, SUM(count), MAX(last) FROM t_mcp_calls GROUP BY server")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<i64>>(2)?)))
            .map_err(db_err)?;
        for row in rows {
            let (s, n, last) = row.map_err(db_err)?;
            stats.mcp.insert(s, Calls { count: n as u64, last: last.filter(|&l| l > 0) });
        }
        let mut stmt = conn
            .prepare("SELECT kind, name, SUM(count), MAX(last) FROM t_uses GROUP BY kind, name")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, Option<i64>>(3)?)))
            .map_err(db_err)?;
        for row in rows {
            let (k, name, n, last) = row.map_err(db_err)?;
            let map = if k == "subagent" { &mut stats.subagents } else { &mut stats.commands };
            map.insert(name, Calls { count: n as u64, last: last.filter(|&l| l > 0) });
        }
        let mut stmt = conn
            .prepare("SELECT name, source, MAX(last) FROM t_market_sources GROUP BY name, source")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<i64>>(2)?)))
            .map_err(db_err)?;
        for row in rows {
            let (name, source, last) = row.map_err(db_err)?;
            stats.markets.insert((name, source), last.filter(|&l| l > 0));
        }
        Ok(stats)
    }

    pub fn dismiss(&self, fix_id: &str, evidence: &str) -> Result<(), AppError> {
        self.conn()
            .execute(
                "INSERT INTO fix_dismissals (fix_id, evidence, at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(fix_id) DO UPDATE SET evidence = ?2, at = ?3",
                params![fix_id, evidence, chrono::Utc::now().to_rfc3339()],
            )
            .map(|_| ())
            .map_err(db_err)
    }

    pub fn dismissals(&self) -> Result<std::collections::HashMap<String, String>, AppError> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT fix_id, evidence FROM fix_dismissals").map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(db_err)?;
        rows.collect::<Result<_, _>>().map_err(db_err)
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>, AppError> {
        self.conn()
            .query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| r.get(0))
            .optional()
            .map_err(db_err)
    }

    pub fn start_run(&self, id: &str, started: &str) -> Result<(), AppError> {
        self.conn()
            .execute("INSERT INTO scan_runs (id, started) VALUES (?1, ?2)", params![id, started])
            .map(|_| ())
            .map_err(db_err)
    }

    pub fn finish_run(&self, id: &str, ended: &str, timings: &str) -> Result<(), AppError> {
        self.conn()
            .execute(
                "UPDATE scan_runs SET ended = ?2, timings = ?3 WHERE id = ?1",
                params![id, ended, timings],
            )
            .map(|_| ())
            .map_err(db_err)
    }

    /// Prompts per origin and UTC quarter hour (sessions with prompts only).
    pub fn activity(&self) -> Result<Vec<(String, i64, u64)>, AppError> {
        let conn = self.conn();
        let mut st = conn
            .prepare(
                "SELECT s.origin, q.quarter, SUM(q.count) FROM session_quarters q JOIN sessions s ON s.id = q.session_id
                 WHERE s.prompts > 0 GROUP BY s.origin, q.quarter",
            )
            .map_err(db_err)?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? as u64))).map_err(db_err)?;
        rows.collect::<Result<_, _>>().map_err(db_err)
    }

    /// Rows of one transcript use kind: (name, last use ms, count). One row per transcript file.
    pub fn uses(&self, kind: &str) -> Result<Vec<(String, Option<i64>, u64)>, AppError> {
        let conn = self.conn();
        let mut st = conn.prepare("SELECT name, last, count FROM t_uses WHERE kind = ?1").map_err(db_err)?;
        let rows = st.query_map([kind], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? as u64))).map_err(db_err)?;
        rows.collect::<Result<_, _>>().map_err(db_err)
    }

    /// Sessions with prompts, per origin.
    pub fn session_counts(&self) -> Result<Vec<(String, u64)>, AppError> {
        let conn = self.conn();
        let mut st = conn.prepare("SELECT origin, COUNT(*) FROM sessions WHERE prompts > 0 GROUP BY origin").map_err(db_err)?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64))).map_err(db_err)?;
        rows.collect::<Result<_, _>>().map_err(db_err)
    }

    pub fn last_scan_end(&self) -> Result<Option<String>, AppError> {
        self.conn()
            .query_row(
                "SELECT ended FROM scan_runs WHERE ended IS NOT NULL ORDER BY ended DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)
    }

    pub fn insert_action(&self, r: &ActionRecord) -> Result<(), AppError> {
        let body = serde_json::to_string(r).map_err(|e| AppError::internal(e.to_string()))?;
        self.conn()
            .execute(
                "INSERT INTO actions (id, at, kind, status, body) VALUES (?1, ?2, ?3, 'applied', ?4)",
                params![r.id, r.at, r.kind, body],
            )
            .map(|_| ())
            .map_err(db_err)
    }

    pub fn action(&self, id: &str) -> Result<Option<ActionRecord>, AppError> {
        let row: Option<(String, String)> = self
            .conn()
            .query_row("SELECT body, status FROM actions WHERE id = ?1", params![id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()
            .map_err(db_err)?;
        row.map(|(body, status)| {
            let mut r: ActionRecord = serde_json::from_str(&body).map_err(|e| AppError::internal(e.to_string()))?;
            r.status = if status == "undone" { ActionStatus::Undone } else { ActionStatus::Applied };
            Ok(r)
        })
        .transpose()
    }

    pub fn set_action_status(&self, id: &str, status: ActionStatus) -> Result<(), AppError> {
        let s = match status {
            ActionStatus::Applied => "applied",
            ActionStatus::Undone => "undone",
        };
        self.conn()
            .execute("UPDATE actions SET status = ?2 WHERE id = ?1", params![id, s])
            .map(|_| ())
            .map_err(db_err)
    }

    pub fn actions(&self) -> Result<Vec<ActionRecord>, AppError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT body, status FROM actions ORDER BY at DESC LIMIT 500")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(db_err)?;
        let mut out = Vec::new();
        for row in rows {
            let (body, status) = row.map_err(db_err)?;
            let mut r: ActionRecord = serde_json::from_str(&body).map_err(|e| AppError::internal(e.to_string()))?;
            r.status = if status == "undone" { ActionStatus::Undone } else { ActionStatus::Applied };
            out.push(r);
        }
        Ok(out)
    }
}

/// Inserts a session or adds an appended part of it: span widens, prompts
/// add up, the first title and prompt stay. Its text goes to the index.
fn merge_session(tx: &rusqlite::Transaction, r: &crate::sessions::SessionRow) -> Result<(), AppError> {
    let Some(o) = r.origin else { return Ok(()) };
    let id = r.id();
    tx.execute(
        "INSERT INTO sessions (id, origin, native_id, source, title, first_prompt, project, started, ended, prompts)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(id) DO UPDATE SET
           title = COALESCE(excluded.title, title),
           first_prompt = COALESCE(first_prompt, excluded.first_prompt),
           project = COALESCE(project, excluded.project),
           started = MIN(COALESCE(started, excluded.started), COALESCE(excluded.started, started)),
           ended = MAX(COALESCE(ended, excluded.ended), COALESCE(excluded.ended, ended)),
           prompts = prompts + excluded.prompts,
           source = excluded.source",
        params![id, o.as_str(), r.native_id, r.source, r.title, r.first_prompt, r.project, r.started, r.ended, r.prompts],
    )
    .map_err(db_err)?;
    // Prompts with no time of their own count at the session start.
    let mut quarters = r.quarters.clone();
    let timed: u32 = quarters.values().sum();
    if let (Some(t), true) = (r.started.or(r.ended), r.prompts > timed) {
        *quarters.entry(t.div_euclid(crate::sessions::QUARTER_MS)).or_default() += r.prompts - timed;
    }
    let mut up_q = tx
        .prepare_cached(
            "INSERT INTO session_quarters (session_id, quarter, count) VALUES (?1, ?2, ?3)
             ON CONFLICT(session_id, quarter) DO UPDATE SET count = count + ?3",
        )
        .map_err(db_err)?;
    for (q, n) in &quarters {
        up_q.execute(params![id, q, n]).map_err(db_err)?;
    }
    // The chat's own name is searchable too.
    let text = match &r.title {
        Some(t) => format!("{t}\n{}", r.text),
        None => r.text.clone(),
    };
    if !text.trim().is_empty() {
        tx.execute("INSERT INTO sessions_fts (session_id, text) VALUES (?1, ?2)", params![id, text]).map_err(db_err)?;
    }
    Ok(())
}
