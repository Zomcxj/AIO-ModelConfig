//! DeepSeek Harness（DSH）后端：`~/.dsh/settings.yaml`。
//!
//! 只管理 `llm-pi-ai.providers` 及 `agent-default-model`，其他 loader 配置
//! （如 ui、conversation、插件设置）一律以 raw 为基底原样保留。

use super::{Backend, BackendLoad};
use crate::convert;
use crate::credentials;
use crate::format::ConfigFormat;
use crate::model::{AgentRow, ModelRow, ProviderRow};
use crate::util::{parse_yaml_content, read_config_content, to_yaml_string, wsl_home, WslPathProbe};
use serde_json::{Map, Value};
use std::path::Path;

pub struct DeepSeekHarnessBackend;
pub static BACKEND: DeepSeekHarnessBackend = DeepSeekHarnessBackend;

fn default_local_path() -> String {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    format!("{}\\.dsh\\settings.yaml", home)
}

fn is_dsh_root(v: &Value) -> bool {
    v.get("llm-pi-ai")
        .and_then(|x| x.get("providers"))
        .and_then(Value::as_object)
        .is_some()
}

fn model_from_dsh(v: &Value) -> ModelRow {
    let mut m = convert::model_from_pi(v);
    m.variants = v
        .get("reasoningEfforts")
        .and_then(Value::as_object)
        .map(|o| o.values().filter_map(Value::as_str).collect::<Vec<_>>().join(", "))
        .unwrap_or_default();
    m.raw = v.clone();
    m
}

fn model_to_dsh(m: &ModelRow) -> Value {
    let mut obj = m.raw.as_object().cloned().unwrap_or_default();
    obj.insert("id".into(), Value::String(m.id.clone()));
    if m.name.trim().is_empty() {
        obj.remove("name");
    } else {
        obj.insert("name".into(), Value::String(m.name.clone()));
    }
    let input: Vec<Value> = m.modalities_input.split(',').map(str::trim).filter(|s| !s.is_empty()).map(|s| Value::String(s.to_string())).collect();
    if input.is_empty() { obj.remove("input"); } else { obj.insert("input".into(), Value::Array(input)); }
    if let Ok(v) = m.context.parse::<i64>() { obj.insert("contextWindow".into(), v.into()); } else { obj.remove("contextWindow"); }
    if let Ok(v) = m.output.parse::<i64>() { obj.insert("maxTokens".into(), v.into()); } else { obj.remove("maxTokens"); }
    if m.variants.trim().is_empty() {
        obj.remove("reasoningEfforts");
    } else {
        let mut efforts = Map::new();
        if let Some(raw) = m.raw.get("reasoningEfforts").and_then(Value::as_object) {
            for (k, v) in raw {
                if m.variants.split(',').any(|name| name.trim() == v.as_str().unwrap_or("")) {
                    efforts.insert(k.clone(), v.clone());
                }
            }
        }
        for name in m.variants.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            efforts.entry(name.to_string()).or_insert_with(|| Value::String(name.to_string()));
        }
        obj.insert("reasoningEfforts".into(), Value::Object(efforts));
    }
    Value::Object(obj)
}

fn provider_from_dsh(key: &str, v: &Value, credentials_root: &Value) -> ProviderRow {
    let api = v.get("api").and_then(Value::as_str).unwrap_or_default();
    let models = v.get("models").and_then(Value::as_array).map(|a| a.iter().map(model_from_dsh).collect()).unwrap_or_default();
    let env = v.get("apiKeyEnv").and_then(Value::as_str).unwrap_or_default().to_string();
    ProviderRow {
        key: key.to_string(), description: String::new(), npm: convert::api_to_npm(api),
        base_url: v.get("baseURL").and_then(Value::as_str).unwrap_or_default().to_string(),
        api_key: String::new(), api_key_env: env.clone(), api_key_secret: credentials::secret_for(credentials_root, &env),
        timeout: String::new(), compat: true, models, new_model: ModelRow::new(), raw: v.clone(), pi_api: api.to_string(),
    }
}

fn provider_to_dsh(p: &ProviderRow) -> Value {
    let mut obj = p.raw.as_object().cloned().unwrap_or_default();
    obj.insert("api".into(), Value::String(if p.pi_api.is_empty() { convert::npm_to_api(&p.npm) } else { p.pi_api.clone() }));
    if p.base_url.is_empty() { obj.remove("baseURL"); } else { obj.insert("baseURL".into(), Value::String(p.base_url.clone())); }
    let env = credentials::effective_env_name(p);
    if env.is_empty() { obj.remove("apiKeyEnv"); } else { obj.insert("apiKeyEnv".into(), Value::String(env)); }
    obj.insert("models".into(), Value::Array(p.models.iter().map(model_to_dsh).collect()));
    Value::Object(obj)
}

impl Backend for DeepSeekHarnessBackend {
    fn id(&self) -> ConfigFormat {
        ConfigFormat::DeepSeekHarness
    }
    fn default_local_path(&self) -> String { default_local_path() }
    fn default_wsl_path(&self) -> Option<String> { Some(format!("{}/.dsh/settings.yaml", wsl_home()?)) }
    fn local_available(&self, path: &str) -> bool { Path::new(path).exists() || Path::new(path).parent().map(|p| p.exists()).unwrap_or(false) }
    fn wsl_available(&self, probe: WslPathProbe) -> bool { probe.path_exists || probe.parent_dir_exists }
    fn detect(&self, content: &str, path: &str) -> bool {
        let lower = path.to_lowercase();
        if lower.contains("/.dsh/") || lower.contains("\\.dsh\\") { return true; }
        parse_yaml_content(content).map(|v| is_dsh_root(&v)).unwrap_or(false)
    }
    fn parse(&self, content: &str) -> Result<BackendLoad, String> {
        self.parse_at(content, "")
    }
    fn parse_at(&self, content: &str, path: &str) -> Result<BackendLoad, String> {
        let root = parse_yaml_content(content)?;
        let creds = if path.is_empty() { Value::Object(Map::new()) } else { credentials::load_root(path) };
        let providers = root.get("llm-pi-ai").and_then(|x| x.get("providers")).and_then(Value::as_object).map(|o| o.iter().map(|(k, v)| provider_from_dsh(k, v, &creds)).collect()).unwrap_or_default();
        let default_model = root
            .get("agent-default-model")
            .and_then(Value::as_object)
            .and_then(|m| Some((m.get("provider")?.as_str()?.to_string(), m.get("model")?.as_str()?.to_string())));
        Ok(BackendLoad { root: root.clone(), agents: Vec::new(), providers, extras: root, default_model })
    }
    fn serialize_root(&self, _agents: &[AgentRow], providers: &[ProviderRow], extras: &Value, target_root: Option<&Value>) -> Value {
        let mut root = target_root.cloned().unwrap_or_else(|| extras.clone()).as_object().cloned().unwrap_or_default();
        let mut llm = root.remove("llm-pi-ai").and_then(|v| v.as_object().cloned()).unwrap_or_default();
        llm.insert("providers".into(), Value::Object(providers.iter().filter(|p| !p.key.is_empty()).map(|p| (p.key.clone(), provider_to_dsh(p))).collect()));
        root.insert("llm-pi-ai".into(), Value::Object(llm));
        Value::Object(root)
    }
    fn serialize_root_with_default(
        &self,
        agents: &[AgentRow],
        providers: &[ProviderRow],
        extras: &Value,
        target_root: Option<&Value>,
        default_model: Option<&Option<(String, String)>>,
    ) -> Value {
        let mut root = self.serialize_root(agents, providers, extras, target_root);
        if let Some(default_model) = default_model {
            if let Some(object) = root.as_object_mut() {
                match default_model {
                    Some((provider, model)) => {
                        let mut default = object
                            .remove("agent-default-model")
                            .and_then(|value| value.as_object().cloned())
                            .unwrap_or_default();
                        default.insert("provider".into(), Value::String(provider.clone()));
                        default.insert("model".into(), Value::String(model.clone()));
                        object.insert("agent-default-model".into(), Value::Object(default));
                    }
                    None => { object.remove("agent-default-model"); }
                }
            }
        }
        root
    }

    fn load_target_root(&self, path: &str) -> Value {
        read_config_content(path).ok().and_then(|s| parse_yaml_content(&s).ok()).unwrap_or_else(|| Value::Object(Map::new()))
    }
    fn save_sidecars(&self, path: &str, providers: &[ProviderRow]) -> Result<(), String> { credentials::save(path, providers) }
    fn icon_rgba(&self) -> Option<(&'static [u8], u32, u32)> { None }
    fn render(&self, root: &Value, _compact: bool) -> Result<String, String> { to_yaml_string(root) }
}
