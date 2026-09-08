use crate::model::{ModelRow, ProviderRow};
use crate::util::{bool_at, nested_list_str, num_at, str_at};
use serde_json::{Map, Value};

pub fn npm_to_api(npm: &str) -> String {
    match npm {
        "@ai-sdk/anthropic" => "anthropic-messages".to_string(),
        "" | "@ai-sdk/openai" => "openai-completions".to_string(),
        other => other.to_string(),
    }
}

pub fn api_to_npm(api: &str) -> String {
    match api {
        "anthropic-messages" => "@ai-sdk/anthropic".to_string(),
        "openai-completions" => String::new(),
        other => other.to_string(),
    }
}

pub fn model_from_pi(v: &Value) -> ModelRow {
    let id = str_at(v, "id").to_string();
    let modalities_input = nested_list_str(v, &["input"]);
    ModelRow {
        id: id.clone(),
        name: str_at(v, "name").to_string(),
        reasoning: bool_at(v, "reasoning"),
        tool_call: true,
        context: num_at(v, "contextWindow"),
        output: num_at(v, "maxTokens"),
        modalities_input,
        modalities_output: "text".to_string(),
        variants: String::new(),
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
    Value::Object(obj)
}

pub fn provider_from_pi(key: &str, v: &Value) -> ProviderRow {
    let api = str_at(v, "api");
    let npm = api_to_npm(api);
    let raw_url = str_at(v, "baseUrl").to_string();
    let base_url = if api == "anthropic-messages" && !raw_url.ends_with("/v1") {
        format!("{}/v1", raw_url.trim_end_matches('/'))
    } else {
        raw_url
    };
    let models = v
        .get("models")
        .and_then(|x| x.as_array())
        .map(|arr| arr.iter().map(|mv| model_from_pi(mv)).collect())
        .unwrap_or_default();
    let mut r = ProviderRow {
        key: key.to_string(),
        description: String::new(),
        npm,
        base_url,
        api_key: str_at(v, "apiKey").to_string(),
        timeout: String::new(),
        models,
        new_model: ModelRow::new(),
        raw: v.clone(),
        haystack: String::new(),
    };
    r.refresh_haystack();
    r
}

pub fn provider_to_pi(p: &ProviderRow) -> Value {
    let mut obj = Map::new();
    if let Some(compat) = p.raw.get("compat") {
        obj.insert("compat".into(), compat.clone());
    }
    let api = npm_to_api(&p.npm);
    if !p.base_url.is_empty() {
        let save_url = if api == "anthropic-messages" {
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
    let models: Vec<Value> = p.models.iter().map(|m| model_to_pi(m)).collect();
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
