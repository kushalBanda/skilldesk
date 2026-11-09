//! Claude memories: `~/.claude/projects/<encoded project>/memory/*.md`, one
//! fact per file with frontmatter (`name`, `description`, `type` or
//! `metadata.type`), indexed by that folder's `MEMORY.md`.
//!
//! Read only. Memories that say the same thing are clustered by word overlap
//! of name + description within one type; stale flags mark a project folder
//! that is gone and project notes whose deadline or "as of" date is past.

use crate::env::Env;
use crate::model::{AgentKey, Item, ItemId, ItemKind, ItemStatus, KindFacts, MemoryFacts, MemoryScope, MemoryType, SourceRef, StaleFlag};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// Two memories of one type with at least this word overlap (Jaccard) say the
/// same thing. Tuned on real files: the five "never commit" notes link at
/// 0.33–0.57; the closest unrelated pair is 0.24.
const SAME: f64 = 0.3;

/// A project note is stale when its newest deadline or snapshot date is this many days back.
const PAST_DAYS: i64 = 14;

pub fn scan(env: &Env) -> Vec<Item> {
    scan_at(env, chrono::Local::now().date_naive())
}

pub fn scan_at(env: &Env, today: chrono::NaiveDate) -> Vec<Item> {
    let root = env.home.join(".claude/projects");
    let Ok(dirs) = std::fs::read_dir(&root) else { return Vec::new() };
    let known = crate::parse::claude::project_keys(env);
    let mut found: Vec<(Item, HashSet<String>)> = Vec::new();
    for d in dirs.flatten() {
        let mem = d.path().join("memory");
        let Ok(files) = std::fs::read_dir(&mem) else { continue };
        let encoded = d.file_name().to_string_lossy().into_owned();
        let project = project_for(&encoded, &known);
        let gone = project.as_ref().is_none_or(|p| !p.is_dir());
        let label = project
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| fallback_label(env, &encoded));
        let index_path = mem.join("MEMORY.md");
        let index = std::fs::read_to_string(&index_path).unwrap_or_default();
        let mut paths: Vec<PathBuf> = files
            .flatten()
            .map(|f| f.path())
            .filter(|p| p.extension().is_some_and(|e| e == "md") && p.file_name().is_some_and(|n| n != "MEMORY.md"))
            .collect();
        paths.sort();
        for path in paths {
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let fm = Front::parse(&text);
            let name = fm.name.clone().unwrap_or_else(|| file.trim_end_matches(".md").to_string());
            let mut stale = Vec::new();
            if gone {
                stale.push(StaleFlag::RepoGone);
            }
            if matches!(fm.mem_type, MemoryType::Project | MemoryType::Reference) {
                if let Some(date) = newest_date(body(&text)).filter(|d| (today - *d).num_days() >= PAST_DAYS) {
                    stale.push(StaleFlag::PastDate { date: date.to_string() });
                }
            }
            let modified = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
            let words = words(&format!("{} {}", name.replace(['-', '_'], " "), fm.description.as_deref().unwrap_or("")));
            let item = Item {
                id: ItemId::derive(ItemKind::Memory, &path, ""),
                kind: ItemKind::Memory,
                name,
                agents: vec![AgentKey("claude_code".into())],
                status: ItemStatus::Ok,
                sources: vec![SourceRef { path: path.clone(), line: None }],
                usage: None,
                facts: KindFacts::Memory(MemoryFacts {
                    mem_type: fm.mem_type,
                    // Claude's memory for the home folder applies in every session started there.
                    scope: if project.as_deref() == Some(env.home.as_path()) { MemoryScope::User } else { MemoryScope::Project },
                    description: fm.description,
                    project: project.clone(),
                    project_label: label.clone(),
                    index: index.contains(&format!("({file})")).then(|| index_path.clone()),
                    modified,
                    stale,
                    cluster: None,
                }),
            };
            found.push((item, words));
        }
    }
    cluster(&mut found);
    let mut items: Vec<Item> = found.into_iter().map(|(i, _)| i).collect();
    items.extend(instructions(env, &known));
    items.sort_by(|a, b| label_of(a).cmp(label_of(b)).then(a.name.cmp(&b.name)));
    items
}

fn label_of(i: &Item) -> &str {
    match &i.facts {
        KindFacts::Memory(f) => &f.project_label,
        _ => "",
    }
}

/// Claude names a project folder by its path with every non-alphanumeric
/// character as `-`. Known projects first (also ones that are gone), then the
/// folders on disk.
fn project_for(encoded: &str, known: &[PathBuf]) -> Option<PathBuf> {
    known.iter().find(|p| encode(p) == encoded).cloned().or_else(|| walk(Path::new("/"), encoded.strip_prefix('-')?))
}

fn encode(p: &Path) -> String {
    p.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// Rebuild `/a/b-c/d` from `a-b-c-d` by trying the folders that exist.
fn walk(dir: &Path, rest: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let enc = encode(Path::new(&name));
        if !e.path().is_dir() || enc.is_empty() {
            continue;
        }
        if rest == enc {
            return Some(e.path());
        }
        if let Some(r) = rest.strip_prefix(&format!("{enc}-")) {
            if let Some(p) = walk(&e.path(), r) {
                return Some(p);
            }
        }
    }
    None
}

fn fallback_label(env: &Env, encoded: &str) -> String {
    let home = encode(&env.home);
    let rest = encoded.strip_prefix(&home).unwrap_or(encoded).trim_start_matches('-');
    rest.strip_prefix("Github-").unwrap_or(rest).to_string()
}

struct Front {
    name: Option<String>,
    description: Option<String>,
    mem_type: MemoryType,
}

impl Front {
    fn parse(text: &str) -> Front {
        let fm = text
            .strip_prefix('\u{feff}')
            .unwrap_or(text)
            .strip_prefix("---")
            .and_then(|r| r.find("\n---").map(|e| &r[..e]))
            .and_then(|f| yaml_serde::from_str::<serde_json::Value>(f).ok())
            .unwrap_or_default();
        let s = |v: Option<&serde_json::Value>| v.and_then(|v| v.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let ty = s(fm.get("type")).or_else(|| s(fm.pointer("/metadata/type")));
        Front {
            name: s(fm.get("name")),
            description: s(fm.get("description")),
            mem_type: match ty.as_deref() {
                Some("user") => MemoryType::User,
                Some("feedback") => MemoryType::Feedback,
                Some("project") => MemoryType::Project,
                Some("reference") => MemoryType::Reference,
                _ => MemoryType::Other,
            },
        }
    }
}

/// Text after the frontmatter (its `modified:` stamp is not a date the note names).
pub fn body(text: &str) -> &str {
    text.strip_prefix("---")
        .and_then(|r| r.find("\n---").map(|e| &r[e + 4..]))
        .map(|r| r.trim_start_matches(['-']).trim_start())
        .unwrap_or(text)
}

/// Words that make a date a deadline or a snapshot ("Due 2026-10-06",
/// "as of 2026-10-17"), not a decision date that stays true.
const TIMED: &[&str] = &["due", "by", "until", "deadline", "scheduled", "as of", "state on", "before"];

/// Newest date the text names right after one of `TIMED`.
fn newest_date(text: &str) -> Option<chrono::NaiveDate> {
    let b = text.as_bytes();
    let mut best = None;
    for i in 0..b.len().saturating_sub(9) {
        let w = &b[i..i + 10];
        let shape = w.iter().enumerate().all(|(k, c)| if k == 4 || k == 7 { *c == b'-' } else { c.is_ascii_digit() });
        let edge = |j: Option<&u8>| j.is_none_or(|c| !c.is_ascii_digit());
        if !(shape && edge(i.checked_sub(1).and_then(|j| b.get(j))) && edge(b.get(i + 10))) {
            continue;
        }
        let mut from = i.saturating_sub(14);
        while !text.is_char_boundary(from) {
            from += 1;
        }
        let before = text[from..i].to_lowercase();
        let lead = before.trim_end_matches([' ', ':', '*', '(']);
        if !TIMED.iter().any(|t| lead.ends_with(t) && lead[..lead.len() - t.len()].chars().last().is_none_or(|c| !c.is_ascii_alphanumeric())) {
            continue;
        }
        if let Ok(d) = chrono::NaiveDate::parse_from_str(std::str::from_utf8(w).unwrap_or(""), "%Y-%m-%d") {
            best = best.max(Some(d));
        }
    }
    best
}

const STOP: &[&str] = &[
    "the", "and", "for", "with", "are", "its", "this", "that", "these", "those", "from", "not", "never", "don", "dont",
    "user", "users", "claude", "feedback", "project", "reference", "memory", "always", "any", "all", "every", "only",
    "when", "without", "explicit", "explicitly", "unless", "their", "them", "they", "you", "your", "leave", "let",
    "lets", "wants", "want",
];

fn words(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| w.len() >= 3 && !STOP.contains(w))
        .map(|w| if w.len() > 4 && w.ends_with('s') { w[..w.len() - 1].to_string() } else { w.to_string() })
        .collect()
}

fn cluster(found: &mut [(Item, HashSet<String>)]) {
    let n = found.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn root(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let ty = |i: &Item| match &i.facts {
        KindFacts::Memory(f) => Some(f.mem_type),
        _ => None,
    };
    for a in 0..n {
        for b in a + 1..n {
            let (wa, wb) = (&found[a].1, &found[b].1);
            if ty(&found[a].0) != ty(&found[b].0) || wa.is_empty() || wb.is_empty() {
                continue;
            }
            let inter = wa.intersection(wb).count() as f64;
            if inter / (wa.union(wb).count() as f64) >= SAME {
                let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
                parent[ra] = rb;
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..n {
        let r = root(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    for members in groups.values().filter(|m| m.len() > 1) {
        let mut ids: Vec<&str> = members.iter().map(|&i| found[i].0.id.0.as_str()).collect();
        ids.sort_unstable();
        let key = crate::actions::sha(&ids.join(","))[..12].to_string();
        for &i in members {
            if let KindFacts::Memory(f) = &mut found[i].0.facts {
                f.cluster = Some(key.clone());
            }
        }
    }
}

fn installed(env: &Env, key: &str) -> bool {
    crate::adapters::find(key).is_some_and(|a| a.is_installed(env)) && !crate::adapters::is_off(key)
}

fn modified(p: &Path) -> Option<String> {
    let t = std::fs::metadata(p).and_then(|m| m.modified()).ok()?;
    Some(chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339())
}

/// Agents' instructions files: each agent's global file (user scope) and the
/// `CLAUDE.md` / `AGENTS.md` / `GEMINI.md` at the root of each known project.
fn instructions(env: &Env, projects: &[PathBuf]) -> Vec<Item> {
    let h = &env.home;
    let mut out = Vec::new();
    let mut push = |path: PathBuf, agents: Vec<AgentKey>, scope: MemoryScope, project: Option<PathBuf>, label: String| {
        if agents.is_empty() || !path.is_file() {
            return;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let first = body(&text).lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("<!--")).map(|l| l.trim_start_matches('#').trim().chars().take(140).collect::<String>());
        let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        out.push(Item {
            id: ItemId::derive(ItemKind::Memory, &path, "instructions"),
            kind: ItemKind::Memory,
            name: file,
            agents,
            status: ItemStatus::Ok,
            sources: vec![SourceRef { path: path.clone(), line: None }],
            usage: None,
            facts: KindFacts::Memory(MemoryFacts {
                mem_type: MemoryType::Instructions,
                scope,
                description: first,
                project,
                project_label: label,
                index: None,
                modified: modified(&path),
                stale: vec![],
                cluster: None,
            }),
        });
    };
    let user = "All projects".to_string();
    for (key, path) in [
        ("claude_code", h.join(".claude/CLAUDE.md")),
        ("codex", h.join(".codex/AGENTS.md")),
        ("gemini_cli", h.join(".gemini/GEMINI.md")),
        ("opencode", env.resolve(".config/opencode/AGENTS.md")),
        ("github_copilot", h.join(".copilot/copilot-instructions.md")),
    ] {
        if installed(env, key) {
            push(path, vec![AgentKey(key.into())], MemoryScope::User, None, user.clone());
        }
    }
    let agents_md: Vec<AgentKey> = ["codex", "opencode", "cursor", "github_copilot"]
        .into_iter()
        .filter(|k| installed(env, k))
        .map(|k| AgentKey(k.into()))
        .collect();
    let mut seen = HashSet::new();
    for p in projects {
        if p == h || !p.is_dir() || !seen.insert(p.clone()) {
            continue;
        }
        let label = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let one = |k: &str| if installed(env, k) { vec![AgentKey(k.into())] } else { vec![] };
        push(p.join("CLAUDE.md"), one("claude_code"), MemoryScope::Project, Some(p.clone()), label.clone());
        push(p.join("AGENTS.md"), agents_md.clone(), MemoryScope::Project, Some(p.clone()), label.clone());
        push(p.join("GEMINI.md"), one("gemini_cli"), MemoryScope::Project, Some(p.clone()), label);
    }
    out
}
