# AIO-ModelConfig

可视化编辑 [`opencode`](https://opencode.ai)、[`pi-agent`](https://github.com/anthropics/pi-agent) 与 [`oh-my-pi`](https://github.com/can1357/oh-my-pi)（omp）配置文件的桌面 GUI 工具。

基于 Rust + egui/eframe 构建，单文件可执行程序，无需安装运行时。

## 功能特性

- **三格式支持**：opencode (`opencode.json`)、pi-agent (`models.json`)、oh-my-pi (`models.yml`，YAML)
- **自动格式检测**：优先加载 opencode，其次 oh-my-pi / pi-agent；`.yml`/`.yaml` 文件优先识别为 oh-my-pi
- **三方互转**：任意格式加载后可保存到任一目标（需已安装对应工具）；写入时先读取目标文件已有内容，仅合并 Providers/Agents，其余字段保留
- **oh-my-pi 方言适配**：pi 风格 `thinkingLevelMap` 与 omp 官方 `thinking: {mode, efforts, effortMap}` 双向自动翻译；provider/model 的扩展字段（`headers`/`auth`/`discovery`/`modelOverrides`/`cost` 等）保存时原样保留
- **Agents / Providers 卡片式管理**
  - 卡片折叠 / 展开（`▶` / `▼`）
  - 拖拽排序：拖动卡片时**仅有目标卡片被高亮**，松手后完成排序
  - 新增、编辑、删除、复制 agent 与 provider
- **Provider models 管理**
  - 为 provider 增删 model
  - 配置 model 参数：`name`、`reasoning`、`tool_call`、`limit.context`、`limit.output`、`modalities.input`、`modalities.output`
  - `variants` 推理档位多选：`none / low / medium / high / xhigh / max / ultra`
  - npm 包名选项：`@ai-sdk/openai`、`@ai-sdk/anthropic`、`@ai-sdk/google`、`@ai-sdk/openai-compatible` 等
- **搜索过滤**：按 key / description / model / baseURL 等关键字过滤列表
- **文件加载**：支持直接填写配置路径、文件对话框浏览、WSL 路径读取、拖拽导入
- **保存格式**：默认（pretty）/ 压缩（compact）两种 JSON 格式
- **主题切换**：Dark / Light / Ocean / Nord / Rose 五种主题
- **中文界面**：自动加载 Windows 系统中文字体（微软雅黑等）
- **Windows 特性**：自定义"抓取"手势拖拽光标、应用图标

## 技术栈

- Rust（2021 edition）
- [eframe / egui](https://github.com/emilk/egui) 0.31
- [serde_json](https://github.com/serde-rs/json)（`preserve_order` 保留字段顺序）
- [serde_yaml_ng](https://github.com/nbatchelor/serde_yaml_ng)（oh-my-pi YAML 序列化）
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

产物为单文件可执行程序：`target/release/aio-model-config.exe`

## 资源文件

`assets/` 目录存放图标与光标的资源文件：

| 文件 | 用途 |
| ---- | ---- |
| `icon.png` | 应用图标源图（构建时生成派生文件） |
| `icon.ico` / `icon_rgba.bin` | 编译进 exe 的窗口图标 |
| `grab.png` / `grab_rgba.bin` | 拖拽时使用的"抓取"手势光标 |

## 配置文件格式参考

### opencode

工具读取 / 写入 `opencode.json`，核心结构示例如下：

```jsonc
{
  "agent": {
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
  "provider": {
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

### pi-agent

工具读取 / 写入 `~/.pi/agent/models.json`，核心结构示例如下：

```json
{
  "providers": {
    "openai": {
      "baseUrl": "https://api.openai.com/v1",
      "apiKey": "sk-...",
      "api": "openai-completions",
      "models": [
        {
          "id": "gpt-4o",
          "name": "GPT-4o",
          "reasoning": false,
          "input": ["text"],
          "contextWindow": 128000,
          "maxTokens": 4096
        }
      ]
    },
    "anthropic": {
      "baseUrl": "https://api.anthropic.com",
      "apiKey": "sk-ant-...",
      "api": "anthropic-messages",
      "models": [
        {
          "id": "claude-sonnet-4-20250514",
          "name": "Claude Sonnet 4",
          "reasoning": false,
          "input": ["text", "image"],
          "contextWindow": 200000,
          "maxTokens": 8192
        }
      ]
    }
  }
}
```

**格式差异对照：**

| 字段 | opencode | pi-agent | oh-my-pi |
|------|----------|----------|----------|
| Provider key | `provider.{name}` | `providers.{name}` | `providers.{name}` |
| Base URL | `options.baseURL` | `baseUrl` | `baseUrl` |
| API Key | `options.apiKey` | `apiKey` | `apiKey`（环境变量名或字面量） |
| 模型存储 | Map（key=model id） | Array（含 id 字段） | Array（含 id 字段） |
| 上下文长度 | `limit.context` | `contextWindow` | `contextWindow` |
| 输出限制 | `limit.output` | `maxTokens` | `maxTokens` |
| 输入模态 | `modalities.input` | `input` | `input` |
| API 类型 | `npm` | `api` | `api`（9 种枚举） |
| 推理档位 | `variants`（保留原始详情如 `reasoningEffort`） | `thinkingLevelMap` | `thinking: {mode, efforts, effortMap}` |
| 工具调用 | `tool_call` | 不支持 | 不支持 |
| Agent 定义 | `agent` | 不支持 | 不支持 |
| 扩展字段 | 顶层字段保留 | 顶层字段保留 | provider/model 级字段保留（`headers`/`auth`/`discovery`/`modelOverrides`/`cost`/`tokenizer` 等） |

### oh-my-pi

工具读取 / 写入 `~/.omp/agent/models.yml`（本地优先，本地不可用回落 WSL `~/.omp/agent/models.yml`），YAML 格式，结构与 pi-agent 同族：

```yaml
providers:
  my-gateway:
    baseUrl: https://gateway.example.com/v1
    api: openai-completions
    apiKey: sk-...
    authHeader: true            # 注入 Authorization: Bearer
    headers:                    # 保存时原样保留
      X-Team: platform
    discovery:
      type: openai-models-list
    models:
    - id: m1
      name: Model One
      reasoning: true
      input: [text, image]
      contextWindow: 200000
      maxTokens: 16384
      thinking:
        mode: effort
        efforts: [medium, high, xhigh, max]
```

**oh-my-pi 注意事项：**

- `apiKey` 为“环境变量名或字面量”语义：值若匹配已存在的环境变量名则取该变量，否则按字面量使用（`!` 前缀会执行 shell 命令——本工具不使用该特性，原样保存）；
- 推理档位：omp 官方字段为 `thinking` 块（pi 旧字段 `thinkingLevelMap` 在 omp 中**无效**，本工具加载双方言兼容、保存时自动翻译为官方字段）；
- 非对称档位映射（如 pi `{high: max}` ↔ omp `efforts: [high] + effortMap: {high: max}`）双向转换自动保持；
- YAML 注释与文件风格：serde 序列化不保留注释（保存后注释丢失），输出为标准块风格；
- 根目录仅 `providers` 键有效，其余顶层字段原样保留。

## 许可证

见 [LICENSE](LICENSE)。

## 平台与安全说明

- 本工具当前**仅支持 Windows**（依赖 Win32 光标子系统、微软雅黑字体路径与 `wsl` 命令）。
- 配置文件中的 `apiKey` 以**明文**读取与写回（与 opencode / pi-agent 本身的存储方式一致），请勿将配置文件提交到公开仓库。
- 保存到 opencode / pi-agent 目标时采用“先读后合并”策略：仅更新 `agent`/`provider`（或 `providers`）字段，目标文件其余配置（如 `mcp`、`instructions`）原样保留。
- 本地与 WSL 同时存在同名配置时，保存目标**优先本地路径**，仅本地不存在时回落 WSL。