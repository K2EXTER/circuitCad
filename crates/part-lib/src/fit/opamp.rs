//! Op-amp macro models from a data sheet's typ values (M3; the bundled LM358, LM324, TL072, NE5532 and MCP600x are made
//! this way). The template: input (offset, a voltage noise source, bias currents, input resistance) → gain stage (gm
//! limited to the slew rate's current into Cp 1 nF ∥ Rp) → an optional second pole for the phase margin → output swing
//! clamps → output (resistance, current limited to the short-circuit current, taken from the supply pins) + the quiescent
//! current. Was `tools/gen_opamp_models.py`; the text is the same to the character (`tests.rs`). How the bundled ones
//! measure against their data sheets: `docs/models/opamps.md`.

use super::num::g;
use super::{Common, Out};
use serde::Deserialize;

/// The typ values of one op amp (one amplifier), and the min / max the variation analyses spread over.
#[derive(Clone, Debug, Deserialize)]
pub struct OpampSpec {
    #[serde(flatten)]
    pub common: Common,
    /// Where in the data sheet, for the notes ("TI SLOS068AB の 5.7 節").
    pub cite: String,
    /// The description's first part (package, pins, supply range); the summary of the model's values follows it.
    #[serde(default)]
    pub about: String,
    /// Open-loop gain at DC (dB).
    pub aol_db: f64,
    /// Gain-bandwidth product (Hz).
    pub gbw: f64,
    /// Slew rate (V/s).
    pub sr: f64,
    /// Input offset voltage, typ (V; 0 when the data sheet has no typ) and max (either way).
    pub vos: f64,
    pub vos_max: f64,
    /// The input offset voltage's drift with temperature, typ (V/°C, either way: its sign differs from part to part; M8). The
    /// model's drift is 0; the variation analyses move it between ∓ this. None: not in the data sheet.
    #[serde(default)]
    pub vos_drift: Option<f64>,
    /// Input bias current, typ (A; negative out of the pins, as PNP inputs) and max (absolute; none: not given).
    pub ib: f64,
    #[serde(default)]
    pub ib_max: Option<f64>,
    /// Quiescent current per amplifier (A): typ, min (none: not given, a spread then starts at typ), max.
    pub iq: f64,
    #[serde(default)]
    pub iq_min: Option<f64>,
    pub iq_max: f64,
    /// Input resistance as written in the model ("2Meg", "1e12").
    pub rin: String,
    /// How far the output gets from the positive and the negative rail (V).
    pub hu: f64,
    pub hl: f64,
    /// Short-circuit output current (A).
    pub isc: f64,
    /// Input voltage noise at 1 kHz (V/√Hz).
    pub en: f64,
    /// Open-loop output resistance (Ω).
    pub ro: f64,
    /// Phase margin (degrees), when the data sheet gives one: a second pole is added for it.
    #[serde(default)]
    pub pm: Option<f64>,
}

/// 4 k T at the simulator's default 27 °C.
pub(crate) const K4T: f64 = 4.0 * 1.380649e-23 * 300.15;

/// A value with an SI prefix and a unit, 3 digits ("700 kHz", "5 mV", "0 V"): the notes' number format.
pub(crate) fn eng(v: f64, unit: &str) -> String {
    for (k, pre) in [(1e6, "M"), (1e3, "k"), (1.0, ""), (1e-3, "m"), (1e-6, "µ"), (1e-9, "n"), (1e-12, "p")] {
        if v.abs() >= k {
            return format!("{} {pre}{unit}", g(v / k, 3));
        }
    }
    format!("0 {unit}")
}

fn bias_spread(p: &OpampSpec) -> Vec<String> {
    let Some(max) = p.ib_max else { return vec![] };
    let sign = if p.ib < 0.0 { -1.0 } else { 1.0 };
    let note = format!("(data sheet: typ {}, max {}; no min, so from typ)", eng(p.ib.abs(), "A"), eng(max, "A")).replace('µ', "u");
    [("Ibp", "ib+"), ("Ibn", "ib-")].iter().map(|(el, what)| format!("* spread {el} {} {} {what} {note}", g(p.ib, 6), g(sign * max, 6))).collect()
}

fn iq_spread(p: &OpampSpec) -> String {
    let lo = p.iq_min.unwrap_or(p.iq);
    let note = match p.iq_min {
        Some(min) => format!("{} to {}", eng(min, "A"), eng(p.iq_max, "A")),
        None => format!("typ {}, max {}; no min, so from typ", eng(p.iq, "A"), eng(p.iq_max, "A")),
    };
    format!("* spread Iq {} {} iq (data sheet: {note})", g(lo, 6), g(p.iq_max, 6)).replace('µ', "u")
}

/// The `.subckt` (pins: inp inn out vp vn).
pub fn model(p: &OpampSpec) -> String {
    let n = &p.common.name;
    let cp = 1e-9;
    let mut gm = 2.0 * std::f64::consts::PI * p.gbw * cp;
    if let Some(pm) = p.pm {
        // with the second pole the gain at gbw is sin(pm) of the one-pole gain: raise gm so that unity stays at gbw
        gm /= pm.to_radians().sin();
    }
    let rp = 10f64.powf(p.aol_db / 20.0) / gm;
    let imax = p.sr * cp;
    // the noise of a 1 kΩ resistor, scaled to the data sheet's input voltage noise at 1 kHz
    let en_gain = p.en / (K4T * 1e3).sqrt();
    let mut l: Vec<String> = vec![
        format!(".subckt {n} inp inn out vp vn"),
        "* input: offset (data sheet typ), voltage noise, bias currents (+ = into the pin), resistance".into(),
        format!("Vos inp i1 DC {}", g(p.vos, 6)),
        format!("* spread Vos {} {} offset (data sheet: max {} mV either way)", g(-p.vos_max, 6), g(p.vos_max, 6), g(p.vos_max * 1e3, 6)),
    ];
    match p.vos_drift {
        None => l.push(format!("En i1 i2 nz 0 {}", g(en_gain, 6))),
        Some(d) => {
            // the offset's drift rides on the noise source (no node of its own: every node leaks gmin, picoamps at the input):
            // 0 V/°C as made (its sign differs from part to part), V(dr) volts per °C away from 27 °C
            l.extend([
                "* the offset's drift with temperature, on the noise source (0 V/degree as made; the variation analyses move it".into(),
                "* between the data sheet's typ either way): V(dr) volts per degree away from 27 C".into(),
                format!("Ben i1 i2 V={}*V(nz)+V(dr)*(temp-{})", g(en_gain, 6), g(super::T_MODEL, 6)),
                "Vdr dr 0 DC 0".into(),
                format!("* spread Vdr {} {} offset-drift (data sheet: typ {} uV/C either way)", g(-d, 6), g(d, 6), g(d * 1e6, 6)),
            ]);
        }
    }
    l.push("Rnz nz 0 1k".into());
    l.extend([
        format!("Ibp inp vn DC {}", g(p.ib, 6)),
        format!("Ibn inn vn DC {}", g(p.ib, 6)),
    ]);
    l.extend(bias_spread(p));
    l.extend([
        format!("Rin inp inn {}", p.rin),
        "* gain stage: gm into Cp || Rp; the current is limited to SR * Cp (slew rate)".into(),
        format!("Bgm 0 n0 I=limit({}*V(i2,inn),{},{})", g(gm, 6), g(-imax, 6), g(imax, 6)),
        format!("Rp n0 0 {}", g(rp, 6)),
        format!("Cp n0 0 {}", g(cp, 6)),
    ]);
    let mut src = "n0";
    if let Some(pm) = p.pm {
        // a second pole that leaves the data sheet's phase margin at the unity-gain frequency
        let f2 = p.gbw / (90.0 - pm).to_radians().tan();
        l.extend([
            "* second pole (phase margin)".into(),
            "E2 p2a 0 n0 0 1".into(),
            "R2 p2a p2 1k".into(),
            format!("C2 p2 0 {}", g(1.0 / (2.0 * std::f64::consts::PI * 1e3 * f2), 6)),
        ]);
        src = "p2";
    }
    let lim = format!("limit((V(n)-V(out))/{},{},{})", g(p.ro, 6), g(-p.isc, 6), g(p.isc, 6));
    l.extend([
        "* output swing: clamped hu below vp and hl above vn".into(),
        "Eu u 0 vp 0 1".into(),
        format!("Vhu u uh DC {}", g(p.hu, 6)),
        format!("Ex1 x1 0 {src} uh 1000"),
        "D1 x1 y1 DLIM".into(),
        "Ry1 y1 0 1k".into(),
        "Ey1 y1k 0 y1 0 0.001".into(),
        format!("Ec1 n1 0 {src} y1k 1"),
        "El l 0 vn 0 1".into(),
        "Vtop u ut DC -0.5".into(),
        "Dtop n0 ut DLIM".into(),
        "Vbot l lb DC 0.5".into(),
        "Dbot lb n0 DLIM".into(),
        format!("Vhl lh l DC {}", g(p.hl, 6)),
        "Ex2 x2 0 lh n1 1000".into(),
        "D2 x2 y2 DLIM".into(),
        "Ry2 y2 0 1k".into(),
        "Ey2 y2k 0 0 y2 0.001".into(),
        "Ec2 n 0 n1 y2k 1".into(),
        "* output: ro, current limited to the short-circuit current, taken from vp (source) and returned to vn (sink)".into(),
        // the output current, split smoothly (step) so that the sum is it with a smooth slope at 0
        format!("Bsrc vp out I={lim}*step({lim},1u)"),
        format!("Bsnk out vn I=-{lim}*step(-{lim},1u)"),
        "* quiescent current of one amplifier".into(),
        format!("Iq vp vn DC {}", g(p.iq, 6)),
        iq_spread(p),
        ".model DLIM D(IS=1e-12 N=1)".into(),
        format!(".ends {n}"),
    ]);
    l.join("\n")
}

/// The model's values in a line, for the description and the notes.
pub fn summary(p: &OpampSpec) -> String {
    let mut s = format!("開ループ利得 {:.0} dB、GBW {}", p.aol_db, eng(p.gbw, "Hz"));
    if let Some(pm) = p.pm {
        s += &format!("(位相余裕 {}°)", g(pm, 6));
    }
    s + &format!("、スルーレート {} V/µs、電源電流 {}/回路", g(p.sr / 1e6, 3), eng(p.iq, "A"))
}

fn spreads(p: &OpampSpec) -> String {
    let mut s = format!("オフセット ±{}", eng(p.vos_max, "V"));
    if let Some(max) = p.ib_max {
        s += &format!("、バイアス電流 {}〜{} (入力ごと)", eng(p.ib.abs(), "A"), eng(max, "A"));
    }
    s += &format!("、電源電流 {}〜{}", eng(p.iq_min.unwrap_or(p.iq), "A"), eng(p.iq_max, "A"));
    s + if p.iq_min.is_none() || p.ib_max.is_some() { "。データシートの max、min が無いものは typ から" } else { "。データシートの min〜max" }
}

/// What the model is made from and what it leaves out (`sim.limits`).
pub fn limits(p: &OpampSpec) -> Vec<String> {
    let vos = if p.vos != 0.0 { format!("入力オフセット {}", eng(p.vos, "V")) } else { "入力オフセット 0 (データシートに typ が無い)".into() };
    vec![
        format!("データシートの typ の値から作った等価モデルです (メーカーのモデルではありません)。出典: {}。データシートとの比べは docs/models/opamps.md。", p.cite),
        format!(
            "再現しているもの: {}、{vos}、入力バイアス電流 {}、出力の振れ幅 (電源から {} / {})、出力の短絡電流 {}、1 kHz の入力換算雑音 {}/√Hz。部品ごとの違い ({}) は、ばらつき・最悪ケースの解析でだけ入ります (ふだんの解析は typ)。",
            summary(p),
            eng(p.ib.abs(), "A"),
            eng(p.hu, "V"),
            eng(p.hl, "V"),
            eng(p.isc, "A"),
            eng(p.en, "V"),
            spreads(p)
        ),
        match p.vos_drift {
            Some(d) => format!(
                "入力オフセットの温度ドリフト (データシートの typ ±{} µV/℃): 向きが部品ごとに違うので、ふだんの解析では 0 です。ばらつき・最悪ケースの解析で ±{} µV/℃ の間で動き、温度を 27 ℃ から変えたときに効きます。",
                g(d * 1e6, 6),
                g(d * 1e6, 6)
            ),
            None => String::new(),
        },
        format!(
            "扱わないもの: 入力できる電圧の範囲 (超えたときの動き)、CMRR と PSRR、1/f 雑音 (低い周波数でも 1 kHz と同じ雑音)、{}、電源電圧による GBW の変化、{}チャネル2。",
            if p.vos_drift.is_some() { "オフセット以外の温度による変化" } else { "温度による変化" },
            if p.ib_max.is_none() { "バイアス電流のばらつき (データシートに max が無いので typ の値で固定)、" } else { "" }
        ),
        super::LAST_WORD.into(),
        "電源ピンをつながないと、出力は 0V に張り付きます。".into(),
    ]
    .into_iter()
    .filter(|r| !r.is_empty())
    .collect()
}

pub(super) fn fit(p: &OpampSpec) -> Result<Out, String> {
    check(p)?;
    Ok((model(p), limits(p), vec![], vec![]))
}

/// The description: the part's own words, then the model's values.
pub(super) fn description(p: &OpampSpec) -> String {
    format!("{}{}。データシートから作った等価モデル", p.about, summary(p))
}

/// Values the template cannot use, with what to fix.
pub fn check(p: &OpampSpec) -> Result<(), String> {
    let positive = [("GBW", p.gbw), ("スルーレート", p.sr), ("電源電流", p.iq), ("短絡電流", p.isc), ("出力抵抗", p.ro)];
    for (what, v) in positive {
        if !(v.is_finite() && v > 0.0) {
            return Err(format!("{what}は 0 より大きい数にしてください (データシートの typ)"));
        }
    }
    if !(p.aol_db.is_finite() && p.aol_db > 20.0) {
        return Err("開ループ利得は dB で入れてください (多くは 80〜140 dB。V/mV の表なら 20·log10(値 × 1000))".into());
    }
    if p.iq_max < p.iq || p.vos_max < p.vos.abs() {
        return Err("max が typ より小さくなっています。データシートの max を確かめてください".into());
    }
    if let Some(pm) = p.pm
        && !(1.0..90.0).contains(&pm)
    {
        return Err("位相余裕は 1〜89° の間で入れてください (分からなければ空に)".into());
    }
    if p.hu < 0.0 || p.hl < 0.0 || p.en < 0.0 {
        return Err("出力の振れ幅と雑音は 0 以上にしてください".into());
    }
    Ok(())
}
