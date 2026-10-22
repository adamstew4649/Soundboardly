#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod clip;
mod download;
mod hotkeys;
mod library;

fn main() -> eframe::Result {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([760.0, 560.0]).with_title("Soundboardly"),
        ..Default::default()
    };
    eframe::run_native("Soundboardly", opts, Box::new(|cc| Ok(Box::new(app::App::new(&cc.egui_ctx)?))))
}
