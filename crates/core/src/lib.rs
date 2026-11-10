//! SkillDesk core: all logic, no Tauri types (ADR 0002).
//! Used by the desktop app (`src-tauri`) and the `skilldesk` CLI (`crates/cli`).

pub mod actions;
pub mod adapters;
mod core;
pub mod env;
pub mod error;
pub mod fsx;
pub mod install;
pub mod model;
pub mod net;
pub mod parse;
pub mod rules;
pub mod scan;
pub mod secrets;
pub mod sessions;
pub mod settings;
pub mod storage;
pub mod store;
pub mod watch;

pub use crate::core::Core;
pub use env::Env;
pub use error::{AppError, ErrorKind};
pub use model::*;
