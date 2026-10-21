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
                v.extend(super::glyphs::led_arrow_heads());
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

/// What a voltage source's value says it produces, so the symbol can show it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SourceShape {
    Dc,
    Sine,
    Pulse,
    Other,
}

pub(super) fn source_shape(value: &str) -> SourceShape {
    let v = value.trim().to_ascii_uppercase();
    if v.starts_with("SIN") {
        SourceShape::Sine
    } else if v.starts_with("PULSE") {
        SourceShape::Pulse
    } else if v.starts_with("PWL") || v.starts_with("NOISE") {
        SourceShape::Other
    } else {
        SourceShape::Dc
    }
}

/// A voltage source's symbol: a circle with +/- for DC, a sine wave for an AC source, a square
/// wave for a pulse source (a small + near the top still marks pin 1).
pub(super) fn source_symbol(value: &str) -> Vec<Sh> {
    use Sh::*;
    let shape = source_shape(value);
    if shape == SourceShape::Dc {
        return symbol(Kind::VSource);
    }
    let mut v = vec![L(0., -2., 0., -1.2), L(0., 1.2, 0., 2.), C(0., 0., 1.2), L(-0.2, -0.85, 0.2, -0.85), L(0., -1.05, 0., -0.65)];
    match shape {
        SourceShape::Sine => {
            let n = 16;
            let pt = |i: i32| {
                let x = -0.8 + 1.6 * i as f32 / n as f32;
                (x, 0.2 - 0.4 * (2.0 * std::f32::consts::PI * (x + 0.8) / 1.6).sin())
            };
            for i in 0..n {
                let (a, b) = (pt(i), pt(i + 1));
                v.push(L(a.0, a.1, b.0, b.1));
            }
        }
        SourceShape::Pulse => {
            let pts = [(-0.9, 0.6), (-0.45, 0.6), (-0.45, -0.1), (0.1, -0.1), (0.1, 0.6), (0.9, 0.6)];
            for w in pts.windows(2) {
                v.push(L(w[0].0, w[0].1, w[1].0, w[1].1));
            }
        }
        _ => {
            // arbitrary waveform: a zigzag
            let pts = [(-0.9, 0.5), (-0.4, -0.1), (0.0, 0.4), (0.4, 0.0), (0.9, 0.5)];
            for w in pts.windows(2) {
                v.push(L(w[0].0, w[0].1, w[1].0, w[1].1));
            }
        }
    }
    v
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
    if sym.is_two_lead_glyph() {
        return (-2., 2., -1., 1.);
    }
    let l = sym.layout();
    // a compact diode / battery: the leads are the whole width, the body is one step each side of the line
    if l.hw == 0 && l.hh == 1 {
        // (a four-pin crystal also has its case pins below the body)
        let bottom = l.pins.iter().map(|p| p.tip.1).max().unwrap_or(0).max(1);
        return (-2., 2., -1., bottom as f32);
    }
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
        self.draw_shapes(painter, rect, symbol(kind), pos, rot, color);
    }

    fn draw_shapes(&self, painter: &egui::Painter, rect: Rect, shapes: Vec<Sh>, pos: Pt, rot: u8, color: Color32) {
        let tr = |x: f32, y: f32| {
            let r = rot_f((x, y), rot);
            self.to_screen(rect, (pos.0 as f32 + r.0, pos.1 as f32 + r.1))
        };
        let stroke = Stroke::new(1.6, color);
        for s in shapes {
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
            None if c.kind == Kind::VSource => self.draw_shapes(painter, rect, source_symbol(&c.value), c.pos, c.rot, color),
            None => self.draw_symbol(painter, rect, c.kind, c.pos, c.rot, color),
        }
    }

    pub(super) fn draw_custom(&self, painter: &egui::Painter, rect: Rect, sym: &CustomSymbol, pos: Pt, rot: u8, color: Color32) {
        let l = sym.layout();
        // diodes, transistors, op-amps ... get their standard symbol; the pins stay where the box puts them
        if let Some(shapes) = super::glyphs::body_glyph(sym).and_then(|g| super::glyphs::body(g, &l)) {
            self.draw_shapes(painter, rect, shapes, pos, rot, color);
            let tr = |x: f32, y: f32| {
                let r = rot_f((x, y), rot);
                self.to_screen(rect, (pos.0 as f32 + r.0, pos.1 as f32 + r.1))
            };
            let num_font = FontId::proportional((self.zoom * 0.34).clamp(6.0, 11.0));
            for p in &l.pins {
                let (mx, my) = ((p.tip.0 + p.edge.0) as f32 / 2.0, (p.tip.1 + p.edge.1) as f32 / 2.0);
                let off = if matches!(p.side, PinSide::Left | PinSide::Right) { (0.0, -0.45) } else { (0.45, 0.0) };
                painter.text(tr(mx + off.0, my + off.1), Align2::CENTER_CENTER, &p.number, num_font.clone(), Color32::from_gray(110));
            }
            return;
        }
        // a chip capacitor / resistor / inductor from the library looks like the basic part
        let glyph = match sym.glyph.as_str() {
            "capacitor" | "electrolytic" => Some(Kind::Capacitor),
            "resistor" => Some(Kind::Resistor),
            "inductor" => Some(Kind::Inductor),
            _ => None,
        };
        if let Some(kind) = glyph.filter(|_| sym.is_two_lead_glyph()) {
            let tr = |x: f32, y: f32| {
                let r = rot_f((x, y), rot);
                self.to_screen(rect, (pos.0 as f32 + r.0, pos.1 as f32 + r.1))
            };
            let stroke = Stroke::new(1.6, color);
            let num_font = FontId::proportional((self.zoom * 0.34).clamp(6.0, 11.0));
            for p in &l.pins {
                let s = if p.tip.0 < 0 { -2.0 } else { 2.0 };
                painter.line_segment([tr(p.tip.0 as f32, p.tip.1 as f32), tr(s, 0.0)], stroke);
                painter.text(tr((p.tip.0 as f32 + s) / 2.0, -0.45), Align2::CENTER_CENTER, &p.number, num_font.clone(), Color32::from_gray(110));
            }
            self.draw_symbol(painter, rect, kind, pos, rot, color);
            if super::glyphs::is_thermistor(sym) {
                self.draw_shapes(painter, rect, super::glyphs::thermistor_mark(), pos, rot, color);
            }
            if sym.glyph == "electrolytic" {
                // polarity: a "+" beside the plate of the left (pin 1) lead
                for (a, b) in [((-1.5, -0.9), (-0.9, -0.9)), ((-1.2, -1.2), (-1.2, -0.6))] {
                    painter.line_segment([tr(a.0, a.1), tr(b.0, b.1)], stroke);
                }
            }
            return;
        }
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

/// A small picture of the plain symbol of `kind`, scaled to fit `rect` (for the parts list).
pub(super) fn paint_kind_preview(painter: &egui::Painter, rect: Rect, kind: Kind, color: Color32) {
    let (x0, x1, y0, y1) = extents(kind);
    let (w, h) = ((x1 - x0).max(0.5), (y1 - y0).max(0.5));
    let scale = ((rect.width() - 4.0) / w).min((rect.height() - 4.0) / h);
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let tr = |x: f32, y: f32| Pos2::new(rect.center().x + (x - cx) * scale, rect.center().y + (y - cy) * scale);
    let stroke = Stroke::new(1.3, color);
    for s in symbol(kind) {
        match s {
            Sh::L(a, b, c, d) => painter.line_segment([tr(a, b), tr(c, d)], stroke),
            Sh::C(x, y, r) => painter.circle_stroke(tr(x, y), r * scale, stroke),
        };
    }
}

/// A small picture of a library part's symbol (line segments in grid units), scaled to fit `rect`.
pub(super) fn paint_lines_preview(painter: &egui::Painter, rect: Rect, lines: &[([f32; 2], [f32; 2])], color: Color32) {
    if lines.is_empty() {
        return;
    }
    let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for p in lines.iter().flat_map(|(a, b)| [a, b]) {
        (x0, x1, y0, y1) = (x0.min(p[0]), x1.max(p[0]), y0.min(p[1]), y1.max(p[1]));
    }
    let (w, h) = ((x1 - x0).max(0.5), (y1 - y0).max(0.5));
    let scale = ((rect.width() - 4.0) / w).min((rect.height() - 4.0) / h);
    let tr = |p: [f32; 2]| Pos2::new(rect.center().x + (p[0] - (x0 + x1) / 2.0) * scale, rect.center().y + (p[1] - (y0 + y1) / 2.0) * scale);
    let stroke = Stroke::new(1.2, color);
    for (a, b) in lines {
        painter.line_segment([tr(*a), tr(*b)], stroke);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_symbol_follows_its_waveform() {
        assert_eq!(source_shape("5"), SourceShape::Dc);
        assert_eq!(source_shape("5 AC 1"), SourceShape::Dc);
        assert_eq!(source_shape("SIN(0 141.4 50) AC 1"), SourceShape::Sine);
        assert_eq!(source_shape(" pulse(0 5 1u 1n 1n 1u 2u)"), SourceShape::Pulse);
        assert_eq!(source_shape("PWL(0 0 1 1)"), SourceShape::Other);
        // DC keeps the +/- circle; the others draw a waveform (more segments than the DC symbol)
        assert_eq!(source_symbol("5").len(), symbol(Kind::VSource).len());
        assert!(source_symbol("SIN(0 1 1k)").len() > 10);
        // pins stay where they were: every variant has the same two leads
        let leads = |v: Vec<Sh>| v.iter().filter(|s| matches!(s, Sh::L(0., y, 0., _) if y.abs() >= 1.2 || *y == -2.0)).count();
        assert!(leads(source_symbol("SIN(0 1 1k)")) >= 2);
    }
}
