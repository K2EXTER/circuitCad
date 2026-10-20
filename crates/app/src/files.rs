use super::*;

/// What the user answered to "there are unsaved changes".
#[derive(PartialEq)]
pub(super) enum Ask {
    Save,
    Discard,
    Cancel,
}

pub(super) fn ask_unsaved(what: &str) -> Ask {
    let r = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Warning)
        .set_title("保存していない変更があります")
        .set_description(format!("{what}\n\nはい: 保存してから続ける\nいいえ: 保存せずに続ける (変更は失われます)\nキャンセル: やめる"))
        .set_buttons(rfd::MessageButtons::YesNoCancel)
        .show();
    match r {
        rfd::MessageDialogResult::Yes => Ask::Save,
        rfd::MessageDialogResult::No => Ask::Discard,
        _ => Ask::Cancel,
    }
}

impl App {
    /// Has the schematic or the board changed since it was last saved or opened?
    pub(super) fn dirty(&self) -> bool {
        self.sch.dirty() || self.board_ed.dirty()
    }

    /// Before the work is thrown away (open, new, close): when something is unsaved, ask. True = go on.
    pub(super) fn confirm_unsaved(&mut self, what: &str) -> bool {
        if !self.dirty() {
            return true;
        }
        match ask_unsaved(&format!("回路図か基板に、保存していない変更があります。{what}")) {
            Ask::Cancel => false,
            Ask::Discard => true,
            Ask::Save => {
                self.save_everything();
                !self.dirty()
            }
        }
    }

    /// Like `confirm_unsaved`, for one editor only ("新規" and "開く" of the schematic or of the board).
    pub(super) fn confirm_unsaved_one(&mut self, schematic: bool, what: &str) -> bool {
        let dirty = if schematic { self.sch.dirty() } else { self.board_ed.dirty() };
        if !dirty {
            return true;
        }
        match ask_unsaved(&format!("{}に、保存していない変更があります。{what}", if schematic { "回路図" } else { "基板" })) {
            Ask::Cancel => false,
            Ask::Discard => true,
            Ask::Save => {
                if schematic { self.sch.save(false) } else { self.board_ed.save(false) }
                !(if schematic { self.sch.dirty() } else { self.board_ed.dirty() })
            }
        }
    }

    /// Save what changed: the project when there is one (or nothing was saved to its own file yet),
    /// else the schematic and the board to their own files.
    pub(super) fn save_everything(&mut self) {
        if self.project.is_some() || (self.sch.path.is_none() && self.board_ed.path.is_none()) {
            self.save_project(false);
        } else {
            if self.sch.dirty() {
                self.sch.save(false);
            }
            if self.board_ed.dirty() {
                self.board_ed.save(false);
            }
        }
    }

    /// Save the schematic and the board together; asks for the file when there is none yet or `as_new`.
    pub(super) fn save_project(&mut self, as_new: bool) {
        let path = if as_new || self.project.is_none() {
            let name = if self.board_ed.board.name.is_empty() { "project".to_string() } else { self.board_ed.board.name.clone() };
            rfd::FileDialog::new().add_filter("circuitCad プロジェクト", &[project::EXTENSION]).set_file_name(format!("{name}.{}", project::EXTENSION)).save_file()
        } else {
            self.project.clone()
        };
        let Some(p) = path else { return };
        let msg = match project::save(&p, &self.sch.sch, &self.board_ed.board, &self.sch.spec_items()) {
            Ok(()) => {
                self.project = Some(p.clone());
                self.sch.mark_saved();
                self.board_ed.mark_saved();
                format!("プロジェクトを保存しました (回路図と基板): {}", p.display())
            }
            Err(e) => format!("プロジェクトを保存できませんでした: {e}。保存先のフォルダに書き込めるか確認してください"),
        };
        self.sch.status = msg.clone();
        self.board_ed.status = msg;
    }

    /// Open a project: both the schematic and the board are replaced.
    pub(super) fn open_project(&mut self) {
        if !self.confirm_unsaved("プロジェクトを開くと、いまの回路図と基板は置き換わります。") {
            return;
        }
        if let Some(p) = rfd::FileDialog::new().add_filter("circuitCad プロジェクト", &[project::EXTENSION]).pick_file() {
            self.load_project(p);
        }
    }

    pub(super) fn load_project(&mut self, p: std::path::PathBuf) {
        let msg = match project::load(&p) {
            Ok(pr) => {
                self.sch.adopt(pr.schematic, None);
                self.sch.set_spec_items(&pr.specs);
                self.board_ed.adopt(pr.board, None);
                self.project = Some(p.clone());
                format!("プロジェクトを開きました: {}", p.display())
            }
            Err(e) => format!("プロジェクトを開けませんでした: {e}"),
        };
        self.sch.status = msg.clone();
        self.board_ed.status = msg;
    }

    /// Opens a board lesson: 0 = schematic to order files (with its sample circuit), 1 = copper width and voltage drop,
    /// 2 = the pre-order check challenge (each with its sample project).
    pub(super) fn start_board_lesson(&mut self, which: usize) {
        self.mode = Mode::Board;
        if which == 1 || which == 2 {
            let text = if which == 1 { board_editor::BoardEditor::copper_lesson_project() } else { board_editor::BoardEditor::fix_lesson_project() };
            match project::parse(text) {
                Ok(pr) => {
                    self.sch.adopt(pr.schematic, None);
                    self.sch.wiring.clear();
                    self.sch.couplings.clear();
                    self.board_ed.adopt(pr.board, None);
                    self.project = None;
                    if which == 1 {
                        self.board_ed.start_copper_lesson();
                    } else {
                        self.board_ed.start_fix_lesson();
                    }
                }
                Err(e) => self.board_ed.status = format!("レッスンのプロジェクトを読み込めませんでした: {e}"),
            }
            return;
        }
        if !self.sch.load_sample(board_editor::BoardEditor::lesson_circuit()) {
            self.board_ed.status = "レッスンの回路を読み込めませんでした".into();
        } else {
            self.board_ed.start_lesson();
        }
    }
}
