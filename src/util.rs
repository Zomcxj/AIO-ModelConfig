use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 构造 wsl 命令：Windows 下带 CREATE_NO_WINDOW，
/// 避免 GUI 程序拉起控制台进程（wsl.exe）时闪现终端窗口。
fn wsl_command() -> Command {
    let mut cmd = Command::new("wsl");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

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

pub fn wsl_home() -> Option<String> {
    let out = wsl_command()
        .args(["-e", "sh", "-c", "printf %s \"$HOME\""])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let home = String::from_utf8(out.stdout).unwrap_or_default().trim().to_string();
    if home.is_empty() {
        None
    } else {
        Some(home)
    }
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\\\''"))
}

pub fn wsl_path_exists(path: &str) -> bool {
    let out = wsl_command()
        .args(["-e", "sh", "-c", &format!("test -e {} && echo y", shell_quote(path))])
        .output();
    matches!(out, Ok(o) if o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "y")
}

pub fn wsl_file_exists(path: &str) -> bool {
    let out = wsl_command()
        .args(["-e", "sh", "-c", &format!("test -f {} && echo y", shell_quote(path))])
        .output();
    matches!(out, Ok(o) if o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "y")
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
    let out = wsl_command()
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
    let tmp: PathBuf = std::env::temp_dir().join(format!(
        "opencode_config_tmp_{}.json",
        std::process::id()
    ));
    fs::write(&tmp, content).map_err(|e| format!("写入临时文件失败: {}", e))?;
    let tmp_str = tmp.to_string_lossy().replace('\\', "/");
    let tmp_wsl = win_to_wsl(&tmp_str);
    let out = wsl_command()
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
    rfd::FileDialog::new()
        .set_title("选择配置文件")
        .add_filter("JSON", &["json", "jsonc"])
        .add_filter("YAML", &["yml", "yaml"])
        .add_filter("所有文件", &["*"])
        .pick_file()
        .map(|p| p.to_string_lossy().to_string())
}

/// 剥离 JSONC 的行注释、块注释与尾逗号，产出可被 serde_json 解析的 JSON。
/// 字符串字面量内的注释符与逗号不受影响；注释中的换行保留以维持行号。
pub fn strip_jsonc_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' => match chars.peek() {
                Some('/') => {
                    chars.next();
                    for c2 in chars.by_ref() {
                        if c2 == '\n' {
                            out.push('\n');
                            break;
                        }
                    }
                }
                Some('*') => {
                    chars.next();
                    while let Some(c2) = chars.next() {
                        if c2 == '*' && chars.peek() == Some(&'/') {
                            chars.next();
                            break;
                        }
                        if c2 == '\n' {
                            out.push('\n');
                        }
                    }
                }
                _ => out.push(c),
            },
            _ => out.push(c),
        }
    }
    remove_trailing_commas(&out)
}

/// 移除 `}` / `]` 前的尾逗号（JSONC 允许，严格 JSON 不允许）。
fn remove_trailing_commas(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == ',' {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j < chars.len() && (chars[j] == '}' || chars[j] == ']') {
                i += 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// 读取配置文件内容（支持本地与 WSL 路径）；不存在返回空串。
pub fn read_config_content(path: &str) -> Result<String, String> {
    if path.is_empty() {
        return Ok(String::new());
    }
    if is_wsl_path(path) {
        if !wsl_file_exists(path) {
            return Ok(String::new());
        }
        read_wsl_file(path)
    } else if std::path::Path::new(path).exists() {
        std::fs::read_to_string(path).map_err(|e| format!("读取失败: {}", e))
    } else {
        Ok(String::new())
    }
}

/// 解析配置内容（支持 JSONC 注释与尾逗号）；空内容视为空对象。
pub fn parse_config_content(content: &str) -> Result<serde_json::Value, String> {
    if content.trim().is_empty() {
        return Ok(serde_json::Value::Object(serde_json::Map::new()));
    }
    let stripped = strip_jsonc_comments(content);
    serde_json::from_str(&stripped).map_err(|e| format!("解析失败: {}", e))
}

/// 解析 YAML 配置内容；空内容视为空对象。
pub fn parse_yaml_content(content: &str) -> Result<serde_json::Value, String> {
    if content.trim().is_empty() {
        return Ok(serde_json::Value::Object(serde_json::Map::new()));
    }
    serde_yaml_ng::from_str(content).map_err(|e| format!("解析失败: {}", e))
}

/// 将 Value 序列化为块风格 YAML 文本。
pub fn to_yaml_string(value: &serde_json::Value) -> Result<String, String> {
    serde_yaml_ng::to_string(value).map_err(|e| format!("序列化失败: {}", e))
}

/// WSL 侧路径的父目录是否存在（"已安装" 判定：配置文件或其目录存在即可）。
pub fn wsl_parent_dir_exists(path: &str) -> bool {
    let Some((dir, _)) = path.rsplit_once('/') else {
        return false;
    };
    let out = wsl_command()
        .args(["-e", "sh", "-c", &format!("test -d {} && echo y", shell_quote(dir))])
        .output();
    matches!(out, Ok(o) if o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "y")
}
