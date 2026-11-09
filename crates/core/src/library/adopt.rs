//! Duplicate skills (ADR 0015 "Adopt"): the separate copies of one skill,
//! grouped by content, and a line diff between two copies.

use super::SIDECAR;
use crate::core::Core;
use crate::error::{AppError, ErrorKind};
use crate::model::{AgentKey, ItemId, KindFacts, LinkState, SkillFacts};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use ts_rs::TS;

const SKIP: &[&str] = &[".git", ".DS_Store", SIDECAR];

/// The skill's own folders: global real folders outside plugins and projects.
pub fn own_copies(f: &SkillFacts) -> Vec<&crate::model::SkillLocation> {
    f.locations
        .iter()
        .filter(|l| l.link == LinkState::RealDir && l.plugin.is_none() && l.project.is_none())
        .collect()
}

/// Content hash of a skill folder: relative paths and bytes, sorted; links,
/// `.git`, `.DS_Store` and the sidecar left out.
pub fn content_hash(dir: &Path) -> String {
    use sha2::{Digest, Sha256};
    let mut files: Vec<PathBuf> = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            if SKIP.contains(&e.file_name().to_string_lossy().as_ref()) {
                continue;
            }
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                stack.push(e.path());
            } else if ft.is_file() {
                files.push(e.path());
            }
        }
    }
    files.sort();
    let mut h = Sha256::new();
    for f in files {
        h.update(f.strip_prefix(dir).unwrap_or(&f).to_string_lossy().as_bytes());
        h.update([0]);
        h.update(std::fs::read(&f).unwrap_or_default());
        h.update([0]);
    }
    hex::encode(h.finalize())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CopyInfo {
    #[ts(type = "string")]
    pub path: PathBuf,
    pub agents: Vec<AgentKey>,
    /// 0 = the content most copies share; same number = same content.
    pub group: u32,
    pub in_library: bool,
    /// SKILL.md lines.
    pub lines: u32,
    pub files: u32,
    /// Lines that differ from the suggested keeper's SKILL.md (0 = same file).
    pub differs: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LinkInfo {
    #[ts(type = "string")]
    pub path: PathBuf,
    pub agents: Vec<AgentKey>,
    #[ts(type = "string")]
    pub to: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AdoptView {
    pub name: String,
    /// The library copy first (when there is one), then group 0, then the rest.
    pub copies: Vec<CopyInfo>,
    pub links: Vec<LinkInfo>,
    /// Suggested copy to keep: the library copy, else one from the largest group.
    #[ts(type = "string")]
    pub keep: PathBuf,
}

fn skill_md(dir: &Path) -> String {
    crate::parse::skills::marker_file(dir).and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default()
}

/// Lines on either side that are not part of an unchanged run.
pub fn changed_lines(a: &str, b: &str) -> u32 {
    use similar::{ChangeTag, TextDiff};
    TextDiff::from_lines(a, b).iter_all_changes().filter(|c| c.tag() == ChangeTag::Insert).count() as u32
}

pub fn view(core: &Core, id: &ItemId) -> Result<AdoptView, AppError> {
    let item = core.store.item(id)?.ok_or_else(|| AppError::new(ErrorKind::NotFound, "This skill is no longer in the inventory."))?;
    let KindFacts::Skill(f) = &item.facts else { return Err(AppError::new(ErrorKind::InvalidInput, "Not a skill.")) };
    let mut dirs: Vec<(PathBuf, Vec<AgentKey>, bool)> = Vec::new();
    if let Some(lib) = &f.library {
        dirs.push((lib.clone(), Vec::new(), true));
    }
    for l in own_copies(f) {
        dirs.push((l.path.clone(), l.agents.clone(), false));
    }
    // Group by content, largest group first.
    let hashes: Vec<String> = dirs.iter().map(|(p, _, _)| content_hash(p)).collect();
    let mut order: Vec<(String, usize)> = Vec::new();
    for h in &hashes {
        match order.iter_mut().find(|(x, _)| x == h) {
            Some((_, n)) => *n += 1,
            None => order.push((h.clone(), 1)),
        }
    }
    order.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    let group = |h: &str| order.iter().position(|(x, _)| x == h).unwrap_or(0) as u32;
    let keep_idx = dirs
        .iter()
        .position(|(_, _, lib)| *lib)
        .or_else(|| {
            let g0: Vec<usize> = (0..dirs.len()).filter(|&i| group(&hashes[i]) == 0).collect();
            g0.iter().copied().find(|&i| dirs[i].1.iter().any(|a| a.0 == "claude_code")).or(g0.first().copied())
        })
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, format!("{} has no folder of its own to adopt.", item.name)))?;
    let keep_md = skill_md(&dirs[keep_idx].0);
    let mut copies: Vec<CopyInfo> = dirs
        .iter()
        .zip(&hashes)
        .map(|((path, agents, lib), h)| {
            let md = skill_md(path);
            let (files, _) = super::dir_stats(path);
            CopyInfo {
                path: path.clone(),
                agents: agents.clone(),
                group: group(h),
                in_library: *lib,
                lines: md.lines().count() as u32,
                files: files.saturating_sub(u32::from(path.join(SIDECAR).exists())),
                differs: if md == keep_md { 0 } else { changed_lines(&keep_md, &md).max(1) },
            }
        })
        .collect();
    copies.sort_by_key(|c| (!c.in_library, c.group));
    let links = f
        .locations
        .iter()
        .filter_map(|l| match &l.link {
            LinkState::Link(to) if l.plugin.is_none() => Some(LinkInfo { path: l.path.clone(), agents: l.agents.clone(), to: to.clone() }),
            _ => None,
        })
        .collect();
    Ok(AdoptView { name: item.name.clone(), keep: dirs[keep_idx].0.clone(), copies, links })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum DiffRow {
    /// Lines `from..=to` (1-based, left side) match on both sides.
    Same { from: u32, to: u32 },
    /// One row of a changed block; either side may be empty.
    Change { left: Option<(u32, String)>, right: Option<(u32, String)> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SkillDiff {
    pub rows: Vec<DiffRow>,
    /// Other files (not SKILL.md) that differ or exist on one side only.
    pub other_files: Vec<String>,
}

fn files_of(dir: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut out = std::collections::BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            if SKIP.contains(&e.file_name().to_string_lossy().as_ref()) {
                continue;
            }
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                stack.push(e.path());
            } else if ft.is_file() {
                let rel = e.path().strip_prefix(dir).unwrap_or(&e.path()).to_string_lossy().into_owned();
                out.insert(rel, std::fs::read(e.path()).unwrap_or_default());
            }
        }
    }
    out
}

/// Line diff of two copies' SKILL.md; both must be copies of the same skill.
pub fn diff(core: &Core, id: &ItemId, a: &Path, b: &Path) -> Result<SkillDiff, AppError> {
    let v = view(core, id)?;
    let known = |p: &Path| v.copies.iter().any(|c| c.path == p);
    if !known(a) || !known(b) {
        return Err(AppError::new(ErrorKind::InvalidInput, "Those folders are not copies of this skill."));
    }
    use similar::{DiffOp, TextDiff};
    let (ta, tb) = (skill_md(a), skill_md(b));
    let la: Vec<&str> = ta.lines().collect();
    let lb: Vec<&str> = tb.lines().collect();
    let d = TextDiff::from_lines(&ta, &tb);
    let mut rows = Vec::new();
    for op in d.ops() {
        match *op {
            DiffOp::Equal { old_index, len, .. } => rows.push(DiffRow::Same { from: old_index as u32 + 1, to: (old_index + len) as u32 }),
            DiffOp::Delete { old_index, old_len, .. } => {
                for i in old_index..old_index + old_len {
                    rows.push(DiffRow::Change { left: Some((i as u32 + 1, la.get(i).unwrap_or(&"").to_string())), right: None });
                }
            }
            DiffOp::Insert { new_index, new_len, .. } => {
                for i in new_index..new_index + new_len {
                    rows.push(DiffRow::Change { left: None, right: Some((i as u32 + 1, lb.get(i).unwrap_or(&"").to_string())) });
                }
            }
            DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                for k in 0..old_len.max(new_len) {
                    let left = (k < old_len).then(|| (old_index as u32 + k as u32 + 1, la.get(old_index + k).unwrap_or(&"").to_string()));
                    let right = (k < new_len).then(|| (new_index as u32 + k as u32 + 1, lb.get(new_index + k).unwrap_or(&"").to_string()));
                    rows.push(DiffRow::Change { left, right });
                }
            }
        }
    }
    let (fa, fb) = (files_of(a), files_of(b));
    let marker = |k: &str| k == "SKILL.md" || k == "skill.md";
    let mut other: Vec<String> = fa.keys().chain(fb.keys()).filter(|k| !marker(k) && fa.get(*k) != fb.get(*k)).cloned().collect();
    other.sort();
    other.dedup();
    Ok(SkillDiff { rows, other_files: other })
}
