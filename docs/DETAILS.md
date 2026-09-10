# ModelHarbor 技术细节

本文为 [README](../README.md) 的详细补充：技术栈、构建细节、四格式字段对照、分页方言表单、DeepSeek Harness 配置说明、oh-my-pi 注意事项与平台安全说明。

## 技术栈

- Rust（2021 edition）
- [eframe / egui](https://github.com/emilk/egui) 0.31
- [serde_json](https://github.com/serde-rs/json)（`preserve_order` 保留字段顺序）
- [serde_yaml_ng](https://github.com/nbatchelor/serde_yaml_ng)（oh-my-pi YAML 序列化）
- [rfd](https://github.com/PolyMeilex/rfd)（文件对话框）
- [ureq](https://github.com/algesten/ureq)（“获取模型”的 HTTP 客户端，rustls TLS，后台线程执行不阻塞 UI）
- [winres](https://github.com/shadows-withal/winres)（Windows 图标打包）
- [windows-sys](https://github.com/microsoft/windows-rs)（自定义光标）

## 构建细节

前置要求：

- Rust 工具链
- 若项目根目录存在 `assets/icon.png`，构建脚本 `build.rs` 会调用 Python + Pillow（PIL）生成 `assets/icon.ico` 与 `assets/icon_rgba.bin`；需安装 Python 及 `pillow` 库。若不存在 `icon.png`，则回退为代码生成的纯色图标，无需 Python。

```bash
cargo build --release
```

产物为单文件可执行程序：`target/release/model-harbor.exe`

### 资源文件

`assets/` 目录存放图标与光标的资源文件：

| 文件 | 用途 |
| ---- | ---- |
| `icon.png` | 应用图标源图（构建时生成派生文件） |
| `icon.ico` / `icon_rgba.bin` | 编译进 exe 的窗口图标 |
| `grab.png` / `grab_rgba.bin` | 拖拽时使用的"抓取"手势光标 |
| `agents/*.bin` | 三个 agent 的官方图标（32×32 RGBA，顶栏标签与来源行渲染） |

## 分页方言表单

顶栏四个 agent 图标标签（opencode / DeepSeek Harness / oh-my-pi / pi-agent）点击切换；加载任意一份配置后四个页面共享同一份数据，修改 provider 参数在所有页面同步生效（provider/model 顺序亦跨页同步）；Agents 区块仅属于 opencode 页面；各页表单按自身方言显示字段与枚举（无对应字段不显示占位）：

- **opencode 页**：`options.baseURL` / `options.timeout` / `npm` 下拉 / `limit.context` / `modalities` / `variants`（none…ultra）
- **pi-agent 页**：`baseUrl` / `apiKey` / `api` 下拉（pi KnownApi 10 值）/ `compat` / `contextWindow` / `maxTokens` / `input` / `thinkingLevelMap`（off/minimal…max）
- **oh-my-pi 页**：`baseUrl` / `apiKey` / `api` 下拉（omp 官方 9 值）/ `compat` / `contextWindow` / `maxTokens` / `input` / `thinking.efforts`（minimal…max）
- **DeepSeek Harness 页**：`baseURL` / `apiKeyEnv` + 实际密钥（存同级 `.credentials.yaml`）/ `api` 下拉 / `timeoutMs` / `retryPolicy.mode` / `retryPolicy.maxRetries` / `models`（`id` / `name` / `contextWindow` / `maxTokens` / `input` / `reasoningEfforts`）

**获取模型**：每个 provider 卡片与“新增 Provider”弹窗的 Models 标题右侧都有“获取模型”按钮。点击后按 provider 的 api 类型请求模型列表接口并弹层展示：

- 地址：`{baseURL}/models`；`anthropic-messages` 固定使用 `/v1/models`（`baseURL` 已去 `/v1` 时自动补回）
- 鉴权：`anthropic-messages` 用 `x-api-key` + `anthropic-version`，其余用 `Authorization: Bearer`
- 解析兼容 `data` / `models` / 裸数组三种响应格式（含 Gemini 式 `name: models/...` 前缀清理与去重）
- 已配置的模型自动打勾；勾选未配置的模型即新增一行 `ModelRow`；取消勾选不删除既有配置，避免误伤已填写的模型参数

**缺省默认值**：配置文件未写 `timeout` 时，opencode 的 `options.timeout` 与 DSH 的 `timeoutMs` 均默认显示 `180000`（ms）；未修改时保存不写回，避免污染配置。DSH 的 `retryPolicy.mode` 缺省显示 `normal`。

保存语义：当前文件属于本页格式且已加载时写当前文件（整体替换）；手动修改了路径但未点“加载”时，仍写该路径但自动切换为“先读后合并”，不会破坏目标文件已有配置；其余情况写该后端默认目标（Windows 本地路径）。跨格式写入采用“先读后合并”，仅更新 `agent`/`provider`（或 `providers`）字段，目标文件其余配置（如 `mcp`、`instructions`）原样保留；保存时不产生空对象污染（空列表、空 `limit`/`options` 省略不写）。

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

**oh-my-pi 注意事项：**

- `apiKey` 为"环境变量名或字面量"语义：值若匹配已存在的环境变量名则取该变量，否则按字面量使用（`!` 前缀会执行 shell 命令——本工具不使用该特性，原样保存）；
- 推理档位：omp 官方字段为 `thinking` 块（pi 旧字段 `thinkingLevelMap` 在 omp 中**无效**，本工具加载双方言兼容、保存时自动翻译为官方字段）；
- 非对称档位映射（如 pi `{high: max}` ↔ omp `efforts: [high] + effortMap: {high: max}`）双向转换自动保持；
- YAML 注释与文件风格：serde 序列化不保留注释（保存后注释丢失），输出为标准块风格；
- 根目录仅 `providers` 键有效，其余顶层字段原样保留。

### DeepSeek Harness（DSH）

工具读取 / 写入 `~/.dsh/settings.yaml`，只管理 `llm-pi-ai.providers`；其余顶层配置（`ui`、`conversation`、`agent-default-model`、插件设置等）一律原样保留。核心结构示例如下：

```yaml
ui-theme:
  name: dark
agent-default-model:
  provider: sensenova
  model: deepseek-v4-flash
  reasoningEffort: max
llm-pi-ai:
  providers:
    sensenova:
      apiKeyEnv: SENSENOVA_API_KEY   # 凭据引用名，存于主配置
      api: openai-completions
      baseURL: https://api.sensenova.cn/v1
      timeoutMs: 180000
      retryPolicy:
        mode: normal
        maxRetries: 3
      models:
        - id: deepseek-v4-flash
          name: DeepSeek V4 Flash
          contextWindow: 131072
          maxTokens: 8192
          input: [text]
          reasoningEfforts:
            medium: medium
```

**凭据分离**（仅 DSH）：`settings.yaml` 只保存 `apiKeyEnv`（引用名），实际密钥保存在同级 `.credentials.yaml` 的 `refs` 下（`refs: { SENSENOVA_API_KEY: sk-... }`）：

- 加载 DSH 配置时自动查找同级凭据文件并读取密钥；找不到时密钥为空
- 密钥在 DSH 页与 opencode / pi-agent / oh-my-pi 页面间同步显示与编辑；保存 DSH 时写回 `.credentials.yaml`（重命名 `apiKeyEnv` 会清理旧 ref，清空密钥默认不删除旧 ref，避免误伤其他配置）
- 凭据文件中的未知 ref、`records` 等其他字段原样保留

## 注意事项（所有格式通用）

- `baseURL` 仅在 `api = anthropic-messages` 时去掉末尾 `/v1`（Anthropic 官方域名为根地址），`openai-completions` 等其他 api 必须保留 `/v1`；页面显示与保存均按此规则
- provider / model 只保存各自支持的字段，其他格式的方言字段不会互相泄漏
- 跨格式写入“先读后合并”：目标文件已有配置与未知字段原样保留；DSH 当前文件保存以 raw 为基底，未做任何修改时保留原始 YAML 文本

## 平台与安全说明

- 本工具当前**仅支持 Windows**（依赖 Win32 光标子系统、微软雅黑字体路径与 `wsl` 命令）。
- 配置文件中的 `apiKey` 以**明文**读取与写回（与 opencode / pi-agent 本身的存储方式一致），请勿将配置文件提交到公开仓库；DSH 的实际密钥存放于同级 `.credentials.yaml`，同样为明文，请勿提交。
- 保存到 opencode / pi-agent 目标时采用“先读后合并”策略：仅更新 `agent`/`provider`（或 `providers`）字段，目标文件其余配置（如 `mcp`、`instructions`）原样保留。
- 默认保存目标为 Windows 本地路径；WSL 侧仅在勾选「WSL同步」后写入，保存前会按当前页面检测对应 agent 是否已在 WSL 安装（未安装则禁用勾选并跳过同步）。
