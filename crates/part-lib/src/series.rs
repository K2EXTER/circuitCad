//! A maker's series of parts that differ only by their value (a chip resistor in every E24 / E96 value), written once as a
//! `*.series.json` file and expanded into one library part per value. The library list shows the series as one row with a
//! value field; the schematic, the board and the CLI see ordinary parts named by their maker's part number.
//! Format and the series bundled so far: docs/part-format.md, docs/code-notes/part-lib.md.

use crate::PartDef;
use pcb_core::package::PackageSpec;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

pub const SERIES_SUFFIX: &str = ".series.json";

/// The E24 values of a decade, times 100 (1.0, 1.1, ... 9.1).
pub const E24: [u16; 24] = [100, 110, 120, 130, 150, 160, 180, 200, 220, 240, 270, 300, 330, 360, 390, 430, 470, 510, 560, 620, 680, 750, 820, 910];

/// The E96 values of a decade, times 100 (1.00, 1.02, ... 9.76).
pub const E96: [u16; 96] = [
    100, 102, 105, 107, 110, 113, 115, 118, 121, 124, 127, 130, 133, 137, 140, 143, 147, 150, 154, 158, 162, 165, 169, 174, 178, 182, 187, 191, 196, 200, 205,
    210, 215, 221, 226, 232, 237, 243, 249, 255, 261, 267, 274, 280, 287, 294, 301, 309, 316, 324, 332, 340, 348, 357, 365, 374, 383, 392, 402, 412, 422,
    432, 442, 453, 464, 475, 487, 499, 511, 523, 536, 549, 562, 576, 590, 604, 619, 634, 649, 665, 681, 698, 715, 732, 750, 768, 787, 806, 825, 845, 866,
    887, 909, 931, 953, 976,
];

/// Which values the series is made in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ValueRange {
    /// "E24", "E96" (several are joined).
    pub sets: Vec<String>,
    /// Smallest and largest value, ohms.
    pub min: f64,
    pub max: f64,
}

/// A range of values (ohms, both ends included) with its own figures.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Band {
    pub min: f64,
    pub max: f64,
    #[serde(default)]
    pub tc_ppm: Option<f64>,
    /// The coefficient as the data sheet writes it when it is not the same either way ("+350〜−450"): `tc_ppm` is then its
    /// largest size.
    #[serde(default)]
    pub tc_text: Option<String>,
    #[serde(default)]
    pub p_max: Option<f64>,
}

/// One series file. Only resistors for now (P2 in docs/candidates.md); capacitors come with their own fields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeriesDef {
    /// The series' name as the maker writes it ("ERJ-3EKF").
    pub series: String,
    pub maker: String,
    pub category: String,
    /// One line: kind, tolerance, size.
    pub summary: String,
    /// The size in inches ("0603"), shown on the series' row in the library list.
    #[serde(default)]
    pub size: String,
    pub datasheet: String,
    /// Which documents the figures were read from, and when.
    pub checked: String,
    pub values: ValueRange,
    /// The part number with `{code}` where the value's code goes ("ERJ-3EKF{code}V").
    pub mpn: String,
    /// How the value is written in the part number: "sig3" = three significant figures and the number of zeros, `R` for the
    /// decimal point below 100 (`1002` = 10 kΩ, `10R0` = 10 Ω; Panasonic ERJ, KOA RK73H, ROHM MCR).
    pub code: String,
    pub tolerance_pct: f64,
    /// The temperature coefficient's limit (either way), ppm/K.
    pub tc_ppm: f64,
    /// Rated power at up to `derate_from_c`, watts.
    pub p_max: f64,
    /// Limiting element voltage: the rated voltage is the smaller of this and √(P·R).
    pub v_limit: f64,
    #[serde(default)]
    pub v_overload: f64,
    pub derate_from_c: f64,
    /// Where the derating line reaches 0 W.
    pub derate_zero_c: f64,
    pub temp_min_c: f64,
    pub temp_max_c: f64,
    /// Ranges of value with their own temperature coefficient or rated power (KOA RK73H: ±100 ppm/K from 10 Ω to 1 MΩ, ±200 below
    /// and above; 0.125 W up to 1 kΩ in 0603). A value outside every band takes `tc_ppm` and `p_max`.
    #[serde(default)]
    pub bands: Vec<Band>,
    /// Quality marks from the data sheet ("AEC-Q200 Grade 0", "耐硫化"): shown on the series' row.
    #[serde(default)]
    pub quality: Vec<String>,
    pub height: f64,
    pub footprint_name: String,
    pub package: PackageSpec,
    /// Where the land pattern comes from.
    pub land: String,
    /// The body's size from the data sheet, in words.
    #[serde(default)]
    pub body: String,
    /// The body drawn on the board's silkscreen and in 3D (a leaded part lying down: its length and diameter).
    #[serde(default)]
    pub outline: Option<pcb_core::body::BodySpec>,
}

/// The series that ship with the program (`crates/part-lib/series/`).
const BUNDLED: &[(&str, &str)] = &[
    ("ERJ-1GJF", include_str!("../series/ERJ-1GJF.series.json")),
    ("ERJ-2RKF", include_str!("../series/ERJ-2RKF.series.json")),
    ("ERJ-3EKF", include_str!("../series/ERJ-3EKF.series.json")),
    ("ERJ-6ENF", include_str!("../series/ERJ-6ENF.series.json")),
    ("RK73H1E", include_str!("../series/RK73H1E.series.json")),
    ("RK73H1J", include_str!("../series/RK73H1J.series.json")),
    ("RK73H2A", include_str!("../series/RK73H2A.series.json")),
    ("RK73H2B", include_str!("../series/RK73H2B.series.json")),
    ("CF1/4", include_str!("../series/CF1_4.series.json")),
    ("MF1/4D", include_str!("../series/MF1_4D.series.json")),
];

/// The bundled series, read once.
pub fn bundled() -> &'static [SeriesDef] {
    static SERIES: OnceLock<Vec<SeriesDef>> = OnceLock::new();
    SERIES.get_or_init(|| BUNDLED.iter().map(|(name, text)| serde_json::from_str(text).unwrap_or_else(|e| panic!("bundled series {name}: {e}"))).collect())
}

/// Every part of the bundled series, made once.
pub fn bundled_parts() -> &'static [PartDef] {
    static PARTS: OnceLock<Vec<PartDef>> = OnceLock::new();
    PARTS.get_or_init(|| bundled().iter().flat_map(|s| s.expand()).collect())
}

/// The series a part belongs to and its value, by the part's name (`ERJ-3EKF1002V` → (ERJ-3EKF, 10 000)).
pub fn member(name: &str) -> Option<(&'static SeriesDef, f64)> {
    static INDEX: OnceLock<HashMap<String, (usize, f64)>> = OnceLock::new();
    let index = INDEX.get_or_init(|| {
        let mut m = HashMap::new();
        for (k, s) in bundled().iter().enumerate() {
            for v in s.values() {
                m.insert(s.part_name(v), (k, v));
            }
        }
        m
    });
    index.get(name).map(|&(k, v)| (&bundled()[k], v))
}

/// A bundled series by its name ("ERJ-3EKF", any case).
pub fn by_name(name: &str) -> Option<&'static SeriesDef> {
    bundled().iter().find(|s| s.series.eq_ignore_ascii_case(name))
}

/// A resistance as people write it: `4.7k`, `4k7`, `10R`, `1M` (mega, not milli: nobody means 1 mΩ here), `2.2MΩ`, `100 ohm`.
pub fn parse_ohms(text: &str) -> Option<f64> {
    let mut s = text.trim().replace(['Ω', 'Ω'], "");
    for unit in ["ohms", "ohm", "OHM"] {
        if let Some(t) = s.strip_suffix(unit) {
            s = t.to_string();
        }
    }
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    // a capital M alone is mega (4.7M, 1M, 2M2)
    let s = match s.find('M') {
        Some(i) if !s[i..].starts_with("Meg") => format!("{}{}", &s[..i], if i + 1 < s.len() { format!("Meg{}", &s[i + 1..]) } else { "Meg".into() }),
        _ => s.to_string(),
    };
    // `2Meg2` (the RKM style with mega): the digits after the letter are the fraction
    let s = match s.split_once("Meg") {
        Some((a, b)) if !b.is_empty() && b.chars().all(|c| c.is_ascii_digit()) => format!("{a}.{b}Meg"),
        _ => s,
    };
    crate::ltspice_expr::spice_number(&s).filter(|v| v.is_finite() && *v > 0.0)
}

/// `10k`, `4.75k`, `1Meg`, `97.6`: how a value is shown (and read back by the simulator).
pub fn ohms_text(v: f64) -> String {
    crate::passive::eng(v)
}

impl SeriesDef {
    /// Every value the series is made in, smallest first (the union of its E series, within its range).
    pub fn values(&self) -> Vec<f64> {
        let mut mantissas: Vec<u16> = vec![];
        for set in &self.values.sets {
            match set.as_str() {
                "E24" => mantissas.extend(E24),
                "E96" => mantissas.extend(E96),
                _ => {}
            }
        }
        mantissas.sort_unstable();
        mantissas.dedup();
        let mut out = vec![];
        // decades from 0.01 Ω to 100 MΩ cover every chip resistor
        for exp in -4..=6 {
            for &m in &mantissas {
                let v = value_of(m, exp);
                if v >= self.values.min * (1.0 - 1e-9) && v <= self.values.max * (1.0 + 1e-9) {
                    out.push(v);
                }
            }
        }
        out
    }

    /// The value's code in the part number (`sig3`: 1002, 10R0, 1004). None when the value cannot be written so.
    pub fn code(&self, v: f64) -> Option<String> {
        match self.code.as_str() {
            "sig3" => sig3(v),
            "sig2" => sig2(v),
            _ => None,
        }
    }

    /// The maker's part number of the value (`ERJ-3EKF1002V`).
    pub fn part_name(&self, v: f64) -> String {
        self.mpn.replace("{code}", &self.code(v).unwrap_or_else(|| ohms_text(v)))
    }

    /// The series' own value nearest to `v`, and whether it is exactly `v` (within 0.05 %).
    pub fn nearest(&self, v: f64) -> Option<(f64, bool)> {
        let best = self.values().into_iter().min_by(|a, b| (a.ln() - v.ln()).abs().total_cmp(&(b.ln() - v.ln()).abs()))?;
        Some((best, (best / v - 1.0).abs() < 5e-4))
    }

    /// The values next to `v` on either side (for "近い値" when `v` is not made).
    pub fn neighbours(&self, v: f64) -> (Option<f64>, Option<f64>) {
        let vals = self.values();
        let below = vals.iter().rev().find(|&&x| x < v * (1.0 - 5e-4)).copied();
        let above = vals.iter().find(|&&x| x > v * (1.0 + 5e-4)).copied();
        (below, above)
    }

    /// Is `v` one of the series' values?
    pub fn makes(&self, v: f64) -> bool {
        self.nearest(v).is_some_and(|(_, exact)| exact)
    }

    /// The decades the series' values fall in: `d` for values from 10^d up to 10^(d+1) (10 Ω to 1 MΩ → 1 to 6).
    pub fn decades(&self) -> Vec<i32> {
        let lo = self.values.min.log10().floor() as i32;
        let hi = self.values.max.log10().floor() as i32;
        (lo..=hi).collect()
    }

    /// The value shown first when the series is picked: 10 kΩ if it is made, else the middle of the range.
    pub fn default_value(&self) -> f64 {
        let vals = self.values();
        vals.iter().copied().find(|&v| (v / 1e4 - 1.0).abs() < 1e-9).unwrap_or_else(|| vals[vals.len() / 2])
    }

    /// The rated continuous working voltage of the value: √(P·R), at most the limiting element voltage.
    pub fn rated_voltage(&self, v: f64) -> f64 {
        (self.power_at(v) * v).sqrt().min(self.v_limit)
    }

    fn band(&self, v: f64) -> Option<&Band> {
        self.bands.iter().find(|b| v >= b.min * (1.0 - 1e-9) && v <= b.max * (1.0 + 1e-9))
    }

    /// The temperature coefficient's limit at the value `v`, ppm/K.
    pub fn tc_at(&self, v: f64) -> f64 {
        self.band(v).and_then(|b| b.tc_ppm).unwrap_or(self.tc_ppm)
    }

    /// The temperature coefficient as the data sheet writes it at the value `v`: "±100", "+350〜−450".
    pub fn tc_label(&self, v: f64) -> String {
        self.band(v).and_then(|b| b.tc_text.clone()).unwrap_or_else(|| format!("±{}", self.tc_at(v)))
    }

    /// The rated power at the value `v` (up to `derate_from_c`), watts.
    pub fn power_at(&self, v: f64) -> f64 {
        self.band(v).and_then(|b| b.p_max).unwrap_or(self.p_max)
    }

    /// The part of one value.
    pub fn part(&self, v: f64) -> PartDef {
        let name = self.part_name(v);
        let (tc, power) = (self.tc_label(v), self.power_at(v));
        let value = ohms_text(v);
        let tol = self.tolerance_pct / 100.0;
        let sub = spice_name(&name);
        let quality = if self.quality.is_empty() { String::new() } else { format!("{}。", self.quality.join("・")) };
        let description = format!(
            "{} {} {} {} ±{}%。{} W ({} ℃まで。それより高い周囲温度では直線で下げ、{} ℃で 0)、この値で使える電圧 {} V (√(電力×抵抗値) と素子の最高電圧 {} V の小さいほう)、温度係数 {} ppm/K、使える温度 {}〜{} ℃。{}{}。{}",
            self.maker,
            self.series,
            self.summary,
            value_label(v),
            self.tolerance_pct,
            power,
            self.derate_from_c,
            self.derate_zero_c,
            trim3(self.rated_voltage(v)),
            self.v_limit,
            tc,
            self.temp_min_c,
            self.temp_max_c,
            quality,
            self.body,
            self.land
        );
        let text = format!(
            ".subckt {sub} 1 2\nR1 1 2 {value}\n* spread R1 {} {} resistance (data sheet: ±{} %)\n.ends",
            ohms_text(v * (1.0 - tol)),
            ohms_text(v * (1.0 + tol)),
            self.tolerance_pct
        );
        let limits = vec![
            format!("抵抗値はデータシートの公称値です。1 個ずつの違い (±{} %) は、「温度とばらつき」のばらつき解析と最悪ケースで動きます。", self.tolerance_pct),
            format!("温度係数は「{} ppm/K」という保証の範囲で、向きと大きさは 1 個ずつ違うので、モデルでは 0 (温度で値が変わらない) にしています。自己発熱の解析で効かせるときは、部品の設定で温度係数を入れてください。", tc),
            "寄生のインダクタンスと容量はデータシートに無いので入れていません。高い周波数 (数百 MHz 以上) では実物と違います。".into(),
            format!("定格の判定は {} W のままです。周囲温度が {} ℃を超えるときは、データシートの曲線のとおり下げて考えてください。", power, self.derate_from_c),
        ];
        let rows = vec![
            sch_core::SpecRow { name: "許容差".into(), value: format!("±{} %", self.tolerance_pct), condition: String::new() },
            sch_core::SpecRow { name: "温度係数".into(), value: format!("{} ppm/K", tc), condition: String::new() },
            sch_core::SpecRow { name: "定格電力".into(), value: format!("{} W", power), condition: format!("{} ℃まで。{} ℃で 0 W へ直線で下げる", self.derate_from_c, self.derate_zero_c) },
            sch_core::SpecRow { name: "素子の最高電圧".into(), value: format!("{} V", self.v_limit), condition: "定格電圧は √(電力×抵抗値) とこの値の小さいほう".into() },
            sch_core::SpecRow { name: "使える温度".into(), value: format!("{}〜{} ℃", self.temp_min_c, self.temp_max_c), condition: String::new() },
        ];
        let rows = if self.v_overload > 0.0 {
            let mut r = rows;
            r.push(sch_core::SpecRow { name: "最大の過負荷電圧".into(), value: format!("{} V", self.v_overload), condition: "定格電圧 × 2.5 を 5 秒の試験".into() });
            r
        } else {
            rows
        };
        let json = serde_json::json!({
            "name": name,
            "category": self.category,
            "description": description,
            "datasheet": self.datasheet,
            "prefix": "R",
            "value": value,
            "pins": [ { "number": "1", "name": "1", "side": "left" }, { "number": "2", "name": "2", "side": "right" } ],
            "package": self.package,
            "footprint_name": self.footprint_name,
            "height": self.height,
            "sim": {
                "kind": "subckt",
                "element": "",
                "name": sub,
                "text": text,
                "pins": ["1", "2"],
                "params": {},
                "limits": limits,
                "origin": { "kind": "equivalent", "source": format!("{} {} のデータシートの公称値と許容差 ({})", self.maker, self.series, self.checked) }
            },
            "ratings": { "v_max": self.rated_voltage(v), "p_max": power, "rows": rows },
            "glyph": "resistor",
            "body": self.outline
        });
        serde_json::from_value(json).unwrap_or_else(|e| panic!("series {}: part {name}: {e}", self.series))
    }

    /// One part for every value.
    pub fn expand(&self) -> Vec<PartDef> {
        self.values().into_iter().map(|v| self.part(v)).collect()
    }

    /// The values in words: "E24・E96 の 10 Ω〜1 MΩ".
    pub fn range_text(&self) -> String {
        format!("{} の {}〜{}", self.values.sets.join("・"), value_label(self.values.min), value_label(self.values.max))
    }
}

/// The significant figures of a decade (times 100: 470 is 4.7): E24 alone, or with `fine` E24 and E96 together (114 a decade).
pub fn mantissas(fine: bool) -> Vec<u16> {
    let mut m: Vec<u16> = E24.to_vec();
    if fine {
        m.extend(E96);
        m.sort_unstable();
        m.dedup();
    }
    m
}

/// A value from its significant figures (times 100) and its decade: (470, 3) is 4.7 kΩ.
pub fn compose(m: u16, decade: i32) -> f64 {
    value_of(m, decade - 2)
}

/// The significant figures (times 100) and the decade of a value: 4.7 kΩ is (470, 3).
pub fn split(v: f64) -> (u16, i32) {
    let decade = v.log10().floor() as i32;
    let m = (v / 10f64.powi(decade - 2)).round() as u16;
    // 9.995 rounds up to 1000: the next decade's 1.00
    if m >= 1000 { (100, decade + 1) } else { (m, decade) }
}

/// The decade as a multiplier, as the colour code reads it: 3 is "×1k".
pub fn decade_label(decade: i32) -> String {
    let unit = match decade {
        d if d >= 6 => ("M", d - 6),
        d if d >= 3 => ("k", d - 3),
        d => ("", d),
    };
    format!("×{}{}", 10u32.pow(unit.1.max(0) as u32), unit.0)
}

/// `m` (the mantissa times 100) times 10^`exp`, rounded to the digits it has.
fn value_of(m: u16, exp: i32) -> f64 {
    let v = m as f64 * 10f64.powi(exp);
    // 1.02 × 10^-1 must come out as 10.2, not 10.199999...
    let digits = 3 - (v.log10().floor() as i32 + 1);
    let scale = 10f64.powi(digits.max(0));
    (v * scale).round() / scale
}

/// Three significant figures and the number of zeros; `R` for the decimal point below 100 Ω.
fn sig3(v: f64) -> Option<String> {
    if !(v.is_finite() && v > 0.0) {
        return None;
    }
    let exp = v.log10().floor() as i32; // 10.2 → 1, 1000 → 3
    let m = (v / 10f64.powi(exp - 2)).round() as u32; // 102, 100
    if !(100..1000).contains(&m) {
        return None;
    }
    let digits = format!("{m}");
    Some(match exp {
        e if e >= 2 => format!("{digits}{}", e - 2),
        1 => format!("{}R{}", &digits[..2], &digits[2..]),
        0 => format!("{}R{}", &digits[..1], &digits[1..]),
        _ => return None,
    })
}

/// Two significant figures and the number of zeros; `R` for the decimal point below 10 Ω (KOA CF: `103` = 10 kΩ, `2R2` = 2.2 Ω).
fn sig2(v: f64) -> Option<String> {
    if !(v.is_finite() && v > 0.0) {
        return None;
    }
    let exp = v.log10().floor() as i32;
    let m = (v / 10f64.powi(exp - 1)).round() as u32;
    if !(10..100).contains(&m) || (m as f64 * 10f64.powi(exp - 1) / v - 1.0).abs() > 1e-6 {
        return None;
    }
    let digits = format!("{m}");
    Some(match exp {
        e if e >= 1 => format!("{digits}{}", e - 1),
        0 => format!("{}R{}", &digits[..1], &digits[1..]),
        _ => return None,
    })
}

/// `10 kΩ`, `4.75 kΩ`, `1 MΩ`, `97.6 Ω`: the value in sentences.
fn value_label(v: f64) -> String {
    let (scale, p) = if v >= 1e6 { (1e6, " MΩ") } else if v >= 1e3 { (1e3, " kΩ") } else { (1.0, " Ω") };
    format!("{}{p}", trim3(v / scale))
}

fn trim3(x: f64) -> String {
    let t = format!("{x:.3}");
    t.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn spice_name(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn erj() -> &'static SeriesDef {
        by_name("ERJ-3EKF").unwrap()
    }

    #[test]
    fn the_part_number_carries_the_value_the_way_the_data_sheet_explains() {
        // data sheet: "1002 → 10 kΩ", "Decimal point is expressed by R"
        let s = erj();
        assert_eq!(s.part_name(10_000.0), "ERJ-3EKF1002V");
        assert_eq!(s.part_name(10.0), "ERJ-3EKF10R0V");
        assert_eq!(s.part_name(97.6), "ERJ-3EKF97R6V");
        assert_eq!(s.part_name(100.0), "ERJ-3EKF1000V");
        assert_eq!(s.part_name(4_750.0), "ERJ-3EKF4751V");
        assert_eq!(s.part_name(1e6), "ERJ-3EKF1004V");
    }

    #[test]
    fn the_values_are_e24_and_e96_from_10_ohms_to_1_megohm() {
        let v = erj().values();
        assert_eq!(v.first(), Some(&10.0));
        assert_eq!(v.last(), Some(&1e6));
        // E96 has 96 values a decade; E24 shares only 1.0, 1.1, 1.3, 1.5, 2.0 and 7.5 with it and adds 18: 114 a decade
        let decade = v.iter().filter(|&&x| (1000.0..10_000.0).contains(&x)).count();
        assert_eq!(decade, 114);
        assert_eq!(v.len(), 114 * 5 + 1);
        for x in [4700.0, 4750.0, 10.2, 1200.0, 976_000.0] {
            assert!(v.iter().any(|&y| (y - x).abs() < 1e-6), "{x} missing");
        }
        // every name is distinct and comes back to its value
        for &x in &v {
            assert_eq!(member(&erj().part_name(x)).map(|(_, y)| y), Some(x));
        }
    }

    #[test]
    fn every_part_is_valid_and_simulates_its_value_with_the_tolerance_as_a_spread() {
        for d in erj().expand() {
            let issues = d.validate();
            assert!(issues.iter().all(|i| !i.error), "{}: {issues:?}", d.name);
        }
        let d = erj().part(4_700.0);
        let sim = d.sim.as_ref().unwrap();
        assert!(sim.text.contains("R1 1 2 4.7k"), "{}", sim.text);
        assert!(sim.text.contains("* spread R1 4.653k 4.747k resistance"), "{}", sim.text);
        // 0.1 W into 4.7 kΩ is 21.7 V, below the limiting element voltage
        let r = d.ratings.as_ref().unwrap();
        assert!((r.v_max.unwrap() - 21.679).abs() < 0.01, "{:?}", r.v_max);
        assert_eq!(erj().part(1e6).ratings.unwrap().v_max, Some(75.0));
        assert_eq!(d.footprint().unwrap().pads.len(), 2);
    }

    #[test]
    fn the_land_pattern_is_the_middle_of_panasonics_recommendation() {
        // a (gap) 0.7 to 0.9, b (overall) 2.0 to 2.2, c (width) 0.8 to 1.0 mm
        let fp = erj().part(10_000.0).footprint().unwrap();
        let (l, r) = (&fp.pads[0], &fp.pads[1]);
        let (lw, lh) = l.shape.size();
        let (rw, _) = r.shape.size();
        let gap = (r.pos.x - rw / 2.0) - (l.pos.x + lw / 2.0);
        let overall = (r.pos.x + rw / 2.0) - (l.pos.x - lw / 2.0);
        assert!((gap - 0.8).abs() < 1e-9, "{gap}");
        assert!((overall - 2.1).abs() < 1e-9, "{overall}");
        assert!((lh - 0.9).abs() < 1e-9);
    }

    #[test]
    fn the_other_sizes_follow_the_data_sheets_ratings_and_land_patterns() {
        // (series, 10 kΩ part, values, rated power, limiting voltage, gap a, overall b, width c of the land pattern)
        let table = [
            ("ERJ-1GJF", "ERJ-1GJF1002C", 571, 0.05, 25.0, 0.35, 0.85, 0.30),
            ("ERJ-2RKF", "ERJ-2RKF1002X", 571, 0.1, 50.0, 0.55, 1.5, 0.5),
            ("ERJ-3EKF", "ERJ-3EKF1002V", 571, 0.1, 75.0, 0.8, 2.1, 0.9),
            // 10 Ω to 2.2 MΩ: 37 values from 1 MΩ up (E96 2.21 M is above the range)
            ("ERJ-6ENF", "ERJ-6ENF1002V", 607, 0.125, 150.0, 1.2, 3.5, 1.15),
        ];
        let mut names = std::collections::HashSet::new();
        for (series, ten_k, count, p, v, a, b, c) in table {
            let s = by_name(series).unwrap();
            assert_eq!(s.part_name(10_000.0), ten_k);
            assert_eq!(s.values().len(), count, "{series}");
            let d = s.part(1e6);
            let r = d.ratings.as_ref().unwrap();
            assert_eq!(r.p_max, Some(p));
            assert!((r.v_max.unwrap() - (p * 1e6_f64).sqrt().min(v)).abs() < 1e-9, "{series}");
            let fp = d.footprint().unwrap();
            let (l, rt) = (&fp.pads[0], &fp.pads[1]);
            let (w, h) = l.shape.size();
            assert!(((rt.pos.x - w / 2.0) - (l.pos.x + w / 2.0) - a).abs() < 1e-9, "{series} gap");
            assert!(((rt.pos.x + w / 2.0) - (l.pos.x - w / 2.0) - b).abs() < 1e-9, "{series} overall");
            assert!((h - c).abs() < 1e-9, "{series} width");
            for x in s.values() {
                assert!(names.insert(s.part_name(x)), "{} twice", s.part_name(x));
            }
        }
        assert_eq!(by_name("ERJ-6ENF").unwrap().values().last(), Some(&2.2e6));
    }

    #[test]
    fn koa_rk73h_follows_its_ratings_table_by_value_range() {
        // (series, 10 kΩ part, rated voltage limit, gap A, overall B, width C of Appendix B)
        let table = [
            ("RK73H1E", "RK73H1ETTP1002F", 75.0, 0.5, 1.3, 0.3),
            ("RK73H1J", "RK73H1JTTD1002F", 75.0, 1.0, 2.0, 0.6),
            ("RK73H2A", "RK73H2ATTD1002F", 150.0, 1.3, 2.5, 1.05),
            ("RK73H2B", "RK73H2BTTD1002F", 200.0, 2.2, 4.0, 1.4),
        ];
        for (series, ten_k, v, a, b, c) in table {
            let s = by_name(series).unwrap();
            assert_eq!(s.part_name(10_000.0), ten_k);
            // 1.0 Ω to 10 MΩ: seven decades of 114 and 10 MΩ itself
            assert_eq!(s.values().len(), 114 * 7 + 1, "{series}");
            assert_eq!(s.values().first(), Some(&1.0));
            assert_eq!(s.part_name(1.0), ten_k.replace("1002F", "1R00F"));
            assert_eq!(s.rated_voltage(1e7), v);
            // ±100 ppm/K from 10 Ω to 1 MΩ, ±200 below
            assert_eq!(s.tc_at(10_000.0), 100.0);
            assert_eq!(s.tc_at(4.7), 200.0);
            let fp = s.part(1000.0).footprint().unwrap();
            let (l, r) = (&fp.pads[0], &fp.pads[1]);
            let (w, h) = l.shape.size();
            assert!(((r.pos.x - w / 2.0) - (l.pos.x + w / 2.0) - a).abs() < 1e-9, "{series} gap");
            assert!(((r.pos.x + w / 2.0) - (l.pos.x - w / 2.0) - b).abs() < 1e-9, "{series} overall");
            assert!((h - c).abs() < 1e-9, "{series} width");
        }
        // above 1 MΩ: ±200 in 0603, ±400 in 0805; in 1206 ±200 to 5.6 MΩ and ±400 from 5.62 MΩ
        assert_eq!(by_name("RK73H1J").unwrap().tc_at(2.2e6), 200.0);
        assert_eq!(by_name("RK73H2A").unwrap().tc_at(2.2e6), 400.0);
        assert_eq!(by_name("RK73H2B").unwrap().tc_at(5.6e6), 200.0);
        assert_eq!(by_name("RK73H2B").unwrap().tc_at(5.62e6), 400.0);
        // 0603 is 0.125 W up to 1 kΩ and 0.1 W above, and the part says so
        let j = by_name("RK73H1J").unwrap();
        assert_eq!(j.power_at(1000.0), 0.125);
        assert_eq!(j.power_at(1020.0), 0.1);
        assert_eq!(j.part(470.0).ratings.unwrap().p_max, Some(0.125));
        assert!(j.part(4.7).description.contains("±200 ppm/K"));
    }

    #[test]
    fn koa_cf_and_mf_are_leaded_and_follow_their_data_sheets() {
        let cf = by_name("CF1/4").unwrap();
        // two significant figures and the zeros, R for the point below 10 Ω (the data sheet's "103")
        assert_eq!(cf.part_name(10_000.0), "CF1/4CT52A103J");
        assert_eq!(cf.part_name(2.2), "CF1/4CT52A2R2J");
        assert_eq!(cf.part_name(51.0), "CF1/4CT52A510J");
        assert_eq!(cf.part_name(5.1e6), "CF1/4CT52A515J");
        // E24 from 2.2 Ω to 5.1 MΩ: 16 values up to 9.1 Ω, five decades of 24, 18 from 1 MΩ to 5.1 MΩ
        assert_eq!(cf.values().len(), 16 + 24 * 5 + 18);
        // the coefficient as the data sheet writes it, by range
        assert_eq!(cf.tc_label(4700.0), "+350〜−450");
        assert_eq!(cf.tc_label(470_000.0), "0〜−1000");
        assert!(cf.part(1.5e6).description.contains("0〜−1300 ppm/K"));
        assert_eq!(cf.rated_voltage(1e6), 300.0);
        let mf = by_name("MF1/4D").unwrap();
        assert_eq!(mf.part_name(10_000.0), "MF1/4DCT52A1002F");
        assert_eq!(mf.part_name(10.0), "MF1/4DCT52A10R0F");
        assert_eq!(mf.values().len(), 114 * 5 + 38);
        assert_eq!(mf.values().last(), Some(&2.21e6));
        assert_eq!(mf.tc_label(1e5), "±100");
        for s in [cf, mf] {
            for d in s.expand() {
                assert!(d.validate().iter().all(|i| !i.error), "{}: {:?}", d.name, d.validate());
            }
            let fp = s.part(1000.0).footprint().unwrap();
            // through holes 10.16 mm apart, a 0.9 mm drill in a 1.7 mm pad; the body drawn 6.8 × 2.6 mm
            assert_eq!(fp.pads.iter().map(|p| p.drill).collect::<Vec<_>>(), vec![Some(0.9), Some(0.9)]);
            assert!((fp.pads[1].pos.x - fp.pads[0].pos.x - 10.16).abs() < 1e-9);
            let b = fp.body.as_ref().unwrap();
            assert_eq!((b.w, b.h), (6.8, 2.6));
        }
    }

    #[test]
    fn every_bundled_part_name_is_used_once() {
        let mut names = std::collections::HashSet::new();
        for d in bundled_parts() {
            assert!(names.insert(d.name.clone()), "{} twice", d.name);
            assert_eq!(member(&d.name).map(|(s, v)| s.part_name(v)), Some(d.name.clone()));
        }
    }

    #[test]
    fn a_value_splits_into_figures_and_a_decade_for_the_drum() {
        assert_eq!(split(4700.0), (470, 3));
        assert_eq!(split(10.0), (100, 1));
        assert_eq!(split(97.6), (976, 1));
        assert_eq!(split(2.2e6), (220, 6));
        assert_eq!(compose(470, 3), 4700.0);
        assert_eq!(compose(102, 1), 10.2);
        assert_eq!(decade_label(1), "×10");
        assert_eq!(decade_label(3), "×1k");
        assert_eq!(decade_label(5), "×100k");
        assert_eq!(decade_label(6), "×1M");
        assert_eq!(mantissas(false).len(), 24);
        assert_eq!(mantissas(true).len(), 114);
        let s = by_name("ERJ-6ENF").unwrap();
        assert_eq!(s.decades(), vec![1, 2, 3, 4, 5, 6]);
        assert!(s.makes(2.2e6) && !s.makes(4.7e6) && !s.makes(4.8e3));
        // every value of every series is on the drum: its figures are in the fine list and its decade in the series' decades
        for s in bundled() {
            for v in s.values() {
                let (m, d) = split(v);
                assert!(mantissas(true).contains(&m) && s.decades().contains(&d), "{v}");
                assert_eq!(compose(m, d), v);
            }
        }
    }

    #[test]
    fn values_are_read_as_people_write_them_and_the_nearest_is_offered() {
        assert_eq!(parse_ohms("4.7k"), Some(4700.0));
        assert_eq!(parse_ohms("4k7"), Some(4700.0));
        assert_eq!(parse_ohms("1M"), Some(1e6));
        assert_eq!(parse_ohms("2M2"), Some(2.2e6));
        assert_eq!(parse_ohms("1Meg"), Some(1e6));
        assert_eq!(parse_ohms("100Ω"), Some(100.0));
        assert_eq!(parse_ohms("10R"), Some(10.0));
        assert_eq!(parse_ohms(""), None);
        let s = erj();
        assert_eq!(s.nearest(4700.0), Some((4700.0, true)));
        // 4.8 k is not made: 4.75 k is the nearest, 4.87 k the next one up
        assert_eq!(s.nearest(4800.0), Some((4750.0, false)));
        assert_eq!(s.neighbours(4800.0), (Some(4750.0), Some(4870.0)));
        assert_eq!(s.default_value(), 10_000.0);
    }
}
