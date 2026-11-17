//! Loop gain of a feedback circuit by injection (Middlebrook's two-injection method, as in the Tian probe): the loop is cut at one
//! two-terminal part and a test voltage and a test current are put in there, one after the other. Each alone is wrong when the
//! impedance on the two sides of the cut differs (a voltage injection needs a low impedance on the driving side, a current injection
//! a high one), but together they give the loop gain of the circuit as it is, whatever the impedances:
//!
//!   Tv = -V(x)/V(y)   with a voltage source in series between x (the output side) and y (the input side) of the cut,
//!   Ti = I_x/I_y      with a current source into y and the cut closed (I_x, I_y: currents into the circuit at x and y),
//!   T  = (Tv Ti - 1) / (Tv + Ti + 2).
//!
//! (Derived from the two-port seen at the cut: with Z the impedance matrix, 1 + T = det(Z) / (z11 + z22 - 2 z21), which does not depend
//! on where in the loop the cut is, as long as the loop is one loop.)

use crate::ac::{ac_analysis, AcResult, AcSettings, C64};
use crate::circuit::{Circuit, Element};
use crate::dc::SimError;
use crate::waveform::Waveform;

/// Which terminal of the cut part is on the output side of the loop (where the signal comes from).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreakDirection {
    /// Try both and take the one whose loop gain is larger (the other direction gives the reverse transmission, which is small).
    Auto,
    /// The signal goes from the part's first terminal to its second.
    AToB,
    BToA,
}

#[derive(Clone, Debug)]
pub struct LoopGain {
    pub freq: Vec<f64>,
    /// The loop gain (the return ratio): positive and large at low frequency for a feedback amplifier; the circuit is stable when
    /// its phase is still above -180 degrees where |T| crosses 1.
    pub t: Vec<C64>,
    /// The two injections on their own (T is what they combine to).
    pub tv: Vec<C64>,
    pub ti: Vec<C64>,
    /// The nets on the output side and on the input side of the cut, and the part that was cut.
    pub from_net: String,
    pub to_net: String,
    pub element: String,
    pub warnings: Vec<String>,
}

impl LoopGain {
    pub fn db(&self) -> Vec<f64> {
        self.t.iter().map(|t| 20.0 * t.abs().max(1e-15).log10()).collect()
    }

    /// Phase in degrees, continuous along the sweep and starting in (-180, 180].
    pub fn phase_deg(&self) -> Vec<f64> {
        phase_deg_of(&self.t)
    }

    /// The crossovers and the point nearest to -1, for the Nyquist and Nichols charts.
    pub fn marks(&self) -> StabilityMarks {
        stability_marks(&self.freq, &self.t)
    }
}

/// Points of a loop gain to mark on a Nyquist or Nichols chart. On a Nyquist chart (Re T, Im T) the closed loop 1 + T is
/// stable as long as the curve keeps to the right of -1 (does not go round it); the margins say how far it keeps.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StabilityMarks {
    /// Where |T| = 1 (the gain crossover): the frequency and T there, on the unit circle. Its angle from -1 is the phase margin.
    pub gain_cross: Option<(f64, C64)>,
    /// Where the phase is -180 degrees (the phase crossover): the frequency and T there, on the negative real axis between 0 and
    /// -1 when the gain margin is positive (T = -1/10^(GM/20)).
    pub phase_cross: Option<(f64, C64)>,
    /// The point of the curve nearest to -1: the frequency, T there and the distance |1 + T| (the modulus margin, 0.5 or more is
    /// good). 1 / |1 + T| is the peak of the sensitivity: how much the feedback amplifies a disturbance at that frequency.
    pub nearest: Option<(f64, C64, f64)>,
}

impl StabilityMarks {
    /// The distance from -1 as the peak of the sensitivity in dB (20 log 1/|1+T|; 6 dB or less is good).
    pub fn sensitivity_peak_db(&self) -> Option<f64> {
        self.nearest.map(|(_, _, d)| -20.0 * d.max(1e-15).log10())
    }
}

/// T at the frequency `f`, between the sampled points (linear in log f).
fn t_at(freq: &[f64], t: &[C64], f: f64) -> Option<C64> {
    let k = freq.windows(2).position(|w| w[0] <= f && f <= w[1])?;
    let (f0, f1) = (freq[k], freq[k + 1]);
    let a = if f1 > f0 && f0 > 0.0 { (f / f0).ln() / (f1 / f0).ln() } else { 0.0 };
    Some(t[k] * C64::new(1.0 - a, 0.0) + t[k + 1] * C64::new(a, 0.0))
}

/// The crossover points and the point nearest to -1 of a loop gain `t` sampled at `freq` (the same crossings as
/// `measure::stability_margins`, so the Bode table and the Nyquist and Nichols charts agree).
pub fn stability_marks(freq: &[f64], t: &[C64]) -> StabilityMarks {
    let mut out = StabilityMarks::default();
    if freq.len() < 2 || freq.len() != t.len() {
        return out;
    }
    let db: Vec<f64> = t.iter().map(|t| 20.0 * t.abs().max(1e-15).log10()).collect();
    let ph = phase_deg_of(t);
    if let Some(m) = crate::measure::stability_margins(freq, &db, &ph) {
        out.gain_cross = m.gain_crossover.and_then(|f| Some((f, t_at(freq, t, f)?)));
        out.phase_cross = m.phase_crossover.and_then(|f| Some((f, t_at(freq, t, f)?)));
    }
    out.nearest = freq
        .iter()
        .zip(t)
        .map(|(f, t)| (*f, *t, (*t + C64::new(1.0, 0.0)).abs()))
        .filter(|(_, _, d)| d.is_finite())
        .min_by(|a, b| a.2.total_cmp(&b.2));
    out
}

/// A loop gain squeezed for drawing on a Nyquist chart: inside the unit circle as it is, outside it the distance from the
/// origin is 1 + log10 |T| (|T| = 10 is drawn at 2, 1000 at 4). The angle is kept, so -1, the unit circle and the margins are
/// where they are on the plain chart, and a loop gain of 10^5 at low frequency does not push them to a dot in the middle.
pub fn nyquist_squeeze(t: C64) -> C64 {
    let r = t.abs();
    if !(r > 1.0) || !r.is_finite() {
        return t;
    }
    t * C64::new((1.0 + r.log10()) / r, 0.0)
}

/// Phase in degrees of a sampled complex response, continuous along the sweep and starting in (-180, 180].
fn phase_deg_of(t: &[C64]) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::with_capacity(t.len());
    for t in t {
        let mut p = t.arg_deg();
        if let Some(&prev) = out.last() {
            while p - prev > 180.0 {
                p -= 360.0;
            }
            while p - prev < -180.0 {
                p += 360.0;
            }
        }
        out.push(p);
    }
    out
}

impl BreakDirection {
    /// The direction that puts the net `from` on the output side of the part `part` (Auto when `from` is empty). An error names
    /// the two nets the part is between when `from` is neither.
    pub fn from_net(c: &Circuit, part: &str, from: &str) -> Result<BreakDirection, String> {
        if from.trim().is_empty() {
            return Ok(BreakDirection::Auto);
        }
        let e = c.elements.iter().find(|e| e.name().eq_ignore_ascii_case(part.trim())).ok_or_else(|| format!("部品 {part} がありません"))?;
        let (a, b) = terminals(e).ok_or_else(|| format!("{part} は切れません。ループの途中の抵抗・コンデンサ・コイルを選んでください"))?;
        if c.names[a].eq_ignore_ascii_case(from.trim()) {
            Ok(BreakDirection::AToB)
        } else if c.names[b].eq_ignore_ascii_case(from.trim()) {
            Ok(BreakDirection::BToA)
        } else {
            Err(format!("{part} は {} と {} の間にあるので、出力側のネットはそのどちらかにしてください (「{from}」ではなく)", c.names[a], c.names[b]))
        }
    }
}

fn terminals(e: &Element) -> Option<(usize, usize)> {
    match e {
        Element::Resistor { a, b, .. } | Element::Capacitor { a, b, .. } | Element::Inductor { a, b, .. } => Some((*a, *b)),
        _ => None,
    }
}

fn set_far_terminal(e: &mut Element, which_b: bool, to: usize) {
    match e {
        Element::Resistor { a, b, .. } | Element::Capacitor { a, b, .. } | Element::Inductor { a, b, .. } => {
            if which_b {
                *b = to;
            } else {
                *a = to;
            }
        }
        _ => {}
    }
}

fn combine(tv: C64, ti: C64) -> C64 {
    let one = C64::new(1.0, 0.0);
    let num = tv * ti - one;
    let den = tv + ti + C64::new(2.0, 0.0);
    num * inv(den)
}

fn inv(z: C64) -> C64 {
    let d = z.re * z.re + z.im * z.im;
    if d == 0.0 {
        return C64::new(f64::INFINITY, 0.0);
    }
    C64::new(z.re / d, -z.im / d)
}

/// One direction: the part `index` is cut with the output side at its terminal `up_is_a ? a : b`.
fn one_direction(base: &Circuit, index: usize, up_is_a: bool, s: &AcSettings) -> Result<(Vec<f64>, Vec<C64>, Vec<C64>, Vec<C64>, Vec<String>), SimError> {
    let (a, b) = terminals(&base.elements[index]).expect("a two-terminal part");
    let down = if up_is_a { b } else { a };
    let run = |vt: f64, it: f64| -> Result<(AcResult, usize), SimError> {
        let mut c = base.clone();
        let m = c.node("Zbreak.m");
        // the part now ends at the new node m; the break is between m (output side, x) and the old far terminal (input side, y)
        set_far_terminal(&mut c.elements[index], up_is_a, m);
        c.elements.push(Element::VSource { name: "Zbreak.V".into(), p: down, n: m, wave: Waveform::Dc(0.0), ac: Some((vt, 0.0)) });
        c.elements.push(Element::ISource { name: "Zbreak.I".into(), from: 0, to: down, wave: Waveform::Dc(0.0), ac: Some((it, 0.0)) });
        Ok((ac_analysis(&c, s)?, m))
    };
    let (rv, m) = run(1.0, 0.0)?;
    let (ri, _) = run(0.0, 1.0)?;
    let node = |r: &AcResult, name: &str, k: usize| -> C64 {
        let sg = r.signal(&format!("V({name})"));
        sg.map_or(C64::ZERO, |sg| C64::new(sg.re[k], sg.im[k]))
    };
    let _ = m;
    let mname = "Zbreak.m".to_string();
    let down_name = base.names[down].clone();
    let current = ri.signal("I(Zbreak.V)").ok_or_else(|| SimError::Invalid("内部エラー: 注入の電流が結果にありません".into()))?;
    let n = rv.freq.len();
    let (mut t, mut tv_all, mut ti_all) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
    for k in 0..n {
        // voltage injection: V(y) = V(x) + 1
        let (vx, vy) = (node(&rv, &mname, k), if down == 0 { C64::ZERO } else { node(&rv, &down_name, k) });
        let tv = -vx * inv(vy);
        // current injection: 1 A into y with the break closed; the wire carries I_w from x to y, and the source's own current is the other way round
        let iw = -C64::new(current.re[k], current.im[k]);
        let (ix, iy) = (-iw, C64::new(1.0, 0.0) + iw);
        let ti = ix * inv(iy);
        t.push(combine(tv, ti));
        tv_all.push(tv);
        ti_all.push(ti);
    }
    Ok((rv.freq.clone(), t, tv_all, ti_all, rv.warnings))
}

/// Loop gain with the loop cut at the two-terminal part `element` (a resistor, capacitor or inductor in the loop).
pub fn loop_gain(circuit: &Circuit, element: &str, dir: BreakDirection, s: &AcSettings) -> Result<LoopGain, SimError> {
    let index = circuit
        .elements
        .iter()
        .position(|e| e.name().eq_ignore_ascii_case(element.trim()))
        .ok_or_else(|| SimError::Invalid(format!("ループを切る部品「{element}」がありません")))?;
    let Some((a, b)) = terminals(&circuit.elements[index]) else {
        return Err(SimError::Invalid(format!("「{element}」は切れる部品ではありません。ループの途中にある、抵抗・コンデンサ・コイルを選んでください (帰還の抵抗が一番かんたんです)")));
    };
    if a == b {
        return Err(SimError::Invalid(format!("「{element}」の両端が同じネットです。ループの途中にある部品を選んでください")));
    }
    // no source gives a signal of its own: the test voltage and current are the only ones (their DC values stay, so the operating point is the same)
    let mut base = circuit.clone();
    for e in base.elements.iter_mut() {
        match e {
            Element::VSource { ac, .. } | Element::ISource { ac, .. } => *ac = None,
            _ => {}
        }
    }
    let name = circuit.elements[index].name().to_string();
    let net = |n: usize| circuit.names[n].clone();
    let make = |(freq, t, tv, ti, warnings): (Vec<f64>, Vec<C64>, Vec<C64>, Vec<C64>, Vec<String>), up_is_a: bool| {
        let (from, to) = if up_is_a { (net(a), net(b)) } else { (net(b), net(a)) };
        LoopGain { freq, t, tv, ti, from_net: from, to_net: to, element: name.clone(), warnings }
    };
    match dir {
        BreakDirection::AToB => Ok(make(one_direction(&base, index, true, s)?, true)),
        BreakDirection::BToA => Ok(make(one_direction(&base, index, false, s)?, false)),
        BreakDirection::Auto => {
            let fwd = make(one_direction(&base, index, true, s)?, true);
            let rev = make(one_direction(&base, index, false, s)?, false);
            // the right direction has the large loop gain; the wrong one gives only the (small) reverse transmission
            let peak = |l: &LoopGain| l.t.iter().map(|t| t.abs()).filter(|v| v.is_finite()).fold(0.0, f64::max);
            let mut chosen = if peak(&rev) > peak(&fwd) { rev } else { fwd };
            if (peak(&chosen)).max(1e-300) < 1.0 {
                chosen.warnings.push("ループゲインが 1 を超えません。切った部品がループの途中にないか、向きが逆かもしれません (出力側のネットを指定して試してください)".into());
            }
            Ok(chosen)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ac::AcSweep;

    fn sweep() -> AcSettings {
        AcSettings { fstart: 1.0, fstop: 1e6, sweep: AcSweep::Decade(10) }
    }

    /// An inverting amplifier by a controlled source: FB -> E1 (gain -A) -> Rp -> OUT (Cp, the load RL and the divider R1/R2 on it).
    /// The loop: OUT -> R1 -> FB -> E1 -> Rp -> OUT. Its gain is A * (ZL / (Rp + ZL)) * R2 / (R1 + R2), ZL = Cp || RL || (R1 + R2).
    fn amp() -> Circuit {
        let mut c = Circuit::new();
        let (drv, out, fb) = (c.node("DRV"), c.node("OUT"), c.node("FB"));
        c.elements.push(Element::Vcvs { name: "E1".into(), p: drv, n: 0, cp: 0, cn: fb, gain: 1000.0 });
        c.elements.push(Element::Resistor { name: "RP".into(), a: drv, b: out, r: 2000.0 });
        c.elements.push(Element::Capacitor { name: "CP".into(), a: out, b: 0, c: 1e-6 });
        c.elements.push(Element::Resistor { name: "RL".into(), a: out, b: 0, r: 5000.0 });
        c.elements.push(Element::Resistor { name: "R1".into(), a: out, b: fb, r: 9000.0 });
        c.elements.push(Element::Resistor { name: "R2".into(), a: fb, b: 0, r: 1000.0 });
        c
    }

    fn exact(f: f64) -> C64 {
        let w = 2.0 * std::f64::consts::PI * f;
        let y = |g: f64, b: f64| C64::new(g, b);
        // ZL = 1 / (1/RL + 1/(R1+R2) + jwCp)
        let yl = y(1.0 / 5000.0 + 1.0 / 10_000.0, w * 1e-6);
        let zl = inv(yl);
        let k = zl * inv(zl + C64::new(2000.0, 0.0));
        k * C64::new(1000.0 * 0.1, 0.0)
    }

    fn close(a: C64, b: C64, rel: f64) -> bool {
        let d = C64::new(a.re - b.re, a.im - b.im).abs();
        d <= rel * b.abs().max(1e-12)
    }

    #[test]
    fn cutting_the_feedback_resistor_gives_the_exact_loop_gain_although_the_sides_are_mismatched() {
        let r = loop_gain(&amp(), "R1", BreakDirection::AToB, &sweep()).unwrap();
        assert_eq!((r.from_net.as_str(), r.to_net.as_str()), ("OUT", "FB"));
        for (k, f) in r.freq.iter().enumerate().step_by(7) {
            assert!(close(r.t[k], exact(*f), 1e-6), "f={f}: {:?} vs {:?}", r.t[k], exact(*f));
        }
        // either injection alone is off (here by tens of percent: the driving side is not a low impedance)
        let k = r.freq.iter().position(|f| *f > 90.0).unwrap();
        assert!(!close(r.tv[k], exact(r.freq[k]), 0.05) || !close(r.ti[k], exact(r.freq[k]), 0.05));
    }

    #[test]
    fn any_part_in_the_loop_gives_the_same_answer_and_auto_finds_the_direction() {
        let a = loop_gain(&amp(), "R1", BreakDirection::AToB, &sweep()).unwrap();
        let b = loop_gain(&amp(), "RP", BreakDirection::AToB, &sweep()).unwrap();
        assert_eq!((b.from_net.as_str(), b.to_net.as_str()), ("DRV", "OUT"));
        for k in (0..a.freq.len()).step_by(9) {
            assert!(close(a.t[k], b.t[k], 1e-6), "f={}: {:?} vs {:?}", a.freq[k], a.t[k], b.t[k]);
        }
        // the part's terminals are the other way round in the circuit (RP: DRV -> OUT is a -> b); BToA is wrong for it, Auto is not
        let auto = loop_gain(&amp(), "R1", BreakDirection::Auto, &sweep()).unwrap();
        assert_eq!(auto.from_net, "OUT");
        let wrong = loop_gain(&amp(), "R1", BreakDirection::BToA, &sweep()).unwrap();
        assert!(wrong.t[0].abs() < 1.0, "the wrong way round gives a small gain: {}", wrong.t[0].abs());
    }

    #[test]
    fn the_phase_margin_comes_out_of_the_loop_gain() {
        let r = loop_gain(&amp(), "R1", BreakDirection::Auto, &sweep()).unwrap();
        let m = crate::measure::stability_margins(&r.freq, &r.db(), &r.phase_deg()).expect("a crossing");
        // one pole and a flat 100x gain: the phase stays far above -180
        assert!(m.phase_margin.unwrap() > 85.0, "{:?}", m.phase_margin);
        assert!(m.gain_crossover.is_some());
    }

    /// T = K / (1 + s/w_p)^3: the phase is -180 degrees at w = sqrt(3) w_p, where |T| = K / 8, so the curve crosses the negative
    /// real axis at -K/8; |T| = 1 where (1 + (w/w_p)^2)^(3/2) = K.
    fn three_pole(k: f64) -> (Vec<f64>, Vec<C64>) {
        let fp = 1000.0;
        let f: Vec<f64> = (0..=300).map(|i| 10f64.powf(1.0 + i as f64 * 0.01)).collect();
        let t = f.iter().map(|&f| {
            let d = C64::new(1.0, f / fp);
            C64::new(k, 0.0) / (d * d * d)
        });
        (f.clone(), t.collect())
    }

    #[test]
    fn nyquist_marks_sit_where_the_formulas_put_them() {
        let (f, t) = three_pole(4.0);
        let m = stability_marks(&f, &t);
        let (fp, tp) = m.phase_cross.expect("a phase crossover");
        assert!((fp / (1000.0 * 3f64.sqrt()) - 1.0).abs() < 1e-3, "{fp}");
        assert!((tp.re + 0.5).abs() < 2e-3 && tp.im.abs() < 2e-3, "{tp:?}");
        let (fg, tg) = m.gain_cross.expect("a gain crossover");
        let wg = (4f64.powf(2.0 / 3.0) - 1.0).sqrt() * 1000.0;
        assert!((fg / wg - 1.0).abs() < 1e-3, "{fg} vs {wg}");
        assert!((tg.abs() - 1.0).abs() < 1e-3);
        // the angle of the crossover point from the negative real axis is the phase margin of the Bode table
        let pm = crate::measure::stability_margins(&f, &t.iter().map(|t| 20.0 * t.abs().log10()).collect::<Vec<_>>(), &phase_deg_of(&t)).unwrap().phase_margin.unwrap();
        assert!((180.0 + tg.arg_deg() - pm).abs() < 0.05, "{} vs {pm}", 180.0 + tg.arg_deg());
        // nearest to -1: no farther than the phase crossover (0.5 away), and 1/|1+T| is the sensitivity peak
        let (_, _, d) = m.nearest.unwrap();
        assert!(d <= 0.5 + 1e-3 && d > 0.3, "{d}");
        assert!((m.sensitivity_peak_db().unwrap() + 20.0 * d.log10()).abs() < 1e-9);
        // too much gain: the curve passes beyond -1 and the phase crossover is left of it
        let (f, t) = three_pole(20.0);
        assert!(stability_marks(&f, &t).phase_cross.unwrap().1.re < -1.0);
    }

    #[test]
    fn the_squeezed_nyquist_keeps_the_unit_circle_and_the_angle() {
        let z = C64::new(-0.3, 0.4);
        assert_eq!(nyquist_squeeze(z), z);
        let big = C64::from_polar(1000.0, -120.0);
        let s = nyquist_squeeze(big);
        assert!((s.abs() - 4.0).abs() < 1e-9 && (s.arg_deg() + 120.0).abs() < 1e-9);
        assert!((nyquist_squeeze(C64::new(-1.0, 0.0)).re + 1.0).abs() < 1e-12);
    }

    #[test]
    fn the_output_net_picks_the_direction() {
        let c = amp();
        assert_eq!(BreakDirection::from_net(&c, "R1", "OUT"), Ok(BreakDirection::AToB));
        assert_eq!(BreakDirection::from_net(&c, "r1", "fb"), Ok(BreakDirection::BToA));
        assert_eq!(BreakDirection::from_net(&c, "R1", ""), Ok(BreakDirection::Auto));
        assert!(BreakDirection::from_net(&c, "R1", "DRV").unwrap_err().contains("OUT"));
        assert!(BreakDirection::from_net(&c, "E1", "OUT").is_err());
    }

    #[test]
    fn parts_that_cannot_be_cut_are_refused() {
        assert!(loop_gain(&amp(), "E1", BreakDirection::Auto, &sweep()).is_err());
        assert!(loop_gain(&amp(), "NOPE", BreakDirection::Auto, &sweep()).is_err());
    }
}
