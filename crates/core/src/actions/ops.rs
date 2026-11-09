//! Preflight and execution of `FileOp`s. Refusals are errors, never `Ok`
//! (REVIEW failure class 2).

use super::FileOp;
use crate::env::Env;
use crate::error::{AppError, ErrorDetails, ErrorKind, TargetConflictDetail};
use crate::fsx::path_guard::is_path_safe;
use crate::fsx::target::{classify_target, TargetState};
use std::path::Path;

fn conflict(path: &Path, reason: impl Into<String>) -> TargetConflictDetail {
    TargetConflictDetail { path: path.display().to_string(), reason: reason.into() }
}

pub fn conflict_error(conflicts: Vec<TargetConflictDetail>) -> AppError {
    let first = conflicts.first().map(|c| format!("{} {}", c.path, c.reason)).unwrap_or_default();
    AppError {
        kind: ErrorKind::TargetConflict,
        message: format!("Nothing was changed: {first}."),
        details: Some(ErrorDetails { conflicts }),
    }
}

/// Writes may only land inside the user's home (where every agent keeps its files).
fn inside_home(env: &Env, path: &Path) -> bool {
    path.parent().is_some_and(|parent| is_path_safe(&env.home, parent))
}

/// What earlier ops in the same batch leave at a path, so preflight can judge a
/// batch that moves a folder in and then links to it (or clears a spot first).
#[derive(Default)]
struct Sim(Vec<(std::path::PathBuf, Option<Planned>)>);

#[derive(Clone)]
enum Planned {
    Dir,
    Link(std::path::PathBuf),
}

impl Sim {
    /// `Some(None)` = gone; `Some(Some(_))` = put there; `None` = not touched.
    fn at(&self, p: &Path) -> Option<Option<Planned>> {
        self.0.iter().rev().find(|(q, _)| q == p).map(|(_, s)| s.clone())
    }

    /// A path inside a folder an earlier op moves: judged when it runs.
    fn under_moved(&self, p: &Path) -> bool {
        self.0.iter().any(|(q, s)| matches!(s, Some(Planned::Dir)) && p != q && p.starts_with(q))
    }

    fn apply(&mut self, op: &FileOp) {
        match op {
            FileOp::CreateSymlink { link, to } | FileOp::RelinkDangling { link, to } => self.0.push((link.clone(), Some(Planned::Link(to.clone())))),
            FileOp::RemoveSymlink { link, .. } | FileOp::UnlinkDangling { link, .. } => self.0.push((link.clone(), None)),
            FileOp::MovePath { from, to } => {
                self.0.push((from.clone(), None));
                self.0.push((to.clone(), Some(Planned::Dir)));
            }
            FileOp::EditText { .. } | FileOp::MaskText { .. } | FileOp::UnmaskText { .. } => {}
        }
    }

    fn state(&self, p: &Path, src: Option<&Path>) -> std::io::Result<TargetState> {
        match self.at(p) {
            Some(None) => Ok(TargetState::Absent),
            Some(Some(Planned::Dir)) => Ok(TargetState::RealDir),
            Some(Some(Planned::Link(t))) => Ok(if src == Some(t.as_path()) { TargetState::LinkToSource } else { TargetState::ForeignLink }),
            None => classify_target(p, src),
        }
    }

    fn is_dir(&self, p: &Path) -> bool {
        match self.at(p) {
            Some(None) => false,
            Some(Some(_)) => true,
            None => p.is_dir(),
        }
    }
}

fn check(env: &Env, op: &FileOp) -> Option<TargetConflictDetail> {
    check_in(env, op, &Sim::default())
}

fn check_in(env: &Env, op: &FileOp, sim: &Sim) -> Option<TargetConflictDetail> {
    let state = |p: &Path, src: Option<&Path>| sim.state(p, src);
    match op {
        FileOp::CreateSymlink { link, to } => {
            if !inside_home(env, link) {
                return Some(conflict(link, "is outside the home folder"));
            }
            if !sim.is_dir(to) {
                return Some(conflict(to, "is gone, so there is nothing to link to"));
            }
            match state(link, Some(to)) {
                Ok(TargetState::Absent | TargetState::LinkToSource) => None,
                Ok(TargetState::RealDir) => Some(conflict(link, "already holds a folder that is not a link")),
                Ok(TargetState::RealFile) => Some(conflict(link, "already holds a file")),
                Ok(TargetState::ForeignLink) => Some(conflict(link, "already holds a link to somewhere else")),
                Err(e) => Some(conflict(link, format!("cannot be inspected ({e})"))),
            }
        }
        FileOp::RemoveSymlink { link, to } => match state(link, Some(to)) {
            Ok(TargetState::LinkToSource) if inside_home(env, link) => None,
            Ok(TargetState::LinkToSource) => Some(conflict(link, "is outside the home folder")),
            Ok(TargetState::Absent) => Some(conflict(link, "is no longer there")),
            Ok(_) => Some(conflict(link, "changed since and is no longer the expected link")),
            Err(e) => Some(conflict(link, format!("cannot be inspected ({e})"))),
        },
        FileOp::MovePath { from, to } => {
            if !inside_home(env, from) || !inside_home(env, to) {
                return Some(conflict(from, "is outside the home folder"));
            }
            match (state(from, None), state(to, None)) {
                (Ok(TargetState::RealDir | TargetState::RealFile), Ok(TargetState::Absent)) => None,
                (Ok(TargetState::RealDir | TargetState::RealFile), Ok(_)) => Some(conflict(to, "is already taken")),
                (Ok(TargetState::Absent), _) => Some(conflict(from, "is no longer there")),
                (Ok(_), _) => Some(conflict(from, "is no longer a plain file or folder")),
                (Err(e), _) => Some(conflict(from, format!("cannot be inspected ({e})"))),
            }
        }
        FileOp::UnlinkDangling { link, to } => {
            if !inside_home(env, link) {
                return Some(conflict(link, "is outside the home folder"));
            }
            if let Some(s) = sim.at(link) {
                return match s {
                    Some(Planned::Link(t)) if &t == to => None,
                    _ => Some(conflict(link, "is no longer the same broken link")),
                };
            }
            match std::fs::read_link(link) {
                Ok(t) if &t == to && !link.exists() => None,
                Ok(_) => Some(conflict(link, "changed since and is no longer the same broken link")),
                Err(_) => Some(conflict(link, "is no longer there")),
            }
        }
        FileOp::RelinkDangling { link, .. } => {
            if !inside_home(env, link) {
                return Some(conflict(link, "is outside the home folder"));
            }
            if let Some(s) = sim.at(link) {
                return s.map(|_| conflict(link, "is taken by something new"));
            }
            match std::fs::symlink_metadata(link) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                _ => Some(conflict(link, "is taken by something new")),
            }
        }
        FileOp::EditText { path, before_sha, .. } => {
            if !inside_home(env, path) {
                return Some(conflict(path, "is outside the home folder"));
            }
            if sim.under_moved(path) {
                return None;
            }
            match current_sha(path) {
                Ok(cur) if &cur == before_sha => None,
                Ok(_) if before_sha.is_empty() => Some(conflict(path, "now exists")),
                Ok(_) => Some(conflict(path, "changed since. Nothing was overwritten")),
                Err(e) => Some(conflict(path, format!("cannot be read ({e})"))),
            }
        }
        FileOp::MaskText { path, backup, before_sha, .. } => {
            if !inside_home(env, path) || !inside_home(env, backup) {
                return Some(conflict(path, "is outside the home folder"));
            }
            if std::fs::symlink_metadata(backup).is_ok() {
                return Some(conflict(backup, "is already taken"));
            }
            match current_sha(path) {
                Ok(cur) if &cur == before_sha && !cur.is_empty() => None,
                Ok(cur) if cur.is_empty() => Some(conflict(path, "is no longer there")),
                Ok(_) => Some(conflict(path, "changed since. Nothing was overwritten")),
                Err(e) => Some(conflict(path, format!("cannot be read ({e})"))),
            }
        }
        FileOp::UnmaskText { path, backup, before_sha, after_sha, .. } => {
            if !inside_home(env, path) || !inside_home(env, backup) {
                return Some(conflict(path, "is outside the home folder"));
            }
            match (current_sha(path), current_sha(backup)) {
                (Ok(cur), _) if &cur != before_sha => Some(conflict(path, "changed since. Nothing was overwritten")),
                (_, Ok(b)) if b.is_empty() => Some(conflict(backup, "is gone from the Trash, so the original cannot come back")),
                (_, Ok(b)) if &b != after_sha => Some(conflict(backup, "changed since")),
                (Err(e), _) | (_, Err(e)) => Some(conflict(path, format!("cannot be read ({e})"))),
                _ => None,
            }
        }
    }
}

/// Applies mask spans (ascending, positions in `text`).
pub(crate) fn apply_masks(text: &str, spans: &[super::MaskSpan]) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    for s in spans {
        if s.at < pos {
            return None;
        }
        out.push_str(text.get(pos..s.at)?);
        text.get(s.at..s.at + s.len)?;
        out.push_str(&s.insert);
        pos = s.at + s.len;
    }
    out.push_str(text.get(pos..)?);
    Some(out)
}

/// sha256 of the file's text; "" when absent.
fn current_sha(path: &Path) -> std::io::Result<String> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e),
        Ok(m) if !m.is_file() => Err(std::io::Error::other("not a plain file")),
        Ok(_) => Ok(super::sha(&std::fs::read_to_string(path)?)),
    }
}

/// Applies splices (positions in `text`, ascending) and checks each removed
/// span still reads as expected.
pub(crate) fn apply_splices(text: &str, splices: &[super::Splice]) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    for s in splices {
        if s.at < pos || text.get(s.at..s.at + s.remove.len())? != s.remove {
            return None;
        }
        out.push_str(text.get(pos..s.at)?);
        out.push_str(&s.insert);
        pos = s.at + s.remove.len();
    }
    out.push_str(text.get(pos..)?);
    Some(out)
}

/// Same-folder temp file + rename, keeping the file's permissions.
fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().ok_or_else(|| std::io::Error::other("no parent folder"))?;
    std::fs::create_dir_all(dir)?;
    let perms = std::fs::metadata(path).ok().map(|m| m.permissions());
    let tmp = dir.join(format!(".{}.skilldesk-tmp", path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
    }
    if let Some(p) = perms {
        std::fs::set_permissions(&tmp, p)?;
    }
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Every conflict in the batch. Empty = the whole batch may run.
pub fn preflight(env: &Env, ops: &[FileOp]) -> Vec<TargetConflictDetail> {
    let mut sim = Sim::default();
    ops.iter()
        .filter_map(|op| {
            let c = check_in(env, op, &sim);
            sim.apply(op);
            c
        })
        .collect()
}

fn exec(env: &Env, op: &FileOp) -> Result<(), AppError> {
    // Re-check immediately before touching anything: the disk can change between ops.
    if let Some(c) = check(env, op) {
        return Err(conflict_error(vec![c]));
    }
    let io = |what: &str, p: &Path, e: std::io::Error| {
        AppError::new(ErrorKind::Io, format!("Cannot {what} {}: {e}", p.display()))
    };
    match op {
        FileOp::CreateSymlink { link, to } => {
            if classify_target(link, Some(to)).ok() == Some(TargetState::LinkToSource) {
                return Ok(());
            }
            if let Some(parent) = link.parent() {
                std::fs::create_dir_all(parent).map_err(|e| io("create", parent, e))?;
            }
            std::os::unix::fs::symlink(to, link).map_err(|e| io("create link", link, e))
        }
        // `remove_file` on a symlink unlinks it; it never follows it.
        FileOp::RemoveSymlink { link, .. } => std::fs::remove_file(link).map_err(|e| io("remove link", link, e)),
        FileOp::MovePath { from, to } => {
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| io("create", parent, e))?;
            }
            std::fs::rename(from, to).map_err(|e| io("move", from, e))
        }
        FileOp::UnlinkDangling { link, .. } => std::fs::remove_file(link).map_err(|e| io("remove link", link, e)),
        FileOp::RelinkDangling { link, to } => {
            std::os::unix::fs::symlink(to, link).map_err(|e| io("create link", link, e))
        }
        FileOp::EditText { path, after_sha, splices, .. } => {
            let before = match std::fs::read_to_string(path) {
                Ok(t) => t,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
                Err(e) => return Err(io("read", path, e)),
            };
            let after = apply_splices(&before, splices).ok_or_else(|| {
                conflict_error(vec![conflict(path, "changed since. Nothing was overwritten")])
            })?;
            if after_sha.is_empty() {
                if !after.is_empty() {
                    return Err(AppError::new(ErrorKind::Internal, format!("Edit of {} would not leave it empty.", path.display())));
                }
                return std::fs::remove_file(path).map_err(|e| io("remove", path, e));
            }
            if &super::sha(&after) != after_sha {
                return Err(AppError::new(ErrorKind::Internal, format!("Edit of {} did not give the planned result.", path.display())));
            }
            write_atomic(path, &after).map_err(|e| io("write", path, e))
        }
        FileOp::MaskText { path, backup, after_sha, spans, .. } => {
            let before = std::fs::read_to_string(path).map_err(|e| io("read", path, e))?;
            let after = apply_masks(&before, spans)
                .ok_or_else(|| conflict_error(vec![conflict(path, "changed since. Nothing was overwritten")]))?;
            if &super::sha(&after) != after_sha {
                return Err(AppError::new(ErrorKind::Internal, format!("Edit of {} did not give the planned result.", path.display())));
            }
            if let Some(dir) = backup.parent() {
                std::fs::create_dir_all(dir).map_err(|e| io("create", dir, e))?;
            }
            std::fs::copy(path, backup).map_err(|e| io("back up", path, e))?;
            // The file is untouched when the write fails: drop the copy.
            write_atomic(path, &after).map_err(|e| {
                let _ = std::fs::remove_file(backup);
                io("write", path, e)
            })?;
            // Verify: what is on disk now is exactly the planned text.
            match current_sha(path) {
                Ok(cur) if &cur == after_sha => Ok(()),
                _ => {
                    let _ = std::fs::rename(backup, path);
                    Err(AppError::new(ErrorKind::Internal, format!("{} did not read back as written; the original is back.", path.display())))
                }
            }
        }
        FileOp::UnmaskText { path, backup, after_sha, .. } => {
            std::fs::rename(backup, path).map_err(|e| io("restore", path, e))?;
            match current_sha(path) {
                Ok(cur) if &cur == after_sha => Ok(()),
                _ => Err(AppError::new(ErrorKind::Internal, format!("{} did not read back as the original.", path.display()))),
            }
        }
    }
}

/// Preflight the whole batch, then run it. If a step fails, the steps already
/// done are reversed (newest first) and the original error is returned.
pub fn run_ops(env: &Env, ops: &[FileOp]) -> Result<(), AppError> {
    let conflicts = preflight(env, ops);
    if !conflicts.is_empty() {
        return Err(conflict_error(conflicts));
    }
    for (i, op) in ops.iter().enumerate() {
        if let Err(err) = exec(env, op) {
            let mut failed_rollback = Vec::new();
            for done in ops[..i].iter().rev() {
                if let Err(e) = exec(env, &done.inverse()) {
                    failed_rollback.push(e.message);
                }
            }
            if failed_rollback.is_empty() {
                return Err(err);
            }
            return Err(AppError::new(
                err.kind,
                format!("{} Rolling back also failed: {}", err.message, failed_rollback.join("; ")),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup() -> (tempfile::TempDir, Env) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().canonicalize().unwrap();
        let env = Env::for_test(&home, &home.join("data"));
        fs::create_dir_all(home.join("lib/skill-a")).unwrap();
        fs::write(home.join("lib/skill-a/SKILL.md"), "a").unwrap();
        (tmp, env)
    }

    #[test]
    fn preflight_refuses_whole_batch_on_one_conflict() {
        let (_t, env) = setup();
        let src = env.home.join("lib/skill-a");
        let occupied = env.home.join("agent2/skill-a");
        fs::create_dir_all(&occupied).unwrap();
        fs::write(occupied.join("mine.txt"), "user").unwrap();
        let ops = vec![
            FileOp::CreateSymlink { link: env.home.join("agent1/skill-a"), to: src.clone() },
            FileOp::CreateSymlink { link: occupied.clone(), to: src },
        ];
        let err = run_ops(&env, &ops).unwrap_err();
        assert_eq!(err.kind, ErrorKind::TargetConflict);
        assert!(fs::symlink_metadata(env.home.join("agent1/skill-a")).is_err(), "first op must not run");
        assert_eq!(fs::read_to_string(occupied.join("mine.txt")).unwrap(), "user");
    }

    #[test]
    fn apply_failure_mid_plan_rolls_back_all_files() {
        let (_t, env) = setup();
        let src = env.home.join("lib/skill-a");
        let link = env.home.join("agent1/skill-a");
        let real = env.home.join("agent2/real");
        fs::create_dir_all(&real).unwrap();
        // Parent of the move target is a file: passes preflight, fails at exec.
        fs::write(env.home.join("blocker"), "x").unwrap();
        let ops = vec![
            FileOp::CreateSymlink { link: link.clone(), to: src },
            FileOp::MovePath { from: real.clone(), to: env.home.join("blocker/real") },
        ];
        assert!(run_ops(&env, &ops).is_err());
        assert!(fs::symlink_metadata(&link).is_err(), "link from step 1 rolled back");
        assert!(real.is_dir(), "folder untouched");
    }

    #[test]
    fn undo_refuses_when_file_changed_since() {
        let (_t, env) = setup();
        let src = env.home.join("lib/skill-a");
        let link = env.home.join("agent1/skill-a");
        let ops = vec![FileOp::CreateSymlink { link: link.clone(), to: src }];
        run_ops(&env, &ops).unwrap();
        // The user replaces our link with their own folder.
        fs::remove_file(&link).unwrap();
        fs::create_dir_all(&link).unwrap();
        fs::write(link.join("mine.txt"), "user").unwrap();
        let inverse: Vec<_> = ops.iter().rev().map(FileOp::inverse).collect();
        let err = run_ops(&env, &inverse).unwrap_err();
        assert_eq!(err.kind, ErrorKind::TargetConflict);
        assert_eq!(fs::read_to_string(link.join("mine.txt")).unwrap(), "user");
    }
}
