//! Alignment guides ("延長線トラッキング", like AutoCAD's object snap tracking): rest the pointer on a snap point (a pad,
//! a via, a track end) for a moment and the point is picked up; from then on horizontal and vertical dotted guides run through it,
//! and the pointer snaps to a guide, or to the crossing of two guides, so a new track, via or corner lines up with what is there.
//! With the Select tool the points are picked up the same way (pointer at rest, no button down), and a part that is dragged has
//! its own place (not the pointer) snapped to the guides, so it lines up with a pad, a via or a track end.

use super::*;

/// How long (s) the pointer rests on a snap point before the point is picked up.
const HOLD: f64 = 0.3;
/// The most points kept at a time (the oldest is dropped).
const MAX_POINTS: usize = 3;
/// How near (screen pixels) the pointer must be to a guide to snap to it.
const NEAR_PX: f64 = 10.0;

pub(super) struct Tracking {
    /// Switched on (F11 or the checkbox in the panel).
    pub on: bool,
    /// The points that are picked up.
    pub points: Vec<Point>,
    /// The snap point the pointer rests on, and since when (seconds of the UI clock).
    hover: Option<(Point, f64)>,
}

impl Default for Tracking {
    fn default() -> Self {
        Self { on: true, points: vec![], hover: None }
    }
}

/// The pointer `w` (mm) and its grid-snapped place `p`, moved onto the nearest vertical and horizontal guide through `points`
/// (within `tol` mm); the other direction keeps the grid. Returns the place and the x / y of the guides it snapped to.
pub(super) fn guided(points: &[Point], w: Point, p: Point, tol: f64) -> (Point, Option<f64>, Option<f64>) {
    let nearest = |vals: Vec<f64>, at: f64| vals.into_iter().filter(|v| (v - at).abs() <= tol).min_by(|a, b| (a - at).abs().total_cmp(&(b - at).abs()));
    let gx = nearest(points.iter().map(|a| a.x).collect(), w.x);
    let gy = nearest(points.iter().map(|a| a.y).collect(), w.y);
    (Point::new(gx.unwrap_or(p.x), gy.unwrap_or(p.y)), gx, gy)
}

impl BoardEditor {
    /// Do the tools that place copper or corners use the guides? (The Select tool does, for a part that is dragged.)
    fn tracking_tool(&self) -> bool {
        matches!(self.tool, Tool::Track | Tool::Via | Tool::OutlinePoly | Tool::Zone | Tool::Select)
    }

    /// Forget the picked-up points (Esc, another tool).
    pub(super) fn clear_tracking(&mut self) {
        self.tracking.points.clear();
        self.tracking.hover = None;
    }

    /// F11.
    pub(super) fn toggle_tracking(&mut self) {
        self.tracking.on = !self.tracking.on;
        if !self.tracking.on {
            self.clear_tracking();
        }
        self.status = if self.tracking.on { "延長線トラッキング: 入" } else { "延長線トラッキング: 切" }.into();
    }

    /// Pick up the snap point `near` the pointer rests on, draw the guides, and give back where the pointer snaps to. `p` is where
    /// it would be without the guides (the grid, or the object it snapped to: `on_object`).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn track_guides(&mut self, ctx: &egui::Context, painter: &egui::Painter, rect: Rect, w: Point, p: Point, near: Option<Point>, on_object: bool) -> Point {
        if !self.tracking.on || !self.tracking_tool() {
            return p;
        }
        // what is lined up: the pointer, or (dragging a part) the place of the part, which is `off` from the pointer
        let selecting = self.tool == Tool::Select;
        let off = if selecting { self.drag_part.as_ref().map(|(_, o)| *o) } else { Some(Point::new(0.0, 0.0)) };
        // with the Select tool a point is picked up only by a pointer at rest
        let near = if selecting && ctx.input(|i| i.pointer.any_down()) { None } else { near };
        let now = ctx.input(|i| i.time);
        match (near, self.tracking.hover) {
            (Some(c), Some((h, since))) if h.dist(c) < 1e-9 => {
                if now - since >= HOLD {
                    if !self.tracking.points.iter().any(|q| q.dist(c) < 1e-9) {
                        self.tracking.points.push(c);
                        if self.tracking.points.len() > MAX_POINTS {
                            self.tracking.points.remove(0);
                        }
                    }
                } else {
                    ctx.request_repaint_after(std::time::Duration::from_millis(50));
                }
            }
            (Some(c), _) => {
                self.tracking.hover = Some((c, now));
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            (None, _) => self.tracking.hover = None,
        }
        let ink = Color32::from_rgb(90, 200, 255);
        for a in &self.tracking.points {
            let s = self.to_screen(rect, *a);
            painter.line_segment([s - Vec2::new(5.0, 0.0), s + Vec2::new(5.0, 0.0)], Stroke::new(1.5, ink));
            painter.line_segment([s - Vec2::new(0.0, 5.0), s + Vec2::new(0.0, 5.0)], Stroke::new(1.5, ink));
        }
        let Some(off) = off else { return p };
        if on_object {
            return p;
        }
        let at = |v: Point| Point::new(v.x - off.x, v.y - off.y);
        let (q, gx, gy) = guided(&self.tracking.points, at(w), at(p), NEAR_PX / self.zoom as f64);
        let q = Point::new(q.x + off.x, q.y + off.y);
        let dotted = Stroke::new(1.0, ink.gamma_multiply(0.8));
        if let Some(x) = gx {
            let sx = self.to_screen(rect, Point::new(x, 0.0)).x;
            painter.extend(egui::Shape::dashed_line(&[Pos2::new(sx, rect.top()), Pos2::new(sx, rect.bottom())], dotted, 6.0, 4.0));
        }
        if let Some(y) = gy {
            let sy = self.to_screen(rect, Point::new(0.0, y)).y;
            painter.extend(egui::Shape::dashed_line(&[Pos2::new(rect.left(), sy), Pos2::new(rect.right(), sy)], dotted, 6.0, 4.0));
        }
        q
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pointer_snaps_to_a_guide_and_keeps_the_grid_along_it() {
        let pts = [Point::new(10.0, 5.0)];
        // near the vertical guide x = 10: x from the guide, y from the grid
        let (q, gx, gy) = guided(&pts, Point::new(10.05, 12.3), Point::new(10.2, 12.0), 0.1);
        assert_eq!((q, gx, gy), (Point::new(10.0, 12.0), Some(10.0), None));
        // near the horizontal guide
        let (q, _, gy) = guided(&pts, Point::new(20.0, 5.04), Point::new(20.0, 5.0), 0.1);
        assert_eq!((q.x, q.y, gy), (20.0, 5.0, Some(5.0)));
        // far from both: the grid place
        let (q, gx, gy) = guided(&pts, Point::new(30.0, 30.0), Point::new(30.0, 30.0), 0.1);
        assert_eq!((q, gx, gy), (Point::new(30.0, 30.0), None, None));
    }

    #[test]
    fn two_guides_cross_at_their_intersection() {
        let pts = [Point::new(10.0, 5.0), Point::new(3.0, 20.0)];
        let (q, gx, gy) = guided(&pts, Point::new(3.04, 5.03), Point::new(3.0, 5.0), 0.1);
        assert_eq!((q, gx, gy), (Point::new(3.0, 5.0), Some(3.0), Some(5.0)));
    }
}
