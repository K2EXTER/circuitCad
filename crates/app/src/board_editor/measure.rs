//! The measuring tool: two clicks give the distance between the points (nothing on the board changes).

use super::*;
use eframe::egui::FontId;

/// "12.70 mm (ΔX 12.70 / ΔY 0.00, 角度 0°, 500 mil)" for the line from `a` to `b`.
pub(super) fn measure_text(a: Point, b: Point) -> String {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let d = dx.hypot(dy);
    // the board's Y points down, so the angle is turned to read the usual way (up = positive)
    let angle = (-dy).atan2(dx).to_degrees();
    format!("{d:.2} mm (ΔX {:.2} / ΔY {:.2}, 角度 {angle:.1}°, {:.0} mil)", dx.abs(), dy.abs(), d / 0.0254)
}

impl BoardEditor {
    pub(super) fn measure_click(&mut self, p: Point) {
        self.measure = match self.measure {
            Some((a, None)) => {
                self.status = format!("距離: {} — Enter で寸法線として残す (H は横だけ・V は縦だけ)。次のクリックで測り直し、Esc で消します", measure_text(a, p));
                Some((a, Some(p)))
            }
            _ => {
                self.status = "始点を置きました。終点をクリックしてください".into();
                Some((p, None))
            }
        };
    }

    /// The measured line, with its length written on it (the line to the pointer while the end is not yet clicked).
    pub(super) fn draw_measure(&self, painter: &egui::Painter, rect: Rect) {
        if self.tool != Tool::Measure {
            return;
        }
        let Some((a, end)) = self.measure else { return };
        let Some(b) = end.or(self.measure_hover) else { return };
        let col = Color32::from_rgb(255, 190, 60);
        let (sa, sb) = (self.to_screen(rect, a), self.to_screen(rect, b));
        painter.line_segment([sa, sb], Stroke::new(1.5, col));
        for s in [sa, sb] {
            painter.circle_stroke(s, 4.0, Stroke::new(1.5, col));
        }
        // the two legs, thin, so the X and Y parts can be read
        let corner = Pos2::new(sb.x, sa.y);
        let thin = Stroke::new(1.0, col.gamma_multiply(0.5));
        painter.line_segment([sa, corner], thin);
        painter.line_segment([corner, sb], thin);
        let text = measure_text(a, b);
        let mid = Pos2::new((sa.x + sb.x) / 2.0, (sa.y + sb.y) / 2.0) + Vec2::new(8.0, -8.0);
        let galley = painter.layout_no_wrap(text, FontId::proportional(13.0), Color32::BLACK);
        let bg = Rect::from_min_size(mid - Vec2::new(0.0, galley.size().y), galley.size()).expand(3.0);
        painter.rect_filled(bg, 3.0, col);
        painter.galley(bg.min + Vec2::splat(3.0), galley, Color32::BLACK);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_gives_length_legs_angle_and_mil() {
        let t = measure_text(Point { x: 1.0, y: 1.0 }, Point { x: 4.0, y: 5.0 });
        assert!(t.starts_with("5.00 mm"), "{t}");
        assert!(t.contains("ΔX 3.00") && t.contains("ΔY 4.00"), "{t}");
        // y grows downward on the board, so going down-right reads as a negative angle
        assert!(t.contains("角度 -53.1°"), "{t}");
        assert!(measure_text(Point { x: 0.0, y: 0.0 }, Point { x: 2.54, y: 0.0 }).contains("100 mil"));
    }

    #[test]
    fn two_clicks_measure_and_a_third_starts_over() {
        let mut e = BoardEditor::default();
        let (a, b, c) = (Point { x: 0.0, y: 0.0 }, Point { x: 3.0, y: 4.0 }, Point { x: 9.0, y: 9.0 });
        e.measure_click(a);
        assert_eq!(e.measure, Some((a, None)));
        e.measure_click(b);
        assert_eq!(e.measure, Some((a, Some(b))));
        assert!(e.status.contains("5.00 mm"), "{}", e.status);
        e.measure_click(c);
        assert_eq!(e.measure, Some((c, None)));
        assert!(!e.dirty(), "measuring never changes the board");
    }
}
