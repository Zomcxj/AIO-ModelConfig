use eframe::egui;

pub fn card_frame<R>(
    ui: &mut egui::Ui,
    open: bool,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::Response {
    ui.set_min_width(ui.available_width());
    let r = 10.0;
    let fill = if open {
        ui.visuals().faint_bg_color
    } else {
        ui.visuals().extreme_bg_color
    };
    egui::Frame::NONE
        .fill(fill)
        .corner_radius(egui::CornerRadius::same(r as u8))
        .stroke(egui::Stroke::new(
            1.0,
            ui.visuals().widgets.noninteractive.bg_stroke.color,
        ))
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
