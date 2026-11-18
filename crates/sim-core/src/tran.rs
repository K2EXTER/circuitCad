//! Transient analysis: trapezoidal integration (backward Euler for the step after every source
//! corner, to avoid numerical ringing), Newton-Raphson per step, step halving on failure.

use crate::circuit::Circuit;
use crate::dc::{compile, floating_warnings, has_ground, solve_op, v, Attempt, Ctx, Dyn, SimError, Ce};

const MAX_POINTS: f64 = 1_000_000.0;
/// Absolute floors of the step-error check: a capacitor voltage (V) and an inductor current (A).
const LTE_ABS_V: f64 = 1e-6;
const LTE_ABS_I: f64 = 1e-9;
/// Absolute charge tolerance of a diode or transistor junction (SPICE's `CHGTOL`): a junction is checked by the charge its voltage moves, not by
/// the volts, so that a few tens of microvolts of ringing on a junction of a few femtofarads is not taken for an inaccurate step.
const LTE_CHGTOL: f64 = 1e-14;
/// The name ending of an inductor of the board's wiring (`REF.PIN.LW`, a post-layout run, `from_schematic_with_layout`). Its step error is
/// allowed relative to the largest current it has carried so far (`lte_reltol` times it, as an absolute floor), not only to its present
/// current: between two bypass capacitors such an inductor rings at a few MHz with well under 1 % of the pin's peak current, and checked
/// against its own present value, which crosses zero at every swing, that ring held the steps of the 41-part demo near 5 ns (24 000 points
/// for 10 ms, 6 000 against the peak; the clock and counter edges stay as close to a run with a ten times tighter tolerance as before).
const WIRING_INDUCTOR: &str = ".LW";
/// The name endings of a capacitor's series inductance: `Lesl` in the subcircuit of a library capacitor (`CE_100u_16V` and the
/// others), `.ESL` from the ESL field of a plain capacitor (`from_schematic`). Checked against its peak current like the wiring: a
/// bypass capacitor's ESL rings with its neighbours after every logic edge, at a few hundred MHz and microamps.
const ESL_INDUCTORS: [&str; 2] = [".Lesl", ".ESL"];

/// The largest time step used when the user leaves it blank: 1/1000 of the stop time. The error control still shrinks
/// the step where the waveform bends faster, so this is only the ceiling.
pub fn auto_tstep(tstop: f64) -> f64 {
    tstop / 1000.0
}

#[derive(Clone, Debug)]
pub struct TranSettings {
    pub tstop: f64,
    /// Largest time step (and the output spacing away from source corners).
    pub tstep: f64,
    /// Start with every capacitor/inductor at zero instead of from the DC operating point.
    pub uic: bool,
}

#[derive(Clone, Debug)]
pub struct Signal {
    /// `V(node)` or `I(element)`.
    pub name: String,
    pub unit: &'static str,
    pub values: Vec<f64>,
}

#[derive(Clone, Debug)]
pub struct TranResult {
    pub time: Vec<f64>,
    pub signals: Vec<Signal>,
    pub warnings: Vec<String>,
    /// Newton solves that were thrown away and retried with a smaller step.
    pub retries: usize,
    /// Steps thrown away because the error estimate was too large, and redone smaller.
    pub rejected: usize,
    /// The setup and hold checks of the flip-flops that have a `tsu` / `th`.
    pub timing: TimingReport,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimingKind {
    Setup,
    Hold,
    /// A clock (a latch: enable) pulse shorter than the data sheet allows (`tw=`).
    PulseWidth,
}

/// How many times of each violation are kept (`TimingViolation::times`), for the marks on the digital view.
pub const TIMING_TIMES_KEPT: usize = 500;

/// A flip-flop whose data input changed too close to the clock edge, all the times together.
#[derive(Clone, Debug, PartialEq)]
pub struct TimingViolation {
    pub cell: String,
    pub kind: TimingKind,
    pub count: usize,
    /// When it first happened (seconds), the smallest time found between the data change and the clock edge, and the time the data sheet asks for.
    pub first_time: f64,
    pub worst_margin: f64,
    pub required: f64,
    /// When it happened (seconds), the first `TIMING_TIMES_KEPT` times.
    pub times: Vec<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimingReport {
    pub violations: Vec<TimingViolation>,
    /// The smallest margin to the setup time found at a clock edge: (flip-flop, margin in seconds, time since the clock edge before; NaN for the first).
    pub tightest: Option<(String, f64, f64)>,
}

/// A time with its unit, for the messages (`12.3 ns`).
pub fn format_time(t: f64) -> String {
    let a = t.abs();
    if a == 0.0 {
        "0 s".into()
    } else if a < 1e-9 {
        format!("{:.0} ps", t * 1e12)
    } else if a < 1e-6 {
        format!("{:.1} ns", t * 1e9)
    } else if a < 1e-3 {
        format!("{:.2} µs", t * 1e6)
    } else if a < 1.0 {
        format!("{:.2} ms", t * 1e3)
    } else {
        format!("{t:.2} s")
    }
}

impl TranResult {
    pub fn signal(&self, name: &str) -> Option<&Signal> {
        self.signals.iter().find(|s| s.name == name)
    }
}

/// The operating point the run starts from. Capacitors with an initial voltage (`IC=`) are held at
/// it by temporary voltage sources while the rest of the circuit settles around them.
fn start_point(circuit: &Circuit, cm: &crate::dc::Compiled) -> Result<(Vec<f64>, Vec<f64>), SimError> {
    let order = crate::dc::TRAN_START_ORDER;
    if !holds_initial_voltages(circuit) {
        return solve_op(cm, &order).map(|(att, _)| (att.x, att.vj));
    }
    // Capacitors have no branch unknown, and the held sources go last, so every index of the
    // original circuit is unchanged and the extra unknowns sit at the end.
    let mut held = circuit.clone();
    let mut sources = vec![];
    held.elements.retain(|e| match e {
        crate::Element::Capacitor { name, a, b, .. } => match circuit.initial.iter().find(|(n, _)| n == name) {
            Some((_, v)) => {
                sources.push(crate::Element::vdc(&format!("ic.{name}"), *a, *b, *v));
                false
            }
            None => true,
        },
        _ => true,
    });
    held.elements.extend(sources);
    let cm_held = compile(&held);
    let n = cm.n_unknowns;
    match solve_op(&cm_held, &order) {
        Ok((att, _)) => Ok((att.x[..n].to_vec(), att.vj)),
        // an initial voltage across two points that voltage sources already fix (a vendor model's `C1 V+ V- 10p IC=0` with
        // the supply connected) cannot be held: it contradicts the circuit. Start from the plain operating point instead,
        // as SPICE does without UIC.
        Err(SimError::Singular) => solve_op(cm, &order).map(|(att, _)| (att.x, att.vj)),
        Err(e) => Err(e),
    }
}

/// Whether a capacitor of the circuit has an initial voltage (`IC=`): the run then starts from it.
fn holds_initial_voltages(circuit: &Circuit) -> bool {
    circuit.initial.iter().any(|(n, _)| circuit.elements.iter().any(|e| matches!(e, crate::Element::Capacitor { name, .. } if name == n)))
}

/// Whether a voltage-controlled switch of the solution `x` sits inside the middle of its threshold band (within 90 % of
/// `vw` from `vt`): the balance point of a relaxation oscillator made of switches, which a run started there never leaves.
fn switch_balanced(circuit: &Circuit, x: &[f64]) -> bool {
    circuit.elements.iter().any(|e| match e {
        crate::Element::Switch { cp, cn, model, .. } => ((v(x, *cp) - v(x, *cn)) - model.vt).abs() < 0.9 * model.vw,
        _ => false,
    })
}

/// The capacitor voltages and inductor currents of a solution (the integrator's state variables).
fn reactive_state(cm: &crate::dc::Compiled, x: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let (mut cv, mut il) = (vec![0.0; cm.n_caps], vec![0.0; cm.n_inds]);
    for e in &cm.ce {
        match e {
            Ce::C { a, b, idx, .. } => cv[*idx] = v(x, *a) - v(x, *b),
            Ce::L { br, idx, .. } => il[*idx] = x[*br],
            Ce::Q { bi, ci, ei, m, cap: Some(idx), .. } => {
                let pol = if m.pnp { -1.0 } else { 1.0 };
                cv[*idx] = pol * (v(x, *bi) - v(x, *ei));
                cv[*idx + 1] = pol * (v(x, *bi) - v(x, *ci));
            }
            Ce::D { ai, k, cap: Some(idx), .. } => cv[*idx] = v(x, *ai) - v(x, *k),
            Ce::Cq { a, b, c, idx, .. } => {
                cv[*idx] = v(x, *a) - v(x, *b);
                cv[*idx + 1] = c.eval(x, 0.0).0;
            }
            _ => {}
        }
    }
    (cv, il)
}

/// The absolute voltage tolerance of every capacitor state for the step-error check: `LTE_ABS_V`, except for the junction of a diode or a
/// transistor, which has a charge tolerance (`LTE_CHGTOL`) turned into volts with the junction's capacitance at the new voltage.
fn lte_abs_voltages(cm: &crate::dc::Compiled, x: &[f64]) -> Vec<f64> {
    let mut abs = vec![LTE_ABS_V; cm.n_caps];
    for e in &cm.ce {
        match e {
            Ce::D { ai, k, nvt, dm, cap: Some(idx), .. } => {
                let c = crate::dc::diode_q(dm, *nvt, v(x, *ai) - v(x, *k)).1;
                abs[*idx] = LTE_ABS_V.max(LTE_CHGTOL / c.max(1e-30));
            }
            Ce::Q { bi, ci, ei, m, cap: Some(idx), .. } => {
                let pol = if m.pnp { -1.0 } else { 1.0 };
                let cbe = crate::dc::bjt_qbe(m, pol * (v(x, *bi) - v(x, *ei)), pol * (v(x, *bi) - v(x, *ci))).1;
                let cbc = crate::dc::bjt_qbc(m, pol * (v(x, *bi) - v(x, *ci))).1;
                abs[*idx] = LTE_ABS_V.max(LTE_CHGTOL / cbe.max(1e-30));
                abs[*idx + 1] = LTE_ABS_V.max(LTE_CHGTOL / cbc.max(1e-30));
            }
            _ => {}
        }
    }
    abs
}

/// Divided difference of order `pts.len() - 1` through `(t, y)` points (at most four: the step-error check needs three or four).
fn divided_difference(pts: &[(f64, f64)]) -> f64 {
    let mut d = [0.0; 4];
    for (di, p) in d.iter_mut().zip(pts) {
        *di = p.1;
    }
    for k in 1..pts.len() {
        for i in 0..pts.len() - k {
            d[i] = (d[i + 1] - d[i]) / (pts[i + k].0 - pts[i].0);
        }
    }
    d[0]
}

/// One accepted step as the test diagnostics keep it: (step, error ratio, what limited it, element, rejected before).
#[cfg(test)]
type StepStat = (f64, f64, u8, usize, bool);

#[cfg(test)]
thread_local! {
    /// Test-only diagnostics: which capacitor / inductor limited the last step, and why each step was as long as it was.
    pub(crate) static WORST: std::cell::Cell<(usize, bool)> = const { std::cell::Cell::new((0, false)) };
    pub(crate) static STATS: std::cell::RefCell<Vec<StepStat>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// One earlier accepted point: time, capacitor voltages, inductor currents.
type Sample = (f64, Vec<f64>, Vec<f64>);

/// Local truncation error of the step to `t_new` relative to what is allowed (> 1: too big).
/// Trapezoidal: h^3/12 times the third derivative (= 6 DD3); backward Euler: h^2/2 times the second (= 2 DD2).
/// `None` while there are not enough smooth earlier points.
/// Tries of one time step (after the first) whose plain Newton iteration is cut at 40 iterations; the later tries, of which
/// there are few left, run to the full limit. Cutting every try made a bistable logic circuit fail to converge when a last-bit
/// difference of the platform's maths put one step just past 40 iterations.
const CAPPED_TRIES: usize = 6;

/// A step that fails while it is already this many times the smallest step (`tstep * 1e-4`) is at an instant switching event (a
/// multivibrator or latch snapping over: a loop of gain above one with only capacitors to slow it, so no state exists close to the old
/// one). Shrinking further only finds the edge to the nanosecond at the price of four failed solves per level (the failures near
/// such an edge wander for the whole 150 iterations); the largest step is tried at once instead, as after the fourth failure. The
/// edge is then placed by the shortest step that lands (`JUMP_SEARCH`), and the led blinker takes 0.6 s, not 3.2 s.
const SNAP_EARLY: f64 = 64.0;

/// A regenerative flip that is not instant (a multivibrator's transistors, with their junction capacitances, take a few
/// microseconds) has no solution at the largest step from its middle, but steps below `h_min` follow it in 3 or 4 iterations.
/// Once a jump has failed there and a shorter step went through, the circuit is in such a transition: its steps are taken with
/// backward Euler, the next size is twice the last one (not raised to `h_min`), its tries are cut short (`CRAWL_CAP`), and
/// a failure tries the largest step only at the second try, not at once (`SNAP_EARLY`). In `led_blinker` a flip took up to 64
/// failed Newton runs of 300 iterations, from jumping (and failing) again after each short step. The state ends when a step
/// of this many `h_min` goes through at once, or when a jump lands.
const CRAWL: f64 = 64.0;

/// The tries of a step during a transition (`CRAWL`), up to the fourth and except the jump, give up after this many plain
/// Newton iterations: its steps converge in 3 to 8, and one that does not is shrunk. With the full `newton` the retry after a
/// shrink ran the plain pass to 150 and went through by its line search: two thirds of the iterations of a blinker whose
/// transistors have no capacitances (`tests_blinker`), whose small steps creep up to the instant flip.
const CRAWL_CAP: usize = 20;

/// After the largest step landed beyond a flip, the shorter steps that land are searched down to this factor of the size that
/// failed (see the step loop of `transient_with`).
const JUMP_SEARCH: f64 = 16.0;

/// (the new capacitor states and inductor currents, then their absolute tolerances)
fn step_error_ratio(hist: &[Sample], t_new: f64, (cv, il): (&[f64], &[f64]), (abs_v, abs_i): (&[f64], &[f64]), trap: bool, rel: f64) -> Option<f64> {
    let need = if trap { 3 } else { 2 };
    if hist.len() < need {
        return None;
    }
    let past = &hist[hist.len() - need..];
    let h = t_new - past[need - 1].0;
    let mut worst: f64 = 0.0;
    let mut worst_at: (usize, bool) = (0, false);
    let mut check = |get: &dyn Fn(&Sample) -> f64, new: f64, abs: f64, cur_at: (usize, bool)| {
        let mut buf = [(0.0, 0.0); 4];
        for (b, p) in buf.iter_mut().zip(past) {
            *b = (p.0, get(p));
        }
        buf[need] = (t_new, new);
        let pts = &buf[..=need];
        let dd = divided_difference(pts).abs();
        let err = if trap { h.powi(3) * dd / 2.0 } else { h * h * dd };
        let old = pts[need - 1].1;
        let r = err / (rel * new.abs().max(old.abs()) + abs);
        if r > worst {
            worst = r;
            worst_at = cur_at;
        }
    };
    for (k, &new) in cv.iter().enumerate() {
        check(&|p| p.1[k], new, abs_v[k], (k, false));
    }
    for (k, &new) in il.iter().enumerate() {
        check(&|p| p.2[k], new, abs_i[k], (k, true));
    }
    #[cfg(test)]
    WORST.with(|w| w.set(worst_at));
    let _ = worst_at;
    Some(worst)
}

pub fn transient(circuit: &Circuit, s: &TranSettings) -> Result<TranResult, SimError> {
    transient_with(circuit, s, None)
}

/// `transient`, recording only the signals `keep` accepts (by name, `V(node)` / `I(element)`). A circuit with thousands of
/// nodes stores a value of every node and element at every step otherwise, which is more work (and memory) than the
/// simulation itself. `None` records everything.
pub fn transient_with(circuit: &Circuit, s: &TranSettings, keep: Option<&dyn Fn(&str) -> bool>) -> Result<TranResult, SimError> {
    let mut run = TranRun::start(circuit, s, keep)?;
    run.advance_to(s.tstop)?;
    Ok(run.finish())
}

/// A transient run that keeps its state between calls: started once, then solved on to later and later times (`advance_to`),
/// with the sources changed in between if wanted (`set_source`). `transient` is one `advance_to` up to the stop time; a run that
/// is solved in pieces takes the same steps except that each piece ends on a step (like a source corner), so the answers agree
/// to the accuracy of the step control (exactly, for a circuit without capacitors and inductors, whose answer does not depend on
/// the steps). For a display that follows the clock (the live logic view of the schematic), a long session against the
/// hardware, or a waveform that is watched while values change.
pub struct TranRun {
    cm: crate::dc::Compiled,
    s: TranSettings,
    x: Vec<f64>,
    vj: Vec<f64>,
    cap_v: Vec<f64>,
    cap_i: Vec<f64>,
    ind_i: Vec<f64>,
    ind_v: Vec<f64>,
    /// The times the integrator must land on (source corners) and the next one not yet passed.
    bps: Vec<f64>,
    bp_idx: usize,
    result: TranResult,
    warnings: Vec<String>,
    /// (node, index in `result.signals`) of the node voltages kept, and the index of every element's current (None = not kept).
    node_slots: Vec<(usize, usize)>,
    element_slots: Vec<Option<usize>>,
    any_element: bool,
    eps: f64,
    t: f64,
    force_be: bool,
    lte_rel: f64,
    h_min: f64,
    h_ctrl: f64,
    hist: Vec<Sample>,
    wiring_l: Vec<usize>,
    abs_i: Vec<f64>,
    crawl: bool,
}

impl TranRun {
    /// Checks the settings, finds the starting point (the DC operating point, or zero with `uic`) and records it at time 0.
    /// `s.tstop` is the end the run may be solved to (`extend` moves it on).
    pub fn start(circuit: &Circuit, s: &TranSettings, keep: Option<&dyn Fn(&str) -> bool>) -> Result<TranRun, SimError> {
        if !(s.tstop.is_finite() && s.tstop > 0.0) {
            return Err(SimError::Invalid("終了時間は正の値にしてください".into()));
        }
        if !(s.tstep.is_finite() && s.tstep > 0.0) {
            return Err(SimError::Invalid("時間刻みは正の値にしてください".into()));
        }
        if s.tstop / s.tstep > MAX_POINTS {
            return Err(SimError::Invalid(format!("終了時間 / 時間刻み が大きすぎます (最大 {} 点)", MAX_POINTS as u64)));
        }
        if !has_ground(circuit) {
            return Err(SimError::NoGround);
        }
        let mut warnings = floating_warnings(circuit);
        for e in &circuit.elements {
            if let crate::Element::VSource { name, wave, .. } | crate::Element::ISource { name, wave, .. } = e
                && let Some(ts) = wave.noise_period()
                && s.tstep > ts / 3.0
            {
                warnings.push(format!("{name}: ノイズの最高周波数に対して時間刻みが粗すぎます (刻みは {:.3e} 秒以下を推奨)", ts / 3.0));
            }
        }
        let cm = compile(circuit);

        // initial point
        let (mut x, vj) = if s.uic {
            (vec![0.0; cm.n_unknowns], cm.zero_junction_state())
        } else {
            match start_point(circuit, &cm) {
                // a relaxation oscillator built from switches (the NE555) can have an operating point after all: its timing
                // capacitor open, a switch balanced in the middle of its threshold band. Started there the run never leaves it
                // (nothing disturbs it), so it starts from zero charge as when no point is found. Not with initial voltages: they
                // set the start (a behavioural charger's status switch balances at its precharge level, the cell held at IC=)
                Ok((x, _)) if !holds_initial_voltages(circuit) && switch_balanced(circuit, &x) => {
                    warnings.push("DC動作点で、スイッチが切り替わりの途中で釣り合っていたので (発振回路の不安定な釣り合いの点)、すべてゼロの状態から始めました (そこから始めると、発振が始まりません)".into());
                    (vec![0.0; cm.n_unknowns], cm.zero_junction_state())
                }
                Ok((x, vj)) => (x, vj),
                // an oscillator has no stable operating point (with the capacitors open it sits on
                // the unstable balance point): start from zero charge instead, like SPICE's UIC
                Err(SimError::NoConvergence) => {
                    warnings.push("DC動作点が求まらなかったため、すべてゼロの状態から始めました (発振回路では普通のことです。ゆっくり落ち着く回路では、始めの部分が電源を入れた直後の様子になります)".into());
                    (vec![0.0; cm.n_unknowns], cm.zero_junction_state())
                }
                Err(e) => return Err(e),
            }
        };

        // memory elements start from the levels of the initial point (solve_op settles them itself, a start with held capacitor
        // voltages or from zero needs this)
        for _ in 0..4 {
            if !(cm.has_seq() && cm.seq_update(None, &x)) {
                break;
            }
        }

        // digital cells start from the levels of the initial point
        if cm.has_cells() {
            cm.dig_begin_run(&x);
            cm.fix_nodes(&mut x);
        }

        let mut cap_v = vec![0.0; cm.n_caps];
        let cap_i = vec![0.0; cm.n_caps];
        let mut ind_i = vec![0.0; cm.n_inds];
        let ind_v = vec![0.0; cm.n_inds];
        for e in &cm.ce {
            match e {
                Ce::C { a, b, idx, .. } => cap_v[*idx] = v(&x, *a) - v(&x, *b),
                Ce::L { br, idx, .. } => ind_i[*idx] = x[*br],
                Ce::Q { bi, ci, ei, m, cap: Some(idx), .. } => {
                    let pol = if m.pnp { -1.0 } else { 1.0 };
                    cap_v[*idx] = pol * (v(&x, *bi) - v(&x, *ei));
                    cap_v[*idx + 1] = pol * (v(&x, *bi) - v(&x, *ci));
                }
                Ce::D { ai, k, cap: Some(idx), .. } => cap_v[*idx] = v(&x, *ai) - v(&x, *k),
                Ce::Cq { a, b, c, idx, .. } => {
                    cap_v[*idx] = v(&x, *a) - v(&x, *b);
                    cap_v[*idx + 1] = c.eval(&x, 0.0).0;
                }
                _ => {}
            }
        }

        let mut result = TranResult { time: vec![], signals: vec![], warnings: vec![], retries: 0, rejected: 0, timing: TimingReport::default() };
        let wanted = |name: &str| keep.is_none_or(|f| f(name));
        let mut node_slots: Vec<(usize, usize)> = vec![];
        for i in 1..cm.node_names.len() {
            let name = format!("V({})", cm.node_names[i]);
            if wanted(&name) {
                node_slots.push((i, result.signals.len()));
                result.signals.push(Signal { name, unit: "V", values: vec![] });
            }
        }
        let first = cm.element_results(&x, Some(0.0), &cap_i);
        let mut element_slots: Vec<Option<usize>> = vec![];
        for e in &first {
            let name = format!("I({})", e.name);
            if wanted(&name) {
                element_slots.push(Some(result.signals.len()));
                result.signals.push(Signal { name, unit: "A", values: vec![] });
            } else {
                element_slots.push(None);
            }
        }
        let any_element = element_slots.iter().any(|s| s.is_some());
        // the absolute current tolerance of every inductor; the board's wiring inductors and the capacitors' series inductances raise
        // theirs with the current they carry (`WIRING_INDUCTOR`, `ESL_INDUCTORS`)
        let wiring_l: Vec<usize> = cm.ce.iter().filter_map(|e| match e { Ce::L { name, idx, .. } if name.ends_with(WIRING_INDUCTOR) || ESL_INDUCTORS.iter().any(|e| name.ends_with(e)) => Some(*idx), _ => None }).collect();
        let abs_i = vec![LTE_ABS_I; cm.n_inds];
        let hist = vec![(0.0, cap_v.clone(), ind_i.clone())];
        let mut run = TranRun {
            s: s.clone(),
            x,
            vj,
            cap_v,
            cap_i,
            ind_i,
            ind_v,
            bps: vec![],
            bp_idx: 0,
            result,
            warnings,
            node_slots,
            element_slots,
            any_element,
            eps: 1e-12 * s.tstop,
            t: 0.0,
            // first step: no derivative history yet
            force_be: true,
            lte_rel: crate::options::options().lte_reltol,
            // `tstep` is the largest step; `h_ctrl` is the step the error control currently trusts
            h_min: s.tstep * 1e-4,
            h_ctrl: s.tstep / 10.0,
            // the last accepted points (time, capacitor voltages, inductor currents) for the error estimate;
            // a source corner breaks the smoothness, so the history restarts there
            hist,
            wiring_l,
            abs_i,
            // the circuit is in a transition the largest step cannot cross but small steps can follow (see `CRAWL`)
            crawl: false,
            cm,
        };
        run.find_corners();
        run.record();
        Ok(run)
    }

    /// The source corners from now to the end of the run, in order.
    fn find_corners(&mut self) {
        let mut bps: Vec<f64> = vec![];
        for e in &self.cm.ce {
            if let Ce::V { wave, .. } | Ce::I { wave, .. } = e {
                bps.extend(wave.breakpoints(self.s.tstop).into_iter().filter(|&b| b > self.t + self.eps));
            }
        }
        bps.sort_by(|a, b| a.total_cmp(b));
        bps.dedup_by(|a, b| (*a - *b).abs() <= 1e-15 * self.s.tstop.max(1e-12));
        self.bps = bps;
        self.bp_idx = 0;
    }

    fn record(&mut self) {
        let res = &mut self.result;
        res.time.push(self.t);
        for &(i, si) in &self.node_slots {
            res.signals[si].values.push(self.cm.node_voltage(&self.x, i));
        }
        if self.any_element {
            let mut k = 0;
            let slots = &self.element_slots;
            self.cm.visit_elements(&self.x, Some(self.t), &self.cap_i, |_, current, _, _, _| {
                if let Some(Some(si)) = slots.get(k) {
                    res.signals[*si].values.push(current);
                }
                k += 1;
            });
        }
    }

    /// The time the run has been solved to.
    pub fn time(&self) -> f64 {
        self.t
    }

    /// The end the run may be solved to.
    pub fn stop_time(&self) -> f64 {
        self.s.tstop
    }

    /// Lets the run go on to `tstop` (a later end than the one it started with).
    pub fn extend(&mut self, tstop: f64) {
        if tstop > self.s.tstop {
            self.s.tstop = tstop;
            self.find_corners();
        }
    }

    /// Changes the waveform of the voltage or current source `name` from now on (a switch flipped by hand, a clock driven by the
    /// user). The change is a corner: the next step starts afresh (backward Euler, no error history), short. Returns whether there
    /// is such a source.
    pub fn set_source(&mut self, name: &str, wave: crate::waveform::Waveform) -> bool {
        let mut found = false;
        for e in &mut self.cm.ce {
            if let Ce::V { name: n, wave: w, .. } | Ce::I { name: n, wave: w, .. } = e
                && n == name
            {
                *w = wave.clone();
                found = true;
            }
        }
        if found {
            self.force_be = true;
            self.hist.clear();
            self.h_ctrl = self.h_min;
            self.find_corners();
        }
        found
    }

    /// The voltage of node `name` now (digital nets included).
    pub fn voltage(&self, name: &str) -> Option<f64> {
        let i = self.cm.node_names.iter().position(|n| n == name)?;
        Some(self.cm.node_voltage(&self.x, i))
    }

    /// Every node (but ground) with its voltage now.
    pub fn voltages(&self) -> Vec<(String, f64)> {
        (1..self.cm.node_names.len()).map(|i| (self.cm.node_names[i].clone(), self.cm.node_voltage(&self.x, i))).collect()
    }

    /// The state now as an operating point: every node's voltage (ground included) and every element's current, voltage and
    /// power at this instant. What the rating check (`check_ratings`) reads while a circuit runs (a supply turned up burns its LED).
    pub fn snapshot(&self) -> crate::dc::DcResult {
        let mut voltages = vec![(self.cm.node_names[0].clone(), 0.0)];
        voltages.extend(self.voltages());
        crate::dc::DcResult {
            voltages,
            elements: self.cm.element_results(&self.x, Some(self.t), &self.cap_i),
            iterations: 0,
            strategy: "tran",
            warnings: vec![],
            seq_states: self.cell_states(),
            start: Default::default(),
        }
    }

    /// When the next output of a digital cell is due to change (its propagation delay running), if one is on its way.
    pub fn next_digital_event(&self) -> Option<f64> {
        self.cm.dig_next_any_event()
    }

    /// The largest supply (volts between the two supply pins) of the digital cells now: the High level of the logic.
    pub fn logic_supply(&self) -> Option<f64> {
        self.cm.dig_supply_max()
    }

    /// The names and stored states (0 / 1) of the flip-flops and latches among the digital cells.
    pub fn cell_states(&self) -> Vec<(String, bool)> {
        self.cm.cell_states()
    }

    /// Takes the points recorded since the start or the last `drain` (the signals keep their names; the run goes on recording).
    pub fn drain(&mut self) -> TranResult {
        let mut out = TranResult { time: std::mem::take(&mut self.result.time), signals: vec![], warnings: vec![], retries: self.result.retries, rejected: self.result.rejected, timing: TimingReport::default() };
        for s in &mut self.result.signals {
            out.signals.push(Signal { name: s.name.clone(), unit: s.unit, values: std::mem::take(&mut s.values) });
        }
        out
    }

    /// Solves on from the present time to `t_end` (at most the stop time), recording every accepted step; the last step lands on
    /// `t_end`. Nothing happens when `t_end` is not later than now. A digital output on an analog node (one behind its output
    /// resistance) that changes exactly at `t_end` is in the circuit equations of the next step, as in a run solved at once: the
    /// voltages read now still have the old level there (the digital nets have the new one). Solve a little past an event to see it.
    pub fn advance_to(&mut self, t_end: f64) -> Result<(), SimError> {
        let t_end = t_end.min(self.s.tstop);
        let s = self.s.clone();
        let eps = self.eps;
        let (h_min, lte_rel) = (self.h_min, self.lte_rel);
        let mut t = self.t;
        let mut x = std::mem::take(&mut self.x);
        let mut vj = std::mem::take(&mut self.vj);
        let (mut force_be, mut h_ctrl, mut crawl, mut bp_idx) = (self.force_be, self.h_ctrl, self.crawl, self.bp_idx);
        let out = self.solve_on(t_end, &s, eps, (h_min, lte_rel), (&mut t, &mut x, &mut vj), (&mut force_be, &mut h_ctrl, &mut crawl, &mut bp_idx));
        self.t = t;
        self.x = x;
        self.vj = vj;
        (self.force_be, self.h_ctrl, self.crawl, self.bp_idx) = (force_be, h_ctrl, crawl, bp_idx);
        out
    }

    #[allow(clippy::type_complexity)]
    fn solve_on(
        &mut self,
        t_end: f64,
        s: &TranSettings,
        eps: f64,
        (h_min, lte_rel): (f64, f64),
        (t, x, vj): (&mut f64, &mut Vec<f64>, &mut Vec<f64>),
        (force_be, h_ctrl, crawl, bp_idx): (&mut bool, &mut f64, &mut bool, &mut usize),
    ) -> Result<(), SimError> {
        let cm = &self.cm;
        let bps = &self.bps;
        while *t < t_end - eps {
            while *bp_idx < bps.len() && bps[*bp_idx] <= *t + eps {
                *bp_idx += 1;
            }
            let mut h = h_ctrl.min(s.tstep).min(t_end - *t);
            let wanted = h;
            let mut lands_on_bp = false;
            if *bp_idx < bps.len() && *t + h >= bps[*bp_idx] - eps {
                h = bps[*bp_idx] - *t;
                lands_on_bp = true;
            }
            // the next change of an output that sits on an analog node (a delayed output of a digital cell): land a step on it
            if cm.has_cells()
                && let Some(te) = cm.dig_next_event()
                && te > *t + eps
                && te < *t + h - eps
            {
                h = te - *t;
            }
            if *t + h >= t_end - eps {
                h = t_end - *t;
            }

            let (cap_v, cap_i, ind_i, ind_v) = (&self.cap_v, &self.cap_i, &self.ind_i, &self.ind_v);
            let mut retried = false;
            let mut rejects = 0;
            // the size that failed first and whether the largest step was tried, for `crawl` after the step
            let mut step_first;
            let mut step_jumped = false;
            // one pass per tried step size: Newton (shrinking on failure), then the error check
            let (att, ratio, order) = 'step: loop {
                let mut tries = 0;
                let mut h_back = 0.0;
                let h_first = h;
                step_first = h_first;
                // the largest step was tried after a failure: if it lands, the circuit has flipped (see `SNAP_EARLY`)
                let mut jumped = false;
                let att: Attempt = loop {
                    let trap = !(*force_be || retried);
                    let ctx = Ctx {
                        gmin: crate::options::options().gmin,
                        scale: 1.0,
                        t: *t + h,
                        dynamic: Some(Dyn { h, trap, cap_v, cap_i, ind_i, ind_v, capped: tries < CAPPED_TRIES }),
                        sw_widen: 1.0,
                        seed_g: None,
                    };
                    let solved = if *crawl && tries < 4 && !jumped { cm.newton_plain_capped(x.clone(), vj.clone(), &ctx, CRAWL_CAP) } else { cm.newton(x.clone(), vj.clone(), &ctx) };
                    match solved {
                        Ok(a) if jumped && h > JUMP_SEARCH * h_first => {
                            // The largest step landed beyond the flip, which may lie anywhere within it: the step would draw the edge over
                            // its whole length and hand a later digital input the new level that much late (a 555's output and the gate
                            // after it, 15 us late at 10 us steps). Look for the shortest step that still lands, between the size that
                            // failed and this one (by halving their ratio, as fails cost the most): the edge is then within a factor
                            // `JUMP_SEARCH` of the step that found it.
                            let (mut lo, mut hi, mut a) = (h_first, h, a);
                            while hi > JUMP_SEARCH * lo {
                                let mid = (lo * hi).sqrt();
                                let ctx = Ctx {
                                    gmin: crate::options::options().gmin,
                                    scale: 1.0,
                                    t: *t + mid,
                                    dynamic: Some(Dyn { h: mid, trap: false, cap_v, cap_i, ind_i, ind_v, capped: true }),
                                    sw_widen: 1.0,
                                    seed_g: None,
                                };
                                match cm.newton(x.clone(), vj.clone(), &ctx) {
                                    Ok(b) => (hi, a) = (mid, b),
                                    Err(crate::dc::Fail::Singular) => return Err(SimError::Singular),
                                    Err(crate::dc::Fail::NoConv) => {
                                        self.result.retries += 1;
                                        lo = mid;
                                    }
                                }
                            }
                            if hi < h {
                                h = hi;
                                lands_on_bp = false;
                            }
                            break a;
                        }
                        Ok(a) => break a,
                        Err(crate::dc::Fail::Singular) => return Err(SimError::Singular),
                        Err(crate::dc::Fail::NoConv) => {
                            tries += 1;
                            self.result.retries += 1;
                            lands_on_bp = false;
                            // the trapezoidal rule keeps the old capacitor current, which for a fast
                            // switching event no step size can fix: retry with backward Euler
                            retried = true;
                            if (tries == 4 || (tries == if *crawl { 2 } else { 1 } && h_first <= SNAP_EARLY * h_min)) && h * 16.0 < s.tstep.min(t_end - *t) {
                                // An instant switching event (a latch, a logic gate) may have no solution
                                // close to the old state at all, yet one far away that a big step lands on:
                                // shrinking never helps there, so try the largest step once, then go on shrinking.
                                h_back = h / 4.0;
                                jumped = true;
                                step_jumped = true;
                                h = s.tstep.min(t_end - *t);
                                if *bp_idx < bps.len() && *t + h >= bps[*bp_idx] - eps {
                                    h = bps[*bp_idx] - *t;
                                    lands_on_bp = true;
                                }
                            } else if h_back > 0.0 {
                                // the largest step did not land either: try a few sizes between it and the step that was failing, since the
                                // state after the flip may only be reachable from a step of a certain size
                                if h > 64.0 * h_first {
                                    h /= 16.0;
                                } else {
                                    h = h_back;
                                    h_back = 0.0;
                                }
                            } else {
                                h /= 4.0;
                            }
                            if tries > 16 || h < 1e-18 {
                                return Err(SimError::NoConvergence);
                            }
                        }
                    }
                };
                let trap = !(*force_be || retried);
                let order = if trap { 3.0 } else { 2.0 };
                let (cv, il) = reactive_state(cm, &att.x);
                let abs_v = lte_abs_voltages(cm, &att.x);
                let ratio = step_error_ratio(&self.hist, *t + h, (&cv, &il), (&abs_v, &self.abs_i), trap, lte_rel);
                match ratio {
                    // too inaccurate: redo this step with a smaller one (unless it is already tiny)
                    // (not after a step that landed beyond a flip: its error estimate spans the jump, and redoing the step only repeats
                    // the search for the edge, as many times as `rejects` allows, to land on the same step again)
                    Some(r) if r > 1.0 && h > h_min && rejects < 12 && !(jumped && h > 4.0 * h_first) => {
                        rejects += 1;
                        self.result.rejected += 1;
                        lands_on_bp = false;
                        h *= (0.9 * r.powf(-1.0 / order)).clamp(0.125, 0.9);
                    }
                    _ => break 'step (att, ratio, order),
                }
            };

            // accept the step: update the integration state from the new solution
            let trap = !(*force_be || retried);
            let (cap_v, cap_i, ind_i, ind_v) = (&mut self.cap_v, &mut self.cap_i, &mut self.ind_i, &mut self.ind_v);
            for e in &cm.ce {
                match e {
                    Ce::C { a, b, c, idx, .. } => {
                        let vn = v(&att.x, *a) - v(&att.x, *b);
                        let geq = if trap { 2.0 * c / h } else { c / h };
                        cap_i[*idx] = geq * (vn - cap_v[*idx]) - if trap { cap_i[*idx] } else { 0.0 };
                        cap_v[*idx] = vn;
                    }
                    Ce::L { p, n, br, idx, .. } => {
                        ind_i[*idx] = att.x[*br];
                        ind_v[*idx] = v(&att.x, *p) - v(&att.x, *n);
                    }
                    Ce::D { ai, k, nvt, dm, cap: Some(idx), .. } => {
                        let kk = if trap { 2.0 } else { 1.0 };
                        let vnew = v(&att.x, *ai) - v(&att.x, *k);
                        let (q_new, q_old) = (crate::dc::diode_q(dm, *nvt, vnew).0, crate::dc::diode_q(dm, *nvt, cap_v[*idx]).0);
                        cap_i[*idx] = kk / h * (q_new - q_old) - if trap { cap_i[*idx] } else { 0.0 };
                        cap_v[*idx] = vnew;
                    }
                    Ce::Cq { a, b, c, idx, .. } => {
                        let kk = if trap { 2.0 } else { 1.0 };
                        let q_new = c.eval(&att.x, *t + h).0;
                        cap_i[*idx] = kk / h * (q_new - cap_v[*idx + 1]) - if trap { cap_i[*idx] } else { 0.0 };
                        cap_v[*idx] = v(&att.x, *a) - v(&att.x, *b);
                        cap_v[*idx + 1] = q_new;
                    }
                    Ce::Q { bi, ci, ei, m, cap: Some(idx), .. } => {
                        let pol = if m.pnp { -1.0 } else { 1.0 };
                        let k = if trap { 2.0 } else { 1.0 };
                        let vn = [pol * (v(&att.x, *bi) - v(&att.x, *ei)), pol * (v(&att.x, *bi) - v(&att.x, *ci))];
                        for (j, vnew) in vn.into_iter().enumerate() {
                            let (q_new, q_old) = if j == 0 { (crate::dc::bjt_qbe(m, vnew, vn[1]).0, crate::dc::bjt_qbe(m, cap_v[*idx], cap_v[*idx + 1]).0) } else { (crate::dc::bjt_qbc(m, vnew).0, crate::dc::bjt_qbc(m, cap_v[*idx + 1]).0) };
                            cap_i[*idx + j] = k / h * (q_new - q_old) - if trap { cap_i[*idx + j] } else { 0.0 };
                            cap_v[*idx + j] = vnew;
                        }
                    }
                    _ => {}
                }
            }
            #[cfg(test)]
            STATS.with(|st| st.borrow_mut().push((*t, h, if lands_on_bp { 1 } else if rejects > 0 { 2 } else if retried { 3 } else if h >= s.tstep * 0.999 { 4 } else { 0 }, WORST.with(|w| w.get()).0, WORST.with(|w| w.get()).1)));
            *t = if lands_on_bp { bps[*bp_idx] } else { *t + h };
            // a flip-flop or latch that changed: its outputs jump at the next step, so restart like at a source corner
            let seq_changed = cm.has_seq() && cm.seq_update(Some(x.as_slice()), &att.x);
            // digital cells: an output on an analog node that changed jumps at the next step, like a flip-flop's
            let seq_changed = seq_changed | (cm.has_cells() && cm.dig_update(&att.x, Some(*t)).0);
            *x = att.x;
            // (the voltages of the digital nets are read from the cells when recorded, `Compiled::node_voltage`)
            *vj = att.vj;
            // the step after a retry has no reliable current history either
            *force_be = lands_on_bp || retried || seq_changed;
            {
                // record (like `record`, which cannot be called while `cm` and `bps` are borrowed)
                let res = &mut self.result;
                res.time.push(*t);
                for &(i, si) in &self.node_slots {
                    res.signals[si].values.push(cm.node_voltage(x, i));
                }
                if self.any_element {
                    let mut k = 0;
                    let slots = &self.element_slots;
                    cm.visit_elements(x, Some(*t), cap_i, |_, current, _, _, _| {
                        if let Some(Some(si)) = slots.get(k) {
                            res.signals[*si].values.push(current);
                        }
                        k += 1;
                    });
                }
            }

            // a jump that did not land, after which a shorter step went through: the circuit is in the middle of its transition
            // (see `CRAWL`). It ends with a jump that lands or with a step back at the sizes of the quiet circuit.
            let was_crawling = *crawl;
            if step_jumped {
                *crawl = h < step_first;
            } else if h >= CRAWL * h_min {
                *crawl = false;
            }
            // the steps of a transition are taken with backward Euler: the trapezoidal rule leaves an oscillation from point to
            // point behind it that holds the error control at a few microseconds for tens of milliseconds
            *force_be |= *crawl || was_crawling;
            // the next step: grow while the error estimate leaves room, never beyond `tstep`
            if lands_on_bp || seq_changed {
                self.hist.clear();
                // restart from about the length of the step that reached the corner
                *h_ctrl = h.max(s.tstep * 1e-3).min(s.tstep);
            } else if retried {
                *h_ctrl = if *crawl { (2.0 * h).min(s.tstep) } else { h.max(h_min) };
            } else if h >= wanted * 0.999 || rejects > 0 || ratio.is_some_and(|r| r > 1.0) {
                let mut growth = ratio.map_or(2.0, |r| (0.9 * r.max(1e-12).powf(-1.0 / order)).clamp(0.5, 2.0));
                // a step that had to be redone has found the size the circuit allows here: trying twice that again
                // only earns another rejection, but holding the old size for good starves the multivibrator flips
                if rejects > 0 {
                    growth = growth.min(1.05);
                }
                *h_ctrl = (h * growth).clamp(if *crawl { 0.0 } else { h_min }, s.tstep);
            }
            // else: the step was cut short by the end of the run (not by the error control); keep the controller's value
            for &k in &self.wiring_l {
                self.abs_i[k] = self.abs_i[k].max(lte_rel * ind_i[k].abs());
            }
            self.hist.push((*t, cap_v.clone(), ind_i.clone()));
            if self.hist.len() > 3 {
                self.hist.remove(0);
            }
        }
        Ok(())
    }

    /// Ends the run: the warnings (of the start, the buses, the timing checks) and everything recorded since the last `drain`.
    pub fn finish(self) -> TranResult {
        let TranRun { cm, mut warnings, mut result, .. } = self;
        if cm.has_cells() {
            warnings.extend(cm.bus_warnings());
            result.timing = cm.timing_report();
            for v in &result.timing.violations {
                let what = match v.kind {
                    TimingKind::Setup => format!("データがクロックの {} 前に変わっています (必要なのは {} 以上)。クロックを遅くするか、データまでの遅れ (ゲートの段数) を短くしてください", format_time(v.worst_margin), format_time(v.required)),
                    TimingKind::Hold => format!("データがクロックの {} 後に変わっています (クロックのあと {} は変わってはいけません)。データの経路に遅れを足すか、クロックのずれを減らしてください", format_time(v.worst_margin), format_time(v.required)),
                    TimingKind::PulseWidth => format!("クロックのパルスの幅が {} しかありません ({} 以上が必要)。クロックを遅くするか、デューティ比を 50 % に近づけてください", format_time(v.worst_margin), format_time(v.required)),
                };
                let name = match v.kind {
                    TimingKind::Setup => "セットアップ時間",
                    TimingKind::Hold => "ホールド時間",
                    TimingKind::PulseWidth => "クロックのパルス幅",
                };
                let tail = match v.kind {
                    TimingKind::Setup | TimingKind::Hold => "この回の出力は実物では不定 (0 にも 1 にもなりうる) で、ここでは 0 か 1 を乱数で選んでいます (毎回同じ選び方)",
                    TimingKind::PulseWidth => "実物では取り込みに失敗することがあります (ここでは普通に取り込みます)",
                };
                warnings.push(format!("{}: {name}が足りません ({} 回。最初は {})。{what}。{tail}", v.cell, v.count, format_time(v.first_time)));
            }
        }
        warnings.dedup();
        result.warnings = warnings;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Circuit, DiodeModel, Element};

    /// The step-error tolerance of a junction is a charge (`LTE_CHGTOL`) turned into volts with its capacitance: a junction of a few
    /// femtofarads may ring by tens of microvolts (TI's latch clamps did, and the step stayed at its minimum for the whole run), a
    /// 10 pF one may not, and a plain capacitor keeps the tolerance in volts.
    #[test]
    fn a_small_junction_has_a_wide_voltage_tolerance_and_a_capacitor_does_not() {
        let mut c = Circuit::new();
        let (a, b, d) = (c.node("a"), c.node("b"), c.node("d"));
        c.add(Element::Resistor { name: "R1".into(), a, b: 0, r: 1e3 });
        c.add(Element::Resistor { name: "R2".into(), a: b, b: 0, r: 1e3 });
        c.add(Element::Capacitor { name: "C1".into(), a: d, b: 0, c: 1e-9 });
        c.add(Element::Resistor { name: "R3".into(), a: d, b: 0, r: 1e3 });
        c.add(Element::Diode { name: "Dsmall".into(), a, k: 0, model: DiodeModel { cj0: 1e-15, ..DiodeModel::default() } });
        c.add(Element::Diode { name: "Dbig".into(), a: b, k: 0, model: DiodeModel { cj0: 10e-12, ..DiodeModel::default() } });
        let cm = compile(&c);
        let x = vec![0.0; cm.n_unknowns];
        let abs = lte_abs_voltages(&cm, &x);
        assert_eq!(abs.len(), cm.n_caps);
        let mut seen = std::collections::HashMap::new();
        for e in &cm.ce {
            match e {
                Ce::C { name, idx, .. } => seen.insert(name.clone(), abs[*idx]),
                Ce::D { name, cap: Some(idx), .. } => seen.insert(name.clone(), abs[*idx]),
                _ => None,
            };
        }
        // 1e-14 C / 1 fF = 10 V (it is not checked at all); 1e-14 C / 10 pF = 1 mV; the capacitor stays at 1 uV
        assert!(seen["Dsmall"] > 5.0, "{seen:?}");
        assert!((seen["Dbig"] - 1e-3).abs() < 2e-4, "{seen:?}");
        assert_eq!(seen["C1"], LTE_ABS_V, "{seen:?}");
    }
}
