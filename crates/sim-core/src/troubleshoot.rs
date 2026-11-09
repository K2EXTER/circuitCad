//! Troubleshooting: from voltages measured on a circuit that does not work, which single fault explains them, and which point to
//! measure next.
//!
//! Every fault the fault simulation knows (a part open, shorted, off in value or in the wrong way round; a pin of an IC or a
//! transistor not soldered) is solved once, together with the healthy circuit: these are the hypotheses. A reading fits a
//! hypothesis when the voltage it predicts is within the measuring spread (20 mV + 2 %: the meter and the tolerances of the parts).
//! The fit, weighted by how often that fault happens, gives each hypothesis a probability. The point to measure next is the one
//! whose predicted voltages split the likely hypotheses best (the fewest left, on average, once it is read).
//!
//! Only one fault at a time is considered, and only the DC operating point: a circuit that oscillates, or two faults at once, is
//! not explained (the result says so when nothing fits).

use crate::circuit::Circuit;
use crate::dc::{dc_op, SimError};
use crate::fault::{applicable, break_part, breakable, likelihood, Likelihood};
use sch_core::{Kind, Schematic};

/// One voltage read on the circuit, against ground.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub node: String,
    pub volts: f64,
}

/// A possible state of the circuit: healthy (empty `element`) or with one fault.
#[derive(Clone, Debug)]
pub struct Hypothesis {
    /// The part (or "U1" for a pin fault); empty for the healthy circuit.
    pub element: String,
    /// What is wrong, in words ("断線 (オープン)", "ピン 3 (IN+) を断線"); "故障なし" for the healthy circuit.
    pub label: String,
    /// How often this happens, as a weight (healthy 1, common faults 1, occasional 0.3, rare 0.05).
    pub prior: f64,
    /// The DC voltage of every node of the schematic (not the insides of models).
    pub voltages: Vec<(String, f64)>,
}

impl Hypothesis {
    pub fn voltage(&self, node: &str) -> Option<f64> {
        self.voltages.iter().find(|(n, _)| n.eq_ignore_ascii_case(node)).map(|(_, v)| *v)
    }
}

/// The measuring spread at a voltage: a meter's last digits and the parts' tolerances.
pub fn spread(v: f64) -> f64 {
    0.02 + 0.02 * v.abs()
}

fn weight(l: Likelihood) -> f64 {
    match l {
        Likelihood::Common => 1.0,
        Likelihood::Occasional => 0.3,
        Likelihood::Rare => 0.05,
    }
}

/// The healthy circuit and every single fault, solved. `rare`: also the faults that hardly ever happen (they still count, with a
/// small weight). Faults whose circuit has no DC solution are left out (they cannot be measured as steady voltages either).
pub fn hypotheses(sch: &Schematic, temp_c: f64, rare: bool) -> Result<Vec<Hypothesis>, SimError> {
    let s = &*sch.flat();
    let join = |e: Vec<crate::ConvertError>| SimError::Invalid(e.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" / "));
    let c = crate::from_schematic(s).map_err(join)?.at_temperature(temp_c);
    let nodes = |r: &crate::DcResult| -> Vec<(String, f64)> { r.voltages.iter().filter(|(n, _)| !n.contains('.')).cloned().collect() };
    let healthy = dc_op(&c)?;
    let mut out = vec![Hypothesis { element: String::new(), label: "故障なし".into(), prior: 1.0, voltages: nodes(&healthy) }];

    // whole-part faults
    let mut jobs: Vec<(String, String, f64, Circuit)> = vec![];
    for part in breakable(&c) {
        let Some(e) = c.elements.iter().find(|e| e.name() == part) else { continue };
        for fault in applicable(e) {
            let l = likelihood(&c, &part, fault);
            if l == Likelihood::Rare && !rare {
                continue;
            }
            if let Some(b) = break_part(&c, &part, fault) {
                jobs.push((part.clone(), b.label, weight(l), b.circuit));
            }
        }
    }
    // a pin of an IC or a transistor not soldered: as common as anything on a board built by hand
    for comp in s.components.iter().filter(|x| matches!(x.kind, Kind::Custom | Kind::Npn) && !x.sim_only) {
        for (pin, _) in comp.pins() {
            let spec = crate::PinFaultSpec { refdes: comp.refdes.clone(), pin: pin.clone(), how: crate::PinHow::Open };
            let name = comp.custom.as_ref().and_then(|sym| sym.pins.iter().find(|p| p.number == pin)).map(|p| p.name.clone()).filter(|n| *n != pin);
            let label = match name {
                Some(n) => format!("ピン {pin} ({n}) を断線"),
                None => format!("ピン {pin} を断線"),
            };
            if let Ok(broken) = crate::from_schematic_with_pin_fault(s, &spec) {
                jobs.push((comp.refdes.clone(), label, 0.5, broken.at_temperature(temp_c)));
            }
        }
    }
    let solved = crate::par_map(&jobs, |(_, _, _, circuit)| dc_op(circuit).ok().map(|r| nodes(&r)));
    for ((element, label, prior, _), v) in jobs.into_iter().zip(solved) {
        let Some(voltages) = v else { continue };
        // the same part broken two ways that cannot be told apart by any voltage (a diode open, or its pin A open): one line
        let same = |h: &Hypothesis| h.element == element && h.voltages.len() == voltages.len() && h.voltages.iter().zip(&voltages).all(|(a, b)| (a.1 - b.1).abs() < 1e-3);
        match out.iter_mut().skip(1).find(|h| same(h)) {
            Some(h) => {
                h.label = format!("{} / {label}", h.label);
                h.prior += prior;
            }
            None => out.push(Hypothesis { element, label, prior, voltages }),
        }
    }
    Ok(out)
}

/// A hypothesis that fits the readings, with its share of the probability.
#[derive(Clone, Debug)]
pub struct Suspect {
    /// Index into the hypotheses.
    pub index: usize,
    pub probability: f64,
    /// For each reading: (node, predicted, measured).
    pub fit: Vec<(String, f64, f64)>,
    /// The largest miss of a reading, in measuring spreads (below 3: it fits).
    pub worst_miss: f64,
}

/// A point worth measuring next.
#[derive(Clone, Debug)]
pub struct NextPoint {
    pub node: String,
    /// How many hypotheses are left, on average, once this point is read (the effective number: 1 = the cause is known).
    pub left_after: f64,
    /// What the likely hypotheses predict there: (hypothesis index, volts), most likely first.
    pub predictions: Vec<(usize, f64)>,
}

#[derive(Clone, Debug)]
pub struct Diagnosis {
    /// The hypotheses that fit, most likely first (the ones with a probability under 0.1 % are left out).
    pub suspects: Vec<Suspect>,
    /// The effective number of hypotheses left now.
    pub left: f64,
    /// The points to measure next, best first (at most three).
    pub next: Vec<NextPoint>,
    /// No hypothesis fits all the readings: two faults at once, a measuring mistake, or what the model does not know.
    pub unexplained: bool,
    /// Readings at a node the circuit does not have.
    pub unknown_nodes: Vec<String>,
}

/// The effective number of choices of a distribution (the exponential of its entropy): 1 when one is certain.
fn effective(p: &[f64]) -> f64 {
    let total: f64 = p.iter().sum();
    if total <= 0.0 {
        return 0.0;
    }
    (-p.iter().filter(|x| **x > 0.0).map(|x| x / total * (x / total).ln()).sum::<f64>()).exp()
}

pub fn diagnose(hyps: &[Hypothesis], readings: &[Reading]) -> Diagnosis {
    let known = |n: &str| hyps.first().is_some_and(|h| h.voltage(n).is_some());
    let unknown_nodes: Vec<String> = readings.iter().filter(|r| !known(&r.node)).map(|r| r.node.clone()).collect();
    let readings: Vec<&Reading> = readings.iter().filter(|r| known(&r.node)).collect();
    // a heavy-tailed fit: one reading far off lowers a hypothesis a lot, but does not make a near miss and a far miss the same
    let mut logp = vec![];
    let mut misses = vec![];
    for h in hyps {
        let mut lp = h.prior.max(1e-12).ln();
        let mut worst: f64 = 0.0;
        for r in &readings {
            let pred = h.voltage(&r.node).unwrap_or(0.0);
            let z = (pred - r.volts) / spread(r.volts);
            worst = worst.max(z.abs());
            lp -= 2.0 * (1.0 + z * z / 4.0).ln();
        }
        logp.push(lp);
        misses.push(worst);
    }
    let top = logp.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let w: Vec<f64> = logp.iter().map(|l| (l - top).exp()).collect();
    let total: f64 = w.iter().sum();
    let prob: Vec<f64> = w.iter().map(|x| x / total).collect();
    let mut order: Vec<usize> = (0..hyps.len()).collect();
    order.sort_by(|&a, &b| prob[b].total_cmp(&prob[a]));
    let suspects: Vec<Suspect> = order
        .iter()
        .filter(|&&i| prob[i] >= 1e-3)
        .take(20)
        .map(|&i| Suspect {
            index: i,
            probability: prob[i],
            fit: readings.iter().map(|r| (r.node.clone(), hyps[i].voltage(&r.node).unwrap_or(0.0), r.volts)).collect(),
            worst_miss: misses[i],
        })
        .collect();
    let unexplained = !readings.is_empty() && suspects.iter().all(|s| s.worst_miss > 3.0);

    // the next point: group the hypotheses by what they predict there (within a measuring spread of each other); reading the
    // point leaves one group, so the expected effective number left is the groups' sizes weighted by their probability
    let measured: Vec<&str> = readings.iter().map(|r| r.node.as_str()).collect();
    let mut next = vec![];
    if let Some(first) = hyps.first() {
        for (node, _) in &first.voltages {
            if measured.iter().any(|m| m.eq_ignore_ascii_case(node)) || node == "0" || node.eq_ignore_ascii_case("GND") {
                continue;
            }
            let mut pts: Vec<(f64, f64)> = (0..hyps.len()).filter(|&i| prob[i] > 1e-6).map(|i| (hyps[i].voltage(node).unwrap_or(0.0), prob[i])).collect();
            pts.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut groups: Vec<Vec<f64>> = vec![];
            let mut start = f64::NEG_INFINITY;
            for (v, p) in pts {
                if groups.is_empty() || v - start > 2.0 * spread(v) {
                    groups.push(vec![]);
                    start = v;
                }
                groups.last_mut().unwrap().push(p);
            }
            let left_after: f64 = groups.iter().map(|g| g.iter().sum::<f64>() * effective(g)).sum();
            let predictions: Vec<(usize, f64)> = order.iter().filter(|&&i| prob[i] >= 0.01).take(6).map(|&i| (i, hyps[i].voltage(node).unwrap_or(0.0))).collect();
            next.push(NextPoint { node: node.clone(), left_after, predictions });
        }
    }
    let left = effective(&prob);
    // only points that narrow things down
    next.retain(|n| n.left_after < left * 0.95);
    next.sort_by(|a, b| a.left_after.total_cmp(&b.left_after));
    next.truncate(3);
    Diagnosis { suspects, left, next, unexplained, unknown_nodes }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sch_core::Label;

    fn tie(s: &mut Schematic, idx: usize, pin: &str, net: &str) {
        let pt = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap().1;
        s.labels.push(Label { pos: pt, text: net.into() });
    }

    /// 10 V → R1 10k → MID → R2 10k → GND, and MID → R3 1k → OUT → R4 10k → GND.
    fn ladder() -> Schematic {
        let mut s = Schematic::default();
        let v = s.add(Kind::VSource, (0, 0), 0);
        s.components[v].value = "10".into();
        let g = s.add(Kind::Ground, (0, 10), 0);
        tie(&mut s, v, "+", "VIN");
        tie(&mut s, v, "-", "GND");
        tie(&mut s, g, "1", "GND");
        for (k, (a, b, val)) in [("VIN", "MID", "10k"), ("MID", "GND", "10k"), ("MID", "OUT", "1k"), ("OUT", "GND", "10k")].iter().enumerate() {
            let r = s.add(Kind::Resistor, (10 + 10 * k as i32, 0), 0);
            s.components[r].value = val.to_string();
            tie(&mut s, r, "1", a);
            tie(&mut s, r, "2", b);
        }
        s
    }

    #[test]
    fn a_dead_output_points_at_the_parts_that_can_kill_it_and_the_next_point_splits_them() {
        let hyps = hypotheses(&ladder(), 27.0, false).unwrap();
        assert_eq!(hyps[0].label, "故障なし");
        // healthy: R2 || (R3 + R4) = 5.238k, MID = 10 * 5.238 / 15.238 = 3.4375 V, OUT = MID * 10/11 = 3.125 V
        let ok = hyps[0].voltage("OUT").unwrap();
        assert!((ok - 3.125).abs() < 1e-3, "{ok}");
        // measured: OUT is 0 V
        let d = diagnose(&hyps, &[Reading { node: "OUT".into(), volts: 0.0 }]);
        assert!(!d.unexplained);
        let names: Vec<String> = d.suspects.iter().take(3).map(|s| format!("{} {}", hyps[s.index].element, hyps[s.index].label)).collect();
        // R1 open or R3 open (both common enough) explain a dead OUT; the healthy circuit does not
        assert!(names.iter().any(|n| n.starts_with("R1 ") && n.contains("断線")), "{names:?}");
        assert!(names.iter().any(|n| n.starts_with("R3 ") && n.contains("断線")), "{names:?}");
        assert!(d.suspects.iter().all(|s| !hyps[s.index].element.is_empty() || s.probability < 0.01));
        // MID tells them apart: 0 V with R1 open, about 5 V with R3 open (VIN splits off "the supply is dead" just as well)
        assert!(d.next.iter().take(2).any(|n| n.node == "MID"), "{:?}", d.next.iter().map(|n| (&n.node, n.left_after)).collect::<Vec<_>>());
        // reading MID = 5 V settles it
        let d2 = diagnose(&hyps, &[Reading { node: "OUT".into(), volts: 0.0 }, Reading { node: "MID".into(), volts: 5.0 }]);
        let best = &hyps[d2.suspects[0].index];
        assert_eq!((best.element.as_str(), d2.suspects[0].probability > 0.5), ("R3", true), "{}", best.label);
        assert!(d2.left < d.left);
    }

    #[test]
    fn a_healthy_reading_says_healthy_and_nonsense_is_flagged() {
        let hyps = hypotheses(&ladder(), 27.0, false).unwrap();
        let ok = hyps[0].voltage("OUT").unwrap();
        let d = diagnose(&hyps, &[Reading { node: "OUT".into(), volts: ok + 0.01 }]);
        assert_eq!(hyps[d.suspects[0].index].label, "故障なし");
        let d = diagnose(&hyps, &[Reading { node: "OUT".into(), volts: 123.0 }, Reading { node: "NOPE".into(), volts: 1.0 }]);
        assert!(d.unexplained);
        assert_eq!(d.unknown_nodes, ["NOPE"]);
    }
}
