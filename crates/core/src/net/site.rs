//! Site-published skills: skills.sh lists a domain as the source, and the
//! site serves its own `/.well-known/agent-skills/index.json` (or
//! `/.well-known/skills/index.json`). Supports v1 file lists and v2
//! `skill-md` / zip archive with SHA-256 digest. Only on an explicit Install
//! (ADR 0016).

use super::{network, Http};
use crate::error::{AppError, ErrorKind};
use serde_json::Value;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

const LIMIT: u64 = 50 * 1024 * 1024;
const MAX_FILES: usize = 4096;

fn invalid(msg: impl Into<String>) -> AppError {
    AppError::new(ErrorKind::InvalidInput, msg)
}

/// A bare domain: letters, digits, `.`, `-`; no leading/trailing dot or dash.
pub fn check_domain(d: &str) -> Result<(), AppError> {
    let ok = !d.is_empty()
        && d.contains('.')
        && d.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
        && !d.starts_with(['.', '-'])
        && !d.ends_with(['.', '-']);
    if ok {
        Ok(())
    } else {
        Err(invalid(format!("{d} is not a website name.")))
    }
}

fn safe_rel(p: &str) -> Result<PathBuf, AppError> {
    let bad = || invalid(format!("The site lists a file that would land outside the skill ({p}). Nothing was installed."));
    if p.is_empty() || p.len() > 512 || p.contains('\\') || p.contains(':') || p.chars().any(char::is_control) {
        return Err(bad());
    }
    let path = Path::new(p);
    if path.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(bad());
    }
    Ok(path.to_path_buf())
}

/// Downloads `skill` from `domain` into `dest` (created). Returns the
/// revision when the site gives one (a v2 digest).
pub(crate) async fn download(http: &Http, domain: &str, skill: &str, dest: &Path) -> Result<Option<String>, AppError> {
    check_domain(domain)?;
    let (index_url, index) = index(http, domain).await?;
    let entry = index
        .get("skills")
        .and_then(Value::as_array)
        .and_then(|a| a.iter().find(|s| s.get("name").and_then(Value::as_str) == Some(skill)))
        .ok_or_else(|| network(format!("{domain} no longer lists {skill}.")))?
        .clone();
    let io = |e: std::io::Error| AppError::new(ErrorKind::Io, format!("Cannot save the download: {e}"));
    std::fs::create_dir_all(dest).map_err(io)?;
    let base = reqwest::Url::parse(&index_url).map_err(|e| AppError::internal(e.to_string()))?;
    let join = |u: &reqwest::Url, s: &str| u.join(s).map_err(|_| invalid(format!("{domain} lists a bad address.")));
    let revision = if index.get("$schema").is_some() {
        let url = join(&base, entry.get("url").and_then(Value::as_str).ok_or_else(|| invalid("The site's entry has no address."))?)?;
        let bytes = http.get(url.as_str(), LIMIT, None).await?;
        let digest = entry.get("digest").and_then(Value::as_str).map(str::to_string);
        if let Some(d) = &digest {
            use sha2::{Digest, Sha256};
            let want = d.strip_prefix("sha256:").ok_or_else(|| invalid("The site uses a digest SkillDesk does not know."))?;
            if hex::encode(Sha256::digest(&bytes)) != want {
                return Err(invalid("The download does not match the site's SHA-256 digest. Nothing was installed."));
            }
        }
        match entry.get("type").and_then(Value::as_str) {
            Some("skill-md") => std::fs::write(dest.join("SKILL.md"), bytes).map_err(io)?,
            Some("archive") => unzip(&bytes, dest)?,
            other => return Err(invalid(format!("The site offers a kind of skill SkillDesk does not know ({}).", other.unwrap_or("none")))),
        }
        digest
    } else {
        let files = entry.get("files").and_then(Value::as_array).ok_or_else(|| invalid("The site's entry lists no files."))?;
        if files.len() > MAX_FILES {
            return Err(invalid("The site lists too many files for one skill."));
        }
        let rels: Vec<PathBuf> = files
            .iter()
            .map(|f| f.as_str().ok_or_else(|| invalid("The site lists a bad file name.")).and_then(safe_rel))
            .collect::<Result<_, _>>()?;
        let dir = join(&join(&base, "./")?, &format!("{skill}/"))?;
        let mut left = LIMIT;
        for rel in rels {
            let url = join(&dir, &rel.to_string_lossy())?;
            let bytes = http.get(url.as_str(), left, None).await?;
            left = left.saturating_sub(bytes.len() as u64);
            let out = dest.join(&rel);
            if let Some(p) = out.parent() {
                std::fs::create_dir_all(p).map_err(io)?;
            }
            std::fs::OpenOptions::new().write(true).create_new(true).open(&out).and_then(|mut f| std::io::Write::write_all(&mut f, &bytes)).map_err(io)?;
        }
        None
    };
    if !dest.join("SKILL.md").is_file() {
        return Err(invalid("The download has no SKILL.md, so it is not a skill."));
    }
    Ok(revision)
}

async fn index(http: &Http, domain: &str) -> Result<(String, Value), AppError> {
    let mut last = None;
    for p in [".well-known/agent-skills", ".well-known/skills"] {
        let url = format!("https://{domain}/{p}/index.json");
        // A site can answer a path it does not serve with its HTML shell and a
        // 200 (open.feishu.cn does), so only a real index ends the search.
        match http.get(&url, LIMIT, Some("application/json")).await {
            Ok(b) => {
                if let Ok(v) = serde_json::from_slice::<Value>(&b) {
                    if v.get("skills").and_then(Value::as_array).is_some() {
                        return Ok((url, v));
                    }
                }
            }
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| network(format!("{domain} does not publish a skills index."))))
}

fn unzip(bytes: &[u8], dest: &Path) -> Result<(), AppError> {
    let bad = |e: zip::result::ZipError| invalid(format!("The site's archive could not be read ({e})."));
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(bad)?;
    if z.len() > MAX_FILES {
        return Err(invalid("The site's archive has too many files."));
    }
    // Check every header before writing anything.
    let mut declared = 0u64;
    for i in 0..z.len() {
        let e = z.by_index(i).map_err(bad)?;
        safe_rel(e.name().trim_end_matches('/'))?;
        if let Some(mode) = e.unix_mode() {
            let k = mode & 0o170000;
            if k != 0 && k != 0o100000 && k != 0o040000 {
                return Err(invalid("The site's archive contains a link or special file. Nothing was installed."));
            }
        }
        declared = declared.saturating_add(e.size());
        if declared > LIMIT {
            return Err(invalid("The site's archive is larger than 50 MB unpacked."));
        }
    }
    let io = |e: std::io::Error| AppError::new(ErrorKind::Io, format!("Cannot save the download: {e}"));
    let mut left = LIMIT;
    for i in 0..z.len() {
        let mut e = z.by_index(i).map_err(bad)?;
        let out = dest.join(safe_rel(e.name().trim_end_matches('/'))?);
        if e.is_dir() {
            std::fs::create_dir_all(&out).map_err(io)?;
            continue;
        }
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p).map_err(io)?;
        }
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&out).map_err(io)?;
        left -= std::io::copy(&mut (&mut e).take(left), &mut f).map_err(io)?;
        if e.read(&mut [0u8; 1]).map_err(io)? != 0 {
            return Err(invalid("The site's archive is larger than it says."));
        }
    }
    Ok(())
}
