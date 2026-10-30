//! The copper path between two pads: which tracks and vias one follows to get from one to the other, how long it is, how many vias it
//! passes and on which layers it runs. The building block of the checks that look at a connection and not only at a net (a sense wire that
//! leaves its pad on its own, a track without vias, a connection of at most so many mm).
//!
//! Only tracks, vias and pads count; a pour is not walked (two pads that only a pour joins have no path). A track is cut where another
//! track's end, a pad or a via touches it, so a branch is a point where paths part. When copper makes a ring the shortest way is taken.
//! Two tracks that cross in the middle with no end on the other are not joined here (the DRC's `connectivity` joins them).

use super::*;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

/// A pad by part and pin ("RS1.1").
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PadRef {
    pub refdes: String,
    pub pin: String,
}

impl PadRef {
    /// "RS1.1" -> part RS1, pin 1 (the last dot divides, so a part name may hold one).
    pub fn parse(text: &str) -> Option<PadRef> {
        let (r, p) = text.trim().rsplit_once('.')?;
        (!r.is_empty() && !p.is_empty()).then(|| PadRef { refdes: r.to_string(), pin: p.to_string() })
    }
}

impl std::fmt::Display for PadRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.refdes, self.pin)
    }
}

/// The way along the copper from one pad to another.
#[derive(Clone, Debug, PartialEq)]
pub struct CopperPath {
    /// Length (mm) along the tracks (inside a pad or a via the way costs nothing).
    pub length: f64,
    /// How many vias it passes (a through-hole pad that changes the layer is not a via).
    pub vias: usize,
    /// The layers it runs on, in order, one entry for each change.
    pub layers: Vec<Side>,
    /// The points of the way, layer by layer, from the first pad to the last.
    pub points: Vec<(Side, Point)>,
}

/// Where the ways from one pad to two others part.
#[derive(Clone, Debug, PartialEq)]
pub struct Fork {
    /// How far (mm) the two ways run together from the pad before they part (0: they part at the pad).
    pub length: f64,
    /// Where they part.
    pub at: Point,
    /// The lengths (mm) of the two ways.
    pub a_length: f64,
    pub b_length: f64,
}

struct Edge {
    to: usize,
    /// In millionths of a mm (a whole number, so that the search has no rounding to worry about).
    cost: u64,
    via: bool,
}

struct TrackInfo {
    layer: usize,
    a: Point,
    b: Point,
    width: f64,
    /// The places where something joins the track: (position along it from 0 to 1, the node there).
    marks: Vec<(f64, usize)>,
}

/// The copper of a board as a graph of points, built once for many questions.
pub struct CopperGraph {
    layers: Vec<Side>,
    nodes: Vec<(Side, Point)>,
    adj: Vec<Vec<Edge>>,
    pads: HashMap<PadRef, Vec<usize>>,
}

const SCALE: f64 = 1e6;

/// A uniform grid of boxes: which items lie near a place. The search for touching copper asks it instead of looking at every pair.
struct Cells {
    cell: f64,
    x0: f64,
    y0: f64,
    nx: usize,
    ny: usize,
    cells: Vec<Vec<u32>>,
}

type Box4 = (f64, f64, f64, f64);

impl Cells {
    fn new(lo: Point, hi: Point, cell: f64) -> Cells {
        let mut cell = cell;
        let size = |c: f64| (((hi.x - lo.x) / c).floor() as usize + 1, ((hi.y - lo.y) / c).floor() as usize + 1);
        while size(cell).0 * size(cell).1 > 250_000 {
            cell *= 2.0;
        }
        let (nx, ny) = size(cell);
        Cells { cell, x0: lo.x, y0: lo.y, nx, ny, cells: vec![vec![]; nx * ny] }
    }

    fn span(&self, b: Box4) -> (usize, usize, usize, usize) {
        let cx = |x: f64| (((x - self.x0) / self.cell).floor().max(0.0) as usize).min(self.nx - 1);
        let cy = |y: f64| (((y - self.y0) / self.cell).floor().max(0.0) as usize).min(self.ny - 1);
        (cx(b.0), cy(b.1), cx(b.2), cy(b.3))
    }

    fn insert(&mut self, id: u32, b: Box4) {
        let (x0, y0, x1, y1) = self.span(b);
        for iy in y0..=y1 {
            for ix in x0..=x1 {
                let k = iy * self.nx + ix;
                self.cells[k].push(id);
            }
        }
    }

    /// The ids of the boxes that may touch `b`, in order, each once.
    fn query(&self, b: Box4, out: &mut Vec<u32>) {
        out.clear();
        let (x0, y0, x1, y1) = self.span(b);
        for iy in y0..=y1 {
            for ix in x0..=x1 {
                out.extend_from_slice(&self.cells[iy * self.nx + ix]);
            }
        }
        out.sort_unstable();
        out.dedup();
    }
}

fn micro(mm: f64) -> u64 {
    (mm * SCALE).round().max(0.0) as u64
}

struct Builder<'a> {
    board: &'a Board,
    g: CopperGraph,
    tracks: Vec<TrackInfo>,
    /// Nodes made for a position on a track (one for each place, whoever asks).
    at: HashMap<(usize, i64, i64), usize>,
}

impl Builder<'_> {
    fn new_node(&mut self, layer: usize, p: Point) -> usize {
        self.g.nodes.push((self.g.layers[layer], p));
        self.g.adj.push(vec![]);
        self.g.nodes.len() - 1
    }

    fn node_at(&mut self, layer: usize, p: Point) -> usize {
        let key = (layer, (p.x * 1e4).round() as i64, (p.y * 1e4).round() as i64);
        if let Some(&n) = self.at.get(&key) {
            return n;
        }
        let n = self.new_node(layer, p);
        self.at.insert(key, n);
        n
    }

    fn link(&mut self, a: usize, b: usize, mm: f64, via: bool) {
        if a == b {
            return;
        }
        let cost = micro(mm);
        self.g.adj[a].push(Edge { to: b, cost, via });
        self.g.adj[b].push(Edge { to: a, cost, via });
    }

    /// The node where `p` (a point near track `k`) joins it: the end if it is at the end, else a new cut.
    fn split(&mut self, k: usize, p: Point) -> usize {
        let (a, b, layer) = (self.tracks[k].a, self.tracks[k].b, self.tracks[k].layer);
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let l2 = dx * dx + dy * dy;
        let t = if l2 == 0.0 { 0.0 } else { (((p.x - a.x) * dx + (p.y - a.y) * dy) / l2).clamp(0.0, 1.0) };
        let q = Point::new(a.x + t * dx, a.y + t * dy);
        let n = self.node_at(layer, q);
        if !self.tracks[k].marks.iter().any(|&(_, m)| m == n) {
            self.tracks[k].marks.push((t, n));
        }
        n
    }
}

impl CopperGraph {
    pub fn build(board: &Board) -> CopperGraph {
        let layers = board.copper_layers();
        let layer_of = |s: Side| layers.iter().position(|l| *l == s);
        let mut bd = Builder { board, g: CopperGraph { layers: layers.clone(), nodes: vec![], adj: vec![], pads: HashMap::new() }, tracks: vec![], at: HashMap::new() };
        // the tracks and their two ends
        for t in &board.tracks {
            let Some(layer) = layer_of(t.side) else { continue };
            let (na, nb) = (bd.node_at(layer, t.a), bd.node_at(layer, t.b));
            bd.tracks.push(TrackInfo { layer, a: t.a, b: t.b, width: t.width, marks: vec![(0.0, na), (1.0, nb)] });
        }
        let cap = |k: &TrackInfo| Geo::Cap { a: k.a, b: k.b, r: k.width / 2.0 };
        let near = |a: (Point, Point, f64), b: (Point, Point, f64)| -> bool {
            a.0.x.min(a.1.x) - a.2 <= b.0.x.max(b.1.x) + b.2
                && b.0.x.min(b.1.x) - b.2 <= a.0.x.max(a.1.x) + a.2
                && a.0.y.min(a.1.y) - a.2 <= b.0.y.max(b.1.y) + b.2
                && b.0.y.min(b.1.y) - b.2 <= a.0.y.max(a.1.y) + a.2
        };
        // the box of each track, and a grid of them for each layer (what lies near a pad or a via, or another track)
        let (mut lo, mut hi) = (Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN));
        let mut grow = |p: Point, e: f64| {
            lo = Point::new(lo.x.min(p.x - e), lo.y.min(p.y - e));
            hi = Point::new(hi.x.max(p.x + e), hi.y.max(p.y + e));
        };
        for k in &bd.tracks {
            grow(k.a, k.width);
            grow(k.b, k.width);
        }
        for p in &board.pads {
            grow(p.pos, 10.0);
        }
        for v in &board.vias {
            grow(v.pos, v.diameter);
        }
        if lo.x > hi.x {
            (lo, hi) = (Point::new(0.0, 0.0), Point::new(1.0, 1.0));
        }
        let tbox: Vec<Box4> = bd.tracks.iter().map(|k| (k.a.x.min(k.b.x) - k.width / 2.0, k.a.y.min(k.b.y) - k.width / 2.0, k.a.x.max(k.b.x) + k.width / 2.0, k.a.y.max(k.b.y) + k.width / 2.0)).collect();
        let mut tgrid: Vec<Cells> = (0..layers.len()).map(|_| Cells::new(lo, hi, 2.0)).collect();
        for (k, tr) in bd.tracks.iter().enumerate() {
            tgrid[tr.layer].insert(k as u32, tbox[k]);
        }
        let pad_box = |p: &Pad| -> Box4 {
            let e = match p.shape {
                PadShape::Circle { d } => d / 2.0,
                PadShape::Rect { w, h } | PadShape::Oval { w, h } => w.max(h) / 2.0,
            };
            (p.pos.x - e, p.pos.y - e, p.pos.x + e, p.pos.y + e)
        };
        let mut pgrid = Cells::new(lo, hi, 2.0);
        for (j, p) in board.pads.iter().enumerate() {
            pgrid.insert(j as u32, pad_box(p));
        }
        let mut cand: Vec<u32> = vec![];
        // an end of one track on another track of the same layer: a branch
        let n_tracks = bd.tracks.len();
        for i in 0..n_tracks {
            let li = bd.tracks[i].layer;
            tgrid[li].query(tbox[i], &mut cand);
            for jj in 0..cand.len() {
                let j = cand[jj] as usize;
                if j <= i {
                    continue;
                }
                let (ti, tj) = (&bd.tracks[i], &bd.tracks[j]);
                if !near((ti.a, ti.b, ti.width / 2.0), (tj.a, tj.b, tj.width / 2.0)) || dist(&cap(ti), &cap(tj)) > 0.0 {
                    continue;
                }
                let (ai, bi, wi, aj, bj, wj) = (ti.a, ti.b, ti.width, tj.a, tj.b, tj.width);
                for (end, onto, w) in [(aj, i, wi), (bj, i, wi), (ai, j, wj), (bi, j, wj)] {
                    let (ta, tb) = (bd.tracks[onto].a, bd.tracks[onto].b);
                    if point_seg(end, ta, tb) <= w / 2.0 + 1e-6 {
                        let layer = bd.tracks[onto].layer;
                        let n_end = bd.node_at(layer, end);
                        let n_on = bd.split(onto, end);
                        let (pn, pe) = (bd.g.nodes[n_on].1, bd.g.nodes[n_end].1);
                        bd.link(n_on, n_end, pn.dist(pe), false);
                    }
                }
            }
        }
        // pads: a node on each layer the pad is on, joined to the tracks that touch it
        let all: Vec<usize> = (0..layers.len()).collect();
        let mut pad_nodes: Vec<Vec<(usize, usize)>> = vec![]; // per pad: (layer, node)
        for (j, p) in board.pads.iter().enumerate() {
            let on: Vec<usize> = if p.drill.is_some() { all.clone() } else { layer_of(p.side).into_iter().collect() };
            let geo = shape_geo(p.pos, p.shape);
            let mut mine = vec![];
            for &l in &on {
                let n = bd.new_node(l, p.pos);
                mine.push((l, n));
                tgrid[l].query(pad_box(p), &mut cand);
                for kk in 0..cand.len() {
                    let k = cand[kk] as usize;
                    if dist(&cap(&bd.tracks[k]), &geo) <= 0.0 {
                        let m = bd.split(k, p.pos);
                        bd.link(m, n, 0.0, false);
                    }
                }
            }
            // a through-hole pad is one piece of copper on all its layers
            for w in mine.windows(2) {
                bd.link(w[0].1, w[1].1, 0.0, false);
            }
            if !p.refdes.is_empty() && !p.pin.is_empty() {
                bd.g.pads.entry(PadRef { refdes: p.refdes.clone(), pin: p.pin.clone() }).or_default().extend(mine.iter().map(|&(_, n)| n));
            }
            pad_nodes.push(mine);
            let _ = j;
        }
        // pads of one part and pin are one connection (an exposed pad, a package tab)
        let groups: Vec<Vec<usize>> = bd.g.pads.values().filter(|v| v.len() > 1).cloned().collect();
        for v in groups {
            for w in v.windows(2) {
                bd.link(w[0], w[1], 0.0, false);
            }
        }
        // pads that touch each other
        for i in 0..board.pads.len() {
            pgrid.query(pad_box(&board.pads[i]), &mut cand);
            for jj in 0..cand.len() {
                let j = cand[jj] as usize;
                if j <= i {
                    continue;
                }
                let (a, b) = (&board.pads[i], &board.pads[j]);
                if dist(&shape_geo(a.pos, a.shape), &shape_geo(b.pos, b.shape)) > 0.0 {
                    continue;
                }
                for &(la, na) in &pad_nodes[i] {
                    if let Some(&(_, nb)) = pad_nodes[j].iter().find(|(lb, _)| *lb == la) {
                        bd.link(na, nb, 0.0, false);
                    }
                }
            }
        }
        // vias: a node on every layer, the layers joined by the barrel, joined to the tracks and pads that touch it
        for v in &board.vias {
            let vg = Geo::Cap { a: v.pos, b: v.pos, r: v.diameter / 2.0 };
            let mut mine = vec![];
            for &l in &all {
                let n = bd.new_node(l, v.pos);
                mine.push(n);
                let vbox: Box4 = (v.pos.x - v.diameter / 2.0, v.pos.y - v.diameter / 2.0, v.pos.x + v.diameter / 2.0, v.pos.y + v.diameter / 2.0);
                tgrid[l].query(vbox, &mut cand);
                for kk in 0..cand.len() {
                    let k = cand[kk] as usize;
                    if dist(&cap(&bd.tracks[k]), &vg) <= 0.0 {
                        let m = bd.split(k, v.pos);
                        bd.link(m, n, 0.0, false);
                    }
                }
                pgrid.query(vbox, &mut cand);
                for pp in 0..cand.len() {
                    let pi = cand[pp] as usize;
                    let p = &board.pads[pi];
                    if let Some(&(_, np)) = pad_nodes[pi].iter().find(|(lp, _)| *lp == l)
                        && dist(&shape_geo(p.pos, p.shape), &vg) <= 0.0
                    {
                        bd.link(np, n, 0.0, false);
                    }
                }
            }
            for w in mine.windows(2) {
                bd.link(w[0], w[1], 0.0, true);
            }
        }
        // the tracks themselves: an edge between each two neighbouring places
        for k in 0..bd.tracks.len() {
            let mut marks = bd.tracks[k].marks.clone();
            marks.sort_by(|x, y| x.0.total_cmp(&y.0));
            let len = bd.tracks[k].a.dist(bd.tracks[k].b);
            for w in marks.windows(2) {
                bd.link(w[0].1, w[1].1, (w[1].0 - w[0].0) * len, false);
            }
        }
        let _ = bd.board;
        bd.g
    }

    /// Distances (millionths of a mm) and the way back, from every node of the pad.
    fn search(&self, from: &PadRef) -> Option<(Vec<u64>, Vec<Option<usize>>)> {
        let sources = self.pads.get(from)?;
        let mut dist = vec![u64::MAX; self.nodes.len()];
        let mut prev = vec![None; self.nodes.len()];
        let mut heap = BinaryHeap::new();
        for &s in sources {
            dist[s] = 0;
            heap.push(Reverse((0u64, s)));
        }
        while let Some(Reverse((d, u))) = heap.pop() {
            if d > dist[u] {
                continue;
            }
            for e in &self.adj[u] {
                let nd = d + e.cost;
                if nd < dist[e.to] {
                    dist[e.to] = nd;
                    prev[e.to] = Some(u);
                    heap.push(Reverse((nd, e.to)));
                }
            }
        }
        Some((dist, prev))
    }

    /// The nearest node of the pad `to`, and the nodes of the way to it (from the first pad to the last).
    fn way(&self, to: &PadRef, dist: &[u64], prev: &[Option<usize>]) -> Option<Vec<usize>> {
        let end = *self.pads.get(to)?.iter().filter(|&&n| dist[n] != u64::MAX).min_by_key(|&&n| dist[n])?;
        let mut chain = vec![end];
        while let Some(p) = prev[*chain.last().unwrap()] {
            chain.push(p);
        }
        chain.reverse();
        Some(chain)
    }

    /// The shortest way along the copper from pad `a` to pad `b` (None when no tracks and vias join them).
    pub fn path(&self, a: &PadRef, b: &PadRef) -> Option<CopperPath> {
        let (dist, prev) = self.search(a)?;
        let way = self.way(b, &dist, &prev)?;
        let mut vias = 0;
        for w in way.windows(2) {
            if self.adj[w[0]].iter().any(|e| e.to == w[1] && e.via) {
                vias += 1;
            }
        }
        let points: Vec<(Side, Point)> = way.iter().map(|&n| self.nodes[n]).collect();
        let mut layers: Vec<Side> = vec![];
        for (s, _) in &points {
            if layers.last() != Some(s) {
                layers.push(*s);
            }
        }
        let length = dist[*way.last()?] as f64 / SCALE;
        Some(CopperPath { length, vias, layers, points })
    }

    /// Where the ways from pad `from` to pads `a` and `b` part: how far they run together first. None when either pad cannot be reached.
    pub fn fork(&self, from: &PadRef, a: &PadRef, b: &PadRef) -> Option<Fork> {
        let (dist, prev) = self.search(from)?;
        let (wa, wb) = (self.way(a, &dist, &prev)?, self.way(b, &dist, &prev)?);
        let common = wa.iter().zip(&wb).take_while(|(x, y)| x == y).count();
        let (length, at) = if common == 0 {
            // they leave the pad on their own (or on different layers of a through-hole pad)
            (0.0, self.nodes[wa[0]].1)
        } else {
            let last = wa[common - 1];
            (dist[last] as f64 / SCALE, self.nodes[last].1)
        };
        Some(Fork { length, at, a_length: dist[*wa.last()?] as f64 / SCALE, b_length: dist[*wb.last()?] as f64 / SCALE })
    }
}

/// The copper items of a board (tracks, pads, vias: the order of `copper_items`) with the net of each, from the copper that joins them.
/// Built once for the checks that look at nets (the gap between nets, a guard, the properties of a net class).
pub struct CopperIndex {
    items: Vec<Cu>,
    nets: Vec<Option<String>>,
    conn: Connectivity,
}

impl CopperIndex {
    /// The net of the item `i` (tracks come first), if it has one.
    pub fn net_of(&self, i: usize) -> Option<&str> {
        self.nets.get(i).and_then(|n| n.as_deref())
    }

    pub fn build(board: &Board) -> CopperIndex {
        let conn = connectivity(board);
        let items = copper_items(board);
        let (nt, np) = (board.tracks.len(), board.pads.len());
        let nets = (0..items.len())
            .map(|i| {
                let n = if i < nt {
                    conn.track_net(i)
                } else if i < nt + np {
                    conn.pad_net(i - nt)
                } else {
                    conn.via_net(i - nt - np)
                };
                n.map(str::to_string)
            })
            .collect();
        CopperIndex { items, nets, conn }
    }

    /// The net of track `i` of the board.
    pub fn track_net(&self, i: usize) -> Option<&str> {
        self.conn.track_net(i)
    }
}

/// What the checks of the requirements ask of the copper, built once for all of them: the graph of ways and the index of nets.
pub struct CopperModel {
    pub graph: CopperGraph,
    pub index: CopperIndex,
}

impl CopperModel {
    pub fn build(board: &Board) -> CopperModel {
        CopperModel { graph: CopperGraph::build(board), index: CopperIndex::build(board) }
    }
}

/// The names of the nets one entry of a measure stands for: names (or ending in `*`) joined by `+` ("A+SA").
pub fn net_set(entry: &str) -> Vec<String> {
    entry.split('+').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

fn in_set(set: &[String], n: &str) -> bool {
    set.iter().any(|p| design_spec::matches_name(p, n))
}

/// The nearest copper of two sets of nets (names, or ending in `*`): the gap between the edges (0 when they touch) and the two points
/// (the centres of the two items) to draw a line between. None when one of the sets has no copper on the board. Only copper on a
/// common layer counts (a through-hole pad and a via are on all of them).
pub fn net_gap(board: &Board, a: &[String], b: &[String]) -> Option<(f64, Point, Point)> {
    net_gap_in(&CopperIndex::build(board), a, b)
}

/// [`net_gap`] on an index that is already built.
pub fn net_gap_in(index: &CopperIndex, a: &[String], b: &[String]) -> Option<(f64, Point, Point)> {
    net_gap_except(index, None, a, b, &[])
}

/// [`net_gap_in`] with the pads of some parts left out (`except`: part names, or "pads" for every pad: only tracks and vias are looked at).
/// `board` is where the pads' parts are read from; without it nothing is left out.
pub fn net_gap_except(index: &CopperIndex, board: Option<&Board>, a: &[String], b: &[String], except: &[String]) -> Option<(f64, Point, Point)> {
    let (items, nets) = (&index.items, &index.nets);
    let left_out = |i: usize| -> bool {
        let Some(board) = board else { return false };
        let (nt, np) = (board.tracks.len(), board.pads.len());
        if except.is_empty() || i < nt || i >= nt + np {
            return false;
        }
        let part = board.pads[i - nt].refdes.as_str();
        except.iter().any(|e| e == "pads" || e == part)
    };
    let (mut left, mut right) = (vec![], vec![]);
    for (i, n) in nets.iter().enumerate() {
        if left_out(i) {
            continue;
        }
        match n.as_deref() {
            Some(n) if in_set(a, n) => left.push(i),
            Some(n) if in_set(b, n) => right.push(i),
            _ => {}
        }
    }
    let mut best: Option<(f64, Point, Point)> = None;
    for &i in &left {
        for &j in &right {
            if !items[i].layers.iter().any(|l| items[j].layers.contains(l)) {
                continue;
            }
            let d = dist(&items[i].geo, &items[j].geo);
            if best.is_none_or(|x| d < x.0) {
                best = Some((d, center(&items[i].geo), center(&items[j].geo)));
            }
        }
    }
    best
}

/// How much of the tracks of a sensitive net has a guard on both sides.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuardCover {
    /// Length (mm) of the tracks looked at, and of those with guard copper within reach on each side.
    pub total: f64,
    pub covered: f64,
    /// The stretches with no guard on one side or both, as runs of points (to draw).
    pub bare: Vec<Vec<Point>>,
}

impl GuardCover {
    /// Percent of the length that is guarded (100 when there is nothing to guard).
    pub fn percent(&self) -> f64 {
        if self.total <= 1e-9 { 100.0 } else { 100.0 * self.covered / self.total }
    }
}

/// Near a pad of the net the guard stops short on purpose (the net leaves the track there): this far from a pad, nothing is asked.
pub const GUARD_PAD_ZONE: f64 = 2.0;

/// A guard looks at the tracks of the nets `nets` (names, or ending in `*`): every 0.2 mm along a track, is there copper of the nets `guard`
/// on the same layer within `reach` mm of the track's edge on the left, and on the right? Vias and pads of the guard count on every
/// layer they are on. The ends of the tracks within `GUARD_PAD_ZONE` of a pad of the net are left out. None when the sensitive nets have
/// no track.
pub fn guard_cover(board: &Board, nets: &[String], guard: &[String], reach: f64) -> Option<GuardCover> {
    guard_cover_in(&CopperIndex::build(board), board, nets, guard, reach)
}

/// [`guard_cover`] on an index that is already built.
pub fn guard_cover_in(index: &CopperIndex, board: &Board, nets: &[String], guard: &[String], reach: f64) -> Option<GuardCover> {
    let (items, item_net) = (&index.items, &index.nets);
    let nt = board.tracks.len();
    let guards: Vec<usize> = (0..items.len()).filter(|&i| item_net[i].as_deref().is_some_and(|n| in_set(guard, n))).collect();
    let pads: Vec<Point> = board.pads.iter().filter(|p| in_set(nets, &p.net)).map(|p| p.pos).collect();
    let mut out = GuardCover::default();
    let mut any = false;
    let step = 0.2;
    for i in 0..nt {
        if !item_net[i].as_deref().is_some_and(|n| in_set(nets, n)) {
            continue;
        }
        let t = &board.tracks[i];
        let len = t.a.dist(t.b);
        if len < 1e-6 {
            continue;
        }
        any = true;
        let n = ((len / step).ceil() as usize).max(1);
        let piece = len / n as f64;
        let mut run: Vec<Point> = vec![];
        for k in 0..n {
            let f = (k as f64 + 0.5) / n as f64;
            let p = Point::new(t.a.x + (t.b.x - t.a.x) * f, t.a.y + (t.b.y - t.a.y) * f);
            if pads.iter().any(|q| q.dist(p) < GUARD_PAD_ZONE) {
                if run.len() > 1 {
                    out.bare.push(std::mem::take(&mut run));
                }
                run.clear();
                continue;
            }
            out.total += piece;
            let probe = Geo::Cap { a: p, b: p, r: t.width / 2.0 };
            let mut sides = [false, false];
            for &g in &guards {
                if !items[g].on(t.side) {
                    continue;
                }
                let c = center(&items[g].geo);
                let cross = (t.b.x - t.a.x) * (c.y - t.a.y) - (t.b.y - t.a.y) * (c.x - t.a.x);
                if cross.abs() < 1e-9 {
                    continue;
                }
                let s = usize::from(cross > 0.0);
                if !sides[s] && dist(&probe, &items[g].geo) <= reach {
                    sides[s] = true;
                }
            }
            if sides[0] && sides[1] {
                out.covered += piece;
                if run.len() > 1 {
                    out.bare.push(std::mem::take(&mut run));
                }
                run.clear();
            } else {
                run.push(p);
            }
        }
        if run.len() > 1 {
            out.bare.push(run);
        }
    }
    any.then_some(out)
}

/// The narrowest track along a way (the way's points, layer by layer): its width, and where it is (the middle of that piece). None when
/// the way has no track (it lies inside a pad).
pub fn narrowest_on_way(board: &Board, way: &[(Side, Point)]) -> Option<(f64, Point)> {
    let mut best: Option<(f64, Point)> = None;
    for w in way.windows(2) {
        let ((s0, p), (s1, q)) = (w[0], w[1]);
        if s0 != s1 || p.dist(q) < 1e-6 {
            continue;
        }
        let mid = Point::new((p.x + q.x) / 2.0, (p.y + q.y) / 2.0);
        if let Some(width) = carrier_width(board, s0, p, q)
            && best.is_none_or(|b| width < b.0)
        {
            best = Some((width, mid));
        }
    }
    best
}

/// The width of the track that carries the piece `p`-`q` of a way on layer `side`: the narrowest of the tracks that hold both ends and the
/// middle (the way's points are track ends and the places where tracks meet). None when no track does.
fn carrier_width(board: &Board, side: Side, p: Point, q: Point) -> Option<f64> {
    let mid = Point::new((p.x + q.x) / 2.0, (p.y + q.y) / 2.0);
    let on = |t: &Track, x: Point| {
        let (dx, dy) = (t.b.x - t.a.x, t.b.y - t.a.y);
        let l2 = dx * dx + dy * dy;
        let f = if l2 < 1e-12 { 0.0 } else { (((x.x - t.a.x) * dx + (x.y - t.a.y) * dy) / l2).clamp(0.0, 1.0) };
        Point::new(t.a.x + dx * f, t.a.y + dy * f).dist(x) < 1e-3
    };
    let width = board.tracks.iter().filter(|t| t.side == side && on(t, p) && on(t, q) && on(t, mid)).map(|t| t.width).fold(f64::INFINITY, f64::min);
    width.is_finite().then_some(width)
}

/// The DC resistance (ohm) of a way (its points, layer by layer): each piece of track by its length and width, each change of layer at a via
/// by the barrel of that via (the board thickness split evenly between the copper layers). Copper of `thickness_um` (35 = 1 oz). A pour
/// is not walked, a through-hole pad that changes the layer costs nothing. Also the middle of the piece with the most resistance.
pub fn way_resistance(board: &Board, way: &[(Side, Point)], thickness_um: f64) -> (f64, Option<Point>) {
    let spec = crate::irdrop::IrSpec { thickness_um };
    let layers = board.copper_layers();
    let (mut total, mut worst) = (0.0, (0.0, None));
    for w in way.windows(2) {
        let ((s0, p), (s1, q)) = (w[0], w[1]);
        let r = if s0 == s1 {
            if p.dist(q) < 1e-6 {
                continue;
            }
            let Some(width) = carrier_width(board, s0, p, q) else { continue };
            crate::irdrop::track_resistance(p.dist(q), width, &spec)
        } else {
            let Some(via) = board.vias.iter().find(|v| v.pos.dist(p) < 0.01) else { continue };
            let (i0, i1) = (layers.iter().position(|&l| l == s0), layers.iter().position(|&l| l == s1));
            let (Some(i0), Some(i1)) = (i0, i1) else { continue };
            let height = crate::irdrop::BOARD_MM * i0.abs_diff(i1) as f64 / (layers.len().max(2) - 1) as f64;
            crate::irdrop::via_resistance(via.drill, height)
        };
        total += r;
        if r > worst.0 {
            worst = (r, Some(Point::new((p.x + q.x) / 2.0, (p.y + q.y) / 2.0)));
        }
    }
    (total, worst.1)
}

/// How much of a way has copper of the `ground` nets on a layer next to it (the layers above and below it in the stack): the return current
/// of a signal runs there. Every 0.2 mm along the tracks of the way is looked at; the stretches without are kept to draw.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UnderCover {
    pub covered: f64,
    pub total: f64,
    pub bare: Vec<Vec<Point>>,
}

impl UnderCover {
    /// Percent of the length that has ground next to it (100 when the way has no track).
    pub fn percent(&self) -> f64 {
        if self.total <= 1e-9 { 100.0 } else { 100.0 * self.covered / self.total }
    }
}

pub fn under_cover(index: &CopperIndex, board: &Board, way: &[(Side, Point)], ground: &[String]) -> UnderCover {
    let layers = board.copper_layers();
    let (items, item_net) = (&index.items, &index.nets);
    let grounds: Vec<usize> = (0..items.len()).filter(|&i| item_net[i].as_deref().is_some_and(|n| in_set(ground, n))).collect();
    let pours: Vec<(Side, Vec<crate::zonefill::FillShape>)> =
        crate::zonefill::merged_fills(board, &index.conn).into_iter().filter(|(_, n, _)| !n.is_empty() && in_set(ground, n)).map(|(s, _, f)| (s, f)).collect();
    let mut out = UnderCover::default();
    for w in way.windows(2) {
        let ((s0, p), (s1, q)) = (w[0], w[1]);
        let len = p.dist(q);
        if s0 != s1 || len < 1e-6 {
            continue;
        }
        let Some(i) = layers.iter().position(|&l| l == s0) else { continue };
        let next: Vec<Side> = [i.checked_sub(1), Some(i + 1)].into_iter().flatten().filter_map(|k| layers.get(k).copied()).collect();
        let n = (len / 0.2).ceil().max(1.0) as usize;
        let at = |t: f64| Point::new(p.x + (q.x - p.x) * t, p.y + (q.y - p.y) * t);
        let mut run: Vec<Point> = vec![];
        for k in 0..n {
            let (t0, t1) = (k as f64 / n as f64, (k + 1) as f64 / n as f64);
            let x = at((t0 + t1) / 2.0);
            let probe = Geo::Cap { a: x, b: x, r: 0.0 };
            let under = next.iter().any(|&l| {
                grounds.iter().any(|&g| items[g].on(l) && dist(&probe, &items[g].geo) <= 1e-6)
                    || pours.iter().any(|(s, f)| *s == l && f.iter().any(|sh| inside_polygon(&sh.outer, x) && !sh.holes.iter().any(|h| inside_polygon(h, x))))
            });
            let seg = len / n as f64;
            out.total += seg;
            if under {
                out.covered += seg;
                if run.len() > 1 {
                    out.bare.push(std::mem::take(&mut run));
                }
                run.clear();
            } else {
                if run.is_empty() {
                    run.push(at(t0));
                }
                run.push(at(t1));
            }
        }
        if run.len() > 1 {
            out.bare.push(run);
        }
    }
    out
}

/// How much copper of the nets `a` lies over or under copper of the nets `b` on another layer (mm²): pads, tracks, vias and pours, each
/// layer's copper joined into one shape. Also the middle of the biggest overlap. None when a set has no copper.
pub fn layer_overlap(index: &CopperIndex, board: &Board, a: &[String], b: &[String]) -> Option<(f64, Option<Point>)> {
    use crate::zonefill::{overlap_shapes, pad_piece, track_piece, via_piece, FillShape};
    let layers = board.copper_layers();
    let (nt, np) = (board.tracks.len(), board.pads.len());
    let mut copper: [HashMap<Side, Vec<FillShape>>; 2] = Default::default();
    for (k, set) in [a, b].into_iter().enumerate() {
        let hit = |i: usize| index.nets[i].as_deref().is_some_and(|n| in_set(set, n));
        for (i, t) in board.tracks.iter().enumerate() {
            if hit(i) {
                copper[k].entry(t.side).or_default().push(track_piece(t.a, t.b, t.width));
            }
        }
        for (j, p) in board.pads.iter().enumerate() {
            if hit(nt + j) {
                let on: Vec<Side> = if p.drill.is_some() { layers.clone() } else { vec![p.side] };
                for s in on {
                    copper[k].entry(s).or_default().push(pad_piece(p.pos, p.shape));
                }
            }
        }
        for (j, v) in board.vias.iter().enumerate() {
            if hit(nt + np + j) {
                for &s in &layers {
                    copper[k].entry(s).or_default().push(via_piece(v.pos, v.diameter));
                }
            }
        }
        for (s, n, f) in crate::zonefill::merged_fills(board, &index.conn) {
            if !n.is_empty() && in_set(set, &n) {
                copper[k].entry(s).or_default().extend(f);
            }
        }
    }
    if copper[0].is_empty() || copper[1].is_empty() {
        return None;
    }
    let (mut area, mut biggest) = (0.0, (0.0, None));
    for &la in &layers {
        for &lb in &layers {
            if la == lb {
                continue;
            }
            let (Some(x), Some(y)) = (copper[0].get(&la), copper[1].get(&lb)) else { continue };
            for piece in overlap_shapes(x, y) {
                let s = crate::zonefill::fill_area(std::slice::from_ref(&piece));
                area += s;
                if s > biggest.0 {
                    let n = piece.outer.len().max(1) as f64;
                    biggest = (s, Some(Point::new(piece.outer.iter().map(|p| p.x).sum::<f64>() / n, piece.outer.iter().map(|p| p.y).sum::<f64>() / n)));
                }
            }
        }
    }
    Some((area, biggest.1))
}

/// How much of the way round a pad has copper of the guard nets within `reach` mm of its edge: 72 directions from the pad's middle, and in
/// each the copper of the guard nets on the pad's layer (both layers for a through-hole pad) must be met within `reach` of the pad's edge.
/// Percent (a track that leaves the pad leaves a gap). None when the pad is not on the board.
pub fn ring_cover(index: &CopperIndex, board: &Board, pad: &PadRef, guard: &[String], reach: f64) -> Option<f64> {
    let p = board.pads.iter().find(|p| p.refdes == pad.refdes && p.pin == pad.pin)?;
    let (w, h) = p.shape.size();
    let (items, item_net) = (&index.items, &index.nets);
    let layers: Vec<Side> = if p.drill.is_some() { vec![Side::Top, Side::Bottom] } else { vec![p.side] };
    // only the guard copper on the pad's layers that lies within reach of the pad at all (every probe point is within this of the middle)
    let around = w.hypot(h) / 2.0 + reach + 0.2;
    let middle = Geo::Cap { a: p.pos, b: p.pos, r: 0.0 };
    let guards: Vec<usize> = (0..items.len())
        .filter(|&i| item_net[i].as_deref().is_some_and(|n| in_set(guard, n)) && layers.iter().any(|&l| items[i].on(l)) && dist(&middle, &items[i].geo) <= around)
        .collect();
    let steps = 72;
    let mut covered = 0;
    for k in 0..steps {
        let a = 2.0 * std::f64::consts::PI * k as f64 / steps as f64;
        let (c, s) = (a.cos(), a.sin());
        // where the ray leaves the pad's box
        let edge = (w / 2.0 / c.abs().max(1e-9)).min(h / 2.0 / s.abs().max(1e-9));
        let mut t = edge + 0.05;
        let mut hit = false;
        while t <= edge + reach {
            let q = Point::new(p.pos.x + c * t, p.pos.y + s * t);
            let probe = Geo::Cap { a: q, b: q, r: 0.0 };
            if guards.iter().any(|&g| dist(&probe, &items[g].geo) <= 1e-6) {
                hit = true;
                break;
            }
            t += 0.1;
        }
        if hit {
            covered += 1;
        }
    }
    Some(100.0 * covered as f64 / steps as f64)
}

/// The way the return current can take: the shortest run through the ground copper (tracks, pads, vias and pours of the `ground` nets) on the
/// layers next to the layers of the way, from the ground nearest its start to the ground nearest its end. The copper is laid on a grid
/// (0.3 mm, coarser for a big area) and the run goes cell to cell in 8 directions, so it is longer than the real one by up to 8 %; the
/// layers next to the way are taken as one copper (they are taken to be stitched together).
#[derive(Clone, Debug, PartialEq)]
pub struct ReturnPath {
    /// Length (mm) of the run.
    pub length: f64,
    /// The straight distance (mm) between the two ends of the run (what the run is compared with when the way itself is shorter).
    pub chord: f64,
    /// The run as points (every few cells).
    pub points: Vec<Point>,
}

/// None when there is no ground within 3 mm of an end of the way, or the ground copper is in pieces so that no run joins the two ends.
pub fn return_path(index: &CopperIndex, board: &Board, way: &[(Side, Point)], ground: &[String]) -> Option<ReturnPath> {
    use crate::zonefill::merged_fills;
    let layers = board.copper_layers();
    let mut next: Vec<Side> = vec![];
    for &(s, _) in way {
        let Some(i) = layers.iter().position(|&l| l == s) else { continue };
        for k in [i.checked_sub(1), Some(i + 1)].into_iter().flatten() {
            if let Some(&l) = layers.get(k) && !next.contains(&l) {
                next.push(l);
            }
        }
    }
    let (start, end) = (way.first()?.1, way.last()?.1);
    // the area looked at: the way and 5 mm round it
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for &(_, p) in way {
        x0 = x0.min(p.x);
        x1 = x1.max(p.x);
        y0 = y0.min(p.y);
        y1 = y1.max(p.y);
    }
    let margin = 5.0;
    let (x0, y0, x1, y1) = (x0 - margin, y0 - margin, x1 + margin, y1 + margin);
    let step = 0.3_f64.max((((x1 - x0) * (y1 - y0)) / 40000.0).sqrt());
    let (nx, ny) = (((x1 - x0) / step).ceil() as usize + 1, ((y1 - y0) / step).ceil() as usize + 1);
    let mut cover = vec![false; nx * ny];
    let cell_range = |lo: f64, hi: f64, o: f64, n: usize| -> std::ops::Range<usize> {
        let a = (((lo - o) / step).floor().max(0.0)) as usize;
        let b = ((((hi - o) / step).ceil().max(0.0)) as usize + 1).min(n);
        a.min(n)..b
    };
    // tracks, pads and vias of the ground nets: every cell within half a cell of the copper
    let (items, item_net) = (&index.items, &index.nets);
    for (i, it) in items.iter().enumerate() {
        if !item_net[i].as_deref().is_some_and(|n| in_set(ground, n)) || !next.iter().any(|&l| it.on(l)) {
            continue;
        }
        let (bx0, bx1, by0, by1) = bbox(&it.geo);
        if bx1 < x0 || bx0 > x1 || by1 < y0 || by0 > y1 {
            continue;
        }
        for cy in cell_range(by0 - step, by1 + step, y0, ny) {
            for cx in cell_range(bx0 - step, bx1 + step, x0, nx) {
                let q = Point::new(x0 + cx as f64 * step, y0 + cy as f64 * step);
                if !cover[cy * nx + cx] && dist(&Geo::Cap { a: q, b: q, r: step * 0.5 }, &it.geo) <= 1e-6 {
                    cover[cy * nx + cx] = true;
                }
            }
        }
    }
    // the pours of the ground nets
    for (side, net, fills) in merged_fills(board, &index.conn) {
        if net.is_empty() || !in_set(ground, &net) || !next.contains(&side) {
            continue;
        }
        for sh in &fills {
            let (mut bx0, mut bx1, mut by0, mut by1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
            for p in &sh.outer {
                bx0 = bx0.min(p.x);
                bx1 = bx1.max(p.x);
                by0 = by0.min(p.y);
                by1 = by1.max(p.y);
            }
            if bx1 < x0 || bx0 > x1 || by1 < y0 || by0 > y1 {
                continue;
            }
            // row by row: where the outlines cross the row, the copper lies between the 1st and 2nd, 3rd and 4th ... (holes included)
            for cy in cell_range(by0, by1, y0, ny) {
                let y = y0 + cy as f64 * step;
                let mut xs: Vec<f64> = vec![];
                for ring in std::iter::once(&sh.outer).chain(sh.holes.iter()) {
                    for k in 0..ring.len() {
                        let (p, q) = (ring[k], ring[(k + 1) % ring.len()]);
                        if (p.y <= y) != (q.y <= y) {
                            xs.push(p.x + (y - p.y) / (q.y - p.y) * (q.x - p.x));
                        }
                    }
                }
                xs.sort_by(|a, b| a.total_cmp(b));
                for pair in xs.chunks_exact(2) {
                    let a = (((pair[0] - x0) / step).ceil().max(0.0)) as usize;
                    let b = ((pair[1] - x0) / step).floor();
                    if b < 0.0 {
                        continue;
                    }
                    for cx in a..=(b as usize).min(nx - 1) {
                        cover[cy * nx + cx] = true;
                    }
                }
            }
        }
    }
    // the covered cell nearest a point, within 3 mm
    let nearest = |p: Point| -> Option<usize> {
        let (px, py) = (((p.x - x0) / step).round() as i64, ((p.y - y0) / step).round() as i64);
        let reach = (3.0 / step).ceil() as i64;
        let mut best: Option<(f64, usize)> = None;
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                let (cx, cy) = (px + dx, py + dy);
                if cx < 0 || cy < 0 || cx >= nx as i64 || cy >= ny as i64 || !cover[cy as usize * nx + cx as usize] {
                    continue;
                }
                let d = ((dx * dx + dy * dy) as f64).sqrt() * step;
                if d <= 3.0 && best.is_none_or(|b| d < b.0) {
                    best = Some((d, cy as usize * nx + cx as usize));
                }
            }
        }
        best.map(|b| b.1)
    };
    let (s, e) = (nearest(start)?, nearest(end)?);
    // the shortest run, 8 directions (costs in units of 0.001 mm)
    let unit = |d: f64| (d * step * 1000.0).round() as u64;
    let (straight, diagonal) = (unit(1.0), unit(std::f64::consts::SQRT_2));
    let mut best = vec![u64::MAX; nx * ny];
    let mut from = vec![usize::MAX; nx * ny];
    let mut heap = BinaryHeap::new();
    best[s] = 0;
    heap.push(Reverse((0u64, s)));
    while let Some(Reverse((d, c))) = heap.pop() {
        if d > best[c] {
            continue;
        }
        if c == e {
            break;
        }
        let (cx, cy) = ((c % nx) as i64, (c / nx) as i64);
        for dy in -1i64..=1 {
            for dx in -1i64..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let (qx, qy) = (cx + dx, cy + dy);
                if qx < 0 || qy < 0 || qx >= nx as i64 || qy >= ny as i64 {
                    continue;
                }
                let q = qy as usize * nx + qx as usize;
                if !cover[q] {
                    continue;
                }
                let nd = d + if dx != 0 && dy != 0 { diagonal } else { straight };
                if nd < best[q] {
                    best[q] = nd;
                    from[q] = c;
                    heap.push(Reverse((nd, q)));
                }
            }
        }
    }
    if best[e] == u64::MAX {
        return None;
    }
    let mut cells = vec![e];
    while *cells.last()? != s {
        cells.push(from[*cells.last()?]);
    }
    cells.reverse();
    let points: Vec<Point> = cells.iter().enumerate().filter(|(k, _)| k % 3 == 0 || *k + 1 == cells.len()).map(|(_, &c)| Point::new(x0 + (c % nx) as f64 * step, y0 + (c / nx) as f64 * step)).collect();
    let chord = Point::new(x0 + (s % nx) as f64 * step, y0 + (s / nx) as f64 * step).dist(Point::new(x0 + (e % nx) as f64 * step, y0 + (e / nx) as f64 * step));
    Some(ReturnPath { length: best[e] as f64 / 1000.0, chord, points })
}

/// A track that ends on nothing: from its free end back to the place where it joins the rest of the net.
#[derive(Clone, Debug, PartialEq)]
pub struct Stub {
    /// Length (mm) from the free end to the pad, via, pour or branch it hangs on.
    pub length: f64,
    /// The stub as points, from the free end inwards.
    pub points: Vec<Point>,
}

/// The dead-end branches of the tracks of the nets: a track end that touches no pad, via or pour of the net and no other track, followed
/// inwards to the first pad, via, pour or place where three tracks meet. A track that is free at both ends is not a stub (it is loose, the
/// DRC says so). Only the tracks are walked.
pub fn stubs(index: &CopperIndex, board: &Board, nets: &[String]) -> Vec<Stub> {
    let nt = board.tracks.len();
    let mine: Vec<usize> = (0..nt).filter(|&i| index.nets[i].as_deref().is_some_and(|n| in_set(nets, n))).collect();
    if mine.is_empty() {
        return vec![];
    }
    // the places where tracks of the nets are joined by a pad or via lying on them, and the tracks cut there
    let key = |s: Side, p: Point| (s, (p.x * 200.0).round() as i64, (p.y * 200.0).round() as i64);
    let anchors: Vec<usize> = (nt..index.items.len()).filter(|&i| index.nets[i].as_deref().is_some_and(|n| in_set(nets, n))).collect();
    // the pieces of the pours of the nets, each with its box
    let mut pours: Vec<(Side, (f64, f64, f64, f64), crate::zonefill::FillShape)> = vec![];
    for (s, n, fills) in crate::zonefill::merged_fills(board, &index.conn) {
        if n.is_empty() || !in_set(nets, &n) {
            continue;
        }
        for sh in fills {
            let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
            for p in &sh.outer {
                x0 = x0.min(p.x);
                x1 = x1.max(p.x);
                y0 = y0.min(p.y);
                y1 = y1.max(p.y);
            }
            pours.push((s, (x0, x1, y0, y1), sh));
        }
    }
    // the pads and vias on a grid of 2 mm cells, so that a track end looks only at what is near it
    let cell = |v: f64| (v / 2.0).floor() as i64;
    // a track end touches a pad or via when the round end of the track (half its width) reaches it
    let widest = mine.iter().map(|&i| board.tracks[i].width).fold(0.0, f64::max) / 2.0;
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for &i in &anchors {
        let (x0, x1, y0, y1) = bbox(&index.items[i].geo);
        for cx in cell(x0 - 0.02 - widest)..=cell(x1 + 0.02 + widest) {
            for cy in cell(y0 - 0.02 - widest)..=cell(y1 + 0.02 + widest) {
                grid.entry((cx, cy)).or_default().push(i);
            }
        }
    }
    let anchored = |s: Side, p: Point, half: f64| {
        let probe = Geo::Cap { a: p, b: p, r: 0.0 };
        grid.get(&(cell(p.x), cell(p.y))).is_some_and(|v| v.iter().any(|&i| index.items[i].on(s) && dist(&probe, &index.items[i].geo) <= 0.01 + half))
            || pours.iter().any(|(ps, b, sh)| *ps == s && p.x >= b.0 && p.x <= b.1 && p.y >= b.2 && p.y <= b.3 && inside_polygon(&sh.outer, p) && !sh.holes.iter().any(|h| inside_polygon(h, p)))
    };
    // atomic pieces: each track cut where the end of another track of the nets lies on it
    let mut pieces: Vec<(Side, Point, Point, f64)> = vec![];
    // the track ends sorted along x, so that a track looks only at the ends within its own span
    let mut ends: Vec<(f64, usize, Point)> = mine.iter().flat_map(|&j| [(board.tracks[j].a.x, j, board.tracks[j].a), (board.tracks[j].b.x, j, board.tracks[j].b)]).collect();
    ends.sort_by(|a, b| a.0.total_cmp(&b.0));
    // the end of a track that lies in the copper of another track of the nets (within the two half widths of its middle line) is joined to
    // it: the end is moved onto that track's middle line, and the track is cut there
    let mut cuts: Vec<Vec<f64>> = vec![vec![]; board.tracks.len()];
    let mut snap: HashMap<(Side, i64, i64), Point> = HashMap::new();
    for &i in &mine {
        let t = &board.tracks[i];
        let (dx, dy) = (t.b.x - t.a.x, t.b.y - t.a.y);
        let l2 = dx * dx + dy * dy;
        if l2 <= 1e-12 {
            continue;
        }
        let reach = t.width / 2.0;
        let (lo, hi) = (t.a.x.min(t.b.x) - t.width - 0.5, t.a.x.max(t.b.x) + t.width + 0.5);
        let from = ends.partition_point(|e| e.0 < lo);
        for &(_, j, e) in ends[from..].iter().take_while(|e| e.0 <= hi) {
            let o = &board.tracks[j];
            if j == i || o.side != t.side {
                continue;
            }
            let f = (((e.x - t.a.x) * dx + (e.y - t.a.y) * dy) / l2).clamp(0.0, 1.0);
            let proj = Point::new(t.a.x + dx * f, t.a.y + dy * f);
            if proj.dist(e) <= reach + o.width / 2.0 + 1e-6 {
                snap.entry(key(t.side, e)).or_insert(proj);
                if f > 1e-6 && f < 1.0 - 1e-6 {
                    cuts[i].push(f);
                }
            }
        }
    }
    for &i in &mine {
        let t = &board.tracks[i];
        let fix = |p: Point| snap.get(&key(t.side, p)).copied().unwrap_or(p);
        let (a, b) = (fix(t.a), fix(t.b));
        let (dx, dy) = (t.b.x - t.a.x, t.b.y - t.a.y);
        let c = &mut cuts[i];
        c.sort_by(|x, y| x.total_cmp(y));
        c.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
        let mut last = a;
        for &f in c.iter().chain([&1.0]) {
            let q = if f >= 1.0 { b } else { Point::new(t.a.x + dx * f, t.a.y + dy * f) };
            pieces.push((t.side, last, q, t.width / 2.0));
            last = q;
        }
    }
    let mut at: HashMap<(Side, i64, i64), Vec<usize>> = HashMap::new();
    for (k, &(s, a, b, _)) in pieces.iter().enumerate() {
        at.entry(key(s, a)).or_default().push(k);
        at.entry(key(s, b)).or_default().push(k);
    }
    let mut out = vec![];
    for (nodekey, ks) in &at {
        if ks.len() != 1 {
            continue;
        }
        let k = ks[0];
        let (s, a, b, half) = pieces[k];
        // the free end is the one of this piece that is the node
        let (free, far) = if key(s, a) == *nodekey { (a, b) } else { (b, a) };
        if anchored(s, free, half) {
            continue;
        }
        let (mut length, mut points, mut prev, mut cur) = (free.dist(far), vec![free, far], k, far);
        let found = loop {
            let here = &at[&key(s, cur)];
            if anchored(s, cur, pieces[prev].3) || here.len() >= 3 {
                break true;
            }
            if here.len() == 1 {
                break false;
            }
            let Some(&n) = here.iter().find(|&&n| n != prev) else { break false };
            let (_, na, nb, _) = pieces[n];
            let next = if key(s, na) == key(s, cur) { nb } else { na };
            length += cur.dist(next);
            points.push(next);
            prev = n;
            cur = next;
            if points.len() > pieces.len() + 2 {
                break false;
            }
        };
        if found {
            out.push(Stub { length, points });
        }
    }
    out.sort_by(|a, b| b.length.total_cmp(&a.length));
    out
}

/// How many times a way bends (a turn of more than 1 degree on one layer).
pub fn way_bends(way: &[(Side, Point)]) -> usize {
    let mut dirs: Vec<(f64, f64)> = vec![];
    for w in way.windows(2) {
        let ((s0, p), (s1, q)) = (w[0], w[1]);
        let l = p.dist(q);
        if s0 == s1 && l > 1e-6 {
            dirs.push(((q.x - p.x) / l, (q.y - p.y) / l));
        }
    }
    dirs.windows(2).filter(|d| d[0].0 * d[1].0 + d[0].1 * d[1].1 < 1.0_f64.to_radians().cos()).count()
}

/// How long two sets of nets run side by side.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParallelRun {
    /// Length (mm) of the tracks of the first nets that have copper of the second nets within reach on the same layer.
    pub length: f64,
    /// The stretches, as runs of points (to draw).
    pub runs: Vec<Vec<Point>>,
}

/// Every 0.2 mm along a track of the nets `a`: is there copper of the nets `b` on the same layer within `reach` mm of the track's edge? The
/// length of those stretches. The pads of the parts in `except` (or all pads, `pads`) are not copper of `b`. None when `a` has no track.
pub fn parallel_run(index: &CopperIndex, board: &Board, a: &[String], b: &[String], reach: f64, except: &[String]) -> Option<ParallelRun> {
    let (items, item_net) = (&index.items, &index.nets);
    let (nt, np) = (board.tracks.len(), board.pads.len());
    let others: Vec<usize> = (0..items.len())
        .filter(|&i| item_net[i].as_deref().is_some_and(|n| in_set(b, n)))
        .filter(|&i| !(i >= nt && i < nt + np && except.iter().any(|e| e == "pads" || *e == board.pads[i - nt].refdes)))
        .collect();
    let mut out = ParallelRun::default();
    let mut any = false;
    let step = 0.2;
    for i in 0..nt {
        if !item_net[i].as_deref().is_some_and(|n| in_set(a, n)) {
            continue;
        }
        let t = &board.tracks[i];
        let len = t.a.dist(t.b);
        if len < 1e-6 {
            continue;
        }
        any = true;
        let n = ((len / step).ceil() as usize).max(1);
        let piece = len / n as f64;
        let mut run: Vec<Point> = vec![];
        for k in 0..n {
            let f = (k as f64 + 0.5) / n as f64;
            let p = Point::new(t.a.x + (t.b.x - t.a.x) * f, t.a.y + (t.b.y - t.a.y) * f);
            let probe = Geo::Cap { a: p, b: p, r: t.width / 2.0 };
            if others.iter().any(|&g| items[g].on(t.side) && dist(&probe, &items[g].geo) <= reach) {
                out.length += piece;
                run.push(p);
            } else if run.len() > 1 {
                out.runs.push(std::mem::take(&mut run));
            } else {
                run.clear();
            }
        }
        if run.len() > 1 {
            out.runs.push(run);
        }
    }
    any.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(b: &mut Board, x: f64, y: f64, r: &str, pin: &str, net: &str) {
        let mut p = Pad::new(Point::new(x, y), PadShape::Circle { d: 1.0 }, None, Side::Top);
        p.refdes = r.into();
        p.pin = pin.into();
        p.net = net.into();
        b.pads.push(p);
    }

    fn track(b: &mut Board, side: Side, a: (f64, f64), c: (f64, f64)) {
        b.tracks.push(Track { side, a: Point::new(a.0, a.1), b: Point::new(c.0, c.1), width: 0.25 });
    }

    fn r(s: &str) -> PadRef {
        PadRef::parse(s).unwrap()
    }

    #[test]
    fn a_pad_reference_is_part_dot_pin() {
        assert_eq!(PadRef::parse("RS1.1"), Some(PadRef { refdes: "RS1".into(), pin: "1".into() }));
        assert_eq!(PadRef::parse("U1.A.2").unwrap().refdes, "U1.A");
        assert_eq!(PadRef::parse("RS1"), None);
        assert_eq!(PadRef::parse(".1"), None);
    }

    #[test]
    fn the_path_follows_the_tracks_and_counts_the_length() {
        let mut b = Board::new("t", 50.0, 50.0);
        pad(&mut b, 5.0, 5.0, "A", "1", "N");
        pad(&mut b, 25.0, 5.0, "B", "1", "N");
        track(&mut b, Side::Top, (5.0, 5.0), (15.0, 5.0));
        track(&mut b, Side::Top, (15.0, 5.0), (15.0, 9.0));
        track(&mut b, Side::Top, (15.0, 9.0), (25.0, 5.0));
        let g = CopperGraph::build(&b);
        let p = g.path(&r("A.1"), &r("B.1")).expect("a path");
        let expect = 10.0 + 4.0 + (100.0f64 + 16.0).sqrt();
        assert!((p.length - expect).abs() < 1e-3, "{} vs {expect}", p.length);
        assert_eq!((p.vias, p.layers.clone()), (0, vec![Side::Top]));
        assert_eq!(p.points.first().map(|x| x.1), Some(Point::new(5.0, 5.0)));
        // no copper between A and C: no path, however near
        pad(&mut b, 5.0, 30.0, "C", "1", "N");
        let g = CopperGraph::build(&b);
        assert!(g.path(&r("A.1"), &r("C.1")).is_none());
        assert!(g.path(&r("A.1"), &r("Z.9")).is_none(), "a pad that is not on the board");
    }

    #[test]
    fn a_via_changes_the_layer_and_is_counted() {
        let mut b = Board::new("t", 50.0, 50.0);
        pad(&mut b, 5.0, 5.0, "A", "1", "N");
        pad(&mut b, 25.0, 5.0, "B", "1", "N");
        b.pads[1].side = Side::Bottom;
        track(&mut b, Side::Top, (5.0, 5.0), (15.0, 5.0));
        b.vias.push(Via { pos: Point::new(15.0, 5.0), diameter: 0.6, drill: 0.3, net: String::new() });
        track(&mut b, Side::Bottom, (15.0, 5.0), (25.0, 5.0));
        let g = CopperGraph::build(&b);
        let p = g.path(&r("A.1"), &r("B.1")).expect("a path");
        assert_eq!(p.vias, 1);
        assert_eq!(p.layers, vec![Side::Top, Side::Bottom]);
        assert!((p.length - 20.0).abs() < 1e-3, "{}", p.length);
    }

    #[test]
    fn two_ways_part_where_a_branch_leaves_the_track() {
        let mut b = Board::new("t", 50.0, 50.0);
        pad(&mut b, 5.0, 5.0, "R", "1", "N");
        pad(&mut b, 25.0, 5.0, "F", "1", "N");
        pad(&mut b, 8.0, 15.0, "S", "1", "N");
        // the force track runs on along x; the sense branch leaves it 3 mm from the pad
        track(&mut b, Side::Top, (5.0, 5.0), (25.0, 5.0));
        track(&mut b, Side::Top, (8.0, 5.0), (8.0, 15.0));
        let g = CopperGraph::build(&b);
        let f = g.fork(&r("R.1"), &r("F.1"), &r("S.1")).expect("a fork");
        assert!((f.length - 3.0).abs() < 1e-3, "{f:?}");
        assert!(f.at.dist(Point::new(8.0, 5.0)) < 1e-3);
        assert!((f.a_length - 20.0).abs() < 1e-3 && (f.b_length - 13.0).abs() < 1e-3, "{f:?}");
    }

    #[test]
    fn ways_that_leave_the_pad_on_their_own_part_at_the_pad() {
        let mut b = Board::new("t", 50.0, 50.0);
        pad(&mut b, 5.0, 5.0, "R", "1", "N");
        pad(&mut b, 25.0, 5.0, "F", "1", "N");
        pad(&mut b, 5.0, 20.0, "S", "1", "N");
        track(&mut b, Side::Top, (5.0, 5.0), (25.0, 5.0));
        track(&mut b, Side::Top, (5.0, 5.0), (5.0, 20.0));
        let g = CopperGraph::build(&b);
        let f = g.fork(&r("R.1"), &r("F.1"), &r("S.1")).expect("a fork");
        assert!(f.length < 1e-6, "{f:?}");
    }

    #[test]
    fn a_ring_of_copper_gives_the_shorter_way() {
        let mut b = Board::new("t", 50.0, 50.0);
        pad(&mut b, 5.0, 5.0, "A", "1", "N");
        pad(&mut b, 15.0, 5.0, "B", "1", "N");
        track(&mut b, Side::Top, (5.0, 5.0), (15.0, 5.0));
        track(&mut b, Side::Top, (5.0, 5.0), (5.0, 15.0));
        track(&mut b, Side::Top, (5.0, 15.0), (15.0, 15.0));
        track(&mut b, Side::Top, (15.0, 15.0), (15.0, 5.0));
        let p = CopperGraph::build(&b).path(&r("A.1"), &r("B.1")).unwrap();
        assert!((p.length - 10.0).abs() < 1e-3, "{}", p.length);
    }

    /// A guard on both sides of a sensitive track is asked for along the track except near its pads; a side with no guard for the second
    /// half is found, with the bare run to draw.
    #[test]
    fn the_cover_of_a_guard_is_measured_along_the_sensitive_track() {
        let mut b = Board::new("t", 50.0, 50.0);
        pad(&mut b, 5.0, 5.0, "A", "1", "S");
        pad(&mut b, 25.0, 5.0, "B", "1", "S");
        track(&mut b, Side::Top, (5.0, 5.0), (25.0, 5.0));
        let via = |x: f64, y: f64| Via { pos: Point::new(x, y), diameter: 0.6, drill: 0.3, net: "G".into() };
        // guard tracks 1 mm either side from x = 6 to 24
        for y in [4.0, 6.0] {
            b.vias.push(via(6.0, y));
            track(&mut b, Side::Top, (6.0, y), (24.0, y));
        }
        let s = vec!["S".to_string()];
        let g = vec!["G".to_string()];
        let full = guard_cover(&b, &s, &g, 1.2).expect("tracks to look at");
        assert!(full.percent() > 99.0, "{full:?}");
        assert!((full.total - 16.0).abs() < 0.3, "the 2 mm round each pad is left out: {}", full.total);
        assert!(full.bare.is_empty());
        // the guard on one side stops at x = 15
        b.tracks.retain(|t| !(t.a.y == 6.0));
        b.tracks.push(Track { side: Side::Top, a: Point::new(6.0, 6.0), b: Point::new(15.0, 6.0), width: 0.25 });
        let half = guard_cover(&b, &s, &g, 1.2).unwrap();
        assert!(half.percent() > 45.0 && half.percent() < 70.0, "{half:?}");
        assert!(!half.bare.is_empty() && half.bare[0][0].x > 14.0);
        // a reach too short finds nothing guarded; a net with no track gives None; `+` joins nets
        assert!(guard_cover(&b, &s, &g, 0.2).unwrap().percent() < 1.0);
        assert!(guard_cover(&b, &["NOPE".to_string()], &g, 1.0).is_none());
        assert_eq!(net_set("A+SA + B"), vec!["A", "SA", "B"]);
    }

    /// Against the DRC's own idea of which pads are one piece of copper (without the pours): every such pair should have a path.
    /// Needs a board file (set `CCAD_PATH_BOARD`); not run by default.
    #[test]
    #[ignore]
    fn the_paths_agree_with_the_connectivity_on_a_real_board() {
        let Ok(file) = std::env::var("CCAD_PATH_BOARD") else { return };
        let b: Board = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
        let t0 = std::time::Instant::now();
        let g = CopperGraph::build(&b);
        let built = t0.elapsed();
        let conn = connectivity_without_pours(&b);
        let mut by_group: HashMap<usize, Vec<usize>> = HashMap::new();
        for (i, _) in b.pads.iter().enumerate().filter(|(_, p)| !p.refdes.is_empty() && !p.pin.is_empty()) {
            by_group.entry(conn.pad_group(i)).or_default().push(i);
        }
        let (mut pairs, mut missing) = (0, vec![]);
        for pads in by_group.values().filter(|v| v.len() > 1) {
            let first = &b.pads[pads[0]];
            let a = PadRef { refdes: first.refdes.clone(), pin: first.pin.clone() };
            for &j in &pads[1..] {
                let q = &b.pads[j];
                let to = PadRef { refdes: q.refdes.clone(), pin: q.pin.clone() };
                if to == a {
                    continue;
                }
                pairs += 1;
                if g.path(&a, &to).is_none() {
                    missing.push(format!("{a} -> {to} ({})", first.net));
                }
            }
        }
        println!("built in {built:?}; {pairs} pairs of one copper group, {} without a path: {:?}", missing.len(), &missing[..missing.len().min(12)]);
        assert!(missing.is_empty(), "{} of {pairs}", missing.len());
    }

    /// Prints how much sensitive track a guard looks at on a real board (set `CCAD_PATH_BOARD`); not run by default.
    #[test]
    #[ignore]
    fn the_guard_of_a_real_board_is_looked_at() {
        let Ok(file) = std::env::var("CCAD_PATH_BOARD") else { return };
        let b: Board = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
        for (s, g) in [("A+SA", "GRDA"), ("B+SB", "GRDB")] {
            let c = guard_cover(&b, &net_set(s), &net_set(g), 1.0);
            println!("{s} <- {g}: {:?}", c.map(|c| (c.total, c.covered, c.bare.len())));
        }
    }

    /// How long the checks of the requirements take on a real board (set `CCAD_PATH_BOARD`); not run by default.
    #[test]
    #[ignore]
    fn the_requirements_check_is_timed_on_a_real_board() {
        let Ok(file) = std::env::var("CCAD_PATH_BOARD") else { return };
        let mut b: Board = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
        let time = |f: &dyn Fn()| {
            let t0 = std::time::Instant::now();
            for _ in 0..5 {
                f();
            }
            t0.elapsed() / 5
        };
        println!("check: {:?}", time(&|| drop(crate::requirements::check(&b))));
        println!("marks: {:?}", time(&|| drop(crate::requirements::marks(&b))));
        println!("requirements_hash: {:?}", time(&|| { std::hint::black_box(crate::requirements::requirements_hash(&b)); }));
        println!("geometry_key: {:?}", time(&|| { std::hint::black_box(crate::zonefill::geometry_key(&b)); }));
        println!("measure_marks: {:?}", time(&|| drop(crate::requirements::measure_marks(&b))));
        b.measures.clear();
        println!("check without measures: {:?}", time(&|| drop(crate::requirements::check(&b))));
    }

    /// How long the board's structure computations take on a real board (set `CCAD_PATH_BOARD`); not run by default.
    #[test]
    #[ignore]
    fn the_structure_computations_are_timed_on_a_real_board() {
        let Ok(file) = std::env::var("CCAD_PATH_BOARD") else { return };
        let b: Board = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
        let time = |name: &str, f: &dyn Fn()| {
            let t0 = std::time::Instant::now();
            for _ in 0..5 {
                f();
            }
            println!("{name}: {:?}", t0.elapsed() / 5);
        };
        println!("tracks {} pads {} vias {} zones {}", b.tracks.len(), b.pads.len(), b.vias.len(), b.zones.len());
        time("connectivity (with pours)", &|| drop(connectivity(&b)));
        time("connectivity_without_pours", &|| drop(connectivity_without_pours(&b)));
        time("ratsnest", &|| drop(crate::ratsnest::ratsnest(&b)));
        time("pour_split_airwires", &|| drop(pour_split_airwires(&b)));
        time("open_connections", &|| drop(crate::ratsnest::open_connections(&b)));
        time("CopperGraph::build", &|| drop(CopperGraph::build(&b)));
        time("CopperIndex::build", &|| drop(CopperIndex::build(&b)));
        time("CopperModel::build", &|| drop(CopperModel::build(&b)));
        time("drc::check", &|| drop(check(&b)));
        time("drc::check_final", &|| drop(check_final(&b)));
        time("quality::measure", &|| drop(crate::quality::measure(&b)));
        time("requirements::check", &|| drop(crate::requirements::check(&b)));
        time("zonefill geometry_key", &|| { std::hint::black_box(crate::zonefill::geometry_key(&b)); });
    }
}

