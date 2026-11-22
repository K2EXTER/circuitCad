//! The power inductor's lineup picker: the open data sheet's table of one row per part number (inductance, DCR, the
//! rated and the saturation currents, the SRF: `datasheet::lineup::inductor`), a row put into the coil's fields, or the
//! checked rows made at once with a bundled series (`part_lib::coil`, Bourns SRN6045) and handed to the editor to save
//! (`PartForm::batch`). The capacitor's picker is `elcap_lineup.rs`.

use super::*;
use part_lib::coil;
use std::collections::BTreeSet;

#[derive(Default)]
pub struct CoilLineupInput {
    /// The rows read (empty until 「表を読む」).
    pub rows: Vec<coil::Row>,
    /// The bundled series the part numbers belong to.
    pub series: Option<coil::Series>,
    /// Shows only rows with this in the part number or the inductance.
    pub filter: String,
    pub picked: BTreeSet<String>,
    pub message: Option<(bool, String)>,
}

fn row_of(r: &datasheet::lineup::InductorRow) -> coil::Row {
    coil::Row { part: r.part.clone(), l_uh: r.l_uh, tol_pct: r.tol_pct, dcr_ohm: r.dcr_ohm, dcr_max: r.dcr_max, i_rms: r.i_rms, i_sat: r.i_sat, srf_hz: r.srf_hz }
}

fn shown(v: f64) -> String {
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn opt(v: Option<f64>, unit: &str) -> String {
    v.map_or("-".into(), |v| format!("{} {unit}", shown(v)))
}

impl PartForm {
    /// The inductor table of the open PDF read into the picker.
    pub(super) fn coil_lineup_read(&mut self) {
        let Some(ds) = self.sheet.datasheet() else {
            self.coil_lineup.message = Some((false, "データシート (PDF) を開いてください".into()));
            return;
        };
        let rows: Vec<coil::Row> = ds.inductor_rows().iter().map(row_of).collect();
        let l = &mut self.coil_lineup;
        l.picked.clear();
        l.series = rows.iter().find_map(|r| coil::Series::for_part(&r.part));
        l.message = Some(if rows.is_empty() {
            (false, "型番ごとの行の表 (型番とインダクタンスの列) が見つかりませんでした。文字の無い PDF や、文字を抜き出すと列が崩れる PDF は読めません。値は上の欄に手で入れてください".into())
        } else {
            let mut m = format!("{} 行を読みました", rows.len());
            match &l.series {
                Some(s) => m += &format!("。{} {} の共通の表 (本体・推奨ランド・飽和の定義) があるので、チェックした型番をまとめて作れます", s.maker, s.series),
                None => m += "。このシリーズの共通の表 (本体・推奨ランド) は同梱に無いので、1 行ずつフォームに入れて作ります",
            }
            (true, m)
        });
        l.rows = rows;
    }

    /// One row into the coil's fields: the name, L, DCR, Isat (with its 30 % drop), SRF and the rated current.
    pub(super) fn coil_lineup_fill(&mut self, part: &str) {
        let Some(r) = self.coil_lineup.rows.iter().find(|r| r.part == part).cloned() else { return };
        let series = self.coil_lineup.series.clone();
        self.name = r.part.clone();
        self.pas_value = passive::eng(r.l_uh * 1e-6);
        self.pas_dcr = r.dcr_ohm.map(passive::eng).unwrap_or_default();
        self.pas_isat = r.i_sat.map(passive::eng).unwrap_or_default();
        self.pas_ldrop = series.as_ref().filter(|s| s.sat_drop > 0.0).map_or("30".into(), |s| shown(s.sat_drop * 100.0));
        self.pas_srf = r.srf_hz.map(passive::eng).unwrap_or_default();
        if let Some(i) = r.i_rms {
            self.rt_i = shown(i);
        }
        if let Some(s) = &series {
            self.height = s.height;
        }
        self.coil_lineup.message = Some((
            true,
            format!(
                "{} をフォームに入れました (L {} µH・DCR {}・Isat {}・定格電流 {})。「ピンとモデルを作る」のあと、パッケージ (データシートの推奨ランド) を入れて保存します",
                r.part,
                shown(r.l_uh),
                opt(r.dcr_ohm.map(|d| d * 1000.0), "mΩ"),
                opt(r.i_sat, "A"),
                opt(r.i_rms, "A")
            ),
        ));
    }

    /// The checked rows made with the series' table and handed to the editor to save.
    pub(super) fn coil_lineup_make(&mut self) {
        let l = &mut self.coil_lineup;
        let Some(s) = &l.series else { return };
        let (mut parts, mut errors) = (vec![], vec![]);
        for r in l.rows.iter().filter(|r| l.picked.contains(&r.part)) {
            match coil::make(s, r) {
                Ok(p) => parts.push(p),
                Err(e) => errors.push(e),
            }
        }
        let mut m = format!("{} 個を作ってライブラリに保存します: {}", parts.len(), parts.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join("、"));
        if !errors.is_empty() {
            m += &format!(" (作れなかったもの: {})", errors.join(" / "));
        }
        l.message = Some((errors.is_empty() && !parts.is_empty(), m));
        self.batch = parts;
    }

    /// The picker read and the rows checked (`--form-lineup [PN,PN]` with a coil, for screenshots and the tests).
    pub fn coil_lineup_read_and_pick(&mut self, parts: &[&str]) {
        self.coil_lineup_read();
        for p in parts {
            if self.coil_lineup.rows.iter().any(|r| r.part == *p) {
                self.coil_lineup.picked.insert(p.to_string());
            }
        }
    }

    /// The section under the coil's fields, while a data sheet is open.
    pub(super) fn coil_lineup_ui(&mut self, ui: &mut egui::Ui) {
        if !self.sheet.has_doc() {
            return;
        }
        egui::CollapsingHeader::new(i18n::tr("型番の一覧の表から作る (データシート)")).id_salt("coil_lineup").default_open(!self.coil_lineup.rows.is_empty()).show(ui, |ui| {
            crate::icons::help_small(
                ui,
                "パワーインダクタのデータシートの「1 行 = 1 型番」の表を読みます",
                "パワーインダクタのデータシートには、型番・インダクタンス・DCR・定格電流 (発熱)・飽和電流・自己共振周波数を 1 行に並べた表があります。表を読むと型番の一覧が出ます。1 行をフォームに入れるか、本体と推奨ランドの表が同梱されているシリーズ (いまは Bourns SRN6045) なら、チェックした型番をまとめてライブラリに作れます",
            );
            if ui.button(i18n::tr("表を読む")).on_hover_text(i18n::tr("開いている PDF から、型番ごとの行の表を探して読みます")).clicked() {
                self.coil_lineup_read();
            }
            if let Some((ok, m)) = &self.coil_lineup.message {
                ui.colored_label(if *ok { crate::theme::ok() } else { crate::theme::warn() }, egui::RichText::new(m).small());
            }
            if self.coil_lineup.rows.is_empty() {
                return;
            }
            ui.horizontal(|ui| {
                ui.label(i18n::tr("絞り込み")).on_hover_text(i18n::tr("型番かインダクタンスの一部 (例: 100M、4.7) で行を絞ります"));
                ui.add(egui::TextEdit::singleline(&mut self.coil_lineup.filter).desired_width(100.0).hint_text("10")).on_hover_text(i18n::tr("型番かインダクタンスの一部 (例: 100M、4.7) で行を絞ります"));
            });
            let filter = self.coil_lineup.filter.trim().to_ascii_uppercase();
            let mut fill = None;
            let has_series = self.coil_lineup.series.is_some();
            egui::ScrollArea::vertical().id_salt("coil_lineup_rows").max_height(220.0).show(ui, |ui| {
                egui::Grid::new("coil_lineup_grid").striped(true).num_columns(8).show(ui, |ui| {
                    for h in ["", "型番", "L", "DCR", "定格電流", "飽和電流", "SRF", ""] {
                        ui.label(egui::RichText::new(i18n::tr(h)).small().strong());
                    }
                    ui.end_row();
                    let l = &mut self.coil_lineup;
                    for r in &l.rows {
                        let words = format!("{} {}", r.part, shown(r.l_uh));
                        if !filter.is_empty() && !words.to_ascii_uppercase().contains(&filter) {
                            continue;
                        }
                        let mut on = l.picked.contains(&r.part);
                        if crate::widgets::unless(ui, (!has_series).then_some("先にシリーズの共通の表を読んでください"), egui::Checkbox::without_text(&mut on)).on_hover_text(i18n::tr("まとめて作る型番に入れます")).changed() {
                            if on {
                                l.picked.insert(r.part.clone());
                            } else {
                                l.picked.remove(&r.part);
                            }
                        }
                        ui.label(&r.part);
                        ui.label(format!("{} µH", shown(r.l_uh)));
                        ui.label(opt(r.dcr_ohm.map(|d| d * 1000.0), "mΩ"));
                        ui.label(opt(r.i_rms, "A"));
                        ui.label(opt(r.i_sat, "A"));
                        ui.label(opt(r.srf_hz.map(|f| f / 1e6), "MHz"));
                        if ui.small_button(i18n::tr("フォームへ")).on_hover_text(i18n::tr("この行の型番・インダクタンス・DCR・飽和電流・自己共振周波数・定格電流を、欄に入れます")).clicked() {
                            fill = Some(r.part.clone());
                        }
                        ui.end_row();
                    }
                });
            });
            if let Some(p) = fill {
                self.coil_lineup_fill(&p);
            }
            if has_series {
                let n = self.coil_lineup.picked.len();
                let button = crate::widgets::unless(ui, (!(n > 0)).then_some("まとめて作る型番にチェックを入れてください"), egui::Button::new(i18n::trf!("チェックした型番をまとめて作る ({n} 個)")));
                if button
                    .on_hover_text(i18n::tr("チェックした型番を、表の値とシリーズの共通の表 (本体の大きさ・推奨ランド・飽和の定義) から部品にして、ライブラリに保存します。モデルは DCR + インダクタンス (飽和つき) + 巻線の容量です"))
                    .clicked()
                {
                    self.coil_lineup_make();
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bourns SRN6045 (skipped without SRN6045.pdf in microohm-meter/): its rows read, one into the coil's fields, two
    /// made at once like the bundled parts.
    #[test]
    fn srn6045_rows_into_the_form_and_made_at_once() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../microohm-meter/SRN6045.pdf");
        if !path.exists() {
            return;
        }
        let mut f = PartForm::default();
        f.sheet.load(&path);
        f.coil_lineup_read_and_pick(&["SRN6045-100M", "SRN6045-4R7Y"]);
        assert_eq!(f.coil_lineup.rows.len(), 16);
        assert!(f.coil_lineup.series.is_some());
        f.coil_lineup_fill("SRN6045-100M");
        assert_eq!((f.name.as_str(), f.pas_value.as_str(), f.pas_isat.as_str(), f.pas_ldrop.as_str(), f.rt_i.as_str(), f.height), ("SRN6045-100M", "10u", "3", "30", "2.5", 4.5));
        f.coil_lineup_make();
        assert_eq!(f.batch.len(), 2, "{:?}", f.coil_lineup.message);
        let made = f.batch.iter().find(|p| p.name == "SRN6045-100M").unwrap();
        let bundled = part_lib::EXAMPLES.iter().find(|e| e.0 == "SRN6045-100M").unwrap().1;
        assert_eq!(part_lib::coil::to_text(made), bundled.replace("\r\n", "\n"));
    }
}
