//! Solver accuracy settings (SPICE's .options): tolerances, minimum conductance and the
//! iteration limit. Kept per thread so every analysis reads the same values without threading
//! a parameter through each call; the GUI sets them before it runs an analysis.

use std::cell::Cell;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SimOptions {
    /// Relative tolerance of the Newton iteration.
    pub reltol: f64,
    /// Absolute voltage tolerance (V).
    pub vntol: f64,
    /// Absolute current tolerance (A).
    pub abstol: f64,
    /// Conductance added from every node to ground (S): keeps the matrix solvable.
    pub gmin: f64,
    /// Newton iterations per solve before giving up.
    pub max_iter: usize,
    /// Transient step control: allowed error of a capacitor voltage / inductor current per step,
    /// relative to its size. The time step shrinks below `tstep` where the waveform bends faster.
    pub lte_reltol: f64,
}

impl Default for SimOptions {
    fn default() -> Self {
        SimOptions { reltol: 1e-6, vntol: 1e-8, abstol: 1e-12, gmin: 1e-12, max_iter: 150, lte_reltol: 1e-4 }
    }
}

thread_local! {
    static OPTIONS: Cell<SimOptions> = Cell::new(SimOptions::default());
}

/// The settings in force on this thread.
pub fn options() -> SimOptions {
    OPTIONS.with(|o| o.get())
}

/// Replaces the settings on this thread. Values that are not positive and finite fall back to the defaults.
pub fn set_options(o: SimOptions) {
    let d = SimOptions::default();
    let ok = |v: f64, def: f64| if v.is_finite() && v > 0.0 { v } else { def };
    OPTIONS.with(|c| {
        c.set(SimOptions {
            reltol: ok(o.reltol, d.reltol),
            vntol: ok(o.vntol, d.vntol),
            abstol: ok(o.abstol, d.abstol),
            gmin: ok(o.gmin, d.gmin),
            max_iter: o.max_iter.clamp(10, 10_000),
            lte_reltol: ok(o.lte_reltol, d.lte_reltol),
        })
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_values_fall_back_and_settings_are_per_thread() {
        set_options(SimOptions { reltol: -1.0, gmin: f64::NAN, max_iter: 3, ..SimOptions::default() });
        let o = options();
        assert_eq!(o.reltol, SimOptions::default().reltol);
        assert_eq!(o.gmin, SimOptions::default().gmin);
        assert_eq!(o.max_iter, 10);
        set_options(SimOptions::default());
    }
}
