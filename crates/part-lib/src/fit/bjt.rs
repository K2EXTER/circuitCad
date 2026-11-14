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
    /// The hFE where the normalised curve is 1 (at `hfe_vce`); `hfe_norm` is the normalised curve.
    pub hfe_peak: f64,
    /// How `hfe_peak` was chosen and how much real parts vary (one or two sentences for the limits text).
    pub hfe_note: String,
    /// (IC A, hFE / hfe_peak).
    pub hfe_norm: Vec<[f64; 2]>,
    /// The hFE graph is the gain itself, not a curve normalised to 1 (then `hfe_peak` is 1 and `hfe_norm` holds hFE).
    #[serde(default)]
    pub hfe_absolute: bool,
    /// Fit the forward emission coefficient NF too (1 when false): a VBE(on) graph that rises more than 60 mV a decade
    /// (Fairchild's straight lines for the PN2222A: 73 mV) cannot be followed with NF = 1, and the fit then bends hFE.
    #[serde(default)]
    pub fit_nf: bool,
    /// The collector-emitter voltage of the hFE curve (V; 1 V when not given).
    #[serde(default = "one_volt")]
    pub hfe_vce: f64,
    /// (IC A, VBE V) at `on_vce`.
    pub vbe_on: Vec<[f64; 2]>,
    /// (IC A, V) at IC/IB = 10.
    pub vbe_sat: Vec<[f64; 2]>,
    pub vce_sat: Vec<[f64; 2]>,
    /// (reverse voltage V, F): emitter-base with the collector open, collector-base with the emitter open.
    pub cibo: Vec<[f64; 2]>,
    pub cobo: Vec<[f64; 2]>,
    /// fT (Hz); the min when no typ is given (the model is then as slow as the min allows).
    #[serde(default)]
    pub ft_min: Option<f64>,
    /// fT typ (Hz), when the data sheet gives one: used instead of the min.
    #[serde(default)]
    pub ft_typ: Option<f64>,
    /// The collector-emitter voltage of the VBE(on) curve (V; 1 V when not given).
    #[serde(default = "one_volt")]
    pub on_vce: f64,
    /// hoe min and max (S) at IC = 1 mA, VCE = 10 V: VAF from their geometric mean.
    #[serde(default)]
    pub hoe: Option<[f64; 2]>,
    /// VAF (V) and where it comes from, when there is no hoe ("データシートに出力の特性が無いので仮定").
    #[serde(default)]
    pub vaf: Option<(f64, String)>,
}

fn one_volt() -> f64 {
    1.0
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
    nf: f64,
}

/// (collector current, base current) of the intrinsic transistor.
fn ev(p: &P, vbe: f64, vbc: f64) -> (f64, f64) {
    let (ef, er) = ((vbe / (p.nf * VT)).min(200.0).exp(), (vbc / VT).min(200.0).exp());
    let (iff, ir) = (p.is * (ef - 1.0), p.is * (er - 1.0));
    let q2 = iff / p.ikf;
    let h = if q2 > 0.0 { 2.0 / (1.0 + (1.0 + 4.0 * q2).sqrt()) } else { 1.0 };
    let ile = p.ise * ((vbe / (p.ne * VT)).min(200.0).exp() - 1.0);
    let k = 1.0 - vbc / p.vaf;
    ((iff - ir) * k * h - ir / p.br, iff / p.bf + ile + ir / p.br)
}

/// VBE at the pins and hFE at collector current `ic` and collector-emitter voltage `vce`.
fn on(p: &P, ic: f64, vce: f64) -> Option<(f64, f64)> {
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

fn vaf_of(b: &BjtSpec) -> Result<f64, String> {
    match (&b.hoe, &b.vaf) {
        (Some(h), _) => Ok(((1e-3 / (h[0] * h[1]).sqrt() - 10.0) / 10.0).round_ties_even() * 10.0),
        (None, Some((v, _))) => Ok(*v),
        (None, None) => Err("Early 電圧の元がありません。hoe (min と max、1 mA) か、vaf (値と、その理由) を書いてください".into()),
    }
}

/// x = log10 of IS, BF, ISE; NE; log10 of IKF, RB, RE, BR, RC.
/// x[9], when there, is NF.
fn unpack(x: &[f64], vaf: f64) -> P {
    let e = |v: f64| 10f64.powf(v);
    P { is: e(x[0]), bf: e(x[1]), ise: e(x[2]), ne: x[3], ikf: e(x[4]), rb: e(x[5]), re: e(x[6]), br: e(x[7]), rc: e(x[8]), vaf, nf: x.get(9).copied().unwrap_or(1.0) }
}

fn cost(x: &[f64], b: &BjtSpec, vaf: f64) -> f64 {
    let p = unpack(x, vaf);
    // physical ranges: the leakage's emission coefficient, the reverse gain, and the ideal gain BF within 4 times the
    // curve's top (a wide hFE curve otherwise pulls BF to tens of thousands with a tiny IKF)
    let top = b.hfe_norm.iter().map(|r| r[1]).fold(0.0, f64::max) * b.hfe_peak;
    if !((1.2..=3.0).contains(&p.ne) && (0.5..=50.0).contains(&p.br) && p.bf <= 4.0 * top && (1.0..=1.5).contains(&p.nf)) {
        return 1e6;
    }
    let mut e = 0.0;
    for [ic, n] in &b.hfe_norm {
        let Some((_, h)) = on(&p, *ic, b.hfe_vce) else { return 1e6 };
        e += 4.0 * (h / (b.hfe_peak * n)).ln().powi(2);
    }
    for [ic, v] in &b.vbe_on {
        let Some((m, _)) = on(&p, *ic, b.on_vce) else { return 1e6 };
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
    let vaf = vaf_of(b)?;
    let ft = b.ft_typ.or(b.ft_min).ok_or("fT がありません。ft_typ (typ) か ft_min (min) を書いてください")?;
    let l = f64::log10;
    let starts = [
        [l(2e-14), l(200.0), l(1e-13), 1.6, l(0.05), 1.0, l(0.3), l(5.0), 0.0],
        [l(3e-14), l(300.0), l(5e-12), 1.8, l(0.02), 0.8, l(0.5), l(30.0), 0.4],
        [l(1e-14), l(250.0), l(1e-12), 2.0, l(0.03), 0.5, l(0.2), l(10.0), 0.2],
        // from the readings: BF a little over the curve's top, IS through the lowest VBE(on) point, IKF well above the
        // largest current (a flat hFE curve, PN2222A: the three above are made for a peaked one)
        {
            let top = b.hfe_norm.iter().map(|r| r[1]).fold(0.0, f64::max) * b.hfe_peak;
            let [ic, v] = b.vbe_on[0];
            let imax = b.hfe_norm.iter().map(|r| r[0]).fold(0.0, f64::max);
            [l(ic / (v / VT).exp()), l(1.2 * top), l(ic / (v / VT).exp() * 10.0), 1.6, l(5.0 * imax), 0.5, l(0.2), l(10.0), l(0.5)]
        },
    ];
    let mut f = |x: &[f64]| cost(x, b, vaf);
    let mut best: Option<(Vec<f64>, f64)> = None;
    for x0 in starts {
        let mut x0 = x0.to_vec();
        if b.fit_nf {
            x0.push(1.1);
        }
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
    let tf = 1.0 / (2.0 * std::f64::consts::PI * ft);
    let kind = if b.pnp { "PNP" } else { "NPN" };
    let text = format!(
        ".model {} {kind}(IS={} BF={} NF={} VAF={} IKF={} ISE={} NE={:.3} BR={} RB={} RE={} RC={} CJE={} VJE={} MJE={} CJC={} VJC={} MJC={} TF={})",
        b.common.name,
        g(p.is, 4),
        g(p.bf, 4),
        if b.fit_nf { format!("{:.3}", p.nf) } else { "1".into() },
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
    let speed = match b.ft_typ {
        Some(f) => format!("fT の typ {} MHz から TF を決めました", g(f / 1e6, 6)),
        None => format!("fT の min {} MHz から TF を決めました (実物は、ふつうこれより速い)", g(ft / 1e6, 6)),
    };
    let early = match (&b.hoe, &b.vaf) {
        (Some(h), _) => format!("Early 電圧 VAF {} V は、hoe ({}〜{} µmho、1 mA) の中ほどから。", g(vaf, 4), g(h[0] * 1e6, 6), g(h[1] * 1e6, 6)),
        (None, Some((_, why))) => format!("Early 電圧 VAF {} V は、{why}。", g(vaf, 4)),
        (None, None) => String::new(),
    };
    let limits = vec![
        format!("データシートの表と代表特性から作った等価モデルです (メーカーのモデルではありません)。出典: {}。データシートとの比べは docs/models/transistors.md。", b.common.source),
        format!("直流: {} (hFE の電流による変化{})・{} (VBE、VBE(sat)、VCE(sat)) に、IS・BF{}・ISE・NE・IKF・BR・RB・RE・RC を当てはめました。{}", b.figs.hfe, if b.hfe_absolute { "" } else { "。相対値" }, b.figs.on, if b.fit_nf { "・NF (VBE の傾き)" } else { "" }, b.hfe_note),
        format!(
            "容量: {} の Cibo・Cobo に CJE・CJC を当てはめました。速さ: {speed}。{early}",
            b.figs.cap,
        ),
        "扱わないもの: 蓄積時間 (逆方向の蓄積 TR は入れていない。飽和から抜ける速さは実物と違います)、雑音の 1/f 成分、温度によるパラメータの変化はシミュレータの標準の式、耐圧・定格電流・許容損失を超えても壊れません。".into(),
        LAST_WORD.into(),
    ];
    let mut rows: Vec<Row> = vec![];
    for [ic, n] in &b.hfe_norm {
        rows.push(row("hFE", format!("IC = {}、VCE = {} V", amps(*ic), g(b.hfe_vce, 4)), b.hfe_peak * n, on(&p, *ic, b.hfe_vce).map_or(f64::NAN, |o| o.1), ""));
    }
    for [ic, v] in &b.vbe_on {
        rows.push(row("VBE", format!("IC = {}、VCE = {} V", amps(*ic), g(b.on_vce, 4)), *v, on(&p, *ic, b.on_vce).map_or(f64::NAN, |o| o.0), "V"));
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
    let mut notes: Vec<String> = rows
        .iter()
        .filter(|r| r.item == "hFE" && r.error_pct().abs() > 15.0)
        .map(|r| format!("hFE が {} で {:+.0}% ずれています。Gummel-Poon の形では、hFE の曲線の両端を同時に合わせきれないことがあります", r.at, r.error_pct()))
        .collect();
    // voltages: 30 mV or more (VCE(sat) at a small current is a few tens of mV, so its percent says little)
    notes.extend(rows.iter().filter(|r| r.unit == "V" && (r.model - r.datasheet).abs() >= 0.03).map(|r| {
        let why = if r.item == "VCE(sat)" { "小さい電流の飽和電圧は、この形では逆方向の電流増幅率 BR で決まり、hFE や大きい電流の点と同時には合わせきれません (シミュレーションでは実物より低く出ます)" } else { "この形の式では、この電流の電圧まで合わせきれません" };
        format!("{} が {} で {:+.0} mV ずれています。{why}", r.item, r.at, (r.model - r.datasheet) * 1e3)
    }));
    Ok((text, limits, rows, notes))
}
