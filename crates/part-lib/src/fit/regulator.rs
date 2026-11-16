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
}

/// Python's `repr` of a float: the shortest that reads back, with ".0" on whole numbers ("5.0", "1.25").
fn repr(x: f64) -> String {
    format!("{x:?}")
}

/// The `.subckt` (pins: in gnd out, or in adj out).
pub fn regulator_model(p: &RegulatorSpec) -> String {
    let n = &p.common.name;
    let adj = p.adjustable;
    let r = if adj { "adj" } else { "gnd" };
    let head = if adj { format!(".subckt {n} in adj out") } else { format!(".subckt {n} in gnd out") };
    let vin0 = p.vin0.unwrap_or(6.25);
    let mut k_rr = 10f64.powf(-p.rr / 20.0);
    if adj {
        // the ripple rejection is given at the output set to rr_vout: on the reference it is that much smaller
        k_rr *= p.vout / p.rr_vout.unwrap_or(p.vout);
    }
    // at DC the input reaches the output by the line regulation; above ~1 Hz only by the (flat) ripple rejection
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
        "* output noise (taken as flat over the data sheet's band): a 1 k resistor's noise, scaled".into(),
        format!("Rnz nz {r} 1k"),
    ];
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
        ),
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
            "再現しているもの: {out}、ライン・レギュレーション、ロード・レギュレーション、ドロップアウト、{}電流制限 (入出力の差で変わる)、リップル除去 {} dB ({} Hz{at10}。どの周波数でも同じとした)、出力雑音 {} nV/√Hz (平ら)。",
            if adj { String::new() } else { format!("グラウンド電流 {} mA (入力が数 V 以上で)、", g(p.iq * 1e3, 6)) },
            g(p.rr, 6),
            g(p.rr_f, 6),
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
    rows.push(format!(
        "扱わないもの: {}温度による変化と熱による停止、負荷や入力が急に変わったときの応答、出力の容量による安定性、出力に電流を押し込まれたとき (実物は吸い込めないが、このモデルは出力抵抗を通して吸い込む){}。",
        if p.vrange.is_some() { "" } else { "出力電圧の誤差 (出力はちょうど公称値)、" },
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
    /// REF input current (A).
    pub iref: f64,
    /// ΔVref / ΔVKA (V/V, negative).
    pub dvref_dvka: f64,
    /// Dynamic impedance (Ω).
    pub zka: f64,
    /// Off-state cathode current (A) at `ioff_v` (V).
    pub ioff: f64,
    pub ioff_v: f64,
    /// Minimum cathode current for regulation (A; said in the notes).
    pub imin: f64,
}

/// The `.subckt` (pins: cat ref an).
pub fn shunt_model(p: &ShuntSpec) -> String {
    let n = &p.common.name;
    let gk = 1.0 / p.zka;
    let w = 0.3e-3;
    // the knee: below it the cathode carries almost nothing; above it gk A/V. At IKA = 10 mA, REF is the data sheet's Vref
    let th = p.vref - 10e-3 / gk;
    let x = format!("((V(ref,an)-V(vt,an)-{}-{}*(V(cat,an)-{}))/{})", g(th, 6), g(p.dvref_dvka, 6), g(p.vref, 6), g(w, 6));
    [
        format!(".subckt {n} cat ref an"),
        format!("* reference input current (typ, at {} V)", g(p.vref, 6)),
        format!("Rref ref an {}", g(p.vref / p.iref, 6)),
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
        format!("基準電圧の部品ごとの違い ({:.3}〜{:.3} V、データシートの min と max) は、ばらつき・最悪ケースの解析でだけ入ります (ふだんの解析は typ)。", p.vref_min, p.vref_max),
        format!(
            "扱わないもの: 温度による変化、最小カソード電流 ({} mA) より下で安定しなくなること (このモデルは小さな電流でも安定します)、周波数特性と容量負荷での発振、耐圧・最大電流を超えたときの破壊。",
            g(p.imin * 1e3, 6)
        ),
        "最終の確認は、メーカーの公式モデルで行ってください。".into(),
    ]
}

pub(super) fn fit_shunt(p: &ShuntSpec) -> Result<Out, String> {
    if !(p.vref > 0.0 && p.iref > 0.0 && p.zka > 0.0 && p.ioff > 0.0 && p.ioff_v > 0.0) {
        return Err("基準電圧・REF の電流・動的インピーダンス・停止中の電流は 0 より大きい数にしてください".into());
    }
    if !(p.vref_min <= p.vref && p.vref <= p.vref_max) {
        return Err("基準電圧の min・max の間に typ が入っていません".into());
    }
    Ok((shunt_model(p), shunt_limits(p), vec![], vec![]))
}
