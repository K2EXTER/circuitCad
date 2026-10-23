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

    /// [`checkpoint`](Self::checkpoint) once for a whole drag of a value (or a whole click on a check box): a number dragged in a panel
    /// changes every frame, and each of those frames must not be a step of its own. The next step can be taken once the pointer is up.
    pub(super) fn checkpoint_once(&mut self) {
        if !self.drag_edit {
            self.checkpoint();
            self.drag_edit = true;
        }
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
