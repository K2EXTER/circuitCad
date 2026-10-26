//! Pan / zoom transforms and small lookups used while drawing.

use super::*;

/// Width of the floating layer panel at the canvas's right edge, in screen pixels (with a little air).
const OVERLAY_PANEL_PX: f32 = 230.0;

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
        // the "物の表示とロック" panel floats over the right edge of the canvas: fit into the part of the canvas left of it
        let free = (rect.width() - OVERLAY_PANEL_PX).max(rect.width() * 0.5);
        self.zoom = ((free as f64 / w).min(rect.height() as f64 / h) as f32).clamp(2.0, 60.0);
        // the view's centre moves right by half the panel, so the board sits in the free part
        let shift = (rect.width() - free) / 2.0 / self.zoom;
        self.pan = Vec2::new(((lo.x + hi.x) / 2.0) as f32 + shift, ((lo.y + hi.y) / 2.0) as f32);
    }

    /// The connections of the copper, worked out again only when the board has changed (it costs milliseconds
    /// on a big board, and the picture asks for it every frame).
    pub(super) fn cached_connectivity(&self) -> std::rc::Rc<drc::Connectivity> {
        let key = pcb_core::zonefill::geometry_key(&self.board);
        let mut c = self.calc_cache.borrow_mut();
        if c.key != key || (c.conn.is_none() && c.rats.is_none()) {
            *c = CalcCache { key, conn: None, rats: None };
        }
        c.key = key;
        c.conn.get_or_insert_with(|| std::rc::Rc::new(connectivity(&self.board))).clone()
    }

    /// Are only parts being moved (one part, or a group of parts)? The pours and the finished tracks are then kept as they were
    /// until the drag ends: they cost tens of milliseconds to work out, and the part follows the pointer much more smoothly
    /// without them. Anything that moves a track is not included (the track would stay where it was).
    pub(super) fn moving_parts_only(&self) -> bool {
        self.drag_part.is_some() || self.gdrag.as_ref().is_some_and(|g| g.items.iter().all(|i| matches!(i, Item::Part(_))))
    }

    /// Is a group being moved that is not a pour (parts, tracks, vias, silk ...)? The pours are then kept as they were until the drop,
    /// like while parts are moved (they cost 100 ms a frame on a board with guards, and a group of a hundred tracks made the drag crawl).
    /// The finished tracks are not kept: the tracks of the group would stay where they were, so the plain tracks are drawn instead.
    pub(super) fn moving_group(&self) -> bool {
        self.gdrag.as_ref().is_some_and(|g| !g.items.iter().any(|i| matches!(i, Item::Zone(_))))
    }

    /// The slow results (pours, islands, thermal count) are kept as they were: parts or a group are being moved.
    pub(super) fn slow_results_kept(&self) -> bool {
        self.moving_parts_only() || self.moving_group()
    }

    /// The unrouted connections (yellow lines), kept like `cached_connectivity`.
    pub(super) fn cached_airwires(&self) -> std::rc::Rc<Vec<pcb_core::ratsnest::Airwire>> {
        let key = pcb_core::zonefill::geometry_key(&self.board);
        // while parts are dragged, the lines follow the part (they cost milliseconds), but a pour that is in pieces is not looked for
        // (it fills every pour: 150 ms on a board with guards) and the answer is not kept, so the full one is made when the drag ends
        if self.slow_results_kept() {
            return std::rc::Rc::new(ratsnest(&self.board));
        }
        let mut c = self.calc_cache.borrow_mut();
        if c.key != key {
            *c = CalcCache { key, conn: None, rats: None };
        }
        c.rats
            .get_or_insert_with(|| {
                // the airwires of what no copper joins, and those of a pour that is in pieces (its outline joins them, its copper does not)
                let mut w = ratsnest(&self.board);
                w.extend(drc::pour_split_airwires(&self.board));
                std::rc::Rc::new(w)
            })
            .clone()
    }

    pub(super) fn side_color(&self, s: Side) -> Color32 {
        match s {
            Side::Top => Color32::from_rgb(220, 60, 60),
            Side::Bottom => Color32::from_rgb(70, 110, 230),
            Side::Inner(n) => [Color32::from_rgb(210, 160, 40), Color32::from_rgb(60, 170, 90), Color32::from_rgb(170, 90, 200), Color32::from_rgb(40, 170, 180)][(n as usize - 1) % 4],
        }
    }

    pub(super) fn side_visible(&self, s: Side) -> bool {
        match s {
            Side::Top => self.vis.top,
            Side::Bottom => self.vis.bottom,
            Side::Inner(n) => self.vis.inner_hidden & (1 << n.min(31)) == 0,
        }
    }

    /// The copper layers in drawing order: the ones not being worked on first, the working layer last (on top).
    pub(super) fn layer_order(&self) -> Vec<Side> {
        let mut v = self.board.copper_layers();
        v.retain(|&s| s != self.side);
        v.push(self.side);
        v
    }

    /// The next copper layer down (and back to the top after the bottom), for the L key and for vias.
    pub(super) fn next_layer(&self) -> Side {
        let v = self.board.copper_layers();
        let i = v.iter().position(|&s| s == self.side).unwrap_or(0);
        v[(i + 1) % v.len()]
    }

    pub(super) fn nets(&self) -> Vec<String> {
        let mut v: Vec<String> = self.board.pads.iter().filter(|p| !p.net.is_empty()).map(|p| p.net.clone()).collect();
        v.sort();
        v.dedup();
        v
    }
}

impl BoardEditor {
    /// A rectangular outline with the corner radius set in the panel (square corners when it is 0).
    pub(super) fn set_outline_box(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        if self.size_r > 0.0 {
            self.board.outline = pcb_core::outline::rounded_rect(x0, y0, x1, y1, self.size_r);
        } else {
            self.board.set_rect_outline(x0, y0, x1, y1);
        }
    }

    /// Rounds every corner of the outline with radius `r` mm (the "いまの外形の角を丸める" button).
    pub fn round_outline(&mut self, r: f64) {
        self.size_r = r;
        if r > 0.0 && self.board.outline.len() >= 3 {
            self.checkpoint();
            self.board.outline = pcb_core::outline::round_corners(&self.board.outline, r);
            self.status = format!("基板の角を R{r} mm で丸めました ({} 点)", self.board.outline.len());
        } else {
            self.status = "「角R」に 0 より大きい値を入れてください".into();
        }
    }
}
