//! Chip capacitors, resistors and inductors as library parts: a value plus the parasitics a data sheet
//! gives (ESR, ESL, DCR, self-resonance) become one `.subckt`, so the part simulates like the real one.
//! Also the other way round: fit ESR and ESL to the impedance table of a capacitor data sheet.

use crate::PinDef;
use sch_core::PinSide;
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PassiveKind {
    Capacitor,
    Resistor,
    Inductor,
}

impl PassiveKind {
    pub fn label(self) -> &'static str {
        match self {
            PassiveKind::Capacitor => "コンデンサ",
            PassiveKind::Resistor => "抵抗",
            PassiveKind::Inductor => "コイル",
        }
    }

    pub fn prefix(self) -> &'static str {
        match self {
            PassiveKind::Capacitor => "C",
            PassiveKind::Resistor => "R",
            PassiveKind::Inductor => "L",
        }
    }

    pub fn unit(self) -> &'static str {
        match self {
            PassiveKind::Capacitor => "F",
            PassiveKind::Resistor => "Ω",
            PassiveKind::Inductor => "H",
        }
    }

    /// Name of the symbol drawn for the part (`CustomSymbol.glyph`).
    pub fn glyph(self) -> &'static str {
        match self {
            PassiveKind::Capacitor => "capacitor",
            PassiveKind::Resistor => "resistor",
            PassiveKind::Inductor => "inductor",
        }
    }
}

/// What the learner typed (all in SI units: F, Ω, H, Hz; `None` = leave out).
#[derive(Clone, Debug, PartialEq)]
pub struct PassiveTemplate {
    pub kind: PassiveKind,
    /// Capacitance, resistance or inductance.
    pub value: f64,
    /// Capacitor: series resistance. Resistor: not used.
    pub esr: Option<f64>,
    /// Capacitor: series inductance. Resistor: parasitic series inductance.
    pub esl: Option<f64>,
    /// Inductor: winding resistance.
    pub dcr: Option<f64>,
    /// Inductor / resistor: parallel capacitance across the part.
    pub cp: Option<f64>,
    /// Capacitor: the part of the capacitance lost at the working DC bias (0.4 = only 60 % left).
    pub bias_loss: f64,
}

impl PassiveTemplate {
    pub fn new(kind: PassiveKind, value: f64) -> Self {
        Self { kind, value, esr: None, esl: None, dcr: None, cp: None, bias_loss: 0.0 }
    }
}

#[derive(Debug)]
pub struct Generated {
    pub pins: Vec<PinDef>,
    pub sim_name: String,
    pub sim_text: String,
    pub sim_pins: Vec<String>,
    /// The value shown in the schematic ("100n", "10u", "4.7k").
    pub value_text: String,
    pub description: String,
    pub limits: Vec<String>,
}

/// `4.7u`, `100n`, `2.2k`: engineering notation SPICE reads back.
pub fn eng(v: f64) -> String {
    if v == 0.0 {
        return "0".into();
    }
    let a = v.abs();
    let (scale, suffix) = [(1e9, "G"), (1e6, "Meg"), (1e3, "k"), (1.0, ""), (1e-3, "m"), (1e-6, "u"), (1e-9, "n"), (1e-12, "p"), (1e-15, "f")]
        .into_iter()
        .find(|(s, _)| a >= *s * 0.9999)
        .unwrap_or((1e-15, "f"));
    let x = v / scale;
    let t = format!("{x:.3}");
    format!("{}{suffix}", t.trim_end_matches('0').trim_end_matches('.'))
}

/// A subcircuit name SPICE accepts: letters, digits and `_`.
fn spice_name(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    if s.is_empty() || s.chars().all(|c| c == '_') { "PASSIVE".into() } else { s }
}

fn positive(what: &str, v: Option<f64>) -> Result<(), String> {
    match v {
        Some(x) if !(x.is_finite() && x > 0.0) => Err(format!("{what}は 0 より大きい数にしてください (空にすると入れません)")),
        _ => Ok(()),
    }
}

impl PassiveTemplate {
    pub fn generate(&self, name: &str) -> Result<Generated, String> {
        if !(self.value.is_finite() && self.value > 0.0) {
            return Err(format!("{}の値は 0 より大きい数にしてください (例: 100n / 4.7k / 10u)", self.kind.label()));
        }
        positive("ESR", self.esr)?;
        positive("ESL", self.esl)?;
        positive("DCR (巻線抵抗)", self.dcr)?;
        positive("並列容量", self.cp)?;
        if !(0.0..0.95).contains(&self.bias_loss) {
            return Err("DCバイアスによる容量の低下は 0〜95% の間で入れてください (例: 30)".into());
        }
        let sub = spice_name(name);
        let mut l = vec![format!(".subckt {sub} 1 2")];
        let mut limits = vec!["温度による値の変化は扱いません。".to_string()];
        let mut parts: Vec<String> = vec![];
        let mut node = "1".to_string();
        let next = |n: &mut String, label: &str| -> String {
            let m = format!("n_{label}");
            *n = m.clone();
            m
        };
        match self.kind {
            PassiveKind::Capacitor => {
                let ceff = self.value * (1.0 - self.bias_loss);
                if let Some(esr) = self.esr {
                    let m = next(&mut node, "esr");
                    l.push(format!("Resr 1 {m} {}", eng(esr)));
                    parts.push(format!("ESR {}", eng(esr)));
                }
                if let Some(esl) = self.esl {
                    let from = node.clone();
                    let m = next(&mut node, "esl");
                    l.push(format!("Lesl {from} {m} {}", eng(esl)));
                    parts.push(format!("ESL {}", eng(esl)));
                }
                l.push(format!("C1 {node} 2 {}", eng(ceff)));
                if self.bias_loss > 0.0 {
                    parts.push(format!("DCバイアス時の容量 {} ({:.0}% 低下)", eng(ceff), self.bias_loss * 100.0));
                    limits.push(format!("容量は、かける電圧で下がった値 ({}) の一定値として扱います。電圧が変わっても動きません。", eng(ceff)));
                } else {
                    limits.push("セラミックコンデンサは、直流の電圧をかけると容量が下がります。このモデルは下がりません (「DCバイアスで下がる量」に入れると反映できます)。".into());
                }
                if self.esr.is_none() && self.esl.is_none() {
                    limits.push("ESR と ESL を入れていないので、理想のコンデンサと同じ動きです。".into());
                }
            }
            PassiveKind::Resistor => {
                if let Some(esl) = self.esl {
                    let m = next(&mut node, "esl");
                    l.push(format!("Lpar 1 {m} {}", eng(esl)));
                    parts.push(format!("寄生インダクタンス {}", eng(esl)));
                }
                l.push(format!("R1 {node} 2 {}", eng(self.value)));
                if let Some(cp) = self.cp {
                    l.push(format!("Cpar 1 2 {}", eng(cp)));
                    parts.push(format!("並列容量 {}", eng(cp)));
                }
                limits.push("抵抗値は電圧や電力では変わりません (電力による温度上昇も扱いません)。".into());
            }
            PassiveKind::Inductor => {
                if let Some(dcr) = self.dcr {
                    let m = next(&mut node, "dcr");
                    l.push(format!("Rdcr 1 {m} {}", eng(dcr)));
                    parts.push(format!("DCR {}", eng(dcr)));
                }
                l.push(format!("L1 {node} 2 {}", eng(self.value)));
                if let Some(cp) = self.cp {
                    l.push(format!("Cp 1 2 {}", eng(cp)));
                    parts.push(format!("並列容量 {}", eng(cp)));
                }
                limits.push("電流が大きいと起こる磁気飽和 (インダクタンスの低下) は扱いません。".into());
                if self.dcr.is_none() {
                    limits.push("DCR (巻線の抵抗) を入れていないので、損失のない理想のコイルです。".into());
                }
            }
        }
        l.push(".ends".into());
        let extra = if parts.is_empty() { "理想の部品".to_string() } else { parts.join("・") };
        let value_text = eng(self.value);
        let pin = |n: &str, side| PinDef { number: n.into(), name: n.into(), side: Some(side), sim_only: false };
        Ok(Generated {
            pins: vec![pin("1", PinSide::Left), pin("2", PinSide::Right)],
            sim_name: sub,
            sim_text: l.join("\n"),
            sim_pins: vec!["1".into(), "2".into()],
            description: format!("{} {}{} ({extra})。データシートの数値から作ったモデルです。実物の値は個体差や温度で変わります", self.kind.label(), value_text, self.kind.unit()),
            value_text,
            limits,
        })
    }
}

// ---------------------------------------------------------------- fitting

/// ESR and ESL found from an impedance table.
#[derive(Clone, Debug, PartialEq)]
pub struct CapFit {
    pub esr: f64,
    pub esl: f64,
    /// Self-resonant frequency of the fitted model, in Hz.
    pub srf: f64,
    /// Largest difference between the model and the table, in percent of |Z|.
    pub max_error_pct: f64,
    /// Things the table could not pin down, in Japanese.
    pub warnings: Vec<String>,
}

/// |Z| of a capacitor with series ESR and ESL at `f`.
pub fn cap_impedance(c: f64, esr: f64, esl: f64, f: f64) -> f64 {
    let w = 2.0 * PI * f;
    let x = w * esl - 1.0 / (w * c);
    (esr * esr + x * x).sqrt()
}

/// Self-resonant frequency of a capacitor `c` with series inductance `esl`.
pub fn srf(c: f64, esl: f64) -> f64 {
    1.0 / (2.0 * PI * (esl * c).sqrt())
}

/// ESL of a capacitor `c` that resonates at `f0` (the data sheet's "SRF").
pub fn esl_from_srf(c: f64, f0: f64) -> Result<f64, String> {
    if c > 0.0 && f0 > 0.0 { Ok(1.0 / ((2.0 * PI * f0).powi(2) * c)) } else { Err("容量と自己共振周波数は 0 より大きい数にしてください".into()) }
}

/// Parallel capacitance of an inductor `l` that resonates by itself at `f0`.
pub fn cp_from_srf(l: f64, f0: f64) -> Result<f64, String> {
    if l > 0.0 && f0 > 0.0 { Ok(1.0 / ((2.0 * PI * f0).powi(2) * l)) } else { Err("インダクタンスと自己共振周波数は 0 より大きい数にしてください".into()) }
}

/// Fit ESR and ESL of a capacitor `c` to `(frequency Hz, |Z| ohm)` points by least squares on ln|Z|.
pub fn fit_cap(c: f64, points: &[(f64, f64)]) -> Result<CapFit, String> {
    if !(c.is_finite() && c > 0.0) {
        return Err("先に容量を入れてください".into());
    }
    if points.len() < 2 {
        return Err("インピーダンスの表が足りません。周波数と |Z| の組を 2 行以上 (できれば谷の前後 3〜5 行) 入れてください".into());
    }
    if let Some(p) = points.iter().find(|p| !(p.0 > 0.0 && p.1 > 0.0 && p.0.is_finite() && p.1.is_finite())) {
        return Err(format!("周波数 {} Hz / |Z| {} Ω は 0 より大きい数にしてください", p.0, p.1));
    }
    let cost = |esr: f64, esl: f64| -> f64 { points.iter().map(|&(f, z)| (cap_impedance(c, esr, esl, f).ln() - z.ln()).powi(2)).sum() };
    // coarse grid in log10, then two finer passes around the best cell
    let (mut best, mut lo_r, mut hi_r, mut lo_l, mut hi_l) = ((f64::MAX, 0.0, 0.0), -4.0, 2.0, -12.0, -5.0);
    let mut step = 0.05;
    for _ in 0..3 {
        let mut r = lo_r;
        while r <= hi_r + 1e-9 {
            let mut e = lo_l;
            while e <= hi_l + 1e-9 {
                let (esr, esl) = (10f64.powf(r), 10f64.powf(e));
                let k = cost(esr, esl);
                if k < best.0 {
                    best = (k, r, e);
                }
                e += step;
            }
            r += step;
        }
        let (_, br, be) = best;
        (lo_r, hi_r, lo_l, hi_l) = (br - step, br + step, be - step, be + step);
        step /= 10.0;
    }
    let (esr, esl) = (10f64.powf(best.1), 10f64.powf(best.2));
    let max_error_pct = points.iter().map(|&(f, z)| ((cap_impedance(c, esr, esl, f) - z) / z).abs() * 100.0).fold(0.0, f64::max);
    let f0 = srf(c, esl);
    let (fmin, fmax) = (points.iter().map(|p| p.0).fold(f64::MAX, f64::min), points.iter().map(|p| p.0).fold(0.0, f64::max));
    let mut warnings = vec![];
    if fmax < f0 * 0.5 {
        warnings.push(format!("表の最高周波数 ({}Hz) が自己共振 ({}Hz) よりだいぶ低いので、ESL は目安です。共振を過ぎた高い周波数の行を足すと確かになります", eng(fmax), eng(f0)));
    }
    if !points.iter().any(|p| p.0 > f0 / 3.0 && p.0 < f0 * 3.0) {
        warnings.push(format!("共振 ({}Hz) の近くの行がないので、ESR は目安です。インピーダンスが一番低い付近の行を足すと確かになります", eng(f0)));
    }
    if fmin > f0 {
        warnings.push("表の周波数がすべて共振より高く、容量の値を確かめられません。低い周波数の行も足してください".into());
    }
    if max_error_pct > 25.0 {
        warnings.push(format!("表とモデルの差が最大 {max_error_pct:.0}% あります。容量の値が表と合っているか (DCバイアス時の表なら、その時の容量) 確かめてください"));
    }
    Ok(CapFit { esr, esl, srf: f0, max_error_pct, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engineering_notation_reads_back() {
        assert_eq!(eng(100e-9), "100n");
        assert_eq!(eng(4.7e-6), "4.7u");
        assert_eq!(eng(2200.0), "2.2k");
        assert_eq!(eng(0.02), "20m");
        assert_eq!(eng(1e6), "1Meg");
        assert_eq!(eng(0.8e-9), "800p");
    }

    #[test]
    fn a_capacitor_model_has_its_parasitics_in_order() {
        let mut t = PassiveTemplate::new(PassiveKind::Capacitor, 100e-9);
        t.esr = Some(0.02);
        t.esl = Some(0.8e-9);
        t.bias_loss = 0.3;
        let g = t.generate("C 100n/0603").unwrap();
        assert_eq!(g.sim_name, "C_100n_0603");
        assert_eq!(g.sim_text, ".subckt C_100n_0603 1 2\nResr 1 n_esr 20m\nLesl n_esr n_esl 800p\nC1 n_esl 2 70n\n.ends");
        assert_eq!(g.value_text, "100n");
        assert_eq!(g.pins.len(), 2);
        assert!(g.limits.iter().any(|l| l.contains("下がった値")));
    }

    #[test]
    fn inductor_and_resistor_models() {
        let mut l = PassiveTemplate::new(PassiveKind::Inductor, 10e-6);
        l.dcr = Some(0.1);
        l.cp = Some(cp_from_srf(10e-6, 30e6).unwrap());
        let g = l.generate("L10u").unwrap();
        assert!(g.sim_text.contains("Rdcr 1 n_dcr 100m") && g.sim_text.contains("L1 n_dcr 2 10u") && g.sim_text.contains("Cp 1 2 "));
        let mut r = PassiveTemplate::new(PassiveKind::Resistor, 4700.0);
        r.esl = Some(2e-9);
        let g = r.generate("R4k7").unwrap();
        assert_eq!(g.sim_text, ".subckt R4k7 1 2\nLpar 1 n_esl 2n\nR1 n_esl 2 4.7k\n.ends");
    }

    #[test]
    fn bad_numbers_are_explained() {
        assert!(PassiveTemplate::new(PassiveKind::Capacitor, 0.0).generate("x").unwrap_err().contains("0 より大きい"));
        let mut t = PassiveTemplate::new(PassiveKind::Capacitor, 1e-6);
        t.esr = Some(-1.0);
        assert!(t.generate("x").unwrap_err().contains("ESR"));
        t.esr = None;
        t.bias_loss = 0.99;
        assert!(t.generate("x").unwrap_err().contains("0〜95"));
    }

    #[test]
    fn esr_and_esl_are_recovered_from_an_impedance_table() {
        let (c, esr, esl) = (100e-9, 0.03, 0.9e-9);
        let pts: Vec<(f64, f64)> = [1e5, 3e5, 1e6, 3e6, 1e7, 2.5e7, 6e7, 2e8].iter().map(|&f| (f, cap_impedance(c, esr, esl, f))).collect();
        let fit = fit_cap(c, &pts).unwrap();
        assert!((fit.esr / esr - 1.0).abs() < 0.03, "{fit:?}");
        assert!((fit.esl / esl - 1.0).abs() < 0.03, "{fit:?}");
        assert!(fit.max_error_pct < 3.0 && fit.warnings.is_empty(), "{fit:?}");
        assert!((fit.srf / srf(c, esl) - 1.0).abs() < 0.03);
    }

    #[test]
    fn a_table_that_misses_the_resonance_is_flagged() {
        let (c, esr, esl) = (1e-6, 0.01, 1e-9);
        let pts: Vec<(f64, f64)> = [1e3, 1e4, 1e5].iter().map(|&f| (f, cap_impedance(c, esr, esl, f))).collect();
        let fit = fit_cap(c, &pts).unwrap();
        assert!(fit.warnings.iter().any(|w| w.contains("ESL は目安")), "{fit:?}");
        assert!(fit_cap(c, &pts[..1]).is_err());
        assert!(fit_cap(c, &[(1e3, 1.0), (0.0, 1.0)]).is_err());
    }

    #[test]
    fn srf_helpers_are_inverses() {
        let esl = esl_from_srf(10e-9, 50e6).unwrap();
        assert!((srf(10e-9, esl) - 50e6).abs() < 1.0);
        assert!(cp_from_srf(0.0, 1e6).is_err());
    }
}
