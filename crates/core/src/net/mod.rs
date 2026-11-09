//! Every network request SkillDesk makes (ADR 0016). Only the Marketplace
//! screen reaches this module: no scan stage, rule, tray item or CLI command
//! calls it. HTTPS only, a fixed host list, redirects checked against it, a
//! fixed User-Agent, no cookies, no tokens. Search text is never logged.

pub mod github;
pub mod market;
pub mod site;
pub mod skillssh;

use crate::error::{AppError, ErrorKind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Hosts any request may reach. A site-published skill adds its own domain
/// for that one install (ADR 0016 "Hosts").
pub const HOSTS: &[&str] = &["skills.sh", "www.skills.sh", "codeload.github.com", "api.github.com", "raw.githubusercontent.com", "github.com", "avatars.githubusercontent.com"];

const TIMEOUT: Duration = Duration::from_secs(30);

static REQUESTS: AtomicU64 = AtomicU64::new(0);

/// Requests sent since the process started (for the no-request-at-scan test).
pub fn requests_made() -> u64 {
    REQUESTS.load(Ordering::Relaxed)
}

pub(crate) fn network(msg: impl Into<String>) -> AppError {
    AppError::new(ErrorKind::Network, msg)
}

fn host_allowed(url: &reqwest::Url, hosts: &[String]) -> bool {
    url.scheme() == "https" && url.host_str().is_some_and(|h| hosts.iter().any(|x| x == h))
}

pub(crate) struct Http {
    client: reqwest::Client,
    hosts: Vec<String>,
}

impl Http {
    /// `extra`: one more host for this client (a site-published skill's domain).
    pub fn new(extra: Option<&str>) -> Result<Self, AppError> {
        let mut hosts: Vec<String> = HOSTS.iter().map(|h| h.to_string()).collect();
        if let Some(h) = extra {
            hosts.push(h.to_ascii_lowercase());
        }
        let allowed = hosts.clone();
        let client = reqwest::Client::builder()
            .user_agent(concat!("SkillDesk/", env!("CARGO_PKG_VERSION")))
            .https_only(true)
            .timeout(TIMEOUT)
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() >= 5 {
                    attempt.error("too many redirects")
                } else if host_allowed(attempt.url(), &allowed) {
                    attempt.follow()
                } else {
                    attempt.error("redirect left the allowed hosts")
                }
            }))
            .build()
            .map_err(|e| AppError::internal(format!("Cannot set up the network client: {e}")))?;
        Ok(Self { client, hosts })
    }

    /// GET with a byte limit. `accept` sets the Accept header.
    pub async fn get(&self, url: &str, limit: u64, accept: Option<&str>) -> Result<Vec<u8>, AppError> {
        let parsed = reqwest::Url::parse(url).map_err(|_| AppError::internal(format!("Bad address {url}")))?;
        let host = parsed.host_str().unwrap_or("").to_string();
        if !host_allowed(&parsed, &self.hosts) {
            return Err(network(format!("SkillDesk does not connect to {host}.")));
        }
        REQUESTS.fetch_add(1, Ordering::Relaxed);
        let mut req = self.client.get(parsed);
        if let Some(a) = accept {
            req = req.header("Accept", a);
        }
        let mut resp = req.send().await.map_err(|e| describe(&host, &e))?;
        let status = resp.status();
        if !status.is_success() {
            let msg = match status.as_u16() {
                403 | 429 if host == "api.github.com" => "GitHub's hourly limit for requests without an account is used up. Try again later.".to_string(),
                404 => format!("{host} has no such page (404)."),
                n => format!("{host} answered {n}."),
            };
            return Err(network(msg));
        }
        if resp.content_length().is_some_and(|n| n > limit) {
            return Err(AppError::new(ErrorKind::InvalidInput, too_big(limit)));
        }
        let mut out = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(|e| describe(&host, &e))? {
            out.extend_from_slice(&chunk);
            if out.len() as u64 > limit {
                return Err(AppError::new(ErrorKind::InvalidInput, too_big(limit)));
            }
        }
        Ok(out)
    }

    pub async fn json(&self, url: &str, limit: u64) -> Result<serde_json::Value, AppError> {
        let bytes = self.get(url, limit, Some("application/json")).await?;
        let host = reqwest::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
        serde_json::from_slice(&bytes).map_err(|_| AppError::new(ErrorKind::Parse, format!("{host} sent something SkillDesk could not read.")))
    }
}

fn too_big(limit: u64) -> String {
    format!("The download is larger than {} MB, so SkillDesk stopped.", limit / (1024 * 1024))
}

fn describe(host: &str, e: &reqwest::Error) -> AppError {
    if e.is_timeout() {
        network(format!("{host} did not answer within 30 seconds."))
    } else if e.is_redirect() {
        network(format!("{host} sent SkillDesk somewhere it does not connect to."))
    } else if e.is_connect() {
        network(format!("No connection to {host}."))
    } else {
        network(format!("The connection to {host} failed."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn net_only_https_on_listed_hosts() {
        let hosts: Vec<String> = HOSTS.iter().map(|h| h.to_string()).collect();
        let ok = |u: &str| host_allowed(&reqwest::Url::parse(u).unwrap(), &hosts);
        assert!(ok("https://www.skills.sh/api/search?q=x"));
        assert!(ok("https://codeload.github.com/a/b/tar.gz/abc"));
        assert!(!ok("http://skills.sh/"), "plain HTTP");
        assert!(!ok("https://evil.example/"), "other host");
        assert!(!ok("https://skills.sh.evil.example/"), "suffix trick");
    }
}
