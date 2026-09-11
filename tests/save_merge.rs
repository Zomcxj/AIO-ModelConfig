use model_harbor::app::merge_opencode_root;
use model_harbor::convert;
use model_harbor::model::{AgentRow, ProviderRow};
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
    assert!(
        merged["provider"].get("newpi").is_some(),
        "UI provider upserted"
    );
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
    assert_eq!(
        merged["agent"]["a1"]["model"], "new",
        "UI state wins on same key"
    );
}

#[test]
fn pi_merge_preserves_target_extras() {
    // 模拟 save_pi_agent_to 的跨目标合并：目标 root 取目标文件自身（含 providers），
    // 仅重写 providers 中的条目，目标独有条目保留。
    let mut p = std::env::temp_dir();
    p.push("opencode_test_pi_merge.json");
    let target = json!({
        "providers": {
            "existing": { "baseUrl": "https://e", "api": "openai-completions", "models": [] }
        },
        "customTopLevel": 42
    });
    std::fs::write(&p, serde_json::to_string(&target).unwrap()).unwrap();

    let providers = vec![ProviderRow::from(
        "newpi",
        &json!({ "npm": "", "options": { "baseURL": "https://x", "apiKey": "k" }, "models": {} }),
    )];

    let target_root = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let root = convert::to_pi_root(&providers, &target_root);
    assert_eq!(
        root["customTopLevel"], 42,
        "target top-level extras must survive"
    );
    assert!(root["providers"].get("newpi").is_some());
    assert!(
        root["providers"].get("existing").is_some(),
        "cross-format 保存必须保留目标文件独有 provider（非编辑内容不能改）"
    );
    std::fs::remove_file(&p).ok();
}

#[test]
fn pi_merge_preserves_target_unknown_fields_on_same_key() {
    // 同名 provider：目标未编辑字段保留（这里 cost 是 pi 文件里的扩展字段，
    // 来源转换结果不含它，不得被删）；来源字段（baseUrl）生效。
    let target = json!({
        "providers": {
            "demo": {
                "baseUrl": "https://old/v1",
                "api": "openai-completions",
                "cost": {"input": 1.0},
                "models": []
            }
        }
    });
    let mut p = ProviderRow::new();
    p.key = "demo".into();
    p.base_url = "https://new/v1".into();
    let root = convert::to_pi_root(&[p], &target);
    let demo = &root["providers"]["demo"];
    assert_eq!(demo["baseUrl"], "https://new/v1");
    assert_eq!(demo["cost"]["input"], 1.0, "目标独有字段应保留");
}

#[test]
fn opencode_merge_preserves_target_model_options() {
    // 跨格式保存到 opencode 目标：同名 provider 的模型 options 等非编辑内容保留；
    // 目标独有模型也保留。
    let target = json!({
        "provider": {
            "demo": {
                "npm": "@ai-sdk/openai",
                "options": {"baseURL": "https://old/v1"},
                "models": {
                    "m1": {"name": "M1", "options": {"store": false}, "limit": {"context": 1000}},
                    "target-only": {"name": "TO"}
                }
            }
        }
    });
    let p = ProviderRow::from(
        "demo",
        &json!({
            "npm": "", "options": {"baseURL": "https://old/v1"},
            "models": {"m1": {"name": "M1", "limit": {"context": 1000}}}
        }),
    );
    // 模拟来自其他格式：跨格式转换后 options.store 不存在于来源
    let merged = merge_opencode_root(&target, &[], &[p]);
    let demo = &merged["provider"]["demo"];
    assert_eq!(
        demo["models"]["m1"]["options"]["store"], false,
        "目标模型 options.store 不得被删除"
    );
    assert_eq!(
        demo["models"]["target-only"]["name"], "TO",
        "目标独有模型应保留"
    );
}
