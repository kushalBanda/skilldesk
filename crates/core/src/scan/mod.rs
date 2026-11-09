//! Staged scan (ADR 0008): stage `Config` (skills, hooks, …), then
//! `Transcripts` (hook run timing, Claude sessions), then `Deep` (other
//! agents' sessions, from their databases opened read-only).

pub mod defs;
pub(crate) mod hooks;
pub mod mcp;
pub mod memory;
pub mod plugins;
mod plugins_other;
mod skills;

use crate::core::Core;
use crate::error::{AppError, ErrorKind};
use crate::model::{ItemKind, ScanProgress, ScanStage};
use std::time::Instant;

/// Receives scan events. The app forwards them as Tauri events; the CLI ignores them.
pub trait ProgressSink: Send + Sync {
    fn progress(&self, p: &ScanProgress);
}

pub struct NoSink;
impl ProgressSink for NoSink {
    fn progress(&self, _: &ScanProgress) {}
}

/// Full scan: config files, then transcripts.
pub fn run_scan(core: &Core, sink: &dyn ProgressSink) -> Result<String, AppError> {
    scan_impl(core, sink, true)
}

/// After SkillDesk's own writes: config files only, reusing the hook runs from
/// the last full scan (transcripts do not change because of our writes).
pub fn rescan_config(core: &Core, sink: &dyn ProgressSink) -> Result<String, AppError> {
    scan_impl(core, sink, false)
}

fn scan_impl(core: &Core, sink: &dyn ProgressSink, transcripts: bool) -> Result<String, AppError> {
    // A full scan refuses to start twice; a rescan after a write waits for
    // the running scan, so it never reads config from before the write.
    let _guard = if transcripts {
        match core.scan_lock.try_lock() {
            Ok(g) => g,
            Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                return Err(AppError::new(ErrorKind::InvalidInput, "A scan is already running."))
            }
        }
    } else {
        core.scan_lock.lock().unwrap_or_else(|p| p.into_inner())
    };

    let run_id = format!("run-{}", chrono::Utc::now().timestamp_millis());
    let started = Instant::now();
    core.store.start_run(&run_id, &chrono::Utc::now().to_rfc3339())?;
    let mut changed: Vec<ItemKind> = Vec::new();
    let put = |kind: ItemKind, items: &[crate::model::Item], changed: &mut Vec<ItemKind>| -> Result<(), AppError> {
        let items = without_off_agents(items);
        if core.store.replace_kind(kind, &items)? && !changed.contains(&kind) {
            changed.push(kind);
        }
        Ok(())
    };

    // Stage a: config files. Timing comes from the last transcript read
    // (memory, else the database), so rows never lose it in between.
    let skills = skills::scan(&core.env);
    put(ItemKind::Skill, &skills.items, &mut changed)?;
    core.store.set_setting(
        "stat.skills_used",
        &skills.skills_used.map(|n| n.to_string()).unwrap_or_default(),
    )?;
    let skill_names: std::collections::HashSet<String> = skills.items.iter().map(|i| i.name.clone()).collect();
    let put_defs = |stats: Option<&crate::parse::claude_runs::TranscriptStats>, changed: &mut Vec<ItemKind>| -> Result<(), AppError> {
        let runs = stats.map(|t| defs::Runs { subagents: &t.subagents, commands: &t.commands });
        let d = defs::scan(&core.env, runs, &skill_names);
        put(ItemKind::Subagent, &d.subagents, changed)?;
        put(ItemKind::Command, &d.commands, changed)
    };
    let cached = {
        let mut slot = core.transcripts.lock().unwrap_or_else(|p| p.into_inner());
        if slot.is_none() {
            let from_db = core.store.transcript_stats()?;
            if !from_db.hooks.is_empty() || !from_db.mcp.is_empty() || !from_db.commands.is_empty() {
                *slot = Some(std::sync::Arc::new(from_db));
            }
        }
        slot.clone()
    };
    let plugins = plugins::scan(&core.env, cached.as_deref().map(|t| &t.markets));
    put(ItemKind::Plugin, &plugins.plugins, &mut changed)?;
    put(ItemKind::Marketplace, &plugins.marketplaces, &mut changed)?;
    put(ItemKind::PluginCache, &plugins.cache, &mut changed)?;
    put(ItemKind::Hook, &hooks::scan(&core.env, cached.as_deref().map(|t| &t.hooks)).items, &mut changed)?;
    let mcp_calls = crate::parse::claude_runs::all_mcp(&core.env, cached.as_deref().map(|t| &t.mcp));
    put(ItemKind::McpServer, &mcp::scan(&core.env, Some(&mcp_calls)), &mut changed)?;
    put_defs(cached.as_deref(), &mut changed)?;
    put(ItemKind::Memory, &memory::scan(&core.env), &mut changed)?;
    if !transcripts && core.storage_dirty.swap(false, std::sync::atomic::Ordering::SeqCst) {
        core.storage_refresh()?;
        changed.push(ItemKind::Storage);
    }
    let config_ms = started.elapsed().as_millis() as u64;
    sink.progress(&ScanProgress {
        run_id: run_id.clone(),
        stage: ScanStage::Config,
        kinds: changed.clone(),
        elapsed_ms: config_ms,
        done: !transcripts,
    });
    if !transcripts {
        core.store.finish_run(&run_id, &chrono::Utc::now().to_rfc3339(), &serde_json::json!({ "config_ms": config_ms }).to_string())?;
        return Ok(run_id);
    }

    // Stage c: transcripts, incrementally: only lines appended since the last read.
    // Claude Code switched off in Settings: no transcripts, so the ones read
    // before drop out of the cache.
    let files = if crate::adapters::is_off("claude_code") { Vec::new() } else { crate::parse::claude_runs::transcript_files(&core.env) };
    let stamps = core.store.transcript_stamps()?;
    let (deltas, gone) = crate::parse::claude_runs::read_changed(&files, &stamps);
    let read_files = deltas.len();
    let mut changed: Vec<ItemKind> = Vec::new();
    if !deltas.is_empty() || !gone.is_empty() || cached.is_none() {
        core.store.apply_transcripts(&deltas, &gone)?;
        let stats = std::sync::Arc::new(core.store.transcript_stats()?);
        *core.transcripts.lock().unwrap_or_else(|p| p.into_inner()) = Some(stats.clone());
        let hooks = hooks::scan(&core.env, Some(&stats.hooks));
        if let Some(t) = &hooks.totals {
            core.store.set_setting("stat.hooks", &serde_json::to_string(t).unwrap_or_default())?;
        }
        put(ItemKind::Hook, &hooks.items, &mut changed)?;
        // Restore steps read marketplace sources from the transcripts.
        put(ItemKind::Plugin, &plugins::scan(&core.env, Some(&stats.markets)).plugins, &mut changed)?;
        put(ItemKind::McpServer, &mcp::scan(&core.env, Some(&crate::parse::claude_runs::all_mcp(&core.env, Some(&stats.mcp)))), &mut changed)?;
        put_defs(Some(&stats), &mut changed)?;
        if deltas.iter().any(|d| d.session.is_some()) || !gone.is_empty() {
            changed.push(ItemKind::Session);
        }
        if deltas.iter().any(|d| !d.secrets.is_empty() || d.reset) || !gone.is_empty() {
            changed.push(ItemKind::Secret);
        }
    }
    let transcripts_ms = started.elapsed().as_millis() as u64 - config_ms;
    sink.progress(&ScanProgress {
        run_id: run_id.clone(),
        stage: ScanStage::Transcripts,
        kinds: changed,
        elapsed_ms: started.elapsed().as_millis() as u64,
        done: false,
    });

    // Stage d: other agents' sessions. Cursor's 1 GB database is read again
    // only when Cursor wrote it since the last read.
    use crate::sessions::{deep, SessionOrigin};
    let mut origins = vec![SessionOrigin::Codex, SessionOrigin::Opencode, SessionOrigin::CursorCli, SessionOrigin::Gemini];
    let mut rows = deep::codex(&core.env);
    rows.extend(deep::opencode(&core.env));
    rows.extend(deep::cursor_cli(&core.env));
    rows.extend(deep::gemini(&core.env));
    let stamp = deep::cursor_stamp(&core.env);
    let cursor_new = core.store.setting("sessions.cursor_stamp")?.as_deref() != Some(stamp.as_str());
    if cursor_new {
        origins.push(SessionOrigin::CursorEditor);
        rows.extend(deep::cursor_editor(&core.env));
    }
    rows.retain(|r| r.origin.is_none_or(|o| !crate::adapters::is_off(o.agent())));
    core.store.replace_sessions(&origins, &rows)?;
    if cursor_new {
        core.store.set_setting("sessions.cursor_stamp", &stamp)?;
    }
    // Secrets in the other history files (transcripts were read above).
    let secret_started = Instant::now();
    let mut history = crate::secrets::scan::text_files(&core.env);
    history.retain(|(_, place)| !crate::adapters::is_off(crate::secrets::scan::place_agent(place)));
    let (secret_files, secret_gone) = crate::secrets::scan::read_changed(&history, &core.store.secret_file_stamps()?);
    let secret_files_read = secret_files.len();
    core.store.apply_secret_files(&secret_files, &secret_gone)?;
    let secrets_ms = secret_started.elapsed().as_millis() as u64;
    core.storage_dirty.store(false, std::sync::atomic::Ordering::SeqCst);
    let storage_ms = core.storage_refresh()?.measure_ms;
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let timings = serde_json::json!({
        "config_ms": config_ms,
        "transcripts_ms": transcripts_ms,
        "deep_ms": elapsed_ms - config_ms - transcripts_ms,
        "deep_sessions": rows.len(),
        "secrets_ms": secrets_ms,
        "storage_ms": storage_ms,
        "secret_files": history.len(),
        "secret_files_read": secret_files_read,
        "transcript_files": files.len(),
        "transcript_files_read": read_files,
    })
    .to_string();
    core.store
        .finish_run(&run_id, &chrono::Utc::now().to_rfc3339(), &timings)?;
    sink.progress(&ScanProgress {
        run_id: run_id.clone(),
        stage: ScanStage::Deep,
        kinds: vec![ItemKind::Session, ItemKind::Secret, ItemKind::Storage],
        elapsed_ms,
        done: true,
    });
    Ok(run_id)
}

/// Agents switched off in Settings leave every item; an item only they had
/// is not kept. Items with no agent (marketplaces, caches) stay.
fn without_off_agents(items: &[crate::model::Item]) -> Vec<crate::model::Item> {
    items
        .iter()
        .filter_map(|i| {
            if i.agents.is_empty() || !i.agents.iter().any(|a| crate::adapters::is_off(&a.0)) {
                return Some(i.clone());
            }
            let mut i = i.clone();
            i.agents.retain(|a| !crate::adapters::is_off(&a.0));
            (!i.agents.is_empty()).then_some(i)
        })
        .collect()
}
