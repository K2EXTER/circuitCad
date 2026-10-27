//! Gate and pin swapping: the gates of a logic IC (four OR gates, six inverters) are alike, and so are the two inputs of a
//! gate, so the nets of their pins can trade places without changing the circuit. After the parts are placed, the trade that
//! makes the airwires shortest and crosses the fewest of them is looked for. Only gates with no copper on their pins yet are
//! traded (a track that ends on the pad would end on the wrong net).
//!
//! The trade is kept in `Board::pin_map` (pad pin -> the schematic pin whose net it carries), so that importing the schematic
//! again keeps it, and the comparison with the schematic knows it is meant.

use crate::model::*;
use crate::ratsnest::{join_groups, ratsnest, Airwire};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Which pins of a part may trade nets (written in the part's JSON).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SwapSpec {
    /// The alike gates, each as its pins in the same order (input, input, output): gate k's pin i may trade with gate m's pin i.
    pub gates: Vec<Vec<String>>,
    /// Positions inside a gate whose pins are alike (the two inputs of an OR gate: `[0, 1]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<usize>,
}

/// A crossing of two airwires is worth this many mm of airwire (it costs a via or a detour about that long).
const CROSSING_COST: f64 = 3.0;
/// A change must win at least this much (mm) to be taken.
const MIN_GAIN: f64 = 0.05;
/// More free gates than this are not tried in every order (a greedy search takes over).
const EXHAUSTIVE_UP_TO: usize = 6;

/// What `optimize` found for one part.
#[derive(Clone, Debug, PartialEq)]
pub struct SwapPlan {
    pub refdes: String,
    /// Pad pin -> the pin of the same part whose net it gets now (only pins that change).
    pub moves: BTreeMap<String, String>,
    /// Airwire length (mm) and number of crossings over the nets of the part's free gates, before and after.
    pub length: (f64, f64),
    pub crossings: (usize, usize),
}

/// What `run` did over the whole board.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SwapReport {
    pub plans: Vec<SwapPlan>,
    /// Parts with a swap spec that had no gate free to trade (copper on every gate's pads).
    pub locked: Vec<String>,
}

pub(crate) fn pad_has_copper(board: &Board, p: &Pad) -> bool {
    let (w, h) = p.shape.size();
    let on = |q: Point| (q.x - p.pos.x).abs() <= w / 2.0 + 1e-6 && (q.y - p.pos.y).abs() <= h / 2.0 + 1e-6;
    board.tracks.iter().any(|t| (p.drill.is_some() || t.side == p.side) && (on(t.a) || on(t.b))) || board.vias.iter().any(|v| on(v.pos))
}

/// The state searched for one part: which free gate sits in each slot, and which slots have their alike inputs crossed.
#[derive(Clone)]
struct Candidate {
    order: Vec<usize>,
    flip: Vec<bool>,
}

struct Ctx {
    refdes: String,
    spec: SwapSpec,
    /// Indices into `spec.gates` of the gates that may be traded.
    free: Vec<usize>,
    /// The net each pin of the free gates carries now.
    now: HashMap<String, String>,
    /// Pad positions of each pin of the free gates.
    at: HashMap<String, Vec<Point>>,
    /// The points (position, copper group) of the touched nets that the free gates do not decide.
    fixed: BTreeMap<String, Vec<(Point, usize)>>,
    /// The airwires of the nets the free gates have nothing to do with.
    others: Vec<Airwire>,
}

fn is_ground(net: &str) -> bool {
    crate::drc::is_ground_net(net)
}

/// How many pairs of airwires of different nets cross (ground nets are carried by a pour: left out).
pub(crate) fn cross_count(mine: &[Airwire], others: &[Airwire]) -> usize {
    let side = |o: Point, a: Point, b: Point| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let crosses = |p: &Airwire, q: &Airwire| {
        if p.net == q.net || is_ground(&p.net) || is_ground(&q.net) {
            return false;
        }
        side(p.a, p.b, q.a) * side(p.a, p.b, q.b) < 0.0 && side(q.a, q.b, p.a) * side(q.a, q.b, p.b) < 0.0
    };
    let mut n = 0;
    for (i, p) in mine.iter().enumerate() {
        n += others.iter().filter(|q| crosses(p, q)).count();
        n += mine[i + 1..].iter().filter(|q| crosses(p, q)).count();
    }
    n
}

impl Ctx {
    /// The pin that slot `slot`'s pin `i` takes its net from.
    fn source(&self, c: &Candidate, slot: usize, i: usize) -> &str {
        let gate = &self.spec.gates[self.free[c.order[slot]]];
        let mut pos = i;
        if c.flip[slot] && self.spec.inputs.len() == 2 {
            if i == self.spec.inputs[0] {
                pos = self.spec.inputs[1];
            } else if i == self.spec.inputs[1] {
                pos = self.spec.inputs[0];
            }
        }
        &gate[pos]
    }

    /// (pad pin, the pin it takes its net from) for every pin of the free gates.
    fn pairs(&self, c: &Candidate) -> Vec<(&str, &str)> {
        let mut out = vec![];
        for (slot, &gi) in self.free.iter().enumerate() {
            for (i, pin) in self.spec.gates[gi].iter().enumerate() {
                out.push((pin.as_str(), self.source(c, slot, i)));
            }
        }
        out
    }

    fn airwires(&self, c: &Candidate) -> Vec<Airwire> {
        let mut by_net = self.fixed.clone();
        let mut group = 1_000_000;
        for (pin, src) in self.pairs(c) {
            let net = &self.now[src];
            if let Some(spots) = self.at.get(pin) {
                for p in spots {
                    // a pad with no copper is a group of its own
                    by_net.entry(net.clone()).or_default().push((*p, group));
                    group += 1;
                }
            }
        }
        by_net.iter().flat_map(|(n, pts)| join_groups(n, pts)).collect()
    }

    fn cost(&self, c: &Candidate) -> (f64, usize) {
        let mine = self.airwires(c);
        (mine.iter().map(|w| w.a.dist(w.b)).sum(), cross_count(&mine, &self.others))
    }

    fn score(&self, c: &Candidate) -> f64 {
        let (l, n) = self.cost(c);
        l + CROSSING_COST * n as f64
    }

    fn identity(&self) -> Candidate {
        Candidate { order: (0..self.free.len()).collect(), flip: vec![false; self.free.len()] }
    }

    /// The best arrangement: gate orders (every one up to a few gates, else swaps of two) and input flips, in turn until nothing wins.
    fn search(&self) -> Candidate {
        let mut best = self.identity();
        let mut best_score = self.score(&best);
        for _ in 0..4 {
            let before = best_score;
            // the order of the gates
            let mut orders: Vec<Vec<usize>> = vec![];
            if self.free.len() <= EXHAUSTIVE_UP_TO {
                permutations(&mut (0..self.free.len()).collect::<Vec<_>>(), 0, &mut orders);
            } else {
                for i in 0..self.free.len() {
                    for j in i + 1..self.free.len() {
                        let mut o = best.order.clone();
                        o.swap(i, j);
                        orders.push(o);
                    }
                }
            }
            for o in orders {
                let c = Candidate { order: o, flip: best.flip.clone() };
                let s = self.score(&c);
                if s < best_score - 1e-9 {
                    best = c;
                    best_score = s;
                }
            }
            // the inputs of each gate
            if self.spec.inputs.len() == 2 {
                for slot in 0..self.free.len() {
                    let mut c = best.clone();
                    c.flip[slot] = !c.flip[slot];
                    let s = self.score(&c);
                    if s < best_score - 1e-9 {
                        best = c;
                        best_score = s;
                    }
                }
            }
            if best_score >= before - 1e-9 {
                break;
            }
        }
        best
    }
}

fn permutations(v: &mut Vec<usize>, k: usize, out: &mut Vec<Vec<usize>>) {
    if k == v.len() {
        out.push(v.clone());
        return;
    }
    for i in k..v.len() {
        v.swap(k, i);
        permutations(v, k + 1, out);
        v.swap(k, i);
    }
}

/// What the board would gain from trading the pins of `refdes` (nothing is changed). None: no spec, nothing free, or nothing wins.
fn plan_part(board: &Board, refdes: &str, spec: &SwapSpec) -> Result<Option<SwapPlan>, ()> {
    let pads_of = |pin: &str| -> Vec<usize> { (0..board.pads.len()).filter(|&i| board.pads[i].refdes == refdes && board.pads[i].pin == pin).collect() };
    let zone_nets: BTreeSet<&str> = board.zones.iter().map(|z| z.net.as_str()).collect();
    // a gate is free when all its pins have pads, with a net, a net that no pour carries, and no copper
    let free: Vec<usize> = (0..spec.gates.len())
        .filter(|&g| {
            spec.gates[g].iter().all(|pin| {
                let idx = pads_of(pin);
                !idx.is_empty() && idx.iter().all(|&i| !board.pads[i].net.is_empty() && !zone_nets.contains(board.pads[i].net.as_str()) && !pad_has_copper(board, &board.pads[i]))
            })
        })
        .collect();
    if free.is_empty() || (free.len() < 2 && spec.inputs.len() != 2) {
        return Err(());
    }
    let free_pins: BTreeSet<&str> = free.iter().flat_map(|&g| spec.gates[g].iter().map(|s| s.as_str())).collect();
    let mut now = HashMap::new();
    let mut at: HashMap<String, Vec<Point>> = HashMap::new();
    for pin in &free_pins {
        for i in pads_of(pin) {
            now.insert(pin.to_string(), board.pads[i].net.clone());
            at.entry(pin.to_string()).or_default().push(board.pads[i].pos);
        }
    }
    let touched: BTreeSet<&str> = now.values().map(|s| s.as_str()).collect();
    let conn = crate::drc::connectivity(board);
    let mut fixed: BTreeMap<String, Vec<(Point, usize)>> = BTreeMap::new();
    for (i, p) in board.pads.iter().enumerate() {
        let is_free = p.refdes == refdes && free_pins.contains(p.pin.as_str());
        if !is_free && touched.contains(p.net.as_str()) {
            fixed.entry(p.net.clone()).or_default().push((p.pos, conn.pad_group(i)));
        }
    }
    let others: Vec<Airwire> = ratsnest(board).into_iter().filter(|w| !touched.contains(w.net.as_str())).collect();
    let ctx = Ctx { refdes: refdes.to_string(), spec: spec.clone(), free, now, at, fixed, others };
    let start = ctx.identity();
    let (l0, n0) = ctx.cost(&start);
    let best = ctx.search();
    let (l1, n1) = ctx.cost(&best);
    if ctx.score(&best) > ctx.score(&start) - MIN_GAIN {
        return Ok(None);
    }
    let moves: BTreeMap<String, String> = ctx.pairs(&best).into_iter().filter(|(p, s)| p != s).map(|(p, s)| (p.to_string(), s.to_string())).collect();
    Ok(Some(SwapPlan { refdes: ctx.refdes.clone(), moves, length: (l0, l1), crossings: (n0, n1) }))
}

/// Make the trade of `plan` on the board: the pads of each pin take the net of the pin they get it from.
pub fn apply(board: &mut Board, plan: &SwapPlan) {
    let old: HashMap<String, String> = board.pads.iter().filter(|p| p.refdes == plan.refdes).map(|p| (p.pin.clone(), p.net.clone())).collect();
    for p in board.pads.iter_mut().filter(|p| p.refdes == plan.refdes) {
        if let Some(src) = plan.moves.get(&p.pin) {
            p.net = old[src].clone();
        }
    }
    let map = board.pin_map.entry(plan.refdes.clone()).or_default();
    let before = map.clone();
    for (pin, src) in &plan.moves {
        map.insert(pin.clone(), before.get(src).cloned().unwrap_or_else(|| src.clone()));
    }
    map.retain(|k, v| k != v);
    if map.is_empty() {
        board.pin_map.remove(&plan.refdes);
    }
}

/// Look for the best trade of every part that has a swap spec, and make it. `dry`: only report what would be traded.
pub fn run(board: &mut Board, dry: bool) -> SwapReport {
    let mut rep = SwapReport::default();
    let specs: Vec<(String, SwapSpec)> = board.swap_specs.iter().map(|(r, s)| (r.clone(), s.clone())).collect();
    for (refdes, spec) in specs {
        if !board.parts.iter().any(|p| p.refdes == refdes) {
            continue;
        }
        match plan_part(board, &refdes, &spec) {
            Err(()) => rep.locked.push(refdes),
            Ok(None) => {}
            Ok(Some(plan)) => {
                if !dry {
                    apply(board, &plan);
                }
                rep.plans.push(plan);
            }
        }
    }
    rep
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4-pin "IC" U1 with two alike gates (in 1, out 2 / in 3, out 4) at x = 10, and two resistors: R1 near the gate that
    /// sits at the bottom, R2 near the top one, wired the wrong way round.
    fn board() -> Board {
        let mut b = Board::new("t", 40.0, 40.0);
        b.parts.push(Part { refdes: "U1".into(), value: "x".into(), footprint: "x".into(), pos: Point::new(10.0, 10.0), rot: 0, side: Side::Top });
        for (pin, y, net) in [("1", 10.0, "IA"), ("2", 12.0, "OA"), ("3", 20.0, "IB"), ("4", 22.0, "OB")] {
            let mut p = Pad::new(Point::new(10.0, y), PadShape::Circle { d: 1.0 }, Some(0.6), Side::Top);
            p.refdes = "U1".into();
            p.pin = pin.into();
            p.net = net.into();
            b.pads.push(p);
        }
        for (r, x, y, nets) in [("R1", 30.0, 11.0, ["IB", "OB"]), ("R2", 30.0, 21.0, ["IA", "OA"])] {
            b.parts.push(Part { refdes: r.into(), value: "1k".into(), footprint: "x".into(), pos: Point::new(x, y), rot: 0, side: Side::Top });
            for (k, net) in nets.iter().enumerate() {
                let mut p = Pad::new(Point::new(x, y + k as f64 - 0.5), PadShape::Circle { d: 1.0 }, Some(0.6), Side::Top);
                p.refdes = r.into();
                p.pin = (k + 1).to_string();
                p.net = (*net).into();
                b.pads.push(p);
            }
        }
        b.swap_specs.insert("U1".into(), SwapSpec { gates: vec![vec!["1".into(), "2".into()], vec!["3".into(), "4".into()]], inputs: vec![] });
        b
    }

    #[test]
    fn gates_trade_places_when_that_shortens_the_airwires() {
        let mut b = board();
        let rep = run(&mut b, false);
        assert_eq!(rep.plans.len(), 1, "{rep:?}");
        let p = &rep.plans[0];
        assert!(p.length.1 < p.length.0 && p.crossings.1 <= p.crossings.0, "{p:?}");
        let net = |pin: &str| b.pads.iter().find(|q| q.refdes == "U1" && q.pin == pin).unwrap().net.clone();
        assert_eq!((net("1"), net("2"), net("3"), net("4")), ("IB".into(), "OB".into(), "IA".into(), "OA".into()));
        // the schematic pin whose net each pad carries is kept
        assert_eq!(b.pin_map["U1"].get("1").map(|s| s.as_str()), Some("3"));
        // a second run finds nothing more to win
        assert!(run(&mut b, false).plans.is_empty());
    }

    #[test]
    fn a_dry_run_changes_nothing() {
        let mut b = board();
        let before: Vec<String> = b.pads.iter().map(|p| p.net.clone()).collect();
        assert_eq!(run(&mut b, true).plans.len(), 1);
        assert_eq!(b.pads.iter().map(|p| p.net.clone()).collect::<Vec<_>>(), before);
        assert!(b.pin_map.is_empty());
    }

    #[test]
    fn a_gate_with_a_track_on_its_pad_stays() {
        let mut b = board();
        b.tracks.push(Track { side: Side::Top, a: Point::new(10.0, 10.0), b: Point::new(12.0, 10.0), width: 0.25 });
        let rep = run(&mut b, false);
        assert!(rep.plans.is_empty() && rep.locked == vec!["U1".to_string()], "{rep:?}");
    }

    #[test]
    fn the_inputs_of_a_gate_trade_when_that_uncrosses_the_wires() {
        let mut b = Board::new("t", 40.0, 40.0);
        b.parts.push(Part { refdes: "U1".into(), value: "x".into(), footprint: "x".into(), pos: Point::new(10.0, 10.0), rot: 0, side: Side::Top });
        for (pin, y, net) in [("1", 10.0, "A"), ("2", 14.0, "B"), ("3", 12.0, "Y")] {
            let mut p = Pad::new(Point::new(10.0, y), PadShape::Circle { d: 1.0 }, Some(0.6), Side::Top);
            p.refdes = "U1".into();
            p.pin = pin.into();
            p.net = net.into();
            b.pads.push(p);
        }
        // the sources sit the other way round: A is at the top, B at the bottom
        for (n, y) in [("A", 24.0), ("B", 2.0)] {
            let mut p = Pad::new(Point::new(30.0, y), PadShape::Circle { d: 1.0 }, Some(0.6), Side::Top);
            p.net = n.into();
            b.pads.push(p);
        }
        b.swap_specs.insert("U1".into(), SwapSpec { gates: vec![vec!["1".into(), "2".into(), "3".into()]], inputs: vec![0, 1] });
        let rep = run(&mut b, false);
        assert_eq!(rep.plans.len(), 1, "{rep:?}");
        let net = |pin: &str| b.pads.iter().find(|q| q.refdes == "U1" && q.pin == pin).unwrap().net.clone();
        assert_eq!((net("1"), net("2"), net("3")), ("B".into(), "A".into(), "Y".into()));
    }
}
