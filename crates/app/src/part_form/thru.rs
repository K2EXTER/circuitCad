//! Package tab helper: hole, pad and pitch of a leaded part from the data sheet's mechanical drawing, for the
//! many data sheets that give no recommended land pattern.

use super::*;
use part_lib::thru_hole;

impl PartForm {
    fn thru_number(&self, label: &str, t: &str) -> Result<f64, String> {
        sim_core::parse_value(t.trim().to_ascii_lowercase().trim_end_matches("mm").trim()).filter(|v| *v > 0.0).ok_or_else(|| format!("{label}を、0 より大きい数 (mm) で入れてください (例: 0.864)"))
    }

    /// The suggestion for the numbers typed in the helper.
    pub(super) fn thru_suggest(&self) -> Result<thru_hole::ThruHole, String> {
        let lead = self.thru_number("リード径", &self.thru_lead)?;
        if self.thru_radial {
            thru_hole::radial(lead, self.thru_number("リードの間隔", &self.thru_pitch)?)
        } else {
            thru_hole::axial(lead, self.thru_number("本体の長さ", &self.thru_len)?, self.thru_number("本体の径", &self.thru_dia)?)
        }
    }

    /// Put a suggestion into the package fields.
    pub(super) fn thru_apply(&mut self, t: &thru_hole::ThruHole) {
        self.pkg = PkgKind::Row;
        self.pitch = t.pitch;
        self.drill = t.drill;
        self.pad_d = t.pad_d;
        if !self.thru_radial {
            // lying down: the body runs along the leads (x), its diameter is the height above the board
            self.body_on = true;
            self.body_circle = false;
            (self.body_w, self.body_h, self.body_x, self.body_y) = (t.body_len, t.body_dia, 0.0, 0.0);
            self.height = t.body_dia;
        }
    }

    pub(super) fn thru_ui(&mut self, ui: &mut egui::Ui) {
        let open = self.ptype == Some(PartType::Diode) || self.ptype == Some(PartType::Resistor);
        egui::CollapsingHeader::new("リード付き部品: 外形寸法から穴とピッチを決める (推奨ランドパターンが無いとき)").id_salt("thru_help").default_open(open).show(ui, |ui| {
            crate::icons::help_small(ui, "外形寸法から穴とパッドを決めます", "ダイオードや抵抗のデータシートには、推奨ランドパターンが無く、外形寸法 (本体の大きさとリードの太さ) だけが載っていることがよくあります。その数字から、穴径・パッド径・穴の間隔を決めます");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.thru_radial, false, "軸方向 (ダイオード・抵抗)").on_hover_text("本体の両端からリードが出ている形。基板に寝かせて、リードを折り曲げて挿します");
                ui.selectable_value(&mut self.thru_radial, true, "ラジアル (LED・電解コンデンサ)").on_hover_text("リードが本体の同じ側から出ている形。リードの間隔は、データシートにあります");
                if ui.small_button("DO-41 の例 (1N4007)").on_hover_text("1N4007 のデータシート (Diodes Inc.) の外形寸法の最大値を入れます。リード径 0.864、本体 5.21 × 2.72").clicked() {
                    (self.thru_radial, self.thru_lead, self.thru_len, self.thru_dia) = (false, "0.864".into(), "5.21".into(), "2.72".into());
                }
            });
            egui::Grid::new("thru_grid").num_columns(2).show(ui, |ui| {
                ui.label("リード径 (最大)").on_hover_text("データシートの外形図の、リードの直径 (Lead Diameter) の最大値。1N4007 は 0.71〜0.864 なので 0.864");
                ui.add(egui::TextEdit::singleline(&mut self.thru_lead).desired_width(70.0).hint_text("0.864"));
                ui.end_row();
                if self.thru_radial {
                    ui.label("リードの間隔").on_hover_text("データシートの寸法表の、リードの間隔 (Lead Spacing、F)。電解コンデンサは 2.0 / 2.5 / 3.5 / 5.0 など");
                    ui.add(egui::TextEdit::singleline(&mut self.thru_pitch).desired_width(70.0).hint_text("2.5"));
                } else {
                    ui.label("本体の長さ (最大)").on_hover_text("外形図の、リードに沿った本体の長さ。1N4007 は 4.06〜5.21 なので 5.21");
                    ui.add(egui::TextEdit::singleline(&mut self.thru_len).desired_width(70.0).hint_text("5.21"));
                    ui.end_row();
                    ui.label("本体の径 (最大)").on_hover_text("外形図の、本体の直径。1N4007 は 2.00〜2.72 なので 2.72。基板に寝かせたときの高さにもなります");
                    ui.add(egui::TextEdit::singleline(&mut self.thru_dia).desired_width(70.0).hint_text("2.72"));
                }
                ui.end_row();
            });
            if self.thru_lead.trim().is_empty() {
                return;
            }
            match self.thru_suggest() {
                Ok(t) => {
                    for n in &t.notes {
                        ui.label(egui::RichText::new(format!("・{n}")).size(13.5));
                    }
                    if ui.button("この寸法を入れる").on_hover_text("穴径・パッド径・ピッチを下の欄に入れます。軸方向なら、本体の外形 (長さ × 径) と高さも入ります").clicked() {
                        self.thru_apply(&t);
                    }
                }
                Err(e) => {
                    ui.colored_label(Color32::from_rgb(230, 160, 40), e);
                }
            }
        });
        // a hole that is too tight for the lead typed above
        let tight = self.thru_number("", &self.thru_lead).ok().filter(|lead| self.pkg == PkgKind::Row && self.fp_preset.is_empty() && self.drill < lead + 0.15);
        if let Some(lead) = tight {
            ui.colored_label(Color32::from_rgb(230, 160, 40), format!("穴径 {:.2}mm は、リード径 {lead:.3}mm に対して余裕が {:.2}mm しかありません。リードが入らなかったり、はんだが流れなかったりします (目安は リード径 + 0.25mm)", self.drill, self.drill - lead));
        }
    }
}
