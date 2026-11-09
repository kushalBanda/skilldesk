use crate::{events, AppState};
use skilldesk_core::actions::{ActionPlan, ActionRecord, ActionRequest};
use skilldesk_core::{scan, AppError};
use tauri::{AppHandle, Emitter, State};

#[tauri::command]
pub async fn action_plan(state: State<'_, AppState>, request: ActionRequest) -> Result<ActionPlan, AppError> {
    let core = state.core.clone();
    super::blocking(move || core.action_plan(request).map(|p| p.redacted())).await
}

/// Apply, then rescan so every screen shows the new state.
#[tauri::command]
pub async fn action_apply(app: AppHandle, state: State<'_, AppState>, plan_id: String) -> Result<ActionRecord, AppError> {
    let core = state.core.clone();
    let sink_app = app.clone();
    let record = super::blocking(move || {
        let record = core.action_apply(&plan_id)?;
        rescan(&core, sink_app, &record.kind);
        Ok(record)
    })
    .await?;
    let _ = app.emit(events::HISTORY_CHANGED, ());
    Ok(record.redacted())
}

#[tauri::command]
pub async fn action_undo(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<ActionRecord, AppError> {
    let core = state.core.clone();
    let sink_app = app.clone();
    let record = super::blocking(move || {
        let record = core.action_undo(&id)?;
        rescan(&core, sink_app, &record.kind);
        Ok(record)
    })
    .await?;
    let _ = app.emit(events::HISTORY_CHANGED, ());
    Ok(record.redacted())
}

#[tauri::command]
pub async fn history_list(state: State<'_, AppState>) -> Result<Vec<ActionRecord>, AppError> {
    let core = state.core.clone();
    super::blocking(move || core.history().map(|h| h.iter().map(|r| r.redacted()).collect())).await
}

/// Rescan before returning, so the UI's refetch sees the new state. A
/// cleanup can move transcripts, so it reads them again too (only changed
/// files are read); when a scan is already running, config is enough.
fn rescan(core: &skilldesk_core::Core, app: AppHandle, kind: &str) {
    let sink = events::TauriSink(app);
    if kind == "storage.trash" && scan::run_scan(core, &sink).is_ok() {
        return;
    }
    if let Err(e) = scan::rescan_config(core, &sink) {
        eprintln!("rescan after write failed: {e}");
    }
}
