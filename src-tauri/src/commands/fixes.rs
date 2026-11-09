use crate::{events, AppState};
use skilldesk_core::rules::Fix;
use skilldesk_core::AppError;
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn fixes_list(state: State<'_, AppState>) -> Result<Vec<Fix>, AppError> {
    let core = state.core.clone();
    super::blocking(move || core.fixes()).await
}

/// Hide a fix until its evidence changes.
#[tauri::command]
pub async fn fix_dismiss(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    let core = state.core.clone();
    super::blocking(move || core.fix_dismiss(&id)).await?;
    events::fixes_changed(&app);
    Ok(())
}
