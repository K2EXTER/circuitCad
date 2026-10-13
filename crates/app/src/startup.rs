use super::*;

/// Without a console a panic would vanish silently: keep it in a log file.
pub(super) fn install_crash_log() {
    let dir = part_lib::default_dir().parent().map(|p| p.to_path_buf());
    std::panic::set_hook(Box::new(move |info| {
        if let Some(d) = &dir {
            let _ = std::fs::create_dir_all(d);
            let _ = std::fs::write(d.join("crash.log"), format!("{info}\n"));
        }
    }));
}

/// egui's built-in fonts lack Japanese glyphs, and mixing them with a Windows font puts Latin
/// letters and kana on slightly different baselines. So use one system font that covers both
/// Japanese and half-width alphanumerics for everything; egui's own fonts stay as a fallback.
pub(super) const BUNDLED_FONT: &[u8] = include_bytes!("../assets/fonts/BIZUDGothic-Regular.ttf");

pub(super) fn install_japanese_font(ctx: &egui::Context) {
    pub(super) const CANDIDATES: &[&str] = &[
        r"C:\Windows\Fonts\BIZ-UDGothicR.ttc",
        r"C:\Windows\Fonts\meiryo.ttc",
        r"C:\Windows\Fonts\YuGothM.ttc",
        r"C:\Windows\Fonts\msgothic.ttc",
        // macOS
        "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/Library/Fonts/Arial Unicode.ttf",
        // Linux (Noto CJK / IPA / Takao packages)
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf",
        "/usr/share/fonts/truetype/takao-gothic/TakaoPGothic.ttf",
    ];
    // fall back to the bundled BIZ UD Gothic (SIL OFL 1.1, see assets/fonts/OFL.txt) so every OS shows Japanese
    // CIRCUITCAD_BUNDLED_FONT=1 skips the system fonts (to check the bundled one)
    let use_system = std::env::var_os("CIRCUITCAD_BUNDLED_FONT").is_none();
    let bytes = CANDIDATES.iter().filter(|_| use_system).find_map(|p| std::fs::read(p).ok()).unwrap_or_else(|| BUNDLED_FONT.to_vec());
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("jp".into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
    for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families.entry(fam).or_default().insert(0, "jp".into());
    }
    ctx.set_fonts(fonts);
    // the default body / button size (12.5 px) is not a whole pixel count (13 px fails the same way), and with this font the
    // thin hyphen / minus stroke then vanishes ("1e-6" reads "1e 6", "-3dB" reads "3dB")
    ctx.all_styles_mut(|style| {
        for ts in [egui::TextStyle::Body, egui::TextStyle::Button] {
            style.text_styles.insert(ts, egui::FontId::proportional(14.0));
        }
    });
}
