//! Where things live. One override pair for tests and the CLI (Gate 2: External).

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Env {
    /// The user's home, where agents keep their files.
    pub home: PathBuf,
    /// Platform config dir (`~/Library/Application Support` on macOS).
    pub config_dir: Option<PathBuf>,
    /// SkillDesk's own data: database, backups (`…/Application Support/SkillDesk`).
    pub data_dir: PathBuf,
}

impl Env {
    /// Reads `SKILLDESK_HOME` and `SKILLDESK_DATA_DIR` when set.
    pub fn from_os() -> Self {
        let home = std::env::var_os("SKILLDESK_HOME")
            .map(PathBuf::from)
            .or_else(dirs::home_dir)
            .expect("cannot determine home directory");
        let data_dir = std::env::var_os("SKILLDESK_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                dirs::data_dir()
                    .unwrap_or_else(|| home.join("Library/Application Support"))
                    .join("SkillDesk")
            });
        Self {
            config_dir: dirs::config_dir(),
            home,
            data_dir,
        }
    }

    pub fn for_test(home: &Path, data_dir: &Path) -> Self {
        Self {
            home: home.to_path_buf(),
            config_dir: None,
            data_dir: data_dir.to_path_buf(),
        }
    }

    /// Scratch copies for installs not yet applied (inside the data dir; never
    /// read by an agent).
    pub fn staging_dir(&self) -> PathBuf {
        self.data_dir.join("staging")
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("skilldesk.db")
    }

    /// `~/<rel>` first; for `.config/…` also the platform config dir. First that
    /// exists wins, else the first.
    pub fn resolve(&self, rel: &str) -> PathBuf {
        let mut candidates = vec![self.home.join(rel)];
        if let (Some(suffix), Some(cfg)) = (rel.strip_prefix(".config/"), &self.config_dir) {
            let p = cfg.join(suffix);
            if !candidates.contains(&p) {
                candidates.push(p);
            }
        }
        candidates
            .iter()
            .find(|p| p.exists())
            .cloned()
            .unwrap_or_else(|| candidates.swap_remove(0))
    }
}
