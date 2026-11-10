use super::*;

impl PartForm {
    /// The first question: what kind of part is this. Fills pins, package, body and symbol with a starting point.
    pub(super) fn kind_bar(&mut self, ui: &mut egui::Ui) {
        // an opened part keeps what it has: the kinds only give a new part its starting values
        if self.editing().is_some() {
            return;
        }
        if self.type_base.is_none() {
            self.type_base = Some(if self.ptype.is_some() { self.fingerprint() } else { PartForm::default().fingerprint() });
        }
        crate::widgets::section_heading(ui, "部品の種類");
        let items: Vec<(&str, bool)> = PART_TYPES.iter().map(|(_, label, _)| (*label, true)).collect();
        let bar = crate::widgets::segment_bar(ui, &items, PART_TYPES.iter().position(|(t, _, _)| self.ptype == Some(*t)));
        for (n, r) in bar.iter().enumerate() {
            r.clone().on_hover_text(i18n::tr(PART_TYPES[n].2));
            if r.clicked() {
                let t = PART_TYPES[n].0;
                if self.type_base.as_deref() == Some(self.fingerprint().as_str()) {
                    self.apply_type(t);
                    self.type_base = Some(self.fingerprint());
                } else {
                    self.pending_type = Some(t);
                }
            }
        }
        if let Some(t) = self.pending_type {
            let name = PART_TYPES.iter().find(|(k, _, _)| *k == t).map_or("", |(_, l, _)| *l);
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(crate::theme::WARN, i18n::trf!("「{name}」にすると、入力したピン表・パッケージ・本体などが、{name}の初期値に置き換わります。"));
                if ui.button(i18n::tr("置き換える")).on_hover_text(i18n::tr("入力した内容を捨てて、選んだ種類の初期値にします")).clicked() {
                    self.apply_type(t);
                    self.type_base = Some(self.fingerprint());
                    self.pending_type = None;
                }
                if ui.button(i18n::tr("やめる")).on_hover_text(i18n::tr("いまの入力をそのまま残します")).clicked() {
                    self.pending_type = None;
                }
            });
        }
        match self.ptype {
            Some(PartType::Transistor) => {
                ui.label(i18n::tr("どの種類?"));
                let options = [
                        ("npn", "NPN", "ベース電流でコレクタ→エミッタの電流を制御する。負荷を GND 側で切り替える"),
                        ("pnp", "PNP", "NPN と電流の向きが逆。電源側で切り替える"),
                        ("nmos", "Nch MOSFET", "ゲート電圧で制御。大電流のスイッチに向く"),
                        ("pmos", "Pch MOSFET", "N チャネルと極性が逆。電源側で切り替える"),
                    ];
                let items: Vec<(&str, bool)> = options.iter().map(|(_, label, _)| (*label, true)).collect();
                let bar = crate::widgets::segment_bar(ui, &items, options.iter().position(|(k, _, _)| self.glyph == *k));
                for (n, r) in bar.iter().enumerate() {
                    r.clone().on_hover_text(i18n::tr(options[n].2));
                    if r.clicked() {
                        let k = options[n].0;
                        self.set_transistor(k);
                    }
                }
            }
            Some(PartType::Diode) => {
                ui.label(i18n::tr("どの種類?"));
                let options = [
                        ("diode", "整流・スイッチング", "電流を一方向に流す。記号は三角と線"),
                        ("schottky", "ショットキー", "順方向の電圧降下が小さい。棒の両端が S 形の記号"),
                        ("zener", "ツェナー", "逆向きで電圧を一定に保つ。棒の両端が Z 形の記号"),
                    ];
                let items: Vec<(&str, bool)> = options.iter().map(|(_, label, _)| (*label, true)).collect();
                let bar = crate::widgets::segment_bar(ui, &items, options.iter().position(|(k, _, _)| self.glyph == *k));
                for (n, r) in bar.iter().enumerate() {
                    r.clone().on_hover_text(i18n::tr(options[n].2));
                    if r.clicked() {
                        let k = options[n].0;
                        self.glyph = k.into();
                    }
                }
            }
            Some(PartType::Capacitor) => {
                ui.label(i18n::tr("どの種類?"));
                let options = [
                        ("ceramic", "積層セラミック", "チップ部品。小さくて高い周波数に強い。極性なし。電圧がかかると容量が減ります"),
                        ("electrolytic", "電解", "リード付きの円筒形。容量が大きく、電源の平滑に使う。極性があり、ESR は大きめ"),
                        ("tantalum", "タンタル", "小さくて容量が大きい。極性があり、耐圧に余裕を見ないと壊れます"),
                        ("film", "フィルム", "リード付きの箱形。極性なし。ESR・ESL が小さく、容量が安定します"),
                    ];
                let items: Vec<(&str, bool)> = options.iter().map(|(_, label, _)| (*label, true)).collect();
                let bar = crate::widgets::segment_bar(ui, &items, options.iter().position(|(k, _, _)| self.ptype_sub == *k));
                for (n, r) in bar.iter().enumerate() {
                    r.clone().on_hover_text(i18n::tr(options[n].2));
                    if r.clicked() {
                        let k = options[n].0;
                        self.set_capacitor(k);
                    }
                }
            }
            Some(t @ (PartType::Resistor | PartType::Inductor)) => {
                ui.label(i18n::tr("どの形?"));
                let options = [("chip", "チップ", "面実装の小さい部品 (0603・0805 など)"), ("leaded", "リード付き", "足を基板の穴に通す部品")];
                let items: Vec<(&str, bool)> = options.iter().map(|(_, label, _)| (*label, true)).collect();
                let bar = crate::widgets::segment_bar(ui, &items, options.iter().position(|(k, _, _)| self.ptype_sub == *k));
                for (n, r) in bar.iter().enumerate() {
                    r.clone().on_hover_text(i18n::tr(options[n].2));
                    if r.clicked() {
                        let k = options[n].0;
                        self.set_resistor_or_inductor(t, k);
                    }
                }
            }
            _ => {}
        }
    }

    /// The fields a kind of part fills in, as one text: two equal texts mean nothing was typed in between.
    pub(super) fn fingerprint(&self) -> String {
        format!(
            "{:?}",
            (
                (&self.name, &self.category, &self.prefix, &self.value, &self.description, &self.pins_text, &self.glyph, &self.extras),
                (self.pkg as u8, self.pitch, self.row, self.span, self.len, self.width, self.pad_d, self.drill, self.tht),
                (self.thermal_on, &self.thermal_pin, self.thermal_w, self.thermal_h),
                (self.body_on, self.body_circle, self.body_w, self.body_h, self.body_x, self.body_y, self.height, &self.body_pin),
            )
        )
    }

    /// Shows one tab ("basic", "ratings", "sim", "pins", "package"), for scripts and screenshots.
    pub fn show_tab(&mut self, name: &str) {
        self.tab = match name {
            "basic" => FormTab::Basic,
            "ratings" => FormTab::Basic,
            "sim" => FormTab::Sim,
            "pins" => FormTab::Pins,
            "package" => FormTab::Package,
            _ => return,
        };
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
            PartType::Connector => by_name("コネクタ"),
            PartType::Capacitor | PartType::Resistor | PartType::Inductor => by_name("そのほか"),
        }
    }

    /// A form that starts from this kind of part ("ic", "transistor", "diode", "chip", "connector"), for scripts and screenshots.
    pub fn for_type(name: &str) -> Self {
        let mut f = PartForm::default();
        // a kind of capacitor by its own name ("chip" is the small ceramic one)
        let name = if name == "chip" { "ceramic" } else { name };
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
        self.body_fit = None;
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
            }
            PartType::Transistor => {
                self.prefix = "Q".into();
                self.pkg = PkgKind::Row;
                (self.pitch, self.pad_d, self.drill) = (2.54, 1.5, 0.8);
                self.body_on = false;
                self.set_transistor("npn");
            }
            PartType::Diode => {
                self.prefix = "D".into();
                self.glyph = "diode".into();
                self.pins_text = "1 K right\n2 A left".into();
                self.pkg = PkgKind::Row;
                (self.pitch, self.pad_d, self.drill) = (7.62, 1.8, 0.9);
                self.body_on = true;
                self.body_circle = false;
                self.row_body(3.8, 1.8);
                self.body_mark = BodyMark::Bar;
                self.body_pin = "1".into();
            }
            PartType::Capacitor => {
                self.set_capacitor("ceramic");
            }
            PartType::Resistor | PartType::Inductor => {
                self.set_resistor_or_inductor(t, "chip");
            }
            PartType::Connector => {
                self.prefix = "J".into();
                self.glyph.clear();
                // one side, in order, like the header itself (the automatic sides split 1 2 | 3 4 across the box)
                self.pins_text = "1 1 left\n2 2 left\n3 3 left\n4 4 left".into();
                self.pkg = PkgKind::Row;
                (self.pitch, self.pad_d, self.drill) = (2.54, 1.7, 1.0);
                self.body_on = true;
                self.body_circle = false;
                self.span_body(2.5);
                self.body_mark = BodyMark::Dot;
            }
        }
    }

    /// Starting points for the kinds of capacitor: the footprint, the polarity, the height and the name prefix.
    pub(super) fn set_capacitor(&mut self, kind: &'static str) {
        self.ptype = Some(PartType::Capacitor);
        self.ptype_sub = kind;
        self.pas_kind = PassiveKind::Capacitor;
        self.body_fit = None;
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
        self.body_fit = None;
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

    /// How many pads the pin table gives.
    fn pad_count(&self) -> usize {
        self.pins_text.lines().filter(|l| !l.trim().is_empty()).count()
    }

    /// The body (w, h, x, y) a fit gives for the pads as they are now; None when the package is not the one the fit is for.
    /// A single row runs from pad 1 at the origin down -y (`PackageSpec::SingleRow`), so its body is long in y and
    /// centred on the middle of the row, not on pad 1.
    pub(super) fn body_for(&self, fit: BodyFit) -> Option<[f64; 4]> {
        let n = self.pad_count().max(1);
        let middle = -((n - 1) as f64) * self.pitch / 2.0;
        match fit {
            BodyFit::Dual if self.pkg == PkgKind::Dual => {
                let rows = (n.max(2) / 2).saturating_sub(1);
                Some([(self.row - 1.5).max(1.0), self.pitch * rows as f64 + 1.1, 0.0, 0.0])
            }
            BodyFit::Span(across) if self.pkg == PkgKind::Row => Some([across, n as f64 * self.pitch, 0.0, middle]),
            BodyFit::Middle(along, across) if self.pkg == PkgKind::Row => Some([across, along, 0.0, middle]),
            _ => None,
        }
    }

    fn set_body_fit(&mut self, fit: BodyFit) {
        if let Some(v) = self.body_for(fit) {
            [self.body_w, self.body_h, self.body_x, self.body_y] = v;
            self.body_fit = Some((fit, v));
        }
    }

    /// A body that fits the pads of a dual-row IC (the data sheet's outline is the value to type; this is a start).
    pub(super) fn fit_body(&mut self) {
        self.set_body_fit(BodyFit::Dual);
    }

    /// A body of a fixed size (`along` the row x `across` it) in the middle of a single row: a diode between its holes.
    pub(super) fn row_body(&mut self, along: f64, across: f64) {
        self.set_body_fit(BodyFit::Middle(along, across));
    }

    /// A body over the whole single row, `across` wide: a pin header.
    pub(super) fn span_body(&mut self, across: f64) {
        self.set_body_fit(BodyFit::Span(across));
    }

    /// Keeps a fitted body on the pads when the pins or the pitch change (a 4-pin IC made 8-pin had its pads outside
    /// the outline). A body whose numbers were typed, or that was made round, is left as it is.
    pub(super) fn follow_body(&mut self) {
        let Some((fit, last)) = self.body_fit else { return };
        if self.body_circle || [self.body_w, self.body_h, self.body_x, self.body_y] != last {
            self.body_fit = None;
            return;
        }
        if let Some(v) = self.body_for(fit).filter(|v| *v != last) {
            [self.body_w, self.body_h, self.body_x, self.body_y] = v;
            self.body_fit = Some((fit, v));
        }
    }
}
