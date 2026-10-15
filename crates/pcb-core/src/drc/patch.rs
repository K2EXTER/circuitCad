use super::*;

/// May the polygon `outline` (a piece of copper of the same group as track `track`) lie on `side`? Not if copper of
/// another group comes closer to it than the clearance rule, or lies inside it.
pub fn patch_clear(board: &Board, conn: &Connectivity, side: Side, outline: &[Point], track: usize) -> bool {
    let items = copper_items(board);
    let own = conn.items[track];
    let n = outline.len();
    for (k, it) in items.iter().enumerate() {
        if conn.items[k] == own || !it.on(side) {
            continue;
        }
        if inside_polygon(outline, center(&it.geo)) {
            return false;
        }
        for e in 0..n {
            let edge = Geo::Cap { a: outline[e], b: outline[(e + 1) % n], r: 0.0 };
            if dist(&edge, &it.geo) + 1e-9 < board.rules.min_clearance {
                return false;
            }
        }
    }
    true
}

/// The same check as [`check`], on the copper as it will be made: the board's finish applied (see [`crate::smooth`]).
pub fn check_final(board: &Board) -> Vec<Violation> {
    match crate::smooth::finished(board) {
        Some(f) => check(&f.board),
        None => check(board),
    }
}
