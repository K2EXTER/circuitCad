//! "Can the chip be programmed?": a part that says how it is programmed (`CustomSymbol::programming`) needs the pins of one
//! way (ISP, UART bootloader, SWD ...) on a connector, or the board cannot be written to once it is built.

use crate::{Component, Kind, Net, Pt, Schematic, Severity};
use std::collections::HashMap;

use crate::netlist::Problem;

/// A pin header, or a library part named like a connector (J1, P1, CN1).
fn is_connector(c: &Component) -> bool {
    match c.kind {
        Kind::Header(_) => true,
        Kind::Custom => c.refdes.starts_with('J') || c.refdes.starts_with('P') || c.refdes.starts_with("CN"),
        _ => false,
    }
}

/// A two-pin resistor or capacitor: a signal may pass through it on the way to the connector (a reset line with a capacitor,
/// a series resistor), so the net on its other side counts too.
fn is_series_part(c: &Component) -> bool {
    match c.kind {
        Kind::Resistor | Kind::Capacitor => true,
        Kind::Custom => c.custom.as_ref().is_some_and(|s| s.pins.len() == 2 && matches!(s.glyph.as_str(), "resistor" | "capacitor" | "electrolytic")),
        _ => false,
    }
}

pub(crate) fn programming_problems(s: &Schematic, nets: &[Net]) -> Vec<Problem> {
    let comps: HashMap<&str, &Component> = s.components.iter().map(|c| (c.refdes.as_str(), c)).collect();
    let mut net_of: HashMap<(&str, &str), usize> = HashMap::new();
    for (i, n) in nets.iter().enumerate() {
        for p in &n.pins {
            net_of.entry((p.refdes.as_str(), p.pin.as_str())).or_insert(i);
        }
    }
    // the connector is on this net, or on the far side of one series part
    let reaches_connector = |ni: usize, from: &str| -> bool {
        let on = |n: usize| nets[n].pins.iter().any(|p| p.refdes != from && comps.get(p.refdes.as_str()).is_some_and(|c| is_connector(c)));
        if on(ni) {
            return true;
        }
        nets[ni].pins.iter().any(|p| {
            let Some(c) = comps.get(p.refdes.as_str()).filter(|c| is_series_part(c) && p.refdes != from) else { return false };
            c.pins().iter().filter(|(pin, _)| *pin != p.pin).any(|(pin, _)| net_of.get(&(c.refdes.as_str(), pin.as_str())).is_some_and(|&o| o != ni && on(o)))
        })
    };

    let mut out = vec![];
    for c in &s.components {
        let Some(sym) = c.custom.as_ref().filter(|y| !y.programming.is_empty()) else { continue };
        if c.sim_only {
            continue;
        }
        // for each way: the pin names that do not reach a connector
        let missing: Vec<Vec<&str>> = sym
            .programming
            .iter()
            .map(|way| {
                way.pins
                    .iter()
                    .filter(|name| {
                        !sym.pins.iter().filter(|p| p.name.eq_ignore_ascii_case(name)).any(|p| net_of.get(&(c.refdes.as_str(), p.number.as_str())).is_some_and(|&ni| reaches_connector(ni, &c.refdes)))
                    })
                    .map(|n| n.as_str())
                    .collect()
            })
            .collect();
        if missing.iter().any(|m| m.is_empty()) {
            continue;
        }
        let best = (0..missing.len()).min_by_key(|&i| missing[i].len()).unwrap_or(0);
        let ways: Vec<String> = sym.programming.iter().zip(&missing).map(|(w, m)| format!("{} (足りない: {})", w.name, m.join("・"))).collect();
        let at: Option<Pt> = Some(c.pos);
        out.push(Problem::new(
            Severity::Warning,
            format!("{} は、プログラムを書き込むピンがコネクタに出ていません ({} が足りません)", c.refdes, missing[best].join("・")),
            at,
            &format!(
                "このチップは、基板に載せたあとで書き込む部品です。書き込みの方法のどれか 1 つ分のピンを、ヘッダ (コネクタ) までつなぎましょう。{}。抵抗やコンデンサを 1 つ挟んでつないでも構いません。ほかの方法で書き込む (書き込み済みのチップを載せる) なら、この指摘は無視して構いません",
                ways.join(" / ")
            ),
        ));
    }
    out
}
