use crate::{events::TauriSink, AppState};
use skilldesk_core::{scan, AppError};
use tauri::{AppHandle, State};

/// Starts a scan in the background and returns at once. Results arrive as
/// `scan-progress` and `inventory-changed` events.
#[tauri::command]
pub async fn scan_start(app: AppHandle, state: State<'_, AppState>) -> Result<(), AppError> {
    let core = state.core.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(e) = scan::run_scan(&core, &TauriSink(app)) {
            eprintln!("scan failed: {e}");
        }
    });
    Ok(())
}
