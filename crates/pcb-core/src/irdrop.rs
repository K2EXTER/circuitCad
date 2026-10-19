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

/// Partial self-inductance (H) of a straight trace of `len` and `width` (mm) on the copper of `spec`
/// (Terman: 0.2 nH/mm * len * (ln(2 len / (w + t)) + 0.5 + 0.2235 (w + t) / len)). The return path is not considered,
/// so a pair of close traces (supply and ground) is overestimated: the real loop is smaller because their fields cancel.
pub fn track_inductance(len: f64, width: f64, spec: &IrSpec) -> f64 {
    if len <= 1e-6 {
        return 0.0;
    }
    let wt = width.max(0.01) + spec.thickness_um / 1000.0;
    (0.2e-9 * len * ((2.0 * len / wt).ln() + 0.5 + 0.2235 * wt / len)).max(0.0)
}

/// Inductance of a plated hole through the board (H), about 1 nH for a 0.3 mm hole in 1.6 mm.
pub fn via_inductance(drill: f64) -> f64 {
    let (h, d) = (BOARD_MM / 25.4, drill.max(0.1) / 25.4);
    5.08e-9 * h * ((4.0 * h / d).ln() + 1.0)
}

/// Inductance charged for a current that goes through a copper pour (H): the plane is wide, so only a little.
const POUR_HENRY: f64 = 0.3e-9;

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
    /// Inductance of the copper on the lowest-resistance way from the supply pad (H); `None` when not reached.
    pub henry: Option<f64>,
    /// The tracks of that way, in order from the supply pad (for the coupling between wires).
    pub path: Vec<PathSeg>,
}

/// One straight piece of a copper path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathSeg {
    pub a: Point,
    pub b: Point,
    /// Height of the layer above the top copper (mm).
    pub z: f64,
    pub width: f64,
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
        Side::Inner(n) => 1 + n,
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
    /// What each edge is, in the order of `edges` (for the inductance of a path).
    links: Vec<Link>,
}

#[derive(Clone, Copy)]
enum Link {
    Track { len: f64, width: f64 },
    Via(f64),
    Pour,
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

    fn edge(&mut self, a: usize, b: usize, r: f64, link: Link) {
        if a != b {
            self.edges.push((a, b, r.max(1e-6)));
            self.links.push(link);
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
    let layers = board.copper_layers();
    // pads: a through-hole pad is on both layers, joined by its barrel
    let mut pad_node: HashMap<usize, usize> = HashMap::new();
    for &i in &pad_ids {
        let p = &board.pads[i];
        let n = g.node(p.side, p.pos);
        pad_node.insert(i, n);
        if let Some(d) = p.drill {
            // the barrel joins every copper layer, one edge per step between neighbouring layers
            for w in layers.windows(2) {
                let (x, y) = (g.node(w[0], p.pos), g.node(w[1], p.pos));
                g.edge(x, y, via_resistance(d), Link::Via(d));
            }
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
                for w in layers.windows(2) {
                    let (x, y) = (g.node(w[0], v.pos), g.node(w[1], v.pos));
                    if !g.edges.iter().any(|&(e, f, _)| (e == x && f == y) || (e == y && f == x)) {
                        g.edge(x, y, via_resistance(v.drill), Link::Via(v.drill));
                    }
                }
                return g.node(side, v.pos);
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
            g.edge(on[k].1, on[k + 1].1, r, Link::Track { len: on[k + 1].0 - on[k].0, width: w });
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
                g.edge(n, hub, spec.sheet() * (p.dist(c) / a.sqrt() + 0.15), Link::Pour);
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
        let (henry, path) = match ohm {
            Some(_) => {
                let (h, path) = path_inductance(&g, src, pad_node[&i], spec, &layers);
                (Some(h), path)
            }
            None => (None, vec![]),
        };
        out.pads.push(PadDrop { refdes: p.refdes.clone(), pin: p.pin.clone(), pos: p.pos, ohm, drop_v: ohm.map(|r| r * amps), henry, path });
    }
    out
}

/// Inductance of the lowest-resistance way from `src` to `dst`: the tracks on it are cut into straight runs (consecutive
/// segments that go the same way on the same layer); each run is one trace of its length and length-weighted width, and the runs
/// couple to each other (`mutual`), so a corner counts as it does on a real board: nothing at a right angle, a lot at a sharp
/// turn. Plus a via inductance for each hole and a little for each pour.
fn path_inductance(g: &Graph, src: usize, dst: usize, spec: &IrSpec, layers: &[Side]) -> (f64, Vec<PathSeg>) {
    let n = g.pos.len();
    let mut adj: Vec<Vec<(usize, usize)>> = vec![vec![]; n];
    for (k, &(a, b, _)) in g.edges.iter().enumerate() {
        adj[a].push((b, k));
        adj[b].push((a, k));
    }
    let mut dist = vec![f64::INFINITY; n];
    let mut prev: Vec<Option<(usize, usize)>> = vec![None; n];
    let mut done = vec![false; n];
    dist[src] = 0.0;
    // the nets are small (at most MAX_NODES), so a plain O(n^2) Dijkstra is fine
    for _ in 0..n {
        let Some(u) = (0..n).filter(|&i| !done[i] && dist[i].is_finite()).min_by(|&a, &b| dist[a].total_cmp(&dist[b])) else { break };
        done[u] = true;
        for &(v, k) in &adj[u] {
            let d = dist[u] + g.edges[k].2;
            if d < dist[v] {
                dist[v] = d;
                prev[v] = Some((u, k));
            }
        }
    }
    let mut extra = 0.0;
    let mut segs: Vec<PathSeg> = vec![];
    let mut at = dst;
    while let Some((u, k)) = prev[at] {
        match g.links[k] {
            Link::Track { .. } => {
                let width = if let Link::Track { width, .. } = g.links[k] { width } else { 0.0 };
                let side = g.pos[u].0;
                let z = layers.iter().position(|&x| x == side).map_or(0.0, |i| BOARD_MM * i as f64 / (layers.len().max(2) - 1) as f64);
                segs.push(PathSeg { a: g.pos[u].1, b: g.pos[at].1, z, width });
            }
            Link::Via(d) => extra += via_inductance(d),
            Link::Pour => extra += POUR_HENRY,
        }
        at = u;
    }
    segs.reverse();
    let runs = straight_runs(&segs);
    let mut henry = extra;
    for r in &runs {
        let len: f64 = r.iter().map(|s| s.a.dist(s.b)).sum();
        let len_over_w: f64 = r.iter().map(|s| s.a.dist(s.b) / s.width.max(0.01)).sum();
        henry += track_inductance(len, if len_over_w > 0.0 { len / len_over_w } else { 1.0 }, spec);
    }
    for i in 0..runs.len() {
        for j in i + 1..runs.len() {
            henry += 2.0 * mutual(runs[i], runs[j], spec);
        }
    }
    (henry, segs)
}

/// The runs of a path: maximal stretches of consecutive segments that go the same way (within about a degree) on one layer.
fn straight_runs(segs: &[PathSeg]) -> Vec<&[PathSeg]> {
    let mut out = vec![];
    let mut from = 0;
    for i in 1..=segs.len() {
        let same = i < segs.len() && {
            let (a, b) = (&segs[i - 1], &segs[i]);
            let (va, vb) = ((a.b.x - a.a.x, a.b.y - a.a.y), (b.b.x - b.a.x, b.b.y - b.a.y));
            let (la, lb) = ((va.0 * va.0 + va.1 * va.1).sqrt(), (vb.0 * vb.0 + vb.1 * vb.1).sqrt());
            la > 1e-12 && lb > 1e-12 && (va.0 * vb.1 - va.1 * vb.0).abs() / (la * lb) < 0.02 && va.0 * vb.0 + va.1 * vb.1 > 0.0 && (a.z - b.z).abs() < 1e-9
        };
        if !same {
            out.push(&segs[from..i]);
            from = i;
        }
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

/// Series resistance of the board's copper in front of one pad.
#[derive(Clone, Debug, PartialEq)]
pub struct PadSeries {
    pub net: String,
    pub refdes: String,
    pub pin: String,
    pub ohm: f64,
    /// Inductance of the same copper (H); see `track_inductance`.
    pub henry: f64,
    /// The tracks of the way from the supply pad to this pad, in order (see `wiring_couplings`).
    pub path: Vec<PathSeg>,
}

/// For every net with two or more pads: the resistance from its supply pad to each other pad, as a
/// star around the supply pad (exact for a tree of copper, the same worst case as `net_drop` otherwise).
/// Nets the copper does not connect, and nets too large to solve, are left out.
/// Used to simulate the circuit as it is wired on the board.
pub fn wiring_resistances(board: &Board, spec: &IrSpec) -> Vec<PadSeries> {
    let conn = drc::connectivity(board);
    let mut nets: Vec<&str> = board.pads.iter().filter(|p| !p.net.is_empty()).map(|p| p.net.as_str()).collect();
    nets.sort_unstable();
    nets.dedup();
    let mut out = vec![];
    for net in nets {
        let d = net_drop(board, &conn, net, 0.0, spec, None);
        for p in d.pads {
            if let Some(ohm) = p.ohm.filter(|o| *o > 1e-6) {
                out.push(PadSeries { net: net.to_string(), refdes: p.refdes, pin: p.pin, ohm, henry: p.henry.unwrap_or(0.0), path: p.path });
            }
        }
    }
    out
}

/// Magnetic coupling between the copper of two pads that are on different nets, as the coupling factor `k = M / sqrt(L1 L2)`
/// of the two inductors `wiring_resistances` lists. The mutual inductance comes from the Neumann integral over the two
/// paths cut into short pieces (parallel, close traces couple most). With both paths read from the supply pad outward, a
/// supply trace and its return trace have a positive `k`, which makes the loop inductance `L1 + L2 - 2M` smaller than
/// the sum, as on a real board.
#[derive(Clone, Debug, PartialEq)]
pub struct Coupling {
    pub a: (String, String),
    pub b: (String, String),
    pub k: f64,
}

/// Most pads for which couplings are worked out (the number of pairs grows with the square), and most couplings kept.
const MAX_COUPLED_PADS: usize = 200;
const MAX_COUPLINGS: usize = 200;
/// Smaller couplings are left out.
const MIN_K: f64 = 0.03;

/// A short straight piece of a path: where it starts, its vector (mm), its height and the trace width.
struct Piece {
    from: [f64; 2],
    v: [f64; 2],
    z: f64,
    width: f64,
}

fn pieces(path: &[PathSeg]) -> Vec<Piece> {
    let total: f64 = path.iter().map(|s| s.a.dist(s.b)).sum();
    let step = (total / 60.0).clamp(0.3, 2.0);
    let mut out = vec![];
    for s in path {
        let len = s.a.dist(s.b);
        let n = (len / step).ceil().max(1.0) as usize;
        let v = [(s.b.x - s.a.x) / n as f64, (s.b.y - s.a.y) / n as f64];
        for i in 0..n {
            out.push(Piece { from: [s.a.x + v[0] * i as f64, s.a.y + v[1] * i as f64], v, z: s.z, width: s.width });
        }
    }
    out
}

/// Mutual inductance (H) of two straight, parallel filaments `d` apart that cover the axial ranges [0, a] and [lo, hi] (mm):
/// 0.1 nH * (F(hi) - F(hi - a) - F(lo) + F(lo - a)) with F(z) = z asinh(z / d) - sqrt(z^2 + d^2) (Grover).
fn parallel_mutual(a: f64, lo: f64, hi: f64, d: f64) -> f64 {
    let f = |z: f64| z * (z / d).asinh() - (z * z + d * d).sqrt();
    0.1e-9 * (f(hi) - f(hi - a) - f(lo) + f(lo - a))
}

/// Mutual inductance (H) of two paths, summed over pairs of their pieces. Parallel pieces use the exact formula for two
/// filaments at the distance of the traces (softened by the geometric mean distance of a cross-section, 0.2235 (w + t),
/// which also keeps it finite); pieces at an angle use 1e-7 H/m * dl1 . dl2 / r between their midpoints.
fn mutual(a: &[PathSeg], b: &[PathSeg], spec: &IrSpec) -> f64 {
    let (pa, pb) = (pieces(a), pieces(b));
    let t = spec.thickness_um / 1000.0;
    let mut total = 0.0;
    for x in &pa {
        let la = (x.v[0] * x.v[0] + x.v[1] * x.v[1]).sqrt();
        if la < 1e-12 {
            continue;
        }
        let u = [x.v[0] / la, x.v[1] / la];
        for y in &pb {
            let lb = (y.v[0] * y.v[0] + y.v[1] * y.v[1]).sqrt();
            if lb < 1e-12 {
                continue;
            }
            let g = 0.2235 * (0.5 * (x.width + y.width) + t);
            let (rx, ry) = (y.from[0] - x.from[0], y.from[1] - x.from[1]);
            let cross = (x.v[0] * y.v[1] - x.v[1] * y.v[0]) / (la * lb);
            let dz = x.z - y.z;
            if cross.abs() < 1e-6 {
                let (p0, p1) = (rx * u[0] + ry * u[1], (rx + y.v[0]) * u[0] + (ry + y.v[1]) * u[1]);
                let perp = rx * u[1] - ry * u[0];
                let d = (perp * perp + dz * dz + g * g).sqrt();
                let sign = if x.v[0] * y.v[0] + x.v[1] * y.v[1] >= 0.0 { 1.0 } else { -1.0 };
                total += sign * parallel_mutual(la, p0.min(p1), p0.max(p1), d);
            } else {
                let (mx, my) = (rx + 0.5 * y.v[0] - 0.5 * x.v[0], ry + 0.5 * y.v[1] - 0.5 * x.v[1]);
                let dot = x.v[0] * y.v[0] + x.v[1] * y.v[1];
                // mm * mm / mm = mm; 1e-7 H/m = 1e-10 H per mm
                total += 1e-10 * dot / (mx * mx + my * my + dz * dz + g * g).sqrt();
            }
        }
    }
    total
}

/// The couplings between the pads of `series` (from `wiring_resistances`). Pads of the same net are not coupled: their
/// shared stretch is already counted once per pad. Returns nothing for more than `MAX_COUPLED_PADS` pads.
pub fn wiring_couplings(series: &[PadSeries], spec: &IrSpec) -> Vec<Coupling> {
    let pads: Vec<&PadSeries> = series.iter().filter(|p| p.henry > 0.0 && !p.path.is_empty()).collect();
    if pads.len() > MAX_COUPLED_PADS {
        return vec![];
    }
    let mut out = vec![];
    for i in 0..pads.len() {
        for j in i + 1..pads.len() {
            if pads[i].net == pads[j].net {
                continue;
            }
            let m = mutual(&pads[i].path, &pads[j].path, spec);
            let k = (m / (pads[i].henry * pads[j].henry).sqrt()).clamp(-0.9, 0.9);
            if k.abs() >= MIN_K {
                out.push(Coupling { a: (pads[i].refdes.clone(), pads[i].pin.clone()), b: (pads[j].refdes.clone(), pads[j].pin.clone()), k });
            }
        }
    }
    out.sort_by(|a, b| b.k.abs().total_cmp(&a.k.abs()));
    out.truncate(MAX_COUPLINGS);
    out
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
    fn wiring_resistances_list_every_pad_but_the_supply_pad() {
        let b = board(
            vec![pad(0.0, 0.0, "J1", "1", "VCC", None), pad(100.0, 0.0, "U1", "1", "VCC", None), pad(0.0, 20.0, "J1", "2", "GND", None), pad(100.0, 20.0, "U1", "2", "GND", None)],
            vec![track(Side::Top, (0.0, 0.0), (100.0, 0.0), 0.25)],
        );
        let w = wiring_resistances(&b, &IrSpec::default());
        // VCC is wired; GND has no copper, so it is left out instead of getting a made-up value
        assert_eq!(w.len(), 1, "{w:?}");
        assert_eq!((w[0].net.as_str(), w[0].refdes.as_str(), w[0].pin.as_str()), ("VCC", "U1", "1"));
        assert!((w[0].ohm - 0.1966).abs() < 0.002);
    }

    #[test]
    fn a_long_thin_track_has_over_a_hundred_nanohenry_and_a_wide_one_less() {
        let b = board(vec![pad(0.0, 0.0, "J1", "1", "VCC", None), pad(100.0, 0.0, "U1", "1", "VCC", None)], vec![track(Side::Top, (0.0, 0.0), (100.0, 0.0), 0.25)]);
        let w = wiring_resistances(&b, &IrSpec::default());
        // 0.2 nH/mm * 100 mm * (ln(200 / 0.285) + 0.5) = 141 nH
        assert!((w[0].henry * 1e9 - 141.0).abs() < 8.0, "{}", w[0].henry);
        let wide = board(vec![pad(0.0, 0.0, "J1", "1", "VCC", None), pad(100.0, 0.0, "U1", "1", "VCC", None)], vec![track(Side::Top, (0.0, 0.0), (100.0, 0.0), 2.0)]);
        let ww = wiring_resistances(&wide, &IrSpec::default());
        assert!(ww[0].henry < w[0].henry && ww[0].henry > 0.6 * w[0].henry, "width matters only a little: {} vs {}", ww[0].henry, w[0].henry);
        // two half-length tracks end to end are one trace of the total length, not two short ones
        let split = board(vec![pad(0.0, 0.0, "J1", "1", "VCC", None), pad(100.0, 0.0, "U1", "1", "VCC", None)], vec![track(Side::Top, (0.0, 0.0), (50.0, 0.0), 0.25), track(Side::Top, (50.0, 0.0), (100.0, 0.0), 0.25)]);
        let sp = wiring_resistances(&split, &IrSpec::default());
        assert!((sp[0].henry - w[0].henry).abs() < 1e-12, "{} vs {}", sp[0].henry, w[0].henry);
    }

    /// Two 100 mm traces `gap` apart (a supply pad and a return pad, both read from the connector outward).
    fn pair(gap: f64) -> Vec<PadSeries> {
        let b = board(
            vec![
                pad(0.0, 0.0, "J1", "1", "VCC", None),
                pad(100.0, 0.0, "U1", "1", "VCC", None),
                pad(0.0, gap, "J1", "2", "GND", None),
                pad(100.0, gap, "U1", "2", "GND", None),
            ],
            vec![track(Side::Top, (0.0, 0.0), (100.0, 0.0), 0.25), track(Side::Top, (0.0, gap), (100.0, gap), 0.25)],
        );
        wiring_resistances(&b, &IrSpec::default())
    }

    #[test]
    fn close_supply_and_return_traces_couple_and_the_loop_gets_smaller() {
        let s = pair(2.5);
        let k = wiring_couplings(&s, &IrSpec::default());
        assert_eq!(k.len(), 1, "{k:?}");
        // Grover: M = 0.2 nH/mm * l * (ln(l/d + sqrt(1 + (l/d)^2)) - sqrt(1 + (d/l)^2) + d/l) = 68 nH at l = 100 mm, d = 2.5 mm
        let m = k[0].k * (s[0].henry * s[1].henry).sqrt();
        assert!((m * 1e9 - 68.0).abs() < 5.0, "{} nH", m * 1e9);
        assert!(k[0].k > 0.4 && k[0].k < 0.9, "{k:?}");
        let far = wiring_couplings(&pair(20.0), &IrSpec::default());
        assert!(far.is_empty() || far[0].k < k[0].k, "{far:?}");
    }

    /// One case of `tools/inductance_cases.json`: (name, "self" or "mutual", width, polylines from the supply end).
    type Case = (String, String, f64, Vec<Vec<(f64, f64)>>);

    fn cases() -> Vec<Case> {
        let v: serde_json::Value = serde_json::from_str(include_str!("../../../tools/inductance_cases.json")).unwrap();
        v.as_array()
            .unwrap()
            .iter()
            .map(|c| {
                let paths = c["paths"].as_array().unwrap().iter().map(|p| p.as_array().unwrap().iter().map(|q| (q[0].as_f64().unwrap(), q[1].as_f64().unwrap())).collect()).collect();
                (c["name"].as_str().unwrap().to_string(), c["kind"].as_str().unwrap().to_string(), c["w"].as_f64().unwrap(), paths)
            })
            .collect()
    }

    /// The program's inductance for a case, in nH: the self inductance of the first trace, or the mutual inductance of the
    /// pair (0 when the program finds no coupling). Each polyline is one net with a connector pad at its start and a part pad at
    /// its end, and a track per segment.
    fn ours_case(kind: &str, w: f64, paths: &[Vec<(f64, f64)>]) -> f64 {
        let mut pads = vec![];
        let mut tracks = vec![];
        for (k, path) in paths.iter().enumerate() {
            let net = format!("N{k}");
            for (i, (end, r)) in [(path[0], "J"), (path[path.len() - 1], "U")].into_iter().enumerate() {
                let mut p = Pad::new(Point::new(end.0, end.1), PadShape::Circle { d: 0.1 }, None, Side::Top);
                p.refdes = format!("{r}{k}");
                p.pin = format!("{}", i + 1);
                p.net = net.clone();
                pads.push(p);
            }
            for q in path.windows(2) {
                tracks.push(track(Side::Top, q[0], q[1], w));
            }
        }
        let b = board(pads, tracks);
        let series = wiring_resistances(&b, &IrSpec::default());
        let of = |net: &str| series.iter().find(|s| s.net == net).unwrap_or_else(|| panic!("{net} not wired: {series:?}")).henry;
        if kind == "self" {
            return of("N0") * 1e9;
        }
        let k = wiring_couplings(&series, &IrSpec::default());
        k.first().map_or(0.0, |c| c.k * (of("N0") * of("N1")).sqrt() * 1e9)
    }

    /// `cargo test -p pcb-core inductance_cases_table -- --ignored --nocapture`: `case,value_nH` for every case of
    /// `tools/inductance_cases.json` (to compare with FastHenry, see `tools/fasthenry_cases.py`).
    #[test]
    #[ignore]
    fn inductance_cases_table() {
        println!("case,value_nH");
        for (name, kind, w, paths) in cases() {
            println!("{name},{:.3}", ours_case(&kind, w, &paths));
        }
    }

    /// Against FastHenry2 3.6 over every case of `tools/inductance_cases.json` (parallel traces, corners of 45, 90 and 135
    /// degrees, a dogleg and a staircase, traces at an angle and antiparallel, a pair going round a corner). The FastHenry
    /// figures are in `tools/fasthenry_results.csv` (1 kHz, 12 x 4 filaments; see `tools/fasthenry_cases.py`): the self inductance
    /// of a trace must be within 1.5 % and the mutual inductance of a pair within 1 % (or 0.3 nH when it is nearly zero).
    #[test]
    fn inductance_agrees_with_fasthenry() {
        let table: std::collections::HashMap<String, f64> = include_str!("../../../tools/fasthenry_results.csv")
            .lines()
            .skip(1)
            .map(|l| {
                let f: Vec<&str> = l.split(',').collect();
                (f[0].to_string(), f[1].parse().unwrap())
            })
            .collect();
        let all = cases();
        assert_eq!(table.len(), all.len(), "the FastHenry table and the case list must have the same cases");
        for (name, kind, w, paths) in all {
            let fh = table[&name];
            let ours = ours_case(&kind, w, &paths);
            let tol = if kind == "self" { 0.015 * fh } else { (0.01 * fh.abs()).max(0.3) };
            assert!((ours - fh).abs() <= tol, "{name}: {ours} nH vs FastHenry {fh} nH");
        }
    }

    #[test]
    fn a_trace_going_away_at_right_angles_does_not_couple() {
        let b = board(
            vec![pad(0.0, 0.0, "J1", "1", "VCC", None), pad(50.0, 0.0, "U1", "1", "VCC", None), pad(0.0, 5.0, "J2", "1", "SIG", None), pad(0.0, 55.0, "U2", "1", "SIG", None)],
            vec![track(Side::Top, (0.0, 0.0), (50.0, 0.0), 0.25), track(Side::Top, (0.0, 5.0), (0.0, 55.0), 0.25)],
        );
        assert!(wiring_couplings(&wiring_resistances(&b, &IrSpec::default()), &IrSpec::default()).is_empty());
    }

    #[test]
    fn a_via_adds_about_a_nanohenry() {
        let h = via_inductance(0.3);
        assert!((0.8e-9..1.8e-9).contains(&h), "{h}");
    }

    #[test]
    fn via_resistance_is_about_a_milliohm() {
        let r = via_resistance(0.3);
        assert!((0.0008..0.0015).contains(&r), "{r}");
    }
}
