//! The colours that carry a meaning, one value each (text on the dark panels and windows, and marks on the canvases):
//! red for what is wrong, amber for what to watch, green for what is fine. Use these instead of a new `Color32::from_rgb`,
//! so the same meaning looks the same in every window.

use eframe::egui::{self, Color32};

/// The accent (selected tab or tool, focus, the main action).
pub const ACCENT: Color32 = Color32::from_rgb(0, 122, 178);
/// The accent when the pointer is over it.
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(20, 145, 205);

/// Titles and links in the light accent (block headings, a step to do now, a pointer to a feature).
pub const INFO: Color32 = Color32::from_rgb(90, 190, 255);
/// "Look here" marks (the next lesson, the recommended choice): a warm gold, apart from the amber of WARN.
pub const HIGHLIGHT: Color32 = Color32::from_rgb(255, 205, 70);

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
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ACCENT_HOVER);
    v.selection.bg_fill = Color32::from_rgb(0, 84, 130);
    v.selection.stroke = egui::Stroke::new(1.0, INFO);
    v.window_corner_radius = egui::CornerRadius::same(8);
    v.menu_corner_radius = egui::CornerRadius::same(6);
}

/// Something is wrong (an error, a failed check, a violation).
pub const ERROR: Color32 = Color32::from_rgb(230, 60, 60);
/// Something to watch (a warning, a caution, a value near its limit).
pub const WARN: Color32 = Color32::from_rgb(220, 160, 40);
/// Something is fine (a check passed, a step done).
pub const OK: Color32 = Color32::from_rgb(60, 160, 90);
