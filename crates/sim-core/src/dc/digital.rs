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
}

/// What the cells need to know about the nodes, fixed when the circuit is compiled.
#[derive(Default)]
pub(crate) struct DigNets {
    /// Per node: only cells touch it.
    pub digital: Vec<bool>,
    /// Per node: the cells that read it.
    pub fanout: Vec<Vec<usize>>,
    /// Per digital node: the cell that drives it and whether through `outb`.
    pub driver: Vec<Option<(usize, bool)>>,
}

fn is_memory(k: CellKind) -> bool {
    !matches!(k, CellKind::And | CellKind::Nand | CellKind::Or | CellKind::Nor | CellKind::Xor | CellKind::Xnor | CellKind::Buf | CellKind::Inv)
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

    /// Evaluate one cell: its next output. `before` are the levels seen at the last evaluation (for the clock edge and the
    /// data sampled by it).
    fn eval(&self, d: &mut Dig, ci: usize) -> bool {
        let c = &self.cells[ci];
        let now: Vec<bool> = (0..c.ins.len()).map(|k| self.level(d, ci, k)).collect();
        let before: Vec<bool> = (0..c.ins.len()).map(|k| d.seen[c.slot0 + k]).collect();
        let started = d.started;
        for (k, l) in now.iter().enumerate() {
            d.seen[c.slot0 + k] = *l;
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

    /// Take the accepted solution `x` in: refresh the supplies and the analog inputs, then settle the cells. Returns whether the
    /// circuit equations have to be solved again (an output on an analog node changed, or a supply moved enough to matter) and
    /// whether any cell output changed at all.
    ///
    /// The cells settle in two phases, repeated until nothing changes: the gates (no memory) one after the other, each output
    /// applied at once, so a latch built from gates falls into one of its states instead of ringing; then every flip-flop and
    /// latch whose inputs changed, together, on the levels the gates left (a flip-flop samples what its data input was before the
    /// clock edge, so a shift register shifts by one).
    pub(crate) fn dig_update(&self, x: &[f64]) -> (bool, bool) {
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
        let n_cells = self.cells.len();
        let mut comb: std::collections::VecDeque<usize> = Default::default();
        let mut queued = std::mem::take(&mut d.queued);
        queued.resize(n_cells, false);
        let mut mem: Vec<usize> = vec![];
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
            for (k, &n) in c.ins.iter().enumerate() {
                if n != 0 && !self.is_digital(n) {
                    d.afrac[c.slot0 + k] = if span.abs() > 1e-9 { (v(x, n) - vss) / span } else { 0.0 };
                }
            }
            let mut changed = !d.done_first[ci];
            for k in 0..c.ins.len() {
                if self.level(d, ci, k) != d.seen[c.slot0 + k] {
                    changed = true;
                }
            }
            if changed || c.kind == CellKind::Schmitt {
                if is_memory(c.kind) {
                    mem.push(ci);
                } else {
                    comb.push_back(ci);
                    queued[ci] = true;
                }
            }
        }
        let mut any_change = false;
        let mut analog_changed = false;
        let mut budget = 200 * n_cells + 10_000;
        // apply the output of one cell; the cells that read a digital net that changed are queued
        let mut apply = |d: &mut Dig, ci: usize, q: bool, comb: &mut std::collections::VecDeque<usize>, queued: &mut Vec<bool>, mem_next: &mut Vec<usize>| {
            let c = &self.cells[ci];
            let force = !d.done_first[ci];
            d.done_first[ci] = true;
            let changed = d.q[ci] != q;
            d.q[ci] = q;
            if changed {
                any_change = true;
            }
            for (node, val) in [(c.out, q), (c.outb, !q)] {
                if node == 0 {
                    continue;
                }
                if self.is_digital(node) {
                    let moved = d.lv[node] != val;
                    if moved || force {
                        d.lv[node] = val;
                        for &f in &self.dnets.fanout[node] {
                            if is_memory(self.cells[f].kind) {
                                mem_next.push(f);
                            } else if !queued[f] {
                                queued[f] = true;
                                comb.push_back(f);
                            }
                        }
                    }
                } else if changed || force {
                    analog_changed = true;
                }
            }
        };
        'settle: loop {
            // phase 1: the gates, one after the other
            let mut mem_next: Vec<usize> = vec![];
            while let Some(ci) = comb.pop_front() {
                queued[ci] = false;
                if budget == 0 {
                    break 'settle;
                }
                budget -= 1;
                let q = self.eval(d, ci);
                if !d.done_first[ci] || q != d.q[ci] {
                    apply(d, ci, q, &mut comb, &mut queued, &mut mem_next);
                }
            }
            mem.extend(mem_next);
            mem.sort_unstable();
            mem.dedup();
            if mem.is_empty() {
                break;
            }
            // phase 2: the flip-flops and latches whose inputs changed, together
            let batch = std::mem::take(&mut mem);
            if budget < batch.len() {
                break;
            }
            budget -= batch.len();
            let results: Vec<(usize, bool)> = batch.iter().map(|&ci| (ci, self.eval(d, ci))).collect();
            let mut mem_next: Vec<usize> = vec![];
            for (ci, q) in results {
                if !d.done_first[ci] || q != d.q[ci] {
                    apply(d, ci, q, &mut comb, &mut queued, &mut mem_next);
                }
            }
            mem = mem_next;
            mem.sort_unstable();
            mem.dedup();
        }
        // a loop that did not settle leaves gates in the queue: clear their marks for the next call
        for ci in comb {
            queued[ci] = false;
        }
        d.queued = queued;
        d.started = true;
        (analog_changed || (sup_moved && self.dstat.analog_out.iter().any(|b| *b)), any_change)
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
    pub(crate) fn cell_level(&self, ci: usize, inverted: bool) -> (f64, (f64, f64)) {
        let d = self.dig.borrow();
        let q = d.q[ci] != inverted;
        (if q { 1.0 } else { 0.0 }, d.gsup[self.dstat.cell_group[ci]])
    }

    /// The names and stored states of the cells with a memory.
    pub(crate) fn cell_states(&self) -> Vec<(String, bool)> {
        let d = self.dig.borrow();
        self.cells.iter().enumerate().filter(|(_, c)| is_memory(c.kind)).map(|(i, c)| (c.name.clone(), d.st[i])).collect()
    }
}
