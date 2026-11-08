//! Toolbar icons drawn with the painter (no icon font, so they look the same on every OS), and the
//! "icon + label" / "icon only" switch shared by the schematic and board toolbars.

use eframe::egui::{self, pos2, vec2, Color32, Pos2, Rect, Stroke};
use std::sync::atomic::{AtomicBool, Ordering};

static ICON_ONLY: AtomicBool = AtomicBool::new(false);

pub fn icon_only() -> bool {
    ICON_ONLY.load(Ordering::Relaxed)
}

pub fn set_icon_only(on: bool) {
    ICON_ONLY.store(on, Ordering::Relaxed);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Select,
    Wire,
    Bus,
    Label,
    Via,
    Zone,
    /// A keep-out area: a dashed box with a "no" sign.
    Keepout,
    OutlineRect,
    OutlinePoly,
    PadTh,
    PadSmd,
    Hole,
    Silk,
    Undo,
    Redo,
    Part,
    Airwire,
    Ruler,
    /// A dashed frame (a group of the requirements).
    Frame,
    /// A distance between two points.
    Gap,
    /// Two sheets of paper, one over the other (the schematic sheets).
    Sheet,
    /// A dimension line: two extension lines and the measured line with arrows between them.
    Dimension,
    /// A finding of the rule check: a ring with a cross.
    DrcMark,
    Layers,
    Eye,
    EyeOff,
    Lock,
    Unlock,
}

/// Draw `icon` inside `r` (a square) with `c`.
pub fn paint(p: &egui::Painter, r: Rect, icon: Icon, c: Color32) {
    let s = Stroke::new(1.6, c);
    // unit square (0..1) -> r
    let q = |x: f32, y: f32| pos2(r.min.x + x * r.width(), r.min.y + y * r.height());
    let u = r.width();
    match icon {
        Icon::Select => {
            let pts = vec![q(0.25, 0.1), q(0.25, 0.85), q(0.43, 0.68), q(0.58, 0.95), q(0.7, 0.89), q(0.55, 0.62), q(0.8, 0.6)];
            p.add(egui::Shape::convex_polygon(pts, c.gamma_multiply(0.35), s));
        }
        Icon::Wire => {
            p.line(vec![q(0.1, 0.85), q(0.1, 0.4), q(0.55, 0.4), q(0.55, 0.15), q(0.9, 0.15)], s);
            p.circle_filled(q(0.1, 0.85), u * 0.09, c);
            p.circle_filled(q(0.9, 0.15), u * 0.09, c);
        }
        Icon::Bus => {
            let thick = Stroke::new(3.4, c);
            p.line(vec![q(0.08, 0.7), q(0.5, 0.7), q(0.5, 0.3), q(0.92, 0.3)], thick);
            for x in [0.2, 0.35] {
                p.line_segment([q(x, 0.7), q(x + 0.1, 0.9)], Stroke::new(1.2, c));
            }
        }
        Icon::Label => {
            let pts = vec![q(0.1, 0.25), q(0.65, 0.25), q(0.92, 0.5), q(0.65, 0.75), q(0.1, 0.75)];
            p.add(egui::Shape::closed_line(pts, s));
            p.circle_filled(q(0.25, 0.5), u * 0.07, c);
        }
        Icon::Via => {
            p.circle_stroke(q(0.5, 0.5), u * 0.38, s);
            p.circle_filled(q(0.5, 0.5), u * 0.14, c);
        }
        Icon::Zone => {
            let b = Rect::from_min_max(q(0.12, 0.12), q(0.88, 0.88));
            p.rect_stroke(b, 1.0, s, egui::StrokeKind::Inside);
            for i in 1..4 {
                let t = i as f32 * 0.25;
                p.line_segment([q(0.12, 0.12 + t * 0.76 + 0.0), q(0.12 + t * 0.76, 0.12)], Stroke::new(1.0, c));
                p.line_segment([q(0.88, 0.12 + t * 0.76), q(0.12 + t * 0.76, 0.88)], Stroke::new(1.0, c));
            }
        }
        Icon::Keepout => {
            // dashed box
            let (x0, y0, x1, y1) = (0.12, 0.12, 0.88, 0.88);
            for k in 0..4 {
                let (a, b) = (x0 + k as f32 * 0.19, x0 + k as f32 * 0.19 + 0.11);
                for (pa, pb) in [(q(a, y0), q(b, y0)), (q(a, y1), q(b, y1)), (q(x0, a), q(x0, b)), (q(x1, a), q(x1, b))] {
                    p.line_segment([pa, pb], Stroke::new(1.2, c));
                }
            }
            let _ = y1;
            // the sign
            p.circle_stroke(q(0.5, 0.5), u * 0.22, Stroke::new(1.4, c));
            p.line_segment([q(0.35, 0.35), q(0.65, 0.65)], Stroke::new(1.4, c));
        }
        Icon::OutlineRect => {
            let b = Rect::from_min_max(q(0.1, 0.2), q(0.9, 0.8));
            p.rect_stroke(b, 0.0, Stroke::new(1.6, c), egui::StrokeKind::Inside);
            for pt in [b.left_top(), b.right_top(), b.left_bottom(), b.right_bottom()] {
                p.circle_filled(pt, u * 0.07, c);
            }
        }
        Icon::OutlinePoly => {
            let pts: Vec<Pos2> = vec![q(0.12, 0.2), q(0.65, 0.12), q(0.9, 0.5), q(0.6, 0.88), q(0.15, 0.7)];
            p.add(egui::Shape::closed_line(pts.clone(), s));
            for pt in pts {
                p.circle_filled(pt, u * 0.07, c);
            }
        }
        Icon::PadTh => {
            p.circle_filled(q(0.5, 0.5), u * 0.38, c.gamma_multiply(0.45));
            p.circle_stroke(q(0.5, 0.5), u * 0.38, s);
            p.circle_filled(q(0.5, 0.5), u * 0.13, Color32::from_gray(30));
        }
        Icon::PadSmd => {
            let b = Rect::from_min_max(q(0.15, 0.3), q(0.85, 0.7));
            p.rect(b, 1.0, c.gamma_multiply(0.45), s, egui::StrokeKind::Inside);
        }
        Icon::Hole => {
            p.circle_stroke(q(0.5, 0.5), u * 0.38, s);
            p.circle_stroke(q(0.5, 0.5), u * 0.2, Stroke::new(1.0, c));
            p.line_segment([q(0.5, 0.2), q(0.5, 0.8)], Stroke::new(1.0, c));
            p.line_segment([q(0.2, 0.5), q(0.8, 0.5)], Stroke::new(1.0, c));
        }
        Icon::Silk => {
            p.line_segment([q(0.1, 0.8), q(0.9, 0.2)], Stroke::new(1.6, c));
            p.line_segment([q(0.1, 0.8), q(0.2, 0.62)], Stroke::new(1.6, c));
        }
        Icon::Part => {
            p.rect_stroke(Rect::from_min_max(q(0.25, 0.2), q(0.75, 0.8)), 1.0, s, egui::StrokeKind::Inside);
            for y in [0.32, 0.68] {
                p.line_segment([q(0.08, y), q(0.25, y)], s);
                p.line_segment([q(0.75, y), q(0.92, y)], s);
            }
        }
        Icon::Airwire => {
            for (a, b) in [(0.1, 0.3), (0.4, 0.6), (0.7, 0.9)] {
                p.line_segment([q(a, 0.8 - a * 0.7), q(b, 0.8 - b * 0.7)], s);
            }
            p.circle_filled(q(0.1, 0.73), u * 0.08, c);
            p.circle_filled(q(0.9, 0.17), u * 0.08, c);
        }
        Icon::Frame => {
            p.rect_stroke(Rect::from_min_max(q(0.12, 0.2), q(0.88, 0.8)), 2.0, s, egui::StrokeKind::Inside);
            p.circle_filled(q(0.35, 0.5), u * 0.08, c);
            p.circle_filled(q(0.65, 0.5), u * 0.08, c);
        }
        Icon::Sheet => {
            p.rect_stroke(Rect::from_min_max(q(0.1, 0.1), q(0.65, 0.65)), 1.0, Stroke::new(1.0, c), egui::StrokeKind::Inside);
            p.rect_stroke(Rect::from_min_max(q(0.35, 0.35), q(0.9, 0.9)), 1.0, s, egui::StrokeKind::Inside);
        }
        Icon::Gap => {
            p.line_segment([q(0.15, 0.5), q(0.85, 0.5)], s);
            for x in [0.15, 0.85] {
                p.line_segment([q(x, 0.3), q(x, 0.7)], s);
            }
            p.line_segment([q(0.15, 0.5), q(0.28, 0.4)], Stroke::new(1.0, c));
            p.line_segment([q(0.15, 0.5), q(0.28, 0.6)], Stroke::new(1.0, c));
            p.line_segment([q(0.85, 0.5), q(0.72, 0.4)], Stroke::new(1.0, c));
            p.line_segment([q(0.85, 0.5), q(0.72, 0.6)], Stroke::new(1.0, c));
        }
        Icon::Dimension => {
            for x in [0.15, 0.85] {
                p.line_segment([q(x, 0.2), q(x, 0.85)], Stroke::new(1.0, c));
            }
            p.line_segment([q(0.15, 0.35), q(0.85, 0.35)], s);
            for (x, d) in [(0.15, 0.13), (0.85, -0.13)] {
                p.line_segment([q(x, 0.35), q(x + d, 0.25)], Stroke::new(1.0, c));
                p.line_segment([q(x, 0.35), q(x + d, 0.45)], Stroke::new(1.0, c));
            }
            p.line_segment([q(0.3, 0.7), q(0.7, 0.7)], Stroke::new(1.0, c));
        }
        Icon::DrcMark => {
            p.circle_stroke(q(0.5, 0.5), r.width() * 0.36, s);
            p.line_segment([q(0.33, 0.33), q(0.67, 0.67)], s);
            p.line_segment([q(0.33, 0.67), q(0.67, 0.33)], s);
        }
        Icon::Ruler => {
            p.rect_stroke(Rect::from_min_max(q(0.08, 0.35), q(0.92, 0.65)), 1.0, s, egui::StrokeKind::Inside);
            for i in 0..5 {
                let x = 0.2 + i as f32 * 0.15;
                p.line_segment([q(x, 0.35), q(x, if i % 2 == 0 { 0.55 } else { 0.48 })], Stroke::new(1.0, c));
            }
        }
        Icon::Layers => {
            for dy in [0.0, 0.2, 0.4] {
                let pts = vec![q(0.5, 0.15 + dy), q(0.9, 0.3 + dy), q(0.5, 0.45 + dy), q(0.1, 0.3 + dy)];
                p.add(egui::Shape::closed_line(pts, if dy == 0.0 { s } else { Stroke::new(1.0, c) }));
            }
        }
        Icon::Eye => {
            let top: Vec<Pos2> = (0..=8).map(|i| { let t = i as f32 / 8.0; q(0.05 + 0.9 * t, 0.5 - 0.32 * (std::f32::consts::PI * t).sin()) }).collect();
            let bot: Vec<Pos2> = (0..=8).map(|i| { let t = i as f32 / 8.0; q(0.05 + 0.9 * t, 0.5 + 0.32 * (std::f32::consts::PI * t).sin()) }).collect();
            p.line(top, s);
            p.line(bot, s);
            p.circle_filled(q(0.5, 0.5), u * 0.14, c);
        }
        Icon::EyeOff => {
            paint(p, r, Icon::Eye, c);
            p.line_segment([q(0.12, 0.88), q(0.88, 0.12)], Stroke::new(2.0, c));
        }
        Icon::Lock | Icon::Unlock => {
            p.rect(Rect::from_min_max(q(0.2, 0.45), q(0.8, 0.9)), 1.5, c.gamma_multiply(0.45), s, egui::StrokeKind::Inside);
            // the shackle: closed, or swung open on one side
            let arc: Vec<Pos2> = (0..=8).map(|i| { let a = std::f32::consts::PI * i as f32 / 8.0; q(0.5 - 0.2 * a.cos(), 0.45 - 0.22 * a.sin()) }).collect();
            if icon == Icon::Lock {
                p.line(arc, s);
            } else {
                p.line(arc[..=5].to_vec(), s);
                p.line_segment([arc[5], q(0.3, 0.3)], s);
            }
        }
        Icon::Undo | Icon::Redo => {
            let f = if icon == Icon::Undo { 1.0 } else { -1.0 };
            let x = |v: f32| if f > 0.0 { v } else { 1.0 - v };
            p.line(vec![q(x(0.3), 0.3), q(x(0.65), 0.3), q(x(0.85), 0.5), q(x(0.65), 0.8), q(x(0.35), 0.8)], s);
            p.add(egui::Shape::convex_polygon(vec![q(x(0.12), 0.3), q(x(0.38), 0.12), q(x(0.38), 0.48)], c, Stroke::NONE));
        }
    }
}

/// A toolbar button. With text: icon + label. Icon only: a square button whose tooltip starts with the label. It is drawn with its own
/// fill and edge; `selected` fills it with the accent colour; `enabled` greys it out. It also shows its keyboard shortcut (`key`, e.g. "Ctrl+Z"; empty for none): dim text after the
/// label, and a line in the tooltip (also when only the icon is shown).
pub fn icon_button_key(ui: &mut egui::Ui, icon: Icon, label: &str, key: &str, selected: bool, enabled: bool, tip: &str) -> egui::Response {
    icon_button_shaped(ui, icon, label, key, selected, enabled, tip, None)
}

/// A button in a joined row (one tool of a segmented bar): `shape` is the fixed width and the corner radii (square where it touches
/// its neighbour); the icon and label are centred in it.
pub fn segment_button(ui: &mut egui::Ui, icon: Icon, label: &str, key: &str, selected: bool, enabled: bool, tip: &str, width: f32, corners: egui::CornerRadius) -> egui::Response {
    icon_button_shaped(ui, icon, label, key, selected, enabled, tip, Some((width, corners)))
}

#[allow(clippy::too_many_arguments)]
fn icon_button_shaped(ui: &mut egui::Ui, icon: Icon, label: &str, key: &str, selected: bool, enabled: bool, tip: &str, shape: Option<(f32, egui::CornerRadius)>) -> egui::Response {
    let side = ui.spacing().interact_size.y.max(22.0);
    let pad = ui.spacing().button_padding;
    let (size, galley) = if icon_only() {
        (vec2(side, side), None)
    } else {
        let g = ui.painter().layout_no_wrap(label.to_owned(), egui::TextStyle::Button.resolve(ui.style()), Color32::PLACEHOLDER);
        let kw = if key.is_empty() { 0.0 } else { ui.painter().layout_no_wrap(key.to_owned(), egui::TextStyle::Button.resolve(ui.style()), Color32::PLACEHOLDER).size().x + 10.0 };
        (vec2(side + 2.0 * pad.x + g.size().x + kw, side), Some(g))
    };
    let size = match shape {
        Some((w, _)) => vec2(w, size.y),
        None => size,
    };
    // content is centred when the width is fixed
    let dx = shape.map_or(0.0, |_| {
        let kw = if key.is_empty() { 0.0 } else { ui.painter().layout_no_wrap(key.to_owned(), egui::TextStyle::Button.resolve(ui.style()), Color32::PLACEHOLDER).size().x + 10.0 };
        let natural = if galley.is_some() { size.x.min(side + 2.0 * pad.x + galley.as_ref().map_or(0.0, |g| g.size().x) + kw) } else { side };
        ((size.x - natural) / 2.0).max(0.0)
    });
    let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    if ui.is_rect_visible(rect) {
        let vis = ui.style().interact_selectable(&resp, selected);
        // always drawn as a button (a resting fill and edge), so that it is not taken for a label; the chosen one is filled with the accent
        let widgets = &ui.visuals().widgets;
        let (fill, stroke) = if selected {
            (ui.visuals().selection.bg_fill, Stroke::new(1.0, crate::theme::INFO))
        } else if resp.hovered() || resp.has_focus() {
            (vis.weak_bg_fill, vis.bg_stroke)
        } else {
            (widgets.inactive.weak_bg_fill, widgets.inactive.bg_stroke)
        };
        ui.painter().rect(rect, shape.map_or(vis.corner_radius, |(_, c)| c), fill, stroke, egui::StrokeKind::Inside);
        // a faint line between two joined buttons (the left one of a pair carries it on its right edge)
        if shape.is_some_and(|(_, c)| c.ne == 0) && !selected {
            ui.painter().vline(rect.max.x - 0.5, rect.y_range().shrink(4.0), Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color));
        }
        let col = if enabled { vis.text_color() } else { ui.visuals().widgets.noninteractive.text_color().gamma_multiply(0.5) };
        let ir = Rect::from_min_size(rect.min + vec2(dx + if galley.is_some() { pad.x } else { 3.0 }, 3.0), vec2(side - 6.0, side - 6.0));
        paint(ui.painter(), ir, icon, col);
        if let Some(g) = galley {
            let pos = pos2(rect.min.x + dx + pad.x + side - 2.0, rect.center().y - g.size().y / 2.0);
            let label_end = pos.x + g.size().x;
            ui.painter().galley(pos, g, col);
            if !key.is_empty() {
                // the shortcut sits at the right end, dimmer than the label
                let dim = col.gamma_multiply(0.55);
                let kg = ui.painter().layout_no_wrap(key.to_owned(), egui::TextStyle::Button.resolve(ui.style()), dim);
                // in a fixed-width (centred) button the shortcut follows the label; otherwise it sits at the right end
                let x = if shape.is_some() { label_end + 10.0 } else { rect.max.x - pad.x - kg.size().x };
                ui.painter().galley(pos2(x, rect.center().y - kg.size().y / 2.0), kg, dim);
            }
        }
    }
    let tip = if icon_only() { format!("{label}\n{tip}") } else { tip.to_owned() };
    let tip = if key.is_empty() { tip } else { format!("{tip}\nショートカット: {key}") };
    resp.on_hover_text(tip)
}

/// An on/off icon whose picture changes with the state (eye / crossed eye, closed / open lock). `on` is lit.
pub fn icon_switch(ui: &mut egui::Ui, on_icon: Icon, off_icon: Icon, on: &mut bool, tip: &str) -> egui::Response {
    let (rect, mut resp) = ui.allocate_exact_size(vec2(24.0, 24.0), egui::Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let dark = Color32::from_rgb(25, 35, 45);
        if *on {
            ui.painter().rect(rect, 4.0, Color32::from_rgba_unmultiplied(70, 130, 200, 90), Stroke::new(1.0, Color32::from_rgb(70, 130, 200)), egui::StrokeKind::Inside);
        } else if resp.hovered() {
            ui.painter().rect_filled(rect, 4.0, Color32::from_black_alpha(25));
        }
        paint(ui.painter(), rect.shrink(4.0), if *on { on_icon } else { off_icon }, if *on { dark } else { dark.gamma_multiply(0.45) });
    }
    resp.on_hover_text(tip)
}

/// A fold header for the glass panels: a triangle and the title; a click anywhere on it folds or unfolds.
/// The state lives in egui's memory under `id`. Returns whether the body is open.
pub fn fold_header(ui: &mut egui::Ui, id: egui::Id, title: &str, default_open: bool, tip: &str) -> bool {
    let mut open = ui.ctx().data_mut(|d| *d.get_persisted_mut_or_insert_with(id, || default_open));
    let col = Color32::from_rgb(25, 35, 45);
    let g = ui.painter().layout_no_wrap(title.to_owned(), egui::TextStyle::Body.resolve(ui.style()), col);
    // as wide as the body was the last time it was open, so the empty space beside the title clicks too
    let wide = if open { ui.ctx().data_mut(|d| d.get_temp::<f32>(id.with("w"))).unwrap_or(0.0) } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2((g.size().x + 26.0).max(wide), g.size().y.max(18.0) + 2.0), egui::Sense::click());
    if resp.clicked() {
        open = !open;
        ui.ctx().data_mut(|d| d.insert_persisted(id, open));
    }
    if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, Color32::from_black_alpha(20));
    }
    let c = pos2(rect.min.x + 10.0, rect.center().y);
    let tri = if open { vec![c + vec2(-4.0, -2.5), c + vec2(4.0, -2.5), c + vec2(0.0, 3.5)] } else { vec![c + vec2(-2.5, -4.0), c + vec2(-2.5, 4.0), c + vec2(3.5, 0.0)] };
    ui.painter().add(egui::Shape::convex_polygon(tri, col, Stroke::NONE));
    ui.painter().galley(pos2(rect.min.x + 20.0, rect.center().y - g.size().y / 2.0), g, col);
    resp.on_hover_text(tip);
    open
}

/// Call with the width of the body drawn under a `fold_header` so that its header can span it next time.
pub fn fold_remember_width(ui: &egui::Ui, id: egui::Id, width: f32) {
    ui.ctx().data_mut(|d| d.insert_temp(id.with("w"), width));
}

/// A row label drawn as an icon (with a tooltip), for the glass panels.
pub fn icon_label(ui: &mut egui::Ui, icon: Icon, tip: &str) {
    let (rect, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), egui::Sense::hover());
    paint(ui.painter(), rect.shrink(3.0), icon, Color32::from_rgb(25, 35, 45));
    resp.on_hover_text(tip);
}

/// A small "(?)" mark whose hover text carries an explanation that would otherwise take a line of the panel.
pub fn help(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(egui::RichText::new("(?)").weak()).sense(egui::Sense::hover()))
        .on_hover_cursor(egui::CursorIcon::Help)
        .on_hover_text(text);
}

/// A short caption (small text) followed by a "(?)" that holds the long explanation.
pub fn help_small(ui: &mut egui::Ui, short: &str, long: &str) {
    ui.horizontal(|ui| {
        ui.small(short);
        help(ui, long);
    });
}

/// The same with normal-size text (for a heading above an input box).
pub fn help_label(ui: &mut egui::Ui, short: &str, long: &str) {
    ui.horizontal(|ui| {
        ui.label(short);
        help(ui, long);
    });
}
