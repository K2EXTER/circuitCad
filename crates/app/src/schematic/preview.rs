//! The sheet drawn read-only into any space (the proto tab's small window): fitted to it, the part picked elsewhere ringed, a click
//! on a part tells which one.

use super::*;

/// Room round the parts (grid units) when the sheet is fitted.
const MARGIN: f32 = 4.0;

impl SchEditor {
    /// The corners of what is on the shown sheet (grid units): x0, x1, y0, y1; None when it is empty.
    pub(super) fn sheet_extent(&self) -> Option<(f32, f32, f32, f32)> {
        let mut pts: Vec<(f32, f32)> = vec![];
        for c in &self.sch.components {
            let (x0, x1, y0, y1) = comp_extents(c);
            for corner in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)] {
                let r = rot_f(corner, c.rot);
                pts.push((c.pos.0 as f32 + r.0, c.pos.1 as f32 + r.1));
            }
        }
        pts.extend(self.sch.wires.iter().flat_map(|w| [w.a, w.b]).map(|p| (p.0 as f32, p.1 as f32)));
        pts.extend(self.sch.buses.iter().flat_map(|w| [w.a, w.b]).map(|p| (p.0 as f32, p.1 as f32)));
        pts.extend(self.sch.labels.iter().map(|l| (l.pos.0 as f32, l.pos.1 as f32)));
        pts.extend(self.sch.ports.iter().map(|p| (p.pos.0 as f32, p.pos.1 as f32)));
        if pts.is_empty() {
            return None;
        }
        let x0 = pts.iter().map(|p| p.0).fold(f32::MAX, f32::min);
        let x1 = pts.iter().map(|p| p.0).fold(f32::MIN, f32::max);
        let y0 = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min);
        let y1 = pts.iter().map(|p| p.1).fold(f32::MIN, f32::max);
        Some((x0, x1, y0, y1))
    }

    /// A sheet fitted into `ui`'s space, read-only, the part picked elsewhere (`linked`) ringed. With several sheets a row of tabs
    /// picks the sheet (the one being edited at first), and a part picked elsewhere that is on another sheet turns the window to it.
    /// Returns the part clicked.
    pub fn preview(&mut self, ui: &mut egui::Ui) -> Option<String> {
        let count = self.sch.sheets.len();
        if count < 2 {
            self.preview_sheet = None;
            return self.preview_sheet_now(ui);
        }
        // a part newly picked on the board (or the 3D view) that lies on another sheet: show that sheet
        let picked = self.linked.part.clone();
        if picked != self.preview_followed {
            if let Some(k) = picked.as_deref().and_then(|r| self.sheet_with_part(r)) {
                self.preview_sheet = Some(k);
            }
            self.preview_followed = picked;
        }
        let active = self.sch.active_sheet;
        let shown = self.preview_sheet.filter(|k| *k < count).unwrap_or(active);
        ui.horizontal_wrapped(|ui| {
            for k in 0..count {
                let tip = if k == active { i18n::tr("このシートを小窓に出します (回路図の画面で開いているシート)") } else { i18n::tr("このシートを小窓に出します") };
                if ui.selectable_label(k == shown, self.sch.sheet_name(k)).on_hover_text(tip).clicked() {
                    self.preview_sheet = Some(k);
                }
            }
        });
        let shown = self.preview_sheet.filter(|k| *k < count).unwrap_or(active);
        if shown == active {
            return self.preview_sheet_now(ui);
        }
        // draw the other sheet by taking it into the editing fields for the moment (moves, no copies), and put the editor's back
        self.sch.switch_sheet(shown);
        let clicked = self.preview_sheet_now(ui);
        self.sch.switch_sheet(active);
        clicked
    }

    /// The sheet the small window shows when it is not the one being edited (for the tests).
    #[cfg(test)]
    pub fn preview_shown_sheet(&self) -> Option<usize> {
        self.preview_sheet.filter(|k| *k != self.sch.active_sheet)
    }

    /// The sheet that holds part `refdes` (the one being edited first).
    fn sheet_with_part(&self, refdes: &str) -> Option<usize> {
        let has = |cs: &[Component]| cs.iter().any(|c| c.refdes == refdes);
        if has(&self.sch.components) {
            return Some(self.sch.active_sheet);
        }
        self.sch.sheets.iter().position(|s| has(&s.components))
    }

    /// The sheet in the editing fields, fitted into `ui`'s space (what `preview` shows).
    fn preview_sheet_now(&mut self, ui: &mut egui::Ui) -> Option<String> {
        let (response, painter) = ui.allocate_painter(ui.available_size().max(Vec2::new(120.0, 80.0)), egui::Sense::click());
        let rect = response.rect;
        painter.rect_filled(rect, 0.0, crate::canvas_theme::col(crate::canvas_theme::Role::Bg));
        let Some((x0, x1, y0, y1)) = self.sheet_extent() else {
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, i18n::tr("回路図に何もありません"), egui::FontId::proportional(14.0), Color32::from_gray(180));
            return None;
        };
        // draw with a view of its own, the editor's view and selection put back afterwards
        let saved = (self.zoom, self.pan, self.selected.take(), std::mem::take(&mut self.multi));
        self.zoom = ((rect.width() - 8.0) / (x1 - x0 + 2.0 * MARGIN)).min((rect.height() - 8.0) / (y1 - y0 + 2.0 * MARGIN)).clamp(1.0, 40.0);
        self.pan = Vec2::new((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        let painter = painter.with_clip_rect(rect);
        self.draw_wires(&painter, rect);
        self.draw_buses(&painter, rect);
        self.draw_junctions(&painter, rect);
        self.draw_components(&painter, rect);
        self.draw_labels(&painter, rect);
        self.draw_ports(&painter, rect);
        self.draw_linked(&painter, rect);
        let clicked = if response.clicked() { response.interact_pointer_pos().and_then(|p| self.comp_at(self.to_grid(rect, p))).map(|i| self.sch.components[i].refdes.clone()) } else { None };
        let hover = response.hover_pos().and_then(|p| self.comp_at(self.to_grid(rect, p))).map(|i| self.sch.components[i].refdes.clone());
        self.zoom = saved.0;
        self.pan = saved.1;
        self.selected = saved.2;
        self.multi = saved.3;
        if let Some(r) = hover {
            response.on_hover_text(i18n::trf!("{} (クリックで基板の上でも光ります)", r));
        }
        clicked
    }
}
