use crate::util::{wsl_file_exists, wsl_home, wsl_path_exists};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConfigFormat {
    Opencode,
    PiAgent,
}

impl ConfigFormat {
    pub fn label(&self) -> &str {
        match self {
            ConfigFormat::Opencode => "opencode",
            ConfigFormat::PiAgent => "pi-agent",
        }
    }
}

pub struct ConfigPaths {
    pub opencode: String,
    pub pi_agent: PathBuf,
}

impl Default for ConfigPaths {
    fn default() -> Self {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_default();
        let opencode = format!("{}\\.config\\opencode\\opencode.json", home);
        let pi_agent = PathBuf::from(format!("{}\\.pi\\agent\\models.json", home));
        Self { opencode, pi_agent }
    }
}

impl ConfigPaths {
    pub fn wsl_paths() -> Option<(String, String)> {
        let home = wsl_home()?;
        let opencode = format!("{}/.config/opencode/opencode.json", home);
        let pi_agent = format!("{}/.pi/agent/models.json", home);
        Some((opencode, pi_agent))
    }

    pub fn wsl_target(&self, format: ConfigFormat) -> Option<String> {
        let (oc, pi) = ConfigPaths::wsl_paths()?;
        match format {
            ConfigFormat::Opencode => {
                if wsl_file_exists(&oc) {
                    Some(oc)
                } else {
                    None
                }
            }
            ConfigFormat::PiAgent => {
                if wsl_path_exists(&pi) {
                    Some(pi)
                } else {
                    None
                }
            }
        }
    }

    pub fn detect() -> Option<(ConfigFormat, String)> {
        let paths = ConfigPaths::default();
        if Path::new(&paths.opencode).exists() {
            return Some((ConfigFormat::Opencode, paths.opencode));
        }
        if paths.pi_agent.exists() {
            return Some((ConfigFormat::PiAgent, paths.pi_agent.to_string_lossy().into_owned()));
        }
        None
    }

    pub fn detect_for_path(path: &str) -> (ConfigFormat, String) {
        if let Ok(content) = fs::read_to_string(path) {
            if let Ok(root) = serde_json::from_str::<serde_json::Value>(&content) {
                if root.get("providers").and_then(|v| v.as_object()).is_some()
                    && root.get("provider").is_none()
                {
                    return (ConfigFormat::PiAgent, path.to_string());
                }
            }
        }
        (ConfigFormat::Opencode, path.to_string())
    }

    pub fn validate_target(&self, format: ConfigFormat) -> bool {
        let local_ok = match format {
            ConfigFormat::Opencode => Path::new(&self.opencode).exists(),
            ConfigFormat::PiAgent => {
                self.pi_agent.exists()
                    || self
                        .pi_agent
                        .parent()
                        .map(|p| p.exists())
                        .unwrap_or(false)
            }
        };
        local_ok || self.wsl_target(format).is_some()
    }

    pub fn target_path(&self, format: ConfigFormat) -> String {
        if let Some(wsl) = self.wsl_target(format) {
            return wsl;
        }
        match format {
            ConfigFormat::Opencode => self.opencode.clone(),
            ConfigFormat::PiAgent => self.pi_agent.to_string_lossy().into_owned(),
        }
    }
}
