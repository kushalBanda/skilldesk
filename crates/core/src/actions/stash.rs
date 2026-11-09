//! `~/.skilldesk/disabled.json`: hooks switched off by removing them from an
//! agent file that has no off switch of its own. Each entry keeps the exact
//! text removed and where it sat, so turning it back on puts it back.

use crate::env::Env;
use crate::model::ItemId;
use crate::parse::hooks::{HookPos, RawHook};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StashEntry {
    pub id: ItemId,
    pub agent: String,
    pub file: PathBuf,
    pub pos: HookPos,
    /// The whole group (`{matcher, hooks: [...]}`) was removed, not one hook in it.
    pub whole_group: bool,
    /// Exact JSON text removed from the file.
    pub text: String,
    /// Exact bytes cut (entry plus one separator), so putting it back is byte-exact.
    #[serde(default)]
    pub cut: String,
    pub matcher: Option<String>,
    pub command: String,
    pub timeout_ms: Option<u64>,
    pub project: Option<PathBuf>,
    pub stashed_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StashFile {
    pub version: u32,
    #[serde(default)]
    pub hooks: Vec<StashEntry>,
    #[serde(default)]
    pub mcp: Vec<McpStashEntry>,
}

/// An MCP server removed from a JSON config that has no off switch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpStashEntry {
    /// `RawMcp::entry()` at the time it was switched off.
    pub entry: String,
    pub agent: String,
    pub file: PathBuf,
    /// Path of the object that held it (`["mcpServers"]`).
    pub path: Vec<String>,
    pub name: String,
    /// Position among the object's members.
    pub index: usize,
    /// Exact member text (`"name": {…}`) and exact bytes cut.
    pub text: String,
    pub cut: String,
    pub project: Option<PathBuf>,
    pub stashed_at: String,
}

pub fn read_mcp(env: &Env) -> Vec<McpStashEntry> {
    load(env).map(|(_, f)| f.mcp).unwrap_or_default()
}

impl McpStashEntry {
    pub fn to_raw(&self) -> crate::scan::mcp::RawMcp {
        let value = self
            .text
            .split_once(':')
            .and_then(|(_, v)| serde_json::from_str(&crate::fsx::json_span::strip_comments(v)).ok())
            .unwrap_or(serde_json::Value::Null);
        crate::scan::mcp::RawMcp {
            agent: static_agent(&self.agent),
            file: self.file.clone(),
            path: self.path.clone(),
            name: self.name.clone(),
            value,
            project: self.project.clone(),
            plugin: None,
            enabled: false,
            format: crate::scan::mcp::Format::Json,
        }
    }
}

pub fn static_agent(a: &str) -> &'static str {
    match a {
        "claude_code" => "claude_code",
        "cursor" => "cursor",
        "gemini_cli" => "gemini_cli",
        "github_copilot" => "github_copilot",
        "codex" => "codex",
        _ => "opencode",
    }
}

pub fn path(env: &Env) -> PathBuf {
    env.skilldesk_dir().join("disabled.json")
}

/// (current text, parsed). A missing file is empty; an unparsable one is an
/// error so it is never overwritten.
pub fn load(env: &Env) -> Result<(Option<String>, StashFile), String> {
    match std::fs::read_to_string(path(env)) {
        Ok(t) => serde_json::from_str(&t).map(|f| (Some(t), f)).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((None, StashFile { version: 1, ..Default::default() })),
        Err(e) => Err(e.to_string()),
    }
}

pub fn read(env: &Env) -> Vec<StashEntry> {
    load(env).map(|(_, f)| f.hooks).unwrap_or_default()
}

pub fn render(f: &StashFile) -> Option<String> {
    if f.hooks.is_empty() && f.mcp.is_empty() {
        return None;
    }
    serde_json::to_string_pretty(f).ok().map(|s| s + "\n")
}

impl StashEntry {
    pub fn to_raw(&self) -> (ItemId, RawHook) {
        let agent = static_agent(&self.agent);
        (
            self.id.clone(),
            RawHook {
                agent,
                file: self.file.clone(),
                event: self.pos.event.clone(),
                matcher: self.matcher.clone(),
                command: self.command.clone(),
                timeout_ms: self.timeout_ms,
                plugin: None,
                plugin_root: None,
                project: self.project.clone(),
                native_key: None,
                pos: Some(self.pos.clone()),
            },
        )
    }
}
