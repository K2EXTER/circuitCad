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
    /// The on resistance against the junction temperature, normalized to 25 °C (the data sheets' "Normalized
    /// On-Resistance vs. Temperature"): with it the series resistances RD and RS become resistors of a `.subckt`
    /// with temperature coefficients, fitted so that the whole RDS(on) rises as the curve does.
    #[serde(default)]
    pub rds_temp: Option<RdsTemp>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RdsTemp {
    /// Where the points were read ("図 4 (オン抵抗の温度変化、VGS = 10 V、ID = 31 A)").
    pub at: String,
    /// The gate voltage and drain current of the curve (magnitudes).
    pub vgs: f64,
    pub id: f64,
    /// (junction temperature °C, RDS(on) / RDS(on) at 25 °C).
    pub points: Vec<[f64; 2]>,
}

/// The fitted channel (magnitudes, VTO > 0) with its series resistances, for what is built on it.
#[derive(Clone, Copy, Debug)]
struct Chan {
    kp: f64,
    vto: f64,
    rd: f64,
    rs: f64,
}

/// RDS(on) at `temp` °C as the simulator makes it (`sim_core::temperature`: KP ∝ T^-1.5, the threshold 2 mV/°C
/// lower, the resistors scaled by 1 + TC1·ΔT + TC2·ΔT², all from 27 °C).
fn rds_at(c: Chan, (tc1, tc2): (f64, f64), temp: f64, vgs: f64, i: f64) -> f64 {
    let d = temp - 27.0;
    let kp = c.kp * ((temp + 273.15) / 300.15).powf(-1.5);
    let vto = (c.vto - 0.002 * d).max(0.05);
    let k = (1.0 + tc1 * d + tc2 * d * d).max(0.01);
    rds_rs(kp, vto, c.rd * k, c.rs * k, vgs, i)
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
    let (out, chan) = match &p.transfer {
        Some(tr) => fit_transfer(p, tr)?,
        None => fit_table(p)?,
    };
    let out = match &p.rds_temp {
        Some(rt) => with_tempco(p, rt, chan, out)?,
        None => out,
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
    let diode = if p.pch { "Dbody d s DBODY" } else { "Dbody s d DBODY" };
    let dmodel = format!(".model DBODY D(IS={} N={:.3} RS={})", g(is, 4), n, g(rs, 4));
    let text = match text.find("\n.ends") {
        // already a subcircuit (the series resistances with their temperature coefficients): the diode joins it
        Some(end) if text.starts_with(".subckt") => {
            let at = text[..end].find("\n.model").unwrap_or(end);
            format!("{}\n{diode}{}\n{dmodel}{}", &text[..at], &text[at..end], &text[end..])
        }
        _ => {
            let channel = text.replacen(&format!(".model {name} "), ".model MCH ", 1);
            format!(".subckt {name} d g s\nM1 d g s s MCH\n{diode}\n{channel}\n{dmodel}\n.ends {name}")
        }
    };
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

/// The series resistances as resistors of a `.subckt` with TC1 and TC2 fitted to the on resistance's rise with
/// temperature (the channel's own rise, KP ∝ T^-1.5, is too slow for a power MOSFET whose RDS(on) is mostly RD and RS).
fn with_tempco(p: &MosSpec, rt: &RdsTemp, c: Chan, (text, mut limits, mut rows, mut notes): Out) -> Result<Out, String> {
    if rt.points.is_empty() || rt.points.iter().any(|[t, k]| !(t.is_finite() && *k > 0.0)) || !(rt.vgs > 0.0 && rt.id > 0.0) {
        return Err("オン抵抗の温度変化 (rds_temp) は、温度と 25 °C に対する比 (正の数) の点、ゲート電圧と電流 (大きさ) で書いてください".into());
    }
    if c.rd + c.rs < 1e-9 {
        return Err("直列抵抗 RD・RS が 0 なので、温度係数を付けられません (オン抵抗を 2 点以上入れてください)".into());
    }
    let ratio = |tc: (f64, f64), t: f64| rds_at(c, tc, t, rt.vgs, rt.id) / rds_at(c, tc, 25.0, rt.vgs, rt.id);
    // TC1 in 1e-3/°C and TC2 in 1e-5/°C² so the solver's steps are alike
    let tc_of = |x: &[f64]| (x[0] * 1e-3, x.get(1).map_or(0.0, |v| v * 1e-5));
    let res = |x: &[f64]| -> Vec<f64> { rt.points.iter().map(|[t, k]| (ratio(tc_of(x), *t) / k).ln()).collect() };
    let x = if rt.points.len() >= 3 { levenberg_marquardt(&res, &[4.0, 0.0]) } else { levenberg_marquardt(&res, &[4.0]) };
    let tc = tc_of(&x);
    let name = &p.common.name;
    let model = text.replacen(&format!(".model {name} "), ".model MCH ", 1);
    // RD and RS leave the model for the resistors
    let model = model.split(' ').filter(|w| !w.starts_with("RD=") && !w.starts_with("RS=")).collect::<Vec<_>>().join(" ");
    let tcs = format!("TC1={} TC2={}", g(tc.0, 4), g(tc.1, 4));
    let mut lines = vec![format!(".subckt {name} d g s")];
    let (dn, sn) = (if c.rd > 1e-9 { "di" } else { "d" }, if c.rs > 1e-9 { "si" } else { "s" });
    if c.rd > 1e-9 {
        // not "RD": a name ending in ".RD" is the board's wiring to the self heating (`sim_core::selfheat::is_board_wiring`)
        lines.push(format!("RD1 d di {} {tcs}", g(c.rd, 4)));
    }
    if c.rs > 1e-9 {
        lines.push(format!("RS1 si s {} {tcs}", g(c.rs, 4)));
    }
    lines.push(format!("M1 {dn} g {sn} {sn} MCH"));
    lines.push(model);
    lines.push(format!(".ends {name}"));
    for [t, k] in &rt.points {
        rows.push(row("RDS(on) の温度", format!("Tj = {} °C、VGS = {} V、ID = {} (25 °C との比)", g(*t, 4), g(rt.vgs, 4), amps(rt.id)), *k, ratio(tc, *t), ""));
    }
    if let Some(r) = rows.iter().filter(|r| r.item == "RDS(on) の温度").max_by(|a, b| a.error_pct().abs().total_cmp(&b.error_pct().abs()))
        && r.error_pct().abs() > 5.0
    {
        notes.push(format!("オン抵抗の温度変化が {} で {:+.0}% ずれています。点の読みを確かめてください", r.at, r.error_pct()));
    }
    for l in limits.iter_mut() {
        *l = l.replace("温度によるオン抵抗の増加、", "");
    }
    limits.insert(
        2,
        format!(
            "温度: {} の {} 点に、直列抵抗 RD・RS の温度係数 (TC1 = {}/°C、TC2 = {}/°C²) を合わせました (チャネルの移動度 KP ∝ T^-1.5 と、しきい値の −2 mV/°C は、シミュレータが温度から決めます)。",
            rt.at,
            rt.points.len(),
            g(tc.0, 3),
            g(tc.1, 3)
        ),
    );
    Ok((lines.join("\n"), limits, rows, notes))
}

/// KP and RD from the on resistances, VTO from the typ threshold (the data sheets with a typ VGS(th)).
fn fit_table(p: &MosSpec) -> Result<(Out, Chan), String> {
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
    Ok(((text, limits, rows, notes), Chan { kp, vto, rd, rs: 0.0 }))
}

/// KP, RD, VTO and RS from the transfer curve and the on resistances (a data sheet with no typ threshold).
fn fit_transfer(p: &MosSpec, tr: &Transfer) -> Result<(Out, Chan), String> {
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
    let text = format!(".model {} {kind}(VTO={} KP={} RD={} RS={} CGS={} CGD={})", p.common.name, g(sign * vto, 4), g(kp, 4), g(if rd < 1e-9 { 0.0 } else { rd }, 4), g(if rs < 1e-9 { 0.0 } else { rs }, 4), g(ciss - crss, 4), g(crss, 4));
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
    Ok(((text, limits, rows, notes), Chan { kp, vto, rd: if rd < 1e-9 { 0.0 } else { rd }, rs: if rs < 1e-9 { 0.0 } else { rs } }))
}

