//! The drawing area: input handling and keyboard shortcuts.

use super::*;

impl BoardEditor {
    pub fn canvas(&mut self, ui: &mut egui::Ui) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = resp.rect;
        painter.rect_filled(rect, 0.0, BG);
        if self.fit_pending {
            self.fit_pending = false;
            self.fit_view(rect);
        }

        // pan (middle/right drag) and zoom (wheel, around the cursor)
        // in the Select tool the right button draws a crossing selection box instead of panning
        let rmb_selects = self.tool == Tool::Select;
        if resp.dragged_by(egui::PointerButton::Middle) || (resp.dragged_by(egui::PointerButton::Secondary) && !rmb_selects) {
            let d = resp.drag_delta();
            self.pan.x -= d.x / self.zoom;
            self.pan.y += d.y / self.zoom;
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let cursor = ui.input(|i| i.pointer.hover_pos()).unwrap_or(rect.center());
                let before = self.to_world(rect, cursor);
                self.zoom = (self.zoom * (1.0 + scroll * 0.002)).clamp(1.0, 400.0);
                let after = self.to_world(rect, cursor);
                self.pan.x += (before.x - after.x) as f32;
                self.pan.y += (before.y - after.y) as f32;
            }
        }

        self.draw_grid(&painter, rect);
        let conn = (!self.board.zones.is_empty() || self.selected.is_some()).then(|| connectivity(&self.board));
        self.draw_zones(&painter, rect, conn.as_ref());
        self.draw_copper(&painter, rect);
        self.draw_highlight(&painter, rect, conn.as_ref());
        self.draw_overlays(&painter, rect);

        // rubber-band selection: left drag = fully enclosed, right drag = anything touched
        if self.tool == Tool::Select {
            let origin = ui.input(|i| i.pointer.press_origin());
            for (btn, crossing) in [(egui::PointerButton::Primary, false), (egui::PointerButton::Secondary, true)] {
                if resp.drag_started_by(btn) && self.drag_part.is_none() && self.gdrag.is_none()
                    && let Some(o) = origin {
                        let w0 = self.to_world(rect, o);
                        // a left drag that starts on a part or wire moves it instead (below); the inside
                        // of a copper pour does not count, or a box could not be drawn over one
                        if crossing || self.hit_test(w0).is_none_or(|h| matches!(h, Item::Zone(_))) {
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

        // cursor handling
        let typing = ui.ctx().egui_wants_keyboard_input();
        let mut sp = None;
        if let Some(pos) = resp.hover_pos() {
            let w = self.to_world(rect, pos);
            let mut p = Point::new(snap(w.x, self.grid), snap(w.y, self.grid));
            if matches!(self.tool, Tool::Track) {
                // snap routing to pad / via centres so off-grid footprints can be connected
                let tol = 10.0 / self.zoom as f64;
                let near = self
                    .board
                    .pads
                    .iter()
                    .map(|p| p.pos)
                    .chain(self.board.vias.iter().map(|v| v.pos))
                    .filter(|c| c.dist(w) <= tol)
                    .min_by(|a, b| a.dist(w).total_cmp(&b.dist(w)));
                if let Some(c) = near {
                    p = c;
                }
            }
            sp = Some(p);
            self.draw_cursor(&painter, rect, p);

            if resp.drag_started_by(egui::PointerButton::Primary) && self.tool == Tool::Select && self.multi.len() > 1 && self.boxsel.is_none()
                && self.hit_test(w).is_some_and(|h| self.multi.contains(&h)) {
                    self.checkpoint();
                    self.gdrag = Some(GroupDrag { anchor: p, orig: self.board.clone(), items: self.multi.clone() });
                }
            if let Some(g) = self.gdrag.take()
                && resp.dragged_by(egui::PointerButton::Primary) {
                    self.move_group(&g, p.x - g.anchor.x, p.y - g.anchor.y);
                    self.gdrag = Some(g);
                }
            if resp.drag_started_by(egui::PointerButton::Primary) && self.tool == Tool::Select && self.gdrag.is_none() && self.multi.len() <= 1
                && let Some(Item::Part(i)) = self.hit_test(w) {
                    self.checkpoint();
                    let part = &self.board.parts[i];
                    self.drag_part = Some((part.refdes.clone(), Point::new(p.x - part.pos.x, p.y - part.pos.y)));
                    self.selected = Some(Item::Part(i));
                    self.multi.clear();
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
            let layer = if self.side == Side::Top { "表面" } else { "裏面" };
            self.status = format!("X {:.2}  Y {:.2} mm   [{layer}]", p.x, p.y);
        }
        if resp.drag_stopped() {
            self.drag_part = None;
            self.gdrag = None;
        }
        if resp.clicked_by(egui::PointerButton::Secondary) {
            self.pending = None;
            self.poly.clear();
        }

        if !typing {
            self.handle_keys(ui, sp);
        }
    }

    pub(super) fn handle_keys(&mut self, ui: &egui::Ui, cursor: Option<Point>) {
        use egui::Key;
        let (cmd, shift) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
        let pressed = |k: Key| ui.input(|i| i.key_pressed(k));
        if cmd && pressed(Key::Z) {
            if shift { self.redo() } else { self.undo() }
            return;
        }
        if cmd && pressed(Key::Y) {
            self.redo();
            return;
        }
        if pressed(Key::Delete) || pressed(Key::Backspace) {
            if !self.multi.is_empty() {
                self.delete_multi();
            } else if let Some(it) = self.selected {
                self.delete(it);
            }
        }
        if pressed(Key::Escape) {
            self.pending = None;
            self.poly.clear();
            self.multi.clear();
            self.boxsel = None;
        }
        if pressed(Key::Enter) && matches!(self.tool, Tool::OutlinePoly | Tool::Zone) {
            self.finish_polygon();
        }
        if pressed(Key::L) {
            self.side = self.side.flip();
        }
        if pressed(Key::F) {
            self.fit_pending = true;
        }
        if pressed(Key::Space) {
            self.bend_flip = !self.bend_flip;
        }
        let grouped_parts: Vec<String> = self.multi.iter().filter_map(|it| if let Item::Part(i) = it { self.board.parts.get(*i).map(|p| p.refdes.clone()) } else { None }).collect();
        if pressed(Key::B) {
            if grouped_parts.is_empty() {
                self.flip_selected_part();
            } else {
                self.checkpoint();
                for r in &grouped_parts {
                    self.board.flip_part(r);
                }
            }
        }
        if pressed(Key::R) {
            if let Some(Item::Part(i)) = self.selected {
                self.checkpoint();
                let p = self.board.parts[i].clone();
                self.board.place_part(&p.refdes, p.pos, (p.rot + 1) % 4);
            } else if !grouped_parts.is_empty() {
                self.checkpoint();
                for r in &grouped_parts {
                    if let Some(p) = self.board.parts.iter().find(|p| &p.refdes == r).cloned() {
                        self.board.place_part(&p.refdes, p.pos, (p.rot + 1) % 4);
                    }
                }
            }
        }
        // V while routing: run to the cursor, drop a via and continue on the other layer
        if pressed(Key::V) && self.tool == Tool::Track
            && let (Some(a), Some(p)) = (self.pending, cursor) {
                self.route_via(a, p);
            }
    }
}
