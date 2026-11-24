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

/// The op amp's figures against its readings, measured on a `vs` V supply; the open-loop gain and the short-circuit current on
/// `vs_hi` V, where the data sheets often give them (LM358: AOL at 15 V, ISC at 10 V; the rest at 5 V).
pub fn compare_opamp(s: &fit::OpampSpec, f: &Fitted, vs: f64, vs_hi: f64) -> Result<Vec<CompareRow>, String> {
    let name = f.text.split_whitespace().nth(1).unwrap_or("X").to_string();
    let mut m = model_meas::opamp(&name, &f.text, vs)?;
    if (vs_hi - vs).abs() > 1e-9 {
        let hi = model_meas::opamp(&name, &f.text, vs_hi)?;
        m.retain(|x| !matches!(x.key, "aol" | "isc"));
        m.extend(hi.into_iter().filter(|x| matches!(x.key, "aol" | "isc")));
    }
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
            let (vs, hi) = self.opamp.meas_supplies();
            self.opamp.measured = Some(compare_opamp(&s, f, vs, hi));
        }
        if let (Ok(s), Some(Ok(f))) = (self.reg.spec(&name, &source), &self.reg.result) {
            self.reg.measured = Some(compare_regulator(&s, f));
        }
    }
}

/// One line of the report card: which section, what, the conditions, the data sheet's value and the model's.
#[derive(Clone, Debug)]
pub struct ReportRow {
    pub section: &'static str,
    pub item: String,
    pub condition: String,
    pub datasheet: f64,
    pub model: f64,
    pub unit: String,
    /// Shown for comparison only (the fit does not aim at it).
    pub compare_only: bool,
}

impl PartForm {
    /// The report card (M3): every model made in the form's sections against its data sheet, in one list: the fits' own rows (a
    /// diode's forward points, a MOSFET's RDS(on), a reference's PSRR, a buck's frequency …) and the op amp's and regulator's
    /// measured tables.
    pub fn report_rows(&self) -> Vec<ReportRow> {
        let fitted: [(&'static str, Option<&Result<Fitted, String>>); 15] = [
            ("ダイオード", self.dio_result.as_ref()),
            ("MOSFET", self.mos.result.as_ref()),
            ("トランジスタ", self.bjt.result.as_ref()),
            ("ダーリントン", self.darl.result.as_ref()),
            ("フォトカプラ", self.opto.result.as_ref()),
            ("フォトダイオード", self.pd.result.as_ref()),
            ("オペアンプ", self.opamp.result.as_ref()),
            ("レギュレータ", self.reg.result.as_ref()),
            ("電圧基準", self.vref.result.as_ref()),
            ("SET ピンの LDO", self.setldo.result.as_ref()),
            ("降圧 DC-DC", self.buck.result.as_ref()),
            ("NTC", self.ntc.result.as_ref()),
            ("水晶", self.xtal.result.as_ref()),
            ("TVS", self.tvs.result.as_ref()),
            ("リレー", self.relay.result.as_ref()),
        ];
        let mut out = vec![];
        for (section, r) in fitted {
            if let Some(Ok(f)) = r {
                out.extend(f.rows.iter().map(|x| ReportRow { section, item: x.item.clone(), condition: x.at.clone(), datasheet: x.datasheet, model: x.model, unit: x.unit.clone(), compare_only: x.compare_only }));
            }
        }
        for (section, m) in [("オペアンプ (測った値)", &self.opamp.measured), ("レギュレータ (測った値)", &self.reg.measured)] {
            if let Some(Ok(rows)) = m {
                out.extend(rows.iter().map(|x| ReportRow { section, item: x.item.to_string(), condition: x.condition.clone(), datasheet: x.datasheet, model: x.model, unit: x.unit.to_string(), compare_only: false }));
            }
        }
        out
    }

    /// The report card's box: the rows (the gap over 5 % in orange), the worst gap, and a button to copy them as a table.
    pub(super) fn report_ui(&mut self, ui: &mut egui::Ui) {
        let rows = self.report_rows();
        if rows.is_empty() {
            return;
        }
        egui::CollapsingHeader::new(i18n::tr("成績表: 作ったモデルとデータシートのずれ (全部の欄)")).id_salt("report_card").show(ui, |ui| {
            let gap = |r: &ReportRow| (r.model / r.datasheet - 1.0) * 100.0;
            let worst = rows.iter().filter(|r| !r.compare_only && r.datasheet != 0.0).map(|r| gap(r).abs()).fold(0.0, f64::max);
            let over = rows.iter().filter(|r| !r.compare_only && gap(r).abs() > 5.0).count();
            ui.label(i18n::trf!("{} 行、いちばん大きいずれ {worst:.1} %、5 % を超える行 {over}", rows.len()));
            if ui.button(i18n::tr("表を写す")).on_hover_text(i18n::tr("成績表を、表計算に貼れる形 (タブ区切り) でクリップボードに写します")).clicked() {
                let mut t = String::from("欄\t項目\t条件\tデータシート\tモデル\t単位\tずれ %\n");
                for r in &rows {
                    t += &format!("{}\t{}\t{}\t{}\t{}\t{}\t{:.2}\n", r.section, r.item, r.condition, r.datasheet, r.model, r.unit, gap(r));
                }
                ui.ctx().copy_text(t);
            }
            egui::ScrollArea::vertical().max_height(300.0).id_salt("report_rows").show(ui, |ui| {
                egui::Grid::new("report_grid").num_columns(6).striped(true).show(ui, |ui| {
                    for h in ["欄", "項目", "条件", "データシート", "モデル", "ずれ"] {
                        ui.label(egui::RichText::new(i18n::tr(h)).strong());
                    }
                    ui.end_row();
                    for r in &rows {
                        let e = gap(r);
                        let color = if r.compare_only { ui.visuals().weak_text_color() } else if e.abs() > 5.0 { crate::theme::warn() } else { ui.visuals().text_color() };
                        ui.label(i18n::tr_dyn(r.section));
                        ui.label(&r.item);
                        ui.label(egui::RichText::new(&r.condition).small());
                        ui.label(format!("{}{}", eng(r.datasheet), r.unit));
                        ui.label(format!("{}{}", eng(r.model), r.unit));
                        ui.colored_label(color, format!("{e:+.1}%{}", if r.compare_only { i18n::tr(" (比べるだけ)") } else { "" }));
                        ui.end_row();
                    }
                });
            });
        });
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// The report card gathers the sections' rows: the voltage reference's PSRR points and the op amp's measured table, each
    /// under its section.
    #[test]
    fn the_report_card_gathers_every_made_model() {
        let mut f = PartForm::default();
        assert!(f.report_rows().is_empty());
        f.refldo_example_and_make(super::super::refldo::RefLdo::Reference);
        f.opamp_example_and_make();
        f.measure_models();
        let rows = f.report_rows();
        let n = |s: &str| rows.iter().filter(|r| r.section == s).count();
        assert_eq!(n("電圧基準"), 7, "the PSRR points");
        assert!(n("オペアンプ (測った値)") >= 9, "{rows:?}");
    }
}
