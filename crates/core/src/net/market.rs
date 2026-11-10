//! Marketplace screen logic (ADR 0016): lists with an offline cache, download
//! of one skill into SkillDesk's data folder, and update checks for skills
//! installed from here. The install itself is the skill install action (a copy
//! into each agent's folder); this module fetches, and records in SkillDesk's
//! database where an installed skill came from.

use super::skillssh::{self, Board, MarketSkill};
use super::{github, site, Http};
use crate::core::Core;
use crate::error::{AppError, ErrorKind};
use crate::install::InstallPreview;
use crate::model::{ItemKind, KindFacts};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use ts_rs::TS;

/// Update checks per open: the unauthenticated GitHub limit is 60 an hour.
const CHECKS_PER_OPEN: usize = 40;
const RECHECK_SECS: i64 = 15 * 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum RowState {
    Available,
    /// Installed from this source; `update` once a check found a newer folder.
    Installed { name: String, update: bool },
    /// A skill of this name is on this Mac but did not come from here.
    SameName { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MarketRow {
    pub skill: MarketSkill,
    pub rank: u32,
    pub state: RowState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MarketList {
    pub rows: Vec<MarketRow>,
    /// RFC 3339; when the list was read from skills.sh.
    pub fetched_at: Option<String>,
    /// The request failed and these rows are the last list read.
    pub offline: bool,
    /// Why the list is old or missing (offline, or skills.sh changed its page).
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Cached {
    at: String,
    skills: Vec<MarketSkill>,
}

/// Where a marketplace download came from (kept until it is installed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketOrigin {
    pub source: String,
    pub skill_id: String,
    pub path: Option<String>,
    pub revision: Option<String>,
}

const INSTALLED_KEY: &str = "market.installed";

/// A skill installed from the marketplace (recorded in SkillDesk's database).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InstalledSkill {
    /// Skill folder name in the agent folders.
    pub name: String,
    pub source: String,
    pub skill_id: String,
    pub path: Option<String>,
    pub revision: Option<String>,
    pub installed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum UpdateState {
    Current,
    Available { #[ts(type = "string")] revision: String },
    /// Not checked this hour (GitHub limit) or not checkable (site skills).
    Unchecked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UpdateRow {
    pub skill: InstalledSkill,
    pub state: UpdateState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Updates {
    pub rows: Vec<UpdateRow>,
    pub checked_at: Option<String>,
    pub note: Option<String>,
}

/// A downloaded skill, ready for the skill install action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MarketFetched {
    pub preview: InstallPreview,
    /// SKILL.md as downloaded (first 256 KB).
    pub skill_md: String,
    pub revision: Option<String>,
    /// The folder inside the repository.
    pub path: Option<String>,
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn recorded(core: &Core) -> Vec<InstalledSkill> {
    core.store.setting(INSTALLED_KEY).ok().flatten().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// Skills installed from the marketplace that still have a folder in some
/// agent (local reads only).
pub fn installed(core: &Core) -> Vec<InstalledSkill> {
    let present: std::collections::HashSet<String> = core
        .store
        .items(ItemKind::Skill)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|i| match &i.facts {
            KindFacts::Skill(f) if !f.locations.is_empty() => f.home.file_name().map(|n| n.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    let mut out: Vec<InstalledSkill> = recorded(core).into_iter().filter(|s| present.contains(&s.name)).collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// After an install from a marketplace download: remember its source, so the
/// Marketplace can show it as installed and check it for updates.
pub(crate) fn record_install(core: &Core, from: &Path, name: &str) {
    let Some(o) = core.origin_of(from) else { return };
    let mut list = recorded(core);
    list.retain(|s| s.name != name);
    list.push(InstalledSkill { name: name.to_string(), source: o.source, skill_id: o.skill_id, path: o.path, revision: o.revision, installed_at: now() });
    if let Ok(json) = serde_json::to_string(&list) {
        let _ = core.store.set_setting(INSTALLED_KEY, &json);
    }
}

fn cached_updates(core: &Core) -> Option<(String, Vec<(String, UpdateState)>)> {
    serde_json::from_str(&core.store.setting("market.updates").ok()??).ok()
}

fn rows(core: &Core, skills: Vec<MarketSkill>) -> Vec<MarketRow> {
    let mine = installed(core);
    let updates = cached_updates(core).map(|(_, u)| u).unwrap_or_default();
    let names: std::collections::HashSet<String> = core
        .store
        .items(ItemKind::Skill)
        .unwrap_or_default()
        .into_iter()
        // Only skills with a folder on this Mac; usage-only ("gone") names do not clash.
        .filter(|i| matches!(&i.facts, KindFacts::Skill(f) if !f.locations.is_empty()))
        .map(|i| i.name.to_ascii_lowercase())
        .collect();
    skills
        .into_iter()
        .enumerate()
        .map(|(i, s)| {
            let state = if let Some(m) = mine.iter().find(|m| m.source == s.source && m.skill_id == s.skill_id) {
                let update = updates.iter().any(|(n, u)| n == &m.name && matches!(u, UpdateState::Available { .. }));
                RowState::Installed { name: m.name.clone(), update }
            } else if names.contains(&s.name.to_ascii_lowercase()) || names.contains(&s.skill_id.to_ascii_lowercase()) {
                RowState::SameName { name: s.name.clone() }
            } else {
                RowState::Available
            };
            MarketRow { skill: s, rank: i as u32 + 1, state }
        })
        .collect()
}

/// A leaderboard. On failure the last list read is returned with `offline`.
pub async fn board(core: &Core, board: Board) -> Result<MarketList, AppError> {
    let key = format!("market.board.{}", board.as_str());
    let fetched = match Http::new(None) {
        Ok(http) => skillssh::leaderboard(&http, board).await,
        Err(e) => Err(e),
    };
    match fetched {
        Ok(skills) => {
            let at = now();
            if let Ok(json) = serde_json::to_string(&Cached { at: at.clone(), skills: skills.clone() }) {
                let _ = core.store.set_setting(&key, &json);
            }
            Ok(MarketList { rows: rows(core, skills), fetched_at: Some(at), offline: false, note: None })
        }
        Err(e) => {
            let cached: Option<Cached> = core.store.setting(&key).ok().flatten().and_then(|s| serde_json::from_str(&s).ok());
            match cached {
                Some(c) => Ok(MarketList { rows: rows(core, c.skills), fetched_at: Some(c.at), offline: e.kind == ErrorKind::Network, note: Some(e.message) }),
                None => Err(e),
            }
        }
    }
}

pub async fn search(core: &Core, query: &str) -> Result<MarketList, AppError> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(MarketList { rows: vec![], fetched_at: None, offline: false, note: None });
    }
    let http = Http::new(None)?;
    let skills = skillssh::search(&http, q, 60).await?;
    Ok(MarketList { rows: rows(core, skills), fetched_at: Some(now()), offline: false, note: None })
}

/// Skills installed from here, with what the last check found. Checks GitHub
/// when `check` is set or the last check is older than 15 minutes.
pub async fn updates(core: &Core, check: bool) -> Result<Updates, AppError> {
    let mine = installed(core);
    let prev = cached_updates(core);
    let fresh = prev
        .as_ref()
        .and_then(|(at, _)| chrono::DateTime::parse_from_rfc3339(at).ok())
        .is_some_and(|t| (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_seconds() < RECHECK_SECS);
    let known = |n: &str| prev.as_ref().and_then(|(_, u)| u.iter().find(|(x, _)| x == n).map(|(_, s)| s.clone()));
    if mine.is_empty() || (fresh && !check) {
        let rows = mine.into_iter().map(|s| UpdateRow { state: known(&s.name).unwrap_or(UpdateState::Unchecked), skill: s }).collect();
        return Ok(Updates { rows, checked_at: prev.map(|(at, _)| at), note: None });
    }
    let http = Http::new(None)?;
    let mut out = Vec::new();
    let mut note = None;
    let mut checked = 0usize;
    for s in mine {
        let state = if s.source.contains('/') && checked < CHECKS_PER_OPEN && note.is_none() {
            checked += 1;
            match github::folder_commit(&http, &s.source, s.path.as_deref().unwrap_or(""), None).await {
                Ok(Some(rev)) if Some(&rev) != s.revision.as_ref() => UpdateState::Available { revision: rev },
                Ok(_) => UpdateState::Current,
                Err(e) => {
                    note = Some(e.message);
                    known(&s.name).unwrap_or(UpdateState::Unchecked)
                }
            }
        } else {
            known(&s.name).unwrap_or(UpdateState::Unchecked)
        };
        out.push(UpdateRow { skill: s, state });
    }
    let at = now();
    let save: Vec<(String, UpdateState)> = out.iter().map(|r| (r.skill.name.clone(), r.state.clone())).collect();
    if let Ok(json) = serde_json::to_string(&(at.clone(), save)) {
        let _ = core.store.set_setting("market.updates", &json);
    }
    Ok(Updates { rows: out, checked_at: Some(at), note })
}

fn downloads(core: &Core) -> PathBuf {
    core.env.data_dir.join("downloads")
}

/// Downloads older than an hour are SkillDesk's own leftovers.
fn prune(root: &Path) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    for e in rd.flatten() {
        let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|d| d.as_secs() > 3600);
        if old {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// Downloads one skill to `<data>/downloads/<id>/<skill>` and returns what
/// installing it would do. No agent folder changes.
pub async fn fetch(core: &Core, source: &str, skill_id: &str) -> Result<MarketFetched, AppError> {
    let folder = crate::fsx::path_guard::sanitize_name(skill_id);
    if folder.is_empty() || folder != skill_id {
        return Err(AppError::new(ErrorKind::InvalidInput, format!("{skill_id} is not a skill name SkillDesk can use.")));
    }
    let root = downloads(core);
    prune(&root);
    let token = crate::actions::new_id();
    let dir = root.join(&token).join(&folder);
    let cleanup = || {
        let _ = std::fs::remove_dir_all(root.join(&token));
    };
    let got: Result<(Option<String>, Option<String>), AppError> = async {
        if source.contains('/') {
            github::check_repo(source)?;
            let http = Http::new(None)?;
            let sha = github::head(&http, source).await?;
            let path = match github::archive(&http, source, &sha).await? {
                Some(bytes) => github::extract_skill(&bytes, skill_id, &dir)?,
                None => github::fetch_skill_files(&http, source, &sha, skill_id, &dir).await?,
            };
            let rev = github::folder_commit(&http, source, &path, Some(&sha)).await.ok().flatten().unwrap_or(sha);
            Ok((Some(rev), Some(path)))
        } else {
            site::check_domain(source)?;
            let http = Http::new(Some(source))?;
            Ok((site::download(&http, source, skill_id, &dir).await?, None))
        }
    }
    .await;
    let (revision, path) = match got {
        Ok(x) => x,
        Err(e) => {
            cleanup();
            return Err(e);
        }
    };
    let preview = match core.skill_inspect(&dir) {
        Ok(p) => p,
        Err(e) => {
            cleanup();
            return Err(e);
        }
    };
    let skill_md = crate::parse::skills::marker_file(&dir)
        .and_then(|f| std::fs::read(f).ok())
        .map(|b| String::from_utf8_lossy(&b[..b.len().min(256 * 1024)]).into_owned())
        .unwrap_or_default();
    core.remember_origin(dir, MarketOrigin { source: source.into(), skill_id: skill_id.into(), path: path.clone(), revision: revision.clone() });
    Ok(MarketFetched { preview, skill_md, revision, path })
}

#[cfg(test)]
mod tests {
    /// ADR 0016: no request at launch or during a scan. Opening the core,
    /// scanning and reading every screen's data sends nothing.
    #[test]
    fn marketplace_no_request_at_launch_or_scan() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().canonicalize().unwrap();
        std::fs::create_dir_all(home.join(".claude/skills/demo")).unwrap();
        std::fs::write(home.join(".claude/skills/demo/SKILL.md"), "---\nname: demo\n---\n").unwrap();
        let before = super::super::requests_made();
        let core = crate::core::Core::open(crate::env::Env::for_test(&home, &home.join("data"))).unwrap();
        crate::scan::run_scan(&core, &crate::scan::NoSink).unwrap();
        let _ = core.overview().unwrap();
        let _ = super::installed(&core);
        assert_eq!(super::super::requests_made(), before);
    }
}

/// Avatars older than a week are fetched again; a failed fetch keeps the old one.
const AVATAR_TTL_SECS: u64 = 7 * 24 * 3600;
const AVATAR_LIMIT: u64 = 256 * 1024;

/// The GitHub owner's avatar as a `data:` URL, cached in `<data>/avatars`.
/// The page never loads remote images (CSP); this request is the Marketplace's
/// own: `github.com/<owner>.png`, redirected to `avatars.githubusercontent.com` (ADR 0016).
pub async fn avatar(core: &Core, owner: &str) -> Result<String, AppError> {
    github::check_repo(&format!("{owner}/x"))?;
    let dir = core.env.data_dir.join("avatars");
    let file = dir.join(owner.to_ascii_lowercase());
    let cached = std::fs::read(&file).ok();
    let fresh = std::fs::metadata(&file).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|d| d.as_secs() < AVATAR_TTL_SECS);
    if let (Some(b), true) = (&cached, fresh) {
        if let Some(url) = data_url(b) {
            return Ok(url);
        }
    }
    let got = async {
        let http = Http::new(None)?;
        // github.com/<owner>.png redirects to the right avatar for users and orgs alike.
        http.get(&format!("https://github.com/{owner}.png?size=96"), AVATAR_LIMIT, Some("image/png,image/jpeg")).await
    }
    .await;
    match got {
        Ok(b) => {
            let url = data_url(&b).ok_or_else(|| AppError::new(ErrorKind::Parse, "GitHub sent an avatar that is not a PNG or JPEG."))?;
            let _ = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&file, &b));
            Ok(url)
        }
        Err(e) => cached.as_deref().and_then(data_url).ok_or(e),
    }
}

/// PNG or JPEG only, judged by the bytes, not the server's word.
fn data_url(b: &[u8]) -> Option<String> {
    let mime = if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else {
        return None;
    };
    Some(format!("data:{mime};base64,{}", base64(b)))
}

fn base64(b: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            out.push(if i <= c.len() { T[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

/// The skill's page on skills.sh (opened in the browser, never fetched here).
pub fn page_url(source: &str, skill_id: &str) -> Result<String, AppError> {
    let bad = || AppError::new(ErrorKind::InvalidInput, "Not a skills.sh skill.");
    let ok = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':'));
    if !ok(skill_id) {
        return Err(bad());
    }
    if source.contains('/') {
        github::check_repo(source)?;
        Ok(format!("https://skills.sh/{source}/{skill_id}"))
    } else if ok(source) {
        Ok(format!("https://skills.sh/site/{source}/{skill_id}"))
    } else {
        Err(bad())
    }
}

#[cfg(test)]
mod avatar_tests {
    #[test]
    fn base64_matches_reference() {
        assert_eq!(super::base64(b""), "");
        assert_eq!(super::base64(b"f"), "Zg==");
        assert_eq!(super::base64(b"fo"), "Zm8=");
        assert_eq!(super::base64(b"foo"), "Zm9v");
        assert_eq!(super::base64(b"foobar"), "Zm9vYmFy");
        assert!(super::data_url(b"<svg onload=x>").is_none(), "only PNG/JPEG bytes become data URLs");
    }
}
