//! The drawing area: view, input handling and painting.

use super::*;

impl SchEditor {
    pub fn canvas(&mut self, ui: &mut egui::Ui) {
        self.track_history(ui);
        self.live_update();
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = resp.rect;
        painter.rect_filled(rect, 0.0, SCH_BG);
        if self.dc.is_some() {
            self.sig_cache = self.signature();
        }
        if self.fit_pending {
            self.fit_pending = false;
            self.fit_view(rect);
        }

        // in the Select tool the right button draws a crossing selection box instead of panning
        let rmb_selects = self.tool == Tool::Select && !self.noise_pick;
        if resp.dragged_by(egui::PointerButton::Middle) || (resp.dragged_by(egui::PointerButton::Secondary) && !rmb_selects) {
            let d = resp.drag_delta();
            self.pan.x -= d.x / self.zoom;
            self.pan.y -= d.y / self.zoom;
        }
        let hover = resp.hover_pos();
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let cur = hover.unwrap_or(rect.center());
                let before = self.to_grid(rect, cur);
                self.zoom = (self.zoom * (1.0 + scroll * 0.002)).clamp(6.0, 120.0);
                let after = self.to_grid(rect, cur);
                self.pan.x += before.0 - after.0;
                self.pan.y += before.1 - after.1;
            }
        }

        let typing = ui.ctx().egui_wants_keyboard_input();
        if self.noise_pick && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.noise_pick = false;
            self.status = "ノードの選択を中止しました".into();
        }
        if let Some(i) = self.rotate_req.take() {
            self.rotate_comp(i);
        }
        if !typing {
            let (cmd, shift) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
            if cmd && ui.input(|i| i.key_pressed(egui::Key::Z)) {
                if shift { self.redo() } else { self.undo() }
            } else if cmd && ui.input(|i| i.key_pressed(egui::Key::Y)) {
                self.redo();
            }
            if ui.input(|i| i.key_pressed(egui::Key::R)) {
                match self.selected {
                    Some(Sel::Comp(i)) => self.rotate_comp(i),
                    _ if self.multi.iter().any(|s| matches!(s, Sel::Comp(_))) => {
                        for s in self.multi.clone() {
                            if let Sel::Comp(i) = s {
                                self.rotate_comp(i);
                            }
                        }
                    }
                    _ => self.rot = (self.rot + 1) % 4,
                }
            }
            if ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) {
                self.delete_selected();
            }
            if ui.input(|i| i.key_pressed(egui::Key::F)) {
                self.fit_pending = true;
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.pending = None;
                self.selected = None;
                self.multi.clear();
                self.boxsel = None;
                self.tool = Tool::Select;
            }
        }

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
            let ctr = (c.pos.0 as f32 + (x0 + x1) / 2.0, c.pos.1 as f32);
            if c.kind.is_power_symbol() {
                if c.kind == Kind::Power {
                    painter.text(self.to_screen(rect, (c.pos.0 as f32, c.pos.1 as f32 - 0.8)), Align2::CENTER_BOTTOM, &c.value, f.clone(), SYM);
                }
            } else {
                let top = c.pos.1 as f32 + if c.rot % 2 == 0 { y0.min(-1.0) - 0.3 } else { -x1.max(2.0) - 0.3 };
                painter.text(self.to_screen(rect, (ctr.0, top)), Align2::CENTER_BOTTOM, &c.refdes, f.clone(), Color32::from_rgb(20, 60, 140));
                let bot = c.pos.1 as f32 + if c.rot % 2 == 0 { y1.max(1.0) + 0.3 } else { x1.max(2.0) + 0.3 };
                painter.text(self.to_screen(rect, (ctr.0, bot)), Align2::CENTER_TOP, &c.value, f, Color32::from_gray(60));
            }
        }
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
        for pr in &self.problems {
            if let Some(a) = pr.at {
                painter.circle_stroke(self.to_screen(rect, (a.0 as f32, a.1 as f32)), 11.0, Stroke::new(2.0, Color32::from_rgb(230, 0, 200)));
            }
        }

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

        // rubber-band selection: left drag = fully enclosed, right drag = anything touched
        if self.tool == Tool::Select && !self.noise_pick {
            let origin = ui.input(|i| i.pointer.press_origin());
            for (btn, crossing) in [(egui::PointerButton::Primary, false), (egui::PointerButton::Secondary, true)] {
                if resp.drag_started_by(btn) && self.drag.is_none() && self.gdrag.is_none()
                    && let Some(o) = origin {
                        let g0 = self.to_grid(rect, o);
                        // a left drag that starts on a part moves it instead (below)
                        if crossing || self.comp_at(g0).is_none() {
                            self.boxsel = Some(BoxSel { start: g0, crossing });
                        }
                    }
            }
            if let Some(b) = &self.boxsel {
                let cur = ui.input(|i| i.pointer.latest_pos()).map(|p| self.to_grid(rect, p)).unwrap_or(b.start);
                let (p0, p1) = (self.to_screen(rect, b.start), self.to_screen(rect, cur));
                let r = Rect::from_two_pos(p0, p1);
                let (fill, line) = if b.crossing {
                    (Color32::from_rgba_unmultiplied(40, 160, 60, 40), Color32::from_rgb(30, 140, 50))
                } else {
                    (Color32::from_rgba_unmultiplied(40, 100, 230, 40), Color32::from_rgb(30, 80, 210))
                };
                painter.rect_filled(r, 0.0, fill);
                painter.rect_stroke(r, 0.0, Stroke::new(1.2, line), egui::StrokeKind::Inside);
                if resp.drag_stopped() {
                    let (start, crossing) = (b.start, b.crossing);
                    let mut items = self.objects_in_box(start, cur, crossing);
                    if ui.input(|i| i.modifiers.shift) {
                        let mut cur_sel = self.multi.clone();
                        cur_sel.extend(self.selected);
                        for s in items {
                            if !cur_sel.contains(&s) {
                                cur_sel.push(s);
                            }
                        }
                        items = cur_sel;
                    }
                    self.status = format!("{} 個を選択 ({}範囲)", items.len(), if crossing { "接触した物を含む" } else { "完全に囲んだ" });
                    self.set_selection(items);
                    self.boxsel = None;
                }
            }
        } else {
            self.boxsel = None;
        }

        // interaction
        let Some(pos) = hover else { return };
        let g = self.to_grid(rect, pos);
        let snapped: Pt = (g.0.round() as i32, g.1.round() as i32);
        let sp = self.to_screen(rect, (snapped.0 as f32, snapped.1 as f32));
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
        if !self.noise_pick {
            self.status = format!("({}, {})  部品 {}  配線 {}", snapped.0, snapped.1, self.sch.components.len(), self.sch.wires.len());
        }

        if resp.drag_started_by(egui::PointerButton::Primary) && self.tool == Tool::Select && !self.noise_pick
            && let Some(i) = self.comp_at(g) {
                if self.multi.len() > 1 && self.multi.contains(&Sel::Comp(i)) {
                    self.start_group_drag(snapped);
                } else {
                    let c = &self.sch.components[i];
                    self.drag = Some((i, (snapped.0 - c.pos.0, snapped.1 - c.pos.1)));
                    self.selected = Some(Sel::Comp(i));
                    self.multi.clear();
                }
            }
        if let Some(gd) = &self.gdrag {
            if resp.dragged_by(egui::PointerButton::Primary) {
                let (dx, dy) = (snapped.0 - gd.anchor.0, snapped.1 - gd.anchor.1);
                let mv = |p: Pt| (p.0 + dx, p.1 + dy);
                // the selection moves as one piece: if any part would land on a part that is
                // not being moved, nothing moves this frame
                let moved: Vec<usize> = gd.comps.iter().map(|c| c.0).collect();
                let blocked = gd.comps.iter().any(|&(i, p)| {
                    let c = &self.sch.components[i];
                    self.would_overlap(c.kind, comp_extents(c), mv(p), c.rot, &moved)
                });
                if blocked {
                    self.status = OVERLAP_MSG.into();
                } else {
                    for &(i, p) in &gd.comps {
                        self.sch.components[i].pos = mv(p);
                    }
                    for &(i, a, b) in &gd.wires {
                        self.sch.wires[i].a = mv(a);
                        self.sch.wires[i].b = mv(b);
                    }
                    for &(i, p) in &gd.labels {
                        self.sch.labels[i].pos = mv(p);
                    }
                }
            } else {
                self.gdrag = None;
            }
        }
        if let Some((i, off)) = self.drag {
            if resp.dragged_by(egui::PointerButton::Primary) {
                let np = (snapped.0 - off.0, snapped.1 - off.1);
                let c = &self.sch.components[i];
                if self.would_overlap(c.kind, comp_extents(c), np, c.rot, &[i]) {
                    self.status = OVERLAP_MSG.into();
                } else {
                    self.sch.components[i].pos = np;
                }
            } else {
                self.drag = None;
            }
        }
        if resp.drag_stopped() {
            self.drag = None;
            self.gdrag = None;
        }

        if self.noise_pick {
            if resp.clicked_by(egui::PointerButton::Primary) {
                let shift = ui.input(|i| i.modifiers.shift);
                self.pick_noise_node(snapped, shift);
            }
        } else if resp.clicked_by(egui::PointerButton::Primary) {
            match self.tool {
                Tool::Select => {
                    self.multi.clear();
                    self.selected = self.pick(g);
                }
                Tool::Place => {
                    let lib = self.place_lib.and_then(|k| self.library.get(k)).cloned();
                    let (kind, ext) = match &lib {
                        Some(d) => (Kind::Custom, sym_extents(&d.symbol())),
                        None => (self.place, extents(self.place)),
                    };
                    if self.would_overlap(kind, ext, snapped, self.rot, &[]) {
                        self.status = OVERLAP_MSG.into();
                    } else {
                        match lib {
                            Some(d) => {
                                self.sch.add_custom(d.symbol(), &d.default_value(), &d.footprint_name(), snapped, self.rot);
                            }
                            None => {
                                self.sch.add(self.place, snapped, self.rot);
                            }
                        }
                    }
                }
                Tool::Wire => match self.pending.take() {
                    None => self.pending = Some(snapped),
                    Some(a) => {
                        self.add_wire(a, snapped);
                        self.pending = Some(snapped);
                    }
                },
                Tool::Label => {
                    if !self.label_text.is_empty() {
                        self.sch.labels.push(Label { pos: snapped, text: self.label_text.clone() });
                    }
                }
            }
        }
        if resp.double_clicked() && self.tool == Tool::Wire {
            self.pending = None;
        }
        if resp.clicked_by(egui::PointerButton::Secondary) {
            self.pending = None;
        }
    }
}
