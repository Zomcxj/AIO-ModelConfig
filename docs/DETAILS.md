# ModelHarbor 功能细节

本文为 [README](../README.md) 的补充：功能说明、各格式字段对照、配置示例、注意事项与平台安全说明。

## 技术栈

Rust（2021 edition）+ [eframe / egui](https://github.com/emilk/egui) 0.31；JSON 使用 serde_json，YAML 使用 serde_yaml_ng，文件对话框使用 rfd，网络请求使用 ureq。

## 构建运行

```bash
cargo build --release
```

产物为单文件可执行程序：`target/release/ModelHarbor.exe`

构建脚本在项目根存在 `assets/icon.png` 时调用 Python + Pillow 生成图标资源，需要 Python 与 `pillow`；不存在该源图时使用内置图标，无需 Python。

## 页面与格式

顶栏图标切换四个页面（opencode / pi / omp / DSH）。加载任意一份配置后，各页面共享同一份数据，修改 provider 参数在所有页面同步生效（provider / model 顺序亦跨页同步）；Agents 区块仅属于 opencode 页面。

各页表单按自身方言显示字段与枚举，无对应字段不显示占位：

- **opencode 页**：`options.baseURL` / `options.timeout` / `npm` 下拉 / `limit.context` / `modalities` / `variants`（none…ultra）
- **pi 页**：`baseUrl` / `apiKey` / `api` 下拉（pi KnownApi 10 值）/ `compat` / `contextWindow` / `maxTokens` / `input` / `thinkingLevelMap`（off/minimal…max）
- **oh-my-pi 页**：`baseUrl` / `apiKey` / `api` 下拉（omp 官方 9 值）/ `compat` / `contextWindow` / `maxTokens` / `input` / `thinking.efforts`（minimal…max）
- **DeepSeek Harness 页**：`baseURL` / `apiKeyEnv` + 实际密钥 / `api` 下拉 / `timeoutMs` / `retryPolicy.mode` / `retryPolicy.maxRetries` / `models`（`id` / `name` / `contextWindow` / `maxTokens` / `input` / `reasoningEfforts`）

## 获取模型

每个 provider 卡片与「新增 Provider」弹窗的 Models 标题右侧都有「获取模型」按钮，按 provider 的 api 类型请求模型列表并弹层展示：

- 展示为多列 checkbox 网格，高度固定，超出部分在卡片内滚动；请求中显示进度指示
- 已配置的模型自动勾选；勾选未配置的模型即新增，取消勾选不会删除已有配置
- 兼容 `data` / `models` / 裸数组三种响应结构（含 `models/` 前缀清理与去重）

## 延迟 / 连通性测试

- **厂商连通性**：Providers 标题行右侧「连通性测试」按钮，一键测试当前页面全部厂商，耗时显示在各厂商卡片名字右侧（失败显示错误码，悬停看完整错误）；卡片收起时依然可见
- **模型延迟**：每个 provider 的 Models 标题右侧「模型延迟」按钮，并发测试该 provider 的全部模型，结果显示在模型卡片头部，并实时显示测试进度；超时 8 秒
- 测试会消耗极少量 token（单条最短对话），请勿在计费敏感的账号上频繁测试

## 配置预览 / 编辑面板

右侧面板实时显示当前页面的待保存内容（与保存按钮同路径、同规则）：

- JSON / YAML **语法高亮**（键、字符串、数字、布尔、注释分色）
- 文本框可直接编辑：改动实时应用到左侧表单；停止输入约 0.8 秒后自动保存
- 格式错误时在标题行提示，编辑内容不会被写盘
- `Ctrl+F` 查找，Enter / Shift+Enter 跳转上下一个命中，Esc 关闭
- 面板左边缘的分隔条可拖动调整宽度，窗口缩放时按调整后的比例适配

## 保存与 WSL 同步

- 每页有独立保存按钮与写入路径，默认写 Windows 本地路径
- 当前文件属于本页格式且已加载时写当前文件；手动改了路径但未加载时写入该路径并保留目标文件其余配置
- 跨格式写入只更新 `agent` / `provider`（或 `providers`）字段，目标文件其余配置（如 `mcp`、`instructions`）原样保留，不产生空对象污染
- 勾选「WSL同步」后同时写入 WSL 侧对应路径；未在 WSL 中安装对应 agent 时禁用勾选

## 缺省值与字段映射

- 配置未写 `timeout` / `timeoutMs` 时显示默认 `180000` ms，未修改时不写回
- DSH 的 `retryPolicy.mode` 缺省显示 `normal`
- pi 的 `compat.requiresReasoningContentOnAssistantMessages` 与 omp 的 `compat.requiresReasoningContentForAllAssistantTurns` 相互映射；加载 opencode / DSH 或新建时默认不勾选

## 配置文件格式参考

### opencode

工具读取 / 写入 `opencode.json`：

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

### pi

工具读取 / 写入 `~/.pi/agent/models.json`：

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
    }
  }
}
```

### oh-my-pi

工具读取 / 写入 `~/.omp/agent/models.yml`（本地优先，本地不可用回落 WSL），YAML 格式，结构与 pi 同族：

```yaml
providers:
  my-gateway:
    baseUrl: https://gateway.example.com/v1
    api: openai-completions
    apiKey: sk-...
    authHeader: true            # 注入 Authorization: Bearer
    headers:                    # 原样保留
      X-Team: platform
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

### DeepSeek Harness（DSH）

工具读取 / 写入 `~/.dsh/settings.yaml`，只管理 `llm-pi-ai.providers`，其余顶层配置（`ui`、`conversation`、`agent-default-model`、插件设置等）原样保留：

```yaml
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

## 字段对照

| 字段 | opencode | pi | oh-my-pi |
| ------ | ---------- | ---------- | ---------- |
| Provider key | `provider.{name}` | `providers.{name}` | `providers.{name}` |
| Base URL | `options.baseURL` | `baseUrl` | `baseUrl` |
| API Key | `options.apiKey` | `apiKey` | `apiKey`（环境变量名或字面量） |
| 模型存储 | Map（key = model id） | Array（含 id 字段） | Array（含 id 字段） |
| 上下文长度 | `limit.context` | `contextWindow` | `contextWindow` |
| 输出限制 | `limit.output` | `maxTokens` | `maxTokens` |
| 输入模态 | `modalities.input` | `input` | `input` |
| API 类型 | `npm` | `api` | `api`（9 种枚举） |
| 推理档位 | `variants` | `thinkingLevelMap` | `thinking: {mode, efforts, effortMap}` |
| 工具调用 | `tool_call` | 不支持 | 不支持 |
| Agent 定义 | `agent` | 不支持 | 不支持 |
| 扩展字段 | 顶层字段保留 | 顶层字段保留 | provider / model 级字段保留 |

## 注意事项

- `baseURL` 仅在 `api = anthropic-messages` 时去掉末尾 `/v1`，其他 api 保留 `/v1`
- provider / model 只保存各自支持的字段，方言字段不会互相泄漏
- oh-my-pi 的 `apiKey` 为「环境变量名或字面量」语义；推理档位保存为官方 `thinking` 块
- 保存 YAML 时文件注释不会保留，输出为标准块风格
- DSH 的实际密钥保存在同级 `.credentials.yaml` 的 `refs` 下，加载时自动读取，保存时写回；凭据文件中的其他字段原样保留

## 平台与安全

- 当前**仅支持 Windows**
- 配置文件中的 `apiKey` 为**明文**，DSH 的 `.credentials.yaml` 同样为明文，请勿提交到公开仓库
