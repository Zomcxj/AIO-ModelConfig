# Save Format Modes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a right-bottom save-format selector with the existing format and a compact format that groups Agent second-level fields and Provider Model third-level fields until each line exceeds 80 characters.

**Architecture:** Keep the existing save path as the current-format path. Add a separate compact formatter that operates on the assembled `serde_json::Value`, preserving the existing container hierarchy while formatting only Agent entries and Provider Model entries with field-level grouping. The compact formatter treats nested values such as `limit`, `options`, and `variants` as indivisible fields and serializes variant keys as empty objects (`"medium": {}`).

**Tech Stack:** Rust, eframe/egui, serde_json with preserve-order maps, existing `App` state and model `to_value()` methods.

## Global Constraints

- Keep the existing save format unchanged when the current-format option is selected.
- Compact mode groups Agent fields starting at the Agent object fields.
- Compact mode groups Provider Model fields starting at the Model object fields.
- A field is never split across lines; add the next field while the resulting line is at most 80 characters, and continue grouping until the next field would exceed 80 characters.
- Nested objects and arrays are complete field values for grouping purposes.
- Serialize configured variants in compact mode as empty objects, for example `"variants": { "medium": {}, "high": {} }`.
- Place the selector in the bottom-right status area.
- Do not commit or push without explicit user approval.

---

### Task 1: Add Save-Format State and Bottom-Right Selector

**Files:**
- Modify: `src/app.rs` (`App` fields, `Default`, status-bar UI, save path)
- Test: `tests/model_io.rs` only if state-independent helpers need coverage

**Interfaces:**
- Produces a `SaveFormat` enum with `Current` and `Compact` variants.
- Produces a bottom-right egui selector that updates `App.save_format` without changing the loaded configuration.

- [ ] **Step 1: Add the enum and default state**

Add a small `SaveFormat` enum near the `App` definition:

```rust
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum SaveFormat {
    #[default]
    Current,
    Compact,
}

impl SaveFormat {
    fn label(self) -> &'static str {
        match self {
            Self::Current => "当前格式",
            Self::Compact => "压缩格式",
        }
    }
}
```

Add `save_format: SaveFormat` to `App` and initialize it to `SaveFormat::Current` in `Default`.

- [ ] **Step 2: Add the bottom-right selector**

In the existing bottom status-bar layout, keep the status text on the left and add a right-aligned `egui::ComboBox` labeled `保存格式`. Selecting an item only updates `self.save_format`; it must not trigger a save automatically.

- [ ] **Step 3: Run the existing tests**

Run:

```text
cargo test --release
```

Expected: all existing tests pass; no save behavior has changed yet.

---

### Task 2: Extract Compact Field Serialization Helpers

**Files:**
- Modify: `src/app.rs` near the existing save helpers
- Test: `tests/model_io.rs` or a new `tests/save_format.rs`

**Interfaces:**
- `compact_json_value(value: &Value) -> String`
- `compact_field_line(fields: &[(&str, String)], indent: &str) -> Vec<String>`
- `compact_object_entry(key: &str, value: &Value, indent: &str, level: usize) -> Vec<String>`

- [ ] **Step 1: Add failing formatter tests**

Add tests covering the exact grouping rule:

```rust
#[test]
fn compact_agent_fields_group_until_next_field_would_exceed_80() {
    let value = serde_json::json!({
        "mode": "subagent",
        "description": "short description",
        "model": "provider/model",
        "variant": "high"
    });
    let lines = compact_agent_object(&value, "  ");
    assert!(lines.iter().any(|line| line.contains("mode") && line.contains("description")));
    assert!(lines.iter().all(|line| line.chars().count() > 80 || line == lines.last().unwrap()));
}

#[test]
fn compact_variants_use_empty_objects() {
    let value = serde_json::json!({
        "variants": {
            "medium": { "reasoningEffort": "medium" },
            "high": { "reasoningEffort": "high" }
        }
    });
    let text = compact_provider_model_object(&value, "  ").join("\n");
    assert!(text.contains("\"variants\": { \"medium\": {}, \"high\": {} }"));
    assert!(!text.contains("reasoningEffort"));
}
```

- [ ] **Step 2: Run the focused tests and verify failure**

Run:

```text
cargo test --release compact_agent_fields_group_until_next_field_would_exceed_80 compact_variants_use_empty_objects
```

Expected: FAIL because the compact formatter does not exist yet.

- [ ] **Step 3: Implement scalar and nested-field serialization**

Serialize each field as one complete JSON field. For nested objects, recursively serialize the object on one line when it is a field value. For arrays, serialize the full array as one field value. For `variants`, emit each variant key with `{}` regardless of the source variant object contents.

- [ ] **Step 4: Implement 80-character grouping**

Build each output line from consecutive complete fields. If adding the next field would make the line longer than 80 characters, flush the current line and start the next one with that field. The first line may be shorter than 80 if the next field would exceed the threshold; never split a field.

- [ ] **Step 5: Run the focused tests**

Run the same focused command. Expected: PASS.

---

### Task 3: Format Only Agent and Provider Model Entries

**Files:**
- Modify: `src/app.rs` compact root formatter and save dispatch
- Test: `tests/save_format.rs`

**Interfaces:**
- `compact_root_json(root: &Value, indent: &str) -> String`
- `compact_agent_object(value: &Value, indent: &str) -> Vec<String>`
- `compact_provider_model_object(value: &Value, indent: &str) -> Vec<String>`

- [ ] **Step 1: Add structural tests**

Test that compact mode changes only the requested levels:

```rust
#[test]
fn compact_mode_keeps_root_and_containers_structural() {
    let root = serde_json::json!({
        "agent": { "writer": { "mode": "subagent", "description": "text", "model": "p/m" } },
        "provider": { "p": { "models": { "m": { "name": "M", "variants": { "high": {} } } } } }
    });
    let text = compact_root_json(&root, "  ");
    assert!(text.contains("\"agent\": {"));
    assert!(text.contains("\"provider\": {"));
    assert!(text.contains("\"writer\": {"));
    assert!(text.contains("\"m\": {"));
}
```

- [ ] **Step 2: Implement Agent formatting**

Walk the root `agent` object and format each agent entry using the Agent field grouping helper. Keep the `agent` container and each agent key at their existing structural levels.

- [ ] **Step 3: Implement Provider Model formatting**

Walk the root `provider` object, then each provider's `models` object, and format each model entry using the same complete-field grouping rule. Keep provider fields and model containers structural; do not compact unrelated root/provider fields.

- [ ] **Step 4: Preserve fields not represented by the UI**

Start from the assembled root `Value` so existing top-level configuration remains present. Ensure missing `agent` or `provider` sections are handled without panics.

- [ ] **Step 5: Run save-format tests**

Run:

```text
cargo test --release save_format
```

Expected: PASS.

---

### Task 4: Connect Both Save Modes

**Files:**
- Modify: `src/app.rs` `save()` dispatch
- Test: `tests/save_format.rs`

**Interfaces:**
- Current mode continues using the existing current-format serializer.
- Compact mode calls `compact_root_json`.

- [ ] **Step 1: Add mode-selection tests**

Verify current mode and compact mode produce valid JSON and that current mode remains unchanged from the pre-feature path.

- [ ] **Step 2: Dispatch in `save()`**

Use:

```rust
let content = match self.save_format {
    SaveFormat::Current => existing_current_format_output(&root, &orig),
    SaveFormat::Compact => compact_root_json(&root, &detect_indent(&orig)),
};
```

Keep existing file-writing and WSL handling unchanged.

- [ ] **Step 3: Validate JSON parsing after both modes**

Parse each generated string with `serde_json::from_str::<Value>()` and assert success in tests.

- [ ] **Step 4: Run the complete test suite**

Run:

```text
cargo test --release
cargo build --release
```

Expected: all tests pass and release build completes successfully.

---

### Task 5: Manual UI Verification

**Files:**
- No source changes expected

- [ ] **Step 1: Start the release executable**

Run:

```text
target/release/opencode-model-config.exe
```

- [ ] **Step 2: Verify the bottom-right selector**

Confirm it displays `当前格式` by default and allows selecting `压缩格式` without changing the loaded data.

- [ ] **Step 3: Verify compact save output**

Load `C:\Users\cxj\Desktop\opencode.json`, choose `压缩格式`, save to a copy, parse the copy as JSON, and inspect that Agent fields and Provider Model fields are grouped by the 80-character rule.

- [ ] **Step 4: Verify variants**

Confirm output uses:

```json
"variants": { "medium": {}, "high": {} }
```

and never writes `reasoningEffort` for these variant entries.

- [ ] **Step 5: Verify current format remains available**

Choose `当前格式`, save another copy, and confirm it follows the existing current-format behavior rather than compact-mode grouping.

---

## Verification Summary

The implementation is complete only when the following pass:

```text
cargo test --release
cargo build --release
```

and both save modes produce valid JSON, with compact mode grouping only Agent second-level fields and Provider Model third-level fields according to the 80-character rule.
