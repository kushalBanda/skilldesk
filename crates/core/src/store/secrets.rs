//! Secret places. A value is stored only as its salted hash, its kind and its
//! known prefix (ADR 0010).

use super::{db_err, Store};
use crate::error::{AppError, ErrorKind};
use crate::secrets::{value_hash, Confidence, Hit, PlaceStore, Secret, SecretKind, SecretPlace};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Places a secret card lists; the rest are counted.
const PLACES_SHOWN: usize = 200;

pub(super) fn salt(conn: &Connection) -> Result<String, AppError> {
    let have: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = 'secrets.salt'", [], |r| r.get(0))
        .optional()
        .map_err(db_err)?;
    if let Some(s) = have {
        return Ok(s);
    }
    use std::io::Read;
    let mut b = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .map_err(|e| AppError::new(ErrorKind::Io, format!("Cannot read random bytes: {e}")))?;
    let s = hex::encode(b);
    conn.execute("INSERT INTO settings (key, value) VALUES ('secrets.salt', ?1)", params![s]).map_err(db_err)?;
    Ok(s)
}

fn conf_str(c: Confidence) -> &'static str {
    match c {
        Confidence::Likely => "likely",
        Confidence::Possible => "possible",
    }
}

/// Adds hits of one place (one file, or one chat in an agent database).
#[allow(clippy::too_many_arguments)]
pub(super) fn put_hits(
    tx: &Transaction,
    salt: &str,
    path: &str,
    place: &str,
    store: PlaceStore,
    origin: Option<&str>,
    chat: &str,
    hits: &[Hit],
) -> Result<(), AppError> {
    if hits.is_empty() {
        return Ok(());
    }
    let mut st = tx
        .prepare_cached(
            "INSERT INTO secret_places (value_hash, kind, confidence, prefix, path, place, store, origin, chat, offset, first, last, count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11, 1)
             ON CONFLICT(value_hash, path, chat) DO UPDATE SET
               count = count + 1,
               confidence = CASE WHEN confidence = 'likely' OR excluded.confidence = 'likely' THEN 'likely' ELSE 'possible' END,
               offset = MIN(COALESCE(offset, excluded.offset), COALESCE(excluded.offset, offset)),
               first = MIN(COALESCE(first, excluded.first), COALESCE(excluded.first, first)),
               last = MAX(COALESCE(last, excluded.last), COALESCE(excluded.last, last))",
        )
        .map_err(db_err)?;
    let store_s = match store {
        PlaceStore::TextFile => "file",
        PlaceStore::AgentDb => "db",
    };
    for h in hits {
        let offset = (store == PlaceStore::TextFile).then_some(h.offset);
        st.execute(params![
            value_hash(salt, &h.value),
            h.kind.as_str(),
            conf_str(h.confidence),
            h.prefix,
            path,
            place,
            store_s,
            origin,
            chat,
            offset,
            h.seen,
        ])
        .map_err(db_err)?;
    }
    Ok(())
}

pub(super) fn delete_path(tx: &Transaction, path: &str) -> Result<(), AppError> {
    tx.execute("DELETE FROM secret_places WHERE path = ?1", params![path]).map_err(db_err)?;
    Ok(())
}

/// A text file the secrets pass read, with what it holds.
pub struct FileHits {
    pub path: String,
    pub place: &'static str,
    pub size: u64,
    pub mtime: i64,
    pub hits: Vec<Hit>,
}

impl Store {
    pub fn secret_file_stamps(&self) -> Result<HashMap<String, (u64, i64)>, AppError> {
        let conn = self.conn();
        let mut st = conn.prepare("SELECT path, size, mtime FROM secret_files").map_err(db_err)?;
        let rows = st
            .query_map([], |r| Ok((r.get::<_, String>(0)?, (r.get::<_, i64>(1)? as u64, r.get::<_, i64>(2)?))))
            .map_err(db_err)?;
        rows.collect::<Result<_, _>>().map_err(db_err)
    }

    /// Replaces the places of every changed file; drops files that are gone.
    pub fn apply_secret_files(&self, changed: &[FileHits], gone: &[String]) -> Result<(), AppError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(db_err)?;
        for p in gone {
            delete_path(&tx, p)?;
            tx.execute("DELETE FROM secret_files WHERE path = ?1", params![p]).map_err(db_err)?;
        }
        for f in changed {
            delete_path(&tx, &f.path)?;
            put_hits(&tx, &self.salt, &f.path, f.place, PlaceStore::TextFile, None, "", &f.hits)?;
            tx.execute(
                "INSERT INTO secret_files (path, size, mtime) VALUES (?1, ?2, ?3)
                 ON CONFLICT(path) DO UPDATE SET size = ?2, mtime = ?3",
                params![f.path, f.size as i64, f.mtime],
            )
            .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)
    }

    /// After SkillDesk rewrote these files: their places are found again now,
    /// and the next scan reads each one from the start.
    pub fn refresh_secret_paths(&self, files: &[(String, &'static str, Vec<Hit>)]) -> Result<(), AppError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(db_err)?;
        for (path, place, hits) in files {
            delete_path(&tx, path)?;
            put_hits(&tx, &self.salt, path, place, PlaceStore::TextFile, None, "", hits)?;
            tx.execute("DELETE FROM secret_files WHERE path = ?1", params![path]).map_err(db_err)?;
            tx.execute("UPDATE transcript_files SET mtime = 0, offset = 9223372036854775807 WHERE path = ?1", params![path])
                .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)
    }

    pub fn set_secret_ignored(&self, id: &str, on: bool) -> Result<(), AppError> {
        let conn = self.conn();
        if on {
            conn.execute("INSERT OR IGNORE INTO secret_ignored (value_hash) VALUES (?1)", params![id])
        } else {
            conn.execute("DELETE FROM secret_ignored WHERE value_hash = ?1", params![id])
        }
        .map(|_| ())
        .map_err(db_err)
    }

    /// Every place of one value (redact, reveal).
    pub fn secret_places(&self, id: &str) -> Result<Vec<SecretPlace>, AppError> {
        Ok(self.secrets_where(Some(id))?.into_iter().next().map(|s| s.places).unwrap_or_default())
    }

    /// Every value: likely first, then by kind, newest first.
    pub fn secrets(&self) -> Result<Vec<Secret>, AppError> {
        let mut out = self.secrets_where(None)?;
        for s in &mut out {
            s.places.truncate(PLACES_SHOWN);
        }
        Ok(out)
    }

    pub fn secret(&self, id: &str) -> Result<Option<Secret>, AppError> {
        Ok(self.secrets_where(Some(id))?.into_iter().next())
    }

    fn secrets_where(&self, id: Option<&str>) -> Result<Vec<Secret>, AppError> {
        let conn = self.conn();
        let ignored: HashSet<String> = {
            let mut st = conn.prepare("SELECT value_hash FROM secret_ignored").map_err(db_err)?;
            let rows = st.query_map([], |r| r.get::<_, String>(0)).map_err(db_err)?;
            rows.collect::<Result<_, _>>().map_err(db_err)?
        };
        let mut st = conn
            .prepare(
                "SELECT value_hash, kind, confidence, prefix, path, place, store, chat, offset, first, last, count
                 FROM secret_places WHERE ?1 IS NULL OR value_hash = ?1
                 ORDER BY value_hash, store DESC, last DESC, path",
            )
            .map_err(db_err)?;
        let rows = st
            .query_map(params![id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, String>(7)?,
                    r.get::<_, Option<i64>>(8)?,
                    r.get::<_, Option<i64>>(9)?,
                    r.get::<_, Option<i64>>(10)?,
                    r.get::<_, i64>(11)?,
                ))
            })
            .map_err(db_err)?;
        let mut by: BTreeMap<String, Secret> = BTreeMap::new();
        for row in rows {
            let (hash, kind, conf, prefix, path, place, store, chat, offset, first, last, count) = row.map_err(db_err)?;
            let Some(kind) = SecretKind::parse(&kind) else { continue };
            let s = by.entry(hash.clone()).or_insert_with(|| Secret {
                id: hash.clone(),
                kind,
                confidence: Confidence::Possible,
                masked: format!("{prefix}{}", crate::secrets::MASK),
                places: Vec::new(),
                place_count: 0,
                occurrences: 0,
                first_seen: None,
                last_seen: None,
                ignored: ignored.contains(&hash),
                rotate_at: kind.rotate_at().to_string(),
            });
            if conf == "likely" {
                s.confidence = Confidence::Likely;
            }
            s.place_count += 1;
            s.occurrences += count as u32;
            s.first_seen = match (s.first_seen, first) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            s.last_seen = s.last_seen.max(last);
            s.places.push(SecretPlace {
                path,
                offset,
                store: if store == "db" { PlaceStore::AgentDb } else { PlaceStore::TextFile },
                place,
                chat: (!chat.is_empty()).then_some(chat),
                count: count as u32,
            });
        }
        let mut out: Vec<Secret> = by.into_values().collect();
        out.sort_by(|a, b| {
            (a.confidence != Confidence::Likely)
                .cmp(&(b.confidence != Confidence::Likely))
                .then(a.kind.cmp(&b.kind))
                .then(b.last_seen.cmp(&a.last_seen))
        });
        Ok(out)
    }
}
