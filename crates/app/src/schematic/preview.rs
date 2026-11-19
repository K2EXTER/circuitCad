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

    /// The shown sheet fitted into `ui`'s space, read-only, the part picked elsewhere (`linked`) ringed. Returns the part clicked.
    pub fn preview(&mut self, ui: &mut egui::Ui) -> Option<String> {
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
