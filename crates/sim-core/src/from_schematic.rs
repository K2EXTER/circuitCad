//! Build a simulation circuit from a schematic.

use crate::circuit::{BjtModel, Circuit, DiodeModel, Element};
use crate::spice_in::{add_model_instance, add_subckt_instance};
use crate::value::parse_part_value;
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
    build(s, None, &[], &[])
}

/// Resistance and inductance of the copper between a net's supply pad and one pin, measured on the board
/// (`pcb_core::irdrop::wiring_resistances`). The pin gets a node of its own behind this resistor and inductor.
#[derive(Clone, Debug, PartialEq)]
pub struct WiringR {
    pub refdes: String,
    pub pin: String,
    pub ohm: f64,
    /// Inductance of the same copper (H); 0 = none. Matters for AC and fast transients, not for DC.
    pub henry: f64,
    /// Capacitance (F) of the same copper to the ground plane under it; 0 = none. Joins the pin's wire node to ground,
    /// which with the inductor makes the ringing of a long trace.
    pub farad: f64,
}

/// Highest quality factor of a pad's wiring L-C resonance (see the `.RD` resistor in `build`).
const WIRING_Q_MAX: f64 = 8.0;

/// Magnetic coupling between the wiring inductors of two pins (`pcb_core::irdrop::wiring_couplings`): the coupling
/// factor of `REF.PIN.LW` and `REF.PIN.LW`. Positive when both pins are read from their supply pad outward and the
/// copper runs the same way, which makes a supply trace and its return trace lower the loop inductance.
#[derive(Clone, Debug, PartialEq)]
pub struct WiringK {
    pub a: (String, String),
    pub b: (String, String),
    pub k: f64,
}

/// The circuit of `s` with the board's wiring resistance added in series with the listed pins
/// ("post-layout" simulation). Pins that are not in the schematic are ignored.
pub fn from_schematic_with_wiring(s: &Schematic, wiring: &[WiringR]) -> Result<Circuit, Vec<ConvertError>> {
    build(s, None, wiring, &[])
}

/// Like `from_schematic_with_wiring`, and the wiring inductors that `couplings` name are coupled (`Mutual`).
/// A coupling whose inductors are not both in the circuit is ignored.
pub fn from_schematic_with_layout(s: &Schematic, wiring: &[WiringR], couplings: &[WiringK]) -> Result<Circuit, Vec<ConvertError>> {
    build(s, None, wiring, couplings)
}

/// What happens to one pin in a pin fault.
#[derive(Clone, Debug, PartialEq)]
pub enum PinHow {
    /// The pin is disconnected: it sits on a node of its own.
    Open,
    /// The pin is joined to ground.
    Ground,
    /// The pin is joined to the named net (a solder bridge to a neighbour).
    Join(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PinFaultSpec {
    pub refdes: String,
    /// Pin as the schematic names it (`Component::pins()`): "1", "A", "C", a library pin number, ...
    pub pin: String,
    pub how: PinHow,
}

/// The circuit of `s` with one pin of one part disconnected, grounded or bridged to another net.
/// The rest of the part keeps its connections, so the fault is faithful for library ICs as well.
pub fn from_schematic_with_pin_fault(s: &Schematic, f: &PinFaultSpec) -> Result<Circuit, Vec<ConvertError>> {
    build(s, Some(f), &[], &[])
}

fn build(s: &Schematic, fault: Option<&PinFaultSpec>, wiring: &[WiringR], couplings: &[WiringK]) -> Result<Circuit, Vec<ConvertError>> {
    let s = &*s.flat();
    let mut c = Circuit::new();
    let mut pin_node: HashMap<(String, String), usize> = HashMap::new();
    for net in extract(s) {
        let n = c.node(&net.name);
        for p in net.pins {
            pin_node.insert((p.refdes, p.pin), n);
        }
    }
    if let Some(f) = fault {
        let node = match &f.how {
            PinHow::Open => c.node(&format!("{}.{}.open", f.refdes, f.pin)),
            PinHow::Ground => 0,
            PinHow::Join(net) => c.node(net),
        };
        pin_node.insert((f.refdes.clone(), f.pin.clone()), node);
    }
    // net --R-- --L-- pin
    for w in wiring.iter().filter(|w| w.ohm > 0.0 || w.henry > 0.0) {
        let key = (w.refdes.clone(), w.pin.clone());
        if let Some(&net_node) = pin_node.get(&key) {
            let m = c.node(&format!("{}.{}.wire", w.refdes, w.pin));
            let mut from = net_node;
            if w.ohm > 0.0 {
                let mid = if w.henry > 0.0 { c.node(&format!("{}.{}.wirel", w.refdes, w.pin)) } else { m };
                c.add(Element::Resistor { name: format!("{}.{}.RW", w.refdes, w.pin), a: from, b: mid, r: w.ohm });
                from = mid;
            }
            if w.henry > 0.0 {
                c.add(Element::Inductor { name: format!("{}.{}.LW", w.refdes, w.pin), a: from, b: m, l: w.henry });
                // The copper's own loss (skin effect, dielectric, radiation) keeps a real trace's resonance from ringing for ever,
                // but the lumped R is only milliohms: Q in the thousands, a GHz ring that no step size can settle and the
                // transient crawls at picosecond steps. A resistor across the inductor caps Q at `WIRING_Q_MAX`; DC is unchanged.
                if w.farad > 0.0 {
                    c.add(Element::Resistor { name: format!("{}.{}.RD", w.refdes, w.pin), a: from, b: m, r: WIRING_Q_MAX * (w.henry / w.farad).sqrt() });
                }
            }
            if w.farad > 0.0 {
                c.add(Element::Capacitor { name: format!("{}.{}.CW", w.refdes, w.pin), a: m, b: 0, c: w.farad });
            }
            pin_node.insert(key, m);
        }
    }
    for (n, w) in couplings.iter().enumerate() {
        let name = |p: &(String, String)| format!("{}.{}.LW", p.0, p.1);
        let have = |n: &str| c.elements.iter().any(|e| e.name() == n);
        let (l1, l2) = (name(&w.a), name(&w.b));
        if w.k != 0.0 && have(&l1) && have(&l2) {
            c.add(Element::Mutual { name: format!("K{}.{}.{}", n, w.a.0, w.b.0), l1, l2, k: w.k.clamp(-0.99, 0.99) });
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
        match comp.kind {
            Kind::Resistor | Kind::Capacitor | Kind::Inductor => {
                let (x, extra) = match parse_part_value(&comp.value) {
                    Ok(v) => v,
                    Err(m) => {
                        errors.push(err(r, m));
                        continue;
                    }
                };
                let (a, b) = (node_of(&mut c, r, "1"), node_of(&mut c, r, "2"));
                if comp.kind == Kind::Resistor && x <= 0.0 {
                    errors.push(err(r, "抵抗値は正の値にしてください"));
                    continue;
                }
                let get = |k: &str| extra.iter().find(|(n, _)| n == k).map(|(_, v)| *v).filter(|v| *v > 0.0);
                let allowed: &[&str] = match comp.kind {
                    Kind::Capacitor => &["ESR", "ESL", "IC"],
                    Kind::Inductor => &["DCR"],
                    _ => &[],
                };
                if let Some((k, _)) = extra.iter().find(|(k, _)| !allowed.contains(&k.as_str())) {
                    errors.push(err(r, format!("{k} はこの部品では使えません (使えるもの: {})", if allowed.is_empty() { "なし".to_string() } else { allowed.join(", ") })));
                    continue;
                }
                match comp.kind {
                    Kind::Resistor => c.add(Element::Resistor { name: r.into(), a, b, r: x }),
                    Kind::Capacitor => {
                        // a --ESR-- --ESL-- C -- b
                        let mut from = a;
                        if let Some(esr) = get("ESR") {
                            let m = c.node(&format!("{r}.esr"));
                            c.add(Element::Resistor { name: format!("{r}.ESR"), a: from, b: m, r: esr });
                            from = m;
                        }
                        if let Some(esl) = get("ESL") {
                            let m = c.node(&format!("{r}.esl"));
                            c.add(Element::Inductor { name: format!("{r}.ESL"), a: from, b: m, l: esl });
                            from = m;
                        }
                        c.add(Element::Capacitor { name: r.into(), a: from, b, c: x });
                        if let Some((_, ic)) = extra.iter().find(|(k, _)| k == "IC") {
                            c.set_initial_voltage(r, *ic);
                        }
                    }
                    _ => {
                        // a -- L -- DCR -- b
                        let mut to = b;
                        if let Some(dcr) = get("DCR") {
                            let m = c.node(&format!("{r}.dcr"));
                            c.add(Element::Resistor { name: format!("{r}.DCR"), a: m, b: to, r: dcr });
                            to = m;
                        }
                        c.add(Element::Inductor { name: r.into(), a, b: to, l: x });
                    }
                }
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
                let model = if comp.kind == Kind::Led { DiodeModel { is: 1e-18, n: 2.0, rs: 5.0, ..DiodeModel::default() } } else { DiodeModel::default() };
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
