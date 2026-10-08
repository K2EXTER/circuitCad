use crate::{extract, Kind, Schematic, SimKind};
use std::collections::HashMap;
use std::collections::BTreeSet;
use std::fmt::Write;

fn node(name: &str) -> String {
    if name == "GND" {
        return "0".into();
    }
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect()
}

/// SPICE deck for the schematic (components only; analysis cards are left to the user).
pub fn to_spice(s: &Schematic, title: &str) -> String {
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
                let _ = writeln!(out, "{r} {} {} {}", nd(r, "1"), nd(r, "2"), c.value);
            }
            Kind::Diode | Kind::Led => {
                diode_models = true;
                let m = if c.kind == Kind::Led { "DLED" } else { "DDEF" };
                let _ = writeln!(out, "{r} {} {} {m}", nd(r, "A"), nd(r, "K"));
            }
            Kind::VSource => {
                let _ = writeln!(out, "{r} {} {} DC {}", nd(r, "+"), nd(r, "-"), c.value);
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
                let nodes: Vec<String> = sim.pins.iter().map(|p| nd(r, p)).collect();
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
    out
}
