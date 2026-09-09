//! Agent 配置后端注册表 —— 每种配置格式一个后端模块。
//!
//! 新增 agent 集成：新建 `src/backends/<id>.rs` 实现 [`Backend`]，
//! 在 [`BACKENDS`] 注册即可；加载 / 保存 / 判别 / 目标解析全部走本注册表，
//! app 层不出现按格式的硬编码分支。
//!
//! 设计约束（Phase 1）：
//! - 序列化风格（pretty / compact JSON）由 app 层的序列化器处理，
//!   后端只产出 `serde_json::Value` 形式的 root；
//! - 跨格式保存的合并语义内置于各后端的 `serialize_root`。

pub mod opencode;
pub mod pi_agent;

use crate::format::ConfigFormat;
use crate::model::{AgentRow, ProviderRow};
use crate::util::{ensure_parent_dir, is_wsl_path};
use serde_json::Value;
use std::path::Path;

/// 一个后端加载结果的完整快照。
#[derive(Clone)]
pub struct BackendLoad {
    /// 完整原始 root（当前文件保存时的基底）。
    pub root: Value,
    /// opencode 格式的 agents（pi 系为空）。
    pub agents: Vec<AgentRow>,
    /// providers（两种格式都有）。
    pub providers: Vec<ProviderRow>,
    /// 本后端顶层未知字段：opencode 为整个 root（agent/provider 会被整体替换），
    /// pi 系为 `providers` 之外的顶层字段。
    pub extras: Value,
}

/// 配置后端：一种 agent 配置格式的加载 / 保存 / 判别 / 路径知识。
pub trait Backend: Sync {
    /// 唯一标识（对应 ConfigFormat 枚举）。
    fn id(&self) -> ConfigFormat;

    /// 配置文件扩展名（不含点），如 "json" / "yml"。
    fn file_ext(&self) -> &'static str;

    /// 默认本地路径（Windows 风格）。
    fn default_local_path(&self) -> String;

    /// 默认 WSL 路径（None = 不支持 WSL）。
    fn default_wsl_path(&self) -> Option<String>;

    /// 本地目标是否可用（一般：文件存在；宽松后端：文件或父目录存在）。
    fn local_available(&self, local_path: &str) -> bool;

    /// WSL 目标是否可用（存在性检查方式由后端决定）。
    fn wsl_available(&self, wsl_path: &str) -> bool;

    /// 内容判别：该内容是否属于本格式。`path` 提供扩展名上下文（可为空）。
    fn detect(&self, content: &str, path: &str) -> bool;

    /// 内容 → 结构化数据；读取/解析失败返回 Err。
    fn parse(&self, content: &str) -> Result<BackendLoad, String>;

    /// 构造保存用 root。
    /// - `target_root = None`：当前文件保存，以 `extras` 为基底整体写入 UI 状态；
    /// - `target_root = Some`：跨格式目标保存，与目标现有内容合并（upsert）。
    fn serialize_root(
        &self,
        agents: &[AgentRow],
        providers: &[ProviderRow],
        extras: &Value,
        target_root: Option<&Value>,
    ) -> Value;

    /// 跨格式目标保存时读取目标现有 root（容错：读不到返回空对象）。
    fn load_target_root(&self, path: &str) -> Value;
}

/// 全部后端。**顺序即语义**：第 0 个是判别回落项，其余按"更具体优先"排列。
pub static BACKENDS: &[&dyn Backend] = &[&opencode::BACKEND, &pi_agent::BACKEND];

/// 按标识查找后端。
pub fn backend(id: ConfigFormat) -> &'static dyn Backend {
    BACKENDS
        .iter()
        .copied()
        .find(|b| b.id() == id)
        .expect("BACKENDS 必须覆盖所有 ConfigFormat 变体")
}

/// 内容判别：遍历非回落后端找首个命中，否则回落第 0 个。
pub fn detect_format(content: &str, path: &str) -> ConfigFormat {
    for b in BACKENDS.iter().skip(1) {
        if b.detect(content, path) {
            return b.id();
        }
    }
    BACKENDS[0].id()
}

/// 通用加载：读文件（本地或 WSL）+ 按后端解析。
pub fn load_backend(id: ConfigFormat, path: &str) -> Result<BackendLoad, String> {
    let content = crate::util::read_config_content(path)?;
    backend(id).parse(&content)
}

/// WSL 目标路径（存在则返回）。
pub fn wsl_target(id: ConfigFormat) -> Option<String> {
    let b = backend(id);
    let wsl = b.default_wsl_path()?;
    if b.wsl_available(&wsl) {
        Some(wsl)
    } else {
        None
    }
}

/// 目标是否可用（本地或 WSL）。
pub fn target_available(id: ConfigFormat, local_path: &str) -> bool {
    backend(id).local_available(local_path) || wsl_target(id).is_some()
}

/// 实际写入路径：本地优先，本地不可用回落 WSL，最后回退本地默认（新建场景）。
pub fn target_path(id: ConfigFormat, local_path: &str) -> String {
    if Path::new(local_path).exists() {
        return local_path.to_string();
    }
    if let Some(wsl) = wsl_target(id) {
        return wsl;
    }
    local_path.to_string()
}

/// 统一写入：WSL 路径走 wsl 命令，本地路径自动创建父目录。
pub fn write_config(path: &str, content: &str) -> Result<(), String> {
    if is_wsl_path(path) {
        crate::util::write_wsl_file(path, content)
    } else {
        ensure_parent_dir(path)?;
        std::fs::write(path, content).map_err(|e| e.to_string())
    }
}
