//! skills.sh lists: leaderboards (parsed from the page payload) and search
//! (JSON API). Parsers handle three page formats; skills.sh changed its format
//! twice.

use super::Http;
use crate::error::{AppError, ErrorKind};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use ts_rs::TS;

const BASE: &str = "https://www.skills.sh";
const PAGE_LIMIT: u64 = 20 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MarketSkill {
    /// `<source>/<skill_id>`.
    pub id: String,
    pub skill_id: String,
    pub name: String,
    /// `owner/repo` on GitHub, or a domain for a site-published skill.
    pub source: String,
    #[ts(type = "number")]
    pub installs: u64,
}

impl MarketSkill {
    pub fn is_site(&self) -> bool {
        !self.source.contains('/')
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Board {
    AllTime,
    Trending,
    Hot,
}

impl Board {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AllTime => "all_time",
            Self::Trending => "trending",
            Self::Hot => "hot",
        }
    }

    fn url(self) -> String {
        match self {
            Self::AllTime => format!("{BASE}/"),
            Self::Trending => format!("{BASE}/trending"),
            Self::Hot => format!("{BASE}/hot"),
        }
    }
}

pub(crate) async fn leaderboard(http: &Http, board: Board) -> Result<Vec<MarketSkill>, AppError> {
    let bytes = http.get(&board.url(), PAGE_LIMIT, Some("text/html")).await?;
    let skills = parse_leaderboard_html(&String::from_utf8_lossy(&bytes));
    if skills.is_empty() {
        return Err(AppError::new(ErrorKind::Parse, "skills.sh changed its page, so SkillDesk could not read the list. Search still works."));
    }
    Ok(skills)
}

pub(crate) async fn search(http: &Http, query: &str, limit: usize) -> Result<Vec<MarketSkill>, AppError> {
    let mut url = reqwest::Url::parse(&format!("{BASE}/api/search")).map_err(|e| AppError::internal(e.to_string()))?;
    url.query_pairs_mut().append_pair("q", query).append_pair("limit", &limit.clamp(1, 300).to_string());
    let v = http.json(url.as_str(), PAGE_LIMIT).await?;
    let arr = v.as_array().or_else(|| v.get("skills").and_then(|s| s.as_array()));
    Ok(arr.map(|a| parse_skills_array(a)).unwrap_or_default())
}

pub fn parse_leaderboard_html(html: &str) -> Vec<MarketSkill> {
    let next = parse_next_data(html);
    if !next.is_empty() {
        return next;
    }
    parse_embedded_skill_objects(html)
}

fn parse_next_data(html: &str) -> Vec<MarketSkill> {
    let marker = r#"<script id="__NEXT_DATA__" type="application/json">"#;
    let Some(start) = html.find(marker).map(|i| i + marker.len()) else { return Vec::new() };
    let Some(end) = html[start..].find("</script>").map(|i| i + start) else { return Vec::new() };
    let Ok(data) = serde_json::from_str::<serde_json::Value>(&html[start..end]) else { return Vec::new() };
    data.pointer("/props/pageProps/initialSkills")
        .or_else(|| data.pointer("/props/pageProps/skills"))
        .or_else(|| data.pointer("/props/pageProps/items"))
        .and_then(|v| v.as_array())
        .map(|a| parse_skills_array(a))
        .unwrap_or_default()
}

fn parse_skills_array(arr: &[serde_json::Value]) -> Vec<MarketSkill> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for item in arr {
        let s = |k: &str| item.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let source = s("source");
        let skill_id = item
            .get("skillId")
            .or_else(|| item.get("skill_id"))
            .or_else(|| item.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if source.is_empty() || skill_id.is_empty() {
            continue;
        }
        let id = format!("{source}/{skill_id}");
        if !seen.insert(id.clone()) {
            continue;
        }
        let name = Some(s("name")).filter(|n| !n.is_empty()).unwrap_or_else(|| skill_id.clone());
        let installs = item.get("installs").and_then(|v| v.as_u64()).unwrap_or(0);
        out.push(MarketSkill { id, skill_id, name, source, installs });
    }
    out
}

fn parse_embedded_skill_objects(html: &str) -> Vec<MarketSkill> {
    let current = regex_lite::Regex::new(
        r#"(?:\\)?"source(?:\\)?":(?:\\)?"(?P<source>[^"\\]+)(?:\\)?",(?:[^{}]|\\.)*?(?:(?:\\)?"skillId(?:\\)?"|(?:\\)?"skill_id(?:\\)?"):(?:\\)?"(?P<skill_id>[^"\\]+)(?:\\)?",(?:[^{}]|\\.)*?(?:\\)?"name(?:\\)?":(?:\\)?"(?P<name>[^"\\]*)(?:\\)?",(?:[^{}]|\\.)*?(?:\\)?"installs(?:\\)?":(?P<installs>\d+)"#,
    )
    .expect("valid regex");
    let legacy = regex_lite::Regex::new(
        r#"\{"source":"(?P<source>[^"]+)","skill_id":"(?P<skill_id>[^"]+)"(?:,"name":"(?P<name>[^"]*)")?(?:.*?"installs":(?P<installs>\d+))?\}"#,
    )
    .expect("valid regex");
    let found = with_regex(html, &current);
    if found.is_empty() {
        with_regex(html, &legacy)
    } else {
        found
    }
}

fn with_regex(html: &str, re: &regex_lite::Regex) -> Vec<MarketSkill> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for c in re.captures_iter(html) {
        let (Some(source), Some(skill_id)) = (c.name("source"), c.name("skill_id")) else { continue };
        let (source, skill_id) = (source.as_str().to_string(), skill_id.as_str().to_string());
        let id = format!("{source}/{skill_id}");
        if !seen.insert(id.clone()) {
            continue;
        }
        let name = c.name("name").map(|m| m.as_str().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| skill_id.clone());
        let installs = c.name("installs").and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
        out.push(MarketSkill { id, skill_id, name, source, installs });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixtures for the three page formats skills.sh has used, plus
    /// the current page (read 2026-11-08, trimmed).
    #[test]
    fn skillssh_parses_every_page_format() {
        let next = r#"<html><script id="__NEXT_DATA__" type="application/json">
            {"props":{"pageProps":{"initialSkills":[{"source":"antfu/skills","skillId":"vite","name":"vite","installs":152}]}}}
          </script></html>"#;
        assert_eq!(parse_leaderboard_html(next)[0].id, "antfu/skills/vite");

        let rsc = r#"<script>self.__next_f.push([1,"...\n[{\"source\":\"anthropics/skills\",\"skillId\":\"template-skill\",\"name\":\"template-skill\",\"installs\":238},{\"source\":\"vercel/ai\",\"skillId\":\"ai-sdk\",\"name\":\"ai-sdk\",\"installs\":265}]...\n"])</script>"#;
        let r = parse_leaderboard_html(rsc);
        assert_eq!((r.len(), r[1].id.as_str()), (2, "vercel/ai/ai-sdk"));

        let legacy = r#"{"source":"openai/skills","skill_id":"playwright","name":"playwright","installs":2}"#;
        assert_eq!(parse_leaderboard_html(legacy)[0].id, "openai/skills/playwright");

        let now = r#"<script>self.__next_f.push([1,"56:[\"$\",\"$L64\",null,{\"initialSkills\":[{\"source\":\"vercel-labs/skills\",\"skillId\":\"find-skills\",\"name\":\"find-skills\",\"installs\":4259616,\"weeklyInstalls\":[101317,105144],\"isOfficial\":true},{\"source\":\"open.feishu.cn\",\"skillId\":\"lark-doc\",\"name\":\"lark-doc\",\"installs\":91}]}]"])</script>"#;
        let n = parse_leaderboard_html(now);
        assert_eq!(n.len(), 2);
        assert_eq!((n[0].name.as_str(), n[0].installs), ("find-skills", 4_259_616));
        assert!(n[1].is_site());
    }
}
