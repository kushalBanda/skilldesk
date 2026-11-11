//! What sits at a write target, and what a caller may destroy there.
//!
//! Classified from `symlink_metadata` only — never `is_dir()`, which follows
//! links and would send a directory symlink down a delete path. SkillDesk has
//! no copy-mode deployments, so `Recorded` only vouches for links.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetState {
    /// Nothing there. Writing cannot destroy anything.
    Absent,
    /// A link that resolves to the source we are about to link. Ours by construction.
    LinkToSource,
    /// A link pointing somewhere else, or a dangling one. User configuration.
    ForeignLink,
    /// A real directory.
    RealDir,
    /// A real file. We never write a bare file at a skill target.
    RealFile,
}

/// An enum, not a bool, so every call site states its intent (#363).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplacePolicy {
    /// Proceed only when nothing of the user's can be lost.
    NoClobber,
    /// Our History says we created a link here. Authorizes replacing a link only.
    RecordedLink,
    /// Explicit user intent (confirmed in the plan dialog).
    UserConfirmed,
}

impl ReplacePolicy {
    pub fn permits(&self, state: TargetState) -> bool {
        match state {
            TargetState::Absent | TargetState::LinkToSource => true,
            TargetState::ForeignLink => !matches!(self, ReplacePolicy::NoClobber),
            TargetState::RealDir | TargetState::RealFile => matches!(self, ReplacePolicy::UserConfirmed),
        }
    }

    pub fn describe(&self) -> &'static str {
        match self {
            ReplacePolicy::NoClobber => "is not managed by SkillDesk",
            ReplacePolicy::RecordedLink => "does not match what SkillDesk recorded",
            ReplacePolicy::UserConfirmed => "cannot be replaced",
        }
    }
}

/// Classify `target` without following links. `source` is used only to
/// recognise a link that already points at it; pass `None` when there is none
/// (an empty source would canonicalize to the process CWD and make a user's
/// link look like ours).
pub fn classify_target(target: &Path, source: Option<&Path>) -> std::io::Result<TargetState> {
    let metadata = match std::fs::symlink_metadata(target) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(TargetState::Absent),
        // A target we cannot even stat is not evidence that it is ours.
        Err(e) => return Err(e),
    };
    if metadata.file_type().is_symlink() {
        if source.is_some_and(|s| symlink_points_to(target, s)) {
            return Ok(TargetState::LinkToSource);
        }
        return Ok(TargetState::ForeignLink);
    }
    Ok(if metadata.is_dir() { TargetState::RealDir } else { TargetState::RealFile })
}

pub fn symlink_points_to(link: &Path, source: &Path) -> bool {
    let Ok(dest) = std::fs::read_link(link) else { return false };
    let resolved: PathBuf = if dest.is_absolute() {
        dest
    } else {
        link.parent().unwrap_or_else(|| Path::new("")).join(dest)
    };
    if resolved == source {
        return true;
    }
    matches!((resolved.canonicalize(), source.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole authorization contract in one place.
    #[test]
    fn target_policy_decision_table() {
        use ReplacePolicy::*;
        use TargetState::*;
        for state in [Absent, LinkToSource] {
            for p in [NoClobber, RecordedLink, UserConfirmed] {
                assert!(p.permits(state), "{p:?} {state:?}");
            }
        }
        assert!(!NoClobber.permits(ForeignLink));
        assert!(RecordedLink.permits(ForeignLink));
        assert!(UserConfirmed.permits(ForeignLink));
        for state in [RealDir, RealFile] {
            assert!(!NoClobber.permits(state), "{state:?}");
            assert!(!RecordedLink.permits(state), "{state:?}");
            assert!(UserConfirmed.permits(state), "{state:?}");
        }
    }

    #[test]
    fn target_classify_never_follows_links() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real");
        let other = tmp.path().join("other");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(classify_target(&link, None).unwrap(), TargetState::ForeignLink);
        assert_eq!(classify_target(&link, Some(&other)).unwrap(), TargetState::ForeignLink);
        assert_eq!(classify_target(&link, Some(&real)).unwrap(), TargetState::LinkToSource);
        assert_eq!(classify_target(&real, Some(&real)).unwrap(), TargetState::RealDir);
    }
}
