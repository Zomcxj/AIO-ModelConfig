# ModelHarbor

可视化编辑 [`opencode`](https://opencode.ai)、[`pi-agent`](https://github.com/earendil-works/pi)、[`oh-my-pi`](https://github.com/can1357/oh-my-pi)（omp）与 [`DeepSeek Harness`](https://github.com/amorvincit-omnia/llm-pi-ai)（DSH）配置文件的桌面 GUI 工具。

Rust + egui 构建，单文件可执行程序，无需安装运行时。

## 功能特性

- **多页面编辑**：顶栏四个 agent 图标切换页面；加载任意一份配置，四页共享同一份数据，修改即时同步
- **四格式互转**：opencode / pi-agent / oh-my-pi / DeepSeek Harness 任意加载、任意保存；跨格式写入“先读后合并”，不破坏目标文件已有配置
- **DSH 独立后端**：读写 `~/.dsh/settings.yaml` 的 `llm-pi-ai.providers`；`apiKeyEnv` 存凭据引用名，实际密钥存同级 `.credentials.yaml` 的 `refs`，加载时读取并在各页面同步显示、保存时写回；未管理字段（`ui`、`agent-default-model`、插件设置等）原样保留
- **方言表单**：各页按自身格式显示字段与枚举（api 下拉、推理档位、字段标签），没有的字段不占位；DSH 页面管理 `baseURL`、`timeoutMs`、`retryPolicy.mode`/`maxRetries`、`api`、`models`
- **获取模型**：每个 provider（含新增 Provider 弹窗）的 Models 标题右侧“获取模型”按钮，从提供商 `/models` 接口拉取模型列表，checkbox 展示，已配置自动打勾，勾选未配置模型即新增
- **分页保存**：每页独立保存按钮与写入路径（本地优先，WSL 回落）
- **WSL 同步**：勾选后保存时一键同步 WSL 侧已安装 agent 的配置
- **卡片式管理**：Agents / Providers 增删改复制、拖拽排序、折叠展开；model 参数与推理档位编辑
- **其他**：自动格式检测、拖拽导入、pretty / compact 两种保存格式、五种主题、中文界面

## 构建运行

```bash
cargo build --release
```

产物为单文件可执行程序：`target/release/model-harbor.exe`

## 更多细节

字段对照表、分页方言表单说明、DeepSeek Harness 配置说明、oh-my-pi 注意事项、技术栈、构建细节、平台与安全说明，见 **[docs/DETAILS.md](docs/DETAILS.md)**。

## 许可证

见 [LICENSE](LICENSE)。
