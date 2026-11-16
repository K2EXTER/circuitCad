//! The surrounding temperature swept with the self-heating (candidate H8): at each ambient the parts warm by their own power on top of
//! it (`self_heat_coupled`), and each is checked against its temperature limit (`check_temperatures`). The plain temperature sweep
//! (`temp_sweep`) puts every part at the ambient; here a part runs hotter than the air by its own heat, so its margin shrinks with the
//! air one for one and a part whose heat grows with temperature (a transistor biased by a fixed voltage) runs away sooner. The result
//! is summed up as the worst case of each part: its hottest, the ambient where it reaches its limit, and the nets that move most.

use crate::circuit::Circuit;
use crate::ratings::{check_temperatures, TempCheck};
use crate::selfheat::{self_heat_coupled, HeatedPart, Heater};
use crate::SimError;
use sch_core::Schematic;

/// One ambient of the sweep.
#[derive(Clone, Debug)]
pub struct AmbientPoint {
    pub ambient: f64,
    pub parts: Vec<HeatedPart>,
    pub checks: Vec<TempCheck>,
}

/// A part over the whole sweep.
#[derive(Clone, Debug, PartialEq)]
pub struct PartWorst {
    pub name: String,
    /// Its hottest (°C) and the ambient of it.
    pub hottest_c: f64,
    pub at_ambient: f64,
    /// Its limit (°C) and whether the limit is a guess from the kind of part.
    pub t_max: f64,
    pub guessed_limit: bool,
    /// The ambient (°C) at which it reaches its limit, between two points of the sweep (straight between them); `None` if it stays below
    /// over the whole range.
    pub over_from: Option<f64>,
    /// The ambient (°C) at which it first comes within the caution band (`RatingLevel::Caution` or worse); `None` if never.
    pub caution_from: Option<f64>,
    /// Its rise above the air at the lowest and the highest ambient (K): the same when its heat does not change with temperature.
    pub rise_low: f64,
    pub rise_high: f64,
    /// More power than its rating lowered for the ambient, somewhere in the range.
    pub derated_over: bool,
}

/// How much a net moves over the sweep.
#[derive(Clone, Debug, PartialEq)]
pub struct NetDrift {
    pub net: String,
    pub low: f64,
    pub high: f64,
    /// At the lowest and the highest ambient (V).
    pub at_low: f64,
    pub at_high: f64,
}

#[derive(Clone, Debug)]
pub struct AmbientSweep {
    pub points: Vec<AmbientPoint>,
    /// The parts, the closest to their limit (or the furthest over) first.
    pub worst: Vec<PartWorst>,
    /// The nets that move the most (V), the most first.
    pub nets: Vec<NetDrift>,
    /// Where the self-heating could not settle (a part past 1000 °C: thermal runaway), and why; the sweep stops there.
    pub runaway: Option<(f64, String)>,
}

/// Sweeps the ambient from `t0` to `t1` in steps of `step` (°C), the self-heating solved at each (`heaters` with their θ, `mutual` the
/// board's couplings, as for `self_heat_coupled`).
pub fn ambient_sweep(sch: &Schematic, circuit: &Circuit, heaters: &[Heater], mutual: &[(String, String, f64)], t0: f64, t1: f64, step: f64) -> Result<AmbientSweep, SimError> {
    if !(t0.is_finite() && t1.is_finite() && step.is_finite()) || step <= 0.0 || t1 < t0 {
        return Err(SimError::Invalid("周囲の温度の開始・終了・刻みを確かめてください (開始 ≦ 終了、刻み > 0)".into()));
    }
    let n = ((t1 - t0) / step + 1e-9).floor() as usize + 1;
    if n > 200 {
        return Err(SimError::Invalid("点数が多すぎます (200 点まで)。刻みを大きくしてください".into()));
    }
    let mut points = vec![];
    let mut voltages: Vec<Vec<(String, f64)>> = vec![];
    let mut runaway = None;
    for k in 0..n {
        let ambient = t0 + step * k as f64;
        match self_heat_coupled(circuit, heaters, ambient, mutual) {
            Ok(r) => {
                let parts: Vec<HeatedPart> = r.parts.into_iter().filter(|p| p.power > 1e-9).collect();
                let checks = check_temperatures(sch, &parts, ambient);
                voltages.push(r.warm.voltages.iter().filter(|(n, _)| !n.contains('.') && n != "GND" && n != "0").cloned().collect());
                points.push(AmbientPoint { ambient, parts, checks });
            }
            Err(SimError::Invalid(why)) if !points.is_empty() => {
                runaway = Some((ambient, why));
                break;
            }
            Err(e) => return Err(e),
        }
    }
    // each part over the sweep
    let mut worst: Vec<PartWorst> = vec![];
    let names: Vec<String> = points.iter().flat_map(|p| p.checks.iter().map(|c| c.name.clone())).fold(vec![], |mut v, n| {
        if !v.contains(&n) {
            v.push(n);
        }
        v
    });
    for name in names {
        let series: Vec<(f64, &TempCheck)> = points.iter().filter_map(|p| p.checks.iter().find(|c| c.name == name).map(|c| (p.ambient, c))).collect();
        let Some(&(_, first)) = series.first() else { continue };
        let &(_, last) = series.last().unwrap();
        let hottest = series.iter().max_by(|a, b| a.1.temp_c.total_cmp(&b.1.temp_c)).unwrap();
        let cross = |pick: &dyn Fn(&TempCheck) -> f64| -> Option<f64> {
            // where pick(c) (the margin to a level) crosses zero, straight between the points
            for w in series.windows(2) {
                let (a, b) = (pick(w[0].1), pick(w[1].1));
                if a > 0.0 && b <= 0.0 {
                    return Some(w[0].0 + (w[1].0 - w[0].0) * a / (a - b));
                }
            }
            (pick(series[0].1) <= 0.0).then_some(series[0].0)
        };
        let over_from = cross(&|c: &TempCheck| c.limit.t_max - c.temp_c);
        let caution_from = series.iter().find(|(_, c)| c.level.is_some()).map(|(t, _)| *t);
        let derated_over = series.iter().any(|(_, c)| c.p_derated.is_some_and(|pd| points.iter().flat_map(|p| p.parts.iter()).any(|hp| hp.name == c.name && hp.power > pd)));
        worst.push(PartWorst {
            name,
            hottest_c: hottest.1.temp_c,
            at_ambient: hottest.0,
            t_max: last.limit.t_max,
            guessed_limit: last.limit.source == crate::LimitSource::Guess,
            over_from,
            caution_from,
            rise_low: first.temp_c - series[0].0,
            rise_high: last.temp_c - series.last().unwrap().0,
            derated_over,
        });
    }
    worst.sort_by(|a, b| (a.t_max - a.hottest_c).total_cmp(&(b.t_max - b.hottest_c)));
    // the nets that move
    let mut nets: Vec<NetDrift> = vec![];
    if let (Some(lo), Some(hi)) = (voltages.first(), voltages.last()) {
        for (net, v0) in lo {
            let all: Vec<f64> = voltages.iter().filter_map(|vs| vs.iter().find(|(n, _)| n == net).map(|x| x.1)).collect();
            let (low, high) = all.iter().fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
            let at_high = hi.iter().find(|(n, _)| n == net).map_or(*v0, |x| x.1);
            if high - low > 1e-9 {
                nets.push(NetDrift { net: net.clone(), low, high, at_low: *v0, at_high });
            }
        }
    }
    nets.sort_by(|a, b| (b.high - b.low).total_cmp(&(a.high - a.low)));
    nets.truncate(8);
    Ok(AmbientSweep { points, worst, nets, runaway })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heaters_from_schematic;
    use std::collections::BTreeMap;

    fn sample(name: &str) -> Schematic {
        let p = format!("{}/../../docs/examples/{name}.schjson", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    /// The heat-sink sample: U2 on its sink rises about 48 K whatever the air, so it reaches the 125 °C of its (guessed) limit when the air is
    /// about 125 − 48 = 77 °C; U1 without a sink is past its limit over the whole range.
    #[test]
    fn a_part_reaches_its_limit_when_the_air_takes_its_margin() {
        let s = sample("regulator_heatsink");
        let c = crate::from_schematic(&s).unwrap();
        let h = heaters_from_schematic(&s, &c, &BTreeMap::new());
        let r = ambient_sweep(&s, &c, &h, &[], 0.0, 100.0, 10.0).unwrap();
        assert_eq!(r.points.len(), 11);
        let part = |n: &str| r.worst.iter().find(|w| w.name == n).unwrap();
        let (u1, u2) = (part("U1"), part("U2"));
        assert_eq!(u1.over_from, Some(0.0), "{u1:?}");
        let at = u2.over_from.unwrap();
        assert!((at - (u2.t_max - u2.rise_high)).abs() < 3.0 && (70.0..85.0).contains(&at), "{u2:?}");
        assert!((u2.rise_low - u2.rise_high).abs() < 2.0, "a regulator's heat hardly follows the air: {u2:?}");
        assert_eq!(r.worst[0].name, "U1", "the worst first");
        assert!(r.runaway.is_none());
    }

    /// The transistor without an emitter resistor: as the air warms its current, and so its own heat, grows; its rise above the air at
    /// 80 °C is well above the one at 0 °C, while the one with an emitter resistor keeps nearly the same rise. A net it drives moves.
    #[test]
    fn a_part_that_feeds_its_own_heat_rises_faster_with_the_air() {
        let s = sample("bjt_self_heat");
        let c = crate::from_schematic(&s).unwrap();
        let h = heaters_from_schematic(&s, &c, &BTreeMap::new());
        let r = ambient_sweep(&s, &c, &h, &[], 0.0, 80.0, 20.0).unwrap();
        let part = |n: &str| r.worst.iter().find(|w| w.name == n).unwrap();
        let (q1, q2) = (part("Q1"), part("Q2"));
        assert!(q1.rise_high > 1.3 * q1.rise_low, "{q1:?}");
        assert!(q2.rise_high < 1.15 * q2.rise_low, "{q2:?}");
        assert!(r.nets.iter().any(|n| n.net == "C1" && n.at_high < n.at_low), "{:?}", r.nets);
        assert!(ambient_sweep(&s, &c, &h, &[], 50.0, 0.0, 10.0).is_err());
    }
}
