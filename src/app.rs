use crate::model::{AgentRow, ModelRow, ProviderRow};
use crate::ui::{card_frame, card_grid, DragHandle, move_item};
use crate::util::{
    default_config_path, ensure_parent_dir, is_wsl_path, read_wsl_file, show_file_dialog,
};
use eframe::egui;
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::fs;

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
}

impl Default for App {
    fn default() -> Self {
        let path = default_config_path().unwrap_or_default();
        let (root, agents, providers) = load_or_empty(&path);
        let agent_open: HashSet<String> = agents.iter().map(|a| a.key.clone()).collect();
        let provider_open: HashSet<String> = providers.iter().map(|p| p.key.clone()).collect();
        Self {
            root,
            agents,
            providers,
            new_agent: AgentRow::new(),
            new_provider: ProviderRow::new(),
            config_path: path,
            status: String::new(),
            filter: String::new(),
            show_new_agent: false,
            show_new_provider: false,
            agent_open,
            provider_open,
            variant_open: HashSet::new(),
            agent_drag_src: None,
            agent_drag_target: None,
            provider_drag_src: None,
            provider_drag_target: None,
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ui_top_bar(ctx);
        self.ui_status_bar(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
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
    fn ui_top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.style_mut().spacing.interact_size.y = 10.0;
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
                ui.label("搜索:");
                ui.add(egui::TextEdit::singleline(&mut self.filter).desired_width(160.0));
                if ui.button("清空").clicked() {
                    self.filter.clear();
                }
                ui.separator();
                let all_open = self
                    .agents
                    .iter()
                    .all(|a| self.agent_open.contains(&a.key))
                    && self
                        .providers
                        .iter()
                        .all(|p| self.provider_open.contains(&p.key));
                let btn_label = if all_open { "隐藏全部" } else { "展开全部" };
                if ui.button(btn_label).clicked() {
                    if all_open {
                        self.agent_open.clear();
                        self.provider_open.clear();
                    } else {
                        self.agent_open = self.agents.iter().map(|a| a.key.clone()).collect();
                        self.provider_open =
                            self.providers.iter().map(|p| p.key.clone()).collect();
                    }
                }
            });
        });
    }

    fn ui_status_bar(&self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&self.status).weak());
            });
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "agents: {} | providers: {}",
                        self.agents.len(),
                        self.providers.len()
                    ))
                    .weak(),
                );
            });
            ui.add_space(6.0);
        });
    }

    fn ui_agents_section(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("Agents");
        });
        ui.separator();

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
                "opencode/deepseek-v4-flash-free".into(),
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
                    "opencode/deepseek-v4-flash-free".into(),
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
        });
        ui.separator();

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
        ui.horizontal(|ui| {
            ui.add_sized([60.0, 24.0], egui::Label::new(egui::RichText::new("key").weak()));
            ui.add(egui::TextEdit::singleline(&mut p.key).desired_width(120.0));
            ui.add_sized(
                [60.0, 24.0],
                egui::Label::new(egui::RichText::new("description").weak()),
            );
            ui.add(egui::TextEdit::singleline(&mut p.description).desired_width(450.0));
            ui.add_sized([60.0, 24.0], egui::Label::new(egui::RichText::new("npm").weak()));
            let npm_options = [
                "",
                "@ai-sdk/openai",
                "@ai-sdk/anthropic",
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
        });
        ui.horizontal(|ui| {
            ui.add_sized(
                [60.0, 24.0],
                egui::Label::new(egui::RichText::new("baseURL").weak()),
            );
            ui.add(egui::TextEdit::singleline(&mut p.base_url).desired_width(200.0));
            ui.add_sized(
                [60.0, 24.0],
                egui::Label::new(egui::RichText::new("apiKey").weak()),
            );
            ui.add(egui::TextEdit::singleline(&mut p.api_key).desired_width(350.0));
            ui.add_sized(
                [60.0, 24.0],
                egui::Label::new(egui::RichText::new("timeout").weak()),
            );
            ui.add(egui::TextEdit::singleline(&mut p.timeout).desired_width(53.0));
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
                ui.checkbox(&mut p.models[j].tool_call, "tool_call");
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new("context:").weak()),
                );
                ui.add(egui::TextEdit::singleline(&mut p.models[j].context).desired_width(53.0));
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new("output:").weak()),
                );
                ui.add(egui::TextEdit::singleline(&mut p.models[j].output).desired_width(53.0));
            });
            ui.horizontal_wrapped(|ui| {
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new("modalities.input:").weak()),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut p.models[j].modalities_input)
                        .desired_width(80.0),
                );
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new("modalities.output:").weak()),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut p.models[j].modalities_output)
                        .desired_width(80.0),
                );
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new("variants:").weak()),
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
                ui.checkbox(&mut p.new_model.tool_call, "tool_call");
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new("context:").weak()),
                );
                ui.add(egui::TextEdit::singleline(&mut p.new_model.context).desired_width(53.0));
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new("output:").weak()),
                );
                ui.add(egui::TextEdit::singleline(&mut p.new_model.output).desired_width(53.0));
            });
            ui.horizontal(|ui| {
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new("modalities.input:").weak()),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut p.new_model.modalities_input)
                        .desired_width(80.0),
                );
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new("modalities.output:").weak()),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut p.new_model.modalities_output)
                        .desired_width(80.0),
                );
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new("variants:").weak()),
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
                if ui.button("添加").clicked() {
                    if !p.new_model.id.trim().is_empty() {
                        p.models.push(p.new_model.clone());
                        p.new_model = ModelRow::new();
                        self.variant_open.remove(&show_new_model_key);
                    }
                }
            });
        }
    }

    fn ui_new_provider_form(&mut self, ui: &mut egui::Ui) {
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
                ui.add_sized([60.0, 24.0], egui::Label::new(egui::RichText::new("npm").weak()));
                let npm_options = [
                    "",
                    "@ai-sdk/openai",
                    "@ai-sdk/anthropic",
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
            });
            ui.horizontal(|ui| {
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new("baseURL").weak()),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_provider.base_url)
                        .hint_text("https://api.openai.com/v1")
                        .desired_width(200.0),
                );
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new("apiKey").weak()),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_provider.api_key)
                        .hint_text("sk-xxx")
                        .desired_width(408.0),
                );
                ui.add_sized(
                    [60.0, 24.0],
                    egui::Label::new(egui::RichText::new("timeout").weak()),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_provider.timeout)
                        .hint_text("180000")
                        .desired_width(53.0),
                );
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
                    ui.checkbox(&mut self.new_provider.models[j].tool_call, "tool_call");
                    ui.add_sized(
                        [60.0, 24.0],
                        egui::Label::new(egui::RichText::new("context:").weak()),
                    );
                    ui.add(egui::TextEdit::singleline(&mut self.new_provider.models[j].context).desired_width(53.0));
                    ui.add_sized(
                        [60.0, 24.0],
                        egui::Label::new(egui::RichText::new("output:").weak()),
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
                    ui.checkbox(&mut self.new_provider.new_model.tool_call, "tool_call");
                    ui.add_sized(
                        [60.0, 24.0],
                        egui::Label::new(egui::RichText::new("context:").weak()),
                    );
                    ui.add(egui::TextEdit::singleline(&mut self.new_provider.new_model.context).desired_width(53.0));
                    ui.add_sized(
                        [60.0, 24.0],
                        egui::Label::new(egui::RichText::new("output:").weak()),
                    );
                    ui.add(egui::TextEdit::singleline(&mut self.new_provider.new_model.output).desired_width(53.0));
                });
                ui.horizontal_wrapped(|ui| {
                    ui.add_sized(
                        [60.0, 24.0],
                        egui::Label::new(egui::RichText::new("modalities.input:").weak()),
                    );
                    ui.add(egui::TextEdit::singleline(&mut self.new_provider.new_model.modalities_input).desired_width(80.0));
                    ui.add_sized(
                        [60.0, 24.0],
                        egui::Label::new(egui::RichText::new("modalities.output:").weak()),
                    );
                    ui.add(egui::TextEdit::singleline(&mut self.new_provider.new_model.modalities_output).desired_width(80.0));
                    ui.add_sized(
                        [60.0, 24.0],
                        egui::Label::new(egui::RichText::new("variants:").weak()),
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
                    if ui.button(display).clicked() {}
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
                    if ui.button("添加").clicked() {
                        if !self.new_provider.new_model.id.trim().is_empty() {
                            self.new_provider.models.push(self.new_provider.new_model.clone());
                            self.new_provider.new_model = ModelRow::new();
                            self.variant_open.remove(&show_new_model_key);
                        }
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
        let (root, agents, providers) = load_or_empty(&self.config_path);
        self.root = root;
        self.agents = agents;
        self.providers = providers;
        self.agent_open = self.agents.iter().map(|a| a.key.clone()).collect();
        self.provider_open = self.providers.iter().map(|p| p.key.clone()).collect();
        self.status = format!(
            "已加载: {} agents, {} providers",
            self.agents.len(),
            self.providers.len()
        );
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

    fn save(&mut self) {
        if let Err(e) = ensure_parent_dir(&self.config_path) {
            self.status = format!("保存失败: {}", e);
            return;
        }
        let mut root = std::mem::take(&mut self.root);
        if let Value::Object(o) = &mut root {
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
        let content = match serde_json::to_string_pretty(&root) {
            Ok(s) => s,
            Err(e) => {
                self.root = root;
                self.status = format!("保存失败: 序列化错误 {}", e);
                return;
            }
        };
        let write_res = if is_wsl_path(&self.config_path) {
            crate::util::write_wsl_file(&self.config_path, &content)
        } else {
            fs::write(&self.config_path, content).map_err(|e| e.to_string())
        };
        match write_res {
            Ok(()) => {
                self.status = "已保存成功".into();
            }
            Err(e) => {
                self.root = root;
                self.status = format!("保存失败: {}", e);
            }
        }
    }
}

pub fn load_or_empty(path: &str) -> (Value, Vec<AgentRow>, Vec<ProviderRow>) {
    let content = if path.is_empty() {
        String::new()
    } else if is_wsl_path(path) {
        read_wsl_file(path).unwrap_or_default()
    } else if std::path::Path::new(path).exists() {
        match fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("load error: {}", e);
                String::new()
            }
        }
    } else {
        String::new()
    };

    let v: Value = serde_json::from_str(&content).unwrap_or_else(|_| Value::Object(Map::new()));

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

    (v, agents, providers)
}
