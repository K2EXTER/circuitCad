//! Self-heating of resistors, diodes and transistors. A part that carries current warms up by `P · θ` above its surroundings (θ in
//! K/W is the heat resistance from the part to the air, board copper included). A resistor with a temperature coefficient changes
//! its value as it warms, and a diode, LED, bipolar or MOS transistor its model (the rules of `temperature.rs`: a silicon junction
//! drops about 2 mV less per °C), which changes the current and the power: the answer is found by iterating until the
//! temperatures stop moving. This is a steady state (DC); the board's own heat spreading is in θ, not simulated.

use crate::circuit::{Circuit, Element};
use crate::dc::{dc_op, dc_op_from, DcResult, SimError};
use crate::temperature::T_NOM_C;
use sch_core::{Component, Kind, Schematic};
use std::collections::BTreeMap;

/// A part that warms itself: the resistors, diodes and transistors inside it (all share one temperature), its heat resistance and
/// the temperature coefficients of its resistors.
#[derive(Clone, Debug, PartialEq)]
pub struct Heater {
    /// The part (reference designator).
    pub name: String,
    /// Names of its resistor, diode and transistor elements in the circuit (a library part may hold several).
    pub elements: Vec<String>,
    /// Heat resistance from the element to the surroundings (K/W).
    pub theta: f64,
    /// `R(T) = R·(1 + tc1·ΔT + tc2·ΔT²)` with ΔT from 27 °C.
    pub tc1: f64,
    pub tc2: f64,
    /// Its heat is the power that comes in through its pins (every element inside it, controlled sources included: a library part's
    /// macro model, whose resistors and diodes are only clamps and sensing). False: the power of its resistors, diodes and transistors
    /// (a plain part, and a cell, whose source stores the energy instead of turning it into heat).
    pub through_pins: bool,
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
    /// Of `rise`, what the other parts' heat brought through the board (K; 0 without a board's matrix, `self_heat_coupled`).
    pub from_others: f64,
    /// How many diodes and transistors the part holds (their models follow its temperature).
    pub junctions: usize,
    /// For a part with one diode or transistor: the voltage across it (diode a−k, transistor C−E or D−S) cold and warm (V).
    pub junction_v: Option<(f64, f64)>,
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

/// The parts of a schematic that can heat themselves: every resistor, diode, LED and transistor, and library parts that hold
/// resistors, diodes or transistors. θ comes from the part (`theta`, or a guess from the package); the temperature coefficient of the
/// resistors from the part (`tc_ppm`) or from the model (`TC=`).
///
/// `copper` holds, by reference, the conductance (W/K) that the board's copper joined to the part adds beside its own θ
/// (`pcb_core::heat_copper`, carried over with the board's wiring); empty when there is no board.
pub fn heaters_from_schematic(sch: &Schematic, circuit: &Circuit, copper: &BTreeMap<String, f64>) -> Vec<Heater> {
    let mut out = vec![];
    for comp in &sch.components {
        let resistors = matches!(comp.kind, Kind::Resistor | Kind::Custom);
        if !resistors && !matches!(comp.kind, Kind::Diode | Kind::Led | Kind::Npn) {
            continue;
        }
        let r = comp.refdes.as_str();
        let prefix = format!("{r}.");
        let elements: Vec<String> = circuit
            .elements
            .iter()
            .filter(|e| e.name() == r || (e.name().starts_with(&prefix) && !is_board_wiring(e.name())))
            .filter(|e| match e {
                Element::Resistor { .. } => resistors,
                _ => is_junction(e),
            })
            .map(|e| e.name().to_string())
            .collect();
        if elements.is_empty() {
            continue;
        }
        let theta = part_theta(comp, copper);
        let (tc1, tc2) = if comp.tc_ppm != 0.0 {
            (comp.tc_ppm * 1e-6, 0.0)
        } else {
            circuit.tempcos.iter().find(|(n, _, _)| elements.contains(n)).map_or((0.0, 0.0), |(_, a, b)| (*a, *b))
        };
        out.push(Heater { name: r.to_string(), elements, theta, tc1, tc2, through_pins: through_pins(comp) });
    }
    out
}

/// Whether a part's heat is the power through its pins (`Heater::through_pins`): a library part that is not a cell.
pub fn through_pins(comp: &Component) -> bool {
    comp.kind == Kind::Custom && comp.custom.as_ref().is_some_and(|c| c.glyph != "battery" && !(c.glyph.is_empty() && c.inferred_glyph() == "battery"))
}

/// A part's own θ (K/W, on the smallest pads): what it was given, else its library part's θJA from the data sheet, else the guess
/// from its package.
pub fn own_theta(comp: &Component) -> f64 {
    if comp.theta > 0.0 {
        return comp.theta;
    }
    let ja = comp.custom.as_ref().and_then(|c| c.ratings.as_ref()).and_then(|r| r.theta_ja).filter(|x| *x > 0.0);
    ja.or_else(|| default_theta(&comp.footprint_name())).unwrap_or(100.0)
}

/// The share of θ taken for θjc when neither the part nor its library part gives one: NDT3055L's 12 of 110 K/W (SOT-223, minimum
/// pad), the data sheet the board copper's law is fitted to, so that a part without θjc meets the copper as that part did.
pub const JC_GUESS_SHARE: f64 = 12.0 / 110.0;

/// A part's θjc (K/W, junction or element to case) and whether it is a guess: what it was given, its library part's, or a share of its
/// own θ (`JC_GUESS_SHARE`). Never above its own θ.
pub fn theta_jc(comp: &Component) -> (f64, bool) {
    let own = own_theta(comp);
    let given = if comp.theta_jc > 0.0 { Some(comp.theta_jc) } else { comp.custom.as_ref().and_then(|c| c.ratings.as_ref()).and_then(|r| r.theta_jc).filter(|x| *x > 0.0) };
    match given {
        Some(jc) => (jc.min(own), false),
        None => (own * JC_GUESS_SHARE, true),
    }
}

/// θ (K/W) of a part split at its case: `jc` from the junction to the case, then the case's own way to the air (`own − jc`) in parallel
/// with the board copper's conductance `g` (W/K) and a heat sink of `sink` K/W (0 = none). The copper and the sink only shorten the
/// path beyond the case: a part with a large θjc gains little from them.
pub fn split_theta(own: f64, jc: f64, g: f64, sink: f64) -> f64 {
    let jc = jc.clamp(0.0, own);
    let ca = own - jc;
    if !(ca > 1e-12) {
        return own;
    }
    let mut gc = 1.0 / ca + g.max(0.0);
    if sink > 0.0 {
        gc += 1.0 / sink;
    }
    jc + 1.0 / gc
}

/// A part's θ (K/W) with the board's copper and its heat sink: θjc, then its own case-to-air θ in parallel with the copper's conductance
/// from `copper` (W/K) and the heat sink (`Component::heatsink`).
pub fn part_theta(comp: &Component, copper: &BTreeMap<String, f64>) -> f64 {
    let own = own_theta(comp);
    let g = copper.get(&comp.refdes).copied().unwrap_or(0.0);
    if !(g > 0.0) && !(comp.heatsink > 0.0) {
        return own;
    }
    split_theta(own, theta_jc(comp).0, g, comp.heatsink)
}

/// A part to put on the board's temperature map: its power at the cold operating point and what its heat goes through.
#[derive(Clone, Debug, PartialEq)]
pub struct BoardHeater {
    pub name: String,
    pub power: f64,
    /// Its own θ on the minimum pad, θjc and its heat sink (K/W; 0 = none).
    pub own_theta: f64,
    pub theta_jc: f64,
    pub heatsink: f64,
}

/// Most parts given to the board's matrix (one grid solve each).
pub const BOARD_HEATERS_MAX: usize = 24;

/// The parts whose heat is worth the board's matrix: the heaters with at least 1 mW and 1 % of the hottest one's power at the cold
/// operating point, the most first, at most `BOARD_HEATERS_MAX`; `on_board` says whether a part has pads on the board. The others keep
/// their θ (the copper formula).
pub fn board_heaters(sch: &Schematic, circuit: &Circuit, heaters: &[Heater], on_board: impl Fn(&str) -> bool) -> Result<Vec<BoardHeater>, SimError> {
    let r = dc_op(circuit)?;
    let mut out: Vec<BoardHeater> = heaters
        .iter()
        .filter(|h| on_board(&h.name))
        .filter_map(|h| {
            let c = sch.components.iter().find(|c| c.refdes == h.name)?;
            Some(BoardHeater { name: h.name.clone(), power: part_power(circuit, h, &r), own_theta: own_theta(c), theta_jc: theta_jc(c).0, heatsink: c.heatsink })
        })
        .collect();
    let top = out.iter().map(|b| b.power).fold(0.0, f64::max);
    out.retain(|b| b.power >= 1e-3 && b.power >= 0.01 * top);
    out.sort_by(|a, b| b.power.total_cmp(&a.power));
    out.truncate(BOARD_HEATERS_MAX);
    Ok(out)
}

/// The board's matrix (`names`, `r[i][j]` K of part i per W in part j) on the heaters: each named part's θ becomes its diagonal, and
/// the rest comes back as the couplings for `self_heat_coupled`.
pub fn heaters_with_matrix(heaters: &mut [Heater], names: &[String], r: &[Vec<f64>]) -> Vec<(String, String, f64)> {
    for h in heaters.iter_mut() {
        if let Some(k) = names.iter().position(|n| *n == h.name) {
            h.theta = r[k][k];
        }
    }
    let mut mutual = vec![];
    for (i, a) in names.iter().enumerate() {
        for (j, b) in names.iter().enumerate() {
            if i != j {
                mutual.push((a.clone(), b.clone(), r[i][j]));
            }
        }
    }
    mutual
}

/// What each part gets from the others' power through `links` (a, b, K/W).
fn from_others(links: &[(usize, usize, f64)], powers: &[f64], n: usize) -> Vec<f64> {
    let mut out = vec![0.0; n];
    for &(a, b, r) in links {
        out[a] += r * powers[b];
    }
    out
}

/// Whether an element is the board's wiring in front of a pin (`REF.PIN.RW` and its inductor, damping and capacitance, added by
/// `from_schematic_with_layout`): it carries the pin's current but its heat is the board's copper, not the part's (the board's map
/// takes it as a heat source of its own).
pub fn is_board_wiring(name: &str) -> bool {
    [".RW", ".LW", ".RD", ".CW"].iter().any(|s| name.ends_with(s))
}

fn is_junction(e: &Element) -> bool {
    matches!(e, Element::Diode { .. } | Element::Bjt { .. } | Element::Mosfet { .. })
}

/// The heat a part gives off (W) at an operating point: what every element inside it absorbs, its controlled sources included. For a
/// library part's macro model that is the power that comes in through its pins (Tellegen: the sum over the elements of a subcircuit
/// is the sum of pin voltage × current into the pin), e.g. a regulator's (in − out) × current, which its resistors and diodes alone
/// miss: those are often only clamps and sensing, whose own large currents are balanced by the sources that drive them. An independent
/// source that gives power (a cell's voltage) is a store of energy, not heat, and is left out; one that takes power (a model's
/// quiescent current between its supply pins) is heat. Never below zero (a part that gives energy back is not cooling).
pub fn part_power(circuit: &Circuit, h: &Heater, r: &DcResult) -> f64 {
    if !h.through_pins {
        return h.elements.iter().filter_map(|n| r.element(n)).map(|e| e.power.max(0.0)).sum();
    }
    let prefix = format!("{}.", h.name);
    let mine = |n: &str| n == h.name || (n.starts_with(&prefix) && !is_board_wiring(n)) || h.elements.iter().any(|e| e == n);
    let store = |n: &str| circuit.elements.iter().any(|e| matches!(e, Element::VSource { name, .. } | Element::ISource { name, .. } if name == n));
    r.elements.iter().filter(|e| mine(&e.name)).map(|e| if e.power < 0.0 && store(&e.name) { 0.0 } else { e.power }).sum::<f64>().max(0.0)
}

pub(crate) fn scale(h: &Heater, temp_c: f64) -> f64 {
    let d = temp_c - T_NOM_C;
    (1.0 + h.tc1 * d + h.tc2 * d * d).max(0.01)
}

/// DC operating point with the `heaters` warmed by their own power, the surroundings at `t_amb` °C.
pub fn self_heat(circuit: &Circuit, heaters: &[Heater], t_amb: f64) -> Result<SelfHeat, SimError> {
    self_heat_coupled(circuit, heaters, t_amb, &[])
}

/// `self_heat` with the parts warming each other: `mutual` holds `(a, b, K/W)`, the rise of part a for 1 W in part b (the board's
/// thermal matrix, `pcb_core::thermal::thermal_matrix`, off its diagonal; each heater's `theta` is the diagonal). A part's target is
/// `t_amb + θ·P + Σ R·P_other`.
pub fn self_heat_coupled(circuit: &Circuit, heaters: &[Heater], t_amb: f64, mutual: &[(String, String, f64)]) -> Result<SelfHeat, SimError> {
    let index = |n: &str| heaters.iter().position(|h| h.name == n);
    let links: Vec<(usize, usize, f64)> = mutual.iter().filter_map(|(a, b, r)| Some((index(a)?, index(b)?, *r))).filter(|(a, b, r)| a != b && *r > 0.0).collect();
    // the heated resistors keep their own coefficients; everything else is taken to the surrounding temperature
    let mut base = circuit.clone();
    base.tempcos.retain(|(n, _, _)| !heaters.iter().any(|h| h.elements.contains(n)));
    let nominal = |name: &str| -> f64 {
        circuit.elements.iter().find_map(|e| match e {
            Element::Resistor { name: n, r, .. } if n == name => Some(*r),
            _ => None,
        }).unwrap_or(0.0)
    };
    // the diodes and transistors of each part: their models follow the part's temperature
    let junctions: Vec<Vec<String>> = heaters.iter().map(|h| circuit.elements.iter().filter(|e| is_junction(e) && h.elements.iter().any(|n| n == e.name())).map(|e| e.name().to_string()).collect()).collect();
    let build = |temps: &[f64]| -> Circuit {
        let own: Vec<(String, f64)> = junctions.iter().zip(temps).flat_map(|(names, &t)| names.iter().map(move |n| (n.clone(), t))).collect();
        let mut c = base.at_temperatures(t_amb, &own);
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
    // each part's temperature and target of the previous round
    let mut last: Vec<Option<(f64, f64)>> = vec![None; heaters.len()];
    let mut rounds = 0;
    for round in 1..=60 {
        rounds = round;
        let mut moved: f64 = 0.0;
        for (k, h) in heaters.iter().enumerate() {
            powers[k] = part_power(circuit, h, &warm);
        }
        let others = from_others(&links, &powers, heaters.len());
        for (k, h) in heaters.iter().enumerate() {
            let target = t_amb + h.theta * powers[k] + others[k];
            if target > 1000.0 {
                return Err(SimError::Invalid(format!("{} の温度が 1000 ℃ を超えます (電力 {:.1} W × 熱抵抗 {} K/W)。電流を減らすか、熱抵抗の小さい部品にしてください", h.name, powers[k], h.theta)));
            }
            // The step towards the target: the secant of the last two rounds gives how the target follows the part's own temperature
            // (slope s), and the step 1/(1 − s) lands where the two meet. A resistor whose value rises with heat has s < 0 (the plain
            // step would swing between two temperatures), a transistor biased by a fixed voltage s > 0 (the plain step crawls). Without
            // a slope yet, or when it says the part runs away (s near 1 or above), the old fixed fraction of the step.
            let step = match last[k] {
                Some((t0, g0)) if (temps[k] - t0).abs() > 1e-12 => {
                    let s = (target - g0) / (temps[k] - t0);
                    if s < 0.9 { (1.0 / (1.0 - s)).clamp(0.2, 10.0) } else { 0.6 }
                }
                _ => 0.6,
            };
            last[k] = Some((temps[k], target));
            let next = temps[k] + step * (target - temps[k]);
            moved = moved.max((next - temps[k]).abs());
            temps[k] = next;
        }
        // the values moved only a little since the last round: start from its answer
        warm = dc_op_from(&build(&temps), &warm)?;
        if moved < 1e-6 {
            break;
        }
    }
    let final_powers: Vec<f64> = heaters.iter().map(|h| part_power(circuit, h, &warm)).collect();
    let others = from_others(&links, &final_powers, heaters.len());
    let parts = heaters
        .iter()
        .enumerate()
        .map(|(k, h)| {
            let power = final_powers[k];
            let junction_v = match junctions[k].as_slice() {
                [one] => cold.element(one).zip(warm.element(one)).map(|(a, b)| (a.voltage, b.voltage)),
                _ => None,
            };
            HeatedPart { name: h.name.clone(), power, rise: temps[k] - t_amb, temp_c: temps[k], change: scale(h, temps[k]) / scale(h, t_amb) - 1.0, theta: h.theta, tc1: h.tc1, tc2: h.tc2, from_others: others[k], junctions: junctions[k].len(), junction_v }
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
        Heater { name: "R1".into(), elements: vec!["R1".into()], theta, tc1, tc2: 0.0, through_pins: false }
    }

    /// The board's wiring in front of a pin is not the part's: R1's heater holds R1 alone, not `R1.1.RW`, and its power is R1's own
    /// I²R even when the wiring carries the same current.
    #[test]
    fn the_boards_wiring_is_not_part_of_the_parts_heat() {
        let mut s = Schematic::default();
        let v = s.add(Kind::VSource, (0, 0), 0);
        s.components[v].value = "10".into();
        let r = s.add(Kind::Resistor, (20, 0), 0);
        s.components[r].value = "100".into();
        let (vp, vm) = (s.components[v].pins()[0].1, s.components[v].pins()[1].1);
        let (r1, r2) = (s.components[r].pins()[0].1, s.components[r].pins()[1].1);
        s.wires.push(sch_core::Wire { a: vp, b: r1 });
        s.wires.push(sch_core::Wire { a: r2, b: (r2.0, vm.1) });
        s.wires.push(sch_core::Wire { a: (r2.0, vm.1), b: vm });
        s.labels.push(sch_core::Label { pos: vm, text: "GND".into() });
        let wiring = [crate::WiringR { refdes: "R1".into(), pin: "1".into(), ohm: 1.0, henry: 0.0, farad: 0.0 }];
        let c = crate::from_schematic_with_wiring(&s, &wiring).unwrap();
        assert!(c.elements.iter().any(|e| e.name() == "R1.1.RW"));
        let h = heaters_from_schematic(&s, &c, &BTreeMap::new());
        let r1 = h.iter().find(|h| h.name == "R1").unwrap();
        assert_eq!(r1.elements, vec!["R1".to_string()]);
        let dc = dc_op(&c).unwrap();
        let i: f64 = 10.0 / 101.0;
        assert!((part_power(&c, r1, &dc) - i * i * 100.0).abs() < 1e-9, "R1's own heat, not its wiring's");
    }

    /// Two parts on one board (the board's matrix): R2 has no temperature coefficient, so its power stays 0.25 W, and R1 also gets 20 K/W
    /// of it on top of its own 100 K/W × 0.25 W; the coupling shows as `from_others`.
    #[test]
    fn a_neighbour_on_the_board_adds_its_heat() {
        let c = circuit();
        let both = [heater(100.0, 0.0), Heater { name: "R2".into(), elements: vec!["R2".into()], theta: 50.0, tc1: 0.0, tc2: 0.0, through_pins: false }];
        let alone = self_heat(&c, &both, 25.0).unwrap();
        let joined = self_heat_coupled(&c, &both, 25.0, &[("R1".into(), "R2".into(), 20.0), ("R2".into(), "R1".into(), 20.0)]).unwrap();
        let (a, b) = (&joined.parts[0], &joined.parts[1]);
        assert!((alone.parts[0].rise - 25.0).abs() < 1e-6 && alone.parts[0].from_others == 0.0);
        assert!((a.rise - (25.0 + 20.0 * 0.25)).abs() < 1e-6 && (a.from_others - 5.0).abs() < 1e-6, "{a:?}");
        assert!((b.rise - (12.5 + 5.0)).abs() < 1e-6, "{b:?}");
        // a link to a part that is not heated is ignored
        assert!(self_heat_coupled(&c, &both, 25.0, &[("R1".into(), "X9".into(), 20.0)]).is_ok());
    }

    /// θ split at the case (H5): the copper and a heat sink only shorten the way from the case to the air, so θjc stays whatever they do.
    #[test]
    fn copper_and_a_heat_sink_act_beyond_the_case_only() {
        let mut s = Schematic::default();
        let i = s.add(Kind::Resistor, (0, 0), 0);
        let c = &mut s.components[i];
        c.theta = 50.0;
        let none = BTreeMap::new();
        assert_eq!(part_theta(c, &none), 50.0, "nothing beside it: its own θ");
        // no θjc given: a share of θ (NDT3055L's 12 of 110), marked as a guess
        let (jc, guess) = theta_jc(c);
        assert!(guess && (jc - 50.0 * 12.0 / 110.0).abs() < 1e-12);
        // θjc 5 and a 10 K/W heat sink: 5 + 1/(1/45 + 1/10) = 13.18 K/W
        c.theta_jc = 5.0;
        c.heatsink = 10.0;
        assert!((part_theta(c, &none) - (5.0 + 1.0 / (1.0 / 45.0 + 0.1))).abs() < 1e-9);
        // however big the sink and the copper, θ never falls below θjc
        c.heatsink = 1e-6;
        let copper = BTreeMap::from([(c.refdes.clone(), 100.0)]);
        let t = part_theta(c, &copper);
        assert!(t > 5.0 && t < 5.001, "{t}");
        // a θjc above θ is taken as θ: nothing helps
        assert_eq!(split_theta(50.0, 80.0, 1.0, 1.0), 50.0);
        // 0 puts it back to the guess
        c.theta_jc = 0.0;
        c.heatsink = 0.0;
        assert!(theta_jc(c).1);
    }

    #[test]
    fn a_library_parts_theta_ja_is_its_own_theta_when_none_is_given() {
        let mut s = Schematic::default();
        let i = s.add(Kind::Custom, (0, 0), 0);
        let c = &mut s.components[i];
        let guess = own_theta(c);
        let ratings = sch_core::PartRatings { theta_ja: Some(200.0), ..Default::default() };
        c.custom = Some(sch_core::CustomSymbol { name: "Q".into(), prefix: "Q".into(), pins: vec![], sim: None, ratings: Some(ratings), glyph: String::new(), shops: Default::default(), programming: vec![], companions: vec![] });
        assert_eq!(own_theta(c), 200.0, "the data sheet's θJA, not the package guess {guess}");
        // the part's own θ goes first
        c.theta = 150.0;
        assert_eq!(own_theta(c), 150.0);
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
        // the secant step lands in a few rounds (the fixed 0.6 step took about twenty)
        assert!(r.rounds <= 6, "{}", r.rounds);
        // the cold answer is the one without the warming (R1 at 25 °C: 2 K under the 27 °C the coefficient is counted from)
        assert!((r.cold.voltage("o").unwrap() - 1000.0 / 199.8).abs() < 1e-9);
        assert!(r.warm.voltage("o").unwrap() < 5.0);
    }

    #[test]
    fn a_diode_that_warms_itself_drops_less_voltage_in_the_steady_state() {
        // 5 V into 100 ohm and a diode: about 30 mW in the diode, with 500 K/W it warms by about 15 K and gives up about 2 mV per K
        let mut c = Circuit::new();
        let (a, o) = (c.node("a"), c.node("o"));
        c.add(Element::vdc("V1", a, 0, 5.0));
        c.add(Element::Resistor { name: "R1".into(), a, b: o, r: 100.0 });
        c.add(Element::Diode { name: "D1".into(), a: o, k: 0, model: crate::circuit::DiodeModel::default() });
        let d1 = Heater { name: "D1".into(), elements: vec!["D1".into()], theta: 500.0, tc1: 0.0, tc2: 0.0, through_pins: false };
        let r = self_heat(&c, &[d1], 27.0).unwrap();
        let p = &r.parts[0];
        assert!(p.rise > 10.0 && (p.rise - 500.0 * p.power).abs() < 1e-3, "{p:?}");
        let (cold, warm) = p.junction_v.unwrap();
        assert_eq!(p.junctions, 1);
        assert!((cold - r.cold.voltage("o").unwrap()).abs() < 1e-9 && (warm - r.warm.voltage("o").unwrap()).abs() < 1e-9);
        let per_k = (cold - warm) / p.rise;
        assert!(per_k > 1.2e-3 && per_k < 2.6e-3, "{per_k} V/K");
    }

    #[test]
    fn a_solve_from_the_last_answer_gives_the_same_point_in_fewer_steps() {
        let mut c = Circuit::new();
        let (a, o) = (c.node("a"), c.node("o"));
        c.add(Element::vdc("V1", a, 0, 5.0));
        c.add(Element::Resistor { name: "R1".into(), a, b: o, r: 100.0 });
        c.add(Element::Diode { name: "D1".into(), a: o, k: 0, model: crate::circuit::DiodeModel::default() });
        let first = dc_op(&c).unwrap();
        let warmer = c.at_temperatures(27.0, &[("D1".into(), 40.0)]);
        let (fresh, from) = (dc_op(&warmer).unwrap(), dc_op_from(&warmer, &first).unwrap());
        assert!((fresh.voltage("o").unwrap() - from.voltage("o").unwrap()).abs() < 1e-9);
        assert!(from.iterations < fresh.iterations, "{} vs {}", from.iterations, fresh.iterations);
        // another circuit (other unknowns): the start is not used, and the answer is still right
        let mut other = Circuit::new();
        let n = other.node("n");
        other.add(Element::vdc("V1", n, 0, 1.0));
        assert!((dc_op_from(&other, &first).unwrap().voltage("n").unwrap() - 1.0).abs() < 1e-12);
    }

    /// A regulator-like macro model: the output follows a controlled source (no resistor carries the load current), and a clamp
    /// drives 1 k with thousands of volts when the input is dead. Its heat is the power through its pins, (in − out) × load, not the
    /// clamp's; a cell's heat is its resistor's, its source stores the energy.
    #[test]
    fn a_macro_model_gives_the_heat_through_its_pins_and_a_cell_its_resistor() {
        let model = |vin: f64| {
            let mut c = Circuit::new();
            let (i, o, x) = (c.node("in"), c.node("out"), c.node("U1.x"));
            c.add(Element::vdc("V1", i, 0, vin));
            c.add(Element::Vcvs { name: "U1.E".into(), p: o, n: 0, cp: i, cn: 0, gain: 0.5 });
            // the output current is taken from the input (as the bundled regulators do): V(out) / 10 ohm from `in` to ground
            c.add(Element::Vccs { name: "U1.G".into(), p: i, n: 0, cp: o, cn: 0, gm: 0.1 });
            // the clamp: 1000 x (2.5 V - in) into 1 k when the input is low
            c.add(Element::Vcvs { name: "U1.Ex".into(), p: x, n: 0, cp: o, cn: i, gain: -1000.0 });
            c.add(Element::Resistor { name: "U1.Rx".into(), a: x, b: 0, r: 1e3 });
            c.add(Element::Resistor { name: "RL".into(), a: o, b: 0, r: 10.0 });
            c
        };
        let u1 = Heater { name: "U1".into(), elements: vec!["U1.Rx".into()], theta: 50.0, tc1: 0.0, tc2: 0.0, through_pins: true };
        // 10 V in, 5 V across 10 ohm: 0.5 A through the part, (10 − 5) × 0.5 = 2.5 W; the clamp's kilowatts cancel with its source
        let c = model(10.0);
        let r = dc_op(&c).unwrap();
        let clamp = r.element("U1.Rx").unwrap().power;
        assert!(clamp > 1e3, "{clamp}");
        assert!((part_power(&c, &u1, &r) - 2.5).abs() < 1e-4, "{}", part_power(&c, &u1, &r));
        // the old way (its resistors only) would have taken the clamp's kilowatts
        let old = Heater { through_pins: false, ..u1.clone() };
        assert!(part_power(&c, &old, &r) > 1e3);
        // a cell: 3 V source behind 1 ohm into 2 ohm: 1 A, 1 W in its resistor, the source's 3 W are not heat
        let mut c = Circuit::new();
        let (p, m) = (c.node("p"), c.node("BT1.m"));
        c.add(Element::vdc("BT1.V", m, 0, 3.0));
        c.add(Element::Resistor { name: "BT1.R".into(), a: m, b: p, r: 1.0 });
        c.add(Element::Resistor { name: "RL".into(), a: p, b: 0, r: 2.0 });
        let r = dc_op(&c).unwrap();
        let cell = Heater { name: "BT1".into(), elements: vec!["BT1.R".into()], theta: 50.0, tc1: 0.0, tc2: 0.0, through_pins: true };
        assert!((part_power(&c, &cell, &r) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_part_that_would_run_away_is_reported() {
        let r = self_heat(&circuit(), &[heater(1e5, 0.0)], 25.0);
        assert!(matches!(r, Err(SimError::Invalid(m)) if m.contains("1000")));
    }
}
