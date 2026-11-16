//! Common-mode rejection (CMRR) of a differential amplifier: how much better it amplifies the difference of its two inputs than
//! what the two have in common. Two input sources are chosen (one on each input, both to ground); every other AC excitation is
//! taken away and two AC sweeps are run:
//!
//! - differential: +0.5 V on the + input's source and −0.5 V (180°) on the − input's, so the inputs differ by 1 V → `Ad`;
//! - common: 1 V on both → `Acm`.
//!
//! CMRR = |Ad / Acm| in dB. With ideal parts and resistors that match exactly, `Acm` is zero and the ratio has no end: it is shown
//! as at most [`CMRR_CAP_DB`]. The bundled op amp models leave their own CMRR out, so the answer is the one of the resistors
//! around them; [`cmrr_spread`] draws the resistors inside a tolerance and shows how quickly a mismatch spoils it (a difference
//! amplifier of gain G with resistors of tolerance t is near (1 + G) / 4t at the worst).

use crate::ac::{ac_analysis, AcResult, AcSettings, AcSignal, AcSweep};
use crate::circuit::{Circuit, Element};
use crate::dc::SimError;
use crate::montecarlo::uniform;
use crate::psrr::{psrr_summary, PsrrSummary};

/// The largest CMRR shown (dB): what is left when the common gain is below 10⁻¹⁰ of the differential one.
pub const CMRR_CAP_DB: f64 = 200.0;

/// The sources that can drive an input: voltage sources of the schematic (not inside a part's model). The ones with an AC value
/// come first (an input's source usually has one), then the ones with a DC value of 0, then the rest.
pub fn cmrr_input_sources(c: &Circuit) -> Vec<String> {
    let mut v: Vec<(u8, String)> = c
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::VSource { name, wave, ac, .. } if !name.contains('.') => {
                let zero = matches!(wave, crate::waveform::Waveform::Dc(x) if *x == 0.0);
                Some((if ac.is_some() { 0 } else if zero { 1 } else { 2 }, name.clone()))
            }
            _ => None,
        })
        .collect();
    v.sort_by_key(|(k, _)| *k);
    v.into_iter().map(|(_, n)| n).collect()
}

#[derive(Clone, Debug)]
pub struct CmrrSettings {
    /// The source on the + input, and the one on the − input.
    pub plus: String,
    pub minus: String,
    /// The node measured.
    pub output: String,
    pub ac: AcSettings,
}

#[derive(Clone, Debug)]
pub struct Cmrr {
    pub freq: Vec<f64>,
    /// The output for a 1 V difference between the inputs (`Ad(OUT)`).
    pub diff: AcSignal,
    /// The output for 1 V on both inputs (`Acm(OUT)`).
    pub common: AcSignal,
    /// 20 log |Ad / Acm|, at most `CMRR_CAP_DB`.
    pub cmrr_db: Vec<f64>,
    pub warnings: Vec<String>,
}

impl Cmrr {
    /// The figures as an AC result for the AC viewer: `CMRR` (as a ratio, its dB is the CMRR), `Ad(OUT)` and `Acm(OUT)`.
    pub fn as_ac_result(&self) -> AcResult {
        let ratio = AcSignal { name: "CMRR".into(), unit: "V", re: self.cmrr_db.iter().map(|d| 10f64.powf(d / 20.0)).collect(), im: vec![0.0; self.freq.len()] };
        AcResult { freq: self.freq.clone(), signals: vec![ratio, self.diff.clone(), self.common.clone()], warnings: self.warnings.clone() }
    }

    /// The CMRR at the lowest frequency, the worst one and at the round frequencies (100 Hz, 1 kHz ...).
    pub fn summary(&self) -> Option<PsrrSummary> {
        let neg: Vec<f64> = self.cmrr_db.iter().map(|d| -d).collect();
        psrr_summary(&self.freq, &neg)
    }
}

/// The circuit with `plus` and `minus` driven by the given (magnitude, phase °) and every other AC excitation taken away.
fn driven(c: &Circuit, plus: &str, minus: &str, p: (f64, f64), m: (f64, f64)) -> Result<Circuit, SimError> {
    let mut out = c.clone();
    let (mut found_p, mut found_m) = (false, false);
    for e in &mut out.elements {
        match e {
            Element::VSource { name, ac, .. } => {
                *ac = if name.eq_ignore_ascii_case(plus) {
                    found_p = true;
                    Some(p)
                } else if name.eq_ignore_ascii_case(minus) {
                    found_m = true;
                    Some(m)
                } else {
                    None
                }
            }
            Element::ISource { ac, .. } => *ac = None,
            _ => {}
        }
    }
    for (found, name, side) in [(found_p, plus, "+"), (found_m, minus, "−")] {
        if !found {
            return Err(SimError::Invalid(format!("{side} 入力の信号源 {name} が見つかりません。入力につながった電圧源 (V3 など) を選んでください")));
        }
    }
    Ok(out)
}

fn output_signal(r: &AcResult, c: &Circuit, node: &str) -> Result<AcSignal, SimError> {
    r.signals.iter().find(|s| s.name.eq_ignore_ascii_case(&format!("V({node})"))).cloned().ok_or_else(|| crate::dc::no_such_net(c, node))
}

fn ratio_db(d: (f64, f64), cm: (f64, f64)) -> f64 {
    let (a, b) = (d.0.hypot(d.1), cm.0.hypot(cm.1));
    if a <= 0.0 {
        return 0.0;
    }
    (20.0 * (a / b.max(a * 1e-10)).log10()).min(CMRR_CAP_DB)
}

pub fn cmrr(c: &Circuit, s: &CmrrSettings) -> Result<Cmrr, SimError> {
    let (plus, minus, node) = (s.plus.trim(), s.minus.trim(), s.output.trim());
    if plus.is_empty() || minus.is_empty() {
        return Err(SimError::Invalid("+ 入力と − 入力の信号源 (電圧源) を 1 つずつ選んでください".into()));
    }
    if plus.eq_ignore_ascii_case(minus) {
        return Err(SimError::Invalid("+ 入力と − 入力に同じ信号源が選ばれています。それぞれの入力につながった別の電圧源を選んでください".into()));
    }
    if node.is_empty() {
        return Err(SimError::Invalid("出力のネットを選んでください".into()));
    }
    let dc = driven(c, plus, minus, (0.5, 0.0), (0.5, 180.0))?;
    let cm = driven(c, plus, minus, (1.0, 0.0), (1.0, 0.0))?;
    let rd = ac_analysis(&dc, &s.ac)?;
    let rc = ac_analysis(&cm, &s.ac)?;
    let mut diff = output_signal(&rd, c, node)?;
    let mut common = output_signal(&rc, c, node)?;
    diff.name = format!("Ad({node})");
    common.name = format!("Acm({node})");
    let cmrr_db = (0..rd.freq.len()).map(|i| ratio_db((diff.re[i], diff.im[i]), (common.re[i], common.im[i]))).collect();
    let mut warnings = rd.warnings;
    for w in rc.warnings {
        if !warnings.contains(&w) {
            warnings.push(w);
        }
    }
    Ok(Cmrr { freq: rd.freq, diff, common, cmrr_db, warnings })
}

/// The CMRR at one frequency when the resistors are drawn inside a tolerance.
#[derive(Clone, Debug)]
pub struct CmrrSpread {
    pub freq: f64,
    pub tol_r: f64,
    /// With every resistor at its value.
    pub nominal: f64,
    /// One CMRR (dB) per run that solved, smallest first.
    pub values: Vec<f64>,
    pub failed: usize,
    /// Each resistor alone at the high end of its tolerance and the CMRR that leaves, the lowest (the most harmful) first.
    pub alone: Vec<(String, f64)>,
}

impl CmrrSpread {
    /// The value below which `p` (0 … 1) of the runs lie.
    pub fn quantile(&self, p: f64) -> Option<f64> {
        (!self.values.is_empty()).then(|| self.values[((self.values.len() - 1) as f64 * p.clamp(0.0, 1.0)).round() as usize])
    }
}

/// `runs` circuits with every resistor of the schematic (not the ones inside models) drawn evenly inside `tol_r` (0.01 = ±1 %),
/// and the CMRR at `freq` of each.
pub fn cmrr_spread(c: &Circuit, s: &CmrrSettings, freq: f64, tol_r: f64, runs: usize, seed: u64) -> Result<CmrrSpread, SimError> {
    if !(tol_r > 0.0 && tol_r < 1.0) {
        return Err(SimError::Invalid("抵抗の許容差は 0 より大きく 100 % 未満にしてください".into()));
    }
    if runs == 0 || runs > 20_000 {
        return Err(SimError::Invalid("回数は 1〜20000 にしてください".into()));
    }
    let one = CmrrSettings { ac: AcSettings { fstart: freq, fstop: freq, sweep: AcSweep::Decade(1) }, ..s.clone() };
    let at = |c: &Circuit| cmrr(c, &one).map(|r| r.cmrr_db[0]);
    let nominal = at(c)?;
    let resistors: Vec<usize> = c.elements.iter().enumerate().filter(|(_, e)| matches!(e, Element::Resistor { .. }) && !e.name().contains('.')).map(|(i, _)| i).collect();
    if resistors.is_empty() {
        return Err(SimError::Invalid("ばらつかせる抵抗がありません".into()));
    }
    let scale = |e: &mut Element, f: f64| {
        if let Element::Resistor { r, .. } = e {
            *r *= f;
        }
    };
    let ids: Vec<usize> = (0..runs).collect();
    let got = crate::par_map(&ids, |&run| {
        let mut c = c.clone();
        for (k, &i) in resistors.iter().enumerate() {
            let u = uniform(seed, (run * resistors.len() + k) as u64);
            scale(&mut c.elements[i], 1.0 + tol_r * (2.0 * u - 1.0));
        }
        at(&c)
    });
    let mut values: Vec<f64> = got.iter().filter_map(|r| r.as_ref().ok().copied()).filter(|v| v.is_finite()).collect();
    let failed = runs - values.len();
    values.sort_by(f64::total_cmp);
    let alone = crate::par_map(&resistors, |&i| {
        let mut c = c.clone();
        scale(&mut c.elements[i], 1.0 + tol_r);
        at(&c).ok().map(|v| (c.elements[i].name().to_string(), v))
    });
    let mut alone: Vec<(String, f64)> = alone.into_iter().flatten().collect();
    alone.sort_by(|a, b| a.1.total_cmp(&b.1));
    Ok(CmrrSpread { freq, tol_r, nominal, values, failed, alone })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::waveform::Waveform;

    /// A difference amplifier of gain `g` around an ideal amplifier (a VCVS of gain 1e7), R4 scaled by `r4`.
    fn diff_amp(g: f64, r4: f64) -> Circuit {
        let mut c = Circuit::new();
        let (inp, inn, ninv, inv, out) = (c.node("INP"), c.node("INN"), c.node("NINV"), c.node("INV"), c.node("OUT"));
        c.add(Element::VSource { name: "V3".into(), p: inp, n: 0, wave: Waveform::Dc(0.0), ac: Some((1.0, 0.0)) });
        c.add(Element::VSource { name: "V4".into(), p: inn, n: 0, wave: Waveform::Dc(0.0), ac: Some((1.0, 0.0)) });
        c.add(Element::Resistor { name: "R1".into(), a: inn, b: inv, r: 10e3 });
        c.add(Element::Resistor { name: "R2".into(), a: inv, b: out, r: 10e3 * g });
        c.add(Element::Resistor { name: "R3".into(), a: inp, b: ninv, r: 10e3 });
        c.add(Element::Resistor { name: "R4".into(), a: ninv, b: 0, r: 10e3 * g * r4 });
        c.add(Element::Vcvs { name: "E1".into(), p: out, n: 0, cp: ninv, cn: inv, gain: 1e7 });
        c
    }

    fn settings() -> CmrrSettings {
        CmrrSettings { plus: "V3".into(), minus: "V4".into(), output: "OUT".into(), ac: AcSettings { fstart: 10.0, fstop: 100e3, sweep: AcSweep::Decade(5) } }
    }

    #[test]
    fn matched_resistors_reject_everything_and_a_mismatch_follows_the_textbook() {
        let r = cmrr(&diff_amp(10.0, 1.0), &settings()).unwrap();
        // Ad = 10, Acm ≈ 0 (only the amplifier's finite gain is left)
        assert!((r.diff.mag()[0] - 10.0).abs() < 1e-3, "{:?}", r.diff.mag()[0]);
        assert!(r.cmrr_db[0] > 140.0, "{}", r.cmrr_db[0]);
        // R4 1 % high: Acm = G δ / (1 + G)(1 + δ)... ≈ 10·0.01/11, CMRR ≈ 20 log(10 / 0.00901) = 60.9 dB
        let r = cmrr(&diff_amp(10.0, 1.01), &settings()).unwrap();
        let acm = 10.0 * 1.01 / (1.0 + 10.0 * 1.01) - 10.0 / 11.0;
        let want = 20.0 * ((r.diff.mag()[0]) / (acm * 11.0)).log10();
        assert!((r.cmrr_db[0] - want).abs() < 0.2, "{} vs {want}", r.cmrr_db[0]);
        assert!((55.0..65.0).contains(&r.cmrr_db[0]));
        let s = r.summary().unwrap();
        assert_eq!(s.low.0, 10.0);
        let ac = r.as_ac_result();
        assert!((ac.signal("CMRR").unwrap().db()[0] - r.cmrr_db[0]).abs() < 1e-9);
        assert!(ac.signal("Ad(OUT)").is_some() && ac.signal("Acm(OUT)").is_some());
    }

    #[test]
    fn the_spread_of_one_percent_resistors_stays_above_the_worst_case_estimate() {
        let s = cmrr_spread(&diff_amp(10.0, 1.0), &settings(), 100.0, 0.01, 200, 1).unwrap();
        assert_eq!((s.values.len(), s.failed), (200, 0));
        // the worst case (1 + G) / 4t = 275 → 48.8 dB; random draws mostly do better
        let (lo, med) = (s.values[0], s.quantile(0.5).unwrap());
        assert!(lo > 48.0 && med > lo && med < 90.0, "{lo} {med}");
        assert!(s.nominal > 140.0);
        assert_eq!(s.alone.len(), 4);
        assert!(s.alone.iter().all(|(_, v)| *v < 70.0), "{:?}", s.alone);
    }

    #[test]
    fn inputs_are_listed_and_bad_choices_are_refused() {
        let c = diff_amp(1.0, 1.0);
        assert_eq!(cmrr_input_sources(&c), vec!["V3".to_string(), "V4".to_string()]);
        let bad = |p: &str, m: &str, o: &str| cmrr(&c, &CmrrSettings { plus: p.into(), minus: m.into(), output: o.into(), ..settings() }).is_err();
        assert!(bad("V3", "V3", "OUT"));
        assert!(bad("V3", "V9", "OUT"));
        assert!(bad("V3", "V4", "NOPE"));
        assert!(bad("", "V4", "OUT"));
        assert!(cmrr_spread(&c, &settings(), 100.0, 0.0, 10, 1).is_err());
    }
}
