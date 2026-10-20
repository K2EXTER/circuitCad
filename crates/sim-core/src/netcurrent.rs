//! Current each net carries, to size the board's copper from the simulation.
//!
//! By Kirchhoff's current law the current that flows through a node is half the sum of the absolute
//! currents of the element terminals on it (what flows in equals what flows out), so a supply net
//! with one source and several loads reports the source current, and ground reports the total return.
//!
//! `net_currents` reads the DC operating point. `net_currents_tran` reads a transient run for circuits
//! whose current only exists in time (switching, oscillators, pulsed loads): the copper heats with the
//! effective (RMS) current, not with a spike a few nanoseconds long, so the declared current is the RMS
//! of the settled part of the run, weighted by time (the solver's steps are dense around spikes).

use crate::circuit::{Circuit, Element};
use crate::dc::DcResult;
use crate::tran::TranResult;

/// The terminals of `e` and the current that flows INTO the net at each (the sign matters; with the main current of the
/// element taken from its first terminal to its second, and the base / emitter currents of a transistor, `None` when the
/// result has no figure).
fn terminals(e: &Element, main: f64, ib: Option<f64>, ie: Option<f64>, mut add: impl FnMut(usize, f64)) {
    match e {
        Element::Resistor { a, b, .. } | Element::Inductor { a, b, .. } | Element::Switch { a, b, .. } | Element::Capacitor { a, b, .. } | Element::ChargeCap { a, b, .. } => {
            add(*a, -main);
            add(*b, main);
        }
        Element::VSource { p, n, .. } | Element::BSource { p, n, .. } | Element::Vcvs { p, n, .. } | Element::Vccs { p, n, .. } => {
            add(*p, -main);
            add(*n, main);
        }
        Element::ISource { from, to, .. } => {
            add(*from, -main);
            add(*to, main);
        }
        Element::Diode { a, k, .. } => {
            add(*a, -main);
            add(*k, main);
        }
        // the emitter carries the collector current plus the base current; without the base figure it is the collector current
        Element::Bjt { c, b, e, .. } => {
            add(*c, -main);
            add(*b, -ib.unwrap_or(0.0));
            add(*e, ie.unwrap_or(main));
        }
        Element::Mosfet { d, s, .. } => {
            add(*d, -main);
            add(*s, main);
        }
        Element::Mutual { .. } | Element::Seq { .. } => {}
    }
}

/// The part an element belongs to: the elements of a library part's model are named `U1.Rin`, `U1.Ex`, ..., and
/// what a part takes from a net is the signed sum over all of them (a model's inner loops, a current that goes in
/// through one element and out through another, are not current through the net).
fn group_of(name: &str) -> &str {
    name.split('.').next().unwrap_or(name)
}

fn visible(name: &str) -> bool {
    !name.contains('.')
}

/// `(net name, amps)` for every net with a current above 1 uA, largest first. Nodes inside a library
/// part (names with a `.`) are left out.
pub fn net_currents(c: &Circuit, dc: &DcResult) -> Vec<(String, f64)> {
    // per part: what it puts into each node (signed); the current through a node is half the sum of those, in absolute value
    let mut parts: std::collections::BTreeMap<&str, Vec<f64>> = Default::default();
    for e in &c.elements {
        let Some(r) = dc.element(e.name()) else { continue };
        let extra = |k: &str| r.extra.iter().find(|(n, _)| n == k).map(|(_, v)| *v);
        let into = parts.entry(group_of(e.name())).or_insert_with(|| vec![0.0; c.names.len()]);
        terminals(e, r.current, extra("Ib"), extra("Ie"), |node, i| into[node] += i);
    }
    let mut sum = vec![0.0f64; c.names.len()];
    for into in parts.values() {
        for (node, i) in into.iter().enumerate() {
            sum[node] += i.abs();
        }
    }
    let mut out: Vec<(String, f64)> = c.names.iter().zip(sum).filter(|(n, s)| visible(n) && *s / 2.0 > 1e-6).map(|(n, s)| (n.clone(), s / 2.0)).collect();
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    out
}

/// What a transient run says about the current of one net.
#[derive(Clone, Debug, PartialEq)]
pub struct NetStat {
    pub net: String,
    /// Time-weighted RMS over the analysed part of the run (A): what the copper heats with. This is the figure to declare.
    pub rms: f64,
    /// Largest RMS over any window of `HEAT_WINDOW` seconds (A): the heating of a burst that is short but repeats.
    pub burst_rms: f64,
    /// Largest instantaneous current (A): shown, not used for the width.
    pub peak: f64,
}

impl NetStat {
    /// The peak is far above the RMS: a start-up surge or a spike, which a real source or its wiring would limit.
    pub fn spiky(&self) -> bool {
        self.rms > 0.0 && self.peak > 10.0 * self.rms
    }
}

/// Copper warms over seconds; 10 ms is short enough to catch a burst and long enough to ignore a spike.
pub const HEAT_WINDOW: f64 = 10e-3;

/// Per-net current statistics of a transient run, over the part of the run after `from_frac` of its length
/// (0.5 = the second half, which leaves out the start-up). Nets with an RMS below 1 uA are left out; largest first.
pub fn net_currents_tran(c: &Circuit, tr: &TranResult, from_frac: f64) -> Vec<NetStat> {
    let n = tr.time.len();
    if n < 2 {
        return vec![];
    }
    let t_end = tr.time[n - 1];
    let t0 = tr.time[0] + from_frac.clamp(0.0, 0.95) * (t_end - tr.time[0]);
    let first = tr.time.partition_point(|&t| t < t0).min(n - 2);
    // current of every node at every sample: half the sum of the absolute terminal currents
    let mut node_i = vec![vec![0.0f64; n]; c.names.len()];
    // per part: what it puts into each node over time (signed), then half the absolute value of each part's figure
    let mut parts: std::collections::BTreeMap<&str, std::collections::BTreeMap<usize, Vec<f64>>> = Default::default();
    for e in &c.elements {
        let Some(sig) = tr.signal(&format!("I({})", e.name())) else { continue };
        let into = parts.entry(group_of(e.name())).or_default();
        for k in first..n {
            terminals(e, sig.values[k], None, None, |node, i| into.entry(node).or_insert_with(|| vec![0.0; n])[k] += i);
        }
    }
    for into in parts.values() {
        for (node, series) in into {
            for k in first..n {
                node_i[*node][k] += series[k].abs() / 2.0;
            }
        }
    }
    let mut out = vec![];
    for (node, i) in node_i.iter().enumerate() {
        if !visible(&c.names[node]) {
            continue;
        }
        // cumulative integral of i^2 over time (trapezoid), so any window's RMS is a difference of two values
        let mut cum = vec![0.0f64; n];
        for k in first + 1..n {
            cum[k] = cum[k - 1] + 0.5 * (i[k] * i[k] + i[k - 1] * i[k - 1]) * (tr.time[k] - tr.time[k - 1]);
        }
        let span = tr.time[n - 1] - tr.time[first];
        if span <= 0.0 {
            continue;
        }
        let rms = (cum[n - 1] / span).sqrt();
        if rms < 1e-6 {
            continue;
        }
        let peak = i[first..].iter().fold(0.0f64, |m, v| m.max(*v));
        let w = HEAT_WINDOW.min(span);
        let mut burst = rms;
        for k in first..n {
            let end = tr.time[k] + w;
            if end > tr.time[n - 1] + 1e-15 {
                break;
            }
            // cumulative value at time `end`, interpolated between samples
            let j = tr.time.partition_point(|&t| t < end).clamp(first + 1, n - 1);
            let f = ((end - tr.time[j - 1]) / (tr.time[j] - tr.time[j - 1]).max(1e-300)).clamp(0.0, 1.0);
            let at_end = cum[j - 1] + f * (cum[j] - cum[j - 1]);
            burst = burst.max(((at_end - cum[k]) / w).max(0.0).sqrt());
        }
        out.push(NetStat { net: c.names[node].clone(), rms, burst_rms: burst, peak });
    }
    out.sort_by(|a, b| b.rms.partial_cmp(&a.rms).unwrap());
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

    use crate::tran::{transient, TranSettings};
    use crate::waveform::parse_source;

    fn stat<'a>(v: &'a [NetStat], net: &str) -> &'a NetStat {
        v.iter().find(|s| s.net == net).unwrap_or_else(|| panic!("{net} not in {v:?}"))
    }

    #[test]
    fn a_pulsed_load_has_the_textbook_rms_and_peak() {
        // 10 V across 10 ohm for half of every 2 ms: peak 1 A, RMS sqrt(0.5) A
        let mut c = Circuit::new();
        let vin = c.node("VIN");
        c.add(Element::VSource { name: "V1".into(), p: vin, n: 0, wave: parse_source("PULSE(0 10 0 1n 1n 1m 2m)").unwrap(), ac: None });
        c.add(Element::Resistor { name: "R1".into(), a: vin, b: 0, r: 10.0 });
        let tr = transient(&c, &TranSettings { tstop: 40e-3, tstep: 20e-6, uic: false }).unwrap();
        let v = net_currents_tran(&c, &tr, 0.5);
        let s = stat(&v, "VIN");
        assert!((s.rms - 0.5f64.sqrt()).abs() < 0.02, "{s:?}");
        assert!((s.peak - 1.0).abs() < 1e-3, "{s:?}");
        assert!(s.burst_rms >= s.rms - 1e-9 && !s.spiky());
    }

    #[test]
    fn a_start_up_surge_is_flagged_and_left_out_of_the_settled_part() {
        // 5 V into 10 mohm and 100 uF: a 500 A surge that lasts about a microsecond, then nothing
        let mut c = Circuit::new();
        let (vin, x) = (c.node("VIN"), c.node("X"));
        c.add(Element::vdc("V1", vin, 0, 5.0));
        c.add(Element::Resistor { name: "R1".into(), a: vin, b: x, r: 0.01 });
        c.add(Element::Capacitor { name: "C1".into(), a: x, b: 0, c: 100e-6 });
        c.add(Element::Resistor { name: "R2".into(), a: x, b: 0, r: 100.0 });
        let tr = transient(&c, &TranSettings { tstop: 5e-3, tstep: 1e-6, uic: true }).unwrap();
        let whole = net_currents_tran(&c, &tr, 0.0);
        let s = stat(&whole, "VIN");
        assert!(s.peak > 100.0 && s.spiky(), "the surge is seen and flagged: {s:?}");
        let settled = net_currents_tran(&c, &tr, 0.5);
        let s = stat(&settled, "VIN");
        assert!((s.rms - 0.05).abs() < 0.005 && !s.spiky(), "after the surge the load takes 50 mA: {s:?}");
    }

    #[test]
    fn a_models_inner_loop_is_not_current_through_the_net() {
        // R1 carries 1 A from VIN to GND. A part "U1" has two current sources that push 5 A into GND and take 5 A out
        // again (a macro-model does this inside): that is not current through GND.
        let mut c = Circuit::new();
        let (vin, inner) = (c.node("VIN"), c.node("U1.x"));
        c.add(Element::vdc("V1", vin, 0, 1.0));
        c.add(Element::Resistor { name: "R1".into(), a: vin, b: 0, r: 1.0 });
        let dc = |a: f64| crate::waveform::Waveform::Dc(a);
        c.add(Element::ISource { name: "U1.Ia".into(), from: 0, to: inner, wave: dc(5.0), ac: None });
        c.add(Element::ISource { name: "U1.Ib".into(), from: inner, to: 0, wave: dc(5.0), ac: None });
        c.add(Element::Resistor { name: "U1.Rleak".into(), a: inner, b: 0, r: 1e6 });
        let n = net_currents(&c, &dc_op(&c).unwrap());
        let get = |k: &str| n.iter().find(|(m, _)| m == k).map_or(0.0, |x| x.1);
        assert!((get("GND") - 1.0).abs() < 1e-3, "{n:?}");
    }
}
