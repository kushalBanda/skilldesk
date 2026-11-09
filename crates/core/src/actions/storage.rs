//! Cleanup: candidates move to the Trash (`MovePath`), so Undo puts each
//! one back. The paths come from disk at plan time, never from the UI.

use super::plugins::trash_target;
use super::{new_id, ActionPlan, ActionRequest, FileOp};
use crate::core::Core;
use crate::error::{AppError, ErrorKind};
use crate::model::AgentKey;
use crate::storage::{self, CandidateKind};
use std::path::PathBuf;

fn size_text(bytes: u64) -> String {
    let mb = bytes as f64 / 1_048_576.0;
    if mb >= 1024.0 {
        format!("{:.2} GB", mb / 1024.0)
    } else if mb >= 10.0 {
        format!("{mb:.0} MB")
    } else {
        format!("{mb:.1} MB")
    }
}

fn words(kind: CandidateKind, paths: &[PathBuf]) -> String {
    let n = |one: &str, many: &str, count: usize| if count == 1 { format!("1 {one}") } else { format!("{count} {many}") };
    match kind {
        CandidateKind::CursorBackup => "Cursor's backup database".into(),
        CandidateKind::ClaudeJobs => n("finished Claude job", "finished Claude jobs", paths.len()),
        CandidateKind::OrphanProjects => n("folder of a gone project", "folders of gone projects", paths.len()),
        CandidateKind::OldTranscripts => {
            let files = paths.iter().filter(|p| p.extension().is_some_and(|e| e == "jsonl")).count();
            n("old Claude transcript", "old Claude transcripts", files)
        }
        CandidateKind::CodexTmp => "Codex's download area".into(),
        CandidateKind::CursorLogs => n("old Cursor log folder", "old Cursor log folders", paths.len()),
        CandidateKind::Marketplaces | CandidateKind::PluginCache => String::new(),
    }
}

pub fn trash(core: &Core, req: ActionRequest, kinds: &[CandidateKind]) -> Result<ActionPlan, AppError> {
    let mut ops = Vec::new();
    let mut taken: Vec<PathBuf> = Vec::new();
    let mut parts = Vec::new();
    let mut agents: Vec<AgentKey> = Vec::new();
    let mut bytes = 0;
    for &kind in kinds {
        if !storage::TRASH_KINDS.contains(&kind) {
            return Err(AppError::new(ErrorKind::InvalidInput, "Use the Plugins screen for this one."));
        }
        let paths = storage::trash_paths(&core.env, kind);
        if paths.is_empty() {
            continue;
        }
        for p in &paths {
            bytes += storage::size_of(p);
            let mut to = trash_target(core, p);
            while taken.contains(&to) {
                to = to.with_file_name(format!("{} {}", to.file_name().unwrap_or_default().to_string_lossy(), taken.len()));
            }
            taken.push(to.clone());
            ops.push(FileOp::MovePath { from: p.clone(), to });
        }
        parts.push(words(kind, &paths));
        let agent = AgentKey(if matches!(kind, CandidateKind::CursorBackup | CandidateKind::CursorLogs) { "cursor" } else if kind == CandidateKind::CodexTmp { "codex" } else { "claude_code" }.into());
        if !agents.contains(&agent) {
            agents.push(agent);
        }
    }
    if ops.is_empty() {
        return Err(AppError::new(ErrorKind::NotFound, "Nothing is left to move. Rescan and try again."));
    }
    Ok(ActionPlan {
        id: new_id(),
        summary: format!("Moved {} to the Trash ({})", parts.join(" and "), size_text(bytes)),
        needs_confirm: kinds.iter().any(|k| k.holds_history()),
        request: req,
        agents,
        ops,
        removed: vec![],
        untouched: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::actions::{ActionRequest, FileOp};
    use crate::storage::CandidateKind;
    use crate::{Core, Env};
    use std::fs;

    /// Finished jobs go to the Trash; a running job stays; Undo puts every
    /// file back.
    #[test]
    fn trash_jobs_keeps_running_job_and_undo_restores() {
        let t = tempfile::tempdir().unwrap();
        let env = Env::for_test(&t.path().join("home"), &t.path().join("data"));
        let jobs = env.home.join(".claude/jobs");
        for (id, state) in [("aaa", "done"), ("bbb", "running"), ("ccc", "failed")] {
            fs::create_dir_all(jobs.join(id).join("tmp")).unwrap();
            fs::write(jobs.join(id).join("state.json"), format!(r#"{{"state":"{state}"}}"#)).unwrap();
            fs::write(jobs.join(id).join("tmp/out.md"), id).unwrap();
        }
        fs::create_dir_all(env.home.join(".Trash")).unwrap();
        let core = Core::open(env.clone()).unwrap();
        let plan = core.action_plan(ActionRequest::StorageTrash { kinds: vec![CandidateKind::ClaudeJobs] }).unwrap();
        let moved: Vec<_> = plan.ops.iter().filter_map(|o| if let FileOp::MovePath { from, .. } = o { Some(from.clone()) } else { None }).collect();
        assert_eq!(moved, vec![jobs.join("aaa"), jobs.join("ccc")]);
        let record = core.action_apply(&plan.id).unwrap();
        assert!(!jobs.join("aaa").exists() && !jobs.join("ccc").exists());
        assert_eq!(fs::read_to_string(jobs.join("bbb/tmp/out.md")).unwrap(), "bbb");
        assert!(env.home.join(".Trash/aaa/tmp/out.md").is_file());
        core.action_undo(&record.id).unwrap();
        for id in ["aaa", "bbb", "ccc"] {
            assert_eq!(fs::read_to_string(jobs.join(id).join("tmp/out.md")).unwrap(), id);
        }
        assert!(!env.home.join(".Trash/aaa").exists());
    }
}
