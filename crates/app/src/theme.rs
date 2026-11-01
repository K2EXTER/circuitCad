//! The colours that carry a meaning, one value each (text on the dark panels and windows, and marks on the canvases):
//! red for what is wrong, amber for what to watch, green for what is fine. Use these instead of a new `Color32::from_rgb`,
//! so the same meaning looks the same in every window.

use eframe::egui::Color32;

/// Something is wrong (an error, a failed check, a violation).
pub const ERROR: Color32 = Color32::from_rgb(230, 60, 60);
/// Something to watch (a warning, a caution, a value near its limit).
pub const WARN: Color32 = Color32::from_rgb(220, 160, 40);
/// Something is fine (a check passed, a step done).
pub const OK: Color32 = Color32::from_rgb(60, 160, 90);
