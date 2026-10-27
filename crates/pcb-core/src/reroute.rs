//! Make a finished route shorter and use fewer vias: nets that run well beyond their shortest possible length, or that
//! carry vias, are ripped up and routed again, alone or together with a net that stands in their way.
//!
//! The router makes one connection at a time, shortest first, and never moves what it has made, so a long net that
//! came late has to detour round the nets that came before. Here the net with the most to gain goes first: the net and
//! one of its neighbours are taken out, the net is routed first and the neighbour after it. A change is kept only when
//! every net is still connected, the DRC finds no more than before and the cost (length + [`VIA_WEIGHT`] mm per via + [`BEND_WEIGHT`] mm per bend)
//! has gone down; otherwise the board is left as it was.

use crate::autoroute::{autoroute, RouteOptions};
use crate::drc::{check, connectivity};
use crate::model::*;
use crate::ratsnest::ratsnest;
use crate::shove::rip_up_net;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// What a via is worth in millimetres of track when comparing two routes of a net.
pub const VIA_WEIGHT: f64 = 10.0;
/// What a bend of a route is worth in millimetres of track (a staircase of short jogs costs more than a straight run).
pub const BEND_WEIGHT: f64 = 0.7;
/// The search of a route made again counts a via as much as [`VIA_WEIGHT`] and bends dearly, so that what it makes is what the
/// comparison asks for (the router's own, cheaper, defaults are for the first run).
const SEARCH_BEND_COST: f64 = 1.0;
/// A net must be this much (mm) above its shortest length (or carry a via) to be tried.
const MIN_EXCESS: f64 = 2.0;
/// A change must save at least this much (mm of cost) to be kept.
const MIN_GAIN: f64 = 0.3;
/// How many neighbours are tried with one net.
const NEIGHBOURS: usize = 4;
/// How far (mm) round a net's pads a track counts as standing in its way.
const REACH: f64 = 4.0;
/// Most nets tried per run, and the time allowed in all.
const MAX_NETS: usize = 16;
/// Right after a run, nets with more pads than this (a ground or supply tree) are not routed again: it takes
/// long and rarely gains. When asked for by hand they are tried too.
const MAX_PADS: usize = 8;
const BUDGET: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ImproveReport {
    /// Routes made again that were kept.
    pub improved: usize,
    /// Cost saved (mm of track plus [`VIA_WEIGHT`] per via).
    pub saved: f64,
    /// Net change in the number of vias (negative: fewer).
    pub vias: i64,
    /// Net change in track length (mm).
    pub length: f64,
}

fn dist(a: Point, b: Point) -> f64 {
    a.dist(b)
}

/// Length of the shortest tree over the points (straight lines).
fn shortest_tree(pts: &[Point]) -> f64 {
    if pts.len() < 2 {
        return 0.0;
    }
    let mut inside = vec![false; pts.len()];
    inside[0] = true;
    let mut near: Vec<f64> = pts.iter().map(|p| dist(*p, pts[0])).collect();
    let mut total = 0.0;
    for _ in 1..pts.len() {
        let Some((k, d)) = near.iter().enumerate().filter(|(i, _)| !inside[*i]).min_by(|a, b| a.1.total_cmp(b.1)).map(|(i, d)| (i, *d)) else { break };
        inside[k] = true;
        total += d;
        for (i, n) in near.iter_mut().enumerate() {
            if !inside[i] {
                *n = n.min(dist(pts[i], pts[k]));
            }
        }
    }
    total
}

/// Per net: (length of its tracks, number of its vias, number of bends).
fn net_use(board: &Board) -> HashMap<String, (f64, usize, usize)> {
    let conn = connectivity(board);
    let mut out: HashMap<String, (f64, usize, usize)> = HashMap::new();
    for (i, t) in board.tracks.iter().enumerate() {
        if let Some(n) = conn.track_net(i) {
            out.entry(n.to_string()).or_default().0 += t.a.dist(t.b);
        }
    }
    for i in 0..board.vias.len() {
        if let Some(n) = conn.via_net(i) {
            out.entry(n.to_string()).or_default().1 += 1;
        }
    }
    for (n, b) in crate::quality::bends_by_net(board, &conn) {
        out.entry(n).or_default().2 += b;
    }
    out
}

fn cost_of(use_: &HashMap<String, (f64, usize, usize)>, nets: &[&str]) -> f64 {
    nets.iter().map(|n| use_.get(*n).map_or(0.0, |(l, v, b)| l + VIA_WEIGHT * *v as f64 + BEND_WEIGHT * *b as f64)).sum()
}

/// Route `net` (its tracks are gone), and tidy what was added the way the editor does after a run.
fn route_net(board: &mut Board, opt: &RouteOptions, net: &str) {
    let before = board.tracks.len();
    autoroute(board, opt, Some(net));
    if let Some(lead) = opt.pad_lead {
        let added: Vec<usize> = (before..board.tracks.len()).collect();
        crate::padentry::align_entries(board, &crate::padentry::EntryOptions { lead, spread: true }, Some(&added));
    }
    crate::merge::merge_collinear(board, before);
}

/// Try to make the routes of `board` shorter and its vias fewer (see the module). With `from` = `Some(n)` (right after
/// a run that made the tracks numbered `n` and up) only the nets of those tracks are touched, and big trees are skipped;
/// `None` is the whole board, asked for by hand. With `only`,
/// only that net is tried first-hand (its neighbours may still move). Nets that a pour carries are left alone, and so
/// are nets that are not routed completely. Nothing is changed when nothing gets better.
pub fn improve_routes(board: &mut Board, opt: &RouteOptions, from: Option<usize>, only: Option<&str>) -> ImproveReport {
    let start = Instant::now();
    let search = RouteOptions { via_cost: opt.via_cost.max(VIA_WEIGHT), bend_cost: opt.bend_cost.max(SEARCH_BEND_COST), ..opt.clone() };
    let opt = &search;
    let mut report = ImproveReport::default();
    if board.tracks.is_empty() {
        return report;
    }
    let poured: Vec<String> = board.zones.iter().map(|z| z.net.clone()).collect();
    let open: Vec<String> = ratsnest(board).into_iter().map(|w| w.net).collect();
    let big: Vec<String> = {
        let mut count: HashMap<&str, usize> = HashMap::new();
        for p in board.pads.iter().filter(|p| !p.net.is_empty()) {
            *count.entry(p.net.as_str()).or_default() += 1;
        }
        count.into_iter().filter(|(_, n)| from.is_some() && *n > MAX_PADS).map(|(k, _)| k.to_string()).collect()
    };
    let mut tried: Vec<String> = vec![];
    let mut drc = check(board).len();
    for _ in 0..MAX_NETS {
        if start.elapsed() > BUDGET {
            break;
        }
        // the net furthest above its shortest length that has not been tried yet
        let used = net_use(board);
        let fresh: Vec<String> = {
            let conn = connectivity(board);
            let mut v: Vec<String> = (from.unwrap_or(0)..board.tracks.len()).filter_map(|i| conn.track_net(i).map(str::to_string)).collect();
            v.sort();
            v.dedup();
            v
        };
        let mut cands: Vec<(String, f64)> = vec![];
        for (net, (len, vias, bends)) in &used {
            if poured.contains(net) || big.contains(net) || open.contains(net) || tried.contains(net) || !fresh.contains(net) || only.is_some_and(|o| o != net) {
                continue;
            }
            let pts: Vec<Point> = board.pads.iter().filter(|p| &p.net == net).map(|p| p.pos).collect();
            // (the bends a route needs anyway, two for a dogleg, are not held against it)
            let excess = len + VIA_WEIGHT * *vias as f64 + BEND_WEIGHT * bends.saturating_sub(2) as f64 - shortest_tree(&pts);
            if excess > MIN_EXCESS || *vias > 0 && excess > 0.0 {
                cands.push((net.clone(), excess));
            }
        }
        cands.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        let Some((net, _)) = cands.into_iter().next() else { break };
        tried.push(net.clone());

        // the neighbours: nets with a track near this net's pads, the one with the most track there first
        let pts: Vec<Point> = board.pads.iter().filter(|p| p.net == net).map(|p| p.pos).collect();
        let (lo, hi) = pts.iter().fold((Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN)), |(a, b), p| (Point::new(a.x.min(p.x), a.y.min(p.y)), Point::new(b.x.max(p.x), b.y.max(p.y))));
        let conn = connectivity(board);
        let mut near: HashMap<&str, f64> = HashMap::new();
        for (i, t) in board.tracks.iter().enumerate() {
            let Some(n) = conn.track_net(i) else { continue };
            if n == net || poured.iter().any(|p| p == n) || big.iter().any(|b| b == n) || open.iter().any(|o| o == n) || !fresh.iter().any(|f| f == n) {
                continue;
            }
            let inside = |p: Point| p.x >= lo.x - REACH && p.x <= hi.x + REACH && p.y >= lo.y - REACH && p.y <= hi.y + REACH;
            if inside(t.a) || inside(t.b) {
                *near.entry(n).or_default() += t.a.dist(t.b);
            }
        }
        let mut neighbours: Vec<(&str, f64)> = near.into_iter().collect();
        neighbours.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(b.0)));
        // alone first, then with each neighbour
        let mut plans: Vec<Vec<String>> = vec![vec![net.clone()]];
        plans.extend(neighbours.iter().take(NEIGHBOURS).map(|(n, _)| vec![net.clone(), n.to_string()]));

        let before_use = used;
        for nets in plans {
            if start.elapsed() > BUDGET {
                break;
            }
            let names: Vec<&str> = nets.iter().map(String::as_str).collect();
            let was = cost_of(&before_use, &names);
            let mut trial = board.clone();
            for n in &nets {
                rip_up_net(&mut trial, n);
            }
            for n in &nets {
                route_net(&mut trial, opt, n);
            }
            if ratsnest(&trial).iter().any(|w| nets.contains(&w.net)) {
                continue;
            }
            let after_use = net_use(&trial);
            let now = cost_of(&after_use, &names);
            if now >= was - MIN_GAIN {
                continue;
            }
            let violations = check(&trial).len();
            if violations > drc {
                continue;
            }
            report.improved += 1;
            report.saved += was - now;
            for n in &names {
                let (l0, v0, _) = before_use.get(*n).copied().unwrap_or((0.0, 0, 0));
                let (l1, v1, _) = after_use.get(*n).copied().unwrap_or((0.0, 0, 0));
                report.length += l1 - l0;
                report.vias += v1 as i64 - v0 as i64;
            }
            *board = trial;
            drc = violations;
            break;
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(b: &mut Board, x: f64, y: f64, net: &str) {
        let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
        p.net = net.into();
        b.pads.push(p);
    }

    fn line(b: &mut Board, pts: &[(f64, f64)]) {
        for w in pts.windows(2) {
            b.tracks.push(Track { side: Side::Top, a: Point::new(w[0].0, w[0].1), b: Point::new(w[1].0, w[1].1), width: 0.25 });
        }
    }

    fn length(b: &Board) -> f64 {
        b.tracks.iter().map(|t| t.a.dist(t.b)).sum()
    }

    /// N runs round a bulge of B; B bulges only because it was drawn that way.
    fn detour_round_a_bulge() -> Board {
        let mut b = Board::new("t", 40.0, 30.0);
        pad(&mut b, 5.0, 10.0, "N");
        pad(&mut b, 35.0, 10.0, "N");
        pad(&mut b, 18.0, 5.0, "B");
        pad(&mut b, 22.0, 5.0, "B");
        line(&mut b, &[(18.0, 5.0), (18.0, 15.0), (22.0, 15.0), (22.0, 5.0)]);
        line(&mut b, &[(5.0, 10.0), (5.0, 20.0), (35.0, 20.0), (35.0, 10.0)]);
        b
    }

    #[test]
    fn a_net_and_its_neighbour_are_routed_again_when_that_is_shorter() {
        let mut b = detour_round_a_bulge();
        assert!(ratsnest(&b).is_empty() && check(&b).is_empty(), "the start is a clean, complete board");
        let before = length(&b);
        let r = improve_routes(&mut b, &RouteOptions::default(), None, None);
        assert!(r.improved >= 1, "{r:?}");
        assert!(ratsnest(&b).is_empty(), "still connected");
        assert!(check(&b).is_empty(), "{:?}", check(&b));
        assert!(length(&b) < before - 15.0, "{before} -> {}", length(&b));
        assert!(r.saved > 15.0 && r.length < -15.0, "{r:?}");
    }

    #[test]
    fn a_route_of_many_small_jogs_is_straightened_though_it_is_hardly_longer() {
        let mut b = Board::new("t", 40.0, 30.0);
        pad(&mut b, 5.0, 10.0, "N");
        pad(&mut b, 35.0, 10.0, "N");
        line(&mut b, &[(5.0, 10.0), (10.0, 10.0), (10.5, 10.5), (15.5, 10.5), (16.0, 10.0), (20.0, 10.0), (20.5, 10.5), (25.5, 10.5), (26.0, 10.0), (35.0, 10.0)]);
        let (n, len) = (b.tracks.len(), length(&b));
        assert!(len - 30.0 < 2.0, "the jogs add hardly any length: {len}");
        assert!(net_use(&b)["N"].2 >= 8, "{:?}", net_use(&b));
        let r = improve_routes(&mut b, &RouteOptions::default(), None, None);
        assert!(r.improved >= 1, "{r:?}");
        assert!(ratsnest(&b).is_empty() && check(&b).is_empty());
        assert!(b.tracks.len() < n && net_use(&b)["N"].2 <= 2, "{n} -> {} tracks, {:?}", b.tracks.len(), net_use(&b));
    }

    #[test]
    fn a_route_that_is_already_short_is_left_alone() {
        let mut b = Board::new("t", 40.0, 30.0);
        pad(&mut b, 5.0, 10.0, "N");
        pad(&mut b, 35.0, 10.0, "N");
        line(&mut b, &[(5.0, 10.0), (35.0, 10.0)]);
        let before = b.tracks.clone();
        assert_eq!(improve_routes(&mut b, &RouteOptions::default(), None, None), ImproveReport::default());
        assert_eq!(b.tracks.len(), before.len());
    }

    #[test]
    fn the_result_is_the_same_every_time() {
        let run = || {
            let mut b = detour_round_a_bulge();
            improve_routes(&mut b, &RouteOptions::default(), None, None);
            b.tracks.iter().map(|t| (t.a, t.b)).collect::<Vec<_>>()
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn nets_of_a_pour_and_open_nets_are_not_touched() {
        let mut b = detour_round_a_bulge();
        b.zones.push(Zone::new("N", Side::Bottom, vec![Point::new(1.0, 1.0), Point::new(1.0, 29.0), Point::new(39.0, 29.0), Point::new(39.0, 1.0)]));
        let before: Vec<_> = b.tracks.iter().map(|t| (t.a, t.b)).collect();
        improve_routes(&mut b, &RouteOptions::default(), None, None);
        // N is poured: only B may change, and B alone has no neighbour to gain from
        assert_eq!(b.tracks.iter().filter(|t| t.a.y >= 19.9 && t.b.y >= 19.9).count(), 1, "{before:?}");
    }

    #[test]
    fn only_the_tracks_from_the_given_number_on_may_change() {
        let mut b = detour_round_a_bulge();
        let n = b.tracks.len();
        let before: Vec<_> = b.tracks.iter().map(|t| (t.a, t.b)).collect();
        assert_eq!(improve_routes(&mut b, &RouteOptions::default(), Some(n), None), ImproveReport::default(), "nothing is new");
        assert_eq!(before, b.tracks.iter().map(|t| (t.a, t.b)).collect::<Vec<_>>());
        // a named net: only it is tried first-hand (the other is no candidate, but may be ripped as its neighbour)
        let mut b = detour_round_a_bulge();
        assert!(improve_routes(&mut b, &RouteOptions::default(), None, Some("N")).improved >= 1);
        let mut b = detour_round_a_bulge();
        assert_eq!(improve_routes(&mut b, &RouteOptions::default(), None, Some("nothing")), ImproveReport::default());
    }
}
