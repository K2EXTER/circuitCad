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

/// The one main action of a panel or form ("保存してライブラリに追加", "クイック追加"). Buttons are grey until pressed, so it keeps the grey
/// and is told apart by its bold text and a little more height; the hover edge and the press colour are the shared accent. Use at most
/// one per panel.
pub fn primary_button(ui: &mut egui::Ui, enabled: bool, label: &str) -> egui::Response {
    let button = egui::Button::new(egui::RichText::new(label).strong()).min_size(egui::vec2(0.0, 26.0));
    ui.add_enabled(enabled, button)
}

/// The title of a block inside a panel or window ("回路図の記号"): a step stronger than the body, in the light accent, with a little air above.
pub fn section_heading(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    ui.add_space(crate::theme::SPACE_S);
    heading_inline(ui, text)
}

/// The same title without the air above, for a title that shares a row with a button (the top row of a result window).
pub fn heading_inline(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    ui.label(egui::RichText::new(text).strong().color(crate::theme::INFO))
}

/// Where a guide or table window first opens: just right of the left panel, so the panel it talks about ("左の…") stays in view
/// and the buttons in it can still be pressed (egui would open it in the top-left corner, on top of the panel).
pub fn beside_panel() -> egui::Pos2 {
    egui::pos2(crate::PANEL_WIDTH + 14.0, 56.0)
}

/// A row of tabs: each tab is a button, the chosen one is filled and underlined, and a line runs under the row, so it reads as tabs and
/// not as a few words. `tabs` is `(value, label, hover text)`; an empty hover text shows none. The row wraps when the window is narrow.
pub fn tab_bar<T: PartialEq + Copy>(ui: &mut egui::Ui, current: &mut T, tabs: &[(T, &str, &str)]) {
    let accent = crate::theme::INFO;
    let row = ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        for &(value, label, hover) in tabs {
            let selected = *current == value;
            let text = egui::RichText::new(label).size(14.0);
            let (text, fill) = if selected {
                (text.strong().color(ui.visuals().strong_text_color()), ui.visuals().selection.bg_fill)
            } else {
                (text, ui.visuals().widgets.inactive.weak_bg_fill)
            };
            let r = ui.add(egui::Button::new(text).fill(fill).min_size(egui::vec2(72.0, 26.0)).corner_radius(egui::CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }));
            let r = if hover.is_empty() { r } else { r.on_hover_text(hover) };
            if selected {
                ui.painter().hline(r.rect.x_range(), r.rect.bottom(), egui::Stroke::new(3.0, accent));
            }
            if r.clicked() {
                *current = value;
            }
        }
    });
    // the line under the whole row (the chosen tab's underline sits on it)
    let y = row.response.rect.bottom() + 1.0;
    let x = ui.max_rect().x_range();
    ui.painter().hline(x, y, egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color));
    ui.add_space(6.0);
}
