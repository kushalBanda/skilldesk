//! Marketplace sources seen in Claude transcripts. A removed plugin often
//! outlives its marketplace; `/plugin install` then fails, and the source in
//! `known_marketplaces.json` is gone with it. Transcripts keep two kinds of
//! evidence:
//!
//! - A copy of `known_marketplaces.json` (a tool read it, or it was pasted):
//!   `"<name>": {"source": {"source": "github", "repo": "owner/repo"}}`. Exact.
//! - A typed or run `marketplace add <source>` (`/plugin …`, `claude plugin …`).
//!   The name is the source's last path part, as Claude names most marketplaces.
//!
//! Lines arrive as raw JSONL, so the text is unescaped first (one or more
//! levels of `\"`, `\n`). Placeholders (`<owner/repo>`) never pass the checks.

use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

/// (marketplace name, source) → last seen (ms since epoch).
pub type MarketSources = HashMap<(String, String), Option<i64>>;

/// Cheap test before the regexes run.
pub fn mentions(line: &[u8]) -> bool {
    static F: OnceLock<memchr::memmem::Finder<'static>> = OnceLock::new();
    F.get_or_init(|| memchr::memmem::Finder::new(b"marketplace")).find(line).is_some()
}

fn known_copy() -> &'static [Regex; 2] {
    static R: OnceLock<[Regex; 2]> = OnceLock::new();
    R.get_or_init(|| {
        [
            // "name": { "source": { "source": "github", "repo": "x/y" } }
            Regex::new(
                r#""([A-Za-z0-9._-]+)"\s*:\s*\{\s*"source"\s*:\s*\{\s*"source"\s*:\s*"(?:github|git|url|directory|file)"\s*,\s*"(?:repo|url|path)"\s*:\s*"([^"]+)""#,
            )
            .expect("regex"),
            // Same, with the kind after the location.
            Regex::new(
                r#""([A-Za-z0-9._-]+)"\s*:\s*\{\s*"source"\s*:\s*\{\s*"(?:repo|url|path)"\s*:\s*"([^"]+)"\s*,\s*"source"\s*:\s*"(?:github|git|url|directory|file)""#,
            )
            .expect("regex"),
        ]
    })
}

fn add_command() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?:<command-args>\s*|plugin\s+)marketplace\s+add\s+([^\s\x22'<>`]+)").expect("regex"))
}

fn unescape(text: &str) -> String {
    text.replace("\\n", " ").replace("\\t", " ").replace('\\', "")
}

/// `owner/repo`, a git or web URL, or an absolute or home path.
pub fn valid_source(s: &str) -> bool {
    static REPO: OnceLock<Regex> = OnceLock::new();
    let repo = REPO.get_or_init(|| Regex::new(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$").expect("regex"));
    if s.len() > 300 || s.contains(['<', '>', '{', '}', ' ']) {
        return false;
    }
    repo.is_match(s)
        || s.starts_with("https://")
        || s.starts_with("http://")
        || s.starts_with("git@")
        || s.starts_with("ssh://")
        || s.starts_with('/')
        || s.starts_with("~/")
}

/// The name Claude gives a marketplace added from `source` when its
/// `marketplace.json` agrees with the repo name (the usual case).
pub fn name_from_source(source: &str) -> Option<String> {
    let mut s = source.trim_end_matches('/');
    if let Some(dir) = s.strip_suffix("marketplace.json") {
        s = dir.trim_end_matches('/');
        s = s.strip_suffix(".claude-plugin").unwrap_or(s).trim_end_matches('/');
    }
    let s = s.strip_suffix(".git").unwrap_or(s);
    let last = s.rsplit(['/', ':']).next()?;
    (!last.is_empty() && last.len() <= 120).then(|| last.to_string())
}

/// Adds what one transcript line shows.
pub fn scan_line(line: &[u8], at: Option<i64>, out: &mut MarketSources) {
    let Ok(raw) = std::str::from_utf8(line) else { return };
    let text = unescape(raw);
    let mut put = |name: &str, source: &str| {
        let source = source.trim_end_matches(['.', ',', ';', ':', ')']);
        if !valid_source(source) || name.is_empty() || matches!(name, "source" | "marketplaces" | "plugins") {
            return;
        }
        let e = out.entry((name.to_string(), source.to_string())).or_insert(None);
        *e = (*e).max(at);
    };
    for re in known_copy() {
        for c in re.captures_iter(&text) {
            put(&c[1], &c[2]);
        }
    }
    for c in add_command().captures_iter(&text) {
        let source = c[1].trim_end_matches(['.', ',', ';', ':', ')']);
        if let Some(name) = name_from_source(source) {
            put(&name, source);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(line: &str) -> Vec<(String, String)> {
        let mut out = MarketSources::new();
        scan_line(line.as_bytes(), Some(1), &mut out);
        let mut v: Vec<_> = out.into_keys().collect();
        v.sort();
        v
    }

    #[test]
    fn reads_an_escaped_copy_of_known_marketplaces() {
        let line = r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"{\n  \"ponytail\": {\n    \"source\": {\n      \"source\": \"github\",\n      \"repo\": \"DietrichGebert/ponytail\"\n    }\n  }\n}"}]}}"#;
        assert_eq!(scan(line), vec![("ponytail".into(), "DietrichGebert/ponytail".into())]);
    }

    #[test]
    fn reads_a_typed_marketplace_add() {
        let line = r#"{"type":"user","message":{"content":"<command-name>/plugin</command-name>\n            <command-message>plugin</command-message>\n            <command-args>marketplace add cathrynlavery/diagram-design\n/plugin install diagram-design@diagram-design</command-args>"}}"#;
        assert_eq!(scan(line), vec![("diagram-design".into(), "cathrynlavery/diagram-design".into())]);
    }

    #[test]
    fn reads_a_cli_add_with_a_git_url() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"claude plugin marketplace add https://github.com/acme/tools.git"}}]}}"#;
        assert_eq!(scan(line), vec![("tools".into(), "https://github.com/acme/tools.git".into())]);
    }

    #[test]
    fn skips_placeholders() {
        assert!(scan(r#"{"content":"/plugin marketplace add <owner/repo>"}"#).is_empty());
        assert!(scan(r#"{"content":"/plugin marketplace add owner"}"#).is_empty());
    }

    #[test]
    fn names_from_sources() {
        assert_eq!(name_from_source("a/b").as_deref(), Some("b"));
        assert_eq!(name_from_source("/x/Quant/.claude-plugin/marketplace.json").as_deref(), Some("Quant"));
        assert_eq!(name_from_source("git@github.com:o/r.git").as_deref(), Some("r"));
    }
}
