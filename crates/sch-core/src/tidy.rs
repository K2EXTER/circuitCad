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

use crate::{extract, Component, Kind, Label, Pt, Schematic};
use std::collections::{BTreeMap, BTreeSet};

/// What a tidy did.
#[derive(Clone, Debug, PartialEq)]
pub struct Tidied {
    pub moved: usize,
    pub columns: usize,
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
    if !s.ports.is_empty() || !s.buses.is_empty() {
        return Err("ポートかバスのあるシートは、まだ整えられません (位置でつながっているので)".into());
    }
    let before = grouping(s);
    let nets = extract(s);
    let supplies: BTreeSet<String> = crate::breadboard::read(s).supplies.into_iter().collect();
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
            let c = &mut out.components[i];
            c.rot = turns[&i];
            // the column's parts centred on one line, each below the one before (room for the labels and symbols above)
            c.pos = (x + (width - (b.2 - b.0)) / 2 - b.0, y + 3 - b.1);
            y += b.3 - b.1 + 7;
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
    let power_refs: BTreeSet<&str> = s.components.iter().filter(|c| c.kind.is_power_symbol()).map(|c| c.refdes.as_str()).collect();
    // a pin left open on a net without a name of its own needs nothing (a named one keeps its label: the name may be used, the ADC
    // measure table's nets)
    let alone: BTreeSet<String> = nets.iter().filter(|n| n.name.starts_with("N$") && n.pins.iter().filter(|p| !power_refs.contains(p.refdes.as_str())).count() <= 1).map(|n| n.name.clone()).collect();
    let mut joints: Vec<(Pt, String, Option<Kind>)> = vec![];
    for i in &parts {
        let c = out.components.iter().find(|x| x.refdes == s.components[*i].refdes).expect("kept");
        let at = c.pins();
        let (top, bottom) = (at.iter().map(|p| p.1 .1).min().unwrap_or(0), at.iter().map(|p| p.1 .1).max().unwrap_or(0));
        for (p, n) in pins_of(&s.components[*i]) {
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
            joints.push((pos, n, symbol));
        }
    }
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
    Ok((out, Tidied { moved: parts.len(), columns }))
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
        for e in std::fs::read_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples")).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_none_or(|x| x != "schjson") {
                continue;
            }
            let s: Schematic = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
            match tidy(&s, None) {
                Ok((t, _)) => {
                    n += 1;
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
    }

    /// The divider from a supply: the source on the left, the resistors turned with the supply pin up and ground down, the signal
    /// flowing to the right; a part chosen alone goes below the others.
    #[test]
    fn a_divider_reads_from_left_to_right() {
        let s = load("divider_tolerance.schjson");
        let (t, r) = tidy(&s, None).unwrap();
        assert!(r.columns >= 2, "{r:?}");
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
    }
}
