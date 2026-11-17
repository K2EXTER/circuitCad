//! The operations of `crate::actions` that act on the board (from the palette): the same names and keys as in the
//! schematic.

use super::*;
use crate::actions::Act;

impl BoardEditor {
    /// Does the operation; false when it is not one of the board's.
    pub fn run_act(&mut self, act: Act) -> bool {
        match act {
            Act::Undo => self.undo(),
            Act::Redo => self.redo(),
            Act::SelectAll => self.select_all(),
            Act::Fit => self.fit_pending = true,
            Act::Cancel => {
                self.pending = None;
                self.selected = None;
                self.multi.clear();
                self.set_tool(Tool::Select);
            }
            Act::Delete => {
                if !self.multi.is_empty() {
                    self.delete_multi();
                } else if let Some(it) = self.selected {
                    self.delete(it);
                }
            }
            Act::Flip => self.flip_selected_part(),
            Act::Wire => self.set_tool(Tool::Track),
            Act::Via => self.set_tool(Tool::Via),
            Act::Measure => self.set_tool(Tool::Measure),
            Act::Layer => self.side = self.next_layer(),
            Act::Drc => self.run_drc(),
            _ => return false,
        }
        true
    }

    /// Why an operation cannot be done now (the palette shows it instead of hiding the row).
    pub fn why_not(&self, act: Act) -> Option<&'static str> {
        match act {
            Act::Drc | Act::SelectAll if self.board.parts.is_empty() => Some("基板に部品がありません。先に「回路図から取り込み」を押してください"),
            Act::Flip if !matches!(self.selected, Some(Item::Part(_))) => Some("先に、裏返す部品を選んでください"),
            Act::Delete if self.selected.is_none() && self.multi.is_empty() => Some("先に、消すものを選んでください"),
            _ => None,
        }
    }

    /// Every part on the board (Ctrl+A), in the Select tool.
    pub(super) fn select_all(&mut self) {
        self.set_tool(Tool::Select);
        self.multi = (0..self.board.parts.len()).map(Item::Part).collect();
        self.selected = None;
        self.status = format!("部品 {} 個を選びました", self.multi.len());
    }
}
