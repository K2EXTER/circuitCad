//! LDOs whose output follows a SET pin driven by a precision current source (P6: ADI LT3045; the same architecture as the
//! LT3042 and LT3080). The model: the SET current (with its line regulation, its part-to-part spread and its noise current)
//! into the user's RSET and CSET, so that the output, the soft start (2.3 × RSET × CSET) and the noise follow the resistor
//! and the capacitor as on the real part; a unity-gain buffer from SET to OUTS (its offset, its own noise, the ripple
//! through a shelf and high-passes fitted to the PSRR table); the pass device from IN to OUT, a transconductance (the load
//! regulation) no more than the dropout allows and the internal current limit (with its foldback), cut back when the ILIM
//! pin (which sources the output current / ratio) reaches its threshold; EN/UV; PG pulled low while PGFB is below its
//! threshold, and the fast start-up current into SET meanwhile. Details: `docs/code-notes/part-lib.md`.

use super::num::g;
use super::opamp::K4T;
use super::reference::psrr_shape;
use super::regulator::{LimitAt, Noise};
use super::{Out, Row};
use serde::Deserialize;

/// The ILIM pin: sources the output current / `ratio`; the current limit acts when it reaches `threshold` (V) on the
/// user's resistor to ground (LT3045: 1:500, 300 mV, so 150 mA·kΩ).
#[derive(Clone, Debug, Deserialize)]
pub struct IlimPin {
    pub ratio: f64,
    pub threshold: f64,
}

/// The PG and PGFB pins: PG (open collector, `r_on` Ω) pulls low while PGFB is below `threshold` (V); meanwhile `fast` (A)
/// more flows into SET (the fast start-up; 0: none).
#[derive(Clone, Debug, Deserialize)]
pub struct PowerGood {
    pub threshold: f64,
    pub r_on: f64,
    #[serde(default)]
    pub fast: f64,
}

/// A SET-pin LDO's typ values.
#[derive(Clone, Debug, Deserialize)]
pub struct SetLdoSpec {
    #[serde(flatten)]
    pub common: super::Common,
    pub cite: String,
    #[serde(default)]
    pub from_figures: String,
    /// The SET pin current (A), with its min and max (part to part, for the variation analyses).
    pub iset: f64,
    #[serde(default)]
    pub iset_range: Option<[f64; 2]>,
    /// The output offset VOUT − VSET: its min and max (V; typ 0).
    #[serde(default)]
    pub vos_range: Option<[f64; 2]>,
    /// The input the table's values are at (V).
    pub vin0: f64,
    /// Line regulation of the SET current (A/V) and of the offset (V/V).
    #[serde(default)]
    pub line_iset: f64,
    #[serde(default)]
    pub line_vos: f64,
    /// Load regulation: the offset's change over the table's load step, as a resistance (Ω).
    pub rout: f64,
    /// Dropout: vdo0 + rdo × I (V, Ω).
    pub vdo0: f64,
    pub rdo: f64,
    /// The minimum input (V; the UVLO's rising threshold): below it the part is off.
    pub vin_min: f64,
    /// Quiescent current (A) and its growth with the output current (A per A: the GND pin current at full load), and in
    /// shutdown (A).
    pub iq: f64,
    #[serde(default)]
    pub iq_per_a: f64,
    #[serde(default)]
    pub iq_off: f64,
    /// The internal current limit at two input − output differences (the foldback), joined by a straight line.
    pub ilim: [LimitAt; 2],
    /// The EN/UV pin's turn-on threshold (V); none: no EN pin.
    #[serde(default)]
    pub en_threshold: Option<f64>,
    #[serde(default)]
    pub ilim_pin: Option<IlimPin>,
    #[serde(default)]
    pub pg: Option<PowerGood>,
    /// Ripple rejection (Hz, dB): fitted as `reference::psrr_shape`.
    pub psrr: Vec<[f64; 2]>,
    /// The buffer's own output noise (taken as flat; the data sheet's figure with a large CSET).
    pub noise: Noise,
    /// The SET current's noise (rms A over a band, or a density A/√Hz).
    pub iset_noise: Noise,
    /// The SET current against the temperature (M8; the output follows it through RSET), and the quiescent current's: relative
    /// to 25 °C, read off the data sheet's figures. None: the same at every temperature.
    #[serde(default)]
    pub iset_temp: Option<super::TempCurve>,
    #[serde(default)]
    pub iq_temp: Option<super::TempCurve>,
}

/// The pins, in the order of `sim.pins`: in gnd out outs set, then en, pg pgfb, ilim when the part has them.
fn pin_list(p: &SetLdoSpec) -> String {
    let mut s = "in gnd out outs set".to_string();
    if p.en_threshold.is_some() {
        s += " en";
    }
    if p.pg.is_some() {
        s += " pg pgfb";
    }
    if p.ilim_pin.is_some() {
        s += " ilim";
    }
    s
}

/// The `.subckt`.
pub fn setldo_model(p: &SetLdoSpec) -> String {
    let n = &p.common.name;
    let sh = psrr_shape(&p.psrr);
    let k_dc = p.line_vos - sh.k_lo;
    // the gain on a 1 k resistor's noise that gives the density d (V/√Hz)
    let nz = |d: f64| g(d / (K4T * 1e3).sqrt(), 6);
    let mut l = vec![format!(".subckt {n} {}", pin_list(p))];
    // on: the input above its minimum and EN above its threshold
    let en = p.en_threshold.map(|t| format!("*step(V(en,gnd)-{},0.01)", g(t, 6))).unwrap_or_default();
    l.push(format!("* on: the input above its minimum (UVLO){}", if p.en_threshold.is_some() { " and EN/UV above its threshold" } else { "" }));
    l.push(format!("Bon on gnd V=step(V(in,gnd)-{},0.05){en}", g(p.vin_min, 6)));
    if p.en_threshold.is_some() {
        l.push("Ren en gnd 100Meg".into());
    }
    // the SET current: its value, its spread (a 0 V node the variation analyses move, read as amps), its line regulation,
    // and its noise (a 1 k resistor's noise into a transconductance)
    l.push("* the SET current (into the user's RSET and CSET)".into());
    let spread = match p.iset_range {
        Some([lo, hi]) => {
            l.extend([
                // a 0 V source read as amps (a current source into a resistor would put the resistor's noise on SET)
                "* the SET current's difference from part to part (0 V read as amps; the variation analyses move it between the data sheet's min and max)".into(),
                "Vsd isd gnd DC 0".into(),
                format!("* spread Vsd {} {} iset (data sheet: {} to {} uA)", g(lo - p.iset, 6), g(hi - p.iset, 6), g(lo * 1e6, 6), g(hi * 1e6, 6)),
            ]);
            "+V(isd,gnd)"
        }
        None => "",
    };
    let iset_tc = p.iset_temp.as_ref().map(|c| format!("+{}*{}", g(p.iset, 6), c.expr())).unwrap_or_default();
    if p.iset_temp.is_some() {
        l.push("* (with its change with the temperature, 0 at 27 C)".into());
    }
    l.push(format!("Bset in set I=V(on,gnd)*({}{spread}{iset_tc}+{}*(V(in,gnd)-{}))", g(p.iset, 6), g(p.line_iset, 6), g(p.vin0, 6)));
    let di = p.iset_noise.density();
    if di > 0.0 {
        l.extend(["Rnzi nzi gnd 1k".into(), format!("Gni gnd set nzi gnd {}", g(di / (K4T * 1e3).sqrt(), 6))]);
    }
    if let Some(pg) = &p.pg
        && pg.fast > 0.0
    {
        l.extend([
            "* fast start-up: more current into SET while PGFB is below the power-good threshold".into(),
            format!("Bfs in set I=V(on,gnd)*{}*step({}-V(pgfb,gnd),0.005)", g(pg.fast, 6), g(pg.threshold, 6)),
        ]);
    }
    l.extend(["* line regulation through a slow path (the data sheet's DC figure)".into(), "Elf lfa gnd in gnd 1".into(), "Rlf lfa lf 1.59Meg".into(), "Clf lf gnd 100n".into()]);
    let (hp, hp_sum) = sh.lines("in", "gnd");
    l.extend(hp);
    let vos = match p.vos_range {
        Some([lo, hi]) => {
            l.extend([
                "* the offset VOUT - VSET from part to part (0 V; the variation analyses move it between the data sheet's min and max)".into(),
                "Vvos vos gnd DC 0".into(),
                format!("* spread Vvos {} {} offset (data sheet: {} to {} mV)", g(lo, 6), g(hi, 6), g(lo * 1e3, 6), g(hi * 1e3, 6)),
            ]);
            "+V(vos,gnd)"
        }
        None => "",
    };
    l.extend([
        "* the buffer's target: SET, its offset, the line regulation and the ripple, with the buffer's own noise".into(),
        format!(
            "Btgt t0 gnd V=V(set,gnd){vos}+{}*(V(lf,gnd)-{})+{}*(V(in,gnd)-{}){hp_sum}",
            g(k_dc, 6),
            g(p.vin0, 6),
            g(sh.k_lo, 6),
            g(p.vin0, 6)
        ),
        "Rnz nz gnd 1k".into(),
        format!("En tgt t0 nz gnd {}", nz(p.noise.density())),
        "* OUTS senses the output (a large resistor to OUT keeps it defined if left open)".into(),
        "Routs outs out 1Meg".into(),
    ]);
    // the pass device: the load regulation's transconductance, no more than the dropout and the internal current limit allow,
    // cut back when ILIM reaches its threshold
    let [LimitAt { diff: d1, amps: i1 }, LimitAt { diff: d2, amps: i2 }] = p.ilim;
    let slope = if d2 != d1 { (i2 - i1) / (d2 - d1) } else { 0.0 };
    let x = format!(
        "min(min((V(tgt,gnd)-V(outs,gnd))/{},(V(in,gnd)-V(out,gnd)-{})/{}),limit({}+{}*(V(in,gnd)-V(out,gnd)-{}),{},{}))",
        g(p.rout, 6),
        g(p.vdo0, 6),
        g(p.rdo.max(1e-3), 6),
        g(i1, 6),
        g(slope, 6),
        g(d1, 6),
        g(i1.min(i2), 6),
        g(i1.max(i2), 6)
    );
    let cut = p.ilim_pin.as_ref().map(|il| format!("*limit(1+({}-V(ilim,gnd))*1000,0,1)", g(il.threshold, 6))).unwrap_or_default();
    l.extend([
        "* the pass device: what the buffer asks for (the load regulation), no more than the dropout and the internal current".into(),
        format!("* limit (folding back at large input - output differences) allow{}; only from IN to OUT", if p.ilim_pin.is_some() { ", cut back when ILIM reaches its threshold" } else { "" }),
        format!("Bpass in op I=V(on,gnd)*max({x},0){cut}"),
        "Vs op out DC 0".into(),
    ]);
    if let Some(il) = &p.ilim_pin {
        l.extend([format!("* ILIM sources the output current / {} (a current monitor)", g(il.ratio, 6)), format!("Bil in ilim I=I(Vs)/{}", g(il.ratio, 6))]);
    }
    l.extend([
        format!("* quiescent current: grows with the output current{}; a little in shutdown", if p.iq_temp.is_some() { " and with the temperature" } else { "" }),
        format!(
            "Bq in gnd I=V(on,gnd)*({}{}+{}*I(Vs))+{}*tanh(V(in,gnd)/2)",
            g(p.iq, 6),
            p.iq_temp.as_ref().map(|c| format!("*(1+{})", c.expr())).unwrap_or_default(),
            g(p.iq_per_a, 6),
            g(p.iq_off, 6)
        ),
    ]);
    if let Some(pg) = &p.pg {
        l.extend([
            "* PG: open collector, low while PGFB is below the threshold (and the part is on); PGFB itself draws next to nothing".into(),
            format!("Bpg pg gnd I=V(on,gnd)*step({}-V(pgfb,gnd),0.005)*V(pg,gnd)/{}", g(pg.threshold, 6), g(pg.r_on, 6)),
            "Rpgfb pgfb gnd 1G".into(),
        ]);
    }
    l.push(format!(".ends {n}"));
    l.join("\n")
}

fn hz(f: f64) -> String {
    if f >= 1e6 {
        format!("{} MHz", g(f / 1e6, 3))
    } else if f >= 1e3 {
        format!("{} kHz", g(f / 1e3, 3))
    } else {
        format!("{} Hz", g(f, 3))
    }
}

/// What the model is made from and what it leaves out (`sim.limits`).
pub fn setldo_limits(p: &SetLdoSpec) -> Vec<String> {
    let sh = psrr_shape(&p.psrr);
    let mut rows = vec![
        format!("データシートの typ の値から作った等価モデルです (メーカーのモデルではありません)。出典: {}。", p.cite),
        format!(
            "再現しているもの: SET ピンの電流 {} µA (出力 = 電流 × RSET、起動は RSET と CSET の時定数)、出力のオフセット (VOUT − VSET) 0 V、ロード・レギュレーション {} mΩ、ドロップアウト ({} V + {} Ω × 電流)、内部の電流制限 ({} V の差で {} A、{} V で {} A。差が大きいと下げる)、電源電流 ({} mA + 出力電流の {} %、停止中 {} µA)、最小の入力 {} V、PSRR ({})、雑音 (増幅器 {} nV/√Hz、SET の電流 {} pA/√Hz。どちらも平らとした)。",
            g(p.iset * 1e6, 6),
            g(p.rout * 1e3, 4),
            g(p.vdo0, 6),
            g(p.rdo, 6),
            g(p.ilim[0].diff, 6),
            g(p.ilim[0].amps, 6),
            g(p.ilim[1].diff, 6),
            g(p.ilim[1].amps, 6),
            g(p.iq * 1e3, 6),
            g(p.iq_per_a * 100.0, 4),
            g(p.iq_off * 1e6, 6),
            g(p.vin_min, 6),
            p.psrr.iter().map(|q| format!("{} で {:.0} dB", hz(q[0]), sh.rejection(q[0]))).collect::<Vec<_>>().join("、"),
            g(p.noise.density() * 1e9, 3),
            g(p.iset_noise.density() * 1e12, 3)
        ),
    ];
    let mut pins = vec![];
    if let Some(t) = p.en_threshold {
        pins.push(format!("EN/UV ({} V で入る。ヒステリシスは扱わない)", g(t, 6)));
    }
    if let Some(pg) = &p.pg {
        pins.push(format!(
            "PG (PGFB が {} mV より下の間は低く引く{})",
            g(pg.threshold * 1e3, 6),
            if pg.fast > 0.0 { format!("。その間は SET に {} mA 多く流して速く起動する", g(pg.fast * 1e3, 6)) } else { String::new() }
        ));
    }
    if let Some(il) = &p.ilim_pin {
        pins.push(format!(
            "ILIM (出力電流の 1/{} を流し出す。GND との抵抗で {} V になると電流を制限: {} mA·kΩ ÷ 抵抗)",
            g(il.ratio, 6),
            g(il.threshold, 6),
            g(il.ratio * il.threshold, 6)
        ));
    }
    if !pins.is_empty() {
        rows.push(format!("ピン: {}。", pins.join("、")));
    }
    if !p.from_figures.is_empty() {
        rows.push(format!("データシートの表に無い値: {}。", p.from_figures));
    }
    let mut spread = vec![];
    if let Some([lo, hi]) = p.iset_range {
        spread.push(format!("SET の電流 {}〜{} µA", g(lo * 1e6, 6), g(hi * 1e6, 6)));
    }
    if let Some([lo, hi]) = p.vos_range {
        spread.push(format!("オフセット {}〜{} mV", g(lo * 1e3, 6), g(hi * 1e3, 6)));
    }
    if !spread.is_empty() {
        rows.push(format!("部品ごとの違い ({}、データシートの min と max) は、ばらつき・最悪ケースの解析でだけ入ります (ふだんの解析は typ)。", spread.join("・")));
    }
    let mut temps = vec![];
    if let Some(c) = &p.iset_temp {
        temps.push(format!("SET の電流 ({}。出力も同じ割合で動く。{})", c.span(), c.from));
    }
    if let Some(c) = &p.iq_temp {
        temps.push(format!("電源電流 ({}。{})", c.span(), c.from));
    }
    if !temps.is_empty() {
        rows.push(format!("温度による変化: {}。27 ℃ では表の typ のままです。", temps.join("、")));
    }
    rows.push(format!("扱わないもの: {}熱による停止、1/f 雑音、逆の電圧の保護、出力のコンデンサによる安定性と負荷の急変の応答、EN/UV と PGFB のヒステリシス、ドロップアウトや電流制限の間も速い起動が続くこと (実物は止める)。", if temps.is_empty() { "温度による変化と" } else { "ほかの温度による変化 (ドロップアウト・電流制限など) と" }));
    rows.push("最終の確認は、メーカーの公式モデルで行ってください。".into());
    rows
}

/// Values the template cannot use, with what to fix.
pub fn check_setldo(p: &SetLdoSpec) -> Result<(), String> {
    for c in [&p.iset_temp, &p.iq_temp].into_iter().flatten() {
        c.check()?;
    }
    for (what, v) in [("SET ピンの電流", p.iset), ("出力抵抗 (ロード・レギュレーション)", p.rout), ("最小の入力電圧", p.vin_min)] {
        if !(v.is_finite() && v > 0.0) {
            return Err(format!("{what}は 0 より大きい数にしてください"));
        }
    }
    if p.ilim.iter().any(|l| !(l.amps > 0.0)) {
        return Err("内部の電流制限は 0 より大きい電流にしてください".into());
    }
    if p.psrr.is_empty() || p.psrr.iter().any(|q| !(q[0] > 0.0 && q[1] > 0.0)) {
        return Err("PSRR は (周波数 Hz, dB) の点を 1 つ以上、どちらも正の数で".into());
    }
    if let Some([lo, hi]) = p.iset_range
        && !(lo <= p.iset && p.iset <= hi)
    {
        return Err("SET ピンの電流の min・max の間に typ が入っていません".into());
    }
    if let Some([lo, hi]) = p.vos_range
        && !(lo <= 0.0 && 0.0 <= hi)
    {
        return Err("オフセットの min・max は 0 をはさむ値にしてください".into());
    }
    if p.vdo0 < 0.0 || p.rdo < 0.0 || p.iq < 0.0 || p.iq_per_a < 0.0 || p.iq_off < 0.0 {
        return Err("ドロップアウト・電源電流は 0 以上にしてください".into());
    }
    if let Some(il) = &p.ilim_pin
        && !(il.ratio > 0.0 && il.threshold > 0.0)
    {
        return Err("ILIM ピンの比としきい値は 0 より大きくしてください".into());
    }
    if let Some(pg) = &p.pg
        && !(pg.threshold > 0.0 && pg.r_on > 0.0 && pg.fast >= 0.0)
    {
        return Err("PG のしきい値と引く抵抗は 0 より大きく、速い起動の電流は 0 以上にしてください".into());
    }
    for n in [&p.noise, &p.iset_noise] {
        if let Noise::Rms { f_low, f_high, .. } = *n
            && f_high <= f_low
        {
            return Err("雑音の帯域は、上の周波数を下の周波数より大きくしてください".into());
        }
    }
    Ok(())
}

pub(super) fn fit_setldo(p: &SetLdoSpec) -> Result<Out, String> {
    check_setldo(p)?;
    let sh = psrr_shape(&p.psrr);
    let rows: Vec<Row> = p.psrr.iter().map(|q| super::row("PSRR", hz(q[0]), q[1], sh.rejection(q[0]), "dB")).collect();
    Ok((setldo_model(p), setldo_limits(p), rows, vec![]))
}
