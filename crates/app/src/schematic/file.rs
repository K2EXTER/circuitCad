//! New / open / save.

use super::*;

impl SchEditor {
    pub fn new_file(&mut self) {
        // `rev` keeps counting up: the board's "differs from the schematic" cache is keyed by it, and a sheet that
        // started again from 0 could look unchanged to it
        let rev = self.rev;
        *self = SchEditor::default();
        self.rev = rev + 1;
        self.saved_rev = self.rev;
    }

    pub fn save(&mut self, as_new: bool) {
        let path = if as_new || self.path.is_none() {
            rfd::FileDialog::new().add_filter("circuitCad 回路図", &["schjson"]).set_file_name("circuit.schjson").save_file()
        } else {
            self.path.clone()
        };
        if let Some(p) = path {
            self.status = match self.sheet_json().map(|s| std::fs::write(&p, s)) {
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

    /// The sheet and its spec table as the text of a `.schjson` file.
    fn sheet_json(&self) -> Result<String, serde_json::Error> {
        let mut value = serde_json::to_value(&self.sch)?;
        with_specs(&mut value, &self.spec_items());
        serde_json::to_string_pretty(&value)
    }

    pub fn open(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter("circuitCad 回路図", &["schjson"]).pick_file() {
            self.load_path(p);
        }
    }

    pub fn load_path(&mut self, p: PathBuf) {
        let read = |p: &PathBuf| -> Result<(Schematic, Vec<SpecItem>), String> {
            let text = std::fs::read_to_string(p).map_err(|e| e.to_string())?;
            let mut value: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            let specs = take_specs(&mut value);
            Ok((serde_json::from_value(value).map_err(|e| e.to_string())?, specs))
        };
        match read(&p) {
            Ok((s, specs)) => {
                self.adopt(s, Some(p.clone()));
                self.set_spec_items(&specs);
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
        // an opened circuit is first looked at: the resistor the blank sheet has ready would land on a stray click
        self.tool = Tool::Select;
        self.problems.clear();
        self.specs.clear();
        self.saved_specs.clear();
        self.dc = None;
        self.fit_pending = true;
    }
}
