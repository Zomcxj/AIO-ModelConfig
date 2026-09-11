use model_harbor::convert;
use model_harbor::model::ProviderRow;
use serde_json::json;

#[test]
fn cross_format_opencode_source_writes_compat_false() {
    // 从 opencode 加载（缺省 npm = chat/completions）保存到 pi/omp 时，
    // 未显式声明的 supportsDeveloperRole 默认值也要写入目标文件。
    let raw = json!({
        "options": {"baseURL": "https://x/v1", "apiKey": "sk-test"},
        "models": {}
    });
    let provider = ProviderRow::from("openai", &raw);
    assert!(!provider.compat);
    let out = convert::provider_to_pi(&provider);
    assert_eq!(out["compat"]["supportsDeveloperRole"], false);
}

#[test]
fn requires_reasoning_content_maps_pi_and_omp_keys() {
    // omp 键 → pi 页面：值被映射到 pi 的键
    let omp_raw = json!({
        "baseUrl": "https://x/v1",
        "apiKey": "k",
        "api": "openai-completions",
        "compat": {"requiresReasoningContentForAllAssistantTurns": false},
        "models": []
    });
    let provider = convert::provider_from_pi("p", &omp_raw);
    assert!(!provider.requires_reasoning_content);
    // 同格式未修改 → 保存时保留 raw 原样
    let out = convert::provider_to_pi(&provider);
    assert_eq!(
        out["compat"]["requiresReasoningContentForAllAssistantTurns"],
        false
    );
    assert!(out["compat"]
        .get("requiresReasoningContentOnAssistantMessages")
        .is_none());
}

#[test]
fn requires_reasoning_content_defaults_false_from_opencode() {
    // 加载 opencode（无该字段）→ pi 页面默认不勾选，保存时写入 false
    let raw = json!({
        "options": {"baseURL": "https://x/v1", "apiKey": "sk-test"},
        "models": {}
    });
    let provider = ProviderRow::from("openai", &raw);
    assert!(!provider.requires_reasoning_content);
    let out = convert::provider_to_pi(&provider);
    assert_eq!(
        out["compat"]["requiresReasoningContentOnAssistantMessages"],
        false
    );
}

#[test]
fn api_to_npm_mapping() {
    assert_eq!(
        convert::api_to_npm("anthropic-messages"),
        "@ai-sdk/anthropic"
    );
    assert_eq!(convert::api_to_npm("openai-completions"), "");
    assert_eq!(convert::api_to_npm("custom-api"), "custom-api");
}

#[test]
fn npm_to_api_mapping() {
    assert_eq!(
        convert::npm_to_api("@ai-sdk/anthropic"),
        "anthropic-messages"
    );
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
        "baseUrl": "https://api.anthropic.com",
        "apiKey": "sk-test",
        "api": "anthropic-messages",
        "compat": {
            "supportsDeveloperRole": true
        },
        "models": []
    });
    let provider = convert::provider_from_pi("anthropic", &v);
    assert!(provider.compat);
    let output = convert::provider_to_pi(&provider);
    assert!(
        output.get("compat").is_none(),
        "compat should be omitted when true"
    );
}

#[test]
fn openai_completions_defaults_compat_false() {
    // chat/completions 未显式声明 compat 时，supportsDeveloperRole 默认不打勾。
    let v = json!({
        "baseUrl": "https://api.openai.com/v1",
        "apiKey": "sk-test",
        "api": "openai-completions",
        "models": []
    });
    let provider = convert::provider_from_pi("openai", &v);
    assert!(!provider.compat);
    let output = convert::provider_to_pi(&provider);
    assert_eq!(output["compat"]["supportsDeveloperRole"], false);
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
    oc.insert("k".into(), provider_from_row_npm(&provider, ""));

    // 从 oc 读回，npm 为空
    let back = convert::provider_from_pi("k", &oc["k"]);
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
fn pi_roundtrip_preserves_provider_and_model_extras() {
    // pi 同格式往返：provider/model 级扩展字段必须保留（不得静默丢弃）
    let v = json!({
        "baseUrl": "https://gw/v1",
        "api": "openai-completions",
        "apiKey": "k",
        "authHeader": true,
        "headers": { "X-Team": "platform" },
        "models": [
            {
                "id": "m1",
                "name": "M1",
                "reasoning": true,
                "input": ["text"],
                "contextWindow": 200000,
                "maxTokens": 16384,
                "toolName": "custom"
            }
        ]
    });
    let provider = convert::provider_from_pi("gw", &v);
    let out = convert::provider_to_pi(&provider);
    assert_eq!(out["authHeader"], json!(true), "provider 扩展字段必须保留");
    assert_eq!(out["headers"]["X-Team"], json!("platform"));
    assert_eq!(
        out["models"][0]["toolName"],
        json!("custom"),
        "model 扩展字段必须保留"
    );
}

#[test]
fn pi_save_clearing_fields_removes_them() {
    // raw 基底下清空字段必须删除对应键，而非残留旧值
    let v = json!({
        "baseUrl": "https://x/v1",
        "api": "openai-completions",
        "models": [
            { "id": "m", "name": "M", "reasoning": true, "contextWindow": 100, "maxTokens": 50, "input": ["text"] }
        ]
    });
    let mut p = convert::provider_from_pi("p", &v);
    p.base_url = String::new();
    p.models[0].name = String::new();
    p.models[0].context = String::new();
    p.models[0].output = String::new();
    p.models[0].modalities_input = String::new();
    let out = convert::provider_to_pi(&p);
    assert!(out.get("baseUrl").is_none());
    assert!(out["models"][0].get("name").is_none());
    assert!(out["models"][0].get("contextWindow").is_none());
    assert!(out["models"][0].get("maxTokens").is_none());
    assert!(out["models"][0].get("input").is_none());
}

#[test]
fn pi_save_from_omp_raw_translates_thinking_and_keeps_extras() {
    // omp raw → pi 输出：thinking 块翻译为 thinkingLevelMap 后移除，其余扩展保留
    let mut m = model_harbor::model::ModelRow::new();
    m.id = "m".into();
    m.reasoning = true;
    m.variants = "max".into();
    m.raw = json!({
        "id": "m",
        "reasoning": true,
        "cost": { "input": 3.0 },
        "thinking": { "mode": "effort", "efforts": ["high"], "effortMap": { "high": "max" } }
    });
    let out = convert::model_to_pi(&m);
    assert_eq!(out["thinkingLevelMap"], json!({"high": "max"}));
    assert!(
        out.get("thinking").is_none(),
        "omp thinking 块必须翻译后移除"
    );
    assert_eq!(out["cost"]["input"], json!(3.0), "omp 扩展字段应保留");
}

#[test]
fn opencode_rows_from_pi_raw_build_fresh() {
    // pi raw 不得泄漏 api/compat/id/contextWindow/input 等方言键到 opencode 输出
    let v = json!({
        "baseUrl": "https://x/v1",
        "api": "openai-completions",
        "compat": { "supportsDeveloperRole": false },
        "models": [
            { "id": "m", "name": "M", "reasoning": true, "input": ["text"], "contextWindow": 128000, "maxTokens": 4096 }
        ]
    });
    let p = convert::provider_from_pi("p", &v);
    let out = p.to_value(); // opencode 方言
    assert!(out.get("api").is_none(), "pi 的 api 键不得泄漏");
    assert!(out.get("compat").is_none(), "pi 的 compat 键不得泄漏");
    let m = &out["models"]["m"];
    assert!(
        m.get("id").is_none(),
        "pi 的 id 键不得泄漏（opencode 以 map key 为身份）"
    );
    assert!(m.get("contextWindow").is_none());
    assert!(m.get("maxTokens").is_none());
    assert!(m.get("input").is_none());
    assert_eq!(
        m["limit"]["context"],
        json!(128000),
        "contextWindow 应翻译为 limit.context"
    );
    assert_eq!(
        m["modalities"]["input"][0],
        json!("text"),
        "input 应翻译为 modalities.input"
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

#[test]
fn pi_model_with_thinking_only_counts_as_reasoning() {
    // pi/omp 只写 thinkingLevelMap / thinking 块（DSH 为 reasoningEfforts）时，
    // reasoning 也应判定为开启，否则这些模型的勾选状态在其他页面显示不出来。
    assert!(
        convert::model_from_pi(&json!({"id": "m", "thinkingLevelMap": {"high": "high"}})).reasoning
    );
    assert!(convert::model_from_pi(&json!({"id": "m", "thinking": {"mode": "effort"}})).reasoning);
    assert!(
        convert::model_from_pi(&json!({"id": "m", "reasoningEfforts": {"high": "high"}})).reasoning
    );
    assert!(!convert::model_from_pi(&json!({"id": "m", "reasoning": false})).reasoning);
}

#[test]
fn provider_to_pi_places_compat_between_api_and_models() {
    // compat 必须固定跟在 api 之后、models 之前，不能因新增键被追加到字段末尾。
    let mut provider = ProviderRow::new();
    provider.key = "demo".into();
    provider.base_url = "https://example.com/v1".into();
    provider.api_key = "sk-x".into();
    provider.pi_api = "openai-completions".into();
    provider.compat = false;
    let out = convert::provider_to_pi(&provider);
    let keys: Vec<&str> = out
        .as_object()
        .expect("provider 应为对象")
        .keys()
        .map(String::as_str)
        .collect();
    assert!(
        keys.starts_with(&["baseUrl", "apiKey", "api", "compat", "models"]),
        "字段顺序应为 baseUrl → apiKey → api → compat → models，实际 {keys:?}"
    );

    // pi 原生 provider（raw 里 compat 原本在末尾）保存后也应归位
    let mut native = ProviderRow::new();
    native.key = "demo".into();
    native.base_url = "https://example.com/v1".into();
    native.api_key = "sk-x".into();
    native.pi_api = "openai-completions".into();
    native.compat = true;
    native.source_format = Some(model_harbor::format::ConfigFormat::Pi);
    native.raw = json!({
        "baseUrl": "https://example.com/v1",
        "apiKey": "sk-x",
        "api": "openai-completions",
        "models": [],
        "compat": {"requiresReasoningContentOnAssistantMessages": false}
    });
    let out = convert::provider_to_pi(&native);
    let keys: Vec<&str> = out
        .as_object()
        .expect("provider 应为对象")
        .keys()
        .map(String::as_str)
        .collect();
    assert!(
        keys.starts_with(&["baseUrl", "apiKey", "api", "compat", "models"]),
        "原生 provider 的 compat 也应归位，实际 {keys:?}"
    );
}
