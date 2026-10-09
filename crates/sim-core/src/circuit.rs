//! Simulator-side circuit: nodes and elements, independent of the schematic.

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
}

impl Default for DiodeModel {
    fn default() -> Self {
        Self { is: 1e-14, n: 1.0, rs: 0.0, bv: f64::INFINITY, ibv: 1e-3 }
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
}

impl Default for MosModel {
    fn default() -> Self {
        Self { kp: 2e-5, vto: 1.0, lambda: 0.0, pmos: false, cgs: 0.0, cgd: 0.0 }
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
    pub pnp: bool,
}

impl Default for BjtModel {
    fn default() -> Self {
        Self { is: 1e-16, bf: 100.0, br: 1.0, nf: 1.0, nr: 1.0, rb: 0.0, pnp: false }
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
    /// Half the width of the transition.
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
        let (gon, goff) = (1.0 / self.ron.max(1e-9), 1.0 / self.roff.max(1e-9));
        let w = self.vw.abs().max(1e-6);
        let s = ((vc - (self.vt - w)) / (2.0 * w)).clamp(0.0, 1.0);
        let smooth = s * s * (3.0 - 2.0 * s);
        let dsmooth = if s > 0.0 && s < 1.0 { 6.0 * s * (1.0 - s) / (2.0 * w) } else { 0.0 };
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
            | Element::Switch { name, .. } => name,
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
}

impl Default for Circuit {
    fn default() -> Self {
        Self::new()
    }
}

impl Circuit {
    pub fn new() -> Self {
        Self { names: vec!["GND".into()], index: HashMap::new(), elements: vec![], warnings: vec![], initial: vec![] }
    }

    /// Node index for a name; `0` and `GND` (any case) are ground.
    pub fn node(&mut self, name: &str) -> usize {
        if name == "0" || name.eq_ignore_ascii_case("gnd") {
            return 0;
        }
        if let Some(&i) = self.index.get(name) {
            return i;
        }
        self.names.push(name.to_string());
        self.index.insert(name.to_string(), self.names.len() - 1);
        self.names.len() - 1
    }

    /// Start capacitor `cap` at `volts` in the transient analysis (SPICE `IC=`).
    pub fn set_initial_voltage(&mut self, cap: &str, volts: f64) {
        self.initial.retain(|(n, _)| n != cap);
        self.initial.push((cap.to_string(), volts));
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
