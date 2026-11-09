pub mod a11y;
pub mod actions;
pub mod fixes;
pub mod items;
pub mod library;
pub mod market;
pub mod overview;
pub mod scan;
pub mod secrets;
pub mod sessions;
pub mod settings;
pub mod storage;
pub mod wrapped;

use skilldesk_core::AppError;

/// Run blocking core work off the IPC thread (ADR 0008).
pub async fn blocking<T, F>(f: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| AppError::internal(format!("background task failed: {e}")))?
}
