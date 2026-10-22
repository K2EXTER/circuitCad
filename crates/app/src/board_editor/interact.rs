//! Mouse interaction on the board: rubber-band selection and what the pointer does for the active tool.

use super::*;

impl BoardEditor {
    pub(super) fn box_selection(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect, painter: &egui::Painter) {
        // rubber-band selection: left drag = fully enclosed, right drag = anything touched
        if self.tool == Tool::Select {
            let origin = ui.input(|i| i.pointer.press_origin());
            for (btn, crossing) in [(egui::PointerButton::Primary, false), (egui::PointerButton::Secondary, true)] {
                if resp.drag_started_by(btn) && self.drag_part.is_none() && self.gdrag.is_none()
                    && let Some(o) = origin {
                        let w0 = self.to_world(rect, o);
                        // a left drag that starts on a part or wire moves it instead (below); the inside
                        // of a copper pour does not count, or a box could not be drawn over one
                        if crossing || self.hit_test(w0).is_none_or(|h| matches!(h, Item::Zone(_)) || self.is_locked_item(&h)) {
                            self.boxsel = Some(BoxSel { start: w0, crossing });
                        }
                    }
            }
            if let Some(b) = &self.boxsel {
                let cur = ui.input(|i| i.pointer.latest_pos()).map(|p| self.to_world(rect, p)).unwrap_or(b.start);
                let r = Rect::from_two_pos(self.to_screen(rect, b.start), self.to_screen(rect, cur));
                let (fill, line) = if b.crossing {
                    (Color32::from_rgba_unmultiplied(60, 200, 90, 40), Color32::from_rgb(80, 220, 110))
                } else {
                    (Color32::from_rgba_unmultiplied(80, 140, 255, 40), Color32::from_rgb(110, 160, 255))
                };
                painter.rect_filled(r, 0.0, fill);
                painter.rect_stroke(r, 0.0, Stroke::new(1.2, line), egui::StrokeKind::Inside);
                if resp.drag_stopped() {
                    let (start, crossing) = (b.start, b.crossing);
                    let mut items = self.items_in_box(start, cur, crossing);
                    if ui.input(|i| i.modifiers.shift) {
                        let mut all = self.multi.clone();
                        all.extend(self.selected);
                        for it in items {
                            if !all.contains(&it) {
                                all.push(it);
                            }
                        }
                        items = all;
                    }
                    self.status = format!("{} 個を選択 ({}範囲)", items.len(), if crossing { "接触した物を含む" } else { "完全に囲んだ" });
                    self.set_selection(items);
                    self.boxsel = None;
                }
            }
        } else {
            self.boxsel = None;
        }
    }

    /// The pad centre (or end of a long pad), via centre or track end close to `w` (within 10 px), for routing and vias to meet what is
    /// already there, whatever the grid: off-grid footprints, and the tracks the auto-router laid.
    pub(super) fn snap_object(&self, w: Point) -> Option<Point> {
        let tol = 10.0 / self.zoom as f64;
        let pads = self.board.pads.iter().filter(|p| p.drill.is_some() || p.side == self.side).map(|p| p.pos);
        let vias = self.board.vias.iter().map(|v| v.pos);
        let ends = self.board.tracks.iter().filter(|t| t.side == self.side).flat_map(|t| [t.a, t.b]);
        // routing also snaps to the two ends of a long pad, so two tracks can leave it side by side
        let pad_ends = if self.tool == Tool::Track { self.pad_end_points() } else { vec![] };
        pads.chain(vias).chain(ends).chain(pad_ends).filter(|c| c.dist(w) <= tol).min_by(|a, b| a.dist(w).total_cmp(&b.dist(w)))
    }

    pub(super) fn pointer(&mut self, resp: &egui::Response, rect: Rect, painter: &egui::Painter) -> Option<Point> {
        let mut sp = None;
        if let Some(pos) = resp.hover_pos() {
            let w = self.to_world(rect, pos);
            let mut p = Point::new(snap(w.x, self.grid), snap(w.y, self.grid));
            if self.tool == Tool::Track {
                // the ends of the pads near the cursor, where the route can snap to
                for e in self.pad_end_points().into_iter().filter(|e| e.dist(w) * (self.zoom as f64) < 60.0) {
                    painter.circle_stroke(self.to_screen(rect, e), 4.0, Stroke::new(1.0, Color32::from_rgba_unmultiplied(90, 230, 120, 170)));
                }
            }
            if matches!(self.tool, Tool::Track | Tool::Via)
                && let Some(c) = self.snap_object(w) {
                    // snapped onto something that is already there: a green ring says so
                    p = c;
                    painter.circle_stroke(self.to_screen(rect, c), 9.0, Stroke::new(2.0, Color32::from_rgb(90, 230, 120)));
                }
            sp = Some(p);
            self.draw_cursor(painter, rect, p);

            if resp.drag_started_by(egui::PointerButton::Primary) && self.tool == Tool::Select && self.multi.len() > 1 && self.boxsel.is_none()
                && self.hit_test(w).is_some_and(|h| self.multi.contains(&h) && !self.is_locked_item(&h)) {
                    self.checkpoint();
                    self.gdrag = Some(GroupDrag { anchor: p, orig: self.board.clone(), items: self.multi.clone() });
                }
            if let Some(g) = self.gdrag.take()
                && resp.dragged_by(egui::PointerButton::Primary) {
                    self.move_group(&g, p.x - g.anchor.x, p.y - g.anchor.y);
                    self.gdrag = Some(g);
                }
            if resp.drag_started_by(egui::PointerButton::Primary) && self.tool == Tool::Select && self.gdrag.is_none() && self.multi.len() <= 1
                && let Some(Item::Part(i)) = self.hit_test(w)
                && !self.is_locked_item(&Item::Part(i)) {
                    self.checkpoint();
                    let part = &self.board.parts[i];
                    self.drag_part = Some((part.refdes.clone(), Point::new(p.x - part.pos.x, p.y - part.pos.y)));
                    self.selected = Some(Item::Part(i));
                    self.multi.clear();
                }
            // a part's designator on the silkscreen: grab it where the pointer pressed, move it freely (not on the grid)
            if resp.drag_started_by(egui::PointerButton::Primary) && self.tool == Tool::Select && self.gdrag.is_none() && self.multi.len() <= 1 && self.boxsel.is_none()
                && let Some(o) = resp.ctx.input(|i| i.pointer.press_origin())
                && let w0 = self.to_world(rect, o)
                && let Some(Item::Label(i)) = self.hit_test(w0)
                && let Some(t) = self.board.texts.get(i) {
                    let grab = Point::new(w0.x - t.pos.x, w0.y - t.pos.y);
                    self.drag_label = Some((t.refdes.clone(), grab));
                    self.selected = Some(Item::Label(i));
                    self.multi.clear();
                    self.checkpoint();
                }
            if let Some((r, grab)) = self.drag_label.clone()
                && resp.dragged_by(egui::PointerButton::Primary) {
                    self.board.move_label(&r, Point::new(w.x - grab.x, w.y - grab.y));
                }
            if resp.drag_started_by(egui::PointerButton::Primary) && self.tool == Tool::Select && self.gdrag.is_none() && self.multi.len() <= 1 && self.boxsel.is_none()
                // the pointer has already moved a few pixels when the drag is recognised: look where it was pressed
                && let Some(o) = resp.ctx.input(|i| i.pointer.press_origin())
                && let w0 = self.to_world(rect, o)
                && let Some(Item::Track(i)) = self.hit_test(w0) {
                    self.start_track_drag(i, Point::new(snap(w0.x, self.grid), snap(w0.y, self.grid)));
                }
            if self.seg_drag.is_some() && resp.dragged_by(egui::PointerButton::Primary) {
                self.update_track_drag(p);
            }
            if let Some((r, off)) = self.drag_part.clone()
                && resp.dragged_by(egui::PointerButton::Primary)
                    && let Some(rot) = self.board.parts.iter().find(|q| q.refdes == r).map(|q| q.rot) {
                        self.board.place_part(&r, Point::new(p.x - off.x, p.y - off.y), rot);
                    }
            if resp.clicked_by(egui::PointerButton::Primary) {
                self.click(p);
            }
            if resp.double_clicked() && matches!(self.tool, Tool::OutlinePoly | Tool::Zone) {
                self.finish_polygon();
            }
            let layer = self.side.label();
            self.status = format!("X {:.2}  Y {:.2} mm   [{layer}]", p.x, p.y);
        }
        if resp.drag_stopped() {
            self.finish_track_drag();
            self.drag_part = None;
            self.drag_label = None;
            self.gdrag = None;
        }
        if resp.clicked_by(egui::PointerButton::Secondary) {
            self.leave_tool();
        }
        sp
    }

    /// The right click, AutoCAD style: it confirms what is being drawn (ends the track; closes a
    /// polygon of three or more points, drops a shorter one) and the tool stays; with nothing in
    /// progress it returns to the Select tool. Returns whether anything happened.
    pub(super) fn leave_tool(&mut self) -> bool {
        if self.pending.take().is_some() {
            self.status = "配線を確定しました (もう一度右クリックで選択ツールに戻ります)".into();
            return true;
        }
        if !self.poly.is_empty() {
            if self.poly.len() >= 3 && matches!(self.tool, Tool::OutlinePoly | Tool::Zone) {
                self.finish_polygon();
            } else {
                self.poly.clear();
                self.status = "多角形の入力を取り消しました".into();
            }
            return true;
        }
        if self.tool == Tool::Select {
            return false;
        }
        self.tool = Tool::Select;
        self.status = "選択ツールに戻りました".into();
        true
    }
}
