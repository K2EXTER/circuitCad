use crate::{extract, Kind, Schematic, SimKind};
use std::collections::HashMap;
use std::collections::BTreeSet;
use std::fmt::Write;

/// The node name `to_spice` writes for a net (ground is `0`, other characters become `_`).
pub fn node(name: &str) -> String {
    if name == "GND" {
        return "0".into();
    }
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect()
}

/// Which simulator the deck is for. Behavioural-source expressions differ between them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    /// Our own expression syntax, as the part files write it.
    Generic,
    /// `if`/`step`/`limit`/`temp` rewritten for ngspice (see `ngspice.rs`).
    Ngspice,
}

/// SPICE deck for the schematic (components only; analysis cards are left to the user).
pub fn to_spice(s: &Schematic, title: &str) -> String {
    to_spice_for(s, title, Dialect::Generic)
}

/// `to_spice` for a given simulator.
pub fn to_spice_for(s: &Schematic, title: &str, dialect: Dialect) -> String {
    let mut pin_node: HashMap<(String, String), String> = HashMap::new();
    for n in extract(s) {
        for p in &n.pins {
            pin_node.insert((p.refdes.clone(), p.pin.clone()), node(&n.name));
        }
    }
    let nd = |r: &str, p: &str| pin_node.get(&(r.to_string(), p.to_string())).cloned().unwrap_or_else(|| format!("NC_{r}_{p}"));

    let mut out = String::new();
    let _ = writeln!(out, "* {title}");
    let mut diode_models = false;
    let mut npn_models = false;
    let mut included: BTreeSet<String> = BTreeSet::new();
    let mut model_text = String::new();
    for c in &s.components {
        let r = &c.refdes;
        match c.kind {
            Kind::Resistor | Kind::Capacitor | Kind::Inductor => {
                // "100n ESR=20m ESL=5n" / "10u DCR=0.1": expand the parasitics into series elements
                let mut tok = c.value.split_whitespace();
                let main = tok.next().unwrap_or("0");
                let get = |key: &str| c.value.split_whitespace().find_map(|t| t.to_ascii_uppercase().strip_prefix(&format!("{key}=")).map(|v| v.to_string()));
                let (n1, n2) = (nd(r, "1"), nd(r, "2"));
                match c.kind {
                    Kind::Capacitor if get("ESR").is_some() || get("ESL").is_some() => {
                        let mut from = n1;
                        if let Some(esr) = get("ESR") {
                            let _ = writeln!(out, "R{r}_ESR {from} {r}_esr {esr}");
                            from = format!("{r}_esr");
                        }
                        if let Some(esl) = get("ESL") {
                            let _ = writeln!(out, "L{r}_ESL {from} {r}_esl {esl}");
                            from = format!("{r}_esl");
                        }
                        let _ = writeln!(out, "{r} {from} {n2} {main}");
                    }
                    Kind::Inductor if get("DCR").is_some() => {
                        let _ = writeln!(out, "{r} {n1} {r}_dcr {main}");
                        let _ = writeln!(out, "R{r}_DCR {r}_dcr {n2} {}", get("DCR").unwrap());
                    }
                    _ => {
                        let _ = writeln!(out, "{r} {n1} {n2} {main}");
                    }
                }
            }
            Kind::Diode | Kind::Led => {
                diode_models = true;
                let m = if c.kind == Kind::Led { "DLED" } else { "DDEF" };
                let _ = writeln!(out, "{r} {} {} {m}", nd(r, "A"), nd(r, "K"));
            }
            Kind::VSource => {
                // "5" -> "DC 5"; "SIN(0 1 1k) AC 1", "PULSE(...)", "DC 1.7 AC 1" are already source specs
                let v = c.value.trim();
                let word = v.split(|ch: char| !ch.is_ascii_alphabetic()).next().unwrap_or("").to_ascii_uppercase();
                let spec = matches!(word.as_str(), "SIN" | "PULSE" | "PWL" | "EXP" | "SFFM" | "DC" | "AC");
                if spec {
                    let _ = writeln!(out, "{r} {} {} {v}", nd(r, "+"), nd(r, "-"));
                } else {
                    let _ = writeln!(out, "{r} {} {} DC {v}", nd(r, "+"), nd(r, "-"));
                }
            }
            Kind::Npn => {
                npn_models = true;
                let _ = writeln!(out, "{r} {} {} {} QNPN", nd(r, "C"), nd(r, "B"), nd(r, "E"));
            }
            Kind::Custom => {
                let Some(sim) = c.custom.as_ref().and_then(|s| s.sim.as_ref()) else {
                    let _ = writeln!(out, "* {r} ({}): シミュレーションモデルなし", c.value);
                    continue;
                };
                let mut nodes: Vec<String> = sim.pins.iter().map(|p| nd(r, p)).collect();
                // a MOSFET card has four nodes (D G S B); the part has three, so the body goes to the source
                if sim.kind == SimKind::Model && sim.element.eq_ignore_ascii_case("M") && nodes.len() == 3 {
                    nodes.push(nodes[2].clone());
                }
                let inst = match sim.kind {
                    SimKind::Subckt => format!("X{r}"),
                    SimKind::Model if r.starts_with(sim.element.as_str()) => r.clone(),
                    SimKind::Model => format!("{}{r}", sim.element),
                };
                let _ = writeln!(out, "{inst} {} {}", nodes.join(" "), sim.name);
                if included.insert(sim.name.to_ascii_lowercase()) {
                    model_text.push_str(sim.text.trim_end());
                    model_text.push('\n');
                }
            }
            Kind::Header(_) | Kind::Ground | Kind::Power => {}
        }
    }
    if diode_models {
        out.push_str(".model DDEF D\n.model DLED D(IS=1e-18 N=2 RS=5)\n");
    }
    if npn_models {
        out.push_str(".model QNPN NPN(BF=200)\n");
    }
    out.push_str(&model_text);
    out.push_str(".end\n");
    match dialect {
        Dialect::Generic => out,
        // the whole deck: a switch part's card sits at the top level and its model in the model text
        Dialect::Ngspice => crate::ngspice::convert_text(&out),
    }
}
