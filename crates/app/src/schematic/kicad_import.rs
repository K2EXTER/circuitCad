//! "KiCadから取り込み" window: pick parts from a `.kicad_sym` file (and a `.kicad_mod` footprint) and add them to the library.

use super::*;
use part_lib::kicad::{self, KicadFootprint, KicadSymbol};

pub(super) struct KicadImport {
    file: String,
    syms: Vec<KicadSymbol>,
    fp: Option<KicadFootprint>,
    fp_file: String,
    filter: String,
    sel: Option<usize>,
    msg: String,
}

impl SchEditor {
    pub(super) fn open_kicad_import(&mut self) {
        let Some(p) = rfd::FileDialog::new().add_filter("KiCad シンボルライブラリ", &["kicad_sym"]).pick_file() else { return };
        let parsed = std::fs::read_to_string(&p).map_err(|e| format!("読み込み失敗: {e}")).and_then(|s| kicad::parse_symbols(&s));
        match parsed {
            Ok(syms) => {
                self.kicad = Some(KicadImport {
                    file: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    sel: (syms.len() == 1).then_some(0),
                    syms,
                    fp: None,
                    fp_file: String::new(),
                    filter: String::new(),
                    msg: String::new(),
                })
            }
            Err(e) => self.status = format!("KiCad の取り込みに失敗: {e}"),
        }
    }

    pub(super) fn kicad_window(&mut self, ctx: &egui::Context) {
        let Some(mut k) = self.kicad.take() else { return };
        let mut open = true;
        let mut add = false;
        egui::Window::new("KiCadから取り込み").open(&mut open).default_width(460.0).vscroll(true).show(ctx, |ui| {
            ui.label(format!("{} ({} 個の部品)", k.file, k.syms.len()));
            ui.small("ピン配置とフットプリントのパッドを取り込みます。KiCad の記号の絵とシミュレーションモデルは取り込みません (記号は箱になります)。");
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
                        k.sel = Some(i);
                        k.msg.clear();
                    }
                }
            });
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
            if ui.add_enabled(k.sel.is_some(), egui::Button::new("ライブラリに追加")).on_hover_text("選んだ部品を、ライブラリに保存します").clicked() {
                add = true;
            }
            if !k.msg.is_empty() {
                ui.colored_label(Color32::from_rgb(230, 60, 60), &k.msg);
            }
        });
        if add && let Some(s) = k.sel.and_then(|i| k.syms.get(i)) {
            match kicad::to_part(s, k.fp.as_ref()) {
                Err(e) => k.msg = e,
                Ok(def) => match def.validate().into_iter().find(|i| i.error) {
                    Some(i) => k.msg = format!("{} に問題があります: {}", def.name, i.message),
                    None => match part_lib::save(&self.lib_dir, &def) {
                        Ok(_) => {
                            self.reload_library();
                            self.status = format!("ライブラリに追加しました: {} (KiCad)", def.name);
                        }
                        Err(e) => k.msg = format!("保存失敗: {e}"),
                    },
                },
            }
        }
        if open {
            self.kicad = Some(k);
        }
    }
}
