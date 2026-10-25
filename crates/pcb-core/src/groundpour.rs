//! A ground pour for a two-layer board: a zone of the ground net over the whole bottom layer, and for every surface
//! mount ground pad on top a short straight track to a via, which joins the pad to the pour. Through-hole ground pads
//! and ground pads on the bottom meet the pour by themselves. After that the ground needs no wires on top.
//! When the free copper of the top is poured too (`PourOptions.top`), the surface pads sit on that pour, so one way
//! through the board (a through-hole pad or a single via) is all that is added.
//!
//! The vias go where the DRC finds nothing wrong, the nearest first. A pad that has no such place is counted and left
//! for the router.

use crate::drc::{check, connectivity};
use crate::model::*;
use std::collections::HashSet;

#[derive(Clone, Debug)]
pub struct PourOptions {
    /// The net to pour; `None`: the net named GND.
    pub net: Option<String>,
    /// Width of the short tracks from the pads to their vias.
    pub track_width: f64,
    pub via_diameter: f64,
    pub via_drill: f64,
    /// Also fill the free copper of the top with the net (`top_pour`), so that the router works on a board that has the
    /// pour on both sides.
    pub top: bool,
    /// The copper layer of the pour: the bottom (default), or an inner layer of a 4+ layer board. Through-hole pads and
    /// vias reach every layer, so the surface pads of the net get their via here just as on the bottom.
    pub side: Side,
}

impl Default for PourOptions {
    fn default() -> Self {
        Self { net: None, track_width: 0.3, via_diameter: 0.6, via_drill: 0.3, top: true, side: Side::Bottom }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PourReport {
    /// A zone was added (it was not there yet).
    pub zone_added: bool,
    /// The zone on the top layer was added too.
    pub top_added: bool,
    /// Surface pads joined to the pour with a new via.
    pub vias: usize,
    /// Pads that were on the pour already.
    pub already: usize,
    /// Surface pads that found no place for a via.
    pub left: usize,
}

/// The four corners of a pour over a rectangular board, kept off the edge by the edge clearance and a little more.
fn pour_corners(board: &Board) -> Result<Vec<Point>, String> {
    let Some((lo, hi)) = board.bounds().filter(|_| board.outline.len() >= 3) else {
        return Err("先に基板の外形を決めてください (ベタは基板の外形いっぱいに置きます)".into());
    };
    let inset = board.rules.edge_clearance + 0.2;
    let is_rect = board.outline.len() == 4 && board.outline.iter().all(|p| (p.x == lo.x || p.x == hi.x) && (p.y == lo.y || p.y == hi.y));
    if !is_rect {
        return Err("四角い外形の基板にだけ、自動でベタを置けます。ほかの形は「銅箔ゾーン」ツールで、外形の内側に多角形を描いてください".into());
    }
    if hi.x - lo.x <= 2.0 * inset + 1.0 || hi.y - lo.y <= 2.0 * inset + 1.0 {
        return Err("基板が小さすぎて、ベタを置けません".into());
    }
    Ok(vec![Point::new(lo.x + inset, lo.y + inset), Point::new(hi.x - inset, lo.y + inset), Point::new(hi.x - inset, hi.y - inset), Point::new(lo.x + inset, hi.y - inset)])
}

/// Fill the unused copper of the top layer with the pour net (after the routing: the zone keeps its clearance from
/// every wire, so the wires stay as they are). Surface pads of the net on the zone join it by themselves. `Ok(false)`:
/// the top has such a zone already. Nothing changes (and the reason is returned) when the DRC would find more
/// problems than before or a ground connection would be lost.
pub fn top_pour(board: &mut Board, net: Option<&str>) -> Result<bool, String> {
    let net = net.unwrap_or("GND");
    if !board.pads.iter().any(|p| p.net == net) {
        return Err(format!("{net} につながるパッドがありません"));
    }
    if board.zones.iter().any(|z| z.net == net && z.side == Side::Top) {
        return Ok(false);
    }
    let mut zone = Zone::new(net, Side::Top, pour_corners(board)?);
    zone.remove_islands = true;
    let mut work = board.clone();
    work.zones.push(zone);
    let open = |b: &Board| crate::ratsnest::ratsnest(b).iter().filter(|w| w.net == net).count();
    if check(&work).len() > check(board).len() || open(&work) > open(board) {
        return Err("表面のベタを置くと、ルール違反が増えるか、つながりが切れるので、置きませんでした".into());
    }
    *board = work;
    Ok(true)
}

/// How far (mm) from a pad's centre a via may be put, at most.
const REACH: f64 = 5.0;
/// Step (mm) along each direction.
const STEP: f64 = 0.25;

/// Put the pour and the vias on `board` (see the module). Changes nothing when it fails.
pub fn ground_pour(board: &mut Board, opt: &PourOptions) -> Result<PourReport, String> {
    let net = opt.net.clone().unwrap_or_else(|| "GND".to_string());
    if !board.pads.iter().any(|p| p.net == net) {
        return Err(format!("{net} につながるパッドがありません。先に回路図から取り込んで、ネット名を確かめてください"));
    }
    let corners = pour_corners(board)?;
    let mut work = board.clone();
    let mut report = PourReport::default();
    if let Side::Inner(n) = opt.side
        && n > work.inner_layers {
            return Err(format!("内層{n} がありません。先に層の数を増やしてください (いまは {} 層)", work.inner_layers as usize + 2));
        }
    if !work.zones.iter().any(|z| z.net == net && z.side == opt.side) {
        let mut zone = Zone::new(&net, opt.side, corners);
        zone.remove_islands = true;
        work.zones.push(zone);
        report.zone_added = true;
    }
    // with the top filled too, every surface pad is on the pour already: the pour needs only one way through the board
    // (a through-hole pad or one via) and no via for each pad. A top pour that would break something is left out, and the
    // pads get their vias one by one.
    if opt.top && opt.side == Side::Bottom {
        report.top_added = top_pour(&mut work, Some(&net)) == Ok(true);
    }
    let ground_pads: Vec<usize> = (0..work.pads.len()).filter(|&i| work.pads[i].net == net).collect();
    // the pads on the pour: in a group with a via of the net or a through-hole pad (both meet the bottom zone)
    let on_pour = |b: &Board| -> HashSet<usize> {
        let conn = connectivity(b);
        let mut groups: HashSet<usize> = (0..b.vias.len()).filter(|&v| conn.via_net(v) == Some(net.as_str())).map(|v| conn.via_group(v)).collect();
        groups.extend(ground_pads.iter().filter(|&&i| b.pads[i].drill.is_some()).map(|&i| conn.pad_group(i)));
        groups
    };
    for &i in &ground_pads {
        let pad = work.pads[i].clone();
        // a pad on the pour's own layer is inside the zone already
        if pad.drill.is_none() && pad.side == opt.side {
            report.already += 1;
            continue;
        }
        if pad.drill.is_some() || on_pour(&work).contains(&connectivity(&work).pad_group(i)) {
            report.already += 1;
            continue;
        }
        let before = check(&work).len();
        let (w, h) = pad.shape.size();
        let mut done = false;
        let first = opt.via_diameter / 2.0 + work.rules.min_clearance + 0.1;
        'search: for k in 0..((REACH / STEP) as usize) {
            for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                let half = if dx != 0.0 { w / 2.0 } else { h / 2.0 };
                let d = half + first + k as f64 * STEP;
                let at = Point::new(pad.pos.x + dx * d, pad.pos.y + dy * d);
                let mut trial = work.clone();
                trial.tracks.push(Track { side: pad.side, a: pad.pos, b: at, width: opt.track_width });
                trial.vias.push(Via { pos: at, diameter: opt.via_diameter, drill: opt.via_drill, net: String::new() });
                if check(&trial).len() <= before {
                    work = trial;
                    done = true;
                    break 'search;
                }
            }
        }
        if done {
            report.vias += 1;
        } else {
            report.left += 1;
        }
    }
    *board = work;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{drc, ratsnest::ratsnest};

    fn pad(b: &mut Board, x: f64, y: f64, net: &str, drill: Option<f64>) {
        let shape = if drill.is_some() { PadShape::Circle { d: 1.6 } } else { PadShape::Rect { w: 1.0, h: 1.4 } };
        let mut p = Pad::new(Point::new(x, y), shape, drill, Side::Top);
        p.net = net.into();
        b.pads.push(p);
    }

    fn board() -> Board {
        let mut b = Board::new("t", 40.0, 30.0);
        // three surface ground pads and a through-hole one, and a signal between them
        for (x, y) in [(8.0, 8.0), (30.0, 8.0), (8.0, 22.0)] {
            pad(&mut b, x, y, "GND", None);
        }
        pad(&mut b, 30.0, 22.0, "GND", Some(0.8));
        pad(&mut b, 15.0, 15.0, "SIG", None);
        pad(&mut b, 25.0, 15.0, "SIG", None);
        b
    }

    #[test]
    fn the_ground_pads_join_the_bottom_pour_through_vias() {
        let mut b = board();
        assert!(ratsnest(&b).iter().any(|w| w.net == "GND"));
        let r = ground_pour(&mut b, &PourOptions { top: false, ..Default::default() }).unwrap();
        assert!(r.zone_added && !r.top_added);
        assert_eq!((r.vias, r.already, r.left), (3, 1, 0), "{r:?}");
        assert!(ratsnest(&b).iter().all(|w| w.net != "GND"), "the ground needs no wires");
        assert_eq!(b.vias.len(), 3);
        assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
        // the pour keeps off the board's edge
        assert!(drc::check_final(&b).is_empty(), "{:?}", drc::check_final(&b));
    }

    #[test]
    fn a_pour_on_an_inner_layer_gets_a_via_for_every_surface_pad_of_the_net() {
        let mut b = board();
        b.inner_layers = 2;
        let r = ground_pour(&mut b, &PourOptions { top: false, side: Side::Inner(1), ..Default::default() }).unwrap();
        assert!(r.zone_added && !r.top_added);
        assert_eq!((r.vias, r.already, r.left), (3, 1, 0), "{r:?}");
        assert!(b.zones.iter().all(|z| z.side == Side::Inner(1)), "only the inner layer is poured");
        assert!(ratsnest(&b).iter().all(|w| w.net != "GND"), "the ground needs no wires");
        assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
        // a layer the board does not have is refused, and nothing changes
        let mut two = board();
        assert!(ground_pour(&mut two, &PourOptions { side: Side::Inner(1), ..Default::default() }).is_err());
        assert!(two.zones.is_empty() && two.vias.is_empty());
    }

    #[test]
    fn with_the_top_poured_too_no_via_is_put_for_each_pad() {
        // a through-hole ground pad is the way through the board: no via at all
        let mut b = board();
        let r = ground_pour(&mut b, &PourOptions::default()).unwrap();
        assert!(r.zone_added && r.top_added, "{r:?}");
        assert_eq!((r.vias, r.left), (0, 0), "{r:?}");
        assert!(b.vias.is_empty() && b.tracks.is_empty());
        assert!(ratsnest(&b).iter().all(|w| w.net != "GND"), "ground is joined");
        // without one, a single via joins the two sides
        let mut c = board();
        c.pads.retain(|p| p.drill.is_none());
        let r = ground_pour(&mut c, &PourOptions::default()).unwrap();
        assert_eq!((r.top_added, r.vias, r.left), (true, 1, 0), "{r:?}");
        assert!(ratsnest(&c).iter().all(|w| w.net != "GND"), "ground is joined");
        assert!(drc::check_final(&c).is_empty(), "{:?}", drc::check_final(&c));
    }

    #[test]
    fn pouring_twice_changes_nothing_more() {
        let mut b = board();
        ground_pour(&mut b, &PourOptions::default()).unwrap();
        let (vias, tracks, zones) = (b.vias.len(), b.tracks.len(), b.zones.len());
        let r = ground_pour(&mut b, &PourOptions::default()).unwrap();
        assert_eq!((r.zone_added, r.vias), (false, 0));
        assert!(!r.top_added);
        assert_eq!((b.vias.len(), b.tracks.len(), b.zones.len()), (vias, tracks, zones));
    }

    #[test]
    fn a_board_without_the_net_or_with_a_round_outline_is_refused_and_left_alone() {
        let mut b = Board::new("t", 40.0, 30.0);
        pad(&mut b, 8.0, 8.0, "SIG", None);
        assert!(ground_pour(&mut b, &PourOptions::default()).unwrap_err().contains("GND"));
        let mut c = board();
        c.outline = vec![Point::new(0.0, 0.0), Point::new(40.0, 0.0), Point::new(40.0, 30.0), Point::new(20.0, 35.0), Point::new(0.0, 30.0)];
        let before = c.zones.len();
        assert!(ground_pour(&mut c, &PourOptions::default()).is_err());
        assert_eq!((c.zones.len(), c.vias.len()), (before, 0));
    }

    #[test]
    fn the_signal_nets_route_over_a_board_poured_on_both_sides() {
        let mut b = board();
        let r = ground_pour(&mut b, &PourOptions::default()).unwrap();
        assert!(r.top_added);
        let r = crate::autoroute::autoroute(&mut b, &Default::default(), None);
        assert!(r.failed.is_empty(), "{:?}", r.failed);
        assert!(ratsnest(&b).is_empty());
        assert!(drc::check_final(&b).is_empty(), "{:?}", drc::check_final(&b));
    }

    #[test]
    fn the_signal_nets_route_round_the_pour_vias() {
        let mut b = board();
        ground_pour(&mut b, &PourOptions::default()).unwrap();
        let r = crate::autoroute::autoroute(&mut b, &Default::default(), None);
        assert!(r.failed.is_empty(), "{:?}", r.failed);
        assert!(ratsnest(&b).is_empty());
        assert!(drc::check_final(&b).is_empty(), "{:?}", drc::check_final(&b));
    }

    #[test]
    fn the_top_is_filled_with_ground_after_the_routing_and_stays_clean() {
        let mut b = board();
        ground_pour(&mut b, &PourOptions { top: false, ..Default::default() }).unwrap();
        crate::autoroute::autoroute(&mut b, &Default::default(), None);
        assert_eq!(top_pour(&mut b, None), Ok(true));
        assert!(b.zones.iter().any(|z| z.net == "GND" && z.side == Side::Top));
        assert!(ratsnest(&b).is_empty());
        assert!(drc::check_final(&b).is_empty(), "{:?}", drc::check_final(&b));
        // a second time there is nothing to add
        assert_eq!(top_pour(&mut b, None), Ok(false));
    }

    #[test]
    fn the_top_pour_is_refused_for_a_missing_net_or_a_round_outline() {
        let mut b = Board::new("t", 40.0, 30.0);
        pad(&mut b, 8.0, 8.0, "SIG", None);
        assert!(top_pour(&mut b, None).is_err());
        let mut c = board();
        c.outline = vec![Point::new(0.0, 0.0), Point::new(40.0, 0.0), Point::new(40.0, 30.0), Point::new(20.0, 35.0), Point::new(0.0, 30.0)];
        assert!(top_pour(&mut c, None).is_err());
        assert!(c.zones.is_empty());
    }
}
