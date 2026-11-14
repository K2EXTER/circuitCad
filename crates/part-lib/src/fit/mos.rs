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
    /// VGS(th) typ (V) and the drain current it is measured at (A).
    pub vth: [f64; 2],
    /// (VGS V, ID A, RDS(on) Ω) typ.
    pub rds: Vec<[f64; 3]>,
    pub caps: Caps,
    /// gFS typ (S), VDS (V), ID (A): compared only (the square law fitted to the on resistance seldom meets it).
    #[serde(default)]
    pub gfs: Option<[f64; 3]>,
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

pub fn fit(p: &MosSpec) -> Result<Out, String> {
    let [vth, ith] = p.vth;
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
