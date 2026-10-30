//! Voltage drop along a net's copper: the tracks, vias and pads of a net become a network of
//! resistors, and the resistance from the supply pad to every other pad of the net says how much
//! voltage the wiring eats at the net's declared current (`Board::net_currents`).
//!
//! Assumptions (kept simple on purpose, shown to the user):
//! * the current enters at one pad (a connector if there is one, else the first pad of the net) and every other
//!   pad is looked at as if it alone drew the whole current, which is the worst case for that pad;
//! * a copper pour is a grid of square cells (the sheet resistance between neighbours), each pad, via or track end under it touching
//!   the cells around it through the spreading resistance of a contact of its size, so the pour's shape counts: a narrow strip has
//!   as many squares between two pads as its length over its width. The copper is the pour as it is filled (`zonefill`): the
//!   clearance around the copper of other nets is cut out, and a through-hole pad with thermal relief is tied by its four spokes.

use std::collections::HashMap;

use crate::drc::{self, Connectivity};
use crate::model::*;
use crate::zonefill;

/// Resistivity of copper at about 25 C, in ohm * mm.
const RHO_MM: f64 = 1.72e-5;
/// Board thickness a via barrel crosses (mm).
pub(crate) const BOARD_MM: f64 = 1.6;
/// Plating thickness of a via barrel (mm).
const PLATING_MM: f64 = 0.025;
/// Largest network that is solved; bigger nets are reported as too large. A pour adds up to `POUR_CELLS` cells.
const MAX_NODES: usize = 9000;
/// Cells of the grid a copper pour is cut into (the pitch is chosen from its area; 0.3 to 1.5 mm).
const POUR_CELLS: f64 = 4000.0;

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

/// Partial self-inductance of `height` mm (the stretch of the barrel a current crosses) of a plated hole, taken as a round solid wire
/// of the drill diameter: 0.2 nH/mm * h * (ln(2 h / r) - 0.75), about 0.74 nH for a 0.3 mm hole through 1.6 mm. (The common
/// "5.08 h (ln(4h/d) + 1)" is a loop figure with a return path and is about 1.8 times too high for a segment of a path whose other
/// parts are counted separately. The real barrel is hollow, which makes it roughly 20 % lower still.)
pub fn via_inductance(drill: f64, height: f64) -> f64 {
    let r = drill.max(0.1) / 2.0;
    let h = height.max(0.01);
    (0.2e-9 * h * ((2.0 * h / r).ln() - 0.75)).max(0.0)
}

/// Relative permittivity of the board (FR-4).
const EPS_R: f64 = 4.5;
/// Vacuum permittivity (F/mm).
const EPS0_PER_MM: f64 = 8.854e-15;

/// Capacitance (F) of a trace of `len` and `width` (mm), `height` mm over a plane: Sakurai-Tamaru,
/// `C / (eps len) = 1.15 w/h + 2.80 (t/h)^0.222` (the first term is the parallel plate with a little fringe, the second the
/// edges). Within about 6 % of a field solver for w/h from 0.3 to 30; outside that it is a rough figure, which is what a
/// first look at ringing needs. Neighbouring traces are not counted, only the plane.
pub fn track_capacitance(len: f64, width: f64, height: f64, spec: &IrSpec) -> f64 {
    if len <= 1e-6 || height <= 1e-6 {
        return 0.0;
    }
    let t = spec.thickness_um / 1000.0;
    EPS_R * EPS0_PER_MM * len * (1.15 * width.max(0.01) / height + 2.80 * (t / height).powf(0.222))
}

/// Capacitance (F) of the copper of a path to the pours under or over it (`PathSeg::plane`); the pour is taken as the circuit's
/// ground. Stretches of vias and of tracks with no pour near them count nothing.
fn path_capacitance(path: &[PathSeg], spec: &IrSpec) -> f64 {
    path.iter().filter_map(|s| s.plane.map(|pz| track_capacitance(s.a.dist(s.b), s.width, (pz - s.z).abs(), spec))).sum()
}

/// Inductance charged for a current that goes through a copper pour (H): the plane is wide, so only a little.
const POUR_HENRY: f64 = 0.3e-9;

/// Resistance of `height` mm (the stretch between two layers) of the barrel of a plated hole of `drill` (mm).
pub fn via_resistance(drill: f64, height: f64) -> f64 {
    let area = std::f64::consts::PI * (drill + PLATING_MM) * PLATING_MM;
    RHO_MM * height / area
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
    /// Height of the layer above the top copper (mm), at `a`; `z2` is the height at `b` (different for a stretch of a via, where
    /// `a` and `b` are the same point and `width` is the drill).
    pub z: f64,
    pub z2: f64,
    pub width: f64,
    /// Height of a copper pour of another net on another layer that lies under (or over) this piece: the return current of
    /// the trace runs in it, which is worked out with an image of the trace in the pour (see `image_of`).
    pub plane: Option<f64>,
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
    Track { width: f64 },
    Via(f64),
    /// A pad, via or track end touching a pour: counted as a little inductance (the current goes into the plane).
    Pour,
    /// A link between two cells of a pour's grid: no inductance of its own (the pour is wide; see `image_of`).
    Cell,
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

/// Point-in-copper tests for the pieces of a pour (outer contour minus holes), with a bounding box in front of every contour.
struct FillIndex {
    shapes: Vec<FillBox>,
}

struct FillBox {
    outer: (Point, Point, Vec<Point>),
    holes: Vec<(Point, Point, Vec<Point>)>,
}

fn bounds_of(poly: &[Point]) -> (Point, Point) {
    poly.iter().fold((Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN)), |(l, h), p| (Point::new(l.x.min(p.x), l.y.min(p.y)), Point::new(h.x.max(p.x), h.y.max(p.y))))
}

impl FillIndex {
    fn new(shapes: &[zonefill::FillShape]) -> Self {
        let boxed = |poly: &Vec<Point>| {
            let (lo, hi) = bounds_of(poly);
            (lo, hi, poly.clone())
        };
        Self { shapes: shapes.iter().map(|s| FillBox { outer: boxed(&s.outer), holes: s.holes.iter().map(boxed).collect() }).collect() }
    }

    fn contains(&self, p: Point) -> bool {
        let inside = |b: &(Point, Point, Vec<Point>)| p.x >= b.0.x && p.x <= b.1.x && p.y >= b.0.y && p.y <= b.1.y && in_polygon(&b.2, p);
        self.shapes.iter().any(|s| inside(&s.outer) && !s.holes.iter().any(inside))
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

/// Is this reference designator a connector / battery / switch / voltage source, i.e. where power usually comes in?
fn looks_like_supply(refdes: &str) -> bool {
    let up = refdes.to_ascii_uppercase();
    // a voltage source ("V1"), too: without a connector the first pad of the net used to be taken, and that follows
    // the order of the pad list, which moving a part changes
    let source = up.strip_prefix('V').is_some_and(|rest| rest.chars().next().is_some_and(|c| c.is_ascii_digit()));
    (source || ["J", "P", "CN", "X", "BT", "SW"].iter().any(|p| up.starts_with(p))) && up.chars().last().is_some_and(|c| c.is_ascii_digit())
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
    // copper pours of other nets: (height, polygon), which act as the return plane of the traces of this net
    let planes: Vec<(f64, Vec<Point>)> = board
        .zones
        .iter()
        .filter(|z| z.net != net && z.has_area())
        .filter_map(|z| layers.iter().position(|&x| x == z.side).map(|i| (layer_z(&layers, i), z.region())))
        .flat_map(|(h, region)| region.into_iter().map(move |piece| (h, piece.outer)))
        .collect();
    // pads: a through-hole pad is on both layers, joined by its barrel
    // one step of a barrel, between neighbouring copper layers
    let layer_h = BOARD_MM / (layers.len().max(2) - 1) as f64;
    let mut pad_node: HashMap<usize, usize> = HashMap::new();
    for &i in &pad_ids {
        let p = &board.pads[i];
        let n = g.node(p.side, p.pos);
        pad_node.insert(i, n);
        if let Some(d) = p.drill {
            // the barrel joins every copper layer, one edge per step between neighbouring layers
            for w in layers.windows(2) {
                let (x, y) = (g.node(w[0], p.pos), g.node(w[1], p.pos));
                g.edge(x, y, via_resistance(d, layer_h), Link::Via(d));
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
                        g.edge(x, y, via_resistance(v.drill, layer_h), Link::Via(v.drill));
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
            g.edge(on[k].1, on[k + 1].1, r, Link::Track { width: w });
        }
    }
    // copper pours: a grid of square cells (the sheet resistance between neighbours); every pad, via or track end that is under a
    // pour touches the cells around it, through the spreading resistance of a contact of its size
    let pad_of_node: HashMap<usize, usize> = pad_node.iter().map(|(&pi, &n)| (n, pi)).collect();
    let contact_radius: HashMap<usize, f64> = pad_node.iter().map(|(&pi, &n)| (n, board.pads[pi].shape.size().0.min(board.pads[pi].shape.size().1) / 2.0)).collect();
    for z in board.zones.iter().filter(|z| z.net == net && z.has_area()) {
        // the area of the pour (its pieces without their holes) and the box round it
        let region = z.region();
        let a = region.iter().map(|s| area(&s.outer) - s.holes.iter().map(|h| area(h)).sum::<f64>()).sum::<f64>().max(1.0);
        let step = (a / POUR_CELLS).sqrt().clamp(0.3, 1.5);
        let (lo, hi) = region.iter().flat_map(|s| s.outer.iter()).fold((Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN)), |(l, h), p| (Point::new(l.x.min(p.x), l.y.min(p.y)), Point::new(h.x.max(p.x), h.y.max(p.y))));
        let (nx, ny) = (((hi.x - lo.x) / step).ceil().max(1.0) as usize, ((hi.y - lo.y) / step).ceil().max(1.0) as usize);
        if nx * ny > 4 * POUR_CELLS as usize {
            out.note = Some("ベタが大きすぎて、電圧降下は調べられません".into());
            return out;
        }
        let centre = |ix: usize, iy: usize| Point::new(lo.x + (ix as f64 + 0.5) * step, lo.y + (iy as f64 + 0.5) * step);
        // the copper that is really there: the polygon clipped to the board, without the clearance around the copper of other nets
        let fill = FillIndex::new(&zonefill::zone_fill(board, z, conn));
        let mut cell: Vec<Option<usize>> = vec![None; nx * ny];
        let mut cover = vec![0.0f64; nx * ny];
        for ix in 0..nx {
            for iy in 0..ny {
                // the share of the cell that is copper, from nine sample points; a cell with less than a fifth is not there
                let c = centre(ix, iy);
                let hit = (0..9).filter(|k| fill.contains(Point::new(c.x + ((k % 3) as f64 - 1.0) * step / 3.0, c.y + ((k / 3) as f64 - 1.0) * step / 3.0))).count();
                if hit >= 2 {
                    cover[ix * ny + iy] = hit as f64 / 9.0;
                    cell[ix * ny + iy] = Some(g.pos.len());
                    g.pos.push((z.side, c));
                }
            }
        }
        // a link between two cells has the sheet resistance of a square when both are full, and more when copper covers only part of them
        let link = |a: usize, b: usize| spec.sheet() / (0.5 * (cover[a] + cover[b])).clamp(0.2, 1.0);
        for ix in 0..nx {
            for iy in 0..ny {
                let Some(me) = cell[ix * ny + iy] else { continue };
                if ix + 1 < nx && let Some(r) = cell[(ix + 1) * ny + iy] {
                    g.edge(me, r, link(ix * ny + iy, (ix + 1) * ny + iy), Link::Cell);
                }
                if iy + 1 < ny && let Some(u) = cell[ix * ny + iy + 1] {
                    g.edge(me, u, link(ix * ny + iy, ix * ny + iy + 1), Link::Cell);
                }
            }
        }
        for (n, sd, p) in all.iter().copied() {
            if sd != z.side || !region.iter().any(|s| in_polygon(&s.outer, p) && !s.holes.iter().any(|h| in_polygon(h, p))) {
                continue;
            }
            // the nearest cells, as many as it takes for them (tied together by short links) to spread the current like a disc of
            // the contact's radius; no fewer than one
            let r = contact_radius.get(&n).copied().unwrap_or(0.5).max(0.05);
            let (cx, cy) = (((p.x - lo.x) / step) as isize, ((p.y - lo.y) / step) as isize);
            let span = ((r / step).ceil() as isize + 2).min(60);
            let mut cand: Vec<(f64, usize)> = vec![];
            for ix in (cx - span).max(0)..=(cx + span).min(nx as isize - 1) {
                for iy in (cy - span).max(0)..=(cy + span).min(ny as isize - 1) {
                    if let Some(c) = cell[ix as usize * ny + iy as usize] {
                        cand.push((centre(ix as usize, iy as usize).dist(p), c));
                    }
                }
            }
            if cand.is_empty() {
                // a node in a thin spur of the pour that holds no cell centre near it: take the nearest cell
                let best = (0..nx).flat_map(|ix| (0..ny).map(move |iy| (ix, iy))).filter_map(|(ix, iy)| cell[ix * ny + iy].map(|c| (centre(ix, iy).dist(p), c))).min_by(|a, b| a.0.total_cmp(&b.0));
                cand.extend(best);
            }
            cand.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut take = 1;
            while take < cand.len() && step * cell_blob_radius(take) < r && cand[take].0 <= r + 1.5 * step {
                take += 1;
            }
            let near: Vec<(f64, usize)> = cand.into_iter().take(take).collect();
            if near.is_empty() {
                continue;
            }
            // a contact of radius r on a plane spreads like RS / (2 pi) ln(r_eq / r) from the equivalent radius of the cells it is
            // tied to up to r, which the grid cannot see (or minus that, when the cells are wider than the pad)
            let n_c = near.len() as f64;
            let r_eq = step * cell_blob_radius(near.len());
            let corr = spec.sheet() / (2.0 * std::f64::consts::PI) * (r_eq / r).ln();
            // a negative correction (cells smaller than the pad would give) is not taken back out of the cell links
            let corr = corr.max(0.0);
            // a through-hole pad of a pour with thermal relief is tied by four spokes across the gap, not by solid copper
            let relief = if z.thermal && pad_of_node.get(&n).is_some_and(|&pi| board.pads[pi].drill.is_some()) { spec.sheet() * z.thermal_gap / (4.0 * z.spoke_width.max(0.05)) } else { 0.0 };
            for (_, c) in near {
                g.edge(n, c, n_c * (corr + relief) + spec.sheet() * 0.1, Link::Pour);
            }
            out.has_zone = true;
        }
    }
    if g.pos.len() > MAX_NODES {
        out.note = Some(format!("配線やベタが細かすぎて ({} 点) 調べられません", g.pos.len()));
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
    let targets: Vec<usize> = pad_ids.iter().copied().filter(|&i| i != src_id && seen[pad_node[&i]] && pad_node[&i] != src).collect();
    // the Laplacian of the network with the supply node grounded, as lists of (neighbour, conductance); one solve per target
    let mut nbr: Vec<Vec<(usize, f64)>> = vec![vec![]; n];
    let mut diag = vec![0.0f64; n];
    for &(x, y, r) in &g.edges {
        if !(seen[x] && seen[y]) {
            continue;
        }
        let c = 1.0 / r;
        diag[x] += c;
        diag[y] += c;
        nbr[x].push((y, c));
        nbr[y].push((x, c));
    }
    let mut res: HashMap<usize, f64> = HashMap::new();
    let mut x = vec![0.0f64; n];
    for &t in &targets {
        // push 1 A into the target; its voltage is the resistance from the supply pad
        let tn = pad_node[&t];
        match cg_resistance(&nbr, &diag, src, tn, &mut x) {
            Some(v) => {
                res.insert(t, v);
            }
            None => {
                out.note = Some("電圧降下の計算が収束しませんでした (配線かベタの形が極端です)".into());
                return out;
            }
        }
    }

    for &i in &pad_ids {
        if i == src_id {
            continue;
        }
        let p = &board.pads[i];
        let ohm = res.get(&i).copied();
        let (henry, path) = match ohm {
            Some(_) => {
                let (h, path) = path_inductance(&g, src, pad_node[&i], spec, &layers, &planes);
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
fn path_inductance(g: &Graph, src: usize, dst: usize, spec: &IrSpec, layers: &[Side], planes: &[(f64, Vec<Point>)]) -> (f64, Vec<PathSeg>) {
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
                let z = layers.iter().position(|&x| x == side).map_or(0.0, |i| layer_z(layers, i));
                let (a, b) = (g.pos[u].1, g.pos[at].1);
                let mid = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
                // the nearest pour on another layer that covers the middle of the piece
                let plane = planes.iter().filter(|(pz, poly)| (pz - z).abs() > 1e-9 && in_polygon(poly, mid)).map(|(pz, _)| *pz).min_by(|x, y| (x - z).abs().total_cmp(&(y - z).abs()));
                segs.push(PathSeg { a, b, z, z2: z, width, plane });
            }
            Link::Via(d) => {
                let zl = |side: Side| layers.iter().position(|&x| x == side).map_or(0.0, |i| layer_z(layers, i));
                segs.push(PathSeg { a: g.pos[u].1, b: g.pos[at].1, z: zl(g.pos[u].0), z2: zl(g.pos[at].0), width: d, plane: None });
            }
            Link::Pour => extra += POUR_HENRY,
            Link::Cell => {}
        }
        at = u;
    }
    segs.reverse();
    let runs = straight_runs(&segs);
    let mut henry = extra;
    for r in &runs {
        if r.iter().all(is_via_piece) {
            // consecutive steps of one barrel: one hole as high as they are together
            henry += via_inductance(r[0].width, r.iter().map(|s| (s.z2 - s.z).abs()).sum());
            continue;
        }
        let len: f64 = r.iter().map(|s| s.a.dist(s.b)).sum();
        let len_over_w: f64 = r.iter().map(|s| s.a.dist(s.b) / s.width.max(0.01)).sum();
        henry += track_inductance(len, if len_over_w > 0.0 { len / len_over_w } else { 1.0 }, spec);
    }
    for i in 0..runs.len() {
        for j in i + 1..runs.len() {
            henry += 2.0 * mutual(runs[i], runs[j], spec);
        }
    }
    // the return current in a pour under the trace: the loop is the trace minus its image in the pour
    let image = image_of(&segs);
    if !image.is_empty() {
        henry -= mutual(&segs, &image, spec);
    }
    (henry, segs)
}

/// A stretch of a plated hole: it goes up or down at one place.
fn is_via_piece(s: &PathSeg) -> bool {
    s.a.dist(s.b) < 1e-9 && (s.z2 - s.z).abs() > 1e-9
}

/// Height (mm) of copper layer number `i` of `layers` below the top face: the board thickness split evenly.
fn layer_z(layers: &[Side], i: usize) -> f64 {
    BOARD_MM * i as f64 / (layers.len().max(2) - 1) as f64
}

/// The images of the pieces of a path that have a pour (`PathSeg::plane`) under or over them: each piece mirrored in the plane of
/// the pour. A trace over a (perfect) plane drives a return current in it that is the mirror image of the trace with the
/// opposite sign, so the loop inductance is `L - M(trace, image)`: 100 mm of 0.25 mm trace 1.6 mm over a plane is 78 nH, not
/// 141 nH, and two traces over the same plane couple with `M(a, b) - M(a, image of b)`.
fn image_of(path: &[PathSeg]) -> Vec<PathSeg> {
    path.iter().filter_map(|s| s.plane.map(|pz| PathSeg { z: 2.0 * pz - s.z, z2: 2.0 * pz - s.z2, plane: None, ..*s })).collect()
}

/// The runs of a path: maximal stretches of consecutive segments that go the same way (within about a degree) on one layer (or
/// steps of one hole).
fn straight_runs(segs: &[PathSeg]) -> Vec<&[PathSeg]> {
    let mut out = vec![];
    let mut from = 0;
    for i in 1..=segs.len() {
        let same = i < segs.len() && {
            let (a, b) = (&segs[i - 1], &segs[i]);
            let vec3 = |s: &PathSeg| [s.b.x - s.a.x, s.b.y - s.a.y, s.z2 - s.z];
            let (va, vb) = (vec3(a), vec3(b));
            let norm = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            let cross = [va[1] * vb[2] - va[2] * vb[1], va[2] * vb[0] - va[0] * vb[2], va[0] * vb[1] - va[1] * vb[0]];
            let (la, lb) = (norm(va), norm(vb));
            la > 1e-12 && lb > 1e-12 && norm(cross) / (la * lb) < 0.02 && va[0] * vb[0] + va[1] * vb[1] + va[2] * vb[2] > 0.0 && (a.z - b.z).abs() < 1e-9 + (a.z2 - a.z).abs().max((b.z2 - b.z).abs())
        };
        if !same {
            out.push(&segs[from..i]);
            from = i;
        }
    }
    out
}

/// Equivalent radius (in cells) of a blob of `n` neighbouring nodes of a square resistor grid held at one potential: the radius
/// of the disc that spreads the current the same way far away. A single node is 0.198 cells (the lattice Green's function), a
/// large blob tends to sqrt(n / pi); in between, from numerical solutions of the lattice (n = 1, 4, 5, 9, 13, 21, 29, 49).
fn cell_blob_radius(n: usize) -> f64 {
    const TABLE: [(f64, f64); 8] = [(1.0, 0.198), (4.0, 0.716), (5.0, 0.953), (9.0, 1.282), (13.0, 1.763), (21.0, 2.179), (29.0, 2.752), (49.0, 3.648)];
    let n = n as f64;
    if n <= 1.0 {
        return TABLE[0].1;
    }
    for w in TABLE.windows(2) {
        if n <= w[1].0 {
            let t = (n - w[0].0) / (w[1].0 - w[0].0);
            return w[0].1 + t * (w[1].1 - w[0].1);
        }
    }
    0.92 * (n / std::f64::consts::PI).sqrt()
}

/// Resistance between `src` and `target` of a network given as Laplacian lists: ground `src`, push 1 A into `target`, and solve for
/// the potentials by conjugate gradients with a Jacobi preconditioner (`x` is a work vector of the size of the network). The
/// potential of `target` is the resistance. `None` when it does not converge.
fn cg_resistance(nbr: &[Vec<(usize, f64)>], diag: &[f64], src: usize, target: usize, x: &mut [f64]) -> Option<f64> {
    let n = diag.len();
    let live: Vec<usize> = (0..n).filter(|&i| diag[i] > 0.0 && i != src).collect();
    for &i in &live {
        x[i] = 0.0;
    }
    x[src] = 0.0;
    let mut r = vec![0.0f64; n];
    r[target] = 1.0;
    let precond = |v: f64, i: usize| v / diag[i];
    let mut z = vec![0.0f64; n];
    for &i in &live {
        z[i] = precond(r[i], i);
    }
    let mut pdir = z.clone();
    let mut rz: f64 = live.iter().map(|&i| r[i] * z[i]).sum();
    let norm0 = 1.0f64;
    let mut ap = vec![0.0f64; n];
    for _ in 0..20_000 {
        for &i in &live {
            let mut v = diag[i] * pdir[i];
            for &(j, c) in &nbr[i] {
                if j != src {
                    v -= c * pdir[j];
                }
            }
            ap[i] = v;
        }
        let pap: f64 = live.iter().map(|&i| pdir[i] * ap[i]).sum();
        if pap <= 0.0 {
            return None;
        }
        let alpha = rz / pap;
        let mut rnorm = 0.0;
        for &i in &live {
            x[i] += alpha * pdir[i];
            r[i] -= alpha * ap[i];
            rnorm += r[i] * r[i];
        }
        if rnorm.sqrt() <= 1e-10 * norm0 {
            return Some(x[target]);
        }
        let mut rz_new = 0.0;
        for &i in &live {
            z[i] = precond(r[i], i);
            rz_new += r[i] * z[i];
        }
        let beta = rz_new / rz;
        rz = rz_new;
        for &i in &live {
            pdir[i] = z[i] + beta * pdir[i];
        }
    }
    None
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
    /// Capacitance (F) of the same copper to the pours under it; see `track_capacitance`.
    pub farad: f64,
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
                let farad = path_capacitance(&p.path, spec);
                out.push(PadSeries { net: net.to_string(), refdes: p.refdes, pin: p.pin, ohm, henry: p.henry.unwrap_or(0.0), farad, path: p.path });
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

/// A short straight piece of a path: where it starts and its vector (mm) in x, y and height, and the trace width (the drill for a
/// stretch of a via).
struct Piece {
    from: [f64; 3],
    v: [f64; 3],
    width: f64,
}

fn len3(s: &PathSeg) -> f64 {
    ((s.b.x - s.a.x).powi(2) + (s.b.y - s.a.y).powi(2) + (s.z2 - s.z).powi(2)).sqrt()
}

fn pieces(path: &[PathSeg]) -> Vec<Piece> {
    let total: f64 = path.iter().map(len3).sum();
    let step = (total / 60.0).clamp(0.3, 2.0);
    let mut out = vec![];
    for s in path {
        let len = len3(s);
        let n = (len / step).ceil().max(1.0) as usize;
        let v = [(s.b.x - s.a.x) / n as f64, (s.b.y - s.a.y) / n as f64, (s.z2 - s.z) / n as f64];
        for i in 0..n {
            out.push(Piece { from: [s.a.x + v[0] * i as f64, s.a.y + v[1] * i as f64, s.z + v[2] * i as f64], v, width: s.width });
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

/// Mutual inductance (H) of two paths, summed over pairs of their pieces. Parallel pieces (horizontal traces, or two holes) use the
/// exact formula for two filaments at the distance of the conductors (softened by the geometric mean distance of a cross-section:
/// 0.2235 (w + t) for a trace, 0.39 d for a round hole, which also keeps it finite); pieces at an angle use 1e-7 H/m * dl1 . dl2 / r
/// between their midpoints, which is nothing between a hole and a trace (they are at a right angle).
fn mutual(a: &[PathSeg], b: &[PathSeg], spec: &IrSpec) -> f64 {
    let (pa, pb) = (pieces(a), pieces(b));
    let t = spec.thickness_um / 1000.0;
    let norm = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let gmd = |p: &Piece| if p.v[0].abs() + p.v[1].abs() < 1e-12 { 0.389 * p.width } else { 0.2235 * (p.width + t) };
    let mut total = 0.0;
    for x in &pa {
        let la = norm(x.v);
        if la < 1e-12 {
            continue;
        }
        let u = [x.v[0] / la, x.v[1] / la, x.v[2] / la];
        for y in &pb {
            let lb = norm(y.v);
            if lb < 1e-12 {
                continue;
            }
            let g = 0.5 * (gmd(x) + gmd(y));
            let r = [y.from[0] - x.from[0], y.from[1] - x.from[1], y.from[2] - x.from[2]];
            let dot = x.v[0] * y.v[0] + x.v[1] * y.v[1] + x.v[2] * y.v[2];
            let cross = [x.v[1] * y.v[2] - x.v[2] * y.v[1], x.v[2] * y.v[0] - x.v[0] * y.v[2], x.v[0] * y.v[1] - x.v[1] * y.v[0]];
            if norm(cross) / (la * lb) < 1e-6 {
                let p0 = r[0] * u[0] + r[1] * u[1] + r[2] * u[2];
                let p1 = p0 + (y.v[0] * u[0] + y.v[1] * u[1] + y.v[2] * u[2]);
                let perp2 = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2] - p0 * p0).max(0.0);
                let sign = if dot >= 0.0 { 1.0 } else { -1.0 };
                total += sign * parallel_mutual(la, p0.min(p1), p0.max(p1), (perp2 + g * g).sqrt());
            } else if dot.abs() > 1e-12 {
                let m = [r[0] + 0.5 * (y.v[0] - x.v[0]), r[1] + 0.5 * (y.v[1] - x.v[1]), r[2] + 0.5 * (y.v[2] - x.v[2])];
                // mm * mm / mm = mm; 1e-7 H/m = 1e-10 H per mm
                total += 1e-10 * dot / (m[0] * m[0] + m[1] * m[1] + m[2] * m[2] + g * g).sqrt();
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
            let image = image_of(&pads[j].path);
            let m = mutual(&pads[i].path, &pads[j].path, spec) - if image.is_empty() { 0.0 } else { mutual(&pads[i].path, &image, spec) };
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
        b.vias.push(Via { pos: Point::new(10.0, 0.0), diameter: 0.6, drill: 0.3, net: String::new() });
        let d = drops(&b, 1.0);
        let r = d.pads.iter().find(|p| p.refdes == "U1").unwrap().ohm.unwrap();
        let tracks_only = 2.0 * track_resistance(10.0, 0.5, &IrSpec::default());
        assert!(r > tracks_only && r < tracks_only + 0.01, "{r} vs {tracks_only}");
        assert!(d.pads.iter().find(|p| p.refdes == "U2").unwrap().ohm.is_none(), "not wired");
    }

    /// Sakurai-Tamaru by hand: 100 mm of 0.25 mm trace, 1.6 mm over FR-4 (4.5): (1.15 * 0.15625 + 2.8 * (0.035 / 1.6)^0.222) = 1.378
    /// times 4.5 * 8.854 pF/m * 0.1 m = 5.49 pF. A trace with no pour near it has no capacitance, and a wider one has more.
    #[test]
    fn a_trace_over_a_pour_has_capacitance_to_it() {
        let mut b = board(vec![pad(0.0, 0.0, "J1", "1", "SIG", None), pad(100.0, 0.0, "U1", "1", "SIG", None)], vec![track(Side::Top, (0.0, 0.0), (100.0, 0.0), 0.25)]);
        assert_eq!(wiring_resistances(&b, &IrSpec::default())[0].farad, 0.0);
        b.zones.push(Zone::new("GND", Side::Bottom, vec![Point::new(-5.0, -20.0), Point::new(105.0, -20.0), Point::new(105.0, 20.0), Point::new(-5.0, 20.0)]));
        let c = wiring_resistances(&b, &IrSpec::default())[0].farad;
        assert!((c * 1e12 - 5.49).abs() < 0.3, "{} pF", c * 1e12);
        assert!(track_capacitance(100.0, 1.0, 1.6, &IrSpec::default()) > track_capacitance(100.0, 0.25, 1.6, &IrSpec::default()));
        assert!(track_capacitance(100.0, 0.25, 0.2, &IrSpec::default()) > 2.0 * track_capacitance(100.0, 0.25, 1.6, &IrSpec::default()), "closer plane, more capacitance");
    }

    #[test]
    fn a_pour_lowers_the_drop() {
        let mut b = board(vec![pad(0.0, 0.0, "J1", "1", "VCC", None), pad(80.0, 0.0, "U1", "1", "VCC", None)], vec![track(Side::Top, (0.0, 0.0), (80.0, 0.0), 0.25)]);
        // a board round the pads (the pour is cut back from the edge, and a pour that touches no pad of its net is left out)
        b.outline = vec![Point::new(-10.0, -25.0), Point::new(90.0, -25.0), Point::new(90.0, 25.0), Point::new(-10.0, 25.0)];
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

    /// One case of `tools/inductance_cases.json`: (name, "self" or "mutual", width, via drill, polylines of (x, y, layer) from
    /// the supply end; two points at the same place on different layers are a via).
    type Case = (String, String, f64, f64, Vec<Vec<(f64, f64, u8)>>, u8, Option<(u8, f64)>);

    fn cases() -> Vec<Case> {
        let v: serde_json::Value = serde_json::from_str(include_str!("../../../tools/inductance_cases.json")).unwrap();
        v.as_array()
            .unwrap()
            .iter()
            .map(|c| {
                let paths = c["paths"].as_array().unwrap().iter().map(|p| p.as_array().unwrap().iter().map(|q| (q[0].as_f64().unwrap(), q[1].as_f64().unwrap(), q[2].as_u64().unwrap() as u8)).collect()).collect();
                let plane = c.get("plane").map(|p| (p["layer"].as_u64().unwrap() as u8, p["margin"].as_f64().unwrap()));
                (c["name"].as_str().unwrap().to_string(), c["kind"].as_str().unwrap().to_string(), c["w"].as_f64().unwrap(), c["drill"].as_f64().unwrap(), paths, c["layers"].as_u64().unwrap() as u8, plane)
            })
            .collect()
    }

    /// The program's inductance for a case, in nH: the self inductance of the first trace, or the mutual inductance of the
    /// pair (0 when the program finds no coupling). Each polyline is one net with a connector pad at its start and a part pad at
    /// its end, and a track per segment.
    fn ours_case(c: &Case) -> f64 {
        let series = ours_series(c);
        let of = |net: &str| series.iter().find(|s| s.net == net).unwrap_or_else(|| panic!("{net} not wired: {series:?}")).henry;
        if c.1 == "self" || c.1 == "loop" {
            return of("N0") * 1e9;
        }
        let k = wiring_couplings(&series, &IrSpec::default());
        k.first().map_or(0.0, |c| c.k * (of("N0") * of("N1")).sqrt() * 1e9)
    }

    /// The program's resistance (ohm) of the first trace of a case, supply end to far end.
    fn ours_ohm(c: &Case) -> f64 {
        ours_series(c).iter().find(|s| s.net == "N0").expect("N0 not wired").ohm
    }

    fn ours_series(c: &Case) -> Vec<PadSeries> {
        let (_, _kind, w, drill, paths, layers, plane) = (&c.0, c.1.as_str(), c.2, c.3, &c.4, c.5, &c.6);
        let side = |l: u8| if l == 0 { Side::Top } else if l + 1 == layers { Side::Bottom } else { Side::Inner(l) };
        let mut pads = vec![];
        let mut tracks = vec![];
        let mut vias = vec![];
        for (k, path) in paths.iter().enumerate() {
            let net = format!("N{k}");
            for (i, (end, r)) in [(path[0], "J"), (path[path.len() - 1], "U")].into_iter().enumerate() {
                let mut p = Pad::new(Point::new(end.0, end.1), PadShape::Circle { d: 0.1 }, None, side(end.2));
                p.refdes = format!("{r}{k}");
                p.pin = format!("{}", i + 1);
                p.net = net.clone();
                pads.push(p);
            }
            for q in path.windows(2) {
                if (q[0].0, q[0].1) == (q[1].0, q[1].1) {
                    vias.push(Via { pos: Point::new(q[0].0, q[0].1), diameter: 2.0 * drill, drill, net: String::new() });
                } else {
                    tracks.push(track(side(q[0].2), (q[0].0, q[0].1), (q[1].0, q[1].1), w));
                }
            }
        }
        let mut b = board(pads, tracks);
        b.vias = vias;
        b.inner_layers = layers - 2;
        if let Some((layer, margin)) = plane {
            // a pour of net GND over the traces and `margin` beyond them
            let xs: Vec<f64> = paths.iter().flatten().map(|p| p.0).collect();
            let ys: Vec<f64> = paths.iter().flatten().map(|p| p.1).collect();
            let (x0, x1) = (xs.iter().cloned().fold(f64::MAX, f64::min) - margin, xs.iter().cloned().fold(f64::MIN, f64::max) + margin);
            let (y0, y1) = (ys.iter().cloned().fold(f64::MAX, f64::min) - margin, ys.iter().cloned().fold(f64::MIN, f64::max) + margin);
            b.zones.push(Zone::new("GND", side(*layer), vec![Point::new(x0, y0), Point::new(x1, y0), Point::new(x1, y1), Point::new(x0, y1)]));
        }
        wiring_resistances(&b, &IrSpec::default())
    }

    /// The program's resistance (ohm) between two pads of a pour of `tools/pour_cases.json` (a rectangle of `w` x `h`, round pads
    /// of 1.6 mm at `p1` and `p2`).
    fn ours_pour(c: &serde_json::Value) -> f64 {
        // the whole case is moved 5 mm in from the corner of the board so that the pour's edge is not cut back by the board outline
        let off = 5.0;
        let (w, h) = (c["w"].as_f64().unwrap(), c["h"].as_f64().unwrap());
        let pt = |k: &str| (c[k][0].as_f64().unwrap() + off, c[k][1].as_f64().unwrap() + off);
        let mk = |p: (f64, f64), r: &str, net: &str| {
            let mut pad = Pad::new(Point::new(p.0, p.1), PadShape::Circle { d: 1.6 }, None, Side::Top);
            pad.refdes = r.into();
            pad.pin = "1".into();
            pad.net = net.into();
            pad
        };
        let mut pads = vec![mk(pt("p1"), "J0", "N0"), mk(pt("p2"), "U0", "N0")];
        for (k, o) in c.get("others").and_then(|o| o.as_array()).into_iter().flatten().enumerate() {
            pads.push(mk((o[0].as_f64().unwrap() + off, o[1].as_f64().unwrap() + off), &format!("X{k}"), "X"));
        }
        let mut b = board(pads, vec![]);
        let mut zone = Zone::new("N0", Side::Top, vec![Point::new(off, off), Point::new(off + w, off), Point::new(off + w, off + h), Point::new(off, off + h)]);
        zone.clearance = c.get("clear").and_then(|v| v.as_f64()).unwrap_or(0.3);
        b.zones.push(zone);
        wiring_resistances(&b, &IrSpec::default()).iter().find(|s| s.refdes == "U0").expect("U0 not reached").ohm
    }

    fn pour_cases() -> Vec<serde_json::Value> {
        serde_json::from_str::<serde_json::Value>(include_str!("../../../tools/pour_cases.json")).unwrap().as_array().unwrap().clone()
    }

    /// The resistance between two pads of a pour against a finite-difference solution of the same plate (`tools/ref_pour.py`, a
    /// 0.125 mm grid, good to about 3 %): compact plates, pads near an edge or in a corner, and narrow strips used as a power route
    /// (where the number of squares between the pads decides) must agree within 6 %. The first version of the program put every pad
    /// behind a "hub" and was 17 to 76 % low (more than a factor of four on a strip) and 53 % high in a corner.
    #[test]
    fn pour_resistance_agrees_with_a_finite_difference_solution() {
        let table: std::collections::HashMap<String, f64> = include_str!("../../../tools/pour_reference.csv")
            .lines()
            .skip(1)
            .map(|l| {
                let f: Vec<&str> = l.split(',').collect();
                (f[0].to_string(), f[1].parse().unwrap())
            })
            .collect();
        let cases = pour_cases();
        assert_eq!(table.len(), cases.len());
        for c in cases {
            let name = c["name"].as_str().unwrap();
            let (ours, fd) = (ours_pour(&c), table[name]);
            assert!((ours / fd - 1.0).abs() < 0.06, "{name}: {:.3} mohm vs {:.3} mohm", ours * 1e3, fd * 1e3);
        }
    }

    /// A through-hole pad of a pour with thermal relief is tied by four spokes across the gap (0.3 mm gap, 0.3 mm spokes: a quarter
    /// of a square each way, about 0.12 mohm per pad at 35 um copper), which solid connection does not add.
    #[test]
    fn thermal_relief_adds_the_resistance_of_its_spokes() {
        let r = |thermal: bool| {
            let mk = |x: f64, name: &str| {
                let mut p = Pad::new(Point::new(x, 25.0), PadShape::Circle { d: 1.6 }, Some(0.8), Side::Top);
                p.refdes = name.into();
                p.pin = "1".into();
                p.net = "N0".into();
                p
            };
            let mut b = board(vec![mk(30.0, "J0"), mk(70.0, "U0")], vec![]);
            let mut z = Zone::new("N0", Side::Top, vec![Point::new(5.0, 5.0), Point::new(105.0, 5.0), Point::new(105.0, 55.0), Point::new(5.0, 55.0)]);
            z.thermal = thermal;
            b.zones.push(z);
            wiring_resistances(&b, &IrSpec::default()).iter().find(|s| s.refdes == "U0").unwrap().ohm
        };
        let extra = r(true) - r(false);
        assert!((extra - 2.0 * 0.123e-3).abs() < 0.03e-3, "{} mohm", extra * 1e3);
    }

    /// `cargo test -p pcb-core pour_resistance_table -- --ignored --nocapture`: `case,ohm` of every case of `tools/pour_cases.json`.
    #[test]
    #[ignore]
    fn pour_resistance_table() {
        println!("case,ohm");
        for c in pour_cases() {
            println!("{},{:.6e}", c["name"].as_str().unwrap(), ours_pour(&c));
        }
    }

    /// The resistance of a trace against FastHenry2 (Re Z at 1 kHz, `tools/fasthenry_resistance.csv`) for the straight, corner,
    /// dogleg and staircase cases: within 0.5 % (the program's copper is 1.72e-5 ohm mm, FastHenry's 1.724e-5, which is the 0.24 %
    /// it is low by; corners and angles change nothing because both count squares along the centre line).
    #[test]
    fn trace_resistance_agrees_with_fasthenry() {
        let table: Vec<(String, f64)> = include_str!("../../../tools/fasthenry_resistance.csv")
            .lines()
            .skip(1)
            .map(|l| {
                let f: Vec<&str> = l.split(',').collect();
                (f[0].to_string(), f[1].parse().unwrap())
            })
            .collect();
        assert!(table.len() >= 15);
        for (name, fh) in table {
            let c = cases().into_iter().find(|c| c.0 == name).unwrap();
            let ours = ours_ohm(&c);
            assert!((ours / fh - 1.0).abs() < 0.005, "{name}: {:.3} mohm vs FastHenry {:.3} mohm", ours * 1e3, fh * 1e3);
        }
    }

    /// A hole through four layers is one barrel, not three: its resistance and inductance are those of 1.6 mm, as through two layers
    /// (each step between neighbouring layers used to be charged as the whole board).
    #[test]
    fn a_hole_through_four_layers_counts_the_barrel_once() {
        let spec = IrSpec::default();
        let find = |name: &str| cases().into_iter().find(|c| c.0 == name).unwrap();
        let four = find("via through 4 layers");
        let want = 2.0 * track_resistance(1.0, 0.25, &spec) + via_resistance(0.3, BOARD_MM);
        assert!((ours_ohm(&four) / want - 1.0).abs() < 0.01, "{} vs {}", ours_ohm(&four), want);
        // the same stubs and barrel on two layers
        let via2 = find("via d=0.3 with 1 mm stubs");
        assert!((ours_ohm(&via2) - ours_ohm(&four)).abs() < 1e-6);
        assert!((ours_case(&via2) - ours_case(&four)).abs() < 0.01, "{} vs {}", ours_case(&via2), ours_case(&four));
        // a part of the barrel is a part of the height
        let part = find("via from layer 0 to 1 of 4");
        assert!(ours_case(&part) < ours_case(&four) && ours_case(&part) > 0.6 * ours_case(&four));
    }

    /// Two holes side by side couple: with the same current direction `M` is 0.42 nH at 1.27 mm and 0.24 nH at 2.54 mm, and with
    /// opposite directions (supply and return) it lowers the loop (FastHenry: 0.422 and 0.239 nH). Without the coupling the pair
    /// would have shown only what its 1 mm stubs couple.
    #[test]
    fn holes_side_by_side_couple() {
        let find = |name: &str| cases().into_iter().find(|c| c.0 == name).unwrap();
        let (near, far, back) = (ours_case(&find("via pair d=1.27 same way")), ours_case(&find("via pair d=2.54 same way")), ours_case(&find("via pair d=2.54 back (supply and return)")));
        assert!(near > far && far > 0.2 && (back + far).abs() < 0.01, "{near} {far} {back}");
    }

    /// `cargo test -p pcb-core resistance_cases_table -- --ignored --nocapture`: `case,ohm` of the first trace of every plane-free
    /// `self` case without a via (to compare with FastHenry's Re(Z), see `tools/compare_fasthenry.py`).
    #[test]
    #[ignore]
    fn resistance_cases_table() {
        println!("case,ohm");
        for c in cases() {
            if c.1 == "self" && c.6.is_none() && !c.0.contains("via") {
                println!("{},{:.6}", c.0, ours_ohm(&c));
            }
        }
    }

    /// `cargo test -p pcb-core inductance_cases_table -- --ignored --nocapture`: `case,value_nH` for every case of
    /// `tools/inductance_cases.json` (to compare with FastHenry, see `tools/fasthenry_cases.py`).
    #[test]
    #[ignore]
    fn inductance_cases_table() {
        println!("case,value_nH");
        for (name, kind, w, drill, paths, layers, plane) in cases() {
            println!("{name},{:.3}", ours_case(&(name.clone(), kind, w, drill, paths, layers, plane)));
        }
    }

    /// Against FastHenry2 3.6 over every case of `tools/inductance_cases.json` without a plane (parallel traces, corners of 45, 90
    /// and 135 degrees, a dogleg and a staircase, traces at an angle and antiparallel, a pair going round a corner, vias, and traces
    /// on the two layers). The FastHenry figures are in `tools/fasthenry_results.csv` (1 kHz, 12 x 4 filaments; see
    /// `tools/fasthenry_cases.py`): the self inductance of a trace must be within 1.5 % (or 0.1 nH, for the short via cases) and the
    /// mutual inductance of a pair within 1 % (or 0.3 nH when it is nearly zero). The `loop` cases (a trace over a copper pour that
    /// carries the return current) are checked against the textbook microstrip formula in `tools/plane_reference.csv`
    /// (`tools/ref_microstrip.py`) within 6 %: FastHenry2 3.6 did not run planes here, and the formula is for an infinitely long
    /// line, so a 50 mm trace differs by its end effects.
    #[test]
    fn inductance_agrees_with_fasthenry_and_the_microstrip_formula() {
        let read = |text: &str| -> std::collections::HashMap<String, f64> {
            text.lines()
                .skip(1)
                .map(|l| {
                    let f: Vec<&str> = l.split(',').collect();
                    (f[0].to_string(), f[1].parse().unwrap())
                })
                .collect()
        };
        let fasthenry = read(include_str!("../../../tools/fasthenry_results.csv"));
        let microstrip = read(include_str!("../../../tools/plane_reference.csv"));
        let all = cases();
        let (mut with_plane, mut without) = (0, 0);
        for c in all {
            let name = c.0.clone();
            let ours = ours_case(&c);
            if c.6.is_some() {
                // a pour under the traces: only the loop cases have an independent reference
                if c.1 == "loop" {
                    let r = microstrip[&name];
                    assert!((ours / r - 1.0).abs() < 0.06, "{name}: {ours} nH vs microstrip formula {r} nH");
                    with_plane += 1;
                }
                continue;
            }
            let fh = fasthenry[&name];
            let tol = if c.1 == "self" { (0.015 * fh).max(0.1) } else { (0.01 * fh.abs()).max(0.3) };
            assert!((ours - fh).abs() <= tol, "{name}: {ours} nH vs FastHenry {fh} nH");
            without += 1;
        }
        assert_eq!(fasthenry.len(), without, "the FastHenry table and the plane-free cases must match");
        assert_eq!(microstrip.len(), with_plane, "the microstrip table and the loop cases must match");
    }

    /// Two traces over the same plane couple with M(a, b) - M(a, image of b) (Grover's formula for two filaments, 50 mm long,
    /// 2.5 mm apart and 3.2 mm apart from the image: 27.4 - 22.8 = 4.6 nH), far less than the 27 nH they have without the pour.
    #[test]
    fn a_pour_under_two_traces_weakens_their_coupling_by_the_image() {
        let grover = |l: f64, d: f64| 0.2 * l * ((l / d).asinh() - (1.0 + (d / l).powi(2)).sqrt() + d / l);
        let c = cases().into_iter().find(|c| c.0 == "pair over plane l=50 s=2.5 h=1.6").unwrap();
        let with = ours_case(&c);
        let expect = grover(50.0, 2.5) - grover(50.0, (2.5f64 * 2.5 + 3.2 * 3.2).sqrt());
        assert!((with - expect).abs() < 0.3, "{with} vs {expect}");
        let mut bare = c.clone();
        bare.6 = None;
        assert!(ours_case(&bare) > 5.0 * with, "without the pour the traces couple at {}", ours_case(&bare));
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
    fn a_via_adds_about_three_quarters_of_a_nanohenry() {
        let h = via_inductance(0.3, BOARD_MM);
        assert!((0.6e-9..0.9e-9).contains(&h), "{h}");
    }

    #[test]
    fn via_resistance_is_about_a_milliohm() {
        let r = via_resistance(0.3, BOARD_MM);
        assert!((0.0008..0.0015).contains(&r), "{r}");
    }
}
