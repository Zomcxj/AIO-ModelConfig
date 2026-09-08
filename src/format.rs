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
        match format {
            ConfigFormat::Opencode => Path::new(&self.opencode).exists(),
            ConfigFormat::PiAgent => self.pi_agent.exists(),
        }
    }

    pub fn target_path(&self, format: ConfigFormat) -> String {
        match format {
            ConfigFormat::Opencode => self.opencode.clone(),
            ConfigFormat::PiAgent => self.pi_agent.to_string_lossy().into_owned(),
        }
    }
}
