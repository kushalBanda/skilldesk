use crate::AppState;
use skilldesk_core::{AppError, Item, ItemId, ItemKind};
use tauri::State;

#[tauri::command]
pub async fn items_list(state: State<'_, AppState>, kind: ItemKind) -> Result<Vec<Item>, AppError> {
    let core = state.core.clone();
    super::blocking(move || core.items(kind)).await
}

#[tauri::command]
pub async fn item_document(state: State<'_, AppState>, id: ItemId) -> Result<String, AppError> {
    let core = state.core.clone();
    super::blocking(move || core.item_document(&id)).await
}

/// Agents whose global instructions file can take a rule from a memory.
#[tauri::command]
pub async fn memory_rule_targets(state: State<'_, AppState>) -> Result<Vec<skilldesk_core::actions::memory::RuleTarget>, AppError> {
    let core = state.core.clone();
    super::blocking(move || Ok(skilldesk_core::actions::memory::rule_targets(&core))).await
}
