//! The drawing area: view, keyboard and the order in which the sheet is drawn and handled.

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

        let hover = resp.hover_pos();
        self.pan_and_zoom(ui, &resp, rect, hover);
        self.handle_keys(ui);

        self.draw_grid(&painter, rect);
        self.draw_wires(&painter, rect);
        self.draw_junctions(&painter, rect);
        self.draw_components(&painter, rect);
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
            self.pick_probe = false;
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
    }
}
