//! A small grid router for boards with two or more copper layers.
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
    /// Run next to this net (of an already routed differential partner), centre to centre `distance` mm away:
    /// cells at that distance are cheaper, so the new line follows the old one.
    pub follow: Option<(String, f64)>,
    /// Leave and enter rectangular pads at a right angle to their edge for this many mm (the route starts and ends at
    /// a point that far outside the pad, straight out of one of its four edges). `None`: from anywhere around the pad.
    pub pad_lead: Option<f64>,
}

impl Default for RouteOptions {
    fn default() -> Self {
        Self { track_width: 0.25, via_diameter: 0.6, via_drill: 0.3, grid: 0.2, follow: None, pad_lead: None }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RouteReport {
    pub routed: usize,
    /// Connections that could not be routed.
    pub failed: Vec<Airwire>,
}

/// Route every airwire of the board (or only those of `only_net`).
///
/// With a pad lead the whole run is made twice when the first one leaves connections open: once with the feet of the
/// leads slid onto the grid lines (no little steps), once with the leads straight out of the pad's centre. The run
/// that connects more wins (the first, when equal): the order the connections are made in decides what room is left.
pub fn autoroute(board: &mut Board, opt: &RouteOptions, only_net: Option<&str>) -> RouteReport {
    let mut slid = board.clone();
    let first = autoroute_mode(&mut slid, opt, only_net, true);
    if first.failed.is_empty() || opt.pad_lead.is_none() {
        *board = slid;
        return first;
    }
    let mut centred = board.clone();
    let second = autoroute_mode(&mut centred, opt, only_net, false);
    if second.failed.len() < first.failed.len() {
        *board = centred;
        second
    } else {
        *board = slid;
        first
    }
}

fn autoroute_mode(board: &mut Board, opt: &RouteOptions, only_net: Option<&str>, slide: bool) -> RouteReport {
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
        if route_airwire_mode(board, &w, opt, slide) {
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
///
/// With a pad lead it tries the lead slid onto the grid line first, then the plain lead, and last no lead at all, so
/// asking for tidy pad entries never costs a connection the router could have made.
pub fn route_airwire(board: &mut Board, w: &Airwire, opt: &RouteOptions) -> bool {
    route_airwire_mode(board, w, opt, true)
}

/// [`route_airwire`] with the lead's foot slid onto the grid line tried first (`slide`) or not at all.
fn route_airwire_mode(board: &mut Board, w: &Airwire, opt: &RouteOptions, slide: bool) -> bool {
    let plain = RouteOptions { pad_lead: None, ..opt.clone() };
    let attempts: &[(&RouteOptions, bool)] = match (opt.pad_lead.is_some(), slide) {
        (true, true) => &[(opt, true), (opt, false), (&plain, false)],
        (true, false) => &[(opt, false), (&plain, false)],
        _ => &[(opt, false)],
    };
    attempts.iter().any(|&(o, slide)| route_airwire_with(board, w, o, slide))
}

fn route_airwire_with(board: &mut Board, w: &Airwire, opt: &RouteOptions, slide: bool) -> bool {
    let Some((tracks, vias)) = find_route(board, w, opt, slide) else { return false };
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
    trk: Vec<Vec<i32>>,
    via: Vec<Vec<i32>>,
    /// The copper layers, top to bottom: the index is the layer number used in `trk` / `via`.
    layers: Vec<Side>,
    /// Per layer: cells next to the net to follow (empty when not following).
    near: Vec<Vec<bool>>,
    /// Cells where no via may be put because its copper would sit on a pad (any layer, any net).
    no_via: Vec<bool>,
}

impl Grid {
    fn layer(&self, s: Side) -> usize {
        self.layers.iter().position(|&x| x == s).unwrap_or(0)
    }

    fn side(&self, l: usize) -> Side {
        self.layers[l]
    }
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

    /// Forbid vias whose centre is closer than `r` to the pad shape (the via would overlap the pad).
    fn mark_no_via(&mut self, geo: &Geo, r: f64) {
        let (i0, j0, i1, j1) = self.cells_near(geo_bbox(geo), r);
        for j in j0..=j1 {
            for i in i0..=i1 {
                if geo_dist(self.pos(i, j), geo) < r {
                    self.no_via[j * self.nx + i] = true;
                }
            }
        }
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

fn pad_layers(p: &Pad, layers: usize) -> Vec<usize> {
    if p.drill.is_some() {
        (0..layers).collect()
    } else if p.side == Side::Top {
        vec![0]
    } else {
        vec![layers - 1]
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
    let layers = board.copper_layers();
    let nl = layers.len();
    let mut grid = Grid { x0: lo.x, y0: lo.y, g, nx, ny, trk: vec![vec![0; nx * ny]; nl], via: vec![vec![0; nx * ny]; nl], layers, near: vec![], no_via: vec![false; nx * ny] };

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
        for l in 0..nl {
            grid.mark(l, &edge, -1, t_edge, v_edge);
        }
    }
    for j in 0..ny {
        for i in 0..nx {
            if !inside_polygon(&board.outline, grid.pos(i, j)) {
                for l in 0..nl {
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
        grid.mark(grid.layer(t.side), &geo, net, t_need, v_need);
    }
    for (k, p) in board.pads.iter().enumerate() {
        let net = id_of(conn.pad_net(k).or(if p.net.is_empty() { None } else { Some(p.net.as_str()) }));
        let geo = shape_geo(p.pos, p.shape);
        for l in pad_layers(p, nl) {
            grid.mark(l, &geo, net, t_need, v_need);
        }
        grid.mark_no_via(&geo, opt.via_diameter / 2.0);
    }
    for (k, v) in board.vias.iter().enumerate() {
        let net = id_of(conn.via_net(k));
        let geo = Geo::Cap { a: v.pos, b: v.pos, r: v.diameter / 2.0 };
        for l in 0..nl {
            grid.mark(l, &geo, net, t_need, v_need);
        }
    }
    for h in &board.holes {
        let (ha, hb) = h.ends();
        let geo = Geo::Cap { a: ha, b: hb, r: h.drill / 2.0 };
        for l in 0..nl {
            grid.mark(l, &geo, -1, t_need, v_need);
        }
    }
    if let Some((follow, d0)) = &opt.follow {
        grid.near = vec![vec![false; nx * ny]; nl];
        let (lo_d, hi_d) = (d0 - g * 0.6, d0 + g * 0.6);
        for t in board.tracks.iter().enumerate().filter(|(k, _)| conn.track_net(*k) == Some(follow.as_str())).map(|(_, t)| t) {
            let geo = Geo::Cap { a: t.a, b: t.b, r: 0.0 };
            let l = grid.layer(t.side);
            let (i0, j0, i1, j1) = grid.cells_near(geo_bbox(&geo), hi_d);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let d = geo_dist(grid.pos(i, j), &geo);
                    if d >= lo_d && d <= hi_d {
                        grid.near[l][j * nx + i] = true;
                    }
                }
            }
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
    for l in pad_layers(pad, grid.layers.len()) {
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

/// Where a search starts or ends at a pad: a grid cell, the cost of getting from the pad to it, and (for a pad that is
/// left at a right angle) where the straight lead starts on the pad and the point outside its edge it ends at.
#[derive(Clone, Copy)]
struct EndCell {
    l: usize,
    i: usize,
    j: usize,
    cost: f32,
    lead: Option<(Point, Point)>,
}

/// The cells a route may start or end in for `pad`. With a lead length, a rectangular or oval pad is left straight
/// out of one of its four edges (the lead is a clear straight line of cells); where no edge has room, or for other pads,
/// it is any cell next to the pad.
///
/// A pad that sits a hair off the grid would need a tiny jog from the end of its lead to the grid cell. Where the
/// track still fits on the pad, the foot of the lead is slid along the pad's edge instead, so that the lead runs
/// straight to the cell (`slide_width`: the track width; `None`: no sliding).
fn end_cells(grid: &Grid, pad: &Pad, net: i32, lead: Option<f64>, slide_width: Option<f64>) -> Vec<EndCell> {
    if let Some(len) = lead
        && !matches!(pad.shape, PadShape::Circle { .. })
    {
        let (w, h) = pad.shape.size();
        let mut out = vec![];
        for n in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            let half = if n.0 != 0.0 { w / 2.0 } else { h / 2.0 };
            let q = Point::new(pad.pos.x + n.0 * (half + len), pad.pos.y + n.1 * (half + len));
            let (i, j) = grid_cell(grid, q);
            let cell_pos = grid.pos(i, j);
            if cell_pos.dist(q) > grid.g {
                continue; // outside the grid
            }
            // slide the foot sideways onto the cell's line when the track still fits on the pad
            let perp = (-n.1, n.0);
            let side = (cell_pos.x - pad.pos.x) * perp.0 + (cell_pos.y - pad.pos.y) * perp.1;
            let room = (if n.0 != 0.0 { h } else { w }) / 2.0 - 0.05;
            let slide = slide_width.is_some() && side.abs() > 1e-6 && side.abs() <= room && (cell_pos.x - pad.pos.x) * n.0 + (cell_pos.y - pad.pos.y) * n.1 > half + 0.05;
            // the lead stops at the cell's distance when the cell is nearer than `q`: running on to `q` and back would leave
            // a stub sticking out of the corner
            let along = (cell_pos.x - pad.pos.x) * n.0 + (cell_pos.y - pad.pos.y) * n.1;
            let stop = if along > half + 0.05 && along < half + len { Point::new(pad.pos.x + n.0 * along, pad.pos.y + n.1 * along) } else { q };
            let plain = (pad.pos, stop);
            let slid = (Point::new(pad.pos.x + perp.0 * side, pad.pos.y + perp.1 * side), cell_pos);
            let tries: &[(Point, Point)] = if slide { &[slid, plain] } else { &[plain] };
            for l in pad_layers(pad, grid.layers.len()) {
                // the first way whose cells, from the pad's edge to the end, are all free for this net
                for &(foot, end) in tries {
                    // the cells that must be free: up to the lead's end (the lead's end is where the plain lead stops)
                    let reach = if foot == pad.pos { (half + len).max(foot.dist(end)) } else { foot.dist(end) };
                    let steps = (reach / (grid.g * 0.5)).ceil() as usize;
                    let clear = (0..=steps).all(|k| {
                        let t = reach * k as f64 / steps as f64;
                        let p = Point::new(foot.x + n.0 * t, foot.y + n.1 * t);
                        // points inside the pad's own copper are fine: only the cells outside it count
                        if t <= half {
                            return true;
                        }
                        let (ci, cj) = grid_cell(grid, p);
                        passable(grid.trk[l][cj * grid.nx + ci], net)
                    });
                    if clear && passable(grid.trk[l][j * grid.nx + i], net) {
                        // the same cost whichever way the lead is drawn: the choice of route does not depend on the sliding
                        out.push(EndCell { l, i, j, cost: (half + len) as f32 + cell_pos.dist(q) as f32, lead: Some((foot, end)) });
                        break;
                    }
                }
            }
        }
        if !out.is_empty() {
            return out;
        }
    }
    pad_cells(grid, pad, net).into_iter().map(|(l, i, j)| EndCell { l, i, j, cost: grid.pos(i, j).dist(pad.pos) as f32, lead: None }).collect()
}

fn find_route(board: &Board, w: &Airwire, opt: &RouteOptions, slide: bool) -> Option<(Vec<Track>, Vec<Via>)> {
    let mut nets: Vec<String> = vec![];
    let grid = build_grid(board, opt, &mut nets)?;
    let net = nets.iter().position(|n| *n == w.net)? as i32 + 1;
    let pad_a = board.pads.iter().find(|p| p.pos == w.a && p.net == w.net)?;
    let pad_b = board.pads.iter().find(|p| p.pos == w.b && p.net == w.net)?;
    let starts = end_cells(&grid, pad_a, net, opt.pad_lead, slide.then_some(opt.track_width));
    let goals = end_cells(&grid, pad_b, net, opt.pad_lead, slide.then_some(opt.track_width));
    if starts.is_empty() || goals.is_empty() {
        return None;
    }

    let (nx, ny) = (grid.nx, grid.ny);
    let nl = grid.layers.len();
    let n_states = nl * nx * ny * 9;
    let enc = |l: usize, i: usize, j: usize, d: usize| (((l * ny + j) * nx + i) * 9 + d) as u32;
    let dec = |s: u32| {
        let s = s as usize;
        let (cell, d) = (s / 9, s % 9);
        (cell / (nx * ny), cell % nx, (cell / nx) % ny, d)
    };
    let mut is_goal = vec![false; nl * nx * ny];
    for g in &goals {
        is_goal[(g.l * ny + g.j) * nx + g.i] = true;
    }
    let gc = grid_cell(&grid, pad_b.pos);
    let following = !grid.near.is_empty();
    let g = grid.g as f32;
    let h = |i: usize, j: usize| {
        let (dx, dy) = ((i as f32 - gc.0 as f32).abs(), (j as f32 - gc.1 as f32).abs());
        g * (dx.max(dy) + 0.4142 * dx.min(dy)) * if following { 0.4 } else { 0.8 }
    };
    let via_cost = 3.0f32; // a via costs as much as 3 mm of track

    let mut cost = vec![f32::INFINITY; n_states];
    let mut parent = vec![u32::MAX; n_states];
    let mut heap = BinaryHeap::new();
    for e in &starts {
        let s = enc(e.l, e.i, e.j, NO_DIR);
        let c = e.cost;
        if c < cost[s as usize] {
            cost[s as usize] = c;
            heap.push(Node { f: c + h(e.i, e.j), s });
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
            let mut step = if diagonal { g * std::f32::consts::SQRT_2 } else { g };
            if following && grid.near[l][nj * nx + ni] {
                step *= 0.5; // running next to the partner line is cheap
            }
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
        // (a via goes through every layer, so the spot must be free on all of them)
        let idx = j * nx + i;
        if !grid.no_via[idx] && (0..nl).all(|k| passable(grid.via[k][idx], net)) {
            for o in (0..nl).filter(|&o| o != l) {
                if !passable(grid.trk[o][idx], net) {
                    continue;
                }
                let ns = enc(o, i, j, NO_DIR);
                let nc = c0 + via_cost;
                if nc < cost[ns as usize] {
                    cost[ns as usize] = nc;
                    parent[ns as usize] = s;
                    heap.push(Node { f: nc + h(i, j), s: ns });
                }
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

    // the leads out of and into the pads (where the search used them)
    let (first, last_state) = (dec(states[0]), dec(*states.last().unwrap()));
    let start_lead = starts.iter().find(|e| (e.l, e.i, e.j) == (first.0, first.1, first.2)).and_then(|e| e.lead);
    let end_lead = goals.iter().find(|e| (e.l, e.i, e.j) == (last_state.0, last_state.1, last_state.2)).and_then(|e| e.lead);
    // straight runs become one track each; a layer change becomes a via
    let mut tracks = vec![];
    let mut vias = vec![];
    let mut emit = |l: usize, a: Point, b: Point| {
        if a.dist(b) > 1e-9 {
            tracks.push(Track { side: grid.side(l), a, b, width: opt.track_width });
        }
    };
    match start_lead {
        Some((foot, q)) => {
            emit(cells[0].0, foot, q);
            emit(cells[0].0, q, cells[0].1);
        }
        // no lead (no room for one): still only horizontal, vertical and 45° pieces when leads were asked for
        None => {
            let pts = if opt.pad_lead.is_some() { crate::padentry::bend45(pad_a.pos, cells[0].1, false) } else { vec![cells[0].1] };
            let mut prev = pad_a.pos;
            for p in pts {
                emit(cells[0].0, prev, p);
                prev = p;
            }
        }
    }
    let mut run_from = cells[0].1;
    let mut run_dir: Option<usize> = None;
    for k in 1..cells.len() {
        let (l, p, d) = cells[k];
        if l != cells[k - 1].0 {
            emit(cells[k - 1].0, run_from, cells[k - 1].1);
            if !vias.iter().any(|v: &Via| v.pos == p) {
                vias.push(Via { pos: p, diameter: opt.via_diameter, drill: opt.via_drill });
            }
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
    match end_lead {
        Some((foot, q)) => {
            emit(last.0, last.1, q);
            emit(last.0, q, foot);
        }
        None => {
            let pts = if opt.pad_lead.is_some() { crate::padentry::bend45(pad_b.pos, last.1, false) } else { vec![last.1] };
            let mut prev = pad_b.pos;
            for p in pts {
                emit(last.0, prev, p);
                prev = p;
            }
        }
    }
    Some((tracks, vias))
}

/// What [`Router::path`] found.
#[derive(Clone, Debug, Default)]
pub struct PathResult {
    pub tracks: Vec<Track>,
    pub vias: Vec<Via>,
}

/// The obstacle grid of a board, kept so that many routes between two points can be searched one after the other
/// quickly (the live route from the start of a track to the cursor): the grid is built once for the copper as it
/// is, and a search only looks at the box around the two points.
pub struct Router {
    grid: Grid,
    nets: Vec<String>,
    opt: RouteOptions,
}

/// The most cells (per layer) a search may look at: a longer route is not searched.
const MAX_WINDOW_CELLS: usize = 640_000;

impl Router {
    /// `None` when the board has no outline or is too large for a grid.
    pub fn new(board: &Board, opt: &RouteOptions) -> Option<Router> {
        let mut nets: Vec<String> = vec![];
        let grid = build_grid(board, opt, &mut nets)?;
        Some(Router { grid, nets, opt: opt.clone() })
    }

    /// The route from `a` to `b` on layer `side` for net `net` (`None`: a net of its own, which keeps off all other
    /// copper). It runs between the two points on the grid, with the straight links from the points to the grid at the
    /// ends. `vias`: it may change layer. Only the box around the two points, grown by `margin` mm, is searched, and
    /// the search gives up after `budget`. `None` when nothing was found.
    pub fn path(&self, net: Option<&str>, a: Point, b: Point, side: Side, vias: bool, margin: f64, budget: std::time::Duration) -> Option<PathResult> {
        let grid = &self.grid;
        let net_id = net.and_then(|n| self.nets.iter().position(|x| x == n)).map_or(0, |k| k as i32 + 1);
        let nl = grid.layers.len();
        let l0 = grid.layer(side);
        let (ca, cb) = (grid_cell(grid, a), grid_cell(grid, b));
        let m = (margin / grid.g).ceil() as usize;
        let (i0, i1) = (ca.0.min(cb.0).saturating_sub(m), (ca.0.max(cb.0) + m).min(grid.nx - 1));
        let (j0, j1) = (ca.1.min(cb.1).saturating_sub(m), (ca.1.max(cb.1) + m).min(grid.ny - 1));
        let (ww, wh) = (i1 - i0 + 1, j1 - j0 + 1);
        if ww * wh > MAX_WINDOW_CELLS {
            return None;
        }
        // a cell of the window: (layer, i, j) -> index
        let cell = |l: usize, i: usize, j: usize| (l * wh + (j - j0)) * ww + (i - i0);
        let cells_near = |p: Point, l: usize| -> Vec<(usize, usize)> {
            let c = grid_cell(grid, p);
            let mut out = vec![];
            for j in c.1.saturating_sub(1)..=(c.1 + 1).min(grid.ny - 1) {
                for i in c.0.saturating_sub(1)..=(c.0 + 1).min(grid.nx - 1) {
                    if i >= i0 && i <= i1 && j >= j0 && j <= j1 && passable(grid.trk[l][j * grid.nx + i], net_id) {
                        out.push((i, j));
                    }
                }
            }
            out
        };
        let starts = cells_near(a, l0);
        let goals = cells_near(b, l0);
        if starts.is_empty() || goals.is_empty() {
            return None;
        }
        let n_states = nl * ww * wh * 9;
        let enc = |l: usize, i: usize, j: usize, d: usize| (cell(l, i, j) * 9 + d) as u32;
        let dec = |s: u32| {
            let s = s as usize;
            let (c, d) = (s / 9, s % 9);
            (c / (ww * wh), i0 + c % ww, j0 + (c / ww) % wh, d)
        };
        let mut is_goal = vec![false; nl * ww * wh];
        for &(i, j) in &goals {
            is_goal[cell(l0, i, j)] = true;
        }
        let g = grid.g as f32;
        let h = |i: usize, j: usize| {
            let (dx, dy) = ((i as f32 - cb.0 as f32).abs(), (j as f32 - cb.1 as f32).abs());
            g * (dx.max(dy) + 0.4142 * dx.min(dy)) * 0.8
        };
        let via_cost = 3.0f32;
        let mut cost = vec![f32::INFINITY; n_states];
        let mut parent = vec![u32::MAX; n_states];
        let mut heap = BinaryHeap::new();
        for &(i, j) in &starts {
            let s = enc(l0, i, j, NO_DIR);
            let c = grid.pos(i, j).dist(a) as f32;
            if c < cost[s as usize] {
                cost[s as usize] = c;
                heap.push(Node { f: c + h(i, j), s });
            }
        }
        let started = std::time::Instant::now();
        let mut end = None;
        let mut pops = 0usize;
        while let Some(Node { s, .. }) = heap.pop() {
            pops += 1;
            if pops % 1024 == 0 && started.elapsed() > budget {
                return None;
            }
            let (l, i, j, d) = dec(s);
            let c0 = cost[s as usize];
            if is_goal[cell(l, i, j)] {
                end = Some(s);
                break;
            }
            for (nd, (dx, dy)) in DIRS.iter().enumerate() {
                let (ni, nj) = (i as i32 + dx, j as i32 + dy);
                if ni < i0 as i32 || nj < j0 as i32 || ni > i1 as i32 || nj > j1 as i32 {
                    continue;
                }
                let (ni, nj) = (ni as usize, nj as usize);
                if !passable(grid.trk[l][nj * grid.nx + ni], net_id) {
                    continue;
                }
                let diagonal = *dx != 0 && *dy != 0;
                if diagonal && !(passable(grid.trk[l][j * grid.nx + ni], net_id) && passable(grid.trk[l][nj * grid.nx + i], net_id)) {
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
            let idx = j * grid.nx + i;
            if vias && !grid.no_via[idx] && (0..nl).all(|k| passable(grid.via[k][idx], net_id)) {
                for o in (0..nl).filter(|&o| o != l) {
                    if !passable(grid.trk[o][idx], net_id) {
                        continue;
                    }
                    let ns = enc(o, i, j, NO_DIR);
                    let nc = c0 + via_cost;
                    if nc < cost[ns as usize] {
                        cost[ns as usize] = nc;
                        parent[ns as usize] = s;
                        heap.push(Node { f: nc + h(i, j), s: ns });
                    }
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
        let mut tracks = vec![];
        let mut via_list: Vec<Via> = vec![];
        let width = self.opt.track_width;
        let mut emit = |l: usize, p: Point, q: Point| {
            if p.dist(q) > 1e-9 {
                tracks.push(Track { side: grid.side(l), a: p, b: q, width });
            }
        };
        emit(cells[0].0, a, cells[0].1);
        let mut run_from = cells[0].1;
        let mut run_dir: Option<usize> = None;
        for k in 1..cells.len() {
            let (l, p, d) = cells[k];
            if l != cells[k - 1].0 {
                emit(cells[k - 1].0, run_from, cells[k - 1].1);
                if !via_list.iter().any(|v: &Via| v.pos == p) {
                    via_list.push(Via { pos: p, diameter: self.opt.via_diameter, drill: self.opt.via_drill });
                }
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
        emit(last.0, last.1, b);
        Some(PathResult { tracks, vias: via_list })
    }
}

fn grid_cell(grid: &Grid, p: Point) -> (usize, usize) {
    let i = (((p.x - grid.x0) / grid.g).round().max(0.0) as usize).min(grid.nx - 1);
    let j = (((p.y - grid.y0) / grid.g).round().max(0.0) as usize).min(grid.ny - 1);
    (i, j)
}

/// Route a differential pair: the "+" net first, then the "-" net, which prefers to run beside the first at the
/// pair's gap. Every route is still checked with the DRC (see [`route_airwire`]); the pair rules are checked
/// afterwards by [`crate::diffpair::check_pairs`].
pub fn route_pair(board: &mut Board, pair: &crate::diffpair::DiffPair, opt: &RouteOptions) -> RouteReport {
    let mut first = opt.clone();
    first.follow = None;
    let mut report = autoroute(board, &first, Some(&pair.p));
    let mut second = opt.clone();
    second.follow = Some((pair.p.clone(), opt.track_width + pair.gap));
    let r2 = autoroute(board, &second, Some(&pair.n));
    report.routed += r2.routed;
    report.failed.extend(r2.failed);
    report
}
