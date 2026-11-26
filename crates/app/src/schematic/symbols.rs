//! Drawing the schematic symbols and the screen/grid transforms.

use super::*;

// ------------------------------------------------------------- symbol drawing

pub(super) use sch_core::symbol_shapes::{Sh, symbol, source_symbol, extents, sym_extents, comp_extents, sheet_box, boxes_overlap, rot_f};

pub(super) const OVERLAP_MSG: &str = "部品が重なるので、そこには置けません";

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
            Some(sym) => {
                // a sheet symbol (a child sheet used as a part) gets a tinted body so it is not mistaken for an ordinary part
                if c.sub.is_some() {
                    let l = sym.layout();
                    let (hw, hh) = (l.hw as f32, l.hh as f32);
                    let pts: Vec<egui::Pos2> = [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)]
                        .iter()
                        .map(|&p| {
                            let r = rot_f(p, c.rot);
                            self.to_screen(rect, (c.pos.0 as f32 + r.0, c.pos.1 as f32 + r.1))
                        })
                        .collect();
                    painter.add(egui::Shape::convex_polygon(pts, crate::canvas_theme::col(crate::canvas_theme::Role::PartFill), Stroke::NONE));
                }
                self.draw_custom(painter, rect, sym, c.pos, c.rot, color)
            }
            None if c.kind == Kind::VSource => self.draw_shapes(painter, rect, source_symbol(&c.value), c.pos, c.rot, color),
            None => {
                if let Kind::Header(_) = c.kind {
                    // its rows and its kind (a socket) are its own: `component_shapes`
                    self.draw_shapes(painter, rect, sch_core::symbol_shapes::component_shapes(c), c.pos, c.rot, color);
                } else {
                    self.draw_symbol(painter, rect, c.kind, c.pos, c.rot, color);
                }
                // the name of each pin of a pin header, on the side of the body away from the pins (where the net labels go), upright
                // whatever the turn; on a header of two columns, under each pin's lead, outside the body
                if let Kind::Header(n) = c.kind {
                    let font = crate::theme::canvas_font(painter, (self.zoom * 0.5).clamp(12.0, 16.0));
                    let two = c.two_columns;
                    for i in 0..n as usize {
                        if let Some(name) = c.pin_name(&(i + 1).to_string()) {
                            if two {
                                let left = i % 2 == 0;
                                // under the lead (a net label on its end sits above it)
                                let at = (if left { -1.6 } else { 1.6 }, 2.0 * (i / 2) as f32 + 0.3);
                                let r = rot_f(at, c.rot);
                                let p = self.to_screen(rect, (c.pos.0 as f32 + r.0, c.pos.1 as f32 + r.1));
                                painter.text(p, Align2::CENTER_TOP, name, font.clone(), color);
                                continue;
                            }
                            let r = rot_f((-0.4, 2.0 * i as f32), c.rot);
                            let p = self.to_screen(rect, (c.pos.0 as f32 + r.0, c.pos.1 as f32 + r.1));
                            let anchor = match c.rot % 4 {
                                0 => Align2::RIGHT_CENTER,
                                2 => Align2::LEFT_CENTER,
                                1 => Align2::CENTER_BOTTOM,
                                _ => Align2::CENTER_TOP,
                            };
                            painter.text(p, anchor, name, font.clone(), color);
                        }
                    }
                }
            }
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
            for p in &l.pins {
                let (mx, my) = ((p.tip.0 + p.edge.0) as f32 / 2.0, (p.tip.1 + p.edge.1) as f32 / 2.0);
                let off = if matches!(p.side, PinSide::Left | PinSide::Right) { (0.0, -0.45) } else { (0.45, 0.0) };
                self.pin_number(painter, tr(mx + off.0, my + off.1), &p.number);
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
            for p in &l.pins {
                let s = if p.tip.0 < 0 { -2.0 } else { 2.0 };
                painter.line_segment([tr(p.tip.0 as f32, p.tip.1 as f32), tr(s, 0.0)], stroke);
                self.pin_number(painter, tr((p.tip.0 as f32 + s) / 2.0, -0.45), &p.number);
            }
            self.draw_symbol(painter, rect, kind, pos, rot, color);
            // a leaded resistor of a maker's series: its colour bands inside the box, read from pin 1 (the left end)
            if kind == Kind::Resistor
                && self.zoom >= 9.0
                && let Some(bands) = color_bands(&sym.name)
            {
                let places = pcb_core::body::band_places(bands.len());
                for (b, f) in bands.iter().zip(places) {
                    let mid = -1.0 + 2.0 * *f as f32;
                    let half = 0.08;
                    let [r, g, bl] = b.rgb();
                    let pts = vec![tr(mid - half, -0.36), tr(mid + half, -0.36), tr(mid + half, 0.36), tr(mid - half, 0.36)];
                    painter.add(egui::Shape::convex_polygon(pts, Color32::from_rgb(r, g, bl), Stroke::NONE));
                }
            }
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
        if let Some(g) = super::glyphs::overlay_glyph(sym) {
            self.draw_shapes(painter, rect, super::glyphs::overlay(g, &l), pos, rot, color);
        }
        let name_font = self.tier_font(painter, super::draw::Tier::Sub, 0.42);
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
            if let Some(f) = &name_font {
                painter.text(tr(ex + inward.0 * 0.3, ey + inward.1 * 0.3), anchor, &p.name, f.clone(), color);
            }
            // pin number beside the lead
            let (mx, my) = ((tx + ex) / 2.0, (ty + ey) / 2.0);
            let off = if matches!(p.side, PinSide::Left | PinSide::Right) { (0.0, -0.45) } else { (0.45, 0.0) };
            self.pin_number(painter, tr(mx + off.0, my + off.1), &p.number);
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
        let base = (
            pts.iter().map(|p| p.0).min().unwrap() as f32,
            pts.iter().map(|p| p.0).max().unwrap() as f32,
            pts.iter().map(|p| p.1).min().unwrap() as f32,
            pts.iter().map(|p| p.1).max().unwrap() as f32,
        );
        // a running lesson's (or challenge's) window floats over the right of the canvas: the circuit goes into the part left of it
        let free = if self.lesson.is_some() || self.challenge.is_some() { (rect.width() - LESSON_WINDOW_PX).max(rect.width() * 0.5) } else { rect.width() };
        let fit = |(x0, x1, y0, y1): (f32, f32, f32, f32)| {
            let (w, h) = ((x1 - x0).max(4.0) + 6.0, (y1 - y0).max(4.0) + 6.0);
            (free / w).min(rect.height() / h).clamp(6.0, 40.0)
        };
        // the names and values written beside the parts take room too (a source's "SIN(0 0.1 1k) AC 1" ran off the left edge): their
        // width in sheet units depends on the zoom (the text is 0.55 × zoom px, at least 8), so fit once without them, then again with
        let zoom = fit(base);
        let b = self.text_bounds(base, zoom);
        self.zoom = fit(b);
        let b = self.text_bounds(base, self.zoom);
        let shift = (rect.width() - free) / 2.0 / self.zoom;
        self.pan = Vec2::new((b.0 + b.1) / 2.0 + shift, (b.2 + b.3) / 2.0);
    }

    /// `bounds` widened by the part names and values as `draw_components` places them at `zoom` (an estimate of their width:
    /// 0.6 em per character, CJK twice that).
    fn text_bounds(&self, bounds: (f32, f32, f32, f32), zoom: f32) -> (f32, f32, f32, f32) {
        let em = (zoom * 0.55).clamp(8.0, 18.0) / zoom;
        let width = |t: &str| t.chars().map(|c| if c.is_ascii() { 0.6 } else { 1.0 }).sum::<f32>() * em;
        let (mut x0, mut x1, mut y0, mut y1) = bounds;
        for c in self.sch.components.iter().filter(|c| !c.kind.is_power_symbol()) {
            let ext = comp_extents(c);
            let Some((bx0, bx1, by0, by1)) = sheet_box(c.kind, ext, c.pos, c.rot) else { continue };
            let w = width(&c.shown_refdes()).max(width(&c.value));
            if by1 - by0 <= bx1 - bx0 || super::draw::side_pinned(c, bx0, bx1) {
                // centred above and below the body
                let cx = (bx0 + bx1) / 2.0;
                x0 = x0.min(cx - w / 2.0 - 0.4);
                x1 = x1.max(cx + w / 2.0);
                y0 = y0.min(by0 - 0.3 - em * 1.2);
                y1 = y1.max(by1 + 0.3 + em * 1.2);
            } else {
                // left of the body, level with its middle
                x0 = x0.min(bx0 - 0.4 - w);
            }
        }
        (x0, x1, y0, y1)
    }
}

/// Width of the lesson window (440) and its margin, in screen pixels.
const LESSON_WINDOW_PX: f32 = 470.0;

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

/// The colour bands of a library part that is one value of a maker's series with a colour code (KOA CF, MF), first to last.
pub(super) fn color_bands(name: &str) -> Option<Vec<pcb_core::body::BandColor>> {
    let (s, v) = part_lib::series::member(name)?;
    s.outline.as_ref()?.color_code.as_ref()?.bands_for(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sch_core::symbol_shapes::{source_shape, SourceShape};

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
