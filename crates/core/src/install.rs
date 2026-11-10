//! Install a skill from a folder, `.zip` or `.skill` archive straight into
//! agent skill folders (each agent gets its own copy).
//! Rules: one skill folder per source, symlinks and `.git` never copied, unix
//! modes kept (setuid stripped). An archive entry that would land outside its
//! folder refuses the whole archive instead of being skipped.
//!
//! `inspect` only reads. `stage` copies into `<data dir>/staging/<plan>/`,
//! SkillDesk's own scratch space; the plan moves each staged copy into an
//! agent folder.

use crate::adapters;
use crate::core::Core;
use crate::error::{AppError, ErrorKind};
use crate::fsx::path_guard::sanitize_name;
use crate::fsx::target::{classify_target, TargetState};
use crate::model::AgentKey;
use crate::parse::skills::{parse_frontmatter, read_meta};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use ts_rs::TS;

const MAX_BYTES: u64 = 50 * 1024 * 1024;
const MAX_FILES: u32 = 5_000;
const SKIP: &[&str] = &[".git", ".DS_Store", "__MACOSX"];

fn invalid(msg: impl Into<String>) -> AppError {
    AppError::new(ErrorKind::InvalidInput, msg)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SourceKind {
    Folder,
    Zip,
    Skill,
}

/// What already sits where a link would go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum TargetExisting {
    Nothing,
    /// This skill is already there (the same folder, or a link to it).
    Linked,
    /// The agent's own folder of the same name (Replace preview).
    Folder { files: u32, #[ts(type = "number")] bytes: u64 },
    Link { #[ts(type = "string")] to: PathBuf },
    File,
}

/// One "Turn on for" chip: a skills folder and every agent that reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InstallTarget {
    pub key: AgentKey,
    #[ts(type = "string")]
    pub dir: PathBuf,
    pub agents: Vec<AgentKey>,
    pub existing: TargetExisting,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InstallPreview {
    #[ts(type = "string")]
    pub source: PathBuf,
    pub kind: SourceKind,
    pub name: String,
    pub description: Option<String>,
    pub files: u32,
    #[ts(type = "number")]
    pub bytes: u64,
    pub targets: Vec<InstallTarget>,
}

/// The skill inside a source: a folder on disk, or a path prefix in an archive.
enum Located {
    Dir(PathBuf),
    Archive { prefix: PathBuf, files: u32, bytes: u64, skill_md: String },
}

fn kind_of(path: &Path) -> Result<SourceKind, AppError> {
    if std::fs::symlink_metadata(path).map_err(|e| invalid(format!("Cannot open {}: {e}", path.display())))?.is_dir() {
        return Ok(SourceKind::Folder);
    }
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("zip") => Ok(SourceKind::Zip),
        Some("skill") => Ok(SourceKind::Skill),
        _ => Err(invalid("SkillDesk installs a folder, a .zip or a .skill file. Nothing else.")),
    }
}

fn open_zip(path: &Path) -> Result<zip::ZipArchive<std::fs::File>, AppError> {
    let f = std::fs::File::open(path).map_err(|e| invalid(format!("Cannot open {}: {e}", path.display())))?;
    zip::ZipArchive::new(f).map_err(|e| invalid(format!("{} is not a readable archive ({e}).", path.display())))
}

/// The entry's path inside the archive, or `None` when it is absolute or
/// climbs out with `..` (zip slip). Checked on the raw name, not trusted to
/// the zip crate's normalisation.
fn safe_rel(raw: &str) -> Option<PathBuf> {
    if raw.contains('\0') || raw.starts_with('/') || raw.starts_with('\\') {
        return None;
    }
    let mut out = PathBuf::new();
    for part in raw.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return None,
            p if p.contains(':') => return None,
            p => out.push(p),
        }
    }
    Some(out)
}

fn skipped(rel: &Path) -> bool {
    rel.components().any(|c| SKIP.contains(&c.as_os_str().to_string_lossy().as_ref()))
}

/// Every entry must stay inside the archive's folder; the one SKILL.md found
/// (at most 3 folders deep) marks the skill.
fn locate_in_zip(path: &Path) -> Result<Located, AppError> {
    let mut zip = open_zip(path)?;
    let mut markers: Vec<(PathBuf, usize)> = Vec::new();
    let mut entries: Vec<(PathBuf, u64, bool)> = Vec::new();
    for i in 0..zip.len() {
        let e = zip.by_index(i).map_err(|e| invalid(format!("Cannot read the archive ({e}).")))?;
        let raw = e.name().to_string();
        let Some(rel) = safe_rel(&raw).filter(|_| e.enclosed_name().is_some()) else {
            return Err(invalid(format!("This archive has an entry that would land outside its folder ({raw}). Nothing was installed.")));
        };
        if skipped(&rel) {
            continue;
        }
        let name = rel.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if !e.is_dir() && (name == "SKILL.md" || name == "skill.md") && rel.components().count() <= 4 {
            markers.push((rel.parent().map(Path::to_path_buf).unwrap_or_default(), i));
        }
        entries.push((rel, e.size(), e.is_dir() || e.is_symlink()));
    }
    markers.sort_by_key(|(p, _)| p.components().count());
    let Some((prefix, idx)) = markers.first().cloned() else {
        return Err(invalid("No SKILL.md in this archive, so it is not a skill."));
    };
    if markers.iter().filter(|(p, _)| *p != prefix && !p.starts_with(&prefix)).count() > 0 {
        return Err(invalid("This archive holds more than one skill. Install them one at a time."));
    }
    let (mut files, mut bytes) = (0u32, 0u64);
    for (rel, size, dir_or_link) in &entries {
        if !dir_or_link && rel.starts_with(&prefix) {
            files += 1;
            bytes += size;
        }
    }
    if bytes > MAX_BYTES || files > MAX_FILES {
        return Err(invalid("This skill is too large to install (over 50 MB or 5,000 files)."));
    }
    let mut skill_md = String::new();
    zip.by_index(idx)
        .map_err(|e| invalid(format!("Cannot read SKILL.md ({e}).")))?
        .take(1024 * 1024)
        .read_to_string(&mut skill_md)
        .map_err(|e| invalid(format!("Cannot read SKILL.md ({e}).")))?;
    Ok(Located::Archive { prefix, files, bytes, skill_md })
}

fn locate(path: &Path, kind: SourceKind) -> Result<Located, AppError> {
    if kind != SourceKind::Folder {
        return locate_in_zip(path);
    }
    if crate::parse::skills::is_skill_dir(path) {
        return Ok(Located::Dir(path.to_path_buf()));
    }
    // A folder holding exactly one skill folder.
    let inner: Vec<PathBuf> = std::fs::read_dir(path)
        .map_err(|e| invalid(format!("Cannot read {}: {e}", path.display())))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir()) && crate::parse::skills::is_skill_dir(p))
        .collect();
    match inner.as_slice() {
        [one] => Ok(Located::Dir(one.clone())),
        [] => Err(invalid("No SKILL.md in this folder, so it is not a skill.")),
        _ => Err(invalid("This folder holds more than one skill. Pick one skill folder.")),
    }
}

/// Every detected agent's own skills folder, one chip per folder on disk.
/// `source` = the skill's own folder on this Mac, when there is one: a target
/// that already holds it (or links to it) is `Linked`.
pub fn targets(core: &Core, name: &str, source: Option<&Path>) -> Vec<InstallTarget> {
    let env = &core.env;
    let mut out: Vec<InstallTarget> = Vec::new();
    let mut agents = adapters::detected(env);
    // Full agents first, then the shared folder, then skills-only agents.
    agents.sort_by_key(|a| match a.tier {
        adapters::Tier::Full => 0,
        adapters::Tier::Shared => 1,
        adapters::Tier::Skills => 2,
    });
    for a in agents {
        let dir = env.resolve(a.skills_dir);
        let readers = crate::actions::agents_reading(core, &dir);
        if out.iter().any(|t| t.dir == dir || same_dir(&t.dir, &dir)) {
            continue;
        }
        let link = dir.join(name);
        let existing = match classify_target(&link, source) {
            Ok(TargetState::Absent) => TargetExisting::Nothing,
            Ok(TargetState::LinkToSource) => TargetExisting::Linked,
            Ok(TargetState::RealDir) if source.is_some_and(|s| same_dir(s, &link)) => TargetExisting::Linked,
            Ok(TargetState::RealDir) => {
                let (files, bytes) = dir_stats(&link);
                TargetExisting::Folder { files, bytes }
            }
            Ok(TargetState::ForeignLink) => TargetExisting::Link { to: std::fs::read_link(&link).unwrap_or_default() },
            Ok(TargetState::RealFile) | Err(_) => TargetExisting::File,
        };
        let mut agents = readers;
        if !agents.contains(&a.agent_key()) {
            agents.insert(0, a.agent_key());
        }
        out.push(InstallTarget { key: a.agent_key(), dir, agents, existing });
    }
    out
}

pub(crate) fn same_dir(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
        _ => false,
    }
}

pub fn inspect(core: &Core, source: &Path) -> Result<InstallPreview, AppError> {
    let kind = kind_of(source)?;
    if overlaps(source, &core.env.staging_dir()) {
        return Err(invalid("That is SkillDesk's own folder (or holds it). Pick a skill from somewhere else."));
    }
    let located = locate(source, kind)?;
    let (meta, fallback, files, bytes) = match &located {
        Located::Dir(d) => {
            let (files, bytes) = dir_stats(d);
            if bytes > MAX_BYTES || files > MAX_FILES {
                return Err(invalid("This skill is too large to install (over 50 MB or 5,000 files)."));
            }
            (read_meta(d), d.file_name().map(|n| n.to_string_lossy().into_owned()), files, bytes)
        }
        Located::Archive { prefix, files, bytes, skill_md } => {
            let stem = prefix
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .or_else(|| source.file_stem().map(|n| n.to_string_lossy().into_owned()));
            (parse_frontmatter(skill_md), stem, *files, *bytes)
        }
    };
    let raw = meta.name.clone().or(fallback).unwrap_or_else(|| "skill".into());
    let name = sanitize_name(crate::parse::claude::bare_name(&raw));
    Ok(InstallPreview {
        source: source.to_path_buf(),
        kind,
        description: meta.description,
        files,
        bytes,
        targets: targets(core, &name, None),
        name,
    })
}

/// Copies the skill into `<data dir>/staging/<plan_id>/<slot>/<name>`.
/// One slot per agent folder it goes to. Returns the staged folder.
pub fn stage(core: &Core, source: &Path, plan_id: &str, slot: usize, name: &str) -> Result<PathBuf, AppError> {
    let kind = kind_of(source)?;
    let located = locate(source, kind)?;
    let staging = core.env.staging_dir();
    prune_staging(core, &staging);
    let dest = staging.join(plan_id).join(slot.to_string()).join(name);
    let io = |e: std::io::Error| AppError::new(ErrorKind::Io, format!("Cannot prepare the install: {e}"));
    std::fs::create_dir_all(&dest).map_err(io)?;
    let result = match &located {
        Located::Dir(d) => copy_dir(d, &dest).map_err(io),
        Located::Archive { prefix, .. } => extract(source, prefix, &dest),
    };
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(staging.join(plan_id));
        return Err(e);
    }
    Ok(dest)
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

/// Whether `a` and `b` overlap (one is the other or inside it), after resolving links.
pub fn overlaps(a: &Path, b: &Path) -> bool {
    let c = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let (a, b) = (c(a), c(b));
    a.starts_with(&b) || b.starts_with(&a)
}

/// Staged copies nobody applied (or installs that were undone) go to the
/// Trash after 7 days.
fn prune_staging(core: &Core, staging: &Path) {
    let Ok(rd) = std::fs::read_dir(staging) else { return };
    for e in rd.flatten() {
        let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|d| d.as_secs() > 7 * 86_400);
        if old {
            let _ = std::fs::rename(e.path(), crate::actions::trash_target(core, &e.path()));
        }
    }
}

pub(crate) fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let name = entry.file_name();
        if SKIP.contains(&name.to_string_lossy().as_ref()) || ft.is_symlink() {
            // Links could pull in files from outside the skill.
            continue;
        }
        let to = dst.join(&name);
        if ft.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else if ft.is_file() {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

fn extract(archive: &Path, prefix: &Path, dest: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;
    let mut zip = open_zip(archive)?;
    let io = |e: std::io::Error| AppError::new(ErrorKind::Io, format!("Cannot unpack the archive: {e}"));
    for i in 0..zip.len() {
        let mut e = zip.by_index(i).map_err(|e| invalid(format!("Cannot read the archive ({e}).")))?;
        let Some(rel) = safe_rel(e.name()).filter(|_| e.enclosed_name().is_some()) else {
            return Err(invalid("This archive has an entry that would land outside its folder. Nothing was installed."));
        };
        if skipped(&rel) || e.is_symlink() {
            continue;
        }
        let Ok(inner) = rel.strip_prefix(prefix) else { continue };
        if inner.as_os_str().is_empty() {
            continue;
        }
        let out = dest.join(inner);
        if !out.starts_with(dest) {
            return Err(invalid("This archive has an entry that would land outside its folder. Nothing was installed."));
        }
        if e.is_dir() {
            std::fs::create_dir_all(&out).map_err(io)?;
            continue;
        }
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p).map_err(io)?;
        }
        let mut f = std::fs::File::create(&out).map_err(io)?;
        std::io::copy(&mut (&mut e).take(MAX_BYTES), &mut f).map_err(io)?;
        if let Some(mode) = e.unix_mode() {
            let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode & 0o755));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip_with(path: &Path, entries: &[(&str, &str)]) {
        let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        for (name, body) in entries {
            z.start_file(*name, opts).unwrap();
            z.write_all(body.as_bytes()).unwrap();
        }
        z.finish().unwrap();
    }

    /// An entry like `../../.zshrc` refuses the whole archive; nothing is
    /// written outside (or inside) SkillDesk's folders.
    #[test]
    fn skill_install_zip_rejects_zip_slip() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().canonicalize().unwrap();
        let core = Core::open(crate::env::Env::for_test(&home, &home.join("data"))).unwrap();
        let evil = home.join("evil.zip");
        zip_with(&evil, &[("demo/SKILL.md", "---\nname: demo\n---\n"), ("demo/../../../escaped.txt", "pwned")]);
        assert!(inspect(&core, &evil).is_err());
        assert!(stage(&core, &evil, "p1", 0, "demo").is_err());
        let req = crate::actions::ActionRequest::SkillInstall { from: evil.clone(), agents: vec![] };
        assert!(core.action_plan(req).is_err());
        assert!(!home.join("escaped.txt").exists() && !tmp.path().parent().unwrap().join("escaped.txt").exists());
        assert!(!core.env.staging_dir().join("p1").exists(), "failed stage cleans up");

        // An absolute entry is refused the same way.
        zip_with(&evil, &[("demo/SKILL.md", "---\nname: demo\n---\n"), ("/tmp/abs.txt", "x")]);
        assert!(inspect(&core, &evil).is_err());
    }
}
