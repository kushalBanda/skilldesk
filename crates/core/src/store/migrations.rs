//! `user_version` step migrations. A database from a newer SkillDesk is refused.

use crate::error::{AppError, ErrorKind};
use rusqlite::Connection;

pub const SCHEMA_VERSION: u32 = 9;

pub fn run(conn: &Connection) -> Result<(), AppError> {
    let db_err = |e: rusqlite::Error| AppError::new(ErrorKind::Database, e.to_string());
    let current: u32 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(db_err)?;
    if current > SCHEMA_VERSION {
        return Err(AppError::new(
            ErrorKind::Database,
            format!(
                "This database was written by a newer SkillDesk (schema {current}, this app knows {SCHEMA_VERSION}). Update SkillDesk, or rebuild the cache."
            ),
        ));
    }
    for version in current..SCHEMA_VERSION {
        step(conn, version).map_err(db_err)?;
        conn.pragma_update(None, "user_version", version + 1)
            .map_err(db_err)?;
    }
    Ok(())
}

fn step(conn: &Connection, from: u32) -> rusqlite::Result<()> {
    match from {
        0 => conn.execute_batch(
            "
            CREATE TABLE items (
              id     TEXT PRIMARY KEY,
              kind   TEXT NOT NULL,
              name   TEXT NOT NULL,
              status TEXT NOT NULL,
              body   TEXT NOT NULL
            );
            CREATE INDEX items_kind ON items(kind, status);
            CREATE TABLE item_sources (
              item_id TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
              path    TEXT NOT NULL,
              line    INTEGER
            );
            CREATE INDEX item_sources_item ON item_sources(item_id);
            CREATE INDEX item_sources_path ON item_sources(path);
            CREATE TABLE scan_runs (
              id      TEXT PRIMARY KEY,
              started TEXT NOT NULL,
              ended   TEXT,
              timings TEXT
            );
            CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            ",
        ),
        // History (durable; kept when the cache is rebuilt).
        1 => conn.execute_batch(
            "
            CREATE TABLE actions (
              id      TEXT PRIMARY KEY,
              at      TEXT NOT NULL,
              kind    TEXT NOT NULL,
              status  TEXT NOT NULL,
              body    TEXT NOT NULL
            );
            CREATE INDEX actions_at ON actions(at);
            ",
        ),
        // Fix list dismissals: hidden while the evidence is unchanged.
        2 => conn.execute_batch(
            "CREATE TABLE fix_dismissals (fix_id TEXT PRIMARY KEY, evidence TEXT NOT NULL, at TEXT NOT NULL);",
        ),
        // Incremental transcript reading: per-file resume offsets and
        // what each file contributed, so a rescan reads only appended lines.
        3 => conn.execute_batch(
            "
            CREATE TABLE transcript_files (
              path   TEXT PRIMARY KEY,
              size   INTEGER NOT NULL,
              mtime  INTEGER NOT NULL,
              offset INTEGER NOT NULL
            );
            CREATE TABLE t_hook_runs (
              path    TEXT NOT NULL,
              event   TEXT NOT NULL,
              command TEXT NOT NULL,
              ms      INTEGER NOT NULL,
              outcome INTEGER NOT NULL,
              at      INTEGER
            );
            CREATE INDEX t_hook_runs_path ON t_hook_runs(path);
            CREATE TABLE t_mcp_calls (
              path   TEXT NOT NULL,
              server TEXT NOT NULL,
              count  INTEGER NOT NULL,
              last   INTEGER,
              PRIMARY KEY (path, server)
            );
            ",
        ),
        // Subagent and slash command runs from transcripts. Stamps are cleared
        // so every transcript is read once more for the new counts.
        4 => conn.execute_batch(
            "
            CREATE TABLE t_uses (
              path  TEXT NOT NULL,
              kind  TEXT NOT NULL,
              name  TEXT NOT NULL,
              count INTEGER NOT NULL,
              last  INTEGER,
              PRIMARY KEY (path, kind, name)
            );
            DELETE FROM transcript_files;
            DELETE FROM t_hook_runs;
            DELETE FROM t_mcp_calls;
            ",
        ),
        // Sessions of every agent and their masked prompt text (FTS5). Stamps
        // are cleared so every transcript is read once more for sessions.
        5 => conn.execute_batch(
            "
            CREATE TABLE sessions (
              id           TEXT PRIMARY KEY,
              origin       TEXT NOT NULL,
              native_id    TEXT NOT NULL,
              source       TEXT NOT NULL,
              title        TEXT,
              first_prompt TEXT,
              project      TEXT,
              started      INTEGER,
              ended        INTEGER,
              prompts      INTEGER NOT NULL
            );
            CREATE INDEX sessions_source ON sessions(source);
            CREATE INDEX sessions_origin ON sessions(origin);
            CREATE VIRTUAL TABLE sessions_fts USING fts5(session_id UNINDEXED, text, tokenize = 'unicode61 remove_diacritics 2');
            DELETE FROM transcript_files;
            DELETE FROM t_hook_runs;
            DELETE FROM t_mcp_calls;
            DELETE FROM t_uses;
            ",
        ),
        // Secret places: values only as salted hashes. Every
        // transcript is read once more for secrets; Claude sessions are
        // rebuilt by that read (merging into old rows would count twice).
        6 => conn.execute_batch(
            "
            CREATE TABLE secret_places (
              value_hash TEXT NOT NULL,
              kind       TEXT NOT NULL,
              confidence TEXT NOT NULL,
              prefix     TEXT NOT NULL,
              path       TEXT NOT NULL,
              place      TEXT NOT NULL,
              store      TEXT NOT NULL,
              origin     TEXT,
              chat       TEXT NOT NULL DEFAULT '',
              offset     INTEGER,
              first      INTEGER,
              last       INTEGER,
              count      INTEGER NOT NULL,
              PRIMARY KEY (value_hash, path, chat)
            );
            CREATE INDEX secret_places_path ON secret_places(path);
            CREATE INDEX secret_places_origin ON secret_places(origin);
            CREATE TABLE secret_files (path TEXT PRIMARY KEY, size INTEGER NOT NULL, mtime INTEGER NOT NULL);
            CREATE TABLE secret_ignored (value_hash TEXT PRIMARY KEY);
            DELETE FROM sessions_fts WHERE session_id IN (SELECT id FROM sessions WHERE origin = 'claude');
            DELETE FROM sessions WHERE origin = 'claude';
            DELETE FROM transcript_files;
            DELETE FROM t_hook_runs;
            DELETE FROM t_mcp_calls;
            DELETE FROM t_uses;
            DELETE FROM settings WHERE key = 'sessions.cursor_stamp';
            ",
        ),
        // Prompts per quarter hour (Dashboard activity). Transcripts are read
        // once more, so every row keyed by a transcript path is cleared first.
        7 => conn.execute_batch(
            "
            CREATE TABLE session_quarters (
              session_id TEXT NOT NULL,
              quarter    INTEGER NOT NULL,
              count      INTEGER NOT NULL,
              PRIMARY KEY (session_id, quarter)
            );
            DELETE FROM secret_places WHERE path IN (SELECT path FROM transcript_files);
            DELETE FROM sessions_fts WHERE session_id IN (SELECT id FROM sessions WHERE origin = 'claude');
            DELETE FROM sessions WHERE origin = 'claude';
            DELETE FROM transcript_files;
            DELETE FROM t_hook_runs;
            DELETE FROM t_mcp_calls;
            DELETE FROM t_uses;
            DELETE FROM settings WHERE key = 'sessions.cursor_stamp';
            ",
        ),
        // Marketplace sources seen in transcripts (restore steps for removed
        // plugins). Transcripts are read once more, so every row keyed by a
        // transcript path is cleared first.
        8 => conn.execute_batch(
            "
            CREATE TABLE t_market_sources (
              path   TEXT NOT NULL,
              name   TEXT NOT NULL,
              source TEXT NOT NULL,
              last   INTEGER,
              PRIMARY KEY (path, name, source)
            );
            DELETE FROM secret_places WHERE path IN (SELECT path FROM transcript_files);
            DELETE FROM session_quarters WHERE session_id IN (SELECT id FROM sessions WHERE origin = 'claude');
            DELETE FROM sessions_fts WHERE session_id IN (SELECT id FROM sessions WHERE origin = 'claude');
            DELETE FROM sessions WHERE origin = 'claude';
            DELETE FROM transcript_files;
            DELETE FROM t_hook_runs;
            DELETE FROM t_mcp_calls;
            DELETE FROM t_uses;
            DELETE FROM settings WHERE key = 'sessions.cursor_stamp';
            ",
        ),
        _ => unreachable!("no migration from schema {from}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_refuses_newer_user_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.sqlite");
        {
            let conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1).unwrap();
        }
        let err = crate::store::Store::open(&path).err().expect("must refuse");
        assert_eq!(err.kind, ErrorKind::Database);
        // Untouched: still the newer version.
        let conn = Connection::open(&path).unwrap();
        let v: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION + 1);
    }
}
