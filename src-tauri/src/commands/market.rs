//! Marketplace. The only commands that use the network; each runs
//! only because the Marketplace screen asked.
use crate::AppState;
use skilldesk_core::net::market::{self, MarketFetched, MarketList, Updates};
use skilldesk_core::net::skillssh::Board;
use skilldesk_core::AppError;
use tauri::State;

#[tauri::command]
pub async fn market_board(state: State<'_, AppState>, board: Board) -> Result<MarketList, AppError> {
    let core = state.core.clone();
    market::board(&core, board).await
}

#[tauri::command]
pub async fn market_search(state: State<'_, AppState>, query: String) -> Result<MarketList, AppError> {
    let core = state.core.clone();
    market::search(&core, &query).await
}

#[tauri::command]
pub async fn market_updates(state: State<'_, AppState>, check: bool) -> Result<Updates, AppError> {
    let core = state.core.clone();
    market::updates(&core, check).await
}

#[tauri::command]
pub async fn market_fetch(state: State<'_, AppState>, source: String, skill_id: String) -> Result<MarketFetched, AppError> {
    let core = state.core.clone();
    market::fetch(&core, &source, &skill_id).await
}

/// The owner's GitHub avatar as a `data:` URL (cached a week).
#[tauri::command]
pub async fn market_avatar(state: State<'_, AppState>, owner: String) -> Result<String, AppError> {
    let core = state.core.clone();
    market::avatar(&core, &owner).await
}

/// Opens the skill's skills.sh page in the default browser.
#[tauri::command]
pub fn market_open_page(source: String, skill_id: String) -> Result<(), AppError> {
    let url = market::page_url(&source, &skill_id)?;
    std::process::Command::new("/usr/bin/open")
        .arg(&url)
        .status()
        .map_err(|e| AppError::internal(format!("Cannot open the browser: {e}")))?;
    Ok(())
}
