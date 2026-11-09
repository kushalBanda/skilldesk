use crate::AppState;
use skilldesk_core::{AppError, ErrorKind, WrappedData};
use tauri::ipc::{InvokeBody, Request};
use tauri::State;
use tauri_plugin_dialog::DialogExt;

/// One year's Wrapped numbers; no year = the newest year with prompts.
#[tauri::command]
pub async fn wrapped_data(state: State<'_, AppState>, year: Option<i32>) -> Result<WrappedData, AppError> {
    let core = state.core.clone();
    super::blocking(move || core.wrapped(year)).await
}

const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Save a PNG the page drew (raw body) where the User picks in the save
/// dialog. The page never sees or names a path. Returns false on Cancel.
#[tauri::command]
pub async fn file_save(app: tauri::AppHandle, request: Request<'_>) -> Result<bool, AppError> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err(AppError::new(ErrorKind::InvalidInput, "Expected the image bytes."));
    };
    if !bytes.starts_with(PNG_MAGIC) {
        return Err(AppError::new(ErrorKind::InvalidInput, "Only PNG images can be saved."));
    }
    let bytes = bytes.clone();
    let name: String = request
        .headers()
        .get("x-name")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("SkillDesk Wrapped.png")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || " -_.".contains(*c))
        .collect();
    let name = if name.ends_with(".png") { name } else { format!("{name}.png") };
    super::blocking(move || {
        let Some(picked) = app.dialog().file().set_file_name(&name).add_filter("PNG image", &["png"]).blocking_save_file() else {
            return Ok(false);
        };
        let mut path = picked.into_path().map_err(|e| AppError::new(ErrorKind::InvalidInput, format!("Cannot save there: {e}")))?;
        if path.extension().is_none_or(|e| !e.eq_ignore_ascii_case("png")) {
            path.set_extension("png");
        }
        std::fs::write(&path, &bytes).map_err(|e| AppError::new(ErrorKind::Io, format!("Cannot save {}: {e}", path.display())))?;
        Ok(true)
    })
    .await
}
