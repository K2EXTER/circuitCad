//! Dimensions kept on the board (`pcb_core::dimension`): drawn on the canvas, made from a measurement of the measuring tool
//! (Enter keeps it as it is, H only its width, V only its height) or from the outline (a button), and taken off again.

use super::*;
use pcb_core::dimension::{add_outline_dimensions, DimDir, Dimension};

/// How far (mm) a dimension kept from the measuring tool stands off the measured line.
const STAND_OFF: f64 = 2.0;

impl BoardEditor {
    /// Keep the finished measurement as a dimension on the board. The dimension line stands off on the side away from the middle
    /// of the board, so it is drawn over less of the board. Returns false when there is no finished measurement.
    pub fn keep_measure(&mut self, dir: DimDir) -> bool {
        let Some((a, Some(b))) = self.measure else { return false };
        let mut d = Dimension::new(a, b, STAND_OFF, dir);
        if d.length() < 0.01 {
            self.status = match dir {
                DimDir::Horizontal => "横の長さが 0 なので、横の寸法線は作れません (縦なら V、斜めなら Enter)".into(),
                DimDir::Vertical => "縦の長さが 0 なので、縦の寸法線は作れません (横なら H、斜めなら Enter)".into(),
                DimDir::Aligned => "2点が同じ所なので、寸法線は作れません".into(),
            };
            return true;
        }
        if let Some((lo, hi)) = self.board.bounds() {
            let centre = Point::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
            let line = |d: &Dimension| d.drawing().lines[0];
            let mid = |l: [Point; 2]| Point::new((l[0].x + l[1].x) / 2.0, (l[0].y + l[1].y) / 2.0);
            let mut other = d.clone();
            other.offset = -STAND_OFF;
            if mid(line(&other)).dist(centre) > mid(line(&d)).dist(centre) {
                d = other;
            }
        }
        self.checkpoint();
        let len = d.length();
        self.board.dimensions.push(d);
        self.measure = None;
        let what = match dir {
            DimDir::Aligned => "",
            DimDir::Horizontal => "横の",
            DimDir::Vertical => "縦の",
        };
        self.status = format!("{what}寸法線 {len:.2} mm を基板に残しました (DXF と SVG の図に出ます。ガーバーには出ません)。「元に戻す」で取り消せます");
        true
    }

    pub(super) fn draw_dimensions(&self, painter: &egui::Painter, rect: Rect) {
        if self.board.dimensions.is_empty() {
            return;
        }
        let col = Color32::from_rgb(120, 175, 235);
        let z = self.zoom;
        for d in &self.board.dimensions {
            let g = d.drawing();
            for [a, b] in &g.lines {
                painter.line_segment([self.to_screen(rect, *a), self.to_screen(rect, *b)], Stroke::new(1.0, col));
            }
            // the length, upright along the line (the screen's y points down, so the angle turns the other way)
            let size = (d.text_height as f32 * z).clamp(9.0, 40.0);
            let galley = painter.layout_no_wrap(format!("{} mm", g.text), egui::FontId::proportional(size), col);
            let at = self.to_screen(rect, g.text_at);
            let angle = -(g.angle as f32).to_radians();
            let half = galley.size() / 2.0;
            let (c, s) = (angle.cos(), angle.sin());
            let corner = at - Vec2::new(half.x * c - half.y * s, half.x * s + half.y * c);
            painter.add(egui::epaint::TextShape::new(corner, galley, col).with_angle(angle));
        }
    }

    /// The dimension row of the finishing section.
    pub(super) fn panel_dimensions(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if ui.button("外形の寸法線").on_hover_text("基板の外形の幅と高さを、寸法線にして基板の下と左に置きます (外形を変えたら、もう一度押すと置き直します)。\\nケースを設計する人に渡す DXF と、SVG の図に出ます。ガーバー (工場に渡すデータ) には出ません。\\n好きな2点の寸法線は、「距離を測る」ツールで2点を測ってから Enter (斜めの長さ)・H (横)・V (縦) で残せます").clicked() {
                self.checkpoint();
                match add_outline_dimensions(&mut self.board) {
                    Ok((w, h)) => self.status = format!("外形の寸法線を置きました: 幅 {w:.2} mm・高さ {h:.2} mm"),
                    Err(e) => {
                        self.undo.pop();
                        self.status = e;
                    }
                }
            }
            let n = self.board.dimensions.len();
            if n > 0 {
                if ui.button("最後の寸法線を消す").on_hover_text("いちばん最後に置いた寸法線を1本消します").clicked() {
                    self.checkpoint();
                    self.board.dimensions.pop();
                    self.status = "寸法線を1本消しました".into();
                }
                if ui.button(format!("寸法線を全部消す ({n})")).on_hover_text("基板の寸法線を全部消します (「元に戻す」で戻せます)").clicked() {
                    self.checkpoint();
                    self.board.dimensions.clear();
                    self.status = format!("寸法線を {n} 本消しました");
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_measurement_is_kept_as_a_dimension_away_from_the_board_and_undone() {
        let mut e = BoardEditor::default();
        e.board = pcb_core::Board::new("t", 40.0, 30.0);
        // measure along the bottom edge: the line goes below the board, not over it
        e.measure_click(Point::new(0.0, 0.0));
        e.measure_click(Point::new(40.0, 3.0));
        assert!(e.keep_measure(DimDir::Horizontal));
        let d = &e.board.dimensions[0];
        assert!((d.length() - 40.0).abs() < 1e-9);
        assert!(d.drawing().lines[0][0].y < 0.0, "{:?}", d.drawing().lines[0]);
        assert!(e.measure.is_none() && e.status.contains("40.00"), "{}", e.status);
        assert!(e.dirty());
        // nothing measured: nothing kept
        assert!(!e.keep_measure(DimDir::Aligned));
        e.undo();
        assert!(e.board.dimensions.is_empty());
        // a vertical one of a horizontal measurement is refused with a reason
        e.measure_click(Point::new(5.0, 5.0));
        e.measure_click(Point::new(15.0, 5.0));
        assert!(e.keep_measure(DimDir::Vertical));
        assert!(e.board.dimensions.is_empty() && e.status.contains("縦の長さが 0"), "{}", e.status);
    }
}
