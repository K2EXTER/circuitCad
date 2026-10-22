//! Picking a chip capacitor out of Murata's SPICE netlist zip (the user downloads it from Murata; only its path is
//! kept, in `settings.json`). Picking a part fills the form the same way the chip part section does.

use super::*;
use crate::prefs::Prefs;
use part_lib::murata::{Filter, MurataEntry, MurataIndex};

/// Rows shown at once; more than this and the user is asked to narrow the search.
const SHOWN: usize = 200;

#[derive(Default)]
pub struct MurataPicker {
    index: Option<MurataIndex>,
    /// The zip path from the settings, read once (None = not read yet).
    zip: Option<String>,
    /// The path `index` was read from.
    loaded: String,
    text: String,
    size: String,
    dielectric: String,
    c_min: String,
    c_max: String,
    v_min: String,
    message: Option<Result<String, String>>,
}

/// A capacitance as people type it: "10u", "100n", "4.7u"; empty = no limit.
fn capacitance(label: &str, t: &str) -> Result<Option<f64>, String> {
    let t = t.trim().trim_end_matches(['F', 'f']);
    if t.is_empty() {
        return Ok(None);
    }
    sim_core::parse_value(&t.to_lowercase()).filter(|v| *v > 0.0).map(Some).ok_or_else(|| format!("{label}「{t}」を、0 より大きい数で入力してください (例: 10u / 100n / 22p)"))
}

impl PartForm {
    /// Reads the zip named in the settings (again when the path changed).
    fn murata_load(&mut self, path: &str) {
        if path.is_empty() {
            self.mu.index = None;
            self.mu.loaded.clear();
            return;
        }
        match MurataIndex::open(std::path::Path::new(path)) {
            Ok(idx) => {
                self.mu.message = Some(Ok(format!("{} 個のモデルを読み込みました", idx.entries.len())));
                self.mu.index = Some(idx);
            }
            Err(e) => {
                self.mu.message = Some(Err(e));
                self.mu.index = None;
            }
        }
        self.mu.loaded = path.to_string();
    }

    fn murata_filter(&self) -> Result<Filter, String> {
        let v_min = match self.mu.v_min.trim() {
            "" => None,
            t => Some(t.trim_end_matches(['V', 'v']).parse::<f64>().map_err(|_| format!("耐圧「{t}」を V の数で入力してください (例: 16)"))?),
        };
        Ok(Filter {
            text: self.mu.text.clone(),
            size_inch: self.mu.size.clone(),
            dielectric: self.mu.dielectric.clone(),
            c_min: capacitance("容量 (下限)", &self.mu.c_min)?,
            c_max: capacitance("容量 (上限)", &self.mu.c_max)?,
            v_min,
        })
    }

    /// Fill the form from a Murata part: pins, symbol, value, the `.subckt` and the rated voltage.
    fn apply_murata(&mut self, e: &MurataEntry) -> Result<String, String> {
        let g = self.mu.index.as_ref().ok_or("村田のモデルのzipを読み込んでください")?.generate(e)?;
        self.pas_kind = PassiveKind::Capacitor;
        for s in [&mut self.pas_esr, &mut self.pas_esl, &mut self.pas_srf, &mut self.pas_bias, &mut self.pas_table, &mut self.pas_tc1, &mut self.pas_tc2, &mut self.pas_tctable] {
            s.clear();
        }
        self.pas_value = g.value_text.clone();
        self.name = e.part_number.clone();
        self.prefix = "C".into();
        self.value = g.value_text;
        self.description = g.description;
        if self.category.trim().is_empty() || self.category == "ユーザー" {
            self.category = PassiveKind::Capacitor.label().into();
        }
        self.pins_text = "1 1 left\n2 2 right".into();
        self.sim = SimChoice::Subckt;
        self.sim_name = g.sim_name;
        self.sim_text = g.sim_text;
        self.sim_pins = g.sim_pins.join(" ");
        self.sim_limits = g.limits.join("\n");
        self.glyph = PassiveKind::Capacitor.glyph().into();
        self.rt_v = passive::eng(e.voltage);
        let fp = format!("C_{}", e.size_inch);
        let footprint = if pcb_core::footprints::get(&fp).is_some() {
            self.fp_preset = fp.clone();
            format!("フットプリントは {fp} にしました")
        } else {
            self.fp_preset.clear();
            self.pkg = PkgKind::Chip2;
            format!("{} 用の組み込みフットプリントはまだ無いので、「パッケージ」で寸法を入れてください", e.size_inch)
        };
        Ok(format!(
            "{} を入れました (定格電圧 {}V も「定格」に入れました)。{footprint}。モデルを入れ直してしまうので、このあと下の「部品を作る」は押さずに、そのまま保存してください",
            e.part_number,
            passive::eng(e.voltage)
        ))
    }

    /// The Murata section of the chip capacitor part: where the zip is, a search, and the list.
    pub(super) fn murata_ui(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("村田製作所のセラミックコンデンサから選ぶ (SPICEモデル付き)").id_salt("murata_section").show(ui, |ui| {
            crate::icons::help_small(
                ui,
                "村田が公開しているモデルを、型番ごとに使えます",
                "村田製作所のサイトの「SPICE netlist」からダウンロードした GRM シリーズの zip を選ぶと、容量・サイズ・耐圧で探して、ESR・ESL まで入った等価回路の部品を作れます。zip は村田の利用規約で再配布できないため、このソフトには入っていません。場所だけを覚えます。モデルは 25℃・直流電圧 0V の値で、直流電圧で容量が下がる性質は入っていません",
            );
            let file = Prefs::default_file();
            let zip = self.mu.zip.get_or_insert_with(|| Prefs::load(&file).murata_zip).clone();
            ui.horizontal(|ui| {
                if ui.button("zip を選ぶ…").on_hover_text("村田の GRM シリーズの zip (例: grm-n-v78.zip) を選びます。場所は設定に保存され、次からは自動で読み込みます").clicked()
                    && let Some(p) = rfd::FileDialog::new().set_title("村田の SPICE netlist (zip)").add_filter("zip", &["zip"]).pick_file()
                {
                    let path = p.to_string_lossy().into_owned();
                    // keep the other settings as they are
                    if let Err(e) = (Prefs { murata_zip: path.clone(), ..Prefs::load(&file) }).save(&file) {
                        self.mu.message = Some(Err(e));
                    }
                    self.mu.zip = Some(path);
                }
                ui.small(if zip.is_empty() { "(未設定)" } else { zip.as_str() });
            });
            let zip = self.mu.zip.clone().unwrap_or_default();
            if zip != self.mu.loaded {
                self.murata_load(&zip);
            }
            if let Some(Err(e)) = &self.mu.message {
                ui.colored_label(Color32::from_rgb(230, 60, 60), e);
            }
            let Some(idx) = &self.mu.index else {
                if zip.is_empty() {
                    ui.small("zip は村田の「SPICE netlist」のページからダウンロードできます (規約への同意が要ります)");
                }
                return;
            };
            let sizes = idx.sizes();
            let dielectrics = idx.dielectrics();
            egui::Grid::new("murata_grid").num_columns(2).show(ui, |ui| {
                ui.label("型番の一部").on_hover_text("例: GRM188R6 のように、型番の一部で絞ります");
                ui.add(egui::TextEdit::singleline(&mut self.mu.text).desired_width(140.0));
                ui.end_row();
                ui.label("サイズ (インチ)").on_hover_text("0603 は 1608 (1.6×0.8mm)。メトリックの数字 (1608) でも探せます");
                egui::ComboBox::from_id_salt("mu_size").selected_text(if self.mu.size.is_empty() { "すべて" } else { self.mu.size.as_str() }).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.mu.size, String::new(), "すべて");
                    for s in &sizes {
                        ui.selectable_value(&mut self.mu.size, s.clone(), s);
                    }
                });
                ui.end_row();
                ui.label("温度特性").on_hover_text("X5R・X7R は容量が大きく、直流電圧で容量が下がります。C0G は容量が小さい代わりに安定です");
                egui::ComboBox::from_id_salt("mu_diel").selected_text(if self.mu.dielectric.is_empty() { "すべて" } else { self.mu.dielectric.as_str() }).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.mu.dielectric, String::new(), "すべて");
                    for s in &dielectrics {
                        ui.selectable_value(&mut self.mu.dielectric, s.clone(), s);
                    }
                });
                ui.end_row();
                ui.label("容量").on_hover_text("下限と上限。同じ値を入れると、その容量だけ探します (例: 10u と 10u)");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.mu.c_min).desired_width(60.0).hint_text("下限 10u"));
                    ui.label("〜");
                    ui.add(egui::TextEdit::singleline(&mut self.mu.c_max).desired_width(60.0).hint_text("上限 10u"));
                });
                ui.end_row();
                ui.label("耐圧 (V 以上)").on_hover_text("定格電圧がこの値以上のものだけ。セラミックは電圧が高いほど容量が下がりにくいので、使う電圧の2倍ほどが目安です");
                ui.add(egui::TextEdit::singleline(&mut self.mu.v_min).desired_width(60.0).hint_text("16"));
                ui.end_row();
            });
            let filter = match self.murata_filter() {
                Ok(f) => f,
                Err(e) => {
                    ui.colored_label(Color32::from_rgb(230, 60, 60), e);
                    return;
                }
            };
            let hits: Vec<MurataEntry> = idx.search(&filter).into_iter().take(SHOWN + 1).cloned().collect();
            ui.small(if hits.len() > SHOWN { format!("{SHOWN} 件以上あります。絞り込んでください") } else { format!("{} 件", hits.len()) });
            let mut picked = None;
            egui::ScrollArea::vertical().max_height(180.0).id_salt("murata_list").show(ui, |ui| {
                for e in hits.iter().take(SHOWN) {
                    let label = format!("{}  {}F  {}V  {}  {}", e.part_number, passive::eng(e.capacitance), passive::eng(e.voltage), e.dielectric, e.size_inch);
                    if ui.selectable_label(self.name == e.part_number, label).on_hover_text("クリックすると、この部品の記号・モデル・値をフォームに入れます").clicked() {
                        picked = Some(e.clone());
                    }
                }
            });
            if let Some(e) = picked {
                self.mu.message = Some(self.apply_murata(&e));
            }
            if let Some(Ok(m)) = &self.mu.message {
                ui.colored_label(Color32::from_rgb(60, 170, 90), m);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const MODEL: &str = "* Description :1608M(0603)/X5R(-55to85[deg])/1.00e+01[uF]/10[V]\r\n* Murata P/N :GRM188R61A106MAAL\r\n.SUBCKT GRM188R61A106MAAL Port1 Port2\r\nC01 Port1 N01 7.91e-06\r\nR01 Port1 N01 5.00e+06\r\nL02 N01 N02 1.61e-10\r\nR03 N02 Port2 3.19e-03\r\n.ENDS GRM188R61A106MAAL\r\n";

    fn picker_with_zip(tag: &str) -> (MurataPicker, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("cc_mu_form_{tag}_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("grm.zip");
        let mut z = zip::ZipWriter::new(std::fs::File::create(&p).unwrap());
        z.start_file("High_dielectric_constant/1608_0603/GRM188R61A106MAAL.mod", zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(MODEL.as_bytes()).unwrap();
        z.finish().unwrap();
        (MurataPicker { index: Some(MurataIndex::open(&p).unwrap()), ..Default::default() }, dir)
    }

    /// Picking a part fills the form so that it saves as a two-pin capacitor with the Murata subcircuit and its voltage
    /// rating, and the saved model runs in the simulator.
    #[test]
    fn a_picked_part_saves_as_a_capacitor_that_simulates() {
        let (mu, dir) = picker_with_zip("pick");
        let mut f = PartForm { mu, ..PartForm::default() };
        let e = f.mu.index.as_ref().unwrap().entries[0].clone();
        let msg = f.apply_murata(&e).unwrap();
        assert!(msg.contains("GRM188R61A106MAAL") && msg.contains("C_0603"), "{msg}");
        let d = f.to_def().unwrap();
        assert_eq!((d.name.as_str(), d.prefix.as_str(), d.glyph.as_str(), d.value.as_deref()), ("GRM188R61A106MAAL", "C", "capacitor", Some("10u")));
        assert_eq!(d.ratings.as_ref().and_then(|r| r.v_max), Some(10.0));
        assert_eq!(d.footprint_name.as_deref(), Some("C_0603"));
        let sim = d.sim.as_ref().unwrap();
        assert_eq!(sim.pins, vec!["1", "2"]);
        assert!(sim.limits.iter().any(|l| l.contains("DCバイアス")), "the DC bias gap is stated");
        // 1 V through 1 kΩ into the part: no DC current through a capacitor, so the node stays at 1 V
        let mut c = sim_core::Circuit::new();
        let (a, b) = (c.node("a"), c.node("b"));
        c.add(sim_core::Element::vdc("V1", a, 0, 1.0));
        c.add(sim_core::Element::Resistor { name: "R1".into(), a, b, r: 1e3 });
        sim_core::add_subckt_instance(&mut c, "U1", &sim.name, &sim.text, &[b, 0]).unwrap();
        let op = sim_core::dc_op(&c).unwrap_or_else(|e| panic!("{e:?}"));
        assert!((op.voltage("b").expect("node b") - 1.0).abs() < 1e-3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_size_without_a_built_in_footprint_asks_for_the_dimensions() {
        let (mu, dir) = picker_with_zip("nofp");
        let mut f = PartForm { mu, ..PartForm::default() };
        let mut e = f.mu.index.as_ref().unwrap().entries[0].clone();
        e.size_inch = "0402".into();
        let msg = f.apply_murata(&e).unwrap();
        assert!(msg.contains("寸法"), "{msg}");
        assert!(f.fp_preset.is_empty() && f.pkg == PkgKind::Chip2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_capacitance_filter_reads_engineering_suffixes() {
        assert_eq!(capacitance("x", "").unwrap(), None);
        assert!((capacitance("x", "10u").unwrap().unwrap() - 10e-6).abs() < 1e-12);
        assert!((capacitance("x", "4.7uF").unwrap().unwrap() - 4.7e-6).abs() < 1e-12);
        assert!(capacitance("容量", "abc").unwrap_err().contains("容量"));
    }
}
