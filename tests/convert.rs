use aio_model_config::convert;
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
    assert_eq!(provider.compat, "false");
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
    assert_eq!(provider.compat, "true");
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
    assert_eq!(provider.compat, "");
    let output = convert::provider_to_pi(&provider);
    assert!(output.get("compat").is_none(), "compat should be omitted when not present");
}

#[test]
fn anthropic_url_v1_handling() {
    let v = json!({
        "baseUrl": "https://api.anthropic.com",
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
fn anthropic_url_v1_already_present() {
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
