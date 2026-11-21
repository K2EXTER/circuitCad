//! Sections proposed for a board built by hand (B5b, 2026-11-21): the parts on the board cut into groups with few nets between them
//! (Fiduccia–Mattheyses min-cut, again and again), the supplies and ground left out (they reach every part). Each group is a section
//! the person may keep (the requirements table's "近くに置く組" marked "区画"): its parts go together and its ground goes back to the
//! one ground point by a wire of its own.

use crate::breadboard::{is_ground_name, read};
use crate::Schematic;
use std::collections::{BTreeMap, BTreeSet};

/// A group this small (parts) is not cut again.
const MIN_PARTS: usize = 4;
/// A cut is kept when the nets across it are at most this share of the nets within the group (and at most `MAX_CUT`).
const CUT_SHARE: f64 = 0.3;
const MAX_CUT: usize = 12;
/// How many times a group is cut in two at most (2 → up to 4 sections).
const DEPTH: usize = 2;
/// How many starts each cut is tried from (the best kept).
const STARTS: usize = 12;

/// What a part does on the board, for the sections by role.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Role {
    Digital,
    Relay,
    Analog,
    Power,
}

impl Role {
    fn name(self) -> &'static str {
        match self {
            Role::Digital => "デジタル",
            Role::Relay => "リレー",
            Role::Analog => "アナログ",
            Role::Power => "電源",
        }
    }
}

/// The sections by role, when the parts fall in two roles or more: the chips and connectors whose signals (not the supplies or
/// ground) are half digital or more (`digital_net`: a microcontroller's header, a relay driver; an ADC with its SPI stays analog)
/// are digital, the relays (K1 …) one section, a part on two supplies or more (a regulator) the power, the other chips analog; the
/// two-lead parts and small rows (a resistor, a protection diode) go with the role they share the most signals with, or else with
/// the chip drawn nearest them on the schematic (a decoupling capacitor beside its chip). None when all are in one role.
fn roles(s: &Schematic, c: &crate::breadboard::Circuit) -> Option<Vec<SectionPlan>> {
    use crate::breadboard::Look;
    if c.on.len() < 2 * MIN_PARTS {
        return None;
    }
    let supply = |n: &str| c.supplies.iter().any(|x| x == n) || supply_name(n);
    let rail = |n: &str| n.is_empty() || is_ground_name(n) || supply(n) || c.ground.as_deref() == Some(n);
    let signals = |k: usize| -> BTreeSet<&str> { c.on[k].2.iter().map(|(_, n)| n.as_str()).filter(|n| !rail(n)).collect() };
    let follower = |k: usize| c.on[k].1 == Look::TwoLead || (c.on[k].1 == Look::Row && c.on[k].2.len() <= 3);
    let relay = |r: &str| r.starts_with('K') && r[1..].starts_with(|ch: char| ch.is_ascii_digit());
    let mut role: Vec<Option<Role>> = vec![None; c.on.len()];
    for k in 0..c.on.len() {
        let r = &c.on[k].0;
        if relay(r) {
            role[k] = Some(Role::Relay);
            continue;
        }
        if follower(k) {
            continue;
        }
        let supplies: BTreeSet<&str> = c.on[k].2.iter().map(|(_, n)| n.as_str()).filter(|n| supply(n)).collect();
        let sig = signals(k);
        let digital = sig.iter().filter(|n| digital_net(n)).count();
        role[k] = Some(if supplies.len() >= 2 && sig.len() <= 2 {
            Role::Power
        } else if !sig.is_empty() && digital * 2 >= sig.len() {
            Role::Digital
        } else {
            Role::Analog
        });
    }
    let leaders: Vec<usize> = (0..c.on.len()).filter(|k| role[*k].is_some()).collect();
    let pos = |r: &str| s.components.iter().find(|x| x.refdes == r).map(|x| (x.pos.0 as f64, x.pos.1 as f64));
    let followers: Vec<usize> = (0..c.on.len()).filter(|k| role[*k].is_none()).collect();
    for k in followers {
        let mine = signals(k);
        let mut score: BTreeMap<Role, usize> = BTreeMap::new();
        for l in &leaders {
            let shared = signals(*l).intersection(&mine).count();
            if shared > 0 {
                *score.entry(role[*l].unwrap()).or_default() += shared;
            }
        }
        role[k] = match score.into_iter().max_by_key(|(r, n)| (*n, std::cmp::Reverse(*r))) {
            Some((r, _)) => Some(r),
            // only on the supplies and ground: with the chip drawn nearest it
            None => {
                let at = pos(&c.on[k].0);
                leaders.iter().filter_map(|l| Some((*l, pos(&c.on[*l].0)?))).min_by(|a, b| {
                    let d = |p: (f64, f64)| at.map_or(0.0, |q| (p.0 - q.0).hypot(p.1 - q.1));
                    d(a.1).total_cmp(&d(b.1))
                }).and_then(|(l, _)| role[l])
            }
        };
    }
    let mut by: BTreeMap<Role, Vec<String>> = BTreeMap::new();
    for (k, r) in role.iter().enumerate() {
        by.entry(r.unwrap_or(Role::Analog)).or_default().push(c.on[k].0.clone());
    }
    if by.len() < 2 {
        return None;
    }
    Some(
        by.into_iter()
            .map(|(r, mut ps)| {
                ps.sort();
                SectionPlan { name: r.name().to_string(), parts: ps }
            })
            .collect(),
    )
}

/// Whether a net's name is a supply's (with no source drawn on the sheet, the board's supplies come by name): 5V, +12V, 3V3,
/// VCC, VDD, AVDD, DVDD, VIN, VBAT, VEE, VSS.
pub fn supply_name(name: &str) -> bool {
    let up = name.to_ascii_uppercase();
    let up = up.trim_start_matches(['+', '-']);
    let volts = up.split_once('V').is_some_and(|(a, b)| !a.is_empty() && a.chars().all(|c| c.is_ascii_digit() || c == '.') && b.chars().all(|c| c.is_ascii_digit()));
    volts || ["VCC", "VDD", "AVDD", "DVDD", "IOVDD", "VIN", "VBAT", "VEE", "VSS", "AVCC"].iter().any(|p| up == *p || (up.starts_with(p) && up[p.len()..].starts_with(['_', '1', '2', '3'])))
}

/// Whether a net's name is a digital signal's: a bus (SPI, I²C, a UART), a control line (a chip select, a reset, a clock, an
/// interrupt, a UART's handshake), a crystal's, USB's data or a numbered pin of a microcontroller (D22, GPIO3, PB1, IO5, TXD0),
/// alone or as a word of the name (SPI_MOSI, ADC_CS, USB_DP).
pub fn digital_net(name: &str) -> bool {
    const WORDS: [&str; 50] = [
        "MOSI", "MISO", "SCK", "SCLK", "CS", "NCS", "CSN", "SS", "SDA", "SCL", "TX", "TXD", "RX", "RXD", "DRDY", "START", "RESET", "NRESET", "RST", "NRST", "CLK", "INT", "IRQ", "PWM",
        "SDI", "SDO", "DIN", "DOUT", "SPI", "I2C", "UART", "GPIO", "ALERT", "BUSY", "USB", "DP", "DM", "XTAL", "XTAL1", "XTAL2", "XI", "XO", "OSC", "ICSP", "CTS", "DSR", "RI", "DCD", "DTR", "RTS",
    ];
    let numbered = |w: &str| {
        let head = w.trim_end_matches(|c: char| c.is_ascii_digit());
        head.len() < w.len() && (matches!(head, "D" | "GPIO" | "IO" | "PIN" | "TX" | "RX" | "TXD" | "RXD") || (head.len() == 2 && head.starts_with('P') && head.as_bytes()[1].is_ascii_alphabetic()))
    };
    let up = name.to_ascii_uppercase();
    up.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).any(|w| WORDS.contains(&w) || numbered(w))
}

/// A proposed section: its name (after its part with the most pins) and its parts.
#[derive(Clone, Debug, PartialEq)]
pub struct SectionPlan {
    pub name: String,
    pub parts: Vec<String>,
}

/// The sections proposed for the schematic's parts on the board, or none when it does not part well (one group).
pub fn propose_sections(s: &Schematic) -> Vec<SectionPlan> {
    let c = read(s);
    let parts: Vec<(String, usize)> = c.on.iter().map(|(r, _, pins)| (r.clone(), pins.len().max(1))).collect();
    // the nets between parts, the supplies and ground left out
    let mut by_net: BTreeMap<&str, BTreeSet<usize>> = BTreeMap::new();
    for (i, (_, _, pins)) in c.on.iter().enumerate() {
        for (_, n) in pins {
            if !n.is_empty() && !is_ground_name(n) && !c.supplies.contains(n) && c.ground.as_deref() != Some(n.as_str()) {
                by_net.entry(n.as_str()).or_default().insert(i);
            }
        }
    }
    if let Some(by_role) = roles(s, &c) {
        return by_role;
    }
    let nets: Vec<(Vec<usize>, f64)> = by_net.into_values().filter(|v| v.len() >= 2).map(|v| (v.into_iter().collect(), 1.0)).collect();
    let mut groups: Vec<Vec<usize>> = vec![(0..parts.len()).collect()];
    for _ in 0..DEPTH {
        let mut next = vec![];
        for g in groups {
            match cut(&g, &parts, &nets) {
                Some((a, b)) => {
                    next.push(a);
                    next.push(b);
                }
                None => next.push(g),
            }
        }
        groups = next;
    }
    if groups.len() < 2 {
        return vec![];
    }
    let mut used: BTreeSet<String> = BTreeSet::new();
    groups
        .into_iter()
        .map(|g| {
            let big = g.iter().max_by_key(|i| (parts[**i].1, std::cmp::Reverse(parts[**i].0.clone()))).map(|i| parts[*i].0.clone()).unwrap_or_default();
            let mut name = format!("{big} のまわり");
            while !used.insert(name.clone()) {
                name.push('\'');
            }
            let mut ps: Vec<String> = g.iter().map(|i| parts[*i].0.clone()).collect();
            ps.sort();
            SectionPlan { name, parts: ps }
        })
        .collect()
}

/// The group `g` cut in two with the fewest nets across (each half's pins within a fifth of half), or None when it is too small or
/// every cut crosses too many nets.
fn cut(g: &[usize], parts: &[(String, usize)], nets: &[(Vec<usize>, f64)]) -> Option<(Vec<usize>, Vec<usize>)> {
    if g.len() < 2 * MIN_PARTS {
        return None;
    }
    let inside: BTreeSet<usize> = g.iter().copied().collect();
    // the nets within the group (their parts here)
    let here: Vec<(Vec<usize>, f64)> = nets.iter().map(|(n, w)| (n.iter().copied().filter(|p| inside.contains(p)).collect::<Vec<usize>>(), *w)).filter(|(n, _)| n.len() >= 2).collect();
    if here.is_empty() {
        return None;
    }
    let local: BTreeMap<usize, usize> = g.iter().enumerate().map(|(k, p)| (*p, k)).collect();
    let weight: Vec<f64> = here.iter().map(|x| x.1).collect();
    let here: Vec<Vec<usize>> = here.into_iter().map(|(n, _)| n.into_iter().map(|p| local[&p]).collect()).collect();
    let w: Vec<f64> = g.iter().map(|p| parts[*p].1 as f64).collect();
    let total: f64 = w.iter().sum();
    let mut of: Vec<Vec<usize>> = vec![vec![]; g.len()];
    for (e, n) in here.iter().enumerate() {
        for p in n {
            of[*p].push(e);
        }
    }
    let crossing = |side: &[bool]| -> f64 { here.iter().zip(&weight).filter(|(n, _)| n.iter().any(|p| side[*p]) && n.iter().any(|p| !side[*p])).map(|(_, w)| w).sum() };
    let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut best: Option<(f64, Vec<bool>)> = None;
    for start in 0..STARTS {
        // a start: in the parts' order (the schematic's), or shuffled; the first half of the pins on one side
        let mut order: Vec<usize> = (0..g.len()).collect();
        if start > 0 {
            for i in (1..order.len()).rev() {
                order.swap(i, (rnd() % (i as u64 + 1)) as usize);
            }
        }
        let mut side = vec![false; g.len()];
        let mut acc = 0.0;
        for i in order {
            side[i] = acc >= total / 2.0;
            acc += w[i];
        }
        fm(&mut side, &w, &here, &weight, &of, total);
        let n0 = side.iter().filter(|s| !**s).count();
        if n0 < MIN_PARTS || g.len() - n0 < MIN_PARTS {
            continue;
        }
        let x = crossing(&side);
        if best.as_ref().is_none_or(|b| x < b.0) {
            best = Some((x, side));
        }
    }
    let (x, side) = best?;
    if x > MAX_CUT as f64 || x > CUT_SHARE * weight.iter().sum::<f64>() {
        return None;
    }
    let a: Vec<usize> = g.iter().enumerate().filter(|(k, _)| !side[*k]).map(|(_, p)| *p).collect();
    let b: Vec<usize> = g.iter().enumerate().filter(|(k, _)| side[*k]).map(|(_, p)| *p).collect();
    Some((a, b))
}

/// Fiduccia–Mattheyses: each pass moves every part across once, the one that saves the most nets first, while each half's pins stay
/// within a fifth of half; back to the best point of the pass; until a pass saves nothing.
fn fm(side: &mut [bool], w: &[f64], nets: &[Vec<usize>], weight: &[f64], of: &[Vec<usize>], total: f64) {
    let n = side.len();
    let slack = total / 4.0;
    for _ in 0..8 {
        let mut count: Vec<[u32; 2]> = nets.iter().map(|e| [e.iter().filter(|p| !side[**p]).count() as u32, e.iter().filter(|p| side[**p]).count() as u32]).collect();
        let mut room0: f64 = (0..n).filter(|i| !side[*i]).map(|i| w[i]).sum();
        let mut locked = vec![false; n];
        let (mut run, mut best, mut best_at) = (0.0f64, 0.0f64, 0usize);
        let mut moves = vec![];
        for _ in 0..n {
            let gain = |i: usize, count: &[[u32; 2]]| -> f64 {
                let (from, to) = (usize::from(side[i]), usize::from(!side[i]));
                of[i].iter().map(|e| weight[*e] * (f64::from(u8::from(count[*e][from] == 1 && count[*e][to] > 0)) - f64::from(u8::from(count[*e][to] == 0 && count[*e][from] > 1)))).sum()
            };
            let pick = (0..n)
                .filter(|i| !locked[*i])
                .filter(|i| {
                    let r = if side[*i] { room0 + w[*i] } else { room0 - w[*i] };
                    (r - total / 2.0).abs() <= slack
                })
                .map(|i| (gain(i, &count), i))
                .max_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)));
            let Some((g, i)) = pick else { break };
            let (from, to) = (usize::from(side[i]), usize::from(!side[i]));
            for e in &of[i] {
                count[*e][from] -= 1;
                count[*e][to] += 1;
            }
            room0 += if side[i] { w[i] } else { -w[i] };
            side[i] = !side[i];
            locked[i] = true;
            moves.push(i);
            run += g;
            if run > best + 1e-9 {
                best = run;
                best_at = moves.len();
            }
        }
        for i in moves.drain(best_at..) {
            side[i] = !side[i];
        }
        if best <= 1e-9 {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(name: &str) -> Schematic {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// Which nets' names are a digital signal's and which a supply's (the sections by role go by them).
    #[test]
    fn digital_signals_and_supplies_by_name() {
        for (name, digital) in [("MOSI", true), ("ADC_CS", true), ("SPI_SCLK", true), ("D22", true), ("GPIO3", true), ("PB1", true), ("TXD0", true), ("USB_DP", true), ("DRDY", true), ("RTS", true), ("HI", false), ("VCOM", false), ("RBOT_S", false), ("D", false), ("DIV_A", false), ("PA", false)] {
            assert_eq!(digital_net(name), digital, "{name}");
        }
        for (name, supply) in [("5V", true), ("+12V", true), ("3V3", true), ("AVDD", true), ("VCC", true), ("VDD_IO", true), ("VBAT", true), ("V3", false), ("VCOM", false), ("VREF", false), ("HI", false)] {
            assert_eq!(supply_name(name), supply, "{name}");
        }
    }

    /// The active filter's two stages come out as two sections (each op-amp with its resistors and capacitors); kept in the table, the
    /// perfboard is laid out in those two sections and joins what the schematic joins. The 555 (few parts) is not cut.
    #[test]
    fn the_two_stages_of_a_filter_are_proposed_as_sections() {
        let mut s = load("active_filter.schjson");
        let plan = propose_sections(&s);
        let parts: Vec<Vec<&str>> = plan.iter().map(|g| g.parts.iter().map(String::as_str).collect()).collect();
        assert_eq!(parts, [vec!["C1", "C2", "R1", "R2", "U1"], vec!["C3", "C4", "R3", "R4", "U2"]], "{plan:?}");
        for g in &plan {
            s.requirements.groups.push(design_spec::PlaceGroup { name: g.name.clone(), parts: g.parts.clone(), section: true, ..Default::default() });
        }
        let l = crate::perfboard::perf_layout_with(&s, crate::stripboard::StripOptions::default());
        assert_eq!(l.sections.len(), 2);
        assert!(crate::stripboard::strip_check(&s, &l).is_empty(), "{:?}", crate::stripboard::strip_check(&s, &l));
        assert!(propose_sections(&load("ne555_blinker.schjson")).is_empty());
    }
}
