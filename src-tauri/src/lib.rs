//! Tauri shell. Commands are thin wrappers over `skilldesk-core`.

mod commands;
mod events;
mod tray;

use skilldesk_core::{Core, Env};
use std::sync::Arc;
use tauri::Manager;

pub struct AppState {
    pub core: Arc<Core>,
    /// Kept alive for the app's lifetime; started again when the
    /// agent list changes in Settings.
    watcher: std::sync::Mutex<Option<skilldesk_core::watch::Watcher>>,
}

impl AppState {
    pub fn restart_watcher(&self, app: &tauri::AppHandle) {
        let mut w = self.watcher.lock().unwrap_or_else(|p| p.into_inner());
        *w = None;
        *w = start_watcher(&self.core, app);
    }
}

fn start_watcher(core: &Arc<Core>, app: &tauri::AppHandle) -> Option<skilldesk_core::watch::Watcher> {
    let sink: Arc<dyn skilldesk_core::scan::ProgressSink> = Arc::new(events::TauriSink(app.clone()));
    skilldesk_core::watch::start(core.clone(), sink)
        .inspect_err(|e| eprintln!("file watcher off: {e}"))
        .ok()
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let core = Core::open(Env::from_os())?;
            let watcher = start_watcher(&core, app.handle());
            app.manage(AppState { core, watcher: std::sync::Mutex::new(watcher) });
            tray::setup(app.handle())?;
            tray::refresh(app.handle());
            Ok(())
        })
        // Closing the window keeps SkillDesk in the menu bar.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::overview::overview,
            commands::a11y::reduce_transparency,
            commands::scan::scan_start,
            commands::items::items_list,
            commands::items::item_document,
            commands::items::memory_rule_targets,
            commands::actions::action_plan,
            commands::actions::action_apply,
            commands::actions::action_undo,
            commands::actions::history_list,
            commands::fixes::fixes_list,
            commands::fixes::fix_dismiss,
            commands::skills::skill_inspect,
            commands::skills::reveal_in_finder,
            commands::skills::skill_targets,
            commands::market::market_board,
            commands::market::market_search,
            commands::market::market_updates,
            commands::market::market_fetch,
            commands::market::market_avatar,
            commands::market::market_open_page,
            commands::sessions::sessions_list,
            commands::sessions::sessions_search,
            commands::sessions::session_resume,
            commands::sessions::session_open_project,
            commands::storage::storage_get,
            commands::storage::storage_refresh,
            commands::secrets::secrets_list,
            commands::secrets::secret_ignore,
            commands::secrets::secret_reveal,
            commands::settings::settings_get,
            commands::settings::agent_set_on,
            commands::settings::custom_agent_save,
            commands::settings::custom_agent_delete,
            commands::settings::pick_folder,
            commands::settings::cli_status,
            commands::settings::cli_install,
        ])
        .build(tauri::generate_context!())
        .expect("error while building SkillDesk")
        .run(|app, event| {
            // Dock icon click brings the hidden window back.
            if let tauri::RunEvent::Reopen { .. } = event {
                tray::show(app);
            }
        });
}
