//! Menu-bar tray: fix count and the top 3 fixes, Open, Rescan, Quit (ADR 0012).

use crate::{events, AppState};
use skilldesk_core::rules::Fix;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, Wry};

const TRAY_ID: &str = "main";

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("SkillDesk")
        .menu(&menu(app, &[])?)
        .show_menu_on_left_click(true)
        .on_menu_event(on_menu);
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn menu(app: &AppHandle, fixes: &[Fix]) -> tauri::Result<Menu<Wry>> {
    let m = Menu::new(app)?;
    let head = match fixes.len() {
        0 => "Nothing to fix".to_string(),
        1 => "1 fix".to_string(),
        n => format!("{n} fixes"),
    };
    m.append(&MenuItem::with_id(app, "head", head, false, None::<&str>)?)?;
    for f in fixes.iter().take(3) {
        m.append(&MenuItem::with_id(app, format!("go:{}", f.link), &f.title, true, None::<&str>)?)?;
    }
    if fixes.len() > 3 {
        m.append(&MenuItem::with_id(app, "go:/fixes", "Show all fixes", true, None::<&str>)?)?;
    }
    m.append(&PredefinedMenuItem::separator(app)?)?;
    m.append(&MenuItem::with_id(app, "open", "Open SkillDesk", true, None::<&str>)?)?;
    m.append(&MenuItem::with_id(app, "rescan", "Rescan", true, None::<&str>)?)?;
    m.append(&PredefinedMenuItem::separator(app)?)?;
    m.append(&MenuItem::with_id(app, "quit", "Quit SkillDesk", true, Some("CmdOrCtrl+Q"))?)?;
    Ok(m)
}

pub fn show(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn on_menu(app: &AppHandle, ev: tauri::menu::MenuEvent) {
    let id = ev.id().as_ref();
    match id {
        "open" => show(app),
        "quit" => app.exit(0),
        "rescan" => {
            let core = app.state::<AppState>().core.clone();
            let app = app.clone();
            tauri::async_runtime::spawn_blocking(move || {
                if let Err(e) = skilldesk_core::scan::run_scan(&core, &events::TauriSink(app)) {
                    eprintln!("scan failed: {e}");
                }
            });
        }
        _ => {
            if let Some(route) = id.strip_prefix("go:") {
                show(app);
                let _ = app.emit(events::NAVIGATE, route);
            }
        }
    }
}

/// Rebuild the menu and count from the current fixes. Cheap: reads the cache.
pub fn refresh(app: &AppHandle) {
    let core = app.state::<AppState>().core.clone();
    let fixes = core.fixes().unwrap_or_default();
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(tray) = app2.tray_by_id(TRAY_ID) else { return };
        if let Ok(m) = menu(&app2, &fixes) {
            let _ = tray.set_menu(Some(m));
        }
        let _ = tray.set_title(if fixes.is_empty() { None } else { Some(fixes.len().to_string()) });
    });
}
