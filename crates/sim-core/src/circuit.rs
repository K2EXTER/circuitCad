//! Simulator-side circuit: nodes and elements, independent of the schematic.

use crate::expr::Expr;
use crate::waveform::Waveform;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct DiodeModel {
    pub is: f64,
    pub n: f64,
    pub rs: f64,
    /// Reverse breakdown voltage (positive; infinite = none) and the reverse current at `-bv`.
    pub bv: f64,
    pub ibv: f64,
    /// Zero-bias junction capacitance (F), built-in potential (V), grading coefficient, the forward-bias knee of the
    /// depletion formula (fraction of the potential), and the transit time (s) of the diffusion charge.
    pub cj0: f64,
    pub vj: f64,
    pub m: f64,
    pub fc: f64,
    pub tt: f64,
    /// Recombination current in the depletion layer: `isr * (exp(v / (nr vt)) - 1)`, added to the diode current.
    pub isr: f64,
    pub nr: f64,
    /// Band gap (eV) and saturation-current temperature exponent (the usual silicon values, 1.11 and 3, unless the
    /// model says otherwise: a Schottky diode has about 0.69 and 2).
    pub eg: f64,
    pub xti: f64,
    /// LTspice's piecewise-linear diode (`D(Ron= Roff= Vfwd= ...)`), used instead of the exponential junction when the card
    /// gives `Vfwd` or `Ron`.
    pub ideal: Option<IdealDiode>,
}

/// `I(v) = v/roff + soft(v - vfwd)/ron - soft(-v - vrev)/rrev`, where `soft(x) = (x + sqrt(x^2 + eps^2)) / 2` (a rounded ramp).
#[derive(Clone, Debug, PartialEq)]
pub struct IdealDiode {
    pub ron: f64,
    pub roff: f64,
    pub vfwd: f64,
    pub epsilon: f64,
    /// Reverse breakdown voltage (positive; infinite = none), its resistance and the width of its knee.
    pub vrev: f64,
    pub rrev: f64,
    pub revepsilon: f64,
}

impl DiodeModel {
    /// Does the model store any charge (so that it needs the transient / AC capacitor treatment)?
    pub fn has_charge(&self) -> bool {
        self.cj0 > 0.0 || self.tt > 0.0
    }
}

impl Default for DiodeModel {
    fn default() -> Self {
        Self { is: 1e-14, n: 1.0, rs: 0.0, bv: f64::INFINITY, ibv: 1e-3, cj0: 0.0, vj: 1.0, m: 0.5, fc: 0.5, tt: 0.0, isr: 0.0, nr: 2.0, eg: 1.11, xti: 3.0, ideal: None }
    }
}

/// Level-1 (Shichman-Hodges) MOSFET, bulk tied to the source.
#[derive(Clone, Debug, PartialEq)]
pub struct MosModel {
    /// Transconductance parameter in A/V^2 (includes W/L).
    pub kp: f64,
    /// Threshold voltage; only the magnitude is used (PMOS files carry a negative value).
    pub vto: f64,
    pub lambda: f64,
    pub pmos: bool,
    /// Constant gate-source / gate-drain capacitances (F), for AC and transient.
    pub cgs: f64,
    pub cgd: f64,
    /// Gate overlap capacitances per metre of channel width (F/m): the instance's `W` turns them into `cgs` / `cgd`.
    pub cgso: f64,
    pub cgdo: f64,
    /// A junction FET: `vto` is the (negative) pinch-off voltage in the n-channel orientation and is used with its sign.
    pub jfet: bool,
}

impl Default for MosModel {
    fn default() -> Self {
        Self { kp: 2e-5, vto: 1.0, lambda: 0.0, pmos: false, cgs: 0.0, cgd: 0.0, cgso: 0.0, cgdo: 0.0, jfet: false }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BjtModel {
    pub is: f64,
    pub bf: f64,
    pub br: f64,
    pub nf: f64,
    pub nr: f64,
    /// Base series resistance in ohms (0 = none); adds an internal base node.
    pub rb: f64,
    /// Collector and emitter series resistance in ohms (0 = none); each adds an internal node.
    pub rc: f64,
    pub re: f64,
    /// Forward and reverse Early voltages (infinite = no Early effect): the collector current grows with the
    /// collector voltage by `1 - vbc/vaf - vbe/var`.
    pub vaf: f64,
    pub var: f64,
    /// Junction capacitances at zero bias (F), built-in potentials (V), grading coefficients, and the point
    /// (fraction of the potential) where the depletion formula turns into a straight line.
    pub cje: f64,
    pub vje: f64,
    pub mje: f64,
    pub cjc: f64,
    pub vjc: f64,
    pub mjc: f64,
    pub fc: f64,
    /// Forward and reverse transit times (s): the diffusion charge is `tf * (forward current)`.
    pub tf: f64,
    pub tr: f64,
    /// Base-emitter and base-collector leakage currents (`ise * (exp(vbe / (ne vt)) - 1)`), which add to the base current.
    pub ise: f64,
    pub ne: f64,
    pub isc: f64,
    pub nc: f64,
    /// Knees of the forward and reverse current gain at high injection (A; infinite = none).
    pub ikf: f64,
    pub ikr: f64,
    /// Band gap (eV), saturation-current temperature exponent and current-gain temperature exponent.
    pub eg: f64,
    pub xti: f64,
    pub xtb: f64,
    pub pnp: bool,
}

impl BjtModel {
    /// Does the model store any charge (so that it needs the transient / AC capacitor treatment)?
    pub fn has_charge(&self) -> bool {
        self.cje > 0.0 || self.cjc > 0.0 || self.tf > 0.0 || self.tr > 0.0
    }
}

impl Default for BjtModel {
    fn default() -> Self {
        Self { is: 1e-16, bf: 100.0, br: 1.0, nf: 1.0, nr: 1.0, rb: 0.0, rc: 0.0, re: 0.0, vaf: f64::INFINITY, var: f64::INFINITY, cje: 0.0, vje: 0.75, mje: 0.33, cjc: 0.0, vjc: 0.75, mjc: 0.33, fc: 0.5, tf: 0.0, tr: 0.0, ise: 0.0, ne: 1.5, isc: 0.0, nc: 2.0, ikf: f64::INFINITY, ikr: f64::INFINITY, eg: 1.11, xti: 3.0, xtb: 1.5, pnp: false }
    }
}

/// Voltage-controlled switch. The conductance moves smoothly (log-interpolated, so that both
/// ends are reached without a kink) from `1/roff` to `1/ron` while the control voltage rises
/// from `vt - vw` to `vt + vw`. Smoothness keeps Newton's method well behaved at the threshold.
#[derive(Clone, Debug, PartialEq)]
pub struct SwitchModel {
    pub ron: f64,
    pub roff: f64,
    /// Control voltage at which the switch is half way.
    pub vt: f64,
    /// Half the width of the transition. Negative: the switch is closed *below* the threshold (SPICE `VON < VOFF`).
    pub vw: f64,
}

impl Default for SwitchModel {
    fn default() -> Self {
        Self { ron: 1.0, roff: 1e9, vt: 0.0, vw: 0.01 }
    }
}

impl SwitchModel {
    /// Conductance and its derivative with respect to the control voltage.
    pub fn eval(&self, vc: f64) -> (f64, f64) {
        self.eval_widened(vc, 1.0)
    }

    /// Like `eval` with the transition `widen` times wider (a convergence aid for the DC solver).
    pub fn eval_widened(&self, vc: f64, widen: f64) -> (f64, f64) {
        let (gon, goff) = (1.0 / self.ron.max(1e-9), 1.0 / self.roff.max(1e-9));
        let w = self.vw.abs().max(1e-6) * widen;
        let mut s = ((vc - (self.vt - w)) / (2.0 * w)).clamp(0.0, 1.0);
        let reverse = self.vw < 0.0;
        if reverse {
            s = 1.0 - s;
        }
        let smooth = s * s * (3.0 - 2.0 * s);
        let dsmooth = if s > 0.0 && s < 1.0 { 6.0 * s * (1.0 - s) / (2.0 * w) * if reverse { -1.0 } else { 1.0 } } else { 0.0 };
        let span = gon.ln() - goff.ln();
        let g = (goff.ln() + span * smooth).exp();
        (g, g * span * dsmooth)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Element {
    Resistor { name: String, a: usize, b: usize, r: f64 },
    /// Open circuit at DC.
    Capacitor { name: String, a: usize, b: usize, c: f64 },
    /// Short circuit at DC.
    Inductor { name: String, a: usize, b: usize, l: f64 },
    /// Positive terminal `p`.
    VSource { name: String, p: usize, n: usize, wave: Waveform, ac: Option<(f64, f64)> },
    /// Current `dc` flows from `from`, through the source, to `to`.
    ISource { name: String, from: usize, to: usize, wave: Waveform, ac: Option<(f64, f64)> },
    Diode { name: String, a: usize, k: usize, model: DiodeModel },
    Bjt { name: String, c: usize, b: usize, e: usize, model: BjtModel },
    /// Current flows into the drain; the gate takes none.
    Mosfet { name: String, d: usize, g: usize, s: usize, model: MosModel },
    /// v(p,n) = gain * v(cp,cn)
    Vcvs { name: String, p: usize, n: usize, cp: usize, cn: usize, gain: f64 },
    /// current gm * v(cp,cn) flows from p through the source to n
    Vccs { name: String, p: usize, n: usize, cp: usize, cn: usize, gm: f64 },
    /// Conducts between `a` and `b` when v(cp,cn) is above the model's threshold.
    Switch { name: String, a: usize, b: usize, cp: usize, cn: usize, model: SwitchModel },
    /// Behavioural source. `voltage`: v(p) - v(n) = expr; otherwise the current expr flows from `p`
    /// through the source to `n`.
    BSource { name: String, p: usize, n: usize, voltage: bool, expr: Expr },
    /// A capacitor given by its charge `Q(v)` (SPICE `C1 a b Q=expr`): the current is dQ/dt, open in DC.
    ChargeCap { name: String, a: usize, b: usize, q: Expr },
    /// Coupling of two inductors (SPICE `K`): the mutual inductance is `k * sqrt(L1 * L2)`. No effect at DC.
    Mutual { name: String, l1: String, l2: String, k: f64 },
    /// A digital element with a memory (flip-flop, latch, Schmitt trigger). Its outputs are voltage sources whose level
    /// follows a stored state that changes only between time steps, never inside a Newton iteration. See `SeqKind`.
    Seq { name: String, kind: SeqKind, ins: Vec<usize>, q: usize, qb: usize, levels: SeqLevels },
}

/// What a `Element::Seq` is, with the order of its inputs in `ins` (node 0 = tied low):
/// `Dff` d clk set reset, `Tff` t clk set reset, `Jkff` j k clk set reset, `Srff` s r clk set reset (clk tied low: level
/// sensitive), `Dlatch` d en set reset, `Srlatch` s r en set reset, `Schmitt` in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeqKind {
    Dff,
    Tff,
    Jkff,
    Srff,
    Dlatch,
    Srlatch,
    Schmitt,
}

/// Logic levels of a `Seq`: the output voltages, the input threshold and (Schmitt) the half-width of the hysteresis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeqLevels {
    pub vlow: f64,
    pub vhigh: f64,
    pub vt: f64,
    pub vh: f64,
    /// Set / reset are active when low (PSpice and 74xx flip-flops) instead of when high.
    pub set_low: bool,
    pub reset_low: bool,
}

impl Element {
    pub fn vdc(name: &str, p: usize, n: usize, v: f64) -> Element {
        Element::VSource { name: name.into(), p, n, wave: Waveform::Dc(v), ac: None }
    }

    pub fn name(&self) -> &str {
        match self {
            Element::Resistor { name, .. }
            | Element::Capacitor { name, .. }
            | Element::Inductor { name, .. }
            | Element::VSource { name, .. }
            | Element::ISource { name, .. }
            | Element::Diode { name, .. }
            | Element::Bjt { name, .. }
            | Element::Mosfet { name, .. }
            | Element::Vcvs { name, .. }
            | Element::Vccs { name, .. }
            | Element::Switch { name, .. }
            | Element::BSource { name, .. }
            | Element::ChargeCap { name, .. }
            | Element::Mutual { name, .. }
            | Element::Seq { name, .. } => name,
        }
    }
}

/// Node 0 is ground.
#[derive(Clone, Debug)]
pub struct Circuit {
    pub names: Vec<String>,
    index: HashMap<String, usize>,
    pub elements: Vec<Element>,
    pub warnings: Vec<String>,
    /// Starting voltage of capacitors (by element name): used by the transient analysis unless it
    /// starts from zero.
    pub initial: Vec<(String, f64)>,
    /// Temperature coefficients (SPICE `TC1` / `TC2`, per °C and per °C squared) of resistors, capacitors and
    /// inductors by element name: the value is scaled by `1 + tc1·ΔT + tc2·ΔT²` at a temperature ΔT away from 27 °C.
    pub tempcos: Vec<(String, f64, f64)>,
    /// Saturating inductors by element name: `i0`, the current scale of the model `L(i) = L / (1 + (i/i0)²)`.
    pub saturations: Vec<(String, f64)>,
    /// Where the DC solve should start (SPICE `.nodeset`): node name and voltage. A circuit with a latch built from gates has more than one
    /// solution; the nodes listed here are pulled towards these voltages while the solver steps gmin down, which picks the state.
    pub nodesets: Vec<(String, f64)>,
}

impl Default for Circuit {
    fn default() -> Self {
        Self::new()
    }
}

impl Circuit {
    pub fn new() -> Self {
        Self { names: vec!["GND".into()], index: HashMap::new(), elements: vec![], warnings: vec![], initial: vec![], tempcos: vec![], saturations: vec![], nodesets: vec![] }
    }

    /// Node index for a name; `0` and `GND` (any case) are ground.
    pub fn node(&mut self, name: &str) -> usize {
        if name == "0" || name.eq_ignore_ascii_case("gnd") {
            return 0;
        }
        // SPICE node names are not case sensitive (`VCC` and `vcc` are one node); the first spelling is the one shown
        let key = name.to_ascii_lowercase();
        if let Some(&i) = self.index.get(&key) {
            return i;
        }
        self.names.push(name.to_string());
        self.index.insert(key, self.names.len() - 1);
        self.names.len() - 1
    }

    /// Start capacitor `cap` at `volts` in the transient analysis (SPICE `IC=`).
    pub fn set_initial_voltage(&mut self, cap: &str, volts: f64) {
        self.initial.retain(|(n, _)| n != cap);
        self.initial.push((cap.to_string(), volts));
    }

    /// Give resistor / capacitor / inductor `name` the temperature coefficients `tc1` (1/°C) and `tc2` (1/°C²).
    pub fn set_tempco(&mut self, name: &str, tc1: f64, tc2: f64) {
        self.tempcos.retain(|(n, _, _)| n != name);
        self.tempcos.push((name.to_string(), tc1, tc2));
    }

    /// Make inductor `name` saturate: its incremental inductance is `L / (1 + (i/i0)²)` (flux `L·i0·atan(i/i0)`).
    pub fn set_saturation(&mut self, name: &str, i0: f64) {
        self.saturations.retain(|(n, _)| n != name);
        self.saturations.push((name.to_string(), i0));
    }

    /// `i0` of the saturation model of the inductor `isat` amperes at which the inductance has fallen by `drop` (0..1).
    pub fn saturation_scale(isat: f64, drop: f64) -> f64 {
        isat * ((1.0 - drop) / drop).sqrt()
    }

    pub fn add(&mut self, e: Element) {
        self.elements.push(e);
    }

    /// A MOSFET plus its constant gate capacitances (named `<name>.CGS` / `<name>.CGD`).
    pub fn add_mosfet(&mut self, name: &str, d: usize, g: usize, s: usize, model: MosModel) {
        let (cgs, cgd) = (model.cgs, model.cgd);
        self.add(Element::Mosfet { name: name.into(), d, g, s, model });
        if cgs > 0.0 {
            self.add(Element::Capacitor { name: format!("{name}.CGS"), a: g, b: s, c: cgs });
        }
        if cgd > 0.0 {
            self.add(Element::Capacitor { name: format!("{name}.CGD"), a: g, b: d, c: cgd });
        }
    }
}
