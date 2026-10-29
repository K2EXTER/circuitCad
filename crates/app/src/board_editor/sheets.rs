//! Parts by the schematic sheet they were drawn on: a colour for each sheet (a tint round every part, the sheet's name at the
//! corner of its group) and, in the panel, a list of the sheets with a button that selects all the parts of one.

use super::*;

const COLORS: [Color32; 8] = [
    Color32::from_rgb(230, 100, 90),
    Color32::from_rgb(90, 160, 230),
    Color32::from_rgb(110, 200, 120),
    Color32::from_rgb(230, 180, 60),
    Color32::from_rgb(180, 110, 220),
    Color32::from_rgb(70, 200, 200),
    Color32::from_rgb(230, 130, 180),
    Color32::from_rgb(160, 160, 90),
];

impl BoardEditor {
    /// The sheets that have parts on the board, in order, with their colours. Empty for a one-sheet circuit.
    pub(super) fn sheet_colors(&self) -> Vec<(String, Color32)> {
        let mut names: Vec<&String> = self.board.part_sheets.values().collect();
        names.sort();
        names.dedup();
        names.into_iter().enumerate().map(|(k, n)| (n.clone(), COLORS[k % COLORS.len()])).collect()
    }

    /// The box round a part's pads and outline.
    pub(super) fn part_box(&self, refdes: &str) -> Option<(Point, Point)> {
        let (mut lo, mut hi) = (Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN));
        let mut grow = |p: Point, mx: f64, my: f64| {
            lo = Point::new(lo.x.min(p.x - mx), lo.y.min(p.y - my));
            hi = Point::new(hi.x.max(p.x + mx), hi.y.max(p.y + my));
        };
        for p in self.board.pads.iter().filter(|p| p.refdes == refdes) {
            let (w, h) = p.shape.size();
            grow(p.pos, w / 2.0, h / 2.0);
        }
        for s in self.board.silk.iter().filter(|s| s.refdes == refdes) {
            grow(s.a, 0.0, 0.0);
            grow(s.b, 0.0, 0.0);
        }
        (lo.x <= hi.x).then_some((lo, hi))
    }

    /// A tint round every part in the colour of its sheet, and the sheet's name over the corner of the group.
    pub(super) fn draw_sheet_groups(&self, painter: &egui::Painter, rect: Rect) {
        if !self.show_sheets || !self.vis.parts {
            return;
        }
        let colors = self.sheet_colors();
        for (name, col) in &colors {
            let mut group: Option<(Point, Point)> = None;
            for (refdes, sheet) in self.board.part_sheets.iter().filter(|(_, s)| *s == name) {
                let Some((lo, hi)) = self.part_box(refdes) else { continue };
                let m = 0.4;
                let r = Rect::from_two_pos(self.to_screen(rect, Point::new(lo.x - m, lo.y - m)), self.to_screen(rect, Point::new(hi.x + m, hi.y + m)));
                if r.intersects(rect) {
                    painter.rect_filled(r, 2.0, col.gamma_multiply(0.22));
                    painter.rect_stroke(r, 2.0, Stroke::new(1.0, col.gamma_multiply(0.7)), egui::StrokeKind::Inside);
                }
                let _ = sheet;
                group = Some(match group {
                    None => (lo, hi),
                    Some((a, b)) => (Point::new(a.x.min(lo.x), a.y.min(lo.y)), Point::new(b.x.max(hi.x), b.y.max(hi.y))),
                });
            }
            if let Some((lo, hi)) = group {
                // y is up on the board: the top left corner of the group on the screen
                let at = self.to_screen(rect, Point::new(lo.x, hi.y));
                let galley = painter.layout_no_wrap(name.clone(), egui::FontId::proportional(13.0), Color32::BLACK);
                let tag = Rect::from_min_size(at + Vec2::new(0.0, -galley.size().y - 6.0), galley.size() + Vec2::new(8.0, 3.0));
                painter.rect_filled(tag, 3.0, *col);
                painter.galley(tag.min + Vec2::new(4.0, 1.5), galley, Color32::BLACK);
            }
        }
    }

    /// The list of the sheets in the panel: a colour, the name, the number of parts and a button that selects them.
    pub(super) fn sheet_legend(&mut self, ui: &mut egui::Ui) {
        let colors = self.sheet_colors();
        if colors.is_empty() {
            return;
        }
        // the colours are switched with the eye of "シートの色" in the list of what is shown; here only the list of the sheets
        if !self.show_sheets {
            return;
        }
        ui.label(egui::RichText::new("シート (色は右上の「物の表示とロック」の「シートの色」で切り替え)").small().color(Color32::GRAY));
        for (name, col) in colors {
            let parts: Vec<String> = self.board.part_sheets.iter().filter(|(_, s)| **s == name).map(|(r, _)| r.clone()).collect();
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                ui.painter().rect_filled(r, 2.0, col);
                if ui.small_button(format!("{name} ({}個)", parts.len())).on_hover_text(format!("このシートの部品をまとめて選びます。選んだまま動かせば、グループごと動きます: {}", parts.join(" "))).clicked() {
                    self.multi = self.board.parts.iter().enumerate().filter(|(_, p)| parts.contains(&p.refdes)).map(|(i, _)| Item::Part(i)).collect();
                    self.selected = None;
                    self.status = format!("シート「{name}」の部品 {} 個を選びました", self.multi.len());
                }
            });
        }
    }
}
