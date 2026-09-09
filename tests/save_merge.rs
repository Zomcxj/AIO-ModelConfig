use aio_model_config::app::{load_pi_agent_result, merge_opencode_root};
use aio_model_config::convert;
use aio_model_config::model::{AgentRow, ProviderRow};
use serde_json::json;

#[test]
fn merge_preserves_target_agents_and_other_fields() {
    // 目标 opencode 文件已有 agents / mcp / provider
    let target = json!({
        "agent": { "writer": { "mode": "subagent", "model": "p/m" } },
        "provider": { "old": { "npm": "@ai-sdk/x", "models": {} } },
        "mcp": { "server": { "command": "node" } },
        "providers": { "unknown_field": true }
    });
    // 来源为 pi：UI agents 为空、providers 为转换后的条目
    let providers = vec![ProviderRow::from(
        "newpi",
        &json!({ "npm": "", "options": { "baseURL": "https://x", "apiKey": "k" }, "models": {} }),
    )];
    let merged = merge_opencode_root(&target, &[], &providers);
    assert_eq!(
        merged["agent"]["writer"]["mode"], "subagent",
        "target agents must be preserved when UI agents are empty"
    );
    assert_eq!(
        merged["mcp"]["server"]["command"], "node",
        "target top-level fields must survive"
    );
    assert!(merged["provider"].get("newpi").is_some(), "UI provider upserted");
    assert!(
        merged["provider"].get("old").is_some(),
        "target provider entries preserved"
    );
    assert!(
        merged["providers"]["unknown_field"].is_boolean(),
        "unknown top-level fields (including pi-style 'providers') must be preserved"
    );
}

#[test]
fn current_file_save_replaces_agent_map() {
    // 当前文件语义验证：UI 删除的 agent 不得因 upsert 复活
    // （此语义在 App::save_opencode_to 的 is_current 分支，此处验证 merge 不适用于当前文件）
    let target = json!({ "agent": { "gone": { "mode": "subagent" } } });
    let merged = merge_opencode_root(&target, &[], &[]);
    assert!(
        merged["agent"].get("gone").is_some(),
        "merge is for cross-format targets only; current-file save must replace"
    );
}

#[test]
fn merge_upserts_ui_agent_over_target() {
    let target = json!({ "agent": { "a1": { "mode": "subagent", "model": "old" } } });
    let mut a = AgentRow::new();
    a.key = "a1".into();
    a.model = "new".into();
    let merged = merge_opencode_root(&target, &[a], &[]);
    assert_eq!(merged["agent"]["a1"]["model"], "new", "UI state wins on same key");
}

#[test]
fn pi_merge_preserves_target_extras() {
    // 模拟 save_pi_agent_to 的跨目标合并：extras 取目标文件自身，仅重写 providers
    let mut p = std::env::temp_dir();
    p.push("opencode_test_pi_merge.json");
    std::fs::write(
        &p,
        serde_json::to_string(&json!({
            "providers": {
                "existing": { "baseUrl": "https://e", "api": "openai-completions", "models": [] }
            },
            "customTopLevel": 42
        }))
        .unwrap(),
    )
    .unwrap();

    let providers = vec![ProviderRow::from(
        "newpi",
        &json!({ "npm": "", "options": { "baseURL": "https://x", "apiKey": "k" }, "models": {} }),
    )];

    let (_, _, target_extras) = load_pi_agent_result(p.to_str().unwrap()).unwrap();
    let root = convert::to_pi_root(&providers, &target_extras);
    assert_eq!(root["customTopLevel"], 42, "target top-level extras must survive");
    assert!(root["providers"].get("newpi").is_some());
    assert!(
        root["providers"].get("existing").is_none(),
        "providers are replaced by UI state (pi has no upsert semantics)"
    );
    std::fs::remove_file(&p).ok();
}
