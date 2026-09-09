//! DeepSeek Harness（DSH）凭据 sidecar 支持。
//!
//! DSH 将 provider 配置放在 `settings.yaml`，实际密钥放在同目录的
//! `.credentials.yaml`：`refs.{apiKeyEnv}`。本模块只更新指定 ref，保留其余
//! 版本、记录和未知字段。

use crate::model::ProviderRow;
use crate::util::{is_wsl_path, parse_yaml_content, read_config_content, to_yaml_string};
use serde_json::{Map, Value};

/// 根据模型配置路径得到同级凭据路径。
pub fn sidecar_path(config_path: &str) -> String {
    let separator = if is_wsl_path(config_path) || config_path.contains('/') {
        '/'
    } else {
        '\\'
    };
    match config_path.rsplit_once(separator) {
        Some((parent, _)) if !parent.is_empty() => format!("{}{}{}", parent, separator, ".credentials.yaml"),
        _ => ".credentials.yaml".to_string(),
    }
}

/// 读取凭据文件的完整 root；不存在/无法解析时返回空对象。
pub fn load_root(config_path: &str) -> Value {
    let path = sidecar_path(config_path);
    read_config_content(&path)
        .ok()
        .and_then(|s| parse_yaml_content(&s).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| {
            let mut root = Map::new();
            root.insert("version".into(), Value::Number(1.into()));
            root.insert("refs".into(), Value::Object(Map::new()));
            root.insert("records".into(), Value::Object(Map::new()));
            Value::Object(root)
        })
}

/// 从凭据 root 解析 refs 中的字符串密钥。
pub fn secret_for(root: &Value, env_name: &str) -> String {
    root.get("refs")
        .and_then(Value::as_object)
        .and_then(|refs| refs.get(env_name))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// 将 provider 的实际密钥更新到同级凭据文件。
/// 空密钥删除对应 ref；其余 refs/records/未知字段完全保留。
pub fn save(config_path: &str, providers: &[ProviderRow]) -> Result<(), String> {
    if !providers
        .iter()
        .any(|provider| !effective_env_name(provider).is_empty())
    {
        return Ok(());
    }
    let path = sidecar_path(config_path);
    let mut root = load_root(config_path);
    let Some(object) = root.as_object_mut() else {
        return Err("凭据文件根节点必须是对象".into());
    };
    let refs = object
        .entry("refs")
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(refs) = refs.as_object_mut() else {
        return Err("凭据文件 refs 必须是对象".into());
    };

    for provider in providers {
        let env_name = effective_env_name(provider);
        if env_name.is_empty() {
            continue;
        }
        let secret = effective_secret(provider);
        if !secret.is_empty() {
            refs.insert(env_name, Value::String(secret));
        }
    }

    let content = to_yaml_string(&Value::Object(object.clone()))?;
    crate::backends::write_config(&path, &content)
}

/// DSH 需要写入 settings.yaml 的引用名。
pub fn effective_env_name(provider: &ProviderRow) -> String {
    provider.api_key_env.trim().to_string()
}

/// DSH sidecar 中的实际密钥。跨格式转换不会回退使用普通格式的 api_key，
/// 只有用户明确填写 DSH 密钥字段时才会写入。
pub fn effective_secret(provider: &ProviderRow) -> String {
    provider.api_key_secret.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ProviderRow;
    use serde_json::json;

    #[test]
    fn sidecar_is_same_directory() {
        assert_eq!(sidecar_path(r"C:\Users\cxj\.dsh\settings.yaml"), r"C:\Users\cxj\.dsh\.credentials.yaml");
        assert_eq!(sidecar_path("/home/cxj/.dsh/settings.yaml"), "/home/cxj/.dsh/.credentials.yaml");
    }

    #[test]
    fn save_preserves_unrelated_credentials() {
        let root = json!({
            "version": 1,
            "refs": {"OLD": "old-secret", "DSH_KEY": "old"},
            "records": {"keep": true},
            "extra": {"keep": true}
        });
        let mut p = ProviderRow::new();
        p.key = "dsh".into();
        p.api_key_env = "DSH_KEY".into();
        p.api_key_secret = "new-secret".into();
        let mut refs = root["refs"].as_object().unwrap().clone();
        refs.insert("DSH_KEY".into(), Value::String(effective_secret(&p)));
        let mut out = root.as_object().unwrap().clone();
        out.insert("refs".into(), Value::Object(refs));
        assert_eq!(out["refs"]["OLD"], "old-secret");
        assert_eq!(out["refs"]["DSH_KEY"], "new-secret");
        assert_eq!(out["records"]["keep"], true);
        assert_eq!(out["extra"]["keep"], true);
    }
}
