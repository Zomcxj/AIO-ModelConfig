#![windows_subsystem = "windows"]

use eframe::egui;
use opencode_model_config::app::App;
use opencode_model_config::theme::{Radius, Theme};

const ICON_BYTES: &[u8] = include_bytes!("../assets/icon_rgba.bin");
const ICON_W: u32 = 256;
const ICON_H: u32 = 256;

fn main() -> eframe::Result {
    let mut options = eframe::NativeOptions::default();
    options.viewport = egui::ViewportBuilder::default()
        .with_inner_size([1250.0, 820.0])
        .with_min_inner_size([970.0, 660.0])
        .with_title("opencode-model-config")
        .with_icon(egui::IconData { rgba: ICON_BYTES.to_vec(), width: ICON_W, height: ICON_H });
    eframe::run_native(
        "opencode-model-config",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_fonts(build_cjk_fonts());
            Theme::default().apply(&cc.egui_ctx, Radius::default());
            #[cfg(target_os = "windows")]
            {
                use raw_window_handle::HasWindowHandle;
                if let Ok(handle) = cc.window_handle() {
                    if let raw_window_handle::RawWindowHandle::Win32(w) = handle.as_raw() {
                        unsafe {
                            let hwnd = w.hwnd.get() as *mut core::ffi::c_void;
                            opencode_model_config::cursor::init_grabbing_cursor(hwnd);
                        }
                    }
                }
            }
            Ok(Box::new(App::default()))
        }),
    )
}

fn build_cjk_fonts() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    for path in [
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\simhei.ttf",
        "C:\\Windows\\Fonts\\msjh.ttc",
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert("cjk".to_owned(), egui::FontData::from_owned(bytes).into());
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(family).or_default().push("cjk".to_owned());
            }
            break;
        }
    }
    fonts
}
