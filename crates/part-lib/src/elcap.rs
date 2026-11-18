//! Aluminium electrolytic capacitors made from a maker's lineup table: one row per part number (capacitance, case size,
//! impedance, ripple current, read by `datasheet::lineup`) and the series' tables that hold for every row (ripple current's
//! frequency coefficients by capacitance, endurance by case size and voltage, lead spacing and diameter by case diameter),
//! written once in a `*.elcap.json` (`crates/part-lib/elcaps/`). The model is Resr + C, ESR = √(Z² − Xc²) at the
//! impedance's frequency; holes and pads as `thru_hole::radial`. `ccad lineup` and the part form's lineup picker use it.

use crate::PartDef;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};

/// The series' tables (`*.elcap.json`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Series {
    /// "Nichicon"
    pub maker: String,
    /// "UHE"
    pub series: String,
    /// What the series is for, in the part's description ("小形・低インピーダンス").
    pub feature: String,
    /// The data sheet's address.
    pub datasheet: String,
    /// The data sheet as named in the part ("データシート e-uhe.pdf (UHE series、CAT.8100M)").
    pub source: String,
    /// The day the values were checked against the data sheet ("2026-11-18").
    pub checked: String,
    /// Category temperature range (°C).
    pub temp: [f64; 2],
    pub tolerance_pct: f64,
    /// Leakage current I = k·C·V µA (C in µF), not less than `leak_min_ua`.
    pub leak_k: f64,
    pub leak_min_ua: f64,
    /// The highest rated voltage the formula is for (Nichicon UVR: 100 V; above, the table's own value). None: all.
    pub leak_v_max: Option<f64>,
    /// Minutes after the rated voltage is applied that the leakage is given for (Nichicon UPW: 1). None: 2.
    pub leak_minutes: Option<f64>,
    /// The frequencies of the ripple current's coefficient table (Hz).
    pub coeff_freqs: Vec<f64>,
    /// Its rows, by capacitance: the first with `cap_max` ≥ the part's capacitance.
    pub coeffs: Vec<CoeffRow>,
    /// Endurance at the rated temperature: the first rule the part fits.
    pub life: Vec<LifeRule>,
    /// Lead spacing and diameter by case diameter.
    pub leads: Vec<LeadRule>,
    /// How the part number is extended for cut or formed leads and tape (shown in the description).
    pub order_note: String,
    /// How a part number is made from the values, for a table without them (Rubycon: "{V}ZLJ{C}M{O}{D}X{L}" makes
    /// "25ZLJ1000M10X20"): {V} rated voltage, {C} capacitance in µF, {D} and {L} the case's diameter and length, {O}
    /// the option of the row's mark (`marks`).
    pub part_pattern: String,
    /// The option code of each mark printed in the table (Rubycon ZLJ: "●" → "EFU", the low impedance version).
    pub marks: std::collections::BTreeMap<String, String>,
    /// The part numbers bundled with the program.
    pub bundle: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CoeffRow {
    pub cap_max: f64,
    /// The rated voltages the row is for (Nichicon UVR: 6.3 to 100 V and 160 to 450 V have their own rows).
    pub v_min: Option<f64>,
    pub v_max: Option<f64>,
    pub k: Vec<f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LifeRule {
    pub d_min: Option<f64>,
    pub d_max: Option<f64>,
    /// Case length (Rubycon gives 8×11.5 and 8×16 different lives).
    pub l_min: Option<f64>,
    pub l_max: Option<f64>,
    pub v_min: Option<f64>,
    pub v_max: Option<f64>,
    pub hours: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LeadRule {
    pub d: f64,
    pub pitch: f64,
    pub lead: f64,
    /// A longer case than this (mm) has `long_lead` leads.
    pub long_over: Option<f64>,
    pub long_lead: Option<f64>,
}

/// One row of the lineup table.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub part: String,
    pub cap_uf: f64,
    pub volts: f64,
    pub d_mm: f64,
    pub l_mm: f64,
    /// (temperature °C, frequency Hz, Ω): the impedance (or ESR) columns.
    pub impedance: Vec<(f64, f64, f64)>,
    pub ripple_a: f64,
    pub ripple_freq_hz: f64,
    pub ripple_temp_c: f64,
    /// The table's leakage current, to check the series' formula against.
    pub leak_ua: Option<f64>,
    /// tan δ (120 Hz), the ESR's source where the table has no impedance.
    pub tan_d: Option<f64>,
    /// The data sheet's page (from 0), for the description.
    pub page: Option<usize>,
    /// The mark printed in the row, which picks an option of the part number (`Series::marks`).
    pub mark: String,
}

impl Series {
    pub fn load(path: &Path) -> Result<Series, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The bundled series' tables (`crates/part-lib/elcaps/`).
    pub fn bundled() -> Vec<Series> {
        BUNDLED.iter().filter_map(|t| serde_json::from_str(t).ok()).collect()
    }

    /// The bundled series whose name the part number starts with, after its rated voltage for Rubycon's
    /// ("UHE1C331MPD", "25ZLJ1000M10X20").
    pub fn for_part(part: &str) -> Option<Series> {
        let up = part.to_ascii_uppercase();
        let rest = up.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.');
        Series::bundled().into_iter().find(|s| !s.series.is_empty() && rest.starts_with(&s.series.to_ascii_uppercase()))
    }

    /// The bundled series of a table: the one its first part number belongs to, else the one whose name is a word of
    /// the data sheet (Rubycon's "ZLJ" in the page's title, for a table without part numbers).
    pub fn for_table<'a>(rows: &[Row], words: impl IntoIterator<Item = &'a str>) -> Option<Series> {
        if let Some(r) = rows.iter().find(|r| !r.part.is_empty()) {
            return Series::for_part(&r.part);
        }
        // (full width letters as half width: Rubycon prints "ＺＬＪ")
        let half = |w: &str| -> String { w.trim().chars().map(|c| if ('\u{FF01}'..='\u{FF5E}').contains(&c) { char::from_u32(c as u32 - 0xFEE0).unwrap_or(c) } else { c }).collect() };
        let words: Vec<String> = words.into_iter().map(|w| half(w).to_ascii_uppercase()).collect();
        Series::bundled().into_iter().find(|s| !s.series.is_empty() && words.iter().any(|w| *w == s.series.to_ascii_uppercase()))
    }

    /// A bundled series by its name ("ZLJ").
    pub fn named(name: &str) -> Option<Series> {
        Series::bundled().into_iter().find(|s| s.series.eq_ignore_ascii_case(name.trim()))
    }

    fn coeffs(&self, cap_uf: f64, volts: f64) -> Vec<(f64, f64)> {
        let rows: Vec<&CoeffRow> = self.coeffs.iter().filter(|r| r.v_min.is_none_or(|v| volts >= v - 1e-9) && r.v_max.is_none_or(|v| volts <= v + 1e-9)).collect();
        let Some(row) = rows.iter().find(|r| cap_uf <= r.cap_max).or(rows.last()) else { return vec![] };
        self.coeff_freqs.iter().copied().zip(row.k.iter().copied()).collect()
    }

    fn life_hours(&self, d: f64, l: f64, v: f64) -> Option<f64> {
        let within = |x: f64, lo: Option<f64>, hi: Option<f64>| lo.is_none_or(|lo| x >= lo - 1e-9) && hi.is_none_or(|hi| x <= hi + 1e-9);
        self.life.iter().find(|r| within(d, r.d_min, r.d_max) && within(l, r.l_min, r.l_max) && within(v, r.v_min, r.v_max)).map(|r| r.hours)
    }

    fn lead(&self, d: f64, l: f64) -> Option<(f64, f64)> {
        let r = self.leads.iter().find(|r| (r.d - d).abs() < 1e-6)?;
        let lead = match (r.long_over, r.long_lead) {
            (Some(over), Some(long)) if l > over => long,
            _ => r.lead,
        };
        Some((r.pitch, lead))
    }

    /// The part number the series' pattern makes for a row (None without a pattern).
    pub fn name(&self, r: &Row) -> Option<String> {
        (!self.part_pattern.is_empty()).then(|| {
            let option = self.marks.get(&r.mark).map_or("", |o| o.as_str());
            self.part_pattern.replace("{V}", &g(r.volts)).replace("{C}", &g(r.cap_uf)).replace("{D}", &g(r.d_mm)).replace("{L}", &g(r.l_mm)).replace("{O}", option)
        })
    }

    /// The leakage current the series' formula gives (µA).
    pub fn leakage(&self, cap_uf: f64, volts: f64) -> f64 {
        (self.leak_k * cap_uf * volts).max(self.leak_min_ua)
    }

    fn formula_for(&self, volts: f64) -> bool {
        self.leak_v_max.is_none_or(|v| volts <= v + 1e-9)
    }

    /// The row's leakage current: the formula's, or the table's above `leak_v_max`.
    fn leakage_of(&self, r: &Row) -> Option<f64> {
        if self.formula_for(r.volts) { Some(self.leakage(r.cap_uf, r.volts)) } else { r.leak_ua }
    }

    fn leak_text(&self, volts: f64) -> String {
        if !self.formula_for(volts) {
            "データシートの表の値".into()
        } else if self.leak_min_ua > 0.0 {
            format!("{} CV か {} µA の大きいほう", g(self.leak_k), g(self.leak_min_ua))
        } else {
            format!("{} CV", g(self.leak_k))
        }
    }

    fn leak_condition(&self, volts: f64) -> String {
        if self.formula_for(volts) { format!("定格電圧で {} 分後・20 ℃", g(self.leak_minutes.unwrap_or(2.0))) } else { "定格電圧・20 ℃ (データシートの表の条件)".into() }
    }
}

/// Like C's "%g": six significant figures, no trailing zeros ("52.8", "1000", "0.087").
fn g(x: f64) -> String {
    sig(x, 6)
}

/// `n` significant figures, no trailing zeros (for numbers from 1e-4 to 1e6).
fn sig(x: f64, n: i32) -> String {
    if x == 0.0 || !x.is_finite() {
        return format!("{x}");
    }
    let digits = (n - 1 - x.abs().log10().floor() as i32).max(0) as usize;
    let s = format!("{x:.digits$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

fn up(v: f64, step: f64) -> f64 {
    (((v / step) - 1e-9).ceil() * step * 1000.0).round() / 1000.0
}

/// The part for one row of the table.
pub fn make(s: &Series, r: &Row) -> Result<PartDef, String> {
    let err = |what: &str| format!("{}: {what}", r.part);
    if !(r.cap_uf > 0.0 && r.volts > 0.0 && r.d_mm > 0.0 && r.l_mm > 0.0) {
        return Err(err("容量・定格電圧・大きさが読めていません"));
    }
    let (pitch, lead) = s.lead(r.d_mm, r.l_mm).ok_or_else(|| err(&format!("φ{} mm のリードの間隔と径が、シリーズの表 (leads) にありません", g(r.d_mm))))?;
    let hours = s.life_hours(r.d_mm, r.l_mm, r.volts).ok_or_else(|| err("寿命の決まり (life) に合う行がありません"))?;
    let drill = up(lead + crate::thru_hole::HOLE_CLEARANCE, 0.1);
    let pad = up(drill + 2.0 * crate::thru_hole::ANNULAR_RING, 0.1);
    // the ESR: from the impedance (low impedance series), else from tan δ at 120 Hz (general purpose series, whose
    // tables have no impedance): (ESR, the description's words, the model's note, the first rating rows, what the
    // origin names)
    let z20 = r.impedance.iter().find(|z| (z.0 - 20.0).abs() < 0.5).or(r.impedance.first()).copied();
    let (esr, basis, esr_note, mut rows, from) = match (z20, r.tan_d) {
        (Some((_, z_freq, z20)), _) => {
            let cold = r.impedance.iter().filter(|z| z.0 < 0.0).min_by(|a, b| a.0.total_cmp(&b.0)).copied();
            let xc = 1.0 / (2.0 * std::f64::consts::PI * z_freq * r.cap_uf * 1e-6);
            if xc >= z20 {
                return Err(err(&format!("インピーダンス {} Ω が容量のリアクタンス {} Ω より小さく、ESR が出せません (列の読み違い?)", g(z20), sig(xc, 3))));
            }
            let esr = (z20 * z20 - xc * xc).sqrt();
            let zf = if (z_freq - 100e3).abs() < 1.0 { "100 kHz".to_string() } else { format!("{} kHz", g(z_freq / 1e3)) };
            let cold_text = cold.map(|c| format!("、{} ℃では {} Ω 以下", g(c.0), g(c.2))).unwrap_or_default();
            let cold_limit = match cold {
                Some(c) => format!("({} ℃では {} Ω 以下、約 {:.1} 倍、データシートの表)", g(c.0), g(c.2), c.2 / z20),
                None => "(低い温度での値はデータシートの表を見てください)".to_string(),
            };
            let note = format!("ESR は、データシートの {zf}・20 ℃のインピーダンス {} Ω から容量のリアクタンス ({} mΩ) を除いた値 ({} mΩ) を、全部の周波数で一定にしています。実物は低い周波数と低い温度で大きくなります {cold_limit}。", g(z20), sig(xc * 1000.0, 3), sig(esr * 1000.0, 3));
            let mut rows = vec![json!({"name": "インピーダンス", "value": format!("{} Ω 以下", g(z20)), "condition": format!("{zf}・20 ℃")})];
            if let Some(c) = cold {
                rows.push(json!({"name": "インピーダンス (低温)", "value": format!("{} Ω 以下", g(c.2)), "condition": format!("{zf}・{} ℃", g(c.0))}));
            }
            (esr, format!("インピーダンス {} Ω 以下 ({zf}・20 ℃{cold_text})", g(z20)), note, rows, "インピーダンス")
        }
        (None, Some(tan)) => {
            let esr = tan / (2.0 * std::f64::consts::PI * 120.0 * r.cap_uf * 1e-6);
            let note = format!("ESR は、データシートの 120 Hz・20 ℃の tanδ の最大 {} から ESR = tanδ ÷ (2π × 120 Hz × C) で求めた値 ({} mΩ) を、全部の周波数で一定にしています。最大値なので実物はこれより小さいことが多く、とくに高い周波数 (数 kHz より上) では実物の ESR はもっと小さくなります。低い温度では大きくなります。", g(tan), sig(esr * 1000.0, 3));
            let rows = vec![json!({"name": "tanδ", "value": format!("{} 以下", g(tan)), "condition": "120 Hz・20 ℃"})];
            (esr, format!("tanδ {} 以下 (120 Hz・20 ℃)", g(tan)), note, rows, "tanδ")
        }
        (None, None) => return Err(err("インピーダンスの列も tanδ の列もありません")),
    };
    let esr_m = sig(esr * 1000.0, 4);
    let mut coeffs = s.coeffs(r.cap_uf, r.volts);
    if coeffs.last().is_none_or(|c| c.0 < r.ripple_freq_hz) {
        coeffs.push((r.ripple_freq_hz, 1.0));
    }
    // the ripple current at 120 Hz, when the rating is given elsewhere (100 kHz)
    let k120 = coeffs.iter().find(|c| (c.0 - 120.0).abs() < 1.0 && (r.ripple_freq_hz - 120.0).abs() > 1.0).map(|c| c.1);
    let leak = s.leakage_of(r);
    let value = format!("{}u", g(r.cap_uf));
    let rf = if r.ripple_freq_hz < 1000.0 { format!("{} Hz", g(r.ripple_freq_hz)) } else { format!("{} kHz", g(r.ripple_freq_hz / 1e3)) };
    let tmax = g(s.temp[1]);
    let ripple_ma = g(r.ripple_a * 1000.0);
    let k120_text = k120.map(|k| format!("、120 Hz では ×{k:.2}")).unwrap_or_default();
    let desc = format!(
        "{maker} {series} アルミ電解コンデンサ {cap} µF ±{tol} % {v} V ({tmax} ℃・{hours} 時間、{feature})。本体 φ{d} × {l} mm、リード φ{lead} mm・間隔 {pitch} mm。{basis}、許容リップル電流 {ripple_ma} mA rms ({rf}・{rt} ℃{k120_text})。極性があります: 1 番ピン (四角いパッド) が +。逆につなぐと壊れます。穴径 {drill:.1} mm・パッド径 {pad:.1} mm はリード径からの目安 (部品作成フォームのリード部品と同じ考え方。メーカーの推奨ランドではない)。{order}{source} で確かめた ({checked})",
        maker = s.maker,
        series = s.series,
        cap = g(r.cap_uf),
        tol = g(s.tolerance_pct),
        v = g(r.volts),
        hours = g(hours),
        feature = s.feature,
        d = g(r.d_mm),
        l = g(r.l_mm),
        lead = g(lead),
        pitch = g(pitch),
        rt = g(r.ripple_temp_c),
        order = s.order_note,
        source = s.source,
        checked = s.checked,
    );
    let leak_text = s.leak_text(r.volts);
    rows.push(json!({"name": "許容リップル電流", "value": format!("{ripple_ma} mA rms"), "condition": format!("{rf}・{} ℃", g(r.ripple_temp_c))}));
    rows.push(json!({"name": "寿命", "value": format!("{} 時間", g(hours)), "condition": format!("{tmax} ℃・定格電圧とリップル電流をかけて")}));
    if let Some(leak) = leak {
        rows.push(json!({"name": "漏れ電流", "value": format!("{} µA 以下 ({leak_text})", g(leak)), "condition": s.leak_condition(r.volts)}));
    }
    rows.push(json!({"name": "使える温度", "value": format!("{}〜{} ℃", g(s.temp[0]).replace('-', "−"), tmax), "condition": ""}));
    let part = json!({
        "name": r.part,
        "category": "コンデンサ",
        "description": desc,
        "datasheet": s.datasheet,
        "prefix": "C",
        "value": value,
        "pins": [{"number": "1", "name": "1", "side": "left"}, {"number": "2", "name": "2", "side": "right"}],
        "package": {"type": "explicit", "pads": [
            {"pin": "1", "x": -pitch / 2.0, "y": 0.0, "shape": "rect", "w": pad, "h": pad, "drill": drill, "slot": 0.0, "slot_v": false},
            {"pin": "2", "x": pitch / 2.0, "y": 0.0, "shape": "circle", "w": pad, "h": pad, "drill": drill, "slot": 0.0, "slot_v": false}
        ], "holes": []},
        "footprint_name": format!("CP_Radial_D{}_P{}_{}", g(r.d_mm), g(pitch), s.maker),
        "height": r.l_mm,
        "body": {"shape": "circle", "w": r.d_mm, "h": 0.0, "x": 0.0, "y": 0.0, "mark": "plus", "mark_pin": "1"},
        "sim": {
            "kind": "subckt",
            "element": "",
            "name": r.part,
            "text": format!(".subckt {p} 1 2\nResr 1 n_c {esr_m}m\nC1 n_c 2 {value}\n.ends", p = r.part),
            "pins": ["1", "2"],
            "params": {},
            "limits": [
                esr_note,
                "ESL (リードと巻き取りのインダクタンス) はデータシートに無いので入れていません。数百 kHz より上では実物と違います。",
                format!("容量の許容差 (±{} %)、温度による容量の変化、漏れ電流 ({leak_text}) は入れていません。", g(s.tolerance_pct)),
                "逆電圧と定格を超える電圧は、シミュレーションでは止まりません。極性と定格電圧を守ってください。過渡解析のあと、波形の「部品の定格 (リップル電流・寿命)」で、リップル電流と推定寿命を確かめられます。"
            ],
            "origin": {"kind": "equivalent", "source": format!("{} {} series の{}の容量と{from}", s.maker, s.series, s.source)}
        },
        "ratings": {
            "v_max": r.volts,
            "rows": rows,
            "ripple": {"i_rms": r.ripple_a, "freq_hz": r.ripple_freq_hz, "coeffs": coeffs},
            "life": {"hours": hours, "temp_c": s.temp[1]}
        },
        "glyph": "electrolytic"
    });
    serde_json::from_value(part).map_err(|e| err(&e.to_string()))
}

/// The part's file text, as the bundled parts are written (pretty JSON and a newline).
pub fn to_text(p: &PartDef) -> String {
    serde_json::to_string_pretty(p).unwrap_or_default() + "\n"
}

/// Write the part into `dir` as `<name>.part.json`.
pub fn write(dir: &Path, p: &PartDef) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(crate::file_name(&p.name));
    std::fs::write(&path, to_text(p)).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// Rows read from a table without part numbers given one: the series' pattern (`Series::name`), else a plain name
/// from the values ("C_1000u_16V_10x20").
pub fn name_rows(s: Option<&Series>, rows: &mut [Row]) {
    for r in rows.iter_mut().filter(|r| r.part.is_empty()) {
        r.part = s.and_then(|s| s.name(r)).unwrap_or_else(|| format!("C_{}u_{}V_{}x{}", g(r.cap_uf), g(r.volts), g(r.d_mm), g(r.l_mm)));
    }
}

/// Rows whose leakage current is not what the series' formula gives (the table read wrong, or the formula is wrong).
pub fn leakage_mismatches<'a>(s: &Series, rows: &'a [Row]) -> Vec<&'a Row> {
    rows.iter().filter(|r| s.formula_for(r.volts) && r.leak_ua.is_some_and(|l| (l - s.leakage(r.cap_uf, r.volts)).abs() > 0.06 + 0.001 * l)).collect()
}

const BUNDLED: &[&str] = &[include_str!("../elcaps/UHE.elcap.json"), include_str!("../elcaps/ZLJ.elcap.json"), include_str!("../elcaps/UVR.elcap.json"), include_str!("../elcaps/UPW.elcap.json")];

#[cfg(test)]
mod tests {
    use super::*;

    fn uhe() -> Series {
        Series::for_part("UHE1C102MPD").unwrap()
    }

    /// A row of the table: part, µF, V, (φD, L), (Z at 20 °C, Z at −10 °C), ripple mA.
    fn row(part: &str, cap: f64, v: f64, (d, l): (f64, f64), (z, zc): (f64, f64), ripple_ma: f64) -> Row {
        Row { part: part.into(), cap_uf: cap, volts: v, d_mm: d, l_mm: l, impedance: vec![(20.0, 100e3, z), (-10.0, 100e3, zc)], ripple_a: ripple_ma / 1000.0, ripple_freq_hz: 100e3, ripple_temp_c: 105.0, leak_ua: Some(0.01 * cap * v), tan_d: None, page: None, mark: String::new() }
    }

    /// The bundled UHE parts are what `make` writes from their rows of the data sheet's table.
    #[test]
    fn the_bundled_uhe_parts_are_made_from_their_rows() {
        let s = uhe();
        let rows = [
            row("UHE1C331MPD", 330.0, 16.0, (8.0, 11.5), (0.13, 0.52), 640.0),
            row("UHE1C102MPD", 1000.0, 16.0, (10.0, 20.0), (0.046, 0.18), 1400.0),
            row("UHE1H102MHD", 1000.0, 50.0, (12.5, 40.0), (0.021, 0.069), 2920.0),
        ];
        for r in &rows {
            let p = make(&s, r).unwrap();
            let bundled = crate::EXAMPLES.iter().find(|e| e.0 == r.part).unwrap().1;
            assert_eq!(to_text(&p), bundled.replace("\r\n", "\n"), "{}", r.part);
        }
        assert_eq!(s.bundle.len(), 13);
        assert!(s.bundle.iter().all(|b| crate::EXAMPLES.iter().any(|e| e.0 == b)), "every bundled UHE part is in EXAMPLES");
    }

    /// Rubycon ZLJ: the part number made by the pattern (the "●" rows are the EFU option), the life by length too, the
    /// bundled parts what `make` writes.
    #[test]
    fn the_bundled_zlj_parts_are_made_from_their_rows() {
        let s = Series::named("ZLJ").unwrap();
        let mut rows = vec![row("", 1000.0, 25.0, (10.0, 20.0), (0.028, 0.084), 2500.0), row("", 100.0, 50.0, (8.0, 11.5), (0.12, 0.36), 950.0), row("", 12.0, 80.0, (5.0, 11.0), (0.72, 3.2), 235.0)];
        rows[2].mark = "●".into();
        name_rows(Some(&s), &mut rows);
        assert_eq!(rows.iter().map(|r| r.part.as_str()).collect::<Vec<_>>(), ["25ZLJ1000M10X20", "50ZLJ100M8X11.5", "80ZLJ12MEFU5X11"]);
        for r in &rows[..2] {
            let bundled = crate::EXAMPLES.iter().find(|e| e.0 == r.part).unwrap().1;
            assert_eq!(to_text(&make(&s, r).unwrap()), bundled.replace("\r\n", "\n"), "{}", r.part);
        }
        assert_eq!((s.life_hours(8.0, 11.5, 50.0), s.life_hours(8.0, 20.0, 50.0), s.life_hours(8.0, 20.0, 120.0), s.life_hours(6.3, 11.0, 80.0)), (Some(9000.0), Some(10000.0), Some(9000.0), Some(6000.0)));
        assert!(s.bundle.iter().all(|b| crate::EXAMPLES.iter().any(|e| e.0 == b)), "every bundled ZLJ part is in EXAMPLES");
        // the series found from the title of a table without part numbers ("ＺＬＪ" in full width)
        let nameless = vec![Row { part: String::new(), ..rows[0].clone() }];
        assert_eq!(Series::for_table(&nameless, ["ＺＬＪ"]).map(|s| s.series), Some("ZLJ".into()));
        assert_eq!(Series::for_part("25ZLJ1000M10X20").map(|s| s.series), Some("ZLJ".into()));
    }

    /// Nichicon UVR (general purpose, no impedance in the table): the ESR from tan δ at 120 Hz, the ripple current rated
    /// at 120 Hz with its coefficients by voltage range, the leakage formula up to 100 V only.
    #[test]
    fn the_bundled_uvr_parts_are_made_from_tan_delta() {
        let s = Series::named("UVR").unwrap();
        let mut r = Row { part: "UVR1C102MPD".into(), cap_uf: 1000.0, volts: 16.0, d_mm: 10.0, l_mm: 16.0, ripple_a: 0.79, ripple_freq_hz: 120.0, ripple_temp_c: 85.0, leak_ua: Some(160.0), tan_d: Some(0.2), ..Default::default() };
        let p = make(&s, &r).unwrap();
        let bundled = crate::EXAMPLES.iter().find(|e| e.0 == r.part).unwrap().1;
        assert_eq!(to_text(&p), bundled.replace("\r\n", "\n"));
        // 0.2 / (2 pi 120 Hz 1000 uF) = 265 mohm
        assert!(p.sim.unwrap().text.contains("Resr 1 n_c 265.3m"));
        assert_eq!(s.coeffs(1000.0, 16.0)[0], (50.0, 0.85));
        assert_eq!(s.coeffs(100.0, 160.0)[0], (50.0, 0.8));
        // above 100 V the table's own leakage (after 1 minute), and no mismatch reported for it
        r.volts = 160.0;
        r.leak_ua = Some(1508.0);
        assert_eq!(s.leakage_of(&r), Some(1508.0));
        assert!(leakage_mismatches(&s, std::slice::from_ref(&r)).is_empty());
        assert!(s.bundle.iter().all(|b| crate::EXAMPLES.iter().any(|e| e.0 == b)), "every bundled UVR part is in EXAMPLES");
    }

    /// Nichicon UPW: leakage 0.03 CV after 1 minute (`leak_minutes`), life by diameter only.
    #[test]
    fn the_bundled_upw_parts_are_made_from_their_rows() {
        let s = Series::named("UPW").unwrap();
        let mut r = row("UPW1C102MPD", 1000.0, 16.0, (10.0, 20.0), (0.052, 0.104), 1220.0);
        r.leak_ua = Some(480.0);
        let bundled = crate::EXAMPLES.iter().find(|e| e.0 == r.part).unwrap().1;
        assert_eq!(to_text(&make(&s, &r).unwrap()), bundled.replace("\r\n", "\n"));
        assert!(leakage_mismatches(&s, std::slice::from_ref(&r)).is_empty());
        assert_eq!((s.life_hours(8.0, 11.5, 16.0), s.life_hours(12.5, 25.0, 50.0), s.life_hours(18.0, 40.0, 100.0)), (Some(3000.0), Some(7000.0), Some(8000.0)));
        assert!(s.bundle.iter().all(|b| crate::EXAMPLES.iter().any(|e| e.0 == b)), "every bundled UPW part is in EXAMPLES");
    }

    #[test]
    fn rules_by_size_and_voltage() {
        let s = uhe();
        assert_eq!(s.life_hours(8.0, 11.5, 16.0), Some(7000.0));
        assert_eq!(s.life_hours(10.0, 20.0, 6.3), Some(6000.0));
        assert_eq!(s.life_hours(12.5, 25.0, 50.0), Some(10000.0));
        assert_eq!(s.life_hours(18.0, 40.0, 10.0), Some(8000.0));
        // 12.5 dia.: 0.8 mm leads when longer than 25 mm
        assert_eq!(s.lead(12.5, 25.0), Some((5.0, 0.6)));
        assert_eq!(s.lead(12.5, 30.5), Some((5.0, 0.8)));
        assert_eq!(s.coeffs(1200.0, 16.0)[1], (120.0, 0.8));
        assert_eq!(s.coeffs(27.0, 16.0)[0], (50.0, 0.45));
    }

    #[test]
    fn a_row_that_cannot_be_made_says_why() {
        let s = uhe();
        let mut r = row("UHE1C331MPD", 330.0, 16.0, (6.3, 11.0), (0.13, 0.52), 640.0);
        assert!(make(&s, &r).unwrap_err().contains("φ6.3"));
        r.d_mm = 8.0;
        r.impedance = vec![(20.0, 100e3, 0.001)];
        assert!(make(&s, &r).unwrap_err().contains("リアクタンス"));
        r.impedance.clear();
        assert!(make(&s, &r).unwrap_err().contains("tanδ"));
    }

    #[test]
    fn part_numbers_made_for_a_table_without_them() {
        let s = Series { series: "YXF".into(), part_pattern: "{V}YXF{C}MEFC{D}X{L}".into(), ..Default::default() };
        let mut rows = vec![row("", 1000.0, 16.0, (10.0, 20.0), (0.078, 0.3), 1050.0), row("", 4.7, 6.3, (5.0, 11.0), (1.9, 5.0), 88.0), row("UHE1C331MPD", 330.0, 16.0, (8.0, 11.5), (0.13, 0.52), 640.0)];
        name_rows(Some(&s), &mut rows);
        assert_eq!(rows.iter().map(|r| r.part.as_str()).collect::<Vec<_>>(), ["16YXF1000MEFC10X20", "6.3YXF4.7MEFC5X11", "UHE1C331MPD"]);
        let mut plain = vec![row("", 1000.0, 16.0, (10.0, 20.0), (0.078, 0.3), 1050.0)];
        name_rows(None, &mut plain);
        assert_eq!(plain[0].part, "C_1000u_16V_10x20");
    }

    #[test]
    fn numbers_written_like_the_data_sheet() {
        assert_eq!(g(0.01 * 330.0 * 16.0), "52.8");
        assert_eq!(g(1000.0), "1000");
        assert_eq!(g(0.087), "0.087");
        assert_eq!(sig(129.94, 4), "129.9");
        assert_eq!(sig(4.8229, 3), "4.82");
        assert_eq!(sig(15.915, 3), "15.9");
    }
}
