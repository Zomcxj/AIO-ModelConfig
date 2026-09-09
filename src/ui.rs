use eframe::egui;

pub fn card_frame<R>(
    ui: &mut egui::Ui,
    open: bool,
    highlight: u8,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::Response {
    let corner = ui.visuals().widgets.noninteractive.corner_radius;
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
        .corner_radius(corner)
        .stroke(egui::Stroke::new(stroke_width, stroke_color))
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.style_mut().spacing.item_spacing = egui::vec2(8.0, 2.0);
            add(ui);
        })
        .response
}

/// 依次渲染卡片列表（每卡片之间附加行间距）。
pub fn card_list(ui: &mut egui::Ui, keys: &[usize], row_gap: f32, mut f: impl FnMut(&mut egui::Ui, usize)) {
    for &idx in keys {
        f(ui, idx);
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

/// 数字文本编辑框：内容非空且无法解析为数字时红色高亮并悬停提示。
/// （保存时非法数字字段会被丢弃——这里让用户在丢弃前就看到。）
pub fn numeric_text_edit(
    ui: &mut egui::Ui,
    s: &mut String,
    width: f32,
    hint: &str,
) -> egui::Response {
    let valid = s.trim().is_empty() || crate::util::parse_number_text(s).is_some();
    let edit = egui::TextEdit::singleline(s)
        .desired_width(width)
        .hint_text(hint);
    if valid {
        ui.add(edit)
    } else {
        ui.add(edit.text_color(egui::Color32::from_rgb(220, 90, 90)))
            .on_hover_text("无效数字：保存时该字段将被忽略")
    }
}
