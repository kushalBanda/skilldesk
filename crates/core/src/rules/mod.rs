//! Fix rules over the cached inventory → ranked Fix list (DESIGN §7.2).
//! Each fix names its items; its evidence hash changes when they change, which
//! brings back a dismissed fix.

use crate::actions::ActionRequest;
use crate::adapters;
use crate::core::Core;
use crate::error::{AppError, ErrorKind};
use crate::model::{Item, ItemId, ItemKind, ItemStatus, KindFacts, LinkState, SwitchKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

/// One card. Copy carries no file paths; the linked screen shows them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Fix {
    pub id: String,
    pub rule: String,
    /// 0–100; higher first.
    pub risk: u8,
    pub title: String,
    pub why: String,
    pub item_ids: Vec<ItemId>,
    /// The one-click action, when there is a safe one.
    pub action: Option<ActionRequest>,
    pub action_label: String,
    /// In-app route that shows the evidence ("Show me").
    pub link: String,
    pub evidence: String,
}

/// Hooks costing at least this much in total get their own card.
const SLOW_TOTAL_MS: u64 = 10 * 60_000;

fn agent_name(key: &str) -> String {
    adapters::find(key).map(|a| a.display_name.to_string()).unwrap_or_else(|| key.to_string())
}

fn evidence(ids: &[ItemId]) -> String {
    let mut v: Vec<&str> = ids.iter().map(|i| i.0.as_str()).collect();
    v.sort_unstable();
    crate::actions::sha(&v.join(","))
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn list_names(names: &[String], max: usize) -> String {
    let shown: Vec<&str> = names.iter().take(max).map(String::as_str).collect();
    let rest = names.len().saturating_sub(max);
    match (shown.len(), rest) {
        (0, _) => String::new(),
        (1, 0) => shown[0].to_string(),
        (_, 0) => format!("{} and {}", shown[..shown.len() - 1].join(", "), shown[shown.len() - 1]),
        _ => format!("{} and {rest} more", shown.join(", ")),
    }
}

fn fmt_ms(ms: u32) -> String {
    if ms >= 1000 { format!("{:.1} s", ms as f64 / 1000.0) } else { format!("{ms} ms") }
}

pub struct Inventory {
    pub skills: Vec<Item>,
    pub hooks: Vec<Item>,
    pub plugins: Vec<Item>,
    pub marketplaces: Vec<Item>,
    pub mcp: Vec<Item>,
    pub memories: Vec<Item>,
    pub secrets: Vec<crate::secrets::Secret>,
    pub storage: Option<crate::storage::StorageReport>,
}

impl Inventory {
    #[cfg(test)]
    pub fn load_for_test(core: &Core) -> Self {
        Self::load(core).unwrap()
    }

    fn load(core: &Core) -> Result<Self, AppError> {
        Ok(Self {
            skills: core.store.items(ItemKind::Skill)?,
            hooks: core.store.items(ItemKind::Hook)?,
            plugins: core.store.items(ItemKind::Plugin)?,
            marketplaces: core.store.items(ItemKind::Marketplace)?,
            mcp: core.store.items(ItemKind::McpServer)?,
            memories: core.store.items(ItemKind::Memory)?,
            secrets: core.store.secrets()?,
            storage: core.store.setting(crate::core::STORAGE_KEY)?.and_then(|t| serde_json::from_str(&t).ok()),
        })
    }
}

pub fn evaluate(inv: &Inventory) -> Vec<Fix> {
    let (skills, hooks) = (&inv.skills, &inv.hooks);
    let mut out = Vec::new();
    ghost_plugins(&inv.plugins, &mut out);
    unused_marketplaces(&inv.marketplaces, &mut out);
    never_called_mcp(&inv.mcp, &mut out);
    broken_hooks(hooks, &mut out);
    slow_hooks(hooks, &mut out);
    dangling_links(skills, &mut out);
    never_run_skills(skills, &mut out);
    duplicate_skills(skills, &mut out);
    duplicate_memories(&inv.memories, &mut out);
    secrets_in_history(&inv.secrets, &mut out);
    if let Some(r) = &inv.storage {
        reclaimable_disk(r, &mut out);
    }
    out.sort_by(|a, b| b.risk.cmp(&a.risk).then(b.item_ids.len().cmp(&a.item_ids.len())).then(a.id.cmp(&b.id)));
    out
}

/// Things agents make again or no longer read. History (old or orphan
/// transcripts) is the User's call, so it is left out of the card.
fn reclaimable_disk(r: &crate::storage::StorageReport, out: &mut Vec<Fix>) {
    use crate::storage::CandidateKind as C;
    let list: Vec<&crate::storage::Candidate> = r.candidates.iter().filter(|c| !c.kind.holds_history()).collect();
    let bytes: u64 = list.iter().map(|c| c.bytes).sum();
    if bytes < crate::storage::RULE_MIN_BYTES {
        return;
    }
    let names: Vec<String> = list
        .iter()
        .map(|c| {
            let what = match c.kind {
                C::CursorBackup => "Cursor's backup database".to_string(),
                C::ClaudeJobs => plural(c.path_count, "finished Claude job", "finished Claude jobs"),
                C::CodexTmp => "Codex's download area".to_string(),
                C::CursorLogs => plural(c.path_count, "old Cursor log folder", "old Cursor log folders"),
                C::Marketplaces => plural(c.path_count, "unused marketplace", "unused marketplaces"),
                C::PluginCache => plural(c.path_count, "old plugin version", "old plugin versions"),
                C::OrphanProjects | C::OldTranscripts => String::new(),
            };
            format!("{what} {}", fmt_size(c.bytes))
        })
        .collect();
    out.push(Fix {
        id: "reclaimable_disk".into(),
        rule: "reclaimable_disk".into(),
        risk: 20,
        title: format!("{} can go to the Trash.", fmt_size(bytes)),
        why: format!("{}. The agents make these again or no longer read them.", list_names(&names, 3)),
        evidence: list.iter().map(|c| format!("{}={}", c.kind.as_str(), c.path_count)).collect::<Vec<_>>().join(","),
        item_ids: vec![],
        action: None,
        action_label: "Review cleanup".into(),
        link: "/cleanup".into(),
    });
}

fn fmt_size(bytes: u64) -> String {
    if bytes >= 1 << 30 {
        format!("{:.1} GB", bytes as f64 / (1u64 << 30) as f64)
    } else {
        fmt_mb(bytes)
    }
}

/// Likely secrets, not ignored, in any agent's history.
fn secrets_in_history(secrets: &[crate::secrets::Secret], out: &mut Vec<Fix>) {
    use crate::secrets::{Confidence, SecretKind as K};
    let live: Vec<&crate::secrets::Secret> = secrets.iter().filter(|s| s.confidence == Confidence::Likely && !s.ignored).collect();
    if live.is_empty() {
        return;
    }
    let mut by: BTreeMap<K, usize> = BTreeMap::new();
    for s in &live {
        *by.entry(s.kind).or_default() += 1;
    }
    let mut kinds: Vec<(K, usize)> = by.into_iter().collect();
    kinds.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let names: Vec<String> = kinds
        .iter()
        .map(|(k, n)| {
            let (one, many) = match k {
                K::PrivateKey => ("private key", "private keys"),
                K::Anthropic => ("Anthropic key", "Anthropic keys"),
                K::OpenAi => ("OpenAI key", "OpenAI keys"),
                K::Stripe => ("Stripe key", "Stripe keys"),
                K::Aws => ("AWS key", "AWS keys"),
                K::GitHub => ("GitHub token", "GitHub tokens"),
                K::Slack => ("Slack token", "Slack tokens"),
                K::Google => ("Google key", "Google keys"),
                K::Jwt => ("JWT", "JWTs"),
                K::DatabaseUrl => ("database URL", "database URLs"),
                K::Generic => ("other value", "other values"),
            };
            plural(*n, one, many)
        })
        .collect();
    let ids: Vec<ItemId> = live.iter().map(|s| ItemId(s.id.clone())).collect();
    out.push(Fix {
        id: "secrets_in_history".into(),
        rule: "secrets_in_history".into(),
        risk: 70,
        title: format!("{} sit in agent history.", plural(live.len(), "secret", "secrets")),
        why: format!("{}. Rotate them at the provider, then hide the copies on disk.", list_names(&names, 4)),
        evidence: evidence(&ids),
        item_ids: ids,
        action: None,
        action_label: "Review secrets".into(),
        link: "/secrets".into(),
    });
}

/// Memories in a cluster (`scan::memory`) say the same thing; one global
/// rule replaces them. Merging needs the rule's words, so the card opens the
/// merge sheet instead of running an action.
fn duplicate_memories(memories: &[Item], out: &mut Vec<Fix>) {
    let mut groups: BTreeMap<String, Vec<&Item>> = BTreeMap::new();
    for m in memories {
        if let KindFacts::Memory(f) = &m.facts {
            if let Some(c) = &f.cluster {
                groups.entry(c.clone()).or_default().push(m);
            }
        }
    }
    for (key, list) in groups {
        let ids: Vec<ItemId> = list.iter().map(|m| m.id.clone()).collect();
        let mut projects: Vec<String> = list
            .iter()
            .filter_map(|m| match &m.facts {
                KindFacts::Memory(f) => Some(f.project_label.replace('-', " ")),
                _ => None,
            })
            .collect();
        projects.sort();
        projects.dedup();
        let gist = list
            .iter()
            .map(|m| m.name.as_str())
            .min_by_key(|n| (n.len(), *n))
            .unwrap_or("")
            .trim_start_matches("feedback")
            .replace(['-', '_'], " ")
            .trim()
            .to_string();
        let n = list.len();
        out.push(Fix {
            id: format!("duplicate_memories:{key}"),
            rule: "duplicate_memories".into(),
            risk: 40,
            title: format!("{n} memories say the same rule."),
            why: format!("\u{201c}{gist}\u{201d}, saved in {}. One global rule replaces them.", plural(projects.len(), "project", "projects")),
            evidence: evidence(&ids),
            item_ids: ids,
            action: None,
            action_label: "Merge into one rule".into(),
            link: format!("/memory?cluster={key}"),
        });
    }
}

fn broken_hooks(hooks: &[Item], out: &mut Vec<Fix>) {
    let mut by_agent: BTreeMap<String, Vec<&Item>> = BTreeMap::new();
    for h in hooks.iter().filter(|h| h.status == ItemStatus::Broken) {
        if let Some(a) = h.agents.first() {
            by_agent.entry(a.0.clone()).or_default().push(h);
        }
    }
    for (agent, list) in by_agent {
        let name = agent_name(&agent);
        let ids: Vec<ItemId> = list.iter().map(|h| h.id.clone()).collect();
        let switchable: Vec<ItemId> = list
            .iter()
            .filter(|h| matches!(&h.facts, KindFacts::Hook(f) if matches!(f.switch, SwitchKind::Stash | SwitchKind::Native)))
            .map(|h| h.id.clone())
            .collect();
        let events: Vec<String> = {
            let mut e: Vec<String> = list
                .iter()
                .filter_map(|h| match &h.facts {
                    KindFacts::Hook(f) => Some(f.event.clone()),
                    _ => None,
                })
                .collect();
            e.dedup();
            e
        };
        let on_prompt = events.iter().any(|e| {
            let e = e.to_lowercase();
            e.contains("submitprompt") || e.contains("promptsubmit") || e == "beforeagent"
        });
        let n = list.len();
        let why = if on_prompt {
            format!("{} before every {name} prompt and fail each time.", if n == 1 { "It runs" } else { "They run" })
        } else {
            format!("{} on {} and fail each time.", if n == 1 { "It runs" } else { "They run" }, list_names(&events, 3))
        };
        let title = if n == 1 {
            format!("A {name} hook calls a script that is gone.")
        } else {
            format!("{n} {name} hooks call a script that is gone.")
        };
        let (action, action_label) = if switchable.is_empty() {
            (None, "Show hooks".to_string())
        } else {
            let k = switchable.len();
            (
                Some(ActionRequest::HookSetEnabled { ids: switchable, on: false }),
                format!("Disable {}", plural(k, "hook", "hooks")),
            )
        };
        out.push(Fix {
            id: format!("broken_hooks:{agent}"),
            rule: "broken_hooks".into(),
            risk: 90,
            title,
            why,
            evidence: evidence(&ids),
            item_ids: ids,
            action,
            action_label,
            link: format!("/hooks?status=broken&agent={agent}"),
        });
    }
}

fn slow_hooks(hooks: &[Item], out: &mut Vec<Fix>) {
    for h in hooks {
        let Some(t) = h.usage.as_ref().and_then(|u| u.timing.as_ref()) else { continue };
        if t.total_ms < SLOW_TOTAL_MS {
            continue;
        }
        let minutes = t.total_ms / 60_000;
        let mut why = format!("`{}`, p95 {}", h.name, fmt_ms(t.p95_ms));
        if t.cancelled > 0 {
            why.push_str(&format!(", cancelled {}", plural(t.cancelled as usize, "time", "times")));
        }
        why.push('.');
        if h.status == ItemStatus::Gone {
            why.push_str(" It is no longer configured.");
        }
        let ids = vec![h.id.clone()];
        out.push(Fix {
            id: format!("slow_hook:{}", h.id.0),
            rule: "slow_hook".into(),
            risk: if h.status == ItemStatus::Gone { 50 } else { 70 },
            title: format!("One hook cost {minutes} minutes."),
            why,
            evidence: format!("{}:{}", evidence(&ids), minutes / 10),
            item_ids: ids,
            action: None,
            action_label: "Review hook".into(),
            link: format!("/hooks?id={}", h.id.0),
        });
    }
}

fn dangling_links(skills: &[Item], out: &mut Vec<Fix>) {
    let mut ids = Vec::new();
    let mut names = Vec::new();
    let mut per_place: BTreeMap<String, usize> = BTreeMap::new();
    let mut n = 0;
    for s in skills {
        let KindFacts::Skill(f) = &s.facts else { continue };
        let dangling: Vec<_> = f.locations.iter().filter(|l| matches!(l.link, LinkState::Dangling(_))).collect();
        if dangling.is_empty() {
            continue;
        }
        ids.push(s.id.clone());
        names.push(if dangling.len() > 1 { format!("{} ×{}", s.name, dangling.len()) } else { s.name.clone() });
        for l in dangling {
            n += 1;
            let place = match l.agents.as_slice() {
                [one] => agent_name(&one.0),
                [first, ..] if first.0 == "agents_shared" => "~/.agents".to_string(),
                [first, rest @ ..] => format!("{} (+{})", agent_name(&first.0), rest.len()),
                [] => "no agent".to_string(),
            };
            *per_place.entry(place).or_default() += 1;
        }
    }
    if n == 0 {
        return;
    }
    let mut places: Vec<(String, usize)> = per_place.into_iter().collect();
    places.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let where_ = list_names(&places.iter().map(|(p, c)| format!("{c} in {p}")).collect::<Vec<_>>(), 3);
    out.push(Fix {
        id: "dangling_links".into(),
        rule: "dangling_links".into(),
        risk: 60,
        title: if n == 1 { "A skill link points nowhere.".into() } else { format!("{n} skill links point nowhere.") },
        why: format!("{where_} ({}).", list_names(&names, 3)),
        evidence: evidence(&ids),
        action: Some(ActionRequest::LinksRemoveDangling { ids: ids.clone() }),
        action_label: format!("Remove {}", plural(n, "link", "links")),
        // One skill: open it, so its dead link shows with its Remove button.
        link: if ids.len() == 1 { format!("/skills?status=broken&id={}", ids[0].0) } else { "/skills?status=broken".into() },
        item_ids: ids,
    });
}

/// Skills with more than one copy of their own (or a library copy plus
/// copies): candidates for Adopt.
fn duplicate_skills(skills: &[Item], out: &mut Vec<Fix>) {
    let mut list: Vec<(&Item, usize)> = skills
        .iter()
        .filter_map(|s| match &s.facts {
            crate::model::KindFacts::Skill(f) => {
                let n = crate::library::adopt::own_copies(f).len() + usize::from(f.library.is_some());
                (n >= 2).then_some((s, n))
            }
            _ => None,
        })
        .collect();
    if list.is_empty() {
        return;
    }
    list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.name.cmp(&b.0.name)));
    let (top, n) = list[0];
    let ids: Vec<ItemId> = list.iter().map(|(s, _)| s.id.clone()).collect();
    let k = list.len();
    out.push(Fix {
        id: "duplicate_skills".into(),
        rule: "duplicate_skills".into(),
        risk: 35,
        title: format!("The same skill lives in {n} places."),
        why: if k == 1 {
            format!("{} has {n} separate copies. Keep one in the library and link it everywhere.", top.name)
        } else {
            format!("{} has {n} separate copies; {k} skills have 2 or more. Keep one copy each in the library.", top.name)
        },
        evidence: evidence(&ids),
        item_ids: ids,
        action: None,
        action_label: "Adopt".into(),
        link: format!("/adopt?id={}", top.id.0),
    });
}

fn never_run_skills(skills: &[Item], out: &mut Vec<Fix>) {
    let list: Vec<&Item> = skills.iter().filter(|s| s.status == ItemStatus::Unused).collect();
    if list.is_empty() {
        return;
    }
    let ids: Vec<ItemId> = list.iter().map(|s| s.id.clone()).collect();
    let names: Vec<String> = list.iter().map(|s| s.name.clone()).collect();
    let n = list.len();
    out.push(Fix {
        id: "never_run_skills".into(),
        rule: "never_run_skills".into(),
        risk: 20,
        title: if n == 1 { "A Claude skill never ran.".into() } else { format!("{n} Claude skills never ran.") },
        why: format!("{}, per Claude's own usage counts.", list_names(&names, 3)),
        evidence: evidence(&ids),
        item_ids: ids,
        action: None,
        action_label: "Review".into(),
        link: "/skills?status=unused".into(),
    });
}

fn fmt_mb(bytes: u64) -> String {
    let mb = bytes as f64 / 1_048_576.0;
    if mb >= 10.0 { format!("{mb:.0} MB") } else { format!("{mb:.1} MB") }
}

fn ghost_plugins(plugins: &[Item], out: &mut Vec<Fix>) {
    let mut ghosts: Vec<&Item> = plugins
        .iter()
        .filter(|p| p.status == ItemStatus::Gone && p.usage.as_ref().is_some_and(|u| u.count >= 100))
        .collect();
    if ghosts.is_empty() {
        return;
    }
    ghosts.sort_by_key(|p| std::cmp::Reverse(p.usage.as_ref().map(|u| u.count).unwrap_or(0)));
    let top_overall = plugins
        .iter()
        .max_by_key(|p| p.usage.as_ref().map(|u| u.count).unwrap_or(0))
        .is_some_and(|p| p.id == ghosts[0].id);
    let ids: Vec<ItemId> = ghosts.iter().map(|p| p.id.clone()).collect();
    let parts: Vec<String> = ghosts
        .iter()
        .take(3)
        .map(|p| format!("{}: {} uses", p.name, fmt_count(p.usage.as_ref().map(|u| u.count).unwrap_or(0))))
        .collect();
    let n = ghosts.len();
    out.push(Fix {
        id: "ghost_plugins".into(),
        rule: "ghost_plugins".into(),
        risk: 40,
        title: if top_overall {
            "The most used plugin is gone.".into()
        } else if n == 1 {
            "A much-used plugin is gone.".into()
        } else {
            format!("{n} much-used plugins are gone.")
        },
        why: format!("{}. No longer installed.", parts.join(" · ")),
        evidence: evidence(&ids),
        item_ids: ids,
        action: None,
        action_label: "Reinstall or forget".into(),
        link: "/plugins?status=gone".into(),
    });
}

fn fmt_count(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn unused_marketplaces(markets: &[Item], out: &mut Vec<Fix>) {
    let unused: Vec<&Item> = markets.iter().filter(|m| m.status == ItemStatus::Unused).collect();
    if unused.is_empty() {
        return;
    }
    let ids: Vec<ItemId> = unused.iter().map(|m| m.id.clone()).collect();
    let bytes: u64 = unused
        .iter()
        .filter_map(|m| match &m.facts {
            KindFacts::Marketplace(f) => f.size_bytes,
            _ => None,
        })
        .sum();
    let names: Vec<String> = unused.iter().map(|m| m.name.clone()).collect();
    let n = unused.len();
    out.push(Fix {
        id: "unused_marketplaces".into(),
        rule: "unused_marketplaces".into(),
        risk: 30,
        title: if n == 1 { "A marketplace supplies nothing in use.".into() } else { format!("{n} marketplaces supply nothing in use.") },
        why: format!("{}. {} on disk.", list_names(&names, 3), fmt_mb(bytes)),
        evidence: evidence(&ids),
        action: Some(ActionRequest::MarketplaceRemove { ids: ids.clone() }),
        item_ids: ids,
        action_label: format!("Remove {n}"),
        link: "/plugins?view=marketplaces".into(),
    });
}

fn never_called_mcp(servers: &[Item], out: &mut Vec<Fix>) {
    let list: Vec<&Item> = servers.iter().filter(|s| s.status == ItemStatus::Unused).collect();
    if list.is_empty() {
        return;
    }
    let ids: Vec<ItemId> = list.iter().map(|s| s.id.clone()).collect();
    let names: Vec<String> = list.iter().map(|s| s.name.clone()).collect();
    let n = list.len();
    out.push(Fix {
        id: "never_called_mcp".into(),
        rule: "never_called_mcp".into(),
        risk: 25,
        title: if n == 1 { "An MCP server was never called.".into() } else { format!("{n} MCP servers were never called.") },
        why: format!("{}. Each one starts with Claude and adds its tools to every prompt.", list_names(&names, 4)),
        evidence: evidence(&ids),
        item_ids: ids,
        action: None,
        action_label: "Review".into(),
        link: "/mcp?status=unused".into(),
    });
}

impl Core {
    /// Ranked fixes, minus those dismissed while their evidence is unchanged.
    pub fn fixes(&self) -> Result<Vec<Fix>, AppError> {
        let inv = Inventory::load(self)?;
        let dismissed = self.store.dismissals()?;
        Ok(evaluate(&inv)
            .into_iter()
            .filter(|f| dismissed.get(&f.id) != Some(&f.evidence))
            .collect())
    }

    pub fn fix_dismiss(&self, id: &str) -> Result<(), AppError> {
        let fix = evaluate(&Inventory::load(self)?)
            .into_iter()
            .find(|f| f.id == id)
            .ok_or_else(|| AppError::new(ErrorKind::NotFound, "This fix no longer applies."))?;
        self.store.dismiss(&fix.id, &fix.evidence)
    }
}
