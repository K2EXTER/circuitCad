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
                    self.mark_saved();
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
                self.adopt(b, Some(p.clone()));
                self.status = format!("開きました: {}", p.display());
            }
            Err(e) => self.status = format!("読み込み失敗: {e}"),
        }
    }

    /// Make `b` the board being edited (from a file, or from a project; `path` is None for a project).
    pub fn adopt(&mut self, mut b: Board, path: Option<PathBuf>) {
        b.migrate_finish_exclude();
        let (lo, hi) = b.bounds().unwrap_or((Point::default(), Point::default()));
        self.size_w = hi.x - lo.x;
        self.size_h = hi.y - lo.y;
        self.board = b;
        self.path = path;
        self.selected = None;
        self.multi.clear();
        self.pending = None;
        self.poly.clear();
        self.violations.clear();
        self.undo.clear();
        self.redo.clear();
        self.fit_pending = true;
        // a board that was just opened counts as saved
        self.rev += 1;
        self.saved_rev = self.rev;
    }

    pub fn export(&mut self) {
        self.violations = drc::check_final(&self.board);
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

    /// The parts and nets of the schematic as the board import sees them.
    fn netlist_of(sch: &sch_core::Schematic) -> (Vec<NetlistPart>, Vec<NetlistNet>) {
        let sch = &*sch.flat();
        let parts: Vec<NetlistPart> = sch
            .components
            .iter()
            .filter(|c| !c.kind.is_power_symbol() && !c.sim_only)
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
        (parts, nets)
    }

    /// How the schematic differs from the board right now (what "回路図から取り込み" would change); worked out again
    /// only when the schematic or the board changed. Empty when the board has no parts yet (nothing imported) or the schematic is empty.
    pub fn schematic_diff(&mut self, sch_ed: &SchEditor) -> &SchDiff {
        let key = (sch_ed.rev, self.rev);
        if self.sync_cache.as_ref().is_none_or(|(k, _)| *k != key) {
            // nothing to compare when no part was imported yet, or when no schematic is open (a board opened on its own)
            let diff = if self.board.parts.is_empty() || sch_ed.sch.components.is_empty() { SchDiff::default() } else { self.diff_against(&sch_ed.sch) };
            self.sync_cache = Some((key, diff));
        }
        &self.sync_cache.as_ref().unwrap().1
    }

    fn diff_against(&self, sch: &sch_core::Schematic) -> SchDiff {
        let (parts, nets) = Self::netlist_of(sch);
        netlist::diff(&self.board, &parts, &nets)
    }

    pub fn import_from_schematic(&mut self, sch_ed: &SchEditor) {
        let sch = &*sch_ed.sch.flat();
        let (parts, nets) = Self::netlist_of(sch);
        // library parts bring their own footprint
        self.checkpoint();
        // A part added to the library folder after the program started (ccad part import, a file copied in) is not in `library` yet:
        // look in the folder itself before giving up on its footprint.
        let mut on_disk: Option<Vec<part_lib::PartDef>> = None;
        for c in sch.components.iter().filter(|c| c.custom.is_some()) {
            let found = match sch_ed.library.iter().find(|d| d.footprint_name() == c.footprint) {
                Some(d) => Some(d),
                None => on_disk.get_or_insert_with(|| part_lib::load_dir(&part_lib::default_dir()).0).iter().find(|d| d.footprint_name() == c.footprint),
            };
            if let Some(def) = found
                && let Ok(fp) = def.footprint() {
                    self.board.add_footprint(fp);
                    if let Some(m) = &def.model3d {
                        self.board.models3d.insert(def.footprint_name(), m.clone());
                    }
                    if def.land_unverified {
                        self.board.land_unverified.insert(def.footprint_name(), def.name.clone());
                    }
                }
        }
        let rep = netlist::import(&mut self.board, &parts, &nets);
        let shops: Vec<_> = sch.components.iter().filter_map(|c| c.custom.as_ref().map(|s| (c.refdes.clone(), s.shops.clone()))).collect();
        netlist::import_shops(&mut self.board, &shops);
        netlist::import_hints(&mut self.board, &netlist::NetHints { currents: sch.net_currents.clone().into_iter().collect(), pairs: sch.diff_pairs.clone() });
        self.selected = None;
        self.multi.clear();
        self.currents_tried = false; // the circuit may have changed: the next route asks for the currents again if none are declared
        // a footprint that is neither built in nor in the library: say what to do about it
        self.import_notes = rep
            .warnings
            .iter()
            .map(|w| if w.contains("フットプリント") && w.contains("見つかりません") { format!("{w} (部品ライブラリにこの部品がありません。回路図の「ライブラリ」の「再読み込み」を押すか、部品を作り直してから、もう一度取り込んでください)") } else { w.clone() })
            .collect();
        // new parts land beside the outline: show the whole board with them, not a corner of it under the panel on the right
        if !rep.added.is_empty() {
            self.fit_pending = true;
        }
        self.status = format!("取り込み完了: 追加 {} / 更新 {} / 削除 {} / 警告 {}", rep.added.len(), rep.updated, rep.removed.len(), rep.warnings.len());
    }
}

/// What differs between the schematic and the board (see `BoardEditor::schematic_diff`).
pub type SchDiff = pcb_core::netlist::NetlistDiff;
