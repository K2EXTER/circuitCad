//! Painting the sheet: grid, wires, parts, labels and the analysis overlays.

use super::*;

impl SchEditor {
    pub(super) fn draw_grid(&self, painter: &egui::Painter, rect: Rect) {
        // grid dots
        let tl = self.to_grid(rect, rect.left_top());
        let br = self.to_grid(rect, rect.right_bottom());
        if self.zoom >= 10.0 {
            // one mesh of little squares instead of a circle shape per dot (a big sheet has thousands)
            let mut dots = egui::epaint::Mesh::default();
            for x in (tl.0.floor() as i32)..=(br.0.ceil() as i32) {
                for y in (tl.1.floor() as i32)..=(br.1.ceil() as i32) {
                    dots.add_colored_rect(Rect::from_center_size(self.to_screen(rect, (x as f32, y as f32)), egui::Vec2::splat(1.8)), Color32::from_gray(190));
                }
            }
            painter.add(egui::Shape::mesh(dots));
        }
    }

    pub(super) fn draw_wires(&self, painter: &egui::Painter, rect: Rect) {
        // wires (tinted by the net voltage when a DC result is showing)
        let live_dc = self.dc.as_ref().filter(|d| d.signature == self.signature_cached() && d.outcome.is_ok());
        for (i, w) in self.sch.wires.iter().enumerate() {
            let sel = self.is_sel(Sel::Wire(i));
            let tint = live_dc.filter(|_| self.show_v).and_then(|d| d.wire_v.get(i).copied().flatten().map(|v| volt_color(v, d.v_range)));
            painter.line_segment(
                [self.to_screen(rect, (w.a.0 as f32, w.a.1 as f32)), self.to_screen(rect, (w.b.0 as f32, w.b.1 as f32))],
                Stroke::new(if sel { 3.0 } else { 2.2 }, if sel { Color32::from_rgb(0, 80, 220) } else { tint.unwrap_or(WIRE) }),
            );
        }
    }

    pub(super) fn draw_junctions(&self, painter: &egui::Painter, rect: Rect) {
        // junction dots
        let pins: Vec<Pt> = self.sch.components.iter().flat_map(|c| c.pins().into_iter().map(|p| p.1)).collect();
        for w in &self.sch.wires {
            for p in [w.a, w.b] {
                let ends = self.sch.wires.iter().map(|x| (x.a == p) as u32 + (x.b == p) as u32).sum::<u32>()
                    + pins.iter().filter(|&&q| q == p).count() as u32;
                let on_other = self.sch.wires.iter().any(|x| x.a != p && x.b != p && on_segment(p, x.a, x.b));
                if ends >= 3 || on_other {
                    painter.circle_filled(self.to_screen(rect, (p.0 as f32, p.1 as f32)), 3.5, WIRE);
                }
            }
        }
    }

    pub(super) fn draw_components(&self, painter: &egui::Painter, rect: Rect) {
        // components
        for (i, c) in self.sch.components.iter().enumerate() {
            let sel = self.is_sel(Sel::Comp(i));
            let col = if sel { Color32::from_rgb(0, 80, 220) } else { SYM };
            self.draw_component(painter, rect, c, col);
            for (_, p) in c.pins() {
                painter.rect_filled(
                    Rect::from_center_size(self.to_screen(rect, (p.0 as f32, p.1 as f32)), Vec2::splat(4.0)),
                    0.0,
                    Color32::from_rgb(190, 40, 40),
                );
            }
            // the pins left open on purpose: a cross over the pin
            for (pin, p) in c.pins() {
                if self.sch.is_no_connect(&c.refdes, &pin) {
                    let at = self.to_screen(rect, (p.0 as f32, p.1 as f32));
                    let s = Stroke::new(1.8, Color32::from_rgb(60, 90, 170));
                    painter.line_segment([at + Vec2::new(-4.5, -4.5), at + Vec2::new(4.5, 4.5)], s);
                    painter.line_segment([at + Vec2::new(-4.5, 4.5), at + Vec2::new(4.5, -4.5)], s);
                }
            }
            let f = FontId::proportional((self.zoom * 0.55).clamp(8.0, 18.0));
            let (x0, x1, y0, y1) = comp_extents(c);
            if c.kind.is_power_symbol() {
                if c.kind == Kind::Power {
                    painter.text(self.to_screen(rect, (c.pos.0 as f32, c.pos.1 as f32 - 0.8)), Align2::CENTER_BOTTOM, &c.value, f.clone(), SYM);
                }
            } else {
                let name_col = Color32::from_rgb(20, 60, 140);
                // the part's box on the sheet after rotation; a part taller than wide has its pins at the top and bottom
                let (bx0, bx1, by0, by1) = sheet_box(c.kind, (x0, x1, y0, y1), c.pos, c.rot)
                    .unwrap_or((c.pos.0 as f32 - 2.0, c.pos.0 as f32 + 2.0, c.pos.1 as f32 - 1.0, c.pos.1 as f32 + 1.0));
                // a part with pins down its sides (an IC) has its wires and net labels left and right of the body: its name and
                // value go above and below, like a wide part's
                if by1 - by0 <= bx1 - bx0 || side_pinned(c, bx0, bx1) {
                    let cx = (bx0 + bx1) / 2.0;
                    // a pin on the centre line of the top / bottom edge has its wire running through the centred text:
                    // put the text just left of that wire instead (the voltages are drawn right of the pins)
                    let wire_at = |y: f32| pin_on_edge(c, y, cx);
                    // the arrows of an LED rise 1.5 above its centre (or hang 1.5 below when it is turned upside down)
                    let (arrows_up, arrows_down) = (c.kind == Kind::Led && c.rot % 4 == 0, c.kind == Kind::Led && c.rot % 4 == 2);
                    let name_y = by0 - if arrows_up { 1.2 } else { 0.3 };
                    let value_y = by1 + if arrows_down { 1.2 } else { 0.3 };
                    // a four-pin crystal has its two case pins (and their wires) on one side: name and value both go on the other
                    let case_pins = c.custom.as_ref().is_some_and(|s| s.pins.len() == 4 && super::glyphs::body_glyph(s) == Some("crystal")) && c.rot % 2 == 0;
                    if case_pins {
                        let up = c.rot % 4 == 0;
                        let y = if up { name_y } else { value_y };
                        let (left, right) = if up { (Align2::RIGHT_BOTTOM, Align2::LEFT_BOTTOM) } else { (Align2::RIGHT_TOP, Align2::LEFT_TOP) };
                        painter.text(self.to_screen(rect, (cx - 0.3, y)), left, &c.refdes, f.clone(), name_col);
                        painter.text(self.to_screen(rect, (cx + 0.3, y)), right, &c.value, f.clone(), Color32::from_gray(60));
                    } else if wire_at(by0) {
                        painter.text(self.to_screen(rect, (cx - 0.4, name_y)), Align2::RIGHT_BOTTOM, &c.refdes, f.clone(), name_col);
                    } else {
                        painter.text(self.to_screen(rect, (cx, name_y)), Align2::CENTER_BOTTOM, &c.refdes, f.clone(), name_col);
                    }
                    if !case_pins {
                        if wire_at(by1) {
                            painter.text(self.to_screen(rect, (cx - 0.4, value_y)), Align2::RIGHT_TOP, &c.value, f, Color32::from_gray(60));
                        } else {
                            painter.text(self.to_screen(rect, (cx, value_y)), Align2::CENTER_TOP, &c.value, f, Color32::from_gray(60));
                        }
                    }
                } else {
                    // name and value go left of the body, level with its middle, so they stay clear of the pin
                    // voltages (right of each pin) and the wires that run along the pins' line
                    let (left, mid) = (bx0 - 0.4, (by0 + by1) / 2.0);
                    painter.text(self.to_screen(rect, (left, mid)), Align2::RIGHT_BOTTOM, &c.refdes, f.clone(), name_col);
                    painter.text(self.to_screen(rect, (left, mid)), Align2::RIGHT_TOP, &c.value, f, Color32::from_gray(60));
                }
            }
        }
    }

    pub(super) fn draw_labels(&self, painter: &egui::Painter, rect: Rect) {
        for (i, l) in self.sch.labels.iter().enumerate() {
            let sel = self.is_sel(Sel::Label(i));
            let p = self.to_screen(rect, (l.pos.0 as f32, l.pos.1 as f32));
            painter.circle_filled(p, 3.0, WIRE);
            painter.text(
                p + Vec2::new(5.0, -3.0),
                Align2::LEFT_BOTTOM,
                &l.text,
                FontId::proportional(13.0),
                if sel { Color32::from_rgb(0, 80, 220) } else { Color32::from_rgb(0, 100, 100) },
            );
        }
    }

    /// The net label a pin's voltage must keep clear of, and where the voltage goes: after the label's name, before its dot, or
    /// (for a pin on the top or bottom edge, whose neighbours are close) stacked above the name. The label sits on the pin point
    /// itself, or at the far end of a short level wire from the pin (to the right: after the name; to the left: before the dot).
    fn label_anchor(&self, pt: Pt) -> Option<(&sch_core::Label, VoltageSpot)> {
        if let Some(l) = self.sch.labels.iter().find(|l| l.pos == pt) {
            let part_is_right = self.sch.wires.iter().any(|w| (w.a == pt && w.b.1 == pt.1 && w.b.0 > pt.0) || (w.b == pt && w.a.1 == pt.1 && w.a.0 > pt.0))
                || self.sch.components.iter().any(|c| c.pins().iter().any(|p| p.1 == pt) && pin_left_of_body(c, pt));
            let on_top_or_bottom = self.sch.components.iter().any(|c| c.pins().iter().any(|p| p.1 == pt) && pin_beyond_body_vertically(c, pt));
            let spot = if on_top_or_bottom {
                VoltageSpot::AboveName
            } else if part_is_right {
                VoltageSpot::BeforeDot
            } else {
                VoltageSpot::AfterName
            };
            return Some((l, spot));
        }
        self.sch.wires.iter().find_map(|w| {
            let other = if w.a == pt { w.b } else if w.b == pt { w.a } else { return None };
            if other.1 != pt.1 || (other.0 - pt.0).abs() > 6 {
                return None;
            }
            self.sch.labels.iter().find(|l| l.pos == other).map(|l| (l, if other.0 > pt.0 { VoltageSpot::AfterName } else { VoltageSpot::BeforeDot }))
        })
    }

    /// Where each net label's name is drawn, on the screen.
    fn label_rects(&self, painter: &egui::Painter, rect: Rect) -> Vec<Rect> {
        self.sch
            .labels
            .iter()
            .map(|l| {
                let g = painter.layout_no_wrap(l.text.clone(), FontId::proportional(13.0), Color32::BLACK);
                let at = self.to_screen(rect, (l.pos.0 as f32, l.pos.1 as f32)) + Vec2::new(5.0, -3.0);
                Rect::from_min_size(Pos2::new(at.x, at.y - g.size().y), g.size())
            })
            .collect()
    }

    pub(super) fn draw_voltages(&self, painter: &egui::Painter, rect: Rect) {
        if self.show_v
            && let Some(dc) = self.dc.as_ref().filter(|d| d.signature == self.signature_cached()) {
                let f = FontId::proportional((self.zoom * 0.5).clamp(9.0, 14.0));
                let mut seen: Vec<Pt> = vec![];
                for (pt, v) in &dc.pin_v {
                    if seen.contains(pt) {
                        continue;
                    }
                    seen.push(*pt);
                    let galley = painter.layout_no_wrap(eng(*v, "V"), f.clone(), Color32::from_rgb(10, 60, 200));
                    // a pin that has a net label on it or at the end of a short level wire: the voltage goes beside the label
                    // (after its name, or before its dot), not on top of it
                    // a power symbol (VCC, 5V ...) on the pin has its value drawn above it: the voltage stacks above that
                    let power = self.sch.components.iter().find(|c| c.kind == Kind::Power && c.pos == *pt);
                    let top_left = if let Some(c) = power {
                        let name_font = FontId::proportional((self.zoom * 0.55).clamp(8.0, 18.0));
                        let name_h = painter.layout_no_wrap(c.value.clone(), name_font, Color32::BLACK).size().y;
                        let at = self.to_screen(rect, (c.pos.0 as f32, c.pos.1 as f32 - 0.8));
                        Pos2::new(at.x - galley.size().x / 2.0, at.y - name_h - 1.0 - galley.size().y)
                    } else { match self.label_anchor(*pt) {
                        Some((l, spot)) => {
                            let at = self.to_screen(rect, (l.pos.0 as f32, l.pos.1 as f32));
                            let name = painter.layout_no_wrap(l.text.clone(), FontId::proportional(13.0), Color32::BLACK).size();
                            match spot {
                                VoltageSpot::AfterName => Pos2::new(at.x + 5.0 + name.x + 6.0, at.y - 3.0 - galley.size().y),
                                VoltageSpot::BeforeDot => Pos2::new(at.x - 6.0 - galley.size().x, at.y - 3.0 - galley.size().y),
                                VoltageSpot::AboveName => Pos2::new(at.x + 5.0, at.y - 3.0 - name.y - 1.0 - galley.size().y),
                            }
                        }
                        None => self.to_screen(rect, (pt.0 as f32, pt.1 as f32)) + Vec2::new(5.0, -4.0 - galley.size().y),
                    } };
                    painter.rect_filled(Rect::from_min_size(top_left, galley.size()).expand(1.0), 2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 190));
                    painter.galley(top_left, galley, Color32::from_rgb(10, 60, 200));
                }
            }
    }

    pub(super) fn draw_currents(&self, painter: &egui::Painter, rect: Rect) {
        if self.show_i
            && let Some(dc) = self.dc.as_ref().filter(|d| d.signature == self.signature_cached())
            && let Ok(res) = &dc.outcome {
                let f = FontId::proportional((self.zoom * 0.5).clamp(9.0, 14.0));
                let col = Color32::from_rgb(200, 90, 0);
                let label_rects = self.label_rects(painter, rect);
                for c in self.sch.components.iter().filter(|c| !c.kind.is_power_symbol()) {
                    let Some(el) = res.element(&c.refdes) else { continue };
                    let ext = comp_extents(c);
                    let Some((x0, x1, _, y1)) = sheet_box(c.kind, ext, c.pos, c.rot) else { continue };
                    // below the part's value text
                    let at = self.to_screen(rect, ((x0 + x1) / 2.0, y1 + 1.3));
                    let galley = painter.layout_no_wrap(eng(el.current, "A"), f.clone(), col);
                    // a wire leaves the middle of the bottom edge: the text goes left of it
                    let tall = (y1 - sheet_box(c.kind, ext, c.pos, c.rot).map_or(0.0, |b| b.2)) > (x1 - x0);
                    let wire_below = !tall && pin_on_edge(c, y1, (x0 + x1) / 2.0);
                    let (w, h) = (galley.size().x, galley.size().y);
                    let preferred = if wire_below { at - Vec2::new(w + 6.0, 0.0) } else { at - Vec2::new(w / 2.0, 0.0) };
                    // when that spot lies over a net label (a source's "GND" just below it), try left of the wire, then right of it
                    let spots = [preferred, at - Vec2::new(w + 6.0, 0.0), at + Vec2::new(6.0, 0.0)];
                    let rects: Vec<Rect> = spots.iter().map(|p| Rect::from_min_size(*p, Vec2::new(w, h))).collect();
                    let pos = spots[first_clear(&rects, &label_rects)];
                    painter.rect_filled(Rect::from_min_size(pos, galley.size()).expand(1.0), 2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 190));
                    painter.galley(pos, galley, col);
                    // arrow along the part: the direction the (conventional) current flows
                    let pins = c.pins();
                    if pins.len() == 2 && el.current.abs() > 1e-12 {
                        let (p1, p2) = (self.to_screen(rect, (pins[0].1.0 as f32, pins[0].1.1 as f32)), self.to_screen(rect, (pins[1].1.0 as f32, pins[1].1.1 as f32)));
                        let (from, to) = if el.current >= 0.0 { (p1, p2) } else { (p2, p1) };
                        let dir = (to - from).normalized();
                        let tip = from + (to - from) * 0.5 + dir * 6.0;
                        let side = Vec2::new(-dir.y, dir.x) * 4.0;
                        painter.add(egui::Shape::convex_polygon(vec![tip, tip - dir * 9.0 + side, tip - dir * 9.0 - side], col, Stroke::NONE));
                    }
                }
            }
    }

    pub(super) fn draw_problems(&self, painter: &egui::Painter, rect: Rect) {
        for (lo, hi) in &self.sch.erc_skip {
            let r = Rect::from_two_pos(self.to_screen(rect, (lo.0 as f32, lo.1 as f32)), self.to_screen(rect, (hi.0 as f32, hi.1 as f32)));
            painter.rect(r, 4.0, Color32::from_rgba_unmultiplied(150, 150, 150, 28), Stroke::new(1.2, Color32::from_rgb(130, 130, 130)), egui::StrokeKind::Inside);
            painter.text(r.left_top() + Vec2::new(4.0, 2.0), Align2::LEFT_TOP, "描きかけ (ERC対象外)", FontId::proportional(11.0), Color32::from_rgb(120, 120, 120));
        }
        for (k, pr) in self.problems.iter().enumerate() {
            if let Some(a) = pr.at.filter(|_| pr.sheet == self.sch.active_sheet) {
                let color = match pr.severity {
                    Severity::Error => Color32::from_rgb(230, 0, 200),
                    Severity::Warning => Color32::from_rgb(230, 160, 20),
                };
                let focus = self.erc_focus == Some(k);
                painter.circle_stroke(self.to_screen(rect, (a.0 as f32, a.1 as f32)), if focus { 16.0 } else { 11.0 }, Stroke::new(if focus { 3.5 } else { 2.0 }, color));
            }
        }
    }

    pub(super) fn draw_noise_overlay(&self, painter: &egui::Painter, rect: Rect) {
        for (pt, is_ref) in &self.noise_marks {
            let c = self.to_screen(rect, (pt.0 as f32, pt.1 as f32));
            let col = if *is_ref { Color32::from_rgb(40, 140, 40) } else { Color32::from_rgb(220, 90, 0) };
            painter.circle_stroke(c, 10.0, Stroke::new(2.5, col));
            painter.text(c + Vec2::new(12.0, -12.0), Align2::LEFT_BOTTOM, if *is_ref { "基準" } else { "出力" }, FontId::proportional(12.0), col);
        }
        if self.noise_pick {
            let banner = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 26.0));
            painter.rect_filled(banner, 0.0, Color32::from_rgba_unmultiplied(220, 90, 0, 230));
            painter.text(
                banner.left_center() + Vec2::new(10.0, 0.0),
                Align2::LEFT_CENTER,
                "ノイズを測りたいノードをクリック (Shift+クリック: 基準ノード / Esc: 中止)",
                FontId::proportional(14.0),
                Color32::WHITE,
            );
        }
    }

    pub(super) fn draw_noise_hover(&mut self, painter: &egui::Painter, snapped: Pt, sp: Pos2) {
        if self.noise_pick {
            if self.pick_hover.as_ref().map(|(p, _)| *p) != Some(snapped) {
                self.pick_hover = Some((snapped, net_at(&self.sch, snapped)));
            }
            if let Some((_, name)) = &self.pick_hover {
                let (txt, col) = match name {
                    Some(n) => (format!("ノード {n}"), Color32::from_rgb(220, 90, 0)),
                    None => ("(接続なし)".to_string(), Color32::from_gray(130)),
                };
                painter.circle_stroke(sp, 7.0, Stroke::new(2.0, col));
                let g = painter.layout_no_wrap(txt, FontId::proportional(13.0), Color32::WHITE);
                let r = Rect::from_min_size(sp + Vec2::new(12.0, 10.0), g.size()).expand(3.0);
                painter.rect_filled(r, 3.0, Color32::from_rgba_unmultiplied(40, 40, 40, 220));
                painter.galley(r.min + Vec2::new(3.0, 3.0), g, col);
            }
        }
    }

    pub(super) fn draw_tool_ghost(&self, painter: &egui::Painter, rect: Rect, snapped: Pt, sp: Pos2) {
        match self.tool {
            _ if self.noise_pick => {}
            Tool::Place => {
                let ghost = Color32::from_rgba_unmultiplied(120, 20, 20, 120);
                match self.place_lib.and_then(|k| self.library.get(k)) {
                    Some(d) => self.draw_custom(painter, rect, &d.symbol(), snapped, self.rot, ghost),
                    None => self.draw_symbol(painter, rect, self.place, snapped, self.rot, ghost),
                }
            }
            Tool::Wire => {
                painter.circle_stroke(sp, 4.0, Stroke::new(1.5, WIRE));
                if let Some(a) = self.pending {
                    let a_s = self.to_screen(rect, (a.0 as f32, a.1 as f32));
                    let m = self.to_screen(rect, (snapped.0 as f32, a.1 as f32));
                    let st = Stroke::new(1.5, Color32::from_rgba_unmultiplied(0, 110, 30, 150));
                    painter.line_segment([a_s, m], st);
                    painter.line_segment([m, sp], st);
                }
            }
            Tool::Label => {
                painter.circle_stroke(sp, 4.0, Stroke::new(1.5, Color32::from_rgb(0, 100, 100)));
            }
            Tool::Select => {}
        }
    }
}

/// Where a pin's voltage goes next to the pin's net label.
#[derive(Clone, Copy, Debug, PartialEq)]
enum VoltageSpot {
    AfterName,
    BeforeDot,
    AboveName,
}

/// Whether the pin at `pt` of `c` is on the top or bottom edge of the part's box: away from its left and right edges (the box
/// reaches out to the pin ends, so a side pin lies on an edge and a top pin lies between them).
fn pin_beyond_body_vertically(c: &Component, pt: Pt) -> bool {
    let Some((bx0, bx1, _, _)) = sheet_box(c.kind, comp_extents(c), c.pos, c.rot) else { return false };
    (pt.0 as f32) > bx0 + 0.6 && (pt.0 as f32) < bx1 - 0.6
}

/// Whether the pin at `pt` of `c` sits left of the middle of the part's body (its lead runs to the right, into the part).
fn pin_left_of_body(c: &Component, pt: Pt) -> bool {
    let Some((bx0, bx1, _, _)) = sheet_box(c.kind, comp_extents(c), c.pos, c.rot) else { return false };
    (pt.0 as f32) < (bx0 + bx1) / 2.0 - 0.5
}

/// A part with at least three pins on its left or right edge (an IC): the space beside the body is taken by wires and labels.
fn side_pinned(c: &Component, bx0: f32, bx1: f32) -> bool {
    let pins = c.pins();
    let on = |x: f32| pins.iter().filter(|p| (p.1.0 as f32 - x).abs() < 0.6).count();
    on(bx0) >= 3 || on(bx1) >= 3
}

/// The index of the first candidate that touches none of the obstacles (0 when every one does: the preferred spot stays).
fn first_clear(candidates: &[Rect], obstacles: &[Rect]) -> usize {
    candidates.iter().position(|c| !obstacles.iter().any(|o| o.intersects(*c))).unwrap_or(0)
}

/// Whether a pin of `c` sits on the horizontal line `y` close to the x of the part's middle: a wire leaves the part there.
fn pin_on_edge(c: &Component, y: f32, cx: f32) -> bool {
    c.pins().iter().any(|p| (p.1.1 as f32 - y).abs() < 0.1 && (p.1.0 as f32 - cx).abs() < 1.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f32, y: f32) -> Rect {
        Rect::from_min_size(Pos2::new(x, y), Vec2::new(10.0, 10.0))
    }

    #[test]
    fn the_first_spot_that_touches_no_label_wins() {
        let labels = [r(0.0, 0.0)];
        // the preferred spot is on a label; the second is clear
        assert_eq!(first_clear(&[r(5.0, 5.0), r(30.0, 0.0), r(60.0, 0.0)], &labels), 1);
        // nothing is in the way: the preferred spot stays
        assert_eq!(first_clear(&[r(40.0, 0.0), r(60.0, 0.0)], &labels), 0);
        // every spot is blocked: still the preferred one
        assert_eq!(first_clear(&[r(2.0, 2.0), r(4.0, 4.0)], &labels), 0);
        // rectangles that only touch along an edge are not in the way of each other in practice
        assert_eq!(first_clear(&[r(0.0, 0.0)], &[]), 0);
    }

    #[test]
    fn an_ic_with_pins_down_both_sides_counts_as_side_pinned_but_a_resistor_does_not() {
        let mut s = sch_core::Schematic::default();
        let r1 = s.add(Kind::Resistor, (0, 0), 1);
        let (x0, x1, y0, y1) = comp_extents(&s.components[r1]);
        let b = sheet_box(Kind::Resistor, (x0, x1, y0, y1), (0, 0), 1).unwrap();
        assert!(!side_pinned(&s.components[r1], b.0, b.1), "a vertical resistor: pins top and bottom");
        let q = s.add(Kind::Npn, (20, 0), 0);
        let (x0, x1, y0, y1) = comp_extents(&s.components[q]);
        let b = sheet_box(Kind::Npn, (x0, x1, y0, y1), (20, 0), 0).unwrap();
        assert!(!side_pinned(&s.components[q], b.0, b.1), "a transistor has two pins on its right edge, not three");
        let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "ULN2803A").unwrap();
        let u = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (40, 10), 0);
        let (x0, x1, y0, y1) = comp_extents(&s.components[u]);
        let b = sheet_box(s.components[u].kind, (x0, x1, y0, y1), (40, 10), 0).unwrap();
        assert!(side_pinned(&s.components[u], b.0, b.1), "ULN2803A: eight pins on each side");
    }
}
