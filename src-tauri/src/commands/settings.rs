use crate::AppState;
use skilldesk_core::{AgentKey, AppError, CliStatus, CustomAgent, SettingsData};
use std::path::PathBuf;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
pub async fn settings_get(state: State<'_, AppState>) -> Result<SettingsData, AppError> {
    let core = state.core.clone();
    super::blocking(move || core.settings()).await
}

/// The read switch. The UI starts a scan after it.
#[tauri::command]
pub async fn agent_set_on(app: AppHandle, state: State<'_, AppState>, agent: AgentKey, on: bool) -> Result<(), AppError> {
    let core = state.core.clone();
    super::blocking(move || core.agent_set_on(&agent, on)).await?;
    state.restart_watcher(&app);
    Ok(())
}

#[tauri::command]
pub async fn custom_agent_save(
    app: AppHandle,
    state: State<'_, AppState>,
    key: Option<String>,
    name: String,
    skills_dir: PathBuf,
    project_dir: Option<String>,
) -> Result<CustomAgent, AppError> {
    let core = state.core.clone();
    let saved = super::blocking(move || core.custom_agent_save(key, &name, &skills_dir, project_dir.as_deref())).await?;
    state.restart_watcher(&app);
    Ok(saved)
}

/// Removes the agent from the list; its folder and skills stay.
#[tauri::command]
pub async fn custom_agent_delete(app: AppHandle, state: State<'_, AppState>, key: String) -> Result<(), AppError> {
    let core = state.core.clone();
    super::blocking(move || core.custom_agent_delete(&key)).await?;
    state.restart_watcher(&app);
    Ok(())
}

/// Folder picker for a custom agent's skills folder. Runs in Rust, so the
/// page gets only the folder the User chose. `None` on Cancel.
#[tauri::command]
pub async fn pick_folder(app: AppHandle, state: State<'_, AppState>) -> Result<Option<PathBuf>, AppError> {
    let home = state.core.env.home.clone();
    super::blocking(move || Ok(app.dialog().file().set_directory(home).blocking_pick_folder().and_then(|p| p.into_path().ok()))).await
}

/// The command this app carries: `skilldesk` next to the app's own binary
/// (`Contents/MacOS` in the bundle, `target/<profile>` in a dev build).
fn bundled() -> Option<PathBuf> {
    let p = std::env::current_exe().ok()?.parent()?.join("skilldesk");
    p.is_file().then_some(p)
}

#[tauri::command]
pub async fn cli_status(state: State<'_, AppState>) -> Result<CliStatus, AppError> {
    let core = state.core.clone();
    super::blocking(move || Ok(core.cli_status(bundled().as_deref()))).await
}

#[tauri::command]
pub async fn cli_install(state: State<'_, AppState>) -> Result<CliStatus, AppError> {
    let core = state.core.clone();
    super::blocking(move || {
        let src = bundled().ok_or_else(|| AppError::new(skilldesk_core::ErrorKind::NotFound, "This build of SkillDesk has no skilldesk command."))?;
        core.cli_install(&src)
    })
    .await
}
