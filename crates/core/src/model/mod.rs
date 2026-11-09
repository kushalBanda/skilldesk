//! Item model shared by core, app and CLI (Gate 3: Types & signatures).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ts_rs::TS;

/// Agent key ("claude_code", "cursor", …).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AgentKey(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ItemKind {
    Skill,
    Plugin,
    Marketplace,
    PluginCache,
    Hook,
    McpServer,
    Subagent,
    Command,
    Memory,
    Secret,
    Session,
    Storage,
    Agent,
}

impl ItemKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ItemKind::Skill => "skill",
            ItemKind::Plugin => "plugin",
            ItemKind::Marketplace => "marketplace",
            ItemKind::PluginCache => "plugin_cache",
            ItemKind::Hook => "hook",
            ItemKind::McpServer => "mcp_server",
            ItemKind::Subagent => "subagent",
            ItemKind::Command => "command",
            ItemKind::Memory => "memory",
            ItemKind::Secret => "secret",
            ItemKind::Session => "session",
            ItemKind::Storage => "storage",
            ItemKind::Agent => "agent",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
    }
}

/// Stable id: hex sha256 of (kind, canonical path, key inside file).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ItemId(pub String);

impl ItemId {
    pub fn derive(kind: ItemKind, path: &std::path::Path, key: &str) -> Self {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(kind.as_str().as_bytes());
        h.update([0]);
        h.update(path.to_string_lossy().as_bytes());
        h.update([0]);
        h.update(key.as_bytes());
        ItemId(hex::encode(h.finalize()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case", tag = "state", content = "reason")]
#[ts(export)]
pub enum ItemStatus {
    Ok,
    Off,
    Unused,
    Broken,
    Gone,
    Slow,
    /// A folder or file where an item was expected but cannot be one.
    NotAnItem,
    Unreadable(String),
    Unknown,
}

impl ItemStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ItemStatus::Ok => "ok",
            ItemStatus::Off => "off",
            ItemStatus::Unused => "unused",
            ItemStatus::Broken => "broken",
            ItemStatus::Gone => "gone",
            ItemStatus::Slow => "slow",
            ItemStatus::NotAnItem => "not_an_item",
            ItemStatus::Unreadable(_) => "unreadable",
            ItemStatus::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SourceRef {
    #[ts(type = "string")]
    pub path: PathBuf,
    pub line: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case", tag = "kind", content = "target")]
#[ts(export)]
pub enum LinkState {
    RealDir,
    Link(#[ts(type = "string")] PathBuf),
    Dangling(#[ts(type = "string")] PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SkillLocation {
    pub agents: Vec<AgentKey>,
    #[ts(type = "string")]
    pub path: PathBuf,
    pub link: LinkState,
    /// Project root for project-level folders; `None` = global.
    #[ts(type = "string | null")]
    pub project: Option<PathBuf>,
    /// Claude plugin id (`name@marketplace`) and whether that plugin is on.
    pub plugin: Option<PluginRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PluginRef {
    pub id: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SkillFacts {
    pub description: Option<String>,
    /// Canonical directory of the first real copy (where SKILL.md is read).
    #[ts(type = "string")]
    pub home: PathBuf,
    pub locations: Vec<SkillLocation>,
    /// `~/.skilldesk/skills/<name>` (resolved) when the skill is in the library.
    #[serde(default)]
    #[ts(type = "string | null")]
    pub library: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case", tag = "kind")]
#[ts(export)]
pub enum KindFacts {
    Skill(SkillFacts),
    Hook(HookFacts),
    Plugin(PluginFacts),
    Marketplace(MarketplaceFacts),
    PluginCache(CacheFacts),
    McpServer(McpFacts),
    Subagent(DefFacts),
    Command(DefFacts),
    Memory(MemoryFacts),
    None,
}

/// A Claude memory file's `type` frontmatter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MemoryType {
    User,
    Feedback,
    Project,
    Reference,
    Other,
    /// An agent's instructions file (`CLAUDE.md`, `AGENTS.md`, `GEMINI.md`).
    Instructions,
}

/// Where a memory applies: every project (user) or one project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MemoryScope {
    User,
    Project,
}

/// Why a memory may be out of date.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case", tag = "kind")]
#[ts(export)]
pub enum StaleFlag {
    /// The project folder the memory belongs to is gone.
    RepoGone,
    /// Every date it names is in the past; `date` is the newest (YYYY-MM-DD).
    PastDate { date: String },
}

/// One memory file (`~/.claude/projects/<project>/memory/<name>.md`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MemoryFacts {
    pub mem_type: MemoryType,
    pub scope: MemoryScope,
    pub description: Option<String>,
    /// The project folder, when it can be found.
    #[ts(type = "string | null")]
    pub project: Option<PathBuf>,
    /// Short project name for the row ("Athena").
    pub project_label: String,
    /// The project's `MEMORY.md` index, when it lists this file.
    #[ts(type = "string | null")]
    pub index: Option<PathBuf>,
    /// File modified time, RFC 3339.
    pub modified: Option<String>,
    pub stale: Vec<StaleFlag>,
    /// Memories that say the same thing share this key.
    pub cluster: Option<String>,
}

/// Where a subagent or slash command comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DefOrigin {
    /// Defined in one or more files.
    File,
    /// Ships with the agent (Claude's `general-purpose`, `/compact`).
    BuiltIn,
    /// A skill run as a slash command.
    Skill,
    /// Run before, but no file defines it now.
    Unknown,
}

/// A subagent or slash command, one row per name across agents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DefFacts {
    pub origin: DefOrigin,
    /// Claude also has a skill by this name (`/name` runs the skill).
    pub skill: bool,
    pub description: Option<String>,
    /// Subagent frontmatter `model`.
    pub model: Option<String>,
    pub locations: Vec<DefLocation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DefLocation {
    pub agent: AgentKey,
    /// Name as this agent calls it (`plugin:name`, `prompts:name`).
    pub name: String,
    #[ts(type = "string")]
    pub path: PathBuf,
    #[ts(type = "string | null")]
    pub project: Option<PathBuf>,
    pub plugin: Option<PluginRef>,
}

/// One MCP server name across every agent that configures it (the matrix row).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct McpFacts {
    /// Name as Claude spells it in tool names (`mcp__<this>__tool`).
    pub call_name: String,
    pub locations: Vec<McpLocation>,
    /// Some place sets env vars or headers, which often hold keys. Names only.
    pub holds_keys: bool,
    /// Called by Claude but in no config file (built in, or a claude.ai connector).
    pub unconfigured: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct McpLocation {
    /// Stable key of this config entry (actions name it).
    pub entry: String,
    pub agent: AgentKey,
    /// Name as written in this file.
    pub name: String,
    #[ts(type = "string")]
    pub file: PathBuf,
    #[ts(type = "string | null")]
    pub project: Option<PathBuf>,
    pub plugin: Option<String>,
    /// stdio, http or sse.
    pub transport: String,
    /// Command and args, with anything that looks like a key masked.
    pub command: Option<String>,
    /// URL without its query string.
    pub url: Option<String>,
    pub env_keys: Vec<String>,
    pub header_keys: Vec<String>,
    pub enabled: bool,
    pub switch: SwitchKind,
}

/// A Claude plugin: installed, or only remembered in `pluginUsage` (a ghost).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PluginFacts {
    /// `name@marketplace`.
    pub id: String,
    pub marketplace: String,
    pub installed: bool,
    /// `enabledPlugins` value; `None` when the id is not listed.
    pub enabled: Option<bool>,
    pub version: Option<String>,
    /// "user" or "project".
    pub scope: Option<String>,
    #[ts(type = "string | null")]
    pub project: Option<PathBuf>,
    #[ts(type = "string | null")]
    pub install_path: Option<PathBuf>,
    pub installed_at: Option<String>,
    pub updated_at: Option<String>,
    pub contents: PluginContents,
    #[ts(type = "number | null")]
    pub size_bytes: Option<u64>,
    /// How to bring back a plugin that is no longer installed (Claude ghosts only).
    pub restore: Option<PluginRestore>,
}

/// The steps that reinstall a removed plugin, typed in Claude Code in order.
/// `/plugin install` needs the marketplace added, so when it is not, the
/// first step adds it from the source SkillDesk found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PluginRestore {
    /// Empty when no source for the marketplace is known: a bare install fails.
    pub steps: Vec<String>,
    /// Marketplace source (`owner/repo`, a git URL, a URL or a path), when known.
    pub source: Option<String>,
    pub origin: Option<SourceOrigin>,
}

/// Where SkillDesk found a marketplace's source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SourceOrigin {
    /// Added in Claude Code now (`known_marketplaces.json`).
    Added,
    /// Settings `extraKnownMarketplaces`.
    Settings,
    /// Claude transcripts: a typed `/plugin marketplace add`, or an older copy
    /// of `known_marketplaces.json` read in a session.
    History,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PluginContents {
    pub skills: u32,
    pub commands: u32,
    pub agents: u32,
    pub hooks: u32,
    pub mcp_servers: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MarketplaceFacts {
    /// `owner/repo`, a URL, or a local path.
    pub source: String,
    #[ts(type = "string | null")]
    pub install_location: Option<PathBuf>,
    /// The clone lives in `~/.claude/plugins/marketplaces` (SkillDesk may trash it).
    pub removable: bool,
    pub updated_at: Option<String>,
    /// Installed plugins that come from it.
    pub plugins: Vec<String>,
    /// Also listed in settings `extraKnownMarketplaces`.
    pub in_settings: bool,
    #[ts(type = "number | null")]
    pub size_bytes: Option<u64>,
}

/// A plugin cache folder nothing uses: an older version or a temp clone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CacheFacts {
    #[ts(type = "string")]
    pub path: PathBuf,
    /// Plugin id it belonged to, when known.
    pub plugin: Option<String>,
    pub version: Option<String>,
    pub temp: bool,
    #[ts(type = "number")]
    pub size_bytes: u64,
}

/// How a hook can be switched off (Gate 3 write table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SwitchKind {
    /// The agent has its own switch (Codex `hooks.state`).
    Native,
    /// Remove the entry and keep it in `~/.skilldesk/disabled.json`.
    Stash,
    /// Only the owning Claude plugin can be turned off.
    Plugin,
    /// Not switchable here (OpenCode plugin files).
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HookFacts {
    /// Event as the agent spells it (`PreToolUse`, `beforeSubmitPrompt`).
    pub event: String,
    pub matcher: Option<String>,
    /// Command (or prompt text for prompt hooks) exactly as configured.
    pub command: String,
    /// Tool that installed the hook, read from its command or plugin.
    pub owner: Option<String>,
    /// Scripts the command runs that do not exist (after variable expansion).
    #[ts(type = "string[]")]
    pub missing_paths: Vec<PathBuf>,
    pub switch: SwitchKind,
    #[ts(type = "number | null")]
    pub timeout_ms: Option<u64>,
    pub plugin: Option<PluginRef>,
    #[ts(type = "string | null")]
    pub project: Option<PathBuf>,
}

/// Run timing for hooks (Claude transcripts).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Timing {
    #[ts(type = "number")]
    pub total_ms: u64,
    pub p50_ms: u32,
    pub p95_ms: u32,
    pub max_ms: u32,
    pub cancelled: u32,
    /// Non-zero exit other than 2.
    pub failed: u32,
    /// Exit 2: the hook blocked the agent.
    pub blocked: u32,
}

/// All hook runs seen, including hooks no longer configured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HookTotals {
    #[ts(type = "number")]
    pub runs: u64,
    #[ts(type = "number")]
    pub total_ms: u64,
    #[ts(type = "number")]
    pub cancelled: u64,
    #[ts(type = "number")]
    pub failed: u64,
    #[ts(type = "number")]
    pub blocked: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Item {
    pub id: ItemId,
    pub kind: ItemKind,
    pub name: String,
    pub agents: Vec<AgentKey>,
    pub status: ItemStatus,
    pub sources: Vec<SourceRef>,
    pub usage: Option<Usage>,
    pub facts: KindFacts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Usage {
    #[ts(type = "number")]
    pub count: u64,
    /// RFC 3339.
    pub last_used: Option<String>,
    pub timing: Option<Timing>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct KindCount {
    pub kind: ItemKind,
    #[ts(type = "number")]
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AgentSummary {
    pub key: AgentKey,
    pub display_name: String,
    pub tier: AgentTier,
}

/// Full = all data kinds; Skills = skills only; Shared = the `~/.agents` folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AgentTier {
    Full,
    Skills,
    Shared,
}

/// Dashboard payload. `None` = not computed yet (its slice is not built or the
/// stage has not run); the UI hides it rather than showing a fake number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Overview {
    #[ts(type = "number | null")]
    pub prompts: Option<u64>,
    #[ts(type = "number | null")]
    pub active_days: Option<u64>,
    #[ts(type = "number | null")]
    pub skills_used: Option<u64>,
    pub agents: Vec<AgentSummary>,
    pub kinds: Vec<KindCount>,
    /// `None` until the transcript stage has run.
    pub hooks: Option<HookTotals>,
    /// RFC 3339 time of the last finished scan; `None` before the first scan.
    pub scanned_at: Option<String>,
    /// Prompts per local day and agent, oldest first.
    pub activity: Vec<DayCount>,
    /// All-time prompts per agent, weekday (0 = Monday) and local hour.
    pub hours: Vec<HourCount>,
    /// One row per detected agent with data of its own.
    pub agent_rows: Vec<AgentRow>,
    /// Agents SkillDesk can read (Welcome: "and N more").
    pub known_agents: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DayCount {
    /// Local date, `YYYY-MM-DD`.
    pub day: String,
    pub agent: AgentKey,
    pub prompts: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HourCount {
    pub agent: AgentKey,
    pub dow: u8,
    pub hour: u8,
    pub prompts: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AgentRow {
    pub agent: AgentKey,
    pub name: String,
    pub tier: AgentTier,
    pub skills: u32,
    pub hooks: u32,
    pub mcp: u32,
    pub sessions: u32,
}

/// Numbers for one year's Wrapped card. Holds no repo names, paths or
/// secrets: only counts, times, skill and command names, agent names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WrappedData {
    pub year: i32,
    /// Years with prompts, newest first.
    pub years: Vec<i32>,
    #[ts(type = "number")]
    pub prompts: u64,
    /// Local date of the first prompt in the year, `YYYY-MM-DD`.
    pub first_day: Option<String>,
    pub active_days: u32,
    /// Local hour (0–23) with the most prompts.
    pub peak_hour: Option<u8>,
    /// Weekday with the most prompts, 0 = Monday.
    pub top_weekday: Option<u8>,
    pub longest_streak: u32,
    pub busiest_day: Option<DayTotal>,
    /// Most-run skill among skills last used in this year.
    pub top_skill: Option<NameCount>,
    /// Most-typed slash command in this year's transcripts.
    pub top_command: Option<NameCount>,
    /// Prompts per agent, most first.
    pub agents: Vec<AgentShare>,
    /// Claude Code has a companion. Its name stays private.
    pub companion: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DayTotal {
    pub day: String,
    pub prompts: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NameCount {
    pub name: String,
    #[ts(type = "number")]
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AgentShare {
    pub agent: AgentKey,
    pub name: String,
    #[ts(type = "number")]
    pub prompts: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ScanStage {
    Config,
    Counters,
    Transcripts,
    Deep,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ScanProgress {
    pub run_id: String,
    pub stage: ScanStage,
    pub kinds: Vec<ItemKind>,
    #[ts(type = "number")]
    pub elapsed_ms: u64,
    pub done: bool,
}

/// An agent the User added in Settings (ADR 0003).
/// SkillDesk reads and links skills in its folder, nothing else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CustomAgent {
    /// `custom-<name>`; never a built-in key.
    pub key: String,
    pub name: String,
    #[ts(type = "string")]
    pub skills_dir: std::path::PathBuf,
    /// Relative to each repo, e.g. `.myagent/skills`.
    pub project_dir: Option<String>,
}

/// One agent row in Settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AgentSetting {
    pub agent: AgentKey,
    pub name: String,
    pub tier: AgentTier,
    /// On this Mac.
    pub found: bool,
    /// SkillDesk reads it (the switch).
    pub on: bool,
    pub custom: bool,
    #[ts(type = "string")]
    pub skills_dir: std::path::PathBuf,
    /// Other folders it also reads skills from (e.g. `~/.agents/skills`).
    #[ts(type = "Array<string>")]
    pub also_reads: Vec<std::path::PathBuf>,
    pub skills: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SettingsData {
    /// Found first, then by name.
    pub agents: Vec<AgentSetting>,
    pub custom: Vec<CustomAgent>,
    #[ts(type = "string")]
    pub library_dir: std::path::PathBuf,
    pub library_skills: u32,
}

/// The `skilldesk` command in `~/.skilldesk/bin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum CliState {
    NotInstalled,
    Installed,
    /// SkillDesk put it there; this app has a newer one.
    Outdated,
    /// A file SkillDesk did not put there (or one changed since).
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CliStatus {
    pub state: CliState,
    #[ts(type = "string")]
    pub path: std::path::PathBuf,
    /// This app carries the command (false in a build without it).
    pub bundled: bool,
    /// A shell start file already adds `~/.skilldesk/bin` to PATH.
    pub on_path: bool,
    /// The line to add to `~/.zshrc`.
    pub path_line: String,
}
