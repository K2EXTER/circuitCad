//! The drawing area: the order in which the board is drawn and handled, and the keyboard shortcuts.

use super::*;

impl BoardEditor {
    pub fn canvas(&mut self, ui: &mut egui::Ui) {
        // with the button up, the next edit of a panel value is a new undo step
        if !ui.input(|i| i.pointer.any_down()) {
            self.drag_edit = false;
        }
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = resp.rect;
        painter.rect_filled(rect, 0.0, BG);
        if self.fit_pending {
            self.fit_pending = false;
            self.fit_view(rect);
        }

        self.pan_and_zoom(ui, &resp, rect);

        self.draw_grid(&painter, rect);
        let conn = (!self.board.zones.is_empty() || self.selected.is_some()).then(|| self.cached_connectivity());
        self.draw_zones(&painter, rect, conn.as_deref());
        self.update_net_focus(&resp, rect);
        self.draw_copper(&painter, rect);
        self.draw_net_focus(&painter, rect);
        self.draw_highlight(&painter, rect, conn.as_deref());
        self.draw_overlays(&painter, rect);
        self.draw_layer_badge(&painter, rect);
        self.layer_overlay(ui, rect);

        if !self.alt_pan {
            self.box_selection(ui, &resp, rect, &painter);
        }
        let typing = ui.ctx().egui_wants_keyboard_input();
        // while the view is dragged with Alt, nothing under the pointer is picked up or moved
        let sp = if self.alt_pan { None } else { self.pointer(&resp, rect, &painter) };
        if !typing {
            self.handle_keys(ui, sp);
        }
        self.draw_rulers(&painter, rect, ui.input(|i| i.pointer.hover_pos()));
    }

    pub(super) fn pan_and_zoom(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect) {
        // pan (middle/right drag) and zoom (wheel, around the cursor)
        // in the Select tool the right button draws a crossing selection box instead of panning
        let rmb_selects = self.tool == Tool::Select;
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
        // number keys pick a tool (1-9, 0), in the order of the toolbar
        if !cmd && !shift && !ui.input(|i| i.modifiers.alt) {
            let digits = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9, Key::Num0];
            if let Some(k) = digits.iter().position(|k| pressed(*k))
                && let Some((t, _, _)) = TOOLS.get(k)
            {
                self.set_tool(*t);
                return;
            }
        }
        // R, B and Delete act on what is selected, and only the Select tool selects: in another tool they do nothing
        // to a part or a track picked earlier
        let selecting = self.tool == Tool::Select;
        if selecting && (pressed(Key::Delete) || pressed(Key::Backspace)) {
            if !self.multi.is_empty() {
                self.delete_multi();
            } else if let Some(it) = self.selected {
                self.delete(it);
            }
        }
        if pressed(Key::F11) {
            self.toggle_tracking();
        }
        if pressed(Key::Escape) {
            self.clear_tracking();
            self.pending = None;
            self.arc_mid = None;
            self.poly.clear();
            self.multi.clear();
            self.boxsel = None;
        }
        if pressed(Key::Enter) && matches!(self.tool, Tool::OutlinePoly | Tool::Zone | Tool::ZoneHole) {
            self.finish_polygon();
        }
        if pressed(Key::L) {
            self.side = self.next_layer();
        }
        if pressed(Key::F) {
            self.fit_pending = true;
        }
        if pressed(Key::Space) {
            self.bend_flip = !self.bend_flip;
        }
        let grouped_parts: Vec<String> = self
            .multi
            .iter()
            .filter_map(|it| if let Item::Part(i) = it { self.board.parts.get(*i).filter(|p| !self.board.is_locked(&p.refdes)).map(|p| p.refdes.clone()) } else { None })
            .collect();
        if selecting && pressed(Key::B) {
            if grouped_parts.is_empty() {
                self.flip_selected_part();
            } else {
                self.checkpoint();
                for r in &grouped_parts {
                    self.board.flip_part(r);
                }
            }
        }
        if selecting && pressed(Key::R) {
            if let Some(Item::Part(i)) = self.selected {
                if self.board.is_locked(&self.board.parts[i].refdes) {
                    self.status = "この部品は固定されています。固定を外すと回せます (選択パネルのボタン)".into();
                } else {
                    self.checkpoint();
                    let p = self.board.parts[i].clone();
                    self.board.place_part(&p.refdes, p.pos, (p.rot + 1) % 4);
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn press(e: &mut BoardEditor, key: egui::Key) {
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: egui::Modifiers::NONE });
        let mut out = ctx.run_ui(input, |ui| e.handle_keys(ui, None));
        out.textures_delta.clear();
    }

    fn editor() -> BoardEditor {
        let mut e = BoardEditor::default();
        e.board = pcb_core::Board::new("t", 40.0, 30.0);
        e.board.set_part(pcb_core::Part { refdes: "R1".into(), value: "1k".into(), footprint: "R_0805".into(), pos: pcb_core::Point::new(10.0, 10.0), rot: 0, side: pcb_core::Side::Top }).unwrap();
        e
    }

    /// The number keys pick the tools in the order of the toolbar (1 = select ... 0 = silk), and the toolbar shows them.
    #[test]
    fn number_keys_pick_tools() {
        let mut e = editor();
        press(&mut e, egui::Key::Num2);
        assert!(e.tool == Tool::Track);
        press(&mut e, egui::Key::Num0);
        assert!(e.tool == Tool::Silk);
        press(&mut e, egui::Key::Num1);
        assert!(e.tool == Tool::Select);
        assert_eq!((tool_key(Tool::Select), tool_key(Tool::Silk)), (Some('1'), Some('0')));
    }

    /// R, B and Delete reach a selected part or track only from the Select tool, so a part picked earlier is
    /// not turned, flipped or deleted while another tool is at work.
    #[test]
    fn r_b_and_delete_only_act_in_the_select_tool() {
        let mut e = editor();
        e.tool = Tool::Select;
        e.selected = Some(Item::Part(0));
        press(&mut e, egui::Key::R);
        assert_eq!(e.board.parts[0].rot, 1, "R turns the selected part in the Select tool");
        press(&mut e, egui::Key::B);
        assert_eq!(e.board.parts[0].side, pcb_core::Side::Bottom, "B turns it over");
        press(&mut e, egui::Key::B);
        // the same keys in another tool, with the selection still there (a script or an old state)
        e.tool = Tool::Track;
        e.selected = Some(Item::Part(0));
        press(&mut e, egui::Key::R);
        press(&mut e, egui::Key::B);
        assert_eq!((e.board.parts[0].rot, e.board.parts[0].side), (1, pcb_core::Side::Top));
        // a track selected earlier is not deleted from another tool either
        e.board.tracks.push(pcb_core::Track { a: pcb_core::Point::new(1.0, 1.0), b: pcb_core::Point::new(5.0, 1.0), width: 0.25, side: pcb_core::Side::Top });
        e.selected = Some(Item::Track(0));
        press(&mut e, egui::Key::Delete);
        assert_eq!(e.board.tracks.len(), 1);
        e.tool = Tool::Select;
        press(&mut e, egui::Key::Delete);
        assert!(e.board.tracks.is_empty(), "Delete removes it in the Select tool");
    }
}
