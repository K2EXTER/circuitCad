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
        ("AXIAL", 0.25),
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

/// Over the limit, or past `caution` (a fraction) of it.
fn level_at(value: f64, limit: f64, caution: f64) -> Option<RatingLevel> {
    if value > limit {
        Some(RatingLevel::Over)
    } else if value > caution * limit {
        Some(RatingLevel::Caution)
    } else {
        None
    }
}

/// Power: half the limit is already "little margin" (parts run hot long before they fail).
fn level(value: f64, limit: f64) -> Option<RatingLevel> {
    level_at(value, limit, 0.5)
}

fn fmt_num(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

// ------------------------------------------------------------------ what a part has to withstand here

/// What one part sees in a DC solution.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stress {
    /// Voltage across a one-element model (a transistor's Vce / Vds, a diode's forward drop).
    pub v: Option<f64>,
    /// Current through the part, absolute.
    pub i: Option<f64>,
    /// Power it dissipates.
    pub p: Option<f64>,
    /// The largest node voltage anywhere in the circuit: the most any part can be asked to block.
    pub v_circuit: f64,
}

/// The stress on `refdes` in the DC result `dc`.
pub fn stress(dc: &DcResult, refdes: &str) -> Stress {
    let own = dc.element(refdes);
    let prefix = format!("{refdes}.");
    let inner: Vec<f64> = dc.elements.iter().filter(|e| e.name.starts_with(&prefix)).map(|e| e.power).collect();
    let p = match (own, inner.is_empty()) {
        (Some(e), true) => Some(e.power),
        (_, false) => Some(inner.iter().sum::<f64>() + own.map_or(0.0, |e| e.power)),
        (None, true) => None,
    };
    Stress {
        v: own.map(|e| e.voltage.abs()),
        i: own.map(|e| e.current.abs()),
        p: p.map(|p| p.max(0.0)),
        v_circuit: dc.voltages.iter().filter(|(n, _)| !n.contains('.')).map(|(_, v)| v.abs()).fold(0.0, f64::max),
    }
}

/// What a part must be rated for here, with the margin a beginner should keep (voltage / current: ×margin
/// of what the circuit shows; power: ×margin).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Requirement {
    pub v_min: f64,
    pub i_min: f64,
    pub p_min: f64,
}

pub fn requirement(st: &Stress, margin: f64) -> Requirement {
    Requirement { v_min: st.v_circuit * margin, i_min: st.i.unwrap_or(0.0) * margin, p_min: st.p.unwrap_or(0.0) * margin }
}

/// Does a part with these data sheet ratings meet the requirement? A rating the sheet does not give
/// cannot be checked, so it counts as "not met" only for the figures that matter here (those above zero).
pub fn meets(req: &Requirement, r: &sch_core::PartRatings, check_voltage: bool) -> bool {
    let ok = |need: f64, have: Option<f64>| need <= 1e-12 || have.is_some_and(|h| h >= need);
    (!check_voltage || ok(req.v_min, r.v_max)) && ok(req.i_min, r.i_max) && ok(req.p_min, r.p_max)
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
        // data sheet maximum voltage / current against what this circuit puts on a one-element model
        // (a transistor's Vce / Vds and its current; a diode's voltage is the forward drop, not the reverse stress)
        if let (Some(rt), Some(e)) = (c.custom.as_ref().and_then(|s| s.ratings.as_ref()), dc.element(&c.refdes)) {
            let element = c.custom.as_ref().and_then(|s| s.sim.as_ref()).map(|m| m.element.to_ascii_uppercase()).unwrap_or_default();
            if let Some(vmax) = rt.v_max.filter(|_| matches!(element.as_str(), "Q" | "M" | "J")) {
                let v = e.voltage.abs();
                if let Some(lv) = level_at(v, vmax, 0.8) {
                    push(
                        lv,
                        format!("{} の電圧 {:.1} V は、最大定格 {} V の {:.0}% です{}", c.refdes, v, fmt_num(vmax), v / vmax * 100.0, if lv == RatingLevel::Over { " (超えています)" } else { " (余裕が少ないです)" }),
                        "電圧には、最大定格の 8 割以下に収まるように余裕をとります。電源電圧を下げるか、耐圧の大きい部品を選びます。",
                    );
                }
            }
            if let Some(imax) = rt.i_max {
                let i = e.current.abs();
                if let Some(lv) = level_at(i, imax, 0.8) {
                    push(
                        lv,
                        format!("{} の電流 {:.0} mA は、最大定格 {:.0} mA の {:.0}% です{}", c.refdes, i * 1e3, imax * 1e3, i / imax * 100.0, if lv == RatingLevel::Over { " (超えています)" } else { " (余裕が少ないです)" }),
                        "電流には、最大定格の 8 割以下に収まるように余裕をとります。負荷の電流を減らすか、電流の大きい部品を選びます。",
                    );
                }
            }
        }
        match c.kind {
            Kind::Resistor | Kind::Npn | Kind::Custom | Kind::Diode => {
                // the data sheet's maximum power when the part has one, else the usual limit of its package
                let from_sheet = c.custom.as_ref().and_then(|s| s.ratings.as_ref()).and_then(|r| r.p_max);
                let (Some(p), Some(limit)) = (dissipation(dc, c), from_sheet.or_else(|| package_power_limit(&fp))) else { continue };
                let Some(lv) = level(p, limit) else { continue };
                let word = if lv == RatingLevel::Over { "目安を超えています" } else { "余裕が少ないです" };
                let basis = if from_sheet.is_some() { "最大定格" } else { "この形の目安" };
                let hint = if c.kind == Kind::Resistor {
                    "抵抗値を大きくして電流を減らすか、大きい形(1206 / 2512 や 1/2W 品)に変えます。発熱は P = V × I = I² × R で決まります"
                } else {
                    "電圧差を減らす(入力電圧を下げる / 電流を減らす)か、放熱板つきの形(TO-220 など)にします。LDO やリニアレギュレータは (入力 − 出力) × 電流 が熱になります"
                };
                push(
                    lv,
                    format!("{} ({fp}) の消費電力は {:.0} mW で、{basis} {:.0} mW に対して{word}", c.refdes, p * 1e3, limit * 1e3),
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
    out.sort_by_key(|i| std::cmp::Reverse(i.level));
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
mod sheet {
    use super::*;
    use crate::{dc_op, from_schematic};

    fn load(json: &str) -> Schematic {
        serde_json::from_str(json).unwrap()
    }

    /// A part that carries data sheet ratings is judged against them, not against its package.
    #[test]
    fn sheet_ratings_are_used_and_named_in_the_message() {
        let mut f = load(include_str!("../../../docs/examples/emitter_follower.schjson"));
        let r2 = f.components.iter_mut().find(|c| c.refdes == "R2").unwrap();
        r2.value = "22".into();
        let dc = dc_op(&from_schematic(&f).unwrap()).unwrap();
        let issues = check_ratings(&f, &dc);
        let q = issues.iter().find(|i| i.refdes == "Q1").expect("2N3904 at 0.9 W");
        assert_eq!(q.level, RatingLevel::Over);
        assert!(q.message.contains("最大定格") && q.message.contains("625"), "{}", q.message);
    }

    /// Current against the sheet's maximum: a 2N7000 (0.2 A) asked for 365 mA.
    #[test]
    fn a_mosfet_past_its_maximum_current_is_flagged() {
        let mut m = load(include_str!("../../../docs/examples/mosfet_switch.schjson"));
        m.components.iter_mut().find(|c| c.refdes == "V2").unwrap().value = "5".into();
        m.components.iter_mut().find(|c| c.refdes == "R2").unwrap().value = "10".into();
        let dc = dc_op(&from_schematic(&m).unwrap()).unwrap();
        let issues = check_ratings(&m, &dc);
        assert!(issues.iter().any(|i| i.refdes == "Q1" && i.level == RatingLevel::Over && i.message.contains("電流") && i.message.contains("200 mA")), "{issues:?}");
        // the same part at 100 mA is below 80 % of 200 mA... but past half its power? no: all quiet
        m.components.iter_mut().find(|c| c.refdes == "R2").unwrap().value = "47".into();
        let dc = dc_op(&from_schematic(&m).unwrap()).unwrap();
        assert!(check_ratings(&m, &dc).iter().all(|i| i.refdes != "Q1"), "100 mA is within the sheet's limits");
    }

    /// A diode's DC voltage is its forward drop: it must not be mistaken for the reverse stress.
    #[test]
    fn a_diode_is_not_judged_by_its_forward_voltage() {
        let d = load(include_str!("../../../docs/examples/diode_iv.schjson"));
        let dc = dc_op(&from_schematic(&d).unwrap()).unwrap();
        assert!(check_ratings(&d, &dc).is_empty());
        // even with a 1N4148-style 100 V rating, 0.7 V forward is nothing
        let st = stress(&dc, "D1");
        assert!(st.v.unwrap() < 1.0 && st.i.unwrap() > 0.01 && st.v_circuit >= 3.0 - 1e-9);
    }

    /// What a part must be rated for here, and whether library-style ratings meet it.
    #[test]
    fn requirements_keep_a_margin_of_two() {
        let f = load(include_str!("../../../docs/examples/emitter_follower.schjson"));
        let dc = dc_op(&from_schematic(&f).unwrap()).unwrap();
        let st = stress(&dc, "Q1");
        let req = requirement(&st, 2.0);
        assert!((req.v_min - 2.0 * 9.0).abs() < 1e-6, "the supply is 9 V: {req:?}");
        assert!((req.p_min - 2.0 * st.p.unwrap()).abs() < 1e-12 && req.i_min > 0.03);
        let ok = sch_core::PartRatings { v_max: Some(40.0), i_max: Some(0.2), p_max: Some(0.625), ..Default::default() };
        let small = sch_core::PartRatings { v_max: Some(12.0), i_max: Some(0.2), p_max: Some(0.625), ..Default::default() };
        let unknown = sch_core::PartRatings::default();
        assert!(meets(&req, &ok, true) && !meets(&req, &small, true), "12 V is below twice the supply");
        assert!(!meets(&req, &unknown, true), "a rating the sheet does not give cannot be taken as met");
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
