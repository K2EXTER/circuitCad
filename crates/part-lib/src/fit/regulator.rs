//! Linear regulator (fixed or adjustable) and shunt reference (TL431) models from a data sheet's typ values (M3; the
//! bundled 78xx, 78L05, AMS1117, LM317, MCP1700 and TL431 are made this way). The regulator: a target voltage (the
//! output, with the line regulation through a slow path and the ripple rejection flat) clamped from above by the
//! dropout's ceiling and by the current limit, then the load regulation's resistance; the output current comes from the
//! input, plus the ground current. Was `tools/gen_regulator_models.py`; the text is the same to the character
//! (`tests.rs`). How the bundled ones measure against their data sheets: `docs/models/regulators.md`.

use super::Out;
use super::num::g;
use super::opamp::K4T;
use serde::Deserialize;

/// The output noise as the data sheet gives it.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum Noise {
    /// Rms voltage over a band (V, Hz, Hz): taken as flat over it.
    Rms { rms: f64, f_low: f64, f_high: f64 },
    /// Density (V/√Hz).
    Density { density: f64 },
}

impl Noise {
    fn density(&self) -> f64 {
        match *self {
            Noise::Rms { rms, f_low, f_high } => rms / (f_high - f_low).sqrt(),
            Noise::Density { density } => density,
        }
    }
}

/// The current limit at two input − output differences (V, A), joined by a straight line.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct LimitAt {
    pub diff: f64,
    pub amps: f64,
}

/// A linear regulator's typ values.
#[derive(Clone, Debug, Deserialize)]
pub struct RegulatorSpec {
    #[serde(flatten)]
    pub common: super::Common,
    /// Where in the data sheet, for the notes.
    pub cite: String,
    /// Values not in the data sheet's table (read off a figure, assumed), said in the notes; empty: none.
    #[serde(default)]
    pub from_figures: String,
    /// Adjustable (LM317: pins in adj out, `vout` is the reference OUT − ADJ) or fixed (in gnd out).
    #[serde(default)]
    pub adjustable: bool,
    /// Output (V; adjustable: the reference).
    pub vout: f64,
    /// Output min and max at 25 °C, part to part (V), for the variation analyses.
    #[serde(default)]
    pub vrange: Option<[f64; 2]>,
    /// The data sheet's test input (V); none: 6.25 V.
    #[serde(default)]
    pub vin0: Option<f64>,
    /// Line regulation dVout/dVin (V/V).
    pub line: f64,
    /// Load regulation as a resistance (Ω).
    pub rout: f64,
    /// Dropout: vdo0 + rdo × I (V, Ω).
    pub vdo0: f64,
    pub rdo: f64,
    /// Ground (quiescent) current (A; fixed regulators).
    #[serde(default)]
    pub iq: f64,
    /// Adjustment pin current, typ and max (A; adjustable regulators).
    #[serde(default)]
    pub iadj: f64,
    #[serde(default)]
    pub iadj_max: f64,
    /// Minimum load current (A; adjustable regulators, said in the notes).
    #[serde(default)]
    pub min_load: Option<f64>,
    pub ilim: [LimitAt; 2],
    /// Ripple rejection (dB) at `rr_f` (Hz); an adjustable regulator's is given with the output set to `rr_vout`.
    pub rr: f64,
    pub rr_f: f64,
    #[serde(default)]
    pub rr_vout: Option<f64>,
    pub noise: Noise,
    /// The output (adjustable: the reference) against the temperature (M8); none: the same at every temperature.
    #[serde(default)]
    pub temp: Option<super::TempCurve>,
    /// Ripple rejection against frequency (Q16), read off the data sheet's graph: points (Hz, dB), at the same output as `rr`.
    /// The model's ripple path is then a shelf fitted to them (and to `rr` at `rr_f`): the table's rejection at low frequencies,
    /// falling 20 dB per decade above a corner towards a floor. Empty: the same at every frequency.
    #[serde(default)]
    pub rr_curve: Vec<[f64; 2]>,
}

/// Python's `repr` of a float: the shortest that reads back, with ".0" on whole numbers ("5.0", "1.25").
fn repr(x: f64) -> String {
    format!("{x:?}")
}

/// The ripple path's shape (Q16): k_lo at low frequencies, plus high-passes of the input, each (gain, corner Hz). Two of them make a
/// curve that falls gently and then steeply (the LM317's).
#[derive(Clone, Debug, PartialEq)]
pub struct RippleShape {
    pub k_lo: f64,
    pub terms: Vec<(f64, f64)>,
}

impl RippleShape {
    /// The ripple rejection (dB) at `f` Hz: -20 log |k_lo + sum a jx / (1 + jx)|, x = f / corner.
    pub fn rejection(&self, f: f64) -> f64 {
        let (mut re, mut im) = (self.k_lo, 0.0);
        for &(a, fc) in &self.terms {
            let x = f / fc;
            let d = 1.0 + x * x;
            re += a * x * x / d;
            im += a * x / d;
        }
        -20.0 * (re * re + im * im).sqrt().log10()
    }
}

/// The shape fitted to the graph's points and the table's `rr` at `rr_f` (that one weighted ten times, so that the model keeps the
/// table's value), least squares in dB, from several starts (Q16). None without a graph.
pub fn rr_shape(p: &RegulatorSpec) -> Option<RippleShape> {
    if p.rr_curve.len() < 2 {
        return None;
    }
    let mut pts: Vec<(f64, f64, f64)> = p.rr_curve.iter().map(|q| (q[0], q[1], 1.0)).collect();
    pts.push((p.rr_f, p.rr, 10.0));
    let (f0, f1) = (pts.iter().map(|q| q.0).fold(f64::MAX, f64::min), pts.iter().map(|q| q.0).fold(f64::MIN, f64::max));
    let shape = |x: &[f64]| RippleShape { k_lo: 10f64.powf(x[0]), terms: x[1..].chunks(2).map(|c| (10f64.powf(c[0]), 10f64.powf(c[1]))).collect() };
    let mut cost = |x: &[f64]| -> f64 {
        let s = shape(x);
        // the gains no more than the input itself (0 dB), the corners near the graph (else a gain and its corner run away together)
        if s.k_lo + s.terms.iter().map(|t| t.0).sum::<f64>() > 1.0 || s.terms.iter().any(|t| t.1 < f0 / 10.0 || t.1 > f1 * 100.0) {
            return 1e9;
        }
        pts.iter().map(|&(f, db, w)| w * (s.rejection(f) - db).powi(2)).sum()
    };
    let k0 = -p.rr / 20.0;
    let (l0, l1) = (f0.log10(), f1.log10());
    let mut best: Option<(Vec<f64>, f64)> = None;
    // starts: the first corner low or in the middle, the second high; gains a decade above the table's
    for a in [0.25, 0.5] {
        for b in [0.75, 1.0, 1.5] {
            let x0 = [k0, k0 + 1.0, l0 + a * (l1 - l0), -0.5, l0 + b * (l1 - l0)];
            let (x, c) = super::num::nelder_mead(&mut cost, &x0, 6000, 12000, 1e-10, 1e-12);
            if best.as_ref().is_none_or(|b| c < b.1) {
                best = Some((x, c));
            }
        }
    }
    best.map(|(x, _)| shape(&x))
}

/// The `.subckt` (pins: in gnd out, or in adj out).
pub fn regulator_model(p: &RegulatorSpec) -> String {
    let n = &p.common.name;
    let adj = p.adjustable;
    let r = if adj { "adj" } else { "gnd" };
    let head = if adj { format!(".subckt {n} in adj out") } else { format!(".subckt {n} in gnd out") };
    let vin0 = p.vin0.unwrap_or(6.25);
    // the ripple path: flat (k_rr), or k_rr and high-passes of the input fitted to the graph (Q16)
    let mut shape = rr_shape(p);
    let mut k_rr = shape.as_ref().map_or(10f64.powf(-p.rr / 20.0), |s| s.k_lo);
    if adj {
        // the ripple rejection is given at the output set to rr_vout: on the reference it is that much smaller
        let k = p.vout / p.rr_vout.unwrap_or(p.vout);
        k_rr *= k;
        if let Some(s) = shape.as_mut() {
            for t in &mut s.terms {
                t.0 *= k;
            }
        }
    }
    // at DC the input reaches the output by the line regulation; above ~1 Hz only by the ripple rejection
    let k_dc = p.line - k_rr;
    let density = p.noise.density();
    let ro = p.rout;
    let [LimitAt { diff: d1, amps: i1 }, LimitAt { diff: d2, amps: i2 }] = p.ilim;
    let slope = if d2 != d1 { (i2 - i1) / (d2 - d1) } else { 0.0 };
    let mut l: Vec<String> = vec![
        head,
        "* line regulation through a slow path (the data sheet's DC figure) and the ripple from the input (flat, the ripple rejection)".into(),
        format!("Elf lfa {r} in {r} 1"),
        "Rlf lfa lf 1.59Meg".into(),
        format!("Clf lf {r} 100n"),
    ];
    if let Some(sh) = &shape {
        // the ripple rising with frequency: high-passes of the input (their corners fitted to the data sheet's graph, Q16)
        // (from a copy of the input: a capacitor from the pin would push its current into ADJ, which an LM317's R2 turns into output)
        l.push("* the ripple rising with frequency: high-passes of a copy of the input, fitted to the data sheet's ripple rejection graph".into());
        l.push(format!("Erh rhi 0 in {r} 1"));
        for (k, &(_, fc)) in sh.terms.iter().enumerate() {
            l.push(format!("Crh{k} rhi rh{k} {}", g(1.0 / (2.0 * std::f64::consts::PI * fc * 1e3), 6)));
            l.push(format!("Rrh{k} rh{k} 0 1k"));
        }
    }
    l.extend([
        "* output noise (taken as flat over the data sheet's band): a 1 k resistor's noise, scaled".into(),
        format!("Rnz nz {r} 1k"),
    ]);
    if let Some([lo, hi]) = p.vrange {
        // the data sheet's min and max of the output (part to part), for the variation analyses; 0 V otherwise
        l.extend([
            "* the output's difference from part to part (0 V; the variation analyses move it between the data sheet's min and max)".into(),
            format!("Vtol vt {r} DC 0"),
            format!("* spread Vtol {} {} {} (data sheet: {} to {} V)", g(lo - p.vout, 6), g(hi - p.vout, 6), if adj { "vref" } else { "vout" }, g(lo, 6), g(hi, 6)),
        ]);
    }
    l.extend([
        format!(
            "Btgt t0 {r} V={}+{}*(V(lf,{r})-{})+{}*(V(in,{r})-{}){}",
            g(p.vout, 6),
            g(k_dc, 6),
            g(vin0, 6),
            g(k_rr, 6),
            g(vin0, 6),
            if p.vrange.is_some() { format!("+V(vt,{r})") } else { String::new() }
        ) + &p.temp.as_ref().map(|c| format!("+{}*{}", g(p.vout, 6), c.expr())).unwrap_or_default()
            + &shape.as_ref().map(|sh| sh.terms.iter().enumerate().map(|(k, t)| format!("+{}*V(rh{k})", g(t.0, 6))).collect::<String>()).unwrap_or_default(),
        format!("En tgt t0 nz {r} {}", g(density / (K4T * 1e3).sqrt(), 6)),
    ]);
    // the ceiling the input leaves: in − dropout (the dropout grows with the output current; the output resistance already
    // drops ro × I, the dropout's own part is the rest)
    let rdo = (p.rdo - ro).max(0.0);
    l.extend(["* the ceiling the input leaves (the dropout)".into(), format!("Bdo ind {r} V=V(in,{r})-{}-{}*I(Vs)", g(p.vdo0, 6), g(rdo, 6))]);
    // out = min(x, ceiling), softly, the way the earlier bundled models clamped (a high gain into a diode): it converges
    // where smooth formulas did not (the bundled mixed-signal sample's 555)
    let clamp = |name: &str, x: &str, ceiling: &str, out: &str| -> Vec<String> {
        vec![
            format!("Ex{name} xh{name} {r} {x} {ceiling} 1000"),
            format!("D{name} xh{name} yh{name} DREG"),
            format!("Ry{name} yh{name} {r} 1k"),
            format!("Ey{name} y{name} {r} yh{name} {r} 0.001"),
            format!("Ec{name} {out} {r} {x} y{name} 1"),
        ]
    };
    l.push("* the target, no higher than the dropout's ceiling".into());
    l.extend(clamp("1", "tgt", "ind", "c1"));
    // the current limit: the output no higher than the output voltage plus ro times the limit (smaller at a large input −
    // output difference). Compared as currents (amps as volts): the clamp's own offset (its diode's drop / 1000, 0.65 mV)
    // would be 0.5 A through an output resistance of a milliohm, but is 0.65 mA as a current
    let ilim = format!("limit({}+{}*(V(in,{r})-V(out,{r})-{}),{},{})", g(i1, 6), g(slope, 6), g(d1, 6), g(i1.min(i2), 6), g(i1.max(i2), 6));
    l.extend([
        "* the current limit: the current the target asks for (amps as volts), no more than the limit".into(),
        format!("Bu u {r} V=(V(c1,{r})-V(out,{r}))/{}", g(ro, 6)),
        format!("Blim cl {r} V={ilim}"),
    ]);
    l.extend(clamp("2", "u", "cl", "uc"));
    l.extend([
        format!("Bc2 c2 {r} V=V(out,{r})+{}*V(uc,{r})", g(ro, 6)),
        "* output through the load regulation's resistance; the output current comes from the input (both ways, as the earlier".into(),
        "* bundled models did: a dead or reversed input then leaves the output near 0 V)".into(),
        format!("Eo o {r} c2 {r} 1"),
        "Vs o os DC 0".into(),
        format!("Ro os out {}", g(ro, 6)),
        format!("Bin in {r} I=I(Vs)"),
    ]);
    if adj {
        l.extend([
            "* adjustment pin current (out of ADJ)".into(),
            format!("Iadj in adj DC {}", g(p.iadj, 6)),
            format!(
                "* spread Iadj {} {} iadj (data sheet: typ {} uA, max {} uA; no min, so from typ)",
                g(p.iadj, 6),
                g(p.iadj_max, 6),
                g(p.iadj * 1e6, 6),
                g(p.iadj_max * 1e6, 6)
            ),
        ]);
    } else {
        // flat above a few volts of input, like a resistor near 0 V: a constant current would pull on an unpowered input,
        // and a sharp switch-on (step) left the bundled mixed-signal sample without a DC solution at −40 °C
        l.extend(["* ground (quiescent) current: flat above a few volts, like a resistor near 0 V".into(), format!("Bq in gnd I={}*tanh(V(in,gnd)/2)", g(p.iq, 6))]);
    }
    l.extend([".model DREG D(IS=1e-12 N=1)".into(), format!(".ends {n}")]);
    l.join("\n")
}

/// What the model is made from and what it leaves out (`sim.limits`).
pub fn regulator_limits(p: &RegulatorSpec) -> Vec<String> {
    let adj = p.adjustable;
    let density = p.noise.density();
    let out = if adj { format!("基準 {} V (OUT − ADJ)、ADJ の電流 {} µA", repr(p.vout), g(p.iadj * 1e6, 6)) } else { format!("出力 {} V", repr(p.vout)) };
    let at10 = match (adj, p.rr_vout) {
        (true, Some(v)) => format!("、出力 {} V の設定で", g(v, 6)),
        _ => String::new(),
    };
    let mut rows = vec![
        format!("データシートの typ の値から作った等価モデルです (メーカーのモデルではありません)。出典: {}。データシートとの比べは docs/models/regulators.md。", p.cite),
        format!(
            "再現しているもの: {out}、ライン・レギュレーション、ロード・レギュレーション、ドロップアウト、{}電流制限 (入出力の差で変わる)、リップル除去 {} dB ({} Hz{at10}。{})、出力雑音 {} nV/√Hz (平ら)。",
            if adj { String::new() } else { format!("グラウンド電流 {} mA (入力が数 V 以上で)、", g(p.iq * 1e3, 6)) },
            g(p.rr, 6),
            g(p.rr_f, 6),
            match rr_shape(p) {
                // the model's rejection at the decades of the graph
                Some(sh) => format!(
                    "高い周波数では、データシートのグラフに合わせて下がる: {}",
                    [1e3, 1e4, 1e5].iter().filter(|f| **f <= p.rr_curve.iter().map(|q| q[0]).fold(0.0, f64::max) * 1.5).map(|&f| format!("{} kHz で {:.0} dB", g(f / 1e3, 3), sh.rejection(f))).collect::<Vec<_>>().join("、")
                ),
                None => "どの周波数でも同じとした".to_string(),
            },
            g(density * 1e9, 6)
        ),
    ];
    if !p.from_figures.is_empty() {
        rows.push(format!("データシートの表に無い値: {}。", p.from_figures));
    }
    if let Some([lo, hi]) = p.vrange {
        rows.push(if adj {
            format!(
                "部品ごとの違い (基準電圧 {}〜{} V はデータシートの min と max、ADJ の電流 {}〜{} µA は typ〜max。min は無い) は、ばらつき・最悪ケースの解析でだけ入ります (ふだんの解析は typ)。",
                g(lo, 6),
                g(hi, 6),
                g(p.iadj * 1e6, 6),
                g(p.iadj_max * 1e6, 6)
            )
        } else {
            format!("出力電圧の部品ごとの違い ({}〜{} V、データシートの min と max) は、ばらつき・最悪ケースの解析でだけ入ります (ふだんの解析は typ)。", g(lo, 6), g(hi, 6))
        });
    }
    if let Some(c) = &p.temp {
        rows.push(format!(
            "温度による{}の変化 ({}): {}。温度を設定した解析・温度スイープ・自己発熱で効きます (27 ℃ でデータシートの 25 ℃ の値)。",
            if adj { "基準電圧" } else { "出力電圧" },
            c.from,
            c.span()
        ));
    }
    rows.push(format!(
        "扱わないもの: {}{}熱による停止、負荷や入力が急に変わったときの応答、出力の容量による安定性、出力に電流を押し込まれたとき (実物は吸い込めないが、このモデルは出力抵抗を通して吸い込む){}。",
        if p.vrange.is_some() { "" } else { "出力電圧の誤差 (出力はちょうど公称値)、" },
        if p.temp.is_some() { "出力電圧以外の温度による変化、" } else { "温度による変化と" },
        match (adj, p.min_load) {
            (true, Some(i)) => format!("、最小負荷電流 ({} mA)", g(i * 1e3, 6)),
            _ => String::new(),
        }
    ));
    rows.push("最終の確認は、メーカーの公式モデルで行ってください。".into());
    rows
}

pub(super) fn fit_regulator(p: &RegulatorSpec) -> Result<Out, String> {
    check_regulator(p)?;
    Ok((regulator_model(p), regulator_limits(p), vec![], vec![]))
}

/// The part's `sim.params` the model sets (only those the part already has are written): the output (or the
/// reference) and the dropout at 1 A.
pub(super) fn regulator_params(p: &RegulatorSpec) -> Vec<(String, f64)> {
    vec![("vout".into(), p.vout), ("vref".into(), p.vout), ("dropout".into(), p.vdo0 + p.rdo * 1.0)]
}

/// Values the template cannot use, with what to fix.
pub fn check_regulator(p: &RegulatorSpec) -> Result<(), String> {
    for (what, v) in [("出力電圧", p.vout), ("出力抵抗 (ロード・レギュレーション)", p.rout), ("リップル除去の周波数", p.rr_f)] {
        if !(v.is_finite() && v > 0.0) {
            return Err(format!("{what}は 0 より大きい数にしてください"));
        }
    }
    if let Some(c) = &p.temp {
        c.check()?;
    }
    if p.ilim.iter().any(|l| !(l.amps > 0.0)) {
        return Err("電流制限は 0 より大きい電流にしてください (データシートの「ピーク電流」「短絡電流」)".into());
    }
    if let Some([lo, hi]) = p.vrange
        && !(lo <= p.vout && p.vout <= hi)
    {
        return Err("出力電圧の min・max の間に typ が入っていません。データシートの値を確かめてください".into());
    }
    if p.vdo0 < 0.0 || p.rdo < 0.0 || p.iq < 0.0 || p.rr <= 0.0 {
        return Err("ドロップアウト・グラウンド電流は 0 以上、リップル除去は dB の正の数にしてください".into());
    }
    if let Noise::Rms { f_low, f_high, .. } = p.noise
        && f_high <= f_low
    {
        return Err("雑音の帯域は、上の周波数を下の周波数より大きくしてください".into());
    }
    Ok(())
}

/// A shunt reference's typ values (TL431).
#[derive(Clone, Debug, Deserialize)]
pub struct ShuntSpec {
    #[serde(flatten)]
    pub common: super::Common,
    pub cite: String,
    /// Reference voltage at 10 mA (V), with its min and max.
    pub vref: f64,
    pub vref_min: f64,
    pub vref_max: f64,
    /// REF input current (A), and its max (M2: the variation analyses move Rref between typ and max; none: no spread).
    pub iref: f64,
    #[serde(default)]
    pub iref_max: Option<f64>,
    /// ΔVref / ΔVKA (V/V, negative).
    pub dvref_dvka: f64,
    /// Dynamic impedance (Ω).
    pub zka: f64,
    /// Off-state cathode current (A) at `ioff_v` (V).
    pub ioff: f64,
    pub ioff_v: f64,
    /// Minimum cathode current for regulation (A; said in the notes).
    pub imin: f64,
    /// The reference against the temperature (M8); none: the same at every temperature.
    #[serde(default)]
    pub temp: Option<super::TempCurve>,
}

/// The `.subckt` (pins: cat ref an).
pub fn shunt_model(p: &ShuntSpec) -> String {
    let n = &p.common.name;
    let gk = 1.0 / p.zka;
    let w = 0.3e-3;
    // the knee: below it the cathode carries almost nothing; above it gk A/V. At IKA = 10 mA, REF is the data sheet's Vref
    let th = p.vref - 10e-3 / gk;
    let tc = p.temp.as_ref().map(|c| format!("-{}*{}", g(p.vref, 6), c.expr())).unwrap_or_default();
    let x = format!("((V(ref,an)-V(vt,an)-{}-{}*(V(cat,an)-{}){tc})/{})", g(th, 6), g(p.dvref_dvka, 6), g(p.vref, 6), g(w, 6));
    [
        format!(".subckt {n} cat ref an"),
        format!("* reference input current (typ, at {} V)", g(p.vref, 6)),
        format!("Rref ref an {}", g(p.vref / p.iref, 6)),
        match p.iref_max.filter(|m| *m > p.iref) {
            Some(m) => format!("* spread Rref {} {} iref (data sheet: typ {} uA, max {} uA; no min, so from typ)", g(p.vref / m, 6), g(p.vref / p.iref, 6), g(p.iref * 1e6, 6), g(m * 1e6, 6)),
            None => String::new(),
        },
        "* the cathode current rises with REF above the knee (slope 1 / dynamic impedance; a smooth knee 0.3 mV wide); REF moves with".into(),
        "* the cathode voltage by the data sheet's ratio; the cathode needs about 1.2 V".into(),
        format!("Bk cat an I={}*(0.5*({x}+abs({x}))+ln(1+exp(-abs({x}))))*step(V(cat,an)-1.2,0.2)", g(gk * w, 6)),
        format!("* off-state cathode current (typ, at {} V)", g(p.ioff_v, 6)),
        format!("Rleak cat an {}", g(p.ioff_v / p.ioff, 6)),
        "* the reference's difference from part to part (0 V; the variation analyses move it between the data sheet's min and max)".into(),
        "Vtol vt an DC 0".into(),
        format!(
            "* spread Vtol {} {} vref (data sheet: {} to {} V)",
            g(p.vref_min - p.vref, 6),
            g(p.vref_max - p.vref, 6),
            g(p.vref_min, 6),
            g(p.vref_max, 6)
        ),
        format!(".ends {n}"),
    ]
    .into_iter()
    .filter(|l| !l.is_empty())
    .collect::<Vec<_>>()
    .join("\n")
}

pub fn shunt_limits(p: &ShuntSpec) -> Vec<String> {
    vec![
        format!("データシートの typ の値から作った等価モデルです (メーカーのモデルではありません)。出典: {}。データシートとの比べは docs/models/regulators.md。", p.cite),
        format!(
            "再現しているもの: 基準電圧 {} V (IKA = 10 mA)、REF の入力電流 {} µA、ΔVref/ΔVKA −{} mV/V、動的インピーダンス {} Ω、停止中のカソード電流 {} µA。",
            g(p.vref, 6),
            g(p.iref * 1e6, 6),
            g(-p.dvref_dvka * 1e3, 6),
            g(p.zka, 6),
            g(p.ioff * 1e6, 6)
        ),
        match p.iref_max.filter(|m| *m > p.iref) {
            Some(m) => format!("基準電圧の部品ごとの違い ({:.3}〜{:.3} V、データシートの min と max) と REF の入力電流の違い ({}〜{} µA、typ と max) は、ばらつき・最悪ケースの解析でだけ入ります (ふだんの解析は typ)。", p.vref_min, p.vref_max, g(p.iref * 1e6, 6), g(m * 1e6, 6)),
            None => format!("基準電圧の部品ごとの違い ({:.3}〜{:.3} V、データシートの min と max) は、ばらつき・最悪ケースの解析でだけ入ります (ふだんの解析は typ)。", p.vref_min, p.vref_max),
        },
        match &p.temp {
            Some(c) => format!("温度による基準電圧の変化 ({}): {}。温度を設定した解析・温度スイープ・自己発熱で効きます (27 ℃ でデータシートの 25 ℃ の値)。", c.from, c.span()),
            None => String::new(),
        },
        format!(
            "扱わないもの: {}最小カソード電流 ({} mA) より下で安定しなくなること (このモデルは小さな電流でも安定します)、周波数特性と容量負荷での発振、耐圧・最大電流を超えたときの破壊。",
            if p.temp.is_some() { "" } else { "温度による変化、" },
            g(p.imin * 1e3, 6)
        ),
        "最終の確認は、メーカーの公式モデルで行ってください。".into(),
    ]
    .into_iter()
    .filter(|r| !r.is_empty())
    .collect()
}

pub(super) fn fit_shunt(p: &ShuntSpec) -> Result<Out, String> {
    if !(p.vref > 0.0 && p.iref > 0.0 && p.zka > 0.0 && p.ioff > 0.0 && p.ioff_v > 0.0) {
        return Err("基準電圧・REF の電流・動的インピーダンス・停止中の電流は 0 より大きい数にしてください".into());
    }
    if let Some(c) = &p.temp {
        c.check()?;
    }
    if !(p.vref_min <= p.vref && p.vref <= p.vref_max) {
        return Err("基準電圧の min・max の間に typ が入っていません".into());
    }
    Ok((shunt_model(p), shunt_limits(p), vec![], vec![]))
}
