use crate::AppState;
use skilldesk_core::{AppError, Overview};
use tauri::State;

#[tauri::command]
pub async fn overview(state: State<'_, AppState>) -> Result<Overview, AppError> {
    let core = state.core.clone();
    super::blocking(move || core.overview()).await
}
