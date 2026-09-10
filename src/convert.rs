use crate::model::{ModelRow, ProviderRow};
use crate::util::{bool_at, nested_list_str, num_at, str_at};
use serde_json::{Map, Value};
use std::collections::HashSet;

/// 判断 raw 是否为 opencode 方言（含 opencode 特征键）。
/// pi / omp 输出时以此为界：opencode 形状全新构造，其余以 raw 为基底保留扩展字段。
pub(crate) fn is_opencode_shaped_model(raw: &Value) -> bool {
    ["limit", "modalities", "options", "variants"]
        .iter()
        .any(|k| raw.get(k).is_some())
}

pub(crate) fn is_opencode_shaped_provider(raw: &Value) -> bool {
    raw.get("options").is_some() || raw.get("models").map(|m| m.is_object()).unwrap_or(false)
}

/// 判断 raw 是否为 pi / omp 方言（含 pi 特征键）。
/// opencode 输出时以此为界：pi 形状全新构造，防止方言键泄漏。
pub(crate) fn is_dsh_shaped_model(raw: &Value) -> bool {
    raw.get("reasoningEfforts").is_some()
}

pub(crate) fn is_dsh_shaped_provider(raw: &Value) -> bool {
    raw.get("apiKeyEnv").is_some() || raw.get("baseURL").is_some()
}

/// 仅对 Anthropic 官方端点做 /v1 归一化；自定义代理 URL 原样保留。
pub(crate) fn is_official_anthropic_url(url: &str) -> bool {
    let u = url.trim_end_matches('/');
    u == "https://api.anthropic.com/v1" || u == "https://api.anthropic.com"
}

pub fn npm_to_api(npm: &str) -> String {
    match npm {
        "@ai-sdk/anthropic" => "anthropic-messages".to_string(),
        "@ai-sdk/google" => "google-generative-ai".to_string(),
        "" | "@ai-sdk/openai" => "openai-completions".to_string(),
        other => other.to_string(),
    }
}

pub fn api_to_npm(api: &str) -> String {
    match api {
        "anthropic-messages" => "@ai-sdk/anthropic".to_string(),
        "google-generative-ai" => "@ai-sdk/google".to_string(),
        // 以下 api 无对应 @ai-sdk npm 包（opencode 侧回退默认兼容层）
        "openai-completions"
        | "openai-responses"
        | "openai-codex-responses"
        | "azure-openai-responses"
        | "bedrock-converse-stream"
        | "google-gemini-cli"
        | "google-vertex" => String::new(),
        other => other.to_string(),
    }
}

/// 提取思考档位的“发送值”集合（逗号分隔）：
/// - omp 方言：thinking.effortMap 的值（无 effortMap 时用 efforts）
/// - pi 方言：thinkingLevelMap 的值
fn thinking_values(v: &Value) -> String {
    if let Some(t) = v.get("thinking") {
        if let Some(em) = t.get("effortMap").and_then(|m| m.as_object()) {
            return em
                .values()
                .filter_map(|val| val.as_str())
                .collect::<Vec<_>>()
                .join(", ");
        }
        if let Some(ef) = t.get("efforts").and_then(|a| a.as_array()) {
            return ef
                .iter()
                .filter_map(|val| val.as_str())
                .collect::<Vec<_>>()
                .join(", ");
        }
        // thinking 块无 efforts/effortMap（如 budget 模式）→ 回落 thinkingLevelMap
    }
    v.get("thinkingLevelMap")
        .and_then(|m| m.as_object())
        .map(|obj| {
            obj.values()
                .map(|val| val.as_str().unwrap_or(""))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

pub fn model_from_pi(v: &Value) -> ModelRow {
    let id = str_at(v, "id").to_string();
    let modalities_input = nested_list_str(v, &["input"]);
    ModelRow {
        id: id.clone(),
        name: str_at(v, "name").to_string(),
        reasoning: bool_at(v, "reasoning"),
        tool_call: true,
        store: false,
        context: num_at(v, "contextWindow"),
        output: num_at(v, "maxTokens"),
        modalities_input,
        modalities_output: "text".to_string(),
        variants: thinking_values(v),
        original_variants: thinking_values(v),
        source_format: Some(if is_dsh_shaped_model(v) {
            crate::format::ConfigFormat::DeepSeekHarness
        } else {
            crate::format::ConfigFormat::PiAgent
        }),
        raw: v.clone(),
    }
}

pub fn model_to_pi(m: &ModelRow) -> Value {
    // opencode 来源全新构造；pi/omp 来源以 raw 为基底保留扩展字段（cost/toolName 等）
    let mut obj: Map<String, Value> =
        if is_opencode_shaped_model(&m.raw) || is_dsh_shaped_model(&m.raw) {
            Map::new()
        } else {
            m.raw.as_object().cloned().unwrap_or_default()
        };
    // omp 方言的 thinking 块由 thinkingLevelMap 表达，翻译后移除
    obj.remove("thinking");
    obj.insert("id".into(), Value::String(m.id.clone()));
    if !m.name.trim().is_empty() {
        obj.insert("name".into(), Value::String(m.name.clone()));
    } else {
        obj.remove("name");
    }
    obj.insert("reasoning".into(), Value::Bool(m.reasoning));
    let input: Vec<Value> = m
        .modalities_input
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| Value::String(s.to_string()))
        .collect();
    if input.is_empty() {
        obj.remove("input");
    } else {
        obj.insert("input".into(), Value::Array(input));
    }
    if let Ok(ctx) = m.context.parse::<i64>() {
        obj.insert("contextWindow".into(), Value::Number(ctx.into()));
    } else {
        obj.remove("contextWindow");
    }
    if let Ok(out) = m.output.parse::<i64>() {
        obj.insert("maxTokens".into(), Value::Number(out.into()));
    } else {
        obj.remove("maxTokens");
    }
    if m.variants.trim().is_empty() {
        obj.remove("thinkingLevelMap");
        return Value::Object(obj);
    }
    let names: Vec<&str> = m
        .variants
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    let cur: HashSet<String> = names.iter().map(|s| s.to_string()).collect();
    let thinking_map: Map<String, Value> =
        // 1) pi 方言原样保留：raw.thinkingLevelMap 值集合与当前选择一致
        //    （保护 {"high":"max"} 这类非对称映射的键）
        if let Some(rm) = m.raw.get("thinkingLevelMap").and_then(|v| v.as_object()) {
            let rm_vals: HashSet<String> = rm
                .values()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect();
            if rm_vals == cur {
                rm.clone()
            } else {
                Map::new()
            }
        }
        // 2) omp 方言翻译：raw.thinking.effortMap 值集合一致 → 直接作为 thinkingLevelMap
        else if let Some(em) = m
            .raw
            .get("thinking")
            .and_then(|t| t.get("effortMap"))
            .and_then(|v| v.as_object())
        {
            let em_vals: HashSet<String> = em
                .values()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect();
            if em_vals == cur {
                em.clone()
            } else {
                Map::new()
            }
        } else {
            Map::new()
        };
    // 都不匹配 → 对称映射 {档位: 档位}
    let thinking_map = if thinking_map.is_empty() {
        names
            .iter()
            .map(|n| (n.to_string(), Value::String(n.to_string())))
            .collect()
    } else {
        thinking_map
    };
    if !thinking_map.is_empty() {
        obj.insert("thinkingLevelMap".into(), Value::Object(thinking_map));
    }
    Value::Object(obj)
}

pub fn provider_from_pi(key: &str, v: &Value) -> ProviderRow {
    let api = str_at(v, "api");
    let npm = api_to_npm(api);
    let base_url = str_at(v, "baseUrl").to_string();
    let models = v
        .get("models")
        .and_then(|x| x.as_array())
        .map(|arr| arr.iter().map(model_from_pi).collect())
        .unwrap_or_default();
    let compat = v
        .get("compat")
        .and_then(|c| c.get("supportsDeveloperRole"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let r = ProviderRow {
        key: key.to_string(),
        description: String::new(),
        npm,
        base_url,
        api_key: str_at(v, "apiKey").to_string(),
        api_key_env: String::new(),
        original_api_key_env: String::new(),
        api_key_secret: String::new(),
        original_api_key_secret: String::new(),
        dsh_timeout_ms: String::new(),
        dsh_retry_mode: String::new(),
        dsh_max_retries: String::new(),
        original_dsh_timeout_ms: String::new(),
        original_dsh_retry_mode: String::new(),
        original_dsh_max_retries: String::new(),
        timeout: String::new(),
        compat,
        models,
        new_model: ModelRow::new(),
        source_format: Some(crate::format::ConfigFormat::PiAgent),
        raw: v.clone(),
        pi_api: api.to_string(),
    };
    r
}

pub fn provider_to_pi(p: &ProviderRow) -> Value {
    // opencode 来源全新构造；pi/omp 来源以 raw 为基底保留扩展字段（headers/auth 等）
    let mut obj: Map<String, Value> =
        if is_opencode_shaped_provider(&p.raw) || is_dsh_shaped_provider(&p.raw) {
            Map::new()
        } else {
            p.raw.as_object().cloned().unwrap_or_default()
        };
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
    let api = if !p.npm.is_empty() {
        npm_to_api(&p.npm)
    } else if !p.pi_api.is_empty() {
        p.pi_api.clone()
    } else {
        "openai-completions".to_string()
    };
    if !p.base_url.is_empty() {
        let save_url = if api == "anthropic-messages" && is_official_anthropic_url(&p.base_url) {
            p.base_url.trim_end_matches("/v1").to_string()
        } else {
            p.base_url.clone()
        };
        obj.insert("baseUrl".into(), Value::String(save_url));
    } else {
        obj.remove("baseUrl");
    }
    if !p.api_key.is_empty() {
        obj.insert("apiKey".into(), Value::String(p.api_key.clone()));
    } else {
        obj.remove("apiKey");
    }
    obj.insert("api".into(), Value::String(api));
    let models: Vec<Value> = p.models.iter().map(model_to_pi).collect();
    obj.insert("models".into(), Value::Array(models));
    Value::Object(obj)
}

pub fn load_pi_providers(root: &Value) -> Vec<ProviderRow> {
    root.get("providers")
        .and_then(|x| x.as_object())
        .map(|o| o.iter().map(|(k, pv)| provider_from_pi(k, pv)).collect())
        .unwrap_or_default()
}

pub fn load_pi_extras(root: &Value) -> Value {
    if let Some(obj) = root.as_object() {
        let extras: Map<String, Value> = obj
            .iter()
            .filter(|(k, _)| k.as_str() != "providers")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Value::Object(extras)
    } else {
        Value::Object(Map::new())
    }
}

pub fn to_pi_root(providers: &[ProviderRow], extras: &Value) -> Value {
    let mut root = extras.as_object().cloned().unwrap_or_default();
    let providers_map: Map<String, Value> = providers
        .iter()
        .filter(|p| !p.key.is_empty())
        .map(|p| (p.key.clone(), provider_to_pi(p)))
        .collect();
    root.insert("providers".into(), Value::Object(providers_map));
    Value::Object(root)
}
