//! MCP on/off per config entry, remove, and copy to another agent. Every
//! write lands in the agent's own config file.
//! - Cursor, Gemini, Copilot: no off switch; remove the entry (Undo puts it back).
//! - OpenCode: native `enabled`. Codex: native `enabled` in `[mcp_servers.x]`.
//! - Repo `.mcp.json`: `disabledMcpjsonServers` in `.claude/settings.local.json`.
//! - `~/.claude.json` and plugin servers: refused with the switch that exists.
//!
//! Copy reads the full entry (with key values) inside core and writes it to the
//! target file; values never reach the UI (plans go out redacted).

use super::hooks::{set_toml_enabled, toml_quote};
use super::{new_id, ActionPlan, ActionRequest, FileOp};
use crate::adapters;
use crate::core::Core;
use crate::error::{AppError, ErrorKind};
use crate::fsx::json_span::{self, strip_comments, Seg};
use crate::model::{AgentKey, ItemId, KindFacts};
use crate::scan::mcp::{self as scan, Format, RawMcp};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

fn invalid(msg: impl Into<String>) -> AppError {
    AppError::new(ErrorKind::InvalidInput, msg)
}

fn read(path: &Path) -> Result<Option<String>, AppError> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(AppError::new(ErrorKind::Io, format!("Cannot read {}: {e}", path.display()))),
    }
}

fn unsafe_edit(path: &Path) -> AppError {
    AppError::new(ErrorKind::Parse, format!("Could not edit {} safely. Nothing was changed.", path.display()))
}

fn valid_json(path: &Path, t: &str) -> Result<(), AppError> {
    serde_json::from_str::<Value>(&strip_comments(t)).map(|_| ()).map_err(|_| unsafe_edit(path))
}

fn segs(path: &[String]) -> Vec<Seg> {
    path.iter().map(|k| Seg::Key(k.clone())).collect()
}

fn display(agent: &str) -> String {
    adapters::find(agent).map(|a| a.display_name.to_string()).unwrap_or_else(|| agent.to_string())
}

fn server_name(core: &Core, id: &ItemId) -> Result<(String, String), AppError> {
    let item = core
        .store
        .item(id)?
        .ok_or_else(|| AppError::new(ErrorKind::NotFound, "This server is no longer in the inventory. Rescan and try again."))?;
    match item.facts {
        KindFacts::McpServer(f) => Ok((item.name, f.call_name)),
        _ => Err(invalid("Not an MCP server.")),
    }
}

pub fn set_enabled(core: &Core, req: ActionRequest, id: &ItemId, entry: &str, on: bool) -> Result<ActionPlan, AppError> {
    let (name, _) = server_name(core, id)?;
    let env = &core.env;
    let mut ops = Vec::new();
    let raw = scan::configured(env)
        .into_iter()
        .find(|r| r.entry() == entry)
        .ok_or_else(|| AppError::new(ErrorKind::NotFound, "This config entry changed. Rescan and try again."))?;
    if raw.enabled == on {
        return Err(invalid(if on { "This server is already on here." } else { "This server is already off here." }));
    }
    let path = raw.file.clone();
    match raw.format {
        Format::ClaudeJson => {
            let scope = if raw.project.is_some() { " -s local" } else { " -s user" };
            return Err(invalid(format!(
                "Claude keeps this server in ~/.claude.json, which it rewrites while running. Use `claude mcp remove{scope} {}` in that project.",
                raw.name
            )));
        }
        Format::Plugin => {
            return Err(invalid(format!(
                "This server comes with the plugin {}. Turning the plugin off is the only switch.",
                raw.plugin.clone().unwrap_or_default()
            )))
        }
        Format::Json => {
            return Err(invalid(format!(
                "{} has no off switch for one server. Remove it instead; Undo puts it back.",
                display(raw.agent)
            )))
        }
        Format::OpenCode => {
            let before = read(&path)?.ok_or_else(|| unsafe_edit(&path))?;
            let mut p = segs(&raw.path);
            p.push(Seg::Key(raw.name.clone()));
            let after = json_span::set_member(&before, &p, "enabled", if on { "true" } else { "false" })
                .ok_or_else(|| unsafe_edit(&path))?;
            valid_json(&path, &after)?;
            ops.push(FileOp::edit_text(path.clone(), Some(&before), Some(&after)));
        }
        Format::CodexToml => {
            let before = read(&path)?.ok_or_else(|| unsafe_edit(&path))?;
            let header = format!("[mcp_servers.{}]", toml_key(&raw.name));
            let after = set_toml_enabled(&before, &header, on);
            after.parse::<toml::Table>().map_err(|_| unsafe_edit(&path))?;
            ops.push(FileOp::edit_text(path.clone(), Some(&before), Some(&after)));
        }
        Format::McpJson => {
            let project = raw.project.clone().ok_or_else(|| invalid("Repo server without a project."))?;
            let local = project.join(".claude/settings.local.json");
            let before = read(&local)?;
            let base = before.clone().unwrap_or_else(|| "{\n}\n".into());
            let key = Seg::Key("disabledMcpjsonServers".into());
            let quoted = serde_json::to_string(&raw.name).unwrap_or_default();
            let after = if on {
                let arr = json_span::array_at(&base, std::slice::from_ref(&key));
                let v: Value = serde_json::from_str(&strip_comments(&base)).map_err(|_| unsafe_edit(&local))?;
                let idx = v
                    .get("disabledMcpjsonServers")
                    .and_then(|a| a.as_array())
                    .and_then(|a| a.iter().position(|x| x.as_str() == Some(&raw.name)));
                match (arr, idx) {
                    (Some(_), Some(i)) => json_span::remove_element(&base, &[key], i).map(|r| r.0),
                    _ => None,
                }
                .ok_or_else(|| invalid("Turn it on in the other Claude settings file of this project."))?
            } else {
                match json_span::array_at(&base, std::slice::from_ref(&key)) {
                    Some(a) => json_span::insert_element(&base, &[key], a.elems.len(), &quoted, None),
                    None => json_span::set_member(&base, &[], "disabledMcpjsonServers", &format!("[{quoted}]")),
                }
                .ok_or_else(|| unsafe_edit(&local))?
            };
            valid_json(&local, &after)?;
            ops.push(FileOp::edit_text(local, before.as_deref(), Some(&after)));
        }
    }
    let verb = if on { "Turned on" } else { "Turned off" };
    let where_ = raw.project.as_ref().map(|p| format!(" in {}", p.file_name().unwrap_or_default().to_string_lossy())).unwrap_or_default();
    Ok(plan(req, format!("{verb} {name} for {}{where_}", display(raw.agent)), raw.agent, ops))
}

/// Takes one config entry out of its JSON file. Undo puts the file back.
pub fn remove(core: &Core, req: ActionRequest, id: &ItemId, entry: &str) -> Result<ActionPlan, AppError> {
    let (name, _) = server_name(core, id)?;
    let raw = scan::configured(&core.env)
        .into_iter()
        .find(|r| r.entry() == entry)
        .ok_or_else(|| AppError::new(ErrorKind::NotFound, "This config entry changed. Rescan and try again."))?;
    match raw.format {
        Format::Json | Format::OpenCode | Format::McpJson => {}
        Format::ClaudeJson => {
            let scope = if raw.project.is_some() { " -s local" } else { " -s user" };
            return Err(invalid(format!(
                "Claude keeps this server in ~/.claude.json, which it rewrites while running. Use `claude mcp remove{scope} {}`.",
                raw.name
            )));
        }
        Format::Plugin => {
            return Err(invalid(format!("This server comes with the plugin {}. Remove the plugin instead.", raw.plugin.clone().unwrap_or_default())))
        }
        Format::CodexToml => return Err(invalid(format!("Use `codex mcp remove {}`, or turn it off here.", raw.name))),
    }
    let path = raw.file.clone();
    let before = read(&path)?.ok_or_else(|| unsafe_edit(&path))?;
    let (after, _, _, _) = json_span::remove_member_cut(&before, &segs(&raw.path), &raw.name).ok_or_else(|| unsafe_edit(&path))?;
    valid_json(&path, &after)?;
    let mut p = plan(req, format!("Removed {name} from {}", display(raw.agent)), raw.agent, vec![FileOp::edit_text(path, Some(&before), Some(&after))]);
    p.needs_confirm = true;
    Ok(p)
}

fn plan(req: ActionRequest, summary: String, agent: &str, ops: Vec<FileOp>) -> ActionPlan {
    ActionPlan { id: new_id(), request: req, summary, agents: vec![AgentKey(agent.to_string())], ops, needs_confirm: false, removed: vec![], untouched: vec![] }
}

fn toml_key(name: &str) -> String {
    if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        name.to_string()
    } else {
        toml_quote(name)
    }
}

/// Transport-neutral view of a server entry, read from any agent's format.
struct Spec {
    command: Option<String>,
    args: Vec<String>,
    env: Map<String, Value>,
    url: Option<String>,
    headers: Map<String, Value>,
    sse: bool,
}

fn spec_of(r: &RawMcp) -> Spec {
    let v = &r.value;
    let obj = |k: &str| v.get(k).and_then(|o| o.as_object()).cloned().unwrap_or_default();
    let (command, mut args) = match v.get("command") {
        Some(Value::Array(a)) => {
            let mut it = a.iter().filter_map(|x| x.as_str().map(str::to_string));
            (it.next(), it.collect())
        }
        Some(Value::String(s)) => (Some(s.clone()), Vec::new()),
        _ => (None, Vec::new()),
    };
    args.extend(v.get("args").and_then(|a| a.as_array()).into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)));
    let mut env = obj("env");
    env.extend(obj("environment"));
    let mut headers = obj("headers");
    headers.extend(obj("http_headers"));
    Spec {
        command,
        args,
        env,
        url: ["url", "httpUrl", "serverUrl"].iter().find_map(|k| v.get(*k).and_then(|u| u.as_str()).map(str::to_string)),
        headers,
        sse: v.get("type").and_then(|t| t.as_str()) == Some("sse"),
    }
}

fn json_for(agent: &str, s: &Spec) -> Option<Value> {
    let mut m = Map::new();
    let put = |m: &mut Map<String, Value>, k: &str, v: Value| {
        let empty = matches!(&v, Value::Object(o) if o.is_empty()) || matches!(&v, Value::Array(a) if a.is_empty());
        if !empty {
            m.insert(k.to_string(), v);
        }
    };
    match (agent, &s.command, &s.url) {
        ("opencode", Some(c), _) => {
            let mut cmd = vec![Value::String(c.clone())];
            cmd.extend(s.args.iter().cloned().map(Value::String));
            m.insert("type".into(), json!("local"));
            m.insert("command".into(), Value::Array(cmd));
            put(&mut m, "environment", Value::Object(s.env.clone()));
            m.insert("enabled".into(), json!(true));
        }
        ("opencode", None, Some(u)) => {
            m.insert("type".into(), json!("remote"));
            m.insert("url".into(), json!(u));
            put(&mut m, "headers", Value::Object(s.headers.clone()));
            m.insert("enabled".into(), json!(true));
        }
        ("github_copilot", Some(c), _) => {
            m.insert("type".into(), json!("local"));
            m.insert("command".into(), json!(c));
            m.insert("args".into(), json!(s.args));
            put(&mut m, "env", Value::Object(s.env.clone()));
            m.insert("tools".into(), json!(["*"]));
        }
        ("github_copilot", None, Some(u)) => {
            m.insert("type".into(), json!(if s.sse { "sse" } else { "http" }));
            m.insert("url".into(), json!(u));
            put(&mut m, "headers", Value::Object(s.headers.clone()));
            m.insert("tools".into(), json!(["*"]));
        }
        ("gemini_cli", None, Some(u)) => {
            m.insert(if s.sse { "url" } else { "httpUrl" }.into(), json!(u));
            put(&mut m, "headers", Value::Object(s.headers.clone()));
        }
        (_, Some(c), _) => {
            m.insert("command".into(), json!(c));
            put(&mut m, "args", json!(s.args));
            put(&mut m, "env", Value::Object(s.env.clone()));
        }
        (_, None, Some(u)) => {
            m.insert("url".into(), json!(u));
            put(&mut m, "headers", Value::Object(s.headers.clone()));
        }
        _ => return None,
    }
    Some(Value::Object(m))
}

/// Pretty JSON whose continuation lines start at `indent`.
fn pretty(v: &Value, indent: &str) -> String {
    let s = serde_json::to_string_pretty(v).unwrap_or_default();
    s.lines().enumerate().map(|(i, l)| if i == 0 { l.to_string() } else { format!("{indent}{l}") }).collect::<Vec<_>>().join("\n")
}

fn target_file(core: &Core, agent: &str) -> Option<(PathBuf, Vec<String>)> {
    let h = &core.env.home;
    Some(match agent {
        "cursor" => (h.join(".cursor/mcp.json"), vec!["mcpServers".into()]),
        "gemini_cli" => (h.join(".gemini/settings.json"), vec!["mcpServers".into()]),
        "github_copilot" => (h.join(".copilot/mcp-config.json"), vec!["mcpServers".into()]),
        "opencode" => {
            let jsonc = core.env.resolve(".config/opencode/opencode.jsonc");
            let json = core.env.resolve(".config/opencode/opencode.json");
            (if json.exists() || !jsonc.exists() { json } else { jsonc }, vec!["mcp".into()])
        }
        "codex" => (h.join(".codex/config.toml"), vec!["mcp_servers".into()]),
        _ => return None,
    })
}

pub fn copy_to_agent(core: &Core, req: ActionRequest, id: &ItemId, agent: &AgentKey) -> Result<ActionPlan, AppError> {
    let (name, call_name) = server_name(core, id)?;
    if agent.0 == "claude_code" {
        return Err(invalid(format!(
            "Claude keeps servers in ~/.claude.json, which it rewrites while running. Add it with `claude mcp add-json {} '<config>'`.",
            name
        )));
    }
    if !adapters::find(&agent.0).is_some_and(|a| a.is_installed(&core.env)) {
        return Err(invalid("That agent is not installed on this Mac."));
    }
    let all = scan::configured(&core.env);
    if all.iter().any(|r| r.agent == agent.0 && r.call_name() == call_name) {
        return Err(invalid(format!("{} already has {name}.", display(&agent.0))));
    }
    let source = all
        .iter()
        .filter(|r| r.call_name() == call_name && r.enabled)
        .chain(all.iter().filter(|r| r.call_name() == call_name))
        .next()
        .ok_or_else(|| invalid("No config of this server to copy from."))?;
    let spec = spec_of(source);
    let (file, path) = target_file(core, &agent.0).ok_or_else(|| invalid("SkillDesk cannot write MCP servers for that agent."))?;
    let before = read(&file)?;
    let after = if agent.0 == "codex" {
        let value = json_for("codex", &spec).ok_or_else(|| invalid("This server has neither a command nor a URL."))?;
        let mut server: toml::Table = serde_json::from_value(value).map_err(|_| unsafe_edit(&file))?;
        if let Some(h) = server.remove("headers") {
            server.insert("http_headers".into(), h);
        }
        let mut table = toml::Table::new();
        table.insert(source.name.clone(), toml::Value::Table(server));
        let mut root = toml::Table::new();
        root.insert("mcp_servers".into(), toml::Value::Table(table));
        let block = toml::to_string(&root).map_err(|_| unsafe_edit(&file))?;
        let mut t = before.clone().unwrap_or_default();
        if !t.is_empty() && !t.ends_with('\n') {
            t.push('\n');
        }
        if !t.is_empty() {
            t.push('\n');
        }
        t.push_str(&block);
        t.parse::<toml::Table>().map_err(|_| unsafe_edit(&file))?;
        t
    } else {
        let value = json_for(&agent.0, &spec).ok_or_else(|| invalid("This server has neither a command nor a URL."))?;
        let base = before.clone().unwrap_or_else(|| "{\n}\n".into());
        let p = segs(&path);
        let out = match json_span::object_at(&base, &p) {
            Some(o) => {
                let indent = o.members.last().map(|m| json_span::indent_at(&base, m.1)).unwrap_or_else(|| "    ".into());
                json_span::set_member(&base, &p, &source.name, &pretty(&value, &indent))
            }
            None => {
                let mut wrap = Map::new();
                wrap.insert(source.name.clone(), value);
                json_span::set_member(&base, &[], &path[0], &pretty(&Value::Object(wrap), "  "))
            }
        }
        .ok_or_else(|| unsafe_edit(&file))?;
        valid_json(&file, &out)?;
        out
    };
    Ok(plan(
        req,
        format!("Copied {name} to {}", display(&agent.0)),
        &agent.0,
        vec![FileOp::edit_text(file, before.as_deref(), Some(&after))],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::Env;

    const CURSOR: &str = "{\n  \"mcpServers\": {\n    \"kite\": { \"command\": \"npx\", \"args\": [\"kite\"] },\n    \"ctx\": {\n      \"url\": \"https://ctx.example/mcp?token=QUERYSECRET123\",\n      \"headers\": { \"API_KEY\": \"sk-SECRET-VALUE-0123456789abcdef\" }\n    }\n  }\n}\n";

    /// Key values from config files never reach the item cache or the plan the
    /// UI receives; removing the entry then Undo gives the file back exactly.
    #[test]
    fn secret_value_never_in_item_or_ipc_payload() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().canonicalize().unwrap();
        std::fs::create_dir_all(home.join(".cursor")).unwrap();
        std::fs::write(home.join(".cursor/mcp.json"), CURSOR).unwrap();
        let core = Core::open(Env::for_test(&home, &home.join("data"))).unwrap();
        crate::scan::run_scan(&core, &crate::scan::NoSink).unwrap();

        let items = core.items(crate::model::ItemKind::McpServer).unwrap();
        let all = serde_json::to_string(&items).unwrap();
        assert!(!all.contains("SECRET"), "item cache holds a key value: {all}");
        let ctx = items.iter().find(|i| i.name == "ctx").unwrap();
        let KindFacts::McpServer(f) = &ctx.facts else { panic!() };
        assert_eq!(f.locations[0].header_keys, vec!["API_KEY".to_string()]);

        let entry = f.locations[0].entry.clone();
        // Cursor has no off switch: "off" is refused, and nothing is written.
        let off = core.action_plan(ActionRequest::McpSetEnabled { id: ctx.id.clone(), entry: entry.clone(), on: false });
        assert_eq!(off.unwrap_err().kind, ErrorKind::InvalidInput);

        let plan = core.action_plan(ActionRequest::McpRemove { id: ctx.id.clone(), entry }).unwrap();
        let ui = serde_json::to_string(&plan.redacted()).unwrap();
        assert!(!ui.contains("SECRET"), "plan sent to the UI holds a key value");
        let rec = core.action_apply(&plan.id).unwrap();
        assert!(!serde_json::to_string(&rec.redacted()).unwrap().contains("SECRET"));
        assert!(!std::fs::read_to_string(home.join(".cursor/mcp.json")).unwrap().contains("ctx"));

        core.action_undo(&rec.id).unwrap();
        assert_eq!(std::fs::read_to_string(home.join(".cursor/mcp.json")).unwrap(), CURSOR);
    }
}
