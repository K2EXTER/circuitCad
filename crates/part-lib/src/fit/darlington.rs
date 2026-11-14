//! Darlington pair (TIP120, TIP125): the bundled `.subckt` with the driver `Q1`, the output `Q2`, the resistors `R1`
//! (base to the inner node) and `R2` (inner node to emitter) and the diode across collector and emitter. Data sheets give
//! the pair as one part (its hFE, its "ON" voltages), so the pair is solved as a circuit for each reading: four junction
//! voltages from Kirchhoff's current law at the inner node and the given collector current (and VCE, or the forced
//! IC/IB). One Gummel-Poon model is fitted for the output transistor; the driver is the same, `area` times smaller.

use super::bjt::{P, ev};
use super::num::{g, nelder_mead};
use super::{LAST_WORD, Out, Row, amps, cap_at, fit_cap, row};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct DarlingtonSpec {
    #[serde(flatten)]
    pub common: super::Common,
    #[serde(default)]
    pub pnp: bool,
    /// The built-in resistors (ohm): base to the inner node, inner node to emitter (the data sheet's schematic).
    pub r1: f64,
    pub r2: f64,
    /// How much larger the output transistor is than the driver (not in data sheets: an assumption, said in the limits).
    pub area: f64,
    /// Where the readings come from, for the limits text: the hFE figure, the "ON" voltages figure, the capacitances.
    pub figs: super::bjt::Figs,
    /// (IC A, hFE of the pair) at `hfe_vce`.
    pub hfe: Vec<[f64; 2]>,
    pub hfe_vce: f64,
    /// (IC A, VBE of the pair V) at `on_vce`.
    pub vbe_on: Vec<[f64; 2]>,
    pub on_vce: f64,
    /// (IC A, V) at IC/IB = `sat_ratio`.
    pub vbe_sat: Vec<[f64; 2]>,
    pub vce_sat: Vec<[f64; 2]>,
    pub sat_ratio: f64,
    /// (reverse voltage V, F) of the pair: input (emitter-base) and output (collector-base).
    #[serde(default)]
    pub cib: Vec<[f64; 2]>,
    #[serde(default)]
    pub cob: Vec<[f64; 2]>,
    /// The pair's fT (Hz) and where it comes from ("|hfe| の min 4 (1 MHz) から").
    #[serde(default)]
    pub ft: Option<(f64, String)>,
    /// VAF (V) and why (no output characteristics in the data sheet).
    pub vaf: (f64, String),
    /// The diode across collector and emitter: `.model` parameters and where they come from.
    pub diode: (String, String),
}

/// The pair's state from the four intrinsic junction voltages [vbe1, vbc1, vbe2, vbc2]: (total IC, base current from
/// outside, base terminal voltage, collector voltage seen through Q2, collector voltage seen through Q1, the current
/// left over at the inner node, relative).
struct State {
    ic: f64,
    ib: f64,
    vb: f64,
    vc2: f64,
    vc1: f64,
    kcl: f64,
}

fn state(q1: &P, q2: &P, r1: f64, r2: f64, x: &[f64; 4]) -> State {
    let (ic2, ib2) = ev(q2, x[2], x[3]);
    let ie2 = ic2 + ib2;
    let vn1 = x[2] + ib2 * q2.rb + ie2 * q2.re;
    let vc2 = (x[2] - x[3]) + ic2 * q2.rc + ie2 * q2.re;
    let (ic1, ib1) = ev(q1, x[0], x[1]);
    let ie1 = ic1 + ib1;
    let vb = vn1 + x[0] + ib1 * q1.rb + ie1 * q1.re;
    let vc1 = vn1 + (x[0] - x[1]) + ic1 * q1.rc + ie1 * q1.re;
    let kcl = (ie1 - ib2 - vn1 / r2) / (ie1.abs() + ib2.abs() + 1e-12);
    State { ic: ic1 + ic2, ib: ib1 + (vb - vn1) / r1, vb, vc2, vc1, kcl }
}

/// Damped Newton on four unknowns (steps at most 50 mV), from a few starting points.
fn solve(f: &impl Fn(&[f64; 4]) -> [f64; 4], starts: &[[f64; 4]]) -> Option<[f64; 4]> {
    for s in starts {
        let mut x = *s;
        for _ in 0..300 {
            let r = f(&x);
            if r.iter().all(|v| v.abs() < 1e-10) {
                return Some(x);
            }
            let h = 1e-7;
            let mut a = [[0.0; 5]; 4];
            for k in 0..4 {
                let mut xp = x;
                xp[k] += h;
                let rp = f(&xp);
                for i in 0..4 {
                    a[i][k] = (rp[i] - r[i]) / h;
                }
            }
            for i in 0..4 {
                a[i][4] = -r[i];
            }
            // Gauss elimination
            let mut ok = true;
            for c in 0..4 {
                let Some(piv) = (c..4).max_by(|&p, &q| a[p][c].abs().total_cmp(&a[q][c].abs())) else { ok = false; break };
                if !(a[piv][c].abs() > 1e-300) {
                    ok = false;
                    break;
                }
                a.swap(c, piv);
                let row = a[c];
                for (i, ri) in a.iter_mut().enumerate() {
                    if i != c {
                        let m = ri[c] / row[c];
                        for k in c..5 {
                            ri[k] -= m * row[k];
                        }
                    }
                }
            }
            if !ok {
                break;
            }
            let mut d: [f64; 4] = std::array::from_fn(|i| a[i][4] / a[i][i]);
            let big = d.iter().fold(0.0f64, |m, v| m.max(v.abs()));
            if !big.is_finite() {
                break;
            }
            if big > 0.05 {
                d = d.map(|v| v * 0.05 / big);
            }
            for i in 0..4 {
                x[i] += d[i];
            }
        }
        if f(&x).iter().all(|v| v.abs() < 1e-8) {
            return Some(x);
        }
    }
    None
}

/// (VBE, hFE) of the pair at collector current `ic` and `vce`.
fn on(q1: &P, q2: &P, r1: f64, r2: f64, ic: f64, vce: f64) -> Option<(f64, f64)> {
    let f = |x: &[f64; 4]| {
        let s = state(q1, q2, r1, r2, x);
        [(s.ic - ic) / ic, s.vc2 - vce, s.vc1 - vce, s.kcl]
    };
    let starts = [[0.6, 0.6 + 0.7 - vce, 0.7, 0.7 - vce], [0.5, 0.5 + 0.6 - vce, 0.6, 0.6 - vce], [0.7, 0.7 + 0.8 - vce, 0.8, 0.8 - vce]];
    let x = solve(&f, &starts)?;
    let s = state(q1, q2, r1, r2, &x);
    Some((s.vb, s.ic / s.ib))
}

/// (VBE(sat), VCE(sat)) of the pair at `ic` with IC/IB = `ratio`.
fn sat(q1: &P, q2: &P, r1: f64, r2: f64, ic: f64, ratio: f64) -> Option<(f64, f64)> {
    let ib = ic / ratio;
    let f = |x: &[f64; 4]| {
        let s = state(q1, q2, r1, r2, x);
        [(s.ic - ic) / ic, (s.ib - ib) / ib, s.vc1 - s.vc2, s.kcl]
    };
    let starts = [[0.65, 0.5, 0.75, 0.0], [0.7, 0.6, 0.8, -0.2], [0.6, 0.55, 0.7, 0.1], [0.75, 0.7, 0.85, 0.2]];
    let x = solve(&f, &starts)?;
    let s = state(q1, q2, r1, r2, &x);
    Some((s.vb, s.vc2))
}

/// x = log10 of IS, BF, ISE; NE; log10 of IKF, RB, RE, BR, RC: the output transistor; the driver is `area` times smaller.
fn pair(x: &[f64], d: &DarlingtonSpec) -> (P, P) {
    let e = |v: f64| 10f64.powf(v);
    let q2 = P { is: e(x[0]), bf: e(x[1]), ise: e(x[2]), ne: x[3], ikf: e(x[4]), rb: e(x[5]), re: e(x[6]), br: e(x[7]), rc: e(x[8]), vaf: d.vaf.0, nf: 1.0 };
    let a = d.area;
    let q1 = P { is: q2.is / a, ise: q2.ise / a, ikf: q2.ikf / a, rb: q2.rb * a, re: q2.re * a, rc: q2.rc * a, ..q2 };
    (q1, q2)
}

fn cost(x: &[f64], d: &DarlingtonSpec) -> f64 {
    let (q1, q2) = pair(x, d);
    let top = d.hfe.iter().map(|r| r[1]).fold(0.0, f64::max);
    // BF within 20 times the square root of the pair's top hFE (each transistor gives about that root)
    if !((1.2..=3.0).contains(&q2.ne) && (0.5..=50.0).contains(&q2.br) && q2.bf <= 20.0 * top.sqrt()) {
        return 1e6;
    }
    let mut e = 0.0;
    for [ic, h] in &d.hfe {
        let Some((_, m)) = on(&q1, &q2, d.r1, d.r2, *ic, d.hfe_vce) else { return 1e6 };
        e += 16.0 * (m / h).ln().powi(2);
    }
    for [ic, v] in &d.vbe_on {
        let Some((m, _)) = on(&q1, &q2, d.r1, d.r2, *ic, d.on_vce) else { return 1e6 };
        e += ((m - v) / 0.02).powi(2);
    }
    for [ic, v] in &d.vbe_sat {
        let Some((m, _)) = sat(&q1, &q2, d.r1, d.r2, *ic, d.sat_ratio) else { return 1e6 };
        e += ((m - v) / 0.03).powi(2);
    }
    for [ic, v] in &d.vce_sat {
        let Some((_, m)) = sat(&q1, &q2, d.r1, d.r2, *ic, d.sat_ratio) else { return 1e6 };
        e += ((m - v) / 0.04).powi(2);
    }
    if e.is_finite() { e } else { 1e6 }
}

pub fn fit(d: &DarlingtonSpec) -> Result<Out, String> {
    if d.hfe.len() < 3 || d.vbe_on.len() < 2 || d.vbe_sat.len() < 2 || d.vce_sat.len() < 2 {
        return Err("点が足りません。hFE は 3 点以上、VBE・VBE(sat)・VCE(sat) は 2 点以上読んでください".into());
    }
    if !(d.r1 > 0.0 && d.r2 > 0.0 && d.area >= 1.0) {
        return Err("内蔵の抵抗 r1・r2 (Ω) と、大きさの比 area (1 以上) を入れてください".into());
    }
    let l = f64::log10;
    // hFE of the pair is about the product of the two: each about its square root
    let top = d.hfe.iter().map(|r| r[1]).fold(0.0, f64::max).sqrt();
    let starts = [
        [l(1e-12), l(1.5 * top), l(1e-12), 1.5, l(3.0), 0.0, l(0.05), l(5.0), l(0.1)],
        [l(5e-12), l(2.0 * top), l(1e-11), 1.8, l(1.0), 0.5, l(0.1), l(10.0), l(0.2)],
        [l(1e-13), l(top), l(1e-13), 1.3, l(5.0), -0.5, l(0.02), l(3.0), l(0.05)],
    ];
    let mut f = |x: &[f64]| cost(x, d);
    let mut best: Option<(Vec<f64>, f64)> = None;
    for x0 in starts {
        let (x, _) = nelder_mead(&mut f, &x0, 6000, 6000, 1e-6, 1e-8);
        let (x, c) = nelder_mead(&mut f, &x, 6000, 6000, 1e-7, 1e-9);
        if best.as_ref().is_none_or(|b| c < b.1) {
            best = Some((x, c));
        }
    }
    let (x, c) = best.unwrap_or_default();
    if c >= 1e6 {
        return Err("当てはめが収まりませんでした。点の読み (電流 A、電圧 V) を確かめてください".into());
    }
    let (q1, q2) = pair(&x, d);
    let kind = if d.pnp { "PNP" } else { "NPN" };
    let caps = |a: f64| {
        if d.cib.len() < 2 || d.cob.len() < 2 {
            return String::new();
        }
        let (cje, vje, mje) = fit_cap(&d.cib, 10);
        let (cjc, vjc, mjc) = fit_cap(&d.cob, 10);
        format!(" CJE={} VJE={} MJE={} CJC={} VJC={} MJC={}", g(cje / a, 4), g(vje, 6), g(mje, 6), g(cjc / a, 4), g(vjc, 6), g(mjc, 6))
    };
    let tf = d.ft.as_ref().map(|(f, _)| format!(" TF={}", g(1.0 / (2.0 * std::f64::consts::PI * f), 4))).unwrap_or_default();
    let card = |name: &str, p: &P, a: f64| {
        format!(
            ".model {name} {kind}(IS={} BF={} VAF={} IKF={} ISE={} NE={:.3} BR={} RB={} RE={} RC={}{}{tf})",
            g(p.is, 4),
            g(p.bf, 4),
            g(p.vaf, 4),
            g(p.ikf, 4),
            g(p.ise, 4),
            p.ne,
            g(p.br, 4),
            g(p.rb, 4),
            g(p.re, 4),
            g(p.rc, 4),
            caps(a)
        )
    };
    let name = &d.common.name;
    let diode = if d.pnp { "Dd c e DD" } else { "Dd e c DD" };
    let text = format!(
        ".subckt {name} c b e\nQ1 c b n1 QD\nQ2 c n1 e QP\nR1 b n1 {}\nR2 n1 e {}\n{diode}\n{}\n{}\n.model DD D({})\n.ends {name}",
        g(d.r1, 4),
        g(d.r2, 4),
        card("QD", &q1, d.area),
        card("QP", &q2, 1.0),
        d.diode.0
    );
    let limits = vec![
        format!("データシートの代表特性から作った等価モデルです (メーカーのモデルではありません)。出典: {}。データシートとの比べは docs/models/transistors.md。", d.common.source),
        format!(
            "ダーリントン接続 (駆動の Q1、出力の Q2、内蔵の抵抗 R1 = {}Ω・R2 = {}Ω。データシートの図の値) を 1 つの回路として解き、{} (2 個合わせた hFE)・{} (VBE、VBE(sat)、VCE(sat)) に、出力の Q2 の IS・BF・ISE・NE・IKF・BR・RB・RE・RC を当てはめました。駆動の Q1 は同じ作りで {} 分の 1 の大きさと仮定しました (データシートに無い)。",
            g(d.r1, 4),
            g(d.r2, 4),
            d.figs.hfe,
            d.figs.on,
            g(d.area, 4)
        ),
        format!(
            "容量: {}。速さ: {}。Early 電圧 VAF {} V は、{}。コレクタとエミッタの間のダイオード: {}。",
            if d.cib.len() < 2 { "データシートに無いので入れていません".to_string() } else { format!("{} の Cib・Cob に CJE・CJC を当てはめました (Q1 は大きさの比で小さく)", d.figs.cap) },
            d.ft.as_ref().map_or("データシートに無いので TF は入れていません".to_string(), |(f, why)| format!("fT {} MHz ({why}) から TF を決めました", g(f / 1e6, 4))),
            g(d.vaf.0, 4),
            d.vaf.1,
            d.diode.1
        ),
        "扱わないもの: 蓄積時間 (TR)、温度によるパラメータの変化はシミュレータの標準の式、二次降伏、耐圧・定格電流・許容損失を超えても壊れません。".into(),
        LAST_WORD.into(),
    ];
    let mut rows: Vec<Row> = vec![];
    for [ic, h] in &d.hfe {
        rows.push(row("hFE", format!("IC = {}、VCE = {} V", amps(*ic), g(d.hfe_vce, 4)), *h, on(&q1, &q2, d.r1, d.r2, *ic, d.hfe_vce).map_or(f64::NAN, |o| o.1), ""));
    }
    for [ic, v] in &d.vbe_on {
        rows.push(row("VBE", format!("IC = {}、VCE = {} V", amps(*ic), g(d.on_vce, 4)), *v, on(&q1, &q2, d.r1, d.r2, *ic, d.on_vce).map_or(f64::NAN, |o| o.0), "V"));
    }
    for [ic, v] in &d.vbe_sat {
        rows.push(row("VBE(sat)", format!("IC = {}、IC/IB = {}", amps(*ic), g(d.sat_ratio, 4)), *v, sat(&q1, &q2, d.r1, d.r2, *ic, d.sat_ratio).map_or(f64::NAN, |o| o.0), "V"));
    }
    for [ic, v] in &d.vce_sat {
        rows.push(row("VCE(sat)", format!("IC = {}、IC/IB = {}", amps(*ic), g(d.sat_ratio, 4)), *v, sat(&q1, &q2, d.r1, d.r2, *ic, d.sat_ratio).map_or(f64::NAN, |o| o.1), "V"));
    }
    if d.cib.len() >= 2 && d.cob.len() >= 2 {
        let (cje, vje, mje) = fit_cap(&d.cib, 10);
        let (cjc, vjc, mjc) = fit_cap(&d.cob, 10);
        for [v, cc] in &d.cib {
            rows.push(row("Cib", format!("VEB = {} V", g(*v, 4)), *cc, cap_at(cje, vje, mje, *v), "F"));
        }
        for [v, cc] in &d.cob {
            rows.push(row("Cob", format!("VCB = {} V", g(*v, 4)), *cc, cap_at(cjc, vjc, mjc, *v), "F"));
        }
    }
    // voltages: 60 mV or more (two junctions in series: twice a single transistor's 30 mV)
    let why = "2 個を同じ作りと仮定したダーリントンの形では、ここまでは合わせきれません";
    let notes = rows
        .iter()
        .filter_map(|r| {
            if r.item == "hFE" && r.error_pct().abs() > 20.0 {
                Some(format!("{} が {} で {:+.0}% ずれています。{why}", r.item, r.at, r.error_pct()))
            } else if r.unit == "V" && (r.model - r.datasheet).abs() >= 0.06 {
                Some(format!("{} が {} で {:+.0} mV ずれています。{why}", r.item, r.at, (r.model - r.datasheet) * 1e3))
            } else {
                None
            }
        })
        .collect();
    Ok((text, limits, rows, notes))
}
