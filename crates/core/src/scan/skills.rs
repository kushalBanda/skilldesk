//! Skills inventory across every detected agent (ADR 0003, 0004).
//!
//! - Each skills folder (global and per project) is scanned once and credited
//!   to every agent that reads it.
//! - One row per skill name: links, copies, plugin and project copies of the
//!   same skill are its locations. Dangling links are locations too; a row whose
//!   every location dangles is `Broken`.
//! - Usage: `~/.claude.json` `skillUsage` (all-time). A Claude skill never used
//!   is `Unused`; a used name with nothing on disk is `Gone`. Rows whose only
//!   copies sit in disabled plugins are `Off`.

use crate::adapters;
use crate::env::Env;
use crate::model::{
    AgentKey, Item, ItemId, ItemKind, ItemStatus, KindFacts, LinkState, PluginRef, SkillFacts,
    SkillLocation, SourceRef, Usage,
};
use crate::parse::claude::{self, BUILTIN_SKILLS};
use crate::parse::skills::{self as detect, Found};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct SkillScan {
    pub items: Vec<Item>,
    /// Raw `skillUsage` entries (Dashboard "skills used").
    pub skills_used: Option<u64>,
}

#[derive(Default)]
struct Row {
    locations: Vec<SkillLocation>,
    /// First real copy: (canonical dir, frontmatter description).
    home: Option<(PathBuf, Option<String>)>,
}

pub fn scan(env: &Env) -> SkillScan {
    let claude_key = AgentKey("claude_code".into());
    let claude_on = adapters::find("claude_code").is_some_and(|a| a.is_installed(env));
    let projects = if claude_on { claude::project_paths(env) } else { Vec::new() };

    let mut rows: BTreeMap<String, Row> = BTreeMap::new();
    let mut not_skills: Vec<Item> = Vec::new();

    let mut add_skill = |path: PathBuf, agents: &[AgentKey], project: Option<PathBuf>, plugin: Option<PluginRef>| {
        let home = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        let meta = detect::read_meta(&home);
        let name = meta
            .name
            .as_deref()
            .map(|n| claude::bare_name(n).to_string())
            .unwrap_or_else(|| file_name(&home));
        let link = if std::fs::read_link(&path).is_ok() { LinkState::Link(home.clone()) } else { LinkState::RealDir };
        let row = rows.entry(name).or_default();
        if row.home.is_none() {
            row.home = Some((home, meta.description));
        }
        push_location(&mut row.locations, agents, path, link, project, plugin);
    };

    let mut dangling: Vec<(String, SkillLocation)> = Vec::new();
    for folder in adapters::skill_folders(env, &projects) {
        for found in detect::list_dir(&folder.path, folder.recursive) {
            match found {
                Found::Skill { path } => add_skill(path, &folder.agents, folder.project.clone(), None),
                Found::Dangling { path, target } => dangling.push((
                    file_name(&path),
                    SkillLocation {
                        agents: folder.agents.clone(),
                        path,
                        link: LinkState::Dangling(target),
                        project: folder.project.clone(),
                        plugin: None,
                    },
                )),
                Found::NotASkill { path } => not_skills.push(not_a_skill(&folder.agents, &path)),
            }
        }
    }
    if claude_on {
        for plugin in claude::installed_plugins(env) {
            for found in detect::list_dir(&plugin.install_path.join("skills"), false) {
                if let Found::Skill { path } = found {
                    let p = PluginRef { id: plugin.id.clone(), enabled: plugin.enabled };
                    add_skill(path, std::slice::from_ref(&claude_key), None, Some(p));
                }
            }
        }
    }
    for (name, loc) in dangling {
        rows.entry(name).or_default().locations.push(loc);
    }

    let usage = claude::skill_usage(env);
    let mut items: Vec<Item> = Vec::new();
    for (name, row) in &rows {
        let mut agents: Vec<AgentKey> = row
            .locations
            .iter()
            .filter(|l| !matches!(l.link, LinkState::Dangling(_)))
            .flat_map(|l| l.agents.clone())
            .collect();
        agents.sort();
        agents.dedup();
        let used = usage.as_ref().and_then(|u| u.by_name.get(name));
        let all_dangling = row.home.is_none();
        let only_disabled_plugins = row
            .locations
            .iter()
            .all(|l| l.plugin.as_ref().is_some_and(|p| !p.enabled));
        // Global Claude skills only: the counter is all-time, project skills are
        // per repo and many repos are never opened again (agent-data.md §3).
        let in_claude_outside_plugins = row
            .locations
            .iter()
            .any(|l| {
                l.plugin.is_none()
                    && l.project.is_none()
                    && l.agents.contains(&claude_key)
                    && !matches!(l.link, LinkState::Dangling(_))
            });
        let status = if all_dangling {
            ItemStatus::Broken
        } else if only_disabled_plugins {
            ItemStatus::Off
        } else if used.is_none() && usage.is_some() && in_claude_outside_plugins {
            ItemStatus::Unused
        } else {
            ItemStatus::Ok
        };
        let (home, description) = row
            .home
            .clone()
            .unwrap_or_else(|| (row.locations[0].path.clone(), None));
        items.push(Item {
            id: ItemId::derive(ItemKind::Skill, Path::new(""), &format!("name:{name}")),
            kind: ItemKind::Skill,
            name: name.clone(),
            agents,
            status,
            sources: vec![SourceRef { path: detect::marker_file(&home).unwrap_or_else(|| home.clone()), line: None }],
            usage: used.map(to_usage),
            facts: KindFacts::Skill(SkillFacts { description, home, locations: row.locations.clone() }),
        });
    }

    if let Some(u) = &usage {
        let claude_json = env.home.join(".claude.json");
        for (name, count) in &u.by_name {
            if rows.contains_key(name) || BUILTIN_SKILLS.contains(&name.as_str()) {
                continue;
            }
            items.push(Item {
                id: ItemId::derive(ItemKind::Skill, Path::new(""), &format!("name:{name}")),
                kind: ItemKind::Skill,
                name: name.clone(),
                agents: vec![claude_key.clone()],
                status: ItemStatus::Gone,
                sources: vec![SourceRef { path: claude_json.clone(), line: None }],
                usage: Some(to_usage(count)),
                facts: KindFacts::Skill(SkillFacts { description: None, home: claude_json.clone(), locations: vec![] }),
            });
        }
    }

    items.extend(not_skills);
    SkillScan { items, skills_used: usage.map(|u| u.raw_entries as u64) }
}

fn to_usage(u: &claude::UsageCount) -> Usage {
    Usage {
        count: u.count,
        last_used: u
            .last_used_ms
            .and_then(chrono::DateTime::from_timestamp_millis)
            .map(|d| d.to_rfc3339()),
        timing: None,
    }
}

fn push_location(
    locs: &mut Vec<SkillLocation>,
    agents: &[AgentKey],
    path: PathBuf,
    link: LinkState,
    project: Option<PathBuf>,
    plugin: Option<PluginRef>,
) {
    if let Some(existing) = locs.iter_mut().find(|l| l.path == path) {
        for a in agents {
            if !existing.agents.contains(a) {
                existing.agents.push(a.clone());
            }
        }
        return;
    }
    locs.push(SkillLocation { agents: agents.to_vec(), path, link, project, plugin });
}

fn not_a_skill(agents: &[AgentKey], path: &Path) -> Item {
    Item {
        id: ItemId::derive(ItemKind::Skill, path, "not_an_item"),
        kind: ItemKind::Skill,
        name: file_name(path),
        agents: agents.to_vec(),
        status: ItemStatus::NotAnItem,
        sources: vec![SourceRef { path: path.to_path_buf(), line: None }],
        usage: None,
        facts: KindFacts::Skill(SkillFacts {
            description: None,
            home: path.to_path_buf(),
            locations: vec![SkillLocation {
                agents: agents.to_vec(),
                path: path.to_path_buf(),
                link: LinkState::RealDir,
                project: None,
                plugin: None,
            }],
        }),
    }
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}
