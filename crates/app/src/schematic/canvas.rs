//! The drawing area: view, keyboard and the order in which the sheet is drawn and handled.

use super::*;

impl SchEditor {
    pub fn canvas(&mut self, ui: &mut egui::Ui) {
        self.track_history(ui);
        self.live_update();
        self.sheet_tabs(ui);
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

        let hover = resp.hover_pos();
        self.pan_and_zoom(ui, &resp, rect, hover);
        self.handle_keys(ui);
        self.quick_add_ui(ui);

        self.draw_grid(&painter, rect);
        self.draw_wires(&painter, rect);
        self.draw_junctions(&painter, rect);
        self.draw_components(&painter, rect);
        self.draw_requirement_frames(&painter, rect);
        self.draw_labels(&painter, rect);
        self.draw_voltages(&painter, rect);
        self.draw_currents(&painter, rect);
        self.draw_problems(&painter, rect);
        self.draw_noise_overlay(&painter, rect);
        if self.alt_pan {
            return; // the drag moves the view: no selection box, no moving of parts
        }
        self.box_selection(ui, &resp, rect, &painter);

        // interaction
        let Some(pos) = hover else { return };
        let g = self.to_grid(rect, pos);
        let snapped: Pt = (g.0.round() as i32, g.1.round() as i32);
        let sp = self.to_screen(rect, (snapped.0 as f32, snapped.1 as f32));
        self.draw_noise_hover(&painter, snapped, sp);
        self.draw_tool_ghost(&painter, rect, snapped, sp);
        if !self.noise_pick {
            self.status = format!("({}, {})  部品 {}  配線 {}", snapped.0, snapped.1, self.sch.components.len(), self.sch.wires.len());
        }
        self.handle_drags(&resp, g, snapped);
        self.handle_clicks(ui, &resp, g, snapped);
    }

    pub(super) fn pan_and_zoom(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect, hover: Option<Pos2>) {
        // in the Select tool the right button draws a crossing selection box instead of panning
        let rmb_selects = self.tool == Tool::Select && !self.noise_pick;
        // Alt + left drag pans as well (for mice without a usable wheel button)
        if resp.drag_started_by(egui::PointerButton::Primary) {
            self.alt_pan = ui.input(|i| i.modifiers.alt);
        }
        if !resp.dragged_by(egui::PointerButton::Primary) {
            self.alt_pan = false;
        }
        if resp.dragged_by(egui::PointerButton::Middle) || (resp.dragged_by(egui::PointerButton::Secondary) && !rmb_selects) || self.alt_pan {
            let d = resp.drag_delta();
            self.pan.x -= d.x / self.zoom;
            self.pan.y -= d.y / self.zoom;
        }
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
    }

    pub(super) fn handle_keys(&mut self, ui: &egui::Ui) {
        let typing = ui.ctx().egui_wants_keyboard_input();
        if self.noise_pick && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.noise_pick = false;
            self.z_pick = false;
            self.loop_pick = false;
            self.pick_probe = false;
            self.status = "ノードの選択を中止しました".into();
        }
        if let Some(i) = self.rotate_req.take() {
            self.rotate_comp(i);
        }
        // choosing a part to place puts the selection away, so R (and Delete) cannot hit a part picked earlier
        if self.tool == Tool::Place {
            if !self.place_cleared {
                self.place_cleared = true;
                self.selected = None;
                self.multi.clear();
            }
        } else {
            self.place_cleared = false;
        }
        if !typing {
            let (cmd, shift) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
            if cmd && ui.input(|i| i.key_pressed(egui::Key::Z)) {
                if shift { self.redo() } else { self.undo() }
            } else if cmd && ui.input(|i| i.key_pressed(egui::Key::Y)) {
                self.redo();
            }
            // egui turns Ctrl+C / Ctrl+X / Ctrl+V into these events (no key press), and the text of the system clipboard is not used
            let (copy, cut, paste) = ui.input(|i| {
                let any = |f: fn(&egui::Event) -> bool| i.events.iter().any(f);
                (any(|e| matches!(e, egui::Event::Copy)), any(|e| matches!(e, egui::Event::Cut)), any(|e| matches!(e, egui::Event::Paste(_))))
            });
            if copy {
                self.copy_selection();
            } else if cut {
                self.cut_selection();
            } else if paste {
                self.paste_clipboard();
            } else if cmd && ui.input(|i| i.key_pressed(egui::Key::D)) {
                self.duplicate_selection();
            }
            if ui.input(|i| i.key_pressed(egui::Key::R)) {
                match self.selected {
                    // placing: R turns the part about to be placed, never a part on the sheet
                    _ if self.tool == Tool::Place => self.rot = (self.rot + 1) % 4,
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
            if ui.input(|i| i.key_pressed(egui::Key::A) && !i.modifiers.command) {
                self.open_quick_add();
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(e: &mut SchEditor, key: egui::Key) {
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: egui::Modifiers::NONE });
        let mut out = ctx.run_ui(input, |ui| e.handle_keys(ui));
        out.textures_delta.clear();
    }

    fn send(e: &mut SchEditor, events: Vec<egui::Event>) {
        send_with(e, events, egui::Modifiers::NONE);
    }

    /// One frame with these events and the modifier keys held (egui takes them from a `ModifiersChanged` event, not from the key event).
    fn send_with(e: &mut SchEditor, events: Vec<egui::Event>, modifiers: egui::Modifiers) {
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::ModifiersChanged(modifiers));
        input.events.extend(events);
        let mut out = ctx.run_ui(input, |ui| e.handle_keys(ui));
        out.textures_delta.clear();
    }

    /// Ctrl+C, Ctrl+X and Ctrl+V reach egui as events (not as key presses): the editor has to listen for them.
    #[test]
    fn the_copy_cut_and_paste_events_and_ctrl_d_drive_the_clipboard() {
        let mut e = SchEditor::default();
        e.sch = Schematic::default();
        let r = e.sch.add(Kind::Resistor, (10, 10), 0);
        e.tool = Tool::Select;
        e.selected = Some(Sel::Comp(r));
        send(&mut e, vec![egui::Event::Copy]);
        assert_eq!(e.clipboard.components.len(), 1);
        send(&mut e, vec![egui::Event::Paste(String::new())]);
        assert_eq!(e.sch.components.len(), 2, "pasted");
        // the pasted part is the selection: Ctrl+X takes it off again, Ctrl+V brings it back
        send(&mut e, vec![egui::Event::Cut]);
        assert_eq!(e.sch.components.len(), 1, "cut");
        send(&mut e, vec![egui::Event::Paste("something from another program".into())]);
        assert_eq!(e.sch.components.len(), 2, "the text on the system clipboard is not what is pasted");
        // Ctrl+D copies and pastes in one go
        e.selected = Some(Sel::Comp(r));
        let ctrl = egui::Modifiers::COMMAND;
        send_with(&mut e, vec![egui::Event::Key { key: egui::Key::D, physical_key: Some(egui::Key::D), pressed: true, repeat: false, modifiers: ctrl }], ctrl);
        assert_eq!(e.sch.components.len(), 3, "duplicated");
    }

    /// Choosing a part to place puts the selection away, and R then turns the part about to be placed,
    /// not a part picked earlier.
    #[test]
    fn r_in_the_place_tool_does_not_turn_a_selected_part() {
        let mut e = SchEditor::default();
        let r = e.sch.add(Kind::Resistor, (10, 10), 0);
        // in the Select tool R turns the selected part
        e.tool = Tool::Select;
        e.selected = Some(Sel::Comp(r));
        press(&mut e, egui::Key::R);
        assert_eq!(e.sch.components[r].rot, 1);
        // choosing a part to place clears the selection
        e.choose_library_part(0);
        press(&mut e, egui::Key::A); // any frame in the Place tool
        assert!(e.selected.is_none() && e.multi.is_empty());
        // even with something selected again, R turns the pending part only
        e.selected = Some(Sel::Comp(r));
        let before = e.rot;
        press(&mut e, egui::Key::R);
        assert_eq!(e.sch.components[r].rot, 1, "the part on the sheet stays");
        assert_eq!(e.rot, (before + 1) % 4);
    }
}
