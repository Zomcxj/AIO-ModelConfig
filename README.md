# opencode-model-config

可视化编辑 [`opencode`](https://opencode.ai) 配置文件（`opencode.json`）中 **Agents**（子代理）与 **Providers**（模型供应商）的桌面 GUI 工具。

基于 Rust + egui/eframe 构建，单文件可执行程序，无需安装运行时。

## 功能特性

- **Agents / Providers 卡片式管理**
  - 卡片折叠 / 展开（`▶` / `▼`）
  - 拖拽排序：拖动卡片时**仅有目标卡片被高亮**，松手后完成排序
  - 新增、编辑、删除、复制 agent 与 provider
- **Provider models 管理**
  - 为 provider 增删 model
  - 配置 model 参数：`name`、`reasoning`、`tool_call`、`limit.context`、`limit.output`、`modalities.input`、`modalities.output`
  - `variants` 推理档位多选：`none / low / medium / high / xhigh / max / ultra`
  - npm 包名选项：`@ai-sdk/openai`、`@ai-sdk/anthropic`、`@ai-sdk/openai-compatible` 等
- **搜索过滤**：按 key / description / model / baseURL 等关键字过滤列表
- **文件加载**：支持直接填写配置路径、文件对话框浏览、WSL 路径读取
- **中文界面**：自动加载 Windows 系统中文字体（微软雅黑等）
- **Windows 特性**：自定义"抓取"手势拖拽光标、应用图标

## 技术栈

- Rust（2021 edition）
- [eframe / egui](https://github.com/emilk/egui) 0.31
- [serde_json](https://github.com/serde-rs/json)（`preserve_order` 保留字段顺序）
- [rfd](https://github.com/PolyMeilex/rfd)（文件对话框）
- [winres](https://github.com/shadows-withal/winres)（Windows 图标打包）
- [windows-sys](https://github.com/microsoft/windows-rs)（自定义光标）

## 构建运行

前置要求：

- Rust 工具链
- 若项目根目录存在 `assets/icon.png`，构建脚本 `build.rs` 会调用 Python + Pillow（PIL）生成 `assets/icon.ico` 与 `assets/icon_rgba.bin`；需安装 Python 及 `pillow` 库。若不存在 `icon.png`，则回退为代码生成的纯色图标，无需 Python。

```bash
cargo build --release
```

产物为单文件可执行程序：`target/release/opencode-model-config.exe`

## 资源文件

`assets/` 目录存放图标与光标的资源文件：

| 文件 | 用途 |
| ---- | ---- |
| `icon.png` | 应用图标源图（构建时生成派生文件） |
| `icon.ico` / `icon_rgba.bin` | 编译进 exe 的窗口图标 |
| `grab.png` / `grab_rgba.bin` | 拖拽时使用的"抓取"手势光标 |

## 配置文件格式参考

工具读取 / 写入 `opencode.json`，核心结构示例如下：

```jsonc
{
  "agents": {
    "my-agent": {
      "mode": "subagent",
      "description": "我的子代理",
      "model": "openai/gpt-4o",
      "variant": "",
      "temperature": 0.7,
      "color": "gold",
      "system": "系统提示词"
    }
  },
  "providers": {
    "openai": {
      "npm": "@ai-sdk/openai",
      "description": "OpenAI 官方",
      "options": {
        "baseURL": "https://api.openai.com/v1",
        "apiKey": "sk-...",
        "timeout": 180000
      },
      "models": {
        "gpt-4o": {
          "name": "GPT-4o",
          "reasoning": false,
          "tool_call": true,
          "limit": { "context": 128000, "output": 4096 },
          "modalities": { "input": ["text"], "output": ["text"] },
          "variants": { "high": { "reasoningEffort": "high" } }
        }
      }
    }
  }
}
```

## 许可证

见 [LICENSE](LICENSE)。