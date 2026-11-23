//! Series voltage references from a data sheet's typ values (P6: TI REF5025; the bundled ADR4525 was written by hand in
//! `tools/gen_reference_models.py` before this template). The model: a bandgap (with its noise and its temperature curve)
//! that reaches a non-inverting amplifier through the data sheet's internal resistors, so that a capacitor on the NR pin
//! filters its noise and a resistor into the TRIM pin moves the output as on the real part; the amplifier's own noise;
//! the line regulation through a slow path and the ripple through a shelf and a high-pass of some order fitted to the
//! PSRR graph; the output sources from the input (no higher than the dropout allows) and sinks to ground through the
//! load regulation's resistances, each up to its short-circuit current; the TEMP pin is a voltage through its source
//! resistance. Details: `docs/code-notes/part-lib.md`.

use super::num::g;
use super::opamp::K4T;
use super::regulator::Noise;
use super::{Out, Row};
use serde::Deserialize;

/// The NR (TRIM/NR) pin: the bandgap reaches the amplifier through `r_bg` (the node the pin's `r_pin` hangs on).
#[derive(Clone, Debug, Deserialize)]
pub struct NrPin {
    /// The bandgap's voltage (V; REF50xx: 1.2 V). The amplifier's gain is the output over it.
    pub vbg: f64,
    /// From the bandgap to the amplifier's input (Ω; REF50xx: R4 10 kΩ).
    pub r_bg: f64,
    /// From the amplifier's input to the pin (Ω; REF50xx: R3 1 kΩ).
    pub r_pin: f64,
    /// The output noise left with a large capacitor on the pin, as a fraction of the noise without one (REF50xx: "decreases
    /// the overall noise by half", 0.5): the rest is the bandgap's (and `r_bg`'s own thermal noise).
    pub noise_left: f64,
}

/// The TEMP pin: `v25` + `slope` × (T − 25 °C) through `r_out`.
#[derive(Clone, Debug, Deserialize)]
pub struct TempPin {
    pub v25: f64,
    pub slope: f64,
    pub r_out: f64,
}

/// A series voltage reference's typ values.
#[derive(Clone, Debug, Deserialize)]
pub struct ReferenceSpec {
    #[serde(flatten)]
    pub common: super::Common,
    /// Where in the data sheet, for the notes.
    pub cite: String,
    /// Values not in the data sheet's table (read off a figure, assumed), said in the notes; empty: none.
    #[serde(default)]
    pub from_figures: String,
    /// Output (V).
    pub vout: f64,
    /// Output min and max at 25 °C, part to part (V; the initial accuracy), for the variation analyses.
    #[serde(default)]
    pub vrange: Option<[f64; 2]>,
    /// The input the line regulation is measured from (V); none: the output + 2 V.
    #[serde(default)]
    pub vin0: Option<f64>,
    /// Line regulation (ppm of the output per volt of input).
    pub line_ppm_v: f64,
    /// Load regulation, sourcing and sinking (ppm of the output per mA).
    pub load_src_ppm_ma: f64,
    pub load_snk_ppm_ma: f64,
    /// Quiescent current, typ and max (A; the max for the variation analyses).
    pub iq: f64,
    #[serde(default)]
    pub iq_max: Option<f64>,
    /// Dropout: vdo0 + rdo × I (V, Ω), sourcing.
    pub vdo0: f64,
    pub rdo: f64,
    /// Short-circuit current, sourcing and sinking (A).
    pub isc_src: f64,
    pub isc_snk: f64,
    /// Power-supply rejection (Hz, dB): one point (the table's) is flat; more (read off the graph) are fitted.
    pub psrr: Vec<[f64; 2]>,
    /// Output noise without a capacitor on NR (taken as flat).
    pub noise: Noise,
    #[serde(default)]
    pub nr: Option<NrPin>,
    #[serde(default)]
    pub temp_pin: Option<TempPin>,
    /// The output against the temperature (M8); none: the same at every temperature.
    #[serde(default)]
    pub temp: Option<super::TempCurve>,
}

/// The ripple path: `k_lo` flat, plus terms `a` through `order` high-passes at `fc` (Hz). A reference's PSRR falls 40 to 60 dB
/// per decade above a few kHz (REF5025), steeper than one high-pass gives; an LDO's falls, levels off and falls again (LT3045:
/// 90 dB at 10 kHz, 77 at 100 kHz, 76 at 1 MHz, 53 at 10 MHz), which takes two terms.
#[derive(Clone, Debug, PartialEq)]
pub struct PsrrShape {
    pub k_lo: f64,
    pub terms: Vec<HighPass>,
}

/// One term of [`PsrrShape`]: `a` (jx / (1 + jx))^order, x = f / fc.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HighPass {
    pub a: f64,
    pub fc: f64,
    pub order: u32,
}

impl PsrrShape {
    /// The rejection (dB) at `f` Hz: −20 log |k_lo + Σ a (jx / (1 + jx))^order|.
    pub fn rejection(&self, f: f64) -> f64 {
        let (mut re, mut im) = (self.k_lo, 0.0);
        for t in &self.terms {
            let x = f / t.fc;
            // jx / (1 + jx) = (x² + jx) / (1 + x²): its magnitude and angle, raised to the order
            let (m, th) = (x / (1.0 + x * x).sqrt(), (1.0 / x).atan());
            let n = t.order as f64;
            re += t.a * m.powf(n) * (n * th).cos();
            im += t.a * m.powf(n) * (n * th).sin();
        }
        -20.0 * (re * re + im * im).sqrt().log10()
    }

    /// The netlist lines of the high-passes (a copy of `vin` − `gnd`; each term a chain of buffered RC sections) and the
    /// expression of their sum for the target ("" without terms).
    pub fn lines(&self, vin: &str, gnd: &str) -> (Vec<String>, String) {
        if self.terms.is_empty() {
            return (vec![], String::new());
        }
        let orders = self.terms.iter().map(|t| format!("order {}", t.order)).collect::<Vec<_>>().join(", ");
        let mut l = vec![format!("* the ripple rising with frequency: high-passes of a copy of the input ({orders}), fitted to the data sheet's PSRR"), format!("Erh rh 0 {vin} {gnd} 1")];
        let mut sum = String::new();
        for (j, t) in self.terms.iter().enumerate() {
            let c = g(1.0 / (2.0 * std::f64::consts::PI * t.fc * 1e3), 6);
            for k in 1..=t.order {
                let from = if k == 1 { "rh".to_string() } else { format!("rh{j}_{}", k - 1) };
                l.extend([format!("Crh{j}_{k} {from} rx{j}_{k} {c}"), format!("Rrh{j}_{k} rx{j}_{k} 0 1k"), format!("Erh{j}_{k} rh{j}_{k} 0 rx{j}_{k} 0 1")]);
            }
            sum += &format!("+{}*V(rh{j}_{})", g(t.a, 6), t.order);
        }
        (l, sum)
    }
}

/// The shape fitted to the PSRR points (least squares in dB): one term of order 1 to 3, or (with four points or more) two,
/// where two fit better by more than a decibel rms. One point: flat.
pub fn psrr_shape(points: &[[f64; 2]]) -> PsrrShape {
    let lowest = points.iter().copied().fold([f64::MAX, 0.0], |b, p| if p[0] < b[0] { p } else { b });
    let flat = PsrrShape { k_lo: 10f64.powf(-lowest[1] / 20.0), terms: vec![] };
    if points.len() < 2 {
        return flat;
    }
    let (f0, f1) = (points.iter().map(|q| q[0]).fold(f64::MAX, f64::min), points.iter().map(|q| q[0]).fold(f64::MIN, f64::max));
    let (l0, l1) = (f0.log10(), f1.log10());
    let fit = |orders: &[u32]| -> (PsrrShape, f64) {
        let shape = |x: &[f64]| PsrrShape {
            k_lo: 10f64.powf(x[0]),
            terms: orders.iter().enumerate().map(|(j, &order)| HighPass { a: 10f64.powf(x[1 + 2 * j]), fc: 10f64.powf(x[2 + 2 * j]), order }).collect(),
        };
        let mut cost = |x: &[f64]| -> f64 {
            let s = shape(x);
            // no gain above the input itself; the corners near the graph (else a gain and its corner run away together)
            if s.terms.iter().any(|t| t.a > 1.0 || t.fc < f0 / 10.0 || t.fc > f1 * 100.0) {
                return 1e9;
            }
            points.iter().map(|p| (s.rejection(p[0]) - p[1]).powi(2)).sum()
        };
        // starts: the corners spread over the graph (a second above the first), the gains well below the input
        let starts: &[&[f64]] = if orders.len() == 1 { &[&[0.5], &[1.0], &[1.5]] } else { &[&[0.25, 1.0], &[0.5, 1.25], &[0.25, 1.5]] };
        let mut best: Option<(Vec<f64>, f64)> = None;
        for c in starts {
            let mut x0 = vec![-lowest[1] / 20.0];
            for &cj in c.iter() {
                x0.extend([-2.0, l0 + cj * (l1 - l0)]);
            }
            let (x, e) = super::num::nelder_mead(&mut cost, &x0, 8000, 16000, 1e-10, 1e-12);
            if best.as_ref().is_none_or(|b| e < b.1 - 1e-9) {
                best = Some((x, e));
            }
        }
        let (x, e) = best.unwrap_or_default();
        (shape(&x), e)
    };
    let n = points.len() as f64;
    let mut best = (flat, f64::MAX);
    for order in 1..=3u32 {
        let (s, e) = fit(&[order]);
        if e < best.1 - 1e-9 {
            best = (s, e);
        }
    }
    if points.len() >= 4 {
        for o1 in 1..=3u32 {
            for o2 in 1..=3u32 {
                let (s, e) = fit(&[o1, o2]);
                if (e / n).sqrt() < (best.1 / n).sqrt() - 1.0 {
                    best = (s, e);
                }
            }
        }
    }
    best.0
}

/// The `.subckt` (pins: vin gnd vout, then nr if the part has the NR pin, then temp if it has the TEMP pin).
pub fn reference_model(p: &ReferenceSpec) -> String {
    let n = &p.common.name;
    let vin0 = p.vin0.unwrap_or(p.vout + 2.0);
    let mut pins = "vin gnd vout".to_string();
    if p.nr.is_some() {
        pins += " nr";
    }
    if p.temp_pin.is_some() {
        pins += " temp";
    }
    let sh = psrr_shape(&p.psrr);
    let line = p.line_ppm_v * 1e-6 * p.vout;
    // at DC the input reaches the output by the line regulation; above ~1 Hz only by the PSRR
    let k_dc = line - sh.k_lo;
    let mut l = vec![format!(".subckt {n} {pins}")];
    // the quiescent current: flat above a few volts, like a resistor near 0 V (as the regulators); a 0 A source the variation
    // analyses move up to the max
    let spread_iq = p.iq_max.filter(|m| *m > p.iq);
    l.push("* quiescent current (typ), flat above a few volts of input".into());
    if let Some(max) = spread_iq {
        l.extend([
            format!("Bq vin gnd I=({}+V(qs,gnd))*tanh(V(vin,gnd)/2)", g(p.iq, 6)),
            "* the quiescent current's difference from part to part (0 A; the variation analyses move it up to the data sheet's max)".into(),
            "Iqs gnd qs DC 0".into(),
            "Rqs qs gnd 1".into(),
            format!("* spread Iqs 0 {} iq (data sheet: typ {} uA, max {} uA; no min, so from typ)", g(max - p.iq, 6), g(p.iq * 1e6, 6), g(max * 1e6, 6)),
        ]);
    } else {
        l.push(format!("Bq vin gnd I={}*tanh(V(vin,gnd)/2)", g(p.iq, 6)));
    }
    l.extend([
        "* line regulation through a slow path (the data sheet's DC figure)".into(),
        "Elf lfa gnd vin gnd 1".into(),
        "Rlf lfa lf 1.59Meg".into(),
        "Clf lf gnd 100n".into(),
    ]);
    let (hp, hp_sum) = sh.lines("vin", "gnd");
    l.extend(hp);
    let density = p.noise.density();
    let temp = p.temp.as_ref().map(|c| format!("*(1+{})", c.expr())).unwrap_or_default();
    let ripple = format!("+{}*(V(lf,gnd)-{})+{}*(V(vin,gnd)-{}){hp_sum}", g(k_dc, 6), g(vin0, 6), g(sh.k_lo, 6), g(vin0, 6));
    let vt = if p.vrange.is_some() { "+V(vt,gnd)" } else { "" };
    // the target, no higher than the input less the dropout at no load (below the dropout it follows the input)
    let target = |core: String| format!("Btgt t0 gnd V=min({core}{ripple}{vt},V(vin,gnd)-{})", g(p.vdo0, 6));
    // the gain on a 1 k resistor's noise that gives the density d (V/√Hz)
    let nz = |d: f64| g(d / (K4T * 1e3).sqrt(), 6);
    l.push("* noise sources: a 1 k resistor's noise, scaled".into());
    match &p.nr {
        Some(nr) => {
            let gain = p.vout / nr.vbg;
            // the output's noise without a capacitor: the amplifier's (left with a large one) and the bandgap's, less r_bg's own
            // thermal noise (the simulator adds it), all taken back to the bandgap
            let amp = nr.noise_left * density;
            let r_bg = (K4T * nr.r_bg).sqrt() * gain;
            let bg = ((1.0 - nr.noise_left.powi(2)) * density * density - r_bg * r_bg).max(0.0).sqrt() / gain;
            l.extend(["Rnz1 nz1 gnd 1k".into(), "Rnz2 nz2 gnd 1k".into()]);
            l.extend([
                format!("* the bandgap ({} V){}, with its noise", g(nr.vbg, 6), if p.temp.is_some() { " and its temperature curve" } else { "" }),
                format!("Bbg bg0 gnd V={}{temp}", g(nr.vbg, 6)),
            ]);
            l.push(format!("Ebg bg bg0 nz1 gnd {}", nz(bg)));
            l.extend([
                "* to the amplifier's input through the data sheet's internal resistors (a capacitor on NR filters the bandgap's noise; a resistor into it trims)".into(),
                format!("Rbg bg p {}", g(nr.r_bg, 6)),
                format!("Rpin p nr {}", g(nr.r_pin, 6)),
                format!("* the amplifier (gain {}): its noise, the line regulation and the ripple", g(gain, 6)),
                target(format!("{}*V(p,gnd)", g(gain, 6))),
                format!("En tgt t0 nz2 gnd {}", nz(amp)),
            ]);
        }
        None => {
            l.extend(["Rnz nz gnd 1k".into(), format!("* the output{}, the line regulation and the ripple, with the noise", if p.temp.is_some() { " (and its temperature curve)" } else { "" })]);
            l.push(target(format!("{}{temp}", g(p.vout, 6))));
            l.push(format!("En tgt t0 nz gnd {}", nz(density)));
        }
    }
    if let Some([lo, hi]) = p.vrange {
        l.extend([
            "* the output's difference from part to part (0 V; the variation analyses move it over the data sheet's initial accuracy)".into(),
            "Vtol vt gnd DC 0".into(),
            format!("* spread Vtol {} {} vout (data sheet: {} to {} V)", g(lo - p.vout, 6), g(hi - p.vout, 6), g(lo, 6), g(hi, 6)),
        ]);
    }
    let (r_src, r_snk) = (p.load_src_ppm_ma * 1e-6 * p.vout / 1e-3, p.load_snk_ppm_ma * 1e-6 * p.vout / 1e-3);
    // sourcing: what the load regulation's resistance asks for, no more than the input allows through the dropout's resistance
    let x_src = format!("min((V(tgt,gnd)-V(vout,gnd))/{},(V(vin,gnd)-{}-V(vout,gnd))/{})", g(r_src, 6), g(p.vdo0, 6), g(p.rdo.max(1e-3), 6));
    let x_snk = format!("(V(vout,gnd)-V(tgt,gnd))/{}", g(r_snk, 6));
    l.extend([
        "* output: sourcing from vin (no more than the dropout allows) and sinking to gnd through the load regulation's resistances,".into(),
        "* each up to the short-circuit current".into(),
        format!("Bsrc vin vout I=limit({x_src},0,{})*step({x_src},1u)", g(p.isc_src, 6)),
        format!("Bsnk vout gnd I=limit({x_snk},0,{})*step({x_snk},1u)", g(p.isc_snk, 6)),
    ]);
    if let Some(t) = &p.temp_pin {
        l.extend([
            format!("* TEMP pin: {} mV + {} mV/C x (T - 25 C) through {} (while powered)", g(t.v25 * 1e3, 6), g(t.slope * 1e3, 6), g(t.r_out, 6)),
            format!("Btp tq gnd V=({}+{}*(temp-25))*limit(V(vin,gnd)-1,0,1)", g(t.v25, 6), g(t.slope, 6)),
            format!("Rtp tq temp {}", g(t.r_out, 6)),
        ]);
    }
    l.push(format!(".ends {n}"));
    l.join("\n")
}

/// What the model is made from and what it leaves out (`sim.limits`).
pub fn reference_limits(p: &ReferenceSpec) -> Vec<String> {
    let sh = psrr_shape(&p.psrr);
    let density = p.noise.density();
    let mut rows = vec![
        format!("データシートの typ の値から作った等価モデルです (メーカーのモデルではありません)。出典: {}。", p.cite),
        format!(
            "再現しているもの: 出力 {} V、ライン・レギュレーション {} ppm/V、ロード・レギュレーション (吐き出し {} ppm/mA・吸い込み {} ppm/mA)、電源電流 {} µA、ドロップアウト ({} V + {} Ω × 電流)、短絡電流 (吐き出し {} mA・吸い込み {} mA)、PSRR ({})、出力雑音 {} nV/√Hz (平ら)。",
            g(p.vout, 6),
            g(p.line_ppm_v, 6),
            g(p.load_src_ppm_ma, 6),
            g(p.load_snk_ppm_ma, 6),
            g(p.iq * 1e6, 6),
            g(p.vdo0, 6),
            g(p.rdo, 6),
            g(p.isc_src * 1e3, 6),
            g(p.isc_snk * 1e3, 6),
            if !sh.terms.is_empty() {
                let fmax = p.psrr.iter().map(|q| q[0]).fold(0.0, f64::max);
                format!(
                    "データシートのグラフに合わせた: {}",
                    [100.0, 1e3, 1e4, 1e5].iter().filter(|f| **f <= fmax * 1.5).map(|&f| format!("{} で {:.0} dB", hz(f), sh.rejection(f))).collect::<Vec<_>>().join("、")
                )
            } else {
                format!("{:.0} dB、どの周波数でも同じとした", sh.rejection(p.psrr[0][0]))
            },
            g(density * 1e9, 4)
        ),
    ];
    if let Some(nr) = &p.nr {
        let fc = 1.0 / (2.0 * std::f64::consts::PI * nr.r_bg * 1e-6);
        rows.push(format!(
            "NR (TRIM/NR) ピン: 内部のバンドギャップ {} V から {} を通って増幅器の入力、そこから {} でピン (データシートのブロック図)。ピンに 1 µF を付けると {:.0} Hz の低域通過になり、雑音は {} 倍に下がります。抵抗でピンに電流を流すと出力を微調整できます。",
            g(nr.vbg, 6),
            ohm_text(nr.r_bg),
            ohm_text(nr.r_pin),
            fc,
            g(nr.noise_left, 3)
        ));
    }
    if let Some(t) = &p.temp_pin {
        rows.push(format!("TEMP ピン: 25 ℃ で {} mV、{} mV/℃、出力抵抗 {} (データシートの式)。", g(t.v25 * 1e3, 6), g(t.slope * 1e3, 6), ohm_text(t.r_out)));
    }
    if !p.from_figures.is_empty() {
        rows.push(format!("データシートの表に無い値: {}。", p.from_figures));
    }
    if let Some([lo, hi]) = p.vrange {
        rows.push(format!(
            "出力電圧の部品ごとの違い ({}〜{} V、データシートの初期精度){}は、ばらつき・最悪ケースの解析でだけ入ります (ふだんの解析は typ)。",
            g(lo, 6),
            g(hi, 6),
            p.iq_max.filter(|m| *m > p.iq).map(|m| format!("と電源電流 (typ〜max {} µA) ", g(m * 1e6, 6))).unwrap_or_default()
        ));
    }
    if let Some(c) = &p.temp {
        // in ppm: a reference moves by parts per million (the regulators' span is in percent)
        let (lo, hi) = c.points.iter().fold((f64::MAX, f64::MIN), |(l, h), q| (l.min(q[1]), h.max(q[1])));
        let base = c.at(25.0);
        rows.push(format!(
            "温度による出力電圧の変化 ({}): {}〜{} ℃ で 25 ℃ から {:+.0}〜{:+.0} ppm (ボックス法で {:.1} ppm/℃)。温度を設定した解析・温度スイープ・自己発熱で効きます (27 ℃ でデータシートの 25 ℃ の値)。",
            c.from,
            g(c.points[0][0], 6),
            g(c.points[c.points.len() - 1][0], 6),
            (lo - base) * 1e6,
            (hi - base) * 1e6,
            (hi - lo) * 1e6 / (c.points[c.points.len() - 1][0] - c.points[0][0])
        ));
    }
    rows.push(format!(
        "扱わないもの: {}熱ヒステリシスと長期のドリフト、0.1〜10 Hz の 1/f 雑音、起動の時間、負荷や入力が急に変わったときの応答、出力の容量による安定性、出力電圧以外の温度による変化。",
        if p.temp.is_some() { "" } else { "温度による変化、" }
    ));
    rows.push("最終の確認は、メーカーの公式モデルで行ってください。".into());
    rows
}

/// "10 kΩ", "1 kΩ", "60 kΩ", "470 Ω".
pub(super) fn ohm_text(r: f64) -> String {
    if r >= 1e6 {
        format!("{} MΩ", g(r / 1e6, 4))
    } else if r >= 1e3 {
        format!("{} kΩ", g(r / 1e3, 4))
    } else {
        format!("{} Ω", g(r, 4))
    }
}

fn hz(f: f64) -> String {
    if f >= 1e3 { format!("{} kHz", g(f / 1e3, 3)) } else { format!("{} Hz", g(f, 3)) }
}

/// Values the template cannot use, with what to fix.
pub fn check_reference(p: &ReferenceSpec) -> Result<(), String> {
    for (what, v) in [
        ("出力電圧", p.vout),
        ("ロード・レギュレーション (吐き出し)", p.load_src_ppm_ma),
        ("ロード・レギュレーション (吸い込み)", p.load_snk_ppm_ma),
        ("短絡電流 (吐き出し)", p.isc_src),
        ("短絡電流 (吸い込み)", p.isc_snk),
    ] {
        if !(v.is_finite() && v > 0.0) {
            return Err(format!("{what}は 0 より大きい数にしてください"));
        }
    }
    if p.psrr.is_empty() || p.psrr.iter().any(|q| !(q[0] > 0.0 && q[1] > 0.0)) {
        return Err("PSRR は (周波数 Hz, dB) の点を 1 つ以上、どちらも正の数で".into());
    }
    if p.vdo0 < 0.0 || p.rdo < 0.0 || p.iq < 0.0 || p.line_ppm_v < 0.0 {
        return Err("ドロップアウト・電源電流・ライン・レギュレーションは 0 以上にしてください".into());
    }
    if let Some([lo, hi]) = p.vrange
        && !(lo <= p.vout && p.vout <= hi)
    {
        return Err("出力電圧の min・max の間に typ が入っていません".into());
    }
    if let Some(nr) = &p.nr
        && !(nr.vbg > 0.0 && nr.vbg <= p.vout && nr.r_bg > 0.0 && nr.r_pin > 0.0 && (0.0..=1.0).contains(&nr.noise_left))
    {
        return Err("NR ピン: バンドギャップの電圧は 0 より大きく出力以下、抵抗は正、残る雑音の割合は 0〜1 にしてください".into());
    }
    if let Some(t) = &p.temp_pin
        && !(t.r_out > 0.0)
    {
        return Err("TEMP ピンの出力抵抗は 0 より大きくしてください".into());
    }
    if let Some(c) = &p.temp {
        c.check()?;
    }
    if let Noise::Rms { f_low, f_high, .. } = p.noise
        && f_high <= f_low
    {
        return Err("雑音の帯域は、上の周波数を下の周波数より大きくしてください".into());
    }
    Ok(())
}

pub(super) fn fit_reference(p: &ReferenceSpec) -> Result<Out, String> {
    check_reference(p)?;
    // the PSRR shape against each reading (the table's and the graph's points)
    let sh = psrr_shape(&p.psrr);
    let rows: Vec<Row> = p.psrr.iter().map(|q| super::row("PSRR", hz(q[0]), q[1], sh.rejection(q[0]), "dB")).collect();
    Ok((reference_model(p), reference_limits(p), rows, vec![]))
}
