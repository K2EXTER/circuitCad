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

use crate::drc::{connectivity, point_rect, point_seg, shape_geo, violations_of_added, Geo};
use crate::geom::inside_polygon;
use crate::model::*;
use crate::ratsnest::Airwire;
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
    /// Keep this far from other copper (when more than the board's rule): the clearance of the net class of the net being routed.
    /// What the net classes of the copper that is already there ask for is always kept.
    pub clearance: Option<f64>,
    /// Keep the tracks off the inner layers of a board with more than two (they are planes there; vias still cross them).
    pub outer_only: bool,
    /// The layers the tracks may use (None: all). The others are shut like the inner layers of `outer_only`; vias still cross them.
    pub only_layers: Option<Vec<Side>>,
    /// Put no via at all: the route stays on the layer of its pads (for a sensitive line that must not change layer).
    pub no_vias: bool,
    /// What a via costs the search, in mm of track (a higher value gives routes with fewer vias).
    pub via_cost: f64,
    /// What a bend costs the search, as a multiple of the grid pitch for each 45 degrees turned (a higher value gives straighter lines).
    pub bend_cost: f64,
    /// A track on a layer that has a pour cuts the pour: its length counts this many times (1.0: no difference). Keeps wiring
    /// off the layer of the ground pour where the other layer has room.
    pub pour_layer_cost: f64,
    /// When connections stay open, try them again on a pitch of half, then a quarter of `grid` (down to 0.1 mm), in `autoroute_sized`.
    pub grid_ladder: bool,
    /// Before a route of the whole board, a short wire is drawn out of the pins of the fine-pitch ICs (see `breakout`).
    pub breakout: bool,
    /// Nets this run leaves alone (a sensing line that was routed on its own terms is not routed again with vias).
    pub skip: Vec<String>,
}

impl Default for RouteOptions {
    fn default() -> Self {
        Self { track_width: 0.25, via_diameter: 0.6, via_drill: 0.3, grid: 0.2, follow: None, pad_lead: None, clearance: None, outer_only: false, only_layers: None, no_vias: false, via_cost: 3.0, bend_cost: 0.4, pour_layer_cost: 1.2, grid_ladder: true, breakout: true, skip: vec![] }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RouteReport {
    pub routed: usize,
    /// Connections that could not be routed.
    pub failed: Vec<Airwire>,
    /// Nets routed with a width of their own because of the current they carry: (net, width in mm). See `autoroute_sized`.
    pub wide: Vec<(String, f64)>,
}

/// Route every airwire of the board (or only those of `only_net`).
///
/// The order the connections are made in decides what room is left for the later ones, so when a run leaves
/// connections open it is made again: the nets that failed are routed first, up to three times, and with a pad lead
/// also once with the leads straight out of the pad's centre instead of slid onto the grid lines. The run that
/// connects the most wins (the first, when equal). No new run is started after [`RETRY_BUDGET`].
pub fn autoroute(board: &mut Board, opt: &RouteOptions, only_net: Option<&str>) -> RouteReport {
    let start = std::time::Instant::now();
    let modes: &[bool] = if opt.pad_lead.is_some() { &[true, false] } else { &[true] };
    let mut best: Option<(Board, RouteReport)> = None;
    let mut first: Vec<String> = vec![];
    'search: for &slide in modes {
        for _ in 0..3 {
            let used = first.clone();
            let mut trial = board.clone();
            let report = autoroute_mode(&mut trial, opt, only_net, slide, &used);
            let done = report.failed.is_empty();
            for w in &report.failed {
                if !first.contains(&w.net) {
                    first.push(w.net.clone());
                }
            }
            if best.as_ref().is_none_or(|(_, b)| report.failed.len() < b.failed.len()) {
                best = Some((trial, report));
            }
            if done || start.elapsed() > RETRY_BUDGET {
                break 'search;
            }
            // no net failed that was not put first already: the next run would go in the same order and end the same, so it is not made
            if first == used {
                break;
            }
        }
    }
    let (routed, report) = best.expect("at least one run");
    *board = routed;
    report
}

/// [`autoroute`] that gives every net with a declared current (`Board::net_currents`) the width that current needs
/// (`current::width_for_current`, rounded up to 0.05 mm), when that is wider than `opt.track_width`. Those nets are
/// routed first, each with its own width, and the rest after them. A wide net that fails is tried again with the
/// normal width by the last run (the width check then warns about it). When the wide tracks leave more connections open
/// than a plain run does, the plain run is kept. With `only_net`, only that net is routed.
pub fn autoroute_sized(board: &mut Board, opt: &RouteOptions, only_net: Option<&str>) -> RouteReport {
    let plain = RouteOptions { breakout: false, ..opt.clone() };
    if !opt.breakout || only_net.is_some() {
        return route_sized(board, &plain, only_net);
    }
    // a route of the whole board: when connections stay open, it is made again from a short wire out of every pin of the fine-pitch ICs
    // (`breakout`), and the one with fewer open connections is kept (the first when equal: the wires out of the pins make a route longer)
    let before = board.clone();
    let report = route_sized(board, &plain, None);
    if report.failed.is_empty() {
        return report;
    }
    let mut second = before;
    if crate::breakout::breakout(&mut second, opt.track_width).stubs == 0 {
        return report;
    }
    let again = route_sized(&mut second, &plain, None);
    if again.failed.len() < report.failed.len() {
        *board = second;
        return again;
    }
    report
}

fn route_sized(board: &mut Board, opt: &RouteOptions, only_net: Option<&str>) -> RouteReport {
    let mut report = route_pass(board, opt, only_net);
    // what is still open is tried on a finer grid (half, then a quarter of the pitch, down to 0.1 mm: tracks go between the legs of a fine-pitch IC)
    let started = std::time::Instant::now();
    let mut grid = opt.grid;
    while !report.failed.is_empty() && grid / 2.0 >= MIN_LADDER_GRID - 1e-9 && opt.grid_ladder && started.elapsed() < LADDER_BUDGET {
        grid /= 2.0;
        let finer = route_pass(board, &RouteOptions { grid, ..opt.clone() }, only_net);
        report.routed += finer.routed;
        report.failed = finer.failed;
        report.wide.extend(finer.wide);
    }
    report
}

/// One pass of `autoroute_sized` on one grid: the nets with a role, a width of their own or the like first, in the order of their roles,
/// then the rest.
fn route_pass(board: &mut Board, opt: &RouteOptions, only_net: Option<&str>) -> RouteReport {
    let mut report = RouteReport::default();
    // the nets that are routed on their own terms: a width from the current, the width / via / clearance of the net class, or a role that
    // asks for something of the route; first, in the order of their roles (a big current, switching, sensing, analog, logic)
    let mut special: Vec<(u8, String, RouteOptions)> = vec![];
    let mut names: Vec<String> = board.net_currents.keys().cloned().collect();
    names.extend(board.net_classes.iter().flat_map(|c| c.nets.iter().cloned()));
    names.sort();
    names.dedup();
    for n in names {
        if !only_net.is_none_or(|x| x == n) {
            continue;
        }
        let mut o = opt.clone();
        let (mut order, mut asks) = (Role::Power.priority(), false);
        if let Some(c) = board.class_of(&n) {
            (o.track_width, o.via_diameter, o.via_drill) = (c.track_width, c.via_diameter, c.via_drill);
            o.clearance = (c.clearance > board.rules.min_clearance + 1e-9).then_some(c.clearance);
            (order, asks) = (c.route_order(), c.is_special());
            // no vias when the table says so (a sensing line says so by its role: it stays on the layer of its pads)
            o.no_vias |= c.forbids_vias();
            // only the layers the table allows (the router shuts the others)
            if !c.layers.is_empty() {
                o.only_layers = Some(c.layers.clone());
            }
        }
        if let Some(amps) = board.net_currents.get(&n) {
            let need = crate::current::round_up_width(crate::current::width_for_current(*amps, &Default::default()));
            o.track_width = o.track_width.max(need);
        }
        let differs = (o.track_width - opt.track_width).abs() > 1e-9 || (o.via_diameter - opt.via_diameter).abs() > 1e-9 || (o.via_drill - opt.via_drill).abs() > 1e-9 || o.clearance.is_some() || asks;
        if differs {
            // a wide net of no role goes with the big currents (`route_order`)
            special.push((order, n, o));
        }
    }
    special.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    // the plain run, to fall back on when the wide tracks take room that other connections needed (not when a role asks for something
    // the plain run would not keep to: a sensing line with no via)
    let hard = special.iter().any(|(_, _, o)| (o.no_vias && !opt.no_vias) || (o.only_layers.is_some() && opt.only_layers.is_none()));
    let plain = (!special.is_empty() && !hard).then(|| {
        let mut b = board.clone();
        let r = autoroute(&mut b, opt, only_net);
        (b, r)
    });
    let mut hard_failed: Vec<Airwire> = vec![];
    for (_, n, o) in &special {
        let r = autoroute(board, o, Some(n));
        report.routed += r.routed;
        if (o.no_vias && !opt.no_vias) || (o.only_layers.is_some() && opt.only_layers.is_none()) {
            hard_failed.extend(r.failed.iter().cloned());
        }
        if r.routed > 0 && o.track_width > opt.track_width + 1e-9 {
            report.wide.push((n.clone(), o.track_width));
        }
    }
    // (the sensing lines were routed with their own options above; the plain run must not route them again, with vias)
    let leave: Vec<String> = special.iter().filter(|(_, _, o)| (o.no_vias && !opt.no_vias) || (o.only_layers.is_some() && opt.only_layers.is_none())).map(|(_, n, _)| n.clone()).collect();
    let rest = autoroute(board, &RouteOptions { skip: leave, ..opt.clone() }, only_net);
    report.routed += rest.routed;
    report.failed = rest.failed;
    report.failed.extend(hard_failed);
    // wide tracks that cost connections: the plain run wins when it connects more
    if let Some((plain_board, plain_report)) = plain
        && plain_report.failed.len() < report.failed.len()
    {
        *board = plain_board;
        report = plain_report;
    }
    report
}

/// The finest pitch (mm) the grid ladder goes down to (finer ones leave copper a hair closer than the rules allow after the pads' leads are made).
const MIN_LADDER_GRID: f64 = 0.1;
/// No finer pitch is started after this long (a run on a fine grid of a big board takes many seconds).
const LADDER_BUDGET: std::time::Duration = std::time::Duration::from_secs(25);

/// No further routing run is started once this much time has passed (a run on a big board takes several seconds).
const RETRY_BUDGET: std::time::Duration = std::time::Duration::from_secs(12);

/// One run; the nets in `first` are routed before the others (each group shortest connection first).
fn autoroute_mode(board: &mut Board, opt: &RouteOptions, only_net: Option<&str>, slide: bool, first: &[String]) -> RouteReport {
    let mut report = RouteReport::default();
    let mut failed: Vec<Airwire> = vec![];
    // the wires between two parts that the requirements give a width of their own: made first, each with its width (the rest of the net
    // is joined to them afterwards)
    for l in board.link_widths.clone().iter().filter(|l| only_net.is_none_or(|n| n == l.net) && !opt.skip.contains(&l.net)) {
        let Some(w) = link_airwire(board, l) else { continue };
        // (a width that does not fit - a thick wire into the pad of a small IC - falls back to the net's own width: the connection is made)
        let made = route_airwire_mode(board, &w, &RouteOptions { track_width: l.width, ..opt.clone() }, slide) || (l.width > opt.track_width + 1e-9 && route_airwire_mode(board, &w, opt, slide));
        if made {
            report.routed += 1;
        } else {
            failed.push(w);
        }
    }
    loop {
        // pad to pad: a via the user placed on a net is no end of a connection (the route starts and ends on pads)
        let mut wires: Vec<Airwire> = pad_ratsnest(board)
            .into_iter()
            .filter(|w| only_net.is_none_or(|n| w.net == n) && !opt.skip.contains(&w.net))
            .filter(|w| !failed.contains(w))
            .collect();
        // the nets that failed before first, then short connections first: they are the easiest and leave the long ones
        // room to detour
        wires.sort_by(|a, b| (!first.contains(&a.net)).cmp(&!first.contains(&b.net)).then(a.a.dist(a.b).total_cmp(&b.a.dist(b.b))));
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

/// The connection a width for the wire between two parts asks for: the nearest two pads of the net, one on each part. `None` when the
/// two parts are joined on that net already (or have no pad on it).
fn link_airwire(board: &Board, l: &LinkWidth) -> Option<Airwire> {
    let conn = connectivity(board);
    let on = |r: &str| board.pads.iter().enumerate().filter(move |(_, p)| p.refdes == r && p.net == l.net).map(|(i, _)| i).collect::<Vec<_>>();
    let mut best: Option<(f64, usize, usize)> = None;
    for &i in &on(&l.a) {
        for &j in &on(&l.b) {
            if conn.pad_group(i) == conn.pad_group(j) {
                return None;
            }
            let d = board.pads[i].pos.dist(board.pads[j].pos);
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, i, j));
            }
        }
    }
    best.map(|(_, i, j)| Airwire { net: l.net.clone(), a: board.pads[i].pos, b: board.pads[j].pos })
}

/// The open connections between pads: [`ratsnest`] without the vias as ends (a net's vias that no track leads to would otherwise be
/// the nearest "end", and the connection could not be routed from a via).
fn pad_ratsnest(board: &Board) -> Vec<Airwire> {
    let conn = connectivity(board);
    let groups: Vec<usize> = (0..board.pads.len()).map(|i| conn.pad_group(i)).collect();
    crate::ratsnest::ratsnest_in(board, &groups)
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
    // (options, slide the lead's foot, tidy: horizontal, vertical and 45° pieces where there is no lead). With a lead
    // asked for, the last resort is no lead at all, which first still keeps to the tidy pieces and then to a free angle.
    let attempts: &[(&RouteOptions, bool, bool)] = match (opt.pad_lead.is_some(), slide) {
        (true, true) => &[(opt, true, true), (opt, false, true), (&plain, false, true), (&plain, false, false)],
        (true, false) => &[(opt, false, true), (&plain, false, true), (&plain, false, false)],
        _ => &[(opt, false, false)],
    };
    let Some((&(o, slide, tidy), rest)) = attempts.split_first() else { return false };
    if route_airwire_with(board, w, o, slide, tidy) {
        return true;
    }
    // the others search the same board on their own, so they are searched side by side (a connection that cannot be made costs one search,
    // not one for each); the first of them, in the order above, whose route is good is taken, as when they were tried one after the other
    let found = crate::par::par_map(rest, |&(o, slide, tidy)| find_route(board, w, o, slide, tidy));
    found.into_iter().flatten().any(|(tracks, vias)| add_route(board, w, tracks, vias))
}

fn route_airwire_with(board: &mut Board, w: &Airwire, opt: &RouteOptions, slide: bool, tidy: bool) -> bool {
    let Some((tracks, vias)) = find_route(board, w, opt, slide, tidy) else { return false };
    add_route(board, w, tracks, vias)
}

/// Put a found route on the board when it breaks no rule and joins the two pads (otherwise the board is left as it is).
fn add_route(board: &mut Board, w: &Airwire, tracks: Vec<Track>, vias: Vec<Via>) -> bool {
    let (tracks_before, vias_before) = (board.tracks.len(), board.vias.len());
    let mut trial = board.clone();
    trial.tracks.extend(tracks);
    trial.vias.extend(vias);
    // one pass for the groups of copper; only what the new copper breaks is looked for (not the whole board again)
    let conn = connectivity(&trial);
    if violations_of_added(&trial, tracks_before, vias_before, &conn) > 0 {
        return false;
    }
    let pad_at = |p: Point| trial.pads.iter().position(|q| q.pos == p && q.net == w.net);
    let (Some(ia), Some(ib)) = (pad_at(w.a), pad_at(w.b)) else { return false };
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
    /// Per layer: what a step on it costs, as a multiple (more on a layer that carries a pour).
    layer_cost: Vec<f32>,
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
    let layer_cost: Vec<f32> = layers.iter().map(|l| if board.zones.iter().any(|z| !z.net.is_empty() && z.side == *l) { opt.pour_layer_cost as f32 } else { 1.0 }).collect();
    let mut grid = Grid { x0: lo.x, y0: lo.y, g, nx, ny, trk: vec![vec![0; nx * ny]; nl], via: vec![vec![0; nx * ny]; nl], layers, near: vec![], layer_cost, no_via: vec![false; nx * ny] };

    let r = &board.rules;
    // A path sampled on the grid can pass closer to a corner than its cells do, so inflate a little more.
    let widen = |need: f64| (need * need + g * g / 2.0).sqrt();
    // what an obstacle of net `n` keeps a new track / via away: the board's rule, or the clearance of its net class, or of the class
    // of the net being routed, whichever is the most
    let own_clear = opt.clearance.unwrap_or(0.0);
    let need_for = |n: Option<&str>| {
        let c = board.clearance_between(n, None).max(own_clear);
        (widen(c + opt.track_width / 2.0 + 0.005), widen(c + opt.via_diameter / 2.0 + 0.005))
    };
    let (t_need, v_need) = need_for(None);
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
        let (tn, vn) = need_for(conn.track_net(k));
        grid.mark(grid.layer(t.side), &geo, net, tn, vn);
    }
    for (k, p) in board.pads.iter().enumerate() {
        let net = id_of(conn.pad_net(k).or(if p.net.is_empty() { None } else { Some(p.net.as_str()) }));
        let geo = shape_geo(p.pos, p.shape);
        let (tn, vn) = need_for(conn.pad_net(k).or(if p.net.is_empty() { None } else { Some(p.net.as_str()) }));
        for l in pad_layers(p, nl) {
            grid.mark(l, &geo, net, tn, vn);
        }
        grid.mark_no_via(&geo, opt.via_diameter / 2.0);
    }
    for (k, v) in board.vias.iter().enumerate() {
        let net = id_of(conn.via_net(k));
        let geo = Geo::Cap { a: v.pos, b: v.pos, r: v.diameter / 2.0 };
        let (tn, vn) = need_for(conn.via_net(k));
        for l in 0..nl {
            grid.mark(l, &geo, net, tn, vn);
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
    if opt.no_vias {
        grid.no_via.iter_mut().for_each(|c| *c = true);
    }
    if opt.outer_only {
        for l in 1..nl.saturating_sub(1) {
            grid.trk[l].iter_mut().for_each(|c| *c = -1);
        }
    }
    if let Some(allowed) = &opt.only_layers {
        for l in 0..nl {
            if !allowed.contains(&grid.layers[l]) {
                grid.trk[l].iter_mut().for_each(|c| *c = -1);
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

/// The pieces from the pad to the grid cell `cell` when there is no lead: with only a little sideways offset, the foot
/// of one straight piece is slid along the pad (while the track's centre stays on it); otherwise a horizontal, vertical or
/// 45° pair of pieces from the pad's centre. A cell inside the pad needs none. Returns where the first piece starts and
/// the points it then goes through.
fn tidy_entry(pad: &Pad, cell: Point) -> (Point, Vec<Point>) {
    if on_pad(pad, cell) {
        return (cell, vec![]);
    }
    let (w, h) = pad.shape.size();
    let (dx, dy) = (cell.x - pad.pos.x, cell.y - pad.pos.y);
    let vertical = dy.abs() >= dx.abs();
    let (lat, half) = if vertical { (dx, w / 2.0) } else { (dy, h / 2.0) };
    let room = if matches!(pad.shape, PadShape::Circle { .. }) { w / 2.0 - 0.05 } else { half - 0.05 };
    if lat.abs() <= room {
        let foot = if vertical { Point::new(cell.x, pad.pos.y) } else { Point::new(pad.pos.x, cell.y) };
        // the foot must be on the pad (a corner of a round pad is not)
        if on_pad(pad, foot) {
            return (foot, vec![cell]);
        }
    }
    (pad.pos, crate::padentry::bend45(pad.pos, cell, false))
}

/// Is `p` within the copper of `pad`?
fn on_pad(pad: &Pad, p: Point) -> bool {
    let (w, h) = pad.shape.size();
    match pad.shape {
        PadShape::Circle { d } => p.dist(pad.pos) <= d / 2.0,
        _ => (p.x - pad.pos.x).abs() <= w / 2.0 && (p.y - pad.pos.y).abs() <= h / 2.0,
    }
}

thread_local! {
    /// The cost and parent tables of the search, kept between searches: they are as big as the board's grid, and a search
    /// touches only a part of them, so they are put back as they were instead of being made again.
    static SCRATCH: std::cell::RefCell<(Vec<f32>, Vec<u32>)> = const { std::cell::RefCell::new((Vec::new(), Vec::new())) };
}

/// The tables of one search; they go back clean (and to the next search) when this is dropped.
struct Scratch {
    cost: Vec<f32>,
    parent: Vec<u32>,
    /// The states whose entries were set.
    touched: Vec<u32>,
}

impl Scratch {
    fn take(n_states: usize) -> Scratch {
        let (mut cost, mut parent) = SCRATCH.with(|c| std::mem::take(&mut *c.borrow_mut()));
        if cost.len() != n_states {
            cost = vec![f32::INFINITY; n_states];
            parent = vec![u32::MAX; n_states];
        }
        Scratch { cost, parent, touched: vec![] }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        for &s in &self.touched {
            self.cost[s as usize] = f32::INFINITY;
            self.parent[s as usize] = u32::MAX;
        }
        let tables = (std::mem::take(&mut self.cost), std::mem::take(&mut self.parent));
        SCRATCH.with(|c| *c.borrow_mut() = tables);
    }
}

/// Can any cell of `is_goal` be reached from the `starts` with the moves of the search (eight ways round a free cell,
/// never cutting a corner between blocked cells; a via to the other layers where the spot is free on all of them)?
fn reaches_goal(grid: &Grid, net: i32, starts: &[EndCell], is_goal: &[bool]) -> bool {
    let (nx, ny) = (grid.nx, grid.ny);
    let nl = grid.layers.len();
    let mut seen = vec![false; nl * nx * ny];
    let mut stack: Vec<usize> = vec![];
    for e in starts {
        let c = (e.l * ny + e.j) * nx + e.i;
        if !seen[c] {
            seen[c] = true;
            stack.push(c);
        }
    }
    while let Some(c) = stack.pop() {
        if is_goal[c] {
            return true;
        }
        let (l, rest) = (c / (nx * ny), c % (nx * ny));
        let (i, j) = (rest % nx, rest / nx);
        for (dx, dy) in DIRS.iter() {
            let (ni, nj) = (i as i32 + dx, j as i32 + dy);
            if ni < 0 || nj < 0 || ni >= nx as i32 || nj >= ny as i32 {
                continue;
            }
            let (ni, nj) = (ni as usize, nj as usize);
            if !passable(grid.trk[l][nj * nx + ni], net) {
                continue;
            }
            if *dx != 0 && *dy != 0 && !(passable(grid.trk[l][j * nx + ni], net) && passable(grid.trk[l][nj * nx + i], net)) {
                continue;
            }
            let n = (l * ny + nj) * nx + ni;
            if !seen[n] {
                seen[n] = true;
                stack.push(n);
            }
        }
        let idx = j * nx + i;
        if !grid.no_via[idx] && (0..nl).all(|k| passable(grid.via[k][idx], net)) {
            for o in (0..nl).filter(|&o| o != l) {
                if !passable(grid.trk[o][idx], net) {
                    continue;
                }
                let n = (o * ny + j) * nx + i;
                if !seen[n] {
                    seen[n] = true;
                    stack.push(n);
                }
            }
        }
    }
    false
}

fn find_route(board: &Board, w: &Airwire, opt: &RouteOptions, slide: bool, tidy: bool) -> Option<(Vec<Track>, Vec<Via>)> {
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
    // when no goal can be reached at all, the search would only walk through everything that can be reached and find
    // nothing: a plain fill over the cells (the same moves, no costs) says so at once
    if !reaches_goal(&grid, net, &starts, &is_goal) {
        return None;
    }
    let gc = grid_cell(&grid, pad_b.pos);
    let following = !grid.near.is_empty();
    let g = grid.g as f32;
    let h = |i: usize, j: usize| {
        let (dx, dy) = ((i as f32 - gc.0 as f32).abs(), (j as f32 - gc.1 as f32).abs());
        g * (dx.max(dy) + 0.4142 * dx.min(dy)) * if following { 0.4 } else { 0.8 }
    };
    let via_cost = opt.via_cost as f32;

    let mut sc = Scratch::take(n_states);
    let mut heap = BinaryHeap::new();
    for e in &starts {
        let s = enc(e.l, e.i, e.j, NO_DIR);
        let c = e.cost;
        if c < sc.cost[s as usize] {
            sc.cost[s as usize] = c;
        sc.touched.push(s);
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
        let c0 = sc.cost[s as usize];
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
            let mut step = (if diagonal { g * std::f32::consts::SQRT_2 } else { g }) * grid.layer_cost[l];
            if following && grid.near[l][nj * nx + ni] {
                step *= 0.5; // running next to the partner line is cheap
            }
            let turn = if d == NO_DIR {
                0.0
            } else {
                let diff = (nd as i32 - d as i32).rem_euclid(8);
                g * opt.bend_cost as f32 * diff.min(8 - diff) as f32
            };
            let ns = enc(l, ni, nj, nd);
            let nc = c0 + step + turn;
            if nc < sc.cost[ns as usize] {
                sc.cost[ns as usize] = nc;
        sc.touched.push(ns);
                sc.parent[ns as usize] = s;
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
                if nc < sc.cost[ns as usize] {
                    sc.cost[ns as usize] = nc;
        sc.touched.push(ns);
                    sc.parent[ns as usize] = s;
                    heap.push(Node { f: nc + h(i, j), s: ns });
                }
            }
        }
    }
    let end = end?;

    let mut states = vec![end];
    while sc.parent[*states.last().unwrap() as usize] != u32::MAX {
        states.push(sc.parent[*states.last().unwrap() as usize]);
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
        // (a first cell inside the pad's copper: the track starts there, with no bend out of the pad's centre)
        None => {
            let (mut prev, pts) = if tidy { tidy_entry(pad_a, cells[0].1) } else { (pad_a.pos, vec![cells[0].1]) };
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
                vias.push(Via { pos: p, diameter: opt.via_diameter, drill: opt.via_drill, net: String::new() });
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
            let (mut prev, pts) = if tidy { tidy_entry(pad_b, last.1) } else { (pad_b.pos, vec![last.1]) };
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
    // one search request: the two ends, the layer, whether it may change layer, and how far and how long to look
    #[allow(clippy::too_many_arguments)]
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
        let via_cost = self.opt.via_cost as f32;
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
            if pops.is_multiple_of(1024) && started.elapsed() > budget {
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
                let step = (if diagonal { g * std::f32::consts::SQRT_2 } else { g }) * grid.layer_cost[l];
                let turn = if d == NO_DIR {
                    0.0
                } else {
                    let diff = (nd as i32 - d as i32).rem_euclid(8);
                    g * self.opt.bend_cost as f32 * diff.min(8 - diff) as f32
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
                    via_list.push(Via { pos: p, diameter: self.opt.via_diameter, drill: self.opt.via_drill, net: String::new() });
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
