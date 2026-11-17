//! A plot of curves in the plane (x and y both quantities, not a waveform over time): the Nyquist and Nichols charts of a loop
//! gain and the root locus. Drag = pan, wheel = zoom about the pointer, double click = fit; the pointer reads the nearest point
//! of a curve with its tag (the frequency, the part's value).

use super::{axis_label, nice_ticks, plain_label};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke};

#[derive(Clone, Debug)]
pub struct XyView {
    pub x: (f64, f64),
    pub y: (f64, f64),
    /// Fit the range to the data every frame (until the user pans or zooms).
    pub auto: bool,
    /// One unit is as long on x as on y (the complex plane: a circle stays a circle).
    pub equal: bool,
}

impl XyView {
    pub fn new(equal: bool) -> Self {
        Self { x: (-1.0, 1.0), y: (-1.0, 1.0), auto: true, equal }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Line {
    Solid,
    Dashed,
    /// A line with a dot at every point.
    WithDots,
}

pub struct XyCurve<'a> {
    pub name: &'a str,
    pub color: Color32,
    pub pts: &'a [(f64, f64)],
    /// One value per point for the readout (frequency, part value); empty: the curve is not read.
    pub tags: &'a [f64],
    pub line: Line,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Cross,
    Ring,
    Dot,
    Square,
}

pub struct XyMark {
    pub at: (f64, f64),
    pub color: Color32,
    pub shape: Shape,
    pub text: String,
    /// The text next to the mark; otherwise it goes in the legend box in the corner (marks close together stay readable).
    pub inline: bool,
}

pub struct XyStyle<'a> {
    pub x_name: &'a str,
    pub y_name: &'a str,
    /// Units after the tick labels ("°", " dB", "Hz"); empty for a bare number.
    pub x_unit: &'a str,
    pub y_unit: &'a str,
    /// No SI prefixes on the ticks (dB, degrees, the plain numbers of a Nyquist chart).
    pub plain: bool,
    /// Name and unit of the tags in the readout ("f", "Hz").
    pub tag_name: &'a str,
    pub tag_unit: &'a str,
    /// Tint the half x > 0 (the right half of the s plane, where a pole makes the circuit oscillate).
    pub shade_right: bool,
}

fn label(v: f64, step: f64, plain: bool, unit: &str) -> String {
    format!("{}{unit}", if plain { plain_label(v, step) } else { axis_label(v, step) })
}

/// The range that holds `fit` (or every point and mark), with a margin, and with equal units when the view asks for them.
fn fitted(view: &XyView, pa: Rect, curves: &[XyCurve], marks: &[XyMark], fit: Option<((f64, f64), (f64, f64))>) -> ((f64, f64), (f64, f64)) {
    let (mut x, mut y) = fit.unwrap_or_else(|| {
        let (mut x, mut y) = ((f64::MAX, f64::MIN), (f64::MAX, f64::MIN));
        for p in curves.iter().flat_map(|c| c.pts.iter().copied()).chain(marks.iter().map(|m| m.at)).filter(|p| p.0.is_finite() && p.1.is_finite()) {
            x = (x.0.min(p.0), x.1.max(p.0));
            y = (y.0.min(p.1), y.1.max(p.1));
        }
        (x, y)
    });
    if x.0 > x.1 {
        x = (-1.0, 1.0);
    }
    if y.0 > y.1 {
        y = (-1.0, 1.0);
    }
    let pad = |r: (f64, f64)| {
        let w = r.1 - r.0;
        let p = if w < 1e-300 { r.0.abs().max(1.0) * 0.5 } else { w * 0.08 };
        (r.0 - p, r.1 + p)
    };
    let (mut x, mut y) = (pad(x), pad(y));
    if view.equal {
        // units per pixel: the larger one wins, the other range grows about its centre
        let (ux, uy) = ((x.1 - x.0) / pa.width() as f64, (y.1 - y.0) / pa.height() as f64);
        let u = ux.max(uy);
        let grow = |r: (f64, f64), px: f32| {
            let c = (r.0 + r.1) / 2.0;
            let h = u * px as f64 / 2.0;
            (c - h, c + h)
        };
        x = grow(x, pa.width());
        y = grow(y, pa.height());
    }
    (x, y)
}

/// Draws the curves and marks in `rect`. `fit` is the box the automatic range shows (None: everything).
#[allow(clippy::too_many_arguments)]
pub fn draw_xy(ui: &mut egui::Ui, rect: Rect, id: egui::Id, view: &mut XyView, curves: &[XyCurve], marks: &[XyMark], fit: Option<((f64, f64), (f64, f64))>, style: &XyStyle) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::from_rgb(18, 22, 18));
    let pa = Rect::from_min_max(Pos2::new(rect.left() + 64.0, rect.top() + 20.0), Pos2::new(rect.right() - 16.0, rect.bottom() - 36.0));
    if pa.width() < 20.0 || pa.height() < 20.0 {
        return;
    }
    let resp = ui.interact(pa, id, Sense::click_and_drag());
    if view.auto {
        (view.x, view.y) = fitted(view, pa, curves, marks, fit);
    }
    let (w, h) = (pa.width() as f64, pa.height() as f64);
    if resp.dragged() {
        let d = resp.drag_delta();
        let dx = -(d.x as f64) / w * (view.x.1 - view.x.0);
        let dy = d.y as f64 / h * (view.y.1 - view.y.0);
        view.x = (view.x.0 + dx, view.x.1 + dx);
        view.y = (view.y.0 + dy, view.y.1 + dy);
        view.auto = false;
    }
    if resp.hovered() {
        let scroll = crate::prefs::zoom_scroll(ui);
        if scroll != 0.0 {
            let f = (1.0 - scroll as f64 * 0.002).clamp(0.5, 2.0);
            let p = ui.input(|i| i.pointer.hover_pos()).unwrap_or(pa.center());
            let cx = view.x.0 + (p.x - pa.left()) as f64 / w * (view.x.1 - view.x.0);
            let cy = view.y.1 - (p.y - pa.top()) as f64 / h * (view.y.1 - view.y.0);
            view.x = (cx + (view.x.0 - cx) * f, cx + (view.x.1 - cx) * f);
            view.y = (cy + (view.y.0 - cy) * f, cy + (view.y.1 - cy) * f);
            view.auto = false;
        }
    }
    if resp.double_clicked() {
        view.auto = true;
        (view.x, view.y) = fitted(view, pa, curves, marks, fit);
    }
    let ((x0, x1), (y0, y1)) = (view.x, view.y);
    let sx = |x: f64| pa.left() + ((x - x0) / (x1 - x0)) as f32 * pa.width();
    let sy = |y: f64| pa.bottom() - ((y - y0) / (y1 - y0)) as f32 * pa.height();
    let to = |p: (f64, f64)| Pos2::new(sx(p.0), sy(p.1));

    if style.shade_right && x1 > 0.0 {
        let left = sx(0.0).max(pa.left());
        painter.rect_filled(Rect::from_min_max(Pos2::new(left, pa.top()), pa.max), 0.0, Color32::from_rgba_unmultiplied(120, 30, 30, 60));
    }
    // grid, the axes through 0 a little brighter, tick labels, axis names
    let grid = Stroke::new(1.0, Color32::from_gray(45));
    let zero = Stroke::new(1.0, Color32::from_gray(95));
    let font = FontId::proportional(11.0);
    let col = Color32::from_gray(170);
    let (xt, xs) = nice_ticks(x0, x1, (pa.width() / 80.0).max(2.0) as usize);
    for t in &xt {
        painter.line_segment([Pos2::new(sx(*t), pa.top()), Pos2::new(sx(*t), pa.bottom())], if *t == 0.0 { zero } else { grid });
        painter.text(Pos2::new(sx(*t), pa.bottom() + 3.0), Align2::CENTER_TOP, label(*t, xs, style.plain, style.x_unit), font.clone(), col);
    }
    let (yt, ys) = nice_ticks(y0, y1, (pa.height() / 36.0).max(2.0) as usize);
    for t in &yt {
        painter.line_segment([Pos2::new(pa.left(), sy(*t)), Pos2::new(pa.right(), sy(*t))], if *t == 0.0 { zero } else { grid });
        painter.text(Pos2::new(pa.left() - 5.0, sy(*t)), Align2::RIGHT_CENTER, label(*t, ys, style.plain, style.y_unit), font.clone(), col);
    }
    painter.text(Pos2::new(pa.center().x, rect.bottom() - 2.0), Align2::CENTER_BOTTOM, i18n::tr_dyn(style.x_name), font.clone(), col);
    painter.text(Pos2::new(rect.left() + 4.0, rect.top() + 3.0), Align2::LEFT_TOP, i18n::tr_dyn(style.y_name), font.clone(), col);
    painter.rect_stroke(pa, 0.0, Stroke::new(1.0, Color32::from_gray(80)), egui::StrokeKind::Inside);

    let clip = painter.with_clip_rect(pa);
    for c in curves {
        let pts: Vec<Pos2> = c.pts.iter().filter(|p| p.0.is_finite() && p.1.is_finite()).map(|p| to(*p)).collect();
        match c.line {
            Line::Solid => {
                clip.add(egui::Shape::line(pts, Stroke::new(1.6, c.color)));
            }
            Line::Dashed => {
                clip.add(egui::Shape::dashed_line(&pts, Stroke::new(1.0, c.color), 5.0, 4.0));
            }
            Line::WithDots => {
                clip.add(egui::Shape::line(pts.clone(), Stroke::new(1.4, c.color)));
                for p in pts {
                    clip.circle_filled(p, 2.5, c.color);
                }
            }
        }
    }
    let glyph = |painter: &egui::Painter, p: Pos2, m: &XyMark| {
        let s = Stroke::new(2.0, m.color);
        match m.shape {
            Shape::Cross => {
                painter.line_segment([p + egui::vec2(-6.0, -6.0), p + egui::vec2(6.0, 6.0)], s);
                painter.line_segment([p + egui::vec2(-6.0, 6.0), p + egui::vec2(6.0, -6.0)], s);
            }
            Shape::Ring => {
                painter.circle_stroke(p, 5.5, s);
            }
            Shape::Dot => {
                painter.circle_filled(p, 4.5, m.color);
            }
            Shape::Square => {
                painter.rect_stroke(Rect::from_center_size(p, egui::vec2(10.0, 10.0)), 0.0, s, egui::StrokeKind::Middle);
            }
        }
    };
    for m in marks {
        let p = to(m.at);
        glyph(&clip, p, m);
        if m.inline && !m.text.is_empty() {
            clip.text(p + egui::vec2(8.0, -8.0), Align2::LEFT_BOTTOM, i18n::tr_dyn(&m.text), font.clone(), m.color);
        }
    }
    // the other marks' texts in a box in the lower right corner, each after its own symbol
    let listed: Vec<&XyMark> = marks.iter().filter(|m| !m.inline && !m.text.is_empty()).collect();
    if !listed.is_empty() {
        let galleys: Vec<_> = listed.iter().map(|m| clip.layout_no_wrap(i18n::tr_dyn(&m.text).to_string(), font.clone(), m.color)).collect();
        let w = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max) + 30.0;
        let h = listed.len() as f32 * 16.0 + 6.0;
        let r = Rect::from_min_size(Pos2::new(pa.right() - w - 6.0, pa.bottom() - h - 6.0), egui::vec2(w, h));
        clip.rect_filled(r, 3.0, Color32::from_rgba_unmultiplied(10, 12, 10, 225));
        for (k, (m, g)) in listed.iter().zip(galleys).enumerate() {
            let y = r.top() + 3.0 + k as f32 * 16.0;
            glyph(&clip, Pos2::new(r.left() + 12.0, y + 8.0), m);
            clip.galley(Pos2::new(r.left() + 24.0, y), g, m.color);
        }
    }

    // readout: the nearest point of a curve that has tags, else the pointer itself
    if let Some(hp) = resp.hover_pos() {
        let mut best: Option<(f32, usize, usize)> = None;
        for (ci, c) in curves.iter().enumerate().filter(|(_, c)| c.tags.len() == c.pts.len() && !c.tags.is_empty()) {
            for (i, p) in c.pts.iter().enumerate() {
                let d = to(*p).distance(hp);
                if d < 14.0 && best.is_none_or(|b| d < b.0) {
                    best = Some((d, ci, i));
                }
            }
        }
        let text = match best {
            Some((_, ci, i)) => {
                let c = &curves[ci];
                let p = to(c.pts[i]);
                clip.circle_stroke(p, 6.0, Stroke::new(1.5, Color32::WHITE));
                let tag = c.tags[i];
                format!(
                    "{}  {} = {}{}  ({}, {})",
                    i18n::tr_dyn(c.name),
                    style.tag_name,
                    axis_label(tag, tag.abs() / 1000.0),
                    style.tag_unit,
                    label(c.pts[i].0, (x1 - x0) / 1000.0, style.plain, style.x_unit),
                    label(c.pts[i].1, (y1 - y0) / 1000.0, style.plain, style.y_unit)
                )
            }
            None => {
                let (x, y) = (x0 + (hp.x - pa.left()) as f64 / w * (x1 - x0), y1 - (hp.y - pa.top()) as f64 / h * (y1 - y0));
                format!("({}, {})", label(x, (x1 - x0) / 1000.0, style.plain, style.x_unit), label(y, (y1 - y0) / 1000.0, style.plain, style.y_unit))
            }
        };
        let g = clip.layout_no_wrap(text, font, Color32::from_gray(225));
        let r = Rect::from_min_size(Pos2::new(pa.left() + 6.0, pa.top() + 4.0), g.size()).expand(3.0);
        clip.rect_filled(r, 3.0, Color32::from_rgba_unmultiplied(10, 12, 10, 215));
        clip.galley(Pos2::new(pa.left() + 6.0, pa.top() + 4.0), g, Color32::from_gray(225));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_equal_view_keeps_a_circle_round_and_holds_the_box() {
        let pa = Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 200.0));
        let v = XyView::new(true);
        let ((x0, x1), (y0, y1)) = fitted(&v, pa, &[], &[], Some(((-1.0, 1.0), (-1.0, 1.0))));
        assert!(((x1 - x0) / 400.0 - (y1 - y0) / 200.0).abs() < 1e-12);
        assert!(x0 < -1.0 && x1 > 1.0 && y0 < -1.0 && y1 > 1.0);
        // not equal: each axis only gets its margin
        let ((a, b), _) = fitted(&XyView::new(false), pa, &[], &[], Some(((0.0, 10.0), (0.0, 1.0))));
        assert!((a + 0.8).abs() < 1e-9 && (b - 10.8).abs() < 1e-9);
    }

    #[test]
    fn the_fit_takes_every_point_and_mark_and_skips_infinities() {
        let pa = Rect::from_min_size(Pos2::ZERO, egui::vec2(300.0, 300.0));
        let pts = [(0.0, 0.0), (2.0, 1.0), (f64::INFINITY, 0.0)];
        let c = [XyCurve { name: "T", color: Color32::WHITE, pts: &pts, tags: &[], line: Line::Solid }];
        let m = [XyMark { at: (-1.0, 0.0), color: Color32::RED, shape: Shape::Cross, text: String::new(), inline: true }];
        let ((x0, x1), _) = fitted(&XyView::new(false), pa, &c, &m, None);
        assert!(x0 < -1.0 && x1 > 2.0 && x1 < 3.0, "{x0} {x1}");
    }
}
