//! `Core` facade used by the app and the CLI.

use crate::adapters;
use crate::env::Env;
use crate::error::AppError;
use crate::error::ErrorKind;
use crate::model::{AgentKey, AgentRow, AgentTier, DayCount, HourCount, Item, ItemId, ItemKind, ItemStatus, KindFacts, Overview};
use crate::store::Store;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub(crate) const STORAGE_KEY: &str = "storage.report";

/// Released on drop: the file lock (closing the file) and the mutex.
pub(crate) struct WriteGuard<'a> {
    _file: std::fs::File,
    _lock: std::sync::MutexGuard<'a, ()>,
}

pub struct Core {
    pub env: Env,
    pub store: Store,
    /// Plans shown to the user, waiting for Apply (10 min TTL).
    pub(crate) plans: Mutex<HashMap<String, (Instant, crate::actions::ActionPlan)>>,
    /// One write at a time in this process; `write_guard` adds the file lock
    /// shared with the other process (app or CLI).
    pub(crate) write_lock: Mutex<()>,
    /// One scan at a time: a full scan refuses while one runs, a config rescan waits.
    pub(crate) scan_lock: Mutex<()>,
    /// Hook runs from the last transcript read, reused by config-only rescans.
    /// Paths SkillDesk just wrote; the watcher ignores their echo (ADR 0009).
    pub(crate) muted: Mutex<Vec<(std::path::PathBuf, Instant)>>,
    pub(crate) transcripts: Mutex<Option<Arc<crate::parse::claude_runs::TranscriptStats>>>,
    /// Marketplace downloads waiting for Install, and where each came from.
    pub(crate) origins: Mutex<HashMap<std::path::PathBuf, crate::library::LibrarySource>>,
    /// A cleanup ran: the next config rescan measures disk use again.
    pub(crate) storage_dirty: std::sync::atomic::AtomicBool,
}

impl Core {
    pub fn open(env: Env) -> Result<Arc<Self>, AppError> {
        let store = Store::open(&env.db_path())?;
        let core = Arc::new(Self {
            env,
            store,
            plans: Mutex::new(HashMap::new()),
            write_lock: Mutex::new(()),
            scan_lock: Mutex::new(()),
            transcripts: Mutex::new(None),
            muted: Mutex::new(Vec::new()),
            origins: Mutex::new(HashMap::new()),
            storage_dirty: std::sync::atomic::AtomicBool::new(false),
        });
        crate::settings::load(&core)?;
        Ok(core)
    }

    /// Held for every write to agent files. The in-process mutex, then an
    /// advisory lock on `write.lock` next to the database, so the app and the
    /// CLI never write at the same time. Waits for the other process.
    pub(crate) fn write_guard(&self) -> Result<WriteGuard<'_>, AppError> {
        let lock = self.write_lock.lock().unwrap_or_else(|p| p.into_inner());
        let dir = self.env.data_dir.clone();
        std::fs::create_dir_all(&dir).map_err(|e| AppError::new(ErrorKind::Io, format!("Cannot create {}: {e}", dir.display())))?;
        let path = dir.join("write.lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|e| AppError::new(ErrorKind::Io, format!("Cannot open {}: {e}", path.display())))?;
        file.lock().map_err(|e| AppError::new(ErrorKind::Io, format!("Cannot lock {}: {e}", path.display())))?;
        Ok(WriteGuard { _file: file, _lock: lock })
    }

    pub(crate) fn remember_origin(&self, dir: std::path::PathBuf, source: crate::library::LibrarySource) {
        let mut m = self.origins.lock().unwrap_or_else(|p| p.into_inner());
        m.retain(|d, _| d.exists());
        m.insert(dir, source);
    }

    /// The marketplace source of a download folder (set by `net::market::fetch`).
    pub(crate) fn origin_of(&self, dir: &std::path::Path) -> Option<crate::library::LibrarySource> {
        self.origins.lock().unwrap_or_else(|p| p.into_inner()).get(dir).cloned()
    }

    /// Ignore watcher events for these paths (and anything under them) briefly.
    pub fn mute(&self, paths: impl IntoIterator<Item = std::path::PathBuf>) {
        let mut m = self.muted.lock().unwrap_or_else(|p| p.into_inner());
        m.retain(|(_, at)| at.elapsed() < crate::watch::MUTE_FOR);
        let now = Instant::now();
        m.extend(paths.into_iter().map(|p| (p, now)));
    }

    pub fn is_muted(&self, path: &std::path::Path) -> bool {
        let m = self.muted.lock().unwrap_or_else(|p| p.into_inner());
        m.iter().any(|(p, at)| at.elapsed() < crate::watch::MUTE_FOR && (path.starts_with(p) || path.parent() == p.parent() && path.file_name().is_some_and(|n| n.to_string_lossy().ends_with(".skilldesk-tmp"))))
    }

    /// The last disk measurement; measured now when there is none.
    pub fn storage(&self) -> Result<crate::storage::StorageReport, AppError> {
        if let Some(r) = self.store.setting(STORAGE_KEY)?.and_then(|t| serde_json::from_str(&t).ok()) {
            return Ok(r);
        }
        self.storage_refresh()
    }

    pub fn storage_refresh(&self) -> Result<crate::storage::StorageReport, AppError> {
        let r = crate::storage::measure(&self.env, &self.store.items(ItemKind::Marketplace)?, &self.store.items(ItemKind::PluginCache)?);
        let text = serde_json::to_string(&r).map_err(|e| AppError::new(ErrorKind::Internal, e.to_string()))?;
        self.store.set_setting(STORAGE_KEY, &text)?;
        Ok(r)
    }

    pub fn overview(&self) -> Result<Overview, AppError> {
        use chrono::{Datelike, Local, TimeZone, Timelike};
        use std::collections::BTreeMap;
        let agents: Vec<_> = adapters::detected(&self.env).iter().map(|a| a.summary()).collect();
        let agent_of = |origin: &str| crate::sessions::SessionOrigin::parse(origin).map(|o| o.agent().to_string());
        let mut days: BTreeMap<(String, String), u32> = BTreeMap::new();
        let mut hours: BTreeMap<(String, u8, u8), u32> = BTreeMap::new();
        for (origin, q, n) in self.store.activity()? {
            let (Some(agent), Some(t)) = (agent_of(&origin), Local.timestamp_millis_opt(q * crate::sessions::QUARTER_MS).single()) else { continue };
            *days.entry((t.format("%Y-%m-%d").to_string(), agent.clone())).or_default() += n as u32;
            *hours.entry((agent, t.weekday().num_days_from_monday() as u8, t.hour() as u8)).or_default() += n as u32;
        }
        let activity: Vec<DayCount> = days.into_iter().map(|((day, agent), prompts)| DayCount { day, agent: AgentKey(agent), prompts }).collect();
        let hours = hours.into_iter().map(|((agent, dow, hour), prompts)| HourCount { agent: AgentKey(agent), dow, hour, prompts }).collect();
        let mut active: Vec<&str> = activity.iter().map(|d| d.day.as_str()).collect();
        active.dedup();
        let has_sessions = !activity.is_empty();

        let mut rows: BTreeMap<String, AgentRow> = agents
            .iter()
            .filter(|a| a.tier != AgentTier::Shared)
            .map(|a| (a.key.0.clone(), AgentRow { agent: a.key.clone(), name: a.display_name.clone(), tier: a.tier, skills: 0, hooks: 0, mcp: 0, sessions: 0 }))
            .collect();
        for kind in [ItemKind::Skill, ItemKind::Hook, ItemKind::McpServer] {
            for item in self.store.items(kind)? {
                if matches!(item.status, ItemStatus::Gone | ItemStatus::NotAnItem) {
                    continue;
                }
                for a in &item.agents {
                    if let Some(r) = rows.get_mut(&a.0) {
                        match kind {
                            ItemKind::Skill => r.skills += 1,
                            ItemKind::Hook => r.hooks += 1,
                            _ => r.mcp += 1,
                        }
                    }
                }
            }
        }
        for (origin, n) in self.store.session_counts()? {
            if let Some(r) = agent_of(&origin).and_then(|a| rows.get_mut(&a)) {
                r.sessions += n as u32;
            }
        }
        let mut agent_rows: Vec<AgentRow> = rows.into_values().filter(|r| r.skills + r.hooks + r.mcp + r.sessions > 0).collect();
        agent_rows.sort_by_key(|r| std::cmp::Reverse((r.sessions, r.skills)));

        Ok(Overview {
            prompts: has_sessions.then(|| activity.iter().map(|d| d.prompts as u64).sum()),
            active_days: has_sessions.then_some(active.len() as u64),
            skills_used: self.store.setting("stat.skills_used")?.and_then(|v| v.parse().ok()),
            agents,
            kinds: self.store.kind_counts()?,
            hooks: self.store.setting("stat.hooks")?.and_then(|v| serde_json::from_str(&v).ok()),
            scanned_at: self.store.last_scan_end()?,
            activity,
            hours,
            agent_rows,
            known_agents: adapters::registry().iter().filter(|a| a.summary().tier != AgentTier::Shared).count() as u32,
        })
    }

    /// One year's numbers for the Wrapped card; `None` = newest year with prompts.
    pub fn wrapped(&self, year: Option<i32>) -> Result<crate::model::WrappedData, AppError> {
        crate::wrapped::wrapped(self, year)
    }

    /// Text of the item's main file (SKILL.md for skills), capped at 256 KB.
    pub fn item_document(&self, id: &ItemId) -> Result<String, AppError> {
        let item = self
            .store
            .item(id)?
            .ok_or_else(|| AppError::new(ErrorKind::NotFound, "This item is no longer in the inventory."))?;
        let path = match &item.facts {
            KindFacts::Skill(f) if !f.locations.is_empty() || f.library.is_some() => item.sources.first().map(|s| s.path.clone()),
            KindFacts::Subagent(_) | KindFacts::Command(_) | KindFacts::Memory(_) => item.sources.first().map(|s| s.path.clone()),
            _ => None,
        }
        .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Nothing to show for this item."))?;
        let bytes = std::fs::read(&path)
            .map_err(|e| AppError::new(ErrorKind::Io, format!("Cannot read {}: {e}", path.display())))?;
        let cap = bytes.len().min(256 * 1024);
        Ok(String::from_utf8_lossy(&bytes[..cap]).into_owned())
    }

    /// What installing `path` would do (reads only).
    pub fn library_inspect(&self, path: &std::path::Path) -> Result<crate::library::install::InstallPreview, AppError> {
        crate::library::install::inspect(self, path)
    }

    /// For a library skill: every agent folder it can be linked into, and what is there now.
    pub fn skill_targets(&self, id: &ItemId) -> Result<Vec<crate::library::install::InstallTarget>, AppError> {
        let item = self
            .store
            .item(id)?
            .ok_or_else(|| AppError::new(ErrorKind::NotFound, "This item is no longer in the inventory."))?;
        let KindFacts::Skill(f) = &item.facts else { return Ok(vec![]) };
        let Some(lib) = &f.library else { return Ok(vec![]) };
        let name = lib.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(crate::library::install::targets(self, &name, lib))
    }

    pub fn adopt_view(&self, id: &ItemId) -> Result<crate::library::adopt::AdoptView, AppError> {
        crate::library::adopt::view(self, id)
    }

    pub fn skill_diff(&self, id: &ItemId, a: &std::path::Path, b: &std::path::Path) -> Result<crate::library::adopt::SkillDiff, AppError> {
        crate::library::adopt::diff(self, id, a, b)
    }

    pub fn items(&self, kind: ItemKind) -> Result<Vec<Item>, AppError> {
        self.store.items(kind)
    }
}
