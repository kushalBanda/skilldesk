use crate::AppState;
use skilldesk_core::storage::StorageReport;
use skilldesk_core::AppError;
use tauri::State;

/// The last measurement (from the end of the last scan or cleanup).
#[tauri::command]
pub async fn storage_get(state: State<'_, AppState>) -> Result<StorageReport, AppError> {
    let core = state.core.clone();
    super::blocking(move || core.storage()).await
}

/// Measure again now (about a second).
#[tauri::command]
pub async fn storage_refresh(state: State<'_, AppState>) -> Result<StorageReport, AppError> {
    let core = state.core.clone();
    super::blocking(move || core.storage_refresh()).await
}
