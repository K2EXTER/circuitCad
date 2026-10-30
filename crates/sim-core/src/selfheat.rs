//! Self-heating of resistors. A resistor that carries current warms up by `P · θ` above its surroundings (θ in K/W is the heat
//! resistance from the resistive element to the air, board copper included), and a resistor with a temperature coefficient
//! changes its value as it warms, which changes the current and the power: the answer is found by iterating until the
//! temperatures stop moving. This is a steady state (DC); the board's own heat spreading is in θ, not simulated.

use crate::circuit::{Circuit, Element};
use crate::dc::{dc_op, DcResult, SimError};
use crate::temperature::T_NOM_C;
use sch_core::{Kind, Schematic};

/// A part that warms itself: the resistors inside it (all share one temperature), its heat resistance and temperature coefficients.
#[derive(Clone, Debug, PartialEq)]
pub struct Heater {
    /// The part (reference designator).
    pub name: String,
    /// Names of its resistor elements in the circuit (a library part may hold several).
    pub elements: Vec<String>,
    /// Heat resistance from the element to the surroundings (K/W).
    pub theta: f64,
    /// `R(T) = R·(1 + tc1·ΔT + tc2·ΔT²)` with ΔT from 27 °C.
    pub tc1: f64,
    pub tc2: f64,
}

/// What a heated part did.
#[derive(Clone, Debug, PartialEq)]
pub struct HeatedPart {
    pub name: String,
    /// Power it dissipates once warm (W).
    pub power: f64,
    /// How much hotter than the surroundings it is (K).
    pub rise: f64,
    pub temp_c: f64,
    /// Change of its resistance caused by the warming, relative (0.001 = +0.1 %).
    pub change: f64,
    pub theta: f64,
    pub tc1: f64,
    pub tc2: f64,
}

#[derive(Clone, Debug)]
pub struct SelfHeat {
    pub parts: Vec<HeatedPart>,
    /// The operating point once the parts have warmed up, and the one with every part at the surrounding temperature.
    pub warm: DcResult,
    pub cold: DcResult,
    pub rounds: usize,
}

/// Typical heat resistance (K/W) of a package, from its rated power: a chip resistor reaches its 155 °C limit at the rated power
/// when the surroundings are 70 °C, so θ ≈ 85 K / P. A rough number; a data sheet's θ (and the board copper) is better.
pub fn default_theta(footprint: &str) -> Option<f64> {
    crate::package_power_limit(footprint).map(|p| 85.0 / p)
}

/// The parts of a schematic that can heat themselves: every resistor, and library parts that hold resistors. θ comes from the part
/// (`theta`, or a guess from the package); the temperature coefficient from the part (`tc_ppm`) or from the model (`TC=`).
pub fn heaters_from_schematic(sch: &Schematic, circuit: &Circuit) -> Vec<Heater> {
    let mut out = vec![];
    for comp in &sch.components {
        if !matches!(comp.kind, Kind::Resistor | Kind::Custom) {
            continue;
        }
        let r = comp.refdes.as_str();
        let prefix = format!("{r}.");
        let elements: Vec<String> = circuit
            .elements
            .iter()
            .filter_map(|e| match e {
                Element::Resistor { name, .. } if name == r || name.starts_with(&prefix) => Some(name.clone()),
                _ => None,
            })
            .collect();
        if elements.is_empty() {
            continue;
        }
        let theta = if comp.theta > 0.0 { comp.theta } else { default_theta(&comp.footprint_name()).unwrap_or(100.0) };
        let (tc1, tc2) = if comp.tc_ppm != 0.0 {
            (comp.tc_ppm * 1e-6, 0.0)
        } else {
            circuit.tempcos.iter().find(|(n, _, _)| elements.contains(n)).map_or((0.0, 0.0), |(_, a, b)| (*a, *b))
        };
        out.push(Heater { name: r.to_string(), elements, theta, tc1, tc2 });
    }
    out
}

pub(crate) fn scale(h: &Heater, temp_c: f64) -> f64 {
    let d = temp_c - T_NOM_C;
    (1.0 + h.tc1 * d + h.tc2 * d * d).max(0.01)
}

/// DC operating point with the `heaters` warmed by their own power, the surroundings at `t_amb` °C.
pub fn self_heat(circuit: &Circuit, heaters: &[Heater], t_amb: f64) -> Result<SelfHeat, SimError> {
    // the heated resistors keep their own coefficients; everything else is taken to the surrounding temperature
    let mut base = circuit.clone();
    base.tempcos.retain(|(n, _, _)| !heaters.iter().any(|h| h.elements.contains(n)));
    let base = base.at_temperature(t_amb);
    let nominal = |name: &str| -> f64 {
        circuit.elements.iter().find_map(|e| match e {
            Element::Resistor { name: n, r, .. } if n == name => Some(*r),
            _ => None,
        }).unwrap_or(0.0)
    };
    let build = |temps: &[f64]| -> Circuit {
        let mut c = base.clone();
        for (h, &t) in heaters.iter().zip(temps) {
            for e in &mut c.elements {
                if let Element::Resistor { name, r, .. } = e {
                    if h.elements.contains(name) {
                        *r = nominal(name) * scale(h, t);
                    }
                }
            }
        }
        c
    };
    let mut temps = vec![t_amb; heaters.len()];
    let cold = dc_op(&build(&temps))?;
    let mut warm = cold.clone();
    let mut powers = vec![0.0; heaters.len()];
    let mut rounds = 0;
    for round in 1..=60 {
        rounds = round;
        let mut moved: f64 = 0.0;
        for (k, h) in heaters.iter().enumerate() {
            powers[k] = h.elements.iter().filter_map(|n| warm.element(n)).map(|e| e.power.max(0.0)).sum();
            let target = t_amb + h.theta * powers[k];
            if target > 1000.0 {
                return Err(SimError::Invalid(format!("{} の温度が 1000 ℃ を超えます (電力 {:.1} W × 熱抵抗 {} K/W)。電流を減らすか、熱抵抗の小さい部品にしてください", h.name, powers[k], h.theta)));
            }
            // half a step: a resistor whose value rises with heat would otherwise swing between two temperatures
            let next = temps[k] + 0.6 * (target - temps[k]);
            moved = moved.max((next - temps[k]).abs());
            temps[k] = next;
        }
        warm = dc_op(&build(&temps))?;
        if moved < 1e-6 {
            break;
        }
    }
    let parts = heaters
        .iter()
        .enumerate()
        .map(|(k, h)| {
            let power: f64 = h.elements.iter().filter_map(|n| warm.element(n)).map(|e| e.power.max(0.0)).sum();
            HeatedPart { name: h.name.clone(), power, rise: temps[k] - t_amb, temp_c: temps[k], change: scale(h, temps[k]) / scale(h, t_amb) - 1.0, theta: h.theta, tc1: h.tc1, tc2: h.tc2 }
        })
        .collect();
    Ok(SelfHeat { parts, warm, cold, rounds })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 10 V across 100 Ω (the heated one) in series with 100 Ω: the first dissipates 0.25 W cold.
    fn circuit() -> Circuit {
        let mut c = Circuit::new();
        let (a, o) = (c.node("a"), c.node("o"));
        c.add(Element::vdc("V1", a, 0, 10.0));
        c.add(Element::Resistor { name: "R1".into(), a, b: o, r: 100.0 });
        c.add(Element::Resistor { name: "R2".into(), a: o, b: 0, r: 100.0 });
        c
    }

    fn heater(theta: f64, tc1: f64) -> Heater {
        Heater { name: "R1".into(), elements: vec!["R1".into()], theta, tc1, tc2: 0.0 }
    }

    #[test]
    fn a_part_warms_by_power_times_theta_and_its_value_follows() {
        let r = self_heat(&circuit(), &[heater(100.0, 0.0)], 25.0).unwrap();
        let p = &r.parts[0];
        // no coefficient: P = 0.25 W, rise 25 K, no change
        assert!((p.power - 0.25).abs() < 1e-9 && (p.rise - 25.0).abs() < 1e-4 && p.change.abs() < 1e-12, "{p:?}");
        let r = self_heat(&circuit(), &[heater(100.0, 1e-3)], 25.0).unwrap();
        let p = &r.parts[0];
        // with +1000 ppm/K the value rises, the power falls a little, and the temperature settles where rise = θ·P(R(rise))
        assert!(p.rise < 25.0 && p.rise > 20.0 && p.change > 0.02, "{p:?}");
        let rhot = 100.0 * (1.0 + 1e-3 * (p.temp_c - 27.0));
        assert!((p.power - 100.0 * rhot / (rhot + 100.0).powi(2)).abs() < 1e-6, "{p:?}");
        assert!((p.rise - 100.0 * p.power).abs() < 1e-3, "{p:?}");
        // the cold answer is the one without the warming (R1 at 25 °C: 2 K under the 27 °C the coefficient is counted from)
        assert!((r.cold.voltage("o").unwrap() - 1000.0 / 199.8).abs() < 1e-9);
        assert!(r.warm.voltage("o").unwrap() < 5.0);
    }

    #[test]
    fn a_part_that_would_run_away_is_reported() {
        let r = self_heat(&circuit(), &[heater(1e5, 0.0)], 25.0);
        assert!(matches!(r, Err(SimError::Invalid(m)) if m.contains("1000")));
    }
}
