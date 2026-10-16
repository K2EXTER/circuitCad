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
    Label,
    Via,
    Zone,
    OutlineRect,
    OutlinePoly,
    PadTh,
    PadSmd,
    Hole,
    Silk,
    Undo,
    Redo,
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
        Icon::Undo | Icon::Redo => {
            let f = if icon == Icon::Undo { 1.0 } else { -1.0 };
            let x = |v: f32| if f > 0.0 { v } else { 1.0 - v };
            p.line(vec![q(x(0.3), 0.3), q(x(0.65), 0.3), q(x(0.85), 0.5), q(x(0.65), 0.8), q(x(0.35), 0.8)], s);
            p.add(egui::Shape::convex_polygon(vec![q(x(0.12), 0.3), q(x(0.38), 0.12), q(x(0.38), 0.48)], c, Stroke::NONE));
        }
    }
}

/// A toolbar button. With text: icon + label. Icon only: a square button whose tooltip starts with the label.
/// `selected` highlights it like a selectable label; `enabled` greys it out.
pub fn icon_button(ui: &mut egui::Ui, icon: Icon, label: &str, selected: bool, enabled: bool, tip: &str) -> egui::Response {
    let side = ui.spacing().interact_size.y.max(22.0);
    let pad = ui.spacing().button_padding;
    let (size, galley) = if icon_only() {
        (vec2(side, side), None)
    } else {
        let g = ui.painter().layout_no_wrap(label.to_owned(), egui::TextStyle::Button.resolve(ui.style()), Color32::PLACEHOLDER);
        (vec2(side + 2.0 * pad.x + g.size().x, side), Some(g))
    };
    let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    if ui.is_rect_visible(rect) {
        let vis = ui.style().interact_selectable(&resp, selected);
        if selected || resp.hovered() || resp.has_focus() {
            ui.painter().rect(rect, vis.corner_radius, vis.weak_bg_fill, vis.bg_stroke, egui::StrokeKind::Inside);
        }
        let col = if enabled { vis.text_color() } else { ui.visuals().widgets.noninteractive.text_color().gamma_multiply(0.5) };
        let ir = Rect::from_min_size(rect.min + vec2(if galley.is_some() { pad.x } else { 3.0 }, 3.0), vec2(side - 6.0, side - 6.0));
        paint(ui.painter(), ir, icon, col);
        if let Some(g) = galley {
            let pos = pos2(rect.min.x + pad.x + side - 2.0, rect.center().y - g.size().y / 2.0);
            ui.painter().galley(pos, g, col);
        }
    }
    let tip = if icon_only() { format!("{label}\n{tip}") } else { tip.to_owned() };
    resp.on_hover_text(tip)
}

/// A small "(?)" mark whose hover text carries an explanation that would otherwise take a line of the panel.
pub fn help(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(egui::RichText::new("(?)").weak()).sense(egui::Sense::hover()))
        .on_hover_cursor(egui::CursorIcon::Help)
        .on_hover_text(text);
}
