//! Nudging small parts out of the way of a connection the router could not make.
//!
//! After a full route some connections stay open, usually where a few small parts stand in the way of a fan-out or a
//! channel (an IC with a fine pitch with its capacitors round it). For each open connection the small parts near
//! it are tried at a few places 1-3 mm away: the part's wiring is taken away, the part is moved, the pours get their
//! vias again and the nets of the part and of the connection are routed once more. A move is kept only when fewer
//! connections are open than before and the DRC has found nothing more. Parts of more than three pads and locked parts stay
//! where they are. Meant for a board that is routed in one go: the router does
//! not move a part for a single wire.

use crate::autoroute::{autoroute_sized, RouteOptions};
use crate::drc::check;
use crate::groundpour::{ground_pour, PourOptions};
use crate::model::*;
use crate::ratsnest::{ratsnest, Airwire};
use crate::shove::rip_up_net;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default)]
pub struct NudgeReport {
    /// (part, dx, dy) of every move that was kept.
    pub moved: Vec<(String, f64, f64)>,
    pub open_before: usize,
    pub open_after: usize,
    /// Trials made.
    pub tried: usize,
    /// The time budget ran out before every open connection had its turn.
    pub out_of_time: bool,
}

/// Places to try, nearest first (mm).
const STEPS: [(f64, f64); 24] = [
    (1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0), (2.0, 0.0), (-2.0, 0.0), (0.0, 2.0), (0.0, -2.0),
    (1.5, 1.5), (-1.5, 1.5), (1.5, -1.5), (-1.5, -1.5), (3.0, 0.0), (-3.0, 0.0), (0.0, 3.0), (0.0, -3.0),
    (4.0, 0.0), (-4.0, 0.0), (0.0, 4.0), (0.0, -4.0), (3.0, 3.0), (-3.0, 3.0), (3.0, -3.0), (-3.0, -3.0),
];
/// How many parts near an open connection are tried.
const PARTS_PER_WIRE: usize = 8;

fn dist_to_segment(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l2 = dx * dx + dy * dy;
    let t = if l2 < 1e-12 { 0.0 } else { (((p.x - a.x) * dx + (p.y - a.y) * dy) / l2).clamp(0.0, 1.0) };
    Point::new(a.x + dx * t, a.y + dy * t).dist(p)
}

/// Nets whose copper is a pour on some layer: their pads meet it through vias, which are put again after a move.
fn pour_nets(board: &Board) -> BTreeSet<String> {
    board.zones.iter().map(|z| z.net.clone()).collect()
}

/// Take away the wiring of `refdes`: the whole routing of its signal nets, and for a net that is a pour only the short
/// track and via that join the part's pads to it.
fn rip_up_part(board: &mut Board, refdes: &str) {
    let pours = pour_nets(board);
    let pads: Vec<Pad> = board.pads.iter().filter(|p| p.refdes == refdes).cloned().collect();
    let mut signal: BTreeSet<String> = BTreeSet::new();
    for p in &pads {
        if p.net.is_empty() {
            continue;
        }
        if pours.contains(&p.net) {
            let (w, h) = p.shape.size();
            let (rx, ry) = (w / 2.0 + 0.06, h / 2.0 + 0.06);
            let on_pad = |q: Point| (q.x - p.pos.x).abs() <= rx && (q.y - p.pos.y).abs() <= ry;
            let mut gone_ends: Vec<Point> = vec![];
            board.tracks.retain(|t| {
                if t.side == p.side && (on_pad(t.a) || on_pad(t.b)) {
                    gone_ends.push(if on_pad(t.a) { t.b } else { t.a });
                    false
                } else {
                    true
                }
            });
            // a via at the far end of a removed stub that no other track reaches
            for e in gone_ends {
                let used = board.tracks.iter().any(|t| t.a.dist(e) < 0.01 || t.b.dist(e) < 0.01);
                if !used {
                    board.vias.retain(|v| v.pos.dist(e) >= 0.01);
                }
            }
        } else {
            signal.insert(p.net.clone());
        }
    }
    for n in signal {
        rip_up_net(board, &n);
    }
}

/// Put the part's pads on the pours again after it moved.
fn repour(board: &mut Board, refdes: &str) {
    let mut done: Vec<(String, Side)> = vec![];
    let nets: Vec<String> = board.pads.iter().filter(|p| p.refdes == refdes && !p.net.is_empty()).map(|p| p.net.clone()).collect();
    for n in nets {
        for z in board.zones.clone().iter().filter(|z| z.net == n) {
            if !done.contains(&(n.clone(), z.side)) && !matches!(z.side, Side::Top) {
                done.push((n.clone(), z.side));
                let _ = ground_pour(board, &PourOptions { net: Some(n.clone()), top: false, side: z.side, ..Default::default() });
            }
        }
    }
}

/// One trial: the part moved by (dx, dy), its nets and the net of the connection routed again. `None` when it does not
/// leave the board better.
fn try_move(board: &Board, w: &Airwire, refdes: &str, d: (f64, f64), opt: &RouteOptions, base_open: usize, base_drc: usize) -> Option<Board> {
    let part = board.parts.iter().find(|p| p.refdes == refdes)?;
    let to = Point::new(part.pos.x + d.0, part.pos.y + d.1);
    let mut trial = board.clone();
    rip_up_part(&mut trial, refdes);
    trial.place_part(refdes, to, part.rot);
    if check(&trial).len() > base_drc {
        return None;
    }
    repour(&mut trial, refdes);
    let mut nets: BTreeSet<String> = trial.pads.iter().filter(|p| p.refdes == refdes && !p.net.is_empty()).map(|p| p.net.clone()).collect();
    nets.insert(w.net.clone());
    let pours = pour_nets(&trial);
    for n in nets.iter().filter(|n| !pours.contains(*n)) {
        autoroute_sized(&mut trial, opt, Some(n));
    }
    (ratsnest(&trial).len() < base_open && check(&trial).len() <= base_drc).then_some(trial)
}

pub fn route_with_nudging(board: &mut Board, opt: &RouteOptions, budget: Duration) -> NudgeReport {
    let start = Instant::now();
    let mut report = NudgeReport { open_before: ratsnest(board).len(), ..Default::default() };
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(8);
    let mut skip: BTreeSet<(String, i64, i64)> = BTreeSet::new();
    'outer: loop {
        let open = ratsnest(board);
        let base_drc = check(board).len();
        let mut progressed = false;
        for w in open.iter().filter(|w| !crate::drc::is_ground_net(&w.net)) {
            let key = (w.net.clone(), (w.a.x * 10.0) as i64, (w.a.y * 10.0) as i64);
            if skip.contains(&key) {
                continue;
            }
            if start.elapsed() > budget {
                report.out_of_time = true;
                break 'outer;
            }
            // the small parts near the wire, nearest to an end first
            let mut near: Vec<(f64, String)> = vec![];
            for p in &board.parts {
                if board.is_locked(&p.refdes) {
                    continue;
                }
                let pads: Vec<&Pad> = board.pads.iter().filter(|q| q.refdes == p.refdes).collect();
                if pads.is_empty() || pads.len() > 3 {
                    continue;
                }
                let c = Point::new(pads.iter().map(|q| q.pos.x).sum::<f64>() / pads.len() as f64, pads.iter().map(|q| q.pos.y).sum::<f64>() / pads.len() as f64);
                let d_end = c.dist(w.a).min(c.dist(w.b));
                if d_end <= 8.0 || dist_to_segment(c, w.a, w.b) <= 3.0 {
                    near.push((d_end, p.refdes.clone()));
                }
            }
            near.sort_by(|a, b| a.0.total_cmp(&b.0));
            let trials: Vec<(String, (f64, f64))> = near.iter().take(PARTS_PER_WIRE).flat_map(|(_, r)| STEPS.iter().map(move |s| (r.clone(), *s))).collect();
            let base_open = open.len();
            let snapshot = board.clone();
            let mut found: Option<(String, (f64, f64), Board)> = None;
            for chunk in trials.chunks(threads) {
                if start.elapsed() > budget {
                    report.out_of_time = true;
                    break 'outer;
                }
                let results: Vec<Option<Board>> = std::thread::scope(|sc| {
                    let handles: Vec<_> = chunk.iter().map(|(r, d)| {
                        let (snap, w, opt) = (&snapshot, &*w, opt);
                        sc.spawn(move || try_move(snap, w, r, *d, opt, base_open, base_drc))
                    }).collect();
                    handles.into_iter().map(|h| h.join().unwrap_or(None)).collect()
                });
                report.tried += chunk.len();
                if let Some(k) = results.iter().position(|r| r.is_some()) {
                    let b = results.into_iter().nth(k).flatten().unwrap();
                    found = Some((chunk[k].0.clone(), chunk[k].1, b));
                    break;
                }
            }
            match found {
                Some((r, d, b)) => {
                    *board = b;
                    report.moved.push((r, d.0, d.1));
                    progressed = true;
                    break; // the open list changed: start again from the new one
                }
                None => {
                    skip.insert(key);
                }
            }
        }
        if !progressed {
            break;
        }
    }
    report.open_after = ratsnest(board).len();
    report
}
