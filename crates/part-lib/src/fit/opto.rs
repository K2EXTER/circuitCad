//! Light parts as `.subckt`s of the bundled form: a photodiode (a diode, a photocurrent source driven by the
//! irradiance pin, a shunt resistance) and an optocoupler (an LED whose current drives the base of the output
//! transistor through a current source). The numbers inside come from the data sheet's table and figures.

use super::num::g;
use super::{LAST_WORD, Out, Row, amps, cap_at, fit_cap, row};
use serde::Deserialize;

const VT: f64 = crate::diode::VT;
const K_OVER_Q: f64 = 8.617333e-5;

/// A value and why it is that value (an assumption the data sheet does not settle).
pub type Assumed = (f64, String);

#[derive(Clone, Debug, Deserialize)]
pub struct PhotodiodeSpec {
    #[serde(flatten)]
    pub common: super::Common,
    /// The reverse light current per mW/cm² (A) and where it is from: the IRRAD pin's 1 V is 1 mW/cm².
    pub sensitivity: Assumed,
    /// (irradiance mW/cm², reverse light current A) read off the data sheet's figure: compared with the straight line.
    #[serde(default)]
    pub light_points: Vec<[f64; 2]>,
    /// The short-circuit current at 1 mW/cm² (A), compared.
    #[serde(default)]
    pub isc: Option<f64>,
    /// The open-circuit voltage (V) at 1 mW/cm²: sets IS with `n`.
    pub voc: f64,
    /// The temperature coefficient of the open-circuit voltage (mV/K), compared (it checks the assumed N).
    #[serde(default)]
    pub voc_tc: Option<f64>,
    /// The emission coefficient and why (the data sheet gives one open-circuit voltage only).
    pub n: Assumed,
    /// The series resistance and why.
    pub rs: Assumed,
    /// The dark current: (reverse voltage V, current A). Sets the shunt resistance.
    pub dark: [f64; 2],
    pub breakdown: super::diode::Breakdown,
    /// (reverse voltage V, capacitance F).
    pub capacitance: Vec<[f64; 2]>,
    #[serde(default)]
    pub cap_at: String,
    /// What the model leaves out about speed (rise time and the like), in words.
    pub response: String,
}

/// The open-circuit voltage of a diode lit to `iph` at `temp` °C (IS scaled with EG = 1.11 eV, XTI = 3, the
/// simulator's defaults).
fn voc_at(is: f64, n: f64, iph: f64, temp: f64) -> f64 {
    let t = temp + 273.15;
    let r = t / 300.15;
    let is_t = is * r.powf(3.0 / n) * (1.11 / (n * K_OVER_Q * t) * (r - 1.0)).exp();
    n * K_OVER_Q * t * (iph / is_t + 1.0).ln()
}

pub fn fit_photodiode(p: &PhotodiodeSpec) -> Result<Out, String> {
    let (k, n, rs) = (p.sensitivity.0, p.n.0, p.rs.0);
    if !(k > 0.0 && p.voc > 0.0 && n > 0.0) {
        return Err("感度・開放電圧・N は 0 より大きい数で書いてください".into());
    }
    // the open-circuit voltage at 1 mW/cm²: the photocurrent k all flows into the diode
    let is = k / ((p.voc / (n * VT)).exp() - 1.0);
    let [vd, id] = p.dark;
    let mut notes = vec![];
    let rsh = if id > is { vd / (id - is) } else {
        notes.push(format!("暗電流 {}A が、開放電圧から決めた IS ({}A) より小さいので、並列の抵抗は入れていません", crate::passive::eng(id), crate::passive::eng(is)));
        1e15
    };
    let (cj0, vj, m) = fit_cap(&p.capacitance, 20);
    let name = &p.common.name;
    let text = format!(
        ".subckt {name} a k l\n* IRRAD pin: 1 V = 1 mW/cm2; photocurrent {} A per mW/cm2 (flows from K to A inside)\nRl l 0 100Meg\nDpd a k DPD\nBph k a I={}*max(V(l),0)\nRsh a k {}\n.model DPD D(IS={} N={} RS={} BV={} IBV={} CJO={} VJ={} M={})\n.ends {name}",
        g(k, 4),
        g(k, 4),
        g(rsh, 4),
        g(is, 4),
        g(n, 4),
        g(rs, 4),
        g(p.breakdown.v, 4),
        g(p.breakdown.i, 4),
        g(cj0, 4),
        g(vj, 6),
        g(m, 6)
    );
    let mut rows: Vec<Row> = p.light_points.iter().map(|[e, i]| row("Ira", format!("Ee = {} mW/cm²", g(*e, 4)), *i, k * e, "A")).collect();
    if let Some(isc) = p.isc {
        rows.push(row("Ik", "Ee = 1 mW/cm²、短絡".into(), isc, k, "A"));
    }
    rows.push(row("Vo", "Ee = 1 mW/cm²、開放".into(), p.voc, voc_at(is, n, k, 27.0), "V"));
    if let Some(tc) = p.voc_tc {
        rows.push(row("TKVo", "Ee = 1 mW/cm²".into(), tc, (voc_at(is, n, k, 28.0) - voc_at(is, n, k, 26.0)) / 2.0 * 1e3, "mV/K"));
    }
    rows.push(row("Iro", format!("VR = {} V、暗い", g(vd, 4)), id, is + vd / rsh, "A"));
    rows.extend(p.capacitance.iter().map(|[v, c]| row("CD", format!("VR = {} V", g(*v, 4)), *c, cap_at(cj0, vj, m, *v), "F")));
    for r in rows.iter().filter(|r| r.error_pct().abs() > 15.0 && r.unit != "V") {
        notes.push(format!("{} が {} で {:+.0}% ずれています。光電流は光の強さに比例し、容量は 1 つの式なので、ここまでは合わせきれません", r.item, r.at, r.error_pct()));
    }
    let limits = vec![
        format!("データシートの代表特性から作った等価モデルです (メーカーのモデルではありません)。出典: {}。データシートとの比べは docs/models/diodes.md。", p.common.source),
        format!("シミュレーション専用ピン IRRAD (基板には出ません): 電圧源をつなぎ、その電圧で光の強さ [mW/cm²] を与えます (1V = 1mW/cm²)。つながないと 0。光電流は {}A/(mW/cm²) ({})。光の強さに比例し、波長による感度の違いは扱いません。", crate::passive::eng(k), p.sensitivity.1),
        format!("ダイオード: 開放電圧 {} V (1 mW/cm²) から IS を決めました (N = {}: {})。直列抵抗 RS = {} Ω ({})。暗電流 {} nA ({} V) から並列の抵抗 {} GΩ を決めました。", g(p.voc, 4), g(n, 4), p.n.1, g(rs, 4), p.rs.1, g(id * 1e9, 3), g(vd, 4), g(rsh / 1e9, 3)),
        format!("接合容量: {}。逆電圧で小さくなります (0 V で {} pF)。降伏: {}。", p.cap_at, g(cj0 * 1e12, 3), p.breakdown.note),
        format!("扱わないもの: {}、暗電流の温度による増え方、自己発熱。", p.response),
        "使い方: 逆バイアス (カソードを + 側) で電流を読むか、トランスインピーダンスアンプにつなぎます。".into(),
        LAST_WORD.into(),
    ];
    Ok((text, limits, rows, notes))
}

#[derive(Clone, Debug, Deserialize)]
pub struct OptocouplerSpec {
    #[serde(flatten)]
    pub common: super::Common,
    /// The LED's forward points (IF A, VF V) at 25 °C and where from.
    pub led: Vec<[f64; 2]>,
    pub led_at: String,
    /// The table's VF typ: (IF A, VF V), compared.
    #[serde(default)]
    pub led_typ: Option<[f64; 2]>,
    /// The LED's terminal capacitance at 0 V (F), and its reverse voltage rating (V).
    pub led_cap: f64,
    pub led_vr: f64,
    /// Output points (IF A, VCE V, IC A) on the flat part of the output curves (VCE ≥ about 2 V), and where from.
    pub output: Vec<[f64; 3]>,
    pub output_at: String,
    /// CTR read off another figure: (IF A, CTR %) and where from. Compared only (often another sample or rank).
    #[serde(default)]
    pub ctr_curve: Vec<[f64; 2]>,
    #[serde(default)]
    pub ctr_curve_at: String,
    /// The CTR range of the table (%), its condition, and the ranks.
    pub ctr_range: [f64; 2],
    pub ctr_note: String,
    /// The output transistor's current gain (not seen from outside; CTR = K × BF) and why.
    pub bf: Assumed,
    /// VCE(sat) typ (V) at (IF A, IC A), from the table.
    pub vce_sat: [f64; 3],
    /// The cut-off frequency (Hz) and its condition: sets TF.
    pub fc: (f64, String),
    /// What is left out (dark current, temperature, the isolation's capacitance), in words.
    pub leaves_out: String,
}

/// IC of the output transistor on the flat part: BF·IB·(1 − VBC/VAF) as the simulator's Gummel-Poon has it (VBC =
/// VBE − VCE, VBE from the transistor's IS of 1e-14).
fn ic_flat(k: f64, bf: f64, vaf: f64, i_f: f64, vce: f64) -> f64 {
    let iff = k * i_f * bf;
    iff * (1.0 + (vce - VT * (iff / 1e-14).ln()) / vaf)
}

/// VCE(sat) of a Gummel-Poon transistor (IS, BF, BR; no resistances) with base current `ib` and collector current `ic`:
/// both junctions on, solved in closed form.
fn vce_sat(bf: f64, br: f64, ib: f64, ic: f64) -> f64 {
    // Ebers-Moll: IC = IS(ef - er) - IS/BR er, IB = IS/BF ef + IS/BR er (IS cancels in the ratio of the exponentials)
    // with x = IS·ef, y = IS·er: x − y(1 + 1/BR) = ic, x/BF + y/BR = ib
    let a = 1.0 + 1.0 / br;
    let y = (ib - ic / bf) / (1.0 / br + a / bf);
    let x = ic + a * y;
    if y <= 0.0 {
        return f64::NAN;
    }
    VT * (x / y).ln()
}

pub fn fit_optocoupler(p: &OptocouplerSpec) -> Result<Out, String> {
    if p.led.len() < 3 {
        return Err("LED の順方向の点を 3 つ以上読んでください".into());
    }
    let rows3: Vec<[f64; 3]> = p.led.iter().map(|[i, _]| [i.ln(), 1.0, *i]).collect();
    let vs: Vec<f64> = p.led.iter().map(|[_, v]| *v).collect();
    let s = crate::diode::least_squares(&rows3, &vs, 3).ok_or("LED の点から計算できませんでした")?;
    let (a, c, rs) = (s[0], s[1], s[2].max(0.0));
    let (is, n) = ((-c / a).exp(), a / VT);
    let vf = |i: f64| n * VT * (i / is + 1.0).ln() + rs * i;
    let bf = p.bf.0;
    // K and VAF: least squares on ln IC over a grid of VAF
    let (mut best, mut kbest, mut vafbest) = (f64::INFINITY, 0.0, 0.0);
    for j in 0..400 {
        let vaf = 5.0 * (1.02f64).powi(j);
        // K from the mean of ln(IC / IC with K = 1) (the base-emitter voltage moves with K only through a logarithm: twice)
        let mut k = 1.0;
        for _ in 0..2 {
            let lk: f64 = p.output.iter().map(|[i_f, vce, ic]| (ic / ic_flat(k, bf, vaf, *i_f, *vce)).ln()).sum::<f64>() / p.output.len() as f64;
            k *= lk.exp();
        }
        let e: f64 = p.output.iter().map(|[i_f, vce, ic]| (ic_flat(k, bf, vaf, *i_f, *vce) / ic).ln().powi(2)).sum();
        if e < best {
            (best, kbest, vafbest) = (e, k, vaf);
        }
    }
    let (k, vaf) = (kbest, vafbest);
    // BR so that VCE(sat) is the table's (bisection in log)
    let [vs_v, vs_if, vs_ic] = p.vce_sat;
    let (mut lo, mut hi) = (1e-3f64, 1e3f64);
    for _ in 0..80 {
        let mid = (lo * hi).sqrt();
        // more BR, lower VCE(sat)
        if vce_sat(bf, mid, k * vs_if, vs_ic) > vs_v { lo = mid } else { hi = mid }
    }
    let br = (lo * hi).sqrt();
    // TF from the cut-off: the base is fed by a current source, so its pole is 1 / (2π BF TF)
    let tf = 1.0 / (2.0 * std::f64::consts::PI * p.fc.0 * bf);
    let name = &p.common.name;
    let text = format!(
        ".subckt {name} a k e c\n* input LED; the output transistor's base gets K times the LED current (CTR = K x BF x (1 + VCE/VAF))\nVsen a an DC 0\nDled an k DLED\nBb e b I={}*I(Vsen)\nQo c b e QPT\n.model DLED D(IS={} N={} RS={} BV={} IBV=1e-05 CJO={} VJ=0.7 M=0.5)\n.model QPT NPN(IS=1e-14 BF={} BR={} VAF={} TF={})\n.ends {name}",
        g(k, 4),
        g(is, 4),
        g(n, 4),
        g(rs, 4),
        g(p.led_vr, 4),
        g(p.led_cap, 4),
        g(bf, 4),
        g(br, 4),
        g(vaf, 4),
        g(tf, 4)
    );
    let mut rows: Vec<Row> = p.led.iter().map(|[i, v]| row("VF", amps(*i), *v, vf(*i), "V")).collect();
    if let Some([i, v]) = p.led_typ {
        let mut r = row("VF", format!("{} (表の typ)", amps(i)), v, vf(i), "V");
        r.compare_only = true;
        rows.push(r);
    }
    rows.extend(p.output.iter().map(|[i_f, vce, ic]| row("IC", format!("IF = {}、VCE = {} V", amps(*i_f), g(*vce, 4)), *ic, ic_flat(k, bf, vaf, *i_f, *vce), "A")));
    for [i_f, ctr] in &p.ctr_curve {
        let mut r = row("CTR", format!("IF = {}、VCE = 5 V ({})", amps(*i_f), p.ctr_curve_at), *ctr, ic_flat(k, bf, vaf, *i_f, 5.0) / i_f * 100.0, "%");
        r.compare_only = true;
        rows.push(r);
    }
    rows.push(row("VCE(sat)", format!("IF = {}、IC = {}", amps(vs_if), amps(vs_ic)), vs_v, vce_sat(bf, br, k * vs_if, vs_ic), "V"));
    let mut notes = vec![];
    for r in rows.iter().filter(|r| !r.compare_only && ((r.unit == "V" && (r.model - r.datasheet).abs() >= 0.03) || (r.unit != "V" && r.error_pct().abs() > 15.0))) {
        notes.push(format!("{} が {} で {:+.1}% ずれています。CTR を電流によらない比 (と Early 効果) とした形では、ここまでは合わせきれません", r.item, r.at, r.error_pct()));
    }
    let ctr5 = ic_flat(k, bf, vaf, 5e-3, 5.0) / 5e-3 * 100.0;
    let limits = vec![
        format!("データシートの代表特性から作った等価モデルです (メーカーのモデルではありません)。出典: {}。データシートとの比べは docs/models/transistors.md。", p.common.source),
        format!("入力の LED: {} を読んだ {} 点に、IS・N・RS を当てはめました。端子間容量 {}F。", p.led_at, p.led.len(), crate::passive::eng(p.led_cap)),
        format!("出力: LED の電流の K 倍をトランジスタのベースに流します。{} に K と Early 電圧 VAF ({} V) を合わせ、電流伝達率 CTR は IF = 5mA・VCE = 5V で約 {:.0}% です。内部のトランジスタの hFE は外から見えないので BF = {} と置きました ({})。", p.output_at, g(vaf, 3), ctr5, g(bf, 4), p.bf.1),
        format!("CTR の個体差: {}。実物は表の {:.0}〜{:.0}% のどこかで、このモデルは 1 つの値です。{}", p.ctr_note, p.ctr_range[0], p.ctr_range[1], if p.ctr_curve.is_empty() { String::new() } else { format!("{} の CTR は別の個体 (高いランク) の曲線で、比べるだけにしました。", p.ctr_curve_at) }),
        format!("飽和: 表の VCE(sat) {} V に、逆方向の電流増幅率 BR を合わせました。応答: {} の {} kHz から、通過時間 TF を決めました (立ち上がり・立ち下がりは数 µs)。", g(vs_v, 3), p.fc.1, g(p.fc.0 / 1e3, 3)),
        format!("扱わないもの: {}。入力と出力の絶縁は、回路図では GND を分けることで表します。", p.leaves_out),
        LAST_WORD.into(),
    ];
    Ok((text, limits, rows, notes))
}
