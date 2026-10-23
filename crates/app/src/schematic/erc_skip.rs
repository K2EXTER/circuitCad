//! Work-in-progress areas: ERC findings inside them are left out (`Schematic::erc_skip`).

use super::*;

impl SchEditor {
    /// Grid box around what is selected (a margin of 2 steps for the part bodies and pin leads).
    fn selection_box(&self) -> Option<(Pt, Pt)> {
        let mut pts: Vec<Pt> = vec![];
        for s in self.selected.iter().chain(self.multi.iter()) {
            match *s {
                Sel::Comp(i) => pts.extend(self.sch.components.get(i).map(|c| c.pos).into_iter().chain(self.sch.components.get(i).into_iter().flat_map(|c| c.pins().into_iter().map(|p| p.1)))),
                Sel::Wire(i) => pts.extend(self.sch.wires.get(i).into_iter().flat_map(|w| [w.a, w.b])),
                Sel::Label(i) => pts.extend(self.sch.labels.get(i).map(|l| l.pos)),
            }
        }
        let lo = (pts.iter().map(|p| p.0).min()? - 2, pts.iter().map(|p| p.1).min()? - 2);
        let hi = (pts.iter().map(|p| p.0).max()? + 2, pts.iter().map(|p| p.1).max()? + 2);
        Some((lo, hi))
    }

    pub(super) fn erc_skip_ui(&mut self, ui: &mut egui::Ui) {
        let b = self.selection_box();
        ui.horizontal(|ui| {
            let add = ui
                .add_enabled(b.is_some(), egui::Button::new("選択を描きかけにする"))
                .on_hover_text("選んだ部品・配線を囲む範囲を「描きかけ」にして、その中のERCの指摘を出さなくします。作りながら出るエラーの山を減らすための機能です。範囲は灰色の枠で表示され、完成したら下の×で外します。保存もされます");
            if add.clicked()
                && let Some(b) = b
            {
                self.sch.erc_skip.push(b);
                self.problems = erc(&self.sch);
                self.erc_focus = None;
                self.status = "選んだ範囲をERCの対象から外しました".into();
            }
        });
        let mut remove = None;
        for (k, (lo, hi)) in self.sch.erc_skip.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui.small_button("×").on_hover_text("この範囲をERCの対象に戻します").clicked() {
                    remove = Some(k);
                }
                ui.small(format!("描きかけ {} ({},{})–({},{})", k + 1, lo.0, lo.1, hi.0, hi.1));
            });
        }
        if let Some(k) = remove {
            self.sch.erc_skip.remove(k);
            self.problems = erc(&self.sch);
            self.erc_focus = None;
            self.status = "範囲をERCの対象に戻しました。ERCを押して確かめてください".into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn findings_inside_a_skipped_area_disappear() {
        let mut e = SchEditor::default();
        e.sch = Schematic::default();
        e.sch.add(Kind::Resistor, (20, 20), 0);
        let n = erc(&e.sch).iter().filter(|p| p.at.is_some()).count();
        assert!(n > 0, "a lone resistor has dangling pins");
        e.set_selection(vec![Sel::Comp(0)]);
        let b = e.selection_box().unwrap();
        e.sch.erc_skip.push(b);
        assert!(erc(&e.sch).iter().all(|p| p.at.is_none()), "the pin findings are inside the area");
        let json = serde_json::to_string(&e.sch).unwrap();
        let back: Schematic = serde_json::from_str(&json).unwrap();
        assert_eq!(back.erc_skip, e.sch.erc_skip);
    }
}
