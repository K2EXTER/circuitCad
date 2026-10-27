//! How good the wiring looks, as numbers: the length, the vias, the bends, the short jogs, which layers the copper runs on, how much of
//! it runs at 45 degrees and how much runs side by side with another net. A person routing by hand keeps the bends and the vias few, uses
//! the layers the way the design asks and runs related lines side by side; the router does not mind. These numbers make the difference
//! visible, so that two routes of one board (or one change to the router) can be compared.
//!
//! A *bend* is a point where exactly two tracks of one layer meet and do not run on in a line (a corner, or a jog). A *short* track is
//! shorter than [`SHORT`] mm (the steps of a staircase).

use crate::drc::{connectivity, Connectivity};
use crate::model::*;
use std::collections::{BTreeMap, HashMap};

/// Tracks shorter than this (mm) count as short (a jog or a step).
pub const SHORT: f64 = 0.4;
/// Two straight tracks of different nets run side by side when they are this close (mm, centre to centre) ...
const SIDE_BY_SIDE: f64 = 0.7;
/// ... over at least this much of their length (mm).
const MIN_OVERLAP: f64 = 0.5;

/// The numbers for the whole board.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Quality {
    /// Track length (mm), in all and per layer name ("表面", "裏面", "内層1" ...: `Side::label`).
    pub length: f64,
    pub by_layer: BTreeMap<String, f64>,
    pub vias: usize,
    pub segments: usize,
    pub bends: usize,
    /// Tracks shorter than [`SHORT`].
    pub short: usize,
    /// Length (mm) at exactly 45 degrees, along an axis, and at any other angle.
    pub diagonal: f64,
    pub straight: f64,
    pub other_angle: f64,
    /// Length (mm) of the axis-aligned tracks that run beside a track of another net on the same layer (a bus).
    pub side_by_side: f64,
}

/// The numbers of one net.
#[derive(Clone, Debug, PartialEq)]
pub struct NetQuality {
    pub net: String,
    pub pads: usize,
    pub length: f64,
    pub vias: usize,
    pub bends: usize,
    pub segments: usize,
    /// Length of the shortest tree of straight lines over the pads (0 for a net of one pad).
    pub shortest: f64,
    /// Length (mm) on each layer, by layer name.
    pub by_layer: BTreeMap<String, f64>,
}

impl NetQuality {
    /// Track length over the shortest tree (1.0 is as short as it can be; bigger is a detour). None when the net has no track or one pad.
    pub fn detour(&self) -> Option<f64> {
        (self.shortest > 1e-9 && self.length > 0.0).then(|| self.length / self.shortest)
    }
}

fn shortest_tree(pts: &[Point]) -> f64 {
    if pts.len() < 2 || pts.len() > 60 {
        return 0.0;
    }
    let mut inside = vec![false; pts.len()];
    inside[0] = true;
    let mut near: Vec<f64> = pts.iter().map(|p| p.dist(pts[0])).collect();
    let mut total = 0.0;
    for _ in 1..pts.len() {
        let Some((k, d)) = near.iter().enumerate().filter(|(i, _)| !inside[*i]).min_by(|a, b| a.1.total_cmp(b.1)).map(|(i, d)| (i, *d)) else { break };
        inside[k] = true;
        total += d;
        for (i, n) in near.iter_mut().enumerate() {
            if !inside[i] {
                *n = n.min(pts[i].dist(pts[k]));
            }
        }
    }
    total
}

/// The bends of every net: (net, number). See the module for what a bend is.
pub fn bends_by_net(board: &Board, conn: &Connectivity) -> HashMap<String, usize> {
    let key = |p: Point, side: Side| ((p.x * 1e3).round() as i64, (p.y * 1e3).round() as i64, side);
    let mut ends: HashMap<(i64, i64, Side), Vec<(usize, Point)>> = HashMap::new();
    for (i, t) in board.tracks.iter().enumerate() {
        ends.entry(key(t.a, t.side)).or_default().push((i, t.b));
        ends.entry(key(t.b, t.side)).or_default().push((i, t.a));
    }
    let mut out: HashMap<String, usize> = HashMap::new();
    for ((x, y, _), v) in &ends {
        if v.len() != 2 || v[0].0 == v[1].0 {
            continue;
        }
        let at = Point::new(*x as f64 / 1e3, *y as f64 / 1e3);
        let (u1, u2) = ((v[0].1.x - at.x, v[0].1.y - at.y), (v[1].1.x - at.x, v[1].1.y - at.y));
        let (l1, l2) = (u1.0.hypot(u1.1), u2.0.hypot(u2.1));
        if l1 < 1e-9 || l2 < 1e-9 {
            continue;
        }
        // a straight continuation has the two directions opposite (cosine -1)
        if (u1.0 * u2.0 + u1.1 * u2.1) / (l1 * l2) > -0.9998
            && let Some(n) = conn.track_net(v[0].0)
        {
            *out.entry(n.to_string()).or_default() += 1;
        }
    }
    out
}

/// The numbers of the board and of each net that has tracks or vias (the nets in name order).
pub fn measure(board: &Board) -> (Quality, Vec<NetQuality>) {
    let conn = connectivity(board);
    let bends = bends_by_net(board, &conn);
    let mut q = Quality { vias: board.vias.len(), segments: board.tracks.len(), ..Default::default() };
    let mut nets: BTreeMap<String, NetQuality> = BTreeMap::new();
    let mut pads: BTreeMap<&str, Vec<Point>> = BTreeMap::new();
    for p in board.pads.iter().filter(|p| !p.net.is_empty()) {
        pads.entry(p.net.as_str()).or_default().push(p.pos);
    }
    let entry = |nets: &mut BTreeMap<String, NetQuality>, n: &str| {
        nets.entry(n.to_string()).or_insert_with(|| NetQuality {
            net: n.to_string(),
            pads: pads.get(n).map_or(0, Vec::len),
            length: 0.0,
            vias: 0,
            bends: 0,
            segments: 0,
            shortest: pads.get(n).map_or(0.0, |p| shortest_tree(p)),
            by_layer: BTreeMap::new(),
        });
    };
    for (i, t) in board.tracks.iter().enumerate() {
        let l = t.a.dist(t.b);
        q.length += l;
        *q.by_layer.entry(t.side.label()).or_default() += l;
        if l < SHORT {
            q.short += 1;
        }
        let (dx, dy) = ((t.b.x - t.a.x).abs(), (t.b.y - t.a.y).abs());
        if dx < 1e-6 || dy < 1e-6 {
            q.straight += l;
        } else if (dx - dy).abs() < 1e-3 * dx.max(dy) + 1e-6 {
            q.diagonal += l;
        } else {
            q.other_angle += l;
        }
        if let Some(n) = conn.track_net(i) {
            entry(&mut nets, n);
            let e = nets.get_mut(n).unwrap();
            e.length += l;
            e.segments += 1;
            *e.by_layer.entry(t.side.label()).or_default() += l;
        }
    }
    for k in 0..board.vias.len() {
        if let Some(n) = conn.via_net(k) {
            entry(&mut nets, n);
            nets.get_mut(n).unwrap().vias += 1;
        }
    }
    for (n, b) in &bends {
        entry(&mut nets, n);
        nets.get_mut(n).unwrap().bends = *b;
        q.bends += b;
    }
    q.side_by_side = side_by_side(board, &conn);
    (q, nets.into_values().collect())
}

/// Length of the axis-aligned tracks that run beside another net's axis-aligned track on the same layer.
fn side_by_side(board: &Board, conn: &Connectivity) -> f64 {
    // (index, side, horizontal, fixed coordinate, lo, hi)
    let mut segs: Vec<(usize, Side, bool, f64, f64, f64)> = vec![];
    for (i, t) in board.tracks.iter().enumerate() {
        if (t.a.y - t.b.y).abs() < 1e-6 && (t.a.x - t.b.x).abs() > 1e-6 {
            segs.push((i, t.side, true, t.a.y, t.a.x.min(t.b.x), t.a.x.max(t.b.x)));
        } else if (t.a.x - t.b.x).abs() < 1e-6 && (t.a.y - t.b.y).abs() > 1e-6 {
            segs.push((i, t.side, false, t.a.x, t.a.y.min(t.b.y), t.a.y.max(t.b.y)));
        }
    }
    // sorted by the fixed coordinate, so that only the neighbours within the pitch are compared
    let layer = |s: Side| match s {
        Side::Top => 0u32,
        Side::Bottom => 1,
        Side::Inner(n) => 2 + u32::from(n),
    };
    segs.sort_by(|a, b| (layer(a.1), a.2).cmp(&(layer(b.1), b.2)).then(a.3.total_cmp(&b.3)));
    let mut total = 0.0;
    for (k, s) in segs.iter().enumerate() {
        let net = conn.track_net(s.0);
        let beside = segs[k + 1..]
            .iter()
            .take_while(|o| o.1 == s.1 && o.2 == s.2 && o.3 - s.3 <= SIDE_BY_SIDE)
            .chain(segs[..k].iter().rev().take_while(|o| o.1 == s.1 && o.2 == s.2 && s.3 - o.3 <= SIDE_BY_SIDE))
            .any(|o| conn.track_net(o.0) != net && o.5.min(s.5) - o.4.max(s.4) > MIN_OVERLAP);
        if beside {
            total += s.5 - s.4;
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(b: &mut Board, x: f64, y: f64, net: &str) {
        let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
        p.net = net.into();
        b.pads.push(p);
    }

    fn line(b: &mut Board, side: Side, pts: &[(f64, f64)]) {
        for w in pts.windows(2) {
            b.tracks.push(Track { side, a: Point::new(w[0].0, w[0].1), b: Point::new(w[1].0, w[1].1), width: 0.25 });
        }
    }

    #[test]
    fn the_numbers_of_a_small_board() {
        let mut b = Board::new("t", 40.0, 30.0);
        for (x, y, n) in [(5.0, 10.0, "N"), (25.0, 10.0, "N"), (5.0, 12.0, "M"), (25.0, 12.0, "M")] {
            pad(&mut b, x, y, n);
        }
        // N: straight, a 45 degree step (two bends, a short track in the middle) and on; M: straight beside it
        line(&mut b, Side::Top, &[(5.0, 10.0), (12.0, 10.0), (12.2, 10.2), (25.0, 10.2)]);
        line(&mut b, Side::Top, &[(5.0, 12.0), (25.0, 12.0)]);
        b.tracks.push(Track { side: Side::Top, a: Point::new(25.0, 10.2), b: Point::new(25.0, 10.0), width: 0.25 });
        let (q, nets) = measure(&b);
        assert_eq!(q.segments, 5);
        assert_eq!(q.vias, 0);
        // N: 5-12 (7), 12-12.2 diagonal, 12.2-25 (12.8), 10.2-10 (0.2): bends at (12,10), (12.2,10.2), (25,10.2)
        assert_eq!(q.bends, 3, "{q:?}");
        assert_eq!(q.short, 2, "the diagonal step and the 0.2 mm stub: {q:?}");
        assert!((q.diagonal - 0.2f64.hypot(0.2)).abs() < 1e-9, "{q:?}");
        assert!((q.length - (7.0 + 0.2f64.hypot(0.2) + 12.8 + 0.2 + 20.0)).abs() < 1e-9, "{q:?}");
        assert_eq!(q.by_layer.len(), 1);
        // the long straight tracks of the two nets are 2 mm apart: not side by side (the pitch is 0.7)
        assert_eq!(q.side_by_side, 0.0);
        let n = nets.iter().find(|n| n.net == "N").unwrap();
        assert_eq!((n.pads, n.bends, n.segments), (2, 3, 4));
        assert!(n.detour().unwrap() > 1.0 && n.detour().unwrap() < 1.05, "{:?}", n.detour());
        let m = nets.iter().find(|n| n.net == "M").unwrap();
        assert_eq!((m.bends, m.segments, m.vias), (0, 1, 0));
        assert!((m.detour().unwrap() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn tracks_of_two_nets_at_a_bus_pitch_count_as_side_by_side() {
        let mut b = Board::new("t", 40.0, 30.0);
        // (small pads: the two nets must not touch)
        for (x, y, n) in [(5.0, 10.0, "A"), (25.0, 10.0, "A"), (5.0, 10.5, "B"), (25.0, 10.5, "B")] {
            let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w: 0.2, h: 0.2 }, None, Side::Top);
            p.net = n.into();
            b.pads.push(p);
        }
        line(&mut b, Side::Top, &[(5.0, 10.0), (25.0, 10.0)]);
        line(&mut b, Side::Top, &[(5.0, 10.5), (25.0, 10.5)]);
        let (q, _) = measure(&b);
        assert!((q.side_by_side - 40.0).abs() < 1e-9, "{q:?}");
        // the same on two layers: not beside each other
        let mut c = Board::new("t", 40.0, 30.0);
        line(&mut c, Side::Top, &[(5.0, 10.0), (25.0, 10.0)]);
        line(&mut c, Side::Bottom, &[(5.0, 10.5), (25.0, 10.5)]);
        assert_eq!(measure(&c).0.side_by_side, 0.0);
    }
}
