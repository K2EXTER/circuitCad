//! Current each net carries at the DC operating point, to size the board's copper from the simulation.
//!
//! By Kirchhoff's current law the current that flows through a node is half the sum of the absolute
//! currents of the element terminals on it (what flows in equals what flows out), so a supply net
//! with one source and several loads reports the source current, and ground reports the total return.

use crate::circuit::{Circuit, Element};
use crate::dc::DcResult;

/// `(net name, amps)` for every net with a current above 1 uA, largest first. Nodes inside a library
/// part (names with a `.`) are left out. Terminals of elements the DC result has no current for
/// (switches that are off, sequential logic) count as zero.
pub fn net_currents(c: &Circuit, dc: &DcResult) -> Vec<(String, f64)> {
    let mut sum = vec![0.0f64; c.names.len()];
    for e in &c.elements {
        let Some(r) = dc.element(e.name()) else { continue };
        let extra = |k: &str| r.extra.iter().find(|(n, _)| n == k).map(|(_, v)| *v).unwrap_or(0.0);
        let mut add = |node: usize, i: f64| sum[node] += i.abs();
        match e {
            Element::Resistor { a, b, .. } | Element::Inductor { a, b, .. } | Element::Switch { a, b, .. } => {
                add(*a, r.current);
                add(*b, r.current);
            }
            Element::VSource { p, n, .. } | Element::BSource { p, n, .. } | Element::Vcvs { p, n, .. } | Element::Vccs { p, n, .. } => {
                add(*p, r.current);
                add(*n, r.current);
            }
            Element::ISource { from, to, .. } => {
                add(*from, r.current);
                add(*to, r.current);
            }
            Element::Diode { a, k, .. } => {
                add(*a, r.current);
                add(*k, r.current);
            }
            Element::Bjt { c, b, e, .. } => {
                add(*c, r.current);
                add(*b, extra("Ib"));
                add(*e, extra("Ie"));
            }
            Element::Mosfet { d, s, .. } => {
                add(*d, r.current);
                add(*s, r.current);
            }
            // open at DC, or no current in the result
            Element::Capacitor { .. } | Element::ChargeCap { .. } | Element::Mutual { .. } | Element::Seq { .. } => {}
        }
    }
    let mut out: Vec<(String, f64)> = c.names.iter().zip(sum).filter(|(n, s)| !n.contains('.') && *s / 2.0 > 1e-6).map(|(n, s)| (n.clone(), s / 2.0)).collect();
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dc::dc_op;

    #[test]
    fn supply_and_ground_carry_the_source_current_and_the_midpoint_the_same() {
        let mut c = Circuit::new();
        let (vin, mid) = (c.node("VIN"), c.node("MID"));
        c.add(Element::vdc("V1", vin, 0, 5.0));
        c.add(Element::Resistor { name: "R1".into(), a: vin, b: mid, r: 1.0 });
        c.add(Element::Resistor { name: "R2".into(), a: mid, b: 0, r: 1.0 });
        let n = net_currents(&c, &dc_op(&c).unwrap());
        let get = |k: &str| n.iter().find(|(m, _)| m == k).unwrap().1;
        for k in ["VIN", "MID", "GND"] {
            assert!((get(k) - 2.5).abs() < 1e-6, "{k} {n:?}");
        }
    }

    #[test]
    fn a_node_that_splits_the_current_reports_the_total() {
        let mut c = Circuit::new();
        let top = c.node("VCC");
        c.add(Element::vdc("V1", top, 0, 10.0));
        c.add(Element::Resistor { name: "R1".into(), a: top, b: 0, r: 10.0 });
        c.add(Element::Resistor { name: "R2".into(), a: top, b: 0, r: 5.0 });
        let n = net_currents(&c, &dc_op(&c).unwrap());
        assert!((n[0].1 - 3.0).abs() < 1e-6, "{n:?}");
    }
}
