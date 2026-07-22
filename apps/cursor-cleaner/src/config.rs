use std::{fs, path::PathBuf};

use serde::Deserialize;

use crate::{error::AppError, platform};

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub search_db: PathBuf,
    pub state_db: PathBuf,
    pub workspace_storage: PathBuf,
    pub projects_root: PathBuf,
    pub recovery_root: PathBuf,
    pub max_preview_chars: usize,
}

impl Default for Config {
    fn default() -> Self {
        let home = platform::default_home();
        let user = platform::default_cursor_user(&home);
        Self {
            search_db: user.join("globalStorage/conversation-search.db"),
            state_db: user.join("globalStorage/state.vscdb"),
            workspace_storage: user.join("workspaceStorage"),
            projects_root: home.join(".cursor/projects"),
            recovery_root: std::env::temp_dir().join("cursor-cleaner-recovery"),
            max_preview_chars: 800,
        }
    }
}

impl Config {
    pub fn load(path: Option<&PathBuf>) -> Result<Self, AppError> {
        let config = match path {
            Some(path) => {
                let raw = fs::read_to_string(path).map_err(AppError::ConfigRead)?;
                toml::from_str(&raw).map_err(AppError::ConfigParse)?
            }
            None => Self::default(),
        };
        if !platform::HostPlatform::current().supported() {
            return Err(AppError::InvalidConfig(
                "cursor-cleaner 目前仅支持 Windows 和 macOS".into(),
            ));
        }
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), AppError> {
        for (label, path) in [
            ("search_db", &self.search_db),
            ("state_db", &self.state_db),
            ("workspace_storage", &self.workspace_storage),
            ("projects_root", &self.projects_root),
            ("recovery_root", &self.recovery_root),
        ] {
            if !path.is_absolute() {
                return Err(AppError::InvalidConfig(format!(
                    "{label} 必须是绝对路径：{}",
                    path.display()
                )));
            }
        }
        if self.max_preview_chars == 0 || self.max_preview_chars > 20_000 {
            return Err(AppError::InvalidConfig(
                "max_preview_chars 必须介于 1 和 20000".into(),
            ));
        }
        Ok(())
    }
}
