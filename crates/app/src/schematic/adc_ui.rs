//! "ADC の読み" in the selected part's settings (candidate L1): for an ADC (a part whose model has `adc_bits`), the inputs, the
//! reference and the gain its registers would set, and the code it reads at the DC point (`sim_core::adcread`).

use super::*;
use sim_core::adcread::{adc_parts, default_setup, noise_rates, read, AdcFilter, AdcSetup};

const GAINS: [f64; 8] = [1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0];

impl SchEditor {
    /// The section, when part `i` is an ADC.
    pub(super) fn adc_read_ui(&mut self, ui: &mut egui::Ui, i: usize) {
        let refdes = self.sch.components[i].refdes.clone();
        let Some((_, bits, pins)) = adc_parts(&self.sch).into_iter().find(|(r, _, _)| *r == refdes) else { return };
        let mut setup = self.adc_setups.get(&refdes).cloned().unwrap_or_else(|| default_setup(&pins));
        let rates = noise_rates(self.sch.components[i].custom.as_ref().map(|c| c.name.as_str()).unwrap_or(""));
        let name = |n: &str| pins.iter().find(|(p, _)| p == n).map(|x| x.1.clone()).unwrap_or_else(|| "—".into());
        egui::CollapsingHeader::new(i18n::tr("ADC の読み")).id_salt(("adc_read", i)).default_open(true).show(ui, |ui| {
            ui.small(i18n::tr("どの入力を、どの基準電圧と利得で変換するかはレジスタの設定です。ここで選ぶと、DC 動作点の電圧から、データシートの理想の式でコードを出します"));
            egui::Grid::new(("adc_setup", i)).num_columns(2).show(ui, |ui| {
                for (label, slot, tip) in [
                    ("入力 +", &mut setup.pos, "変換する入力の + 側 (AINP)"),
                    ("入力 −", &mut setup.neg, "変換する入力の − 側 (AINN)"),
                    ("基準 +", &mut setup.refp, "基準電圧の + 側"),
                    ("基準 −", &mut setup.refn, "基準電圧の − 側"),
                ] {
                    ui.label(i18n::tr(label));
                    egui::ComboBox::from_id_salt((label, i)).selected_text(name(slot)).show_ui(ui, |ui| {
                        for (p, n) in &pins {
                            ui.selectable_value(slot, p.clone(), format!("{n} ({p})"));
                        }
                    })
                    .response
                    .on_hover_text(i18n::tr(tip));
                    ui.end_row();
                }
                ui.label(i18n::tr("利得"));
                egui::ComboBox::from_id_salt(("adc_gain", i)).selected_text(format!("{}", setup.gain)).show_ui(ui, |ui| {
                    for g in GAINS {
                        ui.selectable_value(&mut setup.gain, g, format!("{g}"));
                    }
                });
                ui.end_row();
                if !rates.is_empty() {
                    ui.label(i18n::tr("データレート"));
                    egui::ComboBox::from_id_salt(("adc_rate", i)).selected_text(format!("{} SPS", setup.rate)).show_ui(ui, |ui| {
                        for r in &rates {
                            ui.selectable_value(&mut setup.rate, *r, format!("{r} SPS"));
                        }
                    })
                    .response
                    .on_hover_text(i18n::tr("1 秒に何回変換するか。遅いほど多くの回数を平均するので、読みの揺れ (雑音) が小さくなります"));
                    ui.end_row();
                    ui.label(i18n::tr("フィルタ"));
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut setup.filter, AdcFilter::Sinc, "sinc").on_hover_text(i18n::tr("ふつうのフィルタ。雑音が小さい"));
                        ui.selectable_value(&mut setup.filter, AdcFilter::LowLatency, i18n::tr("低遅延")).on_hover_text(i18n::tr("入力を切り替えてすぐ読めるフィルタ。同じデータレートなら雑音が少し大きい"));
                        ui.checkbox(&mut setup.chop, i18n::tr("グローバルチョップ")).on_hover_text(i18n::tr("入力を入れ替えた 2 回の変換を平均して、オフセットと雑音を減らす機能"));
                    });
                    ui.end_row();
                }
            });
            let fresh = self.dc.as_ref().is_some_and(|d| d.signature == self.signature());
            match self.dc.as_ref().filter(|_| fresh).map(|d| d.outcome.as_ref()) {
                Some(Ok(dc)) => match read(&self.sch, dc, &refdes, &setup) {
                    Ok(r) => {
                        ui.label(egui::RichText::new(i18n::trf!("コード {} ({} ビット)", r.code, bits)).strong().color(if r.clipped { crate::theme::WARN } else { crate::theme::OK }));
                        ui.label(i18n::trf!("入力 {}V・基準 {}V・1 段 {}V", sim_core::format_value(r.vin), sim_core::format_value(r.vref), sim_core::format_value(r.lsb)));
                        ui.label(i18n::trf!("コードを電圧に戻すと {}V", sim_core::format_value(r.volts)));
                        if let Some(n) = &r.noise {
                            ui.label(i18n::trf!("揺れ {}V rms・{}V p-p (± {} コード)", sim_core::format_value(n.rms), sim_core::format_value(n.pp), format!("{:.0}", n.pp_codes / 2.0)))
                                .on_hover_text(i18n::tr("データシートの雑音の表 (入力を短絡、25 °C) の、このデータレート・フィルタ・利得の値。入力に換算した電圧です"));
                            ui.label(i18n::trf!("実効の分解能 {} ビット・ちらつかない分解能 {} ビット (約 {} 桁)", format!("{:.1}", n.effective_bits), format!("{:.1}", n.noise_free_bits), format!("{:.1}", n.still_digits)))
                                .on_hover_text(i18n::tr("実効の分解能は rms の雑音から、ちらつかない分解能は p-p の雑音から、データシートの式 (1)・(2) で今の基準電圧を使って出したもの。桁は、範囲全体を p-p の揺れで割った数の 10 の何乗か (表示が止まって見える桁数の目安)"));
                        }
                        if r.clipped {
                            ui.colored_label(crate::theme::WARN, i18n::tr("入力が ±基準電圧 / 利得 の範囲を超えて、コードが端に張り付いています。利得を下げるか、入力を分圧します"));
                        }
                    }
                    Err(e) => {
                        ui.colored_label(crate::theme::WARN, i18n::tr_dyn(&e));
                    }
                },
                _ => {
                    ui.small(i18n::tr("「DC動作点を解析」を実行すると、読みが出ます"));
                }
            }
        });
        if self.adc_setups.get(&refdes) != Some(&setup) {
            self.adc_setups.insert(refdes, setup);
        }
    }
}

/// The setups chosen in the window, by the ADC's reference (kept while the editor is open; not saved with the sheet).
pub(super) type AdcSetups = std::collections::BTreeMap<String, AdcSetup>;
