//! Fault simulation: break one part (open, short, wrong value) and compare the DC operating
//! point with the healthy circuit, so a learner sees what a failure does to the voltages.

use crate::circuit::{Circuit, Element};
use crate::dc::{dc_op, DcResult, SimError};
use crate::value::format_value;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fault {
    /// Disconnected: the part is removed.
    Open,
    /// The part's main terminals joined by a tiny resistance.
    Short,
    /// Value multiplied by this factor (R, C, L only).
    Scale(f64),
}

impl Fault {
    pub fn label(self) -> String {
        match self {
            Fault::Open => "断線 (オープン)".into(),
            Fault::Short => "ショート".into(),
            Fault::Scale(k) if k > 1.0 => format!("値が {} 倍", format_value(k)),
            Fault::Scale(k) => format!("値が 1/{}", format_value(1.0 / k)),
        }
    }
}

/// Resistance that stands in for a short.
const SHORT_OHMS: f64 = 1e-3;

fn is_top_level(name: &str) -> bool {
    !name.contains('.')
}

/// Names of the parts that can be broken (not the parts inside a library IC model).
pub fn breakable(c: &Circuit) -> Vec<String> {
    c.elements
        .iter()
        .filter(|e| is_top_level(e.name()) && !applicable(e).is_empty())
        .map(|e| e.name().to_string())
        .collect()
}

/// Faults that make sense for this kind of element.
pub fn applicable(e: &Element) -> Vec<Fault> {
    match e {
        Element::Resistor { .. } | Element::Capacitor { .. } | Element::Inductor { .. } => {
            vec![Fault::Open, Fault::Short, Fault::Scale(10.0), Fault::Scale(0.1)]
        }
        Element::Diode { .. } | Element::Bjt { .. } | Element::Mosfet { .. } => vec![Fault::Open, Fault::Short],
        _ => vec![],
    }
}

/// The main two terminals of an element (what a short joins).
fn main_pins(e: &Element) -> Option<(usize, usize)> {
    match e {
        Element::Resistor { a, b, .. } | Element::Capacitor { a, b, .. } | Element::Inductor { a, b, .. } => Some((*a, *b)),
        Element::Diode { a, k, .. } => Some((*a, *k)),
        Element::Bjt { c, e, .. } => Some((*c, *e)),
        Element::Mosfet { d, s, .. } => Some((*d, *s)),
        _ => None,
    }
}

/// `c` with `element` broken as `fault`. `None` when the fault does not apply to that element.
pub fn with_fault(c: &Circuit, element: &str, fault: Fault) -> Option<Circuit> {
    let idx = c.elements.iter().position(|e| e.name() == element)?;
    if !applicable(&c.elements[idx]).contains(&fault) {
        return None;
    }
    let mut out = c.clone();
    match fault {
        Fault::Open => {
            out.elements.remove(idx);
        }
        Fault::Short => {
            let (a, b) = main_pins(&c.elements[idx])?;
            out.elements[idx] = Element::Resistor { name: element.into(), a, b, r: SHORT_OHMS };
        }
        Fault::Scale(k) => match &mut out.elements[idx] {
            Element::Resistor { r, .. } => *r *= k,
            Element::Capacitor { c, .. } => *c *= k,
            Element::Inductor { l, .. } => *l *= k,
            _ => return None,
        },
    }
    Some(out)
}

#[derive(Clone, Debug)]
pub struct FaultReport {
    pub element: String,
    pub fault: Fault,
    pub baseline: DcResult,
    /// `Err` when the broken circuit has no DC solution.
    pub faulted: Result<DcResult, SimError>,
    /// (node, healthy V, broken V), biggest change first; only nodes that moved noticeably.
    pub node_changes: Vec<(String, f64, f64)>,
    /// (part, healthy value, broken value, quantity) for parts whose current or power changed a lot.
    pub part_changes: Vec<PartChange>,
}

#[derive(Clone, Debug)]
pub struct PartChange {
    pub name: String,
    pub current: (f64, f64),
    pub power: (f64, f64),
}

/// Breaks `element` as `fault` and compares the DC operating points.
pub fn simulate_fault(c: &Circuit, element: &str, fault: Fault) -> Result<FaultReport, SimError> {
    let broken = with_fault(c, element, fault).ok_or_else(|| SimError::Invalid(format!("{element} には「{}」の故障を設定できません", fault.label())))?;
    let baseline = dc_op(c)?;
    let faulted = dc_op(&broken);
    let mut node_changes = vec![];
    let mut part_changes = vec![];
    if let Ok(f) = &faulted {
        for (n, v0) in baseline.voltages.iter().filter(|(n, _)| is_top_level(n)) {
            let v1 = f.voltage(n).unwrap_or(0.0);
            if (v1 - v0).abs() > 0.01 + 0.02 * v0.abs() {
                node_changes.push((n.clone(), *v0, v1));
            }
        }
        node_changes.sort_by(|a, b| (b.2 - b.1).abs().total_cmp(&(a.2 - a.1).abs()));
        for e0 in baseline.elements.iter().filter(|e| is_top_level(&e.name)) {
            if e0.name == element {
                continue;
            }
            let Some(e1) = f.element(&e0.name) else { continue };
            let di = (e1.current - e0.current).abs();
            let big = di > 1e-6 && di > 0.3 * e0.current.abs().max(1e-6);
            let hot = e1.power > 0.25 && e1.power > 2.0 * e0.power.max(0.0);
            if big || hot {
                part_changes.push(PartChange { name: e0.name.clone(), current: (e0.current, e1.current), power: (e0.power, e1.power) });
            }
        }
    }
    Ok(FaultReport { element: element.into(), fault, baseline, faulted, node_changes, part_changes })
}

fn volts(v: f64) -> String {
    format!("{}V", format_value((v * 1e4).round() / 1e4))
}

fn amps(i: f64) -> String {
    if i.abs() < 1e-9 { "0A".into() } else { format!("{}A", format_value(i)) }
}

impl FaultReport {
    /// What happened, in plain Japanese, most important first.
    pub fn summary(&self) -> Vec<String> {
        let mut out = vec![];
        let head = format!("{} を「{}」にしたとき", self.element, self.fault.label());
        match &self.faulted {
            Err(e) => {
                out.push(format!("{head}、回路は DC では計算できませんでした ({e})。実機でも動作が定まらない、または大電流が流れる状態です"));
                return out;
            }
            Ok(_) => {}
        }
        if self.node_changes.is_empty() && self.part_changes.is_empty() {
            out.push(format!("{head}、DC の電圧と電流はほとんど変わりませんでした。"));
            match (self.fault, self.baseline.element(&self.element)) {
                (Fault::Open, Some(e)) if e.current.abs() < 1e-9 => out.push("この部品には DC では電流が流れていないためです (コンデンサは DC では元から断線扱い)。過渡解析・AC解析なら違いが見えます".into()),
                (Fault::Short, _) if self.element.starts_with('L') => out.push("コイルは DC では元からショート扱いです。過渡解析・AC解析なら違いが見えます".into()),
                _ => {}
            }
            return out;
        }
        out.push(format!("{head}、次のように変わりました:"));
        for (n, a, b) in self.node_changes.iter().take(6) {
            out.push(format!("ノード {n}: {} → {} ({:+.2} V)", volts(*a), volts(*b), b - a));
        }
        for p in self.part_changes.iter().take(6) {
            let mut l = format!("{}: 電流 {} → {}", p.name, amps(p.current.0), amps(p.current.1));
            if p.power.1 > 0.25 && p.power.1 > 2.0 * p.power.0.max(0.0) {
                l += &format!("、消費電力 {:.0} mW → {:.0} mW", p.power.0.max(0.0) * 1e3, p.power.1 * 1e3);
                l += if p.power.1 > 1.0 { " (1W 超: 焼損・発煙のおそれ)" } else { " (一般的な 1/4 W 抵抗の定格を超えます)" };
            } else if p.current.1.abs() < 1e-9 && p.current.0.abs() > 1e-6 {
                l += " (電流が止まります)";
            }
            out.push(l);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 5 V - R1(1k) - node MID - R2(1k) - GND, with an LED-free divider.
    fn divider() -> Circuit {
        let mut c = Circuit::new();
        let (vin, mid) = (c.node("IN"), c.node("MID"));
        c.add(Element::vdc("V1", vin, 0, 5.0));
        c.add(Element::Resistor { name: "R1".into(), a: vin, b: mid, r: 1e3 });
        c.add(Element::Resistor { name: "R2".into(), a: mid, b: 0, r: 1e3 });
        c
    }

    fn mid(r: &FaultReport) -> f64 {
        r.faulted.as_ref().unwrap().voltage("MID").unwrap()
    }

    #[test]
    fn open_and_short_move_the_divider_to_the_rails() {
        let c = divider();
        let open_r1 = simulate_fault(&c, "R1", Fault::Open).unwrap();
        assert!(mid(&open_r1).abs() < 1e-3, "{}", mid(&open_r1));
        let open_r2 = simulate_fault(&c, "R2", Fault::Open).unwrap();
        assert!((mid(&open_r2) - 5.0).abs() < 1e-3);
        let short_r1 = simulate_fault(&c, "R1", Fault::Short).unwrap();
        assert!((mid(&short_r1) - 5.0).abs() < 0.01);
        let x10 = simulate_fault(&c, "R1", Fault::Scale(10.0)).unwrap();
        assert!((mid(&x10) - 5.0 / 11.0).abs() < 1e-3);
    }

    #[test]
    fn summary_names_the_changes() {
        let r = simulate_fault(&divider(), "R2", Fault::Open).unwrap();
        let s = r.summary().join("\n");
        assert!(s.contains("R2") && s.contains("ノード MID") && s.contains("2.5V → 5V"), "{s}");
    }

    #[test]
    fn faults_that_do_not_apply_are_refused() {
        let c = divider();
        assert!(simulate_fault(&c, "V1", Fault::Open).is_err());
        assert!(with_fault(&c, "R1", Fault::Scale(3.0)).is_none());
        assert_eq!(breakable(&c), vec!["R1".to_string(), "R2".to_string()]);
    }

    #[test]
    fn a_capacitor_open_does_nothing_at_dc_and_says_why() {
        let mut c = divider();
        let mid_n = c.node("MID");
        c.add(Element::Capacitor { name: "C1".into(), a: mid_n, b: 0, c: 1e-6 });
        let r = simulate_fault(&c, "C1", Fault::Open).unwrap();
        assert!(r.node_changes.is_empty());
        assert!(r.summary().iter().any(|l| l.contains("DC では電流が流れていない")), "{:?}", r.summary());
    }

    #[test]
    fn an_overloaded_part_is_flagged_as_hot() {
        // 12 V across R1 = 100 and R2 = 10k; shorting R2's neighbour pushes 1.4 W through R1
        let mut c = Circuit::new();
        let (vin, mid_n) = (c.node("IN"), c.node("MID"));
        c.add(Element::vdc("V1", vin, 0, 12.0));
        c.add(Element::Resistor { name: "R1".into(), a: vin, b: mid_n, r: 100.0 });
        c.add(Element::Resistor { name: "R2".into(), a: mid_n, b: 0, r: 10e3 });
        c.add(Element::Resistor { name: "R3".into(), a: mid_n, b: 0, r: 100.0 });
        let r = simulate_fault(&c, "R2", Fault::Short).unwrap();
        let s = r.summary().join("\n");
        assert!(s.contains("R1") && s.contains("1W 超"), "{s}");
    }
}
