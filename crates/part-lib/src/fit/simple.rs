//! Two-terminal parts whose model follows from a few table values, no curve to fit (Q1): an NTC thermistor (R25 and B, the
//! resistance following the simulation's temperature, as the bundled `NTC_10k_B3950`), a quartz crystal or ceramic resonator
//! (the motional R-L-C with the shunt capacitance C0, as the bundled `XTAL_12MHz_3225`) and a TVS diode (its stand-off
//! voltage, breakdown voltage at the test current and clamping voltage at the peak pulse current, a diode's breakdown) and a
//! signal or power relay with one coil and one changeover (or make) contact (the coil's resistance, its operate and release
//! voltages and times, the contact's resistance: a latch with hysteresis driving two switches, as the bundled `TQ2-L2-5V`).

use super::{LAST_WORD, Out, Row, amps, num::g, row};
use serde::Deserialize;

const VT: f64 = 0.025852;

/// An NTC thermistor.
#[derive(Clone, Debug, Deserialize)]
pub struct NtcSpec {
    #[serde(flatten)]
    pub common: super::Common,
    /// Resistance at 25 °C (Ω).
    pub r25: f64,
    /// The B constant (K) and its range as the data sheet names it ("B25/85").
    pub b: f64,
    #[serde(default)]
    pub b_range: String,
    /// The data sheet's R-T table: (°C, Ω), compared with the B equation.
    #[serde(default)]
    pub table: Vec<[f64; 2]>,
    /// The dissipation constant (mW/K), in the notes: self-heating is not in the model.
    #[serde(default)]
    pub dissipation: Option<f64>,
}

/// R(T) of the B equation.
pub fn ntc_r(r25: f64, b: f64, t_c: f64) -> f64 {
    r25 * (b * (1.0 / (t_c + 273.15) - 1.0 / 298.15)).exp()
}

/// The B constant that fits an R-T table best (least squares of ln(R / R25) against 1/T − 1/298.15 K, through R25 at 25 °C),
/// with the table's range of temperatures ("B0/100"), for a data sheet that gives the table but not B, or B for another range
/// than the one the circuit works in. None with fewer than one point away from 25 °C.
pub fn ntc_b_from_table(r25: f64, table: &[[f64; 2]]) -> Option<(f64, String)> {
    let pts: Vec<(f64, f64)> = table.iter().filter(|[t, r]| (t - 25.0).abs() > 0.5 && *r > 0.0).map(|[t, r]| (1.0 / (t + 273.15) - 1.0 / 298.15, (r / r25).ln())).collect();
    if pts.is_empty() || r25.is_nan() || r25 <= 0.0 {
        return None;
    }
    let b = pts.iter().map(|(x, y)| x * y).sum::<f64>() / pts.iter().map(|(x, _)| x * x).sum::<f64>();
    let (lo, hi) = table.iter().fold((f64::MAX, f64::MIN), |(lo, hi), [t, _]| (lo.min(*t), hi.max(*t)));
    Some((b, format!("B{}/{} (表から)", g(lo, 3), g(hi, 3))))
}

pub(super) fn fit_ntc(s: &NtcSpec) -> Result<Out, String> {
    if !(s.r25 > 0.0) || !(s.b > 0.0) {
        return Err("25 °C の抵抗と B 定数は 0 より大きい値にしてください".into());
    }
    if !(500.0..=10000.0).contains(&s.b) {
        return Err(format!("B 定数 {} K は NTC サーミスタとしては外れています (ふつう 2000〜5000 K)。単位を確かめてください", g(s.b, 4)));
    }
    let name = &s.common.name;
    let text = format!(".subckt {name} a b\n* resistance follows the circuit temperature\nBr a b I=V(a,b)/({}*exp({}*(1/(temp+273.15)-1/298.15)))\n.ends {name}", g(s.r25, 6), g(s.b, 6));
    let rows: Vec<Row> = s.table.iter().map(|[t, r]| row("R", format!("{} °C", g(*t, 4)), *r, ntc_r(s.r25, s.b, *t), "Ω")).collect();
    let range = if s.b_range.is_empty() { String::new() } else { format!(" ({})", s.b_range) };
    let mut limits = vec![
        "抵抗値はシミュレーションの温度で決まります。温度は解析タブの温度の設定・温度スイープで変わります (回路全体が同じ温度になります)。".to_string(),
        match s.dissipation {
            Some(d) => format!("自己発熱 (流す電流で温度が上がる。熱放散定数 {} mW/K) と熱の伝わりの遅れは入っていません。電流を流しすぎると実物は測定値がずれます。", g(d, 3)),
            None => "自己発熱 (流す電流で温度が上がる) と熱の伝わりの遅れは入っていません。電流を流しすぎると実物は測定値がずれます。".to_string(),
        },
        format!("B 定数{range}の式で近似しています。B を決めた範囲の外では、データシートの抵抗-温度表とずれます。"),
    ];
    limits.push(LAST_WORD.to_string());
    let mut notes = vec![];
    if let Some(w) = rows.iter().max_by(|a, b| a.error_pct().abs().total_cmp(&b.error_pct().abs())).filter(|r| r.error_pct().abs() > 5.0) {
        notes.push(format!("{} では B の式が表と {:.0} % ずれます (B 定数は 2 つの温度の間で決めた値です)", w.at, w.error_pct()));
    }
    Ok((text, limits, rows, notes))
}

/// A quartz crystal or a ceramic resonator.
#[derive(Clone, Debug, Deserialize)]
pub struct CrystalSpec {
    #[serde(flatten)]
    pub common: super::Common,
    /// The nominal frequency (Hz): at the load capacitance `cl` when it is given, else the series resonance.
    pub f: f64,
    /// The equivalent series resistance (Ω): the data sheet's max, usually.
    pub esr: f64,
    /// The shunt capacitance C0 (F).
    pub c0: f64,
    /// The motional capacitance C1 (F); none: a usual value for the frequency is assumed and said so.
    #[serde(default)]
    pub c1: Option<f64>,
    /// The load capacitance CL (F) the nominal frequency is for.
    #[serde(default)]
    pub cl: Option<f64>,
}

/// The motional capacitance assumed when the data sheet does not give it: about 3 fF for an SMD quartz crystal below
/// 20 MHz, rising to 6 fF above (a few makers' typical values).
fn usual_c1(f: f64) -> f64 {
    if f < 20e6 { 3e-15 } else { 6e-15 }
}

pub(super) fn fit_crystal(s: &CrystalSpec) -> Result<Out, String> {
    if !(s.f > 1e3 && s.f < 1e9) {
        return Err("周波数は 1 kHz〜1 GHz で入れてください (12 MHz なら 12M)".into());
    }
    if !(s.esr > 0.0) || !(s.c0 > 0.0 && s.c0 < 1e-9) {
        return Err("ESR は 0 より大きく、並列容量 C0 は pF の大きさ (例: 3p) で入れてください".into());
    }
    let mut notes = vec![];
    let c1 = match s.c1 {
        Some(c) if c > 0.0 && c < 1e-11 => c,
        Some(_) => return Err("等価直列容量 C1 は fF の大きさ (例: 5f) で入れてください".into()),
        None => {
            notes.push(format!("等価直列容量 C1 がデータシートに無いので、ふつうの値 {} F を使いました。発振の起動や引き込み (負荷容量による周波数のずれ) の大きさが変わります", g(usual_c1(s.f), 2)));
            usual_c1(s.f)
        }
    };
    // the nominal frequency is the load resonance with CL: the series resonance a little below it
    let fs = match s.cl {
        Some(cl) if cl > 0.0 => s.f / (1.0 + c1 / (2.0 * (s.c0 + cl))),
        _ => s.f,
    };
    let w = 2.0 * std::f64::consts::PI * fs;
    let lm = 1.0 / (w * w * c1);
    let name = &s.common.name;
    let text = format!(".subckt {name} a b\nRm a n1 {}\nLm n1 n2 {}\nCm n2 b {}\nC0 a b {}\n.ends {name}", g(s.esr, 4), g(lm, 6), g(c1, 6), g(s.c0, 4));
    let model_fs = 1.0 / (2.0 * std::f64::consts::PI * (lm * c1).sqrt());
    let mut rows = vec![row("直列共振", "Rm-Lm-Cm".into(), fs / 1e6, model_fs / 1e6, "MHz")];
    if let Some(cl) = s.cl.filter(|c| *c > 0.0) {
        let fl = model_fs * (1.0 + c1 / (2.0 * (s.c0 + cl)));
        rows.push(row("負荷共振", format!("CL = {} pF", g(cl * 1e12, 3)), s.f / 1e6, fl / 1e6, "MHz"));
    }
    let q = w * lm / s.esr;
    let limits = vec![
        format!("等価回路 (Rm = {} Ω、Lm = {}H、Cm = {}F、C0 = {}F、Q ≈ {:.0}) だけで、温度や駆動レベルによる周波数の変化は入っていません。負荷容量による引き込みは等価回路の式のとおりです。", g(s.esr, 3), crate::passive::eng(lm), crate::passive::eng(c1), crate::passive::eng(s.c0), q),
        "ESR はデータシートの最大値 (実物はふつうそれより小さい) です。".into(),
        "マイコンの中の発振回路 (反転アンプ) はないので、このモデルだけでは発振しません。AC 解析で共振を見るか、発振回路と組んで使います。".into(),
        LAST_WORD.into(),
    ];
    Ok((text, limits, rows, notes))
}

/// A TVS diode (unidirectional, or bidirectional: two in series back to back).
#[derive(Clone, Debug, Deserialize)]
pub struct TvsSpec {
    #[serde(flatten)]
    pub common: super::Common,
    /// The reverse stand-off voltage VRWM (V) and the leakage at it IR (A).
    pub vrwm: f64,
    pub ir: f64,
    /// The breakdown voltage VBR (V, the table's min or typ) at the test current IT (A).
    pub vbr: f64,
    pub it: f64,
    /// The clamping voltage VC (V) at the peak pulse current IPP (A).
    pub vc: f64,
    pub ipp: f64,
    /// The capacitance at 0 V (F).
    #[serde(default)]
    pub cj: Option<f64>,
    #[serde(default)]
    pub bidirectional: bool,
}

/// The emission coefficient of the forward diode.
const IS: f64 = 1e-12;

/// The voltage of the model at a reverse current `i`: the breakdown's knee and the series resistance (plus the forward drop of the
/// other diode of a bidirectional one).
fn tvs_v(bv: f64, ibv: f64, nbv: f64, rs: f64, bi: bool, i: f64) -> f64 {
    let knee = if i > ibv { nbv * VT * (i / ibv).ln() } else { -nbv * VT * (ibv / i).ln() };
    let fwd = if bi { VT * (i / IS + 1.0).ln() + rs * i } else { 0.0 };
    bv + knee + rs * i + fwd
}

pub(super) fn fit_tvs(s: &TvsSpec) -> Result<Out, String> {
    if !(s.vrwm > 0.0 && s.vbr > s.vrwm && s.vc > s.vbr) {
        return Err("電圧は VRWM < VBR < VC の順になるはずです (表の値と単位を確かめてください)".into());
    }
    if !(s.it > 0.0 && s.ipp > s.it && s.ir > 0.0 && s.ir < s.it) {
        return Err("電流は IR < IT < IPP の順になるはずです (IT は mA、IPP は A の大きさ)".into());
    }
    // the knee's slope: IR at VRWM below the breakdown at IT, but no softer than leaves the series resistance a fifth of the
    // clamp's rise (a table's IR is a maximum, often far above the knee's: the clamp matters more)
    let from_ir = (s.vbr - s.vrwm) / (VT * (s.it / s.ir).ln());
    let most = 0.8 * (s.vc - s.vbr) / (VT * (s.ipp / s.it).ln());
    let nbv = from_ir.min(most).max(0.5);
    let mut notes = vec![];
    if from_ir > most {
        notes.push(format!("VRWM での漏れ電流 {} は表の最大値で、降伏の曲がりから決めるとクランプ電圧に届かないので、曲がりはクランプ電圧から決めました (モデルの漏れ電流はそれより小さくなります)", amps(s.ir)));
    }
    // a bidirectional one: the other diode's forward drop at IT comes off each breakdown
    let fwd_it = if s.bidirectional { VT * (s.it / IS + 1.0).ln() } else { 0.0 };
    let bv = s.vbr - fwd_it;
    // the series resistance: what is left of the clamping voltage at IPP
    let rest = s.vc - tvs_v(bv, s.it, nbv, 0.0, s.bidirectional, s.ipp);
    let rs = rest / (if s.bidirectional { 2.0 } else { 1.0 } * s.ipp);
    let rs = if rs > 0.0 {
        rs
    } else {
        notes.push("クランプ電圧が降伏の曲がりだけで届くので、直列抵抗は小さく (1 mΩ) しました".into());
        1e-3
    };
    let cj = s.cj.filter(|c| *c > 0.0).map_or(String::new(), |c| format!(" CJO={} VJ=0.7 M=0.33", g(c, 4)));
    let card = |n: &str| format!(".model {n} D(IS={} N=1 RS={} BV={} IBV={} NBV={}{cj})", g(IS, 3), g(rs, 4), g(bv, 5), g(s.it, 4), g(nbv, 4));
    let name = &s.common.name;
    let text = if s.bidirectional {
        format!(".subckt {name} a k\nD1 m a {name}_D\nD2 m k {name}_D\n{}\n.ends {name}", card(&format!("{name}_D")))
    } else {
        format!(".subckt {name} a k\nD1 a k {name}_D\n{}\n.ends {name}", card(&format!("{name}_D")))
    };
    let v = |i: f64| tvs_v(bv, s.it, nbv, rs, s.bidirectional, i);
    let mut rows = vec![row("VBR", format!("IT = {}", amps(s.it)), s.vbr, v(s.it), "V"), row("VC", format!("IPP = {}", amps(s.ipp)), s.vc, v(s.ipp), "V")];
    // the leakage at VRWM: the knee's current there (compared only: the knee was set by it)
    let ir_model = s.it * (-(s.vbr - s.vrwm) / (nbv * VT)).exp();
    rows.push(Row { compare_only: true, ..row("IR", format!("VRWM = {} V", g(s.vrwm, 3)), s.ir, ir_model.max(IS), "A") });
    let limits = vec![
        format!("降伏 (BV = {} V、IT = {} で VBR) と、ピーク電流でのクランプ (直列抵抗 {} Ω) を合わせたモデルです。", g(bv, 4), amps(s.it), g(rs, 3)),
        "ESD・サージのパルスの耐量 (ピーク電力・エネルギー) と、温度による降伏電圧の変化は入っていません。部品が壊れるかどうかはデータシートの定格で確かめます。".into(),
        "VBR は表の値 (多くは最小値) で、実物の降伏電圧はそれより少し高いことがあります。".into(),
        LAST_WORD.into(),
    ];
    Ok((text, limits, rows, notes))
}

/// A relay with one coil and one contact: a changeover (COM, NO, NC) or a make contact (COM, NO).
#[derive(Clone, Debug, Deserialize)]
pub struct RelaySpec {
    #[serde(flatten)]
    pub common: super::Common,
    /// The rated coil voltage (V) and the coil's resistance (Ω).
    pub coil_v: f64,
    pub coil_r: f64,
    /// The must-operate (pick-up) voltage (V, the table's max) and the must-release (drop-out) voltage (V, the table's min).
    pub operate_v: f64,
    pub release_v: f64,
    /// The contact's resistance when closed (Ω, the table's initial max; none: 100 mΩ, said so).
    #[serde(default)]
    pub contact_r: Option<f64>,
    /// The operate and release times (s, the table's max; none: 0).
    #[serde(default)]
    pub operate_t: Option<f64>,
    #[serde(default)]
    pub release_t: Option<f64>,
    /// The coil's inductance (H): seldom in the table; with it the coil's current rises slowly and the coil kicks back when cut.
    #[serde(default)]
    pub coil_l: Option<f64>,
    /// A make contact only (1 Form A: COM and NO); else a changeover (1 Form C).
    #[serde(default)]
    pub make_only: bool,
}

/// The contacts' model: closed by the latch's 1 V level, open below.
const RELAY_ROFF: f64 = 1e9;

pub(super) fn fit_relay(s: &RelaySpec) -> Result<Out, String> {
    if !(s.coil_v > 0.0 && s.coil_r > 0.0) {
        return Err("コイルの定格電圧と抵抗は 0 より大きい値にしてください".into());
    }
    if !(s.release_v > 0.0 && s.release_v < s.operate_v) {
        return Err("復帰電圧 (これより下がると離れる) は 0 より大きく、動作電圧 (これを超えると動く) より小さいはずです".into());
    }
    if s.operate_v > s.coil_v * 1.001 {
        return Err(format!("動作電圧 {} V が定格電圧 {} V を超えています (ふつう定格の 70〜80 %)。% で書かれた値は定格電圧を掛けて V にしてください", g(s.operate_v, 3), g(s.coil_v, 3)));
    }
    let mut notes = vec![];
    let ron = match s.contact_r {
        Some(r) if r > 0.0 && r < 10.0 => r,
        Some(_) => return Err("接点の抵抗は Ω で入れてください (100 mΩ なら 100m)".into()),
        None => {
            notes.push("接点の抵抗が無いので 100 mΩ にしました (信号用・電力用のリレーの初期の最大値によくある値)".to_string());
            0.1
        }
    };
    let (top, trel) = (s.operate_t.unwrap_or(0.0).max(0.0), s.release_t.unwrap_or(0.0).max(0.0));
    let name = &s.common.name;
    let pins = if s.make_only { "a b com no" } else { "a b com no nc" };
    // the latch sees the coil's current times its resistance: with an inductance that is the voltage across the resistor
    let (coil, sense) = match s.coil_l.filter(|l| *l > 0.0) {
        Some(l) => (format!("Rc a n {}\nLc n b {}", g(s.coil_r, 5), g(l, 4)), "abs(V(a,n))"),
        None => (format!("Rc a b {}", g(s.coil_r, 5)), "abs(V(a,b))"),
    };
    let (op, rel) = (g(s.operate_v, 5), g(s.release_v, 5));
    let delays = format!("{}{}", if top > 0.0 { format!(" tplh={}", g(top, 4)) } else { String::new() }, if trel > 0.0 { format!(" tphl={}", g(trel, 4)) } else { String::new() });
    let nc = if s.make_only { String::new() } else { "Snc com nc qb 0 SWC\n".into() };
    let text = format!(
        ".subckt {name} {pins}\n* the coil (either polarity)\n{coil}\n* operates above {op} V on the coil, releases below {rel} V: a latch with that hysteresis (1 V levels, threshold 0.75)\n\
         Bs s 0 V=0.75+({sense}-{op})/{op}\nBr r 0 V=0.75+({rel}-{sense})/{op}\nVh hi 0 1\nYlat srlatch q qb 0 hi 60 0.75 0 0 0{delays} s r hi 0 0\n\
         * the contacts: closed {} ohm, open {} ohm\nSno com no q 0 SWC\n{nc}.model SWC SW(RON={} ROFF={} VT=0.5 VW=0.1)\n.ends {name}",
        g(ron, 4),
        g(RELAY_ROFF, 2),
        g(ron, 4),
        g(RELAY_ROFF, 2)
    );
    let i = s.coil_v / s.coil_r;
    let rows = vec![
        row("コイルの電流", format!("{} V", g(s.coil_v, 3)), i * 1e3, i * 1e3, "mA"),
        Row { compare_only: true, ..row("コイルの電力", format!("{} V", g(s.coil_v, 3)), s.coil_v * i * 1e3, s.coil_v * i * 1e3, "mW") },
    ];
    let contacts = if s.make_only { "COM–NO (メーク接点)" } else { "COM–NO と COM–NC (切り替え接点)" };
    let mut limits = vec![
        format!(
            "コイル {} Ω (定格 {} V で {} mA)。コイルの電圧が {} V (動作電圧) を超えると {contacts} が切り替わり、{} V (復帰電圧) より下がると戻ります。その間は前の状態のままです。閉じた接点は {} Ω、開いた接点は 1000 MΩ。",
            g(s.coil_r, 4),
            g(s.coil_v, 3),
            g(i * 1e3, 3),
            op,
            rel,
            g(ron, 3)
        ),
        if top > 0.0 || trel > 0.0 {
            format!("接点が動くのは、コイルの電圧が動作・復帰電圧に届いてから {} ms・{} ms あと (表の最大値) です。", g(top * 1e3, 3), g(trel * 1e3, 3))
        } else {
            "動作時間・復帰時間は入っていません (接点はすぐ動きます)。".into()
        },
        "動作・復帰電圧は表の最大・最小 (保証の値) で、実物はその間のどこかで動きます。".into(),
    ];
    if s.coil_l.is_none_or(|l| l <= 0.0) {
        limits.push("コイルのインダクタンスが無いので、切ったときの逆起電力は出ません。トランジスタで駆動するときの保護のダイオードは、このモデルで要らないように見えても付けます。".into());
    }
    limits.push("扱わないもの: 接点のはね、コイルの抵抗の温度変化、接点の定格 (電流・電圧) を超えたときの溶着。".into());
    limits.push(LAST_WORD.into());
    Ok((text, limits, rows, notes))
}

#[cfg(test)]
mod tests {
    use super::super::{Common, Spec, fit};
    use super::*;

    fn common(name: &str) -> Common {
        Common { part: name.into(), name: name.into(), source: "test".into(), url: String::new(), description: String::new(), read: String::new(), remarks: vec![] }
    }

    /// The bundled 10 kΩ B3950 thermistor's model, from its two numbers; a table point the B equation misses is said.
    #[test]
    fn an_ntc_is_the_bundled_one() {
        let s = NtcSpec { common: common("NTC_10k_B3950"), r25: 10e3, b: 3950.0, b_range: "B25/50".into(), table: vec![[25.0, 10e3], [100.0, 680.0]], dissipation: Some(2.0) };
        let f = fit(&Spec::Ntc(s.clone())).unwrap();
        assert!(f.text.contains("Br a b I=V(a,b)/(10000*exp(3950*(1/(temp+273.15)-1/298.15)))"), "{}", f.text);
        assert!((f.rows[0].model - 10e3).abs() < 1e-6);
        assert!((f.rows[1].model - 680.0).abs() / 680.0 < 0.05, "{:?}", f.rows[1]);
        assert!(f.limits.iter().any(|l| l.contains("2 mW/K")));
        assert!(fit(&Spec::Ntc(NtcSpec { b: 39.5, ..s })).is_err(), "B in the wrong unit");
    }

    /// B from an R-T table: the bundled thermistor's 0, 50 and 100 °C points (B25/50 3950) give a B near 3950; no point away
    /// from 25 °C gives none.
    #[test]
    fn b_from_a_table() {
        let table = [[0.0, ntc_r(10e3, 3950.0, 0.0)], [25.0, 10e3], [50.0, ntc_r(10e3, 3950.0, 50.0)], [100.0, ntc_r(10e3, 3950.0, 100.0)]];
        let (b, range) = ntc_b_from_table(10e3, &table).unwrap();
        assert!((b - 3950.0).abs() < 1e-6 && range.starts_with("B0/100"), "{b} {range}");
        assert!(ntc_b_from_table(10e3, &[[25.0, 10e3]]).is_none());
    }

    /// The bundled 12 MHz crystal (40 Ω, 10 fF, 3 pF): Lm 17.59 mH; with a load capacitance the series resonance moves below the
    /// nominal frequency and the load resonance lands on it.
    #[test]
    fn a_crystal_is_the_bundled_one() {
        let s = CrystalSpec { common: common("XTAL12M"), f: 12e6, esr: 40.0, c0: 3e-12, c1: Some(10e-15), cl: None };
        let f = fit(&Spec::Crystal(s.clone())).unwrap();
        assert!(f.text.contains("Lm n1 n2 0.0175") && f.text.contains("Cm n2 b 1e-14") && f.text.contains("C0 a b 3e-12"), "{}", f.text);
        let f = fit(&Spec::Crystal(CrystalSpec { cl: Some(18e-12), c1: None, ..s })).unwrap();
        assert!(f.rows[0].datasheet < 12.0 && (f.rows[1].model - 12.0).abs() < 1e-6, "{:?}", f.rows);
        assert!(f.notes.iter().any(|n| n.contains("C1")));
    }

    /// SMBJ5.0A (Littelfuse: VRWM 5.0 V, IR 800 µA, VBR 6.40 V at 10 mA, VC 9.2 V at 65.2 A): the model's VBR and VC land on
    /// the table's; the bidirectional one too, with the other diode's drop.
    #[test]
    fn a_tvs_clamps_where_the_table_says() {
        let s = TvsSpec { common: common("SMBJ5_0A"), vrwm: 5.0, ir: 800e-6, vbr: 6.4, it: 10e-3, vc: 9.2, ipp: 65.2, cj: Some(3e-9), bidirectional: false };
        for bi in [false, true] {
            let f = fit(&Spec::Tvs(TvsSpec { bidirectional: bi, ..s.clone() })).unwrap();
            for r in f.rows.iter().filter(|r| !r.compare_only) {
                assert!(r.error_pct().abs() < 1.0, "{bi}: {r:?}\n{}", f.text);
            }
            assert_eq!(f.text.lines().filter(|l| l.starts_with('D')).count(), if bi { 2 } else { 1 }, "{}", f.text);
        }
        assert!(fit(&Spec::Tvs(TvsSpec { vc: 6.0, ..s })).is_err());
    }
}
