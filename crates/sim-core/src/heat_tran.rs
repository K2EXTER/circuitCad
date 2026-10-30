//! Temperature over time. The power of each part comes from a transient run (a resistor `I²R`, a transistor or diode `I·V`); each part
//! is one heat-capacity / heat-resistance stage to the surroundings, and two parts can be tied by a heat resistance of their own
//! (the heat that goes through the board's copper from one to the other). The thermal equations are integrated on the time points of the
//! transient run. A resistor with a temperature coefficient changes its value with the temperature, and a transistor or diode its model (the rules
//! of `temperature.rs`), which changes the current and the power, so the electrical run is redone with the temperature the part had
//! while it was dissipating (a loose coupling:
//! the heat moves in seconds, the circuit settles in milliseconds).

use crate::circuit::{Circuit, Element};
use crate::dc::SimError;
use crate::selfheat::{default_theta, scale, Heater};
use crate::tran::{transient, TranResult, TranSettings};
use sch_core::Schematic;

/// A part for the temperature run: the elements that dissipate inside it, its heat resistance to the surroundings, its heat capacity.
#[derive(Clone, Debug, PartialEq)]
pub struct ThermalNode {
    pub heater: Heater,
    /// Heat capacity (J/K). 0: no capacity, the part follows its power at once (steady state at every moment).
    pub cth: f64,
}

/// Heat that flows between two parts (by name) through the board: a heat resistance (K/W).
#[derive(Clone, Debug, PartialEq)]
pub struct ThermalLink {
    pub a: String,
    pub b: String,
    pub theta: f64,
}

#[derive(Clone, Debug)]
pub struct HeatTranPart {
    pub name: String,
    pub theta: f64,
    pub cth: f64,
    /// Peak and time-mean power (W) over the run.
    pub power_peak: f64,
    pub power_mean: f64,
    /// The highest rise over the surroundings (K), when, and the rise at the end of the run.
    pub rise_peak: f64,
    pub peak_time: f64,
    pub rise_end: f64,
    /// The highest rise if the links to the other parts were left out: what the others add is `rise_peak - rise_alone`.
    pub rise_alone: f64,
    /// The temperature (°C) the part had on average while it dissipated (power-weighted): the one its value was worked out at.
    pub temp_working_c: f64,
    /// Change of its value from the surroundings' temperature to that one (relative; 0 when it has no temperature coefficient).
    pub change: f64,
    pub power: Vec<f64>,
    pub rise: Vec<f64>,
}

#[derive(Clone, Debug)]
pub struct HeatTran {
    pub time: Vec<f64>,
    pub parts: Vec<HeatTranPart>,
    pub rounds: usize,
    /// The last transient run (with the values at the working temperatures).
    pub tran: TranResult,
    /// The first run, with every part at the surrounding temperature: what the warming changed is the difference between the two.
    pub cold: TranResult,
}

/// The parts of a schematic named in `refs` as thermal nodes: all the resistors, diodes, bipolar and MOS transistors inside each part
/// (a library part's macro model counts). θ and the coefficient come as in `heaters_from_schematic`, the capacity from the part.
pub fn thermal_nodes_from_schematic(sch: &Schematic, circuit: &Circuit, refs: &[String]) -> Result<Vec<ThermalNode>, String> {
    let mut out = vec![];
    for r in refs {
        let comp = sch.components.iter().find(|c| !c.kind.is_power_symbol() && c.refdes.eq_ignore_ascii_case(r)).ok_or_else(|| format!("no part {r}"))?;
        let name = comp.refdes.as_str();
        let prefix = format!("{name}.");
        let elements: Vec<String> = circuit
            .elements
            .iter()
            .filter_map(|e| match e {
                Element::Resistor { name: n, .. } | Element::Diode { name: n, .. } | Element::Bjt { name: n, .. } | Element::Mosfet { name: n, .. }
                    if n == name || n.starts_with(&prefix) =>
                {
                    Some(n.clone())
                }
                _ => None,
            })
            .collect();
        if elements.is_empty() {
            return Err(format!("{name} has no resistor, diode or transistor in the circuit (nothing that dissipates power)"));
        }
        let theta = if comp.theta > 0.0 { comp.theta } else { default_theta(&comp.footprint_name()).unwrap_or(100.0) };
        let (tc1, tc2) = if comp.tc_ppm != 0.0 {
            (comp.tc_ppm * 1e-6, 0.0)
        } else {
            circuit.tempcos.iter().find(|(n, _, _)| elements.contains(n)).map_or((0.0, 0.0), |(_, a, b)| (*a, *b))
        };
        out.push(ThermalNode { heater: Heater { name: name.to_string(), elements, theta, tc1, tc2 }, cth: comp.cth });
    }
    Ok(out)
}

/// Power (W) of one element at every time point of a run (never below zero: a part that gives energy back is not cooling).
fn element_power(c: &Circuit, tr: &TranResult, e: &Element) -> Vec<f64> {
    let n = tr.time.len();
    let volt = |idx: usize| -> Vec<f64> {
        if idx == 0 {
            vec![0.0; n]
        } else {
            c.names.get(idx).and_then(|nm| tr.signal(&format!("V({nm})"))).map_or(vec![0.0; n], |s| s.values.clone())
        }
    };
    let cur = |name: &str| tr.signal(&format!("I({name})")).map_or(vec![0.0; n], |s| s.values.clone());
    let across = |name: &str, p: usize, q: usize| -> Vec<f64> {
        let (i, vp, vq) = (cur(name), volt(p), volt(q));
        (0..n).map(|k| (i[k] * (vp[k] - vq[k])).max(0.0)).collect()
    };
    match e {
        Element::Resistor { name, r, .. } => cur(name).iter().map(|i| i * i * r).collect(),
        Element::Diode { name, a, k, .. } => across(name, *a, *k),
        Element::Bjt { name, c: col, e: em, .. } => across(name, *col, *em),
        Element::Mosfet { name, d, s, .. } => across(name, *d, *s),
        _ => vec![0.0; n],
    }
}

fn solve(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Vec<f64> {
    let n = b.len();
    for i in 0..n {
        let p = (i..n).max_by(|&x, &y| a[x][i].abs().total_cmp(&a[y][i].abs())).unwrap_or(i);
        a.swap(i, p);
        b.swap(i, p);
        for j in i + 1..n {
            let f = a[j][i] / a[i][i];
            for k in i..n {
                a[j][k] -= f * a[i][k];
            }
            b[j] -= f * b[i];
        }
    }
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        x[i] = (b[i] - (i + 1..n).map(|k| a[i][k] * x[k]).sum::<f64>()) / a[i][i];
    }
    x
}

/// Rise (K) of every node over the surroundings at every time point: `C·du/dt = P − u/θ − Σ (u − u_other)/θ_link`, integrated by the backward
/// Euler step on the points of `time` (the power in a step is the mean of its two ends). The run starts with everything at the surroundings.
fn integrate(time: &[f64], power: &[Vec<f64>], nodes: &[ThermalNode], links: &[(usize, usize, f64)]) -> Vec<Vec<f64>> {
    let n = nodes.len();
    let mut g = vec![vec![0.0; n]; n];
    for (k, nd) in nodes.iter().enumerate() {
        g[k][k] += 1.0 / nd.heater.theta;
    }
    for &(a, b, th) in links {
        g[a][a] += 1.0 / th;
        g[b][b] += 1.0 / th;
        g[a][b] -= 1.0 / th;
        g[b][a] -= 1.0 / th;
    }
    let mut u = vec![vec![0.0; time.len()]; n];
    for i in 1..time.len() {
        let dt = (time[i] - time[i - 1]).max(1e-15);
        let mut m = g.clone();
        let mut rhs = vec![0.0; n];
        for k in 0..n {
            let c = nodes[k].cth / dt;
            m[k][k] += c;
            rhs[k] = c * u[k][i - 1] + 0.5 * (power[k][i - 1] + power[k][i]);
        }
        let x = solve(m, rhs);
        for k in 0..n {
            u[k][i] = x[k];
        }
    }
    u
}

/// The transient run with the parts' own power turned into temperatures (see the module comment). `t_amb` is the surroundings (°C).
pub fn heat_tran(circuit: &Circuit, nodes: &[ThermalNode], links: &[ThermalLink], t_amb: f64, s: &TranSettings) -> Result<HeatTran, SimError> {
    if nodes.is_empty() {
        return Err(SimError::Invalid("no part to warm: name at least one (parts=Q1,RS1)".into()));
    }
    if let Some(nd) = nodes.iter().find(|n| !(n.heater.theta > 0.0) || n.cth < 0.0) {
        return Err(SimError::Invalid(format!("{}: the heat resistance must be above zero and the heat capacity zero or more", nd.heater.name)));
    }
    let index = |name: &str| nodes.iter().position(|n| n.heater.name.eq_ignore_ascii_case(name));
    let mut ties = vec![];
    for l in links {
        let (Some(a), Some(b)) = (index(&l.a), index(&l.b)) else {
            return Err(SimError::Invalid(format!("couple {}-{}: both parts must be among the parts to warm", l.a, l.b)));
        };
        if a == b || !(l.theta > 0.0) {
            return Err(SimError::Invalid(format!("couple {}-{}: two different parts and a heat resistance above zero", l.a, l.b)));
        }
        ties.push((a, b, l.theta));
    }
    // heated resistors keep their own coefficients (set below); everything else is taken to the surrounding temperature
    let mut base = circuit.clone();
    base.tempcos.retain(|(n, _, _)| !nodes.iter().any(|nd| nd.heater.elements.contains(n)));
    // the transistors and diodes of each part: they follow the part's temperature through the model's own temperature rules
    let junctions: Vec<Vec<String>> = nodes
        .iter()
        .map(|nd| {
            circuit.elements.iter().filter(|e| matches!(e, Element::Diode { .. } | Element::Bjt { .. } | Element::Mosfet { .. }) && nd.heater.elements.iter().any(|n| n == e.name())).map(|e| e.name().to_string()).collect()
        })
        .collect();
    let nominal = |name: &str| -> f64 {
        circuit.elements.iter().find_map(|e| match e {
            Element::Resistor { name: n, r, .. } if n == name => Some(*r),
            _ => None,
        }).unwrap_or(0.0)
    };
    let follows = |k: usize| nodes[k].heater.tc1 != 0.0 || nodes[k].heater.tc2 != 0.0 || !junctions[k].is_empty();
    let feedback = (0..nodes.len()).any(follows);
    let mut temps = vec![t_amb; nodes.len()];
    let mut rounds = 0;
    let mut cold: Option<TranResult> = None;
    loop {
        rounds += 1;
        let own: Vec<(String, f64)> = junctions.iter().zip(&temps).flat_map(|(names, &t)| names.iter().map(move |n| (n.clone(), t))).collect();
        let mut c = base.at_temperatures(t_amb, &own);
        for (nd, &t) in nodes.iter().zip(&temps) {
            for e in &mut c.elements {
                if let Element::Resistor { name, r, .. } = e {
                    if nd.heater.elements.contains(name) {
                        *r = nominal(name) * scale(&nd.heater, t);
                    }
                }
            }
        }
        let tr = transient(&c, s)?;
        if cold.is_none() {
            cold = Some(tr.clone());
        }
        let power: Vec<Vec<f64>> = nodes
            .iter()
            .map(|nd| {
                let mut sum = vec![0.0; tr.time.len()];
                for e in c.elements.iter().filter(|e| nd.heater.elements.iter().any(|n| n == e.name())) {
                    for (a, b) in sum.iter_mut().zip(element_power(&c, &tr, e)) {
                        *a += b;
                    }
                }
                sum
            })
            .collect();
        let rise = integrate(&tr.time, &power, nodes, &ties);
        let alone = integrate(&tr.time, &power, nodes, &[]);
        // the temperature each part had while it dissipated: the weight is its power
        let weighted: Vec<f64> = (0..nodes.len())
            .map(|k| {
                let (mut wp, mut wt) = (0.0, 0.0);
                for i in 1..tr.time.len() {
                    let dt = tr.time[i] - tr.time[i - 1];
                    let p = 0.5 * (power[k][i - 1] + power[k][i]) * dt;
                    wp += p;
                    wt += p * 0.5 * (rise[k][i - 1] + rise[k][i]);
                }
                if wp > 0.0 { t_amb + wt / wp } else { t_amb }
            })
            .collect();
        let moved = (0..nodes.len()).map(|k| if follows(k) { (weighted[k] - temps[k]).abs() } else { 0.0 }).fold(0.0, f64::max);
        if !feedback || moved < 1e-3 || rounds >= 8 {
            let parts = nodes
                .iter()
                .enumerate()
                .map(|(k, nd)| {
                    let t = &tr.time;
                    let mean = (1..t.len()).map(|i| 0.5 * (power[k][i - 1] + power[k][i]) * (t[i] - t[i - 1])).sum::<f64>() / (t[t.len() - 1] - t[0]).max(1e-15);
                    let (ip, peak) = rise[k].iter().enumerate().fold((0, 0.0_f64), |a, (i, &x)| if x > a.1 { (i, x) } else { a });
                    HeatTranPart {
                        name: nd.heater.name.clone(),
                        theta: nd.heater.theta,
                        cth: nd.cth,
                        power_peak: power[k].iter().cloned().fold(0.0, f64::max),
                        power_mean: mean,
                        rise_peak: peak,
                        peak_time: t[ip],
                        rise_end: *rise[k].last().unwrap_or(&0.0),
                        rise_alone: alone[k].iter().cloned().fold(0.0, f64::max),
                        temp_working_c: weighted[k],
                        change: scale(&nd.heater, weighted[k]) / scale(&nd.heater, t_amb) - 1.0,
                        power: power[k].clone(),
                        rise: rise[k].clone(),
                    }
                })
                .collect();
            let cold = cold.unwrap_or_else(|| tr.clone());
            return Ok(HeatTran { time: tr.time.clone(), parts, rounds, tran: tr, cold });
        }
        temps = weighted;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 10 V pulse (on for `ton`) into 100 Ω (R1) in series with 100 Ω (R2): R1 dissipates 0.25 W while on.
    fn pulse(ton: f64) -> Circuit {
        let mut c = Circuit::new();
        let (a, o) = (c.node("a"), c.node("o"));
        c.add(Element::VSource { name: "V1".into(), p: a, n: 0, wave: crate::waveform::Waveform::Pulse { v1: 0.0, v2: 10.0, td: 0.0, tr: 1e-6, tf: 1e-6, pw: ton, per: 1e3 }, ac: None });
        c.add(Element::Resistor { name: "R1".into(), a, b: o, r: 100.0 });
        c.add(Element::Resistor { name: "R2".into(), a: o, b: 0, r: 100.0 });
        c
    }

    fn node(name: &str, theta: f64, cth: f64) -> ThermalNode {
        ThermalNode { heater: Heater { name: name.into(), elements: vec![name.into()], theta, tc1: 0.0, tc2: 0.0 }, cth }
    }

    #[test]
    fn a_short_pulse_warms_less_than_the_steady_state_by_the_time_constant() {
        // θ = 100 K/W, C = 0.05 J/K: τ = 5 s. A 1 s pulse of 0.25 W reaches 25·(1 − e^(−1/5)) = 4.53 K
        let s = TranSettings { tstop: 3.0, tstep: 0.01, uic: false };
        let r = heat_tran(&pulse(1.0), &[node("R1", 100.0, 0.05)], &[], 25.0, &s).unwrap();
        let p = &r.parts[0];
        assert!((p.power_peak - 0.25).abs() < 1e-6, "{}", p.power_peak);
        assert!((p.rise_peak - 25.0 * (1.0 - (-0.2f64).exp())).abs() < 0.1, "{}", p.rise_peak);
        assert!(p.peak_time > 0.99 && p.peak_time < 1.02 && p.rise_end < p.rise_peak);
    }

    #[test]
    fn a_link_passes_heat_to_a_cold_neighbour() {
        let s = TranSettings { tstop: 60.0, tstep: 0.5, uic: false };
        let nodes = [node("R1", 100.0, 0.05), node("R2", 100.0, 0.05)];
        let alone = heat_tran(&pulse(1e3), &nodes, &[], 25.0, &s).unwrap();
        let tied = heat_tran(&pulse(1e3), &nodes, &[ThermalLink { a: "R1".into(), b: "R2".into(), theta: 50.0 }], 25.0, &s).unwrap();
        // both dissipate 0.25 W here, so a link between two equal parts changes nothing; the point is the bookkeeping
        assert!((alone.parts[0].rise_peak - 25.0).abs() < 0.2);
        assert!((tied.parts[0].rise_peak - tied.parts[0].rise_alone).abs() < 0.2);
        // a cold neighbour: R2 given no power (R2 not in the circuit as a heater of its own) takes the heat from R1
        let cold = ThermalNode { heater: Heater { name: "X".into(), elements: vec![], theta: 100.0, tc1: 0.0, tc2: 0.0 }, cth: 0.05 };
        let r = heat_tran(&pulse(1e3), &[node("R1", 100.0, 0.05), cold], &[ThermalLink { a: "R1".into(), b: "X".into(), theta: 100.0 }], 25.0, &s).unwrap();
        let (hot, x) = (&r.parts[0], &r.parts[1]);
        // R1: 0.25 W into θ=100 in parallel with 100+100 → 100‖200 = 66.7 K/W → 16.7 K; X sits at half of that (divider 100/200)
        assert!((hot.rise_peak - 16.67).abs() < 0.3 && (x.rise_peak - 8.33).abs() < 0.3, "{} {}", hot.rise_peak, x.rise_peak);
        assert!(hot.rise_alone > hot.rise_peak + 5.0);
    }

    #[test]
    fn a_diode_that_warms_itself_drops_less_voltage() {
        // 5 V pulse into 100 ohm and a diode: ~40 mW in the diode; with 500 K/W and a small capacity it warms by tens of K, and a silicon
        // junction gives up about 2 mV per K
        let mut c = Circuit::new();
        let (a, o) = (c.node("a"), c.node("o"));
        c.add(Element::VSource { name: "V1".into(), p: a, n: 0, wave: crate::waveform::Waveform::Pulse { v1: 0.0, v2: 5.0, td: 0.0, tr: 1e-6, tf: 1e-6, pw: 1e3, per: 1e4 }, ac: None });
        c.add(Element::Resistor { name: "R1".into(), a, b: o, r: 100.0 });
        c.add(Element::Diode { name: "D1".into(), a: o, k: 0, model: crate::circuit::DiodeModel::default() });
        let hot = ThermalNode { heater: Heater { name: "D1".into(), elements: vec!["D1".into()], theta: 500.0, tc1: 0.0, tc2: 0.0 }, cth: 0.002 };
        let r = heat_tran(&c, &[hot], &[], 27.0, &TranSettings { tstop: 6.0, tstep: 0.05, uic: false }).unwrap();
        let (cold_v, warm_v) = (*r.cold.signal("V(o)").unwrap().values.last().unwrap(), *r.tran.signal("V(o)").unwrap().values.last().unwrap());
        let rise = r.parts[0].rise_end;
        assert!(rise > 10.0, "{rise}");
        assert!(r.rounds > 1 && cold_v - warm_v > 0.001 * rise, "{cold_v} {warm_v} {rise}");
    }
}
