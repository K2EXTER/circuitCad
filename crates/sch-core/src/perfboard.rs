//! A layout made for a perfboard (B5a): no strips to share, so the parts go where they are nearest what they join and each net is
//! wired by bare wires on the copper side, straight along the rows, the columns or at 45 degrees, never touching another net's (the
//! ones that find no way get an insulated wire on top instead). The stripboard's layout (`stripboard::strip_layout_with`) alternates bands of
//! strips with rows of chips, every chip pin wired up or down to a band: on a perfboard that leaves most of the board empty (the DMM's
//! B stage: 60 × 78 holes, 42 of the rows the chips' own).
//!
//! The result is a `StripLayout` with `perf` set: every hole a piece of its own, joined only by the bare wires and the links, so the
//! check, the ratsnest, the forks and the printed drawing read it as they read any other.
//!
//! Here: the tries and the choice among them (`perf_layout_keeping`, `rank`). The parts' shapes in `shape`, what the requirements
//! table asks in `wants`, the placing in `place` (and the springs' targets in `simpl`), the annealing in `anneal`, the chips on the
//! copper side in `back`, the river in `river`, the wiring in `route`, the wires after the layout (their sides, "自動配線", the
//! parts added to a board edited by hand) in `wires`, the parts to lock first in `lock`.

use crate::breadboard::{is_ground_name, read, Circuit, Look};
use crate::stripboard::{At, Link, StripLayout, StripOptions, StripPart};
use crate::Schematic;
use std::cmp::Reverse;

mod anneal;
mod back;
mod lock;
mod place;
mod river;
mod route;
mod shape;
mod simpl;
mod wants;
mod wires;
use anneal::*;
use back::*;
use place::*;
use river::*;
use shape::*;
use wants::*;
use route::{route, Order, ALONG_COST, JUMP, JUMP_COST};
pub use lock::{lock_first, LockWhy};
pub use wires::{links_along_pins, perf_route_left, wire_sides, without_wires};
pub(crate) use back::back_chips;
pub(crate) use shape::facing_headers;
pub(crate) use wires::wire_added;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

/// Free holes left between two chips (and two rows of chips) for the wires: the fewest first, more when the nets do not all find a
/// way.
const GAPS: [u16; 2] = [2, 3];
/// A two-lead part's leads: this many holes apart (one hole under its body between).
const SPAN: u16 = 2;
/// A row of leads this long or longer (a header) goes along the top edge of the board.
const LONG_ROW: usize = 6;

/// Lays the schematic out on a perfboard: the headers along the top edge, the chips in rows within the board's width (`opt.cols`,
/// else about square) with room between for the wires, each two-lead part where its leads are nearest the leads of their nets, then
/// every net wired.
pub fn perf_layout_with(s: &Schematic, opt: StripOptions) -> StripLayout {
    perf_layout_keeping(s, opt, vec![], BTreeMap::new())
}

/// Lays the schematic out on a perfboard round the parts `from` has locked (`StripLayout::locked`): those stay in their holes as they
/// are, the others are placed round them (the shelves and the springs leave the locked parts' holes and room round them free) and
/// every net is wired again. A locked part no longer on the schematic, or with other pins now, is placed afresh. The sections' boxes
/// the person put by hand (`StripLayout::pinned`) keep their top-left hole and width (the parts inside placed afresh), the other boxes
/// packed round them; a box pinned for a section no longer in the table is let go.
pub fn perf_layout_around(s: &Schematic, opt: StripOptions, from: &StripLayout) -> StripLayout {
    let c = read(s);
    let keep: Vec<StripPart> = from
        .parts
        .iter()
        .filter(|p| from.locked.contains(&p.refdes))
        .filter(|p| {
            c.on.iter().find(|x| x.0 == p.refdes).is_some_and(|x| {
                let mut a: Vec<&str> = x.2.iter().map(|y| y.0.as_str()).collect();
                let mut b: Vec<&str> = p.leads.iter().map(|y| y.0.as_str()).collect();
                a.sort_unstable();
                b.sort_unstable();
                a == b
            })
        })
        .cloned()
        .collect();
    let boxes: BTreeMap<String, (At, u16)> =
        from.sections.iter().filter(|(n, _, _)| from.pinned.contains(n)).map(|(n, a, b)| (n.clone(), (*a, b.1.saturating_sub(a.1) + 1))).collect();
    let mut l = perf_layout_keeping(s, opt, keep.clone(), boxes);
    l.locked = keep.into_iter().map(|p| p.refdes).collect();
    l.pinned = from.pinned.iter().filter(|n| l.sections.iter().any(|x| &x.0 == *n)).cloned().collect();
    l
}

fn perf_layout_keeping(s: &Schematic, opt: StripOptions, keep: Vec<StripPart>, boxes: BTreeMap<String, (At, u16)>) -> StripLayout {
    let c = read(s);
    // the chips going on the copper side are placed after the others (`with_back`): the placing sees the rest only
    let backs = back_chips(s, &c, opt.chips_back);
    let full = c.clone();
    let mut c = c;
    c.on.retain(|(r, _, _)| !backs.contains_key(r));
    let mut wants = Wants::read(s, &c);
    wants.keep = keep;
    wants.boxes = boxes;
    wants.keepout = opt.keepout();
    let net_of_lead = |refdes: &str, pin: &str| lead_net(&full, refdes, pin);
    // each placement (the gaps between chips, the chips upright and turned a quarter turn, without and with annealing) wired the rails first and the small nets
    // first, without and then with a way out kept by each lead, every placement with one order before the next order (so each has its
    // turn within `STEPS`); each again with the nets left apart wired first (a few rounds), and once all are joined with the nets
    // that needed wires on top first: the one with the fewest nets left apart, then the fewest wires on top, then the smallest board
    // (what is left on the mounting holes' corners, the nets left apart, the groups past their distance, the wires on top, the board's
    // holes) and the layout
    let mut best: Option<((usize, usize, usize, usize, usize), StripLayout)> = None;
    let work = std::cell::Cell::new(0u64);
    let done = |_: &Option<((usize, usize, usize, usize, usize), StripLayout)>| work.get() > STEPS;
    // the shelves as placed, then the same made better by annealing (it helps small boards; on a crowded one the estimate it goes by
    // packs the parts too tight for the wires, and the shelves as placed win)
    // and each again with the chips where the springs and the spreading put them (`simpl`)
    // (the chips facing a header turned, the others upright: tried first, when there are such chips)
    c.facing = facing_headers(&c);
    let turns: Vec<Turn> = if c.facing.is_empty() { vec![Turn::Up, Turn::All] } else { vec![Turn::Facing, Turn::Up, Turn::All] };
    let mut placings: Vec<(u16, Turn, bool, bool)> = vec![];
    for q in [false, true] {
        for sa in [false, true] {
            for g in GAPS {
                placings.extend(turns.iter().map(|t| (g, *t, sa, q)));
            }
        }
    }
    // round locked parts the springs go first: they pull the other parts toward the locked parts' leads (the shelves fill the rows
    // after the locked parts in order, far from what they join)
    // (and with sections, the boxes packed across the board go first: the bands stacked down a crowded board run past the budget)
    if !wants.keep.is_empty() || !wants.sections.is_empty() {
        placings.sort_by_key(|p| !p.3);
    }
    // a section's box put by hand: only the boxes packed round it (the bands stacked down the board would not keep it)
    if !wants.boxes.is_empty() && wants.sections.len() >= 2 && wants.keep.is_empty() {
        placings.retain(|p| p.3);
    }
    let mut placed: Vec<Option<Option<StripLayout>>> = vec![None; placings.len()];
    for (rails_first, escapes) in [(true, false), (false, false), (true, true), (false, true)] {
        for (pi, (gap, turned, sa, springs)) in placings.iter().copied().enumerate() {
            if done(&best) {
                break;
            }
            let new = placed[pi].is_none();
            let l = placed[pi]
                .get_or_insert_with(|| {
                    if !springs {
                        return Some(place(&c, opt, gap, turned, sa, &wants, None));
                    }
                    // with sections, this placing packs each section's box across the board (the springs leave sections alone)
                    if !wants.sections.is_empty() {
                        return place_packed(&c, opt, gap, turned, sa, &wants);
                    }
                    simpl::targets(&c, opt, gap, turned, &wants).map(|t| place(&c, opt, gap, turned, sa, &wants, Some(&t)))
                })
                .clone();
            // a placing made counts as work too, by its board's holes (`PLACE_STEPS`)
            if new {
                work.set(work.get() + PLACE_STEPS * l.as_ref().map_or(0, |l| l.rows as u64 * l.cols as u64));
            }
            let Some(l) = l else { continue };
            let mut l = if backs.is_empty() { l } else { with_back(&full, l, &backs) };
            l.keepout = wants.keepout.clone();
            // the table's Kelvin forks where this placing put their leads
            let fork_at: Vec<[At; 3]> = s
                .requirements
                .measures
                .iter()
                .filter(|m| m.what == "fork" && m.pads.len() == 3 && m.op != "min")
                .filter_map(|m| Some([crate::stripboard::pad_at(&l, &m.pads[0])?, crate::stripboard::pad_at(&l, &m.pads[1])?, crate::stripboard::pad_at(&l, &m.pads[2])?]))
                .collect();
            let bypass_at = bypass_pairs(&full, &wants, &l);
            let mut first: Vec<String> = vec![];
            let mut extra = 0;
            for round in 0..ROUNDS + MORE_ROUNDS {
                if best.is_some() && work.get() > STEPS {
                    break;
                }
                let mut m = l.clone();
                route(&full, &mut m, &net_of_lead, Order { rails_first, escapes, first: &first, forks: &fork_at, along: ALONG_COST, bypass: &bypass_at, keep: false, work: &work });
                let apart = crate::stripboard::strip_unjoined(s, &m);
                let key = rank(s, &m, apart.len());
                // a placing far behind the best on its first wiring is not wired again (the rounds after gain less than that)
                if round == 0 && best.as_ref().is_some_and(|(b, _)| key.0 > b.0 || (key.0 == b.0 && (key.1 > b.1 + 1 || (key.1 >= b.1 && key.3 > b.3 + b.3 / 2 + 2)))) {
                    break;
                }
                let on_top: Vec<String> = m.links.iter().map(|k| k.net.clone()).collect();
                if best.as_ref().is_none_or(|(b, _)| key < *b) {
                    best = Some((key, m));
                }
                if apart.is_empty() {
                    if extra >= MORE_ROUNDS {
                        break;
                    }
                    extra += 1;
                    // the nets that needed wires on top, the most first, wired before the others
                    let mut count: BTreeMap<String, usize> = BTreeMap::new();
                    for n in on_top {
                        *count.entry(n).or_default() += 1;
                    }
                    let mut v: Vec<(String, usize)> = count.into_iter().collect();
                    v.sort_by_key(|x| Reverse(x.1));
                    for (n, _) in v.into_iter().rev() {
                        first.retain(|x| *x != n);
                        first.insert(0, n);
                    }
                    continue;
                }
                for (n, _, _) in apart {
                    if !first.contains(&n) {
                        first.insert(0, n);
                    }
                }
            }
        }
    }
    let mut l = best.map(|b| b.1).unwrap_or_default();
    wire_sides(&mut l);
    l
}

/// How good a wired layout is, the smaller the better: what is left on the mounting holes' corners, the nets left apart (`apart`),
/// the groups past their farthest distance and the bypass capacitors past theirs (after the nets left apart: a group kept close is no use on a board that does not work),
/// the wires on top with each one along a chip's row of pins counting three (it lies on the socket, L9), the board's holes.
fn rank(s: &Schematic, m: &StripLayout, apart: usize) -> (usize, usize, usize, usize, usize) {
    let over = crate::stripboard::group_spreads(s, m).iter().filter(|g| g.1 > g.2 + 1e-9).count()
        + crate::stripboard::bypass_spans(s, m).iter().filter(|b| b.judged() > b.limit + 1e-9).count();
    (m.in_keepout().len(), apart, over, m.links.len() + 2 * links_along_pins(m).len(), m.rows as usize * m.cols as usize)
}

/// The net of a part's lead ("" when none).
fn lead_net(c: &Circuit, refdes: &str, pin: &str) -> String {
    c.on.iter().find(|x| x.0 == refdes).and_then(|x| x.2.iter().find(|(p, _)| p == pin)).map(|x| x.1.clone()).unwrap_or_default()
}

/// How much work the tries do before they stop, the best so far kept: the steps of the wiring's search (`Order::work`) and each
/// placing made (`PLACE_STEPS` a hole of its board). A big board does not hold the screen up long (about 1.5 s on a 2026 PC, where
/// about 2.5 million steps a second are taken; 147 of the 152 samples finish every try before it). Counted in work, not in time, so a
/// schematic is laid out the same on every computer and however busy it is.
const STEPS: u64 = 3_700_000;
/// What a placing made counts for, a hole of its board (the shelves, the springs and the annealing take about as long as 200 steps
/// of the wiring's search a hole, the median of the samples).
const PLACE_STEPS: u64 = 200;
/// Once every net is joined, how many times more the nets are wired with the ones that needed wires on top first (fewer of them).
const MORE_ROUNDS: usize = 3;

/// How many times the nets are wired again with the ones left apart first.
const ROUNDS: usize = 4;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stripboard::{strip_check, strip_layout_with};

    fn load(name: &str) -> Schematic {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// With `chips_back`, the 0805 / 1206 resistors and capacitors go on the copper side across two holes side by side, one end on a
    /// hole of its net where it can (on a chip's pin); the board checks clean, every net is joined, and it is smaller.
    #[test]
    fn chips_on_the_copper_side_sit_across_two_holes_and_the_board_shrinks() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/attiny_blinker.schjson");
        let s: Schematic = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let front = crate::test_layouts::perf(&s);
        let l = perf_layout_with(&s, StripOptions { chips_back: true, ..Default::default() });
        assert!(crate::stripboard::strip_check(&s, &l).is_empty(), "{:?}", crate::stripboard::strip_check(&s, &l));
        assert!(crate::stripboard::strip_unjoined(&s, &l).is_empty());
        let backs: Vec<&StripPart> = l.parts.iter().filter(|p| matches!(p.mount, crate::mount::Mount::Back { .. })).collect();
        assert!(!backs.is_empty(), "{:?}", l.parts);
        assert!(front.parts.iter().all(|p| !matches!(p.mount, crate::mount::Mount::Back { .. })), "off by default");
        for p in &backs {
            let (a, b) = (p.leads[0].1, p.leads[1].1);
            assert_eq!(a.0.abs_diff(b.0) + a.1.abs_diff(b.1), 1, "{} across two holes side by side", p.refdes);
        }
        let shared = backs.iter().any(|p| p.leads.iter().any(|(_, h)| l.parts.iter().any(|q| q.refdes != p.refdes && q.leads.iter().any(|(_, g)| g == h))));
        assert!(shared, "an end on a pin's hole");
        assert!((l.rows as u32) * (l.cols as u32) < (front.rows as u32) * (front.cols as u32), "{}x{} < {}x{}", l.cols, l.rows, front.cols, front.rows);
    }

    /// The river: six nets from a header's row to a chip's top row below it, two holes to the right, are wired as Ls on rows of
    /// their own, no hole used by two nets, and each net's wires join its two leads; with too few rows between, the rest are left.
    #[test]
    fn a_header_and_the_chip_row_under_it_are_wired_as_nested_ls() {
        let part = |r: &str, look, leads: Vec<(String, At)>| StripPart { refdes: r.into(), look, leads, mount: Default::default() };
        let l = StripLayout {
            rows: 10,
            cols: 10,
            perf: true,
            parts: vec![
                part("J1", Look::Row, (0..6).map(|k| (format!("{}", k + 1), (0, k))).collect()),
                part("U1", Look::Dip, (0..6).map(|k| (format!("{}", k + 1), (7, k + 2))).chain((0..6).map(|k| (format!("{}", 12 - k), (9, k + 2)))).collect()),
            ],
            ..Default::default()
        };
        // nets 0..5: J1.k to U1's top row pin k; U1's bottom row on nets of their own
        let mut leads: Vec<(At, u32)> = (0..6).map(|k| ((0, k), k as u32)).collect();
        leads.extend((0..6).map(|k| ((7, k + 2), k as u32)));
        leads.extend((0..6).map(|k| ((9, k + 2), 10 + k as u32)));
        let mut owner = vec![None; 100];
        for (a, n) in &leads {
            owner[a.0 as usize * 10 + a.1 as usize] = Some(*n);
        }
        let wired = river(&l, &leads, &owner, &BTreeMap::new());
        assert_eq!(wired.len(), 6, "{wired:?}");
        // with a row fewer between them (5 rows, 6 nets all overlapping), the last one is left to the router
        let mut tight = l.clone();
        for (_, a) in &mut tight.parts[1].leads {
            if a.0 == 7 {
                a.0 = 6;
            }
        }
        let tl: Vec<(At, u32)> = leads.iter().map(|(a, n)| (if a.0 == 7 { (6, a.1) } else { *a }, *n)).collect();
        let mut to = vec![None; 100];
        for (a, n) in &tl {
            to[a.0 as usize * 10 + a.1 as usize] = Some(*n);
        }
        assert_eq!(river(&tight, &tl, &to, &BTreeMap::new()).len(), 5);
        let mut seen: BTreeMap<At, u32> = BTreeMap::new();
        for (net, segs) in &wired {
            for (a, b) in segs {
                assert!(a.0 == b.0 || a.1 == b.1, "straight");
                for h in along(*a, *b) {
                    assert!(seen.insert(h, *net).is_none_or(|n| n == *net), "{h:?} used by two nets");
                }
            }
            let ends: BTreeSet<At> = segs.iter().flat_map(|(a, b)| [*a, *b]).collect();
            assert!(ends.contains(&(0, *net as u16)) && ends.contains(&(7, *net as u16 + 2)), "net {net} joins its leads: {segs:?}");
        }
    }

    /// A sample laid out on a perfboard (`opt`): it joins what its schematic joins and nothing more; every bare wire runs straight
    /// along a row, a column or at 45 degrees; no lead or wire's end shares a hole; all on the board. With the layout, the stripboard's holes.
    fn laid_out(name: &str, opt: StripOptions) -> (StripLayout, u32) {
        let s = load(name);
        let l = crate::test_layouts::perf_with(&s, opt);
        assert!(l.perf);
        assert!(strip_check(&s, &l).is_empty(), "{name}: {:?}", strip_check(&s, &l));
        assert!(l.bare.iter().all(|(a, b)| crate::stripboard::bare_way(*a, *b)), "{name}: straight wires (along a row, a column or at 45 degrees)");
        assert!(l.used().iter().all(|(a, _)| a.0 < l.rows && a.1 < l.cols), "{name}: on the board");
        let st = crate::test_layouts::strip(&s);
        (l, st.rows as u32 * st.cols as u32)
    }

    /// Samples laid out on a perfboard as drawn (`laid_out`), on boards far smaller than the stripboard's, with few wires on top.
    #[test]
    fn samples_lay_out_on_a_perfboard_as_drawn_and_small() {
        let (mut perf, mut strip, mut on_top) = (0u32, 0u32, 0usize);
        for name in ["ne555_blinker.schjson", "active_filter.schjson", "current_sensors.schjson"] {
            let (l, st) = laid_out(name, StripOptions::default());
            on_top += l.links.len();
            perf += l.rows as u32 * l.cols as u32;
            strip += st;
        }
        // the holes: 369 with the shelves alone, 442 since `simpl` (its boards are bigger, about ten holes for each wire on top fewer)
        assert!(perf * 3 < strip * 2 && perf <= 460, "{perf} holes on perfboards, {strip} on stripboards");
        // the wires on top (these three with the seven-segment counter): 41 before a way's jumps were one wire and the nets that needed
        // them were wired first (2026-11-20), 32 since, 30 with the chips also tried turned a quarter turn, 27 with the placement also
        // made better by annealing, 19 with the chips also placed by the springs and the spreading (`simpl`, 2026-11-21): these three
        // 8 → 2
        assert!(on_top <= 3, "{on_top} wires on top");
    }

    /// The seven-segment counter (on its own: it takes the most time) as drawn, on a board far smaller than the stripboard's (60 × 22
    /// holes on strips, 21 × 18 here), with few wires on top (19 → 17 with `simpl`); and a width to keep to kept.
    #[test]
    fn the_seven_segment_counter_on_a_perfboard() {
        let (l, strip) = laid_out("seg7_counter.schjson", StripOptions::default());
        assert!(l.rows as u32 * l.cols as u32 * 2 < strip, "{} × {} holes", l.cols, l.rows);
        assert!(l.links.len() <= 18, "{} wires on top", l.links.len());
        let s = load("seg7_counter.schjson");
        assert!(!strip_layout_with(&s, StripOptions::default()).perf, "the stripboard keeps its strips");
    }

    /// L10: a group with a farthest distance (`max=`) is judged on the board (`strip_rules`), and the layout keeps it: the two parts
    /// farthest apart in the 555 blinker as laid out without the rule end up within half that distance.
    #[test]
    fn a_groups_farthest_distance_is_judged_and_kept() {
        let mut s = load("ne555_blinker.schjson");
        let free = crate::test_layouts::perf(&s);
        let centre = |p: &StripPart| {
            let n = p.leads.len() as f64;
            (p.leads.iter().map(|x| x.1 .0 as f64).sum::<f64>() / n, p.leads.iter().map(|x| x.1 .1 as f64).sum::<f64>() / n)
        };
        let (mut far, mut pair) = (0.0, (String::new(), String::new()));
        for x in &free.parts {
            for y in &free.parts {
                let (a, b) = (centre(x), centre(y));
                let d = (a.0 - b.0).hypot(a.1 - b.1) * 2.54;
                if d > far {
                    (far, pair) = (d, (x.refdes.clone(), y.refdes.clone()));
                }
            }
        }
        let max = (far / 2.0).max(8.0);
        s.requirements.groups = vec![design_spec::PlaceGroup { name: "近く".into(), parts: vec![pair.0.clone(), pair.1.clone()], max_dist: max, ..Default::default() }];
        // judged on the layout made without it
        let before = crate::stripboard::strip_rules(&s, &free);
        assert!(before.iter().any(|r| !r.ok && r.message.starts_with("「近く」を") && r.message.contains("以内に集める") && r.parts.contains(&pair.0) && r.parts.contains(&pair.1)), "{before:?}");
        // kept by the layout made with it
        let l = crate::test_layouts::perf(&s);
        assert!(strip_check(&s, &l).is_empty() && crate::stripboard::strip_unjoined(&s, &l).is_empty());
        let after = crate::stripboard::group_spreads(&s, &l);
        assert!(after[0].1 <= max, "{pair:?}: {far:.1} mm alone, {:.1} mm with the rule (max {max:.1})", after[0].1);
        assert!(crate::stripboard::strip_rules(&s, &l).iter().any(|r| r.ok && r.message.starts_with("「近く」を") && r.message.contains("できています")));
    }

    /// L10, across sections: a group with a farthest distance whose parts are in two sections (the two op amps of the active filter, one
    /// in each) is laid out with the two halves facing each other across the sections' boundary (`Wants::last`/`early`, and the boxes'
    /// mix with the least past the distance): within the distance, where without it they are further apart.
    #[test]
    fn a_group_across_two_sections_is_brought_together() {
        let mut s = load("active_filter.schjson");
        let group = |name: &str, parts: &[&str]| design_spec::PlaceGroup { name: name.into(), parts: parts.iter().map(|p| p.to_string()).collect(), section: true, ..Default::default() };
        s.requirements.groups = vec![group("前段", &["U1", "R1", "R2", "C1", "C2"]), group("後段", &["U2", "R3", "R4", "C3", "C4"])];
        s.requirements.groups.push(design_spec::PlaceGroup { name: "近く".into(), parts: vec!["U1".into(), "U2".into()], ..Default::default() });
        let spread = |s: &Schematic| {
            let l = perf_layout_with(s, StripOptions::default());
            assert!(strip_check(s, &l).is_empty());
            let mut t = s.clone();
            t.requirements.groups[2].max_dist = 1.0;
            crate::stripboard::group_spreads(&t, &l)[0].1
        };
        let free = spread(&s);
        s.requirements.groups[2].max_dist = 25.0;
        let kept = spread(&s);
        assert!(kept <= 25.0 && free > 25.0, "{free:.1} mm without the distance, {kept:.1} mm with it");
    }

    /// L9: the wires on top along a chip's row of pins (lying on its socket) are fewer: of the same wired layouts of the Arduino Mega
    /// board, the one `rank` keeps has fewer (37) than the one kept by the wires on top alone (49). (The whole layout stops at a time budget, so
    /// how many layouts it tries, and its count, depends on the machine: 40 here, 49 on CI's slower runners, 50 before.)
    #[test]
    fn fewer_wires_on_top_run_along_a_row_of_pins() {
        let s = load("arduino_mega_usb.schjson");
        let c = read(&s);
        let wants = Wants::read(&s, &c);
        let net_of_lead = |r: &str, pin: &str| lead_net(&c, r, pin);
        let work = std::cell::Cell::new(0u64);
        let mut tried: Vec<StripLayout> = vec![];
        for turned in [Turn::Up, Turn::All] {
            let placed = place(&c, StripOptions::default(), GAPS[0], turned, false, &wants, None);
            for (rails_first, escapes) in [(true, false), (false, false), (true, true), (false, true)] {
                let mut m = placed.clone();
                route(&c, &mut m, &net_of_lead, Order { rails_first, escapes, first: &[], forks: &[], along: ALONG_COST, bypass: &[], keep: false, work: &work });
                tried.push(m);
            }
        }
        let apart = |m: &StripLayout| crate::stripboard::strip_unjoined(&s, m).len();
        let kept = tried.iter().min_by_key(|m| rank(&s, m, apart(m))).unwrap();
        let by_wires = tried.iter().min_by_key(|m| (apart(m), m.links.len(), m.rows as usize * m.cols as usize)).unwrap();
        let n = |l: &StripLayout| links_along_pins(l).len();
        assert!(n(kept) < n(by_wires) && apart(kept) == apart(by_wires), "{} along a row of pins in the layout kept, {} in the one with the fewest wires", n(kept), n(by_wires));
    }

    /// L11: with mounting holes in the corners of a board of a given size, the layout leaves the 3 × 3 holes of each corner free (no
    /// lead, no wire's end, no bare wire) and still joins what the schematic joins.
    #[test]
    fn the_mounting_holes_corners_are_kept_free() {
        for name in ["attiny_blinker.schjson", "active_filter.schjson"] {
            let opt = StripOptions { cols: Some(24), rows: Some(20), mount_holes: true, ..Default::default() };
            assert_eq!(opt.keepout().len(), 36);
            let (l, _) = laid_out(name, opt);
            assert_eq!(l.keepout, opt.keepout());
            assert!(l.in_keepout().is_empty(), "{name}: {:?}", l.in_keepout());
        }
        // only the top corners without the height
        assert_eq!(StripOptions { cols: Some(24), mount_holes: true, ..Default::default() }.keepout().len(), 18);
        assert!(StripOptions { cols: Some(24), ..Default::default() }.keepout().is_empty());
    }

    /// The seven-segment counter on a board 16 holes wide: kept to.
    #[test]
    fn a_width_to_keep_to_is_kept() {
        let (l, _) = laid_out("seg7_counter.schjson", StripOptions { cols: Some(16), ..Default::default() });
        assert!(l.cols <= 16, "{} wide", l.cols);
    }

    /// A locked part stays where it is and the others are laid out round it: the 555 moved down and right of where the layout put it
    /// and locked; laid out again, it is in the same holes, no other part takes a hole of its outline, the board joins what the
    /// schematic joins, and the lock is kept on the layout.
    #[test]
    fn a_locked_part_stays_and_the_rest_goes_round_it() {
        let s = load("ne555_blinker.schjson");
        let mut l = crate::test_layouts::perf(&s);
        let u = l.parts.iter_mut().find(|p| p.refdes == "U1").unwrap();
        for (_, a) in u.leads.iter_mut() {
            *a = (a.0 + 6, a.1 + 8);
        }
        let held = u.leads.clone();
        l.locked.insert("U1".into());
        let m = perf_layout_around(&s, StripOptions::default(), &l);
        assert_eq!(m.parts.iter().find(|p| p.refdes == "U1").unwrap().leads, held, "U1 stays");
        let taken: BTreeSet<At> = outline(m.parts.iter().find(|p| p.refdes == "U1").unwrap()).into_iter().collect();
        for p in m.parts.iter().filter(|p| p.refdes != "U1") {
            assert!(outline(p).iter().all(|h| !taken.contains(h)), "{} on U1", p.refdes);
        }
        assert!(strip_check(&s, &m).is_empty(), "{:?}", strip_check(&s, &m));
        assert!(m.locked.contains("U1"));
    }

    /// A wire on top along a chip's row of pins is found (and named by the chip); one across the row, or under the chip, is not.
    #[test]
    fn a_wire_along_a_chips_pins_is_found() {
        let s = load("ne555_blinker.schjson");
        let mut l = crate::test_layouts::perf(&s);
        let u = l.parts.iter().find(|p| p.refdes == "U1").unwrap().clone();
        let (r0, r1) = (u.leads.iter().map(|x| x.1 .0).min().unwrap(), u.leads.iter().map(|x| x.1 .0).max().unwrap());
        let (c0, c1) = (u.leads.iter().map(|x| x.1 .1).min().unwrap(), u.leads.iter().map(|x| x.1 .1).max().unwrap());
        // its pins in two columns (upright) or two rows (turned): a wire past both ends of one of them runs along four pins; one
        // across the two, over a pin of each, does not
        let upright = u.leads.iter().map(|x| x.1 .1).collect::<BTreeSet<u16>>().len() == 2;
        let before = links_along_pins(&l).len();
        let k = l.links.len();
        let (along, across) = if upright {
            (Link::new((r0.saturating_sub(1), c0), (r1 + 1, c0), "X"), Link::new((r0, c0.saturating_sub(1)), (r0, c1 + 1), "Y"))
        } else {
            (Link::new((r0, c0.saturating_sub(1)), (r0, c1 + 1), "X"), Link::new((r0.saturating_sub(1), c0), (r1 + 1, c0), "Y"))
        };
        l.links.push(along);
        l.links.push(across);
        let found = links_along_pins(&l);
        assert_eq!(found.len(), before + 1, "{found:?}");
        assert!(found.contains(&(k, "U1".to_string())));
    }

    /// A Kelvin fork from the requirements table is kept on a perfboard: on the active filter, the ways from R4.1 to R3.2 and to C3.1
    /// (on a stripboard both go off the same side of the lead and run together) are drawn each from the lead, and part there.
    #[test]
    fn a_kelvin_fork_parts_at_the_lead_on_a_perfboard() {
        let mut s = load("active_filter.schjson");
        s.requirements.measures.push(design_spec::Measure { name: "検出".into(), what: "fork".into(), pads: ["R4.1", "R3.2", "C3.1"].map(String::from).to_vec(), op: "max".into(), limit: 0.5, ..Default::default() });
        let l = crate::test_layouts::perf(&s);
        assert!(strip_check(&s, &l).is_empty(), "{:?}", strip_check(&s, &l));
        let rules = crate::stripboard::strip_rules(&s, &l);
        assert!(rules.iter().any(|r| r.ok && r.message.starts_with("ケルビン「検出」")), "{rules:?}");
    }

    /// An Arduino Nano goes in two female headers 0.6 inch apart (six holes between its rows), on a perfboard and a stripboard, and
    /// the boards join what the schematic joins.
    #[test]
    fn an_arduino_nano_sits_in_female_headers() {
        let s = load("arduino_blink.schjson");
        for l in [crate::test_layouts::perf(&s), crate::test_layouts::strip(&s)] {
            let u = l.parts.iter().find(|p| p.refdes == "U1").unwrap();
            assert!(matches!(u.mount, crate::mount::Mount::Module { width: 6, pins: 30, .. }), "{:?}", u.mount);
            let at = |pin: &str| u.leads.iter().find(|x| x.0 == pin).unwrap().1;
            let (a, b) = (at("1"), at("30"));
            assert_eq!(a.0.abs_diff(b.0) + a.1.abs_diff(b.1), 6, "pin 1 and pin 30 across the module: {a:?} {b:?}");
            assert!(strip_check(&s, &l).is_empty(), "{} {:?}", l.perf, strip_check(&s, &l));
        }
    }

    /// The shelves as placed wall no lead in: every lead keeps a free hole beside it for its wire (arduino_mega_usb's USB connector,
    /// a row of five pins, had two-lead parts put right above and right below its middle pin, and its net could not be wired). The
    /// annealing may still do it (keeping it from that made the samples' wires on top 265 → 276); a placing that leaves a net apart
    /// loses to the others.
    #[test]
    fn no_lead_is_walled_in() {
        let s = load("arduino_mega_usb.schjson");
        let c = read(&s);
        let wants = Wants::read(&s, &c);
        for turned in [Turn::Up, Turn::All] {
            let l = place(&c, StripOptions::default(), GAPS[0], turned, false, &wants, None);
            let mut taken: BTreeSet<At> = l.off.iter().flat_map(|(_, v)| v.iter().map(|w| w.2)).collect();
            for p in &l.parts {
                let (r0, r1) = (p.leads.iter().map(|x| x.1 .0).min().unwrap(), p.leads.iter().map(|x| x.1 .0).max().unwrap());
                let (c0, c1) = (p.leads.iter().map(|x| x.1 .1).min().unwrap(), p.leads.iter().map(|x| x.1 .1).max().unwrap());
                taken.extend((r0..=r1).flat_map(|r| (c0..=c1).map(move |cc| (r, cc))));
            }
            for p in &l.parts {
                for (pin, a) in &p.leads {
                    assert!(beside4(*a, l.cols).any(|n| !taken.contains(&n)), "turned {turned:?}: {}.{pin} at {a:?} walled in", p.refdes);
                }
            }
        }
    }

    /// The requirements table on a perfboard: two sections, each in a band of rows of its own with all its parts in it, ground
    /// wired within each and back to the first section's by one wire on top; and a pair of chips kept apart by a distance placed
    /// that far apart. The check and the rules both pass.
    #[test]
    fn sections_and_distances_from_the_table() {
        let mut s = load("seg7_counter.schjson");
        let group = |name: &str, parts: &[&str]| design_spec::PlaceGroup { name: name.into(), parts: parts.iter().map(|p| p.to_string()).collect(), section: true, ..Default::default() };
        s.requirements.groups = vec![group("カウンタ", &["U1", "U3"]), group("表示", &["U2", "DS1", "R1", "R2", "R3", "R4", "R5", "R6", "R7"])];
        s.requirements.apart = vec![design_spec::KeepApart { name: "離す".into(), a: vec!["U1".into()], b: vec!["U2".into()], min_dist: 25.0, ..Default::default() }];
        let l = crate::test_layouts::perf(&s);
        assert!(strip_check(&s, &l).is_empty(), "{:?}", strip_check(&s, &l));
        assert_eq!(l.sections.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["カウンタ", "表示"]);
        for (k, (name, p, q)) in l.sections.iter().enumerate() {
            let members = &s.requirements.groups[k].parts;
            for part in l.parts.iter().filter(|x| members.contains(&x.refdes)) {
                assert!(part.leads.iter().all(|(_, a)| a.0 >= p.0 && a.0 <= q.0 && a.1 >= p.1 && a.1 <= q.1), "{} in {name}", part.refdes);
            }
            // the sections' boxes (bands, or boxes packed side by side) do not overlap
            for (other, r, s) in l.sections.iter().skip(k + 1) {
                assert!(q.0 < r.0 || s.0 < p.0 || q.1 < r.1 || s.1 < p.1, "{name} and {other} overlap");
            }
        }
        let rules = crate::stripboard::strip_rules(&s, &l);
        assert!(!rules.is_empty() && rules.iter().all(|r| r.ok), "{rules:?}");
        assert!(rules.iter().any(|r| r.message.starts_with("GND:")), "the one ground point is checked: {rules:?}");
    }

    /// A section's box put by hand: laid out again round it, the box keeps its top-left hole (and is no wider than it was), its parts
    /// inside it, the other box beside or below it (not over it), still marked as put, and the board joins what the schematic joins.
    #[test]
    fn a_section_box_put_by_hand_stays_and_the_others_go_round_it() {
        let mut s = load("seg7_counter.schjson");
        let group = |name: &str, parts: &[&str]| design_spec::PlaceGroup { name: name.into(), parts: parts.iter().map(|p| p.to_string()).collect(), section: true, ..Default::default() };
        s.requirements.groups = vec![group("カウンタ", &["U1", "U3"]), group("表示", &["U2", "DS1", "R1", "R2", "R3", "R4", "R5", "R6", "R7"])];
        let from = StripLayout { perf: true, sections: vec![("表示".into(), (0, 20), (11, 35))], pinned: ["表示".to_string()].into(), ..Default::default() };
        let l = perf_layout_around(&s, StripOptions::default(), &from);
        assert!(strip_check(&s, &l).is_empty(), "{:?}", strip_check(&s, &l));
        assert!(l.pinned.contains("表示"), "still put by hand");
        let (_, p, q) = l.sections.iter().find(|x| x.0 == "表示").unwrap();
        assert_eq!(*p, (0, 20), "the box keeps its top-left hole");
        assert!(q.1 - p.1 < 16, "no wider than it was: {p:?}–{q:?}");
        for part in l.parts.iter().filter(|x| s.requirements.groups[1].parts.contains(&x.refdes)) {
            assert!(part.leads.iter().all(|(_, a)| a.0 >= p.0 && a.0 <= q.0 && a.1 >= p.1 && a.1 <= q.1), "{} in its box", part.refdes);
        }
        let (_, r, t) = l.sections.iter().find(|x| x.0 == "カウンタ").unwrap();
        assert!(q.0 < r.0 || t.0 < p.0 || q.1 < r.1 || t.1 < p.1, "the boxes do not overlap: {p:?}–{q:?}, {r:?}–{t:?}");
    }

    /// The parts to place by hand first: the connector, the switch, the relay and the big chip (in that order), not the small chip,
    /// the resistor or a part already locked.
    #[test]
    fn the_parts_to_place_by_hand_first() {
        let part = |r: &str, look: Look, n: u16| StripPart { refdes: r.into(), look, leads: (0..n).map(|k| (format!("{}", k + 1), (k, 0))).collect(), mount: Default::default() };
        let mut l = StripLayout {
            perf: true,
            parts: vec![part("R1", Look::TwoLead, 2), part("U1", Look::Dip, 16), part("U2", Look::Dip, 8), part("K1", Look::Dip, 8), part("SW1", Look::Row, 3), part("J1", Look::Row, 4), part("U3", Look::Dip, 14)],
            ..Default::default()
        };
        assert_eq!(
            lock_first(&l),
            [("J1".into(), LockWhy::Connector), ("SW1".into(), LockWhy::Control), ("K1".into(), LockWhy::Relay), ("U1".into(), LockWhy::BigChip), ("U3".into(), LockWhy::BigChip)]
        );
        l.locked.insert("J1".into());
        assert!(lock_first(&l).iter().all(|x| x.0 != "J1"), "a locked part is not listed");
    }

    /// With no strips every hole is a piece of its own: two leads side by side are apart until a bare wire joins them; a wire that
    /// meets another in the middle (a T) or crosses it touches it, and one beside it does not.
    #[test]
    fn holes_join_only_by_wires() {
        let part = |r: &str, a: At, b: At| StripPart { refdes: r.into(), look: Look::TwoLead, leads: vec![("1".into(), a), ("2".into(), b)], mount: Default::default() };
        let mut l = StripLayout { perf: true, rows: 6, cols: 6, parts: vec![part("R1", (0, 0), (0, 1)), part("R2", (4, 0), (4, 4))], ..Default::default() };
        assert_eq!(l.pieces().len(), 36);
        assert!(l.joins().is_empty());
        // a wire along row 2, and one down column 2 from R1's row to it (a T): R1.2's hole is not on them, R1.1's neither
        l.bare = vec![((2, 0), (2, 4)), ((0, 2), (2, 2))];
        let touches = |l: &StripLayout, a: At, b: At| l.bare_joins().iter().any(|(x, y)| (*x == a && *y == b) || (*x == b && *y == a));
        assert!(touches(&l, (0, 2), (2, 2)) && touches(&l, (2, 0), (2, 2)), "the T joins the two wires");
        // a wire beside it (row 3) touches nothing
        l.bare.push(((3, 0), (3, 4)));
        assert!(!l.bare_joins().iter().any(|(x, y)| (x.0 == 3) != (y.0 == 3)), "a wire beside another does not touch it");
        // down to R2's lead at (4, 4): the lead's hole joins
        l.bare.push(((2, 4), (4, 4)));
        assert!(touches(&l, (2, 4), (4, 4)));
    }
}
