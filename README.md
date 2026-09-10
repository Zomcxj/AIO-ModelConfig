# ModelHarbor

可视化编辑 [`opencode`](https://opencode.ai)、[`pi-agent`](https://github.com/earendil-works/pi)、[`oh-my-pi`](https://github.com/can1357/oh-my-pi)（omp）与 [`DeepSeek Harness`](https://github.com/amorvincit-omnia/llm-pi-ai)（DSH）等 agent 配置文件的桌面 GUI 工具。

Rust + egui 构建，单文件可执行程序，无需安装运行时。

## 功能特性

- **多页面编辑**：顶栏各 agent 图标切换页面；加载任意一份配置，各页共享同一份数据，修改即时同步（provider / model 顺序亦跨页同步）
- **多格式互转**：任意加载、任意保存；跨格式写入「先读后合并」，不破坏目标文件已有配置
- **方言表单**：各页按自身格式显示字段与枚举（api 下拉、推理档位、字段标签），没有的字段不占位
- **卡片式管理**：Agents / Providers 增删改复制、拖拽排序、折叠展开；model 参数与推理档位编辑
- **获取模型**：每个 provider（含新增 Provider 弹窗）的 Models 标题右侧「获取模型」按钮，从提供商 `/models` 接口拉取模型列表，checkbox 展示（最多 5 列、固定 15 行高、内部滚动），已配置自动打勾，勾选未配置模型即新增
- **延迟 / 连通性测试**：Providers 标题行「连通性测试」一键并发测试全部厂商接口（结果直接显示在各厂商名字右侧）；「模型延迟」并发测试某 provider 的全部模型（每批 8 个，`max_tokens=1` 最小请求，8 秒超时），结果以 `123ms` 形式显示在每个模型卡片上，失败显示错误码（如 `HTTP 403`）
- **凭据处理**：各格式按自身方式读写密钥（DSH 为 `apiKeyEnv` 引用 + 同级 `.credentials.yaml` 实际密钥），加载自动读取、跨页同步显示、保存写回，未知字段原样保留
- **分页保存与 WSL 同步**：每页独立保存按钮与写入路径，默认写 Windows 本地；勾选「WSL同步」且对应 agent 在 WSL 中已安装时同步写入 WSL 侧（保存前按页面检测安装）
- **缺省值**：配置文件未写 `timeout` / `timeoutMs` 时默认显示 `180000` ms、`retryPolicy.mode` 缺省 `normal`；未修改不写回，不污染配置；pi/omp 的 `requiresReasoningContent*` 相互映射，加载 opencode/dsh 时默认不勾选
- **其他**：自动格式检测、拖拽导入、pretty / compact 两种保存格式、五种主题、中文界面、Agents/Providers 吸顶标题（滚动时始终可见）、顶栏图标化切换（悬停显示名称）

## 构建运行

```bash
cargo build --release
```

产物为单文件可执行程序：`target/release/model-harbor.exe`

## 更多细节

字段对照表、分页方言表单说明、DeepSeek Harness 配置说明、oh-my-pi 注意事项、技术栈、构建细节、平台与安全说明，见 **[docs/DETAILS.md](docs/DETAILS.md)**。

## 许可证

见 [LICENSE](LICENSE)。
