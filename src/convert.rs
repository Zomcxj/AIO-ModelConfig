use crate::model::{ModelRow, ProviderRow};
use crate::util::{bool_at, nested_list_str, num_at, str_at};
use serde_json::{Map, Value};
use std::collections::HashSet;

/// 仅对 Anthropic 官方端点做 /v1 归一化；自定义代理 URL 原样保留。
fn is_official_anthropic_url(url: &str) -> bool {
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
        "openai-completions" | "openai-responses" => String::new(),
        other => other.to_string(),
    }
}

pub fn model_from_pi(v: &Value) -> ModelRow {
    let id = str_at(v, "id").to_string();
    let modalities_input = nested_list_str(v, &["input"]);
    let thinking_level_map = v
        .get("thinkingLevelMap")
        .and_then(|m| m.as_object())
        .map(|obj| obj.values().map(|val| val.as_str().unwrap_or("")).collect::<Vec<_>>().join(", "))
        .unwrap_or_default();
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
        variants: thinking_level_map,
        raw: v.clone(),
    }
}

pub fn model_to_pi(m: &ModelRow) -> Value {
    let mut obj = Map::new();
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
    if !m.variants.trim().is_empty() {
        let names: Vec<&str> = m
            .variants
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        // 若 raw 中已有 thinkingLevelMap 且其值集合与当前选择一致，则原样保留
        // （保护 {"high":"max"} 这类非对称映射的键）
        let raw_map = m.raw.get("thinkingLevelMap").and_then(|v| v.as_object());
        let same_values = raw_map
            .map(|rm| {
                let rm_vals: HashSet<String> = rm
                    .values()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect();
                let cur: HashSet<String> =
                    names.iter().map(|s| s.to_string()).collect();
                rm_vals == cur
            })
            .unwrap_or(false);
        let thinking_map: Map<String, Value> = if same_values {
            raw_map.unwrap().clone()
        } else {
            names
                .iter()
                .map(|n| (n.to_string(), Value::String(n.to_string())))
                .collect()
        };
        if !thinking_map.is_empty() {
            obj.insert("thinkingLevelMap".into(), Value::Object(thinking_map));
        }
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
    let mut r = ProviderRow {
        key: key.to_string(),
        description: String::new(),
        npm,
        base_url,
        api_key: str_at(v, "apiKey").to_string(),
        timeout: String::new(),
        compat,
        models,
        new_model: ModelRow::new(),
        raw: v.clone(),
        pi_api: api.to_string(),
        haystack: String::new(),
    };
    r.refresh_haystack();
    r
}

pub fn provider_to_pi(p: &ProviderRow) -> Value {
    let mut obj = Map::new();
    if !p.compat {
        obj.insert(
            "compat".into(),
            serde_json::json!({"supportsDeveloperRole": false}),
        );
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
    }
    if !p.api_key.is_empty() {
        obj.insert("apiKey".into(), Value::String(p.api_key.clone()));
    }
    obj.insert("api".into(), Value::String(api));
    let models: Vec<Value> = p.models.iter().map(model_to_pi).collect();
    obj.insert("models".into(), Value::Array(models));
    Value::Object(obj)
}

pub fn load_pi_providers(root: &Value) -> Vec<ProviderRow> {
    root.get("providers")
        .and_then(|x| x.as_object())
        .map(|o| {
            o.iter()
                .map(|(k, pv)| provider_from_pi(k, pv))
                .collect()
        })
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
