//! ICs: the nets between ICs that pull them together, and the block of an IC with its own small parts round it.

use super::*;

/// ICs that share nets want to sit close: a bus between two ICs (the outputs of one that go to the other) is many wires that all have to run
/// the distance between them. For every pair of ICs (8 pads or more) that have pads on one net, a two-pin net of one pad of each is added
/// twice over (the placement adds up the lengths of nets, so the pair counts more the more nets they share).
pub(super) fn ic_pair_nets(board: &Board, items: &[(String, Point, f64, f64)], gap: f64, label: f64, poured: &[String]) -> Vec<NetPins> {
    let index = |r: &str| items.iter().position(|it| it.0 == r);
    let offset = |pad: &Pad, i: usize| (i, pad.pos.x - items[i].1.x + gap / 2.0, pad.pos.y - items[i].1.y + gap / 2.0 + label);
    let count = |r: &str| board.pads.iter().filter(|p| p.refdes == r).count();
    let ics: Vec<&Part> = board.parts.iter().filter(|p| count(&p.refdes) >= 8 && p.refdes.starts_with('U')).collect();
    let mut out: Vec<NetPins> = vec![];
    for (a, ia) in ics.iter().enumerate() {
        for ib in &ics[a + 1..] {
            let (Some(i), Some(j)) = (index(&ia.refdes), index(&ib.refdes)) else { continue };
            let nets_a: std::collections::BTreeSet<&str> = board.pads.iter().filter(|p| p.refdes == ia.refdes && !p.net.is_empty() && !poured.contains(&p.net)).map(|p| p.net.as_str()).collect();
            for net in board.pads.iter().filter(|p| p.refdes == ib.refdes && nets_a.contains(p.net.as_str())).map(|p| p.net.as_str()).collect::<std::collections::BTreeSet<_>>() {
                // a net on many ICs (a supply, a bus to all of them) says little: only nets of a few pads
                if board.pads.iter().filter(|p| p.net == net).count() > 8 {
                    continue;
                }
                let (Some(pa), Some(pb)) = (board.pads.iter().find(|p| p.refdes == ia.refdes && p.net == net), board.pads.iter().find(|p| p.refdes == ib.refdes && p.net == net)) else { continue };
                for _ in 0..2 {
                    out.push(vec![offset(pa, i), offset(pb, j)]);
                }
            }
        }
    }
    out
}

/// How far (mm) a signal pin of an IC keeps free in front of it, for the wire that leaves it (`escape_zones`).
pub(super) const ESCAPE_LEN: f64 = 1.8;

/// The ground in front of the signal pins on the edge of an IC, where the wires must be able to leave: for each such pin a box from its
/// outer edge, `ESCAPE_LEN` out, as wide as the pin. A capacitor that sits there closes the way out of a fine-pitch package; its own
/// supply pin is not a signal pin, so it still stands next to it. `rail` tells the nets that are not signals.
pub(super) fn escape_zones(board: &Board, ic: &str, rail: &dyn Fn(&str) -> bool) -> Vec<(Point, Point)> {
    let Some((lo, hi)) = extent(board, ic) else { return vec![] };
    let (cx, cy) = ((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
    let (hw, hh) = (((hi.x - lo.x) / 2.0).max(0.1), ((hi.y - lo.y) / 2.0).max(0.1));
    let mut out = vec![];
    for p in board.pads.iter().filter(|p| p.refdes == ic && !p.net.is_empty() && !rail(&p.net)) {
        let (dx, dy) = ((p.pos.x - cx) / hw, (p.pos.y - cy) / hh);
        // only the pins on the edge of the package
        if dx.abs().max(dy.abs()) < 0.6 {
            continue;
        }
        let (w, h) = p.shape.size();
        let zone = if dx.abs() >= dy.abs() {
            let s = dx.signum();
            let edge = p.pos.x + s * w / 2.0;
            (Point::new(edge.min(edge + s * ESCAPE_LEN), p.pos.y - h / 2.0), Point::new(edge.max(edge + s * ESCAPE_LEN), p.pos.y + h / 2.0))
        } else {
            let s = dy.signum();
            let edge = p.pos.y + s * h / 2.0;
            (Point::new(p.pos.x - w / 2.0, edge.min(edge + s * ESCAPE_LEN)), Point::new(p.pos.x + w / 2.0, edge.max(edge + s * ESCAPE_LEN)))
        };
        out.push(zone);
    }
    out
}

/// Whether the box (`lo`, `hi`) overlaps one of the `zones`.
pub(super) fn in_zones(zones: &[(Point, Point)], lo: Point, hi: Point) -> bool {
    zones.iter().any(|(l, h)| l.x < hi.x && lo.x < h.x && l.y < hi.y && lo.y < h.y)
}

/// An IC with the bypass capacitors that belong to it, settled once and kept: how each capacitor is turned, and where its part origin stands from the IC's.
pub(super) struct IcBlock {
    pub(super) ic: String,
    pub(super) caps: Vec<(String, u8, Point)>,
}

/// For every IC (8 pads or more) the bypass capacitors that serve it (a supply pad on a net of the IC, the other pad on another rail), each
/// put at the best free place round the IC's pin (as `attach_bypass_caps` does, on a board of the IC and its capacitors alone), turned so that
/// the supply pad faces the pin. The IC and its capacitors then move as one piece. A capacitor goes to the IC that has both of its nets (a
/// capacitor between VCC and VNEG, to the IC that runs between them), else to the IC with fewer capacitors; `skip` are parts that are in a copy.
pub(super) fn ic_blocks(board: &Board, skip: &std::collections::BTreeSet<String>) -> Vec<IcBlock> {
    use crate::drc::{is_ground_net, is_negative_supply_net, is_supply_net};
    let rail = |n: &str| is_ground_net(n) || is_supply_net(n) || is_negative_supply_net(n);
    let supply = |n: &str| is_supply_net(n) || is_negative_supply_net(n);
    let count = |r: &str| board.pads.iter().filter(|p| p.refdes == r).count();
    let ics: Vec<&Part> = board.parts.iter().filter(|p| p.refdes.starts_with('U') && count(&p.refdes) >= 8 && !skip.contains(&p.refdes)).collect();
    let mut assigned: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for cap in board.parts.iter().filter(|p| p.refdes.starts_with('C') && !skip.contains(&p.refdes)) {
        let cp: Vec<&Pad> = board.pads.iter().filter(|p| p.refdes == cap.refdes).collect();
        let Some(sp) = cp.iter().find(|p| supply(&p.net) && cp.iter().any(|q| q.net != p.net && rail(&q.net))) else { continue };
        let other = cp.iter().find(|q| q.net != sp.net && rail(&q.net)).map(|q| q.net.as_str());
        let has = |ic: &Part, net: &str| board.pads.iter().any(|p| p.refdes == ic.refdes && p.net == net);
        // the table says which IC it serves
        if let Some(named) = bypass_ic(board, &cap.refdes).and_then(|r| ics.iter().find(|ic| ic.refdes == r)).filter(|ic| has(ic, &sp.net)) {
            assigned.entry(named.refdes.clone()).or_default().push(cap.refdes.clone());
            continue;
        }
        let mut fit: Vec<&&Part> = ics.iter().filter(|ic| has(ic, &sp.net)).collect();
        if fit.iter().any(|ic| other.is_some_and(|o| has(ic, o))) {
            fit.retain(|ic| other.is_some_and(|o| has(ic, o)));
        }
        fit.sort_by_key(|ic| assigned.get(&ic.refdes).map_or(0, Vec::len));
        if let Some(ic) = fit.first() {
            assigned.entry(ic.refdes.clone()).or_default().push(cap.refdes.clone());
        }
    }
    // the small parts that hang on one IC and nothing else of size (the resistors round an op amp, a filter on one input): on the nets they
    // share with it, which are not hubs, they meet that one IC and no other IC or header
    let mut sat_net: std::collections::BTreeMap<String, String> = Default::default();
    let taken: std::collections::BTreeSet<String> = assigned.values().flatten().cloned().collect();
    for part in board.parts.iter().filter(|p| !skip.contains(&p.refdes) && !p.refdes.starts_with('U') && count(&p.refdes) <= SATELLITE_PADS && !taken.contains(&p.refdes)) {
        let mut touched: std::collections::BTreeMap<&str, &str> = Default::default();
        let mut other_anchor = false;
        for pad in board.pads.iter().filter(|p| p.refdes == part.refdes && !p.net.is_empty()) {
            let on: std::collections::BTreeSet<&str> = board.pads.iter().filter(|q| q.net == pad.net).map(|q| q.refdes.as_str()).collect();
            if on.len() > 6 {
                continue;
            }
            for r in on.iter().filter(|r| count(r) >= 8) {
                if r.starts_with('U') {
                    touched.insert(r, pad.net.as_str());
                } else {
                    other_anchor = true;
                }
            }
        }
        if touched.len() == 1 && !other_anchor {
            let (ic, net) = touched.into_iter().next().unwrap();
            if ics.iter().any(|i| i.refdes == ic) {
                sat_net.insert(part.refdes.clone(), net.to_string());
                assigned.entry(ic.to_string()).or_default().push(part.refdes.clone());
            }
        }
    }
    let mut out = vec![];
    for (ic_name, caps) in assigned {
        let Some(ic) = board.parts.iter().find(|p| p.refdes == ic_name) else { continue };
        let mut scratch = Board::new("block", 300.0, 300.0);
        let mut wanted: Vec<&str> = vec![ic.footprint.as_str()];
        wanted.extend(caps.iter().filter_map(|c| board.parts.iter().find(|p| &p.refdes == c)).map(|p| p.footprint.as_str()));
        scratch.footprints = board.footprints.iter().filter(|f| wanted.contains(&f.name.as_str())).cloned().collect();
        scratch.rules = board.rules.clone();
        let origin = Point::new(100.0, 100.0);
        if scratch.set_part(Part { pos: origin, ..ic.clone() }).is_err() {
            continue;
        }
        for c in &caps {
            if let Some(part) = board.parts.iter().find(|p| &p.refdes == c)
                && scratch.set_part(Part { pos: Point::new(0.0, 0.0), ..part.clone() }).is_err()
            {
                continue;
            }
        }
        for pad in scratch.pads.iter_mut() {
            pad.net = board.pads.iter().find(|q| q.refdes == pad.refdes && q.pin == pad.pin).map_or(String::new(), |q| q.net.clone());
        }
        let mut placed: Vec<IcBlock> = vec![];
        let mut block = IcBlock { ic: ic_name.clone(), caps: vec![] };
        for c in &caps {
            let cap_pads: Vec<Pad> = scratch.pads.iter().filter(|p| &p.refdes == c).cloned().collect();
            let sp = match sat_net.get(c) {
                Some(net) => cap_pads.iter().find(|p| &p.net == net).cloned(),
                None => cap_pads.iter().find(|p| supply(&p.net) && cap_pads.iter().any(|q| q.net != p.net && rail(&q.net))).cloned(),
            };
            let Some(sp) = sp else { continue };
            let other_net = if sat_net.contains_key(c) { None } else { cap_pads.iter().find(|q| q.net != sp.net && rail(&q.net)).map(|q| q.net.clone()) };
            let pins: Vec<Pad> = scratch.pads.iter().filter(|p| p.refdes == ic_name && p.net == sp.net).cloned().collect();
            let other_pins: Vec<Pad> = other_net.map_or(vec![], |n| scratch.pads.iter().filter(|p| p.refdes == ic_name && p.net == n).cloned().collect());
            // the boxes in the way: the IC and the capacitors already put
            let mut boxes: Vec<(Point, Point)> = extent(&scratch, &ic_name).into_iter().collect();
            boxes.extend(block.caps.iter().filter_map(|(r, ..)| extent(&scratch, r)));
            let cost_of = |pads: &[Pad]| -> f64 {
                let Some(s) = pads.iter().find(|q| q.pin == sp.pin) else { return f64::MAX };
                let o = pads.iter().find(|q| q.pin != sp.pin && q.net != sp.net);
                pins.iter()
                    .map(|pin| {
                        let near = other_pins.iter().map(|op| o.map_or(0.0, |o| o.pos.dist(op.pos)) + 0.0 * pin.pos.dist(op.pos)).fold(f64::MAX, f64::min);
                        s.pos.dist(pin.pos) + 0.5 * if near == f64::MAX { 0.0 } else { near }
                    })
                    .fold(f64::MAX, f64::min)
            };
            let mut best: Option<(f64, Point, u8)> = None;
            let zones = escape_zones(&scratch, &ic_name, &rail);
            // the first pass keeps the way out of the signal pins free; when there is no such place, the second does not
            for strict in [true, false] {
            if best.is_some() {
                break;
            }
            for pin in &pins {
                let steps = (BYPASS_REACH / BYPASS_STEP) as i32;
                for ix in -steps..=steps {
                    for iy in -steps..=steps {
                        let target = Point::new(pin.pos.x + ix as f64 * BYPASS_STEP, pin.pos.y + iy as f64 * BYPASS_STEP);
                        if target.dist(pin.pos) > BYPASS_REACH {
                            continue;
                        }
                        for rot in 0..4u8 {
                            scratch.place_part(c, Point::new(0.0, 0.0), rot);
                            let Some((lo, hi)) = extent(&scratch, c) else { continue };
                            let at = Point::new(snap(target.x - (lo.x + hi.x) / 2.0), snap(target.y - (lo.y + hi.y) / 2.0));
                            scratch.place_part(c, at, rot);
                            let Some((lo, hi)) = extent(&scratch, c) else { continue };
                            if boxes.iter().any(|(l, h)| (l.x - hi.x).max(lo.x - h.x).max((l.y - hi.y).max(lo.y - h.y)) < ALIGN_SPACE) {
                                continue;
                            }
                            if strict && in_zones(&zones, lo, hi) {
                                continue;
                            }
                            let pads: Vec<Pad> = scratch.pads.iter().filter(|q| &q.refdes == c).cloned().collect();
                            let cst = cost_of(&pads);
                            if best.as_ref().is_none_or(|b| cst < b.0 - 1e-9) {
                                best = Some((cst, at, rot));
                            }
                        }
                    }
                }
            }
            }
            if let Some((_, at, rot)) = best {
                scratch.place_part(c, at, rot);
                block.caps.push((c.clone(), rot, Point::new(at.x - origin.x, at.y - origin.y)));
            }
        }
        let _ = &mut placed;
        if !block.caps.is_empty() {
            out.push(block);
        }
    }
    out
}
