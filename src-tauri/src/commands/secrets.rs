use crate::AppState;
use skilldesk_core::secrets::Secret;
use skilldesk_core::{AppError, ErrorKind};
use tauri::State;

#[tauri::command]
pub async fn secrets_list(state: State<'_, AppState>) -> Result<Vec<Secret>, AppError> {
    let core = state.core.clone();
    super::blocking(move || core.secrets()).await
}

#[tauri::command]
pub async fn secret_ignore(state: State<'_, AppState>, id: String, on: bool) -> Result<(), AppError> {
    let core = state.core.clone();
    super::blocking(move || core.secret_ignore(&id, on)).await
}

/// The one command that returns a secret value (ADR 0010): only after Touch
/// ID or the user's password. The UI hides it again after 10 s.
#[tauri::command]
pub async fn secret_reveal(state: State<'_, AppState>, id: String) -> Result<String, AppError> {
    let core = state.core.clone();
    super::blocking(move || {
        authenticate("show a secret value from agent history")?;
        core.secret_value(&id)
    })
    .await
}

#[cfg(target_os = "macos")]
fn authenticate(reason: &str) -> Result<(), AppError> {
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_foundation::{NSError, NSString};
    use objc2_local_authentication::{LAContext, LAPolicy};
    let (tx, rx) = std::sync::mpsc::channel::<bool>();
    let ctx = unsafe { LAContext::new() };
    let reason = NSString::from_str(reason);
    let reply = RcBlock::new(move |ok: Bool, _err: *mut NSError| {
        let _ = tx.send(ok.as_bool());
    });
    unsafe { ctx.evaluatePolicy_localizedReason_reply(LAPolicy::DeviceOwnerAuthentication, &reason, &reply) };
    match rx.recv_timeout(std::time::Duration::from_secs(180)) {
        Ok(true) => Ok(()),
        _ => Err(AppError::new(ErrorKind::Cancelled, "Not confirmed. The value stays hidden.")),
    }
}

#[cfg(not(target_os = "macos"))]
fn authenticate(_: &str) -> Result<(), AppError> {
    Err(AppError::new(ErrorKind::Permission, "Showing a value needs macOS."))
}
