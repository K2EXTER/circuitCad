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
    /// The part numbers bundled with the program.
    pub bundle: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CoeffRow {
    pub cap_max: f64,
    pub k: Vec<f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LifeRule {
    pub d_min: Option<f64>,
    pub d_max: Option<f64>,
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
    /// The data sheet's page (from 0), for the description.
    pub page: Option<usize>,
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

    /// The bundled series whose name the part number starts with.
    pub fn for_part(part: &str) -> Option<Series> {
        Series::bundled().into_iter().find(|s| !s.series.is_empty() && part.to_ascii_uppercase().starts_with(&s.series.to_ascii_uppercase()))
    }

    fn coeffs(&self, cap_uf: f64) -> Vec<(f64, f64)> {
        let Some(row) = self.coeffs.iter().find(|r| cap_uf <= r.cap_max).or(self.coeffs.last()) else { return vec![] };
        self.coeff_freqs.iter().copied().zip(row.k.iter().copied()).collect()
    }

    fn life_hours(&self, d: f64, v: f64) -> Option<f64> {
        self.life
            .iter()
            .find(|r| r.d_min.is_none_or(|x| d >= x) && r.d_max.is_none_or(|x| d <= x) && r.v_min.is_none_or(|x| v >= x) && r.v_max.is_none_or(|x| v <= x))
            .map(|r| r.hours)
    }

    fn lead(&self, d: f64, l: f64) -> Option<(f64, f64)> {
        let r = self.leads.iter().find(|r| (r.d - d).abs() < 1e-6)?;
        let lead = match (r.long_over, r.long_lead) {
            (Some(over), Some(long)) if l > over => long,
            _ => r.lead,
        };
        Some((r.pitch, lead))
    }

    /// The leakage current the series' formula gives (µA).
    pub fn leakage(&self, cap_uf: f64, volts: f64) -> f64 {
        (self.leak_k * cap_uf * volts).max(self.leak_min_ua)
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
    let &(_, z_freq, z20) = r.impedance.iter().find(|z| (z.0 - 20.0).abs() < 0.5).or(r.impedance.first()).ok_or_else(|| err("インピーダンスの列がありません"))?;
    let cold = r.impedance.iter().filter(|z| z.0 < 0.0).min_by(|a, b| a.0.total_cmp(&b.0)).copied();
    let (pitch, lead) = s.lead(r.d_mm, r.l_mm).ok_or_else(|| err(&format!("φ{} mm のリードの間隔と径が、シリーズの表 (leads) にありません", g(r.d_mm))))?;
    let hours = s.life_hours(r.d_mm, r.volts).ok_or_else(|| err("寿命の決まり (life) に合う行がありません"))?;
    let drill = up(lead + crate::thru_hole::HOLE_CLEARANCE, 0.1);
    let pad = up(drill + 2.0 * crate::thru_hole::ANNULAR_RING, 0.1);
    let xc = 1.0 / (2.0 * std::f64::consts::PI * z_freq * r.cap_uf * 1e-6);
    if xc >= z20 {
        return Err(err(&format!("インピーダンス {} Ω が容量のリアクタンス {} Ω より小さく、ESR が出せません (列の読み違い?)", g(z20), sig(xc, 3))));
    }
    let esr = (z20 * z20 - xc * xc).sqrt();
    let esr_m = sig(esr * 1000.0, 4);
    let mut coeffs = s.coeffs(r.cap_uf);
    if coeffs.last().is_none_or(|c| c.0 < r.ripple_freq_hz) {
        coeffs.push((r.ripple_freq_hz, 1.0));
    }
    let k120 = coeffs.iter().find(|c| (c.0 - 120.0).abs() < 1.0).map(|c| c.1);
    let leak = s.leakage(r.cap_uf, r.volts);
    let value = format!("{}u", g(r.cap_uf));
    let zf = if (z_freq - 100e3).abs() < 1.0 { "100 kHz".to_string() } else { format!("{} kHz", g(z_freq / 1e3)) };
    let rf = format!("{} kHz", g(r.ripple_freq_hz / 1e3));
    let tmax = g(s.temp[1]);
    let cold_text = cold.map(|c| format!("、{} ℃では {} Ω 以下", g(c.0), g(c.2))).unwrap_or_default();
    let ripple_ma = g(r.ripple_a * 1000.0);
    let k120_text = k120.map(|k| format!("、120 Hz では ×{k:.2}")).unwrap_or_default();
    let desc = format!(
        "{maker} {series} アルミ電解コンデンサ {cap} µF ±{tol} % {v} V ({tmax} ℃・{hours} 時間、{feature})。本体 φ{d} × {l} mm、リード φ{lead} mm・間隔 {pitch} mm。インピーダンス {z} Ω 以下 ({zf}・20 ℃{cold_text})、許容リップル電流 {ripple_ma} mA rms ({rf}・{rt} ℃{k120_text})。極性があります: 1 番ピン (四角いパッド) が +。逆につなぐと壊れます。穴径 {drill:.1} mm・パッド径 {pad:.1} mm はリード径からの目安 (部品作成フォームのリード部品と同じ考え方。メーカーの推奨ランドではない)。{order}{source} で確かめた ({checked})",
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
        z = g(z20),
        rt = g(r.ripple_temp_c),
        order = s.order_note,
        source = s.source,
        checked = s.checked,
    );
    let cold_limit = match cold {
        Some(c) => format!("({} ℃では {} Ω 以下、約 {:.1} 倍、データシートの表)", g(c.0), g(c.2), c.2 / z20),
        None => "(低い温度での値はデータシートの表を見てください)".to_string(),
    };
    let leak_text = if s.leak_min_ua > 0.0 { format!("{} CV か {} µA の大きいほう", g(s.leak_k), g(s.leak_min_ua)) } else { format!("{} CV", g(s.leak_k)) };
    let mut rows = vec![json!({"name": "インピーダンス", "value": format!("{} Ω 以下", g(z20)), "condition": format!("{zf}・20 ℃")})];
    if let Some(c) = cold {
        rows.push(json!({"name": "インピーダンス (低温)", "value": format!("{} Ω 以下", g(c.2)), "condition": format!("{zf}・{} ℃", g(c.0))}));
    }
    rows.push(json!({"name": "許容リップル電流", "value": format!("{ripple_ma} mA rms"), "condition": format!("{rf}・{} ℃", g(r.ripple_temp_c))}));
    rows.push(json!({"name": "寿命", "value": format!("{} 時間", g(hours)), "condition": format!("{tmax} ℃・定格電圧とリップル電流をかけて")}));
    rows.push(json!({"name": "漏れ電流", "value": format!("{} µA 以下 ({leak_text})", g(leak)), "condition": "定格電圧で 2 分後・20 ℃"}));
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
                format!("ESR は、データシートの {zf}・20 ℃のインピーダンス {} Ω から容量のリアクタンス ({} mΩ) を除いた値 ({} mΩ) を、全部の周波数で一定にしています。実物は低い周波数と低い温度で大きくなります {cold_limit}。", g(z20), sig(xc * 1000.0, 3), sig(esr * 1000.0, 3)),
                "ESL (リードと巻き取りのインダクタンス) はデータシートに無いので入れていません。数百 kHz より上では実物と違います。",
                format!("容量の許容差 (±{} %)、温度による容量の変化、漏れ電流 ({leak_text}) は入れていません。", g(s.tolerance_pct)),
                "逆電圧と定格を超える電圧は、シミュレーションでは止まりません。極性と定格電圧を守ってください。過渡解析のあと、波形の「部品の定格 (リップル電流・寿命)」で、リップル電流と推定寿命を確かめられます。"
            ],
            "origin": {"kind": "equivalent", "source": format!("{} {} series の{}の容量とインピーダンス", s.maker, s.series, s.source)}
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

/// Rows whose leakage current is not what the series' formula gives (the table read wrong, or the formula is wrong).
pub fn leakage_mismatches<'a>(s: &Series, rows: &'a [Row]) -> Vec<&'a Row> {
    rows.iter().filter(|r| r.leak_ua.is_some_and(|l| (l - s.leakage(r.cap_uf, r.volts)).abs() > 0.06 + 0.001 * l)).collect()
}

const BUNDLED: &[&str] = &[include_str!("../elcaps/UHE.elcap.json")];

#[cfg(test)]
mod tests {
    use super::*;

    fn uhe() -> Series {
        Series::for_part("UHE1C102MPD").unwrap()
    }

    /// A row of the table: part, µF, V, (φD, L), (Z at 20 °C, Z at −10 °C), ripple mA.
    fn row(part: &str, cap: f64, v: f64, (d, l): (f64, f64), (z, zc): (f64, f64), ripple_ma: f64) -> Row {
        Row { part: part.into(), cap_uf: cap, volts: v, d_mm: d, l_mm: l, impedance: vec![(20.0, 100e3, z), (-10.0, 100e3, zc)], ripple_a: ripple_ma / 1000.0, ripple_freq_hz: 100e3, ripple_temp_c: 105.0, leak_ua: Some(0.01 * cap * v), page: None }
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

    #[test]
    fn rules_by_size_and_voltage() {
        let s = uhe();
        assert_eq!(s.life_hours(8.0, 16.0), Some(7000.0));
        assert_eq!(s.life_hours(10.0, 6.3), Some(6000.0));
        assert_eq!(s.life_hours(12.5, 50.0), Some(10000.0));
        assert_eq!(s.life_hours(18.0, 10.0), Some(8000.0));
        // 12.5 dia.: 0.8 mm leads when longer than 25 mm
        assert_eq!(s.lead(12.5, 25.0), Some((5.0, 0.6)));
        assert_eq!(s.lead(12.5, 30.5), Some((5.0, 0.8)));
        assert_eq!(s.coeffs(1200.0)[1], (120.0, 0.8));
        assert_eq!(s.coeffs(27.0)[0], (50.0, 0.45));
    }

    #[test]
    fn a_row_that_cannot_be_made_says_why() {
        let s = uhe();
        let mut r = row("UHE1C331MPD", 330.0, 16.0, (6.3, 11.0), (0.13, 0.52), 640.0);
        assert!(make(&s, &r).unwrap_err().contains("φ6.3"));
        r.d_mm = 8.0;
        r.impedance = vec![(20.0, 100e3, 0.001)];
        assert!(make(&s, &r).unwrap_err().contains("リアクタンス"));
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
