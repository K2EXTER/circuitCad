//! The text and mark colours share one lightness and chroma (OKLCH L 0.72-0.78, C 0.14-0.15) and differ only in hue, so none of them
//! looks louder than another on the dark panels. A new one: keep L and C, pick a hue (the blues `INFO`, `ACCENT` and `SELECTED_BG` are the exception: softer, so the UI chrome does not glare).
//!
//! The colours that carry a meaning, one value each (text on the dark panels and windows, and marks on the canvases):
//! red for what is wrong, amber for what to watch, green for what is fine. Use these instead of a new `Color32::from_rgb`,
//! so the same meaning looks the same in every window.

use eframe::egui::{self, Color32};

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
pub const SPACE_M: f32 = 8.0;
pub const SPACE_L: f32 = 12.0;

/// Spacing, corners and widget colours for every window (called once from `startup.rs`, dark panels).
pub fn apply(style: &mut egui::Style) {
    let sp = &mut style.spacing;
    sp.item_spacing = egui::vec2(8.0, 5.0);
    sp.button_padding = egui::vec2(8.0, 3.0);
    sp.interact_size.y = 22.0;
    sp.menu_margin = egui::Margin::same(6);
    sp.window_margin = egui::Margin::same(10);
    let v = &mut style.visuals;
    let r = egui::CornerRadius::same(4);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = r;
    }
    v.widgets.inactive.weak_bg_fill = Color32::from_gray(48);
    v.widgets.inactive.bg_fill = Color32::from_gray(48);
    v.widgets.hovered.weak_bg_fill = Color32::from_gray(62);
    v.widgets.active.weak_bg_fill = ACCENT;
    v.widgets.active.bg_fill = ACCENT;
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ACCENT_HOVER);
    // the chosen tab, tool or list row: one fill, white text (egui draws selected text in `selection.stroke`), a light edge
    v.selection.bg_fill = SELECTED_BG;
    v.selection.stroke = egui::Stroke::new(1.0, Color32::WHITE);
    // three levels of ground: the panels, the windows raised above them, and the wells (text fields, the plot) sunk below
    v.panel_fill = Color32::from_gray(26);
    v.window_fill = Color32::from_gray(34);
    v.extreme_bg_color = Color32::from_gray(15);
    v.faint_bg_color = Color32::from_gray(38);
    v.window_stroke = egui::Stroke::new(1.0, Color32::from_gray(70));
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, Color32::from_gray(58));
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, Color32::from_gray(205));
    v.window_corner_radius = egui::CornerRadius::same(8);
    v.menu_corner_radius = egui::CornerRadius::same(6);
}

/// Something is wrong (an error, a failed check, a violation).
pub const ERROR: Color32 = Color32::from_rgb(244, 123, 116);
/// Something to watch (a warning, a caution, a value near its limit).
pub const WARN: Color32 = Color32::from_rgb(223, 145, 26);
/// Something is fine (a check passed, a step done).
pub const OK: Color32 = Color32::from_rgb(89, 190, 108);
