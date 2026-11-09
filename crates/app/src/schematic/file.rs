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

    /// The sheet as the text of the autosave, when it has changes that are not saved.
    pub fn autosave_json(&self) -> Option<String> {
        if !self.dirty() {
            return None;
        }
        let mut value = serde_json::to_value(&self.sch).ok()?;
        with_specs(&mut value, &self.spec_items());
        with_bench(&mut value, &self.bench);
        with_settings(&mut value, &self.sheet_settings());
        serde_json::to_string(&value).ok()
    }

    /// Take the sheet back from an autosave. It counts as changed (it was never saved), and keeps the name of the file it came from.
    pub fn restore(&mut self, autosave: &std::path::Path, original: Option<PathBuf>) {
        self.load_path(autosave.to_path_buf());
        self.path = original;
        self.rev += 1;
        if self.status.starts_with("開きました") {
            self.status = "保存前の回路図を復元しました。保存してください".into();
        }
    }

    /// The sheet and its spec table as the text of a `.schjson` file.
    pub(super) fn sheet_json(&self) -> Result<String, serde_json::Error> {
        let mut value = serde_json::to_value(&self.sch)?;
        with_specs(&mut value, &self.spec_items());
        with_bench(&mut value, &self.bench);
        with_settings(&mut value, &self.sheet_settings());
        serde_json::to_string_pretty(&value)
    }

    pub fn open(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter("circuitCad 回路図", &["schjson"]).pick_file() {
            self.load_path(p);
        }
    }

    /// Read a KiCad schematic (`.kicad_sch`): the parts are laid out again and joined by net names. What could not be carried over is
    /// told in a dialog (`with_dialog`) or in the status line only.
    pub fn open_kicad_sch(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter("KiCad の回路図 (.kicad_sch)", &["kicad_sch"]).pick_file() {
            self.load_kicad_sch(p, true);
        }
    }

    /// Read an LTspice schematic (`.asc`): the parts are laid out again and joined by net names, and its `.tran` / `.ac` fill the
    /// analysis boxes.
    pub fn open_ltspice(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter("LTspice の回路図 (.asc)", &["asc"]).pick_file() {
            self.load_ltspice(p, true);
        }
    }

    pub fn load_ltspice(&mut self, p: PathBuf, with_dialog: bool) {
        match part_lib::ltspice_import::import_file(&p, &self.library) {
            Ok(imp) => {
                self.adopt(imp.sch, None);
                let an = part_lib::ltspice_import::analyses(&imp.directives);
                if let Some((stop, step, uic)) = an.tran {
                    self.tran_tstop = stop;
                    self.tran_tstep = step;
                    self.tran_uic = uic;
                }
                if let Some((f0, f1, ppd)) = an.ac {
                    self.set_ac_settings(&f0, &f1, &ppd);
                }
                self.status = format!("LTspice の回路図を取り込みました: 部品 {} 個 (シミュレーションできる部品 {}・記号だけの部品 {})。保存するときは名前を付けます", imp.basic + imp.custom, imp.basic, imp.custom);
                if with_dialog {
                    rfd::MessageDialog::new()
                        .set_title(i18n::tr("LTspice の回路図を取り込みました"))
                        .set_description(imp.notes.iter().map(|n| format!("・{}", i18n::tr_dyn(n))).collect::<Vec<_>>().join("
"))
                        .show();
                }
            }
            Err(e) => self.status = format!("取り込めませんでした: {e}"),
        }
    }

    pub fn load_kicad_sch(&mut self, p: PathBuf, with_dialog: bool) {
        let read = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display())).and_then(|t| part_lib::kicad_sch::parse_schematic(&t));
        match read {
            Ok(k) => {
                let imp = part_lib::kicad_import::import(&k);
                self.adopt(imp.sch, None);
                self.status = format!("KiCad の回路図を取り込みました: 部品 {} 個 (基本の部品 {}・記号から作った部品 {})。保存するときは名前を付けます", imp.basic + imp.custom, imp.basic, imp.custom);
                if with_dialog {
                    rfd::MessageDialog::new()
                        .set_title(i18n::tr("KiCad の回路図を取り込みました"))
                        .set_description(imp.notes.iter().map(|n| format!("・{}", i18n::tr_dyn(n))).collect::<Vec<_>>().join("
"))
                        .show();
                }
            }
            Err(e) => self.status = format!("取り込めませんでした: {e}"),
        }
    }

    pub fn load_path(&mut self, p: PathBuf) {
        let read = |p: &PathBuf| -> Result<(Schematic, Vec<SpecItem>, BenchData, SheetSettings), String> {
            let text = std::fs::read_to_string(p).map_err(|e| e.to_string())?;
            let mut value: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            let specs = take_specs(&mut value);
            let bench = take_bench(&mut value);
            let tol = take_settings(&mut value);
            Ok((serde_json::from_value(value).map_err(|e| e.to_string())?, specs, bench, tol))
        };
        match read(&p) {
            Ok((s, specs, bench, tol)) => {
                self.adopt(s, Some(p.clone()));
                self.set_spec_items(&specs);
                self.set_bench(bench);
                self.set_sheet_settings(tol);
                self.status = format!("開きました: {}", p.display());
            }
            Err(e) => self.status = format!("読み込み失敗: {e}"),
        }
    }

    /// Make `s` the sheet being edited (from a file, or from a project; `path` is None for a project).
    pub fn adopt(&mut self, s: Schematic, path: Option<PathBuf>) {
        self.sch = s;
        self.req_dirty = true;
        self.reset_history();
        self.path = path;
        self.selected = None;
        self.pending = None;
        // an opened circuit is first looked at: the resistor the blank sheet has ready would land on a stray click
        self.tool = Tool::Select;
        self.problems.clear();
        self.specs.clear();
        self.saved_specs.clear();
        self.bench = Default::default();
        self.saved_bench = Default::default();
        // a sheet brings its own tolerances: one that has none is at the defaults
        self.set_sheet_settings(Default::default());
        self.dc = None;
        self.fit_pending = true;
    }
}

#[cfg(test)]
mod kicad_tests {
    use super::*;

    #[test]
    fn a_kicad_sheet_opens_as_a_sheet_that_has_no_file_yet() {
        let mut e = SchEditor::default();
        e.path = Some(PathBuf::from("old.schjson"));
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/kicad-divider.kicad_sch");
        e.load_kicad_sch(p, false);
        assert_eq!(e.sch.components.iter().filter(|c| !c.kind.is_power_symbol()).count(), 3, "{}", e.status);
        assert!(e.path.is_none(), "saving must ask for a name, not overwrite the old file");
        assert!(e.status.contains("取り込みました"), "{}", e.status);
        e.load_kicad_sch(PathBuf::from("no-such.kicad_sch"), false);
        assert!(e.status.contains("取り込めませんでした"), "{}", e.status);
    }

    #[test]
    fn an_ltspice_sheet_opens_with_its_analysis_and_simulates() {
        let mut e = SchEditor::default();
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/ltspice-rc.asc");
        e.load_ltspice(p, false);
        assert_eq!(e.sch.components.iter().filter(|c| !c.kind.is_power_symbol()).count(), 3, "{}", e.status);
        assert!(e.path.is_none());
        // the sheet's .ac fills the AC boxes, and the {R} of .param R=1.5k is worked out
        assert_eq!((e.ac_fstart.as_str(), e.ac_fstop.as_str(), e.ac_ppd.as_str()), ("10", "100k", "20"));
        assert_eq!(e.sch.components.iter().find(|c| c.refdes == "R1").unwrap().value, "1.5k");
        e.run_ac();
        let r = e.ac.as_ref().unwrap().outcome.as_ref().expect("simulates");
        let f = sim_core::minus_3db(&r.freq, &r.signal("V(out)").unwrap().mag()).unwrap();
        assert!((f - 1061.0).abs() < 15.0, "{f}");
        e.load_ltspice(PathBuf::from("no-such.asc"), false);
        assert!(e.status.contains("取り込めませんでした"), "{}", e.status);
    }
}
