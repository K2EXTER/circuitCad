//! New / open / save.

use super::*;

impl SchEditor {
    pub fn new_file(&mut self) {
        *self = SchEditor::default();
    }

    pub fn save(&mut self, as_new: bool) {
        let path = if as_new || self.path.is_none() {
            rfd::FileDialog::new().add_filter("circuitCad 回路図", &["schjson"]).set_file_name("circuit.schjson").save_file()
        } else {
            self.path.clone()
        };
        if let Some(p) = path {
            self.status = match serde_json::to_string_pretty(&self.sch).map(|s| std::fs::write(&p, s)) {
                Ok(Ok(())) => {
                    self.path = Some(p.clone());
                    self.mark_saved();
                    format!("保存しました: {}", p.display())
                }
                Ok(Err(e)) => format!("保存失敗: {e}"),
                Err(e) => format!("保存失敗: {e}"),
            };
        }
    }

    pub fn open(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter("circuitCad 回路図", &["schjson"]).pick_file() {
            self.load_path(p);
        }
    }

    pub fn load_path(&mut self, p: PathBuf) {
        match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string())) {
            Ok(s) => {
                self.adopt(s, Some(p.clone()));
                self.status = format!("開きました: {}", p.display());
            }
            Err(e) => self.status = format!("読み込み失敗: {e}"),
        }
    }

    /// Make `s` the sheet being edited (from a file, or from a project; `path` is None for a project).
    pub fn adopt(&mut self, s: Schematic, path: Option<PathBuf>) {
        self.sch = s;
        self.reset_history();
        self.path = path;
        self.selected = None;
        self.pending = None;
        self.problems.clear();
        self.dc = None;
        self.fit_pending = true;
    }
}
