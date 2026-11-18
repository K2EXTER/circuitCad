//! The electrolytic capacitor's lineup picker: the open data sheet's table of one row per part number
//! (`datasheet::lineup`), a row put into the form's fields, or the checked rows made at once with the series' tables
//! (`part_lib::elcap`, bundled for Nichicon UHE) and handed to the editor to save (`PartForm::batch`).

use super::*;
use part_lib::elcap;
use std::collections::BTreeSet;

#[derive(Default)]
pub struct LineupInput {
    /// The rows read (empty until 「表を読む」).
    pub rows: Vec<elcap::Row>,
    /// The bundled series the part numbers belong to.
    pub series: Option<elcap::Series>,
    /// Shows only rows with this in the part number, capacitance or voltage.
    pub filter: String,
    pub picked: BTreeSet<String>,
    pub message: Option<(bool, String)>,
}

fn row_of(r: &datasheet::lineup::ElcapRow) -> elcap::Row {
    elcap::Row {
        part: r.part.clone(),
        cap_uf: r.cap_uf,
        volts: r.volts.unwrap_or(0.0),
        d_mm: r.d_mm,
        l_mm: r.l_mm,
        impedance: r.impedance.clone(),
        ripple_a: r.ripple_a.unwrap_or(0.0),
        ripple_freq_hz: r.ripple_freq_hz,
        ripple_temp_c: r.ripple_temp_c,
        leak_ua: r.leak_ua,
        page: Some(r.page),
    }
}

fn shown(v: f64) -> String {
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl PartForm {
    /// The lineup table of the open PDF read into the picker.
    pub(super) fn lineup_read(&mut self) {
        let Some(ds) = self.sheet.datasheet() else {
            self.lineup.message = Some((false, "データシート (PDF) を開いてください".into()));
            return;
        };
        let mut rows: Vec<elcap::Row> = ds.electrolytic_rows().iter().map(row_of).collect();
        let l = &mut self.lineup;
        l.picked.clear();
        l.series = rows.iter().find(|r| !r.part.is_empty()).and_then(|r| elcap::Series::for_part(&r.part));
        // a table without part numbers (Rubycon's): a plain name from the values, to be renamed in the form
        elcap::name_rows(l.series.as_ref(), &mut rows);
        l.message = Some(if rows.is_empty() {
            (false, "型番ごとの行の表 (容量・大きさ・型番の列) が見つかりませんでした。文字の無い PDF や、文字を抜き出すと列が崩れる PDF は読めません。値は下の欄に手で入れてください".into())
        } else {
            let pages: BTreeSet<usize> = rows.iter().filter_map(|r| r.page).collect();
            let mut m = format!("{} 行を読みました ({} ページ)", rows.len(), pages.len());
            match &l.series {
                Some(s) => {
                    let bad = elcap::leakage_mismatches(s, &rows);
                    m += &format!("。{} {} の共通の表 (寿命・リード・周波数補正) があるので、チェックした型番をまとめて作れます", s.maker, s.series);
                    if !bad.is_empty() {
                        m += &format!("。漏れ電流が式と合わない行があります (読み違いかもしれません): {}", bad.iter().map(|r| r.part.as_str()).collect::<Vec<_>>().join("、"));
                    }
                }
                None => m += "。このシリーズの共通の表 (寿命・リード・周波数補正) は同梱に無いので、1 行ずつフォームに入れて作ります",
            }
            (true, m)
        });
        l.rows = rows;
    }

    /// One row into the form's fields: the name, the capacitance, the impedance and the ESR from it, the height.
    pub(super) fn lineup_fill(&mut self, part: &str) {
        let Some(r) = self.lineup.rows.iter().find(|r| r.part == part).cloned() else { return };
        self.name = r.part.clone();
        self.pas_value = format!("{}u", shown(r.cap_uf));
        if let Some(&(_, f, z)) = r.impedance.iter().find(|z| (z.0 - 20.0).abs() < 0.5).or(r.impedance.first()) {
            self.pas_z100k = shown(z);
            let xc = 1.0 / (2.0 * std::f64::consts::PI * f * r.cap_uf * 1e-6);
            self.pas_esr = passive::eng(if z > xc { (z * z - xc * xc).sqrt() } else { z });
        }
        self.height = r.l_mm;
        self.lineup.message = Some((
            true,
            format!(
                "{} をフォームに入れました (容量 {} µF・{} V・φ{} × {} mm)。「ピンとモデルを作る」のあと、パッケージ (リードの間隔) と定格 (耐圧・リップル電流) を入れて保存します",
                r.part,
                shown(r.cap_uf),
                shown(r.volts),
                shown(r.d_mm),
                shown(r.l_mm)
            ),
        ));
    }

    /// The checked rows made with the series' tables and handed to the editor to save.
    pub(super) fn lineup_make(&mut self) {
        let l = &mut self.lineup;
        let Some(s) = &l.series else { return };
        let mut parts = vec![];
        let mut errors = vec![];
        for r in l.rows.iter().filter(|r| l.picked.contains(&r.part)) {
            match elcap::make(s, r) {
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

    /// The picker read and the rows checked (`--form-lineup [PN,PN]`, for screenshots and the tests).
    pub fn lineup_read_and_pick(&mut self, parts: &[&str]) {
        self.lineup_read();
        for p in parts {
            if self.lineup.rows.iter().any(|r| r.part == *p) {
                self.lineup.picked.insert(p.to_string());
            }
        }
    }

    /// The section under the electrolytic capacitor's fields, while a data sheet is open.
    pub(super) fn lineup_ui(&mut self, ui: &mut egui::Ui) {
        if !self.sheet.has_doc() {
            return;
        }
        egui::CollapsingHeader::new(i18n::tr("型番の一覧の表から作る (データシート)")).id_salt("elcap_lineup").default_open(!self.lineup.rows.is_empty()).show(ui, |ui| {
            crate::icons::help_small(
                ui,
                "電解コンデンサのデータシートの「1 行 = 1 型番」の表を読みます",
                "電解コンデンサのデータシートには、容量・大きさ・インピーダンス・許容リップル電流・型番を 1 行に並べた表があります (Nichicon の Dimensions など)。表を読むと型番の一覧が出ます。1 行をフォームに入れるか、シリーズの共通の表 (寿命・リードの間隔・リップル電流の周波数補正) が同梱されているシリーズ (いまは Nichicon UHE) なら、チェックした型番をまとめてライブラリに作れます",
            );
            if ui.button(i18n::tr("表を読む")).on_hover_text(i18n::tr("開いている PDF から、型番ごとの行の表を探して読みます")).clicked() {
                self.lineup_read();
            }
            if let Some((ok, m)) = &self.lineup.message {
                ui.colored_label(if *ok { crate::theme::OK } else { crate::theme::WARN }, egui::RichText::new(m).small());
            }
            if self.lineup.rows.is_empty() {
                return;
            }
            ui.horizontal(|ui| {
                ui.label(i18n::tr("絞り込み")).on_hover_text(i18n::tr("型番・容量・電圧の一部 (例: 1C、470、25) で行を絞ります"));
                ui.add(egui::TextEdit::singleline(&mut self.lineup.filter).desired_width(100.0).hint_text("470")).on_hover_text(i18n::tr("型番・容量・電圧の一部 (例: 1C、470、25) で行を絞ります"));
            });
            let filter = self.lineup.filter.trim().to_ascii_uppercase();
            let mut fill = None;
            let has_series = self.lineup.series.is_some();
            egui::ScrollArea::vertical().id_salt("elcap_lineup_rows").max_height(220.0).show(ui, |ui| {
                egui::Grid::new("elcap_lineup_grid").striped(true).num_columns(8).show(ui, |ui| {
                    for h in ["", "型番", "容量", "電圧", "大きさ", "Z (20 ℃)", "リップル", ""] {
                        ui.label(egui::RichText::new(i18n::tr(h)).small().strong());
                    }
                    ui.end_row();
                    let l = &mut self.lineup;
                    for r in &l.rows {
                        let words = format!("{} {} {}", r.part, shown(r.cap_uf), shown(r.volts));
                        if !filter.is_empty() && !words.to_ascii_uppercase().contains(&filter) {
                            continue;
                        }
                        let mut on = l.picked.contains(&r.part);
                        if ui.add_enabled(has_series, egui::Checkbox::without_text(&mut on)).on_hover_text(i18n::tr("まとめて作る型番に入れます")).changed() {
                            if on {
                                l.picked.insert(r.part.clone());
                            } else {
                                l.picked.remove(&r.part);
                            }
                        }
                        ui.label(&r.part);
                        ui.label(format!("{} µF", shown(r.cap_uf)));
                        ui.label(format!("{} V", shown(r.volts)));
                        ui.label(format!("φ{} × {}", shown(r.d_mm), shown(r.l_mm)));
                        let z = r.impedance.iter().find(|z| (z.0 - 20.0).abs() < 0.5).or(r.impedance.first());
                        ui.label(z.map_or("-".into(), |z| format!("{} Ω", shown(z.2))));
                        ui.label(format!("{} mA", shown(r.ripple_a * 1000.0)));
                        if ui.small_button(i18n::tr("フォームへ")).on_hover_text(i18n::tr("この行の型番・容量・インピーダンス (と ESR)・高さを、上の欄に入れます")).clicked() {
                            fill = Some(r.part.clone());
                        }
                        ui.end_row();
                    }
                });
            });
            if let Some(p) = fill {
                self.lineup_fill(&p);
            }
            if has_series {
                let n = self.lineup.picked.len();
                let button = ui.add_enabled(n > 0, egui::Button::new(i18n::trf!("チェックした型番をまとめて作る ({n} 個)")));
                if button
                    .on_hover_text(i18n::tr("チェックした型番を、表の値とシリーズの共通の表 (寿命・リードの間隔と径・リップル電流の周波数補正) から部品にして、ライブラリに保存します。モデルは ESR + 容量、穴とパッドはリード径からの目安です"))
                    .clicked()
                {
                    self.lineup_make();
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uhe_form() -> Option<PartForm> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../microohm-meter/e-uhe.pdf");
        if !path.exists() {
            return None;
        }
        let mut f = PartForm::default();
        f.sheet.load(&path);
        f.sheet.has_doc().then_some(f)
    }

    /// Nichicon UHE's data sheet (skipped without e-uhe.pdf in microohm-meter/): its rows read, one put into the form,
    /// two made at once like the bundled parts.
    #[test]
    fn uhe_rows_into_the_form_and_made_at_once() {
        let Some(mut f) = uhe_form() else { return };
        f.lineup_read_and_pick(&["UHE1C102MPD", "UHE1H471MPD"]);
        assert_eq!(f.lineup.rows.len(), 212);
        assert!(f.lineup.series.is_some());
        f.lineup_fill("UHE1C102MPD");
        assert_eq!((f.name.as_str(), f.pas_value.as_str(), f.pas_z100k.as_str(), f.height), ("UHE1C102MPD", "1000u", "0.046", 20.0));
        f.lineup_make();
        assert_eq!(f.batch.len(), 2, "{:?}", f.lineup.message);
        let bundled = part_lib::EXAMPLES.iter().find(|e| e.0 == "UHE1H471MPD").unwrap().1;
        assert_eq!(part_lib::elcap::to_text(&f.batch[1]), bundled.replace("\r\n", "\n"));
    }
}
