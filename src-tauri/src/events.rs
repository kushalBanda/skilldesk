//! Core → UI events (Gate 2: Events).

use skilldesk_core::{scan::ProgressSink, ScanProgress};
use tauri::{AppHandle, Emitter};

pub const SCAN_PROGRESS: &str = "scan-progress";
pub const INVENTORY_CHANGED: &str = "inventory-changed";
pub const HISTORY_CHANGED: &str = "history-changed";
pub const FIXES_CHANGED: &str = "fixes-changed";
/// Tray → UI: open this route.
pub const NAVIGATE: &str = "navigate";

/// Fixes depend on every kind; recompute for the UI badge and the tray.
pub fn fixes_changed(app: &AppHandle) {
    let _ = app.emit(FIXES_CHANGED, ());
    crate::tray::refresh(app);
}

pub struct TauriSink(pub AppHandle);

impl ProgressSink for TauriSink {
    fn progress(&self, p: &ScanProgress) {
        let _ = self.0.emit(SCAN_PROGRESS, p);
        let _ = self
            .0
            .emit(INVENTORY_CHANGED, serde_json::json!({ "kinds": p.kinds }));
        fixes_changed(&self.0);
    }
}
