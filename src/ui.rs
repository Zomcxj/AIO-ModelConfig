use eframe::egui;

pub fn card_frame<R>(
    ui: &mut egui::Ui,
    open: bool,
    highlight: u8,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::Response {
    let r = 10.0;
    let fill = if open {
        ui.visuals().faint_bg_color
    } else {
        ui.visuals().extreme_bg_color
    };
    let (stroke_color, stroke_width) = match highlight {
        1 => (egui::Color32::from_rgb(255, 180, 50), 2.0),  // source: orange
        2 => (egui::Color32::from_rgb(100, 200, 100), 2.0),  // target: green
        _ => (ui.visuals().widgets.noninteractive.bg_stroke.color, 1.0),
    };
    egui::Frame::NONE
        .fill(fill)
        .corner_radius(egui::CornerRadius::same(r as u8))
        .stroke(egui::Stroke::new(stroke_width, stroke_color))
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.style_mut().spacing.item_spacing = egui::vec2(8.0, 2.0);
            add(ui);
        })
        .response
}

pub fn card_grid(
    ui: &mut egui::Ui,
    keys: &[usize],
    cols: usize,
    row_gap: f32,
    mut f: impl FnMut(&mut egui::Ui, usize),
) {
    if cols == 1 {
        for &idx in keys {
            f(ui, idx);
            ui.add_space(row_gap);
        }
        return;
    }
    for chunk in keys.chunks(cols) {
        ui.columns(cols, |cols_ui| {
            for (ci, &idx) in chunk.iter().enumerate() {
                f(&mut cols_ui[ci], idx);
            }
        });
        ui.add_space(row_gap);
    }
}

pub struct DragHandle;

impl egui::Widget for DragHandle {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let button = egui::Button::new("")
            .frame(false)
            .sense(egui::Sense::click_and_drag())
            .min_size(egui::vec2(14.0, 18.0));
        let resp = ui.add(button);
        let painter = ui.painter();
        let rect = resp.rect;
        let active = resp.hovered() || resp.dragged();
        let color = if active {
            ui.visuals().strong_text_color()
        } else {
            ui.visuals().weak_text_color()
        };
        for row in 0..3 {
            for col in 0..2 {
                let c = egui::pos2(
                    rect.left() + 2.5 + col as f32 * 5.0,
                    rect.top() + 3.0 + row as f32 * 5.0,
                );
                painter.circle_filled(c, if active { 1.5 } else { 1.0 }, color);
            }
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        resp
    }
}

pub fn move_item<T>(items: &mut Vec<T>, from: usize, to: usize) {
    if from == to || from >= items.len() || to >= items.len() {
        return;
    }
    let item = items.remove(from);
    items.insert(to, item);
}

pub fn editable_combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    value: &mut String,
    options: &[&str],
    width: f32,
) {
    let popup_id = ui.make_persistent_id(id);

    let response = ui.add(
        egui::TextEdit::singleline(value)
            .desired_width(width)
            .hint_text("选择或输入..."),
    );

    let btn = ui.small_button("▼");

    let show_popup = response.gained_focus() || btn.clicked();

    if show_popup {
        let filtered: Vec<&&str> = options
            .iter()
            .filter(|o| value.is_empty() || o.to_lowercase().contains(&value.to_lowercase()))
            .collect();

        if !filtered.is_empty() {
            egui::Area::new(popup_id)
                .fixed_pos(btn.rect.left_bottom())
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(200.0);
                        for opt in &filtered {
                            if ui
                                .selectable_label(**opt == value.as_str(), opt.to_string())
                                .clicked()
                            {
                                *value = opt.to_string();
                                ui.close_menu();
                            }
                        }
                    });
                });
        }
    }
}
