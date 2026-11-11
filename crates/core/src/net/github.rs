//! GitHub sources: one archive from codeload (no clone, no `git`); for repos
//! whose archive is over 50 MB, the skill folder's own files from
//! raw.githubusercontent.com. Commit
//! lookups on api.github.com without an account.

use super::{network, Http};
use crate::error::{AppError, ErrorKind};
use crate::parse::skills::parse_frontmatter;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

const ARCHIVE_LIMIT: u64 = 50 * 1024 * 1024;
/// Unpacked size and entry count of the whole archive (pass 1 reads it all).
const UNPACKED_LIMIT: u64 = 300 * 1024 * 1024;
const ENTRY_LIMIT: usize = 50_000;
const SKILL_LIMIT: u64 = 50 * 1024 * 1024;
const SKILL_FILES: usize = 5_000;
/// A recursive tree listing (GitHub caps it near 7 MB).
const TREE_LIMIT: u64 = 16 * 1024 * 1024;
const SKIP: &[&str] = &[".git", ".DS_Store"];

fn invalid(msg: impl Into<String>) -> AppError {
    AppError::new(ErrorKind::InvalidInput, msg)
}

/// `owner/repo`, letters, digits, `.`, `-`, `_` only.
pub fn check_repo(repo: &str) -> Result<(), AppError> {
    let ok = |s: &str| !s.is_empty() && s != "." && s != ".." && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    match repo.split('/').collect::<Vec<_>>().as_slice() {
        [o, r] if ok(o) && ok(r) => Ok(()),
        _ => Err(invalid(format!("{repo} is not a GitHub owner/repo."))),
    }
}

fn is_sha(s: &str) -> bool {
    s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Latest commit on the default branch.
pub(crate) async fn head(http: &Http, repo: &str) -> Result<String, AppError> {
    check_repo(repo)?;
    let b = http.get(&format!("https://api.github.com/repos/{repo}/commits/HEAD"), 64 * 1024, Some("application/vnd.github.sha")).await?;
    let sha = String::from_utf8_lossy(&b).trim().to_string();
    if !is_sha(&sha) {
        return Err(network("GitHub sent something SkillDesk could not read."));
    }
    Ok(sha)
}

/// The last commit that touched `path` (on the default branch, or up to `at`).
pub(crate) async fn folder_commit(http: &Http, repo: &str, path: &str, at: Option<&str>) -> Result<Option<String>, AppError> {
    check_repo(repo)?;
    let mut url = reqwest::Url::parse(&format!("https://api.github.com/repos/{repo}/commits")).map_err(|e| AppError::internal(e.to_string()))?;
    {
        let mut q = url.query_pairs_mut();
        if !path.is_empty() {
            q.append_pair("path", path);
        }
        if let Some(sha) = at {
            q.append_pair("sha", sha);
        }
        q.append_pair("per_page", "1");
    }
    let v = http.json(url.as_str(), 1024 * 1024).await?;
    Ok(v.get(0).and_then(|c| c.get("sha")).and_then(|s| s.as_str()).filter(|s| is_sha(s)).map(str::to_string))
}

/// The repo at `sha` as one archive, or `None` when it is over 50 MB (then
/// [`fetch_skill_files`] fetches the skill's own files instead).
pub(crate) async fn archive(http: &Http, repo: &str, sha: &str) -> Result<Option<Vec<u8>>, AppError> {
    check_repo(repo)?;
    if !is_sha(sha) {
        return Err(invalid("Not a commit id."));
    }
    match http.get(&format!("https://codeload.github.com/{repo}/tar.gz/{sha}"), ARCHIVE_LIMIT, None).await {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind == ErrorKind::InvalidInput => Ok(None),
        Err(e) => Err(e),
    }
}

/// Large repos (media, demos, binaries next to a small skill): list the tree
/// once on api.github.com, pick the skill folder the same way as
/// [`extract_skill`], then fetch only its plain files from
/// raw.githubusercontent.com at the same commit.
pub(crate) async fn fetch_skill_files(http: &Http, repo: &str, sha: &str, skill_id: &str, dest: &Path) -> Result<String, AppError> {
    check_repo(repo)?;
    if !is_sha(sha) {
        return Err(invalid("Not a commit id."));
    }
    let tree = http.json(&format!("https://api.github.com/repos/{repo}/git/trees/{sha}?recursive=1"), TREE_LIMIT).await?;
    let unreadable = || network("GitHub sent a file list SkillDesk could not read.");
    let list = tree.get("tree").and_then(|t| t.as_array()).ok_or_else(unreadable)?;
    let mut skills: Vec<(PathBuf, Option<String>)> = Vec::new();
    let mut odd: Vec<PathBuf> = Vec::new();
    let mut files: Vec<(PathBuf, u64, bool)> = Vec::new();
    for e in list {
        let (Some(path), Some(kind), Some(mode)) = (e.get("path").and_then(|v| v.as_str()), e.get("type").and_then(|v| v.as_str()), e.get("mode").and_then(|v| v.as_str())) else {
            return Err(unreadable());
        };
        let rel = safe_rel(path)?;
        match (kind, mode) {
            ("tree", _) => {}
            ("blob", "100644" | "100755") => {
                let name = rel.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                if (name == "SKILL.md" || name == "skill.md") && rel.components().count() <= 7 {
                    skills.push((rel.parent().map(Path::to_path_buf).unwrap_or_default(), None));
                }
                files.push((rel, e.get("size").and_then(|v| v.as_u64()).unwrap_or(0), mode == "100755"));
            }
            _ => odd.push(rel),
        }
    }
    if files.len() + odd.len() > ENTRY_LIMIT {
        return Err(invalid("This repository is too large to read (over 50,000 files)."));
    }
    let chosen = choose(&mut skills, skill_id)?;
    let inside = |p: &Path| chosen.as_os_str().is_empty() || p.starts_with(&chosen);
    let skipped = |p: &Path| p.components().any(|c| SKIP.contains(&c.as_os_str().to_string_lossy().as_ref()));
    if let Some(l) = odd.iter().find(|p| inside(p) && !skipped(p)) {
        return Err(invalid(format!("This skill contains a link or special file ({}). SkillDesk only installs plain files.", l.display())));
    }
    if chosen.as_os_str().is_empty() && skills.len() > 1 {
        return Err(invalid("This repository's root is a skill that also holds other skills. Nothing was installed."));
    }
    let wanted: Vec<_> = files.into_iter().filter(|(p, _, _)| inside(p) && !skipped(p)).collect();
    if wanted.iter().map(|(_, n, _)| n).sum::<u64>() > SKILL_LIMIT || wanted.len() > SKILL_FILES {
        return Err(invalid("This skill is too large to install (over 50 MB or 5,000 files)."));
    }

    use std::os::unix::fs::PermissionsExt;
    let io = |e: std::io::Error| AppError::new(ErrorKind::Io, format!("Cannot save the download: {e}"));
    std::fs::create_dir_all(dest).map_err(io)?;
    let mut total = 0u64;
    for (rel, _, exec) in &wanted {
        let mut url = reqwest::Url::parse("https://raw.githubusercontent.com/").map_err(|e| AppError::internal(e.to_string()))?;
        {
            let mut seg = url.path_segments_mut().map_err(|_| AppError::internal("raw URL"))?;
            seg.extend(repo.split('/')).push(sha);
            seg.extend(rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()));
        }
        let body = http.get(url.as_str(), SKILL_LIMIT - total, None).await?;
        total += body.len() as u64;
        let out = dest.join(rel.strip_prefix(&chosen).unwrap_or(rel));
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p).map_err(io)?;
        }
        std::fs::OpenOptions::new().write(true).create_new(true).open(&out).and_then(|mut f| std::io::Write::write_all(&mut f, &body)).map_err(io)?;
        let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(if *exec { 0o755 } else { 0o644 }));
    }
    Ok(chosen.to_string_lossy().into_owned())
}

/// A tree path as plain components, or `Err` when absolute or climbing out.
fn safe_rel(s: &str) -> Result<PathBuf, AppError> {
    let bad = || invalid(format!("GitHub listed a file that would land outside its folder ({s}). Nothing was installed."));
    if s.is_empty() || s.contains('\0') || s.starts_with('/') || s.contains('\\') {
        return Err(bad());
    }
    let mut out = PathBuf::new();
    for c in Path::new(s).components() {
        match c {
            Component::Normal(p) => out.push(p),
            _ => return Err(bad()),
        }
    }
    Ok(out)
}

/// The skill folder for `skill_id`: `<id>`, `skills/<id>`, `.agents/skills/<id>`,
/// then any folder named `<id>`, then a SKILL.md whose `name` is `<id>`.
fn choose(skills: &mut [(PathBuf, Option<String>)], skill_id: &str) -> Result<PathBuf, AppError> {
    skills.sort_by(|a, b| a.0.components().count().cmp(&b.0.components().count()).then(a.0.cmp(&b.0)));
    let has = |p: &Path| skills.iter().any(|(d, _)| d == p);
    let direct = [PathBuf::from(skill_id), Path::new("skills").join(skill_id), Path::new(".agents/skills").join(skill_id)];
    direct
        .into_iter()
        .find(|p| has(p))
        .or_else(|| skills.iter().find(|(d, _)| d.file_name().is_some_and(|n| n == skill_id)).map(|(d, _)| d.clone()))
        .or_else(|| skills.iter().find(|(_, n)| n.as_deref() == Some(skill_id)).map(|(d, _)| d.clone()))
        .ok_or_else(|| invalid(format!("{skill_id} was not found in this repository. Nothing was installed.")))
}

/// The entry path without the archive's top folder (`owner-repo-sha/`), or
/// `Err` when it is absolute or climbs out with `..` (tar slip).
fn inner_path(raw: &[u8]) -> Result<Option<PathBuf>, AppError> {
    let s = String::from_utf8_lossy(raw);
    let bad = || invalid(format!("The archive has an entry that would land outside its folder ({s}). Nothing was installed."));
    if s.contains('\0') || s.starts_with('/') || s.contains('\\') {
        return Err(bad());
    }
    let mut parts = Vec::new();
    for c in Path::new(s.as_ref()).components() {
        match c {
            Component::Normal(p) => parts.push(p.to_owned()),
            Component::CurDir => {}
            _ => return Err(bad()),
        }
    }
    if parts.len() <= 1 {
        return Ok(None);
    }
    Ok(Some(parts[1..].iter().collect()))
}

enum Kind {
    File,
    Dir,
    /// Links and device files: never written.
    Other,
    /// Archive bookkeeping (pax headers).
    Meta,
}

fn kind(t: tar::EntryType) -> Kind {
    use tar::EntryType as E;
    match t {
        E::Regular | E::Continuous => Kind::File,
        E::Directory => Kind::Dir,
        E::XGlobalHeader | E::XHeader | E::GNULongName | E::GNULongLink => Kind::Meta,
        _ => Kind::Other,
    }
}

fn entries(bytes: &[u8]) -> tar::Archive<flate2::read::GzDecoder<&[u8]>> {
    tar::Archive::new(flate2::read::GzDecoder::new(bytes))
}

fn read_err(e: std::io::Error) -> AppError {
    invalid(format!("The archive from GitHub could not be read ({e})."))
}

/// Finds `skill_id` in a repo archive and writes only that folder to `dest`
/// (created, must not exist). Returns the folder's path inside the repo.
pub fn extract_skill(bytes: &[u8], skill_id: &str, dest: &Path) -> Result<String, AppError> {
    // Pass 1: check every entry, find the skill folders.
    let mut skills: Vec<(PathBuf, Option<String>)> = Vec::new();
    let mut odd: Vec<PathBuf> = Vec::new();
    let mut files: Vec<(PathBuf, u64)> = Vec::new();
    let (mut total, mut count) = (0u64, 0usize);
    let mut ar = entries(bytes);
    for e in ar.entries().map_err(read_err)? {
        let mut e = e.map_err(read_err)?;
        let k = kind(e.header().entry_type());
        if matches!(k, Kind::Meta) {
            continue;
        }
        let Some(rel) = inner_path(&e.path_bytes())? else { continue };
        count += 1;
        total += e.size();
        if count > ENTRY_LIMIT || total > UNPACKED_LIMIT {
            return Err(invalid("This repository is too large to read (over 300 MB unpacked or 50,000 files)."));
        }
        match k {
            Kind::Other => odd.push(rel),
            Kind::File => {
                let name = rel.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                if (name == "SKILL.md" || name == "skill.md") && rel.components().count() <= 7 {
                    let mut text = String::new();
                    let _ = (&mut e).take(1024 * 1024).read_to_string(&mut text);
                    skills.push((rel.parent().map(Path::to_path_buf).unwrap_or_default(), parse_frontmatter(&text).name));
                }
                files.push((rel, e.size()));
            }
            _ => {}
        }
    }
    let chosen = choose(&mut skills, skill_id)?;

    let inside = |p: &Path| chosen.as_os_str().is_empty() || p.starts_with(&chosen);
    let skipped = |p: &Path| p.components().any(|c| SKIP.contains(&c.as_os_str().to_string_lossy().as_ref()));
    if let Some(l) = odd.iter().find(|p| inside(p) && !skipped(p)) {
        return Err(invalid(format!("This skill contains a link or special file ({}). SkillDesk only installs plain files.", l.display())));
    }
    // A skill at the repo root would bring the whole repo: other skill folders.
    if chosen.as_os_str().is_empty() && skills.len() > 1 {
        return Err(invalid("This repository's root is a skill that also holds other skills. Nothing was installed."));
    }
    let size: u64 = files.iter().filter(|(p, _)| inside(p) && !skipped(p)).map(|(_, n)| n).sum();
    if size > SKILL_LIMIT {
        return Err(invalid("This skill is too large to install (over 50 MB)."));
    }

    // Pass 2: write the chosen folder's plain files only.
    use std::os::unix::fs::PermissionsExt;
    let io = |e: std::io::Error| AppError::new(ErrorKind::Io, format!("Cannot save the download: {e}"));
    std::fs::create_dir_all(dest).map_err(io)?;
    let mut ar = entries(bytes);
    for e in ar.entries().map_err(read_err)? {
        let mut e = e.map_err(read_err)?;
        let k = kind(e.header().entry_type());
        if !matches!(k, Kind::File | Kind::Dir) {
            continue;
        }
        let Some(rel) = inner_path(&e.path_bytes())? else { continue };
        if !inside(&rel) || skipped(&rel) {
            continue;
        }
        let sub = rel.strip_prefix(&chosen).unwrap_or(&rel);
        if sub.as_os_str().is_empty() {
            continue;
        }
        let out = dest.join(sub);
        if matches!(k, Kind::Dir) {
            std::fs::create_dir_all(&out).map_err(io)?;
            continue;
        }
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p).map_err(io)?;
        }
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&out).map_err(io)?;
        std::io::copy(&mut (&mut e).take(SKILL_LIMIT), &mut f).map_err(io)?;
        let mode = e.header().mode().unwrap_or(0o644);
        let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode & 0o755));
    }
    Ok(chosen.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tgz(entries: &[(&str, tar::EntryType, &[u8])]) -> Vec<u8> {
        let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut b = tar::Builder::new(gz);
        for (path, t, body) in entries {
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(*t);
            h.set_size(body.len() as u64);
            h.set_mode(0o644);
            // Raw name bytes: `set_path` refuses `..`, a hostile archive does not.
            let name = &mut h.as_old_mut().name;
            name[..path.len()].copy_from_slice(path.as_bytes());
            if *t == tar::EntryType::Symlink {
                h.set_link_name("../../etc").unwrap();
            }
            h.set_cksum();
            b.append(&h, *body).unwrap();
        }
        b.into_inner().unwrap().finish().unwrap()
    }

    /// A `..` entry anywhere refuses the archive before anything is written;
    /// a link inside the skill refuses it too; only the chosen folder is kept.
    #[test]
    fn marketplace_archive_rejects_tar_slip_and_keeps_one_folder() {
        use tar::EntryType as E;
        let tmp = tempfile::tempdir().unwrap();
        let skill = b"---\nname: tdd\n---\n";

        let evil = tgz(&[("r-abc/skills/tdd/SKILL.md", E::Regular, skill), ("r-abc/../../escaped.txt", E::Regular, b"pwned")]);
        let dest = tmp.path().join("a/tdd");
        assert!(extract_skill(&evil, "tdd", &dest).is_err());
        assert!(!dest.exists() && !tmp.path().join("escaped.txt").exists());

        let abs = tgz(&[("/tmp/x/SKILL.md", E::Regular, skill)]);
        assert!(extract_skill(&abs, "tdd", &tmp.path().join("b/tdd")).is_err());

        let link = tgz(&[("r-abc/skills/tdd/SKILL.md", E::Regular, skill), ("r-abc/skills/tdd/up", E::Symlink, b"")]);
        assert!(extract_skill(&link, "tdd", &tmp.path().join("c/tdd")).is_err());
        assert!(!tmp.path().join("c").exists());

        let ok = tgz(&[
            ("r-abc/README.md", E::Regular, b"repo"),
            ("r-abc/skills/other/SKILL.md", E::Regular, b"---\nname: other\n---\n"),
            ("r-abc/skills/tdd/SKILL.md", E::Regular, skill),
            ("r-abc/skills/tdd/refs/guide.md", E::Regular, b"guide"),
            ("r-abc/docs/up", E::Symlink, b""),
        ]);
        let dest = tmp.path().join("d/tdd");
        assert_eq!(extract_skill(&ok, "tdd", &dest).unwrap(), "skills/tdd");
        let mut got: Vec<String> = walk(&dest);
        got.sort();
        assert_eq!(got, ["SKILL.md", "refs/guide.md"]);
    }

    fn walk(d: &Path) -> Vec<String> {
        let mut out = Vec::new();
        let mut stack = vec![d.to_path_buf()];
        while let Some(x) = stack.pop() {
            for e in std::fs::read_dir(&x).unwrap().flatten() {
                if e.file_type().unwrap().is_dir() {
                    stack.push(e.path());
                } else {
                    out.push(e.path().strip_prefix(d).unwrap().to_string_lossy().into_owned());
                }
            }
        }
        out
    }
}
