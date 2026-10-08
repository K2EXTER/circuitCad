//! Build a simulation circuit from a schematic.

use crate::circuit::{BjtModel, Circuit, DiodeModel, Element};
use crate::spice_in::{add_model_instance, add_subckt_instance};
use crate::value::parse_value;
use crate::waveform::parse_source_full;
use sch_core::{extract, Kind, Schematic, SimKind};
use std::collections::HashMap;
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub struct ConvertError {
    pub refdes: String,
    pub message: String,
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.refdes, self.message)
    }
}

fn err(r: &str, m: impl Into<String>) -> ConvertError {
    ConvertError { refdes: r.to_string(), message: m.into() }
}

/// Parts with an explicit failure are reported as errors; parts that simply have nothing to
/// simulate (headers, parts without a model) become warnings on the circuit.
pub fn from_schematic(s: &Schematic) -> Result<Circuit, Vec<ConvertError>> {
    let mut c = Circuit::new();
    let mut pin_node: HashMap<(String, String), usize> = HashMap::new();
    for net in extract(s) {
        let n = c.node(&net.name);
        for p in net.pins {
            pin_node.insert((p.refdes, p.pin), n);
        }
    }
    // pins on no net at all get a private node so elements can still be built
    let mut lone = 0;
    let mut node_of = |c: &mut Circuit, r: &str, p: &str| -> usize {
        match pin_node.get(&(r.to_string(), p.to_string())) {
            Some(&n) => n,
            None => {
                lone += 1;
                c.node(&format!("{r}.{p}.nc{lone}"))
            }
        }
    };

    let mut errors = vec![];
    for comp in &s.components {
        let r = comp.refdes.as_str();
        let value = |errors: &mut Vec<ConvertError>| match parse_value(&comp.value) {
            Some(x) => Some(x),
            None => {
                errors.push(err(r, format!("値「{}」を数値にできません", comp.value)));
                None
            }
        };
        match comp.kind {
            Kind::Resistor | Kind::Capacitor | Kind::Inductor => {
                let Some(x) = value(&mut errors) else { continue };
                let (a, b) = (node_of(&mut c, r, "1"), node_of(&mut c, r, "2"));
                if comp.kind == Kind::Resistor && x <= 0.0 {
                    errors.push(err(r, "抵抗値は正の値にしてください"));
                    continue;
                }
                c.add(match comp.kind {
                    Kind::Resistor => Element::Resistor { name: r.into(), a, b, r: x },
                    Kind::Capacitor => Element::Capacitor { name: r.into(), a, b, c: x },
                    _ => Element::Inductor { name: r.into(), a, b, l: x },
                });
            }
            Kind::VSource => {
                let src = match parse_source_full(&comp.value) {
                    Ok(w) => w,
                    Err(m) => {
                        errors.push(err(r, format!("電源の値「{}」: {m}", comp.value)));
                        continue;
                    }
                };
                let (p, n) = (node_of(&mut c, r, "+"), node_of(&mut c, r, "-"));
                c.add(Element::VSource { name: r.into(), p, n, wave: src.wave, ac: src.ac });
            }
            Kind::Diode | Kind::Led => {
                let (a, k) = (node_of(&mut c, r, "A"), node_of(&mut c, r, "K"));
                // same parameters as the generic models written by the SPICE export
                let model = if comp.kind == Kind::Led { DiodeModel { is: 1e-18, n: 2.0, rs: 5.0 } } else { DiodeModel::default() };
                c.add(Element::Diode { name: r.into(), a, k, model });
            }
            Kind::Npn => {
                let (cc, b, e) = (node_of(&mut c, r, "C"), node_of(&mut c, r, "B"), node_of(&mut c, r, "E"));
                c.add(Element::Bjt { name: r.into(), c: cc, b, e, model: BjtModel { bf: 200.0, ..BjtModel::default() } });
            }
            Kind::Header(_) | Kind::Ground | Kind::Power => {}
            Kind::Custom => {
                let Some(sim) = comp.custom.as_ref().and_then(|x| x.sim.as_ref()) else {
                    c.warnings.push(format!("{r} ({}): シミュレーションモデルがないため無視します", comp.value));
                    continue;
                };
                let nodes: Vec<usize> = sim.pins.iter().map(|p| node_of(&mut c, r, p)).collect();
                let res = match sim.kind {
                    SimKind::Model => add_model_instance(&mut c, r, &sim.element, &sim.name, &sim.text, &nodes),
                    SimKind::Subckt => add_subckt_instance(&mut c, r, &sim.name, &sim.text, &nodes),
                };
                if let Err(m) = res {
                    errors.push(err(r, m));
                }
            }
        }
    }
    if errors.is_empty() { Ok(c) } else { Err(errors) }
}
