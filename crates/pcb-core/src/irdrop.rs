//! Voltage drop along a net's copper: the tracks, vias and pads of a net become a network of
//! resistors, and the resistance from the supply pad to every other pad of the net says how much
//! voltage the wiring eats at the net's declared current (`Board::net_currents`).
//!
//! Assumptions (kept simple on purpose, shown to the user):
//! * the current enters at one pad (a connector if there is one, else the first pad of the net) and every other
//!   pad is looked at as if it alone drew the whole current, which is the worst case for that pad;
//! * a copper pour is a hub reached through a short resistance per pad, so a pour lowers the drop a lot but not to zero.

use std::collections::HashMap;

use crate::drc::{self, Connectivity};
use crate::model::*;

/// Resistivity of copper at about 25 C, in ohm * mm.
const RHO_MM: f64 = 1.72e-5;
/// Board thickness a via barrel crosses (mm).
const BOARD_MM: f64 = 1.6;
/// Plating thickness of a via barrel (mm).
const PLATING_MM: f64 = 0.025;
/// Largest network that is solved (dense solve); bigger nets are reported as too large.
const MAX_NODES: usize = 900;

/// Copper conditions for the resistance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IrSpec {
    /// Copper thickness (um): 35 = 1 oz.
    pub thickness_um: f64,
}

impl Default for IrSpec {
    fn default() -> Self {
        Self { thickness_um: 35.0 }
    }
}

impl IrSpec {
    /// Sheet resistance (ohm per square).
    fn sheet(&self) -> f64 {
        RHO_MM / (self.thickness_um / 1000.0)
    }
}

/// Resistance of a track of `len` and `width` (mm).
pub fn track_resistance(len: f64, width: f64, spec: &IrSpec) -> f64 {
    (spec.sheet() * len / width.max(0.01)).max(1e-6)
}

/// Resistance of a plated hole of `drill` (mm) from the top to the bottom layer.
pub fn via_resistance(drill: f64) -> f64 {
    let area = std::f64::consts::PI * (drill + PLATING_MM) * PLATING_MM;
    RHO_MM * BOARD_MM / area
}

#[derive(Clone, Debug, PartialEq)]
pub struct PadDrop {
    pub refdes: String,
    pub pin: String,
    pub pos: Point,
    /// Resistance from the supply pad (ohm); `None` when the copper does not reach this pad.
    pub ohm: Option<f64>,
    /// Voltage lost on the way at the net's current (V).
    pub drop_v: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NetDrop {
    pub net: String,
    pub amps: f64,
    /// The pad the current is assumed to enter at: (refdes, pin).
    pub source: (String, String),
    /// The supply pad was chosen by the program, not by the user.
    pub source_auto: bool,
    pub pads: Vec<PadDrop>,
    pub has_zone: bool,
    /// Why nothing could be computed (net too large, no pads ...).
    pub note: Option<String>,
}

impl NetDrop {
    /// The pad with the largest drop.
    pub fn worst(&self) -> Option<&PadDrop> {
        self.pads.iter().filter(|p| p.drop_v.is_some()).max_by(|a, b| a.drop_v.partial_cmp(&b.drop_v).unwrap())
    }
}

fn side_id(s: Side) -> u8 {
    match s {
        Side::Top => 0,
        Side::Bottom => 1,
    }
}

type Key = (u8, i64, i64);

fn key(side: Side, p: Point) -> Key {
    (side_id(side), (p.x * 1000.0).round() as i64, (p.y * 1000.0).round() as i64)
}

#[derive(Default)]
struct Graph {
    nodes: HashMap<Key, usize>,
    pos: Vec<(Side, Point)>,
    edges: Vec<(usize, usize, f64)>,
}

impl Graph {
    fn node(&mut self, side: Side, p: Point) -> usize {
        let k = key(side, p);
        if let Some(&i) = self.nodes.get(&k) {
            return i;
        }
        let i = self.pos.len();
        self.nodes.insert(k, i);
        self.pos.push((side, p));
        i
    }

    fn edge(&mut self, a: usize, b: usize, r: f64) {
        if a != b {
            self.edges.push((a, b, r.max(1e-6)));
        }
    }
}

fn pad_contains(p: &Pad, q: Point, slack: f64) -> bool {
    let (w, h) = p.shape.size();
    (q.x - p.pos.x).abs() <= w / 2.0 + slack && (q.y - p.pos.y).abs() <= h / 2.0 + slack
}

fn in_polygon(poly: &[Point], q: Point) -> bool {
    let mut inside = false;
    let mut j = poly.len().wrapping_sub(1);
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > q.y) != (b.y > q.y) && q.x < (b.x - a.x) * (q.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn area(poly: &[Point]) -> f64 {
    let mut s = 0.0;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        s += a.x * b.y - b.x * a.y;
    }
    s.abs() / 2.0
}

/// Is this reference designator a connector / battery / switch, i.e. where power usually comes in?
fn looks_like_supply(refdes: &str) -> bool {
    let up = refdes.to_ascii_uppercase();
    ["J", "P", "CN", "X", "BT", "SW"].iter().any(|p| up.starts_with(p)) && up.chars().last().is_some_and(|c| c.is_ascii_digit())
}

/// Voltage drop of one net. `source` = (refdes, pin) forces the supply pad.
pub fn net_drop(board: &Board, conn: &Connectivity, net: &str, amps: f64, spec: &IrSpec, source: Option<(&str, &str)>) -> NetDrop {
    let pad_ids: Vec<usize> = (0..board.pads.len()).filter(|&i| board.pads[i].net == net).collect();
    let mut out = NetDrop { net: net.into(), amps, source: (String::new(), String::new()), source_auto: true, pads: vec![], has_zone: false, note: None };
    if pad_ids.len() < 2 {
        out.note = Some("パッドが 2 つ以上ないので、電圧降下は調べられません".into());
        return out;
    }
    // supply pad
    let src_id = source
        .and_then(|(r, p)| pad_ids.iter().copied().find(|&i| board.pads[i].refdes == r && board.pads[i].pin == p))
        .inspect(|_| out.source_auto = false)
        .or_else(|| pad_ids.iter().copied().find(|&i| looks_like_supply(&board.pads[i].refdes)))
        .unwrap_or(pad_ids[0]);
    out.source = (board.pads[src_id].refdes.clone(), board.pads[src_id].pin.clone());

    let mut g = Graph::default();
    // pads: a through-hole pad is on both layers, joined by its barrel
    let mut pad_node: HashMap<usize, usize> = HashMap::new();
    for &i in &pad_ids {
        let p = &board.pads[i];
        let n = g.node(p.side, p.pos);
        pad_node.insert(i, n);
        if let Some(d) = p.drill {
            let m = g.node(p.side.flip(), p.pos);
            g.edge(n, m, via_resistance(d));
        }
    }
    // tracks of the net; the end of a track snaps to a pad or via it touches
    let snap = |g: &mut Graph, side: Side, q: Point| -> usize {
        for &i in &pad_ids {
            let p = &board.pads[i];
            if (p.side == side || p.drill.is_some()) && pad_contains(p, q, 0.01) {
                return g.node(side, p.pos);
            }
        }
        for v in &board.vias {
            if v.pos.dist(q) <= v.diameter / 2.0 + 0.01 {
                let (a, b) = (g.node(side, v.pos), g.node(side.flip(), v.pos));
                if !g.edges.iter().any(|&(x, y, _)| (x == a && y == b) || (x == b && y == a)) {
                    g.edge(a, b, via_resistance(v.drill));
                }
                return a;
            }
        }
        g.node(side, q)
    };
    let mut segs: Vec<(Side, usize, usize, f64, Point, Point)> = vec![];
    for (i, t) in board.tracks.iter().enumerate() {
        if conn.track_net(i) != Some(net) {
            continue;
        }
        let (a, b) = (snap(&mut g, t.side, t.a), snap(&mut g, t.side, t.b));
        segs.push((t.side, a, b, t.width, t.a, t.b));
    }
    // a node that lies along a track (a T junction) splits that track there
    let all: Vec<(usize, Side, Point)> = g.pos.iter().enumerate().map(|(i, &(s, p))| (i, s, p)).collect();
    for &(side, a, b, w, pa, pb) in &segs {
        let len = pa.dist(pb);
        let mut on: Vec<(f64, usize)> = vec![(0.0, a), (len, b)];
        if len > 1e-9 {
            for &(n, s, p) in &all {
                if s != side || n == a || n == b {
                    continue;
                }
                let t = ((p.x - pa.x) * (pb.x - pa.x) + (p.y - pa.y) * (pb.y - pa.y)) / len;
                let perp = (((p.x - pa.x) * (pb.y - pa.y) - (p.y - pa.y) * (pb.x - pa.x)) / len).abs();
                if t > 1e-6 && t < len - 1e-6 && perp <= (w / 2.0).max(0.01) {
                    on.push((t, n));
                }
            }
        }
        on.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
        for k in 0..on.len() - 1 {
            let r = track_resistance(on[k + 1].0 - on[k].0, w, spec);
            g.edge(on[k].1, on[k + 1].1, r);
        }
    }
    // copper pours: every node under the pour reaches a hub through a short resistance
    for z in board.zones.iter().filter(|z| z.net == net && z.polygon.len() >= 3) {
        let a = area(&z.polygon).max(1.0);
        let c = Point::new(z.polygon.iter().map(|p| p.x).sum::<f64>() / z.polygon.len() as f64, z.polygon.iter().map(|p| p.y).sum::<f64>() / z.polygon.len() as f64);
        let hub = g.pos.len();
        g.pos.push((z.side, c));
        let mut used = false;
        for (n, s, p) in all.iter().copied() {
            if s == z.side && in_polygon(&z.polygon, p) {
                g.edge(n, hub, spec.sheet() * (p.dist(c) / a.sqrt() + 0.15));
                used = true;
            }
        }
        out.has_zone |= used;
    }
    if g.pos.len() > MAX_NODES {
        out.note = Some(format!("配線が細かすぎて ({} 点) 調べられません", g.pos.len()));
        return out;
    }

    // solve: ground the supply node, push 1 A into each target in turn; the target's voltage is its resistance
    let src = pad_node[&src_id];
    let n = g.pos.len();
    let mut adj = vec![vec![]; n];
    for &(a, b, _) in &g.edges {
        adj[a].push(b);
        adj[b].push(a);
    }
    let mut seen = vec![false; n];
    let mut stack = vec![src];
    seen[src] = true;
    while let Some(x) = stack.pop() {
        for &y in &adj[x] {
            if !seen[y] {
                seen[y] = true;
                stack.push(y);
            }
        }
    }
    let idx: Vec<usize> = (0..n).filter(|&i| seen[i] && i != src).collect();
    let mut at = vec![usize::MAX; n];
    for (k, &i) in idx.iter().enumerate() {
        at[i] = k;
    }
    let m = idx.len();
    let targets: Vec<usize> = pad_ids.iter().copied().filter(|&i| i != src_id && seen[pad_node[&i]] && pad_node[&i] != src).collect();
    // reduced Laplacian [m x m] and one right-hand side per target
    let mut a = vec![0.0f64; m * m];
    for &(x, y, r) in &g.edges {
        if !(seen[x] && seen[y]) {
            continue;
        }
        let c = 1.0 / r;
        for (p, q) in [(x, y), (y, x)] {
            if p != src {
                a[at[p] * m + at[p]] += c;
                if q != src {
                    a[at[p] * m + at[q]] -= c;
                }
            }
        }
    }
    let mut rhs = vec![0.0f64; m * targets.len()];
    for (k, &t) in targets.iter().enumerate() {
        rhs[at[pad_node[&t]] * targets.len() + k] = 1.0;
    }
    solve(&mut a, &mut rhs, m, targets.len());
    let res: HashMap<usize, f64> = targets.iter().enumerate().map(|(k, &t)| (t, rhs[at[pad_node[&t]] * targets.len() + k])).collect();

    for &i in &pad_ids {
        if i == src_id {
            continue;
        }
        let p = &board.pads[i];
        let ohm = res.get(&i).copied();
        out.pads.push(PadDrop { refdes: p.refdes.clone(), pin: p.pin.clone(), pos: p.pos, ohm, drop_v: ohm.map(|r| r * amps) });
    }
    out
}

/// Gaussian elimination with partial pivoting: solves `a x = b` for `k` right-hand sides in place (`b` is m x k).
fn solve(a: &mut [f64], b: &mut [f64], m: usize, k: usize) {
    for c in 0..m {
        let piv = (c..m).max_by(|&x, &y| a[x * m + c].abs().partial_cmp(&a[y * m + c].abs()).unwrap()).unwrap_or(c);
        if piv != c {
            for j in 0..m {
                a.swap(c * m + j, piv * m + j);
            }
            for j in 0..k {
                b.swap(c * k + j, piv * k + j);
            }
        }
        let d = a[c * m + c];
        if d.abs() < 1e-300 {
            continue;
        }
        for r in c + 1..m {
            let f = a[r * m + c] / d;
            if f != 0.0 {
                for j in c..m {
                    a[r * m + j] -= f * a[c * m + j];
                }
                for j in 0..k {
                    b[r * k + j] -= f * b[c * k + j];
                }
            }
        }
    }
    for c in (0..m).rev() {
        let d = a[c * m + c];
        for j in 0..k {
            let mut s = b[c * k + j];
            for q in c + 1..m {
                s -= a[c * m + q] * b[q * k + j];
            }
            b[c * k + j] = if d.abs() < 1e-300 { 0.0 } else { s / d };
        }
    }
}

/// The drop of every net that has a declared current.
pub fn net_drops(board: &Board, spec: &IrSpec) -> Vec<NetDrop> {
    let conn = drc::connectivity(board);
    board.net_currents.iter().filter(|(_, a)| **a > 0.0).map(|(net, a)| net_drop(board, &conn, net, *a, spec, None)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(x: f64, y: f64, refdes: &str, pin: &str, net: &str, drill: Option<f64>) -> Pad {
        let mut p = Pad::new(Point::new(x, y), PadShape::Circle { d: 1.6 }, drill, Side::Top);
        p.refdes = refdes.into();
        p.pin = pin.into();
        p.net = net.into();
        p
    }

    fn track(side: Side, a: (f64, f64), b: (f64, f64), w: f64) -> Track {
        Track { side, a: Point::new(a.0, a.1), b: Point::new(b.0, b.1), width: w }
    }

    fn board(pads: Vec<Pad>, tracks: Vec<Track>) -> Board {
        let mut b = Board::new("t", 200.0, 100.0);
        b.pads = pads;
        b.tracks = tracks;
        b
    }

    fn drops(b: &Board, amps: f64) -> NetDrop {
        let conn = drc::connectivity(b);
        net_drop(b, &conn, "VCC", amps, &IrSpec::default(), None)
    }

    #[test]
    fn one_straight_track_is_rho_l_over_w_t() {
        let b = board(vec![pad(0.0, 0.0, "J1", "1", "VCC", None), pad(100.0, 0.0, "U1", "1", "VCC", None)], vec![track(Side::Top, (0.0, 0.0), (100.0, 0.0), 0.25)]);
        let d = drops(&b, 1.0);
        assert_eq!(d.source.0, "J1");
        let r = d.pads[0].ohm.unwrap();
        // 1.72e-5 * 100 / (0.25 * 0.035) = 0.1966 ohm
        assert!((r - 0.1966).abs() < 0.002, "{r}");
        assert!((d.pads[0].drop_v.unwrap() - r).abs() < 1e-9, "1 A: volts equal ohms");
    }

    #[test]
    fn twice_as_wide_halves_the_drop_and_current_scales_it() {
        let thin = board(vec![pad(0.0, 0.0, "J1", "1", "VCC", None), pad(50.0, 0.0, "U1", "1", "VCC", None)], vec![track(Side::Top, (0.0, 0.0), (50.0, 0.0), 0.25)]);
        let wide = board(vec![pad(0.0, 0.0, "J1", "1", "VCC", None), pad(50.0, 0.0, "U1", "1", "VCC", None)], vec![track(Side::Top, (0.0, 0.0), (50.0, 0.0), 0.5)]);
        let (a, b) = (drops(&thin, 1.0).pads[0].drop_v.unwrap(), drops(&wide, 1.0).pads[0].drop_v.unwrap());
        assert!((a / b - 2.0).abs() < 1e-6, "{a} {b}");
        assert!((drops(&thin, 2.0).pads[0].drop_v.unwrap() / a - 2.0).abs() < 1e-6);
    }

    #[test]
    fn a_branch_that_is_not_the_path_adds_nothing_and_a_t_junction_splits_the_track() {
        // supply at 0, load at 100, and a third pad tapped half way along the track
        let b = board(
            vec![pad(0.0, 0.0, "J1", "1", "VCC", None), pad(100.0, 0.0, "U1", "1", "VCC", None), pad(50.0, 10.0, "U2", "1", "VCC", None)],
            vec![track(Side::Top, (0.0, 0.0), (100.0, 0.0), 0.25), track(Side::Top, (50.0, 0.0), (50.0, 10.0), 0.25)],
        );
        let d = drops(&b, 1.0);
        let far = d.pads.iter().find(|p| p.refdes == "U1").unwrap().ohm.unwrap();
        let mid = d.pads.iter().find(|p| p.refdes == "U2").unwrap().ohm.unwrap();
        assert!((far - 0.1966).abs() < 0.002, "{far}: the stub must not change the main path");
        // half way (0.0983) plus the 10 mm stub (0.0197)
        assert!((mid - 0.118).abs() < 0.002, "{mid}");
    }

    #[test]
    fn a_via_adds_a_small_resistance_and_a_gap_is_reported() {
        let mut b = board(
            vec![pad(0.0, 0.0, "J1", "1", "VCC", None), pad(20.0, 0.0, "U1", "1", "VCC", None), pad(60.0, 0.0, "U2", "1", "VCC", None)],
            vec![track(Side::Top, (0.0, 0.0), (10.0, 0.0), 0.5), track(Side::Bottom, (10.0, 0.0), (20.0, 0.0), 0.5)],
        );
        b.pads[1].side = Side::Bottom;
        b.vias.push(Via { pos: Point::new(10.0, 0.0), diameter: 0.6, drill: 0.3 });
        let d = drops(&b, 1.0);
        let r = d.pads.iter().find(|p| p.refdes == "U1").unwrap().ohm.unwrap();
        let tracks_only = 2.0 * track_resistance(10.0, 0.5, &IrSpec::default());
        assert!(r > tracks_only && r < tracks_only + 0.01, "{r} vs {tracks_only}");
        assert!(d.pads.iter().find(|p| p.refdes == "U2").unwrap().ohm.is_none(), "not wired");
    }

    #[test]
    fn a_pour_lowers_the_drop() {
        let mut b = board(vec![pad(0.0, 0.0, "J1", "1", "VCC", None), pad(80.0, 0.0, "U1", "1", "VCC", None)], vec![track(Side::Top, (0.0, 0.0), (80.0, 0.0), 0.25)]);
        let before = drops(&b, 1.0).pads[0].ohm.unwrap();
        b.zones.push(Zone::new("VCC", Side::Top, vec![Point::new(-5.0, -20.0), Point::new(85.0, -20.0), Point::new(85.0, 20.0), Point::new(-5.0, 20.0)]));
        let d = drops(&b, 1.0);
        assert!(d.has_zone);
        assert!(d.pads[0].ohm.unwrap() < before / 3.0, "{} vs {before}", d.pads[0].ohm.unwrap());
    }

    #[test]
    fn via_resistance_is_about_a_milliohm() {
        let r = via_resistance(0.3);
        assert!((0.0008..0.0015).contains(&r), "{r}");
    }
}
