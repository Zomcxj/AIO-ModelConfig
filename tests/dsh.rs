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
    let dir = std::env::temp_dir().join(format!(
        "model_harbor_dsh_{}_{}",
        std::process::id(),
        nonce
    ));
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
    assert_eq!(load.providers[0].models[0].variants, "medium");
    assert_eq!(
        load.default_model,
        Some(("demo".to_string(), "demo-model".to_string()))
    );
    assert_eq!(load.root["ui-theme"]["name"], "dark");
    assert_eq!(load.providers[0].raw["customProviderField"], "keep-me");

    let backend = backends::backend(ConfigFormat::DeepSeekHarness);
    let output = backend.render(
        &backend.serialize_root(&[], &load.providers, &load.extras, None),
        false,
    ).unwrap();
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
    backend.save_sidecars(settings.to_str().unwrap(), &[provider]).unwrap();

    let saved = std::fs::read_to_string(&credentials).unwrap();
    let root = parse_yaml_content(&saved).unwrap();
    assert_eq!(root["refs"]["OTHER_KEY"], "untouched-placeholder");
    assert_eq!(root["refs"]["DSH_TEST_KEY"], "new-placeholder");
    assert_eq!(root["records"]["keep"], true);
    assert_eq!(root["unknown"], "value");
    assert!(!std::fs::read_to_string(&settings).unwrap().contains("new-placeholder"));

    std::fs::remove_file(&settings).ok();
    std::fs::remove_file(&credentials).ok();
}

#[test]
fn dsh_default_model_is_serialized_only_when_explicitly_changed() {
    let backend = backends::backend(ConfigFormat::DeepSeekHarness);
    let extras = json!({
        "ui-theme": {"name": "dark"},
        "agent-default-model": {"provider": "old", "model": "old-model"}
    });
    let providers = vec![];
    let unchanged = backend.serialize_root_with_default(&[], &providers, &extras, None, None);
    assert_eq!(unchanged["agent-default-model"]["provider"], "old");

    let selected = ("demo".to_string(), "demo-model".to_string());
    let changed = backend.serialize_root_with_default(
        &[],
        &providers,
        &extras,
        None,
        Some(&Some(selected)),
    );
    assert_eq!(changed["agent-default-model"]["provider"], "demo");
    assert_eq!(changed["ui-theme"]["name"], "dark");
}
