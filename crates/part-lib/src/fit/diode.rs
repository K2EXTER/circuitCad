//! Diode: `IS`, `N`, `RS` through the forward points (the same least squares as the part form's diode,
//! `part_lib::diode`), the band gap `EG` from a hot curve, `CJO`/`VJ`/`M` from the capacitance curve, `TT` from `trr`.

use super::num::g;
use super::{LAST_WORD, Out, Row, amps, cap_at, fit_cap, row};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct DiodeSpec {
    #[serde(flatten)]
    pub common: super::Common,
    pub forward: Forward,
    /// The forward curve at another temperature (fits `EG` with `XTI` = 2).
    #[serde(default)]
    pub hot: Option<Hot>,
    pub breakdown: Breakdown,
    pub capacitance: Capacitance,
    #[serde(default)]
    pub recovery: Option<Recovery>,
    /// The sentence shown instead of the rectifier's "no trr" note (a Schottky diode: no stored charge).
    #[serde(default)]
    pub no_recovery: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Forward {
    /// Where the points were read ("図 1 (VF と Tj の代表特性) の 25 °C").
    pub at: String,
    /// (current A, forward voltage V).
    pub points: Vec<[f64; 2]>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Hot {
    /// °C
    pub temp: f64,
    pub points: Vec<[f64; 2]>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Breakdown {
    pub v: f64,
    pub i: f64,
    /// Where `v` and `i` come from.
    pub note: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Capacitance {
    /// Where the points come from (or why there are none).
    pub at: String,
    /// (reverse voltage V, capacitance F); empty when the data sheet gives none.
    #[serde(default)]
    pub points: Vec<[f64; 2]>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Recovery {
    pub trr: f64,
    #[serde(rename = "if")]
    pub i_f: f64,
    pub ir: f64,
    pub note: String,
}

/// The simulator's thermal voltage at 27 °C and its temperature scaling (sim-core temperature.rs).
const VT: f64 = crate::diode::VT;
const K_OVER_Q: f64 = 8.617333e-5;

fn vf(is: f64, n: f64, rs: f64, i: f64) -> f64 {
    n * VT * (i / is + 1.0).ln() + i * rs
}

/// The forward voltage at `temp` °C as the simulator scales `IS` with `EG` and `XTI`.
fn vf_hot(is: f64, n: f64, rs: f64, temp: f64, eg: f64, xti: f64, i: f64) -> f64 {
    let r = (temp + 273.15) / 300.15;
    let is_t = is * r.powf(xti / n) * (eg / (n * K_OVER_Q * (temp + 273.15)) * (r - 1.0)).exp();
    n * r * K_OVER_Q * 300.15 * (i / is_t + 1.0).ln() + rs * i
}

/// `EG` to 1 mV (0.3 .. 1.2 eV) that puts the hot points on the model.
fn fit_eg(is: f64, n: f64, rs: f64, hot: &Hot, xti: f64) -> f64 {
    let mut best = (f64::INFINITY, 0.3);
    for m in 300..1200 {
        let eg = m as f64 / 1000.0;
        let e: f64 = hot.points.iter().map(|[i, u]| (vf_hot(is, n, rs, hot.temp, eg, xti, *i) - u).powi(2)).sum();
        if e < best.0 {
            best = (e, eg);
        }
    }
    best.1
}

pub fn fit(p: &DiodeSpec) -> Result<Out, String> {
    let pts = &p.forward.points;
    if pts.len() < 3 {
        return Err("順方向の点が足りません。電流の小・中・大で 3 点以上読んでください (IS・N・RS の 3 つを決めるため)".into());
    }
    if pts.iter().any(|[i, v]| !(*i > 0.0 && *v > 0.0)) {
        return Err("順方向の点は、電流 (A) と電圧 (V) を 0 より大きい数で書いてください".into());
    }
    // V = a·ln(I) + c + r·I (a = N·VT, c = −a·ln IS): linear least squares
    let rows: Vec<[f64; 3]> = pts.iter().map(|[i, _]| [i.ln(), 1.0, *i]).collect();
    let vs: Vec<f64> = pts.iter().map(|[_, v]| *v).collect();
    let s = crate::diode::least_squares(&rows, &vs, 3).ok_or("順方向の点から計算できませんでした。電流の離れた点を読んでください")?;
    let (a, c, rs) = (s[0], s[1], s[2]);
    if a <= 0.0 {
        return Err("電流を増やしても電圧が増えていません。順方向の点の読みを確かめてください".into());
    }
    if rs < 0.0 {
        return Err("大きい電流で電圧の上がりが小さすぎて、直列抵抗 RS が負になりました。大きい電流の点の読みを確かめてください".into());
    }
    let (is, n) = ((-c / a).exp(), a / VT);
    let mut text = format!(".model {} D(IS={} N={:.3} RS={} BV={} IBV={}", p.common.name, g(is, 4), n, g(rs, 4), g(p.breakdown.v, 4), g(p.breakdown.i, 4));
    let mut out: Vec<Row> = pts.iter().map(|[i, v]| row("VF", format!("{} (25 °C)", amps(*i)), *v, vf(is, n, rs, *i), "V")).collect();
    let mut notes = vec![];
    if !(0.5..=3.0).contains(&n) {
        notes.push(format!("N が {n:.2} になりました。ふつうのダイオードは 1〜2 ほどです。順方向の点の読みを確かめてください"));
    }
    let mut drift = None;
    if let Some(h) = &p.hot {
        let xti = 2.0;
        let eg = fit_eg(is, n, rs, h, xti);
        text += &format!(" EG={} XTI={}", g(eg, 6), g(xti, 6));
        out.extend(h.points.iter().map(|[i, v]| row("VF", format!("{} ({} °C)", amps(*i), g(h.temp, 6)), *v, vf_hot(is, n, rs, h.temp, eg, xti, *i), "V")));
        // mV per °C between 25 °C and the hot curve, at the hot curve's smallest and largest current
        let per = |i: f64| (vf_hot(is, n, rs, 25.0, eg, xti, i) - vf_hot(is, n, rs, h.temp, eg, xti, i)) / (h.temp - 25.0) * 1e3;
        let ends = [h.points.first(), h.points.last()].map(|q| q.map(|[i, _]| per(*i)).unwrap_or(0.0));
        drift = Some((ends[0].min(ends[1]), ends[0].max(ends[1])));
    }
    let cap = &p.capacitance;
    if !cap.points.is_empty() {
        let (cj0, vj, m) = fit_cap(&cap.points, 20);
        text += &format!(" CJO={} VJ={} M={}", g(cj0, 4), g(vj, 6), g(m, 6));
        out.extend(cap.points.iter().map(|[v, c]| row("CJ", format!("VR = {} V", g(*v, 4)), *c, cap_at(cj0, vj, m, *v), "F")));
    }
    if let Some(r) = &p.recovery {
        text += &format!(" TT={}", g(r.trr / (1.0 + r.i_f / r.ir).ln(), 4));
    }
    text += ")";

    let mut limits = vec![
        format!("データシートの代表特性から作った等価モデルです (メーカーのモデルではありません)。出典: {}。データシートとの比べは docs/models/diodes.md。", p.common.source),
        format!("順方向: {} を読んだ {} 点に、IS・N・RS を当てはめました (部品作成フォームのダイオードと同じ計算)。", p.forward.at, pts.len()),
        if cap.points.is_empty() { format!("接合容量: {}。", cap.at) } else { format!("接合容量: {}。逆電圧で小さくなります。", cap.at) },
        format!("降伏: {}。", p.breakdown.note),
    ];
    if let (Some(h), Some((lo, hi))) = (&p.hot, drift) {
        limits.push(format!("温度: 同じ図の {} °C の曲線に、バンドギャップ EG を当てはめました (XTI = 2)。順方向電圧は、1 °C あたり約 {lo:.1}〜{hi:.1} mV 下がります。", g(h.temp, 6)));
    }
    if let Some(r) = &p.recovery {
        limits.push(format!("逆回復: {} から、蓄積時間 TT を決めました (max の値なので、実物はふつう、これより速く戻ります)。", r.note));
    } else if let Some(s) = &p.no_recovery {
        limits.push(s.clone());
    } else {
        limits.push("逆回復時間は扱いません (データシートに trr が無い。標準の整流用で、数 µs ほど逆向きに流れます。逆回復のレッスンのサンプルは、自分で TT を足しています)。".into());
    }
    limits.push(
        if p.hot.is_some() { "扱わないもの: " } else { "扱わないもの: 温度による順方向電圧の変化はシミュレータの標準の式 (1 °C あたり約 −2 mV)、" }.to_string()
            + "逆方向の漏れ電流 (IS の分だけ。実物は電圧と温度で増えます)、自己発熱。耐圧や定格電流を超えても壊れません。",
    );
    limits.push(LAST_WORD.into());
    Ok((text, limits, out, notes))
}
