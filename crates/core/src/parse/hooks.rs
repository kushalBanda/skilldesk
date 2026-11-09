//! Hook configuration for the six full-tier agents (agent-data.md §5a) and the
//! command analysis that finds missing scripts and the tool that owns a hook.
//!
//! Shapes:
//! - Claude, Codex, Gemini: `hooks: { Event: [ { matcher?, hooks: [ { type, command | prompt, timeout } ] } ] }`
//! - Cursor: `hooks: { event: [ { command, matcher?, timeout } ] }`
//! - Copilot: `hooks: { Event: [ { type, bash | command, timeoutSec } ] }`
//! - OpenCode: JS/TS plugin files act as hooks.

use crate::env::Env;
use crate::model::PluginRef;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// One hook entry as found in a config file.
#[derive(Debug, Clone)]
pub struct RawHook {
    pub agent: &'static str,
    pub file: PathBuf,
    pub event: String,
    pub matcher: Option<String>,
    pub command: String,
    pub timeout_ms: Option<u64>,
    pub plugin: Option<PluginRef>,
    /// `${CLAUDE_PLUGIN_ROOT}` for plugin hooks.
    pub plugin_root: Option<PathBuf>,
    pub project: Option<PathBuf>,
    /// Codex `hooks.state` key: `<file>:<snake_event>:<group>:<hook>`.
    pub native_key: Option<String>,
    /// Where the entry sits: `hooks.<event>[group]` and, for nested shapes,
    /// `.hooks[hook]`. `None` for OpenCode plugin files.
    pub pos: Option<HookPos>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HookPos {
    pub event: String,
    pub group: usize,
    pub hook: Option<usize>,
}

/// Seconds or milliseconds, per agent.
#[derive(Clone, Copy)]
pub enum TimeoutUnit {
    Seconds,
    Millis,
}

pub struct FileSpec {
    pub agent: &'static str,
    pub path: PathBuf,
    pub unit: TimeoutUnit,
    pub plugin: Option<(PluginRef, PathBuf)>,
    pub project: Option<PathBuf>,
    pub codex_keys: bool,
}

/// Reads one hooks file. Missing or unreadable files give no hooks; a file that
/// is not JSON gives `Err` so the scan can mark it.
pub fn read_file(spec: &FileSpec) -> Result<Vec<RawHook>, String> {
    let Ok(text) = std::fs::read_to_string(&spec.path) else {
        return Ok(Vec::new());
    };
    let root: serde_json::Value =
        serde_json::from_str(&crate::fsx::json_span::strip_comments(&text)).map_err(|e| e.to_string())?;
    let Some(events) = root.get("hooks").and_then(|h| h.as_object()) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for (event, groups) in events {
        let Some(groups) = groups.as_array() else { continue };
        for (gi, group) in groups.iter().enumerate() {
            let matcher = group.get("matcher").and_then(|m| m.as_str()).map(str::to_string);
            let nested = group.get("hooks").and_then(|h| h.as_array());
            let inner: Vec<&serde_json::Value> = match nested {
                Some(list) => list.iter().collect(),
                None => vec![group],
            };
            for (hi, hook) in inner.into_iter().enumerate() {
                let Some(command) = ["command", "bash", "prompt"]
                    .iter()
                    .find_map(|k| hook.get(*k).and_then(|v| v.as_str()))
                else {
                    continue;
                };
                let timeout_ms = hook
                    .get("timeoutSec")
                    .and_then(|v| v.as_f64())
                    .map(|s| (s * 1000.0) as u64)
                    .or_else(|| {
                        hook.get("timeout").and_then(|v| v.as_f64()).map(|t| match spec.unit {
                            TimeoutUnit::Seconds => (t * 1000.0) as u64,
                            TimeoutUnit::Millis => t as u64,
                        })
                    });
                out.push(RawHook {
                    agent: spec.agent,
                    file: spec.path.clone(),
                    event: event.clone(),
                    matcher: matcher.clone(),
                    command: command.to_string(),
                    timeout_ms,
                    plugin: spec.plugin.as_ref().map(|(p, _)| p.clone()),
                    plugin_root: spec.plugin.as_ref().map(|(_, r)| r.clone()),
                    project: spec.project.clone(),
                    native_key: spec
                        .codex_keys
                        .then(|| format!("{}:{}:{gi}:{hi}", spec.path.display(), snake(event))),
                    pos: Some(HookPos { event: event.clone(), group: gi, hook: nested.is_some().then_some(hi) }),
                });
            }
        }
    }
    Ok(out)
}

/// Codex `config.toml` `[hooks.state."<key>"] enabled = false` → disabled keys.
pub fn codex_disabled(env: &Env) -> HashSet<String> {
    let Ok(text) = std::fs::read_to_string(env.home.join(".codex/config.toml")) else {
        return HashSet::new();
    };
    let Ok(doc) = text.parse::<toml::Table>() else {
        return HashSet::new();
    };
    doc.get("hooks")
        .and_then(|h| h.get("state"))
        .and_then(|s| s.as_table())
        .map(|t| {
            t.iter()
                .filter(|(_, v)| v.get("enabled").and_then(|e| e.as_bool()) == Some(false))
                .map(|(k, _)| k.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// OpenCode plugin files (`~/.config/opencode/plugins/*`) — each one is a hook set.
pub fn opencode_plugins(env: &Env) -> Vec<RawHook> {
    let dir = env.resolve(".config/opencode/plugins");
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut out: Vec<RawHook> = entries
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| RawHook {
            agent: "opencode",
            file: e.path(),
            event: "plugin".into(),
            matcher: None,
            command: e.path().to_string_lossy().into_owned(),
            timeout_ms: None,
            plugin: None,
            plugin_root: None,
            project: None,
            native_key: None,
            pos: None,
        })
        .collect();
    out.sort_by(|a, b| a.file.cmp(&b.file));
    out
}

fn snake(event: &str) -> String {
    let mut s = String::new();
    for (i, c) in event.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                s.push('_');
            }
            s.push(c.to_ascii_lowercase());
        } else {
            s.push(c);
        }
    }
    s
}

// ---------------------------------------------------------------------------
// Command analysis

/// Variables a hook command may use for paths. Anything else is unknown, and a
/// word that uses an unknown variable is not checked (no false "missing").
pub struct Vars<'a> {
    pub home: &'a Path,
    pub plugin_root: Option<&'a Path>,
    pub project: Option<&'a Path>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Analysis {
    pub missing: Vec<PathBuf>,
    pub owner: Option<String>,
    pub label: String,
}

const INTERPRETERS: &[&str] = &[
    "sh", "bash", "zsh", "dash", "fish", "node", "bun", "deno", "python", "python3", "ruby", "perl", "tsx",
    "ts-node", "osascript",
];
const PASSTHROUGH: &[&str] = &["npx", "bunx", "uvx", "pnpx", "exec", "command", "env", "nohup", "time"];
const NOT_OWNERS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "case", "esac", "do", "done", "for", "while", "in", "!", "{", "}",
    "[", "[[", "test", ":", "true", "false", "printf", "echo", "cat", "exit", "return", "export", "set",
    "read", "jq", "sed", "grep", "awk", "tee", "curl", "mkdir", "rm", "cp", "mv", "touch", "date", "sleep",
    "kill", "open", "say", "git", "wc", "ls", "head", "tail", "cut", "sort", "find", "xargs", "basename",
    "dirname", "pwd", "cd", "which", "source", "eval", "hook",
];
/// Dot-folders that belong to agents or runtimes, never to the hook's owner.
const PLAIN_DIRS: &[&str] = &[
    "claude", "cursor", "codex", "gemini", "copilot", "config", "local", "bun", "npm", "cargo", "nvm", "pyenv",
    "deno", "agents", "opencode", "volta", "asdf", "rbenv",
];

pub fn analyze(command: &str, vars: &Vars) -> Analysis {
    let cmds = split_commands(command);
    let mut candidates: Vec<(PathBuf, usize)> = Vec::new();
    let mut guarded: HashSet<PathBuf> = HashSet::new();
    let mut path_words: Vec<PathBuf> = Vec::new();
    let mut bare: Option<String> = None;
    collect(&cmds, vars, &mut candidates, &mut guarded, &mut path_words, &mut bare, 0);

    let mut missing: Vec<PathBuf> = Vec::new();
    for (p, _) in &candidates {
        if !guarded.contains(p) && !p.exists() && !missing.contains(p) {
            missing.push(p.clone());
        }
    }
    let owner = path_words
        .iter()
        .find_map(|p| owner_from_path(p, vars.home))
        .or_else(|| bare.map(|b| b.split('\u{1f}').next().unwrap_or("").to_string()));
    Analysis { missing, owner, label: label(command, &cmds, vars) }
}

fn collect(
    cmds: &[Vec<String>],
    vars: &Vars,
    candidates: &mut Vec<(PathBuf, usize)>,
    guarded: &mut HashSet<PathBuf>,
    path_words: &mut Vec<PathBuf>,
    bare: &mut Option<String>,
    depth: usize,
) {
    for (ci, words) in cmds.iter().enumerate() {
        let words: Vec<&String> = words
            .iter()
            .filter(|w| !is_redirect(w))
            .collect();
        for w in &words {
            if let Some(p) = expand(w, vars) {
                path_words.push(p);
            }
        }
        // Skip keywords and VAR=value prefixes to the command word.
        let mut i = 0;
        while i < words.len() && (is_keyword(words[i]) || is_assignment(words[i])) {
            i += 1;
        }
        let Some(first) = words.get(i) else { continue };
        if first.as_str() == "[" || first.as_str() == "[[" || first.as_str() == "test" {
            for pair in words[i..].windows(2) {
                if matches!(pair[0].as_str(), "-f" | "-r" | "-x" | "-e" | "-d" | "-s" | "-L") {
                    if let Some(p) = expand(pair[1], vars) {
                        guarded.insert(p);
                    }
                }
            }
            continue;
        }
        let mut cur = i;
        while let Some(w) = words.get(cur) {
            let base = basename(w);
            if PASSTHROUGH.contains(&base.as_str()) {
                cur += 1;
                while words.get(cur).is_some_and(|w| w.starts_with('-')) {
                    cur += 1;
                }
                continue;
            }
            break;
        }
        let Some(exec) = words.get(cur) else { continue };
        let base = basename(exec);
        if let Some(p) = expand(exec, vars) {
            candidates.push((p, ci));
        } else if bare.is_none() && !exec.contains('$') && !NOT_OWNERS.contains(&base.as_str())
            && !INTERPRETERS.contains(&base.as_str()) && looks_like_name(&base)
        {
            *bare = Some(base.clone());
            // Remember "tool subcommand" for the label.
            let sub = words[cur + 1..].iter().find(|w| !w.starts_with('-')).filter(|w| looks_like_name(w));
            *bare = Some(match sub {
                Some(sub) => format!("{base}\u{1f}{sub}"),
                None => base.clone(),
            });
        }
        if INTERPRETERS.contains(&base.as_str()) {
            let mut j = cur + 1;
            while let Some(w) = words.get(j) {
                if w.as_str() == "-c" && depth < 3 {
                    if let Some(script) = words.get(j + 1) {
                        collect(&split_commands(script), vars, candidates, guarded, path_words, bare, depth + 1);
                    }
                    break;
                }
                if w.starts_with('-') {
                    j += 1;
                    continue;
                }
                if let Some(p) = expand(w, vars) {
                    candidates.push((p, ci));
                }
                break;
            }
        }
    }
}

fn looks_like_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        && s.chars().next().is_some_and(|c| c.is_ascii_lowercase())
}

fn is_keyword(w: &str) -> bool {
    matches!(w, "if" | "then" | "else" | "elif" | "fi" | "do" | "done" | "while" | "until" | "!" | "{" | "}" | "time")
}

fn is_assignment(w: &str) -> bool {
    match w.find('=') {
        Some(i) if i > 0 => w[..i].chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
        _ => false,
    }
}

fn is_redirect(w: &str) -> bool {
    let t = w.trim_start_matches(|c: char| c.is_ascii_digit() || c == '&');
    t.starts_with('>') || t.starts_with('<')
}

fn basename(w: &str) -> String {
    w.rsplit('/').next().unwrap_or(w).to_string()
}

/// A word that names a file: `/abs`, `~/…`, `$HOME/…`, `${HOME-}/…`,
/// `${CLAUDE_PLUGIN_ROOT}/…`. `None` for anything else, or an unknown variable.
pub fn expand(word: &str, vars: &Vars) -> Option<PathBuf> {
    let mut out = String::new();
    let mut rest = word;
    if let Some(r) = rest.strip_prefix("~/") {
        out.push_str(&vars.home.to_string_lossy());
        out.push('/');
        rest = r;
    }
    while let Some(i) = rest.find('$') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let (name, tail) = if let Some(b) = after.strip_prefix('{') {
            let end = b.find('}')?;
            let inner = &b[..end];
            let name_end = inner.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(inner.len());
            (&inner[..name_end], &b[end + 1..])
        } else {
            let end = after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(after.len());
            (&after[..end], &after[end..])
        };
        let value = match name {
            "HOME" => vars.home,
            "CLAUDE_PLUGIN_ROOT" => vars.plugin_root?,
            "CLAUDE_PROJECT_DIR" => vars.project?,
            _ => return None,
        };
        out.push_str(&value.to_string_lossy());
        rest = tail;
    }
    out.push_str(rest);
    if out.starts_with('/') && out.len() > 1 && !out.contains(['*', '?']) {
        Some(PathBuf::from(out))
    } else {
        None
    }
}

fn owner_from_path(p: &Path, home: &Path) -> Option<String> {
    if let Ok(rel) = p.strip_prefix("/Applications") {
        let app = rel.components().next()?.as_os_str().to_string_lossy();
        return app.strip_suffix(".app").map(|s| s.to_lowercase());
    }
    let rel = p.strip_prefix(home).ok()?;
    let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    let s: Vec<&str> = parts.iter().map(String::as_str).collect();
    match s.as_slice() {
        [".claude", "plugins", "marketplaces", m, ..] => return Some((*m).into()),
        [".claude", "plugins", "cache", _, p, ..] => return Some((*p).into()),
        [_, "skills", name, ..] if s[0].starts_with('.') => return Some((*name).into()),
        ["Library", "Application Support", app, ..] => return Some(app.to_lowercase()),
        [".local" | ".cargo" | ".bun", "bin", x] if !INTERPRETERS.contains(x) => return Some((*x).into()),
        [".config", x, ..] if !PLAIN_DIRS.contains(x) => return Some((*x).into()),
        _ => {}
    }
    if let Some(first) = s.first().and_then(|f| f.strip_prefix('.')) {
        if !PLAIN_DIRS.contains(&first) {
            return Some(first.into());
        }
        // A script inside an agent's hooks folder: `crg-update.sh` → `crg`.
        if s.len() >= 3 && s[1] == "hooks" {
            let stem = s[s.len() - 1].split('.').next().unwrap_or("");
            if let Some((prefix, _)) = stem.split_once('-') {
                return Some(prefix.into());
            }
        }
    }
    None
}

fn label(command: &str, cmds: &[Vec<String>], vars: &Vars) -> String {
    const MAX: usize = 60;
    let one_line = !command.contains('\n');
    // Simple commands read best as themselves, with paths cut to file names.
    if one_line && cmds.len() == 1 && !cmds[0].iter().any(|w| w.contains(char::is_whitespace)) {
        let words: Vec<String> = cmds[0]
            .iter()
            .filter(|w| !is_assignment(w))
            .enumerate()
            .filter(|(i, w)| !(*i == 0 && INTERPRETERS.contains(&basename(w).as_str())))
            .map(|(_, w)| if w.contains('/') { basename(w) } else { w.clone() })
            .collect();
        let s = words.join(" ");
        if !s.is_empty() && s.len() <= MAX {
            return s;
        }
    }
    // Long scripts: the first script they run.
    let mut cands = Vec::new();
    let mut guarded = HashSet::new();
    let mut words = Vec::new();
    let mut bare = None;
    collect(cmds, vars, &mut cands, &mut guarded, &mut words, &mut bare, 0);
    let scripts: Vec<&PathBuf> = cands
        .iter()
        .map(|(p, _)| p)
        .filter(|p| !INTERPRETERS.contains(&basename(&p.to_string_lossy()).as_str()))
        .collect();
    if let Some(p) = scripts.iter().find(|p| p.exists()).or(scripts.first()) {
        return basename(&p.to_string_lossy());
    }
    if let Some(b) = bare {
        return b.replace('\u{1f}', " ");
    }
    let flat = command.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= MAX {
        flat
    } else {
        format!("{}…", flat.chars().take(MAX - 1).collect::<String>())
    }
}

/// Splits a shell command into simple commands of unquoted words. Handles
/// quotes and backslashes; `; | & ( ) newline` end a command. Good enough to
/// find paths, not a shell.
pub fn split_commands(src: &str) -> Vec<Vec<String>> {
    let mut cmds: Vec<Vec<String>> = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = src.chars().peekable();
    let end_word = |word: &mut String, in_word: &mut bool, cur: &mut Vec<String>| {
        if *in_word {
            cur.push(std::mem::take(word));
            *in_word = false;
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                for d in chars.by_ref() {
                    if d == '\'' {
                        break;
                    }
                    word.push(d);
                }
            }
            '"' => {
                in_word = true;
                while let Some(d) = chars.next() {
                    match d {
                        '"' => break,
                        '\\' => {
                            if let Some(e) = chars.next() {
                                if !matches!(e, '"' | '\\' | '$' | '`') {
                                    word.push('\\');
                                }
                                word.push(e);
                            }
                        }
                        _ => word.push(d),
                    }
                }
            }
            '\\' => {
                in_word = true;
                if let Some(e) = chars.next() {
                    word.push(e);
                }
            }
            ' ' | '\t' => end_word(&mut word, &mut in_word, &mut cur),
            ';' | '|' | '&' | '(' | ')' | '\n' | '`' => {
                end_word(&mut word, &mut in_word, &mut cur);
                if !cur.is_empty() {
                    cmds.push(std::mem::take(&mut cur));
                }
            }
            _ => {
                in_word = true;
                word.push(c);
            }
        }
    }
    end_word(&mut word, &mut in_word, &mut cur);
    if !cur.is_empty() {
        cmds.push(cur);
    }
    cmds
}
