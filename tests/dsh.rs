use model_harbor::backends;
use model_harbor::format::ConfigFormat;
use model_harbor::model::ProviderRow;
use model_harbor::util::parse_yaml_content;
use serde_json::json;

fn temp_path(name: &str) -> std::path::PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("model_harbor_dsh_{}_{}", std::process::id(), nonce));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn dsh_parse_reads_sidecar_without_putting_secret_in_settings() {
    let settings = temp_path("settings.yaml");
    let credentials = model_harbor::credentials::sidecar_path(settings.to_str().unwrap());
    std::fs::write(
        &settings,
        r#"
ui-theme:
  name: dark
llm-pi-ai:
  providers:
    demo:
      apiKeyEnv: DSH_TEST_KEY
      api: openai-completions
      baseURL: https://example.invalid/v1
      timeoutMs: 180000
      retryPolicy:
        mode: normal
        maxRetries: 3
      customProviderField: keep-me
      models:
        - id: demo-model
          name: Demo
          contextWindow: 1234
          maxTokens: 321
          input: [text]
          reasoningEfforts:
            medium: medium
agent-default-model:
  provider: demo
  model: demo-model
"#,
    )
    .unwrap();
    std::fs::write(
        &credentials,
        "version: 1\nrefs:\n  DSH_TEST_KEY: test-only-placeholder\nrecords:\n  keep: true\n",
    )
    .unwrap();

    let load = backends::load_backend(ConfigFormat::DeepSeekHarness, settings.to_str().unwrap())
        .expect("DSH 配置应可加载");
    assert_eq!(load.providers.len(), 1);
    assert_eq!(load.providers[0].api_key_env, "DSH_TEST_KEY");
    assert_eq!(load.providers[0].api_key_secret, "test-only-placeholder");
    assert_eq!(load.providers[0].dsh_timeout_ms, "180000");
    assert_eq!(load.providers[0].dsh_retry_mode, "normal");
    assert_eq!(load.providers[0].dsh_max_retries, "3");
    assert_eq!(load.providers[0].models[0].variants, "medium");
    assert_eq!(load.root["ui-theme"]["name"], "dark");
    assert_eq!(load.providers[0].raw["customProviderField"], "keep-me");

    let backend = backends::backend(ConfigFormat::DeepSeekHarness);
    let output = backend
        .render(
            &backend.serialize_root(&[], &load.providers, &load.extras, None),
            false,
        )
        .unwrap();
    assert!(!output.contains("test-only-placeholder"));
    assert!(output.contains("apiKeyEnv"));

    std::fs::remove_file(&settings).ok();
    std::fs::remove_file(&credentials).ok();
}

#[test]
fn dsh_save_updates_one_ref_and_preserves_other_credentials() {
    let settings = temp_path("save-settings.yaml");
    let credentials = model_harbor::credentials::sidecar_path(settings.to_str().unwrap());
    std::fs::write(&settings, "llm-pi-ai:\n  providers: {}\n").unwrap();
    std::fs::write(
        &credentials,
        "version: 1\nrefs:\n  OTHER_KEY: untouched-placeholder\n  DSH_TEST_KEY: old-placeholder\nrecords:\n  keep: true\nunknown: value\n",
    )
    .unwrap();

    let mut provider = ProviderRow::new();
    provider.key = "demo".into();
    provider.api_key_env = "DSH_TEST_KEY".into();
    provider.api_key_secret = "new-placeholder".into();
    let backend = backends::backend(ConfigFormat::DeepSeekHarness);
    backend
        .save_sidecars(settings.to_str().unwrap(), &[provider])
        .unwrap();

    let saved = std::fs::read_to_string(&credentials).unwrap();
    let root = parse_yaml_content(&saved).unwrap();
    assert_eq!(root["refs"]["OTHER_KEY"], "untouched-placeholder");
    assert_eq!(root["refs"]["DSH_TEST_KEY"], "new-placeholder");
    assert_eq!(root["records"]["keep"], true);
    assert_eq!(root["unknown"], "value");
    assert!(!std::fs::read_to_string(&settings)
        .unwrap()
        .contains("new-placeholder"));

    std::fs::remove_file(&settings).ok();
    std::fs::remove_file(&credentials).ok();
}

#[test]
fn dsh_cross_format_save_uses_dsh_schema_without_foreign_keys() {
    let backend = backends::backend(ConfigFormat::DeepSeekHarness);
    let mut provider = ProviderRow::new();
    provider.key = "gateway".into();
    provider.npm = "@ai-sdk/anthropic".into();
    provider.pi_api = "anthropic-messages".into();
    provider.base_url = "https://gateway.example/v1".into();
    provider.dsh_timeout_ms = "180000".into();
    provider.dsh_retry_mode = "normal".into();
    provider.dsh_max_retries = "3".into();
    provider.api_key = "sk-must-not-enter-dsh-settings".into();
    let mut model = model_harbor::model::ModelRow::new();
    model.id = "m1".into();
    model.context = "128000".into();
    model.output = "8192".into();
    model.modalities_input = "text, image".into();
    model.variants = "high".into();
    provider.models.push(model);

    let root = backend.serialize_root(&[], &[provider], &json!({}), None);
    let saved = &root["llm-pi-ai"]["providers"]["gateway"];
    assert_eq!(saved["api"], "anthropic-messages");
    assert_eq!(saved["baseURL"], "https://gateway.example");
    assert_eq!(saved["timeoutMs"], 180000);
    assert_eq!(saved["retryPolicy"]["mode"], "normal");
    assert_eq!(saved["retryPolicy"]["maxRetries"], 3);
    assert!(saved.get("apiKey").is_none());
    assert!(saved.get("baseUrl").is_none());
    assert!(saved["models"][0].get("limit").is_none());
    assert!(saved["models"][0].get("modalities").is_none());
    assert!(saved["models"][0].get("variants").is_none());
    assert_eq!(
        saved["models"][0]["reasoningEfforts"],
        json!({"high": "high"})
    );
}

#[test]
fn dsh_renderer_matches_native_scalar_and_flow_style() {
    let backend = backends::backend(ConfigFormat::DeepSeekHarness);
    let load = backend
        .parse(
            r#"ui-theme:
  preference: system
llm-pi-ai:
  providers:
    demo:
      apiKeyEnv: DSH_TEST_KEY
      api: openai-completions
      baseURL: https://example.invalid/v1
      models:
        - id: demo-model
          name: Demo
          contextWindow: 1234
          maxTokens: 321
          input: [text]
          reasoningEfforts: {medium: medium}
"#,
        )
        .unwrap();
    let root = backend.serialize_root(&[], &load.providers, &load.extras, None);
    let output = backend.render(&root, false).unwrap();
    assert!(output.contains("apiKeyEnv: \"DSH_TEST_KEY\""));
    assert!(output.contains("api: \"openai-completions\""));
    assert!(output.contains("baseURL: \"https://example.invalid/v1\""));
    assert!(output.contains("- id: \"demo-model\""));
    assert!(output.contains("  name: \"Demo\""));
    assert!(output.contains("input: [ \"text\" ]"));
    assert!(output.contains("reasoningEfforts: { \"medium\": \"medium\" }"));
    assert!(!output.contains("ui-theme: {"));
}

#[test]
fn dsh_native_model_without_optional_fields_stays_without_them() {
    let backend = backends::backend(ConfigFormat::DeepSeekHarness);
    let mut provider = ProviderRow::new();
    provider.key = "demo".into();
    provider.source_format = Some(ConfigFormat::DeepSeekHarness);
    let mut model = model_harbor::model::ModelRow::new();
    model.id = "m1".into();
    model.source_format = Some(ConfigFormat::DeepSeekHarness);
    model.raw = json!({"id": "m1", "custom": {"keep": true}});
    model.original_variants = String::new();
    provider.models.push(model);
    let root = backend.serialize_root(&[], &[provider], &json!({}), None);
    let saved = &root["llm-pi-ai"]["providers"]["demo"]["models"][0];
    assert_eq!(saved["id"], "m1");
    assert!(saved.get("name").is_none());
    assert!(saved.get("input").is_none());
    assert!(saved.get("contextWindow").is_none());
    assert!(saved.get("maxTokens").is_none());
    assert!(saved.get("reasoningEfforts").is_none());
    assert_eq!(saved["custom"]["keep"], true);
}

#[test]
fn dsh_agent_default_model_preserved_as_unknown_field() {
    let backend = backends::backend(ConfigFormat::DeepSeekHarness);
    let extras = json!({
        "ui-theme": {"name": "dark"},
        "agent-default-model": {"provider": "old", "model": "old-model"},
        "llm-pi-ai": {"providers": {}}
    });
    let root = backend.serialize_root(&[], &[], &extras, None);
    assert_eq!(root["agent-default-model"]["provider"], "old");
    assert_eq!(root["agent-default-model"]["model"], "old-model");
    assert_eq!(root["ui-theme"]["name"], "dark");
}

#[test]
fn dsh_missing_timeout_defaults_to_180000_without_writeback() {
    let settings = temp_path("no_timeout.yaml");
    std::fs::write(
        &settings,
        "ui-theme:\n  name: dark\nllm-pi-ai:\n  providers:\n    demo:\n      api: openai-completions\n",
    )
    .unwrap();
    let load = backends::load_backend(ConfigFormat::DeepSeekHarness, settings.to_str().unwrap())
        .expect("DSH 配置应可加载");
    // 配置没有 timeoutMs 时默认显示 180000ms
    assert_eq!(load.providers[0].dsh_timeout_ms, "180000");
    // 未修改时保存不应写回 timeoutMs
    let backend = backends::backend(ConfigFormat::DeepSeekHarness);
    let root = backend.serialize_root(&[], &load.providers, &load.root, None);
    let demo = &root["llm-pi-ai"]["providers"]["demo"];
    assert!(demo.get("timeoutMs").is_none());
    assert!(demo.get("retryPolicy").is_none());
}
