//! A small grid router for two-layer boards.
//!
//! Every unrouted connection (see [`ratsnest`]) is searched on a grid with A*: 45° moves,
//! a penalty for bends (so lines stay straight) and vias to change layers. Copper of other
//! nets is inflated by the clearance and half the track width, so a found path keeps the
//! design rules. Each route is checked with the DRC and thrown away if it makes things worse,
//! so the router never leaves a board in a state it could not leave itself.
//!
//! It does not rip up and retry: connections it cannot place are reported and stay as airwires
//! for the user to finish by hand (or after moving parts).

use crate::drc::{check, connectivity, point_rect, point_seg, shape_geo, Geo};
use crate::geom::inside_polygon;
use crate::model::*;
use crate::ratsnest::{ratsnest, Airwire};
use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(Clone, Debug)]
pub struct RouteOptions {
    pub track_width: f64,
    pub via_diameter: f64,
    pub via_drill: f64,
    /// Grid pitch (mm). Smaller finds more routes but is slower.
    pub grid: f64,
}

impl Default for RouteOptions {
    fn default() -> Self {
        Self { track_width: 0.25, via_diameter: 0.6, via_drill: 0.3, grid: 0.2 }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RouteReport {
    pub routed: usize,
    /// Connections that could not be routed.
    pub failed: Vec<Airwire>,
}

/// Route every airwire of the board (or only those of `only_net`).
pub fn autoroute(board: &mut Board, opt: &RouteOptions, only_net: Option<&str>) -> RouteReport {
    let mut report = RouteReport::default();
    let mut failed: Vec<Airwire> = vec![];
    loop {
        let mut wires: Vec<Airwire> = ratsnest(board)
            .into_iter()
            .filter(|w| only_net.is_none_or(|n| w.net == n))
            .filter(|w| !failed.contains(w))
            .collect();
        // short connections first: they are the easiest and leave the long ones room to detour
        wires.sort_by(|a, b| a.a.dist(a.b).total_cmp(&b.a.dist(b.b)));
        let Some(w) = wires.into_iter().next() else { break };
        if route_airwire(board, &w, opt) {
            report.routed += 1;
        } else {
            failed.push(w);
        }
    }
    report.failed = failed;
    report
}

/// Route one connection. Returns false (and leaves the board untouched) when there is no
/// route or the route would break a rule.
pub fn route_airwire(board: &mut Board, w: &Airwire, opt: &RouteOptions) -> bool {
    let Some((tracks, vias)) = find_route(board, w, opt) else { return false };
    let before = check(board).len();
    let mut trial = board.clone();
    trial.tracks.extend(tracks);
    trial.vias.extend(vias);
    if check(&trial).len() > before {
        return false;
    }
    let pad_at = |p: Point| trial.pads.iter().position(|q| q.pos == p && q.net == w.net);
    let (Some(ia), Some(ib)) = (pad_at(w.a), pad_at(w.b)) else { return false };
    let conn = connectivity(&trial);
    if conn.pad_group(ia) != conn.pad_group(ib) {
        return false;
    }
    *board = trial;
    true
}

// ------------------------------------------------------------------ grid

struct Grid {
    x0: f64,
    y0: f64,
    g: f64,
    nx: usize,
    ny: usize,
    /// Per layer: 0 free, n > 0 only net n may pass, -1 blocked. `trk` is for track centres,
    /// `via` for via centres (larger clearance).
    trk: [Vec<i32>; 2],
    via: [Vec<i32>; 2],
}

fn layer(s: Side) -> usize {
    match s {
        Side::Top => 0,
        Side::Bottom => 1,
    }
}

fn side(l: usize) -> Side {
    if l == 0 { Side::Top } else { Side::Bottom }
}

/// Distance from a point to the edge of a shape (0 inside).
fn geo_dist(p: Point, g: &Geo) -> f64 {
    match *g {
        Geo::Cap { a, b, r } => (point_seg(p, a, b) - r).max(0.0),
        Geo::Rect { c, hw, hh } => point_rect(p, c, hw, hh),
    }
}

fn geo_bbox(g: &Geo) -> (f64, f64, f64, f64) {
    match *g {
        Geo::Cap { a, b, r } => (a.x.min(b.x) - r, a.y.min(b.y) - r, a.x.max(b.x) + r, a.y.max(b.y) + r),
        Geo::Rect { c, hw, hh } => (c.x - hw, c.y - hh, c.x + hw, c.y + hh),
    }
}

impl Grid {
    fn pos(&self, i: usize, j: usize) -> Point {
        Point::new(self.x0 + i as f64 * self.g, self.y0 + j as f64 * self.g)
    }

    /// Cell index range (i0, j0, i1, j1) covering a box grown by `margin`.
    fn cells_near(&self, bb: (f64, f64, f64, f64), margin: f64) -> (usize, usize, usize, usize) {
        let lo_i = ((bb.0 - margin - self.x0) / self.g).floor().max(0.0) as usize;
        let lo_j = ((bb.1 - margin - self.y0) / self.g).floor().max(0.0) as usize;
        let hi_i = (((bb.2 + margin - self.x0) / self.g).ceil().max(0.0) as usize).min(self.nx - 1);
        let hi_j = (((bb.3 + margin - self.y0) / self.g).ceil().max(0.0) as usize).min(self.ny - 1);
        (lo_i, lo_j, hi_i, hi_j)
    }

    /// Mark every cell whose centre is closer than `need_*` to the shape.
    fn mark(&mut self, l: usize, geo: &Geo, net: i32, need_trk: f64, need_via: f64) {
        let (i0, j0, i1, j1) = self.cells_near(geo_bbox(geo), need_trk.max(need_via));
        for j in j0..=j1 {
            for i in i0..=i1 {
                let d = geo_dist(self.pos(i, j), geo);
                let idx = j * self.nx + i;
                if d < need_trk {
                    put(&mut self.trk[l][idx], net);
                }
                if d < need_via {
                    put(&mut self.via[l][idx], net);
                }
            }
        }
    }
}

fn put(cell: &mut i32, net: i32) {
    *cell = if net < 0 || (*cell != 0 && *cell != net) { -1 } else { net };
}

fn passable(cell: i32, net: i32) -> bool {
    cell == 0 || cell == net
}

fn pad_layers(p: &Pad) -> &'static [usize] {
    if p.drill.is_some() {
        &[0, 1]
    } else if p.side == Side::Top {
        &[0]
    } else {
        &[1]
    }
}

/// Net ids start at 1; `nets` maps id - 1 to the name.
fn build_grid(board: &Board, opt: &RouteOptions, nets: &mut Vec<String>) -> Option<Grid> {
    if board.outline.len() < 3 {
        return None;
    }
    let (mut lo, mut hi) = (Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN));
    for p in &board.outline {
        lo = Point::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Point::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    let g = opt.grid;
    // the lattice is anchored at the board's origin (not at the outline's corner), so its points are the same
    // ones the editor's grid snaps to, whatever the outline looks like
    lo = Point::new((lo.x / g).floor() * g, (lo.y / g).floor() * g);
    let (nx, ny) = (((hi.x - lo.x) / g).floor() as usize + 1, ((hi.y - lo.y) / g).floor() as usize + 1);
    if nx * ny > 4_000_000 {
        return None;
    }
    let mut grid = Grid { x0: lo.x, y0: lo.y, g, nx, ny, trk: [vec![0; nx * ny], vec![0; nx * ny]], via: [vec![0; nx * ny], vec![0; nx * ny]] };

    let r = &board.rules;
    // A path sampled on the grid can pass closer to a corner than its cells do, so inflate a little more.
    let widen = |need: f64| (need * need + g * g / 2.0).sqrt();
    let t_need = widen(r.min_clearance + opt.track_width / 2.0 + 0.005);
    let v_need = widen(r.min_clearance + opt.via_diameter / 2.0 + 0.005);
    let t_edge = widen(r.edge_clearance + opt.track_width / 2.0 + 0.005);
    let v_edge = widen(r.edge_clearance + opt.via_diameter / 2.0 + 0.005);

    let n = board.outline.len();
    for e in 0..n {
        let edge = Geo::Cap { a: board.outline[e], b: board.outline[(e + 1) % n], r: 0.0 };
        for l in 0..2 {
            grid.mark(l, &edge, -1, t_edge, v_edge);
        }
    }
    for j in 0..ny {
        for i in 0..nx {
            if !inside_polygon(&board.outline, grid.pos(i, j)) {
                for l in 0..2 {
                    grid.trk[l][j * nx + i] = -1;
                    grid.via[l][j * nx + i] = -1;
                }
            }
        }
    }

    let conn = connectivity(board);
    let mut id_of = |name: Option<&str>| -> i32 {
        match name {
            None => -1,
            Some(nm) => match nets.iter().position(|x| x == nm) {
                Some(k) => k as i32 + 1,
                None => {
                    nets.push(nm.to_string());
                    nets.len() as i32
                }
            },
        }
    };

    for (k, t) in board.tracks.iter().enumerate() {
        let net = id_of(conn.track_net(k));
        let geo = Geo::Cap { a: t.a, b: t.b, r: t.width / 2.0 };
        grid.mark(layer(t.side), &geo, net, t_need, v_need);
    }
    for (k, p) in board.pads.iter().enumerate() {
        let net = id_of(conn.pad_net(k).or(if p.net.is_empty() { None } else { Some(p.net.as_str()) }));
        let geo = shape_geo(p.pos, p.shape);
        for &l in pad_layers(p) {
            grid.mark(l, &geo, net, t_need, v_need);
        }
    }
    for (k, v) in board.vias.iter().enumerate() {
        let net = id_of(conn.via_net(k));
        let geo = Geo::Cap { a: v.pos, b: v.pos, r: v.diameter / 2.0 };
        for l in 0..2 {
            grid.mark(l, &geo, net, t_need, v_need);
        }
    }
    for h in &board.holes {
        let (ha, hb) = h.ends();
        let geo = Geo::Cap { a: ha, b: hb, r: h.drill / 2.0 };
        for l in 0..2 {
            grid.mark(l, &geo, -1, t_need, v_need);
        }
    }
    Some(grid)
}

// ------------------------------------------------------------------ search

#[derive(Clone, Copy, PartialEq)]
struct Node {
    f: f32,
    s: u32,
}

impl Eq for Node {}

impl Ord for Node {
    fn cmp(&self, o: &Self) -> Ordering {
        o.f.total_cmp(&self.f) // reversed: BinaryHeap is a max-heap
    }
}

impl PartialOrd for Node {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

const DIRS: [(i32, i32); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];
const NO_DIR: usize = 8;

/// Grid cells (layer, i, j) from which a pad can be reached.
fn pad_cells(grid: &Grid, pad: &Pad, net: i32) -> Vec<(usize, usize, usize)> {
    let geo = shape_geo(pad.pos, pad.shape);
    let (i0, j0, i1, j1) = grid.cells_near(geo_bbox(&geo), grid.g);
    let mut out = vec![];
    for &l in pad_layers(pad) {
        for j in j0..=j1 {
            for i in i0..=i1 {
                if geo_dist(grid.pos(i, j), &geo) <= grid.g && passable(grid.trk[l][j * grid.nx + i], net) {
                    out.push((l, i, j));
                }
            }
        }
    }
    out
}

fn find_route(board: &Board, w: &Airwire, opt: &RouteOptions) -> Option<(Vec<Track>, Vec<Via>)> {
    let mut nets: Vec<String> = vec![];
    let grid = build_grid(board, opt, &mut nets)?;
    let net = nets.iter().position(|n| *n == w.net)? as i32 + 1;
    let pad_a = board.pads.iter().find(|p| p.pos == w.a && p.net == w.net)?;
    let pad_b = board.pads.iter().find(|p| p.pos == w.b && p.net == w.net)?;
    let starts = pad_cells(&grid, pad_a, net);
    let goals = pad_cells(&grid, pad_b, net);
    if starts.is_empty() || goals.is_empty() {
        return None;
    }

    let (nx, ny) = (grid.nx, grid.ny);
    let n_states = 2 * nx * ny * 9;
    let enc = |l: usize, i: usize, j: usize, d: usize| (((l * ny + j) * nx + i) * 9 + d) as u32;
    let dec = |s: u32| {
        let s = s as usize;
        let (cell, d) = (s / 9, s % 9);
        (cell / (nx * ny), cell % nx, (cell / nx) % ny, d)
    };
    let mut is_goal = vec![false; 2 * nx * ny];
    for &(l, i, j) in &goals {
        is_goal[(l * ny + j) * nx + i] = true;
    }
    let gc = grid_cell(&grid, pad_b.pos);
    let g = grid.g as f32;
    let h = |i: usize, j: usize| {
        let (dx, dy) = ((i as f32 - gc.0 as f32).abs(), (j as f32 - gc.1 as f32).abs());
        g * (dx.max(dy) + 0.4142 * dx.min(dy)) * 0.8
    };
    let via_cost = 3.0f32; // a via costs as much as 3 mm of track

    let mut cost = vec![f32::INFINITY; n_states];
    let mut parent = vec![u32::MAX; n_states];
    let mut heap = BinaryHeap::new();
    for &(l, i, j) in &starts {
        let s = enc(l, i, j, NO_DIR);
        let c = grid.pos(i, j).dist(pad_a.pos) as f32;
        if c < cost[s as usize] {
            cost[s as usize] = c;
            heap.push(Node { f: c + h(i, j), s });
        }
    }

    let mut end = None;
    let mut pops = 0usize;
    while let Some(Node { s, .. }) = heap.pop() {
        pops += 1;
        if pops > 3_000_000 {
            return None;
        }
        let (l, i, j, d) = dec(s);
        let c0 = cost[s as usize];
        if is_goal[(l * ny + j) * nx + i] {
            end = Some(s);
            break;
        }
        for (nd, (dx, dy)) in DIRS.iter().enumerate() {
            let (ni, nj) = (i as i32 + dx, j as i32 + dy);
            if ni < 0 || nj < 0 || ni >= nx as i32 || nj >= ny as i32 {
                continue;
            }
            let (ni, nj) = (ni as usize, nj as usize);
            if !passable(grid.trk[l][nj * nx + ni], net) {
                continue;
            }
            // never cut a corner between two blocked cells
            let diagonal = *dx != 0 && *dy != 0;
            if diagonal && !(passable(grid.trk[l][j * nx + ni], net) && passable(grid.trk[l][nj * nx + i], net)) {
                continue;
            }
            let step = if diagonal { g * std::f32::consts::SQRT_2 } else { g };
            let turn = if d == NO_DIR {
                0.0
            } else {
                let diff = (nd as i32 - d as i32).rem_euclid(8);
                g * 0.4 * diff.min(8 - diff) as f32
            };
            let ns = enc(l, ni, nj, nd);
            let nc = c0 + step + turn;
            if nc < cost[ns as usize] {
                cost[ns as usize] = nc;
                parent[ns as usize] = s;
                heap.push(Node { f: nc + h(ni, nj), s: ns });
            }
        }
        // change layer with a via
        let o = 1 - l;
        let idx = j * nx + i;
        if passable(grid.via[l][idx], net) && passable(grid.via[o][idx], net) && passable(grid.trk[o][idx], net) {
            let ns = enc(o, i, j, NO_DIR);
            let nc = c0 + via_cost;
            if nc < cost[ns as usize] {
                cost[ns as usize] = nc;
                parent[ns as usize] = s;
                heap.push(Node { f: nc + h(i, j), s: ns });
            }
        }
    }
    let end = end?;

    let mut states = vec![end];
    while parent[*states.last().unwrap() as usize] != u32::MAX {
        states.push(parent[*states.last().unwrap() as usize]);
    }
    states.reverse();
    let cells: Vec<(usize, Point, usize)> = states
        .iter()
        .map(|&s| {
            let (l, i, j, d) = dec(s);
            (l, grid.pos(i, j), d)
        })
        .collect();

    // straight runs become one track each; a layer change becomes a via
    let mut tracks = vec![];
    let mut vias = vec![];
    let mut emit = |l: usize, a: Point, b: Point| {
        if a.dist(b) > 1e-9 {
            tracks.push(Track { side: side(l), a, b, width: opt.track_width });
        }
    };
    emit(cells[0].0, pad_a.pos, cells[0].1);
    let mut run_from = cells[0].1;
    let mut run_dir: Option<usize> = None;
    for k in 1..cells.len() {
        let (l, p, d) = cells[k];
        if l != cells[k - 1].0 {
            emit(cells[k - 1].0, run_from, cells[k - 1].1);
            vias.push(Via { pos: p, diameter: opt.via_diameter, drill: opt.via_drill });
            run_from = p;
            run_dir = None;
        } else {
            match run_dir {
                Some(rd) if rd != d => {
                    emit(l, run_from, cells[k - 1].1);
                    run_from = cells[k - 1].1;
                    run_dir = Some(d);
                }
                None => run_dir = Some(d),
                _ => {}
            }
        }
    }
    let last = cells[cells.len() - 1];
    emit(last.0, run_from, last.1);
    emit(last.0, last.1, pad_b.pos);
    Some((tracks, vias))
}

fn grid_cell(grid: &Grid, p: Point) -> (usize, usize) {
    let i = (((p.x - grid.x0) / grid.g).round().max(0.0) as usize).min(grid.nx - 1);
    let j = (((p.y - grid.y0) / grid.g).round().max(0.0) as usize).min(grid.ny - 1);
    (i, j)
}
