use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn str_at<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(|x| x.as_str()).unwrap_or_default()
}

pub fn bool_at(v: &Value, k: &str) -> bool {
    v.get(k).and_then(|x| x.as_bool()).unwrap_or_default()
}

pub fn num_at(v: &Value, k: &str) -> String {
    v.get(k)
        .and_then(|x| x.as_f64().or_else(|| x.as_i64().map(|i| i as f64)))
        .map(|n| n.to_string())
        .unwrap_or_default()
}

pub fn nested_str<'a>(v: &'a Value, path: &[&str]) -> &'a str {
    let mut cur = v;
    for p in path {
        match cur.get(*p) {
            Some(x) => cur = x,
            None => return "",
        }
    }
    cur.as_str().unwrap_or_default()
}

pub fn nested_num(v: &Value, path: &[&str]) -> String {
    let mut cur = v;
    for p in path {
        match cur.get(*p) {
            Some(x) => cur = x,
            None => return String::new(),
        }
    }
    cur.as_f64()
        .or_else(|| cur.as_i64().map(|i| i as f64))
        .map(|n| n.to_string())
        .unwrap_or_default()
}

pub fn nested_list_str(v: &Value, path: &[&str]) -> String {
    let mut cur = v;
    for p in path {
        match cur.get(*p) {
            Some(x) => cur = x,
            None => return String::new(),
        }
    }
    cur.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

pub fn set_str(m: &mut Map<String, Value>, k: &str, v: &str) {
    if v.is_empty() {
        m.remove(k);
    } else {
        m.insert(k.into(), v.into());
    }
}

pub fn set_num_opt(m: &mut Map<String, Value>, k: &str, v: &str) {
    let t = v.trim();
    if t.is_empty() {
        m.remove(k);
        return;
    }
    if let Ok(i) = t.parse::<i64>() {
        if i.to_string() == t {
            m.insert(k.into(), i.into());
            return;
        }
    }
    if let Ok(f) = t.parse::<f64>() {
        m.insert(k.into(), f.into());
    } else {
        m.remove(k);
    }
}

pub fn parse_number_text(v: &str) -> Option<Value> {
    let t = v.trim();
    if t.is_empty() {
        return None;
    }
    if let Ok(i) = t.parse::<i64>() {
        if i.to_string() == t {
            return Some(i.into());
        }
    }
    if let Ok(f) = t.parse::<f64>() {
        return Some(f.into());
    }
    None
}

pub fn default_config_path() -> Option<String> {
    if let Ok(p) = std::env::var("OPENCODE_CONFIG_PATH") {
        if !p.is_empty() {
            return Some(p);
        }
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()?;
    let base = format!("{}\\.config\\opencode", home);
    for f in ["opencode.json", "opencode.jsonc"] {
        let p = format!("{}\\{}", base, f);
        if Path::new(&p).exists() {
            return Some(p);
        }
    }
    Some(format!("{}\\opencode.json", base))
}

pub fn ensure_parent_dir(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    if let Some(parent) = p.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
        }
    }
    Ok(())
}

pub fn is_wsl_path(path: &str) -> bool {
    path.starts_with('/') && !path.contains(':')
}

pub fn win_to_wsl(path: &str) -> String {
    if let Some(ch) = path.chars().next() {
        if ch.is_ascii_alphabetic() && path.len() > 1 && path.as_bytes()[1] == b':' {
            let rest = &path[2..];
            let rest = rest.trim_start_matches('/').trim_start_matches('\\');
            return format!("/mnt/{}/{}", ch.to_ascii_lowercase(), rest);
        }
    }
    path.to_string()
}

pub fn read_wsl_file(path: &str) -> Result<String, String> {
    let out = Command::new("wsl")
        .args(["cat", path])
        .output()
        .map_err(|e| format!("wsl 命令失败: {}", e))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("wsl 读取失败: {}", err));
    }
    String::from_utf8(out.stdout).map_err(|e| format!("读取失败: {}", e))
}

pub fn write_wsl_file(path: &str, content: &str) -> Result<(), String> {
    let tmp: PathBuf = std::env::temp_dir().join("opencode_config_tmp.json");
    fs::write(&tmp, content).map_err(|e| format!("写入临时文件失败: {}", e))?;
    let tmp_str = tmp.to_string_lossy().replace('\\', "/");
    let tmp_wsl = win_to_wsl(&tmp_str);
    let out = Command::new("wsl")
        .args(["cp", &tmp_wsl, path])
        .output()
        .map_err(|e| format!("wsl 命令失败: {}", e))?;
    fs::remove_file(&tmp).ok();
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("wsl 写入失败: {}", err));
    }
    Ok(())
}

pub fn show_file_dialog() -> Option<String> {
    let out = Command::new("powershell.exe")
        .args([
            "-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden",
            "-Command",
            r#"
Add-Type -AssemblyName System.Windows.Forms
$dlg = New-Object System.Windows.Forms.OpenFileDialog
$dlg.Title = 'Select config file'
$dlg.Filter = 'JSON files (*.json, *.jsonc)|*.json;*.jsonc|All files|*.*'
$dlg.CheckFileExists = $false
$r = $dlg.ShowDialog()
if ($r -eq 'OK') { $dlg.FileName } else { '' }
"#,
        ])
        .output()
        .ok()?;
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if p.is_empty() { None } else { Some(p) }
}
