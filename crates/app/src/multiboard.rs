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
    /// Stacking a board on another: the gap above the first board's top face (0: the height of its connector), and whether the second is
    /// turned over (connectors facing each other); what runs into what between the boards placed (as of the last stacking or look).
    gap: f64,
    over: bool,
    clashes: Option<Vec<(String, String, String, String)>>,
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
        self.refresh_stack();
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
        self.system.places.push(None);
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
        self.system.places.remove(k);
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

    /// The board of a board of the project (the one in the editor, or parked).
    fn design_board(&self, k: usize) -> Option<&pcb_core::Board> {
        if k == self.system.active { Some(&self.board_ed.board) } else { self.system.parked.get(k)?.as_ref().map(|d| &d.board) }
    }

    /// The boards placed in 3D: (name, board, where it stands).
    fn placed_boards(&self) -> Vec<(&str, &pcb_core::Board, pcb_core::stack::Place)> {
        (0..self.system.names.len()).filter_map(|k| Some((self.system.names[k].as_str(), self.design_board(k)?, self.system.places.get(k).copied().flatten()?))).collect()
    }

    /// The other placed boards drawn in the 3D view of the board in the editor (in its frame), and what collides between the boards.
    pub(crate) fn refresh_stack(&mut self) {
        let here = self.system.places.get(self.system.active).copied().flatten();
        let mut others = vec![];
        if let Some(here) = here {
            let back = here.inverse();
            for k in 0..self.system.names.len() {
                if k == self.system.active {
                    continue;
                }
                let (Some(b), Some(pl)) = (self.design_board(k), self.system.places.get(k).copied().flatten()) else { continue };
                match pcb_core::stack::board_mesh(b) {
                    Ok(mesh) => others.push(pcb_core::stack::placed_mesh(&mesh, &back.then(&pl))),
                    Err(e) => self.boards.message = Some(Err(i18n::trf!("基板「{}」の 3D の形を作れません: {e}", self.system.names[k]))),
                }
            }
        }
        self.board_ed.set_view3d_others(others);
        let placed = self.placed_boards();
        self.boards.clashes = (placed.len() > 1).then(|| pcb_core::stack::clashes(&placed));
    }

    /// Stacks the second board of link `k` on the first by their connectors (the first stays where it is, or at the origin).
    fn stack_link(&mut self, k: usize) -> Result<String, String> {
        let l = self.system.links.get(k).ok_or("つなぎがありません")?.clone();
        let find = |n: &str| self.system.names.iter().position(|x| x == n);
        let (Some(ia), Some(ib)) = (find(&l.a.design), find(&l.b.design)) else { return Err(i18n::tr("つなぎの基板がプロジェクトにありません").into()) };
        if ia == ib {
            return Err(i18n::tr("同じ基板のコネクタどうしは重ねられません").into());
        }
        let (Some(a), Some(b)) = (self.design_board(ia), self.design_board(ib)) else { return Err(i18n::tr("基板が見つかりません").into()) };
        let gap = if self.boards.gap > 0.0 { self.boards.gap } else { pcb_core::stack::connector_height(a, &l.a.refdes).unwrap_or(8.5) };
        let mated = pcb_core::stack::mate(a, &l.a.refdes, b, &l.b.refdes, &l.pairs, gap, self.boards.over)?;
        let base = self.system.places[ia].unwrap_or_default();
        self.system.places[ia] = Some(base);
        self.system.places[ib] = Some(base.then(&mated.place));
        self.system_dirty = true;
        self.refresh_stack();
        let msg = i18n::trf!("「{}」を「{}」の {} の上 {gap:.1} mm に重ねました ({} ピン、ずれは最大 {:.2} mm)", l.b.design, l.a.design, l.a.refdes, mated.pins, mated.worst);
        if mated.worst > 0.2 {
            Err(i18n::trf!("{msg}。ピンがそろいません: コネクタの位置かピンの組を確かめてください"))
        } else {
            Ok(msg)
        }
    }

    /// The boards placed in 3D as one STEP (asks for the file).
    fn save_system_step(&mut self) {
        let placed = self.placed_boards();
        if placed.is_empty() {
            self.boards.message = Some(Err(i18n::tr("3D に置いた基板がありません。つなぎの「3D で重ねる」で、まず基板を重ねてください").into()));
            return;
        }
        let name = self.project.as_ref().and_then(|p| p.file_stem()).map_or_else(|| "system".to_string(), |s| s.to_string_lossy().to_string());
        let text = pcb_core::step_out::write_system(&name, &placed);
        let n = placed.len();
        if let Some(p) = rfd::FileDialog::new().add_filter("STEP", &["step", "stp"]).set_file_name(format!("{name}.step")).save_file() {
            self.boards.message = Some(std::fs::write(&p, text).map(|_| i18n::trf!("{n} 枚の基板を 1 つの STEP にしました: {}", p.display())).map_err(|e| e.to_string()));
        }
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
                self.refresh_stack();
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
        let mut stack = None;
        let mut look = false;
        let mut step = false;
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
                    if ui.small_button(i18n::tr("3D で重ねる")).on_hover_text(i18n::tr("2 枚目の基板を、コネクタのピンが 1 枚目のピンの真上に来るように回して、下の「すき間」だけ上に重ねます (シールドのような積み重ね)。3D の窓に、ほかの基板も出ます")).clicked() {
                        stack = Some(k);
                    }
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
            ui.horizontal(|ui| {
                ui.label(i18n::tr("3D で重ねるとき:"));
                ui.add(egui::DragValue::new(&mut self.boards.gap).range(0.0..=100.0).speed(0.1).prefix(i18n::tr("すき間 ")).suffix(" mm"))
                    .on_hover_text(i18n::tr("1 枚目の基板の表の面から、2 枚目の基板までの高さ。0 なら 1 枚目のコネクタの高さ (ピンヘッダにソケットを差す積み重ね)"));
                ui.checkbox(&mut self.boards.over, i18n::tr("向かい合わせ")).on_hover_text(i18n::tr("2 枚目を裏返して、表の面どうしを向かい合わせます (両方の表にあるコネクタを差し合わせるとき)。すき間は 2 枚目の表の面まで"));
            });
            ui.horizontal(|ui| {
                if ui.button(i18n::tr("3D で全体を見る")).on_hover_text(i18n::tr("いま開いている基板の 3D の窓に、重ねたほかの基板も出します (ほかの基板は選べません)")).clicked() {
                    look = true;
                }
                if ui.button(i18n::tr("全体の STEP を保存…")).on_hover_text(i18n::tr("重ねた基板をすべて、それぞれの位置に置いた 1 つの STEP にします (基板ごとに部品が分かれた組み立て)。ケースの CAD で全体を確かめるのに使います")).clicked() {
                    step = true;
                }
            });
            if let Some(c) = &self.boards.clashes {
                if c.is_empty() {
                    ui.colored_label(crate::theme::OK, i18n::tr("✔ 重ねた基板どうしでぶつかる所はありません"));
                } else {
                    let what = |b: &str, p: &str| if p.is_empty() { i18n::trf!("{b} の基板") } else { format!("{b}.{p}") };
                    let list: Vec<String> = c.iter().take(6).map(|(a, pa, b, pb)| format!("{} ↔ {}", what(a, pa), what(b, pb))).collect();
                    let more = if c.len() > 6 { i18n::trf!(" ほか {} 組", c.len() - 6) } else { String::new() };
                    ui.colored_label(crate::theme::ERROR, i18n::trf!("⚠ ぶつかるかもしれません: {}{more}", list.join(", ")))
                        .on_hover_text(i18n::tr("3D で描く箱どうしで調べています。すき間を広げるか、部品の位置を変えます"));
                }
            }
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
        if let Some(k) = stack {
            self.boards.message = Some(self.stack_link(k));
            self.board_ed.open_view3d();
            self.mode = crate::Mode::Board;
        }
        if look {
            self.refresh_stack();
            self.board_ed.open_view3d();
            self.mode = crate::Mode::Board;
        }
        if step {
            self.save_system_step();
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
