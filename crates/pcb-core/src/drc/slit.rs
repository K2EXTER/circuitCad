use super::*;

/// The segments that make up the outline of a shape (its centre line for a capsule) and the radius it is swept by.
fn segments(g: &Geo) -> (Vec<(Point, Point)>, f64) {
    match *g {
        Geo::Cap { a, b, r } => (vec![(a, b)], r),
        Geo::Rect { c, hw, hh } => {
            let (p0, p1, p2, p3) = (Point::new(c.x - hw, c.y - hh), Point::new(c.x + hw, c.y - hh), Point::new(c.x + hw, c.y + hh), Point::new(c.x - hw, c.y + hh));
            (vec![(p0, p1), (p1, p2), (p2, p3), (p3, p0)], 0.0)
        }
    }
}

fn closest_on_segment(p: Point, a: Point, b: Point) -> Point {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l2 = dx * dx + dy * dy;
    let t = if l2 < 1e-18 { 0.0 } else { (((p.x - a.x) * dx + (p.y - a.y) * dy) / l2).clamp(0.0, 1.0) };
    Point::new(a.x + dx * t, a.y + dy * t)
}

/// The pairs of points, one on the edge of each shape, where the shapes are closest (parallel edges have many pairs, all
/// equally close: the ends of the overlap are given); empty where they touch.
fn gap_points(x: &Geo, y: &Geo) -> Vec<(Point, Point)> {
    let ((sx, rx), (sy, ry)) = (segments(x), segments(y));
    let mut pairs: Vec<(Point, Point)> = vec![];
    for &(a, b) in &sx {
        for &(c, d) in &sy {
            for (p, q) in all_closest(a, b, c, d) {
                pairs.push((p, q));
            }
        }
    }
    let Some(least) = pairs.iter().map(|(p, q)| p.dist(*q)).min_by(f64::total_cmp) else { return vec![] };
    if least <= rx + ry + 1e-12 {
        return vec![]; // the swept discs overlap or touch
    }
    pairs
        .into_iter()
        .filter(|(p, q)| p.dist(*q) <= least + 1e-6)
        .map(|(p, q)| {
            // from the centre lines out to the edges
            let gap = p.dist(q);
            let (ux, uy) = ((q.x - p.x) / gap, (q.y - p.y) / gap);
            (Point::new(p.x + ux * rx, p.y + uy * rx), Point::new(q.x - ux * ry, q.y - uy * ry))
        })
        .collect()
}

/// The four candidate pairs of closest points of two apart segments (an end of one with its nearest point on the other).
fn all_closest(a: Point, b: Point, c: Point, d: Point) -> Vec<(Point, Point)> {
    if seg_seg(a, b, c, d) < 1e-12 {
        return vec![];
    }
    vec![(a, closest_on_segment(a, c, d)), (b, closest_on_segment(b, c, d)), (closest_on_segment(c, a, b), c), (closest_on_segment(d, a, b), d)]
}

/// Copper of one net, in one piece, that comes closer to itself than the clearance rule without touching: the narrow gap
/// between a wide track and a thin track that branches off it, between the two legs of a hairpin, or between a track
/// and a pad of its own net. `check` does not look at these (copper of one group may touch, and the router
/// is free to run beside its own net), but the fab's check goes by shapes alone and sees a slit that the etching may
/// close or trap acid in. Gaps that another piece of the same net fills are not slits (the two pieces of a straight
/// run with a short piece between them). Pads of one part are left to the footprint.
pub fn slit_checks(board: &Board) -> Vec<Violation> {
    let r = &board.rules;
    let items = copper_items(board);
    let conn = connectivity(board);
    let grp = &conn.items;
    let is_pad = |i: usize| i >= conn.n_tracks && i < conn.n_tracks + conn.n_pads;
    let mut near: Vec<(usize, usize)> = vec![];
    near_pairs(&items, r.min_clearance, |i, j| near.push((i.min(j), i.max(j))));
    near.sort_unstable();
    let boxes: Vec<(f64, f64, f64, f64)> = items.iter().map(|it| bbox(&it.geo)).collect();
    let mut out = vec![];
    for side in board.copper_layers() {
        for &(i, j) in &near {
            if grp[i] != grp[j] || !(items[i].on(side) && items[j].on(side)) || (is_pad(i) && is_pad(j)) {
                continue;
            }
            let d = dist(&items[i].geo, &items[j].geo);
            if d <= 1e-9 || d + 1e-9 >= r.min_clearance {
                continue; // touching, or far enough
            }
            let cands = gap_points(&items[i].geo, &items[j].geo);
            // the gap must be empty: nothing else of the layer is in the middle of it (for at least one of the closest pairs)
            let empty = cands.iter().find(|(p, q)| {
                ![0.25, 0.5, 0.75].iter().any(|&t| {
                    let m = Point::new(p.x + (q.x - p.x) * t, p.y + (q.y - p.y) * t);
                    let probe = Geo::Cap { a: m, b: m, r: 0.0 };
                    items.iter().enumerate().any(|(k, it)| {
                        let bx = boxes[k];
                        k != i && k != j && it.on(side) && m.x >= bx.0 - 1e-9 && m.x <= bx.1 + 1e-9 && m.y >= bx.2 - 1e-9 && m.y <= bx.3 + 1e-9 && dist(&it.geo, &probe) <= 1e-9
                    })
                })
            });
            let Some((p, q)) = empty else { continue };
            out.push(v(
                format!("{} 同じネットの銅箔どうしの間に、間隔 {:.3}mm (最小 {:.3}mm より狭い) のすき間があります。細いすき間は、エッチングでつぶれたり、酸が溜まったりします。離すか、つないで埋めましょう", side_name(side), d, r.min_clearance),
                Point::new((p.x + q.x) / 2.0, (p.y + q.y) / 2.0),
            ));
        }
    }
    out
}
