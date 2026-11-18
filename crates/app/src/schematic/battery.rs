//! "電池の持ち時間" (candidate B2): how long the battery parts of the sheet last, from the transient run's average current (the sleep
//! and the bursts of an intermittent circuit together) and the charge at which the circuit stops (`sim_core::battery`).

use super::*;
use sim_core::battery::{battery_life, batteries, hours_text, BatteryCondition};

impl SchEditor {
    /// The circuit of another schematic (a battery at less charge), built as the analyses build this one.
    fn circuit_like(&self, s: &sch_core::Schematic) -> Result<sim_core::Circuit, String> {
        let temp = self.temp_c.trim().parse::<f64>().unwrap_or(T_NOM_C);
        let c = sim_core::from_schematic_with_layout(s, &self.wiring, &self.couplings).map_err(|errs| errs.iter().map(|e| e.to_string()).collect::<Vec<_>>().join(" / "))?;
        Ok(c.at_temperature(temp))
    }

    /// Runs the transient analysis with the boxes of its section, then works out the battery life from it.
    pub fn run_battery(&mut self) {
        self.run_tran();
        let tr = match self.tran.as_ref().map(|t| &t.outcome) {
            Some(Ok(t)) => t.clone(),
            Some(Err(e)) => {
                self.bat_view = Some(Err(format!("過渡解析に失敗しました: {}", e.first().cloned().unwrap_or_default())));
                return;
            }
            None => {
                self.bat_view = Some(Err("過渡解析の終了時間を数値で入れてください (例: 300m)".into()));
                return;
            }
        };
        let time = |s: &str| if s.trim().is_empty() { Ok(0.0) } else { parse_value(s).filter(|t| *t >= 0.0).ok_or(format!("区間の時間「{s}」を、0 以上の数 (秒) で入れてください (例: 100m)")) };
        let result = (|| {
            let (from, to) = (time(&self.bat_from)?, time(&self.bat_to)?);
            let v_min = if self.bat_node.trim().is_empty() { 0.0 } else { parse_value(&self.bat_vmin).ok_or("止まる条件の電圧を数で入れてください (例: 3.2)")? };
            let cond = BatteryCondition { node: self.bat_node.trim().to_string(), v_min };
            battery_life(&self.sch, &tr, from, to, &cond, &|s| self.circuit_like(s))
        })();
        self.status = match &result {
            Ok(v) => format!("電池の持ち時間: {}", v.iter().map(|b| format!("{} {}", b.battery.refdes, hours_text(b.hours))).collect::<Vec<_>>().join("、")),
            Err(e) => format!("電池の持ち時間を出せませんでした: {e}"),
        };
        self.bat_view = Some(result);
    }

    pub(super) fn panel_battery(&mut self, ui: &mut egui::Ui) {
        let found = batteries(&self.sch);
        if found.is_empty() {
            ui.label(egui::RichText::new(i18n::tr("回路図に電池の部品がありません。ライブラリの「電池」(単3アルカリ・CR2032・18650 など) を電源に使うと、持ち時間を出せます。")).small());
            return;
        }
        ui.label(egui::RichText::new(i18n::trf!("電池: {}", found.iter().map(|b| format!("{} ({}, {} mAh)", b.refdes, b.part, b.capacity_mah)).collect::<Vec<_>>().join("、"))).small());
        ui.horizontal(|ui| {
            ui.label(i18n::tr("平均をとる区間"));
            ui.add(egui::TextEdit::singleline(&mut self.bat_from).desired_width(50.0).hint_text("0")).on_hover_text(i18n::tr("過渡解析のこの時間から (秒)。空欄なら、後半の半分 (起動のあと) を使います"));
            ui.label("〜");
            ui.add(egui::TextEdit::singleline(&mut self.bat_to).desired_width(50.0).hint_text(i18n::tr("終わり"))).on_hover_text(i18n::tr("この時間まで (秒)。間欠で動く回路 (眠る・起きる) は、区間を動きの周期のちょうど整数倍にします。半端だと、平均の電流がずれます"));
            ui.label("s");
        });
        let nets: Vec<String> = extract(&self.sch).iter().map(|n| n.name.clone()).filter(|n| n != "GND").collect();
        ui.horizontal(|ui| {
            ui.label(i18n::tr("止まる条件"));
            egui::ComboBox::from_id_salt("bat_node")
                .selected_text(if self.bat_node.is_empty() { i18n::tr("(なし: 電池の終わりまで)").to_string() } else { self.bat_node.clone() })
                .width(120.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.bat_node, String::new(), i18n::tr("(なし: 電池の終わりまで)"));
                    for n in &nets {
                        ui.selectable_value(&mut self.bat_node, n.clone(), n);
                    }
                })
                .response
                .on_hover_text(i18n::tr("回路が動くために、電圧が下がってはいけないネット (レギュレータの出力など)。電池が減って、この電圧が下限を割ったら「止まった」とします。レギュレータのドロップアウトや、マイコンの動作電圧の下限が、ここで効きます"));
            if !self.bat_node.is_empty() {
                ui.label(i18n::tr("が"));
                ui.add(egui::TextEdit::singleline(&mut self.bat_vmin).desired_width(44.0).hint_text("3.2")).on_hover_text(i18n::tr("この電圧 (V) 以上なら動いている、とします"));
                ui.label(i18n::tr("V 以上"));
            }
        });
        if ui.button(i18n::tr("過渡解析を実行して、持ち時間を出す")).on_hover_text(i18n::tr("過渡解析の欄の時間で過渡解析を実行し、区間の電池の平均電流から、何時間・何日もつかを出します。止まる条件があれば、電池の残量を減らしながら DC 動作点を解いて、条件を割る残量を探します")).clicked() {
            self.run_battery();
        }
        let Some(view) = &self.bat_view else { return };
        let list = match view {
            Ok(v) => v,
            Err(e) => {
                ui.colored_label(crate::theme::ERROR, i18n::tr_dyn(e));
                return;
            }
        };
        for b in list {
            ui.separator();
            ui.label(egui::RichText::new(i18n::trf!("{} ({}): {}", b.battery.refdes, b.battery.part, hours_text(b.hours))).strong().size(16.0).color(crate::theme::OK));
            egui::Grid::new(("bat_grid", &b.battery.refdes)).num_columns(2).spacing([16.0, 3.0]).show(ui, |ui| {
                ui.label(i18n::tr("平均の電流"));
                ui.label(format!("{}A", sim_core::format_value(b.i_avg))).on_hover_text(i18n::trf!("{}〜{} s の平均", sim_core::format_value(b.from), sim_core::format_value(b.to)));
                ui.end_row();
                ui.label(i18n::tr("いちばん大きい電流"));
                ui.label(format!("{}A", sim_core::format_value(b.i_peak)));
                ui.end_row();
                if let Some(v) = b.full_volts {
                    ui.label(i18n::tr("満充電の電池の電圧"));
                    ui.label(format!("{}V", sim_core::format_value(v))).on_hover_text(i18n::tr("新しい電池に、平均の負荷がかかっているときの端子の電圧"));
                    ui.end_row();
                }
                ui.label(i18n::tr("止まるとき"));
                let at = match b.stop_volts {
                    Some(v) if b.by_condition => i18n::trf!("電池 {}V・残り {:.0} %", sim_core::format_value(v), b.stop_soc * 100.0),
                    _ => i18n::tr("電池の終わりまで使えます").to_string(),
                };
                ui.label(at);
                ui.end_row();
                ui.label(i18n::tr("使える容量"));
                ui.label(format!("{:.0} mAh", b.battery.capacity_mah * (1.0 - b.stop_soc))).on_hover_text(i18n::tr("容量のうち、止まるまでに使える分"));
                ui.end_row();
            });
            for n in &b.notes {
                ui.label(egui::RichText::new(format!("・{}", i18n::tr_dyn(n))).small().color(crate::learn::NOTE));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sample opens with its settings (300 ms of transient, two whole periods, 3V3 at 3.2 V or more) and gives about 17 days.
    #[test]
    fn the_sample_opens_ready_and_lasts_about_seventeen_days() {
        let mut e = SchEditor::default();
        e.load_path(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/battery_life.schjson"));
        assert_eq!((e.bat_node.as_str(), e.bat_vmin.as_str(), e.tran_tstop.as_str()), ("3V3", "3.2", "300m"));
        e.run_battery();
        let list = e.bat_view.as_ref().unwrap().as_ref().unwrap();
        let days = list[0].hours / 24.0;
        assert!(days > 15.0 && days < 20.0, "{days}");
        assert!(e.status.contains("日"), "{}", e.status);
        // drawn in the analysis tab without trouble
        let ctx = egui::Context::default();
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 900.0))), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| e.panel_battery(ui));
        out.textures_delta.clear();
        // with no battery the panel says so and the run explains why
        let mut none = SchEditor::default();
        none.run_battery();
        assert!(none.bat_view.as_ref().unwrap().is_err());
    }
}
