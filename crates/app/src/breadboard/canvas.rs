//! The breadboard drawn and worked on: the holes, the parts and wires, the pointer's strip lit, dragging, the tools' clicks.

use super::*;

impl BreadboardView {
    /// The board. `focus`: the part picked in the schematic, lit here too.
    pub fn canvas(&mut self, ui: &mut egui::Ui, sch: &mut SchEditor, focus: Option<&str>) {
        if self.kind != BoardKind::Breadboard {
            self.strip_canvas(ui, sch);
            return;
        }
        let mut l = self.layout(sch);
        let (response, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
        let rect = response.rect;
        painter.rect_filled(rect, 0.0, crate::canvas_theme::col(crate::canvas_theme::Role::Bg));
        if l.parts.is_empty() && l.off_board.is_empty() {
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, i18n::tr("回路図に部品がありません。回路図の画面で部品を置いて配線すると、ここにブレッドボードの差し方が出ます"), egui::FontId::proportional(16.0), Color32::from_gray(200));
            return;
        }
        let width = 5.0 + l.cols as f32 + 1.0;
        let pitch = ((rect.width() - 40.0) / width).min((rect.height() - 60.0) / (HEIGHT + 2.0)).clamp(6.0, 28.0);
        let origin = Pos2::new(rect.left() + (rect.width() - width * pitch) / 2.0, rect.top() + 40.0 + pitch);
        let to = |h: Hole| origin + at(h) * pitch;
        let cols = l.cols;
        let hole_at = |p: Pos2| -> Option<Hole> {
            let v = (p - origin) / pitch;
            let col = (v.x - 5.0).round();
            if col < 0.0 || col >= cols as f32 {
                return None;
            }
            let col = col as u16;
            let cands = [Hole::Rail { rail: Rail::TopPlus, col }, Hole::Rail { rail: Rail::TopMinus, col }, Hole::Rail { rail: Rail::BottomMinus, col }, Hole::Rail { rail: Rail::BottomPlus, col }];
            cands.into_iter().chain((0..10).map(|row| Hole::Strip { col, row })).find(|h| (at(*h).y - v.y).abs() < 0.5)
        };
        // a jumper end near the pointer (they lie over the parts' leads)
        let wire_end_at = |p: Pos2| -> Option<(usize, usize)> {
            l.wires.iter().enumerate().flat_map(|(k, w)| [(k, 0, w.a), (k, 1, w.b)]).find(|(_, _, h)| to(*h).distance(p) < pitch * 0.45).map(|(k, e, _)| (k, e))
        };
        // with the circuit running, a press on a switch moves it (a push button closes while held) instead of the tool's click
        let mut on_switch = self.switch_down;
        if let Some(lv) = self.live.as_mut().filter(|v| v.error.is_none()) {
            if response.hovered()
                && ui.input(|i| i.pointer.primary_pressed())
                && let Some(p) = ui.input(|i| i.pointer.interact_pos())
                && let Some(k) = l.parts.iter().filter(|x| part_rect(x, to, pitch).expand(pitch * 0.4).contains(p)).find_map(|x| lv.switch_index(&x.refdes))
            {
                lv.press(k);
                self.switch_down = true;
                on_switch = true;
            }
            if ui.input(|i| i.pointer.primary_released()) {
                lv.release();
                self.switch_down = false;
            }
        }
        // with the circuit running, a potentiometer is turned on the board: dragged right or up turns it towards pin 3, the wheel
        // a notch at a time
        if let Some(lv) = self.live.as_mut().filter(|v| v.error.is_none()) {
            // the leads, or the knob as it was drawn (the hover help on the knob says to drag or wheel it)
            let drawn = &self.drawn;
            let pot_at = |lv: &Live, p: Pos2| {
                l.parts
                    .iter()
                    .filter(|x| part_rect(x, to, pitch).expand(pitch * 0.4).contains(p) || drawn.iter().any(|(r, d)| *r == x.refdes && d.contains(p)))
                    .find_map(|x| lv.knob_index(&x.refdes).filter(|k| lv.knobs[*k].pot.is_some()))
            };
            if !on_switch
                && response.hovered()
                && ui.input(|i| i.pointer.primary_pressed())
                && let Some(p) = ui.input(|i| i.pointer.interact_pos())
                && let Some(k) = pot_at(lv, p)
            {
                self.knob_drag = Some((k, p, lv.knobs[k].value));
            }
            let mut turn = None;
            if let Some((k, from, v0)) = self.knob_drag {
                on_switch = true;
                if let Some(p) = ui.ctx().pointer_latest_pos() {
                    turn = Some((k, knob_dragged(v0, from, p)));
                }
                if ui.input(|i| i.pointer.primary_released()) {
                    self.knob_drag = None;
                }
            } else if let Some(p) = response.hover_pos()
                && let Some(k) = pot_at(lv, p)
            {
                // one step for each wheel event (a notch of a mouse wheel)
                let notches: f32 = ui.input(|i| i.events.iter().filter_map(|e| if let egui::Event::MouseWheel { delta, .. } = e { Some(delta.y.signum()) } else { None }).sum());
                if notches != 0.0 {
                    turn = Some((k, (lv.knobs[k].value + KNOB_NOTCH * notches as f64).clamp(0.0, 1.0)));
                }
            }
            if let Some((k, v)) = turn
                && (v - lv.knobs[k].value).abs() > 1e-9
            {
                lv.set_knob(k, v);
                self.knob_values.insert(lv.knobs[k].refdes.clone(), v);
            }
        } else {
            self.knob_drag = None;
        }
        // dragging (the move tool): a jumper's end, or the part, follows the pointer by whole holes; let go to keep it there
        if self.tool == Tool::Move
            && !on_switch
            && response.drag_started()
            && let Some(p) = response.interact_pointer_pos()
        {
            if let Some(we) = wire_end_at(p) {
                self.wire_drag = Some(we);
                self.picked_wire = Some(we.0);
            } else if let Some(h) = hole_at(p)
                && let Some(part) = l.parts.iter().find(|x| part_rect(x, to, pitch).expand(pitch * 0.6).contains(p))
            {
                self.drag = Some(Drag { refdes: part.refdes.clone(), from: h });
                self.picked = Some(part.refdes.clone());
            }
        }
        if let Some((k, end)) = self.wire_drag
            && let Some(p) = response.interact_pointer_pos().or(ui.ctx().pointer_latest_pos())
            && let Some(h) = hole_at(p)
        {
            let m = wire_end_moved(&sch.sch, &l, k, end, h);
            if response.drag_stopped() {
                sch.breadboard = Some(m.clone());
                self.wire_drag = None;
            }
            l = m;
        }
        // the keys of the move tool: R turns the picked part, Delete removes the picked jumper
        if self.tool == Tool::Move && response.hovered() {
            if ui.input(|i| i.key_pressed(egui::Key::R))
                && let Some(r) = &self.picked
            {
                match rotated(&l, r) {
                    Some(m) => {
                        sch.breadboard = Some(m.clone());
                        l = m;
                    }
                    None => sch.status = format!("{r} は、ここでは回せません (IC・電源の列に差した足・ブレッドボードからはみ出す向き)"),
                }
            }
            if ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
                && let Some(k) = self.picked_wire.take()
            {
                let m = without_wire(&l, k);
                sch.breadboard = Some(m.clone());
                l = m;
            }
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.wire_start = None;
        }
        let mut dragging = false;
        if let Some(d) = &self.drag
            && let Some(p) = response.interact_pointer_pos().or(ui.ctx().pointer_latest_pos())
            && let Some(h) = hole_at(p)
        {
            let dcol = h.col() as i32 - d.from.col() as i32;
            // rows count between strip holes (a step across the groove is one row); a rail lead only moves along its rail
            let drow = match (d.from, h) {
                (Hole::Strip { row: a, .. }, Hole::Strip { row: b, .. }) => b as i32 - a as i32,
                _ => 0,
            };
            if let Some(m) = moved(&l, &d.refdes, dcol, drow) {
                if response.drag_stopped() {
                    sch.breadboard = Some(m.clone());
                } else {
                    dragging = true;
                }
                l = m;
            }
        }
        if response.drag_stopped() {
            self.drag = None;
            self.wire_drag = None;
        }
        // clicks of the other tools
        if response.clicked()
            && !on_switch
            && let Some(h) = response.interact_pointer_pos().and_then(hole_at)
        {
            match self.tool {
                Tool::Wire => match self.wire_start.take() {
                    None => self.wire_start = Some(h),
                    Some(a) if a != h => {
                        let m = with_wire(&sch.sch, &l, a, h);
                        sch.breadboard = Some(m.clone());
                        l = m;
                    }
                    Some(_) => {}
                },
                Tool::Meter => {
                    if self.red.is_none() || self.black.is_some() {
                        (self.red, self.black) = (Some(h), None);
                    } else {
                        self.black = Some(h);
                    }
                }
                Tool::Scope => self.scope = Some(h),
                Tool::Move => {}
            }
        }
        let problems = self.board_check(sch, &l);
        // the leads named in a problem, drawn with a red ring
        let mut wrong: BTreeSet<Hole> = BTreeSet::new();
        for p in &problems {
            let names: Vec<String> = match p {
                Problem::Short { at, .. } => at.clone(),
                Problem::Open { apart, .. } => apart.iter().flat_map(|a| a.split(',').map(String::from).collect::<Vec<_>>()).collect(),
                Problem::SameHole { leads, .. } => leads.clone(),
                Problem::Missing { .. } => vec![],
            };
            for n in names {
                let Some((r, pin)) = n.split_once('.') else { continue };
                for part in l.parts.iter().filter(|x| x.refdes == r) {
                    wrong.extend(part.leads.iter().filter(|(p2, _)| p2 == pin).map(|(_, h)| *h));
                }
            }
        }
        // the board
        let board = Rect::from_min_max(origin + Vec2::new(4.3, -0.8) * pitch, origin + Vec2::new(width - 0.3, HEIGHT + 0.8) * pitch);
        painter.rect_filled(board, pitch * 0.4, BOARD);
        painter.rect_filled(Rect::from_min_max(origin + Vec2::new(4.6, 7.6) * pitch, origin + Vec2::new(width - 0.6, 9.4) * pitch), 0.0, Color32::from_rgb(214, 212, 204));
        for (y, c) in [(-0.5, RED), (1.5, BLUE), (14.5, BLUE), (16.5, RED)] {
            painter.line_segment([origin + Vec2::new(4.6, y) * pitch, origin + Vec2::new(width - 0.6, y) * pitch], Stroke::new(1.5, c));
        }
        // the strip (or rail) under the pointer, lit
        let hover = response.hover_pos().and_then(hole_at);
        let lit_node = hover.map(|h| h.node());
        let small = egui::FontId::proportional((pitch * 0.55).clamp(8.0, 12.0));
        for row in 0..10u8 {
            let y = at(Hole::Strip { col: 0, row }).y;
            painter.text(origin + Vec2::new(4.6, y) * pitch, egui::Align2::CENTER_CENTER, ((b'a' + row) as char).to_string(), small.clone(), Color32::from_gray(120));
        }
        for col in 0..l.cols {
            if col % 5 == 4 || col == 0 {
                painter.text(origin + Vec2::new(5.0 + col as f32, 2.0) * pitch, egui::Align2::CENTER_CENTER, (col + 1).to_string(), small.clone(), Color32::from_gray(120));
            }
            let holes = (0..10).map(|row| Hole::Strip { col, row }).chain([Rail::TopPlus, Rail::TopMinus, Rail::BottomMinus, Rail::BottomPlus].map(|rail| Hole::Rail { rail, col }));
            for h in holes {
                let p = to(h);
                let lit = lit_node == Some(h.node());
                if lit {
                    painter.rect_filled(Rect::from_center_size(p, Vec2::splat(pitch * 0.8)), 2.0, LIT.gamma_multiply(0.6));
                }
                painter.rect_filled(Rect::from_center_size(p, Vec2::splat(pitch * 0.32)), 1.0, if lit { Color32::from_gray(20) } else { HOLE });
            }
        }
        for (key, net) in &l.rails {
            let (y, rail) = match key.as_str() {
                "TopPlus" => (0.0, Rail::TopPlus),
                "TopMinus" => (1.0, Rail::TopMinus),
                "BottomMinus" => (15.0, Rail::BottomMinus),
                _ => (16.0, Rail::BottomPlus),
            };
            painter.text(origin + Vec2::new(width - 0.1, y) * pitch, egui::Align2::LEFT_CENTER, net, small.clone(), rail_colour(rail));
        }
        // the wires, then the parts on top
        for (k, w) in l.wires.iter().enumerate() {
            if self.picked_wire == Some(k) {
                draw_wire(&painter, to(w.a), to(w.b), LIT, pitch * 1.8);
            }
            draw_wire(&painter, to(w.a), to(w.b), wire_colour(&w.net, &l), pitch);
        }
        if let (Some(a), Some(p)) = (self.wire_start, response.hover_pos()) {
            draw_wire(&painter, to(a), p, Color32::from_rgb(40, 140, 60).gamma_multiply(0.6), pitch);
        }
        let font = egui::FontId::proportional((pitch * 0.6).clamp(9.0, 14.0));
        for (k, o) in l.off_board.iter().enumerate() {
            let center = origin + Vec2::new(2.0, 3.0 + 4.0 * k as f32) * pitch;
            let r = Rect::from_center_size(center, Vec2::new(3.2, 2.4) * pitch);
            painter.rect(r, 3.0, Color32::from_rgb(60, 64, 72), Stroke::new(1.0, Color32::from_gray(160)), egui::StrokeKind::Middle);
            // a knob turned while it runs shows its value now
            let knob = self.live.as_ref().filter(|_| self.powered).and_then(|v| v.knob_index(&o.refdes).map(|k| v.knobs[k].value));
            let value = match knob {
                Some(v) => format!("{} V", sim_core::format_value(v)),
                None => sch.sch.components.iter().find(|c| c.refdes == o.refdes).map(|c| c.value.clone()).unwrap_or_default(),
            };
            painter.text(center - Vec2::new(0.0, 0.4 * pitch), egui::Align2::CENTER_CENTER, &o.refdes, font.clone(), Color32::WHITE);
            painter.text(center + Vec2::new(0.0, 0.5 * pitch), egui::Align2::CENTER_CENTER, short(&value, 10), small.clone(), Color32::from_gray(200));
            for (i, (pin, net, h)) in o.pins.iter().enumerate() {
                let from = r.right_center() + Vec2::new(0.0, (i as f32 - 0.5) * 0.8 * pitch);
                draw_wire(&painter, from, to(*h), wire_colour(net, &l), pitch);
                painter.text(from + Vec2::new(-0.2 * pitch, 0.0), egui::Align2::RIGHT_CENTER, pin, small.clone(), Color32::WHITE);
            }
        }
        // with the power on: dots along the wires and strips with the current (under the parts)
        if let (true, Some(lv), Some(b)) = (crate::flow_dots::on() && self.powered, self.live.as_ref().filter(|v| v.error.is_none()), self.board.as_ref()) {
            let off_pin = |k: usize, i: usize| {
                let r = Rect::from_center_size(origin + Vec2::new(2.0, 3.0 + 4.0 * k as f32) * pitch, Vec2::new(3.2, 2.4) * pitch);
                r.right_center() + Vec2::new(0.0, (i as f32 - 0.5) * 0.8 * pitch)
            };
            let at_end = |e: flow_dots::End| match e {
                flow_dots::End::Hole(h) => to(h),
                flow_dots::End::Off(k, i) => off_pin(k, i),
            };
            let t = ui.input(|i| i.time);
            let col = Color32::from_rgb(255, 225, 90);
            let mut any = false;
            for f in flow_dots::board_flows(&l, &b.nets, lv) {
                // a wire follows its drawn curve; the strip inside the board is straight
                let line = if f.inside { vec![at_end(f.a), at_end(f.b)] } else { wire_points(at_end(f.a), at_end(f.b), pitch) };
                let dots = crate::flow_dots::dots_along(&line, f.amps, t, pitch);
                any |= !dots.is_empty();
                crate::flow_dots::paint(&painter, &dots, (pitch * 0.14).clamp(1.5, 3.5), if f.inside { col.gamma_multiply(0.55) } else { col });
            }
            if any {
                crate::flow_dots::keep_moving(ui.ctx());
            }
        }
        let focus = focus.map(String::from);
        let mut hovered_part = None;
        // with the power on: the LEDs' currents and the parts past their ratings
        let on = self.powered;
        let running = self.live.as_ref().filter(|v| on && v.error.is_none());
        let currents: Vec<(String, f64)> = match (running, &self.board) {
            (Some(lv), _) => l.parts.iter().filter_map(|p| lv.current(&p.refdes).map(|i| (p.refdes.clone(), i))).collect(),
            (None, Some(Board { dc: Ok((c, _)), .. })) if on => c.clone(),
            _ => vec![],
        };
        let issues = self.issues();
        for p in l.parts.iter().filter(|p| sch.sch.components.iter().any(|c| c.refdes == p.refdes && c.kind == sch_core::Kind::Led)) {
            let i = currents.iter().find(|(n, _)| *n == p.refdes).map_or(0.0, |(_, i)| *i);
            // the eye sees an LED bright at a few mA: the glow follows the square root of the current (1 at 20 mA)
            let glow = (i.max(0.0) / 0.02).sqrt().clamp(0.0, 1.3) as f32;
            if glow > 0.05 && !issues.iter().any(|r| r.refdes == p.refdes && r.level == sim_core::RatingLevel::Over) {
                let c = part_rect(p, to, pitch).center();
                for k in 0..4 {
                    painter.circle_filled(c, pitch * (1.0 + 0.6 * k as f32) * glow.max(0.5), Color32::from_rgb(255, 80, 60).gamma_multiply(0.25 * glow.min(1.0)));
                }
            }
        }
        for p in &l.parts {
            let lit = self.picked.as_deref() == Some(p.refdes.as_str()) || focus.as_deref() == Some(p.refdes.as_str()) || self.drag.as_ref().is_some_and(|d| d.refdes == p.refdes);
            let comp = sch.sch.components.iter().find(|c| c.refdes == p.refdes);
            // a potentiometer turned while it runs shows its knob where it is now
            let turned = comp.filter(|c| c.kind == sch_core::Kind::Pot).and_then(|c| {
                let lv = running?;
                let v = lv.knobs.get(lv.knob_index(&c.refdes)?)?.value;
                let mut c = c.clone();
                c.value = sch_core::with_pot_position(&c.value, v);
                Some(c)
            });
            let comp = turned.as_ref().or(comp);
            let rect = draw_part(&painter, p, comp, to, pitch, &font, lit);
            self.drawn.retain(|(r, _)| *r != p.refdes);
            self.drawn.push((p.refdes.clone(), rect));
            if response.hover_pos().is_some_and(|h| rect.contains(h)) {
                hovered_part = Some((p.refdes.clone(), comp.map(|c| c.value.clone()).unwrap_or_default()));
            }
        }
        if let (Some(lv), Some(b)) = (running, &self.board) {
            let sounding = self.buzzers(sch, &RunBoard::Bread(l.clone()));
            draw_running(&painter, sch, &l, &b.nets, lv, &sounding, to, pitch, &font, ui.input(|i| i.time));
        }
        for r in &issues {
            let Some(p) = l.parts.iter().find(|p| p.refdes == r.refdes) else { continue };
            let rect = part_rect(p, to, pitch);
            if r.level == sim_core::RatingLevel::Over {
                // burnt: soot and a cross
                for k in 0..3 {
                    painter.circle_filled(rect.center() + Vec2::new((k as f32 - 1.0) * pitch * 0.4, -pitch * 0.5 - k as f32 * pitch * 0.2), pitch * (0.5 + 0.15 * k as f32), Color32::from_gray(60).gamma_multiply(0.5));
                }
                let d = pitch * 0.5;
                for (a, b) in [(Vec2::new(-d, -d), Vec2::new(d, d)), (Vec2::new(-d, d), Vec2::new(d, -d))] {
                    painter.line_segment([rect.center() + a, rect.center() + b], Stroke::new(3.0, BAD));
                }
            } else {
                painter.rect_stroke(rect.expand(pitch * 0.2), 3.0, Stroke::new(2.0, crate::theme::warn()), egui::StrokeKind::Middle);
            }
        }
        for h in &wrong {
            painter.circle_stroke(to(*h), pitch * 0.55, Stroke::new(2.5, BAD));
        }
        // what is still to be joined: a dashed line between the nearest leads of a net's separate groups (as a board's ratsnest)
        for (_, a, b) in unjoined(&sch.sch, &l) {
            rats_line(&painter, to(a), to(b), pitch);
        }
        // the probes
        for (h, c, label) in [(self.red, RED, "+"), (self.black, Color32::from_gray(20), "−")] {
            if let (Some(h), Tool::Meter) = (h, self.tool) {
                let p = to(h);
                painter.line_segment([p, p + Vec2::new(pitch * 0.8, -pitch * 1.6)], Stroke::new(pitch * 0.25, c));
                painter.circle_filled(p, pitch * 0.3, c);
                painter.text(p + Vec2::new(pitch * 0.9, -pitch * 1.8), egui::Align2::CENTER_CENTER, label, font.clone(), c);
            }
        }
        if let (Some(h), Tool::Scope) = (self.scope, self.tool) {
            let p = to(h);
            painter.line_segment([p, p + Vec2::new(-pitch * 0.8, -pitch * 1.6)], Stroke::new(pitch * 0.25, LIT));
            painter.circle_filled(p, pitch * 0.3, LIT);
        }
        // what is under the pointer
        if !dragging {
            if let Some((r, v)) = &hovered_part {
                let switch = running.is_some_and(|lv| lv.switch_index(r).is_some());
                let knob = running.is_some_and(|lv| lv.knob_index(r).is_some());
                response.clone().on_hover_text(if switch {
                    i18n::trf!("{r} {v}\nクリックで切り替え (押しボタンは押している間だけ入る)")
                } else if knob {
                    i18n::trf!("{r} {v}\n右か上へドラッグ・ホイールで回します (左の「つまみ」でも)")
                } else {
                    i18n::trf!("{r} {v}\nドラッグで差し替え")
                });
            } else if let Some(h) = hover {
                let here: Vec<String> = l.parts.iter().flat_map(|p| p.leads.iter().filter(|(_, lh)| lh.node() == h.node()).map(move |(pin, _)| format!("{}.{pin}", p.refdes))).collect();
                let tip = if here.is_empty() { h.name() } else { format!("{}: {}", h.name(), here.join(", ")) };
                response.clone().on_hover_text(tip);
            }
        }
        if response.clicked() && self.tool == Tool::Move {
            self.picked = hovered_part.map(|(r, _)| r);
            // a click on a jumper picks it (Delete removes it)
            self.picked_wire = response.interact_pointer_pos().and_then(|p| l.wires.iter().position(|w| wire_points(to(w.a), to(w.b), pitch).iter().any(|q| q.distance(p) < pitch * 0.35)));
        }
        let legend = match self.tool {
            Tool::Move => i18n::tr("黄: ポインタの下の穴とつながっている穴　赤い丸: 回路図と違う足　部品・線の端はドラッグで差し替え (R で回す・Delete で線を消す)"),
            Tool::Wire => i18n::tr("ジャンパー線: 穴をクリックして、つなぐ先の穴をクリック (Esc でやめる)"),
            Tool::Meter => i18n::tr("テスター: 赤い棒 → 黒い棒の順に穴をクリック。電圧は左に出ます"),
            Tool::Scope => i18n::tr("オシロ: 探り針を当てる穴をクリック。波形は左に出ます"),
        };
        painter.text(rect.left_top() + Vec2::new(12.0, 10.0), egui::Align2::LEFT_TOP, legend, egui::FontId::proportional(13.0), Color32::from_gray(210));
    }
}
