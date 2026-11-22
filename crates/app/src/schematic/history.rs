//! Undo / redo: edits are detected by comparing against the last committed state.

use super::*;

impl SchEditor {
    /// Record edits made since the last frame as one undo step. Waits while the mouse button is held
    /// (a drag is one step) or a text box has focus (typing a value is one step).
    pub(super) fn track_history(&mut self, ui: &egui::Ui) {
        if self.sch == self.committed {
            return;
        }
        if ui.input(|i| i.pointer.any_down()) || ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        self.commit_history();
    }

    /// No edit since the last undo step: `rev` names the sheet as it is (a cache keyed by `rev` may be used).
    pub(crate) fn settled(&self) -> bool {
        self.sch == self.committed
    }

    pub(super) fn commit_history(&mut self) {
        if self.sch == self.committed {
            return;
        }
        self.undo.push(std::mem::replace(&mut self.committed, self.sch.clone()));
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.rev += 1;
    }

    pub(super) fn reset_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.committed = self.sch.clone();
        // a sheet that was just opened or replaced counts as saved
        self.rev += 1;
        self.saved_rev = self.rev;
    }

    /// Takes the undo and redo steps out (the board is being parked: they come back with it, `put_history`).
    pub(crate) fn take_history(&mut self) -> (Vec<Schematic>, Vec<Schematic>) {
        self.commit_history();
        (std::mem::take(&mut self.undo), std::mem::take(&mut self.redo))
    }

    /// Gives back the undo and redo steps of a sheet just adopted again (`take_history`).
    pub(crate) fn put_history(&mut self, (undo, redo): (Vec<Schematic>, Vec<Schematic>)) {
        self.undo = undo;
        self.redo = redo;
    }

    #[cfg(test)]
    pub fn commit_history_for_test(&mut self) {
        self.commit_history();
    }

    /// Has the sheet changed since it was last saved or opened?
    pub fn dirty(&self) -> bool {
        self.rev != self.saved_rev || self.sch != self.committed || self.spec_items() != self.saved_specs || self.bench != self.saved_bench || self.sheet_settings() != self.saved_tol
    }

    /// The sheet was just written to a file.
    pub fn mark_saved(&mut self) {
        self.commit_history();
        self.saved_rev = self.rev;
        self.saved_specs = self.spec_items();
        self.saved_bench = self.bench.clone();
        self.saved_tol = self.sheet_settings();
    }

    pub fn undo(&mut self) {
        self.commit_history();
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.sch, prev));
            self.after_history("元に戻しました");
        }
    }

    pub fn redo(&mut self) {
        self.commit_history();
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.sch, next));
            self.after_history("やり直しました");
        }
    }

    pub(super) fn after_history(&mut self, msg: &str) {
        self.rev += 1;
        self.committed = self.sch.clone();
        self.selected = None;
        self.multi.clear();
        self.boxsel = None;
        self.gdrag = None;
        self.drag = None;
        self.pending = None;
        // the findings were for the circuit before; with them gone the ERC has to be run again, not shown as "no problems"
        self.problems.clear();
        self.erc_ran = false;
        self.status = msg.into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Found by a usability test (2026-11-19): after Undo the ERC box said "no problems found" although the sheet had errors.
    #[test]
    fn undo_does_not_leave_a_stale_no_problems() {
        let mut e = SchEditor::default();
        e.sch = Schematic::default();
        e.commit_history_for_test();
        e.sch.add(Kind::Resistor, (20, 20), 0);
        e.commit_history_for_test();
        e.sch.add(Kind::Resistor, (40, 20), 0);
        e.run_erc();
        assert!(e.erc_ran && !e.problems.is_empty(), "two lone resistors have dangling pins");
        e.undo();
        assert!(!e.sch.components.is_empty(), "one resistor is still there");
        assert!(!(e.erc_ran && e.problems.is_empty()), "the panel must not say the sheet has no problems");
    }
}
