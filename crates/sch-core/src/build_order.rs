//! The order to build a stripboard or perfboard in (candidate B5d): one section after another, from the one the supply comes into,
//! each section's parts lowest first, and what to measure once a section is on — so a mistake shows up in the section just
//! built, with only a few parts to look through, before the next is fitted. The simulation of each step (the parts fitted so far,
//! powered) is the app's (`app/src/breadboard/build_steps.rs`); this module orders and names.

use crate::mount::Mount;
use crate::stripboard::{At, StripLayout};
use crate::{extract, Kind, Schematic};
use std::collections::{BTreeMap, BTreeSet};

/// One step of the build: a section (or the parts outside every section, or the whole board without sections).
#[derive(Clone, Debug, PartialEq)]
pub struct BuildStep {
    /// The section's name; "区画の外" for the parts in none; "全体" when the board has no sections.
    pub name: String,
    /// The parts to fit, in the order to solder them (lowest first: the wire links' turn is before them).
    pub parts: Vec<String>,
    /// The wire links on the parts' side within this step's area (indices into `StripLayout::links`).
    pub links: Vec<usize>,
    /// The named nets this step's parts reach for the first time (ground left out): where to put the meter's probe.
    pub nets: Vec<String>,
}

/// The name of the step for the parts outside every section.
pub const OUTSIDE: &str = "区画の外";
/// The name of the one step of a board without sections.
pub const WHOLE: &str = "全体";

/// The volts a supply's net name says (5V → 5, +3V3 → 3.3, 12V → 12, -5V → -5, VCC → None).
pub fn rail_volts(name: &str) -> Option<f64> {
    let up = name.trim().to_ascii_uppercase();
    let (sign, body) = match up.strip_prefix('-') {
        Some(b) => (-1.0, b),
        None => (1.0, up.trim_start_matches('+')),
    };
    let (a, b) = body.split_once('V')?;
    if a.is_empty() || !a.chars().all(|c| c.is_ascii_digit() || c == '.') || !b.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let v: f64 = if b.is_empty() { a.parse().ok()? } else { format!("{a}.{b}").parse().ok()? };
    (v > 0.0).then_some(sign * v)
}

/// The supply-named nets that reach a connector, with the volts their names say (5V, 3V3): what feeds a board that has no supply
/// of its own on the schematic (it is fed through its header).
pub fn connector_rails(s: &Schematic) -> Vec<(String, f64)> {
    let flat = s.flat();
    let kinds: BTreeMap<&str, &Kind> = flat.components.iter().map(|c| (c.refdes.as_str(), &c.kind)).collect();
    extract(&flat)
        .into_iter()
        .filter(|n| !crate::breadboard::is_ground_name(&n.name) && n.pins.iter().any(|p| connector(&p.refdes, kinds.get(p.refdes.as_str()).copied())))
        .filter_map(|n| rail_volts(&n.name).map(|v| (n.name, v)))
        .collect()
}

/// Whether a part is a connector the board is fed through (a header, a terminal block, a jack).
fn connector(refdes: &str, kind: Option<&Kind>) -> bool {
    let head: String = refdes.chars().take_while(|c| c.is_ascii_alphabetic()).collect::<String>().to_ascii_uppercase();
    // (not X: a crystal is X or Y as often as a connector is X)
    matches!(head.as_str(), "J" | "P" | "CN" | "CON" | "TB") || matches!(kind, Some(Kind::Header(_)))
}

/// The nets the board's supply comes in on: those of the parts off the board (a bench supply, a battery), and the supply-named
/// nets that reach a connector (a board fed through its header) or a module (an Arduino fed by its USB).
pub fn supply_inputs(s: &Schematic, l: &StripLayout) -> BTreeSet<String> {
    let flat = s.flat();
    let kinds: BTreeMap<&str, &Kind> = flat.components.iter().map(|c| (c.refdes.as_str(), &c.kind)).collect();
    let mut out: BTreeSet<String> = l.off.iter().flat_map(|(_, pins)| pins.iter().map(|(_, net, _)| net.clone())).filter(|n| !crate::breadboard::is_ground_name(n)).collect();
    let modules: BTreeSet<&str> = l.parts.iter().filter(|p| matches!(p.mount, Mount::Module { .. })).map(|p| p.refdes.as_str()).collect();
    for n in extract(&flat) {
        if crate::section_plan::supply_name(&n.name) && !crate::breadboard::is_ground_name(&n.name) && n.pins.iter().any(|p| connector(&p.refdes, kinds.get(p.refdes.as_str()).copied()) || modules.contains(p.refdes.as_str())) {
            out.insert(n.name);
        }
    }
    out
}

/// Where a part goes in the order of soldering: the lowest first (resistors and diodes lie flat), then the sockets and the chips,
/// the small capacitors, the transistors and LEDs, the tall ones (trimmers, switches, electrolytics), and last the relays, the
/// connectors and the modules (big, and they get in the way of the rest).
fn fit_rank(refdes: &str, kind: Option<&Kind>, mount: &Mount) -> u8 {
    if matches!(mount, Mount::Module { .. }) {
        return 7;
    }
    // a chip on the copper side: first, while the board lies flat on its parts' side with nothing on top yet
    if matches!(mount, Mount::Back { .. }) {
        return 0;
    }
    if connector(refdes, kind) {
        return 7;
    }
    let head: String = refdes.chars().take_while(|c| c.is_ascii_alphabetic()).collect::<String>().to_ascii_uppercase();
    match (head.as_str(), kind) {
        (_, Some(Kind::Led)) => 4,
        ("R" | "FB" | "D" | "L", _) => 1,
        ("U" | "IC", _) => 2,
        ("C", _) => 3,
        ("Y" | "X", _) | ("Q" | "TR", _) => 4,
        ("VR" | "RV" | "POT" | "SW" | "S" | "BZ" | "LS", _) => 5,
        ("K" | "RL" | "RY", _) => 6,
        _ => 4,
    }
}

/// The steps to build the board in.
pub fn build_steps(s: &Schematic, l: &StripLayout) -> Vec<BuildStep> {
    let flat = s.flat();
    let kinds: BTreeMap<&str, &Kind> = flat.components.iter().map(|c| (c.refdes.as_str(), &c.kind)).collect();
    let inside = |a: At, (p, q): (At, At)| a.0 >= p.0.min(q.0) && a.0 <= p.0.max(q.0) && a.1 >= p.1.min(q.1) && a.1 <= p.1.max(q.1);
    // each part's section: the one its leads lie in (most of them)
    let mut groups: Vec<(String, Vec<&crate::stripboard::StripPart>)> = l.sections.iter().map(|(n, _, _)| (n.clone(), vec![])).collect();
    let mut outside = vec![];
    for p in &l.parts {
        let best = l.sections.iter().enumerate().map(|(k, (_, a, b))| (p.leads.iter().filter(|(_, h)| inside(*h, (*a, *b))).count(), k)).filter(|(n, _)| *n > 0).max();
        match best {
            Some((_, k)) => groups[k].1.push(p),
            None => outside.push(p),
        }
    }
    if l.sections.is_empty() {
        groups = vec![(WHOLE.to_string(), l.parts.iter().collect())];
    } else if !outside.is_empty() {
        groups.push((OUTSIDE.to_string(), outside));
    }
    groups.retain(|(_, ps)| !ps.is_empty());
    // the sections from the supply's way in: how many parts away from a net the supply comes in on (ground not crossed)
    let nets = extract(&flat);
    let net_of: BTreeMap<(&str, &str), &str> = nets.iter().flat_map(|n| n.pins.iter().map(move |p| ((p.refdes.as_str(), p.pin.as_str()), n.name.as_str()))).collect();
    let part_nets: BTreeMap<&str, BTreeSet<&str>> = l
        .parts
        .iter()
        .map(|p| (p.refdes.as_str(), p.leads.iter().filter_map(|(pin, _)| net_of.get(&(p.refdes.as_str(), pin.as_str())).copied()).filter(|n| !crate::breadboard::is_ground_name(n)).collect()))
        .collect();
    let starts = supply_inputs(s, l);
    let mut depth: BTreeMap<&str, usize> = BTreeMap::new();
    let mut reached: BTreeSet<&str> = starts.iter().map(String::as_str).collect();
    let mut frontier = reached.clone();
    let mut d = 0;
    while !frontier.is_empty() {
        let mut next = BTreeSet::new();
        for (part, ns) in &part_nets {
            if !depth.contains_key(part) && ns.iter().any(|n| frontier.contains(n)) {
                depth.insert(part, d);
                next.extend(ns.iter().filter(|n| !reached.contains(*n)).copied());
            }
        }
        reached.extend(next.iter().copied());
        frontier = next;
        d += 1;
    }
    let far = usize::MAX;
    let rank_of = |ps: &[&crate::stripboard::StripPart]| ps.iter().map(|p| depth.get(p.refdes.as_str()).copied().unwrap_or(far)).min().unwrap_or(far);
    // a section with a part that gives a supply (a regulator's output pin, VOUT, on a supply-named net), or one named for the
    // power, goes first: the rest are fed from it
    let comps: BTreeMap<&str, &crate::Component> = flat.components.iter().map(|c| (c.refdes.as_str(), c)).collect();
    let gives_supply = |p: &crate::stripboard::StripPart| {
        p.leads.iter().any(|(pin, _)| {
            let out = comps.get(p.refdes.as_str()).and_then(|c| c.pin_name(pin)).and_then(crate::PinKind::guess_from_name) == Some(crate::PinKind::PowerOut);
            out && net_of.get(&(p.refdes.as_str(), pin.as_str())).is_some_and(|n| crate::section_plan::supply_name(n))
        })
    };
    let power = |k: usize| ["電源", "POWER", "PWR", "SUPPLY"].iter().any(|w| groups[k].0.to_ascii_uppercase().contains(w)) || groups[k].1.iter().any(|p| gives_supply(p));
    let mut order: Vec<usize> = (0..groups.len()).collect();
    // (the parts outside every section stay last: wiring between the sections)
    order.sort_by_key(|k| (groups[*k].0 == OUTSIDE, !power(*k), rank_of(&groups[*k].1), *k));
    let natural = |s: &str| {
        let head: String = s.chars().take_while(|c| !c.is_ascii_digit()).collect();
        let num: String = s.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
        (head, num.parse::<u64>().unwrap_or(0))
    };
    // a link goes on with the later of the two sections its ends are in (both ends' parts are there then); an end in no section
    // box, with the last step
    let box_of = |h: At| l.sections.iter().position(|(_, a, b)| inside(h, (*a, *b))).map(|i| l.sections[i].0.as_str());
    let step_of = |name: Option<&str>| name.and_then(|n| order.iter().position(|k| groups[*k].0 == n)).unwrap_or(order.len().saturating_sub(1));
    let link_step: Vec<usize> = l.links.iter().map(|w| step_of(box_of(w.a)).max(step_of(box_of(w.b)))).collect();
    let mut seen_nets: BTreeSet<&str> = BTreeSet::new();
    let mut out = vec![];
    for (step, k) in order.iter().copied().enumerate() {
        let (name, ps) = &groups[k];
        let mut parts: Vec<&crate::stripboard::StripPart> = ps.clone();
        parts.sort_by(|a, b| fit_rank(&a.refdes, kinds.get(a.refdes.as_str()).copied(), &a.mount).cmp(&fit_rank(&b.refdes, kinds.get(b.refdes.as_str()).copied(), &b.mount)).then_with(|| natural(&a.refdes).cmp(&natural(&b.refdes))));
        let links: Vec<usize> = (0..l.links.len()).filter(|i| link_step[*i] == step).collect();
        let mut fresh: Vec<&str> = vec![];
        for p in &parts {
            for n in part_nets.get(p.refdes.as_str()).into_iter().flatten() {
                if !n.starts_with("N$") && seen_nets.insert(n) {
                    fresh.push(n);
                }
            }
        }
        out.push(BuildStep { name: name.clone(), parts: parts.iter().map(|p| p.refdes.clone()).collect(), links, nets: fresh.iter().map(|n| n.to_string()).collect() });
    }
    out
}

/// The schematic as built up to and including step `upto`: the board's parts of the later steps taken away (the parts off the
/// board, the supplies and the power symbols stay).
pub fn built_so_far(s: &Schematic, l: &StripLayout, steps: &[BuildStep], upto: usize) -> Schematic {
    let fitted: BTreeSet<&str> = steps.iter().take(upto + 1).flat_map(|st| st.parts.iter().map(String::as_str)).collect();
    let on_board: BTreeSet<&str> = l.parts.iter().map(|p| p.refdes.as_str()).collect();
    let mut out = s.flat().into_owned();
    out.components.retain(|c| !on_board.contains(c.refdes.as_str()) || fitted.contains(c.refdes.as_str()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(name: &str) -> Schematic {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn a_rail_name_says_its_volts() {
        for (n, v) in [("5V", Some(5.0)), ("+3V3", Some(3.3)), ("12V", Some(12.0)), ("-5V", Some(-5.0)), ("1V8", Some(1.8)), ("VCC", None), ("V1", None), ("5", None)] {
            assert_eq!(rail_volts(n), v, "{n}");
        }
    }

    /// Without sections, one step with every part, lowest first; with sections, one step each, every part in exactly one.
    #[test]
    fn the_steps_cover_every_part_once_lowest_first() {
        let mut s = load("ne555_blinker.schjson");
        let l = crate::test_layouts::strip(&s);
        let one = build_steps(&s, &l);
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].name, WHOLE);
        let parts = &one[0].parts;
        let at = |r: &str| parts.iter().position(|p| p == r).unwrap();
        assert!(at("R1") < at("U1") && at("U1") < at("C1") && at("C1") < at("D1"), "resistors, the chip, capacitors, the LED: {parts:?}");
        assert_eq!(one[0].links.len(), l.links.len(), "every link in the one step");
        let group = |name: &str, parts: &[&str]| design_spec::PlaceGroup { name: name.into(), parts: parts.iter().map(|p| p.to_string()).collect(), section: true, ..Default::default() };
        s.requirements.groups = vec![group("表示", &["R3", "D1"]), group("タイマ", &["U1", "R1", "R2", "C1", "C2"])];
        let l = crate::perfboard::perf_layout_with(&s, crate::stripboard::StripOptions::default());
        let steps = build_steps(&s, &l);
        let all: Vec<&String> = steps.iter().flat_map(|st| &st.parts).collect();
        assert_eq!(all.len(), l.parts.len(), "{steps:?}");
        assert_eq!(all.iter().collect::<BTreeSet<_>>().len(), all.len(), "no part twice");
        assert_eq!(steps.iter().map(|st| st.links.len()).sum::<usize>(), l.links.len(), "every link once");
        // the timer takes the supply first (the LED's resistor hangs off the timer's output)
        assert_eq!(steps[0].name, "タイマ", "{steps:?}");
        let built = built_so_far(&s, &l, &steps, 0);
        assert!(built.components.iter().all(|c| c.refdes != "D1" && c.refdes != "R3"), "the display is not on yet");
        assert!(built.components.iter().any(|c| c.refdes == "U1"));
    }
}
