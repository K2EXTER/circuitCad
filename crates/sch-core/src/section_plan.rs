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
    let nets: Vec<Vec<usize>> = by_net.into_values().filter(|v| v.len() >= 2).map(|v| v.into_iter().collect()).collect();
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
fn cut(g: &[usize], parts: &[(String, usize)], nets: &[Vec<usize>]) -> Option<(Vec<usize>, Vec<usize>)> {
    if g.len() < 2 * MIN_PARTS {
        return None;
    }
    let inside: BTreeSet<usize> = g.iter().copied().collect();
    // the nets within the group (their parts here)
    let here: Vec<Vec<usize>> = nets.iter().map(|n| n.iter().copied().filter(|p| inside.contains(p)).collect::<Vec<usize>>()).filter(|n| n.len() >= 2).collect();
    if here.is_empty() {
        return None;
    }
    let local: BTreeMap<usize, usize> = g.iter().enumerate().map(|(k, p)| (*p, k)).collect();
    let here: Vec<Vec<usize>> = here.into_iter().map(|n| n.into_iter().map(|p| local[&p]).collect()).collect();
    let w: Vec<f64> = g.iter().map(|p| parts[*p].1 as f64).collect();
    let total: f64 = w.iter().sum();
    let mut of: Vec<Vec<usize>> = vec![vec![]; g.len()];
    for (e, n) in here.iter().enumerate() {
        for p in n {
            of[*p].push(e);
        }
    }
    let crossing = |side: &[bool]| here.iter().filter(|n| n.iter().any(|p| side[*p]) && n.iter().any(|p| !side[*p])).count();
    let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut best: Option<(usize, Vec<bool>)> = None;
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
        fm(&mut side, &w, &here, &of, total);
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
    if x > MAX_CUT || x as f64 > CUT_SHARE * here.len() as f64 {
        return None;
    }
    let a: Vec<usize> = g.iter().enumerate().filter(|(k, _)| !side[*k]).map(|(_, p)| *p).collect();
    let b: Vec<usize> = g.iter().enumerate().filter(|(k, _)| side[*k]).map(|(_, p)| *p).collect();
    Some((a, b))
}

/// Fiduccia–Mattheyses: each pass moves every part across once, the one that saves the most nets first, while each half's pins stay
/// within a fifth of half; back to the best point of the pass; until a pass saves nothing.
fn fm(side: &mut [bool], w: &[f64], nets: &[Vec<usize>], of: &[Vec<usize>], total: f64) {
    let n = side.len();
    let slack = total / 4.0;
    for _ in 0..8 {
        let mut count: Vec<[u32; 2]> = nets.iter().map(|e| [e.iter().filter(|p| !side[**p]).count() as u32, e.iter().filter(|p| side[**p]).count() as u32]).collect();
        let mut room0: f64 = (0..n).filter(|i| !side[*i]).map(|i| w[i]).sum();
        let mut locked = vec![false; n];
        let (mut run, mut best, mut best_at) = (0i32, 0i32, 0usize);
        let mut moves = vec![];
        for _ in 0..n {
            let gain = |i: usize, count: &[[u32; 2]]| -> i32 {
                let (from, to) = (usize::from(side[i]), usize::from(!side[i]));
                of[i].iter().map(|e| i32::from(count[*e][to] == 0 && count[*e][from] > 1) * -1 + i32::from(count[*e][from] == 1 && count[*e][to] > 0)).sum()
            };
            let pick = (0..n)
                .filter(|i| !locked[*i])
                .filter(|i| {
                    let r = if side[*i] { room0 + w[*i] } else { room0 - w[*i] };
                    (r - total / 2.0).abs() <= slack
                })
                .map(|i| (gain(i, &count), i))
                .max_by_key(|x| (x.0, std::cmp::Reverse(x.1)));
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
            if run > best {
                best = run;
                best_at = moves.len();
            }
        }
        for i in moves.drain(best_at..) {
            side[i] = !side[i];
        }
        if best <= 0 {
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
