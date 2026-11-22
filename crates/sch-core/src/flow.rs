//! Which way and how much current runs along each piece of wire, for the moving dots of the schematic and the breadboard.
//!
//! The simulation gives the current at every pin (what a part puts into its net); a net drawn as wires is a little graph of
//! points (pins, wire ends, labels) and segments between them. On a tree the current of each segment follows from Kirchhoff's
//! current law alone: cut the segment, and what the points on one side put in has to leave through it. A net drawn in pieces
//! joined by name (labels, power and ground symbols) has its pieces balanced at those names: what a piece's pins put in leaves
//! through its labels and symbols ("sinks"), shared evenly. Where wires make a loop the split between the two ways round
//! depends on the copper, not on the drawing: the solver takes one way (a spanning tree) and gives the closing segment none.

use crate::{on_segment, Pt, Schematic};
use std::collections::HashMap;

/// The current along each edge, from its first node to its second (negative: the other way), in amps.
///
/// `inject[i]`: what node `i` puts into the graph (a pin's current out of its part); `sink[i]`: the node can take or give
/// whatever is left over in its piece (a label, a power symbol, a pin whose current is not known).
pub fn edge_flows(n: usize, edges: &[(usize, usize)], inject: &[f64], sink: &[bool]) -> Vec<f64> {
    let mut adj: Vec<Vec<(usize, usize)>> = vec![vec![]; n];
    for (k, &(a, b)) in edges.iter().enumerate() {
        if a != b {
            adj[a].push((b, k));
            adj[b].push((a, k));
        }
    }
    let mut flow = vec![0.0; edges.len()];
    let mut seen = vec![false; n];
    for root in 0..n {
        if seen[root] {
            continue;
        }
        // one piece: a breadth-first tree from `root` (the order and each node's parent edge)
        let mut order = vec![root];
        let mut parent: HashMap<usize, (usize, usize)> = HashMap::new();
        seen[root] = true;
        let mut i = 0;
        while i < order.len() {
            let u = order[i];
            i += 1;
            for &(v, k) in &adj[u] {
                if !seen[v] {
                    seen[v] = true;
                    parent.insert(v, (u, k));
                    order.push(v);
                }
            }
        }
        // what is left over in the piece goes to its sinks, shared evenly
        let mut net: HashMap<usize, f64> = order.iter().map(|&u| (u, inject.get(u).copied().unwrap_or(0.0))).collect();
        let total: f64 = net.values().sum();
        let sinks: Vec<usize> = order.iter().copied().filter(|&u| sink.get(u).copied().unwrap_or(false)).collect();
        if !sinks.is_empty() {
            for s in &sinks {
                *net.get_mut(s).unwrap() -= total / sinks.len() as f64;
            }
        }
        // leaves first: what a subtree puts in flows out through its parent edge
        for &u in order.iter().rev() {
            let Some(&(p, k)) = parent.get(&u) else { continue };
            let out = net[&u];
            flow[k] = if edges[k].0 == u { out } else { -out };
            *net.get_mut(&p).unwrap() += out;
        }
    }
    flow
}

/// A straight piece of a drawn wire and the current along it, from `a` to `b` (negative: from `b` to `a`), in amps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WireFlow {
    pub a: Pt,
    pub b: Pt,
    pub amps: f64,
}

/// The current along every piece of the schematic's wires. `pin_current(refdes, pin)` is the current the part puts out of
/// that pin into its wire (None when it is not known: a block's pin, a part the simulation has no figure for); labels and
/// power / ground symbols balance each piece of a net. Wires are cut at every pin, wire end and label lying on them.
pub fn wire_flows(s: &Schematic, pin_current: impl Fn(&str, &str) -> Option<f64>) -> Vec<WireFlow> {
    let mut ids: HashMap<Pt, usize> = HashMap::new();
    let mut inject: Vec<f64> = vec![];
    let mut sink: Vec<bool> = vec![];
    let node = |p: Pt, ids: &mut HashMap<Pt, usize>, inject: &mut Vec<f64>, sink: &mut Vec<bool>| {
        *ids.entry(p).or_insert_with(|| {
            inject.push(0.0);
            sink.push(false);
            inject.len() - 1
        })
    };
    for c in &s.components {
        for (pin, p) in c.pins() {
            let i = node(p, &mut ids, &mut inject, &mut sink);
            match (c.kind.is_power_symbol(), pin_current(&c.refdes, &pin)) {
                (false, Some(a)) => inject[i] += a,
                _ => sink[i] = true,
            }
        }
    }
    for l in &s.labels {
        let i = node(l.pos, &mut ids, &mut inject, &mut sink);
        sink[i] = true;
    }
    for w in &s.wires {
        node(w.a, &mut ids, &mut inject, &mut sink);
        node(w.b, &mut ids, &mut inject, &mut sink);
    }
    // each wire cut at the known points on it, in order from `a`
    let mut pieces: Vec<(Pt, Pt)> = vec![];
    for w in &s.wires {
        let mut on: Vec<Pt> = ids.keys().copied().filter(|&p| on_segment(p, w.a, w.b)).collect();
        let d = |p: Pt| (p.0 - w.a.0).abs() + (p.1 - w.a.1).abs();
        on.sort_by_key(|&p| d(p));
        on.dedup();
        pieces.extend(on.windows(2).map(|q| (q[0], q[1])));
    }
    let edges: Vec<(usize, usize)> = pieces.iter().map(|(a, b)| (ids[a], ids[b])).collect();
    let flow = edge_flows(inject.len(), &edges, &inject, &sink);
    pieces.into_iter().zip(flow).map(|((a, b), amps)| WireFlow { a, b, amps }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, Kind, Wire};

    fn comp(kind: Kind, refdes: &str, value: &str, pos: Pt, rot: u8) -> Component {
        Component { refdes: refdes.into(), kind, value: value.into(), pos, rot, footprint: String::new(), custom: None, sim_only: false, pin_names: vec![], theta: 0.0, tc_ppm: 0.0, cth: 0.0, t_max: 0.0, zth: vec![], theta_jc: 0.0, heatsink: 0.0, sub: None, shops: Default::default() }
    }

    #[test]
    fn a_chain_carries_what_its_end_puts_in() {
        // 0 —— 1 —— 2: node 0 puts in 2 mA, node 2 takes it
        let f = edge_flows(3, &[(0, 1), (1, 2)], &[2e-3, 0.0, -2e-3], &[false; 3]);
        assert_eq!(f, vec![2e-3, 2e-3]);
        // the same with the second edge written backwards
        let f = edge_flows(3, &[(0, 1), (2, 1)], &[2e-3, 0.0, -2e-3], &[false; 3]);
        assert_eq!(f, vec![2e-3, -2e-3]);
    }

    #[test]
    fn a_branch_splits_and_a_sink_takes_the_rest() {
        //      1 (takes 1 mA)
        //      |
        // 0 —— 3 —— 2 (takes 2 mA); node 0 is a power symbol (a sink) that gives what the loads take
        let f = edge_flows(4, &[(0, 3), (3, 1), (3, 2)], &[0.0, -1e-3, -2e-3, 0.0], &[true, false, false, false]);
        assert!((f[0] - 3e-3).abs() < 1e-15 && (f[1] - 1e-3).abs() < 1e-15 && (f[2] - 2e-3).abs() < 1e-15, "{f:?}");
        // a loop: the closing edge carries nothing, the current still gets round
        let f = edge_flows(3, &[(0, 1), (1, 2), (2, 0)], &[1.0, 0.0, -1.0], &[false; 3]);
        assert_eq!(f.iter().filter(|x| **x != 0.0).count(), 1, "{f:?}");
    }

    /// A 1 kΩ resistor from a 5 V power symbol to ground, drawn with a wire on each side: the current runs from the supply to
    /// the resistor on the top wire and from the resistor to ground on the bottom one.
    #[test]
    fn the_wires_of_a_resistor_carry_its_current() {
        let mut s = Schematic::default();
        let r = comp(Kind::Resistor, "R1", "1k", (0, 0), 1);
        let pins = r.pins();
        let (top, bottom) = (pins[0].1, pins[1].1);
        let (up, down) = ((top.0, top.1 - 4), (bottom.0, bottom.1 + 4));
        let (hi, lo) = if top.1 < bottom.1 { (top, bottom) } else { (bottom, top) };
        let (pa, pb) = if top.1 < bottom.1 { (up, down) } else { (down, up) };
        let vcc = comp(Kind::Power, "#PWR1", "5V", pa, 0);
        let gnd = comp(Kind::Ground, "#GND1", "", pb, 0);
        s.components = vec![r, vcc, gnd];
        s.wires = vec![Wire { a: pa, b: hi }, Wire { a: lo, b: pb }];
        // 5 mA into the resistor at its upper pin (it takes from that net: −5 mA out of that pin), 5 mA out at the lower one
        let upper = if top.1 < bottom.1 { pins[0].0.clone() } else { pins[1].0.clone() };
        let f = wire_flows(&s, |refdes, pin| (refdes == "R1").then(|| if pin == upper { -5e-3 } else { 5e-3 }));
        assert_eq!(f.len(), 2);
        assert_eq!((f[0].a, f[0].b), (pa, hi));
        assert!((f[0].amps - 5e-3).abs() < 1e-12, "supply to resistor: {f:?}");
        assert!((f[1].amps - 5e-3).abs() < 1e-12, "resistor to ground: {f:?}");
    }
}
