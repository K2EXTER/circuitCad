//! The row of sheet tabs above the drawing: one tab per sheet, "+" for a new one; right-click a tab to rename or delete it.

use super::*;

impl SchEditor {
    /// Show sheet `k` (the selection and anything half-done on the old sheet is put away).
    pub(super) fn show_sheet(&mut self, k: usize) {
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
        ui.horizontal_wrapped(|ui| {
            if count > 1 {
                for k in 0..count {
                    let name = self.sch.sheet_name(k);
                    let tab = ui.selectable_label(k == self.sch.active_sheet, &name).on_hover_text("このシートを開きます。右クリックで名前の変更と削除ができます。同じ名前のネットラベルと電源記号は、シートが違っても同じ配線になります");
                    if tab.clicked() {
                        go = Some(k);
                    }
                    tab.context_menu(|ui| {
                        let mut n = name.clone();
                        if ui.text_edit_singleline(&mut n).on_hover_text("シートの名前").changed() {
                            self.sch.rename_sheet(k, &n);
                        }
                        if ui.button("このシートを削除").on_hover_text("このシートの部品・線・ラベルをすべて消します (Ctrl+Z で戻せます)").clicked() {
                            delete = Some(k);
                            ui.close();
                        }
                    });
                }
            }
            let tip = "新しいシートを足します。大きな回路を、電源・アナログ・制御などに分けて描けます。シートをまたぐ配線は、同じ名前のネットラベル (と電源記号) で表します";
            if ui.small_button(if count > 1 { "＋" } else { "＋ シートを足す" }).on_hover_text(tip).clicked() {
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
        if let Some(k) = go {
            self.show_sheet(k);
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
