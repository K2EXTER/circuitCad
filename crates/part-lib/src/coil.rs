//! Power inductors made from a maker's lineup table: one row per part number (inductance, tolerance, DCR, the heating
//! and the saturation currents, the self-resonant frequency; read by `datasheet::lineup::inductor`) and what holds for
//! the whole series (body, recommended land pattern, how far the inductance has fallen at Isat, temperatures), written
//! once in a `*.coil.json` (`crates/part-lib/coils/`). The model is the chip part's inductor (`passive`): DCR in
//! series, the saturation as L / (1 + (I / i0)²) through Isat, the winding's capacitance from the SRF. The rated
//! currents go into `ratings.coil`, which a transient run checks (`sim_core::ripple_reports`).

use crate::PartDef;
use crate::passive::{self, PassiveKind, PassiveTemplate};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// What holds for the whole series (`*.coil.json`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Series {
    /// "Bourns"
    pub maker: String,
    /// "SRN6045"
    pub series: String,
    /// What the series is, in the part's description ("半シールド").
    pub feature: String,
    pub datasheet: String,
    /// The data sheet as named in the part.
    pub source: String,
    pub checked: String,
    /// Operating temperature range (°C), the part's own rise included.
    pub temp: [f64; 2],
    /// How far the inductance has fallen at Isat (0.3 = 30 %).
    pub sat_drop: f64,
    /// The temperature rise at the rated rms current (°C).
    pub rise_c: f64,
    /// Body (mm): width across the pads, depth along them, height.
    pub body_w: f64,
    pub body_d: f64,
    pub height: f64,
    /// Recommended land pattern (mm): each pad's width (across) and length, and the distance between the pads' centres.
    pub pad_w: f64,
    pub pad_l: f64,
    pub pad_pitch: f64,
    /// How the part number is ordered, in the description.
    pub order_note: String,
    /// The part numbers bundled with the program.
    pub bundle: Vec<String>,
}

/// One row of the lineup table.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub part: String,
    pub l_uh: f64,
    pub tol_pct: Option<f64>,
    pub dcr_ohm: Option<f64>,
    /// The table's DCR is a maximum (else typical).
    pub dcr_max: bool,
    pub i_rms: Option<f64>,
    pub i_sat: Option<f64>,
    pub srf_hz: Option<f64>,
}

impl Series {
    /// The bundled series' tables (`crates/part-lib/coils/`).
    pub fn bundled() -> Vec<Series> {
        BUNDLED.iter().filter_map(|t| serde_json::from_str(t).ok()).collect()
    }

    /// The bundled series whose name the part number starts with ("SRN6045-100M").
    pub fn for_part(part: &str) -> Option<Series> {
        let up = part.to_ascii_uppercase();
        Series::bundled().into_iter().find(|s| !s.series.is_empty() && up.starts_with(&s.series.to_ascii_uppercase()))
    }

    pub fn named(name: &str) -> Option<Series> {
        Series::bundled().into_iter().find(|s| s.series.eq_ignore_ascii_case(name.trim()))
    }
}

/// Like C's "%g" for the numbers of a data sheet ("0.0586", "10", "2.52").
fn g(x: f64) -> String {
    let s = format!("{x:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" { "0".into() } else { s.to_string() }
}

/// The part for one row of the table.
pub fn make(s: &Series, r: &Row) -> Result<PartDef, String> {
    let err = |what: &str| format!("{}: {what}", r.part);
    if r.l_uh.is_nan() || r.l_uh <= 0.0 {
        return Err(err("インダクタンスが読めていません"));
    }
    if !(s.body_w > 0.0 && s.pad_w > 0.0 && s.pad_l > 0.0 && s.pad_pitch > 0.0) {
        return Err(err("シリーズの表に本体の大きさと推奨のランド (pad_w・pad_l・pad_pitch) がありません"));
    }
    let l = r.l_uh * 1e-6;
    let mut t = PassiveTemplate::new(PassiveKind::Inductor, l);
    t.dcr = r.dcr_ohm;
    t.isat = r.i_sat;
    if s.sat_drop > 0.0 {
        t.ldrop = s.sat_drop;
    }
    if let Some(f0) = r.srf_hz {
        t.cp = passive::cp_from_srf(l, f0).ok();
    }
    let made = t.generate(&r.part)?;
    let value = passive::eng(l);
    let drop_pct = t.ldrop * 100.0;
    let mut desc = format!("{} {} パワーインダクタ ({}) {} µH", s.maker, s.series, s.feature, g(r.l_uh));
    if let Some(tol) = r.tol_pct {
        desc += &format!(" ±{} %", g(tol));
    }
    desc += "。";
    if let Some(dcr) = r.dcr_ohm {
        desc += &format!("DCR {} mΩ{}", g(dcr * 1000.0), if r.dcr_max { " 以下" } else { " (typ)" });
    }
    if let Some(i) = r.i_rms {
        desc += &format!("、定格電流 {} A (温度上昇 {} ℃)", g(i), g(s.rise_c));
    }
    if let Some(i) = r.i_sat {
        desc += &format!("、飽和電流 {} A (インダクタンスが {:.0} % 下がる電流)", g(i), drop_pct);
    }
    if let Some(f) = r.srf_hz {
        desc += &format!("、自己共振周波数 {} MHz", g(f / 1e6));
    }
    desc += &format!(
        "。本体 {} × {} × 高さ {} mm、パッドはデータシートの推奨ランド ({} × {} mm、中心の間隔 {} mm)。{}{} で確かめた ({})",
        g(s.body_w),
        g(s.body_d),
        g(s.height),
        g(s.pad_w),
        g(s.pad_l),
        g(s.pad_pitch),
        s.order_note,
        s.source,
        s.checked
    );
    let mut limits = made.limits.clone();
    if r.dcr_max {
        limits.push("DCR はデータシートの最大値です。実物はこれより少し小さいことが多いです。".into());
    }
    limits.push("インダクタンスの許容差、周波数による損失 (コアの損失) は入れていません。定格電流と飽和電流は、過渡解析のあと「部品の定格」で電流と比べます。".into());
    let mut rows = vec![];
    if let Some(dcr) = r.dcr_ohm {
        rows.push(json!({"name": "DCR", "value": format!("{} mΩ{}", g(dcr * 1000.0), if r.dcr_max { " 以下" } else { "" }), "condition": if r.dcr_max { "最大" } else { "typ" }}));
    }
    if let Some(i) = r.i_rms {
        rows.push(json!({"name": "定格電流 (Irms)", "value": format!("{} A", g(i)), "condition": format!("温度上昇 {} ℃", g(s.rise_c))}));
    }
    if let Some(i) = r.i_sat {
        rows.push(json!({"name": "飽和電流 (Isat)", "value": format!("{} A", g(i)), "condition": format!("インダクタンスが {drop_pct:.0} % 下がる")}));
    }
    if let Some(f) = r.srf_hz {
        rows.push(json!({"name": "自己共振周波数", "value": format!("{} MHz", g(f / 1e6)), "condition": "typ"}));
    }
    rows.push(json!({"name": "使える温度", "value": format!("{}〜{} ℃", g(s.temp[0]).replace('-', "−"), g(s.temp[1])), "condition": "自分の温度上昇を含めて"}));
    let x = s.pad_pitch / 2.0;
    let part = json!({
        "name": r.part,
        "category": "コイル",
        "description": desc,
        "datasheet": s.datasheet,
        "prefix": "L",
        "value": value,
        "pins": [{"number": "1", "name": "1", "side": "left"}, {"number": "2", "name": "2", "side": "right"}],
        "package": {"type": "explicit", "pads": [
            {"pin": "1", "x": -x, "y": 0.0, "shape": "rect", "w": s.pad_w, "h": s.pad_l, "drill": null, "slot": 0.0, "slot_v": false},
            {"pin": "2", "x": x, "y": 0.0, "shape": "rect", "w": s.pad_w, "h": s.pad_l, "drill": null, "slot": 0.0, "slot_v": false}
        ], "holes": []},
        "footprint_name": format!("L_{}_{}", s.maker, s.series),
        "height": s.height,
        "body": {"shape": "rect", "w": s.body_w, "h": s.body_d, "x": 0.0, "y": 0.0, "mark": "none", "mark_pin": ""},
        "sim": {
            "kind": "subckt",
            "element": "",
            "name": made.sim_name,
            "text": made.sim_text,
            "pins": made.sim_pins,
            "params": {},
            "limits": limits,
            "origin": {"kind": "equivalent", "source": format!("{} {} のデータシート ({}) のインダクタンス・DCR・飽和電流・自己共振周波数", s.maker, s.series, s.source)}
        },
        "ratings": {
            "rows": rows,
            "coil": {"i_rms": r.i_rms, "i_sat": r.i_sat, "sat_drop": t.ldrop}
        },
        "glyph": "inductor"
    });
    serde_json::from_value(part).map_err(|e| err(&e.to_string()))
}

/// The part's file text, as the bundled parts are written (pretty JSON and a newline).
pub fn to_text(p: &PartDef) -> String {
    serde_json::to_string_pretty(p).unwrap_or_default() + "\n"
}

const BUNDLED: &[&str] = &[include_str!("../coils/SRN6045.coil.json")];

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> Row {
        Row { part: "SRN6045-100M".into(), l_uh: 10.0, tol_pct: Some(20.0), dcr_ohm: Some(0.0586), dcr_max: true, i_rms: Some(2.5), i_sat: Some(3.0), srf_hz: Some(15e6) }
    }

    /// Bourns SRN6045-100M: the model carries the DCR, the saturation and the winding's capacitance, the ratings the
    /// currents, the pads the recommended layout; the bundled part is what `make` writes.
    #[test]
    fn the_bundled_srn6045_parts_are_made_from_their_rows() {
        let s = Series::for_part("SRN6045-100M").unwrap();
        let p = make(&s, &row()).unwrap();
        let sim = p.sim.as_ref().unwrap();
        assert!(sim.text.contains("Rdcr 1 n_dcr 58.6m") && sim.text.contains("ISAT=3.0000e0 LDROP=0.300"), "{}", sim.text);
        let coil = p.ratings.as_ref().unwrap().coil.as_ref().unwrap();
        assert_eq!((coil.i_rms, coil.i_sat, coil.sat_drop), (Some(2.5), Some(3.0), 0.3));
        let bundled = crate::EXAMPLES.iter().find(|e| e.0 == "SRN6045-100M").unwrap().1;
        assert_eq!(to_text(&p), bundled.replace("\r\n", "\n"));
        assert!(s.bundle.iter().all(|b| crate::EXAMPLES.iter().any(|e| e.0 == b)), "every bundled SRN6045 part is in EXAMPLES");
    }

    #[test]
    fn a_series_without_its_land_pattern_says_so() {
        let s = Series { series: "X".into(), ..Default::default() };
        assert!(make(&s, &row()).unwrap_err().contains("推奨のランド"));
    }
}
