//! oh-my-pi（omp）后端：`~/.omp/agent/models.yml`（YAML）。
//!
//! schema 与 pi-agent 同族（顶层 `providers` + extras），差异：
//! - 思考档位用 `thinking: {mode, efforts, effortMap}`（pi 用 `thinkingLevelMap`）；
//! - provider/model 支持大量扩展字段（headers/auth/discovery/modelOverrides/cost/...），
//!   保存时以 raw 为基底原样保留，仅重写 UI 管理的字段；
//! - 序列化为 YAML。
//!
//! 加载为双方言宽容：`thinking` 与 `thinkingLevelMap` 都能读入 IR variants。

use super::{Backend, BackendLoad};
use crate::convert;
use crate::format::ConfigFormat;
use crate::model::{AgentRow, ModelRow, ProviderRow};
use crate::util::{
    parse_config_content, parse_yaml_content, read_config_content, to_yaml_string, wsl_home,
    wsl_parent_dir_exists, wsl_path_exists,
};
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::path::Path;

pub struct OhMyPiBackend;

pub static BACKEND: OhMyPiBackend = OhMyPiBackend;

fn default_local_path() -> String {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    format!("{}\\.omp\\agent\\models.yml", home)
}

/// 判断 raw 是否为 opencode 方言（含 opencode 特征键）。
fn is_opencode_shaped_model(raw: &Value) -> bool {
    ["limit", "modalities", "options", "variants"]
        .iter()
        .any(|k| raw.get(k).is_some())
}

fn is_opencode_shaped_provider(raw: &Value) -> bool {
    raw.get("options").is_some()
        || raw
            .get("models")
            .map(|m| m.is_object())
            .unwrap_or(false)
}

fn string_set(items: impl Iterator<Item = String>) -> HashSet<String> {
    items.collect()
}

/// 模型 → omp 方言对象（保留 raw 中的未知字段，思考档位输出 thinking 块）。
pub fn model_to_omp(m: &ModelRow) -> Value {
    // opencode 来源全新构造；pi/omp 来源以 raw 为基底保留扩展字段
    let mut obj: Map<String, Value> = if is_opencode_shaped_model(&m.raw) {
        Map::new()
    } else {
        m.raw.as_object().cloned().unwrap_or_default()
    };
    // pi 方言思考键统一转为 thinking 块
    obj.remove("thinkingLevelMap");

    obj.insert("id".into(), Value::String(m.id.clone()));
    if !m.name.trim().is_empty() {
        obj.insert("name".into(), Value::String(m.name.clone()));
    }
    obj.insert("reasoning".into(), Value::Bool(m.reasoning));
    let input: Vec<Value> = m
        .modalities_input
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| Value::String(s.to_string()))
        .collect();
    obj.insert("input".into(), Value::Array(input));
    if let Ok(ctx) = m.context.parse::<i64>() {
        obj.insert("contextWindow".into(), Value::Number(ctx.into()));
    }
    if let Ok(out) = m.output.parse::<i64>() {
        obj.insert("maxTokens".into(), Value::Number(out.into()));
    }

    if m.variants.trim().is_empty() {
        obj.remove("thinking");
    } else if let Some(thinking) = omp_thinking(m) {
        obj.insert("thinking".into(), thinking);
    }
    Value::Object(obj)
}

/// 构造 omp thinking 块（variants 非空时调用）：
/// 1. omp 原生往返：raw.thinking 的档位值集合与当前一致 → 整块保留（含 defaultLevel 等）；
/// 2. pi 方言翻译：raw.thinkingLevelMap 值集合一致 → efforts=键集合，非对称时附 effortMap；
/// 3. 新建对称块。
fn omp_thinking(m: &ModelRow) -> Option<Value> {
    let names: Vec<String> = m
        .variants
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let cur: HashSet<String> = names.iter().cloned().collect();
    let vals_of = |o: &Map<String, Value>| -> HashSet<String> {
        o.values()
            .filter_map(|v| v.as_str())
            .map(|s| s.to_string())
            .collect()
    };

    // 1) omp 原生
    if let Some(t) = m.raw.get("thinking").and_then(|v| v.as_object()) {
        let raw_vals = t
            .get("effortMap")
            .and_then(|v| v.as_object())
            .map(&vals_of)
            .unwrap_or_else(|| {
                t.get("efforts")
                    .and_then(|v| v.as_array())
                    .map(|ef| string_set(ef.iter().filter_map(|v| v.as_str().map(String::from))))
                    .unwrap_or_default()
            });
        if raw_vals == cur {
            return Some(Value::Object(t.clone()));
        }
    }

    // 2) pi 方言翻译
    if let Some(tlm) = m.raw.get("thinkingLevelMap").and_then(|v| v.as_object()) {
        let tlm_vals = vals_of(tlm);
        if tlm_vals == cur {
            let keys: HashSet<String> = tlm.keys().cloned().collect();
            let symmetric = keys == tlm_vals;
            let mut t = Map::new();
            t.insert("mode".into(), json!("effort"));
            t.insert(
                "efforts".into(),
                Value::Array(tlm.keys().map(|k| Value::String(k.clone())).collect()),
            );
            if !symmetric {
                t.insert("effortMap".into(), Value::Object(tlm.clone()));
            }
            return Some(Value::Object(t));
        }
    }

    // 3) 新建对称
    Some(json!({
        "mode": "effort",
        "efforts": names,
    }))
}

/// Provider → omp 方言对象（raw 基底保留 headers/auth/discovery/modelOverrides 等）。
pub fn provider_to_omp(p: &ProviderRow) -> Value {
    let mut obj: Map<String, Value> = if is_opencode_shaped_provider(&p.raw) {
        Map::new()
    } else {
        p.raw.as_object().cloned().unwrap_or_default()
    };

    let api = if !p.npm.is_empty() {
        convert::npm_to_api(&p.npm)
    } else if !p.pi_api.is_empty() {
        p.pi_api.clone()
    } else {
        "openai-completions".to_string()
    };

    if !p.base_url.is_empty() {
        let save_url = if api == "anthropic-messages" && convert::is_official_anthropic_url(&p.base_url)
        {
            p.base_url.trim_end_matches("/v1").to_string()
        } else {
            p.base_url.clone()
        };
        obj.insert("baseUrl".into(), Value::String(save_url));
    }
    if !p.api_key.is_empty() {
        obj.insert("apiKey".into(), Value::String(p.api_key.clone()));
    }
    obj.insert("api".into(), Value::String(api));

    // compat 仅管理 supportsDeveloperRole，其余键（maxTokensField/extraBody/...）保留
    if !p.compat {
        let mut c = obj
            .get("compat")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        c.insert("supportsDeveloperRole".into(), Value::Bool(false));
        obj.insert("compat".into(), Value::Object(c));
    } else if let Some(c) = obj.get_mut("compat").and_then(|v| v.as_object_mut()) {
        c.remove("supportsDeveloperRole");
        if c.is_empty() {
            obj.remove("compat");
        }
    }

    let models: Vec<Value> = p.models.iter().map(model_to_omp).collect();
    obj.insert("models".into(), Value::Array(models));
    Value::Object(obj)
}

impl Backend for OhMyPiBackend {
    fn id(&self) -> ConfigFormat {
        ConfigFormat::OhMyPi
    }

    fn file_ext(&self) -> &'static str {
        "yml"
    }

    fn default_local_path(&self) -> String {
        default_local_path()
    }

    fn default_wsl_path(&self) -> Option<String> {
        Some(format!("{}/.omp/agent/models.yml", wsl_home()?))
    }

    fn local_available(&self, local_path: &str) -> bool {
        // 宽松判定：文件或父目录存在即可（父目录存在 = 可新建）
        Path::new(local_path).exists()
            || Path::new(local_path)
                .parent()
                .map(|p| p.exists())
                .unwrap_or(false)
    }

    fn wsl_available(&self, wsl_path: &str) -> bool {
        // 已安装判定：配置文件或其目录存在
        wsl_path_exists(wsl_path) || wsl_parent_dir_exists(wsl_path)
    }

    fn detect(&self, content: &str, path: &str) -> bool {
        let lower = path.to_lowercase();
        if lower.ends_with(".json") || lower.ends_with(".jsonc") {
            return false; // JSON 文件归 pi-agent 处理
        }
        let ext_yml = lower.ends_with(".yml") || lower.ends_with(".yaml");
        // 无扩展名上下文时，JSON 语法内容让位给 pi-agent（JSON 是 YAML 子集）
        if !ext_yml
            && parse_config_content(content)
                .map(|v| v.get("providers").and_then(|x| x.as_object()).is_some())
                .unwrap_or(false)
        {
            return false;
        }
        parse_yaml_content(content)
            .map(|v| {
                v.get("providers").and_then(|x| x.as_object()).is_some()
                    && v.get("provider").is_none()
            })
            .unwrap_or(false)
    }

    fn parse(&self, content: &str) -> Result<BackendLoad, String> {
        let v = parse_yaml_content(content)?;
        let providers = convert::load_pi_providers(&v);
        let extras = convert::load_pi_extras(&v);
        Ok(BackendLoad {
            root: v,
            agents: Vec::new(),
            providers,
            extras,
        })
    }

    fn serialize_root(
        &self,
        _agents: &[AgentRow],
        providers: &[ProviderRow],
        extras: &Value,
        target_root: Option<&Value>,
    ) -> Value {
        // 跨格式目标：extras 取目标文件自身的顶层字段，仅重写 providers
        let base = match target_root {
            Some(target) => target,
            None => extras,
        };
        let mut root = base.as_object().cloned().unwrap_or_default();
        let providers_map: Map<String, Value> = providers
            .iter()
            .filter(|p| !p.key.is_empty())
            .map(|p| (p.key.clone(), provider_to_omp(p)))
            .collect();
        root.insert("providers".into(), Value::Object(providers_map));
        Value::Object(root)
    }

    fn load_target_root(&self, path: &str) -> Value {
        match read_config_content(path) {
            Ok(content) => parse_yaml_content(&content)
                .map(|v| convert::load_pi_extras(&v))
                .unwrap_or(Value::Object(Map::new())),
            Err(_) => Value::Object(Map::new()),
        }
    }

    fn icon_rgba(&self) -> Option<(&'static [u8], u32, u32)> {
        Some((
            include_bytes!("../../assets/agents/oh-my-pi_32.bin"),
            32,
            32,
        ))
    }

    fn render(&self, root: &Value, _compact: bool) -> Result<String, String> {
        to_yaml_string(root)
    }
}
