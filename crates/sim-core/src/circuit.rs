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
    pub pnp: bool,
}

impl Default for BjtModel {
    fn default() -> Self {
        Self { is: 1e-16, bf: 100.0, br: 1.0, nf: 1.0, nr: 1.0, pnp: false }
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
            | Element::Vccs { name, .. } => name,
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
}

impl Default for Circuit {
    fn default() -> Self {
        Self::new()
    }
}

impl Circuit {
    pub fn new() -> Self {
        Self { names: vec!["GND".into()], index: HashMap::new(), elements: vec![], warnings: vec![] }
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
