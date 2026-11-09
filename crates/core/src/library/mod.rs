//! Central skill library (ADR 0015): `~/.skilldesk/skills/<name>/`, one home per
//! skill; agent folders hold links to it. Per-skill sidecar
//! `.skilldesk.json` is the record; the database is rebuilt from disk.

pub mod adopt;
pub mod install;

use crate::env::Env;
use crate::error::{AppError, ErrorKind};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const SIDECAR: &str = ".skilldesk.json";

/// `~/.skilldesk/skills`, as configured (may not exist yet).
pub fn library_dir(env: &Env) -> PathBuf {
    env.skilldesk_dir().join("skills")
}

/// The library folder, refusing one that is (or sits under) a link: a library
/// reached through a link can point into an agent folder or into a source
/// being installed, and copy into itself.
pub fn checked_library_dir(env: &Env) -> Result<PathBuf, AppError> {
    let lib = library_dir(env);
    for p in [env.skilldesk_dir(), lib.clone()] {
        if std::fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(AppError::new(
                ErrorKind::InvalidInput,
                format!("{} is a link. The library must be a plain folder; nothing was changed.", p.display()),
            ));
        }
    }
    Ok(lib)
}

/// The library folder resolved on disk, when it exists (for comparing links).
pub fn canonical_library_dir(env: &Env) -> Option<PathBuf> {
    checked_library_dir(env).ok().and_then(|l| std::fs::canonicalize(l).ok())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LibrarySource {
    Local { path: PathBuf },
    Zip { path: PathBuf },
    Adopted { from: Vec<PathBuf> },
    /// ADR 0016: `source` is `owner/repo` or a site's domain; `path` the
    /// folder inside the repo; `revision` the folder's commit at install time.
    Marketplace {
        registry: String,
        source: String,
        skill_id: String,
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        revision: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sidecar {
    pub source: LibrarySource,
    /// RFC 3339.
    pub installed_at: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl Sidecar {
    pub fn new(source: LibrarySource) -> Self {
        Self { source, installed_at: chrono::Utc::now().to_rfc3339(), tags: Vec::new() }
    }

    pub fn render(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).unwrap_or_default();
        s.push('\n');
        s
    }
}

pub fn read_sidecar(dir: &Path) -> Option<Sidecar> {
    serde_json::from_str(&std::fs::read_to_string(dir.join(SIDECAR)).ok()?).ok()
}

/// Whether `a` and `b` overlap (one is the other or inside it), after resolving links.
pub fn overlaps(a: &Path, b: &Path) -> bool {
    let c = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let (a, b) = (c(a), c(b));
    a.starts_with(&b) || b.starts_with(&a)
}

/// Files and bytes under a folder (links not followed).
pub fn dir_stats(dir: &Path) -> (u32, u64) {
    let mut files = 0u32;
    let mut bytes = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(m) = std::fs::symlink_metadata(e.path()) else { continue };
            if m.is_dir() {
                stack.push(e.path());
            } else {
                files += 1;
                bytes += m.len();
            }
        }
    }
    (files, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A library reached through a link (here into an
    /// agent folder) is refused before anything is written.
    #[test]
    fn library_path_through_link_into_itself_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().canonicalize().unwrap();
        let env = Env::for_test(&home, &home.join("data"));
        let agent = home.join(".claude/skills");
        std::fs::create_dir_all(&agent).unwrap();
        std::fs::create_dir_all(home.join(".skilldesk")).unwrap();
        std::os::unix::fs::symlink(&agent, home.join(".skilldesk/skills")).unwrap();
        assert!(checked_library_dir(&env).is_err());

        // Installing from a folder that contains the library is refused too.
        std::fs::remove_file(home.join(".skilldesk/skills")).unwrap();
        std::fs::create_dir_all(home.join(".skilldesk/skills")).unwrap();
        let core = crate::core::Core::open(env.clone()).unwrap();
        std::fs::write(home.join(".skilldesk/SKILL.md"), "---\nname: loop\n---\n").unwrap();
        let err = install::inspect(&core, &home.join(".skilldesk")).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        let src = home.join(".skilldesk/skills/inner");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("SKILL.md"), "---\nname: inner\n---\n").unwrap();
        assert!(install::inspect(&core, &src).is_err(), "source inside the library");
        assert_eq!(std::fs::read_dir(home.join(".skilldesk/skills")).unwrap().count(), 1, "nothing staged or copied");
    }
}
