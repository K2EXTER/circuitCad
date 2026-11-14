//! Bipolar transistor (Gummel-Poon, the simulator's `bjt_eval` in sim-core `dc/devices.rs`): `IS`, `BF`, `ISE`, `NE`,
//! `IKF`, `BR`, `RB`, `RE`, `RC` fitted at once to the hFE curve and the "ON" voltages graph (Nelder-Mead from three
//! starting points, as the Python generator did with SciPy); `CJE`/`CJC` from the Cibo/Cobo curves, `TF` from fT,
//! `VAF` from hoe. A PNP is fitted with magnitudes (the equations are the same).

use super::num::{brentq, g, nelder_mead};
use super::{LAST_WORD, Out, Row, amps, cap_at, fit_cap, row};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct BjtSpec {
    #[serde(flatten)]
    pub common: super::Common,
    #[serde(default)]
    pub pnp: bool,
    /// The figure names for the limits text: the hFE curve, the "ON" voltages, the capacitances.
    pub figs: Figs,
    /// The hFE at the curve's peak (VCE = 1 V); `hfe_norm` is the normalised curve.
    pub hfe_peak: f64,
    /// How `hfe_peak` was chosen and how much real parts vary (one or two sentences for the limits text).
    pub hfe_note: String,
    /// (IC A, hFE / hfe_peak).
    pub hfe_norm: Vec<[f64; 2]>,
    /// (IC A, VBE V) at VCE = 1 V.
    pub vbe_on: Vec<[f64; 2]>,
    /// (IC A, V) at IC/IB = 10.
    pub vbe_sat: Vec<[f64; 2]>,
    pub vce_sat: Vec<[f64; 2]>,
    /// (reverse voltage V, F): emitter-base with the collector open, collector-base with the emitter open.
    pub cibo: Vec<[f64; 2]>,
    pub cobo: Vec<[f64; 2]>,
    /// fT (Hz); the min when no typ is given (the model is then as slow as the min allows).
    pub ft_min: f64,
    /// hoe min and max (S) at IC = 1 mA, VCE = 10 V: VAF from their geometric mean.
    pub hoe: [f64; 2],
}

#[derive(Clone, Debug, Deserialize)]
pub struct Figs {
    pub hfe: String,
    pub on: String,
    pub cap: String,
}

const VT: f64 = crate::diode::VT;

#[derive(Clone, Copy, Debug)]
struct P {
    is: f64,
    bf: f64,
    ise: f64,
    ne: f64,
    ikf: f64,
    rb: f64,
    re: f64,
    br: f64,
    rc: f64,
    vaf: f64,
}

/// (collector current, base current) of the intrinsic transistor.
fn ev(p: &P, vbe: f64, vbc: f64) -> (f64, f64) {
    let (ef, er) = ((vbe / VT).min(200.0).exp(), (vbc / VT).min(200.0).exp());
    let (iff, ir) = (p.is * (ef - 1.0), p.is * (er - 1.0));
    let q2 = iff / p.ikf;
    let h = if q2 > 0.0 { 2.0 / (1.0 + (1.0 + 4.0 * q2).sqrt()) } else { 1.0 };
    let ile = p.ise * ((vbe / (p.ne * VT)).min(200.0).exp() - 1.0);
    let k = 1.0 - vbc / p.vaf;
    ((iff - ir) * k * h - ir / p.br, iff / p.bf + ile + ir / p.br)
}

/// VBE at the pins and hFE at collector current `ic`, VCE = 1 V.
fn on(p: &P, ic: f64) -> Option<(f64, f64)> {
    let vce = 1.0;
    let v = brentq(|v| ev(p, v, v - vce).0 - ic, 0.2, 1.3)?;
    let ib = ev(p, v, v - vce).1;
    Some((v + ib * p.rb + (ic + ib) * p.re, ic / ib))
}

/// VBE(sat) and VCE(sat) at the pins with IC/IB = 10 (Newton on the two junction voltages).
fn sat(p: &P, ic: f64) -> Option<(f64, f64)> {
    let ib = ic / 10.0;
    let f = |x: [f64; 2]| {
        let (c, b) = ev(p, x[0], x[1]);
        [(c - ic) / ic, (b - ib) / ib]
    };
    for guess in [[0.75, 0.65], [0.8, 0.7], [0.7, 0.6], [0.85, 0.8]] {
        let mut x = guess;
        for _ in 0..200 {
            let r = f(x);
            let h = 1e-7;
            let (a, b) = (f([x[0] + h, x[1]]), f([x[0], x[1] + h]));
            let j = [[(a[0] - r[0]) / h, (b[0] - r[0]) / h], [(a[1] - r[1]) / h, (b[1] - r[1]) / h]];
            let det = j[0][0] * j[1][1] - j[0][1] * j[1][0];
            if !det.is_finite() || det == 0.0 {
                break;
            }
            let mut d = [(-r[0] * j[1][1] + r[1] * j[0][1]) / det, (-r[1] * j[0][0] + r[0] * j[1][0]) / det];
            let big = d[0].abs().max(d[1].abs());
            if big > 0.05 {
                d = [d[0] * 0.05 / big, d[1] * 0.05 / big];
            }
            x = [x[0] + d[0], x[1] + d[1]];
            if big < 1e-13 {
                break;
            }
        }
        let r = f(x);
        if r[0].abs() + r[1].abs() <= 1e-9 {
            let (vbe, vbc) = (x[0], x[1]);
            let ie = ic + ib;
            return Some((vbe + ib * p.rb + ie * p.re, (vbe - vbc) + ic * p.rc + ie * p.re));
        }
    }
    None
}

fn vaf_of(b: &BjtSpec) -> f64 {
    ((1e-3 / (b.hoe[0] * b.hoe[1]).sqrt() - 10.0) / 10.0).round_ties_even() * 10.0
}

/// x = log10 of IS, BF, ISE; NE; log10 of IKF, RB, RE, BR, RC.
fn unpack(x: &[f64], vaf: f64) -> P {
    let e = |v: f64| 10f64.powf(v);
    P { is: e(x[0]), bf: e(x[1]), ise: e(x[2]), ne: x[3], ikf: e(x[4]), rb: e(x[5]), re: e(x[6]), br: e(x[7]), rc: e(x[8]), vaf }
}

fn cost(x: &[f64], b: &BjtSpec, vaf: f64) -> f64 {
    let p = unpack(x, vaf);
    // physical ranges: the leakage's emission coefficient and the reverse gain
    if !((1.2..=3.0).contains(&p.ne) && (0.5..=50.0).contains(&p.br)) {
        return 1e6;
    }
    let mut e = 0.0;
    for [ic, n] in &b.hfe_norm {
        let Some((_, h)) = on(&p, *ic) else { return 1e6 };
        e += 4.0 * (h / (b.hfe_peak * n)).ln().powi(2);
    }
    for [ic, v] in &b.vbe_on {
        let Some((m, _)) = on(&p, *ic) else { return 1e6 };
        e += ((m - v) / 0.01).powi(2);
    }
    for [ic, v] in &b.vbe_sat {
        let Some((m, _)) = sat(&p, *ic) else { return 1e6 };
        e += ((m - v) / 0.015).powi(2);
    }
    for [ic, v] in &b.vce_sat {
        let Some((_, m)) = sat(&p, *ic) else { return 1e6 };
        e += ((m - v) / 0.02).powi(2);
    }
    if e.is_finite() { e } else { 1e6 }
}

pub fn fit(b: &BjtSpec) -> Result<Out, String> {
    if b.hfe_norm.len() < 3 || b.vbe_on.len() < 2 || b.vbe_sat.len() < 2 || b.vce_sat.len() < 2 {
        return Err("点が足りません。hFE の曲線は 3 点以上、VBE・VBE(sat)・VCE(sat) は 2 点以上読んでください".into());
    }
    if b.cibo.len() < 2 || b.cobo.len() < 2 {
        return Err("容量の点が足りません。Cibo と Cobo を、逆電圧を変えて 2 点以上ずつ読んでください".into());
    }
    let vaf = vaf_of(b);
    let l = f64::log10;
    let starts = [
        [l(2e-14), l(200.0), l(1e-13), 1.6, l(0.05), 1.0, l(0.3), l(5.0), 0.0],
        [l(3e-14), l(300.0), l(5e-12), 1.8, l(0.02), 0.8, l(0.5), l(30.0), 0.4],
        [l(1e-14), l(250.0), l(1e-12), 2.0, l(0.03), 0.5, l(0.2), l(10.0), 0.2],
    ];
    let mut f = |x: &[f64]| cost(x, b, vaf);
    let mut best: Option<(Vec<f64>, f64)> = None;
    for x0 in starts {
        let (x, _) = nelder_mead(&mut f, &x0, 20000, 20000, 1e-7, 1e-10);
        let (x, c) = nelder_mead(&mut f, &x, 20000, 20000, 1e-8, 1e-11);
        if best.as_ref().is_none_or(|bb| c < bb.1) {
            best = Some((x, c));
        }
    }
    let (x, c) = best.unwrap_or_default();
    if c >= 1e6 {
        return Err("当てはめが収まりませんでした。点の読み (電流の単位 A、電圧 V) を確かめてください".into());
    }
    let p = unpack(&x, vaf);
    let (cje, vje, mje) = fit_cap(&b.cibo, 10);
    let (cjc, vjc, mjc) = fit_cap(&b.cobo, 10);
    let tf = 1.0 / (2.0 * std::f64::consts::PI * b.ft_min);
    let kind = if b.pnp { "PNP" } else { "NPN" };
    let text = format!(
        ".model {} {kind}(IS={} BF={} NF=1 VAF={} IKF={} ISE={} NE={:.3} BR={} RB={} RE={} RC={} CJE={} VJE={} MJE={} CJC={} VJC={} MJC={} TF={})",
        b.common.name,
        g(p.is, 4),
        g(p.bf, 4),
        g(vaf, 4),
        g(p.ikf, 4),
        g(p.ise, 4),
        p.ne,
        g(p.br, 4),
        g(p.rb, 4),
        g(p.re, 4),
        g(p.rc, 4),
        g(cje, 4),
        g(vje, 6),
        g(mje, 6),
        g(cjc, 4),
        g(vjc, 6),
        g(mjc, 6),
        g(tf, 4)
    );
    let hoe = format!("{}〜{} µmho", g(b.hoe[0] * 1e6, 6), g(b.hoe[1] * 1e6, 6));
    let limits = vec![
        format!("データシートの表と代表特性から作った等価モデルです (メーカーのモデルではありません)。出典: {}。データシートとの比べは docs/models/transistors.md。", b.common.source),
        format!("直流: {} (hFE の電流による変化。相対値)・{} (VBE、VBE(sat)、VCE(sat)) に、IS・BF・ISE・NE・IKF・BR・RB・RE・RC を当てはめました。{}", b.figs.hfe, b.figs.on, b.hfe_note),
        format!(
            "容量: {} の Cibo・Cobo に CJE・CJC を当てはめました。速さ: fT の min {} MHz から TF を決めました (実物は、ふつうこれより速い)。Early 電圧 VAF {} V は、hoe ({hoe}、1 mA) の中ほどから。",
            b.figs.cap,
            g(b.ft_min / 1e6, 6),
            g(vaf, 4)
        ),
        "扱わないもの: 蓄積時間 (逆方向の蓄積 TR は入れていない。飽和から抜ける速さは実物と違います)、雑音の 1/f 成分、温度によるパラメータの変化はシミュレータの標準の式、耐圧・定格電流・許容損失を超えても壊れません。".into(),
        LAST_WORD.into(),
    ];
    let mut rows: Vec<Row> = vec![];
    for [ic, n] in &b.hfe_norm {
        rows.push(row("hFE", format!("IC = {}、VCE = 1 V", amps(*ic)), b.hfe_peak * n, on(&p, *ic).map_or(f64::NAN, |o| o.1), ""));
    }
    for [ic, v] in &b.vbe_on {
        rows.push(row("VBE", format!("IC = {}、VCE = 1 V", amps(*ic)), *v, on(&p, *ic).map_or(f64::NAN, |o| o.0), "V"));
    }
    for [ic, v] in &b.vbe_sat {
        rows.push(row("VBE(sat)", format!("IC = {}、IC/IB = 10", amps(*ic)), *v, sat(&p, *ic).map_or(f64::NAN, |o| o.0), "V"));
    }
    for [ic, v] in &b.vce_sat {
        rows.push(row("VCE(sat)", format!("IC = {}、IC/IB = 10", amps(*ic)), *v, sat(&p, *ic).map_or(f64::NAN, |o| o.1), "V"));
    }
    for [v, cc] in &b.cibo {
        rows.push(row("Cibo", format!("VEB = {} V", g(*v, 4)), *cc, cap_at(cje, vje, mje, *v), "F"));
    }
    for [v, cc] in &b.cobo {
        rows.push(row("Cobo", format!("VCB = {} V", g(*v, 4)), *cc, cap_at(cjc, vjc, mjc, *v), "F"));
    }
    let notes = rows
        .iter()
        .filter(|r| r.item == "hFE" && r.error_pct().abs() > 15.0)
        .map(|r| format!("hFE が {} で {:+.0}% ずれています。Gummel-Poon の形では、hFE の曲線の両端を同時に合わせきれないことがあります", r.at, r.error_pct()))
        .collect();
    Ok((text, limits, rows, notes))
}
