//! MOSFET (level 1 with the series resistance `RD`, the simulator's `mos_eval`): `KP` and `RD` fitted to the on
//! resistances at each gate voltage, `VTO` placed so the threshold point (VDS = VGS) carries the data sheet's current;
//! the gate capacitances from Ciss and Crss. A P channel part is written with magnitudes and becomes a PMOS with VTO < 0.

use super::num::{brentq, g, levenberg_marquardt};
use super::{LAST_WORD, Out, Row, amps, row};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct MosSpec {
    #[serde(flatten)]
    pub common: super::Common,
    #[serde(default)]
    pub pch: bool,
    /// VGS(th) typ (V) and the drain current it is measured at (A). Not needed with `transfer` (the power MOSFETs of
    /// International Rectifier give only its min and max: `vth_range`).
    #[serde(default)]
    pub vth: Option<[f64; 2]>,
    /// VGS(th) min, max (V) and the drain current (A), for comparing only.
    #[serde(default)]
    pub vth_range: Option<[f64; 3]>,
    /// The transfer characteristics (ID against VGS in saturation, at `vds`): with them VTO and the source resistance RS
    /// are fitted too (a power MOSFET's ID grows much slower than the square law at large currents).
    #[serde(default)]
    pub transfer: Option<Transfer>,
    /// The body diode's forward curve (source to drain): with it the model is a `.subckt` with `M1` and `Dbody`, as the
    /// bundled power MOSFETs are written.
    #[serde(default)]
    pub body_diode: Option<BodyDiode>,
    /// (VGS V, ID A, RDS(on) Ω) typ.
    pub rds: Vec<[f64; 3]>,
    pub caps: Caps,
    /// gFS typ (S), VDS (V), ID (A): compared only (the square law fitted to the on resistance seldom meets it).
    #[serde(default)]
    pub gfs: Option<[f64; 3]>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Transfer {
    /// Where the points were read ("図 3 (伝達特性の代表特性) の 25 °C").
    pub at: String,
    pub vds: f64,
    /// (VGS V, ID A).
    pub points: Vec<[f64; 2]>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BodyDiode {
    /// Where the points were read ("図 7 (ボディダイオードの順方向の代表特性) の 25 °C").
    pub at: String,
    /// (IS A, VSD V), magnitudes.
    pub points: Vec<[f64; 2]>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Caps {
    pub ciss: f64,
    #[serde(default)]
    pub coss: f64,
    pub crss: f64,
}

fn id(kp: f64, vto: f64, vgs: f64, vds: f64) -> f64 {
    let vov = vgs - vto;
    if vov <= 0.0 {
        0.0
    } else if vds < vov {
        kp * (vov * vds - 0.5 * vds * vds)
    } else {
        0.5 * kp * vov * vov
    }
}

/// VDS / ID with ID forced through the drain (RD in series); 1e6 when the channel cannot carry `i` at this gate voltage.
fn rds(kp: f64, vto: f64, rd: f64, vgs: f64, i: f64) -> f64 {
    let vov = vgs - vto;
    if vov <= 0.0 || 0.5 * kp * vov * vov <= i {
        return 1e6;
    }
    match brentq(|v| id(kp, vto, vgs, v) - i, 1e-12, vov) {
        Some(v) => v / i + rd,
        None => 1e6,
    }
}

/// The saturation current with the source resistance `rs` inside the gate loop: id = KP/2 (VGS - id RS - VTO)^2.
fn isat(kp: f64, vto: f64, rs: f64, vgs: f64) -> f64 {
    let u = vgs - vto;
    if u <= 0.0 {
        0.0
    } else if rs * kp * u < 1e-9 {
        0.5 * kp * u * u
    } else {
        ((kp * rs * u + 1.0) - (2.0 * kp * rs * u + 1.0).sqrt()) / (kp * rs * rs)
    }
}

/// RDS(on) with RS: the channel sees the gate voltage less the drop on RS, and both resistances add.
fn rds_rs(kp: f64, vto: f64, rd: f64, rs: f64, vgs: f64, i: f64) -> f64 {
    rds(kp, vto, 0.0, vgs - i * rs, i) + rd + rs
}

pub fn fit(p: &MosSpec) -> Result<Out, String> {
    let out = match &p.transfer {
        Some(tr) => fit_transfer(p, tr)?,
        None => fit_table(p)?,
    };
    match &p.body_diode {
        Some(bd) => with_body_diode(p, bd, out),
        None => Ok(out),
    }
}

/// The model as `.subckt NAME d g s` with the channel `M1` and the body diode `Dbody` (IS, N, RS fitted to its curve).
fn with_body_diode(p: &MosSpec, bd: &BodyDiode, (text, mut limits, mut rows, mut notes): Out) -> Result<Out, String> {
    if bd.points.len() < 3 {
        return Err("ボディダイオードの点は 3 点以上読んでください (電流の小・中・大)".into());
    }
    let r: Vec<[f64; 3]> = bd.points.iter().map(|[i, _]| [i.ln(), 1.0, *i]).collect();
    let v: Vec<f64> = bd.points.iter().map(|[_, v]| *v).collect();
    let s = crate::diode::least_squares(&r, &v, 3).ok_or("ボディダイオードの点から計算できませんでした")?;
    let (a, c, rs) = (s[0], s[1], s[2].max(0.0));
    if a <= 0.0 {
        return Err("ボディダイオードの点で、電流を増やしても電圧が増えていません。読みを確かめてください".into());
    }
    let (is, n) = ((-c / a).exp(), a / crate::diode::VT);
    let name = &p.common.name;
    let channel = text.replacen(&format!(".model {name} "), ".model MCH ", 1);
    let diode = if p.pch { "Dbody d s DBODY" } else { "Dbody s d DBODY" };
    let text = format!(".subckt {name} d g s\nM1 d g s s MCH\n{diode}\n{channel}\n.model DBODY D(IS={} N={:.3} RS={})\n.ends {name}", g(is, 4), n, g(rs, 4));
    for [i, vsd] in &bd.points {
        rows.push(row("VSD", format!("IS = {} (ボディダイオード)", amps(*i)), *vsd, n * crate::diode::VT * (i / is + 1.0).ln() + i * rs, "V"));
    }
    if !(0.8..=3.0).contains(&n) {
        notes.push(format!("ボディダイオードの N が {n:.2} になりました。点の読みを確かめてください"));
    }
    for l in limits.iter_mut() {
        *l = l.replace("ボディダイオード (ソースからドレインへの逆向きのダイオード)、", "");
    }
    limits.insert(2, format!("ボディダイオード: {} の {} 点に IS・N・RS を当てはめました (逆回復は扱いません)。", bd.at, bd.points.len()));
    Ok((text, limits, rows, notes))
}

/// KP and RD from the on resistances, VTO from the typ threshold (the data sheets with a typ VGS(th)).
fn fit_table(p: &MosSpec) -> Result<Out, String> {
    let [vth, ith] = p.vth.ok_or("しきい値 VGS(th) の typ (vth) か、伝達特性の点 (transfer) を入れてください")?;
    if p.rds.is_empty() {
        return Err("オン抵抗 RDS(on) を 1 点以上 (できればゲート電圧を変えて 2〜3 点) 入れてください".into());
    }
    if !(vth > 0.0 && ith > 0.0) || p.rds.iter().any(|[v, i, r]| !(*v > vth && *i > 0.0 && *r > 0.0)) {
        return Err("しきい値・オン抵抗は大きさ (正の数) で書き、オン抵抗のゲート電圧はしきい値より大きくしてください (P チャネルは pch: true)".into());
    }
    let vto_of = |kp: f64| vth - (2.0 * ith / kp).sqrt();
    let res = |x: &[f64]| -> Vec<f64> {
        let (kp, rd) = (10f64.powf(x[0]), 10f64.powf(x[1]));
        let vto = vto_of(kp);
        p.rds.iter().map(|[vgs, i, r]| (rds(kp, vto, rd, *vgs, *i) / r).ln()).collect()
    };
    // start where the channel alone could carry three times the largest current at the lowest gate voltage
    let vmin = p.rds.iter().map(|r| r[0]).fold(f64::INFINITY, f64::min);
    let imax = p.rds.iter().map(|r| r[1]).fold(0.0, f64::max);
    let rmin = p.rds.iter().map(|r| r[2]).fold(f64::INFINITY, f64::min);
    let kp0 = 6.0 * imax / (vmin - vth).powi(2);
    let x = if p.rds.len() == 1 {
        // one point: no series resistance (all of RDS(on) is the channel)
        let kp = levenberg_marquardt(&|x: &[f64]| res(&[x[0], -30.0]), &[kp0.log10()])[0];
        vec![kp, -30.0]
    } else {
        levenberg_marquardt(&res, &[kp0.log10(), (rmin / 3.0).log10()])
    };
    let (kp, rd) = (10f64.powf(x[0]), if p.rds.len() == 1 { 0.0 } else { 10f64.powf(x[1]) });
    let vto = vto_of(kp);
    let Caps { ciss, crss, .. } = p.caps;
    let (kind, sign) = if p.pch { ("PMOS", -1.0) } else { ("NMOS", 1.0) };
    let text = format!(".model {} {kind}(VTO={} KP={} RD={} CGS={} CGD={})", p.common.name, g(sign * vto, 4), g(kp, 4), g(rd, 4), g(ciss - crss, 4), g(crss, 4));
    let limits = vec![
        format!("データシートの typ の値から作った等価モデルです (メーカーのモデルではありません)。出典: {}。データシートとの比べは docs/models/transistors.md。", p.common.source),
        format!(
            "レベル1 (2乗則) に直列抵抗 RD を足し、しきい値 VGS(th) と、ゲート電圧ごとのオン抵抗 RDS(on) ({} 点) に合わせました。ゲートの容量は Ciss・Crss の typ (CGS = Ciss − Crss、CGD = Crss) の一定の値です。",
            p.rds.len()
        ),
        "扱わないもの: ボディダイオード (ソースからドレインへの逆向きのダイオード)、出力容量 Coss のうちドレイン・ソース間の分、容量の電圧による変化、チャネル長変調 (LAMBDA。飽和領域の電流は VDS によらない)、温度によるオン抵抗の増加、耐圧・定格電流を超えたときの破壊。大電流の飽和領域では、2乗則なので実物より電流が大きく出ます。".into(),
        LAST_WORD.into(),
    ];
    let mut rows: Vec<Row> = vec![row("VGS(th)", format!("ID = {} (VDS = VGS)", amps(ith)), vth, vto + (2.0 * ith / kp).sqrt(), "V")];
    for [vgs, i, r] in &p.rds {
        rows.push(row("RDS(on)", format!("VGS = {} V、ID = {}", g(*vgs, 4), amps(*i)), *r, rds(kp, vto, rd, *vgs, *i), "Ω"));
    }
    rows.push(row("Ciss", "VGS = 0".into(), ciss, ciss, "F"));
    rows.push(row("Crss", "VGS = 0".into(), crss, crss, "F"));
    let mut notes = vec![];
    if let Some([gm, vds, i]) = p.gfs {
        // saturation: gm = sqrt(2·KP·ID) (RD on the drain does not change the current there)
        let mut r = row("gFS", format!("VDS = {} V、ID = {}", g(vds, 4), amps(i)), gm, (2.0 * kp * i).sqrt(), "S");
        r.compare_only = true;
        if r.error_pct().abs() > 30.0 {
            notes.push(format!("gFS が表と {:+.0}% 違います。オン抵抗に合わせた 2乗則では、伝達コンダクタンスまでは合わないことがあります (比べるだけ)", r.error_pct()));
        }
        rows.push(r);
    }
    for r in rows.iter().filter(|r| r.item == "RDS(on)" && r.error_pct().abs() > 5.0) {
        notes.push(format!("RDS(on) が {} で {:+.1}% ずれています。レベル1 + RD の形では、ゲート電圧ごとの差を全部は追えません", r.at, r.error_pct()));
    }
    Ok((text, limits, rows, notes))
}

/// KP, RD, VTO and RS from the transfer curve and the on resistances (a data sheet with no typ threshold).
fn fit_transfer(p: &MosSpec, tr: &Transfer) -> Result<Out, String> {
    if tr.points.len() < 3 || p.rds.is_empty() {
        return Err("伝達特性の点を 3 点以上と、オン抵抗 RDS(on) を 1 点以上入れてください".into());
    }
    if tr.points.iter().any(|[v, i]| !(*v > 0.0 && *i > 0.0)) || p.rds.iter().any(|[v, i, r]| !(*v > 0.0 && *i > 0.0 && *r > 0.0)) {
        return Err("伝達特性とオン抵抗は大きさ (正の数) で書いてください (P チャネルは pch: true)".into());
    }
    let e = |v: f64| 10f64.powf(v);
    let res = |x: &[f64]| -> Vec<f64> {
        let (kp, rd, vto, rs) = (e(x[0]), e(x[1]), x[2], e(x[3]));
        let mut r: Vec<f64> = p.rds.iter().map(|[vgs, i, r]| (rds_rs(kp, vto, rd, rs, *vgs, *i) / r).ln()).collect();
        r.extend(tr.points.iter().map(|[vgs, i]| (isat(kp, vto, rs, *vgs).max(1e-12) / i).ln()));
        r
    };
    // start: the square law through the two lowest points (no RS), RD a third of the smallest on resistance
    let mut pts = tr.points.clone();
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let ([v1, i1], [v2, i2]) = (pts[0], pts[1]);
    let s = (i2.sqrt() - i1.sqrt()) / (v2 - v1);
    let vto0 = v1 - i1.sqrt() / s;
    let rmin = p.rds.iter().map(|r| r[2]).fold(f64::INFINITY, f64::min);
    let x = levenberg_marquardt(&res, &[(2.0 * s * s).log10(), (rmin / 3.0).log10(), vto0, (rmin / 3.0).log10()]);
    let (kp, rd, vto, rs) = (e(x[0]), e(x[1]), x[2], e(x[3]));
    let Caps { ciss, crss, .. } = p.caps;
    let (kind, sign) = if p.pch { ("PMOS", -1.0) } else { ("NMOS", 1.0) };
    let text = format!(".model {} {kind}(VTO={} KP={} RD={} RS={} CGS={} CGD={})", p.common.name, g(sign * vto, 4), g(kp, 4), g(rd, 4), g(rs, 4), g(ciss - crss, 4), g(crss, 4));
    let limits = vec![
        format!("データシートの代表特性と typ の値から作った等価モデルです (メーカーのモデルではありません)。出典: {}。データシートとの比べは docs/models/transistors.md。", p.common.source),
        format!(
            "レベル1 (2乗則) にドレインとソースの直列抵抗 RD・RS を足し、{} ({} 点) とオン抵抗 RDS(on) ({} 点) に合わせました (データシートに VGS(th) の typ が無いので、しきい値も伝達特性から決めました)。ゲートの容量は Ciss・Crss の typ (CGS = Ciss − Crss、CGD = Crss) の一定の値です。",
            tr.at,
            tr.points.len(),
            p.rds.len()
        ),
        "扱わないもの: ボディダイオード (ソースからドレインへの逆向きのダイオード)、出力容量 Coss のうちドレイン・ソース間の分、容量の電圧による変化、チャネル長変調 (LAMBDA。飽和領域の電流は VDS によらない)、温度によるオン抵抗の増加、耐圧・定格電流を超えたときの破壊。しきい値のすぐ上の小さい電流は、2乗則なので実物と違います。".into(),
        LAST_WORD.into(),
    ];
    let mut rows: Vec<Row> = vec![];
    if let Some([lo, hi, ith]) = p.vth_range {
        let mut r = row("VGS(th)", format!("ID = {} (VDS = VGS。表は min {}〜max {} V)", amps(ith), g(lo, 4), g(hi, 4)), (lo + hi) / 2.0, vto + (2.0 * ith / kp).sqrt() + ith * rs, "V");
        r.compare_only = true;
        rows.push(r);
    }
    for [vgs, i] in &pts {
        rows.push(row("ID", format!("VGS = {} V、VDS = {} V", g(*vgs, 4), g(tr.vds, 4)), *i, isat(kp, vto, rs, *vgs), "A"));
    }
    for [vgs, i, r] in &p.rds {
        rows.push(row("RDS(on)", format!("VGS = {} V、ID = {}", g(*vgs, 4), amps(*i)), *r, rds_rs(kp, vto, rd, rs, *vgs, *i), "Ω"));
    }
    rows.push(row("Ciss", "VGS = 0".into(), ciss, ciss, "F"));
    rows.push(row("Crss", "VGS = 0".into(), crss, crss, "F"));
    let mut notes = vec![];
    if let Some([gm, vds, i]) = p.gfs {
        // with RS the gate sees less: gm / (1 + gm RS)
        let g0 = (2.0 * kp * i).sqrt();
        let mut r = row("gFS", format!("VDS = {} V、ID = {}", g(vds, 4), amps(i)), gm, g0 / (1.0 + g0 * rs), "S");
        r.compare_only = true;
        rows.push(r);
    }
    for r in rows.iter().filter(|r| !r.compare_only && (r.item == "RDS(on)" || r.item == "ID") && r.error_pct().abs() > 10.0) {
        notes.push(format!("{} が {} で {:+.0}% ずれています。レベル1 + RD・RS の形では、伝達特性の全体とオン抵抗を同時には追えません", r.item, r.at, r.error_pct()));
    }
    if let Some(r) = rows.iter().find(|r| r.item == "VGS(th)")
        && let Some([lo, hi, _]) = p.vth_range
        && !(lo..=hi).contains(&r.model)
    {
        notes.push(format!("しきい値が表の範囲 ({}〜{} V) の外の {:.2} V になりました。伝達特性の小さい電流の点を足すと近づきます", g(lo, 4), g(hi, 4), r.model));
    }
    Ok((text, limits, rows, notes))
}

