//! The moving dots on the breadboard while it runs (X1): the current along each jumper wire, each wire from a bench supply and
//! each strip of holes inside the board, from the pins' currents of the running circuit now.

use super::*;
use sch_core::flow::edge_flows;
use std::collections::HashMap;

/// An end of a piece the current runs along: a hole of the board, or pin `i` of off-board part `k` (its box at the left).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum End {
    Hole(Hole),
    Off(usize, usize),
}

/// A piece and its current from the first end to the second (amps); `inside`: the metal strip under the holes (drawn fainter).
pub(super) struct Piece {
    pub a: End,
    pub b: End,
    pub amps: f64,
    pub inside: bool,
}

/// The current along every wire and strip of board `l` with the circuit running now.
pub(super) fn board_flows(l: &Layout, nets: &BoardNets, lv: &Live) -> Vec<Piece> {
    let Some(parts) = lv.part_currents() else { return vec![] };
    let mut ids: HashMap<End, usize> = HashMap::new();
    let mut inject: Vec<f64> = vec![];
    let mut sink: Vec<bool> = vec![];
    let mut node = |e: End, inject: &mut Vec<f64>, sink: &mut Vec<bool>| {
        *ids.entry(e).or_insert_with(|| {
            inject.push(0.0);
            sink.push(false);
            inject.len() - 1
        })
    };
    // the current out of a pin: the part's current into the pin's net, shared among the part's pins on that net (the wire of an
    // off-board supply is a resistor named after the supply, `V1.+.RW`, so it is part of it: what the part puts into the net
    // already went through it)
    let pin_node = |refdes: &str, pin: &str| -> Option<usize> { lv.node_of(nets.pins.get(&(refdes.to_string(), pin.to_string()))?) };
    let mut share: HashMap<(String, usize), usize> = HashMap::new();
    let mut leads: Vec<(End, String, Option<usize>)> = vec![];
    for p in &l.parts {
        for (pin, h) in &p.leads {
            let n = pin_node(&p.refdes, pin);
            if let Some(n) = n {
                *share.entry((p.refdes.clone(), n)).or_default() += 1;
            }
            leads.push((End::Hole(*h), p.refdes.clone(), n));
        }
    }
    let mut edges: Vec<(End, End, bool)> = vec![];
    for (k, o) in l.off_board.iter().enumerate() {
        for (i, (_, net, h)) in o.pins.iter().enumerate() {
            let n = lv.node_of(net);
            if let Some(n) = n {
                *share.entry((o.refdes.clone(), n)).or_default() += 1;
            }
            leads.push((End::Off(k, i), o.refdes.clone(), n));
            edges.push((End::Off(k, i), End::Hole(*h), false));
        }
    }
    for (e, refdes, n) in &leads {
        let i = node(*e, &mut inject, &mut sink);
        match n.and_then(|n| Some(parts.get(refdes)?.get(n)? / share[&(refdes.clone(), n)] as f64)) {
            Some(a) => inject[i] += a,
            None => sink[i] = true,
        }
    }
    for w in &l.wires {
        edges.push((End::Hole(w.a), End::Hole(w.b), false));
    }
    // the holes in use joined along their strip or rail, in order
    let mut used: Vec<Hole> = edges.iter().flat_map(|(a, b, _)| [*a, *b]).chain(leads.iter().map(|l| l.0)).filter_map(|e| if let End::Hole(h) = e { Some(h) } else { None }).collect();
    used.sort_by_key(|h| (h.node(), h.col(), match h { Hole::Strip { row, .. } => *row, Hole::Rail { .. } => 0 }));
    used.dedup();
    for w in used.windows(2) {
        if w[0].node() == w[1].node() {
            edges.push((End::Hole(w[0]), End::Hole(w[1]), true));
        }
    }
    let idx: Vec<(usize, usize)> = edges.iter().map(|(a, b, _)| (node(*a, &mut inject, &mut sink), node(*b, &mut inject, &mut sink))).collect();
    let flow = edge_flows(inject.len(), &idx, &inject, &sink);
    edges.into_iter().zip(flow).map(|((a, b, inside), amps)| Piece { a, b, amps, inside }).collect()
}
