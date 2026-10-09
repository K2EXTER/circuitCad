//! Pan / zoom transforms and small lookups used while drawing.

use super::*;

impl BoardEditor {
    pub(super) fn to_screen(&self, rect: Rect, p: Point) -> Pos2 {
        let c = rect.center();
        Pos2::new(c.x + (p.x as f32 - self.pan.x) * self.zoom, c.y - (p.y as f32 - self.pan.y) * self.zoom)
    }

    pub(super) fn to_world(&self, rect: Rect, s: Pos2) -> Point {
        let c = rect.center();
        Point::new(((s.x - c.x) / self.zoom + self.pan.x) as f64, (-(s.y - c.y) / self.zoom + self.pan.y) as f64)
    }

    pub(super) fn fit_view(&mut self, rect: Rect) {
        let Some((lo, hi)) = self.board.bounds() else { return };
        let (w, h) = ((hi.x - lo.x).max(5.0) + 10.0, (hi.y - lo.y).max(5.0) + 10.0);
        self.zoom = ((rect.width() as f64 / w).min(rect.height() as f64 / h) as f32).clamp(2.0, 60.0);
        self.pan = Vec2::new(((lo.x + hi.x) / 2.0) as f32, ((lo.y + hi.y) / 2.0) as f32);
    }

    pub(super) fn side_color(&self, s: Side) -> Color32 {
        match s {
            Side::Top => Color32::from_rgb(220, 60, 60),
            Side::Bottom => Color32::from_rgb(70, 110, 230),
        }
    }

    pub(super) fn side_visible(&self, s: Side) -> bool {
        match s {
            Side::Top => self.vis.top,
            Side::Bottom => self.vis.bottom,
        }
    }

    pub(super) fn nets(&self) -> Vec<String> {
        let mut v: Vec<String> = self.board.pads.iter().filter(|p| !p.net.is_empty()).map(|p| p.net.clone()).collect();
        v.sort();
        v.dedup();
        v
    }
}
