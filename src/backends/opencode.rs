//! opencode 后端：`~/.config/opencode/opencode.json`（JSONC）。
//!
//! 结构：顶层 `agent`（subagent 定义 map）+ `provider`（map）+ 其他顶层字段
//! （如 `mcp`）原样保留。

use super::{Backend, BackendLoad};
use crate::format::ConfigFormat;
use crate::model::{AgentRow, ProviderRow};
use crate::util::{parse_config_content, read_config_content, wsl_file_exists, wsl_home};
use serde_json::{Map, Value};
use std::path::Path;

pub struct OpenCodeBackend;

pub static BACKEND: OpenCodeBackend = OpenCodeBackend;

fn default_local_path() -> String {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    format!("{}\\.config\\opencode\\opencode.json", home)
}

impl Backend for OpenCodeBackend {
    fn id(&self) -> ConfigFormat {
        ConfigFormat::Opencode
    }

    fn file_ext(&self) -> &'static str {
        "json"
    }

    fn default_local_path(&self) -> String {
        default_local_path()
    }

    fn default_wsl_path(&self) -> Option<String> {
        Some(format!("{}/.config/opencode/opencode.json", wsl_home()?))
    }

    fn local_available(&self, local_path: &str) -> bool {
        Path::new(local_path).exists()
    }

    fn wsl_available(&self, wsl_path: &str) -> bool {
        wsl_file_exists(wsl_path)
    }

    fn detect(&self, content: &str, _path: &str) -> bool {
        // opencode 是判别回落项，这里给出正向特征：顶层含 provider 对象。
        parse_config_content(content)
            .map(|v| v.get("provider").and_then(|x| x.as_object()).is_some())
            .unwrap_or(false)
    }

    fn parse(&self, content: &str) -> Result<BackendLoad, String> {
        let v = parse_config_content(content)?;
        let agents = v
            .get("agent")
            .and_then(|x| x.as_object())
            .map(|o| o.iter().map(|(k, av)| AgentRow::from(k, av)).collect())
            .unwrap_or_default();
        let providers = v
            .get("provider")
            .and_then(|x| x.as_object())
            .map(|o| o.iter().map(|(k, pv)| ProviderRow::from(k, pv)).collect())
            .unwrap_or_default();
        Ok(BackendLoad {
            root: v.clone(),
            agents,
            providers,
            // opencode 的 extras 载体就是整个 root（agent/provider 保存时整体替换）
            extras: v,
        })
    }

    fn serialize_root(
        &self,
        agents: &[AgentRow],
        providers: &[ProviderRow],
        extras: &Value,
        target_root: Option<&Value>,
    ) -> Value {
        match target_root {
            None => {
                // 当前文件：以 UI 状态为准整体替换 agent / provider（删除即生效）
                let mut r = extras.clone();
                if let Value::Object(o) = &mut r {
                    let mut am = Map::new();
                    for a in agents {
                        if !a.key.is_empty() {
                            am.insert(a.key.clone(), a.to_value());
                        }
                    }
                    o.insert("agent".into(), Value::Object(am));

                    let mut pm = Map::new();
                    for p in providers {
                        if !p.key.is_empty() {
                            pm.insert(p.key.clone(), p.to_value());
                        }
                    }
                    o.insert("provider".into(), Value::Object(pm));
                }
                r
            }
            Some(target) => merge_opencode_root(target, agents, providers),
        }
    }

    fn load_target_root(&self, path: &str) -> Value {
        match read_config_content(path) {
            Ok(content) => parse_config_content(&content).unwrap_or(Value::Object(Map::new())),
            Err(_) => Value::Object(Map::new()),
        }
    }

    fn render(&self, root: &Value, compact: bool) -> Result<String, String> {
        Ok(if compact {
            crate::app::compact_json(root)
        } else {
            crate::app::pretty_json(root)
        })
    }
}

/// 将 UI 状态合并进 opencode 目标 root（跨格式保存用）：
/// agent / provider 以 UI 状态 upsert，目标已有同名条目被覆盖、
/// 不同名条目保留，其余顶层字段原样保留。
pub fn merge_opencode_root(
    target_root: &Value,
    agents: &[AgentRow],
    providers: &[ProviderRow],
) -> Value {
    let mut root = target_root.clone();
    if let Value::Object(o) = &mut root {
        let mut am = o
            .get("agent")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        for a in agents {
            if !a.key.is_empty() {
                am.insert(a.key.clone(), a.to_value());
            }
        }
        o.insert("agent".into(), Value::Object(am));

        let mut pm = o
            .get("provider")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        for p in providers {
            if !p.key.is_empty() {
                pm.insert(p.key.clone(), p.to_value());
            }
        }
        o.insert("provider".into(), Value::Object(pm));
    }
    root
}
