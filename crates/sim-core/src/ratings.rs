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
    // a motor turns most of its power into motion: only its winding resistance makes heat
    if dc.element(&format!("{}.LW", c.refdes)).is_some()
        && let Some(w) = dc.element(&format!("{}.RW", c.refdes))
    {
        return Some(w.power);
    }
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
                // a part on a heat sink is not held to its package's free-air guess: what the sink lets it dissipate at 25 °C around
                // ((highest temperature − 25) / θ with the sink)
                let on_sink = (c.heatsink > 0.0).then(|| {
                    let junctions = usize::from(matches!(c.kind, Kind::Npn | Kind::Diode | Kind::Custom));
                    (temperature_limit(c, junctions).t_max - 25.0) / crate::part_theta(c, &Default::default())
                });
                let (Some(p), Some(limit)) = (dissipation(dc, c), from_sheet.or(on_sink).or_else(|| package_power_limit(&fp))) else { continue };
                let Some(lv) = level(p, limit) else { continue };
                let word = if lv == RatingLevel::Over { "目安を超えています" } else { "余裕が少ないです" };
                let basis = if from_sheet.is_some() { "最大定格" } else if on_sink.is_some() { "放熱板つきの目安" } else { "この形の目安" };
                let motor = dc.element(&format!("{}.LW", c.refdes)).is_some();
                let hint = if motor {
                    "モーターの巻線の発熱 (電流² × 巻線の抵抗) です。負荷が重い・軸が止まっていると電流が増えて熱くなります。負荷を軽くするか、電圧を下げるか、大きいモーターにします"
                } else if c.kind == Kind::Resistor {
                    "抵抗値を大きくして電流を減らすか、大きい形(1206 / 2512 や 1/2W 品)に変えます。発熱は P = V × I = I² × R で決まります"
                } else {
                    "電圧差を減らす(入力電圧を下げる / 電流を減らす)か、放熱板つきの形(TO-220 など)にして放熱板を付けます (自己発熱の表の熱抵抗の横の「放熱」に放熱板の熱抵抗を入れると、それで判定します)。LDO やリニアレギュレータは (入力 − 出力) × 電流 が熱になります"
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

/// Where a part's temperature limit was taken from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitSource {
    /// Given on the part in the schematic (`Component.t_max`).
    Part,
    /// The library part's data sheet ratings (`PartRatings.t_max`).
    Datasheet,
    /// A typical value for the kind of part: to be confirmed.
    Guess,
}

/// The highest temperature a part may reach, and the surrounding temperature its rated power starts to fall from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TempLimit {
    pub t_max: f64,
    pub derate_from: Option<f64>,
    pub source: LimitSource,
}

/// A part's temperature limit: its own setting, else the library part's ratings, else a typical value for its kind (a chip resistor's
/// element 155 °C with the power lowered from 70 °C, a diode or transistor's junction 150 °C, an LED 100 °C, an IC 125 °C). `junctions`
/// is how many diodes or transistors the part holds (a library part without any is taken as a resistor).
pub fn temperature_limit(comp: &Component, junctions: usize) -> TempLimit {
    let rated = comp.custom.as_ref().and_then(|c| c.ratings.as_ref());
    let guess = match comp.kind {
        Kind::Resistor => (155.0, Some(70.0)),
        Kind::Diode | Kind::Npn => (150.0, Some(25.0)),
        Kind::Led => (100.0, None),
        _ if junctions > 0 => (125.0, None),
        _ => (155.0, Some(70.0)),
    };
    let derate_from = rated.and_then(|r| r.derate_from).or(guess.1);
    if comp.t_max > 0.0 {
        TempLimit { t_max: comp.t_max, derate_from, source: LimitSource::Part }
    } else if let Some(t) = rated.and_then(|r| r.t_max) {
        TempLimit { t_max: t, derate_from, source: LimitSource::Datasheet }
    } else {
        TempLimit { t_max: guess.0, derate_from, source: LimitSource::Guess }
    }
}

/// One warmed part against its temperature limit.
#[derive(Clone, Debug)]
pub struct TempCheck {
    pub name: String,
    pub temp_c: f64,
    pub limit: TempLimit,
    /// How far below the limit it stays (K; negative when over).
    pub margin_k: f64,
    /// The rated power lowered for the surrounding temperature (W), when the part has a rated power and the surroundings are above
    /// the temperature it starts to fall from.
    pub p_derated: Option<f64>,
    /// `None`: fine. `Caution`: above the goal (20 % of the way from the surroundings to the limit, as in the calculators). `Over`: past the
    /// limit, or more power than the lowered rating.
    pub level: Option<RatingLevel>,
    pub message: String,
    pub hint: String,
}

/// The self-heating result checked against each part's temperature limit and, above the derating temperature, its lowered rated power.
pub fn check_temperatures(sch: &Schematic, parts: &[crate::HeatedPart], t_amb: f64) -> Vec<TempCheck> {
    let mut out = vec![];
    let flat = sch.flat();
    for p in parts {
        let Some(comp) = flat.components.iter().find(|c| c.refdes == p.name) else { continue };
        let limit = temperature_limit(comp, p.junctions);
        let margin_k = limit.t_max - p.temp_c;
        let goal = limit.t_max - 0.2 * (limit.t_max - t_amb);
        // the rated power: the data sheet's, or for a resistor the package's typical one
        let rated = comp.custom.as_ref().and_then(|c| c.ratings.as_ref()).and_then(|r| r.p_max).or_else(|| (comp.kind == Kind::Resistor).then(|| package_power_limit(&comp.footprint_name())).flatten());
        let p_derated = match (rated, limit.derate_from) {
            (Some(pr), Some(d)) if t_amb > d && limit.t_max > d => Some(pr * ((limit.t_max - t_amb) / (limit.t_max - d)).max(0.0)),
            _ => None,
        };
        let guessed = if limit.source == LimitSource::Guess { " (上限は部品の種類からの目安。データシートの値を入れてください)" } else { "" };
        let (level, message, hint) = if p.temp_c > limit.t_max {
            (
                Some(RatingLevel::Over),
                format!("{} は {:.0} ℃ になり、上限 {:.0} ℃ を {:.0} K 超えます{guessed}", p.name, p.temp_c, limit.t_max, -margin_k),
                "このままでは壊れるか、寿命が大きく縮みます。熱抵抗を下げる (銅を広げる・放熱板。「値の決め方」の「放熱」で面積が分かります)、損失を減らす、定格の大きい部品にする、のどれかです".to_string(),
            )
        } else if p_derated.is_some_and(|pd| p.power > pd) {
            let pd = p_derated.unwrap_or_default();
            (
                Some(RatingLevel::Over),
                format!("{} の電力 {:.3} W は、周囲 {t_amb:.0} ℃ で下げた定格 {pd:.3} W を超えます (定格は {:.0} ℃ から下がります){guessed}", p.name, p.power, limit.derate_from.unwrap_or_default()),
                "まわりが熱いと、部品が出してよい電力は下がります (データシートの低減曲線)。定格の大きい部品にするか、損失を減らします".to_string(),
            )
        } else if p.temp_c > goal {
            (
                Some(RatingLevel::Caution),
                format!("{} は {:.0} ℃ で、上限 {:.0} ℃ まで {margin_k:.0} K しかありません (目安は {goal:.0} ℃ 以下){guessed}", p.name, p.temp_c, limit.t_max),
                "動きますが余裕が少なく、箱に入れる・夏の暑さで上限に近づきます。熱抵抗を下げるか、損失を減らすと安心です".to_string(),
            )
        } else {
            (None, format!("{} は {:.0} ℃ (上限 {:.0} ℃ まで {margin_k:.0} K){guessed}", p.name, p.temp_c, limit.t_max), String::new())
        };
        out.push(TempCheck { name: p.name.clone(), temp_c: p.temp_c, limit, margin_k, p_derated, level, message, hint });
    }
    out.sort_by(|a, b| b.level.cmp(&a.level).then(a.margin_k.total_cmp(&b.margin_k)));
    out
}

/// The temperature-over-time result checked the same way: each part's highest temperature against its limit, and its mean power against
/// the rating lowered for the surroundings (a rated power is for continuous power). The message says when the peak came.
pub fn check_peak_temperatures(sch: &Schematic, parts: &[crate::HeatTranPart], t_amb: f64) -> Vec<TempCheck> {
    let as_steady: Vec<crate::HeatedPart> = parts
        .iter()
        .map(|p| crate::HeatedPart { name: p.name.clone(), power: p.power_mean, rise: p.rise_peak, temp_c: t_amb + p.rise_peak, change: p.change, theta: p.theta, tc1: 0.0, tc2: 0.0, from_others: 0.0, junctions: p.junctions, junction_v: None })
        .collect();
    let mut out = check_temperatures(sch, &as_steady, t_amb);
    for c in &mut out {
        if let Some(p) = parts.iter().find(|p| p.name == c.name) {
            c.message = format!("{}。いちばん熱いのは {:.3} 秒", c.message, p.peak_time);
        }
    }
    out
}

#[cfg(test)]
mod temp_tests {
    use super::*;

    fn part(name: &str, temp_c: f64, power: f64, junctions: usize) -> crate::HeatedPart {
        crate::HeatedPart { name: name.into(), power, rise: temp_c - 25.0, temp_c, change: 0.0, theta: 100.0, tc1: 0.0, tc2: 0.0, from_others: 0.0, junctions, junction_v: None }
    }

    #[test]
    fn a_part_is_judged_against_its_own_limit_then_the_library_then_a_guess() {
        let mut sch = Schematic::default();
        let r = sch.add(Kind::Resistor, (0, 0), 0);
        let q = sch.add(Kind::Npn, (20, 0), 0);
        let (rn, qn) = (sch.components[r].refdes.clone(), sch.components[q].refdes.clone());
        // guesses: the resistor's element 155 C, the transistor's junction 150 C
        let checks = check_temperatures(&sch, &[part(&rn, 60.0, 0.01, 0), part(&qn, 160.0, 0.5, 1)], 25.0);
        let (cr, cq) = (checks.iter().find(|c| c.name == rn).unwrap(), checks.iter().find(|c| c.name == qn).unwrap());
        assert_eq!((cr.limit.t_max, cr.limit.source, cr.level), (155.0, LimitSource::Guess, None));
        assert_eq!((cq.limit.t_max, cq.level), (150.0, Some(RatingLevel::Over)));
        assert!(cq.message.contains("10 K 超え") && cq.message.contains("目安"), "{}", cq.message);
        assert_eq!(checks[0].name, qn, "the worst first");
        // its own limit wins; 130 C on 150 C with the surroundings at 25 C is past the goal of 125 C
        sch.components[q].t_max = 150.0;
        let c = check_temperatures(&sch, &[part(&qn, 130.0, 0.5, 1)], 25.0).remove(0);
        assert_eq!((c.limit.source, c.level), (LimitSource::Part, Some(RatingLevel::Caution)));
        assert!(!c.message.contains("種類からの目安"), "{}", c.message);
    }

    #[test]
    fn the_peak_of_a_pulse_is_what_meets_the_limit() {
        // a transistor whose pulse takes it to 170 C for a moment, though on average it stays far cooler
        let mut sch = Schematic::default();
        let q = sch.add(Kind::Npn, (0, 0), 0);
        let name = sch.components[q].refdes.clone();
        let p = crate::HeatTranPart { name: name.clone(), theta: 300.0, cth: 0.01, power_peak: 2.0, power_mean: 0.05, rise_peak: 145.0, peak_time: 0.012, rise_end: 3.0, rise_alone: 145.0, temp_working_c: 80.0, change: 0.0, junctions: 1, stages: 1, power: vec![], rise: vec![] };
        let c = check_peak_temperatures(&sch, &[p], 25.0).remove(0);
        assert_eq!((c.temp_c, c.limit.t_max, c.level), (170.0, 150.0, Some(RatingLevel::Over)));
        assert!(c.message.contains("0.012 秒"), "{}", c.message);
    }

    #[test]
    fn a_resistor_in_a_hot_place_gets_a_lower_rated_power() {
        // 0603 (0.1 W typical) at 100 C around: 0.1 x (155 - 100) / (155 - 70) = 0.0647 W, so 0.08 W is too much even though
        // the element itself stays under 155 C
        let mut sch = Schematic::default();
        let r = sch.add(Kind::Resistor, (0, 0), 0);
        sch.components[r].footprint = "R_0603".into();
        let name = sch.components[r].refdes.clone();
        let c = check_temperatures(&sch, &[part(&name, 140.0, 0.08, 0)], 100.0).remove(0);
        assert!((c.p_derated.unwrap() - 0.1 * 55.0 / 85.0).abs() < 1e-9, "{c:?}");
        assert_eq!(c.level, Some(RatingLevel::Over));
        assert!(c.message.contains("下げた定格"), "{}", c.message);
    }
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
            // regulator_heatsink's U1 is the regulator without a heat sink, there to be too hot beside U2 on its sink
            let meant = |i: &RatingIssue| e.file_name() == "regulator_heatsink.schjson" && i.refdes == "U1";
            let bad: Vec<_> = check_ratings(&s, &dc).into_iter().filter(|i| i.level == RatingLevel::Over && !meant(i)).collect();
            assert!(bad.is_empty(), "{}: {:?}", e.file_name().to_string_lossy(), bad);
        }
    }
}
