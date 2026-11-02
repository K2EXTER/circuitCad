//! "KiCadから取り込み" window: pick parts from a `.kicad_sym` (or legacy `.lib`) file and a `.kicad_mod` footprint, or from a maker's zip
//! (Ultra Librarian, SnapEDA ... with KiCad chosen) that holds both and perhaps a STEP model, and add them to the library.

use super::*;
use part_lib::kicad::{self, KicadFootprint, KicadSymbol};

pub(super) struct KicadImport {
    file: String,
    syms: Vec<KicadSymbol>,
    fp: Option<KicadFootprint>,
    fp_file: String,
    /// The footprints of a zip, and the STEP model (name, bytes) it holds, if any.
    zip_fps: Vec<KicadFootprint>,
    step: Option<(String, Vec<u8>)>,
    /// Whether the zip is what was read (it should have had a STEP).
    from_zip: bool,
    use_step: bool,
    filter: String,
    sel: Option<usize>,
    msg: String,
}

impl KicadImport {
    /// Selects a symbol; the footprint of the zip that goes with it is taken along.
    fn select(&mut self, i: usize) {
        self.sel = Some(i);
        self.msg.clear();
        if let Some(f) = self.syms.get(i).and_then(|s| kicad::pick_footprint(s, &self.zip_fps)) {
            self.fp_file = self.zip_fps[f].name.clone();
            self.fp = Some(self.zip_fps[f].clone());
        }
    }
}

impl SchEditor {
    pub(super) fn open_kicad_import(&mut self) {
        let Some(p) = rfd::FileDialog::new().add_filter("KiCad のライブラリ (.kicad_sym / .lib) か、メーカーの zip", &["kicad_sym", "lib", "zip"]).pick_file() else { return };
        let is_zip = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip"));
        let parsed = if is_zip {
            kicad::read_package(&p)
        } else {
            std::fs::read_to_string(&p)
                .map_err(|e| format!("読み込み失敗: {e}"))
                .and_then(|s| kicad::parse_symbol_file(&s))
                .map(|symbols| kicad::KicadPackage { symbols, ..Default::default() })
        };
        match parsed {
            Ok(pkg) => {
                let mut k = KicadImport {
                    file: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    sel: None,
                    syms: pkg.symbols,
                    fp: None,
                    fp_file: String::new(),
                    use_step: pkg.step.is_some(),
                    step: pkg.step,
                    from_zip: is_zip,
                    zip_fps: pkg.footprints,
                    filter: String::new(),
                    msg: pkg.warnings.join(" / "),
                };
                if k.syms.len() == 1 {
                    k.select(0);
                }
                self.kicad = Some(k);
            }
            Err(e) => self.status = format!("KiCad の取り込みに失敗: {e}"),
        }
    }

    pub(super) fn kicad_window(&mut self, ctx: &egui::Context) {
        let Some(mut k) = self.kicad.take() else { return };
        let mut open = true;
        let mut add = false;
        egui::Window::new("KiCadから取り込み").open(&mut open).default_pos(crate::widgets::beside_panel()).default_width(460.0).vscroll(true).show(ctx, |ui| {
            ui.label(format!("{} ({} 個の部品)", k.file, k.syms.len()));
            let mut clicked = None;
            ui.small("ピン配置とフットプリントのパッドを取り込みます (zip なら、中の 3D モデルも)。KiCad の記号の絵とシミュレーションモデルは取り込みません (記号は箱になります)。");
            ui.horizontal(|ui| {
                ui.label("検索");
                ui.text_edit_singleline(&mut k.filter).on_hover_text("名前・説明に含まれる文字で絞り込みます");
            });
            let f = k.filter.to_lowercase();
            egui::ScrollArea::vertical().max_height(220.0).id_salt("kicad_list").show(ui, |ui| {
                for (i, s) in k.syms.iter().enumerate() {
                    if !f.is_empty() && !s.name.to_lowercase().contains(&f) && !s.description.to_lowercase().contains(&f) {
                        continue;
                    }
                    let label = format!("{} ({}ピン)", s.name, s.pins.len());
                    if ui.selectable_label(k.sel == Some(i), label).on_hover_text(&s.description).clicked() {
                        clicked = Some(i);
                    }
                }
            });
            if let Some(i) = clicked {
                k.select(i);
            }
            if let Some(s) = k.sel.and_then(|i| k.syms.get(i)) {
                ui.separator();
                ui.label(egui::RichText::new(&s.name).strong());
                if !s.description.is_empty() {
                    ui.small(&s.description);
                }
                if !s.footprint.is_empty() {
                    ui.small(format!("KiCad が指定するフットプリント: {}", s.footprint));
                }
            }
            ui.horizontal(|ui| {
                if ui.button("フットプリントを選ぶ…").on_hover_text("対応する .kicad_mod ファイルを選びます。選ばないと仮のパッド (2.54mm の1列) になります。基板に使うなら必ず選んでください").clicked()
                    && let Some(p) = rfd::FileDialog::new().add_filter("KiCad フットプリント", &["kicad_mod"]).pick_file()
                {
                    match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| kicad::parse_footprint(&s)) {
                        Ok(fp) => {
                            k.fp_file = fp.name.clone();
                            k.fp = Some(fp);
                            k.msg.clear();
                        }
                        Err(e) => k.msg = format!("フットプリントを読めません: {e}"),
                    }
                }
                if k.fp.is_some() && ui.small_button("外す").on_hover_text("フットプリントを使わず、仮のパッドにします").clicked() {
                    k.fp = None;
                }
                ui.small(if k.fp.is_some() { format!("使う: {}", k.fp_file) } else { "未選択 (仮のパッド)".into() });
            });
            if k.zip_fps.len() > 1 {
                let mut pick = None;
                egui::ComboBox::from_label("zip の中のフットプリント").selected_text(if k.fp.is_some() { k.fp_file.clone() } else { "選んでください".into() }).show_ui(ui, |ui| {
                    for (i, f) in k.zip_fps.iter().enumerate() {
                        if ui.selectable_label(k.fp.as_ref().is_some_and(|c| c.name == f.name), &f.name).clicked() {
                            pick = Some(i);
                        }
                    }
                }).response.on_hover_text("zip に複数のフットプリントが入っています。この部品に合うものを選びます");
                if let Some(i) = pick {
                    k.fp_file = k.zip_fps[i].name.clone();
                    k.fp = Some(k.zip_fps[i].clone());
                }
            }
            if let Some((name, _)) = &k.step {
                ui.checkbox(&mut k.use_step, format!("3D の形に使う: {name}")).on_hover_text("zip に入っていた STEP ファイルを、この部品の3Dの形にします (基板の「3Dで見る」に出ます)。パッドの上に、基板の面に立てて置きます。大きなファイルは三角形を間引いて保存します");
            } else if k.from_zip {
                ui.small("この zip には 3D モデル (STEP) が入っていません。ダウンロードのとき STEP も選ぶと、3D の形まで取り込めます").on_hover_text("Ultra Librarian や SnapEDA は、ダウンロードの画面で 3D モデルにチェックを入れたときだけ STEP を zip に入れます");
            }
            if ui.add_enabled(k.sel.is_some(), egui::Button::new("ライブラリに追加")).on_hover_text("選んだ部品を、ライブラリに保存します").clicked() {
                add = true;
            }
            if !k.msg.is_empty() {
                ui.colored_label(crate::theme::ERROR, &k.msg);
            }
        });
        if add && let Some(s) = k.sel.and_then(|i| k.syms.get(i)) {
            match make_part(&k, s) {
                Err(e) => k.msg = e,
                Ok(def) => match part_lib::save(&self.lib_dir, &def) {
                    Ok(_) => {
                        self.reload_library();
                        self.status = format!("ライブラリに追加しました: {} (KiCad{})", def.name, if def.model3d.is_some() { "、3D の形つき" } else { "" });
                    }
                    Err(e) => k.msg = format!("保存失敗: {e}"),
                },
            }
        }
        if open {
            self.kicad = Some(k);
        }
    }
}

/// The part for the chosen symbol, with the footprint and (if asked for) the STEP model, checked.
fn make_part(k: &KicadImport, s: &KicadSymbol) -> Result<part_lib::PartDef, String> {
    let mut def = kicad::to_part(s, k.fp.as_ref())?;
    if let (true, Some((_, bytes))) = (k.use_step, &k.step) {
        let mesh = crate::part_form::mesh_over_pads(bytes, &def).map_err(|e| format!("3D の形 (STEP) を読めません: {e}。チェックを外せば、3D の形なしで追加できます"))?;
        if def.height.is_none() {
            def.height = mesh.bounds().map(|(lo, hi)| ((hi[2] - lo[2]) * 100.0).round() / 100.0).filter(|h| *h > 0.0);
        }
        def.model3d = Some(mesh);
    }
    match def.validate().into_iter().find(|i| i.error) {
        Some(i) => Err(format!("{} に問題があります: {}", def.name, i.message)),
        None => Ok(def),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_makers_package_gives_a_part_with_its_footprint() {
        let lib = "EESchema-LIBRARY Version 2.3\nDEF D1 U 0 10 Y Y 1 F N\nF1 \"D1\" 0 0 60 H V C CNN\nF2 \"SOD27\" 0 0 60 H I C CNN\nDRAW\nX 1 1 0 0 300 R 59 59 1 1 U\nX 2 2 1800 0 300 L 59 59 1 1 U\nENDDRAW\nENDDEF\n";
        let fp = "(module D1 (pad 1 thru_hole rect (at 0 0) (size 1.3 1.3) (drill 0.8)) (pad 2 thru_hole circle (at 8.7 0) (size 1.3 1.3) (drill 0.8)))";
        let mut k = KicadImport {
            file: "x.zip".into(),
            syms: kicad::parse_symbol_file(lib).unwrap(),
            fp: None,
            fp_file: String::new(),
            zip_fps: vec![kicad::parse_footprint(fp).unwrap()],
            step: None,
            from_zip: true,
            use_step: false,
            filter: String::new(),
            sel: None,
            msg: String::new(),
        };
        k.select(0);
        assert_eq!(k.fp_file, "D1", "the only footprint of the zip goes with the symbol");
        let def = make_part(&k, &k.syms[0]).unwrap();
        assert_eq!((def.name.as_str(), def.pins.len(), def.footprint_name.as_deref()), ("D1", 2, Some("D1")));
        assert!(def.model3d.is_none());
    }
}
