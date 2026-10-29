//! The "モデルの前提" section of the analysis tab: what the simulation leaves out or only guesses (`sim_core::model_coverage`).

use super::*;

impl SchEditor {
    /// The coverage of the schematic, worked out again only when the schematic changed.
    pub(super) fn model_coverage(&mut self) -> std::rc::Rc<sim_core::ModelCoverage> {
        if self.coverage_cache.as_ref().is_none_or(|(rev, _)| *rev != self.rev) {
            self.coverage_cache = Some((self.rev, std::rc::Rc::new(sim_core::model_coverage(&self.sch))));
        }
        self.coverage_cache.as_ref().unwrap().1.clone()
    }

    /// For the screenshots of the analysis tab (`--analyze`).
    pub fn show_analysis_tab(&mut self) {
        self.panel_tab = PanelTab::Analyze;
    }

    pub(super) fn panel_coverage(&mut self, ui: &mut egui::Ui) {
        let cov = self.model_coverage();
        ui.label(egui::RichText::new(format!("計算に入る部品 {} 個 (記号・ピンヘッダを除く)", cov.total)).small());
        if cov.is_clean() {
            ui.colored_label(egui::Color32::from_rgb(70, 190, 100), "モデルのない部品も、汎用の値で計算している部品も、注意書きのあるモデルもありません")
                .on_hover_text("抵抗・コンデンサ・コイルは理想の素子 (ESR などは値の欄に書いたぶんだけ) で計算します");
        }
        let orange = egui::Color32::from_rgb(240, 170, 20);
        let section = |ui: &mut egui::Ui, id: &str, title: String, tip: &str, open: bool, color: Option<egui::Color32>, list: &[sim_core::PartNote]| {
            if list.is_empty() {
                return;
            }
            let text = match color {
                Some(c) => egui::RichText::new(title).color(c),
                None => egui::RichText::new(title),
            };
            egui::CollapsingHeader::new(text).id_salt(("coverage", id)).default_open(open).show(ui, |ui| {
                for p in list {
                    ui.label(egui::RichText::new(format!("{} ({})", p.refdes, p.value)).strong());
                    for n in &p.notes {
                        ui.label(egui::RichText::new(format!("・{n}")).small());
                    }
                }
            }).header_response.on_hover_text(tip);
        };
        section(ui, "ignored", format!("計算に入っていない部品 {} 個", cov.ignored.len()), "モデルがないので、回路にないものとして計算します。部品の編集で、モデルのある部品に替えるか、ライブラリにモデルを足してください", true, Some(orange), &cov.ignored);
        section(ui, "generic", format!("汎用の標準値で計算 {} 個", cov.generic.len()), "ダイオード・LED・NPN の基本の部品は、型番の特性ではなく、標準の値で計算します。型番まで合わせたいときは、ライブラリの部品に替えます", false, None, &cov.generic);
        section(ui, "caveats", format!("モデルに注意書きがある部品 {} 個", cov.caveats.len()), "ライブラリのモデルが、何を含まないか・数値の出どころを書いたものです。結果を見るときに、効きそうなものだけ読めば足ります", false, None, &cov.caveats);
        section(ui, "test", format!("シミュレーション専用 {} 個", cov.test_sources.len()), "試験用の電源や、測る相手の代わりに置いた部品です。基板には載りません", false, None, &cov.test_sources);
    }
}
