//! The panel's list of the design's requirements that the placement does not meet (the groups and distances the schematic's table gave
//! the board on import; see `pcb_core::requirements`), with a button that selects the parts of a group that is too spread out.

use super::*;
use pcb_core::requirements::{check, FindingKind};

impl BoardEditor {
    pub(super) fn requirements_check(&mut self, ui: &mut egui::Ui) {
        if self.board.place_groups.is_empty() && self.board.keep_apart.is_empty() {
            return;
        }
        let rep = check(&self.board);
        let orange = Color32::from_rgb(255, 180, 60);
        if rep.findings.is_empty() {
            ui.label(format!("✔ 設計の要件を満たしています ({} 件を確認)", rep.checked)).on_hover_text("回路図の「設計の要件」の表にある、近くに置く組と、離す組を調べました。上限や下限の0のものは調べません");
        } else {
            ui.colored_label(orange, format!("設計の要件を満たしていない所: {} 件", rep.findings.len())).on_hover_text("回路図の「設計の要件」の表にある、近くに置く組の広がりと、離す組の近さです。部品を動かすか、表の数字を見直してください");
            for f in &rep.findings {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(orange, &f.message);
                    if f.kind == FindingKind::Group && ui.small_button("組を選ぶ").on_hover_text("この組の部品をまとめて選びます。どこに散らばっているかが見えます").clicked() {
                        if let Some(g) = self.board.place_groups.iter().find(|g| g.name == f.name) {
                            self.multi = self.board.parts.iter().enumerate().filter(|(_, p)| g.parts.contains(&p.refdes)).map(|(i, _)| Item::Part(i)).collect();
                            self.selected = None;
                            self.status = format!("組「{}」の部品 {} 個を選びました", g.name, self.multi.len());
                        }
                    }
                });
            }
        }
        if !rep.missing.is_empty() {
            ui.label(format!("基板にない名前: {} 件", rep.missing.len())).on_hover_text(format!("表にあるが、基板に部品・パッドがないもの (回路図から取り込み直すと消えることがあります): {}", rep.missing.join("、")));
        }
    }
}
