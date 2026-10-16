use super::*;

impl PartForm {
    /// The first question: what kind of part is this. Fills pins, package, body and symbol with a starting point.
    pub(super) fn kind_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("部品の種類").strong());
            for (t, label, tip) in PART_TYPES {
                if ui.selectable_label(self.ptype == Some(*t), egui::RichText::new(*label).size(15.0)).on_hover_text(*tip).clicked() {
                    self.apply_type(*t);
                }
            }
        });
        match self.ptype {
            Some(PartType::Transistor) => {
                ui.horizontal_wrapped(|ui| {
                    ui.label("どの種類?");
                    for (k, label, tip) in [
                        ("npn", "NPN", "ベース電流でコレクタ→エミッタの電流を制御する。負荷を GND 側で切り替える"),
                        ("pnp", "PNP", "NPN と電流の向きが逆。電源側で切り替える"),
                        ("nmos", "Nch MOSFET", "ゲート電圧で制御。大電流のスイッチに向く"),
                        ("pmos", "Pch MOSFET", "N チャネルと極性が逆。電源側で切り替える"),
                    ] {
                        if ui.selectable_label(self.glyph == k, label).on_hover_text(tip).clicked() {
                            self.set_transistor(k);
                        }
                    }
                });
            }
            Some(PartType::Diode) => {
                ui.horizontal_wrapped(|ui| {
                    ui.label("どの種類?");
                    for (k, label, tip) in [
                        ("diode", "整流・スイッチング", "電流を一方向に流す。記号は三角と線"),
                        ("schottky", "ショットキー", "順方向の電圧降下が小さい。棒の両端が S 形の記号"),
                        ("zener", "ツェナー", "逆向きで電圧を一定に保つ。棒の両端が Z 形の記号"),
                    ] {
                        if ui.selectable_label(self.glyph == k, label).on_hover_text(tip).clicked() {
                            self.glyph = k.into();
                        }
                    }
                });
            }
            Some(PartType::Capacitor) => {
                ui.horizontal_wrapped(|ui| {
                    ui.label("どの種類?");
                    for (k, label, tip) in [
                        ("ceramic", "積層セラミック", "チップ部品。小さくて高い周波数に強い。極性なし。電圧がかかると容量が減ります"),
                        ("electrolytic", "電解", "リード付きの円筒形。容量が大きく、電源の平滑に使う。極性があり、ESR は大きめ"),
                        ("tantalum", "タンタル", "小さくて容量が大きい。極性があり、耐圧に余裕を見ないと壊れます"),
                        ("film", "フィルム", "リード付きの箱形。極性なし。ESR・ESL が小さく、容量が安定します"),
                    ] {
                        if ui.selectable_label(self.ptype_sub == k, label).on_hover_text(tip).clicked() {
                            self.set_capacitor(k);
                        }
                    }
                });
                crate::icons::help_small(ui, "名前の付け方", "名前は、種類の頭文字から始めると、部品一覧の「コンデンサ」の木で種類ごとに分かれます (電解 CE_ / タンタル CT_ / フィルム CF_)。値は「① 基本」のかんたん作成で入れます");
            }
            Some(t @ (PartType::Resistor | PartType::Inductor)) => {
                ui.horizontal_wrapped(|ui| {
                    ui.label("どの形?");
                    for (k, label, tip) in [("chip", "チップ", "面実装の小さい部品 (0603・0805 など)"), ("leaded", "リード付き", "足を基板の穴に通す部品")] {
                        if ui.selectable_label(self.ptype_sub == k, label).on_hover_text(tip).clicked() {
                            self.set_resistor_or_inductor(t, k);
                        }
                    }
                });
            }
            _ => {}
        }
    }

    /// Shows one tab ("basic", "pins", "package", "sim", "ratings"), for scripts and screenshots.
    pub fn show_tab(&mut self, name: &str) {
        if let Some((t, _, _)) = FORM_TABS.iter().zip(["basic", "pins", "package", "sim", "ratings"]).find(|(_, n)| *n == name).map(|(t, _)| t) {
            self.tab = *t;
        }
    }

    /// The checklist of what to look up that the kind of part chosen at the top settles (None = still open: an IC
    /// may be an op-amp or a regulator, or no kind was chosen).
    pub(super) fn checklist_for_type(&self) -> Option<usize> {
        let by_name = |head: &str| CHECKLISTS.iter().position(|c| c.0.starts_with(head));
        match self.ptype? {
            PartType::Ic => None,
            PartType::Transistor => by_name(if matches!(self.glyph.as_str(), "nmos" | "pmos") { "MOSFET" } else { "トランジスタ" }),
            PartType::Diode => by_name(if self.glyph == "zener" { "ツェナー" } else { "ダイオード" }),
            PartType::Capacitor if self.pas_polar => by_name("電解コンデンサ"),
            PartType::Capacitor | PartType::Resistor | PartType::Inductor | PartType::Connector => by_name("そのほか"),
        }
    }

    /// A form that starts from this kind of part ("ic", "transistor", "diode", "chip", "connector"), for scripts and screenshots.
    pub fn for_type(name: &str) -> Self {
        let mut f = PartForm::default();
        // a kind of capacitor by its own name
        if let Some(k) = ["ceramic", "electrolytic", "tantalum", "film"].into_iter().find(|k| *k == name) {
            f.set_capacitor(k);
            return f;
        }
        if let Some((t, _, _)) = PART_TYPES.iter().find(|(t, _, _)| format!("{t:?}").eq_ignore_ascii_case(name)) {
            f.apply_type(*t);
        }
        f
    }

    pub(super) fn apply_type(&mut self, t: PartType) {
        self.ptype = Some(t);
        self.fp_preset.clear();
        self.thermal_on = false;
        self.extras.clear();
        self.body_pin.clear();
        match t {
            PartType::Ic => {
                // the ratings list starts at a regulator (the first kind of IC), not at the diode
                self.rt_kind = CHECKLISTS.iter().position(|c| c.0.starts_with("レギュレータ")).unwrap_or(0);
                self.prefix = "U".into();
                self.glyph.clear();
                self.pins_text = "1 IN\n2 GND\n3 OUT\n4 VCC".into();
                self.pkg = PkgKind::Dual;
                self.tht = false;
                (self.pitch, self.row, self.len, self.width) = (1.27, 5.4, 1.55, 0.6);
                self.body_on = true;
                self.body_circle = false;
                self.body_mark = BodyMark::Dot;
                self.fit_body();
                self.tab = FormTab::Pins;
            }
            PartType::Transistor => {
                self.prefix = "Q".into();
                self.pkg = PkgKind::Row;
                (self.pitch, self.pad_d, self.drill) = (2.54, 1.5, 0.8);
                self.body_on = false;
                self.set_transistor("npn");
                self.tab = FormTab::Pins;
            }
            PartType::Diode => {
                self.prefix = "D".into();
                self.glyph = "diode".into();
                self.pins_text = "1 K right\n2 A left".into();
                self.pkg = PkgKind::Row;
                (self.pitch, self.pad_d, self.drill) = (7.62, 1.8, 0.9);
                self.body_on = true;
                self.body_circle = false;
                (self.body_w, self.body_h, self.body_x, self.body_y) = (3.8, 1.8, 0.0, 0.0);
                self.body_mark = BodyMark::Bar;
                self.body_pin = "1".into();
                self.tab = FormTab::Pins;
            }
            PartType::Capacitor => {
                self.set_capacitor("ceramic");
                self.tab = FormTab::Basic;
            }
            PartType::Resistor | PartType::Inductor => {
                self.set_resistor_or_inductor(t, "chip");
                self.tab = FormTab::Basic;
            }
            PartType::Connector => {
                self.prefix = "J".into();
                self.glyph.clear();
                self.pins_text = "1 1\n2 2\n3 3\n4 4".into();
                self.pkg = PkgKind::Row;
                (self.pitch, self.pad_d, self.drill) = (2.54, 1.7, 1.0);
                self.body_on = true;
                self.body_circle = false;
                (self.body_w, self.body_h, self.body_x, self.body_y) = (10.16, 2.5, 0.0, 0.0);
                self.body_mark = BodyMark::Dot;
                self.tab = FormTab::Pins;
            }
        }
    }

    /// Starting points for the kinds of capacitor: the footprint, the polarity, the height and the name prefix.
    pub(super) fn set_capacitor(&mut self, kind: &'static str) {
        self.ptype = Some(PartType::Capacitor);
        self.ptype_sub = kind;
        self.pas_kind = PassiveKind::Capacitor;
        self.prefix = "C".into();
        self.pas_polar = matches!(kind, "electrolytic" | "tantalum");
        self.glyph = if self.pas_polar { "electrolytic" } else { "capacitor" }.into();
        self.pins_text = if self.pas_polar { "1 + left\n2 - right" } else { "1 1 left\n2 2 right" }.into();
        self.body_on = false;
        self.thermal_on = false;
        self.extras.clear();
        match kind {
            "ceramic" => {
                self.fp_preset.clear();
                self.pkg = PkgKind::Chip2;
                (self.pitch, self.len, self.width) = (1.65, 0.8, 0.95);
                self.height = 0.0;
            }
            "electrolytic" => {
                self.fp_preset = "CP_Radial_D5_P2.0".into();
                self.height = 11.0;
            }
            "tantalum" => {
                self.fp_preset = "CP_Radial_D5_P2.0".into();
                self.height = 7.0;
            }
            _ => {
                self.fp_preset = "C_Disc_5.08".into();
                self.height = 9.0;
            }
        }
        let head = match kind {
            "electrolytic" => "CE_",
            "tantalum" => "CT_",
            "film" => "CF_",
            _ => "C_",
        };
        if self.name.trim().is_empty() {
            self.name = head.into();
        } else if let Some(rest) = ["CE_", "CT_", "CF_", "C_"].iter().find_map(|h| self.name.strip_prefix(h)) {
            self.name = format!("{head}{rest}");
        }
    }

    pub(super) fn set_resistor_or_inductor(&mut self, t: PartType, shape: &'static str) {
        let resistor = t == PartType::Resistor;
        self.ptype = Some(t);
        self.ptype_sub = shape;
        self.pas_kind = if resistor { PassiveKind::Resistor } else { PassiveKind::Inductor };
        self.prefix = if resistor { "R" } else { "L" }.into();
        self.glyph = if resistor { "resistor" } else { "inductor" }.into();
        self.pins_text = "1 1 left\n2 2 right".into();
        self.pas_polar = false;
        self.body_on = false;
        self.thermal_on = false;
        self.extras.clear();
        if shape == "chip" {
            self.fp_preset.clear();
            self.pkg = PkgKind::Chip2;
            (self.pitch, self.len, self.width) = if resistor { (1.65, 0.8, 0.95) } else { (1.825, 1.025, 1.4) };
            self.height = 0.0;
        } else {
            self.fp_preset = "R_Axial_10.16".into();
            self.height = if resistor { 2.4 } else { 5.0 };
        }
    }

    pub(super) fn set_transistor(&mut self, kind: &str) {
        self.glyph = kind.into();
        // pin numbers 1 2 3 along the row; the sides decide where each lead leaves the symbol
        self.pins_text = match kind {
            "npn" => "1 E bottom\n2 B left\n3 C top",
            "pnp" => "1 E top\n2 B left\n3 C bottom",
            "nmos" => "1 S bottom\n2 G left\n3 D top",
            _ => "1 S top\n2 G left\n3 D bottom",
        }
        .into();
    }

    /// A body that fits the pads of a dual-row IC (the data sheet's outline is the value to type; this is a start).
    pub(super) fn fit_body(&mut self) {
        if self.pkg == PkgKind::Dual {
            let n = self.pins_text.lines().filter(|l| !l.trim().is_empty()).count().max(2);
            self.body_w = (self.row - 1.5).max(1.0);
            self.body_h = self.pitch * (n / 2).saturating_sub(1) as f64 + 1.1;
            self.body_x = 0.0;
            self.body_y = 0.0;
        }
    }
}
