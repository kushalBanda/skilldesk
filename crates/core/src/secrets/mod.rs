//! Secrets in agent history. Detectors find high-confidence shapes; each value
//! is kept only as a salted hash plus its kind's known prefix (no value
//! is stored or sent to the UI). Every prompt text SkillDesk stores
//! passes through [`mask`] first.
//!
//! A false positive only hides text. Values that look like examples or
//! placeholders are "possible", not "likely".

pub mod scan;

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SecretKind {
    PrivateKey,
    Anthropic,
    OpenAi,
    Stripe,
    Aws,
    GitHub,
    Slack,
    Google,
    Jwt,
    DatabaseUrl,
    Generic,
}

impl SecretKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SecretKind::PrivateKey => "private_key",
            SecretKind::Anthropic => "anthropic",
            SecretKind::OpenAi => "open_ai",
            SecretKind::Stripe => "stripe",
            SecretKind::Aws => "aws",
            SecretKind::GitHub => "git_hub",
            SecretKind::Slack => "slack",
            SecretKind::Google => "google",
            SecretKind::Jwt => "jwt",
            SecretKind::DatabaseUrl => "database_url",
            SecretKind::Generic => "generic",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
    }

    /// Where the value can be rotated (shown on every card).
    pub fn rotate_at(&self) -> &'static str {
        match self {
            SecretKind::PrivateKey => "Make a new key pair and remove the old public key wherever it is trusted.",
            SecretKind::Anthropic => "console.anthropic.com → Settings → API keys",
            SecretKind::OpenAi => "platform.openai.com → API keys",
            SecretKind::Stripe => "dashboard.stripe.com → Developers → API keys",
            SecretKind::Aws => "AWS console → IAM → Users → Security credentials",
            SecretKind::GitHub => "github.com → Settings → Developer settings → Tokens",
            SecretKind::Slack => "api.slack.com/apps → OAuth & Permissions",
            SecretKind::Google => "console.cloud.google.com → APIs & Services → Credentials",
            SecretKind::Jwt => "The service that issued the token (sign out its sessions or change its signing key).",
            SecretKind::DatabaseUrl => "Change the database user's password.",
            SecretKind::Generic => "The service this value belongs to.",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Confidence {
    Likely,
    Possible,
}

/// (kind, pattern). Group `p` = the known prefix that may be shown; group
/// `v` = the part to hide (else the whole match).
const DETECTORS: &[(SecretKind, &str)] = &[
    (SecretKind::PrivateKey, r"(?P<p>-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY-----)[\s\S]{20,}?(?:-----END (?:[A-Z0-9]+ )*PRIVATE KEY-----|$)"),
    (SecretKind::Anthropic, r"(?-u:\b)(?P<p>sk-ant-(?:[a-z]+\d\d-)?)[A-Za-z0-9_\-]{20,}"),
    (SecretKind::OpenAi, r"(?-u:\b)(?P<p>sk-(?:proj-|svcacct-|admin-)?)[A-Za-z0-9_\-]{20,}"),
    (SecretKind::Stripe, r"(?-u:\b)(?P<p>(?:sk|rk)_(?:live|test)_)[A-Za-z0-9]{16,}"),
    (SecretKind::Aws, r"(?-u:\b)(?P<p>AKIA|ASIA)[0-9A-Z]{16}(?-u:\b)"),
    (SecretKind::GitHub, r"(?-u:\b)(?P<p>gh[pousr]_)[A-Za-z0-9]{30,}"),
    (SecretKind::GitHub, r"(?-u:\b)(?P<p>github_pat_)[A-Za-z0-9_]{22,}"),
    (SecretKind::Slack, r"(?-u:\b)(?P<p>xox[abposr]-)[A-Za-z0-9\-]{10,}"),
    (SecretKind::Google, r"(?-u:\b)(?P<p>AIza)[0-9A-Za-z_\-]{35}"),
    (SecretKind::Jwt, r"(?-u:\b)(?P<p>eyJ)[A-Za-z0-9_\-]{8,}\.eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}"),
    (
        SecretKind::DatabaseUrl,
        r#"(?i)(?-u:\b)(?P<p>(?:postgres(?:ql)?|mysql|mariadb|mongodb(?:\+srv)?|rediss?|amqps?)://)[^\s:/@"'\\]+:(?P<v>[^\s@/"'\\]+)@"#,
    ),
    (
        SecretKind::Generic,
        r#"(?i)(?-u:\b)(?:api[_\-]?key|secret[_\-]?key|client[_\-]?secret|access[_\-]?token|auth[_\-]?token|secret|token|password|passwd)(?-u:\b)(?:\\?["'])?\s*[:=]\s*(?:\\?["'])?(?P<v>[A-Za-z0-9_\-/+=.]{16,})"#,
    ),
];

/// Literals one of which must appear for each detector to match (same order).
const ANCHORS: &[&[&str]] = &[
    &["-----BEGIN"],
    &["sk-ant-"],
    &["sk-"],
    &["sk_live_", "sk_test_", "rk_live_", "rk_test_"],
    &["AKIA", "ASIA"],
    &["ghp_", "gho_", "ghu_", "ghs_", "ghr_"],
    &["github_pat_"],
    &["xox"],
    &["AIza"],
    &["eyJ"],
    &["://"],
    &["key", "secret", "token", "passw"],
];

fn compiled() -> &'static [(SecretKind, Regex)] {
    static C: OnceLock<Vec<(SecretKind, Regex)>> = OnceLock::new();
    C.get_or_init(|| DETECTORS.iter().map(|(k, p)| (*k, Regex::new(p).expect("detector compiles"))).collect())
}

/// Which detectors can match `text` (bit i = detector i); 0 = none, so the
/// detectors are not run at all.
fn candidates(text: &str) -> u32 {
    static AC: OnceLock<(aho_corasick::AhoCorasick, Vec<usize>)> = OnceLock::new();
    let (ac, owner) = AC.get_or_init(|| {
        let mut pats = Vec::new();
        let mut owner = Vec::new();
        for (i, list) in ANCHORS.iter().enumerate() {
            for a in *list {
                pats.push(*a);
                owner.push(i);
            }
        }
        let ac = aho_corasick::AhoCorasick::builder().ascii_case_insensitive(true).build(pats).expect("prefilter builds");
        (ac, owner)
    });
    let mut bits = 0u32;
    for m in ac.find_overlapping_iter(text) {
        bits |= 1 << owner[m.pattern().as_usize()];
    }
    bits
}

/// One detected value in a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub kind: SecretKind,
    pub confidence: Confidence,
    /// Byte span of the hidden part.
    pub start: usize,
    pub end: usize,
    /// The known prefix that may be shown ("" = none).
    pub prefix: String,
}

impl Found {
    pub fn value<'a>(&self, text: &'a str) -> &'a str {
        &text[self.start..self.end]
    }

    /// What replaces the hidden part in a file or a stored text.
    pub fn replacement(&self, text: &str) -> String {
        if text[self.start..self.end].starts_with(&self.prefix) && !self.prefix.is_empty() {
            format!("{}{MASK}", self.prefix)
        } else {
            MASK.to_string()
        }
    }

    /// The masked form the UI shows.
    pub fn shown(&self) -> String {
        format!("{}{MASK}", self.prefix)
    }
}

pub const MASK: &str = "••••";

/// Values that read like documentation examples or placeholders.
fn looks_fake(v: &str) -> bool {
    let l = v.to_ascii_lowercase();
    ["example", "xxxx", "your", "dummy", "sample", "placeholder", "redacted", "changeme", "0000000", "1234567", "abcdef"]
        .iter()
        .any(|w| l.contains(w))
        || entropy(v) < 3.0
}

/// Shannon entropy in bits per character.
fn entropy(v: &str) -> f64 {
    let mut counts = std::collections::HashMap::new();
    let n = v.chars().count() as f64;
    for c in v.chars() {
        *counts.entry(c).or_insert(0u32) += 1;
    }
    counts.values().map(|&c| { let p = c as f64 / n; -p * p.log2() }).sum()
}

fn mixed(v: &str) -> bool {
    v.chars().any(|c| c.is_ascii_digit()) && v.chars().any(|c| c.is_ascii_alphabetic())
}

/// Every value in `text`, left to right; overlapping finds keep the first detector's.
pub fn detect(text: &str) -> Vec<Found> {
    let bits = candidates(text);
    if bits == 0 {
        return Vec::new();
    }
    let mut out: Vec<Found> = Vec::new();
    for (i, (kind, re)) in compiled().iter().enumerate() {
        if bits & (1 << i) == 0 {
            continue;
        }
        for caps in re.captures_iter(text) {
            let Some(m) = caps.get(0) else { continue };
            let span = caps.name("v").unwrap_or(m);
            if out.iter().any(|f| f.start < m.end() && m.start() < f.end) {
                continue;
            }
            let prefix = caps.name("p").map(|p| p.as_str().to_string()).unwrap_or_default();
            let value = span.as_str();
            let body = value.strip_prefix(prefix.as_str()).unwrap_or(value);
            if matches!(kind, SecretKind::DatabaseUrl) && (body.starts_with(['$', '<', '{', '%', '*']) || body.contains(MASK)) {
                continue;
            }
            let confidence = match kind {
                SecretKind::Generic => {
                    if !mixed(body) || entropy(body) < 3.5 || looks_fake(body) {
                        continue;
                    }
                    Confidence::Possible
                }
                SecretKind::PrivateKey => Confidence::Likely,
                SecretKind::DatabaseUrl if ["password", "pass", "secret", "postgres", "root", "admin", "user"].contains(&body.to_ascii_lowercase().as_str()) => Confidence::Possible,
                SecretKind::DatabaseUrl => Confidence::Likely,
                _ if looks_fake(body) || !mixed(body) => Confidence::Possible,
                _ => Confidence::Likely,
            };
            out.push(Found {
                kind: *kind,
                confidence,
                start: span.start(),
                end: span.end(),
                prefix: if matches!(kind, SecretKind::DatabaseUrl) || value.starts_with(&prefix) { prefix } else { String::new() },
            });
        }
    }
    out.sort_by_key(|f| f.start);
    out
}

/// `text` with every detected value replaced by its mask.
pub fn mask(text: &str) -> String {
    let found = detect(text);
    if found.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for f in &found {
        out.push_str(&text[last..f.start]);
        out.push_str(&f.replacement(text));
        last = f.end;
    }
    out.push_str(&text[last..]);
    out
}

/// Salted hash of a value: the only form SkillDesk keeps.
pub fn value_hash(salt: &str, value: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(salt.as_bytes());
    h.update([0]);
    h.update(value.as_bytes());
    hex::encode(&h.finalize()[..16])
}

/// One value as the UI sees it: no value, only kind, prefix and places.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Secret {
    /// Salted hash of the value.
    pub id: String,
    pub kind: SecretKind,
    pub confidence: Confidence,
    /// Known prefix + mask, e.g. `sk-ant-••••`.
    pub masked: String,
    pub places: Vec<SecretPlace>,
    /// Places in total (`places` holds at most 200).
    pub place_count: u32,
    pub occurrences: u32,
    /// ms since epoch.
    #[ts(type = "number | null")]
    pub first_seen: Option<i64>,
    #[ts(type = "number | null")]
    pub last_seen: Option<i64>,
    pub ignored: bool,
    pub rotate_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SecretPlace {
    pub path: String,
    /// Byte offset of the first occurrence in a text file.
    #[ts(type = "number | null")]
    pub offset: Option<i64>,
    pub store: PlaceStore,
    /// transcript, file_history, paste_cache, prompt_history, shell_snapshot,
    /// memory, codex, gemini, cursor, cursor_cli, opencode.
    pub place: String,
    /// The chat a database place belongs to.
    pub chat: Option<String>,
    pub count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PlaceStore {
    TextFile,
    /// An agent's database: never written.
    AgentDb,
}

/// A raw find before it is hashed (memory only; never stored or sent).
#[derive(Debug, Clone)]
pub struct Hit {
    pub kind: SecretKind,
    pub confidence: Confidence,
    pub prefix: String,
    pub value: String,
    pub offset: i64,
    pub seen: Option<i64>,
}

/// Hits of one text, with `base` added to each offset.
pub fn hits(text: &str, base: i64, seen: Option<i64>) -> Vec<Hit> {
    detect(text)
        .into_iter()
        .map(|f| Hit {
            kind: f.kind,
            confidence: f.confidence,
            prefix: f.shown().trim_end_matches(MASK).to_string(),
            value: f.value(text).to_string(),
            offset: base + f.start as i64,
            seen,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::mask;

    #[test]
    fn masks_common_secret_shapes() {
        let raw = [
            "sk-ant-api03-AAAAbbbbCCCCddddEEEEffff1234",
            "sk-proj-ZZZZyyyyXXXXwwwwVVVVuuuu9876",
            "AKIAIOSFODNN7EXAMPLE",
            "ghp_0123456789abcdefghijABCDEFGHIJ012345",
            concat!("eyJhbGciOiJIUzI1NiJ9", ".", "eyJzdWIiOiIxMjM0NTY3ODkwIn0", ".", "dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U"),
        ];
        for r in raw {
            let m = mask(&format!("use {r} please"));
            assert!(!m.contains(r), "{r} leaked: {m}");
            assert!(!m.contains(&r[r.len() - 6..]), "tail of {r} shown: {m}");
        }
        let url = mask("DATABASE_URL=postgres://app:hunter2hunter2@db.example.com:5432/x");
        assert!(!url.contains("hunter2hunter2"), "{url}");
        assert!(url.contains("db.example.com"));
        assert_eq!(mask("plain words stay"), "plain words stay");
    }
}
