//! Mouse interaction on the sheet: rubber-band selection, dragging parts and clicks of the active tool.

use super::*;

impl SchEditor {
    pub(super) fn box_selection(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect, painter: &egui::Painter) {
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
    }

    pub(super) fn handle_drags(&mut self, resp: &egui::Response, g: (f32, f32), snapped: Pt) {
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
    }

    pub(super) fn handle_clicks(&mut self, ui: &egui::Ui, resp: &egui::Response, g: (f32, f32), snapped: Pt) {
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
            let was_drawing = self.pending.take().is_some();
            // right-clicking a part (when not in the middle of a wire) opens its settings window
            if !was_drawing
                && matches!(self.tool, Tool::Select)
                && !self.noise_pick
                && let Some(i) = self.comp_at(g)
                && let Some(pos) = resp.interact_pointer_pos()
            {
                self.multi.clear();
                self.selected = Some(Sel::Comp(i));
                self.popup_serial += 1;
                self.part_popup = Some((i, pos, self.popup_serial));
            }
        }
    }
}
