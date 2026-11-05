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

/// The one main action of a panel or form ("保存してライブラリに追加", "クイック追加"). Buttons are grey until pressed and the same size, so
/// this looks like any other button; it exists so the main actions stay easy to find in the code (and to restyle in one place).
pub fn primary_button(ui: &mut egui::Ui, enabled: bool, label: &str) -> egui::Response {
    ui.add_enabled(enabled, egui::Button::new(label))
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

/// The gap between two groups of controls in a side panel: close controls sit 4 px apart, groups 12 px, with a faint line between.
pub fn group_gap(ui: &mut egui::Ui) {
    ui.add_space(crate::theme::SPACE_S);
    ui.separator();
    ui.add_space(crate::theme::SPACE_S);
}

/// The title of a folded group with the number of items it holds, as a quiet badge after the name ("ADC  1"), so the count does not
/// read as part of the name.
pub fn count_title(ui: &egui::Ui, name: &str, count: usize) -> egui::text::LayoutJob {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    job.append(name, 0.0, egui::TextFormat::simple(font.clone(), ui.visuals().text_color()));
    job.append(&count.to_string(), 8.0, egui::TextFormat::simple(font, ui.visuals().weak_text_color()));
    job
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
