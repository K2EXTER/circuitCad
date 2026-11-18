//! Comparator macro models from a data sheet's typ values (the bundled LM393 and LM339 are made this way). The template:
//! input (offset, bias currents) → a decision stage (gm into Cp ∥ Rp, its current limited, the node clamped to ±1 V) →
//! an open-collector output that sinks when IN− is above IN+ (`tanh`: the table's VOL at its current, at most the sink
//! current) + the quiescent current. The response time comes from the decision stage: with the input stepped over the
//! threshold the node ramps from its clamp to the point where the output crosses the data sheet's level; a small
//! overdrive ramps at gm × overdrive / Cp (`t_od`), a large step at the current limit / Cp (`t_ttl`). How the bundled
//! ones measure against their data sheets: `docs/models/comparators.md`.

use super::num::g;
use super::opamp::eng;
use super::{Common, Out, row};
use serde::Deserialize;

/// The typ values of one comparator (one of the package's), and the min / max the variation analyses spread over.
#[derive(Clone, Debug, Deserialize)]
pub struct ComparatorSpec {
    #[serde(flatten)]
    pub common: Common,
    /// Where in the data sheet, for the notes ("TI JAJSIA8AH の 5.8 節").
    pub cite: String,
    /// Input offset voltage, typ (V) and max (either way).
    pub vos: f64,
    pub vos_max: f64,
    /// Input bias current, typ (A; negative out of the pins, as PNP inputs) and max (absolute).
    pub ib: f64,
    #[serde(default)]
    pub ib_max: Option<f64>,
    /// Large-signal voltage gain (V/V; 200 V/mV = 200000).
    pub avd: f64,
    /// Response time (s) to a small overdrive `od` (V) after an input step, and to a large (TTL-level) step.
    pub t_od: f64,
    pub od: f64,
    pub t_ttl: f64,
    /// The response time's test circuit: the output pulled up to `vpull` through `rpull`, timed to `vth_out` (TI: 5 V,
    /// 5.1 kΩ, 1.4 V).
    pub vpull: f64,
    pub rpull: f64,
    pub vth_out: f64,
    /// Low-level output voltage (V) at the sink current `vol_at` (A).
    pub vol: f64,
    pub vol_at: f64,
    /// The sink current the output gives at most (A; the table's IOL typ at VOL = 1.5 V).
    pub isink: f64,
    /// How `isink` was chosen when the table has no typ (empty: the table's typ).
    #[serde(default)]
    pub isink_note: String,
    /// High-level output leakage (A) at `vpull`.
    #[serde(default)]
    pub ioh: f64,
    /// Quiescent current per comparator (A): typ and max.
    pub iq: f64,
    pub iq_max: f64,
    /// How far below the positive supply the inputs' common-mode range ends (V; for the notes).
    pub vicr_head: f64,
}

/// The decision node's clamp (±V) and the half-width of its range where the output turns on (V).
const CLAMP: f64 = 1.0;
const W: f64 = 0.05;
/// The decision node's capacitance.
const CP: f64 = 1e-9;

/// The values the template derives: (gm S, current limit A, Rp Ω, output resistance Ω, the node where the output crosses
/// `vth_out`).
fn derived(p: &ComparatorSpec) -> (f64, f64, f64, f64, f64) {
    // tanh(VOL / (isink ron)) = vol_at / isink
    let ron = p.vol / (p.isink * (p.vol_at / p.isink).atanh());
    // the sink current that pulls the output to vth_out through the pull-up, as a share of the full output
    let i_cross = (p.vpull - p.vth_out) / p.rpull;
    let share = i_cross / (p.isink * (p.vth_out / (p.isink * ron)).tanh());
    let node = -W + 2.0 * W * share;
    // from the clamp to that node: a small overdrive at gm · od / Cp, a large step at the current limit
    let distance = node + CLAMP;
    let gm = CP * distance / (p.od * p.t_od);
    let imax = CP * distance / p.t_ttl;
    (gm, imax, p.avd / gm, ron, node)
}

/// The `.subckt` (pins: inp inn out vcc gnd).
pub fn model(p: &ComparatorSpec) -> String {
    let n = &p.common.name;
    let (gm, imax, rp, ron, _) = derived(p);
    let mut l: Vec<String> = vec![
        format!(".subckt {n} inp inn out vcc gnd"),
        "* input: offset (data sheet typ), bias currents (+ = into the pin)".into(),
        format!("Vos inp i1 DC {}", g(p.vos, 6)),
        format!("* spread Vos {} {} offset (data sheet: max {} mV either way)", g(-p.vos_max, 6), g(p.vos_max, 6), g(p.vos_max * 1e3, 6)),
        format!("Ibp inp gnd DC {}", g(p.ib, 6)),
        format!("Ibn inn gnd DC {}", g(p.ib, 6)),
    ];
    if let Some(max) = p.ib_max {
        let sign = if p.ib < 0.0 { -1.0 } else { 1.0 };
        let note = format!("(data sheet: typ {}, max {}; no min, so from typ)", eng(p.ib.abs(), "A"), eng(max, "A")).replace('µ', "u");
        for (el, what) in [("Ibp", "ib+"), ("Ibn", "ib-")] {
            l.push(format!("* spread {el} {} {} {what} {note}", g(p.ib, 6), g(sign * max, 6)));
        }
    }
    l.extend([
        "* decision: gm into Cp || Rp (the response time to a small overdrive; Rp gives the gain), the current limited (a large step),".into(),
        "* the node clamped to +-1 V".into(),
        format!("Bgm 0 n0 I=limit({}*V(inn,i1),{},{})", g(gm, 6), g(-imax, 6), g(imax, 6)),
        format!("Rp n0 0 {}", g(rp, 6)),
        format!("Cp n0 0 {}", g(CP, 6)),
        format!("Bcl n0 0 I=1000*(max(V(n0)-{CLAMP},0)+min(V(n0)+{CLAMP},0))"),
        "* output: open collector, sinking when IN- is above IN+; tanh gives the table's VOL at its current, at most the sink current".into(),
        format!("Bout out gnd I={}*limit((V(n0)+{W})/{},0,1)*tanh(V(out,gnd)/{})", g(p.isink, 6), g(2.0 * W, 6), g(p.isink * ron, 6)),
    ]);
    if p.ioh > 0.0 {
        l.push(format!("Rlk out gnd {}", g(p.vpull / p.ioh, 6)));
    }
    l.extend([
        "* quiescent current of one comparator".into(),
        format!("Iq vcc gnd DC {}", g(p.iq, 6)),
        format!("* spread Iq {} {} iq (data sheet: typ {}, max {}; no min, so from typ)", g(p.iq, 6), g(p.iq_max, 6), eng(p.iq, "A"), eng(p.iq_max, "A")).replace('µ', "u"),
        format!(".ends {n}"),
    ]);
    l.join("\n")
}

/// What the model is made from and what it leaves out (`sim.limits`).
pub fn limits(p: &ComparatorSpec) -> Vec<String> {
    let isink = if p.isink_note.is_empty() { format!("最大の吸い込み電流 {}", eng(p.isink, "A")) } else { format!("最大の吸い込み電流 {} ({})", eng(p.isink, "A"), p.isink_note) };
    vec![
        format!("データシートの typ の値から作った等価モデルです (メーカーのモデルではありません)。出典: {}。データシートとの比べは docs/models/comparators.md。", p.cite),
        format!(
            "再現しているもの: 応答時間 ({} のオーバードライブで {}、大きい入力で {}。出力を {} で {} に引き上げ、{} を越えるまで)、入力オフセット {}、入力バイアス電流 {}、利得 {} V/mV、出力の Low 電圧 {} ({} のとき)、{isink}、電源電流 {}/回路。部品ごとの違い (オフセット ±{}、電源電流 〜{}) は、ばらつき・最悪ケースの解析でだけ入ります。",
            eng(p.od, "V"),
            eng(p.t_od, "s"),
            eng(p.t_ttl, "s"),
            eng(p.rpull, "Ω"),
            eng(p.vpull, "V"),
            eng(p.vth_out, "V"),
            eng(p.vos, "V"),
            eng(p.ib.abs(), "A"),
            g(p.avd / 1000.0, 3),
            eng(p.vol, "V"),
            eng(p.vol_at, "A"),
            eng(p.iq, "A"),
            eng(p.vos_max, "V"),
            eng(p.iq_max, "A"),
        ),
        format!(
            "出力はオープンコレクタです。High にするにはプルアップ抵抗が要ります (IN− が IN+ より高いとき Low)。入力の同相範囲は 0 V〜(電源 − {} V) で、超えたときの動きは扱いません。ヒステリシスはありません (要るなら外の抵抗で正帰還をかけます)。温度による変化、電源電圧による応答時間の変化、チャネル 2 以降も扱いません。",
            g(p.vicr_head, 3)
        ),
        super::LAST_WORD.into(),
        "電源ピンをつながないと、電源電流が流れないだけで、比べる動きは変わりません。".into(),
    ]
}

pub(super) fn fit(p: &ComparatorSpec) -> Result<Out, String> {
    check(p)?;
    let (gm, imax, _, ron, node) = derived(p);
    // the template's own response at the data sheet's conditions (the ramp from the clamp; the output's own edge is left out)
    let rows = vec![
        row("応答時間", format!("{} のオーバードライブ", eng(p.od, "V")), p.t_od, CP * (node + CLAMP) / (gm * p.od).min(imax), "s"),
        row("応答時間", "大きい入力 (TTL)".into(), p.t_ttl, CP * (node + CLAMP) / imax, "s"),
        row("VOL", format!("IOL = {}", eng(p.vol_at, "A")), p.vol, p.isink * ron * (p.vol_at / p.isink).atanh(), "V"),
    ];
    Ok((model(p), limits(p), rows, vec![]))
}

/// The description: the part's own words, then the model's values.
pub(super) fn description(p: &ComparatorSpec) -> String {
    format!("{}応答時間 {} ({} のオーバードライブ)・{} (大きい入力)、電源電流 {}/回路。データシートから作った等価モデル", p.common.description, eng(p.t_od, "s"), eng(p.od, "V"), eng(p.t_ttl, "s"), eng(p.iq, "A"))
}

/// Values the template cannot use, with what to fix.
pub fn check(p: &ComparatorSpec) -> Result<(), String> {
    let positive = [("応答時間", p.t_od), ("大きい入力の応答時間", p.t_ttl), ("オーバードライブ", p.od), ("利得", p.avd), ("VOL", p.vol), ("VOL の電流", p.vol_at), ("吸い込み電流", p.isink), ("電源電流", p.iq), ("プルアップ抵抗", p.rpull)];
    for (what, v) in positive {
        if !(v.is_finite() && v > 0.0) {
            return Err(format!("{what}は 0 より大きい数にしてください (データシートの typ)"));
        }
    }
    if p.vol_at >= p.isink {
        return Err("VOL を測る電流は、最大の吸い込み電流より小さくしてください".into());
    }
    if p.t_ttl >= p.t_od {
        return Err("大きい入力の応答時間は、小さいオーバードライブの応答時間より短いはずです".into());
    }
    if !(p.vth_out < p.vpull) {
        return Err("出力を測る電圧は、プルアップの電圧より低くしてください".into());
    }
    if p.iq_max < p.iq || p.vos_max < p.vos.abs() {
        return Err("max が typ より小さくなっています。データシートの max を確かめてください".into());
    }
    let (gm, imax, ..) = derived(p);
    if gm * p.od > imax {
        return Err("小さいオーバードライブの応答が、大きい入力より遅くなりません。応答時間を確かめてください".into());
    }
    Ok(())
}
