//! M3: the op amp and regulator models made in the form, measured the way the bundled ones are compared with their data
//! sheets (`sim_core::model_meas`), next to the values typed from the data sheet: one table, the gap in orange over 5 %.

use super::*;
use part_lib::fit::{self, Fitted};
use part_lib::passive::eng;
use sim_core::model_meas::{self, Measured, RegulatorTest};

/// One line of the table: what, the data sheet's value, the model's, the unit and the conditions.
#[derive(Clone, Debug)]
pub struct CompareRow {
    pub item: &'static str,
    pub datasheet: f64,
    pub model: f64,
    pub unit: &'static str,
    pub condition: String,
}

fn row(m: &[Measured], key: &str, item: &'static str, want: f64, unit: &'static str) -> Option<CompareRow> {
    let x = m.iter().find(|x| x.key == key)?;
    (want.is_finite() && want != 0.0).then(|| CompareRow { item, datasheet: want, model: x.value, unit, condition: x.condition.clone() })
}

/// The op amp's figures against its readings, measured on a `vs` V supply.
pub fn compare_opamp(s: &fit::OpampSpec, f: &Fitted, vs: f64) -> Result<Vec<CompareRow>, String> {
    let name = f.text.split_whitespace().nth(1).unwrap_or("X").to_string();
    let m = model_meas::opamp(&name, &f.text, vs)?;
    let rows = [
        row(&m, "aol", "開ループ利得", s.aol_db, "dB"),
        row(&m, "gbw", "GBW (単位利得の周波数)", s.gbw, "Hz"),
        s.pm.and_then(|pm| row(&m, "pm", "位相余裕", pm, "°")),
        row(&m, "sr", "スルーレート", s.sr, "V/s"),
        row(&m, "vos", "入力オフセット電圧", s.vos.abs(), "V"),
        row(&m, "ib", "入力バイアス電流", s.ib.abs(), "A"),
        row(&m, "iq", "電源電流 (1 回路)", s.iq, "A"),
        row(&m, "hu", "出力の振れ幅 (正の電源から)", s.hu, "V"),
        row(&m, "hl", "出力の振れ幅 (負の電源から)", s.hl, "V"),
        row(&m, "isc", "出力の短絡電流", s.isc, "A"),
        row(&m, "en", "入力換算の雑音 (1 kHz)", s.en, "V/√Hz"),
    ];
    Ok(rows.into_iter().flatten().collect())
}

/// The regulator's figures against its readings: at the table's input (6.25 V when not given), 5 mA of load (the minimum load
/// of an adjustable one), the load step and the dropout at 0.5 A.
pub fn compare_regulator(s: &fit::RegulatorSpec, f: &Fitted) -> Result<Vec<CompareRow>, String> {
    let name = f.text.split_whitespace().nth(1).unwrap_or("X").to_string();
    let vin0 = s.vin0.unwrap_or(6.25).max(s.vout + s.vdo0 + 1.0);
    let light = if s.adjustable { s.min_load.unwrap_or(10e-3).max(1e-3) } else { 5e-3 };
    let heavy = 0.5;
    let t = RegulatorTest { adjustable: s.adjustable, vin0, light, heavy, expected_dropout: s.vdo0 + s.rdo * heavy, rr_f: s.rr_f };
    let m = model_meas::regulator(&name, &f.text, &t)?;
    // the current limit at the input − output difference of a short (the input itself), on the line through the two points
    let ilim = {
        let [a, b] = s.ilim;
        let k = if (b.diff - a.diff).abs() > 1e-12 { (b.amps - a.amps) / (b.diff - a.diff) } else { 0.0 };
        let i = a.amps + k * (vin0 - a.diff);
        i.clamp(a.amps.min(b.amps), a.amps.max(b.amps))
    };
    let rows = [
        row(&m, "vout", if s.adjustable { "基準電圧" } else { "出力電圧" }, s.vout, "V"),
        row(&m, "line", "ライン・レギュレーション (1 V あたり)", s.line, "V/V"),
        row(&m, "load", "ロード・レギュレーション (出力抵抗)", s.rout, "Ω"),
        row(&m, "iq", if s.adjustable { "ADJ の電流" } else { "グラウンド電流" }, if s.adjustable { s.iadj } else { s.iq }, "A"),
        row(&m, "dropout", "ドロップアウト (0.5 A)", s.vdo0 + s.rdo * heavy, "V"),
        row(&m, "ilim", "電流制限 (短絡)", ilim, "A"),
        row(&m, "rr", "リップル除去", s.rr, "dB"),
    ];
    Ok(rows.into_iter().flatten().collect())
}

/// The table: the gap over 5 % in orange.
pub(super) fn compare_table(ui: &mut egui::Ui, id: &str, rows: &[CompareRow]) {
    egui::Grid::new(id).num_columns(5).striped(true).show(ui, |ui| {
        for h in ["項目", "データシート", "モデル", "ずれ", "測った条件"] {
            ui.label(egui::RichText::new(i18n::tr(h)).strong());
        }
        ui.end_row();
        for r in rows {
            let e = (r.model / r.datasheet - 1.0) * 100.0;
            let color = if e.abs() > 5.0 { crate::theme::warn() } else { ui.visuals().text_color() };
            ui.label(i18n::tr_dyn(r.item));
            ui.label(format!("{}{}", eng(r.datasheet), r.unit));
            ui.label(format!("{}{}", eng(r.model), r.unit));
            ui.colored_label(color, format!("{e:+.1}%"));
            ui.label(egui::RichText::new(&r.condition).small());
            ui.end_row();
        }
    });
}

impl PartForm {
    /// The op amp's and the regulator's models, when made, measured against their boxes (`--form-measure`).
    pub fn measure_models(&mut self) {
        let (name, source) = (self.linear_name("X"), self.linear_source());
        if let (Ok(s), Some(Ok(f))) = (self.opamp.spec(&name, &source), &self.opamp.result) {
            let vs = sim_core::parse_value(self.opamp.meas_vs.trim()).filter(|v| *v > 0.0).unwrap_or(5.0);
            self.opamp.measured = Some(compare_opamp(&s, f, vs));
        }
        if let (Ok(s), Some(Ok(f))) = (self.reg.spec(&name, &source), &self.reg.result) {
            self.reg.measured = Some(compare_regulator(&s, f));
        }
    }
}

