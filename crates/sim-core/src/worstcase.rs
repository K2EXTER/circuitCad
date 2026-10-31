//! Worst-case analysis: every part with a tolerance sits at one end of it, chosen so that one result is as large (or as small) as it
//! can get. Monte Carlo shows what is likely; this shows the limits a design has to live with ("the output is never below 4.8 V").
//!
//! Each part is moved alone to both ends of its tolerance to see which end pushes the result up. Putting every part at its
//! pushing end gives a corner, and the corner is solved as a real circuit (so a result that does not move in a straight line
//! with the parts is still right). One round of trying the other end of each part then catches the parts that do not push the same
//! way in the corner as alone.

use crate::circuit::{Circuit, Element};
use crate::dc::SimError;
use crate::montecarlo::{measure, scale, McMeasure, McSettings, TranMetric};

#[derive(Clone, Debug)]
pub struct WcSettings {
    /// Tolerances as fractions (0.05 = ±5 %). `tol_v` is for the DC voltage sources (a supply that is 5 V ± 5 %).
    pub tol_r: f64,
    pub tol_c: f64,
    pub tol_l: f64,
    pub tol_v: f64,
    pub output: String,
    pub measure: McMeasure,
}

impl From<&McSettings> for WcSettings {
    fn from(s: &McSettings) -> Self {
        Self { tol_r: s.tol_r, tol_c: s.tol_c, tol_l: s.tol_l, tol_v: 0.0, output: s.output.clone(), measure: s.measure.clone() }
    }
}

/// What one part does to the result on its own.
#[derive(Clone, Debug)]
pub struct WcPart {
    pub name: String,
    pub tol: f64,
    /// Change of the result with only this part at its low end / high end.
    pub low: f64,
    pub high: f64,
}

impl WcPart {
    /// How far the result can move because of this part alone.
    pub fn span(&self) -> f64 {
        self.low.max(self.high).max(0.0) - self.low.min(self.high).min(0.0)
    }
}

#[derive(Clone, Debug)]
pub struct WorstCase {
    pub output: String,
    pub unit: &'static str,
    pub nominal: f64,
    /// The solved corners: the smallest and the largest result found.
    pub min: f64,
    pub max: f64,
    /// The same limits when the effects of the parts simply add up (no solving): the sum of what each part does alone.
    pub min_sum: f64,
    pub max_sum: f64,
    /// The limits when the parts are independent and add as squares (the root of the sum of squares): what is likely at worst.
    pub min_rss: f64,
    pub max_rss: f64,
    /// Every part with a tolerance, the one that matters most first.
    pub parts: Vec<WcPart>,
    /// For the smallest / largest result: each part and whether it sits at its high end.
    pub min_corner: Vec<(String, bool)>,
    pub max_corner: Vec<(String, bool)>,
    /// Parts whose own runs did not converge (they are left out of the sums and sit at their printed value in the corners).
    pub unsolved: Vec<String>,
}

fn tolerance(e: &Element, s: &WcSettings) -> Option<f64> {
    if e.name().contains('.') {
        return None;
    }
    let t = match e {
        Element::Resistor { .. } => s.tol_r,
        Element::Capacitor { .. } => s.tol_c,
        Element::Inductor { .. } => s.tol_l,
        Element::VSource { wave: crate::waveform::Waveform::Dc(v), .. } if *v != 0.0 => s.tol_v,
        _ => return None,
    };
    (t > 0.0).then_some(t)
}

fn corner(circuit: &Circuit, parts: &[(usize, f64)], high: &[bool]) -> Circuit {
    let mut c = circuit.clone();
    for (&(i, tol), &h) in parts.iter().zip(high) {
        scale(&mut c.elements[i], 1.0 + if h { tol } else { -tol });
    }
    c
}

pub fn worst_case(circuit: &Circuit, s: &WcSettings) -> Result<WorstCase, SimError> {
    if [s.tol_r, s.tol_c, s.tol_l, s.tol_v].iter().any(|t| !(0.0..1.0).contains(t)) {
        return Err(SimError::Invalid("許容差は 0〜100 % 未満にしてください".into()));
    }
    let output = s.output.trim().to_string();
    let nominal = measure(circuit, &output, &s.measure)?;
    let parts: Vec<(usize, f64)> = circuit.elements.iter().enumerate().filter_map(|(i, e)| tolerance(e, s).map(|t| (i, t))).collect();
    if parts.is_empty() {
        return Err(SimError::Invalid("許容差のある部品 (抵抗・コンデンサ・コイル) がありません。許容差を 0 より大きくしてください".into()));
    }

    // every part alone at both ends
    let jobs: Vec<(usize, bool)> = (0..parts.len()).flat_map(|k| [(k, false), (k, true)]).collect();
    let alone = crate::par_map(&jobs, |&(k, h)| {
        let mut c = circuit.clone();
        let (i, tol) = parts[k];
        scale(&mut c.elements[i], 1.0 + if h { tol } else { -tol });
        measure(&c, &output, &s.measure).ok().filter(|v| v.is_finite())
    });
    let mut unsolved = vec![];
    let mut wc_parts = vec![];
    for (k, &(i, tol)) in parts.iter().enumerate() {
        let (lo, hi) = (alone[2 * k], alone[2 * k + 1]);
        if lo.is_none() || hi.is_none() {
            unsolved.push(circuit.elements[i].name().to_string());
        }
        // a side that did not solve counts as no change
        wc_parts.push(WcPart { name: circuit.elements[i].name().to_string(), tol, low: lo.map_or(0.0, |v| v - nominal), high: hi.map_or(0.0, |v| v - nominal) });
    }

    // the corner that pushes the result up (or down): each part at the end that does it
    let pick = |up: bool| -> Vec<bool> { wc_parts.iter().map(|p| if up { p.high >= p.low } else { p.high < p.low }).collect() };
    let better = |a: f64, b: f64, up: bool| if up { a > b } else { a < b };
    let solve_corner = |high: &[bool]| measure(&corner(circuit, &parts, high), &output, &s.measure).ok().filter(|v| v.is_finite());
    let find = |up: bool| -> (f64, Vec<bool>) {
        let mut high = pick(up);
        let mut best = solve_corner(&high).unwrap_or(nominal);
        // one round: try the other end of each part in the corner; the parts that do better there change over
        let tries: Vec<usize> = (0..parts.len()).collect();
        let flipped = crate::par_map(&tries, |&k| {
            let mut h = high.clone();
            h[k] = !h[k];
            solve_corner(&h)
        });
        let wins: Vec<usize> = (0..parts.len()).filter(|&k| flipped[k].is_some_and(|v| better(v, best, up))).collect();
        if !wins.is_empty() {
            let mut h = high.clone();
            for &k in &wins {
                h[k] = !h[k];
            }
            match solve_corner(&h) {
                Some(v) if better(v, best, up) => {
                    best = v;
                    high = h;
                }
                _ => {
                    // changing them all together does not help: take the single best change
                    let k = *wins.iter().max_by(|&&a, &&b| if better(flipped[a].unwrap(), flipped[b].unwrap(), up) { std::cmp::Ordering::Greater } else { std::cmp::Ordering::Less }).unwrap();
                    best = flipped[k].unwrap();
                    high[k] = !high[k];
                }
            }
        }
        (best, high)
    };
    let (max, max_high) = find(true);
    let (min, min_high) = find(false);

    let up_sum: f64 = wc_parts.iter().map(|p| p.low.max(p.high).max(0.0)).sum();
    let down_sum: f64 = wc_parts.iter().map(|p| p.low.min(p.high).min(0.0)).sum();
    let up_rss = wc_parts.iter().map(|p| p.low.max(p.high).max(0.0).powi(2)).sum::<f64>().sqrt();
    let down_rss = wc_parts.iter().map(|p| p.low.min(p.high).min(0.0).powi(2)).sum::<f64>().sqrt();
    wc_parts.sort_by(|a, b| b.span().total_cmp(&a.span()));
    let named = |high: &[bool]| -> Vec<(String, bool)> { parts.iter().zip(high).map(|(&(i, _), &h)| (circuit.elements[i].name().to_string(), h)).collect() };
    let unit = match &s.measure {
        McMeasure::DcVoltage => "V",
        McMeasure::Cutoff { .. } => "Hz",
        McMeasure::Tran { metric: TranMetric::RiseTime | TranMetric::SettlingTime, .. } => "s",
        McMeasure::Tran { metric: TranMetric::Overshoot, .. } => "%",
        McMeasure::Tran { metric: TranMetric::FinalValue, .. } => "V",
    };
    Ok(WorstCase {
        output,
        unit,
        nominal,
        // the printed values are one of the possible circuits
        min: min.min(nominal),
        max: max.max(nominal),
        min_sum: nominal + down_sum,
        max_sum: nominal + up_sum,
        min_rss: nominal - down_rss,
        max_rss: nominal + up_rss,
        parts: wc_parts,
        min_corner: named(&min_high),
        max_corner: named(&max_high),
        unsolved,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::Circuit;

    /// 5 V - R1 - MID - R2 - ground.
    fn divider(r1: f64, r2: f64) -> Circuit {
        let mut c = Circuit::new();
        let (vin, mid) = (c.node("VIN"), c.node("MID"));
        c.elements.push(Element::vdc("V1", vin, 0, 5.0));
        c.elements.push(Element::Resistor { name: "R1".into(), a: vin, b: mid, r: r1 });
        c.elements.push(Element::Resistor { name: "R2".into(), a: mid, b: 0, r: r2 });
        c
    }

    fn dc(tol_r: f64, tol_v: f64) -> WcSettings {
        WcSettings { tol_r, tol_c: 0.0, tol_l: 0.0, tol_v, output: "MID".into(), measure: McMeasure::DcVoltage }
    }

    #[test]
    fn a_divider_is_worst_with_one_resistor_high_and_the_other_low() {
        let w = worst_case(&divider(10e3, 10e3), &dc(0.01, 0.0)).unwrap();
        // R2 high, R1 low: 5 * 1.01 / 2 = 2.525; the other way 2.475
        assert!((w.max - 2.525).abs() < 1e-6 && (w.min - 2.475).abs() < 1e-6, "{} {}", w.min, w.max);
        assert!((w.nominal - 2.5).abs() < 1e-6);
        assert!(w.max_corner.contains(&("R2".to_string(), true)) && w.max_corner.contains(&("R1".to_string(), false)));
        assert!(w.min_corner.contains(&("R2".to_string(), false)) && w.min_corner.contains(&("R1".to_string(), true)));
        // the sum of what each part does alone is about the same here, and the likely limits are tighter
        assert!((w.max_sum - w.max).abs() < 1e-3);
        assert!(w.max_rss < w.max && w.min_rss > w.min);
        assert_eq!(w.parts.len(), 2);
    }

    #[test]
    fn a_supply_tolerance_adds_to_the_limits() {
        let w = worst_case(&divider(10e3, 10e3), &dc(0.01, 0.05)).unwrap();
        assert!((w.max - 5.25 * 1.01 / 2.0).abs() < 1e-6, "{}", w.max);
        assert_eq!(w.parts[0].name, "V1", "the supply matters most: {:?}", w.parts.iter().map(|p| &p.name).collect::<Vec<_>>());
    }

    #[test]
    fn a_cutoff_follows_the_capacitor_and_the_resistor() {
        // R 1k, C 100n: 1.59 kHz; with 5 % R and 10 % C the corner is 1/(2 pi 0.95k 90n) at most
        let mut c = Circuit::new();
        let (vin, out) = (c.node("VIN"), c.node("OUT"));
        c.elements.push(Element::VSource { name: "V1".into(), p: vin, n: 0, wave: crate::waveform::Waveform::Dc(0.0), ac: Some((1.0, 0.0)) });
        c.elements.push(Element::Resistor { name: "R1".into(), a: vin, b: out, r: 1000.0 });
        c.elements.push(Element::Capacitor { name: "C1".into(), a: out, b: 0, c: 100e-9 });
        let s = WcSettings { tol_r: 0.05, tol_c: 0.1, tol_l: 0.0, tol_v: 0.0, output: "OUT".into(), measure: McMeasure::Cutoff { fstart: 10.0, fstop: 1e6, points_per_decade: 50 } };
        let w = worst_case(&c, &s).unwrap();
        let f = |r: f64, cc: f64| 1.0 / (2.0 * std::f64::consts::PI * r * cc);
        // the sweep is sampled, so the frequency is interpolated: a few tenths of a percent
        assert!((w.max / f(950.0, 90e-9) - 1.0).abs() < 0.01, "{} vs {}", w.max, f(950.0, 90e-9));
        assert!((w.min / f(1050.0, 110e-9) - 1.0).abs() < 0.01, "{} vs {}", w.min, f(1050.0, 110e-9));
        assert_eq!(w.unit, "Hz");
    }

    #[test]
    fn no_tolerance_or_no_part_is_refused() {
        assert!(worst_case(&divider(1e3, 1e3), &dc(0.0, 0.0)).is_err());
        assert!(worst_case(&divider(1e3, 1e3), &WcSettings { output: "NOPE".into(), ..dc(0.01, 0.0) }).is_err());
    }
}
