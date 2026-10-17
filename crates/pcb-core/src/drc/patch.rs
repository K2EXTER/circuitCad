use super::*;

/// The copper of a board as [`patch_clear`] looks at it: made once, for asking about many polygons.
pub struct PatchCheck {
    items: Vec<Cu>,
    boxes: Vec<(f64, f64, f64, f64)>,
}

impl PatchCheck {
    pub fn new(board: &Board) -> Self {
        let items = copper_items(board);
        let boxes = items.iter().map(|it| bbox(&it.geo)).collect();
        PatchCheck { items, boxes }
    }

    /// May the polygon `outline` (a piece of copper of the same group as track `track`) lie on `side`? Not if copper of
    /// another group comes closer to it than the clearance rule, or lies inside it.
    pub fn clear(&self, board: &Board, conn: &Connectivity, side: Side, outline: &[Point], track: usize) -> bool {
        let own = conn.items[track];
        let n = outline.len();
        let m = board.rules.min_clearance;
        // only copper whose box is within the clearance of the polygon's box can be too close to it (or inside it)
        let (x0, x1, y0, y1) = outline.iter().fold((f64::MAX, f64::MIN, f64::MAX, f64::MIN), |b, p| (b.0.min(p.x), b.1.max(p.x), b.2.min(p.y), b.3.max(p.y)));
        for (k, it) in self.items.iter().enumerate() {
            let bx = self.boxes[k];
            if bx.0 > x1 + m || bx.1 < x0 - m || bx.2 > y1 + m || bx.3 < y0 - m {
                continue;
            }
            if conn.items[k] == own || !it.on(side) {
                continue;
            }
            if inside_polygon(outline, center(&it.geo)) {
                return false;
            }
            for e in 0..n {
                let edge = Geo::Cap { a: outline[e], b: outline[(e + 1) % n], r: 0.0 };
                if dist(&edge, &it.geo) + 1e-9 < m {
                    return false;
                }
            }
        }
        true
    }
}

/// May the polygon `outline` (a piece of copper of the same group as track `track`) lie on `side`? Not if copper of
/// another group comes closer to it than the clearance rule, or lies inside it. (For many polygons use [`PatchCheck`].)
pub fn patch_clear(board: &Board, conn: &Connectivity, side: Side, outline: &[Point], track: usize) -> bool {
    PatchCheck::new(board).clear(board, conn, side, outline, track)
}

/// The copper of a board as [`StretchCheck::clear`] looks at it: made once, for asking about many stretches.
pub struct StretchCheck {
    items: Vec<Cu>,
    conn: Connectivity,
    min_clearance: f64,
}

impl StretchCheck {
    pub fn new(board: &Board) -> Self {
        StretchCheck { items: copper_items(board), conn: connectivity(board), min_clearance: board.rules.min_clearance }
    }

    /// May a track of `width` run straight from `a` to `b` on `side`, leaving the pad number `pad` (`board.pads[pad]`)?
    /// Not if copper of another net comes closer to it than the clearance rule (copper of the pad's own net is what
    /// the route is for: it is joined to the pad, or will be).
    pub fn clear(&self, side: Side, a: Point, b: Point, width: f64, pad: usize) -> bool {
        let own = self.conn.pad_group(pad);
        let own_net = self.conn.pad_net(pad);
        let stretch = Geo::Cap { a, b, r: width / 2.0 };
        let (x0, x1, y0, y1) = bbox(&stretch);
        let m = self.min_clearance;
        self.items.iter().enumerate().all(|(k, it)| {
            let group = self.conn.items[k];
            if group == own || !it.on(side) || (own_net.is_some() && self.conn.group_net(group) == own_net) {
                return true;
            }
            let bx = bbox(&it.geo);
            if bx.0 > x1 + m || bx.1 < x0 - m || bx.2 > y1 + m || bx.3 < y0 - m {
                return true;
            }
            dist(&it.geo, &stretch) + 1e-9 >= m
        })
    }
}

/// The same check as [`check`], on the copper as it will be made: the board's finish applied (see [`crate::smooth`]).
pub fn check_final(board: &Board) -> Vec<Violation> {
    match crate::smooth::finished(board) {
        Some(f) => check(&f.board),
        None => check(board),
    }
}
