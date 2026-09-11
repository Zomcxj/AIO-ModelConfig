use crate::backends;
use crate::convert;
use crate::credentials;
use crate::format::{ConfigFormat, ConfigPaths};
use crate::model::{AgentRow, ModelRow, ProviderRow};
use crate::theme::Theme;
use crate::ui::{card_frame, card_list, field_label, move_item, numeric_text_edit, secret_text_edit, DragHandle};
use crate::util::{self, is_wsl_path, parse_number_text, show_file_dialog};
use eframe::egui;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum SaveFormat {
    Current,
    #[default]
    Compact,
}

impl SaveFormat {
    fn label(self) -> &'static str {
        match self {
            Self::Current => "默认格式",
            Self::Compact => "压缩格式",
        }
    }
}

/// 一个保存目标的运行时状态（可用性 / 解析路径 / 勾选）。
struct SaveTarget {
    backend: ConfigFormat,
    available: bool,
    path: String,
}

/// 单个 provider 的模型获取状态（后台线程 + 通道）。
struct ModelFetchState {
    rx: Option<std::sync::mpsc::Receiver<Result<Vec<String>, String>>>,
    result: Option<Result<Vec<String>, String>>,
}

/// 新增 Provider 表单使用固定的内部 key 保存获取状态。
const NEW_PROVIDER_FETCH_KEY: &str = "__new_provider__";

/// 吸顶标题占位：在内容流中预留标题行高度，返回绘制锚点。
/// 必须与 [`sticky_end`] 配对，并在 section 内容渲染完成后调用 sticky_end，
/// 以保证标题最后绘制（否则会被下方滚动内容覆盖）。
fn sticky_begin(ui: &mut egui::Ui, height: f32) -> (f32, f32, f32, f32) {
    let avail = ui.available_rect_before_wrap();
    ui.allocate_exact_size(egui::vec2(avail.width(), height), egui::Sense::hover());
    (avail.top(), avail.left(), avail.right(), height)
}

/// 绘制吸顶标题：未滚过时留在内容流中；滚动越过视口顶部后吸附在滚动区顶部。
fn sticky_end(ui: &mut egui::Ui, anchor: (f32, f32, f32, f32), paint: impl FnOnce(&mut egui::Ui)) {
    let (top, left, right, height) = anchor;
    let clip_top = ui.clip_rect().top();
    let y = top.max(clip_top);
    let target = egui::Rect::from_min_max(egui::pos2(left, y), egui::pos2(right, y + height));
    if !ui.clip_rect().intersects(target) {
        return;
    }
    // 用 new_child 而非 scope_builder：后者会推进父 cursor 到吸顶位置，
    // 破坏内容流导致滚动区滚轮失效。
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(target));
    // 拦截层：吸顶条空白区域（标题文字间隙等）的点击/拖拽先被此层消费，
    // 不再穿透到其正下方的卡片控件（误删/误展开/误拖放目标）。
    // 先注册，后续的按钮仍在其上层优先响应。
    child.interact(
        target,
        child.id().with("sticky-block"),
        egui::Sense::click_and_drag(),
    );
    child
        .painter()
        .rect_filled(target, 0.0, ui.visuals().panel_fill);
    paint(&mut child);
    // 标题下边线：吸顶时也能与内容分隔
    child.painter().hline(
        target.x_range(),
        target.bottom() - 1.0,
        ui.visuals().widgets.noninteractive.bg_stroke,
    );
}

/// 错误摘要：HTTP 状态码简写（如 HTTP 403），其他错误截断为短文本。
fn short_err(err: &str) -> String {
    if let Some(rest) = err.strip_prefix("HTTP ") {
        let code = rest.split('（').next().unwrap_or(rest);
        format!("HTTP {}", code)
    } else {
        let t = err.trim();
        if t.chars().count() > 24 {
            let mut s: String = t.chars().take(24).collect();
            s.push('…');
            s
        } else {
            t.to_string()
        }
    }
}

/// 网络错误文本脱敏：ureq 的 Transport Display 会包含目标 URL，
/// 若用户把凭据放进 URL query（如 `?key=sk-…`）会随错误泄漏到
/// 状态栏/悬停提示；剥离 URL 的 query/fragment 后返回。
fn sanitize_network_error(text: &str) -> String {
    const MARK: &str = "for URL \"";
    if let Some(idx) = text.find(MARK) {
        let head = &text[..idx + MARK.len()];
        let rest = &text[idx + MARK.len()..];
        let url = rest.split('"').next().unwrap_or(rest);
        let cut = url.find(['?', '#']).unwrap_or(url.len());
        format!("{}{}\"", head, &url[..cut])
    } else {
        text.to_string()
    }
}

/// 单个 provider 的延迟测试状态（provider 级 + 模型级并发）。
#[derive(Default)]
struct LatencyState {
    /// provider 级：模型列表接口往返耗时（毫秒）。
    provider: Option<Result<u64, String>>,
    provider_rx: Option<std::sync::mpsc::Receiver<Result<u64, String>>>,
    /// 模型级：模型 id → 往返耗时（毫秒）。
    models: HashMap<String, Result<u64, String>>,
    model_rx: Option<std::sync::mpsc::Receiver<(String, Result<u64, String>)>>,
    /// 模型级测试进度：已完成 / 总数。
    done: usize,
    total: usize,
}

/// 延迟测试用的 HTTP 客户端（较短超时，避免卡住 UI 线程池）。
fn latency_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(10))
        .timeout_read(std::time::Duration::from_secs(30))
        .build()
}

fn http_error(err: ureq::Error, elapsed: u64) -> String {
    match err {
        ureq::Error::Status(code, _) => format!("HTTP {}（{} ms）", code, elapsed),
        ureq::Error::Transport(t) => {
            format!("网络错误：{}", sanitize_network_error(&t.to_string()))
        }
    }
}

/// 测量 provider 模型列表接口的往返延迟（毫秒）。
fn measure_provider_latency(url: &str, secret: &str, api: &str) -> Result<u64, String> {
    if url.is_empty() {
        return Err("缺少 baseURL".to_string());
    }
    if secret.is_empty() {
        return Err("缺少 API Key".to_string());
    }
    let agent = latency_agent();
    let mut request = agent
        .get(url)
        .set("User-Agent", "model-harbor")
        .set("Accept", "application/json");
    if api == "anthropic-messages" {
        request = request
            .set("x-api-key", secret)
            .set("anthropic-version", "2023-06-01");
    } else {
        request = request.set("Authorization", &format!("Bearer {}", secret));
    }
    let started = std::time::Instant::now();
    let result = request.call();
    let elapsed = started.elapsed().as_millis() as u64;
    match result {
        Ok(_) => Ok(elapsed),
        Err(err) => Err(http_error(err, elapsed)),
    }
}

/// 模型对话接口地址（用于最小请求延迟测试）。
fn chat_url(base_url: &str, api: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if api == "anthropic-messages" {
        if base.ends_with("/v1") {
            format!("{}/messages", base)
        } else {
            format!("{}/v1/messages", base)
        }
    } else {
        format!("{}/chat/completions", base)
    }
}

/// 对单个模型发一个最小请求，测量往返延迟（毫秒）。
/// max_tokens=1 使消耗最小；失败仍会报出耗时，便于判断服务是否可达。
fn measure_model_latency(
    base_url: &str,
    secret: &str,
    api: &str,
    model: &str,
) -> Result<u64, String> {
    if base_url.trim().is_empty() {
        return Err("缺少 baseURL".to_string());
    }
    if secret.is_empty() {
        return Err("缺少 API Key".to_string());
    }
    let url = chat_url(base_url, api);
    // 模型延迟测试的读取超时固定 8 秒：超过即视为超时。
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(5))
        .timeout_read(std::time::Duration::from_millis(8000))
        .build();
    let started = std::time::Instant::now();
    let result = if api == "anthropic-messages" {
        let body = serde_json::json!({
            "model": model,
            "max_tokens": 1,
            "messages": [{"role": "user", "content": "ping"}]
        });
        agent
            .post(&url)
            .set("x-api-key", secret)
            .set("anthropic-version", "2023-06-01")
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())
    } else {
        let body = serde_json::json!({
            "model": model,
            "max_tokens": 1,
            "stream": false,
            "messages": [{"role": "user", "content": "ping"}]
        });
        agent
            .post(&url)
            .set("Authorization", &format!("Bearer {}", secret))
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())
    };
    let elapsed = started.elapsed().as_millis() as u64;
    if elapsed >= 8000 {
        return Err(format!("超时（{} ms）", elapsed));
    }
    match result {
        Ok(_) => Ok(elapsed),
        Err(err) => Err(http_error(err, elapsed)),
    }
}

/// 调用 OpenAI 兼容 /models 接口获取模型 id 列表（后台线程内执行）。
fn fetch_models_remote(url: &str, secret: &str, api: &str) -> Result<Vec<String>, String> {
    if url.is_empty() {
        return Err("缺少 baseURL，无法获取模型".to_string());
    }
    if secret.is_empty() {
        return Err("缺少 API Key，无法获取模型".to_string());
    }
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(10))
        .timeout_read(std::time::Duration::from_secs(30))
        .build();
    let mut request = agent
        .get(url)
        .set("User-Agent", "model-harbor")
        .set("Accept", "application/json");
    if api == "anthropic-messages" {
        request = request
            .set("x-api-key", secret)
            .set("anthropic-version", "2023-06-01");
    } else {
        request = request.set("Authorization", &format!("Bearer {}", secret));
    }
    let response = request.call().map_err(|err| match err {
        ureq::Error::Status(code, resp) => format!("HTTP {}：{}", code, resp.status_text()),
        ureq::Error::Transport(transport) => format!(
            "网络错误：{}",
            sanitize_network_error(&transport.to_string())
        ),
    })?;
    let text = response.into_string().map_err(|err| err.to_string())?;
    parse_models_response(&text)
}

/// 解析 /models 响应中的模型 id（兼容 OpenAI/Anthropic/Gemini 等格式）。
fn parse_models_response(text: &str) -> Result<Vec<String>, String> {
    let root: Value = serde_json::from_str(text).map_err(|err| {
        let snippet = text.chars().take(160).collect::<String>();
        format!("响应不是合法 JSON（{}）：{}", err, snippet)
    })?;
    if let Some(error) = root.get("error") {
        let msg = error
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| error.as_str())
            .unwrap_or("未知错误");
        return Err(msg.to_string());
    }
    fn push_ids(item: &Value, seen: &mut HashSet<String>, ids: &mut Vec<String>) {
        let raw = item
            .get("id")
            .and_then(Value::as_str)
            .or_else(|| item.get("name").and_then(Value::as_str))
            .unwrap_or("");
        let id = raw.trim().trim_start_matches("models/").to_string();
        if !id.is_empty() && seen.insert(id.clone()) {
            ids.push(id);
        }
    }
    let mut ids: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    if let Some(arr) = root.as_array() {
        for item in arr {
            push_ids(item, &mut seen, &mut ids);
        }
    }
    for key in ["data", "models"] {
        if let Some(arr) = root.get(key).and_then(Value::as_array) {
            for item in arr {
                push_ids(item, &mut seen, &mut ids);
            }
        }
    }
    Ok(ids)
}

/// 本页写入路径的解析结果。
#[derive(Clone)]
enum PageTarget {
    /// 当前文件（已加载）：整体替换 agent / provider。
    Current(String),
    /// 路径已修改但未重新加载：按“先读后合并”写入，不破坏目标文件已有配置。
    Modified(String),
    /// 该后端默认目标（Windows 本地路径；WSL 仅在勾选「WSL同步」后写入）。
    Default(String),
}

pub struct App {
    root: Value,
    agents: Vec<AgentRow>,
    providers: Vec<ProviderRow>,
    new_agent: AgentRow,
    new_provider: ProviderRow,
    config_path: String,
    /// 最近一次实际加载的路径（config_path 与之不等时按“未加载”处理，防止误覆盖）。
    loaded_path: String,
    status: String,
    show_new_agent: bool,
    show_new_provider: bool,
    agent_open: HashSet<String>,
    provider_open: HashSet<String>,
    variant_open: HashSet<String>,
    agent_drag_src: Option<String>,
    agent_drag_target: Option<String>,
    provider_drag_src: Option<String>,
    provider_drag_target: Option<String>,
    model_drag_src: Option<String>,
    model_drag_target: Option<String>,
    /// 每个 provider 的模型获取状态（key → 状态）。
    model_fetch: HashMap<String, ModelFetchState>,
    /// 已展开的模型获取面板（provider key）。
    model_fetch_open: HashSet<String>,
    /// 每个 provider 的延迟测试状态（key → 状态）。
    latency: HashMap<String, LatencyState>,
    theme: Theme,
    save_format: SaveFormat,
    /// 滚轮切换保存格式的门门：一次连续滚动手势只切换一次。
    save_format_wheel_latch: bool,
    source_format: ConfigFormat,
    config_paths: ConfigPaths,
    targets: Vec<SaveTarget>,
    current_page: ConfigFormat,
    sync_wsl: bool,
    show_agents_section: bool,
    show_providers_section: bool,
    /// 全局 API Key 显隐：一键控制所有密钥输入框的明文/掩码显示。
    show_api_keys: bool,
    /// 右侧配置预览/编辑面板是否打开。
    show_preview: bool,
    /// 预览文本框是否持有焦点（编辑中以文本为准，失焦后以组件状态为准）。
    preview_focused: bool,
    /// 预览文本框缓冲（待保存文档；失焦时由组件状态实时重写）。
    preview_draft: String,
    /// 最近一次文本编辑的时间点（ctx 时间），用于防抖自动保存。
    preview_dirty_at: Option<f64>,
    /// 最近一次文本解析是否成功（解析失败不写盘、不覆盖文本）。
    preview_parse_ok: bool,
    /// 最近一次预览文本解析失败的报错（成功时为 None），用于面板内红字提示。
    preview_parse_error: Option<String>,
    /// 光标所在行（1-based；失焦时保留最后位置）。
    preview_cursor_line: usize,
    load_error: Option<String>,
    pi_extras: Value,
    /// 各后端官方图标纹理（与 BACKENDS 顺序对齐，首帧惰性加载）。
    backend_icons: Vec<Option<egui::TextureHandle>>,
}

impl Default for App {
    fn default() -> Self {
        let paths = ConfigPaths::default();
        let (format, path) =
            ConfigPaths::detect().unwrap_or((ConfigFormat::Opencode, String::new()));
        let mut app = Self {
            root: Value::Object(Map::new()),
            agents: Vec::new(),
            providers: Vec::new(),
            new_agent: AgentRow::new(),
            new_provider: ProviderRow::new(),
            config_path: path,
            loaded_path: String::new(),
            status: String::new(),
            show_new_agent: false,
            show_new_provider: false,
            agent_open: HashSet::new(),
            provider_open: HashSet::new(),
            variant_open: HashSet::new(),
            agent_drag_src: None,
            agent_drag_target: None,
            provider_drag_src: None,
            provider_drag_target: None,
            model_drag_src: None,
            model_drag_target: None,
            model_fetch: HashMap::new(),
            model_fetch_open: HashSet::new(),
            latency: HashMap::new(),
            theme: Theme::default(),
            save_format: SaveFormat::default(),
            save_format_wheel_latch: false,
            source_format: format,
            config_paths: paths,
            targets: Vec::new(),
            current_page: format,
            sync_wsl: false,
            show_agents_section: true,
            show_providers_section: true,
            show_api_keys: false,
            show_preview: false,
            preview_focused: false,
            preview_draft: String::new(),
            preview_dirty_at: None,
            preview_parse_ok: true,
            preview_parse_error: None,
            preview_cursor_line: 1,
            load_error: None,
            pi_extras: Value::Object(Map::new()),
            backend_icons: Vec::new(),
        };
        app.apply_load();
        app
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let dropped = ctx.input(|i| {
            i.raw
                .dropped_files
                .first()
                .and_then(|f| f.path.as_ref().map(|p| p.to_string_lossy().into_owned()))
        });
        if let Some(path) = dropped {
            self.config_path = path;
            self.reload();
        }
        // 首帧惰性加载各后端官方图标
        if self.backend_icons.is_empty() {
            self.backend_icons = backends::BACKENDS
                .iter()
                .map(|b| {
                    b.icon_rgba().map(|(rgba, w, h)| {
                        let image = egui::ColorImage::from_rgba_unmultiplied(
                            [w as usize, h as usize],
                            rgba,
                        );
                        ctx.load_texture(
                            format!("backend_icon_{}", b.id().label()),
                            image,
                            egui::TextureOptions::LINEAR,
                        )
                    })
                })
                .collect();
        }
        self.poll_model_fetch();
        self.poll_latency();
        self.ui_top_bar(ctx);
        self.ui_status_bar(ctx);
        // 右侧配置预览/编辑面板：在中央内容区之前挂载，宽度可拖拽调整。
        if self.show_preview {
            egui::SidePanel::right("preview_panel")
                .default_width(440.0)
                .min_width(300.0)
                .show(ctx, |ui| {
                    self.ui_preview_panel(ui);
                });
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            self.ui_page_header(ui);
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .drag_to_scroll(false)
                .show(ui, |ui| {
                    ui.add_space(4.0);
                    // Agents 仅属于 opencode 页面；区块标题吸顶，滚动时始终显示在顶部。
                    if self.current_page == ConfigFormat::Opencode {
                        self.ui_agents_section(ui);
                        ui.add_space(8.0);
                    }
                    self.ui_providers_section(ui);
                    ui.add_space(8.0);
                });
        });
        self.paint_drag_ghost(ctx);
        // 仅拖拽中显示抓取光标（避免任意控件按下时全局变光标）
        let dragging = self.agent_drag_src.is_some()
            || self.provider_drag_src.is_some()
            || self.model_drag_src.is_some();
        #[cfg(target_os = "windows")]
        crate::cursor::set_custom_cursor_active(dragging);
    }
}

impl App {
    /// 解析各保存目标的可用性与实际路径（避免在渲染循环中频繁拉起 wsl 进程）。
    fn refresh_targets(&mut self) {
        // 默认目标固定为 Windows 本地路径；WSL 侧仅通过“WSL同步”勾选写入，
        // 且写入前按页面检测对应 agent 是否已安装。
        self.targets = backends::BACKENDS
            .iter()
            .map(|b| {
                let id = b.id();
                let local = self.config_paths.local_path(id);
                SaveTarget {
                    backend: id,
                    available: self.config_paths.validate_target(id),
                    path: local,
                }
            })
            .collect();
    }

    /// 按 source_format 加载当前 config_path；失败时置空数据并记录 load_error。
    fn apply_load(&mut self) {
        let path = self.config_path.clone();
        self.loaded_path = path.clone();
        let result = backends::load_backend(self.source_format, &path);
        match result {
            Ok(load) => {
                self.root = load.root;
                self.agents = load.agents;
                self.providers = load.providers;
                self.pi_extras = load.extras;
                self.load_error = None;
                self.status = format!(
                    "已加载 ({}): {} agents, {} providers",
                    self.source_format.label(),
                    self.agents.len(),
                    self.providers.len()
                );
            }
            Err(e) => {
                self.root = Value::Object(Map::new());
                self.agents = Vec::new();
                self.providers = Vec::new();
                self.pi_extras = Value::Object(Map::new());
                self.load_error = Some(e.clone());
                self.status = format!("加载失败: {}", e);
            }
        }
        self.agent_open = self.agents.iter().map(|a| a.key.clone()).collect();
        self.provider_open = self.providers.iter().map(|p| p.key.clone()).collect();
        // 重新加载后丢弃旧的模型获取状态
        self.model_fetch.clear();
        self.model_fetch_open.clear();
        self.latency.clear();
        // 加载后跳转到来源格式对应的页面
        self.current_page = self.source_format;
        self.refresh_targets();
    }

    fn ui_top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.style_mut().spacing.interact_size.y = 18.0;
            // 第一行：页面切换 / 来源 / 右侧 WSL 同步 + 主题
            ui.horizontal(|ui| {
                let icons: Vec<Option<egui::TextureHandle>> = self.backend_icons.clone();
                for (i, b) in backends::BACKENDS.iter().enumerate() {
                    let id = b.id();
                    let btn = match icons.get(i).and_then(|o| o.as_ref()) {
                        Some(tex) => egui::Button::image(
                            egui::Image::from_texture(tex)
                                .fit_to_exact_size(egui::vec2(16.0, 16.0)),
                        ),
                        None => egui::Button::new(""),
                    };
                    let is_selected = self.current_page == id;
                    let btn = if is_selected {
                        // 选中态：填充 + 描边，与未选中图标拉开视觉层级
                        btn.fill(ui.visuals().selection.bg_fill).stroke(
                            egui::Stroke::new(1.0, ui.visuals().selection.stroke.color),
                        )
                    } else {
                        btn
                    };
                    // 只显示图标，鼠标悬停提示名称；加大点击区便于操作
                    let btn_resp = ui
                        .add(btn.min_size(egui::vec2(24.0, 22.0)))
                        .on_hover_text(id.label())
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if btn_resp.clicked() {
                        if id == ConfigFormat::DeepSeekHarness
                            && self.current_page != ConfigFormat::DeepSeekHarness
                        {
                            self.project_dsh_credentials();
                        }
                        self.sync_provider_secrets(id);
                        // 对应 agent 未在 WSL 安装的页面：关闭并禁用 WSL 同步
                        if backends::wsl_target(id).is_none() {
                            self.sync_wsl = false;
                        }
                        self.current_page = id;
                    }
                }
                ui.separator();
                // 右侧：WSL 同步 + 主题
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // 按当前页面检测对应 agent 是否已在 WSL 安装
                    let current = self.current_page;
                    let wsl_installed = backends::wsl_target(current).is_some();
                    let wsl_tip = if wsl_installed {
                        format!("保存时同步写入 WSL 侧 {} 的配置", current.label())
                    } else {
                        format!(
                            "WSL 中未检测到 {} 安装（配置文件或其目录均不存在），保存仅写 Windows 本地",
                            current.label()
                        )
                    };
                    ui.add_enabled(
                        wsl_installed,
                        egui::Checkbox::new(&mut self.sync_wsl, "WSL同步"),
                    )
                    .on_hover_text(wsl_tip);
                    ui.separator();
                    ui.label("主题:");
                    let theme_btn = ui.button(self.theme.label());
                    let popup_id = ui.make_persistent_id("theme_popup");
                    if theme_btn.clicked() {
                        ui.memory_mut(|m| m.toggle_popup(popup_id));
                    }
                    egui::popup_below_widget(
                        ui,
                        popup_id,
                        &theme_btn,
                        egui::PopupCloseBehavior::CloseOnClick,
                        |ui| {
                            ui.set_min_width(80.0);
                            for t in Theme::ALL {
                                if ui.selectable_label(self.theme == t, t.label()).clicked() {
                                    self.theme = t;
                                    t.apply(ctx);
                                }
                            }
                        },
                    );
                });
            });
            // 第二行：配置文件 / 保存格式
            ui.horizontal(|ui| {
                ui.label("配置文件:");
                let path_resp =
                    ui.add(egui::TextEdit::singleline(&mut self.config_path).desired_width(420.0));
                // 回车确认：按当前输入路径重新加载（egui 单行编辑回车即失焦）
                if path_resp.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && self.config_path != self.loaded_path
                {
                    self.reload();
                }
                // 文件来源显示在原本“加载”按钮的位置；加载改为回车或“浏览”。
                // 只显示“来源：”+ 各后端官方图标（名称见悬停提示）。
                ui.label(egui::RichText::new("来源:").weak());
                if let Some(icon) = self.icon_for(self.source_format) {
                    ui.add(
                        egui::Image::from_texture(icon)
                            .fit_to_exact_size(egui::vec2(14.0, 14.0)),
                    )
                    .on_hover_text(self.source_format.label());
                } else {
                    ui.label(egui::RichText::new(self.source_format.label()).weak());
                }
                if ui.button("浏览").clicked() {
                    if let Some(p) = show_file_dialog() {
                        self.config_path = p;
                        self.reload();
                    }
                }
                if !self.config_path.is_empty() && self.config_path != self.loaded_path {
                    ui.label(egui::RichText::new("未加载").small().color(egui::Color32::from_rgb(220, 160, 60)))
                        .on_hover_text("路径已修改但未加载：保存时将按“先读后合并”写入该路径（不破坏目标文件已有配置）。\n在此按回车可切换到该文件。");
                }
                ui.separator();
                ui.label("保存格式:");
                let format_btn = ui.button(self.save_format.label());
                if format_btn.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let hovering = format_btn.hovered();
                let scroll = ui
                    .input(|i| i.events.iter().any(|e| matches!(e, egui::Event::MouseWheel { .. })));
                // 滚轮切换：一次连续滚动手势只切换一次，避免快速滚动时来回翻转
                if hovering && scroll && !self.save_format_wheel_latch {
                    self.save_format = match self.save_format {
                        SaveFormat::Current => SaveFormat::Compact,
                        SaveFormat::Compact => SaveFormat::Current,
                    };
                    self.save_format_wheel_latch = true;
                }
                if !scroll || !hovering {
                    self.save_format_wheel_latch = false;
                }
                if format_btn.clicked() {
                    self.save_format = match self.save_format {
                        SaveFormat::Current => SaveFormat::Compact,
                        SaveFormat::Compact => SaveFormat::Current,
                    };
                }
            });
        });
    }

    fn ui_status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("bottom")
            .exact_height(32.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some(err) = &self.load_error {
                        ui.label(
                            egui::RichText::new(format!("⚠ 加载失败: {}", err))
                                .color(egui::Color32::from_rgb(220, 90, 90)),
                        );
                    }
                    ui.label(egui::RichText::new(&self.status).weak());
                    // 右侧：当前页 + 数量统计，随时可见页面身份
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "agents: {} | providers: {}",
                                self.agents.len(),
                                self.providers.len()
                            ))
                            .weak(),
                        );
                        ui.label(
                            egui::RichText::new(format!("当前页: {}", self.current_page.label()))
                                .weak(),
                        );
                    });
                });
            });
    }

    /// Agents 区块：标题行吸顶（滚动时始终显示在顶部），内容紧跟其下。
    fn ui_agents_section(&mut self, ui: &mut egui::Ui) {
        let anchor = sticky_begin(ui, 30.0);
        if self.show_agents_section {
            let matched: Vec<usize> = (0..self.agents.len()).collect();

            if self.agents.is_empty() && !self.show_new_agent {
                self.show_new_agent = true;
            }

            let mut to_remove: Option<usize> = None;
            let mut to_copy: Option<usize> = None;
            let mut hover_target: Option<String> = None;
            card_list(ui, &matched, 0.0, |ui, idx| {
                self.render_agent_card(ui, idx, &mut to_remove, &mut to_copy, &mut hover_target);
            });
            if let Some(idx) = to_remove {
                self.agents.remove(idx);
                self.status = "已删除 agent".into();
            }
            if let Some(idx) = to_copy {
                let mut a = self.agents[idx].clone();
                a.key = format!("{}_copy", a.key);
                self.agents.push(a);
                self.status = "已复制 agent".into();
            }
            if self.agent_drag_src.is_some() {
                self.agent_drag_target = hover_target;
            } else {
                self.agent_drag_target = None;
            }

            ui.add_space(6.0);
            if ui.button("新增 Agent").clicked() {
                self.show_new_agent = !self.show_new_agent;
            }
            if self.show_new_agent {
                self.ui_new_agent_form(ui);
            }
        }
        sticky_end(ui, anchor, |ui| {
            ui.horizontal(|ui| {
                ui.strong("Agents");
                let btn_label = if self.show_agents_section {
                    "隐藏"
                } else {
                    "展开"
                };
                if ui.button(btn_label).clicked() {
                    self.show_agents_section = !self.show_agents_section;
                }
                if self.show_agents_section && !self.agents.is_empty() {
                    let all_open = self.agents.iter().all(|a| self.agent_open.contains(&a.key));
                    if ui
                        .button(if all_open {
                            "收起全部卡片"
                        } else {
                            "展开全部卡片"
                        })
                        .clicked()
                    {
                        if all_open {
                            self.agent_open.clear();
                        } else {
                            self.agent_open = self.agents.iter().map(|a| a.key.clone()).collect();
                        }
                    }
                }
            });
        });
    }

    fn render_agent_card(
        &mut self,
        ui: &mut egui::Ui,
        idx: usize,
        to_remove: &mut Option<usize>,
        to_copy: &mut Option<usize>,
        hover_target: &mut Option<String>,
    ) {
        let key = self.agents[idx].key.clone();
        let open = self.agent_open.contains(&key);
        let highlight = if self.agent_drag_target.as_deref() == Some(key.as_str()) {
            2
        } else if self.agent_drag_src.as_deref() == Some(key.as_str()) {
            1
        } else {
            0
        };
        let resp = card_frame(ui, open, highlight, |ui| {
            ui.horizontal(|ui| {
                let h = ui.add(DragHandle);
                if h.drag_started() {
                    self.agent_drag_src = Some(key.clone());
                    self.agent_drag_target = None;
                }
                if h.drag_stopped() {
                    if self.agent_drag_src == Some(key.clone()) {
                        if let Some(dst) = self.agent_drag_target.clone() {
                            let s = self.agents.iter().position(|a| a.key == key);
                            let d = self.agents.iter().position(|a| a.key == dst);
                            if let (Some(s), Some(d)) = (s, d) {
                                move_item(&mut self.agents, s, d);
                            }
                        }
                    }
                    self.agent_drag_src = None;
                    self.agent_drag_target = None;
                }
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(if open { "▼" } else { "▶" }).size(14.0),
                        )
                        .frame(false),
                    )
                    .clicked()
                {
                    if open {
                        self.agent_open.remove(&key);
                    } else {
                        self.agent_open.insert(key.clone());
                    }
                }
                ui.strong(&self.agents[idx].key);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("删除").clicked() {
                        *to_remove = Some(idx);
                    }
                    if ui.button("复制").clicked() {
                        *to_copy = Some(idx);
                    }
                });
            });
            if open {
                self.render_agent_form(ui, idx);
            }
        });
        if let Some(src_key) = &self.agent_drag_src {
            if src_key != &key && resp.contains_pointer() && hover_target.is_none() {
                *hover_target = Some(key.clone());
            }
        }
    }

    fn render_agent_form(&mut self, ui: &mut egui::Ui, idx: usize) {
        let prev_key = self.agents[idx].key.clone();
        let other_keys: HashSet<String> = self
            .agents
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != idx)
            .map(|(_, a)| a.key.trim().to_string())
            .collect();
        let a = &mut self.agents[idx];
        ui.horizontal_wrapped(|ui| {
            field_label(ui, 120.0, "key");
            let key_resp = ui.add(egui::TextEdit::singleline(&mut a.key).desired_width(120.0));
            if !a.key.trim().is_empty() && other_keys.contains(a.key.trim()) {
                key_resp.on_hover_text("key 与其他 agent 重复，保存将被阻止");
                ui.label(
                    egui::RichText::new("⚠ 重复")
                        .small()
                        .color(egui::Color32::from_rgb(220, 90, 90)),
                );
            }
            field_label(ui, 120.0, "mode");
            ui.add(egui::TextEdit::singleline(&mut a.mode).desired_width(120.0));
            field_label(ui, 120.0, "description");
            ui.add(egui::TextEdit::singleline(&mut a.description).desired_width(450.0));
        });
        ui.horizontal_wrapped(|ui| {
            field_label(ui, 120.0, "model");
            let mut model_options: Vec<String> = self
                .providers
                .iter()
                .flat_map(|p| p.models.iter().map(|m| format!("{}/{}", p.key, m.id)))
                .collect();
            model_options.extend_from_slice(&[
                "opencode/mimo-v2.5-free".into(),
                "opencode/big-pickle".into(),
            ]);
            model_options.sort();
            model_options.dedup();
            let current = a.model.clone();
            let mut selected_idx = model_options.iter().position(|m| m == &current);
            egui::ComboBox::from_id_salt(format!("agent_model_{}", a.key))
                .selected_text(if current.is_empty() {
                    "选择模型..."
                } else {
                    &current
                })
                .width(180.0)
                .show_ui(ui, |ui| {
                    for (i, model) in model_options.iter().enumerate() {
                        let is_selected = selected_idx == Some(i);
                        if ui.selectable_label(is_selected, model.as_str()).clicked() {
                            selected_idx = Some(i);
                        }
                    }
                });
            if let Some(idx) = selected_idx {
                a.model = model_options[idx].clone();
            }
            field_label(ui, 120.0, "variant");
            let variant_options = ["", "low", "medium", "high", "xhigh", "max", "ultra"];
            let current_variant = a.variant.clone();
            let mut selected_variant = variant_options
                .iter()
                .position(|v| *v == current_variant.as_str());
            egui::ComboBox::from_id_salt(format!("agent_variant_{}", a.key))
                .selected_text(if current_variant.is_empty() {
                    "选择..."
                } else {
                    &current_variant
                })
                .width(100.0)
                .show_ui(ui, |ui| {
                    for (i, v) in variant_options.iter().enumerate() {
                        let label = if v.is_empty() { "(空)" } else { v };
                        let is_selected = selected_variant == Some(i);
                        if ui.selectable_label(is_selected, label).clicked() {
                            selected_variant = Some(i);
                        }
                    }
                });
            if let Some(idx) = selected_variant {
                a.variant = variant_options[idx].to_string();
            }
        });
        ui.horizontal_wrapped(|ui| {
            field_label(ui, 120.0, "temperature");
            numeric_text_edit(ui, &mut a.temperature, 120.0, "");
            field_label(ui, 120.0, "color");
            ui.add(egui::TextEdit::singleline(&mut a.color).desired_width(120.0));
            field_label(ui, 120.0, "system");
            ui.add(egui::TextEdit::singleline(&mut a.system).desired_width(450.0));
        });
        // key 重命名后同步展开状态（避免改名导致卡片收起）
        let new_key = self.agents[idx].key.clone();
        if new_key != prev_key {
            self.sync_agent_rename(&prev_key, &new_key);
        }
    }

    fn ui_new_agent_form(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.horizontal_wrapped(|ui| {
                field_label(ui, 120.0, "key");
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_agent.key)
                        .hint_text("coding-assistant")
                        .desired_width(120.0),
                );
                field_label(ui, 120.0, "mode");
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_agent.mode)
                        .hint_text("subagent")
                        .desired_width(120.0),
                );
                field_label(ui, 120.0, "description");
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_agent.description)
                        .hint_text("简要描述此 agent 的用途")
                        .desired_width(450.0),
                );
            });
            ui.horizontal_wrapped(|ui| {
                field_label(ui, 120.0, "model");
                let mut model_options: Vec<String> = self
                    .providers
                    .iter()
                    .flat_map(|p| p.models.iter().map(|m| format!("{}/{}", p.key, m.id)))
                    .collect();
                model_options.extend_from_slice(&[
                    "opencode/mimo-v2.5-free".into(),
                    "opencode/big-pickle".into(),
                ]);
                model_options.sort();
                model_options.dedup();
                let current = self.new_agent.model.clone();
                let mut selected_idx = model_options.iter().position(|m| m == &current);
                let _response = egui::ComboBox::from_id_salt("new_agent_model")
                    .selected_text(if current.is_empty() {
                        "选择模型..."
                    } else {
                        &current
                    })
                    .width(180.0)
                    .show_ui(ui, |ui| {
                        for (i, model) in model_options.iter().enumerate() {
                            let is_selected = selected_idx == Some(i);
                            if ui.selectable_label(is_selected, model.as_str()).clicked() {
                                selected_idx = Some(i);
                            }
                        }
                    });
                if let Some(idx) = selected_idx {
                    self.new_agent.model = model_options[idx].clone();
                }
                field_label(ui, 120.0, "variant");
                let variant_options = ["", "low", "medium", "high", "xhigh", "max", "ultra"];
                let current_variant = self.new_agent.variant.clone();
                let mut selected_variant = variant_options
                    .iter()
                    .position(|v| *v == current_variant.as_str());
                egui::ComboBox::from_id_salt("new_agent_variant")
                    .selected_text(if current_variant.is_empty() {
                        "选择..."
                    } else {
                        &current_variant
                    })
                    .width(100.0)
                    .show_ui(ui, |ui| {
                        for (i, v) in variant_options.iter().enumerate() {
                            let label = if v.is_empty() { "(空)" } else { v };
                            let is_selected = selected_variant == Some(i);
                            if ui.selectable_label(is_selected, label).clicked() {
                                selected_variant = Some(i);
                            }
                        }
                    });
                if let Some(idx) = selected_variant {
                    self.new_agent.variant = variant_options[idx].to_string();
                }
            });
            ui.horizontal_wrapped(|ui| {
                field_label(ui, 120.0, "temperature");
                numeric_text_edit(ui, &mut self.new_agent.temperature, 120.0, "0.7");
                field_label(ui, 120.0, "color");
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_agent.color)
                        .hint_text("#00ccff")
                        .desired_width(120.0),
                );
                field_label(ui, 120.0, "system");
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_agent.system)
                        .hint_text("系统提示词")
                        .desired_width(450.0),
                );
            });
            ui.horizontal(|ui| {
                ui.add_space(60.0);
                if ui.button("确认").clicked() {
                    let key = self.new_agent.key.trim().to_string();
                    if key.is_empty() {
                        self.status = "请填写 agent key".into();
                    } else if self.agents.iter().any(|a| a.key.trim() == key) {
                        self.status = format!("agent key \"{}\" 已存在", key);
                    } else {
                        let na = self.new_agent.clone();
                        self.agents.push(na);
                        self.new_agent = AgentRow::new();
                        self.show_new_agent = false;
                        self.status = "已添加 agent".into();
                    }
                }
                if ui.button("取消").clicked() {
                    self.new_agent = AgentRow::new();
                    self.show_new_agent = false;
                }
            });
        });
    }

    /// 当前 agent 文件是否使用某个 provider 级字段。
    /// 判断范围是整个文件，不是单个 provider；切换到尚未加载的目标页时
    /// 使用完整 schema，保证新增 provider 可以输入所有专属字段。
    fn page_has_provider_field(&self, field: &str) -> bool {
        if self.source_format != self.current_page {
            return true;
        }
        self.providers
            .iter()
            .any(|provider| match self.current_page {
                ConfigFormat::Opencode => match field {
                    "base_url" => provider
                        .raw
                        .get("options")
                        .and_then(|v| v.get("baseURL"))
                        .is_some(),
                    "timeout" => provider
                        .raw
                        .get("options")
                        .and_then(|v| v.get("timeout"))
                        .is_some(),
                    _ => false,
                },
                ConfigFormat::PiAgent | ConfigFormat::OhMyPi => match field {
                    "base_url" => provider.raw.get("baseUrl").is_some(),
                    _ => false,
                },
                ConfigFormat::DeepSeekHarness => match field {
                    "base_url" => provider.raw.get("baseURL").is_some(),
                    _ => false,
                },
            })
    }

    /// 当前 agent 文件是否使用某个 model 级字段。字段存在性按整个
    /// agent 文件判断，避免单个模型缺字段时导致同一页面布局跳变。
    fn page_has_model_field(&self, field: &str) -> bool {
        if self.source_format != self.current_page {
            return true;
        }
        self.providers.iter().any(|provider| {
            provider.models.iter().any(|model| match self.current_page {
                ConfigFormat::Opencode => match field {
                    "name" => model.raw.get("name").is_some(),
                    "reasoning" => model.raw.get("reasoning").is_some(),
                    "tool_call" => model.raw.get("tool_call").is_some(),
                    "store" => model
                        .raw
                        .get("options")
                        .and_then(|v| v.get("store"))
                        .is_some(),
                    "context" => model
                        .raw
                        .get("limit")
                        .and_then(|v| v.get("context"))
                        .is_some(),
                    "output" => model
                        .raw
                        .get("limit")
                        .and_then(|v| v.get("output"))
                        .is_some(),
                    "input" => model
                        .raw
                        .get("modalities")
                        .and_then(|v| v.get("input"))
                        .is_some(),
                    "variants" => model.raw.get("variants").is_some(),
                    _ => false,
                },
                ConfigFormat::PiAgent | ConfigFormat::OhMyPi => match field {
                    "name" => model.raw.get("name").is_some(),
                    // reasoning 也可由 pi/omp 的 thinking 块 / thinkingLevelMap 表达，
                    // 只写了这些键时同样应显示（并勾选）reasoning。
                    "reasoning" => {
                        model.raw.get("reasoning").is_some()
                            || model.raw.get("thinkingLevelMap").is_some()
                            || model.raw.get("thinking").is_some()
                            || model.raw.get("reasoningEfforts").is_some()
                    }
                    "context" => model.raw.get("contextWindow").is_some(),
                    "output" => model.raw.get("maxTokens").is_some(),
                    "input" => model.raw.get("input").is_some(),
                    "variants" => {
                        model.raw.get("thinkingLevelMap").is_some()
                            || model.raw.get("thinking").is_some()
                    }
                    _ => false,
                },
                ConfigFormat::DeepSeekHarness => match field {
                    "name" => model.raw.get("name").is_some(),
                    "context" => model.raw.get("contextWindow").is_some(),
                    "output" => model.raw.get("maxTokens").is_some(),
                    "input" => model.raw.get("input").is_some(),
                    "variants" => model.raw.get("reasoningEfforts").is_some(),
                    _ => false,
                },
            })
        })
    }

    /// 将已加载的公共 provider 凭据投影到 DSH 专属字段。
    /// 只在进入 DSH 页面时执行一次，避免用户在页面内主动清空后被立即回填。
    fn project_dsh_credentials(&mut self) {
        for provider in &mut self.providers {
            if provider.api_key_env.trim().is_empty() {
                provider.api_key_env = credentials::default_env_name(&provider.key);
            }
        }
    }

    /// 页面切换时保持 provider 密钥一致：DSH 页使用 api_key_secret（对应
    /// .credentials.yaml 的 refs），其他页面使用 api_key。切换时把非空值
    /// 同步到目标页字段；若用户在 DSH 页明确清空过密钥（原本有、当前空），
    /// 不再用其他页面的旧值覆盖。
    fn sync_provider_secrets(&mut self, target: ConfigFormat) {
        for provider in &mut self.providers {
            if target == ConfigFormat::DeepSeekHarness {
                let cleared_on_dsh = !provider.original_api_key_secret.is_empty()
                    && provider.api_key_secret.is_empty();
                if !provider.api_key.trim().is_empty() && !cleared_on_dsh {
                    provider.api_key_secret = provider.api_key.clone();
                }
            } else {
                let cleared_on_dsh = !provider.original_api_key_secret.is_empty()
                    && provider.api_key_secret.is_empty();
                if cleared_on_dsh {
                    // 与 DSH 方向对称：在 DSH 页明确清空过的密钥（原本有、当前空）
                    // 不再用旧值填充其他页面，避免已清空的密钥被写回 opencode 等配置。
                    provider.api_key = String::new();
                } else if !provider.api_key_secret.trim().is_empty() {
                    provider.api_key = provider.api_key_secret.clone();
                }
            }
        }
    }

    /// 按 provider 的 api 类型构造模型列表接口地址。
    fn models_url(base_url: &str, api: &str) -> String {
        let base = base_url.trim().trim_end_matches('/');
        if base.is_empty() {
            return String::new();
        }
        if api == "anthropic-messages" {
            // Anthropic 的模型接口固定为 /v1/models。
            if base.ends_with("/v1") {
                format!("{}/models", base)
            } else {
                format!("{}/v1/models", base)
            }
        } else {
            format!("{}/models", base)
        }
    }

    /// 启动后台线程获取 provider 模型列表。
    fn start_model_fetch(&mut self, key: &str, base_url: &str, secret: &str, api: &str) {
        let url = Self::models_url(base_url, api);
        let secret = secret.trim().to_string();
        let api = api.to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = fetch_models_remote(&url, &secret, &api);
            let _ = tx.send(result);
        });
        self.model_fetch.insert(
            key.to_string(),
            ModelFetchState {
                rx: Some(rx),
                result: None,
            },
        );
    }

    /// 启动 provider 级延迟测试（后台线程，结果经通道回传）。
    /// 接收 `&mut HashMap` 而非 `&mut self`，以便与 `providers[idx]` 借用共存。
    fn start_provider_latency(
        latency: &mut HashMap<String, LatencyState>,
        key: &str,
        base_url: &str,
        secret: &str,
        api: &str,
    ) {
        let url = Self::models_url(base_url, api);
        let secret = secret.trim().to_string();
        let api = api.to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(measure_provider_latency(&url, &secret, &api));
        });
        let state = latency.entry(key.to_string()).or_default();
        state.provider = None;
        state.provider_rx = Some(rx);
    }

    /// 并发启动全部模型的延迟测试（每批 8 个并发，结果逐个回传）。
    /// 返回需要提示的状态栏消息（无模型可测时）。
    fn start_models_latency(
        latency: &mut HashMap<String, LatencyState>,
        key: &str,
        base_url: &str,
        secret: &str,
        api: &str,
        models: Vec<String>,
    ) -> Option<String> {
        if models.is_empty() {
            return Some("没有可测试的模型".to_string());
        }
        let base = base_url.to_string();
        let secret = secret.trim().to_string();
        let api = api.to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        let total = models.len();
        std::thread::spawn(move || {
            const BATCH: usize = 8;
            for chunk in models.chunks(BATCH) {
                std::thread::scope(|scope| {
                    for model in chunk {
                        let tx = tx.clone();
                        let base = base.clone();
                        let secret = secret.clone();
                        let api = api.clone();
                        scope.spawn(move || {
                            let result = measure_model_latency(&base, &secret, &api, model);
                            let _ = tx.send((model.clone(), result));
                        });
                    }
                });
            }
        });
        let state = latency.entry(key.to_string()).or_default();
        state.models.clear();
        state.done = 0;
        state.total = total;
        state.model_rx = Some(rx);
        None
    }

    /// 每帧轮询延迟测试结果，并更新状态栏。
    fn poll_latency(&mut self) {
        let mut notices: Vec<String> = Vec::new();
        for (_, state) in self.latency.iter_mut() {
            if let Some(rx) = &state.provider_rx {
                match rx.try_recv() {
                    Ok(result) => {
                        let msg = match &result {
                            Ok(ms) => format!("provider 延迟测试完成：{} ms", ms),
                            Err(err) => format!("provider 延迟测试失败：{}", err),
                        };
                        notices.push(msg);
                        state.provider = Some(result);
                        state.provider_rx = None;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        // 测量线程异常退出（panic 等）时收不到结果：
                        // 终止等待并提示，避免 Spinner/进度永久卡死。
                        state.provider_rx = None;
                        notices.push("provider 延迟测试中断（线程异常退出）".to_string());
                    }
                }
            }
            if let Some(rx) = &state.model_rx {
                loop {
                    match rx.try_recv() {
                        Ok((id, result)) => {
                            state.models.insert(id, result);
                            state.done += 1;
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            state.model_rx = None;
                            state.total = state.done;
                            notices.push(format!(
                                "模型延迟测试完成（{}/{}）",
                                state.done, state.total
                            ));
                            break;
                        }
                    }
                }
            }
        }
        for msg in notices {
            self.status = msg;
        }
    }

    /// 每帧轮询后台线程的模型获取结果，并更新状态栏。
    fn poll_model_fetch(&mut self) {
        let mut finished: Vec<String> = Vec::new();
        for (key, state) in self.model_fetch.iter_mut() {
            if let Some(rx) = &state.rx {
                if let Ok(result) = rx.try_recv() {
                    state.result = Some(result);
                    state.rx = None;
                    finished.push(key.clone());
                }
            }
        }
        for key in finished {
            let msg = match &self.model_fetch[&key].result {
                Some(Ok(models)) if models.is_empty() => {
                    format!("接口未返回任何模型（{}）", key)
                }
                Some(Ok(models)) => format!("已获取 {} 个模型（{}）", models.len(), key),
                Some(Err(err)) => format!("获取模型失败（{}）: {}", key, err),
                _ => continue,
            };
            self.status = msg;
        }
    }

    /// 当前页面的思考档位标签。
    fn dialect_variants(&self) -> (&'static str, &'static [&'static str]) {
        match self.current_page {
            ConfigFormat::Opencode => (
                "variants",
                &["none", "low", "medium", "high", "xhigh", "max", "ultra"],
            ),
            ConfigFormat::PiAgent => (
                "thinkingLevelMap",
                &["off", "minimal", "low", "medium", "high", "xhigh", "max"],
            ),
            ConfigFormat::OhMyPi => (
                "thinking.efforts",
                &["minimal", "low", "medium", "high", "xhigh", "max"],
            ),
            ConfigFormat::DeepSeekHarness => (
                "reasoningEfforts",
                &["minimal", "low", "medium", "high", "xhigh", "max"],
            ),
        }
    }

    /// Providers 区块：标题行吸顶（滚动时始终显示在顶部），内容紧跟其下。
    fn ui_providers_section(&mut self, ui: &mut egui::Ui) {
        let anchor = sticky_begin(ui, 30.0);
        if self.show_providers_section {
            let matched: Vec<usize> = (0..self.providers.len()).collect();

            if self.providers.is_empty() && !self.show_new_provider {
                self.show_new_provider = true;
            }

            let mut to_remove: Option<usize> = None;
            let mut to_copy: Option<usize> = None;
            let mut hover_target: Option<String> = None;
            card_list(ui, &matched, 0.0, |ui, idx| {
                self.render_provider_card(ui, idx, &mut to_remove, &mut to_copy, &mut hover_target);
            });
            if let Some(idx) = to_remove {
                self.providers.remove(idx);
                self.status = "已删除 provider".into();
            }
            if let Some(idx) = to_copy {
                let mut p = self.providers[idx].clone();
                p.key = format!("{}_copy", p.key);
                self.providers.push(p);
                self.status = "已复制 provider".into();
            }
            if self.provider_drag_src.is_some() {
                self.provider_drag_target = hover_target;
            } else {
                self.provider_drag_target = None;
            }

            ui.add_space(10.0);
            if ui.button("新增 Provider").clicked() {
                self.show_new_provider = !self.show_new_provider;
            }
            if self.show_new_provider {
                self.ui_new_provider_form(ui);
            }
        }
        sticky_end(ui, anchor, |ui| {
            ui.horizontal(|ui| {
                ui.strong("Providers");
                let btn_label = if self.show_providers_section {
                    "隐藏"
                } else {
                    "展开"
                };
                if ui.button(btn_label).clicked() {
                    self.show_providers_section = !self.show_providers_section;
                }
                if self.show_providers_section && !self.providers.is_empty() {
                    let all_open = self
                        .providers
                        .iter()
                        .all(|p| self.provider_open.contains(&p.key));
                    if ui
                        .button(if all_open {
                            "收起全部卡片"
                        } else {
                            "展开全部卡片"
                        })
                        .clicked()
                    {
                        if all_open {
                            self.provider_open.clear();
                        } else {
                            self.provider_open =
                                self.providers.iter().map(|p| p.key.clone()).collect();
                        }
                    }
                }
                // 连通性测试：放在标题行右侧，收起全部卡片时也始终可见。
                if self.show_providers_section
                    && ui
                        .button("连通性测试")
                        .on_hover_text("并发测试当前页面全部厂商的接口连通性")
                        .clicked()
                {
                    let targets: Vec<(String, String, String, String)> = self
                        .providers
                        .iter()
                        .map(|p| {
                            let api = if p.pi_api.is_empty() {
                                convert::npm_to_api(&p.npm)
                            } else {
                                p.pi_api.clone()
                            };
                            (
                                p.key.clone(),
                                p.base_url.clone(),
                                credentials::effective_secret(p),
                                api,
                            )
                        })
                        .collect();
                    let count = targets.len();
                    for (key, base, secret, api) in targets {
                        Self::start_provider_latency(&mut self.latency, &key, &base, &secret, &api);
                    }
                    self.status = format!("已开始连通性测试（{} 个厂商）", count);
                }
                // 全局 API Key 显示/隐藏：一键切换全部密钥的明文/掩码。
                // 文案带「密钥」二字，与区块「隐藏/展开」、卡片 ▼/▶ 折叠按钮明确区分。
                if self.show_providers_section
                    && ui
                        .button(if self.show_api_keys {
                            "隐藏密钥"
                        } else {
                            "显示密钥"
                        })
                        .on_hover_text(if self.show_api_keys {
                            "点击掩码全部 API Key（默认状态）"
                        } else {
                            "点击显示全部 API Key 明文（注意防窥）"
                        })
                        .clicked()
                {
                    self.show_api_keys = !self.show_api_keys;
                }
                // 配置预览：右侧面板实时展示当前页面的序列化内容，可编辑并应用回组件。
                if self.show_providers_section
                    && ui
                        .button(if self.show_preview {
                            "关闭预览"
                        } else {
                            "预览"
                        })
                        .on_hover_text("在右侧打开当前页面「待保存文档」预览；可直接编辑，改动实时应用并自动保存")
                        .clicked()
                {
                    self.show_preview = !self.show_preview;
                    if self.show_preview {
                        // 打开时以组件状态重建待保存文档
                        self.preview_focused = false;
                        self.preview_parse_ok = true;
                        self.preview_parse_error = None;
                        self.preview_dirty_at = None;
                    }
                }
            });
        });
    }

    fn render_provider_card(
        &mut self,
        ui: &mut egui::Ui,
        idx: usize,
        to_remove: &mut Option<usize>,
        to_copy: &mut Option<usize>,
        hover_target: &mut Option<String>,
    ) {
        let key = self.providers[idx].key.clone();
        let open = self.provider_open.contains(&key);
        let highlight = if self.provider_drag_target.as_deref() == Some(key.as_str()) {
            2
        } else if self.provider_drag_src.as_deref() == Some(key.as_str()) {
            1
        } else {
            0
        };
        let resp = card_frame(ui, open, highlight, |ui| {
            ui.horizontal(|ui| {
                let h = ui.add(DragHandle);
                if h.drag_started() {
                    self.provider_drag_src = Some(key.clone());
                    self.provider_drag_target = None;
                }
                if h.drag_stopped() {
                    if self.provider_drag_src == Some(key.clone()) {
                        if let Some(dst) = self.provider_drag_target.clone() {
                            let s = self.providers.iter().position(|p| p.key == key);
                            let d = self.providers.iter().position(|p| p.key == dst);
                            if let (Some(s), Some(d)) = (s, d) {
                                move_item(&mut self.providers, s, d);
                            }
                        }
                    }
                    self.provider_drag_src = None;
                    self.provider_drag_target = None;
                }
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(if open { "▼" } else { "▶" }).size(14.0),
                        )
                        .frame(false),
                    )
                    .clicked()
                {
                    if open {
                        self.provider_open.remove(&key);
                    } else {
                        self.provider_open.insert(key.clone());
                    }
                }
                ui.strong(&self.providers[idx].key);
                // 连通性测试结果：显示在厂商名字右侧，卡片收起时也可见。
                if let Some(state) = self.latency.get(&key) {
                    if state.provider_rx.is_some() {
                        ui.add(egui::Spinner::new().size(12.0));
                    } else if let Some(res) = &state.provider {
                        match res {
                            Ok(ms) => {
                                ui.label(
                                    egui::RichText::new(format!("{}ms", ms))
                                        .color(egui::Color32::from_rgb(90, 180, 110)),
                                );
                            }
                            Err(err) => {
                                ui.label(
                                    egui::RichText::new(short_err(err))
                                        .color(egui::Color32::from_rgb(220, 90, 90)),
                                )
                                .on_hover_text(err);
                            }
                        }
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("删除").clicked() {
                        *to_remove = Some(idx);
                    }
                    if ui.button("复制").clicked() {
                        *to_copy = Some(idx);
                    }
                });
            });
            if open {
                self.render_provider_form(ui, idx);
            }
        });
        if let Some(src_key) = &self.provider_drag_src {
            if src_key != &key && resp.contains_pointer() && hover_target.is_none() {
                *hover_target = Some(key.clone());
            }
        }
    }

    fn render_provider_form(&mut self, ui: &mut egui::Ui, idx: usize) {
        let prev_key = self.providers[idx].key.clone();
        let other_keys: HashSet<String> = self
            .providers
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != idx)
            .map(|(_, p)| p.key.trim().to_string())
            .collect();
        let (variants_label, variant_names) = self.dialect_variants();
        let show_oc = self.current_page == ConfigFormat::Opencode;
        let show_omp = self.current_page == ConfigFormat::OhMyPi;
        let show_dsh = self.current_page == ConfigFormat::DeepSeekHarness;
        let show_provider_base_url = self.page_has_provider_field("base_url");
        // opencode 的 options.timeout 始终显示：文件未写该字段时默认 180000ms
        let show_provider_timeout = show_oc || self.page_has_provider_field("timeout");
        let show_dsh_retry = self.current_page == ConfigFormat::DeepSeekHarness;
        let show_model_name = self.page_has_model_field("name");
        let show_model_context = self.page_has_model_field("context");
        let show_model_output = self.page_has_model_field("output");
        let show_model_input = self.page_has_model_field("input");
        let show_model_variants = self.page_has_model_field("variants");
        let show_model_reasoning = self.page_has_model_field("reasoning");
        let show_model_tool_call = self.page_has_model_field("tool_call");
        let show_model_store = self.page_has_model_field("store");
        let p = &mut self.providers[idx];
        let base_label = if show_oc {
            "options.baseURL"
        } else if show_dsh {
            "baseURL"
        } else {
            "baseUrl"
        };
        let api_key_label = if show_oc {
            "options.apiKey"
        } else if show_dsh {
            "apiKeyEnv"
        } else {
            "apiKey"
        };
        let timeout_label = "options.timeout";
        let context_label = if show_oc {
            "limit.context"
        } else {
            "contextWindow"
        };
        let output_label = if show_oc { "limit.output" } else { "maxTokens" };
        let input_label = if show_oc { "modalities.input" } else { "input" };
        ui.horizontal_wrapped(|ui| {
            field_label(ui, 120.0, "key");
            let key_resp = ui.add(egui::TextEdit::singleline(&mut p.key).desired_width(120.0));
            if !p.key.trim().is_empty() && other_keys.contains(p.key.trim()) {
                key_resp.on_hover_text("key 与其他 provider 重复，保存将被阻止");
                ui.label(
                    egui::RichText::new("⚠ 重复")
                        .small()
                        .color(egui::Color32::from_rgb(220, 90, 90)),
                );
            }
            if show_oc {
                field_label(ui, 120.0, "npm");
                let npm_options = [
                    "",
                    "@ai-sdk/openai",
                    "@ai-sdk/anthropic",
                    "@ai-sdk/google",
                    "@ai-sdk/openai-compatible",
                ];
                let current_npm = p.npm.clone();
                let mut selected_npm = npm_options.iter().position(|n| *n == current_npm.as_str());
                egui::ComboBox::from_id_salt(format!("provider_npm_{}", p.key))
                    .selected_text(if current_npm.is_empty() {
                        "选择 npm 包..."
                    } else {
                        &current_npm
                    })
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for (i, npm) in npm_options.iter().enumerate() {
                            let is_selected = selected_npm == Some(i);
                            let label = if npm.is_empty() { "(空)" } else { *npm };
                            if ui.selectable_label(is_selected, label).clicked() {
                                selected_npm = Some(i);
                            }
                        }
                    });
                if let Some(idx) = selected_npm {
                    p.npm = npm_options[idx].to_string();
                }
            }
            if !show_oc {
                field_label(ui, 120.0, "api");
                // api 枚举按方言：omp 官方 9 值 / pi KnownApi 10 值
                let api_options: &[&str] = if show_omp {
                    &[
                        "openai-completions",
                        "openai-responses",
                        "openai-codex-responses",
                        "azure-openai-responses",
                        "anthropic-messages",
                        "bedrock-converse-stream",
                        "google-generative-ai",
                        "google-gemini-cli",
                        "google-vertex",
                    ]
                } else {
                    &[
                        "openai-completions",
                        "mistral-conversations",
                        "openai-responses",
                        "azure-openai-responses",
                        "openai-codex-responses",
                        "anthropic-messages",
                        "bedrock-converse-stream",
                        "google-generative-ai",
                        "google-vertex",
                        "pi-messages",
                    ]
                };
                let current_api = if p.pi_api.is_empty() {
                    convert::npm_to_api(&p.npm)
                } else {
                    p.pi_api.clone()
                };
                egui::ComboBox::from_id_salt(format!("provider_api_{}", p.key))
                    .selected_text(&current_api)
                    .width(180.0)
                    .show_ui(ui, |ui| {
                        for &api in api_options {
                            let is_selected = current_api == api;
                            if ui.selectable_label(is_selected, api).clicked() {
                                p.pi_api = api.to_string();
                                p.npm = convert::api_to_npm(api);
                            }
                        }
                    });
            }
            // pi / omp 的 compat 与 api 同排显示（紧跟 api 之后）。
            if !show_oc && !show_dsh {
                field_label(ui, 120.0, "compat");
                ui.checkbox(&mut p.compat, "supportsDeveloperRole");
                // pi / omp 相互映射字段：加载 opencode/dsh 时缺省不勾选。
                let requires_label = if show_omp {
                    "requiresReasoningContentForAllAssistantTurns"
                } else {
                    "requiresReasoningContentOnAssistantMessages"
                };
                ui.checkbox(&mut p.requires_reasoning_content, requires_label);
            }
            if show_dsh_retry {
                field_label(ui, 120.0, "retryPolicy.mode");
                ui.add(egui::TextEdit::singleline(&mut p.dsh_retry_mode).desired_width(100.0));
                field_label(ui, 120.0, "maxRetries");
                numeric_text_edit(ui, &mut p.dsh_max_retries, 55.0, "3");
            }
        });
        ui.horizontal_wrapped(|ui| {
            if show_provider_base_url {
                field_label(ui, 120.0, base_label);
                ui.add(egui::TextEdit::singleline(&mut p.base_url).desired_width(200.0));
            }
            field_label(ui, 120.0, api_key_label);
            if show_dsh {
                ui.add(egui::TextEdit::singleline(&mut p.api_key_env).desired_width(192.0));
                field_label(ui, 120.0, "API Key");
                secret_text_edit(ui, &mut p.api_key_secret, self.show_api_keys, 408.0, "");
            } else {
                secret_text_edit(ui, &mut p.api_key, self.show_api_keys, 408.0, "");
            }
            if show_oc && show_provider_timeout {
                field_label(ui, 120.0, timeout_label);
                numeric_text_edit(ui, &mut p.timeout, 70.0, "180000");
            }
            if show_dsh_retry {
                field_label(ui, 120.0, "timeoutMs");
                numeric_text_edit(ui, &mut p.dsh_timeout_ms, 70.0, "180000");
            }
        });

        ui.add_space(6.0);
        let mut fetch_request: Option<(String, String, String, String)> = None;
        let mut close_fetch = false;
        let mut latency_models: Option<(String, String, Vec<String>, String)> = None;
        ui.horizontal(|ui| {
            ui.strong("Models");
            let fetch_api = if p.pi_api.is_empty() {
                convert::npm_to_api(&p.npm)
            } else {
                p.pi_api.clone()
            };
            let fetch_secret = credentials::effective_secret(p);
            if ui.button("获取模型").clicked() {
                fetch_request = Some((
                    p.key.clone(),
                    p.base_url.clone(),
                    fetch_secret.clone(),
                    fetch_api.clone(),
                ));
            }
            if self.model_fetch_open.contains(&p.key) && ui.button("关闭").clicked() {
                close_fetch = true;
            }
            if ui.button("模型延迟").clicked() {
                latency_models = Some((
                    p.key.clone(),
                    p.base_url.clone(),
                    p.models
                        .iter()
                        .map(|m| m.id.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect(),
                    fetch_api.clone(),
                ));
            }
            if let Some(state) = self.latency.get(&p.key) {
                if state.model_rx.is_some() {
                    ui.label(
                        egui::RichText::new(format!("模型 {}/{}", state.done, state.total)).small(),
                    );
                }
            }
        });
        if let Some((key, base, models, api)) = latency_models {
            let secret = credentials::effective_secret(p);
            if let Some(msg) =
                Self::start_models_latency(&mut self.latency, &key, &base, &secret, &api, models)
            {
                self.status = msg;
            }
        }
        if let Some((key, base, secret, api)) = fetch_request {
            // 后台线程拉取模型列表（避免阻塞 UI），结果经通道回传。
            let url = Self::models_url(&base, &api);
            let secret = secret.trim().to_string();
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let result = fetch_models_remote(&url, &secret, &api);
                let _ = tx.send(result);
            });
            self.model_fetch.insert(
                key.clone(),
                ModelFetchState {
                    rx: Some(rx),
                    result: None,
                },
            );
            self.model_fetch_open.insert(key);
        }
        if close_fetch {
            self.model_fetch_open.remove(&p.key);
        }
        if self.model_fetch_open.contains(&p.key) {
            card_frame(ui, false, 0, |ui| {
                if let Some(state) = self.model_fetch.get(&p.key) {
                    if state.rx.is_some() {
                        ui.horizontal(|ui| {
                            ui.add(egui::Spinner::new().size(16.0));
                            ui.label(egui::RichText::new("正在获取模型…").weak());
                        });
                    } else if let Some(result) = &state.result {
                        match result {
                            Ok(models) if models.is_empty() => {
                                ui.label(egui::RichText::new("接口未返回任何模型").weak());
                            }
                            Ok(models) => {
                                let ids = models.clone();
                                ui.label(egui::RichText::new("勾选可新增未配置的模型：").weak());
                                // 最多 5 列横向排列；区域高度固定为 22 行，
                                // 每列超出部分在区域内垂直滚动查看。
                                let cols = ids.len().clamp(1, 5);
                                let per_col = ids.len().div_ceil(cols);
                                let row_h =
                                    ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
                                let prev_spacing_x = ui.spacing().item_spacing.x;
                                ui.spacing_mut().item_spacing.x = 28.0;
                                egui::ScrollArea::vertical()
                                    .id_salt("model_fetch_scroll")
                                    .max_height(row_h * 22.5)
                                    .auto_shrink([false, true])
                                    .scroll_bar_visibility(
                                        egui::scroll_area::ScrollBarVisibility::AlwaysVisible,
                                    )
                                    .show(ui, |ui| {
                                        ui.columns(cols, |columns| {
                                            for (ci, column) in columns.iter_mut().enumerate() {
                                                column.set_min_width(150.0);
                                                for id in
                                                    ids.iter().skip(ci * per_col).take(per_col)
                                                {
                                                    let mut checked =
                                                        p.models.iter().any(|m| m.id.trim() == id);
                                                    if column.checkbox(&mut checked, id).changed()
                                                        && checked
                                                    {
                                                        let mut row = ModelRow::new();
                                                        row.id = id.clone();
                                                        row.name = id.clone();
                                                        row.source_format = Some(self.current_page);
                                                        p.models.push(row);
                                                    }
                                                }
                                            }
                                        });
                                    });
                                ui.spacing_mut().item_spacing.x = prev_spacing_x;
                            }
                            Err(err) => {
                                ui.label(
                                    egui::RichText::new(format!("获取失败：{}", err))
                                        .color(egui::Color32::from_rgb(220, 90, 90)),
                                );
                            }
                        }
                    }
                } else {
                    ui.label(egui::RichText::new("尚未获取，请先点击「获取模型」").weak());
                }
            });
        }
        let mut rm: Option<usize> = None;
        let mut model_hover_target: Option<String> = None;
        let mut model_drag_stopped = false;
        for j in 0..p.models.len() {
            let model_key = format!("{}\u{1f}{}", p.key, p.models[j].id);
            let model_highlight = if self.model_drag_target.as_deref() == Some(model_key.as_str()) {
                2
            } else if self.model_drag_src.as_deref() == Some(model_key.as_str()) {
                1
            } else {
                0
            };
            let other_ids: HashSet<String> = p
                .models
                .iter()
                .enumerate()
                .filter(|(j2, _)| *j2 != j)
                .map(|(_, m)| m.id.trim().to_string())
                .collect();
            let model_response = card_frame(ui, true, model_highlight, |ui| {
                ui.horizontal(|ui| {
                    let handle = ui.add(DragHandle);
                    if handle.drag_started() {
                        self.model_drag_src = Some(model_key.clone());
                        self.model_drag_target = None;
                    }
                    if handle.drag_stopped() {
                        model_drag_stopped = true;
                    }
                    // 该行显示延迟（拖动按钮右侧），删除按钮右对齐。
                    if let Some(state) = self.latency.get(&p.key) {
                        if let Some(res) = state.models.get(p.models[j].id.trim()) {
                            match res {
                                Ok(ms) => {
                                    ui.label(
                                        egui::RichText::new(format!("{}ms", ms))
                                            .color(egui::Color32::from_rgb(90, 180, 110)),
                                    );
                                }
                                Err(err) => {
                                    ui.label(
                                        egui::RichText::new(short_err(err))
                                            .color(egui::Color32::from_rgb(220, 90, 90)),
                                    )
                                    .on_hover_text(err);
                                }
                            }
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("删").clicked() {
                            rm = Some(j);
                        }
                    });
                });
                ui.horizontal_wrapped(|ui| {
                    field_label(ui, 120.0, "id:");
                    let id_resp = ui
                        .add(egui::TextEdit::singleline(&mut p.models[j].id).desired_width(120.0));
                    if !p.models[j].id.trim().is_empty()
                        && other_ids.contains(p.models[j].id.trim())
                    {
                        id_resp.on_hover_text("id 与同 provider 内其他模型重复，保存将被阻止");
                        ui.label(
                            egui::RichText::new("⚠ 重复")
                                .small()
                                .color(egui::Color32::from_rgb(220, 90, 90)),
                        );
                    }
                    if show_model_name {
                        field_label(ui, 120.0, "name:");
                        ui.add(
                            egui::TextEdit::singleline(&mut p.models[j].name).desired_width(120.0),
                        );
                    }
                    if show_model_reasoning && (show_oc || !show_dsh) {
                        ui.checkbox(&mut p.models[j].reasoning, "reasoning");
                    }
                    if show_model_tool_call && show_oc {
                        ui.checkbox(&mut p.models[j].tool_call, "tool_call");
                    }
                    if show_model_store && show_oc {
                        ui.checkbox(&mut p.models[j].store, "store");
                    }
                    if show_model_context {
                        field_label(ui, 120.0, context_label);
                        numeric_text_edit(ui, &mut p.models[j].context, 53.0, "");
                    }
                    if show_model_output {
                        field_label(ui, 120.0, output_label);
                        numeric_text_edit(ui, &mut p.models[j].output, 53.0, "");
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    if show_model_input {
                        field_label(ui, 120.0, input_label);
                        ui.add(
                            egui::TextEdit::singleline(&mut p.models[j].modalities_input)
                                .desired_width(80.0),
                        );
                    }
                    if show_oc {
                        field_label(ui, 120.0, "modalities.output");
                        ui.add(
                            egui::TextEdit::singleline(&mut p.models[j].modalities_output)
                                .desired_width(80.0),
                        );
                    }
                    if show_model_variants {
                        field_label(ui, 120.0, variants_label);
                    }
                    let current_variants = p.models[j].variants.clone();
                    let mut selected_variants: Vec<String> = if current_variants.trim().is_empty() {
                        Vec::new()
                    } else {
                        current_variants
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect()
                    };
                    let display = if selected_variants.is_empty() {
                        "选择..."
                    } else {
                        &current_variants
                    };
                    let variant_key = format!("variant_open_{}_{}", p.key, j);
                    let is_open = self.variant_open.contains(&variant_key);
                    if ui.button(display).clicked() {
                        if is_open {
                            self.variant_open.remove(&variant_key);
                        } else {
                            self.variant_open.insert(variant_key.clone());
                        }
                    }
                    if is_open {
                        for vn in variant_names {
                            let mut checked = selected_variants.contains(&vn.to_string());
                            if ui.checkbox(&mut checked, *vn).changed() {
                                if checked {
                                    if !selected_variants.contains(&vn.to_string()) {
                                        selected_variants.push(vn.to_string());
                                    }
                                } else {
                                    selected_variants.retain(|s| s != vn);
                                }
                                p.models[j].variants = selected_variants.join(", ");
                            }
                        }
                    }
                });
            });
            if let Some(src) = &self.model_drag_src {
                if src != &model_key
                    && model_response.contains_pointer()
                    && model_hover_target.is_none()
                {
                    model_hover_target = Some(model_key.clone());
                }
            }
        }
        if self.model_drag_src.is_some() {
            self.model_drag_target = model_hover_target;
        } else {
            self.model_drag_target = None;
        }
        if model_drag_stopped {
            if let Some(src) = self.model_drag_src.take() {
                let target = self.model_drag_target.take();
                if let Some(dst) = target {
                    let source = p
                        .models
                        .iter()
                        .position(|m| format!("{}\u{1f}{}", p.key, m.id) == src);
                    let destination = p
                        .models
                        .iter()
                        .position(|m| format!("{}\u{1f}{}", p.key, m.id) == dst);
                    if let (Some(source), Some(destination)) = (source, destination) {
                        move_item(&mut p.models, source, destination);
                    }
                }
            }
        }
        if let Some(j) = rm {
            p.models.remove(j);
            // 删除后下标错位：关闭该 provider 的档位弹窗，避免状态串到其他模型
            let prefix = format!("variant_open_{}_", p.key);
            self.variant_open.retain(|k| !k.starts_with(&prefix));
        }
        ui.add_space(6.0);
        let show_new_model_key = format!("show_new_model_{}", p.key);
        let show_new_model = self.variant_open.contains(&show_new_model_key);
        let btn_text = if show_new_model {
            "收起"
        } else {
            "添加 Model"
        };
        if ui
            .add_sized([120.0, 20.0], egui::Button::new(btn_text))
            .clicked()
        {
            if show_new_model {
                self.variant_open.remove(&show_new_model_key);
            } else {
                self.variant_open.insert(show_new_model_key.clone());
            }
        }
        if show_new_model {
            ui.horizontal_wrapped(|ui| {
                field_label(ui, 120.0, "id:");
                ui.add(egui::TextEdit::singleline(&mut p.new_model.id).desired_width(120.0));
                field_label(ui, 120.0, "name:");
                ui.add(egui::TextEdit::singleline(&mut p.new_model.name).desired_width(120.0));
                if show_oc || !show_dsh {
                    ui.checkbox(&mut p.new_model.reasoning, "reasoning");
                }
                if show_oc {
                    ui.checkbox(&mut p.new_model.tool_call, "tool_call");
                    ui.checkbox(&mut p.new_model.store, "store");
                }
                field_label(ui, 120.0, context_label);
                numeric_text_edit(ui, &mut p.new_model.context, 53.0, "");
                field_label(ui, 120.0, output_label);
                numeric_text_edit(ui, &mut p.new_model.output, 53.0, "");
            });
            ui.horizontal_wrapped(|ui| {
                field_label(ui, 120.0, input_label);
                ui.add(
                    egui::TextEdit::singleline(&mut p.new_model.modalities_input)
                        .desired_width(80.0),
                );
                if show_oc {
                    field_label(ui, 120.0, "modalities.output");
                    ui.add(
                        egui::TextEdit::singleline(&mut p.new_model.modalities_output)
                            .desired_width(80.0),
                    );
                }
                field_label(ui, 120.0, variants_label);
                let current_variants = p.new_model.variants.clone();
                let mut selected_variants: Vec<String> = if current_variants.trim().is_empty() {
                    Vec::new()
                } else {
                    current_variants
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                };
                let display = if selected_variants.is_empty() {
                    "选择..."
                } else {
                    &current_variants
                };
                let variant_key = format!("new_model_variant_{}", p.key);
                let is_open = self.variant_open.contains(&variant_key);
                if ui.button(display).clicked() {
                    if is_open {
                        self.variant_open.remove(&variant_key);
                    } else {
                        self.variant_open.insert(variant_key.clone());
                    }
                }
                if is_open {
                    for vn in variant_names {
                        let mut checked = selected_variants.contains(&vn.to_string());
                        if ui.checkbox(&mut checked, *vn).changed() {
                            if checked {
                                if !selected_variants.contains(&vn.to_string()) {
                                    selected_variants.push(vn.to_string());
                                }
                            } else {
                                selected_variants.retain(|s| s != vn);
                            }
                            p.new_model.variants = selected_variants.join(", ");
                        }
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.add_space(60.0);
                if ui.button("添加").clicked() && !p.new_model.id.trim().is_empty() {
                    p.models.push(p.new_model.clone());
                    p.new_model = ModelRow::new();
                    self.variant_open.remove(&show_new_model_key);
                }
            });
        }
        // key 重命名后同步展开状态与弹窗键
        let new_key = self.providers[idx].key.clone();
        if new_key != prev_key {
            self.sync_provider_rename(&prev_key, &new_key);
        }
    }

    fn ui_new_provider_form(&mut self, ui: &mut egui::Ui) {
        let show_oc = self.current_page == ConfigFormat::Opencode;
        let show_omp = self.current_page == ConfigFormat::OhMyPi;
        let show_dsh = self.current_page == ConfigFormat::DeepSeekHarness;
        let base_label = if show_oc {
            "options.baseURL"
        } else if show_dsh {
            "baseURL"
        } else {
            "baseUrl"
        };
        let api_key_label = if show_oc {
            "options.apiKey"
        } else if show_dsh {
            "apiKeyEnv"
        } else {
            "apiKey"
        };
        let timeout_label = "options.timeout";
        let context_label = if show_oc {
            "limit.context"
        } else {
            "contextWindow"
        };
        let output_label = if show_oc { "limit.output" } else { "maxTokens" };
        let input_label = if show_oc { "modalities.input" } else { "input" };
        let (variants_label, variant_names) = self.dialect_variants();
        ui.group(|ui| {
            ui.horizontal_wrapped(|ui| {
                field_label(ui, 120.0, "key");
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_provider.key)
                        .hint_text("openai")
                        .desired_width(120.0),
                );
                if show_oc {
                    field_label(ui, 120.0, "npm");
                    let npm_options = [
                        "",
                        "@ai-sdk/openai",
                        "@ai-sdk/anthropic",
                        "@ai-sdk/google",
                        "@ai-sdk/openai-compatible",
                    ];
                    let current_npm = self.new_provider.npm.clone();
                    let mut selected_npm =
                        npm_options.iter().position(|n| *n == current_npm.as_str());
                    egui::ComboBox::from_id_salt("new_provider_npm")
                        .selected_text(if current_npm.is_empty() {
                            "选择 npm 包..."
                        } else {
                            &current_npm
                        })
                        .width(220.0)
                        .show_ui(ui, |ui| {
                            for (i, npm) in npm_options.iter().enumerate() {
                                let is_selected = selected_npm == Some(i);
                                if ui.selectable_label(is_selected, *npm).clicked() {
                                    selected_npm = Some(i);
                                }
                            }
                        });
                    if let Some(idx) = selected_npm {
                        self.new_provider.npm = npm_options[idx].to_string();
                    }
                }
                if !show_oc {
                    field_label(ui, 120.0, "api");
                    // api 枚举按方言：omp 官方 9 值 / pi KnownApi 10 值
                    let api_options: &[&str] = if show_omp {
                        &[
                            "openai-completions",
                            "openai-responses",
                            "openai-codex-responses",
                            "azure-openai-responses",
                            "anthropic-messages",
                            "bedrock-converse-stream",
                            "google-generative-ai",
                            "google-gemini-cli",
                            "google-vertex",
                        ]
                    } else {
                        &[
                            "openai-completions",
                            "mistral-conversations",
                            "openai-responses",
                            "azure-openai-responses",
                            "openai-codex-responses",
                            "anthropic-messages",
                            "bedrock-converse-stream",
                            "google-generative-ai",
                            "google-vertex",
                            "pi-messages",
                        ]
                    };
                    let current_api = if self.new_provider.pi_api.is_empty() {
                        convert::npm_to_api(&self.new_provider.npm)
                    } else {
                        self.new_provider.pi_api.clone()
                    };
                    egui::ComboBox::from_id_salt("new_provider_api")
                        .selected_text(&current_api)
                        .width(180.0)
                        .show_ui(ui, |ui| {
                            for &api in api_options {
                                let is_selected = current_api == api;
                                if ui.selectable_label(is_selected, api).clicked() {
                                    self.new_provider.pi_api = api.to_string();
                                    self.new_provider.npm = convert::api_to_npm(api);
                                }
                            }
                        });
                }
                // pi / omp 的 compat 与 api 同排显示（紧跟 api 之后）。
                if !show_oc && !show_dsh {
                    field_label(ui, 120.0, "compat");
                    ui.checkbox(&mut self.new_provider.compat, "supportsDeveloperRole");
                    let requires_label = if show_omp {
                        "requiresReasoningContentForAllAssistantTurns"
                    } else {
                        "requiresReasoningContentOnAssistantMessages"
                    };
                    ui.checkbox(
                        &mut self.new_provider.requires_reasoning_content,
                        requires_label,
                    );
                }
            });
            ui.horizontal_wrapped(|ui| {
                field_label(ui, 120.0, base_label);
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_provider.base_url)
                        .hint_text("https://api.openai.com/v1")
                        .desired_width(200.0),
                );
                field_label(ui, 120.0, api_key_label);
                if show_dsh {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_provider.api_key_env)
                            .hint_text("DEEPSEEK_API_KEY")
                            .desired_width(192.0),
                    );
                    field_label(ui, 120.0, "API Key");
                    secret_text_edit(
                        ui,
                        &mut self.new_provider.api_key_secret,
                        self.show_api_keys,
                        408.0,
                        "实际密钥",
                    );
                } else {
                    secret_text_edit(
                        ui,
                        &mut self.new_provider.api_key,
                        self.show_api_keys,
                        408.0,
                        "sk-xxx",
                    );
                }
                if show_oc {
                    field_label(ui, 120.0, timeout_label);
                    numeric_text_edit(ui, &mut self.new_provider.timeout, 70.0, "180000");
                }
            });
            ui.add_space(6.0);
            let mut fetch_request: Option<(String, String, String)> = None;
            let mut close_fetch = false;
            let mut latency_models: Option<(String, Vec<String>, String)> = None;
            ui.horizontal(|ui| {
                ui.strong("Models");
                let fetch_api = if self.new_provider.pi_api.is_empty() {
                    convert::npm_to_api(&self.new_provider.npm)
                } else {
                    self.new_provider.pi_api.clone()
                };
                let fetch_secret = credentials::effective_secret(&self.new_provider);
                if ui.button("获取模型").clicked() {
                    fetch_request = Some((
                        self.new_provider.base_url.clone(),
                        fetch_secret.clone(),
                        fetch_api.clone(),
                    ));
                }
                if self.model_fetch_open.contains(NEW_PROVIDER_FETCH_KEY)
                    && ui.button("关闭").clicked()
                {
                    close_fetch = true;
                }
                if ui.button("模型延迟").clicked() {
                    latency_models = Some((
                        self.new_provider.base_url.clone(),
                        self.new_provider
                            .models
                            .iter()
                            .map(|m| m.id.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect(),
                        fetch_api.clone(),
                    ));
                }
                if let Some(state) = self.latency.get(NEW_PROVIDER_FETCH_KEY) {
                    if state.model_rx.is_some() {
                        ui.label(
                            egui::RichText::new(format!("模型 {}/{}", state.done, state.total))
                                .small(),
                        );
                    }
                }
            });
            if let Some((base, models, api)) = latency_models {
                let secret = credentials::effective_secret(&self.new_provider);
                if let Some(msg) = Self::start_models_latency(
                    &mut self.latency,
                    NEW_PROVIDER_FETCH_KEY,
                    &base,
                    &secret,
                    &api,
                    models,
                ) {
                    self.status = msg;
                }
            }
            if let Some((base, secret, api)) = fetch_request {
                self.start_model_fetch(NEW_PROVIDER_FETCH_KEY, &base, &secret, &api);
                self.model_fetch_open
                    .insert(NEW_PROVIDER_FETCH_KEY.to_string());
            }
            if close_fetch {
                self.model_fetch_open.remove(NEW_PROVIDER_FETCH_KEY);
            }
            if self.model_fetch_open.contains(NEW_PROVIDER_FETCH_KEY) {
                card_frame(ui, false, 0, |ui| {
                    if let Some(state) = self.model_fetch.get(NEW_PROVIDER_FETCH_KEY) {
                        if state.rx.is_some() {
                            ui.horizontal(|ui| {
                                ui.add(egui::Spinner::new().size(16.0));
                                ui.label(egui::RichText::new("正在获取模型…").weak());
                            });
                        } else if let Some(result) = &state.result {
                            match result {
                                Ok(models) if models.is_empty() => {
                                    ui.label(egui::RichText::new("接口未返回任何模型").weak());
                                }
                                Ok(models) => {
                                    let ids = models.clone();
                                    ui.label(
                                        egui::RichText::new("勾选可新增未配置的模型：").weak(),
                                    );
                                    // 与 provider 表单一致：最多 5 列、高度固定 22 行、内部滚动。
                                    let cols = ids.len().clamp(1, 5);
                                    let per_col = ids.len().div_ceil(cols);
                                    let row_h =
                                        ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
                                    let prev_spacing_x = ui.spacing().item_spacing.x;
                                    ui.spacing_mut().item_spacing.x = 28.0;
                                    egui::ScrollArea::vertical()
                                        .id_salt("new_provider_fetch_scroll")
                                        .max_height(row_h * 22.5)
                                        .auto_shrink([false, true])
                                        .scroll_bar_visibility(
                                            egui::scroll_area::ScrollBarVisibility::AlwaysVisible,
                                        )
                                        .show(ui, |ui| {
                                            ui.columns(cols, |columns| {
                                                for (ci, column) in columns.iter_mut().enumerate() {
                                                    column.set_min_width(150.0);
                                                    for id in
                                                        ids.iter().skip(ci * per_col).take(per_col)
                                                    {
                                                        let mut checked = self
                                                            .new_provider
                                                            .models
                                                            .iter()
                                                            .any(|m| m.id.trim() == id);
                                                        if column
                                                            .checkbox(&mut checked, id)
                                                            .changed()
                                                            && checked
                                                        {
                                                            let mut row = ModelRow::new();
                                                            row.id = id.clone();
                                                            row.name = id.clone();
                                                            row.source_format =
                                                                Some(self.current_page);
                                                            self.new_provider.models.push(row);
                                                        }
                                                    }
                                                }
                                            });
                                        });
                                    ui.spacing_mut().item_spacing.x = prev_spacing_x;
                                }
                                Err(err) => {
                                    ui.label(
                                        egui::RichText::new(format!("获取失败：{}", err))
                                            .color(egui::Color32::from_rgb(220, 90, 90)),
                                    );
                                }
                            }
                        }
                    } else {
                        ui.label(egui::RichText::new("尚未获取，请先点击「获取模型」").weak());
                    }
                });
            }
            let mut rm_new: Option<usize> = None;
            let mut move_new_request: Option<(usize, usize)> = None;
            for j in 0..self.new_provider.models.len() {
                let model_count = self.new_provider.models.len();
                card_frame(ui, true, 0, |ui| {
                    ui.horizontal(|ui| {
                        if j > 0 && ui.button("↑").clicked() {
                            move_new_request = Some((j, j - 1));
                        }
                        if j + 1 < model_count && ui.button("↓").clicked() {
                            move_new_request = Some((j, j + 1));
                        }
                        // 该行显示延迟（调整按钮右侧），删除按钮右对齐。
                        if let Some(state) = self.latency.get(NEW_PROVIDER_FETCH_KEY) {
                            if let Some(res) =
                                state.models.get(self.new_provider.models[j].id.trim())
                            {
                                match res {
                                    Ok(ms) => {
                                        ui.label(
                                            egui::RichText::new(format!("{}ms", ms))
                                                .color(egui::Color32::from_rgb(90, 180, 110)),
                                        );
                                    }
                                    Err(err) => {
                                        ui.label(
                                            egui::RichText::new(short_err(err))
                                                .color(egui::Color32::from_rgb(220, 90, 90)),
                                        )
                                        .on_hover_text(err);
                                    }
                                }
                            }
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("删").clicked() {
                                rm_new = Some(j);
                            }
                        });
                    });
                    ui.horizontal_wrapped(|ui| {
                        field_label(ui, 120.0, "id:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.new_provider.models[j].id)
                                .desired_width(120.0),
                        );
                        field_label(ui, 120.0, "name:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.new_provider.models[j].name)
                                .desired_width(120.0),
                        );
                        if show_oc || !show_dsh {
                            ui.checkbox(&mut self.new_provider.models[j].reasoning, "reasoning");
                        }
                        if show_oc {
                            ui.checkbox(&mut self.new_provider.models[j].tool_call, "tool_call");
                            ui.checkbox(&mut self.new_provider.models[j].store, "store");
                        }
                        field_label(ui, 120.0, context_label);
                        numeric_text_edit(ui, &mut self.new_provider.models[j].context, 53.0, "");
                        field_label(ui, 120.0, output_label);
                        numeric_text_edit(ui, &mut self.new_provider.models[j].output, 53.0, "");
                    });
                });
            }
            if let Some((from, to)) = move_new_request {
                move_item(&mut self.new_provider.models, from, to);
            }
            if let Some(j) = rm_new {
                self.new_provider.models.remove(j);
            }
            ui.add_space(6.0);
            let show_new_model_key = format!("new_provider_show_model_{}", self.new_provider.key);
            let show_new_model = self.variant_open.contains(&show_new_model_key);
            if ui
                .button(if show_new_model {
                    "收起"
                } else {
                    "添加 Model"
                })
                .clicked()
            {
                if show_new_model {
                    self.variant_open.remove(&show_new_model_key);
                } else {
                    self.variant_open.insert(show_new_model_key.clone());
                }
            }
            if show_new_model {
                ui.horizontal_wrapped(|ui| {
                    field_label(ui, 120.0, "id:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_provider.new_model.id)
                            .desired_width(120.0),
                    );
                    field_label(ui, 120.0, "name:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_provider.new_model.name)
                            .desired_width(120.0),
                    );
                    if show_oc || !show_dsh {
                        ui.checkbox(&mut self.new_provider.new_model.reasoning, "reasoning");
                    }
                    if show_oc {
                        ui.checkbox(&mut self.new_provider.new_model.tool_call, "tool_call");
                        ui.checkbox(&mut self.new_provider.new_model.store, "store");
                    }
                    field_label(ui, 120.0, context_label);
                    numeric_text_edit(ui, &mut self.new_provider.new_model.context, 53.0, "");
                    field_label(ui, 120.0, output_label);
                    numeric_text_edit(ui, &mut self.new_provider.new_model.output, 53.0, "");
                });
                ui.horizontal_wrapped(|ui| {
                    field_label(ui, 120.0, input_label);
                    ui.add(
                        egui::TextEdit::singleline(
                            &mut self.new_provider.new_model.modalities_input,
                        )
                        .desired_width(80.0),
                    );
                    if show_oc {
                        field_label(ui, 120.0, "modalities.output");
                        ui.add(
                            egui::TextEdit::singleline(
                                &mut self.new_provider.new_model.modalities_output,
                            )
                            .desired_width(80.0),
                        );
                    }
                    field_label(ui, 120.0, variants_label);
                    let current_variants = self.new_provider.new_model.variants.clone();
                    let mut selected_variants: Vec<String> = if current_variants.trim().is_empty() {
                        Vec::new()
                    } else {
                        current_variants
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect()
                    };
                    let display = if selected_variants.is_empty() {
                        "选择..."
                    } else {
                        &current_variants
                    };
                    let popup_key = "new_provider_new_model_variant".to_string();
                    let is_open = self.variant_open.contains(&popup_key);
                    if ui.button(display).clicked() {
                        if is_open {
                            self.variant_open.remove(&popup_key);
                        } else {
                            self.variant_open.insert(popup_key.clone());
                        }
                    }
                    if is_open {
                        for vn in variant_names {
                            let mut checked = selected_variants.contains(&vn.to_string());
                            if ui.checkbox(&mut checked, *vn).changed() {
                                if checked {
                                    if !selected_variants.contains(&vn.to_string()) {
                                        selected_variants.push(vn.to_string());
                                    }
                                } else {
                                    selected_variants.retain(|s| s != vn);
                                }
                                self.new_provider.new_model.variants = selected_variants.join(", ");
                            }
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.add_space(60.0);
                    if ui.button("添加").clicked()
                        && !self.new_provider.new_model.id.trim().is_empty()
                    {
                        self.new_provider
                            .models
                            .push(self.new_provider.new_model.clone());
                        self.new_provider.new_model = ModelRow::new();
                        self.variant_open.remove(&show_new_model_key);
                    }
                });
            }
            ui.horizontal(|ui| {
                ui.add_space(60.0);
                if ui.button("确认").clicked() {
                    let key = self.new_provider.key.trim().to_string();
                    if key.is_empty() {
                        self.status = "请填写 provider key".into();
                    } else if self.providers.iter().any(|p| p.key.trim() == key) {
                        self.status = format!("provider key \"{}\" 已存在", key);
                    } else {
                        let np = self.new_provider.clone();
                        self.providers.push(np);
                        self.new_provider = ProviderRow::new();
                        self.show_new_provider = false;
                        self.clear_new_provider_state();
                        self.status = "已添加 provider".into();
                    }
                }
                if ui.button("取消").clicked() {
                    self.new_provider = ProviderRow::new();
                    self.show_new_provider = false;
                    self.clear_new_provider_state();
                }
            });
        });
    }

    /// 关闭新增 provider 表单时清理其测试/获取状态，避免下次打开残留旧结果。
    fn clear_new_provider_state(&mut self) {
        self.latency.remove(NEW_PROVIDER_FETCH_KEY);
        self.model_fetch.remove(NEW_PROVIDER_FETCH_KEY);
        self.model_fetch_open.remove(NEW_PROVIDER_FETCH_KEY);
    }

    fn reload(&mut self) {
        let (fmt, _) = ConfigPaths::detect_for_path(&self.config_path);
        self.source_format = fmt;
        self.apply_load();
    }

    fn paint_drag_ghost(&self, ctx: &egui::Context) {
        let label = if let Some(k) = &self.agent_drag_src {
            self.agents
                .iter()
                .find(|a| &a.key == k)
                .map(|a| a.key.as_str())
                .unwrap_or("")
        } else if let Some(k) = &self.provider_drag_src {
            self.providers
                .iter()
                .find(|p| &p.key == k)
                .map(|p| p.key.as_str())
                .unwrap_or("")
        } else if let Some(k) = &self.model_drag_src {
            k.split_once('\u{1f}')
                .map(|(_, model)| model)
                .unwrap_or(k.as_str())
        } else {
            return;
        };
        if label.is_empty() {
            return;
        }
        let Some(pointer) = ctx.pointer_hover_pos() else {
            return;
        };
        let layer_id = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("drag_ghost"));
        let painter = ctx.layer_painter(layer_id);
        let visuals = &ctx.style().visuals;
        let font_id = egui::FontId::proportional(14.0);
        let text_color = visuals.text_color();
        let bg = visuals.faint_bg_color;
        let stroke_color = visuals.widgets.noninteractive.bg_stroke.color;
        let galley = painter.layout(
            label.to_string(),
            font_id.clone(),
            text_color,
            f32::INFINITY,
        );
        let label_w = galley.size().x;
        let size = egui::vec2(label_w + 24.0, 30.0);
        let ghost_rect = egui::Rect::from_min_size(pointer + egui::vec2(20.0, -15.0), size);
        painter.rect_filled(ghost_rect, 8.0, bg);
        painter.rect_stroke(
            ghost_rect,
            8.0,
            egui::Stroke::new(1.0, stroke_color),
            egui::StrokeKind::Inside,
        );
        painter.text(
            ghost_rect.left_center() + egui::vec2(12.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            font_id,
            text_color,
        );
    }

    /// agent key 重命名后同步 UI 状态（卡片展开集合），避免改名后卡片收起。
    fn sync_agent_rename(&mut self, old: &str, new: &str) {
        if old == new || new.is_empty() {
            return;
        }
        if self.agent_open.remove(old) {
            self.agent_open.insert(new.to_string());
        }
    }

    /// provider key 重命名后同步 UI 状态（展开集合 + 弹窗键）。
    fn sync_provider_rename(&mut self, old: &str, new: &str) {
        if old == new || new.is_empty() {
            return;
        }
        if self.provider_open.remove(old) {
            self.provider_open.insert(new.to_string());
        }
        // 下标型弹窗键直接关闭（避免前缀歧义），需要时重新打开即可
        let variant_prefix = format!("variant_open_{}_", old);
        let show_key = format!("show_new_model_{}", old);
        let new_variant_key = format!("new_model_variant_{}", old);
        self.variant_open
            .retain(|k| !k.starts_with(&variant_prefix) && k != &show_key && k != &new_variant_key);
    }

    /// 统计非法数字字段数（非空且解析失败），保存后提示用户它们被忽略。
    fn count_invalid_numeric_fields(&self) -> usize {
        fn bad(s: &str) -> bool {
            !s.trim().is_empty() && parse_number_text(s).is_none()
        }
        let mut n = 0;
        for a in &self.agents {
            if bad(&a.temperature) {
                n += 1;
            }
        }
        for p in &self.providers {
            if bad(&p.timeout) {
                n += 1;
            }
            for m in &p.models {
                if bad(&m.context) || bad(&m.output) {
                    n += 1;
                }
            }
        }
        n
    }

    /// 校验 agent / provider / model key 唯一性，返回首个冲突描述。
    fn find_duplicate_keys(&self) -> Option<String> {
        let mut seen = HashSet::new();
        for a in &self.agents {
            let k = a.key.trim();
            if !k.is_empty() && !seen.insert(k.to_string()) {
                return Some(format!("agent \"{}\"", k));
            }
        }
        let mut seen_p = HashSet::new();
        for p in &self.providers {
            let k = p.key.trim();
            if k.is_empty() {
                continue;
            }
            if !seen_p.insert(k.to_string()) {
                return Some(format!("provider \"{}\"", k));
            }
            let mut seen_m = HashSet::new();
            for m in &p.models {
                let mk = m.id.trim();
                if !mk.is_empty() && !seen_m.insert(mk.to_string()) {
                    return Some(format!("provider \"{}\" 的 model \"{}\"", k, mk));
                }
            }
        }
        None
    }

    /// 本页写入路径：
    /// - 当前文件属于本页格式且已加载 → 当前文件（整体替换）；
    /// - 路径已修改但未加载 → 仍写该路径，但按“先读后合并”（防止覆盖目标文件已有配置）；
    /// - 其余 → 该后端默认目标（Windows 本地；WSL 需勾选「WSL同步」）。
    fn page_save_path(&self, fmt: ConfigFormat) -> PageTarget {
        if !self.config_path.is_empty() && self.config_path != self.loaded_path {
            // 用户已经在路径框中明确指定了目标文件，即使尚未点击“加载”，
            // 也必须使用该路径；保存流程会先读目标并按目标格式合并，不能静默回落默认路径。
            return PageTarget::Modified(self.config_path.clone());
        }
        if self.source_format == fmt && !self.config_path.is_empty() {
            return PageTarget::Current(self.config_path.clone());
        }
        let path = self
            .targets
            .iter()
            .find(|t| t.backend == fmt)
            .map(|t| t.path.clone())
            .unwrap_or_default();
        PageTarget::Default(path)
    }

    /// 页头：本页保存按钮 + 写入路径。
    fn ui_page_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let fmt = self.current_page;
            let target = self.page_save_path(fmt);
            let (path, kind, can_save) = match &target {
                PageTarget::Current(p) => (p.clone(), "当前文件（agent/provider 整体替换）", true),
                PageTarget::Modified(p) => (p.clone(), "路径已修改未加载：先读后合并写入", true),
                PageTarget::Default(p) => {
                    let ok = self.targets.iter().any(|t| t.backend == fmt && t.available);
                    (p.clone(), "默认目标（Windows 本地；WSL 仅勾选后写入）", ok)
                }
            };
            if ui
                .add_enabled(
                    can_save,
                    egui::Button::new(egui::RichText::new("保存").strong())
                        .fill(ui.visuals().selection.bg_fill),
                )
                .clicked()
            {
                self.save_page(fmt);
            }
            if let Some(icon) = self.icon_for(fmt) {
                ui.add(egui::Image::from_texture(icon).fit_to_exact_size(egui::vec2(12.0, 12.0)));
            }
            ui.label(egui::RichText::new(format!("写入: {}", path)).weak())
                .on_hover_text(kind);
            // 该格式不支持的区块提前提示，避免保存后才发现数据没写入
            if fmt != ConfigFormat::Opencode && !self.agents.is_empty() {
                ui.label(
                    egui::RichText::new(format!(
                        "⚠ {} 个 agents 不会写入该格式",
                        self.agents.len()
                    ))
                    .small()
                    .color(egui::Color32::from_rgb(220, 160, 60)),
                )
                .on_hover_text("该格式不支持 agent 定义，保存时将忽略");
            }
        });
        ui.separator();
    }

    /// 保存当前页面：写入该页格式对应的路径，并按需同步 WSL。
    fn save_page(&mut self, fmt: ConfigFormat) {
        if let Some(dup) = self.find_duplicate_keys() {
            self.status = format!("key 重复: {}，已取消保存", dup);
            return;
        }
        let target = self.page_save_path(fmt);
        let path = match &target {
            PageTarget::Current(p) | PageTarget::Modified(p) | PageTarget::Default(p) => p.clone(),
        };
        match &target {
            PageTarget::Current(_) => {
                if let Some(err) = self.load_error.clone() {
                    self.status = format!("当前文件: 加载失败({})，已跳过", err);
                    return;
                }
            }
            PageTarget::Default(_) => {
                if !self.targets.iter().any(|t| t.backend == fmt && t.available) {
                    self.status = format!("{}: 未安装（本地与 WSL 均未找到配置）", fmt.label());
                    return;
                }
            }
            PageTarget::Modified(_) => {}
        }
        let res = self.save_backend_to(fmt, &path);
        let ok = res.is_ok();
        self.status = match res {
            Ok(()) => format!("{}: 已保存", fmt.label()),
            Err(e) => format!("{}: 保存失败({})", fmt.label(), e),
        };
        if ok {
            // 该格式不支持 agents 时明确告知，避免误以为已写入
            if fmt != ConfigFormat::Opencode && !self.agents.is_empty() {
                self.status.push_str(&format!(
                    "（{} 个 agents 未写入：该格式不支持）",
                    self.agents.len()
                ));
            }
            let bad = self.count_invalid_numeric_fields();
            if bad > 0 {
                self.status
                    .push_str(&format!("（已忽略 {} 个无效数字字段）", bad));
            }
        }
        // WSL 同步：仅勾选“WSL同步”且写入路径为本地时，同步到 WSL 侧默认路径；
        // 写入前检测对应 agent 是否已安装（未安装则跳过并提示）。
        if self.sync_wsl && ok && !is_wsl_path(&path) {
            match backends::wsl_target(fmt) {
                Some(wsl_path) => match self.save_backend_to(fmt, &wsl_path) {
                    Ok(()) => self
                        .status
                        .push_str(&format!("; {}(WSL): 已同步", fmt.label())),
                    Err(e) => {
                        self.status
                            .push_str(&format!("; {}(WSL): 同步失败({})", fmt.label(), e))
                    }
                },
                None => self
                    .status
                    .push_str(&format!("; {}(WSL): 未安装，跳过同步", fmt.label())),
            }
        }
    }

    /// 通用保存：按后端构造 root、渲染内容并写入。
    fn save_backend_to(&mut self, fmt: ConfigFormat, path: &str) -> Result<(), String> {
        let backend = backends::backend(fmt);
        // 仅“已加载的当前文件”允许整体替换；其余目标（含已修改未加载的路径）一律先读后合并
        // 同一路径但切换到其他格式页面时，必须按目标格式先读后合并，
        // 只有来源格式和已加载路径都匹配时才允许整体替换。
        let is_current = self.source_format == fmt && path == self.loaded_path;
        let target_root: Option<Value> = if is_current {
            None
        } else {
            Some(backend.load_target_root(path))
        };
        let root = backend.serialize_root(
            &self.agents,
            &self.providers,
            self.extras_for(fmt),
            target_root.as_ref(),
        );
        let content = if fmt == ConfigFormat::DeepSeekHarness && is_current {
            // 未发生任何结构化修改时直接保留原始 YAML，避免无意义的
            // 缩进、引号、键顺序变化；实际修改后再使用稳定的 DSH 渲染器。
            match util::read_config_content(path) {
                Ok(original)
                    if util::parse_yaml_content(&original).ok().as_ref() == Some(&root) =>
                {
                    original
                }
                _ => backend.render(&root, self.save_format == SaveFormat::Compact)?,
            }
        } else {
            backend.render(&root, self.save_format == SaveFormat::Compact)?
        };
        let dsh_sidecar_backup = if fmt == ConfigFormat::DeepSeekHarness {
            let sidecar = credentials::sidecar_path(path);
            if util::config_exists(&sidecar) {
                Some((sidecar.clone(), util::read_config_content(&sidecar)?))
            } else {
                Some((sidecar, String::new()))
            }
        } else {
            None
        };
        if fmt == ConfigFormat::DeepSeekHarness {
            backend.save_sidecars(path, &self.providers)?;
        }
        if let Err(error) = backends::write_config(path, &content) {
            if let Some((sidecar, old_content)) = dsh_sidecar_backup {
                let restore = if old_content.is_empty() {
                    if util::config_exists(&sidecar) {
                        util::remove_config(&sidecar)
                    } else {
                        Ok(())
                    }
                } else {
                    backends::write_config(&sidecar, &old_content)
                };
                if let Err(restore_error) = restore {
                    return Err(format!("{}；凭据回滚失败: {}", error, restore_error));
                }
            }
            return Err(error);
        }
        // 当前文件保存成功后，回填 opencode 的 extras 载体（self.root）保持与磁盘一致
        if is_current && fmt == ConfigFormat::Opencode {
            self.root = root;
        }
        Ok(())
    }

    /// 当前文件保存时使用的基底 extras（按后端取对应载体）。
    fn extras_for(&self, fmt: ConfigFormat) -> &Value {
        match fmt {
            ConfigFormat::Opencode => &self.root,
            // pi 系（pi-agent / oh-my-pi）共用 extras 载体：providers 之外的顶层字段
            ConfigFormat::PiAgent | ConfigFormat::OhMyPi => &self.pi_extras,
            ConfigFormat::DeepSeekHarness => &self.root,
        }
    }

    /// 预览面板：右侧实时展示「待保存文档」（与保存按钮同路径、同合并语义）；
    /// 文本框始终可编辑：编辑内容实时解析并应用回组件，停止输入后自动写盘。
    fn ui_preview_panel(&mut self, ui: &mut egui::Ui) {
        let now = ui.ctx().input(|i| i.time);
        // 待保存文档：与 page_save_path / save_backend_to 相同路径与合并逻辑。
        let doc = self.preview_document();
        // 失焦（未在编辑）时：组件状态实时重写为待保存文档。
        // 解析失败时保留用户文本，避免打断未完成的编辑。
        if !self.preview_focused && self.preview_parse_ok {
            if let Ok((_, text)) = &doc {
                if text != &self.preview_draft {
                    self.preview_draft = text.clone();
                }
            }
        }
        // 顶部：标题 + 行数/总行数（不显示路径）。
        let total_lines = self.preview_draft.chars().filter(|c| *c == '\n').count() + 1;
        ui.horizontal(|ui| {
            ui.strong("预览编辑");
            ui.label(
                egui::RichText::new(format!(
                    "{} / {} 行",
                    self.preview_cursor_line, total_lines
                ))
                .small()
                .weak(),
            )
            .on_hover_text("光标所在行 / 待保存文档总行数");
            if let Err(e) = &doc {
                ui.colored_label(
                    egui::Color32::from_rgb(220, 90, 90),
                    "生成失败",
                )
                .on_hover_text(e);
            }
        });
        ui.separator();
        // 预览文本解析失败：面板内红字提示（生成失败指序列化阶段，这里指解析阶段）。
        if let Some(e) = &self.preview_parse_error {
            egui::Frame::default()
                .fill(egui::Color32::from_rgb(60, 20, 20))
                .inner_margin(egui::Margin::symmetric(6, 4))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(
                            egui::Color32::from_rgb(255, 140, 140),
                            "⚠ 格式错误",
                        );
                        ui.colored_label(
                            egui::Color32::from_rgb(230, 180, 180),
                            egui::RichText::new(e).small(),
                        )
                        .on_hover_text("继续编辑修正，或切走再切回以撤销文本修改");
                    });
                });
            ui.add_space(4.0);
        }
        // 文本框：常规自上而下布局的最后一个元素，占满剩余高度，
        // 滚轮/滚动条均正常（用 bottom_up 会把滚动错位到底部）。
        let text_width = (ui.available_width() - 14.0).max(120.0);
        let mut edited = false;
        let mut cursor_line: Option<usize> = None;
        egui::ScrollArea::vertical()
            .id_salt("preview_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let edit = egui::TextEdit::multiline(&mut self.preview_draft)
                    .font(egui::TextStyle::Monospace)
                    .code_editor()
                    .desired_width(text_width)
                    .desired_rows(24)
                    .hint_text(
                        "在此直接编辑：改动实时应用到左侧组件，停止输入约 0.8s 后自动保存",
                    );
                // 用 show 而非 add：需要 output.cursor_range 计算光标所在行。
                let output = edit.show(ui);
                let resp = output.response;
                self.preview_focused = resp.has_focus();
                if let Some(range) = output.cursor_range {
                    let idx = range.primary.ccursor.index;
                    let line = self
                        .preview_draft
                        .chars()
                        .take(idx)
                        .filter(|c| *c == '\n')
                        .count()
                        + 1;
                    cursor_line = Some(line);
                }
                if resp.changed() && self.preview_focused {
                    edited = true;
                }
            });
        if let Some(line) = cursor_line {
            self.preview_cursor_line = line;
        }
        // 编辑 → 实时解析并应用回组件状态（解析失败不写盘、不覆盖）。
        if edited {
            self.apply_preview_draft();
            self.preview_dirty_at = Some(now);
        }
        // 防抖自动保存：解析成功且停止输入 0.8s 后写盘。
        if let Some(at) = self.preview_dirty_at {
            if now - at > 0.8 {
                self.preview_dirty_at = None;
                self.preview_autosave();
            }
        }
    }

    /// 生成当前页面「待保存文档」：目标路径 + 序列化内容。
    /// 与保存一致：非当前文件目标先读目标文件并按目标格式合并（upsert）。
    fn preview_document(&self) -> Result<(String, String), String> {
        let fmt = self.current_page;
        let backend = backends::backend(fmt);
        let target = self.page_save_path(fmt);
        let path = match &target {
            PageTarget::Current(p) | PageTarget::Modified(p) | PageTarget::Default(p) => {
                p.clone()
            }
        };
        let is_current = self.source_format == fmt && path == self.loaded_path;
        let target_root = if is_current {
            None
        } else {
            Some(backend.load_target_root(&path))
        };
        let root = backend.serialize_root(
            &self.agents,
            &self.providers,
            self.extras_for(fmt),
            target_root.as_ref(),
        );
        let content = backend.render(&root, self.save_format == SaveFormat::Compact)?;
        Ok((path, content))
    }

    /// 把预览编辑内容解析并写回左侧组件状态；成功返回 true。
    /// 仅更新内存状态，不落盘（落盘由自动保存/立即保存负责）。
    fn apply_preview_draft(&mut self) -> bool {
        let fmt = self.current_page;
        let content = self.preview_draft.clone();
        match backends::backend(fmt).parse_at(&content, &self.config_path) {
            Ok(load) => {
                self.root = load.root;
                self.agents = load.agents;
                self.providers = load.providers;
                self.pi_extras = load.extras;
                self.load_error = None;
                self.source_format = fmt;
                self.preview_parse_ok = true;
                self.preview_parse_error = None;
                self.agent_open = self.agents.iter().map(|a| a.key.clone()).collect();
                self.provider_open = self.providers.iter().map(|p| p.key.clone()).collect();
                self.model_fetch.clear();
                self.model_fetch_open.clear();
                self.latency.clear();
                true
            }
            Err(e) => {
                self.preview_parse_ok = false;
                self.preview_parse_error = Some(e.clone());
                self.status = format!("预览内容解析失败：{}（继续编辑或撤销）", e);
                false
            }
        }
    }

    /// 实时保存：把当前待保存文档写入目标文件（仅本地，不触发 WSL 同步；
    /// 解析失败、目标不可用时跳过并提示，绝不写坏文件）。
    fn preview_autosave(&mut self) {
        if !self.preview_parse_ok {
            self.status = "预览内容解析失败，未保存（修正文本后会自动保存）".into();
            return;
        }
        let fmt = self.current_page;
        let target = self.page_save_path(fmt);
        let path = match &target {
            PageTarget::Current(p) | PageTarget::Modified(p) | PageTarget::Default(p) => {
                p.clone()
            }
        };
        let usable = match &target {
            PageTarget::Default(_) => {
                self.targets.iter().any(|t| t.backend == fmt && t.available)
            }
            _ => true,
        };
        if !usable {
            self.status = format!("{}: 目标不可用（{}），未实时保存——请用保存按钮", fmt.label(), path);
            return;
        }
        match self.save_backend_to(fmt, &path) {
            Ok(()) => self.status = format!("{}: 已实时保存", fmt.label()),
            Err(e) => self.status = format!("{}: 实时保存失败({})", fmt.label(), e),
        }
    }

    /// 按格式取官方图标纹理（图标未加载时返回 None）。
    fn icon_for(&self, fmt: ConfigFormat) -> Option<&egui::TextureHandle> {
        let idx = backends::BACKENDS.iter().position(|b| b.id() == fmt)?;
        self.backend_icons.get(idx).and_then(|o| o.as_ref())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CompactRole {
    Normal,
    AgentContainer,
    ProviderContainer,
    ProviderEntry,
    ModelsContainer,
    Target,
}

pub(crate) fn compact_json(root: &Value) -> String {
    let mut lines = serialize_object(
        root.as_object().unwrap_or(&Map::new()),
        0,
        CompactRole::Normal,
    );
    lines.push('\n');
    lines
}

pub(crate) fn pretty_json(root: &Value) -> String {
    let mut lines = serialize_pretty(
        root.as_object().unwrap_or(&Map::new()),
        0,
        CompactRole::Normal,
    );
    lines.push('\n');
    lines
}

const PRETTY_LINE_MAX: usize = 100;

fn serialize_pretty(object: &Map<String, Value>, level: usize, role: CompactRole) -> String {
    let indent = "  ".repeat(level);
    let child_indent = "  ".repeat(level + 1);

    let mut raw_entries: Vec<(String, Vec<String>)> = Vec::new();
    for (key, value) in object.iter() {
        let child_role = child_role(role, key);
        let value_lines = serialize_pretty_value(value, level + 1, child_role);
        let prefix = format!("{}{}: ", child_indent, json_string(key));
        raw_entries.push((prefix, value_lines));
    }

    let mut lines = vec![format!("{}{{", indent)];
    let mut current: Vec<String> = Vec::new();

    for (pi, (prefix, value_lines)) in raw_entries.iter().enumerate() {
        let single_line = value_lines.len() == 1;
        let has_more = pi + 1 < raw_entries.len();

        if single_line {
            let field = format!("{}{}", prefix, value_lines[0].trim_start());
            if current.is_empty() {
                current.push(field);
                continue;
            }
            let candidate_len: usize =
                current.iter().map(|f| f.len()).sum::<usize>() + current.len() - 1
                    + 2
                    + field.len();
            if candidate_len <= PRETTY_LINE_MAX {
                current.push(field);
                continue;
            }
        }

        if !current.is_empty() {
            let first = &current[0];
            let rest: Vec<&str> = current[1..]
                .iter()
                .map(|f| f.strip_prefix(&child_indent).unwrap_or(f.as_str()))
                .collect();
            let mut parts: Vec<&str> = Vec::new();
            parts.push(first);
            for r in &rest {
                parts.push(r);
            }
            let line = parts.join(", ");
            lines.push(format!("{},", line));
            current.clear();
        }

        if single_line {
            current.push(format!("{}{}", prefix, value_lines[0].trim_start()));
        } else {
            let mut entry_lines: Vec<String> = value_lines
                .iter()
                .enumerate()
                .map(|(i, line)| {
                    if i == 0 {
                        format!("{}{}", prefix, line.trim_start())
                    } else {
                        line.clone()
                    }
                })
                .collect();
            if has_more {
                if let Some(last) = entry_lines.last_mut() {
                    last.push(',');
                }
            }
            lines.extend(entry_lines);
        }
    }
    if !current.is_empty() {
        let first = &current[0];
        let rest: Vec<&str> = current[1..]
            .iter()
            .map(|f| f.strip_prefix(&child_indent).unwrap_or(f.as_str()))
            .collect();
        let mut parts: Vec<&str> = Vec::new();
        parts.push(first);
        for r in &rest {
            parts.push(r);
        }
        lines.push(parts.join(", "));
    }
    lines.push(format!("{}}}", indent));
    lines.join("\n")
}

fn serialize_pretty_value(value: &Value, level: usize, role: CompactRole) -> Vec<String> {
    match value {
        Value::Object(object) => {
            if level > 0 && is_leaf_object(object) {
                let single = compact_object_one_line(object);
                let prefix_len = "  ".repeat(level).len() + 2;
                if single.len() + prefix_len <= PRETTY_LINE_MAX {
                    return vec![single];
                }
            }
            serialize_pretty(object, level, role)
                .lines()
                .map(String::from)
                .collect()
        }
        Value::Array(items) if items.iter().any(|v| v.is_object()) => {
            let indent = "  ".repeat(level);
            let child_indent = "  ".repeat(level + 1);
            let mut lines = vec![format!("{}[", indent)];
            for (i, item) in items.iter().enumerate() {
                let item_lines = serialize_pretty_value(item, level + 1, role);
                let mut entry: Vec<String> = item_lines
                    .into_iter()
                    .enumerate()
                    .map(|(j, line)| {
                        if j == 0 {
                            format!("{}{}", child_indent, line.trim_start())
                        } else {
                            line
                        }
                    })
                    .collect();
                if i + 1 < items.len() {
                    if let Some(last) = entry.last_mut() {
                        last.push(',');
                    }
                }
                lines.extend(entry);
            }
            lines.push(format!("{}]", indent));
            lines
        }
        _ => vec![compact_json_value(value)],
    }
}

fn is_leaf_object(object: &Map<String, Value>) -> bool {
    if object.is_empty() {
        return true;
    }
    object.values().all(|v| {
        matches!(
            v,
            Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null | Value::Array(_)
        ) || (v.is_object() && v.as_object().is_some_and(|o| o.is_empty()))
    })
}

fn compact_object_one_line(object: &Map<String, Value>) -> String {
    if object.is_empty() {
        return "{}".into();
    }
    let inner: Vec<String> = object
        .iter()
        .map(|(k, v)| format!("{}: {}", json_string(k), compact_json_value(v)))
        .collect();
    format!("{{ {} }}", inner.join(", "))
}

fn serialize_object(object: &Map<String, Value>, level: usize, role: CompactRole) -> String {
    let indent = "  ".repeat(level);
    let child_indent = "  ".repeat(level + 1);

    if level > 0
        && !matches!(
            role,
            CompactRole::AgentContainer
                | CompactRole::ProviderContainer
                | CompactRole::ModelsContainer
        )
    {
        return serialize_fields(object, level, role);
    }

    let mut lines = vec![format!("{}{{", indent)];
    for (index, (key, value)) in object.iter().enumerate() {
        let child_role = child_role(role, key);
        let value_lines = serialize_value(value, level + 1, child_role);
        let prefix = format!("{}{}: ", child_indent, json_string(key));
        let mut entry_lines: Vec<String> = value_lines
            .into_iter()
            .enumerate()
            .map(|(i, line)| {
                if i == 0 {
                    format!("{}{}", prefix, line.trim_start())
                } else {
                    line
                }
            })
            .collect();
        if index + 1 < object.len() {
            if let Some(last) = entry_lines.last_mut() {
                last.push(',');
            }
        }
        lines.extend(entry_lines);
    }
    lines.push(format!("{}}}", indent));
    lines.join("\n")
}

// ---------- 配置加载 ----------

/// 读取配置文件内容；本地与 WSL 路径统一处理，文件不存在视为新建场景返回空串。
// —— 兼容再导出：实现迁移至 util / backends，保持既有测试路径可用 ——
pub use crate::backends::opencode::merge_opencode_root;
pub use crate::util::parse_config_content;

/// 加载 opencode 配置；读取/解析失败返回 Err。
pub fn load_opencode_result(
    path: &str,
) -> Result<(Value, Vec<AgentRow>, Vec<ProviderRow>), String> {
    let load = crate::backends::load_backend(ConfigFormat::Opencode, path)?;
    Ok((load.root, load.agents, load.providers))
}

/// 兼容包装：失败时回退空状态（供测试与旧调用方使用）。
pub fn load_or_empty(path: &str) -> (Value, Vec<AgentRow>, Vec<ProviderRow>) {
    load_opencode_result(path)
        .unwrap_or_else(|_| (Value::Object(Map::new()), Vec::new(), Vec::new()))
}

/// 加载 pi-agent 配置（支持本地与 WSL 路径）；读取/解析失败返回 Err。
pub fn load_pi_agent_result(path: &str) -> Result<(Value, Vec<ProviderRow>, Value), String> {
    let load = crate::backends::load_backend(ConfigFormat::PiAgent, path)?;
    Ok((load.root, load.providers, load.extras))
}

fn child_role(parent: CompactRole, key: &str) -> CompactRole {
    match (parent, key) {
        (CompactRole::Normal, "agent") => CompactRole::AgentContainer,
        (CompactRole::Normal, "provider") => CompactRole::ProviderContainer,
        (CompactRole::ProviderContainer, _) => CompactRole::ProviderEntry,
        (CompactRole::ProviderEntry, "models") => CompactRole::ModelsContainer,
        (CompactRole::AgentContainer, _) | (CompactRole::ModelsContainer, _) => CompactRole::Target,
        _ => CompactRole::Normal,
    }
}

fn serialize_value(value: &Value, level: usize, role: CompactRole) -> Vec<String> {
    match value {
        Value::Object(object) => serialize_object(object, level, role)
            .lines()
            .map(String::from)
            .collect(),
        Value::Array(items) if items.iter().any(|v| v.is_object()) => {
            let indent = "  ".repeat(level);
            let child_indent = "  ".repeat(level + 1);
            let mut lines = vec![format!("{}[", indent)];
            for (i, item) in items.iter().enumerate() {
                let item_lines = serialize_value(item, level + 1, role);
                let mut entry: Vec<String> = item_lines
                    .into_iter()
                    .enumerate()
                    .map(|(j, line)| {
                        if j == 0 {
                            format!("{}{}", child_indent, line.trim_start())
                        } else {
                            line
                        }
                    })
                    .collect();
                if i + 1 < items.len() {
                    if let Some(last) = entry.last_mut() {
                        last.push(',');
                    }
                }
                lines.extend(entry);
            }
            lines.push(format!("{}]", indent));
            lines
        }
        _ => vec![compact_json_value(value)],
    }
}

fn has_nested_obj_array(value: &Value) -> bool {
    match value {
        Value::Array(arr) => arr.iter().any(|v| v.is_object()),
        Value::Object(obj) => obj.values().any(has_nested_obj_array),
        _ => false,
    }
}

fn serialize_fields(object: &Map<String, Value>, level: usize, role: CompactRole) -> String {
    let indent = "  ".repeat(level);
    let field_indent = "  ".repeat(level + 1);
    let mut lines = vec!["{".into()];
    let mut current = String::new();
    let fields: Vec<_> = object.iter().collect();

    for (index, (key, value)) in fields.iter().enumerate() {
        let has_obj_array = has_nested_obj_array(value);
        let prefix = format!("{}: ", json_string(key));

        if has_obj_array {
            if !current.is_empty() {
                lines.push(format!("{},", field_indent.clone() + &current));
                current.clear();
            }
            let rendered = serialize_value(value, level + 1, child_role(role, key));
            let mut nested: Vec<String> = rendered
                .into_iter()
                .enumerate()
                .map(|(i, line)| {
                    if i == 0 {
                        format!("{}{}{}", field_indent, prefix, line.trim_start())
                    } else {
                        line
                    }
                })
                .collect();
            if index + 1 < fields.len() {
                if let Some(last) = nested.last_mut() {
                    last.push(',');
                }
            }
            lines.extend(nested);
            continue;
        }

        let single = if *key == "variants" {
            compact_variants(value)
        } else {
            compact_json_value(value)
        };

        if field_indent.chars().count() + prefix.chars().count() + single.chars().count() <= 150 {
            let field = format!("{}{}", prefix, single);
            if current.is_empty() {
                current = field;
                continue;
            }
            let candidate = format!("{}, {}", current, field);
            if candidate.chars().count() + field_indent.chars().count() > 150 {
                lines.push(format!("{},", field_indent.clone() + &current));
                current = field;
            } else {
                current = candidate;
            }
            continue;
        }

        if !current.is_empty() {
            lines.push(format!("{},", field_indent.clone() + &current));
            current.clear();
        }
        let rendered = serialize_value(value, level + 1, child_role(role, key));
        let mut nested: Vec<String> = rendered
            .into_iter()
            .enumerate()
            .map(|(i, line)| {
                if i == 0 {
                    format!("{}{}{}", field_indent, prefix, line.trim_start())
                } else {
                    line
                }
            })
            .collect();
        if index + 1 < fields.len() {
            if let Some(last) = nested.last_mut() {
                last.push(',');
            }
        }
        lines.extend(nested);
    }
    if !current.is_empty() {
        lines.push(format!("{}{}", field_indent, current));
    }
    lines.push(format!("{}}}", indent));
    lines.join("\n")
}

fn compact_json_value(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".into())
}

fn compact_variants(value: &Value) -> String {
    let Some(object) = value.as_object() else {
        return compact_json_value(value);
    };
    let entries: Vec<String> = object
        .iter()
        .map(|(k, v)| format!("{}: {}", json_string(k), compact_json_value(v)))
        .collect();
    format!("{{ {} }}", entries.join(", "))
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into())
}

#[cfg(test)]
mod compact_tests {
    use super::*;

    #[test]
    fn pretty_agent_fields_combine_on_same_line() {
        let root = serde_json::json!({
            "agent": {
                "writing": {
                    "mode": "subagent",
                    "description": "写技术文档",
                    "model": "sensenova/sensenova-6.8-flash-lite",
                    "variant": "high",
                    "temperature": 0.3,
                    "color": "info",
                    "system": "负责文档"
                }
            }
        });
        let output = pretty_json(&root);
        println!("=== PRETTY OUTPUT ===\n{}", output);
        // model, variant, temperature, color should be on the same line
        assert!(
            output.contains(
                "\"model\": \"sensenova/sensenova-6.8-flash-lite\", \"variant\": \"high\","
            ),
            "agent model+variant should combine:\n{}",
            output
        );
        assert!(
            output.contains("\"temperature\": 0.3, \"color\": \"info\", \"system\": \"负责文档\""),
            "agent temperature+color+system should combine:\n{}",
            output
        );
    }

    #[test]
    fn compact_variants_preserve_values() {
        let value = serde_json::json!({
            "variants": {
                "medium": { "reasoningEffort": "medium" },
                "high": { "reasoningEffort": "high" }
            }
        });
        let output = serialize_object(value.as_object().unwrap(), 1, CompactRole::Target);
        assert!(output.contains(
            "\"variants\": { \"medium\": {\"reasoningEffort\":\"medium\"}, \"high\": {\"reasoningEffort\":\"high\"} }"
        ));
    }

    #[test]
    fn pi_models_array_expands_to_multiple_lines() {
        let root = serde_json::json!({
            "providers": {
                "openai": {
                    "api": "openai-completions",
                    "models": [
                        { "id": "gpt-4o", "name": "GPT-4o" },
                        { "id": "gpt-4o-mini", "name": "GPT-4o mini" }
                    ]
                }
            }
        });
        let pretty = pretty_json(&root);
        assert!(pretty.contains("\"models\": [\n"));
        assert!(pretty.contains("\"id\": \"gpt-4o\", \"name\": \"GPT-4o\""));
        assert!(pretty.contains("\"id\": \"gpt-4o-mini\", \"name\": \"GPT-4o mini\""));

        let compact = compact_json(&root);
        assert!(compact.contains("\"models\": [\n"));
        assert!(compact.contains("\"id\": \"gpt-4o\", \"name\": \"GPT-4o\""));
        assert!(compact.contains("\"id\": \"gpt-4o-mini\", \"name\": \"GPT-4o mini\""));
    }

    #[test]
    fn compact_targets_agent_fields_only() {
        let root = serde_json::json!({
            "agent": { "writer": { "mode": "subagent", "description": "text", "model": "p/m" } },
            "other": { "a": 1, "b": 2 }
        });
        let output = compact_json(&root);
        assert!(output.contains("\"writer\": {"));
        assert!(output.contains("\"mode\": \"subagent\", \"description\": \"text\""));
        assert!(output.contains("\"other\": {\n    \"a\": 1,"));
    }

    #[test]
    fn compact_targets_provider_model_fields() {
        let root = serde_json::json!({
            "provider": {
                "p": {
                    "models": {
                        "m": {
                            "name": "Model",
                            "reasoning": true,
                            "variants": { "medium": {}, "high": {} }
                        }
                    }
                }
            }
        });
        let output = compact_json(&root);
        assert!(output.contains("\"models\": {\"m\":{"));
        assert!(output.contains("\"name\":\"Model\",\"reasoning\":true"));
        assert!(output.contains("\"variants\":{\"medium\":{},\"high\":{}}"));
    }

    #[test]
    fn compact_wraps_mcp_and_options_from_second_level() {
        let root = serde_json::json!({
            "mcp": {
                "server": {
                    "command": "node",
                    "args": ["server.js"],
                    "enabled": true
                }
            },
            "provider": {
                "p": {
                    "options": {
                        "baseURL": "https://example.com/v1",
                        "apiKey": "sk-test",
                        "timeout": 30000
                    }
                }
            }
        });
        let output = compact_json(&root);

        assert!(output.contains(
            "\"server\": {\"command\":\"node\",\"args\":[\"server.js\"],\"enabled\":true"
        ));
        assert!(output.contains("\"options\": {\"baseURL\":\"https://example.com/v1\",\"apiKey\":\"sk-test\",\"timeout\":30000"));
    }

    #[test]
    fn default_format_keeps_nested_leaf_objects_on_one_line() {
        let root = serde_json::json!({
            "provider": {
                "p": {
                    "options": { "baseURL": "https://example.com/v1", "timeout": 30000 }
                }
            }
        });

        let output = compact_json(&root);

        assert!(output
            .contains("\"options\": {\"baseURL\":\"https://example.com/v1\",\"timeout\":30000}"));
        assert!(!output.contains("\"options\": {\n"));
    }

    #[test]
    fn save_serializers_preserve_variant_settings() {
        let root = serde_json::json!({
            "provider": {
                "p": {
                    "models": {
                        "m": {
                            "variants": { "high": { "reasoningEffort": "high" } }
                        }
                    }
                }
            }
        });

        assert!(compact_json(&root).contains("reasoningEffort"));
        assert!(compact_json(&root).contains("\"high\":{\"reasoningEffort\":\"high\"}"));
    }
}

#[cfg(test)]
mod model_fetch_tests {
    use super::{parse_models_response, sanitize_network_error, App, chat_url};

    #[test]
    fn parse_openai_style_models() {
        let text = r#"{"object":"list","data":[{"id":"gpt-4o","object":"model"},{"id":"gpt-4o-mini","object":"model"}]}"#;
        let ids = parse_models_response(text).unwrap();
        assert_eq!(ids, vec!["gpt-4o", "gpt-4o-mini"]);
    }

    #[test]
    fn parse_anthropic_style_models() {
        let text = r#"{"data":[{"type":"model","id":"claude-3-7-sonnet-20250219"},{"type":"model","id":"claude-sonnet-4-20250514"}]}"#;
        let ids = parse_models_response(text).unwrap();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"claude-sonnet-4-20250514".to_string()));
    }

    #[test]
    fn parse_gemini_style_models() {
        let text =
            r#"{"models":[{"name":"models/gemini-2.0-flash"},{"name":"models/gemini-2.5-pro"}]}"#;
        let ids = parse_models_response(text).unwrap();
        assert_eq!(ids, vec!["gemini-2.0-flash", "gemini-2.5-pro"]);
    }

    #[test]
    fn parse_error_message() {
        let text = r#"{"error":{"message":"Invalid API key"}}"#;
        let err = parse_models_response(text).unwrap_err();
        assert!(err.contains("Invalid API key"));
    }

    #[test]
    fn parse_dedupes_ids_and_ignores_missing() {
        let text = r#"{"data":[{"id":"a"},{"id":"a"},{"name":"b"},{"foo":"c"}]}"#;
        let ids = parse_models_response(text).unwrap();
        assert_eq!(ids, vec!["a", "b"]);
    }

    #[test]
    fn models_url_openai_variants() {
        assert_eq!(
            App::models_url("https://api.openai.com/v1", "openai-completions"),
            "https://api.openai.com/v1/models"
        );
        assert_eq!(
            App::models_url("https://api.openai.com/v1/", "openai-completions"),
            "https://api.openai.com/v1/models"
        );
        assert_eq!(
            App::models_url("https://gw.example.com", "openai-completions"),
            "https://gw.example.com/models"
        );
    }

    #[test]
    fn models_url_anthropic_uses_v1() {
        assert_eq!(
            App::models_url("https://api.anthropic.com", "anthropic-messages"),
            "https://api.anthropic.com/v1/models"
        );
        assert_eq!(
            App::models_url("https://api.anthropic.com/v1", "anthropic-messages"),
            "https://api.anthropic.com/v1/models"
        );
    }

    #[test]
    fn chat_url_openai_and_anthropic() {
        assert_eq!(
            chat_url("https://api.openai.com/v1", "openai-completions"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            chat_url("https://api.anthropic.com", "anthropic-messages"),
            "https://api.anthropic.com/v1/messages"
        );
    }

    #[test]
    fn sanitize_network_error_strips_url_query() {
        let msg = r#"connection failed: Connection refused (os error 10061) for URL "https://example.com/v1?key=sk-super-secret#frag""#;
        let out = sanitize_network_error(msg);
        assert!(!out.contains("sk-super-secret"), "泄露了 query: {}", out);
        assert!(!out.contains('#'), "泄露了 fragment: {}", out);
        assert!(out.contains("https://example.com/v1"));
    }

    #[test]
    fn sanitize_network_error_passthrough_without_url() {
        let msg = "dns error: failed to lookup address";
        assert_eq!(sanitize_network_error(msg), msg);
    }

    #[test]
    fn sanitize_network_error_url_no_query_untouched() {
        let msg = r#"connection failed for URL "https://api.example.com/v1/models""#;
        assert_eq!(sanitize_network_error(msg), msg);
    }
}
