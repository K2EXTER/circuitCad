//! Two-terminal parts whose model follows from a few table values, no curve to fit (Q1): an NTC thermistor (R25 and B, the
//! resistance following the simulation's temperature, as the bundled `NTC_10k_B3950`), a quartz crystal or ceramic resonator
//! (the motional R-L-C with the shunt capacitance C0, as the bundled `XTAL_12MHz_3225`) and a TVS diode (its stand-off
//! voltage, breakdown voltage at the test current and clamping voltage at the peak pulse current, a diode's breakdown).

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
