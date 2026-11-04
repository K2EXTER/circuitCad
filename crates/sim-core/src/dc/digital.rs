//! Digital cells (`Ce::Cell`): gates, flip-flops, latches and Schmitt triggers that are evaluated outside the circuit equations.
//!
//! A node that only cells touch is a *digital net*: its value is a 0 / 1 in `Dig::lv`, and the circuit equations see it as a node
//! with nothing but gmin (its voltage is written back into the solution afterwards, `fix_nodes`). A cell output on a node that
//! other elements touch (a resistor, an LED, a source) is a source behind `rout` at the level of the cell (Norton form, so the
//! matrix does not change when the level does). The inputs of a cell on such a node are the node voltage compared with the
//! threshold, as a fraction of the cell's supply.
//!
//! After every accepted solution `dig_update` refreshes the supply voltages and the analog inputs, then lets the cells settle in
//! rounds (delta cycles): the cells whose inputs changed are evaluated together, their outputs are applied together, and the
//! cells that read those outputs are evaluated in the next round. A rising clock edge samples the data as it was in the previous
//! round. Zero delay: a loop that does not settle stops after a limit of rounds.

use super::*;
use crate::CellKind;
use CellKind::*;

pub(crate) struct DCell {
    pub name: String,
    pub kind: CellKind,
    pub ins: Vec<usize>,
    /// Index of the first input in the per-input arrays of `Dig`.
    pub slot0: usize,
    pub out: usize,
    pub outb: usize,
    pub vss: usize,
    pub vcc: usize,
    pub rout: f64,
    pub vt: f64,
    pub vh: f64,
    pub set_low: bool,
    pub reset_low: bool,
    pub tplh: f64,
    pub tphl: f64,
    pub tsu: f64,
    pub th: f64,
    /// The output enable node, whether it enables when low, whether the output is open-drain.
    pub oe: Option<usize>,
    pub oe_low: bool,
    pub od: bool,
    /// The look-up table of a `Lut` cell.
    pub table: Option<std::sync::Arc<Vec<bool>>>,
}

#[derive(Default)]
pub(crate) struct Dig {
    /// The logic value of every node a cell drives or reads (digital nets: the value of the net).
    pub lv: Vec<bool>,
    /// Per input: the level at the last evaluation of its cell.
    pub seen: Vec<bool>,
    /// Per input on an analog node: the input as a fraction of the cell's supply.
    pub afrac: Vec<f64>,
    /// Per cell: the stored state of a memory kind.
    pub st: Vec<bool>,
    /// Per cell: the output q (qb is its inverse).
    pub q: Vec<bool>,
    /// Per supply group (cells that share the same vss and vcc pins): (vss, vcc) from the last accepted solution.
    pub gsup: Vec<(f64, f64)>,
    pub started: bool,
    /// Scratch for `dig_update`: which gates are in the queue (all false between two calls).
    pub queued: Vec<bool>,
    /// Per cell: whether it has been evaluated (and its outputs applied) at least once.
    pub done_first: Vec<bool>,
    /// Per cell: the output it is heading for after its propagation delay, when it is not there yet, and when it gets there.
    pub pend: Vec<Option<bool>>,
    pub pend_due: Vec<f64>,
    /// Per cell: the number of the latest schedule (an event with another number is out of date: the output changed its mind).
    pub ver: Vec<u64>,
    /// The events to come: (time bits, order, kind 0 = an output arrives / 1 = a cell reads its inputs, cell, schedule number).
    pub heap: std::collections::BinaryHeap<std::cmp::Reverse<(u64, u64, u8, usize, u64)>>,
    pub seq: u64,
    /// The time of the last `dig_update`.
    pub t_prev: f64,
    /// Per input: when its level last changed (for the setup check); per cell: when its clock last rose (for the hold check).
    pub last_change: Vec<f64>,
    pub last_edge: Vec<f64>,
    /// The setup / hold violations found, one entry per cell and kind.
    pub viol: Vec<crate::TimingViolation>,
    /// The smallest margin found at a clock edge: (cell, margin, time between this clock edge and the one before).
    pub tightest: Option<(String, f64, f64)>,
    /// Per cell: whether its output is enabled (always, without an output enable).
    pub en: Vec<bool>,
    /// The cells whose output enable changed in the evaluation that is going on.
    pub pending_en: Vec<usize>,
    /// Per node: 0 = fine, 1 = two outputs fight, 2 = no output drives it. And the findings, grouped by node and kind:
    /// (node, kind, count, time of the first).
    pub bus_state: Vec<u8>,
    pub bus: Vec<(usize, u8, usize, f64)>,
    /// Counts the changes of which outputs sit on an analog node and push (a change makes another matrix).
    pub epoch: u64,
}

/// What the cells need to know that does not change during a run: which cells share a supply, which read an analog node.
#[derive(Default)]
pub(crate) struct DigStatic {
    /// The distinct (vss, vcc) pairs of the cells.
    pub groups: Vec<(usize, usize)>,
    /// Per cell: its index in `groups`.
    pub cell_group: Vec<usize>,
    /// The cells with an input on an analog node: the only ones whose inputs can change by themselves between two solutions.
    pub analog_cells: Vec<usize>,
    /// Per cell: whether an output sits on an analog node (a source behind `rout` in the circuit equations).
    pub analog_out: Vec<bool>,
    /// The cells with such an output.
    pub analog_out_cells: Vec<usize>,
}

/// What the cells need to know about the nodes, fixed when the circuit is compiled.
#[derive(Default)]
pub(crate) struct DigNets {
    /// Per node: only cells touch it.
    pub digital: Vec<bool>,
    /// Per node: the cells that read it.
    pub fanout: Vec<Vec<usize>>,
    /// Per digital node: the first cell that drives it and whether through `outb` (its supply sets the level of the net).
    pub driver: Vec<Option<(usize, bool)>>,
    /// Per digital node: every cell that drives it.
    pub drivers: Vec<Vec<(usize, bool)>>,
    /// Per digital node: one driver that is always on and not open-drain.
    pub simple: Vec<bool>,
}

/// The queues of one `dig_update`.
struct Work {
    comb: std::collections::VecDeque<usize>,
    queued: Vec<bool>,
    mem: Vec<usize>,
    any_change: bool,
    analog_changed: bool,
}

fn is_memory(k: CellKind) -> bool {
    !matches!(k, CellKind::And | CellKind::Nand | CellKind::Or | CellKind::Nor | CellKind::Xor | CellKind::Xnor | CellKind::Buf | CellKind::Inv | CellKind::Lut)
}

impl Compiled {
    pub(crate) fn has_cells(&self) -> bool {
        !self.cells.is_empty()
    }

    /// Whether node `n` is a digital net.
    pub(crate) fn is_digital(&self, n: usize) -> bool {
        self.dnets.digital.get(n).copied().unwrap_or(false)
    }

    fn level(&self, d: &Dig, ci: usize, k: usize) -> bool {
        let c = &self.cells[ci];
        let n = c.ins[k];
        if n == 0 {
            false
        } else if self.is_digital(n) {
            d.lv[n]
        } else {
            d.afrac[c.slot0 + k] > c.vt
        }
    }

    /// The level of the output enable input of cell `ci` (only for a cell that has one).
    fn oe_level(&self, d: &Dig, ci: usize) -> bool {
        let c = &self.cells[ci];
        match c.oe {
            Some(n) if n != 0 && self.is_digital(n) => d.lv[n],
            Some(n) if n != 0 => d.afrac[c.slot0 + c.ins.len()] > c.vt,
            _ => false,
        }
    }

    /// Whether output `inverted` of cell `ci` drives its node at all (an output that is not enabled, or an open-drain one that is
    /// high, lets go).
    fn drives(&self, d: &Dig, ci: usize, inverted: bool) -> bool {
        let c = &self.cells[ci];
        d.en[ci] && !(c.od && (d.q[ci] != inverted))
    }

    /// Evaluate one cell: its next output. `before` are the levels seen at the last evaluation (for the clock edge and the
    /// data sampled by it).
    fn eval(&self, d: &mut Dig, ci: usize, te: Option<f64>) -> bool {
        let c = &self.cells[ci];
        let now: Vec<bool> = (0..c.ins.len()).map(|k| self.level(d, ci, k)).collect();
        let before: Vec<bool> = (0..c.ins.len()).map(|k| d.seen[c.slot0 + k]).collect();
        let started = d.started;
        for (k, l) in now.iter().enumerate() {
            d.seen[c.slot0 + k] = *l;
        }
        if c.oe.is_some() {
            let lvl = self.oe_level(d, ci);
            d.seen[c.slot0 + c.ins.len()] = lvl;
            let en = lvl != c.oe_low;
            if en != d.en[ci] {
                d.en[ci] = en;
                d.pending_en.push(ci);
            }
        }
        // the data inputs of an edge-triggered flip-flop (timing checks)
        let data: &[usize] = match c.kind {
            Dff | Tff => &[0],
            Jkff | Srff => &[0, 1],
            _ => &[],
        };
        if let Some(te) = te {
            for k in 0..c.ins.len() {
                if now[k] != before[k] {
                    // a data input that changes soon after the clock edge breaks the hold time
                    if c.th > 0.0 && data.contains(&k) {
                        let le = d.last_edge[ci];
                        if te > le && te - le < c.th - 1e-15 {
                            self.note_violation(d, ci, crate::TimingKind::Hold, te, te - le, c.th);
                        }
                    }
                    d.last_change[c.slot0 + k] = te;
                }
            }
        }
        let at = |v: &Vec<bool>, k: usize| v.get(k).copied().unwrap_or(false);
        let count = now.iter().filter(|b| **b).count();
        match c.kind {
            And => !now.is_empty() && count == now.len(),
            Nand => !(!now.is_empty() && count == now.len()),
            Or => count > 0,
            Nor => count == 0,
            Xor => count % 2 == 1,
            Xnor => count % 2 == 0,
            Buf => at(&now, 0),
            Inv => !at(&now, 0),
            Lut => {
                let addr = now.iter().enumerate().fold(0usize, |a, (k, b)| a | (usize::from(*b) << k));
                c.table.as_ref().and_then(|t| t.get(addr)).copied().unwrap_or(false)
            }
            _ => {
                let old = d.st[ci];
                let mut new = old;
                let (set_at, reset_at, clk_at) = match c.kind {
                    Dff | Tff | Dlatch => (2, 3, Some(1)),
                    Jkff | Srff => (3, 4, Some(2)),
                    Srlatch => (3, 4, None),
                    _ => (usize::MAX, usize::MAX, None),
                };
                let active = |k: usize, low: bool| k != usize::MAX && c.ins.len() > k && (at(&now, k) != low);
                let (set, reset) = (active(set_at, c.set_low), active(reset_at, c.reset_low));
                if set || reset {
                    new = set && !reset || (set && reset && old);
                } else {
                    let edge = clk_at.is_some_and(|k| started && !at(&before, k) && at(&now, k));
                    if edge && let Some(te) = te {
                        self.timing_edge(d, ci, te, data);
                    }
                    match c.kind {
                        Dff if edge => new = at(&before, 0),
                        Tff if edge && at(&before, 0) => new = !old,
                        Jkff if edge => match (at(&before, 0), at(&before, 1)) {
                            (true, true) => new = !old,
                            (true, false) => new = true,
                            (false, true) => new = false,
                            _ => {}
                        },
                        Srff => {
                            let level_mode = c.ins.get(2) == Some(&0);
                            if level_mode || edge {
                                let (s, r) = if level_mode { (at(&now, 0), at(&now, 1)) } else { (at(&before, 0), at(&before, 1)) };
                                match (s, r) {
                                    (true, false) => new = true,
                                    (false, true) => new = false,
                                    _ => {}
                                }
                            }
                        }
                        Dlatch if at(&now, 1) => new = at(&now, 0),
                        Srlatch if at(&now, 2) || c.ins.get(2) == Some(&0) => match (at(&now, 0), at(&now, 1)) {
                            (true, false) => new = true,
                            (false, true) => new = false,
                            _ => {}
                        },
                        Schmitt => {
                            let n = c.ins[0];
                            if n == 0 {
                                new = false;
                            } else if self.is_digital(n) {
                                new = d.lv[n];
                            } else {
                                let f = d.afrac[c.slot0];
                                if f > c.vt + c.vh {
                                    new = true;
                                } else if f < c.vt - c.vh {
                                    new = false;
                                }
                            }
                        }
                        _ => {}
                    }
                }
                d.st[ci] = new;
                new
            }
        }
    }

    /// Put the output of cell `ci` at `q` now: the digital nets it drives change, and the cells that read them are queued.
    fn apply_out(&self, d: &mut Dig, w: &mut Work, ci: usize, q: bool, te: Option<f64>, mem_next: &mut Vec<usize>) {
        let c = &self.cells[ci];
        let force = !d.done_first[ci];
        d.done_first[ci] = true;
        let changed = d.q[ci] != q;
        let was = [self.drives(d, ci, false), self.drives(d, ci, true)];
        d.q[ci] = q;
        if changed {
            w.any_change = true;
        }
        for (k, (node, inverted)) in [(c.out, false), (c.outb, true)].into_iter().enumerate() {
            if node == 0 {
                continue;
            }
            if self.is_digital(node) {
                self.recompute_node(d, w, node, force, te, mem_next);
            } else if changed || force {
                w.analog_changed = true;
                // an open-drain output that lets go, or takes hold, makes another matrix
                if was[k] != self.drives(d, ci, inverted) {
                    d.epoch += 1;
                }
            }
        }
    }

    /// The value of digital net `node` from its drivers: one driver that is always on is the net; with several, the ones that are
    /// not off decide (two that disagree: a fight, the net goes low; none: the net keeps its value). The cells that read it are
    /// queued when it changes.
    fn recompute_node(&self, d: &mut Dig, w: &mut Work, node: usize, force: bool, te: Option<f64>, mem_next: &mut Vec<usize>) {
        let drivers = &self.dnets.drivers[node];
        let new = if self.dnets.simple[node] {
            let (ci, inv) = drivers[0];
            d.q[ci] != inv
        } else {
            let (mut lo, mut hi) = (false, false);
            for &(ci, inv) in drivers {
                if self.drives(d, ci, inv) {
                    if d.q[ci] != inv {
                        hi = true;
                    } else {
                        lo = true;
                    }
                }
            }
            let state = match (lo, hi) {
                (true, true) => 1,
                (false, false) => 2,
                _ => 0,
            };
            // only a net that something reads matters when it floats
            let state = if state == 2 && self.dnets.fanout[node].is_empty() { 0 } else { state };
            if state != d.bus_state[node] {
                d.bus_state[node] = state;
                if state != 0 {
                    match d.bus.iter_mut().find(|b| b.0 == node && b.1 == state) {
                        Some(b) => b.2 += 1,
                        None => d.bus.push((node, state, 1, te.unwrap_or(0.0))),
                    }
                }
            }
            match (lo, hi) {
                (true, true) | (true, false) => false,
                (false, true) => true,
                (false, false) => d.lv[node],
            }
        };
        if d.lv[node] != new || force {
            d.lv[node] = new;
            for &f in &self.dnets.fanout[node] {
                if is_memory(self.cells[f].kind) {
                    mem_next.push(f);
                } else if !w.queued[f] {
                    w.queued[f] = true;
                    w.comb.push_back(f);
                }
            }
        }
    }

    /// The cells whose output enable changed: their nets are worked out again.
    fn flush_en(&self, d: &mut Dig, w: &mut Work, te: Option<f64>, mem_next: &mut Vec<usize>) {
        let list = std::mem::take(&mut d.pending_en);
        for ci in list {
            let c = &self.cells[ci];
            for (node, _) in [(c.out, false), (c.outb, true)] {
                if node == 0 {
                    continue;
                }
                if self.is_digital(node) {
                    self.recompute_node(d, w, node, false, te, mem_next);
                } else {
                    d.epoch += 1;
                    w.analog_changed = true;
                }
            }
        }
    }

    /// Let cell `ci` head for `q`: at once when it has no delay (or this is the first evaluation, or the run is static), otherwise
    /// as an event after its propagation delay. A change that is taken back before the delay is over never happens (inertial).
    fn emit(&self, d: &mut Dig, w: &mut Work, ci: usize, q: bool, te: f64, delays: bool, mem_next: &mut Vec<usize>) {
        let c = &self.cells[ci];
        let first = !d.done_first[ci];
        let td = if !delays || first { 0.0 } else if q { c.tplh } else { c.tphl };
        if td <= 0.0 {
            if d.pend[ci].take().is_some() {
                d.ver[ci] += 1;
            }
            self.apply_out(d, w, ci, q, delays.then_some(te), mem_next);
        } else if q == d.q[ci] {
            if d.pend[ci].take().is_some() {
                d.ver[ci] += 1;
            }
        } else if d.pend[ci] != Some(q) {
            d.pend[ci] = Some(q);
            d.ver[ci] += 1;
            d.pend_due[ci] = te + td;
            d.seq += 1;
            let item = std::cmp::Reverse(((te + td).to_bits(), d.seq, 0u8, ci, d.ver[ci]));
            d.heap.push(item);
        }
    }

    /// Settle the cells queued in `w` at time `te`: the gates one after the other, then the flip-flops and latches together,
    /// until nothing changes.
    fn settle(&self, d: &mut Dig, w: &mut Work, te: f64, delays: bool) {
        let mut budget = 200 * self.cells.len() + 10_000;
        loop {
            // phase 1: the gates, one after the other
            let mut mem_next: Vec<usize> = vec![];
            while let Some(ci) = w.comb.pop_front() {
                w.queued[ci] = false;
                if budget == 0 {
                    for ci in w.comb.drain(..) {
                        w.queued[ci] = false;
                    }
                    return;
                }
                budget -= 1;
                let q = self.eval(d, ci, delays.then_some(te));
                let heading = d.pend[ci].unwrap_or(d.q[ci]);
                if !d.done_first[ci] || q != heading {
                    self.emit(d, w, ci, q, te, delays, &mut mem_next);
                }
                self.flush_en(d, w, delays.then_some(te), &mut mem_next);
            }
            w.mem.extend(mem_next);
            w.mem.sort_unstable();
            w.mem.dedup();
            if w.mem.is_empty() {
                return;
            }
            // phase 2: the flip-flops and latches whose inputs changed, together
            let batch = std::mem::take(&mut w.mem);
            if budget < batch.len() {
                return;
            }
            budget -= batch.len();
            let results: Vec<(usize, bool)> = batch.iter().map(|&ci| (ci, self.eval(d, ci, delays.then_some(te)))).collect();
            let mut mem_next: Vec<usize> = vec![];
            for (ci, q) in results {
                let heading = d.pend[ci].unwrap_or(d.q[ci]);
                if !d.done_first[ci] || q != heading {
                    self.emit(d, w, ci, q, te, delays, &mut mem_next);
                }
            }
            self.flush_en(d, w, delays.then_some(te), &mut mem_next);
            w.mem = mem_next;
            w.mem.sort_unstable();
            w.mem.dedup();
        }
    }

    /// The start of a transient run: settle the cells on the initial solution `x` without delays (as the operating point), then
    /// let the gates that are not at what their inputs say (a ring of an odd number of inverters has no such state) head for it
    /// with their delays, so that the run starts them going.
    pub(crate) fn dig_begin_run(&self, x: &[f64]) {
        if self.cells.is_empty() {
            return;
        }
        self.dig_update(x, None);
        let mut guard = self.dig.borrow_mut();
        let d = &mut *guard;
        let mut w = Work { comb: Default::default(), queued: std::mem::take(&mut d.queued), mem: vec![], any_change: false, analog_changed: false };
        w.queued.resize(self.cells.len(), false);
        let mut mem_next: Vec<usize> = vec![];
        for ci in 0..self.cells.len() {
            if is_memory(self.cells[ci].kind) {
                continue;
            }
            let q = self.eval(d, ci, None);
            let heading = d.pend[ci].unwrap_or(d.q[ci]);
            if q != heading {
                self.emit(d, &mut w, ci, q, 0.0, true, &mut mem_next);
            }
        }
        self.flush_en(d, &mut w, None, &mut mem_next);
        d.queued = w.queued;
    }

    /// A clock edge of flip-flop `ci` at `te`: the setup time of its data inputs, and the margin that is left.
    fn timing_edge(&self, d: &mut Dig, ci: usize, te: f64, data: &[usize]) {
        let c = &self.cells[ci];
        let interval = te - d.last_edge[ci];
        d.last_edge[ci] = te;
        if c.tsu <= 0.0 {
            return;
        }
        for &k in data {
            let dt = te - d.last_change[c.slot0 + k];
            if dt < c.tsu - 1e-15 {
                self.note_violation(d, ci, crate::TimingKind::Setup, te, dt, c.tsu);
            }
            // the margin to the setup time (how much later the data could arrive); the time since the clock edge before is NaN
            // for the first edge
            let margin = dt - c.tsu;
            if d.tightest.as_ref().is_none_or(|t| margin < t.1) {
                d.tightest = Some((c.name.clone(), margin, if interval < 1e29 { interval } else { f64::NAN }));
            }
        }
    }

    fn note_violation(&self, d: &mut Dig, ci: usize, kind: crate::TimingKind, time: f64, margin: f64, required: f64) {
        let name = &self.cells[ci].name;
        if let Some(v) = d.viol.iter_mut().find(|v| v.cell == *name && v.kind == kind) {
            v.count += 1;
            if margin < v.worst_margin {
                v.worst_margin = margin;
            }
        } else {
            d.viol.push(crate::TimingViolation { cell: name.clone(), kind, count: 1, first_time: time, worst_margin: margin, required });
        }
    }

    /// What the timing checks found in this run.
    pub(crate) fn timing_report(&self) -> crate::TimingReport {
        let d = self.dig.borrow();
        crate::TimingReport { violations: d.viol.clone(), tightest: d.tightest.clone() }
    }

    /// The time of the next change of an output that sits on an analog node: the transient run lands a step on it.
    pub(crate) fn dig_next_event(&self) -> Option<f64> {
        let d = self.dig.borrow();
        self.dstat.analog_out_cells.iter().filter(|&&ci| d.pend[ci].is_some()).map(|&ci| d.pend_due[ci]).min_by(|a, b| a.total_cmp(b))
    }

    /// Take the accepted solution `x` (at time `time`; `None` = a static solution: the delays are left out) in: refresh the
    /// supplies and the analog inputs, then settle the cells. Returns whether the circuit equations have to be solved again (an
    /// output on an analog node changed, or a supply moved enough to matter) and whether any cell output changed at all.
    ///
    /// The cells settle in two phases, repeated until nothing changes: the gates (no memory) one after the other, each output
    /// applied at once, so a latch built from gates falls into one of its states instead of ringing; then every flip-flop and
    /// latch whose inputs changed, together, on the levels the gates left (a flip-flop samples what its data input was before the
    /// clock edge, so a shift register shifts by one).
    ///
    /// With a time, the cells that read an analog node are looked at at the moment their input crossed the threshold (found by
    /// linear interpolation between this and the last call), an output with a delay arrives as an event, and the events up to
    /// `time` are processed in order. Between two calls the digital part keeps its own time exactly; only what crosses to or from
    /// an analog node is quantised to the steps (an output on an analog node that has a delay is a corner the step lands on).
    pub(crate) fn dig_update(&self, x: &[f64], time: Option<f64>) -> (bool, bool) {
        if self.cells.is_empty() {
            return (false, false);
        }
        let mut guard = self.dig.borrow_mut();
        let d = &mut *guard;
        let first = !d.started;
        if first {
            // the guesses of the user (`nodeset`) for digital nets are where their latches start
            for &(n, volts) in &self.dig_seeds {
                if let Some((ci, _)) = self.dnets.driver[n] {
                    let c = &self.cells[ci];
                    let (vss, vcc) = (v(x, c.vss), v(x, c.vcc));
                    d.lv[n] = vcc - vss > 1e-9 && (volts - vss) / (vcc - vss) > 0.5;
                }
            }
        }
        let timed = time.is_some() && !first;
        let t = time.unwrap_or(0.0);
        let t_prev = d.t_prev;
        let n_cells = self.cells.len();
        let mut w = Work { comb: Default::default(), queued: std::mem::take(&mut d.queued), mem: vec![], any_change: false, analog_changed: false };
        w.queued.resize(n_cells, false);
        let mut sup_moved = false;
        for (g, &(a, b)) in self.dstat.groups.iter().enumerate() {
            let (vss, vcc) = (v(x, a), v(x, b));
            let old = d.gsup[g];
            if (vss - old.0).abs() > 1e-6 + 1e-6 * old.0.abs() || (vcc - old.1).abs() > 1e-6 + 1e-6 * old.1.abs() {
                sup_moved = true;
            }
            d.gsup[g] = (vss, vcc);
        }
        // the first time every cell is looked at; afterwards only those that read an analog node (the others are reached through
        // the cells that drive them)
        let all: Vec<usize>;
        let candidates: &[usize] = if first {
            all = (0..n_cells).collect();
            &all
        } else {
            &self.dstat.analog_cells
        };
        for &ci in candidates {
            let c = &self.cells[ci];
            let (vss, vcc) = d.gsup[self.dstat.cell_group[ci]];
            let span = vcc - vss;
            // when an analog input crossed the threshold since the last call (the earliest of them)
            let mut crossed: Option<f64> = None;
            for (k, &n) in c.ins.iter().enumerate() {
                if n != 0 && !self.is_digital(n) {
                    let old_f = d.afrac[c.slot0 + k];
                    let new_f = if span.abs() > 1e-9 { (v(x, n) - vss) / span } else { 0.0 };
                    d.afrac[c.slot0 + k] = new_f;
                    if timed && (old_f > c.vt) != (new_f > c.vt) && new_f != old_f {
                        let frac = ((c.vt - old_f) / (new_f - old_f)).clamp(0.0, 1.0);
                        let tc = t_prev + frac * (t - t_prev);
                        crossed = Some(crossed.map_or(tc, |e: f64| e.min(tc)));
                    }
                }
            }
            if let Some(n) = c.oe
                && n != 0
                && !self.is_digital(n)
            {
                let slot = c.slot0 + c.ins.len();
                let old_f = d.afrac[slot];
                let new_f = if span.abs() > 1e-9 { (v(x, n) - vss) / span } else { 0.0 };
                d.afrac[slot] = new_f;
                if timed && (old_f > c.vt) != (new_f > c.vt) && new_f != old_f {
                    let frac = ((c.vt - old_f) / (new_f - old_f)).clamp(0.0, 1.0);
                    let tc = t_prev + frac * (t - t_prev);
                    crossed = Some(crossed.map_or(tc, |e: f64| e.min(tc)));
                }
            }
            let mut changed = !d.done_first[ci];
            for k in 0..c.ins.len() {
                if self.level(d, ci, k) != d.seen[c.slot0 + k] {
                    changed = true;
                }
            }
            if c.oe.is_some() && self.oe_level(d, ci) != d.seen[c.slot0 + c.ins.len()] {
                changed = true;
            }
            if changed || c.kind == CellKind::Schmitt {
                if timed {
                    d.seq += 1;
                    let te = crossed.unwrap_or(t);
                    d.heap.push(std::cmp::Reverse((te.to_bits(), d.seq, 1u8, ci, 0)));
                } else if is_memory(c.kind) {
                    w.mem.push(ci);
                } else {
                    w.comb.push_back(ci);
                    w.queued[ci] = true;
                }
            }
        }
        if !timed {
            self.settle(d, &mut w, t, false);
        } else {
            let limit = t + 1e-12 * t.abs() + 1e-18;
            while let Some(&std::cmp::Reverse(top)) = d.heap.peek() {
                let te = f64::from_bits(top.0);
                if te > limit {
                    break;
                }
                // everything due at this moment: the outputs that arrive, then the cells that read their inputs
                let mut arrive: Vec<(usize, u64)> = vec![];
                let mut read: Vec<usize> = vec![];
                while let Some(&std::cmp::Reverse(item)) = d.heap.peek() {
                    if item.0 != top.0 {
                        break;
                    }
                    d.heap.pop();
                    if item.2 == 0 {
                        arrive.push((item.3, item.4));
                    } else {
                        read.push(item.3);
                    }
                }
                let mut mem_next: Vec<usize> = vec![];
                for (ci, ver) in arrive {
                    if d.ver[ci] == ver
                        && let Some(q) = d.pend[ci].take()
                    {
                        self.apply_out(d, &mut w, ci, q, Some(te), &mut mem_next);
                    }
                }
                for ci in read {
                    if is_memory(self.cells[ci].kind) {
                        w.mem.push(ci);
                    } else if !w.queued[ci] {
                        w.queued[ci] = true;
                        w.comb.push_back(ci);
                    }
                }
                w.mem.extend(mem_next);
                w.mem.sort_unstable();
                w.mem.dedup();
                self.settle(d, &mut w, te, true);
            }
        }
        d.queued = w.queued;
        d.started = true;
        d.t_prev = t;
        (w.analog_changed || (sup_moved && self.dstat.analog_out.iter().any(|b| *b)), w.any_change)
    }

    /// Write the voltage of every digital net into the solution (the circuit equations leave those nodes at 0).
    pub(crate) fn fix_nodes(&self, x: &mut [f64]) {
        if self.cells.is_empty() {
            return;
        }
        let d = self.dig.borrow();
        for (n, drv) in self.dnets.driver.iter().enumerate() {
            if n == 0 || !self.is_digital(n) || n - 1 >= x.len() {
                continue;
            }
            x[n - 1] = match drv {
                Some((ci, _)) => {
                    let (vss, vcc) = d.gsup[self.dstat.cell_group[*ci]];
                    if d.lv[n] { vcc } else { vss }
                }
                None => 0.0,
            };
        }
    }

    /// The voltage of node `n` in the solution `x`: a digital net is not in the circuit equations, its voltage is the supply level
    /// of the cell that drives it.
    pub(crate) fn node_voltage(&self, x: &[f64], n: usize) -> f64 {
        if !self.is_digital(n) {
            return v(x, n);
        }
        let d = self.dig.borrow();
        match self.dnets.driver[n] {
            Some((ci, _)) => {
                let (vss, vcc) = d.gsup[self.dstat.cell_group[ci]];
                if d.lv[n] { vcc } else { vss }
            }
            None => 0.0,
        }
    }

    /// The level (0 or 1) an output of cell `ci` is at, for the circuit equations.
pub(crate) fn cell_drive(&self, ci: usize, inverted: bool) -> Option<(f64, (f64, f64))> {
        let d = self.dig.borrow();
        if !self.drives(&d, ci, inverted) {
            return None;
        }
        let q = d.q[ci] != inverted;
        Some((if q { 1.0 } else { 0.0 }, d.gsup[self.dstat.cell_group[ci]]))
    }

    /// The findings about the digital nets that several outputs share: a fight, or no output at all. In Japanese, for the warnings
    /// of a run.
    pub(crate) fn bus_warnings(&self) -> Vec<String> {
        let d = self.dig.borrow();
        d.bus
            .iter()
            .map(|&(node, kind, count, t)| {
                let name = &self.node_names[node];
                let drivers: Vec<&str> = self.dnets.drivers[node].iter().map(|(ci, _)| self.cells[*ci].name.as_str()).collect();
                if kind == 1 {
                    format!(
                        "ネット {name}: 2 つ以上の出力が違う値を同時に出しています ({count} 回。最初は {}。つながっている出力: {})。ここでは Low にして続けます。実物では出力どうしで大きな電流が流れて、壊れることがあります。出力許可 (OE) を切り替える時間をずらして、同時に出ないようにしてください",
                        crate::format_time(t),
                        drivers.join("、")
                    )
                } else {
                    format!(
                        "ネット {name}: どの出力も出ていません ({count} 回。最初は {}。つながっている出力: {})。浮いたネットです。ここでは前の値のままにしていますが、実物では不定です。プルアップ (またはプルダウン) 抵抗を付けるか、どれかの出力を出してください",
                        crate::format_time(t),
                        drivers.join("、")
                    )
                }
            })
            .collect()
    }

    /// The names and stored states of the cells with a memory.
    pub(crate) fn cell_states(&self) -> Vec<(String, bool)> {
        let d = self.dig.borrow();
        self.cells.iter().enumerate().filter(|(_, c)| is_memory(c.kind)).map(|(i, c)| (c.name.clone(), d.st[i])).collect()
    }
}
