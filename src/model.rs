use crate::util::{
    bool_at, nested_list_str, nested_num, nested_str, num_at, parse_number_text, set_num_opt,
    set_str, str_at,
};
use serde_json::{Map, Value};

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
    pub haystack: String,
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
            haystack: String::new(),
        };
        let mut row = row;
        row.refresh_haystack();
        row
    }

    pub fn new() -> Self {
        let mut r = Self {
            key: String::new(),
            mode: "subagent".into(),
            description: String::new(),
            model: String::new(),
            variant: String::new(),
            temperature: String::new(),
            color: String::new(),
            system: String::new(),
            raw: Value::Object(Map::new()),
            haystack: String::new(),
        };
        r.refresh_haystack();
        r
    }

    pub fn refresh_haystack(&mut self) {
        let mut s = String::with_capacity(
            self.key.len() + self.description.len() + self.model.len() + self.mode.len() + 4,
        );
        s.push_str(&self.key);
        s.push(' ');
        s.push_str(&self.mode);
        s.push(' ');
        s.push_str(&self.description);
        s.push(' ');
        s.push_str(&self.model);
        self.haystack = s.to_lowercase();
    }

    pub fn to_value(&self) -> Value {
        let mut m = self.raw.as_object().cloned().unwrap_or_default();
        set_str(&mut m, "mode", &self.mode);
        set_str(&mut m, "description", &self.description);
        set_str(&mut m, "model", &self.model);
        set_str(&mut m, "variant", &self.variant);
        set_str(&mut m, "color", &self.color);
        set_str(&mut m, "system", &self.system);
        match parse_number_text(&self.temperature) {
            Some(v) => {
                m.insert("temperature".into(), v);
            }
            None => {
                m.remove("temperature");
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
            variants,
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
            raw: Value::Object(Map::new()),
        }
    }

    pub fn to_value(&self) -> Value {
        let mut m = self.raw.as_object().cloned().unwrap_or_default();
        if self.name.trim().is_empty() {
            m.remove("name");
        } else {
            set_str(&mut m, "name", &self.name);
        }
        m.insert("reasoning".into(), self.reasoning.into());
        m.insert("tool_call".into(), self.tool_call.into());
        // 仅 store=true 时写入；false 时从 raw 移除 store，options 空则整体省略
        if self.store {
            let mut options = m
                .get("options")
                .and_then(|o| o.as_object())
                .cloned()
                .unwrap_or_default();
            options.insert("store".into(), true.into());
            m.insert("options".into(), Value::Object(options));
        } else if let Some(options) = m.get_mut("options").and_then(|o| o.as_object_mut()) {
            options.remove("store");
            if options.is_empty() {
                m.remove("options");
            }
        }
        let mut limit = m
            .get("limit")
            .and_then(|l| l.as_object())
            .cloned()
            .unwrap_or_default();
        set_num_opt(&mut limit, "context", &self.context);
        set_num_opt(&mut limit, "output", &self.output);
        m.insert("limit".into(), Value::Object(limit));
        let mod_input: Vec<Value> = self
            .modalities_input
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.into())
            .collect();
        let mod_output: Vec<Value> = self
            .modalities_output
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.into())
            .collect();
        // input / output 独立处理：单侧为空只删对应子键，两侧皆空才删整个字段
        if !mod_input.is_empty() || !mod_output.is_empty() {
            let mut mo = m
                .get("modalities")
                .and_then(|x| x.as_object())
                .cloned()
                .unwrap_or_default();
            if !mod_input.is_empty() {
                mo.insert("input".into(), Value::Array(mod_input));
            } else {
                mo.remove("input");
            }
            if !mod_output.is_empty() {
                mo.insert("output".into(), Value::Array(mod_output));
            } else {
                mo.remove("output");
            }
            m.insert("modalities".into(), Value::Object(mo));
        } else {
            m.remove("modalities");
        }
        if self.variants.trim().is_empty() {
            m.remove("variants");
        } else {
            // 保留 raw 中已有 variant 的原始值（如 reasoningEffort），新选档位默认空对象
            let raw_variants = m
                .get("variants")
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();
            let variants_map: Map<String, Value> = self
                .variants
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|name| {
                    let val = raw_variants
                        .get(name)
                        .cloned()
                        .unwrap_or_else(|| Value::Object(Map::new()));
                    (name.to_string(), val)
                })
                .collect();
            if !variants_map.is_empty() {
                m.insert("variants".into(), Value::Object(variants_map));
            } else {
                m.remove("variants");
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
    pub timeout: String,
    pub compat: bool,
    pub models: Vec<ModelRow>,
    pub new_model: ModelRow,
    pub raw: Value,
    pub pi_api: String,
    pub haystack: String,
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
        let mut r = Self {
            key: key.to_string(),
            description: str_at(v, "description").to_string(),
            npm: str_at(v, "npm").to_string(),
            base_url: nested_str(v, &["options", "baseURL"]).to_string(),
            api_key: nested_str(v, &["options", "apiKey"]).to_string(),
            timeout: nested_num(v, &["options", "timeout"]),
            compat,
            models,
            new_model: ModelRow::new(),
            raw: v.clone(),
            pi_api: String::new(),
            haystack: String::new(),
        };
        r.refresh_haystack();
        r
    }

    pub fn new() -> Self {
        let mut r = Self {
            key: String::new(),
            description: String::new(),
            npm: String::new(),
            base_url: String::new(),
            api_key: String::new(),
            timeout: String::new(),
            compat: true,
            models: Vec::new(),
            new_model: ModelRow::new(),
            raw: Value::Object(Map::new()),
            pi_api: String::new(),
            haystack: String::new(),
        };
        r.refresh_haystack();
        r
    }

    pub fn refresh_haystack(&mut self) {
        let mut s = String::with_capacity(
            self.key.len() + self.description.len() + self.base_url.len() + 3,
        );
        s.push_str(&self.key);
        s.push(' ');
        s.push_str(&self.description);
        s.push(' ');
        s.push_str(&self.base_url);
        self.haystack = s.to_lowercase();
    }

    pub fn to_value(&self) -> Value {
        let mut m = self.raw.as_object().cloned().unwrap_or_default();
        set_str(&mut m, "description", &self.description);
        set_str(&mut m, "npm", &self.npm);
        let mut options = m
            .get("options")
            .and_then(|x| x.as_object())
            .cloned()
            .unwrap_or_default();
        set_str(&mut options, "baseURL", &self.base_url);
        set_str(&mut options, "apiKey", &self.api_key);
        set_num_opt(&mut options, "timeout", &self.timeout);
        m.insert("options".into(), Value::Object(options));
        let mut models = Map::new();
        for mdl in &self.models {
            models.insert(mdl.id.clone(), mdl.to_value());
        }
        m.insert("models".into(), Value::Object(models));
        Value::Object(m)
    }
}
