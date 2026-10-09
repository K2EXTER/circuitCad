//! Drawing the schematic symbols and the screen/grid transforms.

use super::*;

// ------------------------------------------------------------- symbol drawing

pub(super) enum Sh {
    L(f32, f32, f32, f32),
    C(f32, f32, f32),
}

pub(super) fn symbol(kind: Kind) -> Vec<Sh> {
    use Sh::*;
    match kind {
        Kind::Resistor => vec![
            L(-2., 0., -1., 0.),
            L(1., 0., 2., 0.),
            L(-1., -0.4, 1., -0.4),
            L(-1., 0.4, 1., 0.4),
            L(-1., -0.4, -1., 0.4),
            L(1., -0.4, 1., 0.4),
        ],
        Kind::Capacitor => vec![
            L(-2., 0., -0.25, 0.),
            L(0.25, 0., 2., 0.),
            L(-0.25, -0.8, -0.25, 0.8),
            L(0.25, -0.8, 0.25, 0.8),
        ],
        Kind::Inductor => {
            let mut v = vec![L(-2., 0., -1.2, 0.), L(1.2, 0., 2., 0.)];
            let n = 48;
            let pt = |i: i32| {
                let x = -1.2 + 2.4 * i as f32 / n as f32;
                (x, -0.5 * (std::f32::consts::PI * (x + 1.2) / 0.6).sin().abs())
            };
            for i in 0..n {
                let (a, b) = (pt(i), pt(i + 1));
                v.push(L(a.0, a.1, b.0, b.1));
            }
            v
        }
        Kind::Diode | Kind::Led => {
            let mut v = vec![
                L(-2., 0., -0.6, 0.),
                L(0.6, 0., 2., 0.),
                L(-0.6, -0.6, -0.6, 0.6),
                L(-0.6, -0.6, 0.6, 0.),
                L(-0.6, 0.6, 0.6, 0.),
                L(0.6, -0.6, 0.6, 0.6),
            ];
            if kind == Kind::Led {
                v.extend([L(0., -0.9, 0.6, -1.5), L(0.6, -0.9, 1.2, -1.5)]);
            }
            v
        }
        Kind::VSource => vec![
            L(0., -2., 0., -1.2),
            L(0., 1.2, 0., 2.),
            C(0., 0., 1.2),
            L(-0.25, -0.6, 0.25, -0.6),
            L(0., -0.85, 0., -0.35),
            L(-0.25, 0.6, 0.25, 0.6),
        ],
        Kind::Npn => vec![
            C(0.5, 0., 1.7),
            L(-2., 0., -0.5, 0.),
            L(-0.5, -1., -0.5, 1.),
            L(-0.5, -0.4, 2., -2.),
            L(-0.5, 0.4, 2., 2.),
            L(1.2, 1.5, 2., 2.),
            L(1.5, 1.1, 2., 2.),
        ],
        Kind::Header(n) => {
            let mut v = vec![
                L(0., -1., 1.2, -1.),
                L(0., -1., 0., 2. * n as f32 - 1.),
                L(1.2, -1., 1.2, 2. * n as f32 - 1.),
                L(0., 2. * n as f32 - 1., 1.2, 2. * n as f32 - 1.),
            ];
            for i in 0..n as i32 {
                v.push(L(1.2, 2. * i as f32, 2., 2. * i as f32));
                v.push(C(0.6, 2. * i as f32, 0.25));
            }
            v
        }
        Kind::Ground => vec![
            L(0., 0., 0., 0.5),
            L(-0.8, 0.5, 0.8, 0.5),
            L(-0.5, 0.8, 0.5, 0.8),
            L(-0.2, 1.1, 0.2, 1.1),
        ],
        Kind::Power => vec![L(0., 0., 0., -0.6), L(-0.6, -0.6, 0.6, -0.6)],
        Kind::Custom => vec![],
    }
}

/// Local bounding box (xmin, xmax, ymin, ymax).
pub(super) fn extents(kind: Kind) -> (f32, f32, f32, f32) {
    match kind {
        Kind::VSource => (-1.4, 1.4, -2., 2.),
        Kind::Npn => (-2., 2., -2., 2.),
        Kind::Header(n) => (0., 2., -1., 2. * n as f32 - 1.),
        Kind::Custom => (-2., 2., -2., 2.),
        Kind::Ground => (-1., 1., 0., 1.2),
        Kind::Power => (-1., 1., -1., 0.),
        _ => (-2., 2., -1., 1.),
    }
}

pub(super) fn sym_extents(sym: &CustomSymbol) -> (f32, f32, f32, f32) {
    let l = sym.layout();
    let (w, h) = ((l.hw + 2) as f32, (l.hh + 2) as f32);
    (-w, w, -h, h)
}

pub(super) fn comp_extents(c: &Component) -> (f32, f32, f32, f32) {
    match &c.custom {
        Some(sym) => sym_extents(sym),
        None => extents(c.kind),
    }
}

pub(super) const OVERLAP_MSG: &str = "部品が重なるので、そこには置けません";

/// The area a part occupies on the sheet (xmin, xmax, ymin, ymax) after rotation. Ground and
/// power symbols have none: they are meant to sit on a pin or wire next to a part.
pub(super) fn sheet_box(kind: Kind, ext: (f32, f32, f32, f32), pos: Pt, rot: u8) -> Option<(f32, f32, f32, f32)> {
    if kind.is_power_symbol() {
        return None;
    }
    let mut b = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for corner in [(ext.0, ext.2), (ext.1, ext.2), (ext.0, ext.3), (ext.1, ext.3)] {
        let (x, y) = rot_f(corner, rot);
        b = (b.0.min(x), b.1.max(x), b.2.min(y), b.3.max(y));
    }
    Some((b.0 + pos.0 as f32, b.1 + pos.0 as f32, b.2 + pos.1 as f32, b.3 + pos.1 as f32))
}

/// Parts that merely touch (a pin on a neighbour's edge) do not overlap.
pub(super) fn boxes_overlap(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> bool {
    const EPS: f32 = 1e-3;
    a.0 < b.1 - EPS && b.0 < a.1 - EPS && a.2 < b.3 - EPS && b.2 < a.3 - EPS
}

pub(super) fn rot_f(p: (f32, f32), rot: u8) -> (f32, f32) {
    let mut q = p;
    for _ in 0..(rot % 4) {
        q = (-q.1, q.0);
    }
    q
}

impl SchEditor {
    pub(super) fn to_screen(&self, rect: Rect, p: (f32, f32)) -> Pos2 {
        let c = rect.center();
        Pos2::new(c.x + (p.0 - self.pan.x) * self.zoom, c.y + (p.1 - self.pan.y) * self.zoom)
    }

    pub(super) fn to_grid(&self, rect: Rect, s: Pos2) -> (f32, f32) {
        let c = rect.center();
        ((s.x - c.x) / self.zoom + self.pan.x, (s.y - c.y) / self.zoom + self.pan.y)
    }

    pub(super) fn draw_symbol(&self, painter: &egui::Painter, rect: Rect, kind: Kind, pos: Pt, rot: u8, color: Color32) {
        let tr = |x: f32, y: f32| {
            let r = rot_f((x, y), rot);
            self.to_screen(rect, (pos.0 as f32 + r.0, pos.1 as f32 + r.1))
        };
        let stroke = Stroke::new(1.6, color);
        for s in symbol(kind) {
            match s {
                Sh::L(a, b, c, d) => {
                    painter.line_segment([tr(a, b), tr(c, d)], stroke);
                }
                Sh::C(x, y, r) => {
                    painter.circle_stroke(tr(x, y), r * self.zoom, stroke);
                }
            }
        }
    }

    pub(super) fn draw_component(&self, painter: &egui::Painter, rect: Rect, c: &Component, color: Color32) {
        match &c.custom {
            Some(sym) => self.draw_custom(painter, rect, sym, c.pos, c.rot, color),
            None => self.draw_symbol(painter, rect, c.kind, c.pos, c.rot, color),
        }
    }

    pub(super) fn draw_custom(&self, painter: &egui::Painter, rect: Rect, sym: &CustomSymbol, pos: Pt, rot: u8, color: Color32) {
        let l = sym.layout();
        let tr = |x: f32, y: f32| {
            let r = rot_f((x, y), rot);
            self.to_screen(rect, (pos.0 as f32 + r.0, pos.1 as f32 + r.1))
        };
        let stroke = Stroke::new(1.6, color);
        let (hw, hh) = (l.hw as f32, l.hh as f32);
        let corners = [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)];
        for i in 0..4 {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            painter.line_segment([tr(a.0, a.1), tr(b.0, b.1)], stroke);
        }
        let name_font = FontId::proportional((self.zoom * 0.42).clamp(7.0, 14.0));
        let num_font = FontId::proportional((self.zoom * 0.34).clamp(6.0, 11.0));
        for p in &l.pins {
            let (tx, ty, ex, ey) = (p.tip.0 as f32, p.tip.1 as f32, p.edge.0 as f32, p.edge.1 as f32);
            painter.line_segment([tr(tx, ty), tr(ex, ey)], stroke);
            // name inside the body, anchored so it stays upright after rotation
            let inward = match p.side {
                PinSide::Left => (1.0, 0.0),
                PinSide::Right => (-1.0, 0.0),
                PinSide::Top => (0.0, 1.0),
                PinSide::Bottom => (0.0, -1.0),
            };
            let r = rot_f(inward, rot);
            let anchor = if r.0 > 0.5 {
                Align2::LEFT_CENTER
            } else if r.0 < -0.5 {
                Align2::RIGHT_CENTER
            } else if r.1 > 0.5 {
                Align2::CENTER_TOP
            } else {
                Align2::CENTER_BOTTOM
            };
            painter.text(tr(ex + inward.0 * 0.3, ey + inward.1 * 0.3), anchor, &p.name, name_font.clone(), color);
            // pin number beside the lead
            let (mx, my) = ((tx + ex) / 2.0, (ty + ey) / 2.0);
            let off = if matches!(p.side, PinSide::Left | PinSide::Right) { (0.0, -0.45) } else { (0.45, 0.0) };
            painter.text(tr(mx + off.0, my + off.1), Align2::CENTER_CENTER, &p.number, num_font.clone(), Color32::from_gray(110));
        }
    }

    /// Zoom and pan so that the whole drawing is visible.
    pub(super) fn fit_view(&mut self, rect: Rect) {
        let mut pts: Vec<Pt> = vec![];
        for c in &self.sch.components {
            let (x0, x1, y0, y1) = comp_extents(c);
            for (x, y) in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)] {
                let r = rot_f((x, y), c.rot);
                pts.push((c.pos.0 + r.0.round() as i32, c.pos.1 + r.1.round() as i32));
            }
        }
        for w in &self.sch.wires {
            pts.push(w.a);
            pts.push(w.b);
        }
        for l in &self.sch.labels {
            pts.push(l.pos);
        }
        if pts.is_empty() {
            return;
        }
        let (x0, x1) = (pts.iter().map(|p| p.0).min().unwrap() as f32, pts.iter().map(|p| p.0).max().unwrap() as f32);
        let (y0, y1) = (pts.iter().map(|p| p.1).min().unwrap() as f32, pts.iter().map(|p| p.1).max().unwrap() as f32);
        let (w, h) = ((x1 - x0).max(4.0) + 6.0, (y1 - y0).max(4.0) + 6.0);
        self.zoom = (rect.width() / w).min(rect.height() / h).clamp(6.0, 40.0);
        self.pan = Vec2::new((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    }
}
