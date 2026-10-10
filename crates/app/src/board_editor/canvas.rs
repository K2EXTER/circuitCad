//! The drawing area: the order in which the board is drawn and handled, and the keyboard shortcuts.

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

        self.pan_and_zoom(ui, &resp, rect);

        self.draw_grid(&painter, rect);
        let conn = (!self.board.zones.is_empty() || self.selected.is_some()).then(|| connectivity(&self.board));
        self.draw_zones(&painter, rect, conn.as_ref());
        self.draw_copper(&painter, rect);
        self.draw_highlight(&painter, rect, conn.as_ref());
        self.draw_overlays(&painter, rect);
        self.draw_layer_badge(&painter, rect);

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
