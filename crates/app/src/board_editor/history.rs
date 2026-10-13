//! Undo / redo: a snapshot of the board is taken before each change.

use super::*;

impl BoardEditor {
    /// Remember the current board before a change.
    pub(super) fn checkpoint(&mut self) {
        self.drc_ran = false;
        self.rev += 1;
        self.undo.push(self.board.clone());
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Has the board changed since it was last saved or opened?
    pub fn dirty(&self) -> bool {
        self.rev != self.saved_rev
    }

    /// The board was just written to a file.
    pub fn mark_saved(&mut self) {
        self.saved_rev = self.rev;
    }

    pub fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.board, prev));
            self.after_history("元に戻しました");
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.board, next));
            self.after_history("やり直しました");
        }
    }

    pub(super) fn after_history(&mut self, msg: &str) {
        self.rev += 1;
        self.selected = None;
        self.multi.clear();
        self.pending = None;
        self.poly.clear();
        self.violations.clear();
        self.status = msg.into();
    }
}
