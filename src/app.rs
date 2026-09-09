use crate::convert;
use crate::format::{ConfigFormat, ConfigPaths};
use crate::model::{AgentRow, ModelRow, ProviderRow};
use crate::theme::Theme;
use crate::ui::{card_frame, card_grid, DragHandle, move_item};
use crate::util::{ensure_parent_dir, is_wsl_path, read_wsl_file, show_file_dialog};
use eframe::egui;
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::fs;

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

pub struct App {
    root: Value,
    agents: Vec<AgentRow>,
    providers: Vec<ProviderRow>,
    new_agent: AgentRow,
    new_provider: ProviderRow,
    config_path: String,
    status: String,
    filter: String,
    show_new_agent: bool,
    show_new_provider: bool,
    agent_open: HashSet<String>,
    provider_open: HashSet<String>,
    variant_open: HashSet<String>,
    agent_drag_src: Option<String>,
    agent_drag_target: Option<String>,
    provider_drag_src: Option<String>,
    provider_drag_target: Option<String>,
    theme: Theme,
    save_format: SaveFormat,
    source_format: ConfigFormat,
    config_paths: ConfigPaths,
    save_current: bool,
    save_opencode: bool,
    save_pi_agent: bool,
    provider_view_opencode: bool,
    show_agents_section: bool,
    show_providers_section: bool,
    load_error: Option<String>,
    oc_available: bool,
    pi_available: bool,
    target_opencode: String,
    target_pi_agent: String,
    pi_extras: Value,
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
            status: String::new(),
            filter: String::new(),
            show_new_agent: false,
            show_new_provider: false,
            agent_open: HashSet::new(),
            provider_open: HashSet::new(),
            variant_open: HashSet::new(),
            agent_drag_src: None,
            agent_drag_target: None,
            provider_drag_src: None,
            provider_drag_target: None,
            theme: Theme::default(),
            save_format: SaveFormat::default(),
            source_format: format,
            config_paths: paths,
            save_current: true,
            save_opencode: false,
            save_pi_agent: false,
            provider_view_opencode: format == ConfigFormat::Opencode,
            show_agents_section: true,
            show_providers_section: true,
            load_error: None,
            oc_available: false,
            pi_available: false,
            target_opencode: String::new(),
            target_pi_agent: String::new(),
            pi_extras: Value::Object(Map::new()),
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
        self.ui_top_bar(ctx);
        self.ui_status_bar(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .drag_to_scroll(false)
                .show(ui, |ui| {
                    ui.add_space(4.0);
                    self.ui_agents_section(ui);
                    ui.add_space(8.0);
                    self.ui_providers_section(ui);
                    ui.add_space(8.0);
                });
        });
        self.paint_drag_ghost(ctx);
        let dragging = self.agent_drag_src.is_some() || self.provider_drag_src.is_some();
        let mouse_down = ctx.input(|i| i.pointer.any_down());
        #[cfg(target_os = "windows")]
        crate::cursor::set_custom_cursor_active(dragging || mouse_down);
    }
}

impl App {
    /// 解析各保存目标的可用性与实际路径（避免在渲染循环中频繁拉起 wsl 进程）。
    fn refresh_targets(&mut self) {
        self.oc_available = self.config_paths.validate_target(ConfigFormat::Opencode);
        self.pi_available = self.config_paths.validate_target(ConfigFormat::PiAgent);
        self.target_opencode = self.config_paths.target_path(ConfigFormat::Opencode);
        self.target_pi_agent = self.config_paths.target_path(ConfigFormat::PiAgent);
    }

    /// 按 source_format 加载当前 config_path；失败时置空数据并记录 load_error。
    fn apply_load(&mut self) {
        let path = self.config_path.clone();
        let result = match self.source_format {
            ConfigFormat::Opencode => load_opencode_result(&path)
                .map(|(r, a, pv)| (r, a, pv, Value::Object(Map::new()))),
            ConfigFormat::PiAgent => load_pi_agent_result(&path)
                .map(|(r, pv, extras)| (r, Vec::new(), pv, extras)),
        };
        match result {
            Ok((r, a, pv, extras)) => {
                self.root = r;
                self.agents = a;
                self.providers = pv;
                self.pi_extras = extras;
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
        self.save_current = true;
        self.save_opencode = false;
        self.save_pi_agent = false;
        self.refresh_targets();
    }

    fn ui_top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.style_mut().spacing.interact_size.y = 18.0;
            ui.horizontal(|ui| {
                ui.label("配置文件:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.config_path).desired_width(420.0),
                );
                if ui.button("浏览").clicked() {
                    if let Some(p) = show_file_dialog() {
                        self.config_path = p;
                        self.reload();
                    }
                }
                if ui.button("保存").clicked() {
                    self.save();
                }
                ui.separator();
                ui.label("保存格式:");
                let format_btn = ui.button(self.save_format.label());
                if format_btn.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let scroll = ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::MouseWheel { .. })));
                if scroll && format_btn.hovered() {
                    self.save_format = match self.save_format {
                        SaveFormat::Current => SaveFormat::Compact,
                        SaveFormat::Compact => SaveFormat::Current,
                    };
                }
                if format_btn.clicked() {
                    self.save_format = match self.save_format {
                        SaveFormat::Current => SaveFormat::Compact,
                        SaveFormat::Compact => SaveFormat::Current,
                    };
                }
                ui.separator();
                ui.label("保存位置:");
                ui.checkbox(&mut self.save_current, "当前文件")
                    .on_hover_text(format!("写入: {}", self.config_path));
                ui.add_enabled(
                    self.oc_available,
                    egui::Checkbox::new(&mut self.save_opencode, "opencode"),
                )
                .on_hover_text(format!("写入: {}", self.target_opencode));
                ui.add_enabled(
                    self.pi_available,
                    egui::Checkbox::new(&mut self.save_pi_agent, "pi-agent"),
                )
                .on_hover_text(format!("写入: {}", self.target_pi_agent));
            });
            ui.horizontal(|ui| {
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
                ui.separator();
                ui.label("搜索:");
                ui.add(egui::TextEdit::singleline(&mut self.filter).desired_width(160.0));
                if ui.button("清空").clicked() {
                    self.filter.clear();
                }
                ui.separator();
                ui.label(
                    egui::RichText::new(format!("来源: {}", self.source_format.label())).weak(),
                );
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
                ui.label(
                    egui::RichText::new(format!(
                        "agents: {} | providers: {}",
                        self.agents.len(),
                        self.providers.len()
                    ))
                    .weak(),
                );
            });
        });
    }

    fn ui_agents_section(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("Agents");
            let btn_label = if self.show_agents_section { "隐藏" } else { "展开" };
            if ui.button(btn_label).clicked() {
                self.show_agents_section = !self.show_agents_section;
            }
            if self.show_agents_section && !self.agents.is_empty() {
                let all_open = self
                    .agents
                    .iter()
                    .all(|a| self.agent_open.contains(&a.key));
                if ui
                    .button(if all_open { "收起全部卡片" } else { "展开全部卡片" })
                    .clicked()
                {
                    if all_open {
                        self.agent_open.clear();
                    } else {
                        self.agent_open =
                            self.agents.iter().map(|a| a.key.clone()).collect();
                    }
                }
            }
        });
        ui.separator();

        if !self.show_agents_section {
            return;
        }

        let f = self.filter.to_lowercase();
        let matched: Vec<usize> = self
            .agents
            .iter()
            .enumerate()
            .filter_map(|(i, a)| {
                if f.is_empty() || a.haystack.contains(&f) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();

        if self.agents.is_empty() && !self.show_new_agent {
            self.show_new_agent = true;
        }

        let mut to_remove: Option<usize> = None;
        let mut to_copy: Option<usize> = None;
        let mut hover_target: Option<String> = None;
        card_grid(ui, &matched, 1, 0.0, |ui, idx| {
            self.render_agent_card(ui, idx, &mut to_remove, &mut to_copy, &f, &mut hover_target);
        });
        if let Some(idx) = to_remove {
            self.agents.remove(idx);
            self.status = "已删除 agent".into();
        }
        if let Some(idx) = to_copy {
            let mut a = self.agents[idx].clone();
            a.key = format!("{}_copy", a.key);
            a.refresh_haystack();
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

    fn render_agent_card(
        &mut self,
        ui: &mut egui::Ui,
        idx: usize,
        to_remove: &mut Option<usize>,
        to_copy: &mut Option<usize>,
        _filter: &str,
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
        let a = &mut self.agents[idx];
        let prev_key = a.key.clone();
        let prev_desc = a.description.clone();
        let prev_model = a.model.clone();
        let prev_mode = a.mode.clone();
        ui.horizontal(|ui| {
            ui.add_sized([60.0, 24.0], egui::Label::new(egui::RichText::new("key").weak()));
            ui.add(egui::TextEdit::singleline(&mut a.key).desired_width(120.0));
            ui.add_sized([60.0, 24.0], egui::Label::new(egui::RichText::new("mode").weak()));
            ui.add(egui::TextEdit::singleline(&mut a.mode).desired_width(120.0));
            ui.add_sized(
                [60.0, 24.0],
                egui::Label::new(egui::RichText::new("description").weak()),
            );
            ui.add(egui::TextEdit::singleline(&mut a.description).desired_width(450.0));
        });
        ui.horizontal(|ui| {
            ui.add_sized([60.0, 24.0], egui::Label::new(egui::RichText::new("model").weak()));
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
            ui.add_sized(
                [60.0, 24.0],
                egui::Label::new(egui::RichText::new("variant").weak()),
            );
            let variant_options = ["", "low", "medium", "high", "xhigh", "max", "ultra"];
            let current_variant = a.variant.clone();
            let mut selected_variant = variant_options.iter().position(|v| *v == current_variant.as_str());
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
        ui.horizontal(|ui| {
            ui.add_sized(
                [60.0, 24.0],
                egui::Label::new(egui::RichText::new("temperature").weak()),
            );
            ui.add(egui::TextEdit::singleline(&mut a.temperature).desired_width(120.0));
            ui.add_sized([60.0, 24.0], egui::Label::new(egui::RichText::new("color").weak()));
            ui.add(egui::TextEdit::singleline(&mut a.color).desired_width(120.0));
            ui.add_sized(
                [60.0, 24.0],
                egui::Label::new(egui::RichText::new("system").weak()),
            );
            ui.add(egui::TextEdit::singleline(&mut a.system).desired_width(450.0));
        });
        if a.key != prev_key || a.description != prev_desc || a.model != prev_model || a.mode != prev_mode {
            a.refresh_haystack();
        }
    }

    fn ui_new_agent_form(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.add_sized([60.0, 24.0], egui::Label::new(egui::RichText::new("key").weak()));
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_agent.key)
                        .hint_text("coding-assistant")
                        .desired_width(120.0),
                );
                ui.add_sized([60.0, 24.0], egui::Label::new(egui::RichText::new("mode").weak()));
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_agent.mode)
                        .hint_text("subagent")
                        .desired_width(120.0),
                );
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new("description").weak()),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_agent.description)
                        .hint_text("简要描述此 agent 的用途")
                        .desired_width(450.0),
                );
            });
            ui.horizontal(|ui| {
                ui.add_sized([60.0, 24.0], egui::Label::new(egui::RichText::new("model").weak()));
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
                            if ui
                                .selectable_label(is_selected, model.as_str())
                                .clicked()
                            {
                                selected_idx = Some(i);
                            }
                        }
                    });
                if let Some(idx) = selected_idx {
                    self.new_agent.model = model_options[idx].clone();
                }
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new("variant").weak()),
                );
                let variant_options = ["", "low", "medium", "high", "xhigh", "max", "ultra"];
                let current_variant = self.new_agent.variant.clone();
                let mut selected_variant = variant_options.iter().position(|v| *v == current_variant.as_str());
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
            ui.horizontal(|ui| {
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new("temperature").weak()),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_agent.temperature)
                        .hint_text("0.7")
                        .desired_width(120.0),
                );
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new("color").weak()),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_agent.color)
                        .hint_text("#00ccff")
                        .desired_width(120.0),
                );
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new("system").weak()),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_agent.system)
                        .hint_text("系统提示词")
                        .desired_width(450.0),
                );
            });
            ui.horizontal(|ui| {
                ui.add_space(60.0);
                if ui.button("确认").clicked() {
                    if !self.new_agent.key.trim().is_empty() {
                        let mut na = self.new_agent.clone();
                        na.refresh_haystack();
                        self.agents.push(na);
                        self.new_agent = AgentRow::new();
                        self.show_new_agent = false;
                        self.status = "已添加 agent".into();
                    } else {
                        self.status = "请填写 agent key".into();
                    }
                }
                if ui.button("取消").clicked() {
                    self.new_agent = AgentRow::new();
                    self.show_new_agent = false;
                }
            });
        });
    }

    fn ui_providers_section(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("Providers");
            let btn_label = if self.show_providers_section { "隐藏" } else { "展开" };
            if ui.button(btn_label).clicked() {
                self.show_providers_section = !self.show_providers_section;
            }
            if self.show_providers_section && !self.providers.is_empty() {
                let all_open = self
                    .providers
                    .iter()
                    .all(|p| self.provider_open.contains(&p.key));
                if ui
                    .button(if all_open { "收起全部卡片" } else { "展开全部卡片" })
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
            let oc_label = if self.provider_view_opencode { "opencode" } else { "pi-agent" };
            let oc_color = if self.provider_view_opencode {
                egui::Color32::from_rgb(100, 200, 100)
            } else {
                egui::Color32::from_rgb(200, 100, 100)
            };
            if ui.button(egui::RichText::new(format!("切换: {}", oc_label)).color(oc_color)).clicked() {
                self.provider_view_opencode = !self.provider_view_opencode;
            }
        });
        ui.separator();

        if !self.show_providers_section {
            return;
        }

        let f = self.filter.to_lowercase();
        let matched: Vec<usize> = self
            .providers
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                if f.is_empty() || p.haystack.contains(&f) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();

        if self.providers.is_empty() && !self.show_new_provider {
            self.show_new_provider = true;
        }

        let mut to_remove: Option<usize> = None;
        let mut to_copy: Option<usize> = None;
        let mut hover_target: Option<String> = None;
        card_grid(ui, &matched, 1, 0.0, |ui, idx| {
            self.render_provider_card(ui, idx, &mut to_remove, &mut to_copy, &f, &mut hover_target);
        });
        if let Some(idx) = to_remove {
            self.providers.remove(idx);
            self.status = "已删除 provider".into();
        }
        if let Some(idx) = to_copy {
            let mut p = self.providers[idx].clone();
            p.key = format!("{}_copy", p.key);
            p.refresh_haystack();
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

    fn render_provider_card(
        &mut self,
        ui: &mut egui::Ui,
        idx: usize,
        to_remove: &mut Option<usize>,
        to_copy: &mut Option<usize>,
        _filter: &str,
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
        let p = &mut self.providers[idx];
        let show_oc = self.provider_view_opencode;
        let base_label = if show_oc { "options.baseURL" } else { "baseUrl" };
        let api_key_label = if show_oc { "options.apiKey" } else { "apiKey" };
        let timeout_label = if show_oc { "options.timeout" } else { "timeout" };        let context_label = if show_oc { "limit.context" } else { "contextWindow" };
        let output_label = if show_oc { "limit.output" } else { "maxTokens" };
        let input_label = if show_oc { "modalities.input" } else { "input" };
        let output_mod_label = if show_oc { "modalities.output" } else { "(无)" };
        let variants_label = if show_oc { "variants" } else { "thinkingLevelMap" };
        ui.horizontal(|ui| {
            ui.add_sized([60.0, 24.0], egui::Label::new(egui::RichText::new("key").weak()));
            ui.add(egui::TextEdit::singleline(&mut p.key).desired_width(120.0));
            ui.add_sized(
                [60.0, 24.0],
                egui::Label::new(egui::RichText::new("description").weak()),
            );
            ui.add(egui::TextEdit::singleline(&mut p.description).desired_width(450.0));
            if show_oc {
                ui.add_sized([60.0, 24.0], egui::Label::new(egui::RichText::new("npm").weak()));
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
                ui.add_sized([60.0, 24.0], egui::Label::new(egui::RichText::new("api").weak()));
                let api_options = [
                    "openai-completions",
                    "openai-responses",
                    "anthropic-messages",
                    "google-generative-ai",
                ];
                let current_api = if p.pi_api.is_empty() {
                    convert::npm_to_api(&p.npm)
                } else {
                    p.pi_api.clone()
                };
                egui::ComboBox::from_id_salt(format!("provider_api_{}", p.key))
                    .selected_text(&current_api)
                    .width(180.0)
                    .show_ui(ui, |ui| {
                        for api in api_options {
                            let is_selected = current_api == api;
                            if ui.selectable_label(is_selected, api).clicked() {
                                p.pi_api = api.to_string();
                                p.npm = convert::api_to_npm(api);
                            }
                        }
                    });
            }
        });
        ui.horizontal(|ui| {
            ui.add_sized(
                [60.0, 24.0],
                egui::Label::new(egui::RichText::new(base_label).weak()),
            );
            ui.add(egui::TextEdit::singleline(&mut p.base_url).desired_width(200.0));
            ui.add_sized(
                [60.0, 24.0],
                egui::Label::new(egui::RichText::new(api_key_label).weak()),
            );
            ui.add(egui::TextEdit::singleline(&mut p.api_key).desired_width(408.0));
            if show_oc {
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new(timeout_label).weak()),
                );
                ui.add(egui::TextEdit::singleline(&mut p.timeout).desired_width(53.0));
            }
            if !show_oc {
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new("compat").weak()),
                );
                ui.checkbox(&mut p.compat, "supportsDeveloperRole");
            }
        });

        ui.add_space(2.0);
        ui.strong("Models");
        let mut rm: Option<usize> = None;
        for j in 0..p.models.len() {
            ui.horizontal_wrapped(|ui| {
                ui.add_sized([60.0, 24.0], egui::Label::new(egui::RichText::new("id:").weak()));
                ui.add(egui::TextEdit::singleline(&mut p.models[j].id).desired_width(120.0));
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new("name:").weak()),
                );
                ui.add(egui::TextEdit::singleline(&mut p.models[j].name).desired_width(120.0));
                ui.checkbox(&mut p.models[j].reasoning, "reasoning");
                if show_oc {
                    ui.checkbox(&mut p.models[j].tool_call, "tool_call");
                    ui.checkbox(&mut p.models[j].store, "store");
                }
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new(context_label).weak()),
                );
                ui.add(egui::TextEdit::singleline(&mut p.models[j].context).desired_width(53.0));
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new(output_label).weak()),
                );
                ui.add(egui::TextEdit::singleline(&mut p.models[j].output).desired_width(53.0));
            });
            ui.horizontal_wrapped(|ui| {
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new(input_label).weak()),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut p.models[j].modalities_input)
                        .desired_width(80.0),
                );
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new(output_mod_label).weak()),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut p.models[j].modalities_output)
                        .desired_width(80.0),
                );
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new(variants_label).weak()),
                );
                let variant_names = [
                    "none", "low", "medium", "high", "xhigh", "max", "ultra",
                ];
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
                    for vn in &variant_names {
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
                if ui.button("删").clicked() {
                    rm = Some(j);
                }
            });
        }
        if let Some(j) = rm {
            p.models.remove(j);
        }
        ui.add_space(2.0);
        let show_new_model_key = format!("show_new_model_{}", p.key);
        let show_new_model = self.variant_open.contains(&show_new_model_key);
        let btn_text = if show_new_model { "收起" } else { "添加 Model" };
        if ui.add_sized([120.0, 20.0], egui::Button::new(btn_text)).clicked() {
            if show_new_model {
                self.variant_open.remove(&show_new_model_key);
            } else {
                self.variant_open.insert(show_new_model_key.clone());
            }
        }
        if show_new_model {
            ui.horizontal(|ui| {
                ui.add_sized([60.0, 24.0], egui::Label::new(egui::RichText::new("id:").weak()));
                ui.add(egui::TextEdit::singleline(&mut p.new_model.id).desired_width(120.0));
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new("name:").weak()),
                );
                ui.add(egui::TextEdit::singleline(&mut p.new_model.name).desired_width(120.0));
                ui.checkbox(&mut p.new_model.reasoning, "reasoning");
                if show_oc {
                    ui.checkbox(&mut p.new_model.tool_call, "tool_call");
                    ui.checkbox(&mut p.new_model.store, "store");
                }
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new(context_label).weak()),
                );
                ui.add(egui::TextEdit::singleline(&mut p.new_model.context).desired_width(53.0));
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new(output_label).weak()),
                );
                ui.add(egui::TextEdit::singleline(&mut p.new_model.output).desired_width(53.0));
            });
            ui.horizontal(|ui| {
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new(input_label).weak()),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut p.new_model.modalities_input)
                        .desired_width(80.0),
                );
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new(output_mod_label).weak()),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut p.new_model.modalities_output)
                        .desired_width(80.0),
                );
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new(variants_label).weak()),
                );
                let variant_names = [
                    "none", "low", "medium", "high", "xhigh", "max", "ultra",
                ];
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
                    for vn in &variant_names {
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
    }

    fn ui_new_provider_form(&mut self, ui: &mut egui::Ui) {
        let show_oc = self.provider_view_opencode;
        let base_label = if show_oc { "options.baseURL" } else { "baseUrl" };
        let api_key_label = if show_oc { "options.apiKey" } else { "apiKey" };
        let timeout_label = if show_oc { "options.timeout" } else { "timeout" };
        let context_label = if show_oc { "limit.context" } else { "contextWindow" };
        let output_label = if show_oc { "limit.output" } else { "maxTokens" };
        let input_label = if show_oc { "modalities.input" } else { "input" };
        let output_mod_label = if show_oc { "modalities.output" } else { "(无)" };
        let variants_label = if show_oc { "variants" } else { "thinkingLevelMap" };
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.add_sized([60.0, 24.0], egui::Label::new(egui::RichText::new("key").weak()));
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_provider.key)
                        .hint_text("openai")
                        .desired_width(120.0),
                );
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new("description").weak()),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_provider.description)
                        .hint_text("简要描述此 provider")
                        .desired_width(450.0),
                );
                if show_oc {
                    ui.add_sized([60.0, 24.0], egui::Label::new(egui::RichText::new("npm").weak()));
                    let npm_options = [
                        "",
                        "@ai-sdk/openai",
                        "@ai-sdk/anthropic",
                        "@ai-sdk/google",
                        "@ai-sdk/openai-compatible",
                    ];
                    let current_npm = self.new_provider.npm.clone();
                    let mut selected_npm = npm_options.iter().position(|n| *n == current_npm.as_str());
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
                    ui.add_sized([60.0, 24.0], egui::Label::new(egui::RichText::new("api").weak()));
                    let api_options = [
                        "openai-completions",
                        "openai-responses",
                        "anthropic-messages",
                        "google-generative-ai",
                    ];
                    let current_api = if self.new_provider.pi_api.is_empty() {
                        convert::npm_to_api(&self.new_provider.npm)
                    } else {
                        self.new_provider.pi_api.clone()
                    };
                    egui::ComboBox::from_id_salt("new_provider_api")
                        .selected_text(&current_api)
                        .width(180.0)
                        .show_ui(ui, |ui| {
                            for api in api_options {
                                let is_selected = current_api == api;
                                if ui.selectable_label(is_selected, api).clicked() {
                                    self.new_provider.pi_api = api.to_string();
                                    self.new_provider.npm = convert::api_to_npm(api);
                                }
                            }
                        });
                }
            });
            ui.horizontal(|ui| {
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new(base_label).weak()),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_provider.base_url)
                        .hint_text("https://api.openai.com/v1")
                        .desired_width(200.0),
                );
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new(api_key_label).weak()),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_provider.api_key)
                        .hint_text("sk-xxx")
                        .desired_width(408.0),
                );
                if show_oc {
                    ui.add_sized(
                        [60.0, 24.0],
                        egui::Label::new(egui::RichText::new(timeout_label).weak()),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_provider.timeout)
                            .hint_text("180000")
                            .desired_width(53.0),
                    );
                }
                if !show_oc {
                    ui.add_sized(
                        [60.0, 24.0],
                        egui::Label::new(egui::RichText::new("compat").weak()),
                    );
                    ui.checkbox(&mut self.new_provider.compat, "supportsDeveloperRole");
                }
            });
            ui.add_space(2.0);
            ui.strong("Models");
            let mut rm_new: Option<usize> = None;
            for j in 0..self.new_provider.models.len() {
                ui.horizontal_wrapped(|ui| {
                    ui.add_sized([60.0, 24.0], egui::Label::new(egui::RichText::new("id:").weak()));
                    ui.add(egui::TextEdit::singleline(&mut self.new_provider.models[j].id).desired_width(120.0));
                    ui.add_sized(
                        [60.0, 24.0],
                        egui::Label::new(egui::RichText::new("name:").weak()),
                    );
                    ui.add(egui::TextEdit::singleline(&mut self.new_provider.models[j].name).desired_width(120.0));
                    ui.checkbox(&mut self.new_provider.models[j].reasoning, "reasoning");
                    if show_oc {
                        ui.checkbox(&mut self.new_provider.models[j].tool_call, "tool_call");
                        ui.checkbox(&mut self.new_provider.models[j].store, "store");
                    }
                    ui.add_sized(
                        [60.0, 24.0],
                        egui::Label::new(egui::RichText::new(context_label).weak()),
                    );
                    ui.add(egui::TextEdit::singleline(&mut self.new_provider.models[j].context).desired_width(53.0));
                    ui.add_sized(
                        [60.0, 24.0],
                        egui::Label::new(egui::RichText::new(output_label).weak()),
                    );
                    ui.add(egui::TextEdit::singleline(&mut self.new_provider.models[j].output).desired_width(53.0));
                    if ui.button("删").clicked() {
                        rm_new = Some(j);
                    }
                });
            }
            if let Some(j) = rm_new {
                self.new_provider.models.remove(j);
            }
            ui.add_space(2.0);
            let show_new_model_key = format!("new_provider_show_model_{}", self.new_provider.key);
            let show_new_model = self.variant_open.contains(&show_new_model_key);
            if ui.button(if show_new_model { "收起" } else { "添加 Model" }).clicked() {
                if show_new_model {
                    self.variant_open.remove(&show_new_model_key);
                } else {
                    self.variant_open.insert(show_new_model_key.clone());
                }
            }
            if show_new_model {
                ui.horizontal_wrapped(|ui| {
                    ui.add_sized([60.0, 24.0], egui::Label::new(egui::RichText::new("id:").weak()));
                    ui.add(egui::TextEdit::singleline(&mut self.new_provider.new_model.id).desired_width(120.0));
                    ui.add_sized(
                        [60.0, 24.0],
                        egui::Label::new(egui::RichText::new("name:").weak()),
                    );
                    ui.add(egui::TextEdit::singleline(&mut self.new_provider.new_model.name).desired_width(120.0));
                    ui.checkbox(&mut self.new_provider.new_model.reasoning, "reasoning");
                    if show_oc {
                        ui.checkbox(&mut self.new_provider.new_model.tool_call, "tool_call");
                        ui.checkbox(&mut self.new_provider.new_model.store, "store");
                    }
                    ui.add_sized(
                        [60.0, 24.0],
                        egui::Label::new(egui::RichText::new(context_label).weak()),
                    );
                    ui.add(egui::TextEdit::singleline(&mut self.new_provider.new_model.context).desired_width(53.0));
                    ui.add_sized(
                        [60.0, 24.0],
                        egui::Label::new(egui::RichText::new(output_label).weak()),
                    );
                    ui.add(egui::TextEdit::singleline(&mut self.new_provider.new_model.output).desired_width(53.0));
                });
                ui.horizontal_wrapped(|ui| {
                    ui.add_sized(
                        [60.0, 24.0],
                        egui::Label::new(egui::RichText::new(input_label).weak()),
                    );
                    ui.add(egui::TextEdit::singleline(&mut self.new_provider.new_model.modalities_input).desired_width(80.0));
                    ui.add_sized(
                        [60.0, 24.0],
                        egui::Label::new(egui::RichText::new(output_mod_label).weak()),
                    );
                    ui.add(egui::TextEdit::singleline(&mut self.new_provider.new_model.modalities_output).desired_width(80.0));
                    ui.add_sized(
                        [60.0, 24.0],
                        egui::Label::new(egui::RichText::new(variants_label).weak()),
                    );
                    let variant_names = [
                        "none", "low", "medium", "high", "xhigh", "max", "ultra",
                    ];
                    let current_variants = self.new_provider.new_model.variants.clone();
                    let mut selected_variants: Vec<String> = if current_variants.trim().is_empty() {
                        Vec::new()
                    } else {
                        current_variants.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
                    };
                    let display = if selected_variants.is_empty() { "选择..." } else { &current_variants };
                    let _ = ui.button(display);
                    for vn in &variant_names {
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
                    if !self.new_provider.key.trim().is_empty() {
                        let mut np = self.new_provider.clone();
                        np.refresh_haystack();
                        self.providers.push(np);
                        self.new_provider = ProviderRow::new();
                        self.show_new_provider = false;
                        self.status = "已添加 provider".into();
                    } else {
                        self.status = "请填写 provider key".into();
                    }
                }
                if ui.button("取消").clicked() {
                    self.new_provider = ProviderRow::new();
                    self.show_new_provider = false;
                }
            });
        });
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

    fn save(&mut self) {
        if !self.save_current && !self.save_opencode && !self.save_pi_agent {
            self.status = "请先选择保存目标".into();
            return;
        }
        if let Some(dup) = self.find_duplicate_keys() {
            self.status = format!("key 重复: {}，已取消保存", dup);
            return;
        }
        let mut status_parts = Vec::new();
        if self.save_current {
            if self.config_path.is_empty() {
                status_parts.push("当前文件: 没有打开的文件".to_string());
            } else if let Some(err) = self.load_error.clone() {
                status_parts.push(format!("当前文件: 加载失败({})，已跳过", err));
            } else {
                let res = match self.source_format {
                    ConfigFormat::Opencode => self.save_opencode_to(&self.config_path.clone()),
                    ConfigFormat::PiAgent => self.save_pi_agent_to(&self.config_path.clone()),
                };
                status_parts.push(match res {
                    Ok(()) => "当前文件: 已保存".to_string(),
                    Err(e) => format!("当前文件: 保存失败({})", e),
                });
            }
        }
        if self.save_opencode {
            if !self.oc_available {
                status_parts
                    .push("opencode: 未安装（本地与 WSL 均未找到配置）".to_string());
            } else {
                let path = self.target_opencode.clone();
                let res = self.save_opencode_to(&path);
                status_parts.push(match res {
                    Ok(()) => "opencode: 已保存".to_string(),
                    Err(e) => format!("opencode: 保存失败({})", e),
                });
            }
        }
        if self.save_pi_agent {
            if !self.pi_available {
                status_parts
                    .push("pi-agent: 未安装（本地与 WSL 均未找到配置）".to_string());
            } else {
                let path = self.target_pi_agent.clone();
                let res = self.save_pi_agent_to(&path);
                status_parts.push(match res {
                    Ok(()) => "pi-agent: 已保存".to_string(),
                    Err(e) => format!("pi-agent: 保存失败({})", e),
                });
            }
        }
        self.status = status_parts.join("; ");
    }

    fn save_opencode_to(&mut self, path: &str) -> Result<(), String> {
        let is_current = path == self.config_path;
        let root = if is_current {
            // 当前文件：以 UI 状态为准整体替换 agent / provider（删除即生效）
            let mut r = std::mem::take(&mut self.root);
            if let Value::Object(o) = &mut r {
                let mut am = Map::new();
                for a in &self.agents {
                    if !a.key.is_empty() {
                        am.insert(a.key.clone(), a.to_value());
                    }
                }
                o.insert("agent".into(), Value::Object(am));

                let mut pm = Map::new();
                for p in &self.providers {
                    if !p.key.is_empty() {
                        pm.insert(p.key.clone(), p.to_value());
                    }
                }
                o.insert("provider".into(), Value::Object(pm));
            }
            r
        } else {
            // 跨格式目标：加载目标现有内容，upsert 合并而非整体覆盖
            merge_opencode_root(&load_or_empty(path).0, &self.agents, &self.providers)
        };

        let content = match self.save_format {
            SaveFormat::Current => pretty_json(&root),
            SaveFormat::Compact => compact_json(&root),
        };

        let write_res = if is_wsl_path(path) {
            crate::util::write_wsl_file(path, &content)
        } else {
            match ensure_parent_dir(path) {
                Err(e) => Err(e),
                Ok(()) => fs::write(path, content).map_err(|e| e.to_string()),
            }
        };
        if is_current {
            self.root = root;
        }
        write_res
    }

    fn save_pi_agent_to(&mut self, path: &str) -> Result<(), String> {
        let is_current = path == self.config_path;
        // 跨格式目标：extras 取目标文件自身的顶层字段，仅重写 providers
        let root = if is_current {
            convert::to_pi_root(&self.providers, &self.pi_extras)
        } else {
            let (_, _, target_extras) = load_pi_agent_result(path)
                .unwrap_or((Value::Object(Map::new()), Vec::new(), Value::Object(Map::new())));
            convert::to_pi_root(&self.providers, &target_extras)
        };

        let content = match self.save_format {
            SaveFormat::Current => pretty_json(&root),
            SaveFormat::Compact => compact_json(&root),
        };

        if is_wsl_path(path) {
            crate::util::write_wsl_file(path, &content)
        } else {
            ensure_parent_dir(path)?;
            fs::write(path, content).map_err(|e| e.to_string())
        }
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

fn compact_json(root: &Value) -> String {
    let mut lines = serialize_object(
        root.as_object().unwrap_or(&Map::new()),
        0,
        CompactRole::Normal,
    );
    lines.push('\n');
    lines
}

fn pretty_json(root: &Value) -> String {
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
            let candidate_len: usize = current.iter().map(|f| f.len()).sum::<usize>()
                + current.len() - 1
                + 2
                + field.len();
            if candidate_len <= PRETTY_LINE_MAX {
                current.push(field);
                continue;
            }
        }

        if !current.is_empty() {
            let first = &current[0];
            let rest: Vec<&str> = current[1..].iter().map(|f| {
                f.strip_prefix(&child_indent).unwrap_or(f.as_str())
            }).collect();
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
        let rest: Vec<&str> = current[1..].iter().map(|f| {
            f.strip_prefix(&child_indent).unwrap_or(f.as_str())
        }).collect();
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

    if level > 0 && !matches!(role, CompactRole::AgentContainer | CompactRole::ProviderContainer | CompactRole::ModelsContainer) {
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
pub fn read_config_content(path: &str) -> Result<String, String> {
    if path.is_empty() {
        return Ok(String::new());
    }
    if is_wsl_path(path) {
        if !crate::util::wsl_file_exists(path) {
            return Ok(String::new());
        }
        read_wsl_file(path)
    } else if std::path::Path::new(path).exists() {
        fs::read_to_string(path).map_err(|e| format!("读取失败: {}", e))
    } else {
        Ok(String::new())
    }
}

/// 解析配置内容（支持 JSONC 注释与尾逗号）；空内容视为空对象。
pub fn parse_config_content(content: &str) -> Result<Value, String> {
    if content.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    let stripped = crate::util::strip_jsonc_comments(content);
    serde_json::from_str(&stripped).map_err(|e| format!("解析失败: {}", e))
}

/// 加载 opencode 配置；读取/解析失败返回 Err。
pub fn load_opencode_result(
    path: &str,
) -> Result<(Value, Vec<AgentRow>, Vec<ProviderRow>), String> {
    let content = read_config_content(path)?;
    let v = parse_config_content(&content)?;
    let agents = v
        .get("agent")
        .and_then(|x| x.as_object())
        .map(|o| o.iter().map(|(k, av)| AgentRow::from(k, av)).collect())
        .unwrap_or_default();
    let providers = v
        .get("provider")
        .and_then(|x| x.as_object())
        .map(|o| o.iter().map(|(k, pv)| ProviderRow::from(k, pv)).collect())
        .unwrap_or_default();
    Ok((v, agents, providers))
}

/// 兼容包装：失败时回退空状态（供测试与旧调用方使用）。
pub fn load_or_empty(path: &str) -> (Value, Vec<AgentRow>, Vec<ProviderRow>) {
    load_opencode_result(path)
        .unwrap_or_else(|_| (Value::Object(Map::new()), Vec::new(), Vec::new()))
}

/// 加载 pi-agent 配置（支持本地与 WSL 路径）；读取/解析失败返回 Err。
pub fn load_pi_agent_result(path: &str) -> Result<(Value, Vec<ProviderRow>, Value), String> {
    let content = read_config_content(path)?;
    let v = parse_config_content(&content)?;
    let providers = convert::load_pi_providers(&v);
    let extras = convert::load_pi_extras(&v);
    Ok((v, providers, extras))
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
        let prefix = format!("{}: ", json_string(key));

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

/// 将 UI 状态合并进 opencode 目标 root（跨格式保存用）：agent / provider 以 UI 状态 upsert，
/// 目标已有同名条目被覆盖、不同名条目保留，其余顶层字段原样保留。
pub fn merge_opencode_root(
    target_root: &Value,
    agents: &[AgentRow],
    providers: &[ProviderRow],
) -> Value {
    let mut root = target_root.clone();
    if let Value::Object(o) = &mut root {
        let mut am = o
            .get("agent")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        for a in agents {
            if !a.key.is_empty() {
                am.insert(a.key.clone(), a.to_value());
            }
        }
        o.insert("agent".into(), Value::Object(am));

        let mut pm = o
            .get("provider")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        for p in providers {
            if !p.key.is_empty() {
                pm.insert(p.key.clone(), p.to_value());
            }
        }
        o.insert("provider".into(), Value::Object(pm));
    }
    root
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
        assert!(output.contains("\"model\": \"sensenova/sensenova-6.8-flash-lite\", \"variant\": \"high\","),
            "agent model+variant should combine:\n{}", output);
        assert!(output.contains("\"temperature\": 0.3, \"color\": \"info\", \"system\": \"负责文档\""),
            "agent temperature+color+system should combine:\n{}", output);
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

assert!(output.contains("\"server\": {\"command\":\"node\",\"args\":[\"server.js\"],\"enabled\":true"));
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

        assert!(output.contains("\"options\": {\"baseURL\":\"https://example.com/v1\",\"timeout\":30000}"));
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

