//! Settings (DESIGN.md §7.19): which agents SkillDesk reads, custom agents,
//! the library folder, and the `skilldesk` command in `~/.skilldesk/bin`.
//! These are SkillDesk's own settings; no agent file changes here.

use crate::adapters::{self, CustomAgent};
use crate::core::Core;
use crate::error::{AppError, ErrorKind};
use crate::model::{AgentKey, AgentSetting, AgentTier, CliState, CliStatus, ItemKind, ItemStatus, KindFacts, SettingsData};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};

const CUSTOM_KEY: &str = "agents.custom";
const OFF_KEY: &str = "agents.off";

fn invalid(msg: impl Into<String>) -> AppError {
    AppError::new(ErrorKind::InvalidInput, msg)
}

fn json_err(e: serde_json::Error) -> AppError {
    AppError::internal(e.to_string())
}

/// Reads custom agents and the off list from the database into the registry.
pub(crate) fn load(core: &Core) -> Result<(), AppError> {
    adapters::configure(&custom(core)?, &off(core)?);
    Ok(())
}

fn custom(core: &Core) -> Result<Vec<CustomAgent>, AppError> {
    Ok(core.store.setting(CUSTOM_KEY)?.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default())
}

fn off(core: &Core) -> Result<Vec<String>, AppError> {
    Ok(core.store.setting(OFF_KEY)?.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default())
}

fn slug(name: &str) -> String {
    let mut s = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
    }
    s.trim_end_matches('-').to_string()
}

/// A project skills folder must stay inside the repo.
fn check_project_dir(rel: &str) -> Result<(), AppError> {
    let p = Path::new(rel);
    if rel.is_empty() || p.is_absolute() || p.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(invalid("The project skills folder must be a path inside the repo, like .myagent/skills."));
    }
    Ok(())
}

impl Core {
    pub fn settings(&self) -> Result<SettingsData, AppError> {
        let found: Vec<&str> = adapters::installed(&self.env).iter().map(|a| a.key).collect();
        let mut skills: std::collections::HashMap<String, u32> = Default::default();
        let mut library_skills = 0;
        for item in self.store.items(ItemKind::Skill)? {
            if matches!(item.status, ItemStatus::Gone | ItemStatus::NotAnItem) {
                continue;
            }
            if matches!(&item.facts, KindFacts::Skill(f) if f.library.is_some()) {
                library_skills += 1;
            }
            for a in &item.agents {
                *skills.entry(a.0.clone()).or_default() += 1;
            }
        }
        let custom = custom(self)?;
        let mut agents: Vec<AgentSetting> = adapters::registry()
            .iter()
            .filter(|a| a.tier != AgentTier::Shared)
            .map(|a| {
                let dirs = a.skill_dirs(&self.env);
                AgentSetting {
                    agent: a.agent_key(),
                    name: a.display_name.to_string(),
                    tier: a.tier,
                    found: found.contains(&a.key),
                    on: !adapters::is_off(a.key),
                    custom: custom.iter().any(|c| c.key == a.key),
                    skills_dir: dirs[0].clone(),
                    also_reads: dirs[1..].to_vec(),
                    skills: skills.get(a.key).copied().unwrap_or(0),
                }
            })
            .collect();
        agents.sort_by(|a, b| b.found.cmp(&a.found).then(b.custom.cmp(&a.custom)).then(a.tier.cmp(&b.tier)).then(b.skills.cmp(&a.skills)).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        Ok(SettingsData { agents, custom, library_dir: crate::library::library_dir(&self.env), library_skills })
    }

    /// The read switch. Takes effect with the next scan.
    pub fn agent_set_on(&self, agent: &AgentKey, on: bool) -> Result<(), AppError> {
        if adapters::find(&agent.0).is_none() {
            return Err(invalid("No such agent."));
        }
        let mut list = off(self)?;
        list.retain(|k| k != &agent.0);
        if !on {
            list.push(agent.0.clone());
        }
        self.store.set_setting(OFF_KEY, &serde_json::to_string(&list).map_err(json_err)?)?;
        if agent.0 == "cursor" {
            // Read Cursor's chat database again when it is switched back on.
            self.store.set_setting("sessions.cursor_stamp", "")?;
        }
        load(self)
    }

    /// Adds a custom agent, or changes the one with the same key.
    pub fn custom_agent_save(&self, key: Option<String>, name: &str, skills_dir: &Path, project_dir: Option<&str>) -> Result<CustomAgent, AppError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 40 {
            return Err(invalid("The name must have 1 to 40 characters."));
        }
        let base = slug(name);
        if base.is_empty() {
            return Err(invalid("The name must have a letter or a digit."));
        }
        if !skills_dir.is_absolute() || !skills_dir.is_dir() {
            return Err(invalid("Choose a folder that exists."));
        }
        let dir = skills_dir.canonicalize().map_err(|e| invalid(format!("Cannot read {}: {e}", skills_dir.display())))?;
        let home = self.env.home.canonicalize().unwrap_or_else(|_| self.env.home.clone());
        if dir == home || dir.parent().is_none() {
            return Err(invalid("Choose the agent's skills folder, not the home folder."));
        }
        let lib = crate::library::library_dir(&self.env);
        if crate::library::overlaps(&dir, &lib) || crate::library::canonical_library_dir(&self.env).is_some_and(|l| crate::library::overlaps(&dir, &l)) {
            return Err(invalid("That folder is the SkillDesk library or inside it."));
        }
        let project_dir = project_dir.map(str::trim).filter(|s| !s.is_empty()).map(|s| s.trim_end_matches('/').to_string());
        if let Some(p) = &project_dir {
            check_project_dir(p)?;
        }
        let mut list = custom(self)?;
        let key = match key {
            Some(k) if list.iter().any(|c| c.key == k) => k,
            Some(_) => return Err(invalid("No such custom agent.")),
            None => {
                let k = format!("custom-{base}");
                if adapters::registry().iter().any(|a| a.key == k) {
                    return Err(invalid(format!("An agent named {name} is already in the list.")));
                }
                k
            }
        };
        let agent = CustomAgent { key: key.clone(), name: name.to_string(), skills_dir: dir, project_dir };
        match list.iter_mut().find(|c| c.key == key) {
            Some(c) => *c = agent.clone(),
            None => list.push(agent.clone()),
        }
        self.store.set_setting(CUSTOM_KEY, &serde_json::to_string(&list).map_err(json_err)?)?;
        load(self)?;
        Ok(agent)
    }

    /// Removes a custom agent from the list. Its folder and skills stay.
    pub fn custom_agent_delete(&self, key: &str) -> Result<(), AppError> {
        let mut list = custom(self)?;
        let before = list.len();
        list.retain(|c| c.key != key);
        if list.len() == before {
            return Err(invalid("No such custom agent."));
        }
        self.store.set_setting(CUSTOM_KEY, &serde_json::to_string(&list).map_err(json_err)?)?;
        let mut o = off(self)?;
        o.retain(|k| k != key);
        self.store.set_setting(OFF_KEY, &serde_json::to_string(&o).map_err(json_err)?)?;
        load(self)
    }
}

// ---- The `skilldesk` command -------------------------------------------------

const STAMP: &str = ".skilldesk-cli.json";

fn bin_dir(core: &Core) -> PathBuf {
    core.env.skilldesk_dir().join("bin")
}

fn sha(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(hex::encode(Sha256::digest(&bytes)))
}

fn stamp_hash(dir: &Path) -> Option<String> {
    let t = std::fs::read_to_string(dir.join(STAMP)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&t).ok()?;
    v.get("sha256")?.as_str().map(str::to_string)
}

/// Shell start files that already put `~/.skilldesk/bin` on PATH (read only).
fn on_path(core: &Core) -> bool {
    [".zshrc", ".zprofile", ".zshenv", ".bash_profile", ".bashrc", ".profile", ".config/fish/config.fish"]
        .iter()
        .filter_map(|f| std::fs::read_to_string(core.env.home.join(f)).ok())
        .any(|t| t.lines().any(|l| !l.trim_start().starts_with('#') && l.contains(".skilldesk/bin")))
}

impl Core {
    /// `source` = the command this app carries (`None` in a build without it).
    pub fn cli_status(&self, source: Option<&Path>) -> CliStatus {
        let dir = bin_dir(self);
        let path = dir.join("skilldesk");
        let have = sha(&path);
        let new = source.and_then(sha);
        let state = match (&have, stamp_hash(&dir)) {
            (None, _) => CliState::NotInstalled,
            (Some(h), Some(s)) if *h == s => {
                if new.as_ref().is_some_and(|n| n != h) {
                    CliState::Outdated
                } else {
                    CliState::Installed
                }
            }
            _ => CliState::Other,
        };
        CliStatus {
            state,
            path,
            bundled: new.is_some(),
            on_path: on_path(self),
            path_line: r#"export PATH="$HOME/.skilldesk/bin:$PATH""#.to_string(),
        }
    }

    /// Copies the command to `~/.skilldesk/bin/skilldesk` and stamps it. A file
    /// there that SkillDesk did not put there goes to the Trash first.
    pub fn cli_install(&self, source: &Path) -> Result<CliStatus, AppError> {
        let io = |what: &str, p: &Path, e: std::io::Error| AppError::new(ErrorKind::Io, format!("Cannot {what} {}: {e}", p.display()));
        let hash = sha(source).ok_or_else(|| AppError::new(ErrorKind::NotFound, "This build of SkillDesk has no skilldesk command."))?;
        let _w = self.write_guard()?;
        let dir = bin_dir(self);
        std::fs::create_dir_all(&dir).map_err(|e| io("create", &dir, e))?;
        let dest = dir.join("skilldesk");
        if self.cli_status(Some(source)).state == CliState::Other {
            let to = crate::actions::trash_target(self, &dest);
            std::fs::rename(&dest, &to).map_err(|e| io("move to the Trash", &dest, e))?;
        }
        let tmp = dir.join(".skilldesk.skilldesk-tmp");
        std::fs::copy(source, &tmp).map_err(|e| io("copy", source, e))?;
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).map_err(|e| io("set permissions on", &tmp, e))?;
        }
        if sha(&tmp).as_deref() != Some(hash.as_str()) {
            let _ = std::fs::remove_file(&tmp);
            return Err(AppError::new(ErrorKind::Io, "The copy did not match. Nothing was installed."));
        }
        std::fs::rename(&tmp, &dest).map_err(|e| io("install", &dest, e))?;
        let stamp = serde_json::json!({ "version": env!("CARGO_PKG_VERSION"), "sha256": hash, "installed_at": chrono::Utc::now().to_rfc3339() });
        std::fs::write(dir.join(STAMP), stamp.to_string()).map_err(|e| io("write", &dir.join(STAMP), e))?;
        Ok(self.cli_status(Some(source)))
    }
}
