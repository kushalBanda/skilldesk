//! Skill detection. Rules:
//! - a dir is a skill only with `SKILL.md` (or legacy `skill.md`);
//! - README.md / CLAUDE.md never mark a skill;
//! - recursive walks stop at a skill, skip `.git`/`node_modules`/`.hub`, and
//!   guard against symlink cycles.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

const MARKERS: &[&str] = &["SKILL.md", "skill.md"];
const SKIP_DIRS: &[&str] = &[".git", "node_modules", ".hub"];

pub fn marker_file(dir: &Path) -> Option<PathBuf> {
    MARKERS.iter().map(|m| dir.join(m)).find(|p| p.is_file())
}

pub fn is_skill_dir(dir: &Path) -> bool {
    marker_file(dir).is_some()
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SkillMeta {
    pub name: Option<String>,
    pub description: Option<String>,
}

/// YAML frontmatter `name`, `description`. Bad YAML yields empty meta.
pub fn read_meta(dir: &Path) -> SkillMeta {
    let Some(file) = marker_file(dir) else {
        return SkillMeta::default();
    };
    let Ok(text) = std::fs::read_to_string(&file) else {
        return SkillMeta::default();
    };
    parse_frontmatter(&text)
}

pub fn parse_frontmatter(text: &str) -> SkillMeta {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(rest) = text.strip_prefix("---") else {
        return SkillMeta::default();
    };
    let Some(end) = rest.find("\n---") else {
        return SkillMeta::default();
    };
    #[derive(serde::Deserialize)]
    struct Fm {
        name: Option<serde_json::Value>,
        description: Option<serde_json::Value>,
    }
    let as_text = |v: Option<serde_json::Value>| match v {
        Some(serde_json::Value::String(s)) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
        Some(serde_json::Value::Null) | None => None,
        Some(other) => Some(other.to_string()),
    };
    match yaml_serde::from_str::<Fm>(&rest[..end]) {
        Ok(fm) => SkillMeta {
            name: as_text(fm.name),
            description: as_text(fm.description),
        },
        Err(_) => SkillMeta::default(),
    }
}

/// One entry found directly in (or, recursively, under) an agent skills folder.
#[derive(Debug, Clone)]
pub enum Found {
    /// A skill directory, reached at `path` (may be a link).
    Skill { path: PathBuf },
    /// A link whose target does not exist.
    Dangling { path: PathBuf, target: PathBuf },
    /// A folder without a skill marker.
    NotASkill { path: PathBuf },
}

/// List what sits in an agent's skills folder. Hidden entries are skipped
/// (agents keep their own `.trash`, `.system` there).
pub fn list_dir(dir: &Path, recursive: bool) -> Vec<Found> {
    let mut out = Vec::new();
    let mut visited = HashSet::new();
    walk(dir, recursive, &mut visited, &mut out);
    out
}

fn walk(dir: &Path, recursive: bool, visited: &mut HashSet<PathBuf>, out: &mut Vec<Found>) {
    let canonical = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    if !visited.insert(canonical) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || SKIP_DIRS.contains(&name.as_ref()) {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            if !path.exists() {
                let target = std::fs::read_link(&path).unwrap_or_default();
                out.push(Found::Dangling { path, target });
                continue;
            }
            if !path.is_dir() {
                continue;
            }
        } else if !meta.is_dir() {
            continue;
        }
        if is_skill_dir(&path) {
            out.push(Found::Skill { path });
        } else if recursive {
            walk(&path, recursive, visited, out);
        } else {
            out.push(Found::NotASkill { path });
        }
    }
}
