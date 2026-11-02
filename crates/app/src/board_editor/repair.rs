//! The repair practice of the board lesson 8: a fault is put into the board on purpose (`pcb_core::breakage`), the learner reads
//! the symptom, looks for the cause with the checks of the editor and mends it. The board is mended when the checks find no more than
//! they found before the fault.

use super::*;
use pcb_core::breakage::{self, Health, Injected};

pub(super) struct RepairState {
    /// The board as it was before the fault (for the next fault, and the standard the checks are compared with).
    original: Board,
    baseline: Health,
    injected: Injected,
    /// What the checks found the last time the learner asked.
    last: Health,
    revealed: bool,
    pub(super) fixed: bool,
    seed: u64,
}

impl BoardEditor {
    /// The seed of the first fault of a practice: it differs from run to run, so that the same board does not break the same way every time.
    fn fresh_seed() -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64)
    }

    /// Put a fault into the board (one undo step). `seed` picks the kind and the place.
    pub fn repair_inject(&mut self, seed: u64) {
        let original = self.board.clone();
        let faults = breakage::possible(&original);
        if faults.is_empty() {
            self.status = "この基板には、仕込める故障がありません (部品と配線のある基板で試します)".into();
            return;
        }
        let fault = faults[(seed % faults.len() as u64) as usize];
        self.checkpoint();
        match breakage::inject(&mut self.board, fault, seed) {
            Ok(injected) => {
                let baseline = breakage::health(&original);
                let last = breakage::health(&self.board);
                self.violations.clear();
                self.status = format!("故障を仕込みました。症状: {}", injected.symptom);
                self.repair = Some(RepairState { original, baseline, injected, last, revealed: false, fixed: false, seed });
            }
            Err(e) => {
                self.undo.pop();
                self.status = e;
            }
        }
    }

    /// The checks again: is the board as good as before the fault?
    pub(super) fn repair_check(&mut self) {
        let Some(r) = self.repair.as_mut() else { return };
        r.last = breakage::health(&self.board);
        r.fixed = breakage::repaired(&r.baseline, &r.last);
        self.status = if r.fixed { "直りました! 点検の結果は、故障の前と同じです".into() } else { "まだ、点検に引っかかる所が残っています。もう少し探してみましょう".into() };
    }

    /// Show what was wrong, and where (a mark on the board).
    pub(super) fn repair_reveal(&mut self) {
        let Some(r) = self.repair.as_mut() else { return };
        r.revealed = true;
        self.violations = vec![pcb_core::drc::Violation { message: r.injected.cause.clone(), at: r.injected.at }];
        self.status = format!("答え: {}", r.injected.cause);
    }

    /// The board as it was before the fault, and a new fault (a different one for a different seed).
    pub(super) fn repair_again(&mut self) {
        let Some(r) = self.repair.take() else { return };
        self.checkpoint();
        self.board = r.original;
        self.violations.clear();
        self.repair_inject(r.seed.wrapping_add(1));
    }

    /// The practice part of the lesson window.
    pub(super) fn repair_ui(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        let Some(r) = self.repair.as_ref() else {
            if ui.button("故障を仕込む").on_hover_text("いまの基板に、故障を 1 つ仕込みます。何が壊れたかは、先に見せません (症状だけが出ます)。元に戻す (Ctrl+Z) で取り消せます").clicked() {
                self.repair_inject(Self::fresh_seed());
            }
            return;
        };
        let (symptom, last, base, fixed, revealed) = (r.injected.symptom.clone(), r.last, r.baseline, r.fixed, r.revealed);
        let cause = r.injected.cause.clone();
        ui.label(egui::RichText::new("症状 (実機で見えること)").strong());
        ui.label(symptom);
        ui.small("点検の結果");
        egui::Grid::new("repair_health").num_columns(3).spacing([14.0, 2.0]).show(ui, |ui| {
            ui.small("");
            ui.small("いま");
            ui.small("故障の前");
            ui.end_row();
            for (name, now, was) in [("DRC の違反", last.drc, base.drc), ("未配線", last.unrouted, base.unrouted), ("基板の外の部品", last.outside, base.outside), ("バイパスが遠い IC", last.bypass, base.bypass)] {
                ui.small(name);
                let text = egui::RichText::new(now.to_string()).small();
                ui.label(if now > was { text.color(crate::theme::WARN) } else { text });
                ui.small(was.to_string());
                ui.end_row();
            }
        });
        if fixed {
            ui.colored_label(crate::theme::OK, "直りました! 点検の結果は、故障の前と同じです");
        }
        if revealed || fixed {
            ui.label(egui::RichText::new("原因と直し方").strong());
            ui.label(cause);
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("直ったか確かめる").on_hover_text("点検 (DRC・未配線・基板の外・バイパス) を、もう一度します。故障の前と同じか、それ以下なら、直ったことになります").clicked() {
                self.repair_check();
            }
            if !revealed && ui.button("答えを見る").on_hover_text("原因と場所を出します。分からないときに。見ても、直す練習にはなります").clicked() {
                self.repair_reveal();
            }
            if ui.button("別の故障で練習").on_hover_text("基板を故障の前に戻して、別の故障を仕込みます").clicked() {
                self.repair_again();
            }
        });
    }

    /// The names of the faults this board can have (for the status line and the tests).
    #[cfg(test)]
    pub(super) fn repair_faults(&self) -> Vec<pcb_core::breakage::Fault> {
        breakage::possible(&self.board)
    }
}
