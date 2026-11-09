//! Redact a secret (architecture "Redact a secret"): every text file that
//! holds the value gets the value replaced by its mask. Each file is copied to
//! the Trash first (Undo puts it back), the result must still parse, and any
//! failure rolls back every file. Agent databases are never written (ADR
//! 0011); the plan lists them as not changed.

use super::{new_id, ActionPlan, ActionRequest, FileOp, MaskSpan, Untouched};
use crate::core::Core;
use crate::error::{AppError, ErrorKind};
use crate::model::AgentKey;
use crate::secrets::{detect, value_hash, PlaceStore};
use std::path::{Path, PathBuf};

fn invalid(msg: impl Into<String>) -> AppError {
    AppError::new(ErrorKind::InvalidInput, msg)
}

/// Agent whose history a place belongs to.
fn agent_of(place: &str) -> &'static str {
    match place {
        "codex" => "codex",
        "gemini" => "gemini",
        "cursor" | "cursor_editor" | "cursor_cli" => "cursor",
        "opencode" => "opencode",
        _ => "claude_code",
    }
}

/// JSON lines that parse, or 1 when a whole JSON file parses (0 if not).
fn parsed(path: &Path, text: &str) -> usize {
    match path.extension().and_then(|e| e.to_str()) {
        Some("jsonl") => text.lines().filter(|l| serde_json::from_str::<serde_json::Value>(l).is_ok()).count(),
        Some("json") => usize::from(serde_json::from_str::<serde_json::Value>(text).is_ok()),
        _ => 0,
    }
}

/// A free name in the Trash, also not used by an earlier file of this plan.
fn backup_path(core: &Core, path: &Path, taken: &mut Vec<PathBuf>) -> PathBuf {
    let trash = core.env.home.join(".Trash");
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    let mut n = 0;
    loop {
        let label = if n == 0 { format!("{name} (SkillDesk backup)") } else { format!("{name} (SkillDesk backup {n})") };
        let p = trash.join(label);
        if !taken.contains(&p) && std::fs::symlink_metadata(&p).is_err() {
            taken.push(p.clone());
            return p;
        }
        n += 1;
    }
}

pub fn redact(core: &Core, req: ActionRequest, id: &str) -> Result<ActionPlan, AppError> {
    let secret = core.store.secret(id)?.ok_or_else(|| AppError::new(ErrorKind::NotFound, "This value is no longer found. Scan again."))?;
    let mut ops = Vec::new();
    let mut untouched = Vec::new();
    let mut agents: Vec<AgentKey> = Vec::new();
    let mut taken = Vec::new();
    for p in &secret.places {
        if p.store == PlaceStore::AgentDb {
            untouched.push(Untouched {
                path: p.path.clone(),
                place: p.place.clone(),
                chat: p.chat.clone(),
                reason: "An agent database. SkillDesk never writes it; delete this chat in the agent.".into(),
            });
            continue;
        }
        let path = PathBuf::from(&p.path);
        if crate::secrets::scan::place_of(&core.env, &path).is_none() {
            untouched.push(Untouched { path: p.path.clone(), place: p.place.clone(), chat: None, reason: "Not a history file SkillDesk changes.".into() });
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            untouched.push(Untouched { path: p.path.clone(), place: p.place.clone(), chat: None, reason: "Cannot be read now.".into() });
            continue;
        };
        let spans: Vec<MaskSpan> = detect(&text)
            .into_iter()
            .filter(|f| value_hash(&core.store.salt, f.value(&text)) == id)
            .map(|f| MaskSpan { at: f.start, len: f.end - f.start, insert: f.replacement(&text) })
            .collect();
        if spans.is_empty() {
            continue;
        }
        let after = super::ops::apply_masks(&text, &spans).ok_or_else(|| AppError::new(ErrorKind::Internal, "Mask spans do not fit the file."))?;
        if parsed(&path, &after) != parsed(&path, &text) {
            return Err(invalid(format!("Hiding the value would break {}. Nothing was changed.", path.display())));
        }
        let agent = AgentKey(agent_of(&p.place).into());
        if !agents.contains(&agent) {
            agents.push(agent);
        }
        ops.push(FileOp::MaskText {
            backup: backup_path(core, &path, &mut taken),
            path,
            before_sha: super::sha(&text),
            after_sha: super::sha(&after),
            spans,
        });
    }
    if ops.is_empty() {
        return Err(invalid(if untouched.is_empty() {
            "No copy of this value is left in a text file. Scan again."
        } else {
            "Only agent databases hold this value. SkillDesk never writes them; delete those chats in the agent."
        }));
    }
    let n = ops.len();
    Ok(ActionPlan {
        id: new_id(),
        request: req,
        summary: format!("Hid {} in {n} {}", secret.masked, if n == 1 { "file" } else { "files" }),
        agents,
        ops,
        needs_confirm: true,
        removed: vec![],
        untouched,
    })
}

impl Core {
    /// After files were masked or put back: find their secrets again now, and
    /// make the next scan read them from the start.
    pub(crate) fn after_masking(&self, ops: &[FileOp]) -> Result<(), AppError> {
        let paths: Vec<&PathBuf> = ops
            .iter()
            .filter_map(|op| match op {
                FileOp::MaskText { path, .. } | FileOp::UnmaskText { path, .. } => Some(path),
                _ => None,
            })
            .collect();
        if paths.is_empty() {
            return Ok(());
        }
        let files: Vec<(String, &'static str, Vec<crate::secrets::Hit>)> = paths
            .into_iter()
            .filter_map(|p| {
                let place = crate::secrets::scan::place_of(&self.env, p)?;
                Some((p.to_string_lossy().into_owned(), place, crate::secrets::scan::file_hits(p)))
            })
            .collect();
        self.store.refresh_secret_paths(&files)
    }

    pub fn secrets(&self) -> Result<Vec<crate::secrets::Secret>, AppError> {
        self.store.secrets()
    }

    pub fn secret_ignore(&self, id: &str, on: bool) -> Result<(), AppError> {
        self.store.set_secret_ignored(id, on)
    }

    /// The raw value, read again from a text file that holds it. Only the
    /// app's reveal command calls this, after Touch ID or the password.
    pub fn secret_value(&self, id: &str) -> Result<String, AppError> {
        for p in self.store.secret_places(id)? {
            if p.store != PlaceStore::TextFile {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p.path) else { continue };
            if let Some(f) = detect(&text).into_iter().find(|f| value_hash(&self.store.salt, f.value(&text)) == id) {
                return Ok(f.value(&text).to_string());
            }
        }
        Err(AppError::new(ErrorKind::NotFound, "No text file holds this value now, so it cannot be shown."))
    }
}

#[cfg(test)]
mod tests {
    use crate::actions::ActionRequest;
    use crate::secrets::SecretKind;

    const KEY: &str = "sk-ant-api03-SECRETsecretSECRETsecret0123456789";
    // Split so secret scanners do not flag this fake test value.
    const STRIPE: &str = concat!("sk_li", "ve_9aZ4kQ7mW2xR8tY1vB6nC3pL");

    /// A home with the key in a transcript, a file-history copy and an
    /// OpenCode database; a Stripe key in a second transcript.
    fn home() -> (tempfile::TempDir, std::sync::Arc<crate::Core>) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let proj = home.join(".claude/projects/-tmp-demo");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::create_dir_all(home.join(".Trash")).unwrap();
        let line = |t: &str| serde_json::json!({"type":"user","message":{"role":"user","content":t},"timestamp":"2026-11-01T10:00:00Z"}).to_string();
        std::fs::write(proj.join("s1.jsonl"), format!("{}\n{}\n", line(&format!("deploy with {KEY} today")), line("thanks"))).unwrap();
        std::fs::write(proj.join("s2.jsonl"), format!("{}\n", line(&format!("pay with {STRIPE} and {KEY}")))).unwrap();
        let fh = home.join(".claude/file-history/abc");
        std::fs::create_dir_all(&fh).unwrap();
        std::fs::write(fh.join("env@v1"), format!("ANTHROPIC_API_KEY={KEY}\n")).unwrap();
        let oc = home.join(".local/share/opencode");
        std::fs::create_dir_all(&oc).unwrap();
        let c = rusqlite::Connection::open(oc.join("opencode.db")).unwrap();
        c.execute_batch(&format!(
            "CREATE TABLE session (id TEXT, title TEXT, directory TEXT, parent_id TEXT, time_created INTEGER, time_updated INTEGER);
             CREATE TABLE message (id TEXT, session_id TEXT, time_created INTEGER, data TEXT);
             CREATE TABLE part (id TEXT, message_id TEXT, session_id TEXT, data TEXT);
             INSERT INTO session VALUES ('ses1', 'Deploy', '/tmp', NULL, 1, 2);
             INSERT INTO message VALUES ('m1', 'ses1', 1, '{{\"role\":\"user\"}}');
             INSERT INTO part VALUES ('p1', 'm1', 'ses1', '{{\"type\":\"text\",\"text\":\"use {KEY} now\"}}');"
        ))
        .unwrap();
        drop(c);
        let core = crate::Core::open(crate::Env::for_test(&home, &dir.path().join("data"))).unwrap();
        crate::scan::run_scan(&core, &crate::scan::NoSink).unwrap();
        (dir, core)
    }

    fn files(core: &crate::Core) -> Vec<(std::path::PathBuf, Vec<u8>)> {
        let mut out: Vec<_> = walk(&core.env.home).into_iter().filter(|p| !p.starts_with(core.env.home.join(".Trash"))).map(|p| { let b = std::fs::read(&p).unwrap(); (p, b) }).collect();
        out.sort();
        out
    }

    fn walk(d: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut v = Vec::new();
        for e in std::fs::read_dir(d).unwrap().flatten() {
            if e.file_type().unwrap().is_dir() { v.extend(walk(&e.path())) } else { v.push(e.path()) }
        }
        v
    }

    #[test]
    fn secret_value_never_in_ipc_payload_and_agent_db_untouched() {
        let (dir, core) = home();
        let db = core.env.home.join(".local/share/opencode/opencode.db");
        let db_before = std::fs::read(&db).unwrap();
        let secrets = core.secrets().unwrap();
        let s = secrets.iter().find(|s| s.kind == SecretKind::Anthropic).expect("key found");
        assert_eq!(s.place_count, 4, "{:?}", s.places);
        let plan = core.action_plan(ActionRequest::SecretRedact { id: s.id.clone() }).unwrap();
        assert_eq!(plan.ops.len(), 3, "two transcripts and one file-history copy");
        assert_eq!(plan.untouched.len(), 1, "the OpenCode database is listed, not written");
        let record = core.action_apply(&plan.id).unwrap();
        let fixes = crate::rules::evaluate(&crate::rules::Inventory::load_for_test(&core));
        let payload = [
            serde_json::to_string(&secrets).unwrap(),
            serde_json::to_string(&plan.redacted()).unwrap(),
            serde_json::to_string(&record.redacted()).unwrap(),
            serde_json::to_string(&core.history().unwrap()).unwrap(),
            serde_json::to_string(&fixes).unwrap(),
            serde_json::to_string(&core.secrets().unwrap()).unwrap(),
        ]
        .join("\n");
        // SkillDesk's own database (and its WAL) holds no value either.
        let mut stored = Vec::new();
        for f in walk(&dir.path().join("data")) {
            stored.extend(std::fs::read(f).unwrap());
        }
        for v in [KEY, "SECRETsecretSECRET", STRIPE] {
            assert!(!payload.contains(v), "{v} in IPC payload");
            assert!(memchr::memmem::find(&stored, v.as_bytes()).is_none(), "{v} in SkillDesk's database");
        }
        // Gone from every text file; the Stripe key next to it stays.
        for (p, b) in files(&core) {
            if p != db {
                assert!(memchr::memmem::find(&b, KEY.as_bytes()).is_none(), "{} still holds the key", p.display());
            }
        }
        let s2 = std::fs::read_to_string(core.env.home.join(".claude/projects/-tmp-demo/s2.jsonl")).unwrap();
        assert!(s2.contains(STRIPE) && s2.contains("sk-ant-api03-••••"));
        assert!(s2.lines().all(|l| serde_json::from_str::<serde_json::Value>(l).is_ok()));
        assert_eq!(std::fs::read(&db).unwrap(), db_before, "agent database changed");
        // Only the database place is left; Undo puts every file back exactly.
        let left = core.store.secret(&s.id).unwrap().unwrap();
        assert!(left.places.iter().all(|p| p.store == crate::secrets::PlaceStore::AgentDb));
        core.action_undo(&record.id).unwrap();
        let s2_back = std::fs::read_to_string(core.env.home.join(".claude/projects/-tmp-demo/s2.jsonl")).unwrap();
        assert!(s2_back.contains(KEY));
        assert_eq!(core.store.secret(&s.id).unwrap().unwrap().place_count, 4);
    }

    #[test]
    fn secret_redact_rolls_back_all_on_one_failure() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, core) = home();
        let before = files(&core);
        let s = core.secrets().unwrap().into_iter().find(|s| s.kind == SecretKind::Anthropic).unwrap();
        let plan = core.action_plan(ActionRequest::SecretRedact { id: s.id.clone() }).unwrap();
        // The last file's folder turns read-only: its write fails after the
        // first files were already masked.
        let last = match plan.ops.last().unwrap() {
            crate::actions::FileOp::MaskText { path, .. } => path.parent().unwrap().to_path_buf(),
            _ => unreachable!(),
        };
        std::fs::set_permissions(&last, std::fs::Permissions::from_mode(0o500)).unwrap();
        let err = core.action_apply(&plan.id);
        std::fs::set_permissions(&last, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(err.is_err());
        assert_eq!(files(&core), before, "every file is back as it was");
        assert!(core.history().unwrap().is_empty(), "nothing recorded");
        assert!(walk(&core.env.home.join(".Trash")).is_empty(), "no backup copies left behind");
    }
}
