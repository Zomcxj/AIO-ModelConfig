use crate::backends;
use crate::util::{is_wsl_path, read_wsl_file};
use std::path::Path;

/// 配置格式标识。新增后端时在 `backends` 模块实现并注册，这里加变体。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ConfigFormat {
    Opencode,
    PiAgent,
    OhMyPi,
    DeepSeekHarness,
}

impl ConfigFormat {
    pub fn label(&self) -> &str {
        match self {
            ConfigFormat::Opencode => "opencode",
            ConfigFormat::PiAgent => "pi-agent",
            ConfigFormat::OhMyPi => "oh-my-pi",
            ConfigFormat::DeepSeekHarness => "deepseek-harness",
        }
    }
}

/// 各后端的解析后路径容器（含用户可覆盖的本地路径）。
pub struct ConfigPaths {
    pub opencode: String,
    pub pi_agent: String,
    pub oh_my_pi: String,
    pub deepseek_harness: String,
}

impl Default for ConfigPaths {
    fn default() -> Self {
        Self {
            opencode: backends::backend(ConfigFormat::Opencode).default_local_path(),
            pi_agent: backends::backend(ConfigFormat::PiAgent).default_local_path(),
            oh_my_pi: backends::backend(ConfigFormat::OhMyPi).default_local_path(),
            deepseek_harness: backends::backend(ConfigFormat::DeepSeekHarness).default_local_path(),
        }
    }
}

impl ConfigPaths {
    /// 本后端实际使用的本地路径。
    pub fn local_path(&self, format: ConfigFormat) -> String {
        match format {
            ConfigFormat::Opencode => self.opencode.clone(),
            ConfigFormat::PiAgent => self.pi_agent.clone(),
            ConfigFormat::OhMyPi => self.oh_my_pi.clone(),
            ConfigFormat::DeepSeekHarness => self.deepseek_harness.clone(),
        }
    }

    /// 启动探测：找到第一个本地存在的默认配置。
    pub fn detect() -> Option<(ConfigFormat, String)> {
        for b in backends::BACKENDS {
            let p = b.default_local_path();
            if Path::new(&p).exists() {
                return Some((b.id(), p));
            }
        }
        None
    }

    /// 根据文件内容判别配置格式（无扩展名上下文）。
    pub fn detect_from_content(content: &str) -> ConfigFormat {
        backends::detect_format(content, "")
    }

    /// 根据路径 + 内容判别配置格式（支持 WSL 路径）。
    /// 内容不可得（新建场景）时按扩展名推断：`.yml/.yaml` 归 oh-my-pi，其余回落 opencode。
    pub fn detect_for_path(path: &str) -> (ConfigFormat, String) {
        let content = if is_wsl_path(path) {
            read_wsl_file(path).ok()
        } else {
            std::fs::read_to_string(path).ok()
        };
        match content {
            Some(content) => (backends::detect_format(&content, path), path.to_string()),
            None => (backends::detect_format("", path), path.to_string()),
        }
    }

    /// 目标是否可用（本地或 WSL）。
    pub fn validate_target(&self, format: ConfigFormat) -> bool {
        backends::target_available(format, &self.local_path(format))
    }

    /// 本地优先：本地文件存在时写本地，否则回落 WSL，最后回退本地默认路径（新建场景）。
    pub fn target_path(&self, format: ConfigFormat) -> String {
        backends::target_path(format, &self.local_path(format))
    }
}
