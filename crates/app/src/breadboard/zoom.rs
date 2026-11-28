//! Zooming and moving the board pictures (the breadboard, the stripboard, the perfboard): each picture works out the hole spacing and
//! the place that fit the whole board in the window, and its view (`ZoomPan`, one per kind of board) scales and shifts that. As on the
//! schematic: the wheel zooms about the pointer, a drag with the middle button (or Alt + the left) moves the picture, and so does the
//! right button in the tools that do not pick with it (the move tool draws a box with it); Home or the button at the bottom right goes
//! back to the whole board.

use super::*;

/// The farthest in and out from the whole board.
const ZOOM_IN: f32 = 8.0;
const ZOOM_OUT: f32 = 0.5;

/// A board picture's view: how much bigger than the fitted picture, and how far moved (screen points).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoomPan {
    pub zoom: f32,
    pub pan: Vec2,
    /// A left drag begun with Alt held: it moves the picture until let go.
    alt_pan: bool,
}

impl Default for ZoomPan {
    fn default() -> Self {
        Self { zoom: 1.0, pan: Vec2::ZERO, alt_pan: false }
    }
}

impl ZoomPan {
    /// The hole spacing and the origin of the view, from those that fit the board in `rect`.
    pub fn apply(&self, rect: Rect, pitch: f32, origin: Pos2) -> (f32, Pos2) {
        let c = rect.center();
        (pitch * self.zoom, c + (origin - c) * self.zoom + self.pan)
    }

    /// Is it the whole board as fitted?
    pub fn is_fit(&self) -> bool {
        (self.zoom - 1.0).abs() < 1e-4 && self.pan.length() < 0.5
    }

    /// The wheel (when `wheel` may zoom: not while it turns a knob), the middle drag, Alt + the left drag, the right drag (when
    /// `right_pans`: the tool does not pick with it) and Home. Applies from the next frame.
    pub fn input(&mut self, ui: &egui::Ui, response: &egui::Response, rect: Rect, wheel: bool, right_pans: bool) {
        if response.drag_started_by(egui::PointerButton::Primary) {
            self.alt_pan = ui.input(|i| i.modifiers.alt);
        }
        if !response.dragged_by(egui::PointerButton::Primary) {
            self.alt_pan = false;
        }
        if response.dragged_by(egui::PointerButton::Middle) || (right_pans && response.dragged_by(egui::PointerButton::Secondary)) || self.alt_pan {
            self.pan += response.drag_delta();
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        if !response.hovered() {
            return;
        }
        let scroll = if wheel { crate::prefs::zoom_scroll(ui) } else { 0.0 };
        if scroll != 0.0 {
            let p = response.hover_pos().unwrap_or(rect.center());
            self.zoom_about(rect, p, 1.0 + scroll * 0.002);
        }
        if !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.key_pressed(egui::Key::Home)) {
            *self = Self::default();
        }
    }

    /// Zoom by `factor`, the point under `p` staying where it is.
    pub fn zoom_about(&mut self, rect: Rect, p: Pos2, factor: f32) {
        let new = (self.zoom * factor).clamp(ZOOM_OUT, ZOOM_IN);
        let f = new / self.zoom;
        // a point s of the picture is c + (o - c)·z + pan; keeping s = p after z → z·f gives pan' = (p - c)(1 - f) + pan·f
        let c = rect.center();
        self.pan = (p - c) * (1.0 - f) + self.pan * f;
        self.zoom = new;
    }

    /// The buttons at the bottom right of the picture: zoom out, in, and the whole board (only when zoomed or moved).
    pub fn buttons(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let size = Vec2::new(150.0, 26.0);
        let area = Rect::from_min_size(rect.right_bottom() - size - Vec2::new(10.0, 10.0), size);
        ui.scope_builder(egui::UiBuilder::new().max_rect(area).layout(egui::Layout::right_to_left(egui::Align::Center)), |ui| {
            if !self.is_fit() && ui.button(i18n::tr("全体")).on_hover_text(i18n::tr("基板の全体が収まる大きさと位置に戻します (Home)")).clicked() {
                *self = Self::default();
            }
            if ui.button(i18n::tr("＋")).on_hover_text(i18n::tr("拡大します (ホイールでも。ポインタの所を中心に)")).clicked() {
                self.zoom_about(rect, rect.center(), 1.25);
            }
            if ui.button(i18n::tr("－")).on_hover_text(i18n::tr("縮小します (ホイールでも)。絵を動かすのは、中ボタンか Alt + 左ボタンのドラッグ")).clicked() {
                self.zoom_about(rect, rect.center(), 0.8);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zooming_keeps_the_point_under_the_pointer_and_home_fits_again() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
        let (pitch, origin) = (10.0, Pos2::new(100.0, 80.0));
        let mut v = ZoomPan::default();
        assert_eq!(v.apply(rect, pitch, origin), (pitch, origin));
        // the hole 5 across, 3 down, under the pointer
        let hole = Vec2::new(5.0, 3.0);
        let p = origin + hole * pitch;
        v.zoom_about(rect, p, 2.0);
        let (p2, o2) = v.apply(rect, pitch, origin);
        assert_eq!(p2, 20.0);
        assert!((o2 + hole * p2).distance(p) < 1e-3, "the hole stays under the pointer");
        v.zoom_about(rect, p, 100.0);
        assert_eq!(v.zoom, ZOOM_IN, "not past the limit");
        assert!(!v.is_fit());
        assert!(ZoomPan::default().is_fit());
    }
}
