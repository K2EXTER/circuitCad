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

/// `primary_button` as wide as the space left in its row (the main action of a side panel, under bars of the same width).
pub fn primary_button_wide(ui: &mut egui::Ui, enabled: bool, label: &str) -> egui::Response {
    let size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
    ui.add_enabled_ui(enabled, |ui| ui.add_sized(size, egui::Button::new(label))).inner
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

/// A top-level folded group of a side panel: its title row sits on a band one step lighter than the panel (lighter again while open), so
/// the groups read as sections and the lists and rows inside them, on the plain panel, as a level below. With `flush` the contents touch
/// the band (no gap), so a box drawn inside (see `well`) joins the band as one piece; the contents restore the row gap themselves.
///
/// Returns the group's response and whether the band was clicked anywhere (the whole band is the target, not just the arrow and the
/// title; `id` tells two bands apart); the caller flips `open` itself.
pub fn band_header<R>(
    ui: &mut egui::Ui,
    id: &str,
    title: impl Into<egui::WidgetText>,
    open: bool,
    flush: bool,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> (egui::CollapsingResponse<R>, bool) {
    let slot = ui.painter().add(egui::Shape::Noop);
    // no guide line under the arrow: the band and the indent already show what belongs to the group
    let r = ui
        .scope(|ui| {
            ui.visuals_mut().indent_has_left_vline = false;
            if flush {
                ui.spacing_mut().item_spacing.y = 0.0;
            }
            egui::CollapsingHeader::new(title).open(Some(open)).show(ui, body)
        })
        .inner;
    let rows = r.header_response.rect.y_range();
    let joined = open && flush;
    let band = egui::Rect::from_x_y_ranges(ui.max_rect().x_range(), rows).expand2(egui::vec2(0.0, 1.0));
    let band = if joined { egui::Rect::from_min_max(band.min, egui::pos2(band.max.x, rows.max)) } else { band };
    let hit = ui.interact(egui::Rect::from_x_y_ranges(ui.max_rect().x_range(), rows), ui.id().with(("band_header", id)), egui::Sense::click());
    let fill = egui::Color32::from_gray(match (open, hit.hovered()) {
        (true, false) => 48,
        (true, true) => 54,
        (false, false) => 38,
        (false, true) => 46,
    });
    let hit = hit.on_hover_cursor(egui::CursorIcon::PointingHand);
    let low = if joined { 0 } else { 4 };
    let corners = egui::CornerRadius { nw: 4, ne: 4, sw: low, se: low };
    ui.painter().set(slot, egui::epaint::RectShape::filled(band, corners, fill));
    // a border round the band and the contents under it (they read as one card)
    if let (true, Some(body)) = (joined, &r.body_response) {
        let card = egui::Rect::from_min_max(band.min, egui::pos2(band.max.x, body.rect.bottom()));
        ui.painter().rect_stroke(card, 4, egui::Stroke::new(1.0, egui::Color32::from_gray(75)), egui::StrokeKind::Inside);
    }
    (r, hit.clicked())
}

/// One of a long list of folded sections in a side panel (the analyses): every row sits on a band, the open one lighter, bold, and boxed
/// with its contents, so the open ones stand out. The whole row is the click target and
/// shows `hint` on hover. The open state is kept by `id`. Returns what `body` returns while the section is open.
pub fn section_header<R>(ui: &mut egui::Ui, id: egui::Id, default_open: bool, title: &str, hint: &str, body: impl FnOnce(&mut egui::Ui) -> R) -> Option<R> {
    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, default_open);
    let open = state.is_open();
    let (row, hit) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ui.spacing().interact_size.y), egui::Sense::click());
    let gray = match (open, hit.hovered()) {
        (true, false) => 48,
        (true, true) => 54,
        (false, false) => 38,
        (false, true) => 46,
    };
    // an open section's band joins the box under it: square at the bottom, no air below
    let low = if open { 0 } else { 4 };
    let band = if open { row.expand2(egui::vec2(0.0, 0.0)) } else { row.expand2(egui::vec2(0.0, 1.0)) };
    ui.painter().rect_filled(band, egui::CornerRadius { nw: 4, ne: 4, sw: low, se: low }, egui::Color32::from_gray(gray));
    // the fold arrow turns from pointing right to pointing down as the section opens
    let colour = if open { ui.visuals().strong_text_color() } else { ui.visuals().text_color() };
    let spin = egui::emath::Rot2::from_angle(state.openness(ui.ctx()) * std::f32::consts::FRAC_PI_2);
    let centre = egui::pos2(row.min.x + 4.0 + ui.spacing().icon_width / 2.0, row.center().y);
    let r = 4.5;
    let points = [egui::vec2(-r * 0.5, -r), egui::vec2(r, 0.0), egui::vec2(-r * 0.5, r)].map(|p| centre + spin * p).to_vec();
    ui.painter().add(egui::Shape::convex_polygon(points, colour, egui::Stroke::NONE));
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(title.to_owned(), font, colour);
    let pos = egui::pos2(row.min.x + 4.0 + ui.spacing().icon_width + 6.0, row.center().y - galley.size().y / 2.0);
    // bold while open: draw twice, a pixel apart (the font has no bold face)
    if open {
        ui.painter().galley(pos + egui::vec2(0.6, 0.0), galley.clone(), colour);
    }
    ui.painter().galley(pos, galley, colour);
    let hit = hit.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(hint);
    // the band already shows what belongs to the section: no guide line, and a narrow indent (the panel is only so wide)
    let mut bottom = row.bottom();
    let out = ui
        .scope(|ui| {
            ui.visuals_mut().indent_has_left_vline = false;
            let indent = ui.spacing().indent;
            ui.spacing_mut().indent = 8.0;
            ui.spacing_mut().item_spacing.y = 0.0;
            state
                .show_body_indented(&hit, ui, |ui| {
                    // the contents stand on the panel's own ground, as wide as the band, inside a border drawn round band and contents
                    let framed = egui::Frame::new()
                        .outer_margin(egui::Margin { left: -(ui.spacing().indent as i8), right: 0, top: 0, bottom: 0 })
                        .inner_margin(egui::Margin::symmetric(6, 6))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.spacing_mut().item_spacing.y = crate::theme::ROW_GAP;
                            ui.spacing_mut().indent = indent;
                            body(ui)
                        });
                    bottom = framed.response.rect.bottom();
                    framed.inner
                })
                .map(|r| r.inner)
        })
        .inner;
    if out.is_some() {
        let frame = egui::Rect::from_min_max(row.min, egui::pos2(row.max.x, bottom));
        ui.painter().rect_stroke(frame, 4, egui::Stroke::new(1.0, egui::Color32::from_gray(75)), egui::StrokeKind::Inside);
    }
    if hit.clicked() {
        state.toggle(ui);
    }
    state.store(ui.ctx());
    out
}

/// A bordered box without a heading, square on top, for what hangs from the bar just above it (the settings of the picked tool) and goes
/// away with it: the same border and inner margin as the cards of the folds, no band and no arrow.
pub fn plain_card<R>(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let framed = egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(75)))
        .corner_radius(egui::CornerRadius { nw: 0, ne: 0, sw: 4, se: 4 })
        .inner_margin(egui::Margin::symmetric(6, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = crate::theme::ROW_GAP;
            body(ui)
        });
    framed.inner
}

/// A row of text buttons joined into one bar (equal widths, square where they meet, a faint line between): the buttons of one purpose
/// ("make or reuse a part"). `items` is `(label, enabled)`; the responses come back in the same order, for the caller's hover text and clicks.
pub fn segment_row(ui: &mut egui::Ui, items: &[(&str, bool)]) -> Vec<egui::Response> {
    segment_bar(ui, items, None)
}

/// The same bar for choosing one of several: `selected` is the one in effect (filled with the selection colour, bold). The responses come
/// back in order; a click on one is the choice.
pub fn segment_bar(ui: &mut egui::Ui, items: &[(&str, bool)], selected: Option<usize>) -> Vec<egui::Response> {
    let mut out = Vec::new();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let width = ui.available_width() / items.len() as f32;
        let height = ui.spacing().interact_size.y;
        for (n, (label, enabled)) in items.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), if *enabled { egui::Sense::click() } else { egui::Sense::hover() });
            let widgets = &ui.visuals().widgets;
            let chosen = selected == Some(n);
            let (fill, stroke) = if chosen {
                (ui.visuals().selection.bg_fill, widgets.inactive.bg_stroke)
            } else if *enabled && (resp.hovered() || resp.has_focus()) {
                (widgets.hovered.weak_bg_fill, widgets.hovered.bg_stroke)
            } else {
                (widgets.inactive.weak_bg_fill, widgets.inactive.bg_stroke)
            };
            let first = if n == 0 { 4 } else { 0 };
            let last = if n + 1 == items.len() { 4 } else { 0 };
            let corners = egui::CornerRadius { nw: first, sw: first, ne: last, se: last };
            ui.painter().rect(rect, corners, fill, stroke, egui::StrokeKind::Inside);
            if last == 0 {
                ui.painter().vline(rect.max.x - 0.5, rect.y_range().shrink(4.0), egui::Stroke::new(1.0, widgets.noninteractive.bg_stroke.color));
            }
            let colour = if chosen { ui.visuals().strong_text_color() } else if *enabled { ui.style().interact(&resp).text_color() } else { widgets.noninteractive.text_color().gamma_multiply(0.5) };
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, *label, egui::TextStyle::Button.resolve(ui.style()), colour);
            out.push(resp);
        }
    });
    out
}

/// The contents of an open `band_header(.., flush: true, ..)`: on the panel's own ground, as wide as the band (it steps back out over the
/// indent), with a little air inside; `band_header` draws the border round band and contents.
pub fn well<R>(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .outer_margin(egui::Margin { left: -(ui.spacing().indent as i8), right: 0, top: 0, bottom: 0 })
        .inner_margin(egui::Margin::symmetric(6, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = crate::theme::ROW_GAP;
            body(ui)
        })
        .inner
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

/// The warning lines of an analysis: one line is shown as it is; several are folded under "⚠ 警告 N 件" (closed), so a long list
/// does not push the result down. A line that `calm` accepts is a note, not a warning (blue, not counted).
pub fn warning_lines(ui: &mut egui::Ui, id: &str, lines: &[String], calm: impl Fn(&str) -> bool) {
    let (notes, warns): (Vec<&String>, Vec<&String>) = lines.iter().partition(|w| calm(w));
    for w in notes {
        ui.colored_label(egui::Color32::from_rgb(130, 170, 210), format!("ⓘ {w}"));
    }
    match warns.len() {
        0 => {}
        1 => {
            ui.colored_label(crate::theme::WARN, warns[0]);
        }
        n => {
            egui::CollapsingHeader::new(egui::RichText::new(format!("⚠ 警告 {n} 件")).color(crate::theme::WARN))
                .id_salt(id)
                .default_open(false)
                .show(ui, |ui| {
                    for w in warns {
                        ui.colored_label(crate::theme::WARN, w);
                    }
                })
                .header_response
                .on_hover_text("解析で見つかった注意です。押すと全部を出します");
        }
    }
}

