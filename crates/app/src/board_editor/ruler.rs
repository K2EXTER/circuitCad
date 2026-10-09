//! Rulers along the top and left edge of the board view: millimetre ticks that follow the zoom,
//! with a mark at the cursor, so that real sizes can be read off the screen.

use super::*;
use eframe::egui::{Align2, FontId};

/// Thickness of the horizontal ruler and width of the vertical one (screen points).
const TOP: f32 = 18.0;
const LEFT: f32 = 36.0;
/// A labelled tick is placed at least this far apart (screen points).
const MIN_MAJOR_PX: f32 = 70.0;

/// Candidate spacings of the labelled ticks in mm, with how many minor intervals each one holds.
const STEPS: &[(f64, u32)] = &[
    (0.05, 5),
    (0.1, 5),
    (0.2, 4),
    (0.5, 5),
    (1.0, 5),
    (2.0, 4),
    (5.0, 5),
    (10.0, 5),
    (20.0, 4),
    (50.0, 5),
    (100.0, 5),
    (200.0, 4),
    (500.0, 5),
];

/// The labelled-tick spacing (mm) and minor divisions for `zoom` pixels per mm: the smallest
/// spacing that keeps labelled ticks `MIN_MAJOR_PX` apart.
pub(super) fn tick_step(zoom: f32) -> (f64, u32) {
    STEPS.iter().copied().find(|(s, _)| *s as f32 * zoom >= MIN_MAJOR_PX).unwrap_or(*STEPS.last().unwrap())
}

/// Tick positions in `[lo, hi]` for a spacing `minor`, each with whether it is a labelled one
/// (every `per_major`-th). Indices are counted from 0 mm so that the board edge is always a tick.
pub(super) fn ticks(lo: f64, hi: f64, minor: f64, per_major: u32) -> Vec<(f64, bool)> {
    let first = (lo / minor).ceil() as i64;
    let last = (hi / minor).floor() as i64;
    (first..=last).map(|i| (i as f64 * minor, i.rem_euclid(per_major as i64) == 0)).collect()
}

fn label(v: f64, step: f64) -> String {
    if step < 1.0 { format!("{v:.1}") } else { format!("{v:.0}") }
}

impl BoardEditor {
    /// Draw the rulers over the view. `cursor` is the pointer position on screen, if it is inside.
    pub(super) fn draw_rulers(&self, painter: &egui::Painter, rect: Rect, cursor: Option<Pos2>) {
        if !self.vis.ruler {
            return;
        }
        let back = Color32::from_rgba_unmultiplied(24, 28, 24, 225);
        let tick = Color32::from_gray(150);
        let text = Color32::from_gray(215);
        let font = FontId::monospace(10.0);
        let (step, per_major) = tick_step(self.zoom);
        let minor = step / per_major as f64;
        let top = Rect::from_min_max(Pos2::new(rect.min.x + LEFT, rect.min.y), Pos2::new(rect.max.x, rect.min.y + TOP));
        let left = Rect::from_min_max(Pos2::new(rect.min.x, rect.min.y + TOP), Pos2::new(rect.min.x + LEFT, rect.max.y));
        painter.rect_filled(top, 0.0, back);
        painter.rect_filled(left, 0.0, back);
        painter.rect_filled(Rect::from_min_max(rect.min, Pos2::new(rect.min.x + LEFT, rect.min.y + TOP)), 0.0, back);
        painter.text(rect.min + Vec2::new(LEFT / 2.0, TOP / 2.0), Align2::CENTER_CENTER, "mm", font.clone(), Color32::from_rgb(120, 200, 140));

        // horizontal ruler: x grows to the right
        let (x0, x1) = (self.to_world(rect, top.left_top()).x, self.to_world(rect, top.right_top()).x);
        for (x, major) in ticks(x0, x1, minor, per_major) {
            let sx = self.to_screen(rect, Point::new(x, 0.0)).x;
            let len = if major { TOP * 0.7 } else { TOP * 0.35 };
            painter.line_segment([Pos2::new(sx, top.max.y), Pos2::new(sx, top.max.y - len)], Stroke::new(1.0, tick));
            if major {
                painter.text(Pos2::new(sx + 2.0, top.min.y + 1.0), Align2::LEFT_TOP, label(x, step), font.clone(), text);
            }
        }
        // vertical ruler: y grows upwards, like the coordinates shown in the status bar
        let (y_hi, y_lo) = (self.to_world(rect, left.left_top()).y, self.to_world(rect, left.left_bottom()).y);
        for (y, major) in ticks(y_lo, y_hi, minor, per_major) {
            let sy = self.to_screen(rect, Point::new(0.0, y)).y;
            let len = if major { LEFT * 0.45 } else { LEFT * 0.22 };
            painter.line_segment([Pos2::new(left.max.x, sy), Pos2::new(left.max.x - len, sy)], Stroke::new(1.0, tick));
            if major {
                painter.text(Pos2::new(left.min.x + 2.0, sy - 1.0), Align2::LEFT_BOTTOM, label(y, step), font.clone(), text);
            }
        }
        painter.line_segment([top.left_bottom(), top.right_bottom()], Stroke::new(1.0, Color32::from_gray(90)));
        painter.line_segment([left.right_top(), left.right_bottom()], Stroke::new(1.0, Color32::from_gray(90)));

        // the cursor, on both rulers
        if let Some(c) = cursor.filter(|c| rect.contains(*c)) {
            let mark = Stroke::new(1.5, Color32::from_rgb(255, 120, 60));
            if c.x >= top.min.x {
                painter.line_segment([Pos2::new(c.x, top.min.y), Pos2::new(c.x, top.max.y)], mark);
            }
            if c.y >= left.min.y {
                painter.line_segment([Pos2::new(left.min.x, c.y), Pos2::new(left.max.x, c.y)], mark);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spacing_follows_the_zoom() {
        // 12 px/mm: a labelled tick every 10 mm would be 120 px, 5 mm is 60 px (< 70): so 10 mm
        assert_eq!(tick_step(12.0).0, 10.0);
        assert_eq!(tick_step(60.0).0, 2.0);
        assert_eq!(tick_step(400.0).0, 0.2);
        // zoomed far out: the largest spacing
        assert_eq!(tick_step(0.01).0, 500.0);
        // whatever the zoom, labelled ticks are never closer than the minimum (unless at the limit)
        for z in [1.0f32, 2.5, 7.0, 12.0, 33.0, 90.0, 250.0] {
            let (s, _) = tick_step(z);
            assert!(s as f32 * z >= MIN_MAJOR_PX, "{z}: {s}");
        }
    }

    #[test]
    fn ticks_land_on_round_numbers_and_the_edge_of_the_board() {
        let t = ticks(-1.3, 10.4, 2.0, 5);
        let xs: Vec<f64> = t.iter().map(|(x, _)| *x).collect();
        assert_eq!(xs, vec![0.0, 2.0, 4.0, 6.0, 8.0, 10.0]);
        // every 5th is labelled, counted from 0
        assert_eq!(t.iter().filter(|(_, m)| *m).map(|(x, _)| *x).collect::<Vec<_>>(), vec![0.0, 10.0]);
        // negative positions keep the pattern
        let t = ticks(-12.0, -1.0, 2.0, 5);
        assert!(t.iter().any(|(x, m)| *x == -10.0 && *m));
        assert!(ticks(3.2, 3.5, 1.0, 5).is_empty());
    }

    #[test]
    fn labels_show_decimals_only_for_small_steps() {
        assert_eq!(label(2.5, 0.5), "2.5");
        assert_eq!(label(10.0, 10.0), "10");
    }
}
