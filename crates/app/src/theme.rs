//! The text and mark colours share one lightness and chroma (OKLCH L 0.72-0.78, C 0.14-0.15) and differ only in hue, so none of them
//! looks louder than another on the dark panels. A new one: keep L and C, pick a hue (the blues `INFO`, `ACCENT` and `SELECTED_BG` are the exception: softer, so the UI chrome does not glare).
//!
//! The colours that carry a meaning, one value each (text on the dark panels and windows, and marks on the canvases):
//! red for what is wrong, amber for what to watch, green for what is fine. Use these instead of a new `Color32::from_rgb`,
//! so the same meaning looks the same in every window.

use eframe::egui::{self, Color32};
use std::sync::atomic::{AtomicBool, Ordering};

/// The panels and windows on a light ground (settings: 表示 → パネルの明るさ). The canvases keep their own theme (`canvas_theme`).
static LIGHT: AtomicBool = AtomicBool::new(false);

/// Switch the panels between dark and light; call `apply` on the style afterwards (`startup::apply_panel_theme` does both).
pub fn set_light(on: bool) {
    LIGHT.store(on, Ordering::Relaxed);
}

/// Whether the panels are light.
pub fn is_light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}

/// The meaning colours for text and marks on the panels: the constants below on the dark panels, darker ones (the same hues, OKLCH
/// L about 0.50) on the light panels, so they keep their contrast. Use these for panel text; the canvases use the constants.
pub fn error() -> Color32 {
    if is_light() { Color32::from_rgb(190, 45, 45) } else { ERROR }
}
pub fn warn() -> Color32 {
    if is_light() { Color32::from_rgb(165, 95, 0) } else { WARN }
}
pub fn ok() -> Color32 {
    if is_light() { Color32::from_rgb(25, 125, 55) } else { OK }
}
pub fn info() -> Color32 {
    if is_light() { Color32::from_rgb(25, 100, 165) } else { INFO }
}
pub fn highlight() -> Color32 {
    if is_light() { Color32::from_rgb(140, 105, 0) } else { HIGHLIGHT }
}

/// A grey of the panel ground at the dark-theme level `g` (26 = the panel, 38 = a band, 75 = a border), mirrored on the light panels,
/// so bands and borders keep their steps from the ground in both.
pub fn gray(g: u8) -> Color32 {
    if is_light() { Color32::from_gray((268 - g as i32).clamp(0, 255) as u8) } else { Color32::from_gray(g) }
}

/// The accent (selected tab or tool, focus, the main action).
pub const ACCENT: Color32 = Color32::from_rgb(26, 90, 130);
/// The accent when the pointer is over it.
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(40, 112, 156);

/// The fill of whatever is chosen (tab, tool button, list row).
pub const SELECTED_BG: Color32 = Color32::from_rgb(24, 82, 120);
/// Titles and links in the light accent (block headings, a step to do now, a pointer to a feature).
pub const INFO: Color32 = Color32::from_rgb(103, 173, 221);
/// "Look here" marks (the next lesson, the recommended choice): a warm gold, apart from the amber of WARN.
pub const HIGHLIGHT: Color32 = Color32::from_rgb(211, 182, 59);

/// Spacing scale: use these instead of a new number in `add_space`.
pub const SPACE_S: f32 = 4.0;
#[allow(dead_code)] // part of the scale (docs/code-notes/app/ui-rules.md), kept for new panels
pub const SPACE_M: f32 = 8.0;
#[allow(dead_code)]
pub const SPACE_L: f32 = 12.0;
/// Indent of a folded group's contents and the gap between rows (the values `apply` sets; a flush group restores them inside).
pub const INDENT: f32 = 20.0;
pub const ROW_GAP: f32 = 5.0;

/// Spacing, corners and widget colours for every window (from `startup.rs`, again when the panel brightness changes).
pub fn apply(style: &mut egui::Style) {
    style.visuals = if is_light() { egui::Visuals::light() } else { egui::Visuals::dark() };
    let sp = &mut style.spacing;
    sp.item_spacing = egui::vec2(8.0, ROW_GAP);
    sp.button_padding = egui::vec2(8.0, 3.0);
    sp.interact_size.y = 24.0;
    // the fold arrow of a list is as wide as this: a larger target, and the guide line of a folded group is drawn at its centre
    sp.icon_width = 18.0;
    sp.indent = INDENT;
    sp.menu_margin = egui::Margin::same(6);
    sp.window_margin = egui::Margin::same(10);
    let v = &mut style.visuals;
    let r = egui::CornerRadius::same(4);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = r;
    }
    v.indent_has_left_vline = true;
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, gray(58));
    v.widgets.inactive.weak_bg_fill = gray(48);
    v.widgets.inactive.bg_fill = gray(48);
    v.widgets.hovered.weak_bg_fill = gray(62);
    v.widgets.active.weak_bg_fill = ACCENT;
    v.widgets.active.bg_fill = ACCENT;
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ACCENT_HOVER);
    // the chosen tab, tool or list row: one fill, white text (egui draws selected text in `selection.stroke`), a light edge
    v.selection.bg_fill = SELECTED_BG;
    v.selection.stroke = egui::Stroke::new(1.0, Color32::WHITE);
    // three levels of ground: the panels, the windows raised above them, and the wells (text fields, the plot) sunk below
    // (on the light panels the windows are the lightest and the wells sit a step darker than the panel)
    v.panel_fill = gray(26);
    v.window_fill = if is_light() { Color32::from_gray(250) } else { gray(34) };
    v.extreme_bg_color = if is_light() { Color32::WHITE } else { gray(15) };
    v.faint_bg_color = gray(38);
    v.window_stroke = egui::Stroke::new(1.0, gray(70));
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, if is_light() { Color32::from_gray(35) } else { Color32::from_gray(205) });
    v.hyperlink_color = info();
    v.window_corner_radius = egui::CornerRadius::same(8);
    v.menu_corner_radius = egui::CornerRadius::same(6);
}

/// Screen pixel sizes (font size × pixels per point) at which the bundled Japanese font draws no hyphen / minus stroke at all:
/// "-5.000 V" came out as "5.000 V". Measured on screen from 5 to 30 px at UI scales 1.0 and 1.25 (2026-11-17); whole and
/// fractional sizes alike, every other size keeps the stroke.
const MINUS_LOST: [(f32, f32); 2] = [(7.5, 8.5), (12.5, 13.5)];

/// `size` (points), moved just above a band of `MINUS_LOST` when it falls in one on a screen of `ppp` pixels per point.
pub fn readable_size(size: f32, ppp: f32) -> f32 {
    let px = size * ppp;
    MINUS_LOST.iter().find(|(lo, hi)| px >= *lo && px < *hi).map_or(size, |(_, hi)| hi / ppp)
}

/// The font for text painted on a canvas or chart at `size` points (a fixed size or one that follows the zoom): never a size at
/// which the minus of a value vanishes.
pub fn canvas_font(painter: &egui::Painter, size: f32) -> egui::FontId {
    egui::FontId::proportional(readable_size(size, painter.ctx().pixels_per_point()))
}

/// The panel text sizes (body and buttons 14, small 12, monospace 14), kept out of `MINUS_LOST` on this screen: at a UI scale
/// such as 1.1 the small text lands on 13.2 px. Cheap enough for every frame; it only writes when the pixels per point change.
pub fn keep_minus_signs(ctx: &egui::Context) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static LAST: AtomicU32 = AtomicU32::new(0);
    let ppp = ctx.pixels_per_point();
    if LAST.swap(ppp.to_bits(), Ordering::Relaxed) == ppp.to_bits() {
        return;
    }
    let (body, small) = (readable_size(14.0, ppp), readable_size(12.0, ppp));
    ctx.all_styles_mut(|style| {
        for ts in [egui::TextStyle::Body, egui::TextStyle::Button] {
            style.text_styles.insert(ts, egui::FontId::proportional(body));
        }
        style.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(small));
        style.text_styles.insert(egui::TextStyle::Monospace, egui::FontId::monospace(body));
    });
}

/// Something is wrong (an error, a failed check, a violation).
pub const ERROR: Color32 = Color32::from_rgb(244, 123, 116);
/// Something to watch (a warning, a caution, a value near its limit).
pub const WARN: Color32 = Color32::from_rgb(223, 145, 26);
/// Something is fine (a check passed, a step done).
pub const OK: Color32 = Color32::from_rgb(89, 190, 108);

/// Colours of the lines of a small chart, one per series (the heat charts).
pub fn series_color(k: usize) -> egui::Color32 {
    const COLORS: [egui::Color32; 5] = [
        egui::Color32::from_rgb(240, 150, 60),
        egui::Color32::from_rgb(90, 170, 240),
        egui::Color32::from_rgb(120, 210, 120),
        egui::Color32::from_rgb(220, 110, 200),
        egui::Color32::from_rgb(220, 210, 90),
    ];
    if crate::canvas_theme::palette().cvd {
        return crate::plot::series(k);
    }
    COLORS[k % COLORS.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_size_where_the_font_drops_the_minus_moves_just_above_it() {
        // at 100 %: 13 px (the old net labels) and 8 px (the smallest pin numbers) lose the stroke, 12 and 14 keep it
        assert_eq!(readable_size(13.0, 1.0), 13.5);
        assert_eq!(readable_size(8.0, 1.0), 8.5);
        assert_eq!(readable_size(12.0, 1.0), 12.0);
        assert_eq!(readable_size(14.0, 1.0), 14.0);
        // the bands are screen pixels: at a UI scale of 1.1 the 12 px small text is 13.2 px on screen
        assert!((readable_size(12.0, 1.1) * 1.1 - 13.5).abs() < 1e-4);
        assert_eq!(readable_size(14.0, 1.25), 14.0);
        // whatever the zoom of a canvas gives, the size that comes out is never in a band
        for k in 0..400 {
            for ppp in [1.0, 1.1, 1.25, 1.5, 2.0] {
                let px = readable_size(5.0 + k as f32 * 0.07, ppp) * ppp;
                assert!(MINUS_LOST.iter().all(|(lo, hi)| px < *lo || px >= hi - 1e-3), "{px} px at {ppp}");
            }
        }
    }
}
