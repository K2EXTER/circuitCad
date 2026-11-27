//! The wiring made shorter once every net is wired (`reroute`): the nets are wired one after the other, each round the copper of the
//! nets before it, so an early net's wires may go a long way round a part that a later net's would not, and a late net's round the
//! early nets' wires. Each net's wires are taken off in turn and the net wired again alone, every other wire kept (`route` with
//! `Order::keep`): kept when it leaves no more nets apart and no more wires on top, and its copper is shorter (a turn counting as a
//! bit of length, so a straighter way wins at the same length).

use super::*;

/// How many times every net is taken off and wired again.
const PASSES: usize = 2;
/// What a piece of bare wire counts for in the length (holes): a turn where one piece meets the next, as the search's turn costs
/// about one and a half steps.
const PIECE: f64 = 1.5;

/// Wires each net again alone round the others' wires, keeping the shorter (`reroute`'s module). Only the wires from `from` (the
/// bare wires, the links) on are taken off: the ones before were on the board already (drawn by hand). The nets in `skip` are left
/// as they are (a Kelvin fork's, a bypass capacitor's, ground split by section: their wiring keeps a rule the plain wiring does not).
pub(super) fn reroute(s: &Schematic, c: &Circuit, l: &mut StripLayout, net_of_lead: &dyn Fn(&str, &str) -> String, from: (usize, usize), skip: &BTreeSet<String>, work: &std::cell::Cell<u64>) {
    let mut score = (crate::stripboard::strip_unjoined(s, l).len(), on_top(l), length(l));
    for _ in 0..PASSES {
        let mut better = false;
        let mut holes = net_of_holes(l, net_of_lead);
        for net in nets_wired(l, &holes, net_of_lead, from).into_iter().filter(|n| !skip.contains(n)) {
            let (bare, links) = wires_of(l, &holes, from, &net);
            if bare.is_empty() && links.is_empty() {
                continue;
            }
            let mut m = l.clone();
            let mut k = 0;
            m.bare.retain(|_| {
                k += 1;
                !bare.contains(&(k - 1))
            });
            let mut k = 0;
            m.links.retain(|_| {
                k += 1;
                !links.contains(&(k - 1))
            });
            route(c, &mut m, net_of_lead, Order { rails_first: true, escapes: false, first: &[], forks: &[], along: ALONG_COST, bypass: &[], keep: true, work });
            // the wires on top and the length first (the nets left apart, the dearest to find, only for a way that is better there)
            let cheap = (on_top(&m), length(&m));
            if score.0 == 0 && cheap >= (score.1, score.2) {
                continue;
            }
            let new = (crate::stripboard::strip_unjoined(s, &m).len(), cheap.0, cheap.1);
            if new < score {
                score = new;
                *l = m;
                holes = net_of_holes(l, net_of_lead);
                better = true;
            }
        }
        if !better {
            break;
        }
    }
}

/// The wires on top, one along a chip's row of pins counting three (as `rank`).
fn on_top(l: &StripLayout) -> usize {
    l.links.len() + 2 * links_along_pins(l).len()
}

/// The copper's length in tenths of a hole (45 degrees √2, each piece of bare wire `PIECE` more).
fn length(l: &StripLayout) -> u64 {
    let copper: f64 = l.bare.iter().map(|(a, b)| octile(*a, *b) + PIECE).sum::<f64>() + l.links.iter().map(|k| octile(k.a, k.b)).sum::<f64>();
    (copper * 10.0).round() as u64
}

type HoleNets = (BTreeMap<At, usize>, BTreeMap<usize, String>);

/// Each hole's net by the copper it is on: the net of a lead (or an off-board wire) on the same piece.
fn net_of_holes(l: &StripLayout, net_of_lead: &dyn Fn(&str, &str) -> String) -> HoleNets {
    let groups = l.hole_groups();
    let mut net_of_group: BTreeMap<usize, String> = BTreeMap::new();
    for p in &l.parts {
        for (pin, a) in &p.leads {
            let n = net_of_lead(&p.refdes, pin);
            if let (Some(g), false) = (groups.get(a), n.is_empty()) {
                net_of_group.entry(*g).or_insert(n);
            }
        }
    }
    for (_, v) in &l.off {
        for (_, n, a) in v {
            if let Some(g) = groups.get(a) {
                net_of_group.entry(*g).or_insert(n.clone());
            }
        }
    }
    (groups, net_of_group)
}

/// The nets with wires from `from` on, the longest copper first (the ones most likely to have gone round). Not a net already as
/// short as can be: no wire on top, its copper no longer than the shortest tree of its leads (`octile`), at most two pieces of bare
/// wire (one turn) a join.
fn nets_wired(l: &StripLayout, (groups, net_of_group): &HoleNets, net_of_lead: &dyn Fn(&str, &str) -> String, from: (usize, usize)) -> Vec<String> {
    // each net's length, pieces of bare wire, wires on top
    let mut wired: BTreeMap<String, (f64, usize, usize)> = BTreeMap::new();
    for (a, b) in &l.bare[from.0.min(l.bare.len())..] {
        if let Some(n) = groups.get(a).and_then(|g| net_of_group.get(g)) {
            let w = wired.entry(n.clone()).or_default();
            w.0 += octile(*a, *b);
            w.1 += 1;
        }
    }
    for k in &l.links[from.1.min(l.links.len())..] {
        let w = wired.entry(k.net.clone()).or_default();
        w.0 += octile(k.a, k.b);
        w.2 += 1;
    }
    let mut leads: BTreeMap<String, Vec<At>> = BTreeMap::new();
    for p in &l.parts {
        for (pin, a) in &p.leads {
            leads.entry(net_of_lead(&p.refdes, pin)).or_default().push(*a);
        }
    }
    for (_, v) in &l.off {
        for (_, n, a) in v {
            leads.entry(n.clone()).or_default().push(*a);
        }
    }
    let mut v: Vec<(String, f64)> = wired
        .into_iter()
        .filter(|(n, (len, pieces, top))| {
            let at = leads.get(n).map_or(&[][..], |v| &v[..]);
            *top > 0 || *pieces > 2 * at.len().saturating_sub(1) || *len > shortest_tree(at) + 1e-6
        })
        .map(|(n, w)| (n, w.0))
        .collect();
    v.sort_by(|a, b| b.1.total_cmp(&a.1));
    v.into_iter().map(|x| x.0).collect()
}

/// The length from `a` to `b` along the rows, the columns and at 45 degrees (holes).
fn octile(a: At, b: At) -> f64 {
    let (dr, dc) = (a.0.abs_diff(b.0) as f64, a.1.abs_diff(b.1) as f64);
    dr.max(dc) - dr.min(dc) + dr.min(dc) * std::f64::consts::SQRT_2
}

/// The shortest tree joining the holes (Prim's), by `octile`.
fn shortest_tree(at: &[At]) -> f64 {
    if at.is_empty() {
        return 0.0;
    }
    let mut inside = vec![false; at.len()];
    let mut d = vec![f64::MAX; at.len()];
    d[0] = 0.0;
    let mut sum = 0.0;
    for _ in 0..at.len() {
        let Some(i) = (0..at.len()).filter(|i| !inside[*i]).min_by(|a, b| d[*a].total_cmp(&d[*b])) else { break };
        inside[i] = true;
        sum += d[i];
        for j in 0..at.len() {
            if !inside[j] {
                d[j] = d[j].min(octile(at[i], at[j]));
            }
        }
    }
    sum
}

/// The bare wires and the links (by index, from `from` on) of `net`.
fn wires_of(l: &StripLayout, (groups, net_of_group): &HoleNets, from: (usize, usize), net: &str) -> (BTreeSet<usize>, BTreeSet<usize>) {
    let bare = (from.0..l.bare.len()).filter(|i| groups.get(&l.bare[*i].0).and_then(|g| net_of_group.get(g)).is_some_and(|n| n == net)).collect();
    let links = (from.1..l.links.len()).filter(|i| l.links[*i].net == net).collect();
    (bare, links)
}
