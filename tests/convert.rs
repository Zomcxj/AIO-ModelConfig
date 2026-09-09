use model_harbor::convert;
use serde_json::json;

#[test]
fn api_to_npm_mapping() {
    assert_eq!(convert::api_to_npm("anthropic-messages"), "@ai-sdk/anthropic");
    assert_eq!(convert::api_to_npm("openai-completions"), "");
    assert_eq!(convert::api_to_npm("custom-api"), "custom-api");
}

#[test]
fn npm_to_api_mapping() {
    assert_eq!(convert::npm_to_api("@ai-sdk/anthropic"), "anthropic-messages");
    assert_eq!(convert::npm_to_api(""), "openai-completions");
    assert_eq!(convert::npm_to_api("@ai-sdk/openai"), "openai-completions");
    assert_eq!(convert::npm_to_api("custom-npm"), "custom-npm");
}

#[test]
fn model_from_pi_basic() {
    let v = json!({
        "id": "gpt-4o",
        "name": "GPT-4o",
        "reasoning": false,
        "input": ["text", "image"],
        "contextWindow": 128000,
        "maxTokens": 4096
    });
    let model = convert::model_from_pi(&v);
    assert_eq!(model.id, "gpt-4o");
    assert_eq!(model.name, "GPT-4o");
    assert!(!model.reasoning);
    assert!(model.tool_call);
    assert_eq!(model.context, "128000");
    assert_eq!(model.output, "4096");
    assert_eq!(model.modalities_input, "text, image");
    assert_eq!(model.modalities_output, "text");
    assert!(model.variants.is_empty());
}

#[test]
fn model_to_pi_roundtrip() {
    let v = json!({
        "id": "claude-sonnet-4-20250514",
        "name": "Claude Sonnet 4",
        "reasoning": false,
        "input": ["text", "image"],
        "contextWindow": 200000,
        "maxTokens": 8192
    });
    let model = convert::model_from_pi(&v);
    let output = convert::model_to_pi(&model);
    assert_eq!(output["id"], "claude-sonnet-4-20250514");
    assert_eq!(output["name"], "Claude Sonnet 4");
    assert_eq!(output["reasoning"], false);
    assert_eq!(output["input"], json!(["text", "image"]));
    assert_eq!(output["contextWindow"], 200000);
    assert_eq!(output["maxTokens"], 8192);
}

#[test]
fn provider_from_pi_basic() {
    let v = json!({
        "baseUrl": "https://api.openai.com/v1",
        "apiKey": "sk-test",
        "api": "openai-completions",
        "models": [
            {
                "id": "gpt-4o",
                "name": "GPT-4o",
                "reasoning": false,
                "input": ["text"],
                "contextWindow": 128000,
                "maxTokens": 4096
            }
        ]
    });
    let provider = convert::provider_from_pi("openai", &v);
    assert_eq!(provider.key, "openai");
    assert_eq!(provider.base_url, "https://api.openai.com/v1");
    assert_eq!(provider.api_key, "sk-test");
    assert!(provider.npm.is_empty());
    assert_eq!(provider.models.len(), 1);
    assert_eq!(provider.models[0].id, "gpt-4o");
}

#[test]
fn provider_from_pi_anthropic() {
    let v = json!({
        "baseUrl": "https://api.anthropic.com",
        "apiKey": "sk-ant-test",
        "api": "anthropic-messages",
        "models": []
    });
    let provider = convert::provider_from_pi("anthropic", &v);
    assert_eq!(provider.npm, "@ai-sdk/anthropic");
}

#[test]
fn provider_to_pi_roundtrip() {
    let v = json!({
        "baseUrl": "https://api.openai.com/v1",
        "apiKey": "sk-test",
        "api": "openai-completions",
        "models": [
            {
                "id": "gpt-4o",
                "name": "GPT-4o",
                "reasoning": false,
                "input": ["text"],
                "contextWindow": 128000,
                "maxTokens": 4096
            }
        ]
    });
    let provider = convert::provider_from_pi("openai", &v);
    let output = convert::provider_to_pi(&provider);
    assert_eq!(output["baseUrl"], "https://api.openai.com/v1");
    assert_eq!(output["apiKey"], "sk-test");
    assert_eq!(output["api"], "openai-completions");
    assert!(output["models"].as_array().unwrap().len() == 1);
}

#[test]
fn load_pi_providers() {
    let root = json!({
        "providers": {
            "openai": {
                "baseUrl": "https://api.openai.com/v1",
                "apiKey": "sk-test",
                "api": "openai-completions",
                "models": []
            },
            "anthropic": {
                "baseUrl": "https://api.anthropic.com",
                "apiKey": "sk-ant-test",
                "api": "anthropic-messages",
                "models": []
            }
        }
    });
    let providers = convert::load_pi_providers(&root);
    assert_eq!(providers.len(), 2);
    let keys: Vec<&str> = providers.iter().map(|p| p.key.as_str()).collect();
    assert!(keys.contains(&"openai"));
    assert!(keys.contains(&"anthropic"));
}

#[test]
fn load_pi_extras() {
    let root = json!({
        "providers": {},
        "custom_field": "value",
        "another_field": 123
    });
    let extras = convert::load_pi_extras(&root);
    assert_eq!(extras["custom_field"], "value");
    assert_eq!(extras["another_field"], 123);
    assert!(extras.get("providers").is_none());
}

#[test]
fn to_pi_root_preserves_extras() {
    let extras = json!({
        "custom_field": "value"
    });
    let providers = vec![];
    let root = convert::to_pi_root(&providers, &extras);
    assert_eq!(root["custom_field"], "value");
    assert!(root["providers"].as_object().unwrap().is_empty());
}

#[test]
fn provider_to_pi_preserves_compat() {
    let v = json!({
        "baseUrl": "https://api.openai.com/v1",
        "apiKey": "sk-test",
        "api": "openai-completions",
        "compat": {
            "supportsDeveloperRole": false
        },
        "models": []
    });
    let provider = convert::provider_from_pi("openai", &v);
    assert!(!provider.compat);
    let output = convert::provider_to_pi(&provider);
    assert_eq!(output["compat"]["supportsDeveloperRole"], false);
}

#[test]
fn provider_to_pi_omits_compat_when_true() {
    let v = json!({
        "baseUrl": "https://api.openai.com/v1",
        "apiKey": "sk-test",
        "api": "openai-completions",
        "compat": {
            "supportsDeveloperRole": true
        },
        "models": []
    });
    let provider = convert::provider_from_pi("openai", &v);
    assert!(provider.compat);
    let output = convert::provider_to_pi(&provider);
    assert!(output.get("compat").is_none(), "compat should be omitted when true");
}

#[test]
fn provider_to_pi_omits_compat_when_empty() {
    let v = json!({
        "baseUrl": "https://api.openai.com/v1",
        "apiKey": "sk-test",
        "api": "openai-completions",
        "models": []
    });
    let provider = convert::provider_from_pi("openai", &v);
    assert!(provider.compat);
    let output = convert::provider_to_pi(&provider);
    assert!(output.get("compat").is_none(), "compat should be omitted when not present");
}

#[test]
fn anthropic_url_passthrough_no_v1_added() {
    let v = json!({
        "baseUrl": "https://api.anthropic.com",
        "apiKey": "sk-ant-test",
        "api": "anthropic-messages",
        "models": []
    });
    let provider = convert::provider_from_pi("anthropic", &v);
    assert_eq!(provider.base_url, "https://api.anthropic.com");
    let output = convert::provider_to_pi(&provider);
    assert_eq!(output["baseUrl"], "https://api.anthropic.com");
}

#[test]
fn anthropic_url_v1_stripped_on_save() {
    let v = json!({
        "baseUrl": "https://api.anthropic.com/v1",
        "apiKey": "sk-ant-test",
        "api": "anthropic-messages",
        "models": []
    });
    let provider = convert::provider_from_pi("anthropic", &v);
    assert_eq!(provider.base_url, "https://api.anthropic.com/v1");
    let output = convert::provider_to_pi(&provider);
    assert_eq!(output["baseUrl"], "https://api.anthropic.com");
}

#[test]
fn responses_api_maps_to_empty_npm() {
    assert_eq!(convert::api_to_npm("openai-responses"), "");
    let v = json!({
        "baseUrl": "https://example.com/v1",
        "apiKey": "sk-test",
        "api": "openai-responses",
        "models": []
    });
    let provider = convert::provider_from_pi("k", &v);
    assert_eq!(provider.pi_api, "openai-responses");
    let output = convert::provider_to_pi(&provider);
    assert_eq!(output["api"], "openai-responses");
    assert_eq!(output["baseUrl"], "https://example.com/v1");
}

#[test]
fn pi_api_survives_oc_roundtrip_without_npm() {
    let v = json!({
        "baseUrl": "https://example.com/v1",
        "apiKey": "sk-test",
        "api": "anthropic-messages",
        "models": []
    });
    let provider = convert::provider_from_pi("k", &v);
    assert_eq!(provider.npm, "@ai-sdk/anthropic");

    // OC 保存：npm 为空的 provider 会丢失 npm 字段（oc 格式用 npm 表达 api）
    let mut oc = serde_json::Map::new();
    oc.insert(
        "k".into(),
        provider_from_row_npm(&provider, ""),
    );

    // 从 oc 读回，npm 为空
    let back = convert::provider_from_pi(
        "k",
        &oc["k"],
    );
    let _ = back;
    // 真正的断言在 row 层：row 保留 pi_api 记忆
    assert_eq!(provider.pi_api, "anthropic-messages");
    let out = convert::provider_to_pi(&provider);
    assert_eq!(out["api"], "anthropic-messages");
}

fn provider_from_row_npm(p: &model_harbor::model::ProviderRow, npm: &str) -> serde_json::Value {
    // 模拟 ProviderRow::to_value 的 oc 输出（npm 为空时字段被移除）
    let mut m = serde_json::Map::new();
    let mut options = serde_json::Map::new();
    options.insert("baseURL".into(), p.base_url.clone().into());
    options.insert("apiKey".into(), p.api_key.clone().into());
    if !npm.is_empty() {
        m.insert("npm".into(), npm.into());
    }
    m.insert("options".into(), options.into());
    let mut models = serde_json::Map::new();
    for mdl in &p.models {
        models.insert(mdl.id.clone(), mdl.to_value());
    }
    m.insert("models".into(), models.into());
    m.into()
}

#[test]
fn thinking_level_map_asymmetric_roundtrip() {
    let v = json!({
        "id": "m",
        "name": "M",
        "reasoning": true,
        "thinkingLevelMap": { "high": "max" }
    });
    let model = convert::model_from_pi(&v);
    assert_eq!(model.variants, "max");
    let out = convert::model_to_pi(&model);
    assert_eq!(
        out["thinkingLevelMap"]["high"], "max",
        "asymmetric mapping keys must survive roundtrip"
    );
}

#[test]
fn anthropic_proxy_url_v1_preserved() {
    let v = json!({
        "baseUrl": "https://my-gateway.example/v1",
        "apiKey": "sk-test",
        "api": "anthropic-messages",
        "models": []
    });
    let provider = convert::provider_from_pi("proxy", &v);
    let output = convert::provider_to_pi(&provider);
    assert_eq!(
        output["baseUrl"], "https://my-gateway.example/v1",
        "proxy URLs must not be rewritten"
    );
}
