//! macOS display settings the webview cannot read itself.
//! WebKit has no `prefers-reduced-transparency`, so the UI asks here.

/// System Settings → Accessibility → Display → Reduce transparency.
#[tauri::command]
pub fn reduce_transparency() -> bool {
    #[cfg(target_os = "macos")]
    {
        use objc2::runtime::{AnyObject, Bool};
        use objc2::{class, msg_send};
        // SAFETY: NSWorkspace.sharedWorkspace is a process-wide singleton (AppKit is
        // linked by Tauri); the getter takes no arguments and returns BOOL.
        unsafe {
            let ws: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
            if ws.is_null() {
                return false;
            }
            let on: Bool = msg_send![ws, accessibilityDisplayShouldReduceTransparency];
            on.as_bool()
        }
    }
    #[cfg(not(target_os = "macos"))]
    false
}
