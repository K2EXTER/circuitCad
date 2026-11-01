//! Buttons that come up in many windows, so they look and read the same everywhere.
//!
//! The rule for the other buttons: `ui.button` is for what a whole panel, window or step does ("DC動作点を解析", "DRC を実行");
//! `ui.small_button` is for what acts on one row or one item next to it ("削除", "外す", "解除"). Every button gets an
//! `on_hover_text` that says what it does and what it leaves alone.

use eframe::egui;

/// The "閉じる" button at the top of a result window (the waveforms and tables under the schematic, a lesson window). `what` is the
/// thing that closes, for the hover text ("波形" gives "波形を閉じます").
pub fn close_button(ui: &mut egui::Ui, what: &str) -> bool {
    ui.button("閉じる").on_hover_text(format!("{what}を閉じます")).clicked()
}
