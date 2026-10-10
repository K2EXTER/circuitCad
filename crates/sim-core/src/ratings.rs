//! Rule-of-thumb rating checks on a DC result: a resistor that cooks, an LED without a limiter,
//! a part running past what its package can shed. The limits are typical values for the package
//! (not a datasheet), so every message says to confirm with the datasheet.

use crate::dc::DcResult;
use sch_core::{Component, Kind, Schematic};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RatingLevel {
    /// Above 50% of the typical limit: works, but runs hot / has no margin.
    Caution,
    /// Above the typical limit: likely to be damaged.
    Over,
}

#[derive(Clone, Debug)]
pub struct RatingIssue {
    pub refdes: String,
    pub level: RatingLevel,
    pub message: String,
    pub hint: String,
}

/// Typical continuous power (W) a footprint can shed without a heat sink; `None` if unknown.
pub fn package_power_limit(footprint: &str) -> Option<f64> {
    let f = footprint.to_ascii_uppercase();
    let table: &[(&str, f64)] = &[
        ("0201", 0.05),
        ("0402", 0.0625),
        ("0603", 0.1),
        ("0805", 0.125),
        ("1206", 0.25),
        ("1210", 0.5),
        ("2010", 0.75),
        ("2512", 1.0),
        ("SOT-23", 0.25),
        ("SOT-223", 1.0),
        ("TO-92", 0.5),
        ("TO-220", 2.0),
        ("SOIC", 0.5),
        ("SMA", 1.0),
        ("DO-41", 1.0),
        ("DO-35", 0.4),
        ("DIP", 1.0),
    ];
    // SOT-23-5 etc. share the SOT-23 prefix; longer keys first so SOT-223 wins over SOT-23
    let mut keys: Vec<_> = table.iter().collect();
    keys.sort_by_key(|(k, _)| std::cmp::Reverse(k.len()));
    keys.into_iter().find(|(k, _)| f.contains(k)).map(|(_, w)| *w)
}

fn level(value: f64, limit: f64) -> Option<RatingLevel> {
    if value > limit {
        Some(RatingLevel::Over)
    } else if value > 0.5 * limit {
        Some(RatingLevel::Caution)
    } else {
        None
    }
}

/// Power dissipated inside a component: its own element, or the net of a sub-circuit's elements.
fn dissipation(dc: &DcResult, c: &Component) -> Option<f64> {
    let own = dc.element(&c.refdes).map(|e| e.power);
    let prefix = format!("{}.", c.refdes);
    let inner: Vec<f64> = dc.elements.iter().filter(|e| e.name.starts_with(&prefix)).map(|e| e.power).collect();
    match (own, inner.is_empty()) {
        (Some(p), true) => Some(p),
        (_, false) => Some(inner.iter().sum::<f64>() + own.unwrap_or(0.0)),
        (None, true) => None,
    }
}

pub fn check_ratings(sch: &Schematic, dc: &DcResult) -> Vec<RatingIssue> {
    let mut out = vec![];
    for c in &sch.components {
        let fp = c.footprint_name();
        let mut push = |level: RatingLevel, message: String, hint: &str| {
            out.push(RatingIssue { refdes: c.refdes.clone(), level, message, hint: hint.to_string() });
        };
        match c.kind {
            Kind::Resistor | Kind::Npn | Kind::Custom | Kind::Diode => {
                let (Some(p), Some(limit)) = (dissipation(dc, c), package_power_limit(&fp)) else { continue };
                // a custom part's package may be a through-hole with a lead-free body: use the footprint table
                let Some(lv) = level(p, limit) else { continue };
                let word = if lv == RatingLevel::Over { "目安を超えています" } else { "余裕が少ないです" };
                let hint = if c.kind == Kind::Resistor {
                    "抵抗値を大きくして電流を減らすか、大きい形(1206 / 2512 や 1/2W 品)に変えます。発熱は P = V × I = I² × R で決まります"
                } else {
                    "電圧差を減らす(入力電圧を下げる / 電流を減らす)か、放熱板つきの形(TO-220 など)にします。LDO やリニアレギュレータは (入力 − 出力) × 電流 が熱になります"
                };
                push(
                    lv,
                    format!("{} ({fp}) の消費電力は {:.0} mW で、この形の目安 {:.0} mW に対して{word}", c.refdes, p * 1e3, limit * 1e3),
                    &format!("{hint}。実際の定格はデータシートで確認してください。"),
                );
            }
            Kind::Led => {
                let Some(e) = dc.element(&c.refdes) else { continue };
                if e.current > 0.02 {
                    let lv = if e.current > 0.03 { RatingLevel::Over } else { RatingLevel::Caution };
                    push(
                        lv,
                        format!("{} の電流が {:.1} mA あります。一般的な LED は 20 mA 前後が上限です", c.refdes, e.current * 1e3),
                        "電流制限抵抗を大きくします。R = (電源電圧 − LED の順電圧) ÷ 目標電流 で決めます (「学ぶ → 値の決め方」が使えます)。",
                    );
                }
                if e.voltage < -5.0 {
                    push(
                        RatingLevel::Over,
                        format!("{} に逆向きに {:.1} V かかっています。LED の逆耐圧は 5 V 程度です", c.refdes, -e.voltage),
                        "向きを確認するか、逆電圧がかかる回路では保護ダイオードを並べます。",
                    );
                }
            }
            Kind::Capacitor => {
                let Some(e) = dc.element(&c.refdes) else { continue };
                let v = e.voltage.abs();
                if v > 16.0 && fp.to_ascii_uppercase().starts_with("C_0") {
                    let lv = if v > 25.0 { RatingLevel::Over } else { RatingLevel::Caution };
                    push(
                        lv,
                        format!("{} に {:.1} V かかっています。小さいチップコンデンサの耐圧は 16〜50 V 程度が多いです", c.refdes, v),
                        "かかる電圧の 2 倍ほどの耐圧がある品を選びます。電解コンデンサは極性も確認してください。",
                    );
                }
            }
            _ => {}
        }
    }
    out.sort_by(|a, b| b.level.cmp(&a.level));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dc_op, from_schematic};

    fn sch(json: &str) -> Schematic {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn limits_follow_the_package() {
        assert_eq!(package_power_limit("R_0805"), Some(0.125));
        assert_eq!(package_power_limit("SOT-223"), Some(1.0));
        assert_eq!(package_power_limit("SOT-23-5_P0.95"), Some(0.25));
        assert_eq!(package_power_limit("Transformer_EI30"), None);
    }

    /// 12 V across a 0805 100 ohm resistor is 1.44 W: far past 125 mW.
    #[test]
    fn a_hot_resistor_and_a_limiterless_led_are_flagged() {
        let s = sch(include_str!("../../../docs/examples/led.schjson"));
        let mut hot = s.clone();
        for c in &mut hot.components {
            if c.kind == Kind::VSource {
                c.value = "12".into();
            }
            if c.kind == Kind::Resistor {
                c.value = "100".into();
            }
        }
        let dc = dc_op(&from_schematic(&hot).unwrap()).unwrap();
        let issues = check_ratings(&hot, &dc);
        assert!(issues.iter().any(|i| i.level == RatingLevel::Over), "{issues:?}");
        assert!(issues.iter().any(|i| i.message.contains("mA")), "{issues:?}");
        // the shipped sample is a sane circuit
        let dc = dc_op(&from_schematic(&s).unwrap()).unwrap();
        assert!(check_ratings(&s, &dc).is_empty(), "{:?}", check_ratings(&s, &dc));
    }
}

#[cfg(test)]
mod examples {
    use super::*;
    use crate::{dc_op, from_schematic};
    #[test]
    fn shipped_examples_stay_within_ratings() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        for e in std::fs::read_dir(dir).unwrap().flatten().filter(|e| e.path().extension().is_some_and(|x| x == "schjson")) {
            let s: Schematic = serde_json::from_str(&std::fs::read_to_string(e.path()).unwrap()).unwrap();
            let Ok(c) = from_schematic(&s) else { continue };
            let Ok(dc) = dc_op(&c) else { continue };
            let bad: Vec<_> = check_ratings(&s, &dc).into_iter().filter(|i| i.level == RatingLevel::Over).collect();
            assert!(bad.is_empty(), "{}: {:?}", e.file_name().to_string_lossy(), bad);
        }
    }
}
