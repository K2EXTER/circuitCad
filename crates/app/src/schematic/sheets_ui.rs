//! The row of sheet tabs above the drawing: one tab per sheet, "+" for a new one; right-click a tab to rename or delete it.

use super::*;

impl SchEditor {
    /// Show sheet `k` (the selection and anything half-done on the old sheet is put away).
    pub(crate) fn show_sheet(&mut self, k: usize) {
        if k == self.sch.active_sheet || !self.sch.switch_sheet(k) {
            return;
        }
        self.selected = None;
        self.multi.clear();
        self.pending = None;
        self.drag = None;
        self.gdrag = None;
        self.stretch = None;
        self.boxsel = None;
        self.part_popup = None;
        self.fit_pending = true;
    }

    /// The ERC list shown is made again after the sheets changed (a deleted sheet would leave findings that point nowhere).
    fn refresh_erc(&mut self) {
        if self.erc_ran {
            self.problems = erc(&self.sch);
            self.problems.extend(self.conversion_problems());
            self.erc_focus = None;
        }
    }

    pub(super) fn sheet_tabs(&mut self, ui: &mut egui::Ui) {
        let count = self.sch.sheet_count();
        let mut go = None;
        let mut delete = None;
        let mut add = false;
        let mut back = false;
        let mut place_symbol = None;
        ui.horizontal_wrapped(|ui| {
            if let Some(&parent) = self.sheet_trail.last() {
                let tip = "この階層の記号をダブルクリックして開いたシートから、もとのシートへ戻ります";
                if ui.button(i18n::trf!("← 親へ戻る ({})", self.sch.sheet_name(parent))).on_hover_text(tip).clicked() {
                    back = true;
                }
            }
            if count > 1 {
                let children = self.sch.child_sheets();
                let parents = self.sch.parent_sheets();
                let instances = self.sch.instances();
                for k in 0..count {
                    let name = self.sch.sheet_name(k);
                    let is_child = children.contains(&k);
                    let symbols = parents.iter().find(|p| p.0 == k).map_or(0, |p| p.1);
                    let mut tip = String::new();
                    if is_child {
                        let copies = instances.iter().filter(|i| i.sheet == k && !i.path.is_empty()).count();
                        tip += &format!("階層の子シートです。どこかのシートの記号として {copies} 個置かれていて、中を直すと全部に効きます。外とのつながりは、シートのポートが記号のピンになって行います。");
                    }
                    if symbols > 0 {
                        tip += &format!("階層の親シートです。ほかのシートの記号を {symbols} 個置いています (記号をダブルクリックで中のシートへ)。");
                    }
                    if tip.is_empty() {
                        tip = "ふつうのシートです (階層の親でも子でもありません)。同じ名前のネットラベルと電源記号は、シートが違っても同じ配線になります。".to_string();
                    }
                    tip += "右クリックで名前の変更と削除ができます";
                    let label = match (is_child, symbols > 0) {
                        (true, true) => egui::RichText::new(i18n::trf!("{name} (親・子)")).color(egui::Color32::from_rgb(150, 130, 220)),
                        (true, false) => egui::RichText::new(i18n::trf!("{name} (子)")).color(egui::Color32::from_rgb(90, 150, 220)),
                        (false, true) => egui::RichText::new(i18n::trf!("{name} (親)")).color(egui::Color32::from_rgb(220, 150, 70)),
                        _ => egui::RichText::new(name.clone()),
                    };
                    let tab = ui.selectable_label(k == self.sch.active_sheet, label).on_hover_text(tip);
                    if tab.clicked() {
                        go = Some(k);
                    }
                    let can_place = k != self.sch.active_sheet && count > 1;
                    tab.context_menu(|ui| {
                        let mut n = name.clone();
                        if ui.text_edit_singleline(&mut n).on_hover_text(i18n::tr("シートの名前")).changed() {
                            self.sch.rename_sheet(k, &n);
                        }
                        if can_place && ui.button(i18n::tr("いまのシートに、記号として置く")).on_hover_text(i18n::tr("このシートを、1つの部品 (記号) として、いまのシートに置きます。シートの中のポートが、記号のピンになります。同じシートを何回も置けて、中を直すと全部に効きます。部品番号は R1_CH1 のようになります")).clicked() {
                            place_symbol = Some(k);
                            ui.close();
                        }
                        if ui.button(i18n::tr("このシートを削除")).on_hover_text(i18n::tr("このシートの部品・線・ラベルをすべて消します (Ctrl+Z で戻せます)")).clicked() {
                            delete = Some(k);
                            ui.close();
                        }
                    });
                }
            }
            let tip = "新しいシートを足します。大きな回路を、電源・アナログ・制御などに分けて描けます。シートをまたぐ配線は、同じ名前のネットラベル (と電源記号) で表します";
            if ui.small_button(if count > 1 { i18n::tr("＋") } else { i18n::tr("＋ シートを足す") }).on_hover_text(tip).clicked() {
                add = true;
            }
        });
        if let Some(k) = delete {
            self.sch.remove_sheet(k);
            self.selected = None;
            self.multi.clear();
            self.fit_pending = true;
            self.refresh_erc();
        }
        if add {
            self.sch.add_sheet();
            self.selected = None;
            self.multi.clear();
            self.pending = None;
            self.fit_pending = true;
            self.refresh_erc();
        }
        if back {
            self.leave_sheet();
            self.refresh_erc();
        }
        if let Some(k) = place_symbol {
            self.place_sheet_symbol(k);
        }
        if let Some(k) = go {
            self.show_sheet(k);
            self.sheet_trail.clear();
            self.refresh_erc();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_sheets_drops_the_selection() {
        let mut e = SchEditor::default();
        e.sch = Schematic::default();
        e.sch.add(Kind::Resistor, (10, 10), 0);
        e.sch.add_sheet();
        e.sch.add(Kind::Resistor, (10, 10), 0);
        e.set_selection(vec![Sel::Comp(0)]);
        e.show_sheet(0);
        assert!(e.selected.is_none() && e.multi.is_empty());
        assert_eq!(e.sch.components[0].refdes, "R1");
    }

    #[test]
    fn the_erc_list_follows_the_sheets() {
        let mut e = SchEditor::default();
        e.sch = Schematic::default();
        e.sch.add(Kind::Resistor, (10, 10), 0);
        e.sch.add_sheet();
        e.erc_ran = true;
        e.problems = vec![];
        e.refresh_erc();
        assert!(e.problems.iter().any(|p| p.sheet == 0), "the dangling part of the first sheet is listed from the second");
        e.sch.remove_sheet(0);
        e.refresh_erc();
        assert!(e.problems.iter().all(|p| p.sheet == 0));
    }
}
