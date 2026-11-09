//! Every write goes through here (ADR 0006): plan → preflight → apply (re-check,
//! write, roll back on failure) → History → Undo.

mod hooks;
mod library;
mod mcp;
pub mod memory;
mod plugins;
mod secrets;
mod storage;
mod ops;
mod plan;
pub mod stash;

use crate::core::Core;
use crate::error::{AppError, ErrorKind};
use crate::model::{AgentKey, ItemId};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use ts_rs::TS;

pub use ops::{preflight, run_ops};
pub(crate) use plan::agents_reading;
pub(crate) use plugins::trash_target;

/// What the user asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum ActionRequest {
    SkillToggle { id: ItemId, agent: AgentKey, on: bool },
    HookSetEnabled { ids: Vec<ItemId>, on: bool },
    /// Take hooks out of their file for good (off hooks leave the stash).
    HookRemove { ids: Vec<ItemId> },
    /// Remove every link that points nowhere in these skills.
    LinksRemoveDangling { ids: Vec<ItemId> },
    /// Remove one link that points nowhere (one place of a skill).
    LinkRemove {
        id: ItemId,
        #[ts(type = "string")]
        path: PathBuf,
    },
    PluginSetEnabled { id: ItemId, on: bool },
    /// Drop a ghost plugin's stale `enabledPlugins` entry.
    PluginForget { id: ItemId },
    MarketplaceRemove { ids: Vec<ItemId> },
    CacheClean { ids: Vec<ItemId> },
    /// Switch one config entry (`McpLocation.entry`) of a server.
    McpSetEnabled { id: ItemId, entry: String, on: bool },
    McpCopyToAgent { id: ItemId, agent: AgentKey },
    /// Install a folder, `.zip` or `.skill` into the library and link it for these agents.
    LibraryInstall {
        #[ts(type = "string")]
        from: PathBuf,
        agents: Vec<AgentKey>,
    },
    /// Move a skill's own folder into the library and link it back.
    SkillAddToLibrary { id: ItemId },
    /// Unlink a library skill everywhere, then move its folder to the Trash.
    LibraryRemove { id: ItemId },
    /// Keep one copy as the library copy; every other copy goes to the Trash
    /// and every place links to the library.
    LibraryAdopt {
        id: ItemId,
        #[ts(type = "string")]
        keep: PathBuf,
    },
    /// Replace a memory file's whole text.
    MemoryEdit { id: ItemId, text: String },
    /// Move memories to the Trash and drop their `MEMORY.md` lines.
    MemoryForget { ids: Vec<ItemId> },
    /// Write `text` as one rule in each agent's global file, then forget the memories.
    MemoryPromote { ids: Vec<ItemId>, agents: Vec<AgentKey>, text: String },
    /// Write `text` as a rule in one agent's global file; the memory stays.
    MemoryCopyToAgent { id: ItemId, agent: AgentKey, text: String },
    /// Hide one secret value in every history text file that holds it.
    SecretRedact { id: String },
    /// Move cleanup candidates (`storage::TRASH_KINDS`) to the Trash. The
    /// paths are read from disk again when the plan is made.
    StorageTrash { kinds: Vec<crate::storage::CandidateKind> },
}

impl ActionRequest {
    pub fn kind(&self) -> &'static str {
        match self {
            ActionRequest::SkillToggle { .. } => "skill.toggle",
            ActionRequest::HookSetEnabled { on: true, .. } => "hook.enable",
            ActionRequest::HookSetEnabled { on: false, .. } => "hook.disable",
            ActionRequest::HookRemove { .. } => "hook.remove",
            ActionRequest::LinksRemoveDangling { .. } => "links.remove_dangling",
            ActionRequest::LinkRemove { .. } => "links.remove_one",
            ActionRequest::PluginSetEnabled { on: true, .. } => "plugin.enable",
            ActionRequest::PluginSetEnabled { on: false, .. } => "plugin.disable",
            ActionRequest::PluginForget { .. } => "plugin.forget",
            ActionRequest::MarketplaceRemove { .. } => "marketplace.remove",
            ActionRequest::CacheClean { .. } => "cache.clean",
            ActionRequest::McpSetEnabled { on: true, .. } => "mcp.enable",
            ActionRequest::McpSetEnabled { on: false, .. } => "mcp.disable",
            ActionRequest::McpCopyToAgent { .. } => "mcp.copy",
            ActionRequest::LibraryInstall { .. } => "library.install",
            ActionRequest::SkillAddToLibrary { .. } => "library.add",
            ActionRequest::LibraryRemove { .. } => "library.remove",
            ActionRequest::LibraryAdopt { .. } => "library.adopt",
            ActionRequest::MemoryEdit { .. } => "memory.edit",
            ActionRequest::MemoryForget { .. } => "memory.forget",
            ActionRequest::MemoryPromote { .. } => "memory.promote",
            ActionRequest::MemoryCopyToAgent { .. } => "memory.copy_to_agent",
            ActionRequest::SecretRedact { .. } => "secret.redact",
            ActionRequest::StorageTrash { .. } => "storage.trash",
        }
    }
}

/// One file-system step. Each has an exact inverse, which is how Undo works.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "op", rename_all = "snake_case")]
#[ts(export)]
pub enum FileOp {
    CreateSymlink {
        #[ts(type = "string")]
        link: PathBuf,
        #[ts(type = "string")]
        to: PathBuf,
    },
    RemoveSymlink {
        #[ts(type = "string")]
        link: PathBuf,
        #[ts(type = "string")]
        to: PathBuf,
    },
    MovePath {
        #[ts(type = "string")]
        from: PathBuf,
        #[ts(type = "string")]
        to: PathBuf,
    },
    /// Remove a link whose target is gone. `to` is the link text exactly as stored.
    UnlinkDangling {
        #[ts(type = "string")]
        link: PathBuf,
        #[ts(type = "string")]
        to: PathBuf,
    },
    /// Put a dangling link back (Undo of `UnlinkDangling`).
    RelinkDangling {
        #[ts(type = "string")]
        link: PathBuf,
        #[ts(type = "string")]
        to: PathBuf,
    },
    /// Byte-exact edit of a text file. Runs only when the file's hash is still
    /// `before_sha` ("" = file absent) and must produce `after_sha` ("" = delete).
    EditText {
        #[ts(type = "string")]
        path: PathBuf,
        before_sha: String,
        after_sha: String,
        splices: Vec<Splice>,
    },
    /// Hide spans of a text file. The file is first copied to `backup` (in
    /// the Trash); no hidden text is kept in History. Runs only when the file's
    /// hash is still `before_sha`, and must produce `after_sha`.
    MaskText {
        #[ts(type = "string")]
        path: PathBuf,
        #[ts(type = "string")]
        backup: PathBuf,
        before_sha: String,
        after_sha: String,
        spans: Vec<MaskSpan>,
    },
    /// Undo of `MaskText`: the backup moves back over the file, only when the
    /// file is still `before_sha` and the backup is `after_sha`.
    UnmaskText {
        #[ts(type = "string")]
        path: PathBuf,
        #[ts(type = "string")]
        backup: PathBuf,
        before_sha: String,
        after_sha: String,
        spans: Vec<MaskSpan>,
    },
}

/// Bytes `at..at + len` (in the file before the edit) become `insert` (a mask).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MaskSpan {
    #[ts(type = "number")]
    pub at: usize,
    #[ts(type = "number")]
    pub len: usize,
    pub insert: String,
}

/// Replace `remove` at byte `at` (in the file before the edit) with `insert`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Splice {
    #[ts(type = "number")]
    pub at: usize,
    pub remove: String,
    pub insert: String,
}

impl FileOp {
    /// Every path this op writes (for the watcher's self-write mute).
    pub fn paths(&self) -> Vec<PathBuf> {
        match self {
            FileOp::CreateSymlink { link, .. }
            | FileOp::RemoveSymlink { link, .. }
            | FileOp::UnlinkDangling { link, .. }
            | FileOp::RelinkDangling { link, .. } => vec![link.clone()],
            FileOp::MovePath { from, to } => vec![from.clone(), to.clone()],
            FileOp::EditText { path, .. } => vec![path.clone()],
            FileOp::MaskText { path, backup, .. } | FileOp::UnmaskText { path, backup, .. } => vec![path.clone(), backup.clone()],
        }
    }

    /// For the UI: edit text blanked (it can hold keys from config files);
    /// History in the database keeps the full bytes for Undo.
    pub fn redacted(&self) -> FileOp {
        match self {
            FileOp::EditText { path, before_sha, after_sha, splices } => FileOp::EditText {
                path: path.clone(),
                before_sha: before_sha.clone(),
                after_sha: after_sha.clone(),
                splices: splices
                    .iter()
                    .map(|s| Splice { at: s.at, remove: String::new(), insert: String::new() })
                    .collect(),
            },
            other => other.clone(),
        }
    }

    pub fn inverse(&self) -> FileOp {
        match self.clone() {
            FileOp::CreateSymlink { link, to } => FileOp::RemoveSymlink { link, to },
            FileOp::RemoveSymlink { link, to } => FileOp::CreateSymlink { link, to },
            FileOp::MovePath { from, to } => FileOp::MovePath { from: to, to: from },
            FileOp::UnlinkDangling { link, to } => FileOp::RelinkDangling { link, to },
            FileOp::RelinkDangling { link, to } => FileOp::UnlinkDangling { link, to },
            FileOp::EditText { path, before_sha, after_sha, splices } => {
                // Positions shift by the size change of every earlier splice.
                let mut delta: isize = 0;
                let inv = splices
                    .iter()
                    .map(|s| {
                        let at = (s.at as isize + delta) as usize;
                        delta += s.insert.len() as isize - s.remove.len() as isize;
                        Splice { at, remove: s.insert.clone(), insert: s.remove.clone() }
                    })
                    .collect();
                FileOp::EditText { path, before_sha: after_sha, after_sha: before_sha, splices: inv }
            }
            FileOp::MaskText { path, backup, before_sha, after_sha, spans } => {
                FileOp::UnmaskText { path, backup, before_sha: after_sha, after_sha: before_sha, spans }
            }
            FileOp::UnmaskText { path, backup, before_sha, after_sha, spans } => {
                FileOp::MaskText { path, backup, before_sha: after_sha, after_sha: before_sha, spans }
            }
        }
    }

    /// The edit that turns `before` into `after` (`None` = no file), as one
    /// splice around the changed middle.
    pub fn edit_text(path: PathBuf, before: Option<&str>, after: Option<&str>) -> FileOp {
        let (b, a) = (before.unwrap_or(""), after.unwrap_or(""));
        let mut pre = b.bytes().zip(a.bytes()).take_while(|(x, y)| x == y).count();
        while !b.is_char_boundary(pre) || !a.is_char_boundary(pre) {
            pre -= 1;
        }
        let max_suf = (b.len() - pre).min(a.len() - pre);
        let mut suf = b.bytes().rev().zip(a.bytes().rev()).take(max_suf).take_while(|(x, y)| x == y).count();
        while !b.is_char_boundary(b.len() - suf) || !a.is_char_boundary(a.len() - suf) {
            suf -= 1;
        }
        let splices = if before == after {
            Vec::new()
        } else {
            vec![Splice { at: pre, remove: b[pre..b.len() - suf].to_string(), insert: a[pre..a.len() - suf].to_string() }]
        };
        FileOp::EditText {
            path,
            before_sha: before.map(sha).unwrap_or_default(),
            after_sha: after.map(sha).unwrap_or_default(),
            splices,
        }
    }
}

pub fn sha(text: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(text.as_bytes()))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ActionPlan {
    pub id: String,
    pub request: ActionRequest,
    pub summary: String,
    /// Every agent whose files change, including agents sharing a folder.
    pub agents: Vec<AgentKey>,
    pub ops: Vec<FileOp>,
    /// True when the change reaches beyond the one agent clicked, or moves a real folder.
    pub needs_confirm: bool,
    /// Folders this change moves to the Trash (Replace preview: "These will be removed").
    pub removed: Vec<Removal>,
    /// Places this change does not write (agent databases), so the user can
    /// clear them in the agent.
    #[serde(default)]
    pub untouched: Vec<Untouched>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Untouched {
    pub path: String,
    pub place: String,
    pub chat: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Removal {
    #[ts(type = "string")]
    pub path: PathBuf,
    pub files: u32,
    #[ts(type = "number")]
    pub bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ActionStatus {
    Applied,
    Undone,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ActionRecord {
    pub id: String,
    /// RFC 3339.
    pub at: String,
    pub kind: String,
    pub summary: String,
    pub agents: Vec<AgentKey>,
    pub status: ActionStatus,
    pub ops: Vec<FileOp>,
}

impl ActionPlan {
    pub fn redacted(&self) -> ActionPlan {
        ActionPlan { ops: self.ops.iter().map(FileOp::redacted).collect(), ..self.clone() }
    }
}

impl ActionRecord {
    pub fn redacted(&self) -> ActionRecord {
        ActionRecord { ops: self.ops.iter().map(FileOp::redacted).collect(), ..self.clone() }
    }
}

const PLAN_TTL: Duration = Duration::from_secs(600);

impl Core {
    pub fn action_plan(&self, req: ActionRequest) -> Result<ActionPlan, AppError> {
        let plan = plan::build(self, req)?;
        let conflicts = preflight(&self.env, &plan.ops);
        if !conflicts.is_empty() {
            return Err(ops::conflict_error(conflicts));
        }
        let mut plans = self.plans.lock().unwrap_or_else(|p| p.into_inner());
        plans.retain(|_, (at, _)| at.elapsed() < PLAN_TTL);
        plans.insert(plan.id.clone(), (Instant::now(), plan.clone()));
        Ok(plan)
    }

    /// Drops a plan the User did not apply, with its staged copy (SkillDesk's
    /// own scratch folder; agent files are never touched by a plan).
    pub fn action_discard(&self, plan_id: &str) {
        self.plans.lock().unwrap_or_else(|p| p.into_inner()).remove(plan_id);
        let staging = self.env.skilldesk_dir().join("staging");
        if plan_id.starts_with("act-") && !plan_id.contains(['/', '.']) {
            let _ = std::fs::remove_dir_all(staging.join(plan_id));
            let _ = std::fs::remove_dir(&staging); // only when empty
        }
    }

    /// Re-checks the disk, writes, records History. On any failure every step
    /// already taken is reversed and nothing is recorded.
    pub fn action_apply(&self, plan_id: &str) -> Result<ActionRecord, AppError> {
        let _w = self.write_guard()?;
        let plan = self
            .plans
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(plan_id)
            .map(|(_, p)| p)
            .ok_or_else(|| AppError::new(ErrorKind::NotFound, "This change expired. Try again."))?;
        self.mute(plan.ops.iter().flat_map(FileOp::paths));
        run_ops(&self.env, &plan.ops)?;
        self.after_masking(&plan.ops)?;
        self.after_cleanup(plan.request.kind());
        let record = ActionRecord {
            id: plan.id.clone(),
            at: chrono::Utc::now().to_rfc3339(),
            kind: plan.request.kind().to_string(),
            summary: plan.summary.clone(),
            agents: plan.agents.clone(),
            status: ActionStatus::Applied,
            ops: plan.ops.clone(),
        };
        self.store.insert_action(&record)?;
        Ok(record)
    }

    /// Reverses an applied action, only if the disk still looks exactly as the
    /// action left it. Otherwise `TargetConflict` and nothing is touched.
    pub fn action_undo(&self, id: &str) -> Result<ActionRecord, AppError> {
        let _w = self.write_guard()?;
        let mut record = self
            .store
            .action(id)?
            .ok_or_else(|| AppError::new(ErrorKind::NotFound, "No such change in History."))?;
        if record.status == ActionStatus::Undone {
            return Err(AppError::new(ErrorKind::InvalidInput, "This change was already undone."));
        }
        let inverse: Vec<FileOp> = record.ops.iter().rev().map(FileOp::inverse).collect();
        self.mute(inverse.iter().flat_map(FileOp::paths));
        run_ops(&self.env, &inverse)?;
        self.after_masking(&inverse)?;
        self.after_cleanup(&record.kind);
        self.store.set_action_status(id, ActionStatus::Undone)?;
        record.status = ActionStatus::Undone;
        Ok(record)
    }

    /// Writes that change disk use get a new measurement on the next rescan
    /// (after the inventory is read again).
    fn after_cleanup(&self, kind: &str) {
        if matches!(kind, "storage.trash" | "marketplace.remove" | "cache.clean") {
            self.storage_dirty.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    pub fn history(&self) -> Result<Vec<ActionRecord>, AppError> {
        self.store.actions()
    }
}

pub(crate) fn new_id() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    format!(
        "act-{}-{}",
        chrono::Utc::now().timestamp_millis(),
        N.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::Env;

    #[test]
    fn apply_rechecks_target_changed_after_plan() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().canonicalize().unwrap();
        let core = Core::open(Env::for_test(&home, &home.join("data"))).unwrap();
        let src = home.join("lib/skill-a");
        std::fs::create_dir_all(&src).unwrap();
        let link = home.join(".cursor/skills/skill-a");
        let plan = ActionPlan {
            id: new_id(),
            request: ActionRequest::SkillToggle { id: ItemId("x".into()), agent: AgentKey("cursor".into()), on: true },
            summary: "test".into(),
            agents: vec![],
            ops: vec![FileOp::CreateSymlink { link: link.clone(), to: src }],
            needs_confirm: false,
            removed: vec![],
            untouched: vec![],
        };
        core.plans.lock().unwrap().insert(plan.id.clone(), (Instant::now(), plan.clone()));
        // Between plan and apply, the user puts their own folder there.
        std::fs::create_dir_all(&link).unwrap();
        std::fs::write(link.join("mine.txt"), "user").unwrap();
        let err = core.action_apply(&plan.id).unwrap_err();
        assert_eq!(err.kind, ErrorKind::TargetConflict);
        assert_eq!(std::fs::read_to_string(link.join("mine.txt")).unwrap(), "user");
        assert!(core.history().unwrap().is_empty(), "nothing recorded");
    }
}
