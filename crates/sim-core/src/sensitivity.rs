//! Sensitivity: which parts set a result, and how strongly. Every part with a value (resistors, capacitors, coils, DC sources)
//! is moved a little up and a little down on its own, and the change of the result is read off (a central difference).
//!
//! The figure that compares parts of different kinds is the elasticity: the relative change of the result for a relative change
//! of the part. 1 means "1 % more of the part gives 1 % more of the result", -0.5 means half a percent less, 0 means the part does
//! not matter. Unlike the worst case it needs no tolerances: it says which parts need precise values before any are chosen.

use crate::circuit::{Circuit, Element};
use crate::dc::SimError;
use crate::montecarlo::{measure, McMeasure};
use crate::waveform::Waveform;

#[derive(Clone, Debug)]
pub struct SensSettings {
    /// Node that is measured.
    pub output: String,
    pub measure: McMeasure,
    /// How far each part is moved, as a fraction (0.01 = ±1 %).
    pub step: f64,
}

/// What one part does to the result.
#[derive(Clone, Debug)]
pub struct SensPart {
    pub name: String,
    /// The part's value (ohms, farads, henries, volts or amperes).
    pub value: f64,
    /// Change of the result when the part grows by 1 % (in the result's unit).
    pub per_percent: f64,
    /// Relative change of the result per relative change of the part (None when the result is 0).
    pub elasticity: Option<f64>,
    /// The result does not move in a straight line with the part: the change up and the change down differ by more than a quarter.
    pub bent: bool,
}

#[derive(Clone, Debug)]
pub struct Sensitivity {
    pub output: String,
    pub unit: &'static str,
    pub nominal: f64,
    pub step: f64,
    /// The parts, the one that moves the result most first.
    pub parts: Vec<SensPart>,
    /// Parts whose moved circuits did not solve.
    pub unsolved: Vec<String>,
}

/// The value of a part that can be moved, or None (a part inside a model, a source that is not a plain DC value, a part that does
/// nothing to a DC result).
fn value_of(e: &Element, dc_only: bool) -> Option<f64> {
    if e.name().contains('.') {
        return None;
    }
    let v = match e {
        Element::Resistor { r, .. } => *r,
        Element::Capacitor { c, .. } if !dc_only => *c,
        Element::Inductor { l, .. } if !dc_only => *l,
        Element::VSource { wave: Waveform::Dc(v), .. } | Element::ISource { wave: Waveform::Dc(v), .. } => *v,
        _ => return None,
    };
    (v != 0.0 && v.is_finite()).then_some(v)
}

fn set_scaled(e: &mut Element, f: f64) {
    match e {
        Element::Resistor { r: x, .. } | Element::Capacitor { c: x, .. } | Element::Inductor { l: x, .. } => *x *= f,
        Element::VSource { wave: Waveform::Dc(x), .. } | Element::ISource { wave: Waveform::Dc(x), .. } => *x *= f,
        _ => {}
    }
}

pub fn sensitivity(circuit: &Circuit, s: &SensSettings) -> Result<Sensitivity, SimError> {
    if !(s.step > 0.0 && s.step <= 0.5) {
        return Err(SimError::Invalid("動かす量は 0 より大きく 50 % 以下にしてください".into()));
    }
    let output = s.output.trim().to_string();
    let nominal = measure(circuit, &output, &s.measure)?;
    // at DC a capacitor is open and a coil a short whatever their values: leave them out instead of listing zeros
    let dc_only = matches!(s.measure, McMeasure::DcVoltage);
    let parts: Vec<(usize, f64)> = circuit.elements.iter().enumerate().filter_map(|(i, e)| value_of(e, dc_only).map(|v| (i, v))).collect();
    if parts.is_empty() {
        return Err(SimError::Invalid("値を動かせる部品 (抵抗・コンデンサ・コイル・直流電源) がありません".into()));
    }
    let jobs: Vec<(usize, bool)> = (0..parts.len()).flat_map(|k| [(k, false), (k, true)]).collect();
    let moved = crate::par_map(&jobs, |&(k, up)| {
        let mut c = circuit.clone();
        set_scaled(&mut c.elements[parts[k].0], if up { 1.0 + s.step } else { 1.0 - s.step });
        measure(&c, &output, &s.measure).ok().filter(|v| v.is_finite())
    });
    let mut out = vec![];
    let mut unsolved = vec![];
    for (k, &(i, value)) in parts.iter().enumerate() {
        let name = circuit.elements[i].name().to_string();
        let (Some(lo), Some(hi)) = (moved[2 * k], moved[2 * k + 1]) else {
            unsolved.push(name);
            continue;
        };
        let slope = (hi - lo) / (2.0 * s.step); // change of the result per unit of relative change
        let (up, down) = (hi - nominal, nominal - lo);
        let size = up.abs().max(down.abs());
        let bent = size > 1e-12 * nominal.abs().max(1e-12) && (up - down).abs() > 0.25 * size;
        out.push(SensPart { name, value, per_percent: slope / 100.0, elasticity: (nominal.abs() > 1e-30).then(|| slope / nominal), bent });
    }
    out.sort_by(|a, b| b.per_percent.abs().total_cmp(&a.per_percent.abs()));
    Ok(Sensitivity { output, unit: s.measure.unit(), nominal, step: s.step, parts: out, unsolved })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn divider(r1: f64, r2: f64) -> Circuit {
        let mut c = Circuit::new();
        let (vin, mid) = (c.node("VIN"), c.node("MID"));
        c.elements.push(Element::vdc("V1", vin, 0, 5.0));
        c.elements.push(Element::Resistor { name: "R1".into(), a: vin, b: mid, r: r1 });
        c.elements.push(Element::Resistor { name: "R2".into(), a: mid, b: 0, r: r2 });
        c.elements.push(Element::Capacitor { name: "C1".into(), a: mid, b: 0, c: 100e-9 });
        c
    }

    fn dc() -> SensSettings {
        SensSettings { output: "MID".into(), measure: McMeasure::DcVoltage, step: 0.01 }
    }

    #[test]
    fn a_divider_follows_its_supply_one_to_one_and_its_resistors_by_their_share() {
        // MID = 5 * R2 / (R1 + R2): elasticity to V1 is 1, to R2 is R1/(R1+R2), to R1 is -R1/(R1+R2)
        let s = sensitivity(&divider(3e3, 1e3), &dc()).unwrap();
        assert!((s.nominal - 1.25).abs() < 1e-9);
        let e = |n: &str| s.parts.iter().find(|p| p.name == n).unwrap().elasticity.unwrap();
        assert!((e("V1") - 1.0).abs() < 1e-6, "{}", e("V1"));
        assert!((e("R2") - 0.75).abs() < 1e-3, "{}", e("R2"));
        assert!((e("R1") + 0.75).abs() < 1e-3, "{}", e("R1"));
        // per percent: 1 % more V1 is 12.5 mV more
        let v1 = s.parts.iter().find(|p| p.name == "V1").unwrap();
        assert!((v1.per_percent - 0.0125).abs() < 1e-6);
        assert_eq!(s.parts[0].name, "V1", "the supply moves the result most");
        // the capacitor does nothing at DC and is not listed
        assert!(s.parts.iter().all(|p| p.name != "C1"));
        assert!(s.parts.iter().all(|p| !p.bent));
    }

    #[test]
    fn a_cutoff_falls_as_the_resistor_and_the_capacitor_grow() {
        // f = 1 / (2π R C) with R = R1 || R2: elasticity -1 to C, and -(share) to each resistor
        let mut c = divider(1e3, 1e3);
        if let Some(Element::VSource { ac, .. }) = c.elements.get_mut(0) {
            *ac = Some((1.0, 0.0));
        }
        let s = sensitivity(&c, &SensSettings { output: "MID".into(), measure: McMeasure::Cutoff { fstart: 10.0, fstop: 1e6, points_per_decade: 40 }, step: 0.01 }).unwrap();
        assert_eq!(s.unit, "Hz");
        let e = |n: &str| s.parts.iter().find(|p| p.name == n).unwrap().elasticity.unwrap();
        assert!((e("C1") + 1.0).abs() < 0.02, "{}", e("C1"));
        assert!((e("R1") + 0.5).abs() < 0.02, "{}", e("R1"));
        // the source's size does not move a cutoff
        assert!(e("V1").abs() < 1e-6);
    }

    #[test]
    fn a_step_too_large_or_no_part_is_refused() {
        assert!(sensitivity(&divider(1e3, 1e3), &SensSettings { step: 0.9, ..dc() }).is_err());
        let mut c = Circuit::new();
        let a = c.node("A");
        c.elements.push(Element::Capacitor { name: "C1".into(), a, b: 0, c: 1e-6 });
        assert!(sensitivity(&c, &SensSettings { output: "A".into(), ..dc() }).is_err());
    }
}
