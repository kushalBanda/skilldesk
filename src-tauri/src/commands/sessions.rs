use crate::AppState;
use skilldesk_core::sessions::{SearchHit, Session};
use skilldesk_core::AppError;
use tauri::State;

#[tauri::command]
pub async fn sessions_list(state: State<'_, AppState>) -> Result<Vec<Session>, AppError> {
    let core = state.core.clone();
    super::blocking(move || core.sessions()).await
}

#[tauri::command]
pub async fn sessions_search(state: State<'_, AppState>, query: String) -> Result<Vec<SearchHit>, AppError> {
    let core = state.core.clone();
    super::blocking(move || core.sessions_search(&query)).await
}

/// Opens Terminal with the agent's resume command; returns the command.
#[tauri::command]
pub async fn session_resume(state: State<'_, AppState>, id: String) -> Result<String, AppError> {
    let core = state.core.clone();
    super::blocking(move || core.session_resume(&id)).await
}

/// Opens a Cursor editor chat's project folder in Cursor.
#[tauri::command]
pub async fn session_open_project(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    let core = state.core.clone();
    super::blocking(move || core.session_open_project(&id)).await
}
