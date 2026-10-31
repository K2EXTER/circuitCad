//! Impedance seen from a node: a 1 A AC test current is pushed into the node (against a reference node, ground by default)
//! with every other source switched off, and the voltage it makes is the impedance. Zout of an amplifier or a supply rail,
//! Zin of an input (take the source and its series resistor out with `remove`), and the impedance a decoupling capacitor
//! network shows to a chip. Built on `ac_analysis`, so models, operating point and small-signal parts are the same.

use crate::ac::{ac_analysis, AcResult, AcSettings, C64};
use crate::circuit::{Circuit, Element};
use crate::dc::SimError;
use crate::waveform::Waveform;

#[derive(Clone, Debug)]
pub struct ImpedanceResult {
    pub freq: Vec<f64>,
    pub z: Vec<C64>,
    pub warnings: Vec<String>,
}

impl ImpedanceResult {
    pub fn mag(&self) -> Vec<f64> {
        self.z.iter().map(|z| z.abs()).collect()
    }

    /// 20*log10(|Z|), in dB ohm.
    pub fn db(&self) -> Vec<f64> {
        self.mag().iter().map(|m| 20.0 * m.max(1e-15).log10()).collect()
    }

    pub fn phase_deg(&self) -> Vec<f64> {
        self.z.iter().map(|z| z.arg_deg()).collect()
    }
}

/// What the impedance looks like at one frequency: a resistor (phase near 0), a coil (positive phase) or a capacitor (negative).
pub fn character(phase_deg: f64) -> &'static str {
    if phase_deg > 45.0 {
        "inductive"
    } else if phase_deg < -45.0 {
        "capacitive"
    } else {
        "resistive"
    }
}

/// A peak or a dip of |Z| along the sweep: the frequency, |Z| there and whether it is a peak (a parallel resonance) or a dip (a series one).
#[derive(Clone, Debug, PartialEq)]
pub struct ZExtremum {
    pub freq: f64,
    pub ohm: f64,
    pub peak: bool,
}

/// Local peaks and dips of |Z| that stand out by at least `min_db` from the next extremum on both sides (the ends count as one).
pub fn z_extrema(freq: &[f64], mag: &[f64], min_db: f64) -> Vec<ZExtremum> {
    let db: Vec<f64> = mag.iter().map(|m| 20.0 * m.max(1e-15).log10()).collect();
    let n = db.len();
    let mut ex: Vec<(usize, bool)> = vec![];
    for i in 1..n.saturating_sub(1) {
        if db[i] > db[i - 1] && db[i] >= db[i + 1] {
            ex.push((i, true));
        } else if db[i] < db[i - 1] && db[i] <= db[i + 1] {
            ex.push((i, false));
        }
    }
    let mut out = vec![];
    for (k, &(i, peak)) in ex.iter().enumerate() {
        let before = if k == 0 { db[0] } else { db[ex[k - 1].0] };
        let after = if k + 1 == ex.len() { db[n - 1] } else { db[ex[k + 1].0] };
        // the smaller of the two drops: a hump that is tall on one side only is a slope with a bump
        let drop = if peak { (db[i] - before).min(db[i] - after) } else { (before - db[i]).min(after - db[i]) };
        if drop >= min_db {
            out.push(ZExtremum { freq: freq[i], ohm: mag[i], peak });
        }
    }
    out
}

fn node_index(c: &Circuit, name: &str) -> Result<usize, SimError> {
    let t = name.trim();
    if t == "0" || t.eq_ignore_ascii_case("gnd") {
        return Ok(0);
    }
    c.names.iter().position(|n| n.eq_ignore_ascii_case(t)).ok_or_else(|| {
        let some: Vec<&str> = c.names.iter().map(String::as_str).filter(|n| !n.contains('.') && *n != "0").take(30).collect();
        SimError::Invalid(format!("「{t}」という名前のネットがありません (ネット: {})", some.join(", ")))
    })
}

/// Impedance between `node` and `reference` (ground when `None`), with all sources switched off.
/// `remove` lists elements (by name) taken out of the circuit first: the source and the part in series with it
/// for an input impedance, the load for the output impedance of a stage.
pub fn impedance_analysis(circuit: &Circuit, node: &str, reference: Option<&str>, remove: &[String], s: &AcSettings) -> Result<ImpedanceResult, SimError> {
    let a = node_index(circuit, node)?;
    let b = match reference {
        Some(r) => node_index(circuit, r)?,
        None => 0,
    };
    if a == b {
        return Err(SimError::Invalid("見るネットと基準のネットが同じです".into()));
    }
    let mut c = circuit.clone();
    for r in remove {
        if !c.elements.iter().any(|e| e.name().eq_ignore_ascii_case(r)) {
            return Err(SimError::Invalid(format!("取り除く部品「{r}」がありません")));
        }
    }
    c.elements.retain(|e| !remove.iter().any(|r| e.name().eq_ignore_ascii_case(r)));
    // every source keeps its DC value (the operating point is the same) but gives no AC signal; the test current is the only one
    for e in c.elements.iter_mut() {
        match e {
            Element::VSource { ac, .. } | Element::ISource { ac, .. } => *ac = None,
            _ => {}
        }
    }
    // the current goes into `a` and comes out of `b`
    c.elements.push(Element::ISource { name: "Ztest".into(), from: b, to: a, wave: Waveform::Dc(0.0), ac: Some((1.0, 0.0)) });
    let r: AcResult = ac_analysis(&c, s)?;
    let volts = |n: usize| -> Vec<(f64, f64)> {
        if n == 0 {
            vec![(0.0, 0.0); r.freq.len()]
        } else {
            let sg = r.signal(&format!("V({})", c.names[n])).expect("a node signal");
            sg.re.iter().cloned().zip(sg.im.iter().cloned()).collect()
        }
    };
    let (va, vb) = (volts(a), volts(b));
    let z = va.iter().zip(&vb).map(|(x, y)| C64::new(x.0 - y.0, x.1 - y.1)).collect();
    Ok(ImpedanceResult { freq: r.freq, z, warnings: r.warnings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ac::AcSweep;

    fn sweep(f0: f64, f1: f64) -> AcSettings {
        AcSettings { fstart: f0, fstop: f1, sweep: AcSweep::Decade(20) }
    }

    /// V1 (5 V) - R1 - OUT, C1 and RL on OUT.
    fn rc() -> Circuit {
        let mut c = Circuit::new();
        let (vin, out) = (c.node("VIN"), c.node("OUT"));
        c.elements.push(Element::vdc("V1", vin, 0, 5.0));
        c.elements.push(Element::Resistor { name: "R1".into(), a: vin, b: out, r: 1000.0 });
        c.elements.push(Element::Capacitor { name: "C1".into(), a: out, b: 0, c: 1e-6 });
        c.elements.push(Element::Resistor { name: "RL".into(), a: out, b: 0, r: 10_000.0 });
        c
    }

    #[test]
    fn a_resistor_and_a_capacitor_in_parallel() {
        // the source is a short: OUT sees R1 || RL || C1
        let r = impedance_analysis(&rc(), "OUT", None, &[], &sweep(1.0, 1e6)).unwrap();
        let rp = 1.0 / (1.0 / 1000.0 + 1.0 / 10_000.0);
        assert!((r.mag()[0] - rp).abs() / rp < 1e-3, "low frequency: {}", r.mag()[0]);
        let i = r.freq.iter().position(|f| *f > 9.9e4).unwrap();
        let w = 2.0 * std::f64::consts::PI * r.freq[i];
        let want = C64::new(1.0 / rp, w * 1e-6);
        let want = 1.0 / want.abs();
        assert!((r.mag()[i] - want).abs() / want < 1e-3);
        assert_eq!(character(r.phase_deg()[i]), "capacitive");
        assert_eq!(character(r.phase_deg()[0]), "resistive");
    }

    #[test]
    fn taking_the_load_out_gives_the_output_impedance_of_the_stage() {
        let r = impedance_analysis(&rc(), "OUT", None, &["RL".into()], &sweep(1.0, 10.0)).unwrap();
        assert!((r.mag()[0] - 1000.0).abs() < 1.0);
    }

    #[test]
    fn a_coil_looks_inductive() {
        let mut c = Circuit::new();
        let a = c.node("A");
        c.elements.push(Element::vdc("V1", a, 0, 1.0));
        let b = c.node("B");
        c.elements.push(Element::Resistor { name: "R1".into(), a, b, r: 1e6 });
        c.elements.push(Element::Inductor { name: "L1".into(), a: b, b: 0, l: 1e-3 });
        let r = impedance_analysis(&c, "B", None, &[], &sweep(1e3, 1e5)).unwrap();
        // 1 mH at 1 kHz: 6.28 ohm, a quarter turn ahead
        assert!((r.mag()[0] - std::f64::consts::TAU).abs() < 0.01, "{}", r.mag()[0]);
        assert_eq!(character(r.phase_deg()[0]), "inductive");
    }

    #[test]
    fn the_same_net_twice_and_a_missing_part_are_refused() {
        assert!(impedance_analysis(&rc(), "OUT", Some("OUT"), &[], &sweep(1.0, 10.0)).is_err());
        assert!(impedance_analysis(&rc(), "NOPE", None, &[], &sweep(1.0, 10.0)).is_err());
        assert!(impedance_analysis(&rc(), "OUT", None, &["R9".into()], &sweep(1.0, 10.0)).is_err());
    }

    #[test]
    fn a_capacitor_with_a_coil_shows_a_series_resonance_dip() {
        // C 10 uF with ESL 5 nH and ESR 20 mohm to ground, seen from the rail through a 1 kohm: the dip is at 1/(2 pi sqrt(LC)) = 715 kHz
        let mut c = Circuit::new();
        let (rail, m1, m2) = (c.node("RAIL"), c.node("M1"), c.node("M2"));
        c.elements.push(Element::Resistor { name: "RB".into(), a: rail, b: 0, r: 1000.0 });
        c.elements.push(Element::Capacitor { name: "C1".into(), a: rail, b: m1, c: 10e-6 });
        c.elements.push(Element::Resistor { name: "ESR".into(), a: m1, b: m2, r: 0.02 });
        c.elements.push(Element::Inductor { name: "ESL".into(), a: m2, b: 0, l: 5e-9 });
        c.elements.push(Element::vdc("V1", rail, 0, 0.0));
        let r = impedance_analysis(&c, "RAIL", None, &["V1".into()], &sweep(1e3, 1e8)).unwrap();
        let ex = z_extrema(&r.freq, &r.mag(), 3.0);
        let dip = ex.iter().find(|e| !e.peak).expect("a dip");
        assert!((dip.freq / 7.12e5 - 1.0).abs() < 0.15, "dip at {}", dip.freq);
        assert!((dip.ohm - 0.02).abs() < 0.005, "depth {}", dip.ohm);
    }
}
