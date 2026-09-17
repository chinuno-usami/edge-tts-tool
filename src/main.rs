#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use edge_tts_tool::app::TtsApp;
use edge_tts_tool::font;

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Edge TTS 语音合成工具")
            .with_inner_size([780.0, 600.0])
            .with_min_inner_size([620.0, 480.0])
            .with_active(true),
        ..Default::default()
    };

    eframe::run_native(
        "Edge TTS 语音合成工具",
        native_options,
        Box::new(|cc| {
            // Setup Chinese system fonts for clean glyph rendering
            font::setup_custom_fonts(&cc.egui_ctx);
            Ok(Box::new(TtsApp::new(cc)))
        }),
    )
}
