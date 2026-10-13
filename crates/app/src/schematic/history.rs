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

    #[cfg(test)]
    pub fn commit_history_for_test(&mut self) {
        self.commit_history();
    }

    /// Has the sheet changed since it was last saved or opened?
    pub fn dirty(&self) -> bool {
        self.rev != self.saved_rev || self.sch != self.committed
    }

    /// The sheet was just written to a file.
    pub fn mark_saved(&mut self) {
        self.commit_history();
        self.saved_rev = self.rev;
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
        self.problems.clear();
        self.status = msg.into();
    }
}
