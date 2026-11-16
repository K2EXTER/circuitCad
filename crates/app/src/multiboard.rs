//! Several boards in one project (E5, multi-board): which board is in the editors, the others parked aside (`project::System`), and the
//! window of the boards and the links between their connectors: the check that both sides carry the same signals pin by pin
//! (`sch_core::system::check`) and the parts list of the whole system.

use crate::project::Design;
use eframe::egui;
use crate::App;
use sch_core::system::{BoardLink, LinkEnd};
use sch_core::Schematic;

/// The boards window: open or not, the name for a new board, and the link being written (board and connector of each end, pin pairs).
#[derive(Default)]
pub struct BoardsWindow {
    pub open: bool,
    pub new_name: String,
    draft_a: (usize, String),
    draft_b: (usize, String),
    draft_pairs: String,
    message: Option<Result<String, String>>,
}

/// The connectors of a schematic (designators of J, P, CN ... and pin headers), for picking the ends of a link.
fn connectors(s: &Schematic) -> Vec<String> {
    let flat = s.flat();
    let mut v: Vec<String> = flat
        .components
        .iter()
        .filter(|c| !c.kind.is_power_symbol() && !c.sim_only)
        .filter(|c| matches!(c.kind, sch_core::Kind::Header(_)) || ["J", "P", "CN", "X", "K"].iter().any(|p| c.refdes.starts_with(p) && c.refdes[p.len()..].chars().next().is_some_and(|ch| ch.is_ascii_digit())))
        .map(|c| c.refdes.clone())
        .collect();
    v.sort();
    v.dedup();
    v
}

/// Pin pairs written as "1-1 2-2 3-4" (or with ':' or '='): empty = the same numbers.
fn parse_pairs(text: &str) -> Result<Vec<(String, String)>, String> {
    text.split([' ', ',', ';', '\n'])
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .map(|w| match w.split_once(['-', ':', '=']) {
            Some((a, b)) if !a.trim().is_empty() && !b.trim().is_empty() => Ok((a.trim().to_string(), b.trim().to_string())),
            _ => Err(i18n::trf!("ピンの組「{w}」が読めません。「1-1 2-2 3-4」のように、こちらのピン-相手のピンで書きます")),
        })
        .collect()
}

impl App {
    /// The board in the editors, as a design.
    pub(crate) fn current_design(&self) -> Design {
        Design { schematic: self.sch.sch.clone(), board: self.board_ed.board.clone(), specs: self.sch.spec_items(), bench: self.sch.bench_data().clone(), settings: self.sch.sheet_settings() }
    }

    /// Puts a design into the editors.
    fn adopt_design(&mut self, d: Design) {
        self.sch.adopt(d.schematic, None);
        self.sch.set_spec_items(&d.specs);
        self.sch.set_bench(d.bench);
        self.sch.set_sheet_settings(d.settings);
        self.board_ed.adopt(d.board, None);
    }

    /// The schematic of a board of the project (the one in the editors, or parked).
    pub(crate) fn design_schematic(&self, k: usize) -> Option<&Schematic> {
        if k == self.system.active { Some(&self.sch.sch) } else { self.system.parked.get(k)?.as_ref().map(|d| &d.schematic) }
    }

    /// Changes the board in the editors (the one there is parked with its changes; undo starts afresh).
    pub(crate) fn switch_board(&mut self, k: usize) {
        if k == self.system.active || k >= self.system.names.len() {
            return;
        }
        let Some(next) = self.system.parked[k].take() else { return };
        if self.sch.dirty() || self.board_ed.dirty() {
            self.system_dirty = true;
        }
        let here = self.current_design();
        self.system.parked[self.system.active] = Some(here);
        self.system.active = k;
        self.adopt_design(next);
        let msg = i18n::trf!("基板「{}」に切り替えました", self.system.names[k]);
        self.sch.status = msg.clone();
        self.board_ed.status = msg;
    }

    /// Adds an empty board (schematic and board) to the project and opens it.
    pub(crate) fn add_board(&mut self, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err(i18n::tr("基板の名前を入れてください").into());
        }
        if self.system.names.iter().any(|n| n == name) {
            return Err(i18n::trf!("「{name}」という基板はもうあります"));
        }
        if self.sch.dirty() || self.board_ed.dirty() {
            self.system_dirty = true;
        }
        // the first board gets a name of its own (its board's, else 基板1) the first time a second one is added
        let here = self.current_design();
        self.system.parked[self.system.active] = Some(here);
        self.system.names.push(name.to_string());
        self.system.copies.push(1);
        self.system.parked.push(None);
        self.system.active = self.system.names.len() - 1;
        self.adopt_design(Design { schematic: Schematic::default(), board: pcb_core::Board::new(name, 50.0, 40.0), specs: vec![], bench: Default::default(), settings: Default::default() });
        self.system_dirty = true;
        Ok(())
    }

    /// Takes a board out of the project (not the last one); its links go too.
    pub(crate) fn remove_board(&mut self, k: usize) {
        if self.system.names.len() < 2 || k >= self.system.names.len() {
            return;
        }
        if k == self.system.active {
            self.switch_board(if k == 0 { 1 } else { 0 });
        }
        let name = self.system.names.remove(k);
        self.system.copies.remove(k);
        self.system.parked.remove(k);
        if self.system.active > k {
            self.system.active -= 1;
        }
        self.system.links.retain(|l| l.a.design != name && l.b.design != name);
        self.system_dirty = true;
    }

    /// Renames a board (its links follow).
    fn rename_board(&mut self, k: usize, new: &str) -> Result<(), String> {
        let new = new.trim();
        if new.is_empty() || self.system.names.iter().enumerate().any(|(i, n)| i != k && n == new) {
            return Err(i18n::tr("ほかの基板と違う名前にしてください").into());
        }
        let old = std::mem::replace(&mut self.system.names[k], new.to_string());
        for l in &mut self.system.links {
            for e in [&mut l.a, &mut l.b] {
                if e.design == old {
                    e.design = new.to_string();
                }
            }
        }
        self.system_dirty = true;
        Ok(())
    }

    /// Every board's name and schematic, for the check and the parts list.
    fn designs(&self) -> Vec<(&str, &Schematic)> {
        (0..self.system.names.len()).filter_map(|k| self.design_schematic(k).map(|s| (self.system.names[k].as_str(), s))).collect()
    }

    /// The parts of the whole system as CSV (asks for the file).
    fn save_system_bom(&mut self) {
        let designs = self.designs();
        let rows = sch_core::system::system_bom(&designs, &self.system.copies);
        let names: Vec<&str> = designs.iter().map(|d| d.0).collect();
        let csv = sch_core::system::bom_csv(&rows, &names);
        if let Some(p) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).set_file_name("system_bom.csv").save_file() {
            self.boards.message = Some(std::fs::write(&p, csv).map(|_| i18n::trf!("全体の部品表を保存しました ({} 行): {}", rows.len(), p.display())).map_err(|e| e.to_string()));
        }
    }

    /// Opens the multi-board sample (two boards linked by their headers) with its window.
    pub(crate) fn open_multiboard_sample(&mut self) {
        match crate::project::parse(include_str!("../../../docs/examples/multiboard_i2c.ccproj")) {
            Ok(p) => {
                self.sch.adopt(p.schematic, None);
                self.sch.set_spec_items(&p.specs);
                self.sch.set_bench(p.bench);
                self.sch.set_sheet_settings(p.settings);
                self.board_ed.adopt(p.board, None);
                self.system = p.system;
                self.system_dirty = false;
                self.project = None;
                self.boards.open = true;
                self.mode = crate::Mode::Schematic;
            }
            Err(e) => self.sch.status = i18n::trf!("サンプルを開けませんでした: {e}"),
        }
    }

    /// The window of the boards of the project and the links between them.
    pub(crate) fn boards_window(&mut self, ctx: &egui::Context) {
        if !self.boards.open {
            return;
        }
        let mut open = true;
        let mut switch = None;
        let mut remove = None;
        let mut rename: Option<(usize, String)> = None;
        let mut add = false;
        let mut add_link = false;
        let mut drop_link = None;
        let mut bom = false;
        egui::Window::new(i18n::tr("基板どうしのつなぎ")).open(&mut open).default_pos(crate::widgets::beside_panel()).default_width(560.0).show(ctx, |ui| {
            ui.label(egui::RichText::new(i18n::tr("ひとつのプロジェクトに、いくつかの基板 (回路図と基板の組) を入れて、コネクタどうしのつなぎを調べます。編集できるのは選んでいる 1 枚です (切り替えると、元に戻す履歴はそこから始まります)")).weak());
            ui.separator();
            ui.label(egui::RichText::new(i18n::tr("基板")).strong());
            egui::Grid::new("boards_list").num_columns(4).show(ui, |ui| {
                for k in 0..self.system.names.len() {
                    let here = k == self.system.active;
                    if ui.selectable_label(here, &self.system.names[k]).on_hover_text(i18n::tr("この基板を回路図と基板の画面で開きます")).clicked() && !here {
                        switch = Some(k);
                    }
                    let mut n = self.system.copies[k] as u32;
                    if ui.add(egui::DragValue::new(&mut n).range(1..=10000).prefix(i18n::tr("× ")).suffix(i18n::tr(" 枚"))).on_hover_text(i18n::tr("この基板を何枚作るか (全体の部品表の数に掛けます)")).changed() {
                        self.system.copies[k] = n.max(1) as usize;
                        self.system_dirty = true;
                    }
                    let mut name = self.system.names[k].clone();
                    if ui.add_sized([120.0, 20.0], egui::TextEdit::singleline(&mut name)).on_hover_text(i18n::tr("基板の名前 (つなぎにも使います)")).lost_focus() && name != self.system.names[k] {
                        rename = Some((k, name));
                    }
                    if ui.add_enabled(self.system.names.len() > 1, egui::Button::new(i18n::tr("外す")).small()).on_hover_text(i18n::tr("この基板をプロジェクトから外します (その基板のつなぎも消えます)")).clicked() {
                        remove = Some(k);
                    }
                    ui.end_row();
                }
            });
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.boards.new_name).hint_text(i18n::tr("新しい基板の名前")).desired_width(160.0));
                if ui.button(i18n::tr("基板を足す")).on_hover_text(i18n::tr("空の回路図と基板を、この名前で足して開きます。いまの基板は、変更ごとプロジェクトの中にとっておきます")).clicked() {
                    add = true;
                }
            });
            ui.separator();
            ui.label(egui::RichText::new(i18n::tr("つなぎ (コネクタ ↔ コネクタ)")).strong());
            for (k, l) in self.system.links.iter().enumerate() {
                ui.horizontal(|ui| {
                    let pairs = if l.pairs.is_empty() { i18n::tr("同じ番号どうし").to_string() } else { l.pairs.iter().map(|(a, b)| format!("{a}-{b}")).collect::<Vec<_>>().join(" ") };
                    ui.label(format!("{}.{} ↔ {}.{}  ({pairs})", l.a.design, l.a.refdes, l.b.design, l.b.refdes));
                    if ui.small_button(i18n::tr("消す")).clicked() {
                        drop_link = Some(k);
                    }
                });
            }
            // a new link: the board and the connector of each end (the second end starts on another board)
            let names = self.system.names.clone();
            if names.len() > 1 && self.boards.draft_b.1.is_empty() && self.boards.draft_b.0 == self.boards.draft_a.0 {
                self.boards.draft_b.0 = (self.boards.draft_a.0 + 1) % names.len();
            }
            ui.horizontal_wrapped(|ui| {
                for (side, which) in [("A", 0), ("B", 1)] {
                    let (d, r) = if which == 0 { self.boards.draft_a.clone() } else { self.boards.draft_b.clone() };
                    let d = d.min(names.len().saturating_sub(1));
                    let mut nd = d;
                    egui::ComboBox::from_id_salt(("link_board", which)).selected_text(names.get(d).cloned().unwrap_or_default()).show_ui(ui, |ui| {
                        for (k, n) in names.iter().enumerate() {
                            ui.selectable_value(&mut nd, k, n);
                        }
                    });
                    let list = self.design_schematic(nd).map(connectors).unwrap_or_default();
                    let mut nr = if list.contains(&r) { r.clone() } else { list.first().cloned().unwrap_or_default() };
                    egui::ComboBox::from_id_salt(("link_part", which)).selected_text(if nr.is_empty() { i18n::tr("(コネクタなし)").to_string() } else { nr.clone() }).show_ui(ui, |ui| {
                        for c in &list {
                            ui.selectable_value(&mut nr, c.clone(), c);
                        }
                    });
                    if which == 0 {
                        self.boards.draft_a = (nd, nr);
                        ui.label("↔");
                    } else {
                        self.boards.draft_b = (nd, nr);
                    }
                    let _ = side;
                }
            });
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.boards.draft_pairs).hint_text(i18n::tr("ピンの組 (空なら同じ番号どうし。例: 1-1 2-2 3-4 4-3)")).desired_width(300.0))
                    .on_hover_text(i18n::tr("ケーブルが入れ替わっているときや、相手のコネクタの番号の付け方が逆のときに、こちらのピン-相手のピンを並べます"));
                if ui.button(i18n::tr("つなぎを足す")).on_hover_text(i18n::tr("上の 2 つのコネクタがつながることを登録して、ピンごとに信号を照合します")).clicked() {
                    add_link = true;
                }
            });
            ui.separator();
            // the check, every frame (it is quick: the netlists of the boards)
            let designs = self.designs();
            let findings = sch_core::system::check(&designs, &self.system.links);
            if self.system.links.is_empty() {
                ui.label(egui::RichText::new(i18n::tr("つなぎを足すと、ここに照合の結果が出ます")).weak());
            } else if findings.is_empty() {
                ui.colored_label(crate::theme::OK, i18n::tr("✔ どのつなぎも、両側の信号の名前が合っています"));
            } else {
                for f in &findings {
                    let col = if f.severity == sch_core::Severity::Error { crate::theme::ERROR } else { crate::theme::WARN };
                    ui.colored_label(col, i18n::tr_dyn(&f.message)).on_hover_text(i18n::tr_dyn(&f.hint));
                }
            }
            ui.horizontal(|ui| {
                if ui.button(i18n::tr("全体の部品表 (CSV) を保存…")).on_hover_text(i18n::tr("すべての基板の部品を、種類ごとに数えた表です (基板ごとの部品番号と、作る枚数を掛けた合計)。まとめて買うときに使います")).clicked() {
                    bom = true;
                }
            });
            match &self.boards.message {
                Some(Ok(m)) => {
                    ui.colored_label(crate::theme::OK, m);
                }
                Some(Err(e)) => {
                    ui.colored_label(crate::theme::ERROR, e);
                }
                None => {}
            }
        });
        if let Some(k) = switch {
            self.switch_board(k);
        }
        if let Some((k, n)) = rename {
            self.boards.message = self.rename_board(k, &n).err().map(Err);
        }
        if let Some(k) = remove {
            self.remove_board(k);
        }
        if add {
            let name = std::mem::take(&mut self.boards.new_name);
            self.boards.message = Some(self.add_board(&name).map(|_| i18n::trf!("基板「{name}」を足しました。回路図から描き始めます")));
        }
        if let Some(k) = drop_link {
            self.system.links.remove(k);
            self.system_dirty = true;
        }
        if add_link {
            let ((da, ra), (db, rb)) = (self.boards.draft_a.clone(), self.boards.draft_b.clone());
            self.boards.message = Some(match parse_pairs(&self.boards.draft_pairs) {
                Err(e) => Err(e),
                Ok(_) if ra.is_empty() || rb.is_empty() => Err(i18n::tr("両側のコネクタを選んでください (回路図に J1 などのコネクタが要ります)").into()),
                Ok(_) if da == db && ra == rb => Err(i18n::tr("同じコネクタどうしはつなげません").into()),
                Ok(pairs) => {
                    let end = |d: usize, r: String| LinkEnd { design: self.system.names[d].clone(), refdes: r };
                    self.system.links.push(BoardLink { a: end(da, ra), b: end(db, rb), pairs });
                    self.system_dirty = true;
                    self.boards.draft_pairs.clear();
                    Ok(i18n::tr("つなぎを足しました").into())
                }
            });
        }
        if bom {
            self.save_system_bom();
        }
        if !open {
            self.boards.open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_pairs_are_read() {
        assert_eq!(parse_pairs("").unwrap(), vec![]);
        assert_eq!(parse_pairs("1-1 2:3, 4=4").unwrap(), vec![("1".into(), "1".into()), ("2".into(), "3".into()), ("4".into(), "4".into())]);
        assert!(parse_pairs("1-").is_err());
    }
}
