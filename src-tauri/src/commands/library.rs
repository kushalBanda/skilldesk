use crate::AppState;
use skilldesk_core::library::adopt::{AdoptView, SkillDiff};
use skilldesk_core::library::install::{InstallPreview, InstallTarget};
use skilldesk_core::{AppError, ErrorKind, ItemId};
use std::path::PathBuf;
use tauri::State;

#[tauri::command]
pub async fn library_inspect(state: State<'_, AppState>, path: PathBuf) -> Result<InstallPreview, AppError> {
    let core = state.core.clone();
    super::blocking(move || core.library_inspect(&path)).await
}

/// Selects the path in Finder. Only paths inside home (where agents keep files).
#[tauri::command]
pub async fn reveal_in_finder(state: State<'_, AppState>, path: PathBuf) -> Result<(), AppError> {
    if !path.starts_with(&state.core.env.home) || std::fs::symlink_metadata(&path).is_err() {
        return Err(AppError::new(ErrorKind::InvalidInput, "That path is not inside the home folder."));
    }
    std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(&path)
        .status()
        .map_err(|e| AppError::new(ErrorKind::Io, format!("Cannot open Finder: {e}")))?;
    Ok(())
}

#[tauri::command]
pub async fn skill_targets(state: State<'_, AppState>, id: ItemId) -> Result<Vec<InstallTarget>, AppError> {
    let core = state.core.clone();
    super::blocking(move || core.skill_targets(&id)).await
}

#[tauri::command]
pub async fn adopt_view(state: State<'_, AppState>, id: ItemId) -> Result<AdoptView, AppError> {
    let core = state.core.clone();
    super::blocking(move || core.adopt_view(&id)).await
}

#[tauri::command]
pub async fn skill_diff(state: State<'_, AppState>, id: ItemId, a: PathBuf, b: PathBuf) -> Result<SkillDiff, AppError> {
    let core = state.core.clone();
    super::blocking(move || core.skill_diff(&id, &a, &b)).await
}
