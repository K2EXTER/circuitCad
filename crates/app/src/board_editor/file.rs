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

    /// The board as the text of the autosave, when it has changes that are not saved.
    pub fn autosave_json(&self) -> Option<String> {
        if !self.dirty() {
            return None;
        }
        serde_json::to_string(&self.board).ok()
    }

    /// Take the board back from an autosave. It counts as changed (it was never saved), and keeps the name of the file it came from.
    pub fn restore(&mut self, autosave: &std::path::Path, original: Option<PathBuf>) {
        self.load_path(autosave.to_path_buf());
        self.path = original;
        self.rev += 1;
        if self.status.starts_with("開きました") {
            self.status = "保存前の基板を復元しました。保存してください".into();
        }
    }

    /// Write the board's outline, holes and part boxes as a DXF, for the case's CAD.
    pub fn export_dxf(&mut self) {
        let name = format!("{}.dxf", if self.board.name.is_empty() { "board" } else { &self.board.name });
        if let Some(p) = rfd::FileDialog::new().add_filter("DXF (ケース設計用)", &["dxf"]).set_file_name(name).save_file() {
            self.status = match std::fs::write(&p, pcb_core::dxf::mechanical(&self.board)) {
                Ok(()) => format!("DXF を保存しました: {}", p.display()),
                Err(e) => format!("DXF の保存に失敗: {e}"),
            };
        }
    }

    /// Take the outline and the holes from a DXF drawing of the case (the loose holes of before are replaced; the parts stay where they are).
    pub fn import_dxf(&mut self) {
        let Some(p) = rfd::FileDialog::new().add_filter("DXF (ケースの図面)", &["dxf"]).pick_file() else { return };
        match std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display())).and_then(|t| pcb_core::dxf_in::read(&t, None)) {
            Ok(shape) => {
                self.checkpoint();
                pcb_core::dxf_in::apply(&mut self.board, &shape, true);
                self.fit_pending = true;
                self.status = format!("DXF から外形と穴 {} 個を取り込みました (元に戻せます)", shape.holes.len());
                let mut notes = shape.notes.clone();
                notes.push("外形の左下を (0, 0) に合わせました。すでに部品を置いていた場合は、外形に収まっているか確かめてください。".into());
                rfd::MessageDialog::new().set_title("DXF を取り込みました").set_description(notes.iter().map(|n| format!("・{n}")).collect::<Vec<_>>().join("
")).show();
            }
            Err(e) => self.status = format!("DXF を取り込めませんでした: {e}"),
        }
    }

    /// Write one side of the board as an SVG (vector; the bottom is drawn as seen from below).
    pub fn export_svg(&mut self, side: pcb_core::Side) {
        let name = format!("{}-{}.svg", if self.board.name.is_empty() { "board" } else { &self.board.name }, if side == pcb_core::Side::Bottom { "bottom" } else { "top" });
        let Some(p) = rfd::FileDialog::new().add_filter("SVG", &["svg"]).set_file_name(name).save_file() else { return };
        self.status = match pcb_core::svg::board_svg(&self.board, &pcb_core::svg::SvgOpts { side, silk: true }).and_then(|s| std::fs::write(&p, s).map_err(|e| e.to_string())) {
            Ok(()) => format!("SVG を保存しました: {}", p.display()),
            Err(e) => format!("SVG の保存に失敗: {e}"),
        };
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
        let key = (sch_ed.rev, self.rev, sch_ed.lib_rev);
        if self.sync_cache.as_ref().is_none_or(|(k, _)| *k != key) {
            // nothing to compare when no part was imported yet, or when no schematic is open (a board opened on its own)
            let diff = if self.board.parts.is_empty() || sch_ed.sch.components.is_empty() { SchDiff::default() } else { self.diff_against(&sch_ed.sch, &sch_ed.library) };
            self.sync_cache = Some((key, diff));
        }
        &self.sync_cache.as_ref().unwrap().1
    }

    /// Take the schematic's table of requirements again after an event told that it may differ (`SchEditor::req_dirty`: the table was
    /// edited, or a circuit was opened); with no event it does nothing. Only the table is taken, not the parts. A board whose last taking
    /// is not known (just opened) is compared with the rows it keeps: when they agree the table is taken as already in.
    pub fn sync_requirements(&mut self, sch_ed: &mut SchEditor) {
        if !std::mem::take(&mut sch_ed.req_dirty) || self.board.parts.is_empty() || sch_ed.sch.components.is_empty() {
            return;
        }
        let spec = &sch_ed.sch.requirements;
        let b = &self.board;
        let stale = match &self.req_synced {
            Some(last) => last != spec,
            None => b.place_groups != spec.groups || b.keep_apart != spec.apart || b.bypass != spec.bypass || b.waivers != spec.waivers || b.measures != spec.measures,
        };
        if stale {
            self.checkpoint();
            let rep = pcb_core::requirements::import_requirements(&mut self.board, spec);
            let warn = rep.warnings();
            self.status = if warn.is_empty() { "回路図の「設計の要件」の変更を、基板に取り込みました".into() } else { format!("設計の要件を取り込みました。{}", warn.join(" / ")) };
        }
        self.req_synced = Some(spec.clone());
    }

    fn diff_against(&self, sch: &sch_core::Schematic, library: &[part_lib::PartDef]) -> SchDiff {
        let (parts, nets) = Self::netlist_of(sch);
        let mut diff = netlist::diff(&self.board, &parts, &nets);
        // a library part edited after it was imported (a body outline, pads, a 3D shape) keeps its footprint name, so the
        // netlist alone does not see it
        let names: Vec<(String, Option<String>)> = sch.flat().components.iter().map(|c| (c.refdes.clone(), c.custom.as_ref().map(|s| s.name.clone()))).collect();
        for r in library_changes(&self.board, &parts, &names, library) {
            if !diff.changed.contains(&r) {
                diff.changed.push(r);
            }
        }
        diff
    }

    /// Write the names of the pins of the pin headers (set in the circuit) on the silk beside the pads.
    pub fn write_pin_names(&mut self, sch_ed: &SchEditor) {
        let names: Vec<(String, Vec<String>)> = sch_ed.sch.flat().components.iter().filter(|c| !c.pin_names.is_empty() && !c.sim_only).map(|c| (c.refdes.clone(), c.pin_names.clone())).collect();
        if names.is_empty() {
            self.status = "名前をつけたヘッダがありません。回路図でヘッダを選んで、「ピンの名前」に書いてください".into();
            return;
        }
        self.checkpoint();
        let n = pcb_core::pinlabels::write_pin_labels(&mut self.board, &names, 1.0);
        self.status = format!("ヘッダのピンの名前を、シルクに {n} か所書きました (もう一度押すと書き直します)");
    }

    pub fn import_from_schematic(&mut self, sch_ed: &SchEditor) {
        let sch = &*sch_ed.sch.flat();
        let (parts, nets) = Self::netlist_of(sch);
        // library parts bring their own footprint
        self.checkpoint();
        // A part added to the library folder after the program started (ccad part import, a file copied in) is not in `library` yet:
        // look in the folder itself before giving up on its footprint.
        let mut on_disk: Option<Vec<part_lib::PartDef>> = None;
        // the footprints whose land is only roughly right are worked out again from the library each time (a part whose land was
        // checked since loses its mark)
        self.board.land_unverified.clear();
        let mut swaps: Vec<(String, pcb_core::swap::SwapSpec)> = vec![];
        // the board keeps one footprint and one 3D shape for each footprint name: parts that share a name but differ in outline or
        // shape cannot both be right
        let mut by_footprint: std::collections::HashMap<String, (String, Option<serde_json::Value>, bool)> = std::collections::HashMap::new();
        let mut clashes: Vec<String> = vec![];
        for c in sch.components.iter().filter(|c| c.custom.is_some()) {
            // the part the component stands for: many library parts share a footprint name, and each has its own outline and shape
            let want = c.custom.as_ref().map(|s| s.name.as_str());
            let pick = |lib: &'_ [part_lib::PartDef]| library_part(lib, want, &c.footprint).cloned();
            let found = match pick(&sch_ed.library) {
                Some(d) => Some(d),
                None => pick(on_disk.get_or_insert_with(|| part_lib::load_library(&part_lib::default_dir()).0)),
            };
            let found = found.as_ref();
            if let Some(spec) = c.custom.as_ref().and_then(|sym| part_lib::swap_for(&sym.name, &sch_ed.library)) {
                swaps.push((c.refdes.clone(), spec));
            }
            if let Some(def) = found
                && let Ok(fp) = def.footprint() {
                    let shape = (def.name.clone(), serde_json::to_value(&fp).ok(), def.model3d.is_some());
                    match by_footprint.get(&def.footprint_name()) {
                        Some(prev) if prev.0 != shape.0 && (prev.1 != shape.1 || prev.2 != shape.2) => {
                            let note = format!("{} と {} は、フットプリント名「{}」が同じで、外形やパッド・3Dの形が違います (基板は名前ごとに1つの形しか持てないので、後の {} の形になります)。部品の「フットプリント名」を変えると、別々の形になります", prev.0, def.name, def.footprint_name(), def.name);
                            if !clashes.contains(&note) {
                                clashes.push(note);
                            }
                        }
                        _ => {}
                    }
                    by_footprint.insert(def.footprint_name(), shape);
                    self.board.add_footprint(fp);
                    match &def.model3d {
                        Some(m) => {
                            self.board.models3d.insert(def.footprint_name(), m.clone());
                        }
                        // the shape was taken away from the part: the board must not keep the old one
                        None => {
                            self.board.models3d.remove(&def.footprint_name());
                        }
                    }
                    if def.land_unverified {
                        self.board.land_unverified.insert(def.footprint_name(), def.name.clone());
                    }
                }
        }
        let rep = netlist::import(&mut self.board, &parts, &nets);
        let shops: Vec<_> = sch.components.iter().map(|c| (c.refdes.clone(), c.shop_numbers())).collect();
        netlist::import_shops(&mut self.board, &shops);
        netlist::import_swaps(&mut self.board, &swaps);
        let sheets: Vec<(String, String)> = sch.components.iter().filter(|c| !c.kind.is_power_symbol() && !c.sim_only).map(|c| (c.refdes.clone(), sch_ed.sch.sheet_name(sch_core::sheet_of(c.pos).0))).collect();
        netlist::import_sheets(&mut self.board, &sheets);
        let (_, _, req) = netlist::import_hints_report(&mut self.board, &netlist::NetHints { currents: sch.net_currents.clone().into_iter().collect(), pairs: sch.diff_pairs.clone(), requirements: sch.requirements.clone() });
        self.req_synced = Some(sch.requirements.clone());
        self.selected = None;
        self.multi.clear();
        self.currents_tried = false; // the circuit may have changed: the next route asks for the currents again if none are declared
        // a footprint that is neither built in nor in the library: say what to do about it
        self.import_notes = rep
            .warnings
            .iter()
            .map(|w| if w.contains("フットプリント") && w.contains("見つかりません") { format!("{w} (部品ライブラリにこの部品がありません。回路図の「ライブラリ」の「再読み込み」を押すか、部品を作り直してから、もう一度取り込んでください)") } else { w.clone() })
            .collect();
        self.import_notes.extend(req.warnings());
        self.import_notes.extend(clashes);
        // new parts land beside the outline: show the whole board with them, not a corner of it under the panel on the right
        if !rep.added.is_empty() {
            self.fit_pending = true;
        }
        self.status = format!("取り込み完了: 追加 {} / 更新 {} / 削除 {} / 警告 {}", rep.added.len(), rep.updated, rep.removed.len(), rep.warnings.len());
    }
}

/// The library part a component stands for: the one of that name with that footprint, else any part with the footprint (a circuit
/// saved before the part got its name back).
fn library_part<'a>(library: &'a [part_lib::PartDef], name: Option<&str>, footprint: &str) -> Option<&'a part_lib::PartDef> {
    library
        .iter()
        .find(|d| Some(d.name.as_str()) == name && d.footprint_name() == footprint)
        .or_else(|| library.iter().find(|d| d.footprint_name() == footprint))
}

/// The parts of the board whose library part has another footprint (pads, silkscreen, holes) or another 3D shape than the board
/// holds. Only footprints the board stores itself count (a built-in one cannot change under it).
pub(super) fn library_changes(board: &pcb_core::Board, parts: &[NetlistPart], names: &[(String, Option<String>)], library: &[part_lib::PartDef]) -> Vec<String> {
    let mut out = vec![];
    for p in parts {
        let want = names.iter().find(|(r, _)| *r == p.refdes).and_then(|(_, n)| n.as_deref());
        let Some(def) = library_part(library, want, &p.footprint) else { continue };
        let Some(stored) = board.footprints.iter().find(|f| f.name == p.footprint) else { continue };
        let fp_differs = def.footprint().ok().is_some_and(|fp| serde_json::to_value(&fp).ok() != serde_json::to_value(stored).ok());
        if fp_differs || def.model3d.as_ref() != board.models3d.get(&p.footprint) {
            out.push(p.refdes.clone());
        }
    }
    out
}

/// What differs between the schematic and the board (see `BoardEditor::schematic_diff`).
pub type SchDiff = pcb_core::netlist::NetlistDiff;

#[cfg(test)]
mod library_tests {
    use super::*;
    use pcb_core::body::{BodyMark, BodySpec};

    /// A library part edited after the board took it (a body outline on the silkscreen, a 3D shape) shows as a change of the part.
    #[test]
    fn an_edited_library_part_shows_as_a_change_on_the_board() {
        let text = part_lib::EXAMPLES.iter().find(|(n, _)| n.starts_with("LM358")).unwrap().1;
        let def: part_lib::PartDef = serde_json::from_str(text).unwrap();
        let mut board = pcb_core::Board::new("t", 50.0, 30.0);
        board.add_footprint(def.footprint().unwrap());
        let parts = vec![NetlistPart { refdes: "U1".into(), value: "LM358".into(), footprint: def.footprint_name() }];
        let names = vec![("U1".to_string(), Some("LM358".to_string()))];
        assert!(library_changes(&board, &parts, &names, std::slice::from_ref(&def)).is_empty(), "the same part is no change");

        let mut with_body = def.clone();
        with_body.body = Some(BodySpec { shape: "rect".into(), w: 3.9, h: 4.9, x: 0.0, y: 0.0, mark: BodyMark::Dot, mark_pin: "1".into() });
        assert_eq!(library_changes(&board, &parts, &names, &[with_body]), vec!["U1".to_string()], "a new outline");

        let mut with_shape = def.clone();
        with_shape.model3d = Some(pcb_core::mesh::Mesh3d { vertices: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 1.0]], triangles: vec![[0, 1, 2]], tri_color: vec![0], palette: vec![[1, 1, 1]] });
        assert_eq!(library_changes(&board, &parts, &names, &[with_shape]), vec!["U1".to_string()], "a new 3D shape");
        // a part that is not in the library, or whose footprint is built in, is left alone
        assert!(library_changes(&board, &parts, &names, &[]).is_empty());
        // another part with the same footprint name, listed first, is not mistaken for the component's part
        let mut other = def.clone();
        other.name = "OTHER".into();
        other.body = Some(BodySpec { shape: "rect".into(), w: 1.0, h: 1.0, x: 0.0, y: 0.0, mark: BodyMark::None, mark_pin: String::new() });
        assert!(library_changes(&board, &parts, &names, &[other, def.clone()]).is_empty(), "the part of the component's name is compared");
    }

    /// Taking the part again after its library part got a body outline puts the outline on the silkscreen of the part already placed.
    #[test]
    fn taking_the_part_again_brings_the_new_silk_outline() {
        let text = part_lib::EXAMPLES.iter().find(|(n, _)| n.starts_with("LM358")).unwrap().1;
        let def: part_lib::PartDef = serde_json::from_str(text).unwrap();
        let mut board = pcb_core::Board::new("t", 50.0, 30.0);
        board.add_footprint(def.footprint().unwrap());
        let part = pcb_core::Part { refdes: "U1".into(), value: "LM358".into(), footprint: def.footprint_name(), pos: pcb_core::Point::new(20.0, 15.0), rot: 0, side: pcb_core::Side::Top };
        board.set_part(part.clone()).unwrap();
        let before = board.silk.iter().filter(|s| s.refdes == "U1").count();
        let mut edited = def.clone();
        edited.body = Some(BodySpec { shape: "rect".into(), w: 3.9, h: 4.9, x: 0.0, y: 0.0, mark: BodyMark::Dot, mark_pin: "1".into() });
        board.add_footprint(edited.footprint().unwrap());
        board.set_part(part).unwrap();
        let after = board.silk.iter().filter(|s| s.refdes == "U1").count();
        assert!(after > before, "silk lines {before} -> {after}");
    }
}
