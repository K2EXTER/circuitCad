//! Painting the sheet: grid, wires, parts, labels and the analysis overlays.

use super::*;

impl SchEditor {
    pub(super) fn draw_grid(&self, painter: &egui::Painter, rect: Rect) {
        // grid dots
        let tl = self.to_grid(rect, rect.left_top());
        let br = self.to_grid(rect, rect.right_bottom());
        if self.zoom >= 10.0 {
            for x in (tl.0.floor() as i32)..=(br.0.ceil() as i32) {
                for y in (tl.1.floor() as i32)..=(br.1.ceil() as i32) {
                    painter.circle_filled(self.to_screen(rect, (x as f32, y as f32)), 0.9, Color32::from_gray(190));
                }
            }
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
            self.draw_component(&painter, rect, c, col);
            for (_, p) in c.pins() {
                painter.rect_filled(
                    Rect::from_center_size(self.to_screen(rect, (p.0 as f32, p.1 as f32)), Vec2::splat(4.0)),
                    0.0,
                    Color32::from_rgb(190, 40, 40),
                );
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
                if by1 - by0 <= bx1 - bx0 {
                    painter.text(self.to_screen(rect, ((bx0 + bx1) / 2.0, by0 - 0.3)), Align2::CENTER_BOTTOM, &c.refdes, f.clone(), name_col);
                    painter.text(self.to_screen(rect, ((bx0 + bx1) / 2.0, by1 + 0.3)), Align2::CENTER_TOP, &c.value, f, Color32::from_gray(60));
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
                    let p = self.to_screen(rect, (pt.0 as f32, pt.1 as f32)) + Vec2::new(5.0, -4.0);
                    let galley = painter.layout_no_wrap(eng(*v, "V"), f.clone(), Color32::from_rgb(10, 60, 200));
                    painter.rect_filled(Rect::from_min_size(p - Vec2::new(0.0, galley.size().y), galley.size()).expand(1.0), 2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 190));
                    painter.galley(p - Vec2::new(0.0, galley.size().y), galley, Color32::from_rgb(10, 60, 200));
                }
            }
    }

    pub(super) fn draw_currents(&self, painter: &egui::Painter, rect: Rect) {
        if self.show_i
            && let Some(dc) = self.dc.as_ref().filter(|d| d.signature == self.signature_cached())
            && let Ok(res) = &dc.outcome {
                let f = FontId::proportional((self.zoom * 0.5).clamp(9.0, 14.0));
                let col = Color32::from_rgb(200, 90, 0);
                for c in self.sch.components.iter().filter(|c| !c.kind.is_power_symbol()) {
                    let Some(el) = res.element(&c.refdes) else { continue };
                    let ext = comp_extents(c);
                    let Some((x0, x1, _, y1)) = sheet_box(c.kind, ext, c.pos, c.rot) else { continue };
                    // below the part's value text
                    let at = self.to_screen(rect, ((x0 + x1) / 2.0, y1 + 1.3));
                    let galley = painter.layout_no_wrap(eng(el.current, "A"), f.clone(), col);
                    let pos = at - Vec2::new(galley.size().x / 2.0, 0.0);
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
        for (k, pr) in self.problems.iter().enumerate() {
            if let Some(a) = pr.at {
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
                    Some(d) => self.draw_custom(&painter, rect, &d.symbol(), snapped, self.rot, ghost),
                    None => self.draw_symbol(&painter, rect, self.place, snapped, self.rot, ghost),
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
