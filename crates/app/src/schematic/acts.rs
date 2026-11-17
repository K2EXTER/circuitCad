//! The operations of `crate::actions` that act on the schematic (from the palette and the keys), and the canvas keys
//! the table gives the schematic: W wire, N net label, Ctrl+A everything.

use super::*;
use crate::actions::Act;

impl SchEditor {
    /// Does the operation; false when it is not one of the schematic's.
    pub fn run_act(&mut self, act: Act) -> bool {
        match act {
            Act::Undo => self.undo(),
            Act::Redo => self.redo(),
            Act::SelectAll => self.select_all(),
            Act::Fit => self.fit_pending = true,
            Act::Rotate => match self.selected {
                Some(Sel::Comp(i)) => self.rotate_comp(i),
                _ => self.rot = (self.rot + 1) % 4,
            },
            Act::Delete => self.delete_selected(),
            Act::Cancel => {
                self.pending = None;
                self.selected = None;
                self.multi.clear();
                self.tool = Tool::Select;
            }
            Act::AddPart => self.open_quick_add(),
            Act::Wire => self.tool = Tool::Wire,
            Act::NetLabel => self.tool = Tool::Label,
            Act::Erc => self.run_erc(),
            Act::Rerun => self.rerun_last(),
            _ => return false,
        }
        true
    }

    /// Why an operation cannot be done now (the palette shows it instead of hiding the row).
    pub fn why_not(&self, act: Act) -> Option<&'static str> {
        let empty = self.sch.components.is_empty();
        match act {
            Act::Rerun | Act::Erc | Act::PowerOn if empty => Some("回路図に部品がありません"),
            Act::SelectAll if empty && self.sch.wires.is_empty() && self.sch.labels.is_empty() => Some("回路図に何もありません"),
            Act::Rotate | Act::Delete if self.selected.is_none() && self.multi.is_empty() && act == Act::Delete => Some("先に、消すものを選んでください"),
            _ => None,
        }
    }

    /// Every part, wire, label and bus of the shown sheet.
    pub(super) fn select_all(&mut self) {
        self.multi = (0..self.sch.components.len())
            .map(Sel::Comp)
            .chain((0..self.sch.wires.len()).map(Sel::Wire))
            .chain((0..self.sch.labels.len()).map(Sel::Label))
            .chain((0..self.sch.buses.len()).map(Sel::Bus))
            .collect();
        self.selected = None;
        self.tool = Tool::Select;
        self.status = format!("{} 個を選びました", self.multi.len());
    }

    pub(super) fn run_erc(&mut self) {
        self.problems = erc(&self.sch);
        self.problems.extend(self.conversion_problems());
        self.erc_focus = None;
        self.erc_ran = true;
        let errors = self.problems.iter().filter(|p| p.severity == Severity::Error).count();
        self.status = format!("ERC: エラー {} 件、注意 {} 件", errors, self.problems.len() - errors);
    }

    /// The canvas keys of the table that the older handler does not read (it reads R, F, Del, A, Esc, Ctrl+Z/Y/C/X/V/D).
    pub(super) fn table_keys(&mut self, ui: &egui::Ui) {
        let (cmd, alt) = ui.input(|i| (i.modifiers.command, i.modifiers.alt));
        if cmd && ui.input(|i| i.key_pressed(egui::Key::A)) {
            self.select_all();
        }
        if cmd || alt {
            return;
        }
        if ui.input(|i| i.key_pressed(egui::Key::W)) {
            self.run_act(Act::Wire);
        }
        if ui.input(|i| i.key_pressed(egui::Key::N)) {
            self.run_act(Act::NetLabel);
        }
    }

    /// The parts (refdes and value) and the named nets, for the palette's search (Ctrl+F).
    pub fn find_targets(&self) -> (Vec<(String, String)>, Vec<String>) {
        let parts = self.sch.components.iter().filter(|c| !c.kind.is_power_symbol()).map(|c| (c.refdes.clone(), c.value.clone())).collect();
        let mut nets: Vec<String> = self.sch.labels.iter().map(|l| l.text.clone()).chain(self.sch.components.iter().filter(|c| c.kind == Kind::Power).map(|c| c.value.clone())).collect();
        nets.sort();
        nets.dedup();
        (parts, nets)
    }

    pub fn goto_part_pub(&mut self, refdes: &str) {
        self.goto_part(refdes);
    }

    pub fn goto_net_pub(&mut self, name: &str) {
        self.goto_net(name);
    }
}
