use super::*;

/// A convex outline of the segment `a`-`b` swept by a disc of radius `r` (a track, a round or oval pad, a via).
pub(super) fn capsule(a: Point, b: Point, r: f64) -> Vec<Point> {
    pub(super) const N: usize = 8;
    let th = if (b.x - a.x).hypot(b.y - a.y) < 1e-9 { 0.0 } else { (b.y - a.y).atan2(b.x - a.x) };
    let half = std::f64::consts::FRAC_PI_2;
    let mut out = Vec::with_capacity(2 * N + 2);
    for (c, from) in [(b, th - half), (a, th + half)] {
        for k in 0..=N {
            let t = from + std::f64::consts::PI * k as f64 / N as f64;
            out.push(Point::new(c.x + r * t.cos(), c.y + r * t.sin()));
        }
    }
    out
}

/// The copper of net `net` (tracks, pads, vias, and what the zones join to it) as triangles per face. Inner
/// layers are not visible from outside, so they are left out.
pub(super) fn net_glow(board: &Board, conn: &drc::Connectivity, net: &str) -> Vec<(bool, [Point; 3])> {
    let mut out: Vec<(bool, [Point; 3])> = vec![];
    let mut add = |bottom: bool, poly: &[Point]| {
        for k in 1..poly.len().saturating_sub(1) {
            out.push((bottom, [poly[0], poly[k], poly[k + 1]]));
        }
    };
    for (i, t) in board.tracks.iter().enumerate() {
        if conn.track_net(i) == Some(net) {
            match t.side {
                Side::Top => add(false, &capsule(t.a, t.b, t.width / 2.0)),
                Side::Bottom => add(true, &capsule(t.a, t.b, t.width / 2.0)),
                Side::Inner(_) => {}
            }
        }
    }
    for (i, p) in board.pads.iter().enumerate() {
        if conn.pad_net(i) != Some(net) {
            continue;
        }
        let poly = match p.shape {
            PadShape::Circle { d } => capsule(p.pos, p.pos, d / 2.0),
            PadShape::Rect { w, h } => vec![Point::new(p.pos.x - w / 2.0, p.pos.y - h / 2.0), Point::new(p.pos.x + w / 2.0, p.pos.y - h / 2.0), Point::new(p.pos.x + w / 2.0, p.pos.y + h / 2.0), Point::new(p.pos.x - w / 2.0, p.pos.y + h / 2.0)],
            PadShape::Oval { w, h } if w >= h => {
                let d = (w - h) / 2.0;
                capsule(Point::new(p.pos.x - d, p.pos.y), Point::new(p.pos.x + d, p.pos.y), h / 2.0)
            }
            PadShape::Oval { w, h } => {
                let d = (h - w) / 2.0;
                capsule(Point::new(p.pos.x, p.pos.y - d), Point::new(p.pos.x, p.pos.y + d), w / 2.0)
            }
        };
        let faces: &[bool] = match (p.drill.is_some(), p.side) {
            (true, _) => &[false, true],
            (false, Side::Top) => &[false],
            (false, Side::Bottom) => &[true],
            _ => &[],
        };
        for &f in faces {
            add(f, &poly);
        }
    }
    for (i, v) in board.vias.iter().enumerate() {
        if conn.via_net(i) == Some(net) {
            let poly = capsule(v.pos, v.pos, v.diameter / 2.0);
            add(false, &poly);
            add(true, &poly);
        }
    }
    out
}
