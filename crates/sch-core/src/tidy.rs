//! Tidying a schematic (candidate N3): the parts laid out again so the sheet reads like a hand-drawn one: the signal flows from
//! left to right (each part in a column by how many signal nets it is from an input: a source, a connector), the parts of a column
//! ordered after the ones they join in the column before (fewer crossings), each two- or three-lead part turned so its supply pin is
//! on top, its ground pin at the bottom and its signal pins toward the parts they join. Made for the sheets `ccad` and the MCP
//! build (every part in a row, joined by net labels) and the ones read from KiCad and LTspice.
//!
//! The joints are drawn again as the `ccad` sheets draw them: a label with the net's name on each pin, a ground symbol on a ground
//! pin at the bottom of its part, a power symbol on a supply pin at its top. The nets never change: the pins' grouping into nets
//! before and after is compared, and a tidy that would change it is not made. Either the whole sheet or only some parts (the
//! others stay where they are; the chosen ones go below them).

use crate::{extract, on_segment, Component, Kind, Label, Pt, Schematic, Wire};
use std::collections::{BTreeMap, BTreeSet};

/// What a tidy did.
#[derive(Clone, Debug, PartialEq)]
pub struct Tidied {
    pub moved: usize,
    pub columns: usize,
    /// The nets drawn as a wire between their two pins (instead of a label on each).
    pub wired: usize,
}

/// The pins' grouping into nets (the parts' pins only; power and ground symbols are drawn again).
fn grouping(s: &Schematic) -> BTreeSet<Vec<(String, String)>> {
    let power: BTreeSet<&str> = s.components.iter().filter(|c| c.kind.is_power_symbol()).map(|c| c.refdes.as_str()).collect();
    extract(s)
        .into_iter()
        .map(|n| {
            let mut v: Vec<(String, String)> = n.pins.iter().filter(|p| !power.contains(p.refdes.as_str())).map(|p| (p.refdes.clone(), p.pin.clone())).collect();
            v.sort();
            v
        })
        .filter(|v| !v.is_empty())
        .collect()
}

/// A part's box at turn `rot` around its position: the pins and the body (grid units, relative to the position).
fn part_box(c: &Component, rot: u8) -> (i32, i32, i32, i32) {
    let mut t = c.clone();
    t.rot = rot;
    t.pos = (0, 0);
    let (hw, hh) = match &c.custom {
        Some(sym) if !sym.is_two_lead_glyph() => {
            let l = sym.layout();
            (l.hw, l.hh)
        }
        _ => (2, 1),
    };
    let body = [(-hw, -hh), (hw, hh)].map(|p| crate::rotate(p, rot));
    let pts = t.pins().into_iter().map(|p| p.1).chain(body);
    pts.fold((i32::MAX, i32::MAX, i32::MIN, i32::MIN), |b, (x, y)| (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y)))
}

/// Whether a part may be turned: the basic two- and three-lead parts and the library parts drawn as them (a chip keeps its symbol
/// the way it was drawn, inputs on the left).
fn turnable(c: &Component) -> bool {
    match &c.custom {
        Some(sym) => sym.is_two_lead_glyph() || sym.pins.len() <= 3,
        None => !matches!(c.kind, Kind::Header(_)),
    }
}

/// The sheet with its parts (all, or those in `only`) laid out again; Err when it cannot be tidied (a child sheet's ports and buses
/// keep their places, so a sheet with them is left alone) or the nets would change.
pub fn tidy(s: &Schematic, only: Option<&BTreeSet<String>>) -> Result<(Schematic, Tidied), String> {
    // the wires are checked like the rest; should one join what it must not, the sheet is drawn with labels only
    tidy_with(s, only, true).or_else(|_| tidy_with(s, only, false))
}

/// Does the open inside of box `r` (x0, y0, x1, y1) meet the level or upright segment `a`-`b`?
fn crosses_box(a: Pt, b: Pt, r: (i32, i32, i32, i32)) -> bool {
    let (x0, x1, y0, y1) = (a.0.min(b.0), a.0.max(b.0), a.1.min(b.1), a.1.max(b.1));
    let across = |lo: i32, hi: i32, r0: i32, r1: i32| if lo == hi { r0 < lo && lo < r1 } else { hi > r0 && lo < r1 };
    across(x0, x1, r.0, r.2) && across(y0, y1, r.1, r.3)
}

/// The ways a wire may go from pin `a` to pin `b`, the plainest first: straight, a step half way across, an L, a step half way down.
fn ways(a: Pt, b: Pt) -> Vec<Vec<Pt>> {
    let (a, b) = if a <= b { (a, b) } else { (b, a) };
    if a.0 == b.0 || a.1 == b.1 {
        return vec![vec![a, b]];
    }
    let mut v: Vec<Vec<Pt>> = vec![];
    // the step across, from the middle outward
    let mid = (a.0 + b.0) / 2;
    for d in 0..=(b.0 - a.0) / 2 {
        for x in [mid - d, mid + d] {
            if x > a.0 && x < b.0 && !v.iter().any(|w| w[1].0 == x) {
                v.push(vec![a, (x, a.1), (x, b.1), b]);
            }
        }
    }
    v.push(vec![a, (b.0, a.1), b]);
    v.push(vec![a, (a.0, b.1), b]);
    let (top, bottom) = (a.1.min(b.1), a.1.max(b.1));
    let mid = (top + bottom) / 2;
    for d in 0..=(bottom - top) / 2 {
        for y in [mid - d, mid + d] {
            if y > top && y < bottom && !v.iter().any(|w| w.len() == 4 && w[1].0 == a.0 && w[1].1 == y) {
                v.push(vec![a, (a.0, y), (b.0, y), b]);
            }
        }
    }
    v
}

fn tidy_with(s: &Schematic, only: Option<&BTreeSet<String>>, wires: bool) -> Result<(Schematic, Tidied), String> {
    if !s.ports.is_empty() || !s.buses.is_empty() {
        return Err("ポートかバスのあるシートは、まだ整えられません (位置でつながっているので)".into());
    }
    let before = grouping(s);
    let nets = extract(s);
    // a source's net without a name is a signal (a symbol would give it the made-up name)
    let supplies: BTreeSet<String> = crate::breadboard::read(s).supplies.into_iter().filter(|n| !n.starts_with("N$")).collect();
    let is_ground = |n: &str| crate::breadboard::is_ground_name(n);
    let net_of: BTreeMap<(String, String), String> = nets.iter().flat_map(|n| n.pins.iter().map(move |p| ((p.refdes.clone(), p.pin.clone()), n.name.clone()))).collect();
    let chosen = |c: &Component| !c.kind.is_power_symbol() && only.is_none_or(|o| o.contains(&c.refdes));
    let parts: Vec<usize> = (0..s.components.len()).filter(|i| chosen(&s.components[*i])).collect();
    if parts.is_empty() {
        return Err("整える部品がありません".into());
    }
    let pins_of = |c: &Component| -> Vec<(String, String)> { c.pins().into_iter().filter_map(|(p, _)| net_of.get(&(c.refdes.clone(), p.clone())).map(|n| (p, n.clone()))).collect() };
    let signal = |n: &str| !is_ground(n) && !supplies.contains(n);
    // the parts each signal net joins
    let mut on_net: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for &i in &parts {
        for (_, n) in pins_of(&s.components[i]) {
            if !is_ground(&n) {
                let v = on_net.entry(n).or_default();
                if !v.contains(&i) {
                    v.push(i);
                }
            }
        }
    }
    // the parts a part joins by its signal nets (a source also by the supply it gives: the parts it feeds come after it)
    let source = |i: usize| matches!(s.components[i].kind, Kind::VSource | Kind::ISource);
    let neighbours = |i: usize| -> Vec<usize> {
        let mut v: Vec<usize> = pins_of(&s.components[i]).iter().filter(|(_, n)| signal(n) || (source(i) && !is_ground(n))).flat_map(|(_, n)| on_net.get(n).cloned().unwrap_or_default()).filter(|j| *j != i && (!source(*j) || !source(i))).collect();
        v.sort();
        v.dedup();
        v
    };
    // the inputs: the sources and the connectors (a source of a supply too: it goes first, on top)
    let input = |c: &Component| matches!(c.kind, Kind::VSource | Kind::ISource | Kind::Header(_)) || c.refdes.starts_with('J') || c.refdes.starts_with('P') && c.kind == Kind::Custom && c.custom.as_ref().is_some_and(|x| x.prefix == "P");
    // columns by distance from an input over the signal nets; a group of parts no input reaches starts from its first part
    let mut col: BTreeMap<usize, usize> = BTreeMap::new();
    let mut queue: std::collections::VecDeque<usize> = parts.iter().copied().filter(|i| input(&s.components[*i])).collect();
    for &i in &queue {
        col.insert(i, 0);
    }
    loop {
        while let Some(i) = queue.pop_front() {
            for j in neighbours(i) {
                if !col.contains_key(&j) {
                    col.insert(j, col[&i] + 1);
                    queue.push_back(j);
                }
            }
        }
        match parts.iter().find(|i| !col.contains_key(i)) {
            Some(&i) => {
                col.insert(i, 0);
                queue.push_back(i);
            }
            None => break,
        }
    }
    let columns = col.values().max().map_or(0, |m| m + 1);
    // each column ordered after its neighbours in the column before (two sweeps)
    let mut order: Vec<Vec<usize>> = vec![vec![]; columns];
    for &i in &parts {
        order[col[&i]].push(i);
    }
    let mut row_of: BTreeMap<usize, f64> = BTreeMap::new();
    for _ in 0..2 {
        for (k, column) in order.iter_mut().enumerate() {
            for (r, i) in column.iter().enumerate() {
                row_of.entry(*i).or_insert(r as f64);
            }
            if k == 0 {
                continue;
            }
            let key = |i: usize| -> f64 {
                let v: Vec<f64> = neighbours(i).iter().filter(|j| col[j] < k).filter_map(|j| row_of.get(j).copied()).collect();
                if v.is_empty() { row_of[&i] } else { v.iter().sum::<f64>() / v.len() as f64 }
            };
            column.sort_by(|a, b| key(*a).total_cmp(&key(*b)));
            for (r, i) in column.iter().enumerate() {
                row_of.insert(*i, r as f64);
            }
        }
    }
    // each part's turn: supply up, ground down, signal pins toward the columns they join
    let turn_of = |i: usize| -> u8 {
        let c = &s.components[i];
        if !turnable(c) {
            return c.rot;
        }
        let pins = pins_of(c);
        let score = |rot: u8| -> i32 {
            let mut t = c.clone();
            t.rot = rot;
            t.pos = (0, 0);
            let at: BTreeMap<String, Pt> = t.pins().into_iter().collect();
            pins.iter()
                .map(|(p, n)| {
                    let (x, y) = at[p];
                    if is_ground(n) {
                        -4 * y
                    } else if supplies.contains(n) {
                        4 * y
                    } else {
                        // toward the earlier columns on the left, the later on the right
                        let others: Vec<usize> = on_net.get(n).map(|v| v.iter().copied().filter(|j| *j != i).collect()).unwrap_or_default();
                        let dir: i32 = others.iter().map(|j| (col[j] as i32 - col[&i] as i32).signum()).sum();
                        -dir * x
                    }
                })
                .sum::<i32>()
                + if rot == 0 { 0 } else { 1 }
        };
        (0..4u8).min_by_key(|r| score(*r)).unwrap_or(0)
    };
    // where the block of parts goes: the sheet's top left, or (only some) below the parts that stay
    let stay: Vec<&Component> = s.components.iter().filter(|c| !c.kind.is_power_symbol() && !chosen(c)).collect();
    let origin: Pt = match only {
        Some(_) if !stay.is_empty() => {
            let left = stay.iter().map(|c| c.pos.0).min().unwrap_or(0);
            let bottom = stay.iter().map(|c| c.pos.1 + part_box(c, c.rot).3).max().unwrap_or(0);
            (left, bottom + 8)
        }
        _ => s.components.iter().filter(|c| !c.kind.is_power_symbol()).map(|c| c.pos).fold((i32::MAX, i32::MAX), |a, p| (a.0.min(p.0), a.1.min(p.1))),
    };
    let origin = if origin.0 == i32::MAX { (8, 8) } else { origin };
    let mut out = s.clone();
    let turns: BTreeMap<usize, u8> = parts.iter().map(|i| (*i, turn_of(*i))).collect();
    let power_refs: BTreeSet<&str> = s.components.iter().filter(|c| c.kind.is_power_symbol()).map(|c| c.refdes.as_str()).collect();
    let pin_count: BTreeMap<&String, usize> = nets.iter().map(|n| (&n.name, n.pins.iter().filter(|p| !power_refs.contains(p.refdes.as_str())).count())).collect();
    let mut x = origin.0;
    for column in &order {
        let width = column.iter().map(|i| {
            let b = part_box(&s.components[*i], turns[i]);
            b.2 - b.0
        });
        let width = width.max().unwrap_or(0);
        let mut y = origin.1;
        for &i in column {
            let b = part_box(&s.components[i], turns[&i]);
            let mut c = out.components[i].clone();
            c.rot = turns[&i];
            // the column's parts centred on one line, each below the one before (room for the labels and symbols above)
            c.pos = (x + (width - (b.2 - b.0)) / 2 - b.0, y + 3 - b.1);
            if wires {
                // lowered so a signal pin is level with the pin it joins in a column before (a straight wire), when there is room
                let level = c.pins().into_iter().filter_map(|(p, at)| {
                    let n = net_of.get(&(c.refdes.clone(), p))?;
                    // a net of two pins only: it is drawn as a wire
                    if pin_count.get(n) != Some(&2) {
                        return None;
                    }
                    let j = on_net.get(n)?.iter().copied().find(|j| *j != i && signal(n) && col[j] < col[&i])?;
                    let there = out.components[j].pins().into_iter().find(|q| net_of.get(&(out.components[j].refdes.clone(), q.0.clone())) == Some(n))?.1;
                    Some(there.1 - at.1)
                });
                if let Some(dy) = level.filter(|dy| *dy > -3 && c.pos.1 + dy + b.1 >= y).min() {
                    c.pos.1 += dy;
                }
            }
            y = c.pos.1 + b.3 + 4;
            out.components[i] = c;
        }
        // room for the labels on the pins (written to the right of a pin)
        x += width + 14;
    }
    // the joints drawn again: what touched the moved parts' old pins goes, then a label, ground or power symbol on each pin
    let old_pins: BTreeSet<Pt> = parts.iter().flat_map(|i| s.components[*i].pins().into_iter().map(|p| p.1)).collect();
    let all = only.is_none();
    out.wires.retain(|w| !all && !old_pins.contains(&w.a) && !old_pins.contains(&w.b));
    out.labels.retain(|l| !all && !old_pins.contains(&l.pos));
    out.components.retain(|c| !(c.kind.is_power_symbol() && (all || old_pins.contains(&c.pos))));
    // a pin left open on a net without a name of its own needs nothing (a named one keeps its label: the name may be used, the ADC
    // measure table's nets)
    let alone: BTreeSet<String> = nets.iter().filter(|n| n.name.starts_with("N$") && n.pins.iter().filter(|p| !power_refs.contains(p.refdes.as_str())).count() <= 1).map(|n| n.name.clone()).collect();
    let mut joints: Vec<(Pt, String, Option<Kind>)> = vec![];
    let mut added: Vec<Wire> = vec![];
    for i in &parts {
        let c = out.components.iter().find(|x| x.refdes == s.components[*i].refdes).expect("kept");
        let at = c.pins();
        let (top, bottom) = (at.iter().map(|p| p.1 .1).min().unwrap_or(0), at.iter().map(|p| p.1 .1).max().unwrap_or(0));
        let pins = pins_of(&s.components[*i]);
        let mut mine: Vec<(Pt, String, Option<Kind>)> = vec![];
        for (p, n) in pins.iter().cloned() {
            if alone.contains(&n) {
                continue;
            }
            let pos = at.iter().find(|x| x.0 == p).map(|x| x.1).expect("pin");
            let symbol = if is_ground(&n) && pos.1 == bottom && top != bottom {
                Some(Kind::Ground)
            } else if supplies.contains(&n) && pos.1 == top && top != bottom {
                Some(Kind::Power)
            } else {
                None
            };
            mine.push((pos, n, symbol));
        }
        if wires {
            // a chip's neighbouring supply (or ground) pins of one net on its top (bottom) row: one symbol in the middle, the
            // others joined to it by a wire along the row (the names of several symbols side by side would overlap)
            let net_at = |q: &(String, Pt)| pins.iter().find(|r| r.0 == q.0).map(|r| &r.1);
            let rows: BTreeSet<(String, i32)> = mine.iter().filter(|j| j.2.is_some()).map(|j| (j.1.clone(), j.0 .1)).collect();
            for (n, y) in rows {
                let mut xs: Vec<i32> = mine.iter().filter(|j| j.2.is_some() && j.1 == n && j.0 .1 == y).map(|j| j.0 .0).collect();
                xs.sort();
                let mut runs: Vec<Vec<i32>> = vec![];
                for x in xs {
                    // no pin of another net between
                    let joins = runs.last().and_then(|r| r.last()).is_some_and(|&px| !at.iter().any(|q| q.1 .1 == y && q.1 .0 > px && q.1 .0 < x && net_at(q) != Some(&n)));
                    match runs.last_mut() {
                        Some(r) if joins => r.push(x),
                        _ => runs.push(vec![x]),
                    }
                }
                for r in runs.iter().filter(|r| r.len() > 1) {
                    let keep = r[r.len() / 2];
                    mine.retain(|j| !(j.2.is_some() && j.1 == n && j.0 .1 == y && r.contains(&j.0 .0) && j.0 .0 != keep));
                    added.push(Wire { a: (r[0], y), b: (*r.last().expect("run"), y) });
                }
            }
        }
        joints.extend(mine);
    }
    // a net on two pins only (no symbol, none of the parts that stay): a wire between them where one fits, past the bodies of the
    // parts and touching nothing else; the net keeps its name by a label on one of them
    let mut wired = 0;
    let stay_nets: BTreeSet<&String> = stay.iter().flat_map(|c| c.pins().into_iter().filter_map(|(p, _)| net_of.get(&(c.refdes.clone(), p)))).collect();
    if wires {
        let boxes: Vec<(i32, i32, i32, i32)> = out
            .components
            .iter()
            .filter(|c| !c.kind.is_power_symbol())
            .map(|c| {
                let b = part_box(c, c.rot);
                (c.pos.0 + b.0, c.pos.1 + b.1, c.pos.0 + b.2, c.pos.1 + b.3)
            })
            .collect();
        let mut points: BTreeSet<Pt> = out.components.iter().filter(|c| !c.kind.is_power_symbol()).flat_map(|c| c.pins().into_iter().map(|p| p.1)).collect();
        points.extend(joints.iter().map(|j| j.0));
        points.extend(out.labels.iter().map(|l| l.pos));
        points.extend(out.components.iter().filter(|c| c.kind.is_power_symbol()).map(|c| c.pos));
        let mut segs: Vec<(Pt, Pt)> = out.wires.iter().chain(added.iter()).map(|w| (w.a, w.b)).collect();
        points.extend(segs.iter().flat_map(|s| [s.0, s.1]));
        let mut by_net: BTreeMap<&String, Vec<Pt>> = BTreeMap::new();
        for j in &joints {
            if j.2.is_none() {
                by_net.entry(&j.1).or_default().push(j.0);
            }
        }
        let mut two: Vec<(String, Pt, Pt)> = by_net.iter().filter(|(n, v)| v.len() == 2 && !stay_nets.contains(*n)).map(|(n, v)| ((*n).clone(), v[0], v[1])).collect();
        // the short ones first: they take the plain ways
        two.sort_by_key(|t| (t.1 .0 - t.2 .0).abs() + (t.1 .1 - t.2 .1).abs());
        let mut drop: BTreeSet<Pt> = BTreeSet::new();
        let mut labels: Vec<Label> = vec![];
        for (n, a, b) in two {
            let fits = |w: &Vec<Pt>| -> bool {
                w.windows(2).all(|s| points.iter().all(|p| *p == a || *p == b || !on_segment(*p, s[0], s[1])) && boxes.iter().all(|r| !crosses_box(s[0], s[1], *r)))
                    && w.iter().all(|p| segs.iter().all(|s| !on_segment(*p, s.0, s.1)))
            };
            let Some(mut w) = ways(a, b).into_iter().find(|w| fits(w)) else { continue };
            wired += 1;
            drop.extend([a, b]);
            // a net with a name (or a symbol elsewhere) keeps it by a label on the wire: one step into its longest level piece, so
            // the name is written along the wire, clear of the parts' bodies (the piece is split there, a label names a point)
            let named = !n.starts_with("N$") || joints.iter().any(|j| j.1 == n && j.2.is_some());
            if named {
                let level = (0..w.len() - 1).filter(|k| w[*k].1 == w[k + 1].1).max_by_key(|k| (w[k + 1].0 - w[*k].0).abs());
                // a point inside the piece, on no other wire (a label there would join it)
                let spot = level.and_then(|k| {
                    let (x0, x1) = (w[k].0.min(w[k + 1].0), w[k].0.max(w[k + 1].0));
                    (x0 + 1..x1).map(|x| (x, w[k].1)).find(|p| segs.iter().all(|s| !on_segment(*p, s.0, s.1))).map(|p| (k, p))
                });
                match spot {
                    Some((k, at)) => {
                        w.insert(k + 1, at);
                        labels.push(Label { pos: at, text: n.clone() });
                    }
                    // no room along the wire: on the end it does not leave to the right
                    None => labels.push(Label { pos: if w[1].0 > w[0].0 { w[w.len() - 1] } else { w[0] }, text: n.clone() }),
                }
            }
            for s in w.windows(2) {
                added.push(Wire { a: s[0], b: s[1] });
                segs.push((s[0], s[1]));
            }
            points.extend(w.iter().copied());
        }
        joints.retain(|j| !(j.2.is_none() && drop.contains(&j.0)));
        out.labels.extend(labels);
    }
    out.wires.extend(added);
    for (pos, n, symbol) in &joints {
        match symbol {
            Some(k) => {
                let i = out.add(*k, *pos, 0);
                if *k == Kind::Power {
                    out.components[i].value = n.clone();
                }
            }
            None => out.labels.push(Label { pos: *pos, text: n.clone() }),
        }
    }
    if !all {
        // the pins that stayed on a net the moved parts are on, now apart from them: a label of the net's name on each
        let moved_nets: BTreeSet<&String> = joints.iter().map(|j| &j.1).collect();
        for c in &stay {
            for (p, pos) in c.pins() {
                if let Some(n) = net_of.get(&(c.refdes.clone(), p))
                    && moved_nets.contains(n)
                    && !out.labels.iter().any(|l| l.pos == pos)
                {
                    out.labels.push(Label { pos, text: n.clone() });
                }
            }
        }
    }
    if grouping(&out) != before {
        return Err("整えると、つながりが変わってしまうので、やめました (階層の記号のように、位置でつながる物がある回路図は、まだ整えられません)".into());
    }
    Ok((out, Tidied { moved: parts.len(), columns, wired }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(name: &str) -> Schematic {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// Every sample tidies with its nets unchanged, and its parts no longer overlap.
    #[test]
    fn every_sample_tidies_without_changing_its_nets() {
        let mut n = 0;
        let mut refused = vec![];
        let mut wired = 0;
        for e in std::fs::read_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples")).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_none_or(|x| x != "schjson") {
                continue;
            }
            let s: Schematic = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
            match tidy(&s, None) {
                Ok((t, r)) => {
                    n += 1;
                    wired += r.wired;
                    assert!(tidy_with(&s, None, true).is_ok(), "{}: the wires join nothing they must not", p.display());
                    assert_eq!(grouping(&t), grouping(&s), "{}", p.display());
                    let names = |s: &Schematic| extract(s).into_iter().map(|n| n.name).filter(|n| !n.starts_with("N$")).collect::<BTreeSet<_>>();
                    assert_eq!(names(&t), names(&s), "{}: the nets keep their names", p.display());
                    let boxes: Vec<(i32, i32, i32, i32)> = t.components.iter().filter(|c| !c.kind.is_power_symbol()).map(|c| {
                        let b = part_box(c, c.rot);
                        (c.pos.0 + b.0, c.pos.1 + b.1, c.pos.0 + b.2, c.pos.1 + b.3)
                    }).collect();
                    for (i, a) in boxes.iter().enumerate() {
                        for b in &boxes[i + 1..] {
                            assert!(a.2 < b.0 || b.2 < a.0 || a.3 < b.1 || b.3 < a.1, "{}: parts overlap {a:?} {b:?}", p.display());
                        }
                    }
                }
                Err(e) => refused.push(format!("{}: {e}", p.display())),
            }
        }
        assert!(n > 100, "{n} tidied; refused: {refused:?}");
        assert!(wired > 100, "{wired} nets wired");
        eprintln!("{n} tidied, {wired} nets wired");
    }

    /// The divider from a supply: the source on the left, the resistors turned with the supply pin up and ground down, the signal
    /// flowing to the right; a part chosen alone goes below the others.
    #[test]
    fn a_divider_reads_from_left_to_right() {
        let s = load("divider_tolerance.schjson");
        let (t, r) = tidy(&s, None).unwrap();
        assert!(r.columns >= 2, "{r:?}");
        // the source to R1 and R1 to R2 by wires, MID keeping its name on its wire
        assert_eq!(r.wired, 2, "{r:?}");
        assert!(t.labels.iter().any(|l| l.text == "MID" && t.wires.iter().any(|w| w.a == l.pos || w.b == l.pos)));
        let source = t.components.iter().find(|c| c.kind == Kind::VSource).unwrap();
        assert!(t.components.iter().filter(|c| c.kind == Kind::Resistor).all(|c| c.pos.0 > source.pos.0), "the source on the left");
        let only: BTreeSet<String> = [t.components.iter().find(|c| c.kind == Kind::Resistor).unwrap().refdes.clone()].into();
        let (u, r) = tidy(&t, Some(&only)).unwrap();
        assert_eq!(r.moved, 1);
        assert_eq!(grouping(&u), grouping(&s));
    }

    /// The nets keep their names (the ADC measure table finds its nets by name), a pin left open on a named net included.
    #[test]
    fn the_nets_keep_their_names() {
        let s = load("adc_measure.schjson");
        let (t, _) = tidy(&s, None).unwrap();
        let names = |s: &Schematic| extract(s).into_iter().map(|n| n.name).filter(|n| !n.starts_with("N$")).collect::<BTreeSet<_>>();
        assert_eq!(names(&t), names(&s));
        assert!(names(&t).contains("HI"), "HI is on one pin only");
        // the ADC's three AVDD pins on its top row: one symbol, the others joined to it by a wire
        let avdd: Vec<&Component> = t.components.iter().filter(|c| c.kind == Kind::Power && c.value == "AVDD").collect();
        assert!(avdd.iter().all(|a| avdd.iter().all(|b| a.pos == b.pos || a.pos.1 != b.pos.1 || (a.pos.0 - b.pos.0).abs() > 4)), "no symbols side by side");
    }
}
