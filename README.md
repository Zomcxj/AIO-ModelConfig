# ModelHarbor

可视化编辑 [`opencode`](https://opencode.ai)、[`pi-agent`](https://github.com/earendil-works/pi) 与 [`oh-my-pi`](https://github.com/can1357/oh-my-pi)（omp）配置文件的桌面 GUI 工具。

Rust + egui 构建，单文件可执行程序，无需安装运行时。

## 功能特性

- **多页面编辑**：顶栏三个 agent 图标切换页面；加载任意一份配置，三页共享同一份数据，修改即时同步
- **三格式互转**：opencode / pi-agent / oh-my-pi 任意加载、任意保存；跨格式写入“先读后合并”，不破坏目标文件已有配置
- **方言表单**：各页按自身格式显示字段与枚举（api 下拉、推理档位、字段标签），没有的字段不占位
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

字段对照表、分页方言表单说明、oh-my-pi 注意事项、技术栈、构建细节、平台与安全说明，见 **[docs/DETAILS.md](docs/DETAILS.md)**。

## 许可证

见 [LICENSE](LICENSE)。
