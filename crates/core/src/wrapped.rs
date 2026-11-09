//! Wrapped: one year's numbers for the shareable card (DESIGN.md §7.13).
//! Only counts, local times, skill and command names leave this module.
//! Repo names, paths, prompt text and secrets never do.

use crate::core::Core;
use crate::error::AppError;
use crate::model::{AgentKey, AgentShare, DayTotal, ItemKind, ItemStatus, NameCount, WrappedData};
use crate::sessions::{SessionOrigin, QUARTER_MS};
use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone, Timelike};
use std::collections::{BTreeMap, BTreeSet, HashMap};

fn local(ms: i64) -> Option<DateTime<Local>> {
    Local.timestamp_millis_opt(ms).single()
}

/// Longest run of consecutive dates (input sorted, no repeats).
fn longest_streak(days: &[NaiveDate]) -> u32 {
    let (mut best, mut run) = (0u32, 0u32);
    let mut prev: Option<NaiveDate> = None;
    for &d in days {
        run = if prev.is_some_and(|p| p.succ_opt() == Some(d)) { run + 1 } else { 1 };
        best = best.max(run);
        prev = Some(d);
    }
    best
}

fn top<K: Ord + Clone>(m: &BTreeMap<K, u64>) -> Option<K> {
    // Ties go to the smallest key, so the result is stable.
    m.iter().fold(None::<(&K, u64)>, |best, (k, &n)| if best.is_none_or(|(_, b)| n > b) { Some((k, n)) } else { best }).map(|(k, _)| k.clone())
}

/// `None` = the newest year with prompts (or the current year when there are none).
pub fn wrapped(core: &Core, year: Option<i32>) -> Result<WrappedData, AppError> {
    let mut stamps: Vec<(String, DateTime<Local>, u64)> = Vec::new();
    let mut years: BTreeSet<i32> = BTreeSet::new();
    for (origin, q, n) in core.store.activity()? {
        let (Some(o), Some(t)) = (SessionOrigin::parse(&origin), local(q * QUARTER_MS)) else { continue };
        years.insert(t.year());
        stamps.push((o.agent().to_string(), t, n));
    }
    let year = year.or_else(|| years.last().copied()).unwrap_or_else(|| Local::now().year());

    let mut per_day: BTreeMap<NaiveDate, u64> = BTreeMap::new();
    let mut per_hour: BTreeMap<u8, u64> = BTreeMap::new();
    let mut per_dow: BTreeMap<u8, u64> = BTreeMap::new();
    let mut per_agent: HashMap<String, u64> = HashMap::new();
    for (agent, t, n) in stamps.iter().filter(|(_, t, _)| t.year() == year) {
        *per_day.entry(t.date_naive()).or_default() += n;
        *per_hour.entry(t.hour() as u8).or_default() += n;
        *per_dow.entry(t.weekday().num_days_from_monday() as u8).or_default() += n;
        *per_agent.entry(agent.clone()).or_default() += n;
    }
    let days: Vec<NaiveDate> = per_day.keys().copied().collect();
    let busiest = per_day.iter().max_by_key(|&(d, n)| (*n, std::cmp::Reverse(*d)));

    // Skills: counters carry no year, so a skill counts for the year of its last use.
    let mut top_skill: Option<NameCount> = None;
    for item in core.store.items(ItemKind::Skill)? {
        if item.status == ItemStatus::NotAnItem {
            continue;
        }
        let Some(u) = &item.usage else { continue };
        let last_year = u.last_used.as_deref().and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|d| d.with_timezone(&Local).year());
        if u.count > 0 && last_year == Some(year) && top_skill.as_ref().is_none_or(|b| u.count > b.count) {
            top_skill = Some(NameCount { name: item.name.clone(), count: u.count });
        }
    }

    // Commands: one row per transcript; `/plugin:name` counts as `/name`.
    let mut commands: BTreeMap<String, u64> = BTreeMap::new();
    for (name, last, n) in core.store.uses("command")? {
        if last.and_then(local).is_some_and(|t| t.year() == year) {
            *commands.entry(crate::parse::claude::bare_name(&name).to_string()).or_default() += n;
        }
    }
    let top_command = top(&commands).map(|name| NameCount { count: commands[&name], name });

    let mut agents: Vec<AgentShare> = per_agent
        .into_iter()
        .map(|(key, prompts)| {
            let name = crate::adapters::find(&key).map(|a| a.summary().display_name).unwrap_or_else(|| key.clone());
            AgentShare { agent: AgentKey(key), name, prompts }
        })
        .collect();
    agents.sort_by(|a, b| b.prompts.cmp(&a.prompts).then_with(|| a.name.cmp(&b.name)));

    Ok(WrappedData {
        year,
        years: years.into_iter().rev().collect(),
        prompts: per_day.values().sum(),
        first_day: days.first().map(|d| d.format("%Y-%m-%d").to_string()),
        active_days: days.len() as u32,
        peak_hour: top(&per_hour),
        top_weekday: top(&per_dow),
        longest_streak: longest_streak(&days),
        busiest_day: busiest.map(|(d, &n)| DayTotal { day: d.format("%Y-%m-%d").to_string(), prompts: n as u32 }),
        top_skill,
        top_command,
        agents,
        companion: crate::parse::claude::has_companion(&core.env),
    })
}
