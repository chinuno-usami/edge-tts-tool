use egui::{FontData, FontDefinitions, FontFamily};

/// Injects system fonts (Microsoft YaHei on Windows, PingFang on macOS) into egui
pub fn setup_custom_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    let font_candidates = [
        // Windows
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msyh.ttf",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
        // macOS
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/Library/Fonts/Arial Unicode.ttf",
        // Linux
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    ];

    for path in font_candidates {
        if let Ok(font_bytes) = std::fs::read(path) {
            fonts.font_data.insert(
                "cjk_fallback".to_owned(),
                FontData::from_owned(font_bytes),
            );

            // Insert at front of proportional font list
            fonts
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .insert(0, "cjk_fallback".to_owned());

            // Add to monospace font list
            fonts
                .families
                .entry(FontFamily::Monospace)
                .or_default()
                .push("cjk_fallback".to_owned());

            ctx.set_fonts(fonts);
            log::info!("Loaded CJK font successfully from: {}", path);
            return;
        }
    }

    log::warn!("No CJK system fonts found; falling back to default egui fonts.");
}
