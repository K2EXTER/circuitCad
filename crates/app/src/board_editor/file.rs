//! New / open / save, fabrication export and the import from the schematic.

use super::*;

impl BoardEditor {
    pub fn new_file(&mut self) {
        *self = BoardEditor::default();
    }

    pub fn save(&mut self, as_new: bool) {
        let path = if as_new || self.path.is_none() {
            rfd::FileDialog::new().add_filter("circuitCad 基板", &["pcbjson"]).set_file_name("board.pcbjson").save_file()
        } else {
            self.path.clone()
        };
        if let Some(p) = path {
            self.status = match serde_json::to_string_pretty(&self.board).map(|s| std::fs::write(&p, s)) {
                Ok(Ok(())) => {
                    self.path = Some(p.clone());
                    format!("保存しました: {}", p.display())
                }
                Ok(Err(e)) => format!("保存失敗: {e}"),
                Err(e) => format!("保存失敗: {e}"),
            };
        }
    }

    pub fn open(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter("circuitCad 基板", &["pcbjson"]).pick_file() {
            self.load_path(p);
        }
    }

    pub fn load_path(&mut self, p: PathBuf) {
        match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str::<Board>(&s).map_err(|e| e.to_string())) {
            Ok(b) => {
                let (lo, hi) = b.bounds().unwrap_or((Point::default(), Point::default()));
                self.size_w = hi.x - lo.x;
                self.size_h = hi.y - lo.y;
                self.board = b;
                self.path = Some(p.clone());
                self.selected = None;
                self.multi.clear();
                self.pending = None;
                self.poly.clear();
                self.violations.clear();
                self.undo.clear();
                self.redo.clear();
                self.fit_pending = true;
                self.status = format!("開きました: {}", p.display());
            }
            Err(e) => self.status = format!("読み込み失敗: {e}"),
        }
    }

    pub fn export(&mut self) {
        self.violations = drc::check(&self.board);
        if !self.violations.is_empty() {
            self.status = format!("DRC違反が{}件あります。修正してから書き出してください", self.violations.len());
            return;
        }
        let Some(dir) = rfd::FileDialog::new().set_title("製造ファイルの出力先フォルダ").pick_folder() else { return };
        self.status = match self.write_fab_files(&dir) {
            Ok(n) => {
                self.last_export_dir = Some(dir.clone());
                format!("書き出し完了: {} ({} ファイル + zip)", dir.display(), n)
            }
            Err(e) => format!("書き出し失敗: {e}"),
        };
    }

    /// Gerber + drill files, the zip for the fab, and BOM / placement CSVs.
    pub fn write_fab_files(&self, dir: &std::path::Path) -> Result<usize, String> {
        let mut n = 0;
        for (name, data) in export::fab_files(&self.board).into_iter().chain(assembly::assembly_files(&self.board)) {
            std::fs::write(dir.join(&name), data).map_err(|e| e.to_string())?;
            n += 1;
        }
        let zip = export::zip_bytes(&self.board).map_err(|e| e.to_string())?;
        std::fs::write(dir.join(format!("{}_gerber.zip", self.board.name)), zip).map_err(|e| e.to_string())?;
        Ok(n)
    }

    pub fn import_from_schematic(&mut self, sch_ed: &SchEditor) {
        let sch = &sch_ed.sch;
        let parts: Vec<NetlistPart> = sch
            .components
            .iter()
            .filter(|c| !c.kind.is_power_symbol())
            .map(|c| NetlistPart { refdes: c.refdes.clone(), value: c.value.clone(), footprint: c.footprint_name() })
            .collect();
        let nets: Vec<NetlistNet> = sch_core::extract(sch)
            .into_iter()
            .map(|n| NetlistNet {
                name: n.name,
                // simulation-only pins (sensor stimulus inputs) have no pad
                pins: n
                    .pins
                    .into_iter()
                    .filter(|p| {
                        !sch.components
                            .iter()
                            .find(|c| c.refdes == p.refdes)
                            .and_then(|c| c.custom.as_ref())
                            .is_some_and(|s| s.pins.iter().any(|sp| sp.sim_only && sp.number == p.pin))
                    })
                    .map(|p| (p.refdes, p.pin))
                    .collect(),
            })
            .collect();
        // library parts bring their own footprint
        self.checkpoint();
        for c in sch.components.iter().filter(|c| c.custom.is_some()) {
            if let Some(def) = sch_ed.library.iter().find(|d| d.footprint_name() == c.footprint)
                && let Ok(fp) = def.footprint() {
                    self.board.add_footprint(fp);
                }
        }
        let rep = netlist::import(&mut self.board, &parts, &nets);
        self.selected = None;
        self.multi.clear();
        self.import_notes = rep.warnings.clone();
        self.status = format!("取り込み完了: 追加 {} / 更新 {} / 削除 {} / 警告 {}", rep.added.len(), rep.updated, rep.removed.len(), rep.warnings.len());
    }
}
