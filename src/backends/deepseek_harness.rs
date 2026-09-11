//! DeepSeek Harness（DSH）后端：`~/.dsh/settings.yaml`。
//!
//! 只管理 `llm-pi-ai.providers`，其他顶层配置（ui、conversation、
//! agent-default-model、插件设置等）一律以 raw 为基底原样保留。

use super::{Backend, BackendLoad};
use crate::convert;
use crate::credentials;
use crate::format::ConfigFormat;
use crate::model::{AgentRow, ModelRow, ProviderRow};
use crate::util::{parse_yaml_content, read_config_content, wsl_home, WslPathProbe};
use serde_json::{Map, Value};
use std::path::Path;

pub struct DeepSeekHarnessBackend;
pub static BACKEND: DeepSeekHarnessBackend = DeepSeekHarnessBackend;

fn default_local_path() -> String {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    format!("{}\\.dsh\\settings.yaml", home)
}

fn is_dsh_root(v: &Value) -> bool {
    v.get("llm-pi-ai")
        .and_then(|x| x.get("providers"))
        .and_then(Value::as_object)
        .is_some()
}

fn model_from_dsh(v: &Value) -> ModelRow {
    let mut m = convert::model_from_pi(v);
    m.variants = v
        .get("reasoningEfforts")
        .and_then(Value::as_object)
        .map(|o| {
            o.values()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    m.original_variants = m.variants.clone();
    m.source_format = Some(ConfigFormat::DeepSeekHarness);
    m.raw = v.clone();
    m
}

fn model_to_dsh(m: &ModelRow, preserve_raw: bool) -> Value {
    // DSH 原生往返保留未知字段；跨格式写入使用 DSH 的固定字段顺序，
    // 避免将 opencode/pi 的方言字段泄漏到 DSH。
    let mut obj = if preserve_raw {
        m.raw.as_object().cloned().unwrap_or_default()
    } else {
        let mut fields = Map::new();
        fields.insert("id".into(), Value::String(m.id.clone()));
        if !m.name.trim().is_empty() {
            fields.insert("name".into(), Value::String(m.name.clone()));
        }
        if let Ok(v) = m.context.parse::<i64>() {
            fields.insert("contextWindow".into(), v.into());
        }
        if let Ok(v) = m.output.parse::<i64>() {
            fields.insert("maxTokens".into(), v.into());
        }
        Map::from_iter(fields)
    };
    if !preserve_raw || m.id != m.raw.get("id").and_then(Value::as_str).unwrap_or("") {
        obj.insert("id".into(), Value::String(m.id.clone()));
    }
    if !preserve_raw || m.name != m.raw.get("name").and_then(Value::as_str).unwrap_or("") {
        if m.name.trim().is_empty() {
            obj.remove("name");
        } else {
            obj.insert("name".into(), Value::String(m.name.clone()));
        }
    }
    let input: Vec<Value> = m
        .modalities_input
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| Value::String(s.to_string()))
        .collect();
    let raw_input = m.raw.get("input").and_then(Value::as_array);
    let input_changed = raw_input
        .map(|raw| raw != input.as_slice())
        .unwrap_or(!input.is_empty());
    if !preserve_raw || input_changed {
        if input.is_empty() {
            obj.remove("input");
        } else {
            obj.insert("input".into(), Value::Array(input));
        }
    }
    let context_changed = m.context != crate::util::num_at(&m.raw, "contextWindow");
    let output_changed = m.output != crate::util::num_at(&m.raw, "maxTokens");
    if !preserve_raw || context_changed {
        if let Ok(v) = m.context.parse::<i64>() {
            obj.insert("contextWindow".into(), v.into());
        } else {
            obj.remove("contextWindow");
        }
    }
    if !preserve_raw || output_changed {
        if let Ok(v) = m.output.parse::<i64>() {
            obj.insert("maxTokens".into(), v.into());
        } else {
            obj.remove("maxTokens");
        }
    }
    if preserve_raw && m.variants == m.original_variants {
        return Value::Object(order_fields(
            obj,
            &[
                "id",
                "name",
                "contextWindow",
                "maxTokens",
                "input",
                "reasoningEfforts",
            ],
        ));
    }
    if m.variants.trim().is_empty() {
        obj.remove("reasoningEfforts");
    } else {
        let mut efforts = Map::new();
        if let Some(raw) = m.raw.get("reasoningEfforts").and_then(Value::as_object) {
            for (k, v) in raw {
                if m.variants
                    .split(',')
                    .any(|name| name.trim() == v.as_str().unwrap_or(""))
                {
                    efforts.insert(k.clone(), v.clone());
                }
            }
        }
        for name in m
            .variants
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            efforts
                .entry(name.to_string())
                .or_insert_with(|| Value::String(name.to_string()));
        }
        obj.insert("reasoningEfforts".into(), Value::Object(efforts));
    }
    Value::Object(order_fields(
        obj,
        &[
            "id",
            "name",
            "contextWindow",
            "maxTokens",
            "input",
            "reasoningEfforts",
        ],
    ))
}

fn order_fields(mut object: Map<String, Value>, keys: &[&str]) -> Map<String, Value> {
    let mut ordered = Map::new();
    for key in keys {
        if let Some(value) = object.remove(*key) {
            ordered.insert((*key).to_string(), value);
        }
    }
    ordered.extend(object);
    ordered
}

fn dsh_base_url(api: &str, url: &str) -> String {
    if api != "anthropic-messages" {
        return url.to_string();
    }
    let trimmed = url.trim_end_matches('/');
    if let Some(stripped) = trimmed.strip_suffix("/v1") {
        stripped.trim_end_matches('/').to_string()
    } else {
        url.to_string()
    }
}

fn provider_from_dsh(key: &str, v: &Value, credentials_root: &Value) -> ProviderRow {
    let api = v.get("api").and_then(Value::as_str).unwrap_or_default();
    let models = v
        .get("models")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(model_from_dsh).collect())
        .unwrap_or_default();
    let env = v
        .get("apiKeyEnv")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| credentials::default_env_name(key));
    // 配置文件没有 timeoutMs 时默认显示 180000ms（保存时未修改则不写回）。
    let timeout_text = {
        let t = crate::util::num_at(v, "timeoutMs");
        if t.is_empty() {
            "180000".to_string()
        } else {
            t
        }
    };
    ProviderRow {
        key: key.to_string(),
        description: String::new(),
        npm: convert::api_to_npm(api),
        base_url: dsh_base_url(
            api,
            v.get("baseURL").and_then(Value::as_str).unwrap_or_default(),
        ),
        api_key: credentials::secret_for(credentials_root, &env),
        api_key_env: env.clone(),
        original_api_key_env: env.clone(),
        api_key_secret: credentials::secret_for(credentials_root, &env),
        original_api_key_secret: credentials::secret_for(credentials_root, &env),
        dsh_timeout_ms: timeout_text.clone(),
        dsh_retry_mode: v
            .get("retryPolicy")
            .and_then(|value| value.get("mode"))
            .and_then(Value::as_str)
            .unwrap_or("normal")
            .to_string(),
        dsh_max_retries: v
            .get("retryPolicy")
            .and_then(|value| value.get("maxRetries"))
            .map(crate::util::number_text_public)
            .unwrap_or_default(),
        original_dsh_timeout_ms: timeout_text.clone(),
        original_dsh_retry_mode: v
            .get("retryPolicy")
            .and_then(|value| value.get("mode"))
            .and_then(Value::as_str)
            .unwrap_or("normal")
            .to_string(),
        original_dsh_max_retries: v
            .get("retryPolicy")
            .and_then(|value| value.get("maxRetries"))
            .map(crate::util::number_text_public)
            .unwrap_or_default(),
        // 切到 opencode 页时沿用 DSH 的 timeoutMs（缺省 180000）。
        timeout: timeout_text.clone(),
        original_timeout: timeout_text.clone(),
        // DSH 无 compat 键；api 缺省或 openai-completions（chat/completions）
        // 不支持 developer role，默认不勾选。
        compat: !api.is_empty() && api != "openai-completions",
        // DSH 无该字段：转到 pi/omp 页面时默认不勾选。
        requires_reasoning_content: false,
        original_requires_reasoning_content: false,
        models,
        new_model: ModelRow::new(),
        source_format: Some(ConfigFormat::DeepSeekHarness),
        raw: v.clone(),
        pi_api: api.to_string(),
        show_api_key: false,
    }
}

fn dsh_api_for(p: &ProviderRow) -> String {
    let api = if p.pi_api.is_empty() {
        convert::npm_to_api(&p.npm)
    } else {
        p.pi_api.clone()
    };
    // openai-compatible 是 opencode 的 npm 名称，不是 DSH 的 api 枚举值。
    if api == "@ai-sdk/openai-compatible" || api.is_empty() {
        "openai-completions".to_string()
    } else {
        api
    }
}

fn yaml_scalar(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        Value::String(v) => {
            if v.is_empty()
                || v.chars().any(|c| c.is_whitespace() && c != ' ')
                || v.contains(':')
                || v.starts_with([
                    '[', ']', '{', '}', '#', '!', '&', '*', '?', '-', '|', '>', '@', '`',
                ])
                || matches!(v.as_str(), "null" | "true" | "false" | "~")
            {
                serde_yaml_ng::to_string(v)
                    .unwrap_or_else(|_| format!("\"{}\"", v.replace('"', "\\\"")))
                    .trim()
                    .to_string()
            } else {
                v.clone()
            }
        }
        _ => serde_yaml_ng::to_string(value)
            .unwrap_or_else(|_| "null\n".into())
            .trim()
            .to_string(),
    }
}

fn yaml_quoted(value: &str) -> String {
    // JSON 字符串也是合法的 YAML 双引号字符串，并能可靠转义引号、反斜杠和换行。
    serde_json::to_string(value).unwrap_or_else(|_| format!("\"{}\"", value.replace('"', "\\\"")))
}

fn dsh_scalar(key: &str, value: &Value) -> String {
    if matches!(key, "apiKeyEnv" | "api" | "baseURL" | "id" | "name") {
        if let Value::String(text) = value {
            return yaml_quoted(text);
        }
    }
    yaml_scalar(value)
}

fn yaml_flow(value: &Value, quote_strings: bool) -> Option<String> {
    let scalar = |value: &Value| match value {
        Value::String(text) if quote_strings => yaml_quoted(text),
        _ => yaml_scalar(value),
    };
    match value {
        Value::Array(items) if items.iter().all(Value::is_string) => Some(format!(
            "[ {} ]",
            items.iter().map(scalar).collect::<Vec<_>>().join(", ")
        )),
        Value::Object(object)
            if object
                .values()
                .all(|v| v.is_string() || v.is_number() || v.is_boolean()) =>
        {
            Some(format!(
                "{{ {} }}",
                object
                    .iter()
                    .map(|(k, v)| {
                        let key = if quote_strings {
                            yaml_quoted(k)
                        } else {
                            yaml_scalar(&Value::String(k.clone()))
                        };
                        format!("{}: {}", key, scalar(v))
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        }
        _ => None,
    }
}

fn render_dsh_value(value: &Value, indent: usize, out: &mut String) {
    let pad = " ".repeat(indent);
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                let key_text = yaml_scalar(&Value::String(key.clone()));
                let flow = if matches!(key.as_str(), "input" | "reasoningEfforts") {
                    yaml_flow(child, true)
                } else {
                    None
                };
                if let Some(flow) = flow {
                    out.push_str(&format!("{}{}: {}\n", pad, key_text, flow));
                } else if child.is_object() {
                    if child.as_object().is_some_and(Map::is_empty) {
                        out.push_str(&format!("{}{}: {{}}\n", pad, key_text));
                    } else {
                        out.push_str(&format!("{}{}:\n", pad, key_text));
                        render_dsh_value(child, indent + 2, out);
                    }
                } else if child.is_array() {
                    if child.as_array().is_some_and(Vec::is_empty) {
                        out.push_str(&format!("{}{}: []\n", pad, key_text));
                    } else {
                        out.push_str(&format!("{}{}:\n", pad, key_text));
                        render_dsh_value(child, indent + 2, out);
                    }
                } else {
                    out.push_str(&format!(
                        "{}{}: {}\n",
                        pad,
                        key_text,
                        dsh_scalar(key, child)
                    ));
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                if let Some(object) = item.as_object() {
                    if object.is_empty() {
                        out.push_str(&format!("{}- {{}}\n", pad));
                        continue;
                    }
                    let mut first = true;
                    for (key, child) in object {
                        let key_text = yaml_scalar(&Value::String(key.clone()));
                        if first {
                            first = false;
                            let flow = if matches!(key.as_str(), "input" | "reasoningEfforts") {
                                yaml_flow(child, true)
                            } else {
                                None
                            };
                            if let Some(flow) = flow {
                                out.push_str(&format!("{}- {}: {}\n", pad, key_text, flow));
                            } else if child.is_object() || child.is_array() {
                                out.push_str(&format!("{}- {}:\n", pad, key_text));
                                render_dsh_value(child, indent + 4, out);
                            } else {
                                out.push_str(&format!(
                                    "{}- {}: {}\n",
                                    pad,
                                    key_text,
                                    dsh_scalar(key, child)
                                ));
                            }
                        } else {
                            let flow = if matches!(key.as_str(), "input" | "reasoningEfforts") {
                                yaml_flow(child, true)
                            } else {
                                None
                            };
                            if let Some(flow) = flow {
                                out.push_str(&format!("{}  {}: {}\n", pad, key_text, flow));
                            } else if child.is_object() || child.is_array() {
                                out.push_str(&format!("{}  {}:\n", pad, key_text));
                                render_dsh_value(child, indent + 4, out);
                            } else {
                                out.push_str(&format!(
                                    "{}  {}: {}\n",
                                    pad,
                                    key_text,
                                    dsh_scalar(key, child)
                                ));
                            }
                        }
                    }
                } else {
                    out.push_str(&format!("{}- {}\n", pad, yaml_scalar(item)));
                }
            }
        }
        _ => out.push_str(&format!("{}{}\n", pad, yaml_scalar(value))),
    }
}

fn render_dsh_yaml(root: &Value) -> Result<String, String> {
    let mut output = String::new();
    render_dsh_value(root, 0, &mut output);
    Ok(output)
}

fn provider_to_dsh(p: &ProviderRow) -> Value {
    // 保存判定基于来源 agent 格式，而不是某个 provider 是否恰好有
    // apiKeyEnv/baseURL。DSH 允许这些字段缺省，不能因此丢失未知字段。
    let preserve_raw = p.source_format == Some(ConfigFormat::DeepSeekHarness);
    let env = credentials::effective_env_name(p);
    let mut obj = if preserve_raw {
        p.raw.as_object().cloned().unwrap_or_default()
    } else {
        let mut fields = Map::new();
        if !env.is_empty() {
            fields.insert("apiKeyEnv".into(), Value::String(env.clone()));
        }
        fields.insert("api".into(), Value::String(dsh_api_for(p)));
        if !p.base_url.is_empty() {
            fields.insert("baseURL".into(), Value::String(p.base_url.clone()));
        }
        fields
    };
    obj.insert("api".into(), Value::String(dsh_api_for(p)));
    if p.base_url.is_empty() {
        obj.remove("baseURL");
    } else {
        obj.insert(
            "baseURL".into(),
            Value::String(dsh_base_url(&dsh_api_for(p), &p.base_url)),
        );
    }
    if env.is_empty() {
        // DSH 原生 provider 未修改 apiKeyEnv 时保留原始引用名，防止
        // 页面切换或空输入把配置中的凭据引用误删。
        if !preserve_raw {
            obj.remove("apiKeyEnv");
        }
    } else {
        obj.insert("apiKeyEnv".into(), Value::String(env));
    }
    obj.insert(
        "models".into(),
        Value::Array(
            p.models
                .iter()
                .map(|model| model_to_dsh(model, preserve_raw))
                .collect(),
        ),
    );
    if p.dsh_timeout_ms != p.original_dsh_timeout_ms || !preserve_raw {
        if let Ok(value) = p.dsh_timeout_ms.parse::<i64>() {
            obj.insert("timeoutMs".into(), value.into());
        } else {
            obj.remove("timeoutMs");
        }
    }
    if p.dsh_retry_mode != p.original_dsh_retry_mode
        || p.dsh_max_retries != p.original_dsh_max_retries
        || !preserve_raw
    {
        if p.dsh_retry_mode.trim().is_empty() {
            obj.remove("retryPolicy");
        } else {
            let mut policy = obj
                .get("retryPolicy")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            policy.insert("mode".into(), Value::String(p.dsh_retry_mode.clone()));
            if let Ok(value) = p.dsh_max_retries.parse::<i64>() {
                policy.insert("maxRetries".into(), value.into());
            } else {
                policy.remove("maxRetries");
            }
            obj.insert("retryPolicy".into(), Value::Object(policy));
        }
    }
    Value::Object(order_fields(
        obj,
        &["apiKeyEnv", "api", "baseURL", "models"],
    ))
}

impl Backend for DeepSeekHarnessBackend {
    fn id(&self) -> ConfigFormat {
        ConfigFormat::DeepSeekHarness
    }
    fn default_local_path(&self) -> String {
        default_local_path()
    }
    fn default_wsl_path(&self) -> Option<String> {
        Some(format!("{}/.dsh/settings.yaml", wsl_home()?))
    }
    fn local_available(&self, path: &str) -> bool {
        Path::new(path).exists()
            || Path::new(path)
                .parent()
                .map(|p| p.exists())
                .unwrap_or(false)
    }
    fn wsl_available(&self, probe: WslPathProbe) -> bool {
        probe.path_exists || probe.parent_dir_exists
    }
    fn detect(&self, content: &str, path: &str) -> bool {
        let lower = path.to_lowercase();
        if lower.contains("/.dsh/") || lower.contains("\\.dsh\\") {
            return true;
        }
        parse_yaml_content(content)
            .map(|v| is_dsh_root(&v))
            .unwrap_or(false)
    }
    fn parse(&self, content: &str) -> Result<BackendLoad, String> {
        self.parse_at(content, "")
    }
    fn parse_at(&self, content: &str, path: &str) -> Result<BackendLoad, String> {
        let root = parse_yaml_content(content)?;
        let creds = if path.is_empty() {
            Value::Object(Map::new())
        } else {
            credentials::load_root(path)
        };
        let providers = root
            .get("llm-pi-ai")
            .and_then(|x| x.get("providers"))
            .and_then(Value::as_object)
            .map(|o| {
                o.iter()
                    .map(|(k, v)| provider_from_dsh(k, v, &creds))
                    .collect()
            })
            .unwrap_or_default();
        Ok(BackendLoad {
            root: root.clone(),
            agents: Vec::new(),
            providers,
            extras: root,
        })
    }
    fn serialize_root(
        &self,
        _agents: &[AgentRow],
        providers: &[ProviderRow],
        extras: &Value,
        target_root: Option<&Value>,
    ) -> Value {
        let mut root = target_root
            .cloned()
            .unwrap_or_else(|| extras.clone())
            .as_object()
            .cloned()
            .unwrap_or_default();
        let provider_values: Map<String, Value> = providers
            .iter()
            .filter(|p| !p.key.is_empty())
            .map(|p| (p.key.clone(), provider_to_dsh(p)))
            .collect();
        // 不 remove + insert llm-pi-ai：serde_json preserve_order 会把重新
        // 插入的键移动到根节点末尾，导致 DSH settings 顶层顺序发生变化。
        if let Some(llm) = root.get_mut("llm-pi-ai").and_then(Value::as_object_mut) {
            llm.insert("providers".into(), Value::Object(provider_values));
        } else {
            let mut llm = Map::new();
            llm.insert("providers".into(), Value::Object(provider_values));
            root.insert("llm-pi-ai".into(), Value::Object(llm));
        }
        Value::Object(root)
    }

    fn load_target_root(&self, path: &str) -> Value {
        read_config_content(path)
            .ok()
            .and_then(|s| parse_yaml_content(&s).ok())
            .unwrap_or_else(|| Value::Object(Map::new()))
    }
    fn save_sidecars(&self, path: &str, providers: &[ProviderRow]) -> Result<(), String> {
        credentials::save(path, providers)
    }
    fn icon_rgba(&self) -> Option<(&'static [u8], u32, u32)> {
        Some((
            include_bytes!("../../assets/agents/deepseek-harness_32.bin"),
            32,
            32,
        ))
    }
    fn render(&self, root: &Value, _compact: bool) -> Result<String, String> {
        render_dsh_yaml(root)
    }
}

#[cfg(test)]
mod tests {
    use super::dsh_base_url;

    #[test]
    fn dsh_base_url_strips_v1_only_for_messages_api() {
        assert_eq!(
            dsh_base_url("anthropic-messages", "https://example.test/v1"),
            "https://example.test"
        );
        assert_eq!(
            dsh_base_url("anthropic-messages", "https://example.test/v1/"),
            "https://example.test"
        );
        assert_eq!(
            dsh_base_url("openai-completions", "https://example.test/v1"),
            "https://example.test/v1"
        );
        assert_eq!(
            dsh_base_url("anthropic-messages", "https://example.test/v10"),
            "https://example.test/v10"
        );
    }
}
