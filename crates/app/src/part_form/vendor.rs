//! Picking a two-pin part out of the SPICE model zips the makers publish (Murata's netlist zip, ...). The user downloads
//! them; only their paths are kept, in `settings.json`. Picking a part fills the form the same way the chip part section does.

use super::*;
use crate::prefs::Prefs;
use part_lib::vendorzip::{Filter, VendorEntry, VendorIndex};

/// Rows shown at once; more than this and the user is asked to narrow the search.
const SHOWN: usize = 200;

#[derive(Default)]
pub struct VendorPicker {
    indexes: Vec<VendorIndex>,
    /// The zip paths from the settings, read once (None = not read yet).
    zips: Option<Vec<String>>,
    /// The paths `indexes` was read from.
    loaded: Vec<String>,
    text: String,
    size: String,
    dielectric: String,
    c_min: String,
    c_max: String,
    v_min: String,
    /// Reading problems, one per zip that failed.
    errors: Vec<String>,
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

fn save_zips(zips: &[String]) -> Result<(), String> {
    let file = Prefs::default_file();
    // keep the other settings as they are
    Prefs { model_zips: zips.to_vec(), ..Prefs::load(&file) }.save(&file)
}

impl PartForm {
    /// Reads the zips named in the settings (again when the list changed).
    fn vendor_load(&mut self) {
        let zips = self.mu.zips.clone().unwrap_or_default();
        if zips == self.mu.loaded {
            return;
        }
        self.mu.indexes.clear();
        self.mu.errors.clear();
        for z in &zips {
            match VendorIndex::open(std::path::Path::new(z)) {
                Ok(idx) => self.mu.indexes.push(idx),
                Err(e) => self.mu.errors.push(e),
            }
        }
        let n: usize = self.mu.indexes.iter().map(|i| i.entries.len()).sum();
        self.mu.message = (n > 0).then(|| Ok(format!("{n} 個のモデルを読み込みました")));
        self.mu.loaded = zips;
    }

    fn vendor_filter(&self) -> Result<Filter, String> {
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

    /// Fill the form from a maker's model: pins, symbol, the `.subckt`, and (when the maker's reader knew them) value and
    /// rated voltage.
    fn apply_vendor(&mut self, index: usize, e: &VendorEntry) -> Result<String, String> {
        let g = self.mu.indexes.get(index).ok_or("モデルのzipを読み込んでください")?.generate(e)?;
        self.pas_kind = PassiveKind::Capacitor;
        for s in [&mut self.pas_esr, &mut self.pas_esl, &mut self.pas_srf, &mut self.pas_bias, &mut self.pas_table, &mut self.pas_tc1, &mut self.pas_tc2, &mut self.pas_tctable] {
            s.clear();
        }
        self.name = e.part_number.clone();
        self.prefix = "C".into();
        if !g.value_text.is_empty() {
            self.pas_value = g.value_text.clone();
            self.value = g.value_text;
        }
        self.description = g.description;
        if self.category.trim().is_empty() || self.category == "ユーザー" {
            self.category = PassiveKind::Capacitor.label().into();
        }
        // an electrolytic (the polarized switch of the chip part section): pin 1 is "+"; the models are linear, so the
        // direction does not change a result
        self.pins_text = if self.pas_polar { "1 + left\n2 - right" } else { "1 1 left\n2 2 right" }.into();
        self.sim = SimChoice::Subckt;
        self.sim_name = g.sim_name;
        self.sim_text = g.sim_text;
        self.sim_pins = g.sim_pins.join(" ");
        self.sim_limits = g.limits.join("\n");
        self.glyph = if self.pas_polar { "electrolytic" } else { PassiveKind::Capacitor.glyph() }.into();
        let Some(m) = &e.meta else {
            return Ok(format!(
                "{} を入れました。このモデルには容量・耐圧の情報がないので、「値」と「定格」は自分で入れてください。モデルを入れ直してしまうので、このあと下の「部品を作る」は押さずに、そのまま保存してください",
                e.part_number
            ));
        };
        self.rt_v = passive::eng(m.voltage);
        if m.size_inch.is_empty() {
            return Ok(format!(
                "{} を入れました (定格電圧 {}V も「定格」に入れました)。パッケージは、データシートの寸法を「パッケージ」で入れてください。モデルを入れ直してしまうので、このあと下の「部品を作る」は押さずに、そのまま保存してください",
                e.part_number,
                passive::eng(m.voltage)
            ));
        }
        let fp = format!("C_{}", m.size_inch);
        let footprint = if pcb_core::footprints::get(&fp).is_some() {
            self.fp_preset = fp.clone();
            format!("フットプリントは {fp} にしました")
        } else {
            self.fp_preset.clear();
            self.pkg = PkgKind::Chip2;
            format!("{} 用の組み込みフットプリントはまだ無いので、「パッケージ」で寸法を入れてください", m.size_inch)
        };
        Ok(format!(
            "{} を入れました (定格電圧 {}V も「定格」に入れました)。{footprint}。モデルを入れ直してしまうので、このあと下の「部品を作る」は押さずに、そのまま保存してください",
            e.part_number,
            passive::eng(m.voltage)
        ))
    }

    /// The maker model section of the chip capacitor part: where the zips are, a search, and the list.
    pub(super) fn vendor_ui(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("メーカーのSPICEモデル (zip) から選ぶ").id_salt("vendor_section").show(ui, |ui| {
            crate::icons::help_small(
                ui,
                "村田などが公開しているモデルを、型番ごとに使えます",
                "メーカーのサイトからダウンロードしたSPICEモデルの zip (例: 村田の「SPICE netlist」の GRM シリーズ) を選ぶと、型番で探して、ESR・ESL まで入った等価回路の部品を作れます。村田のモデルは、容量・サイズ・耐圧でも絞れます。zip はメーカーの利用規約で再配布できないため、このソフトには入っていません。場所だけを覚えます。村田のモデルは 25℃・直流電圧 0V の値で、直流電圧で容量が下がる性質は入っていません",
            );
            let file = Prefs::default_file();
            let mut zips = self.mu.zips.get_or_insert_with(|| Prefs::load(&file).model_zips).clone();
            let mut changed = false;
            let mut remove = None;
            for (i, z) in zips.iter().enumerate() {
                ui.horizontal(|ui| {
                    if ui.small_button("✕").on_hover_text("この zip を一覧から外します (ファイルは消えません)").clicked() {
                        remove = Some(i);
                    }
                    ui.small(z);
                });
            }
            if let Some(i) = remove {
                zips.remove(i);
                changed = true;
            }
            if ui.button("zip を追加…").on_hover_text("メーカーの SPICE モデルの zip を選びます。場所は設定に保存され、次からは自動で読み込みます").clicked()
                && let Some(p) = rfd::FileDialog::new().set_title("メーカーの SPICE モデル (zip)").add_filter("zip", &["zip"]).pick_file()
            {
                let path = p.to_string_lossy().into_owned();
                if !zips.contains(&path) {
                    zips.push(path);
                    changed = true;
                }
            }
            if changed {
                if let Err(e) = save_zips(&zips) {
                    self.mu.message = Some(Err(e));
                }
                self.mu.zips = Some(zips.clone());
            }
            self.vendor_load();
            for e in &self.mu.errors {
                ui.colored_label(Color32::from_rgb(230, 60, 60), e);
            }
            if let Some(Err(e)) = &self.mu.message {
                ui.colored_label(Color32::from_rgb(230, 60, 60), e);
            }
            if self.mu.indexes.is_empty() {
                if zips.is_empty() {
                    ui.small("zip はメーカーの「SPICE」「netlist」のページからダウンロードできます (規約への同意が要ります)");
                }
                return;
            }
            let mut sizes: Vec<String> = self.mu.indexes.iter().flat_map(|i| i.sizes()).collect();
            sizes.dedup();
            let mut dielectrics: Vec<String> = self.mu.indexes.iter().flat_map(|i| i.dielectrics()).collect();
            dielectrics.sort();
            dielectrics.dedup();
            let described = self.mu.indexes.iter().map(|i| i.described()).sum::<usize>();
            egui::Grid::new("vendor_grid").num_columns(2).show(ui, |ui| {
                ui.label("型番の一部").on_hover_text("例: GRM188R6 のように、型番の一部で絞ります。空白で区切ると、全部の語を含むものだけになります");
                ui.add(egui::TextEdit::singleline(&mut self.mu.text).desired_width(140.0));
                ui.end_row();
                if described > 0 {
                    ui.label("サイズ (インチ)").on_hover_text("0603 は 1608 (1.6×0.8mm)。メトリックの数字 (1608) でも探せます。サイズ・温度特性・容量・耐圧で絞れるのは、内容を読み取れるモデル (今は村田・日本ケミコン・ニチコン。サイズ・温度特性は村田だけ) だけです");
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
                }
            });
            let filter = match self.vendor_filter() {
                Ok(f) => f,
                Err(e) => {
                    ui.colored_label(Color32::from_rgb(230, 60, 60), e);
                    return;
                }
            };
            let hits: Vec<(usize, VendorEntry)> =
                self.mu.indexes.iter().enumerate().flat_map(|(n, idx)| idx.search(&filter).into_iter().map(move |e| (n, e.clone()))).take(SHOWN + 1).collect();
            ui.small(if hits.len() > SHOWN { format!("{SHOWN} 件以上あります。絞り込んでください") } else { format!("{} 件", hits.len()) });
            let mut picked = None;
            egui::ScrollArea::vertical().max_height(180.0).id_salt("vendor_list").show(ui, |ui| {
                for (n, e) in hits.iter().take(SHOWN) {
                    let label = match &e.meta {
                        Some(m) => format!("{}  {}F  {}V  {}  {}", e.part_number, passive::eng(m.capacitance), passive::eng(m.voltage), m.dielectric, m.size_inch),
                        None => format!("{}  ({}端子)", e.part_number, e.nodes),
                    };
                    let usable = e.nodes == 2;
                    let tip = if usable { "クリックすると、この部品の記号・モデルをフォームに入れます" } else { "端子が2個のモデルだけ選べます (ここはコンデンサ・抵抗・コイルなどの2端子部品の作成です)" };
                    if ui.add_enabled(usable, egui::Button::selectable(self.name == e.part_number, label)).on_hover_text(tip).clicked() {
                        picked = Some((*n, e.clone()));
                    }
                }
            });
            if let Some((n, e)) = picked {
                self.mu.message = Some(self.apply_vendor(n, &e));
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

    const MODEL: &str = "* Murata\r\n* Description :1608M(0603)/X5R(-55to85[deg])/1.00e+01[uF]/10[V]\r\n* Murata P/N :GRM188R61A106MAAL\r\n.SUBCKT GRM188R61A106MAAL Port1 Port2\r\nC01 Port1 N01 7.91e-06\r\nR01 Port1 N01 5.00e+06\r\nL02 N01 N02 1.61e-10\r\nR03 N02 Port2 3.19e-03\r\n.ENDS GRM188R61A106MAAL\r\n";
    const OTHER: &str = "* another maker\n.SUBCKT MLCC_X 1 2\nC1 1 3 2.2u\nR1 3 2 20m\n.ENDS\n.SUBCKT IC3 a b c\nR1 a b 1\nR2 b c 1\n.ENDS\n";

    fn picker_with_zip(tag: &str) -> (VendorPicker, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("cc_mu_form_{tag}_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("grm.zip");
        let mut z = zip::ZipWriter::new(std::fs::File::create(&p).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        z.start_file("High_dielectric_constant/1608_0603/GRM188R61A106MAAL.mod", o).unwrap();
        z.write_all(MODEL.as_bytes()).unwrap();
        z.start_file("other/lib.lib", o).unwrap();
        z.write_all(OTHER.as_bytes()).unwrap();
        z.finish().unwrap();
        (VendorPicker { indexes: vec![VendorIndex::open(&p).unwrap()], ..Default::default() }, dir)
    }

    fn pick(f: &PartForm, pn: &str) -> VendorEntry {
        f.mu.indexes[0].entries.iter().find(|e| e.part_number == pn).unwrap().clone()
    }

    /// Picking a part fills the form so that it saves as a two-pin capacitor with the maker's subcircuit and its voltage
    /// rating, and the saved model runs in the simulator.
    #[test]
    fn a_picked_part_saves_as_a_capacitor_that_simulates() {
        let (mu, dir) = picker_with_zip("pick");
        let mut f = PartForm { mu, ..PartForm::default() };
        let e = pick(&f, "GRM188R61A106MAAL");
        let msg = f.apply_vendor(0, &e).unwrap();
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

    /// A model no maker's reader described still fills the pins and the subcircuit; the value and rating are left to the user.
    #[test]
    fn a_model_without_figures_leaves_the_value_and_rating_to_the_user() {
        let (mu, dir) = picker_with_zip("plain");
        let mut f = PartForm { mu, ..PartForm::default() };
        f.pas_value = "stale".into();
        let e = pick(&f, "MLCC_X");
        let msg = f.apply_vendor(0, &e).unwrap();
        assert!(msg.contains("自分で入れて"), "{msg}");
        assert_eq!((f.pas_value.as_str(), f.rt_v.as_str()), ("stale", ""), "nothing is invented");
        assert!(f.sim_text.contains("C1 1 3 2.2u") && f.sim_pins == "1 2");
        // three nodes cannot become a two-pin part
        let three = pick(&f, "IC3");
        assert!(f.apply_vendor(0, &three).unwrap_err().contains("3 個"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// With the polarized switch on, the part gets "+" / "-" pins and the electrolytic symbol; a maker's figures without a
    /// size leave the package to the user.
    #[test]
    fn an_electrolytic_gets_plus_and_minus_pins() {
        let (mu, dir) = picker_with_zip("polar");
        let mut f = PartForm { mu, pas_polar: true, ..PartForm::default() };
        let mut e = pick(&f, "GRM188R61A106MAAL");
        e.meta.as_mut().unwrap().size_inch.clear();
        let msg = f.apply_vendor(0, &e).unwrap();
        assert!(msg.contains("寸法"), "{msg}");
        let d = f.to_def().unwrap();
        assert_eq!(d.glyph, "electrolytic");
        assert_eq!(d.pins.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["+", "-"]);
        assert_eq!(d.ratings.as_ref().and_then(|r| r.v_max), Some(10.0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_size_without_a_built_in_footprint_asks_for_the_dimensions() {
        let (mu, dir) = picker_with_zip("nofp");
        let mut f = PartForm { mu, ..PartForm::default() };
        let mut e = pick(&f, "GRM188R61A106MAAL");
        e.meta.as_mut().unwrap().size_inch = "0402".into();
        let msg = f.apply_vendor(0, &e).unwrap();
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
