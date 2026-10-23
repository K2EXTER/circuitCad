//! One thick trunk out of two pads of a net that sit side by side (the two LX pins of a regulator): a thin neck leaves each
//! pad, and a trunk as wide as the pair takes over just in front of them. It is the shape that needs no slot to fill and no
//! hollow between two tracks that merge; the necks keep the solder on the small pads (a wide trace straight on a pad can pull
//! the solder off it in the oven).

use crate::drc;
use crate::model::*;

/// The necks are this long (mm) beyond the pads' edge before the trunk starts.
const NECK_LEN: f64 = 0.4;
/// Offset (mm) the two pad centres may differ by across the pair and still count as lined up.
const LINED_UP: f64 = 0.05;

/// What leaves a pad pair.
#[derive(Clone, Debug)]
pub struct Fan {
    /// A neck from the centre of each pad, out in front of it.
    pub necks: [Track; 2],
    /// Where the trunk starts (its round end covers the ends of the necks) and the way it first runs (a unit vector).
    pub start: Point,
    pub dir: Point,
    /// The trunk's width: the necks' outer edges, so that it is flush with them.
    pub width: f64,
}

fn size_along(p: &Pad, vertical_axis: bool) -> (f64, f64) {
    let (w, h) = p.shape.size();
    // (extent along the line through the two pads, extent across it)
    if vertical_axis { (h, w) } else { (w, h) }
}

/// Do pads `i` and `j` make a pair a trunk can leave (same net and face, surface mount, in one row)? The reason if not.
pub fn check_pair(board: &Board, i: usize, j: usize, neck_width: f64) -> Result<(), String> {
    pair(board, i, j, neck_width).map(|_| ())
}

/// What the pair looks like: (is the line through the two pads vertical, the neck width, the pitch, the pads' extent across).
fn pair(board: &Board, i: usize, j: usize, neck_width: f64) -> Result<(bool, f64, f64, f64), String> {
    if i == j {
        return Err("同じパッドが2回選ばれています。もう1つ、別のパッドを選んでください".into());
    }
    let (a, b) = (&board.pads[i], &board.pads[j]);
    if a.net.is_empty() || a.net != b.net {
        return Err(format!(
            "2つのパッドのネットが違います ({} と {})。同じネットのパッドを選んでください",
            if a.net.is_empty() { "なし" } else { &a.net },
            if b.net.is_empty() { "なし" } else { &b.net }
        ));
    }
    if a.side != b.side || a.drill.is_some() || b.drill.is_some() {
        return Err("表面実装の、同じ面のパッドを2つ選んでください (穴のあるパッドは、この機能の対象外です)".into());
    }
    let (dx, dy) = ((b.pos.x - a.pos.x).abs(), (b.pos.y - a.pos.y).abs());
    let vertical_axis = if dx < LINED_UP {
        true
    } else if dy < LINED_UP {
        false
    } else {
        return Err("2つのパッドが、縦か横の1列にそろっていません。ピンが1列に並んでいる、隣り合うパッドを選んでください".into());
    };
    // size_along: (extent along the line through the two pads, extent across it)
    let (along_a, across_a) = size_along(a, vertical_axis);
    let (along_b, across_b) = size_along(b, vertical_axis);
    let pitch = if vertical_axis { dy } else { dx };
    let neck = neck_width.min(along_a.min(along_b)).max(board.rules.min_track_width);
    if pitch - neck < board.rules.min_clearance {
        return Err("パッドの間隔が狭く、2本のネックが重なります。パッドの間隔を確かめてください".into());
    }
    Ok((vertical_axis, neck, pitch, across_a.max(across_b)))
}

/// The neck and trunk for pads `i` and `j` with the trunk heading `toward` a point. `neck_width` is the width the necks
/// would like (they are never wider than the pads' short side).
pub fn fan(board: &Board, i: usize, j: usize, toward: Point, neck_width: f64) -> Result<Fan, String> {
    let (vertical_axis, neck, pitch, across) = pair(board, i, j, neck_width)?;
    let (a, b) = (&board.pads[i], &board.pads[j]);
    // the way out: across the line of the pair, to the side the destination is on
    let mid = Point::new((a.pos.x + b.pos.x) / 2.0, (a.pos.y + b.pos.y) / 2.0);
    let side_of = |v: f64| if v < 0.0 { -1.0 } else { 1.0 };
    let dir = if vertical_axis { Point::new(side_of(toward.x - mid.x), 0.0) } else { Point::new(0.0, side_of(toward.y - mid.y)) };
    // how far the pads reach out along that way (their half size across the line of the pair)
    let half_out = across / 2.0;
    let width = pitch + neck;
    let run = half_out + NECK_LEN;
    let ahead = (toward.x - mid.x) * dir.x + (toward.y - mid.y) * dir.y;
    if ahead < run + width {
        return Err("行き先が、パッドに近すぎます。パッドから、1mm 以上離れた所をクリックしてください".into());
    }
    let q = |p: &Pad| Point::new(p.pos.x + dir.x * run, p.pos.y + dir.y * run);
    let (qa, qb) = (q(a), q(b));
    let start = Point::new((qa.x + qb.x) / 2.0, (qa.y + qb.y) / 2.0);
    let side = a.side;
    Ok(Fan { necks: [Track { side, a: a.pos, b: qa, width: neck }, Track { side, a: b.pos, b: qb, width: neck }], start, dir, width })
}

/// Put the fan and a trunk path (`path`: the points after the trunk's start) into the board, if the DRC finds nothing new.
/// Returns how many tracks were added.
pub fn add(board: &mut Board, fan: &Fan, path: &[Point]) -> Result<usize, String> {
    let side = fan.necks[0].side;
    let mut tracks: Vec<Track> = fan.necks.to_vec();
    let mut from = fan.start;
    for &to in path {
        if from.dist(to) > 1e-9 {
            tracks.push(Track { side, a: from, b: to, width: fan.width });
        }
        from = to;
    }
    let before = drc::check(board).len();
    let mut trial = board.clone();
    let n = tracks.len();
    trial.tracks.extend(tracks);
    let after = drc::check(&trial).len();
    if after > before {
        return Err(format!("間隔が足りなくなるので、引けません (ルール違反が {} 件、増えます)。行き先を変えるか、近くのものを動かしてください", after - before));
    }
    *board = trial;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two LX pins of a QFN: 0.85 x 0.3 pads, 0.65 mm apart, on the left side of the package.
    fn lx_board() -> Board {
        let mut b = Board::new("t", 40.0, 40.0);
        for (y, pin) in [(18.825, "2"), (18.175, "2")] {
            let mut p = Pad::new(Point::new(15.05, y), PadShape::Rect { w: 0.85, h: 0.3 }, None, Side::Top);
            p.refdes = "U1".into();
            p.pin = pin.into();
            p.net = "LX".into();
            b.pads.push(p);
        }
        // the neighbours (VIN) above and below
        for y in [19.475, 17.525] {
            let mut p = Pad::new(Point::new(15.05, y), PadShape::Rect { w: 0.85, h: 0.3 }, None, Side::Top);
            p.refdes = "U1".into();
            p.pin = "1".into();
            p.net = "VIN".into();
            b.pads.push(p);
        }
        b
    }

    #[test]
    fn the_neck_and_trunk_of_two_lx_pads_have_the_expected_shape() {
        let b = lx_board();
        let f = fan(&b, 0, 1, Point::new(10.0, 18.5), 0.25).unwrap();
        assert_eq!((f.dir.x, f.dir.y), (-1.0, 0.0), "out to the left, towards the destination");
        // necks 0.25 wide from the pad centres, 0.4 beyond the pads' left edge
        assert!(f.necks.iter().all(|n| (n.width - 0.25).abs() < 1e-9));
        assert!((f.necks[0].b.x - (15.05 - 0.425 - 0.4)).abs() < 1e-9, "{:?}", f.necks[0]);
        // the trunk is the pair's outer edges: 0.65 + 0.25 = 0.9 wide, centred between the pads
        assert!((f.width - 0.9).abs() < 1e-9);
        assert!((f.start.y - 18.5).abs() < 1e-9);
    }

    #[test]
    fn it_works_in_the_other_direction_and_for_a_row_of_pads_across() {
        let b = lx_board();
        // a destination on the right of the pads: the way out is to the right
        let f = fan(&b, 0, 1, Point::new(25.0, 18.5), 0.25).unwrap();
        assert_eq!((f.dir.x, f.dir.y), (1.0, 0.0));
        // two pads side by side along x (a pair on the top edge of a package) leave upwards or downwards
        let mut h = Board::new("t", 40.0, 40.0);
        for x in [10.0, 10.65] {
            let mut p = Pad::new(Point::new(x, 10.0), PadShape::Rect { w: 0.3, h: 0.85 }, None, Side::Top);
            p.net = "LX".into();
            h.pads.push(p);
        }
        let f = fan(&h, 0, 1, Point::new(10.3, 3.0), 0.25).unwrap();
        assert_eq!((f.dir.x, f.dir.y), (0.0, -1.0));
    }

    #[test]
    fn bad_pairs_are_refused_with_a_reason() {
        let b = lx_board();
        assert!(fan(&b, 0, 0, Point::new(10.0, 18.5), 0.25).unwrap_err().contains("2回"));
        assert!(fan(&b, 0, 2, Point::new(10.0, 18.5), 0.25).unwrap_err().contains("ネットが違い"));
        assert!(fan(&b, 0, 1, Point::new(14.5, 18.5), 0.25).unwrap_err().contains("近すぎ"));
        let mut skew = lx_board();
        skew.pads[1].pos = Point::new(15.5, 18.175);
        assert!(fan(&skew, 0, 1, Point::new(10.0, 18.5), 0.25).unwrap_err().contains("そろっていません"));
    }

    #[test]
    fn the_trunk_is_added_when_the_drc_agrees_and_refused_when_it_would_touch_another_net() {
        let mut b = lx_board();
        let f = fan(&b, 0, 1, Point::new(10.0, 18.5), 0.25).unwrap();
        assert_eq!(add(&mut b, &f, &[Point::new(10.0, 18.5)]).unwrap(), 3, "two necks and the trunk");
        assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
        // the pads and the tracks are one piece of copper now
        assert_eq!(crate::ratsnest::ratsnest(&b).iter().filter(|w| w.net == "LX").count(), 0);
        // a pad of another net in the way of the trunk
        let mut c = lx_board();
        let mut p = Pad::new(Point::new(12.0, 18.5), PadShape::Rect { w: 0.5, h: 0.5 }, None, Side::Top);
        p.net = "X".into();
        c.pads.push(p);
        let f = fan(&c, 0, 1, Point::new(10.0, 18.5), 0.25).unwrap();
        let err = add(&mut c, &f, &[Point::new(10.0, 18.5)]).unwrap_err();
        assert!(err.contains("間隔が足りなく"), "{err}");
        assert!(c.tracks.is_empty(), "nothing was added");
    }
}

#[cfg(test)]
mod neck_tests {
    use super::*;

    /// The necks of a trunk are thin on purpose: the width check looks at the trunk, not at them.
    #[test]
    fn the_width_check_does_not_count_the_necks_of_a_trunk() {
        let mut b = Board::new("t", 40.0, 40.0);
        for y in [18.825, 18.175] {
            let mut p = Pad::new(Point::new(15.05, y), PadShape::Rect { w: 0.85, h: 0.3 }, None, Side::Top);
            p.refdes = "U1".into();
            p.pin = "2".into();
            p.net = "LX".into();
            b.pads.push(p);
        }
        let mut far = Pad::new(Point::new(8.0, 18.5), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
        far.net = "LX".into();
        b.pads.push(far);
        let f = fan(&b, 0, 1, Point::new(10.0, 18.5), 0.25).unwrap();
        add(&mut b, &f, &[Point::new(10.0, 18.5), Point::new(8.0, 18.5)]).unwrap();
        b.net_currents.insert("LX".into(), 1.48);
        let conn = drc::connectivity(&b);
        let w = crate::current::net_widths(&b, &conn, &Default::default());
        assert_eq!(w.len(), 1);
        assert!(!w[0].too_thin(), "the trunk is 0.9 mm and 1.48 A needs {:.2} mm; the necks do not count: {:?}", w[0].needed, w[0]);
        assert!((w[0].thinnest.unwrap() - 0.9).abs() < 1e-9);
        // a plain thin track of the same net is still found
        let mut c = b.clone();
        c.tracks.push(Track { side: Side::Top, a: Point::new(8.0, 18.5), b: Point::new(8.0, 25.0), width: 0.2 });
        let conn = drc::connectivity(&c);
        assert!(crate::current::net_widths(&c, &conn, &Default::default())[0].too_thin());
    }
}

#[cfg(test)]
mod neck_widen_tests {
    use super::*;

    /// "Widen to the current" leaves the necks of a trunk as they are.
    #[test]
    fn widening_a_net_leaves_the_necks_thin() {
        let mut b = Board::new("t", 40.0, 40.0);
        for y in [18.825, 18.175] {
            let mut p = Pad::new(Point::new(15.05, y), PadShape::Rect { w: 0.85, h: 0.3 }, None, Side::Top);
            p.net = "LX".into();
            b.pads.push(p);
        }
        let mut far = Pad::new(Point::new(8.0, 18.5), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
        far.net = "LX".into();
        b.pads.push(far);
        let f = fan(&b, 0, 1, Point::new(10.0, 18.5), 0.25).unwrap();
        add(&mut b, &f, &[Point::new(10.0, 18.5), Point::new(8.0, 18.5)]).unwrap();
        // a thin stretch of the trunk's net that is not a neck is widened
        b.tracks.push(Track { side: Side::Top, a: Point::new(8.0, 18.5), b: Point::new(8.0, 25.0), width: 0.2 });
        let conn = drc::connectivity(&b);
        let n = crate::current::widen_net(&mut b, &conn, "LX", 1.2);
        assert_eq!(n, 3, "the trunk's two pieces (0.9 -> 1.2) and the thin stretch: {:?}", b.tracks.iter().map(|t| t.width).collect::<Vec<_>>());
        assert!(b.tracks.iter().filter(|t| (t.a.x - 15.05).abs() < 1e-9 || (t.b.x - 15.05).abs() < 1e-9).all(|t| (t.width - 0.25).abs() < 1e-9), "the necks are still 0.25");
    }
}

#[cfg(test)]
mod finish_tests {
    use super::*;

    fn lx_board() -> Board {
        let mut b = Board::new("t", 40.0, 40.0);
        for y in [18.825, 18.175] {
            let mut p = Pad::new(Point::new(15.05, y), PadShape::Rect { w: 0.85, h: 0.3 }, None, Side::Top);
            p.refdes = "U1".into();
            p.pin = "2".into();
            p.net = "LX".into();
            b.pads.push(p);
        }
        let mut far = Pad::new(Point::new(10.0, 18.5), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
        far.net = "LX".into();
        b.pads.push(far);
        b
    }

    fn fills_between_the_pads(b: &Board) -> bool {
        let fin = crate::smooth::finished(b).unwrap();
        fin.patches.iter().any(|p| {
            let n = p.outline.len() as f64;
            let (cx, cy) = (p.outline.iter().map(|q| q.x).sum::<f64>() / n, p.outline.iter().map(|q| q.y).sum::<f64>() / n);
            (14.5..=15.6).contains(&cx) && (18.2..=18.8).contains(&cy)
        })
    }

    /// The pads of a trunk stay apart: the finish does not fill the gap between them (the necks are thin so that the solder stays on them).
    #[test]
    fn the_finish_does_not_fill_between_the_pads_of_a_trunk() {
        let mut b = lx_board();
        let f = fan(&b, 0, 1, Point::new(10.0, 18.5), 0.25).unwrap();
        add(&mut b, &f, &[Point::new(10.0, 18.5)]).unwrap();
        b.finish.on = true;
        assert!(!fills_between_the_pads(&b), "the gap between the two pads is left open");
    }

    /// Two ordinary tracks of one width from the same two pads (no trunk: nothing is wider) are finished as before: the slot between them is
    /// filled (with the notch in front of the pads).
    #[test]
    fn two_ordinary_tracks_from_two_pads_are_still_filled_between() {
        let mut b = lx_board();
        b.pads.pop();
        for y in [18.825, 18.175] {
            b.tracks.push(Track { side: Side::Top, a: Point::new(15.05, y), b: Point::new(12.5, y), width: 0.25 });
        }
        b.tracks.push(Track { side: Side::Top, a: Point::new(12.5, 18.175), b: Point::new(12.5, 18.825), width: 0.25 });
        b.finish.on = true;
        let fin = crate::smooth::finished(&b).unwrap();
        assert!(!fin.patches.is_empty(), "the slot between the two tracks is filled");
    }
}
