//! Simulator-side circuit: nodes and elements, independent of the schematic.

use crate::waveform::Waveform;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct DiodeModel {
    pub is: f64,
    pub n: f64,
    pub rs: f64,
}

impl Default for DiodeModel {
    fn default() -> Self {
        Self { is: 1e-14, n: 1.0, rs: 0.0 }
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
}
