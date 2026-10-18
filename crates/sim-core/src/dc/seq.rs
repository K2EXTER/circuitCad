//! The stored states of the digital elements with a memory (`Ce::Seq`): flip-flops, latches and Schmitt triggers.
//!
//! The state is not an unknown of the circuit equations. The outputs are voltage sources at the level of the state, so Newton's
//! iteration never has to settle a feedback loop of a latch; instead the state is updated between solves, from the accepted
//! node voltages: a rising clock edge (the clock crossed its threshold since the last accepted point) samples the data inputs
//! as they were before the edge, a transparent latch follows its data, and set / reset act at once.

use super::*;

impl Compiled {
    /// Whether the circuit has any element with a memory.
    pub(crate) fn has_seq(&self) -> bool {
        self.ce.iter().any(|e| matches!(e, Ce::Seq { .. }))
    }

    /// Update every state from the node voltages `x` (the accepted solution) and the previous accepted solution `prev` (None at
    /// the operating point: no edges). Returns whether any state changed.
    pub(crate) fn seq_update(&self, prev: Option<&[f64]>, x: &[f64]) -> bool {
        let mut state = self.seq_state.borrow_mut();
        let mut changed = false;
        for e in &self.ce {
            let Ce::Seq { kind, ins, idx, lv, .. } = e else { continue };
            let now = |k: usize| ins.get(k).is_some_and(|n| v(x, *n) > lv.vt);
            let before = |k: usize| ins.get(k).is_some_and(|n| prev.map_or_else(|| v(x, *n), |p| v(p, *n)) > lv.vt);
            use crate::SeqKind::*;
            let old = state[*idx] > 0.5;
            let mut new = old;
            // positions of the set / reset inputs and of the clock in `ins`
            let (set_at, reset_at, clk_at) = match kind {
                Dff | Tff | Dlatch => (2, 3, Some(1)),
                Jkff | Srff => (3, 4, Some(2)),
                Srlatch => (3, 4, None),
                Schmitt => (usize::MAX, usize::MAX, None),
            };
            let active = |at: usize, low: bool| at != usize::MAX && ins.len() > at && (now(at) != low);
            let (set, reset) = (active(set_at, lv.set_low), active(reset_at, lv.reset_low));
            if set || reset {
                new = set && !reset || (set && reset && old);
            } else {
                // a rising clock edge: the clock was below its threshold before and is above it now
                let edge = clk_at.is_some_and(|k| prev.is_some() && !before(k) && now(k));
                match kind {
                    Dff if edge => new = before(0),
                    Tff if edge && before(0) => new = !old,
                    Jkff if edge => match (before(0), before(1)) {
                        (true, true) => new = !old,
                        (true, false) => new = true,
                        (false, true) => new = false,
                        _ => {}
                    },
                    Srff => {
                        // clock tied low (node 0): the inputs act at once, like a latch; otherwise on the rising edge
                        let level_mode = ins.get(2) == Some(&0);
                        if level_mode || edge {
                            let (s, r) = if level_mode { (now(0), now(1)) } else { (before(0), before(1)) };
                            match (s, r) {
                                (true, false) => new = true,
                                (false, true) => new = false,
                                _ => {}
                            }
                        }
                    }
                    Dlatch if now(1) => new = now(0),
                    Srlatch if now(2) || ins.get(2) == Some(&0) => match (now(0), now(1)) {
                        (true, false) => new = true,
                        (false, true) => new = false,
                        _ => {}
                    },
                    Schmitt => {
                        let vin = v(x, ins[0]);
                        if vin > lv.vt + lv.vh {
                            new = true;
                        } else if vin < lv.vt - lv.vh {
                            new = false;
                        }
                    }
                    _ => {}
                }
            }
            if new != old {
                state[*idx] = if new { 1.0 } else { 0.0 };
                changed = true;
            }
        }
        changed
    }
}
