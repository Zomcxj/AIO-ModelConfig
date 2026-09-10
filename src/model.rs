use crate::format::ConfigFormat;
use crate::util::{
    bool_at, nested_list_str, nested_num, nested_str, num_at, parse_number_text, set_num_opt,
    set_str, str_at,
};
use serde_json::{Map, Value};

fn changed_str(raw: &Value, key: &str, current: &str) -> bool {
    current != str_at(raw, key)
}

fn changed_num(raw: &Value, key: &str, current: &str) -> bool {
    raw.get(key)
        .and_then(Value::as_str)
        .map(|value| value != current)
        .unwrap_or_else(|| current != num_at(raw, key))
}

fn changed_nested_num(raw: &Value, path: &[&str], current: &str) -> bool {
    let mut value = raw;
    for key in path {
        let Some(next) = value.get(*key) else {
            return !current.is_empty();
        };
        value = next;
    }
    value
        .as_str()
        .map(|original| original != current)
        .unwrap_or_else(|| current != nested_num(raw, path))
}

fn changed_nested_str(raw: &Value, path: &[&str], current: &str) -> bool {
    current != nested_str(raw, path)
}

fn changed_list(raw: &Value, path: &[&str], current: &str) -> bool {
    current != nested_list_str(raw, path)
}

fn changed_bool(raw: &Value, key: &str, current: bool) -> bool {
    raw.get(key).is_some() && current != bool_at(raw, key) || raw.get(key).is_none() && current
}

fn set_changed_str(m: &mut Map<String, Value>, raw: &Value, key: &str, current: &str) {
    if changed_str(raw, key, current) {
        set_str(m, key, current);
    }
}

#[derive(Clone)]
pub struct AgentRow {
    pub key: String,
    pub mode: String,
    pub description: String,
    pub model: String,
    pub variant: String,
    pub temperature: String,
    pub color: String,
    pub system: String,
    pub raw: Value,
}

impl Default for AgentRow {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentRow {
    pub fn from(key: &str, v: &Value) -> Self {
        let row = Self {
            key: key.to_string(),
            mode: str_at(v, "mode").to_string(),
            description: str_at(v, "description").to_string(),
            model: str_at(v, "model").to_string(),
            variant: str_at(v, "variant").to_string(),
            temperature: num_at(v, "temperature"),
            color: str_at(v, "color").to_string(),
            system: str_at(v, "system").to_string(),
            raw: v.clone(),
        };
        row
    }

    pub fn new() -> Self {
        Self {
            key: String::new(),
            mode: "subagent".into(),
            description: String::new(),
            model: String::new(),
            variant: String::new(),
            temperature: String::new(),
            color: String::new(),
            system: String::new(),
            raw: Value::Object(Map::new()),
        }
    }

    pub fn to_value(&self) -> Value {
        let mut m = self.raw.as_object().cloned().unwrap_or_default();
        // 仅在 UI 值相对原始值发生变化时写入；未修改字段保留原始键、类型和内容。
        set_changed_str(&mut m, &self.raw, "mode", &self.mode);
        set_changed_str(&mut m, &self.raw, "description", &self.description);
        set_changed_str(&mut m, &self.raw, "model", &self.model);
        set_changed_str(&mut m, &self.raw, "variant", &self.variant);
        set_changed_str(&mut m, &self.raw, "color", &self.color);
        set_changed_str(&mut m, &self.raw, "system", &self.system);
        if changed_num(&self.raw, "temperature", &self.temperature) {
            match parse_number_text(&self.temperature) {
                Some(v) => {
                    m.insert("temperature".into(), v);
                }
                None => {
                    m.remove("temperature");
                }
            }
        }
        Value::Object(m)
    }
}

#[derive(Clone)]
pub struct ModelRow {
    pub id: String,
    pub name: String,
    pub reasoning: bool,
    pub tool_call: bool,
    pub store: bool,
    pub context: String,
    pub output: String,
    pub modalities_input: String,
    pub modalities_output: String,
    pub variants: String,
    /// 加载时的思考档位投影，用于区分跨格式继承值与用户在目标页的手动输入。
    pub original_variants: String,
    /// raw 所属格式；None 表示在当前页面中新建的条目。
    pub source_format: Option<ConfigFormat>,
    pub raw: Value,
}

impl Default for ModelRow {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelRow {
    pub fn from(id: &str, v: &Value) -> Self {
        let variants = v
            .get("variants")
            .map(|val| {
                if let Some(obj) = val.as_object() {
                    obj.keys().cloned().collect::<Vec<_>>().join(", ")
                } else {
                    String::new()
                }
            })
            .unwrap_or_default();
        let store = v
            .get("options")
            .and_then(|o| o.get("store"))
            .and_then(|s| s.as_bool())
            .unwrap_or(false);
        Self {
            id: id.to_string(),
            name: str_at(v, "name").to_string(),
            reasoning: bool_at(v, "reasoning"),
            tool_call: bool_at(v, "tool_call"),
            store,
            context: nested_num(v, &["limit", "context"]),
            output: nested_num(v, &["limit", "output"]),
            modalities_input: nested_list_str(v, &["modalities", "input"]),
            modalities_output: nested_list_str(v, &["modalities", "output"]),
            original_variants: variants.clone(),
            variants,
            source_format: Some(ConfigFormat::Opencode),
            raw: v.clone(),
        }
    }

    pub fn new() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            reasoning: false,
            tool_call: false,
            store: false,
            context: String::new(),
            output: String::new(),
            modalities_input: String::new(),
            modalities_output: String::new(),
            variants: String::new(),
            original_variants: String::new(),
            source_format: None,
            raw: Value::Object(Map::new()),
        }
    }

    pub fn to_value(&self) -> Value {
        // pi/omp/DSH 来源需要转换方言，因此从干净对象构造；opencode 来源
        // 则以 raw 为基底，并只更新 UI 实际改动过的字段。
        let convert_dialect = self
            .source_format
            .is_some_and(|format| format != ConfigFormat::Opencode);
        let mut m = if convert_dialect {
            Map::new()
        } else {
            self.raw.as_object().cloned().unwrap_or_default()
        };

        if convert_dialect {
            set_str(&mut m, "name", &self.name);
            // reasoning/tool_call 是 opencode 专属控件。跨格式来源不继承默认值，
            // 但用户在 opencode 页明确勾选后仍可写入。
            if self.reasoning {
                m.insert("reasoning".into(), true.into());
            }
            if self.tool_call {
                m.insert("tool_call".into(), true.into());
            }
        } else {
            set_changed_str(&mut m, &self.raw, "name", &self.name);
            if changed_bool(&self.raw, "reasoning", self.reasoning) {
                m.insert("reasoning".into(), self.reasoning.into());
            }
            if changed_bool(&self.raw, "tool_call", self.tool_call) {
                m.insert("tool_call".into(), self.tool_call.into());
            }
        }

        let raw_store = self
            .raw
            .get("options")
            .and_then(|o| o.get("store"))
            .and_then(Value::as_bool);
        if convert_dialect || raw_store != Some(self.store) {
            if self.store {
                let mut options = m
                    .get("options")
                    .and_then(|o| o.as_object())
                    .cloned()
                    .unwrap_or_default();
                options.insert("store".into(), true.into());
                m.insert("options".into(), Value::Object(options));
            } else if let Some(options) = m.get_mut("options").and_then(Value::as_object_mut) {
                options.remove("store");
                if options.is_empty() {
                    m.remove("options");
                }
            }
        }

        let context_changed =
            convert_dialect || changed_nested_num(&self.raw, &["limit", "context"], &self.context);
        let output_changed =
            convert_dialect || changed_nested_num(&self.raw, &["limit", "output"], &self.output);
        if context_changed || output_changed {
            let mut limit = m
                .get("limit")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            if context_changed {
                set_num_opt(&mut limit, "context", &self.context);
            }
            if output_changed {
                set_num_opt(&mut limit, "output", &self.output);
            }
            if limit.is_empty() {
                m.remove("limit");
            } else {
                m.insert("limit".into(), Value::Object(limit));
            }
        }

        let input_changed = convert_dialect
            || changed_list(&self.raw, &["modalities", "input"], &self.modalities_input);
        let output_changed = convert_dialect
            || changed_list(
                &self.raw,
                &["modalities", "output"],
                &self.modalities_output,
            );
        if input_changed || output_changed {
            let mut modalities = m
                .get("modalities")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            if input_changed {
                let values: Vec<Value> = self
                    .modalities_input
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|value| Value::String(value.to_string()))
                    .collect();
                if values.is_empty() {
                    modalities.remove("input");
                } else {
                    modalities.insert("input".into(), Value::Array(values));
                }
            }
            if output_changed {
                let values: Vec<Value> = self
                    .modalities_output
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|value| Value::String(value.to_string()))
                    .collect();
                if values.is_empty() {
                    modalities.remove("output");
                } else {
                    modalities.insert("output".into(), Value::Array(values));
                }
            }
            if modalities.is_empty() {
                m.remove("modalities");
            } else {
                m.insert("modalities".into(), Value::Object(modalities));
            }
        }

        let raw_variants = self.raw.get("variants").and_then(Value::as_object);
        let raw_variant_names = raw_variants
            .map(|v| v.keys().cloned().collect::<Vec<_>>().join(", "))
            .unwrap_or_default();
        if convert_dialect || self.variants != raw_variant_names {
            if self.variants.trim().is_empty() {
                m.remove("variants");
            } else {
                let raw_variants = m
                    .get("variants")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                let variants_map: Map<String, Value> = self
                    .variants
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|name| {
                        let value = raw_variants
                            .get(name)
                            .cloned()
                            .unwrap_or_else(|| Value::Object(Map::new()));
                        (name.to_string(), value)
                    })
                    .collect();
                m.insert("variants".into(), Value::Object(variants_map));
            }
        }
        Value::Object(m)
    }
}

#[derive(Clone)]
pub struct ProviderRow {
    pub key: String,
    pub description: String,
    pub npm: String,
    pub base_url: String,
    pub api_key: String,
    /// DSH 中保存于主配置的凭据引用名（apiKeyEnv）。
    pub api_key_env: String,
    /// 加载时的 apiKeyEnv，用于清理重命名后的旧 ref。
    pub original_api_key_env: String,
    /// DSH 同级 `.credentials.yaml` 中 refs 下的实际密钥。
    pub api_key_secret: String,
    /// 加载时的密钥，用于区分“原本缺失”与“用户明确清空”。
    pub original_api_key_secret: String,
    /// DSH provider 的 timeoutMs。
    pub dsh_timeout_ms: String,
    /// DSH provider 的 retryPolicy.mode。
    pub dsh_retry_mode: String,
    /// DSH provider 的 retryPolicy.maxRetries。
    pub dsh_max_retries: String,
    /// 加载时的 DSH provider 参数，用于只保存用户实际修改的字段。
    pub original_dsh_timeout_ms: String,
    pub original_dsh_retry_mode: String,
    pub original_dsh_max_retries: String,
    pub timeout: String,
    pub compat: bool,
    pub models: Vec<ModelRow>,
    pub new_model: ModelRow,
    /// raw 所属格式；None 表示在当前页面中新建的条目。
    pub source_format: Option<ConfigFormat>,
    pub raw: Value,
    pub pi_api: String,
}

impl Default for ProviderRow {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderRow {
    pub fn from(key: &str, v: &Value) -> Self {
        let models = v
            .get("models")
            .and_then(|x| x.as_object())
            .map(|o| o.iter().map(|(k, mv)| ModelRow::from(k, mv)).collect())
            .unwrap_or_default();
        let compat = v
            .get("compat")
            .and_then(|c| c.get("supportsDeveloperRole"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        Self {
            key: key.to_string(),
            description: str_at(v, "description").to_string(),
            npm: str_at(v, "npm").to_string(),
            base_url: nested_str(v, &["options", "baseURL"]).to_string(),
            api_key: nested_str(v, &["options", "apiKey"]).to_string(),
            api_key_env: String::new(),
            original_api_key_env: String::new(),
            api_key_secret: String::new(),
            original_api_key_secret: String::new(),
            dsh_timeout_ms: String::new(),
            dsh_retry_mode: "normal".into(),
            dsh_max_retries: String::new(),
            original_dsh_timeout_ms: String::new(),
            original_dsh_retry_mode: "normal".into(),
            original_dsh_max_retries: String::new(),
            timeout: nested_num(v, &["options", "timeout"]),
            compat,
            models,
            new_model: ModelRow::new(),
            source_format: Some(ConfigFormat::Opencode),
            raw: v.clone(),
            pi_api: String::new(),
        }
    }

    pub fn new() -> Self {
        Self {
            key: String::new(),
            description: String::new(),
            npm: String::new(),
            base_url: String::new(),
            api_key: String::new(),
            api_key_env: String::new(),
            original_api_key_env: String::new(),
            api_key_secret: String::new(),
            original_api_key_secret: String::new(),
            dsh_timeout_ms: String::new(),
            dsh_retry_mode: "normal".into(),
            dsh_max_retries: String::new(),
            original_dsh_timeout_ms: String::new(),
            original_dsh_retry_mode: "normal".into(),
            original_dsh_max_retries: String::new(),
            timeout: String::new(),
            compat: true,
            models: Vec::new(),
            new_model: ModelRow::new(),
            source_format: None,
            raw: Value::Object(Map::new()),
            pi_api: String::new(),
        }
    }

    pub fn to_value(&self) -> Value {
        // pi/omp/DSH 来源全新构造，防止方言键泄漏进 opencode；opencode
        // 来源则以 raw 为基底，只更新发生变化的 provider 字段。
        let convert_dialect = self
            .source_format
            .is_some_and(|format| format != ConfigFormat::Opencode);
        let mut m = if convert_dialect {
            Map::new()
        } else {
            self.raw.as_object().cloned().unwrap_or_default()
        };
        if convert_dialect {
            // opencode 专属字段只在用户于 opencode 页明确填写时创建。
            set_str(&mut m, "description", &self.description);
            set_str(&mut m, "npm", &self.npm);
        } else {
            set_changed_str(&mut m, &self.raw, "description", &self.description);
            set_changed_str(&mut m, &self.raw, "npm", &self.npm);
        }
        let base_changed = convert_dialect
            || changed_nested_str(&self.raw, &["options", "baseURL"], &self.base_url);
        let key_changed =
            convert_dialect || changed_nested_str(&self.raw, &["options", "apiKey"], &self.api_key);
        let timeout_changed = convert_dialect
            || changed_nested_num(&self.raw, &["options", "timeout"], &self.timeout);
        if base_changed || key_changed || timeout_changed {
            let mut options = m
                .get("options")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            if base_changed {
                set_str(&mut options, "baseURL", &self.base_url);
            }
            if key_changed {
                set_str(&mut options, "apiKey", &self.api_key);
            }
            if timeout_changed {
                set_num_opt(&mut options, "timeout", &self.timeout);
            }
            if options.is_empty() {
                m.remove("options");
            } else {
                m.insert("options".into(), Value::Object(options));
            }
        }
        let mut models = Map::new();
        for mdl in &self.models {
            models.insert(mdl.id.clone(), mdl.to_value());
        }
        m.insert("models".into(), Value::Object(models));
        Value::Object(m)
    }
}
