use super::*;

impl PartForm {
    pub(super) fn pas_number(&self, label: &str, t: &str, freq: bool) -> Result<Option<f64>, String> {
        let t = t.trim();
        if t.is_empty() {
            return Ok(None);
        }
        let parsed = if freq { parse_freq(t) } else { parse_ohm(t) };
        parsed.filter(|v| *v > 0.0).map(Some).ok_or_else(|| format!("{label}「{t}」を、0 より大きい数で入力してください (例: 20m / 1n / 30M)"))
    }

    /// Fill the form from the chip part section: pins, symbol, value and a `.subckt` with the parasitics.
    pub(super) fn apply_passive(&mut self) -> Result<String, String> {
        let value = self.pas_number(self.pas_kind.label(), &self.pas_value, false)?.ok_or("値を入力してください (例: 100n / 4.7k / 10u)")?;
        let mut t = PassiveTemplate::new(self.pas_kind, value);
        t.esr = self.pas_number("ESR", &self.pas_esr, false)?;
        t.esl = self.pas_number("ESL", &self.pas_esl, false)?;
        t.dcr = self.pas_number("DCR", &self.pas_dcr, false)?;
        t.cp = self.pas_number("並列容量", &self.pas_cp, false)?;
        let mut notes = vec![];
        if let Some(f0) = self.pas_number("自己共振周波数", &self.pas_srf, true)? {
            match t.kind {
                PassiveKind::Capacitor if t.esl.is_none() => {
                    let esl = passive::esl_from_srf(value, f0)?;
                    t.esl = Some(esl);
                    notes.push(format!("ESL は自己共振周波数から {} と求めました", passive::eng(esl)));
                }
                PassiveKind::Inductor | PassiveKind::Resistor if t.cp.is_none() => {
                    let cp = passive::cp_from_srf(value, f0)?;
                    t.cp = Some(cp);
                    notes.push(format!("並列容量は自己共振周波数から {} と求めました", passive::eng(cp)));
                }
                _ => {}
            }
        }
        let bias = self.pas_bias.trim().trim_end_matches('%').trim();
        if !bias.is_empty() {
            t.bias_loss = bias.parse::<f64>().map_err(|_| format!("DCバイアスで下がる量「{bias}」を % の数で入力してください (例: 30)"))? / 100.0;
        }
        if t.kind == PassiveKind::Inductor {
            t.isat = self.pas_number("飽和電流 Isat", &self.pas_isat, false)?;
            if let Some(d) = self.pas_number("インダクタンスの低下", self.pas_ldrop.trim().trim_end_matches('%'), false)? {
                t.ldrop = d / 100.0;
            }
        }
        t.tc1 = self.pas_ppm("温度係数 TC1", &self.pas_tc1)?;
        t.tc2 = self.pas_ppm("温度係数 TC2", &self.pas_tc2)?;
        let size = self.fp_preset.rsplit('_').next().filter(|_| !self.fp_preset.is_empty()).map(|s| format!("_{s}")).unwrap_or_default();
        let name = if self.name.trim().is_empty() { format!("{}_{}{size}", t.kind.prefix(), passive::eng(value)) } else { self.name.trim().to_string() };
        let g = t.generate(&name)?;
        self.name = name;
        self.prefix = t.kind.prefix().into();
        self.value = g.value_text;
        self.description = g.description;
        if self.category.trim().is_empty() || self.category == "ユーザー" {
            self.category = t.kind.label().into();
        }
        self.pins_text = if self.pas_polar && t.kind == PassiveKind::Capacitor { "1 + left\n2 - right" } else { "1 1 left\n2 2 right" }.into();
        self.sim = SimChoice::Subckt;
        self.sim_name = g.sim_name;
        self.sim_text = g.sim_text;
        self.sim_pins = g.sim_pins.join(" ");
        self.sim_limits = g.limits.join("\n");
        self.glyph = if self.pas_polar && t.kind == PassiveKind::Capacitor { "electrolytic" } else { t.kind.glyph() }.into();
        if self.fp_preset.is_empty() {
            self.pkg = PkgKind::Chip2;
        }
        let tail = if notes.is_empty() { String::new() } else { format!("。{}", notes.join("。")) };
        Ok(format!("ピンとモデルを作りました{tail}。このあと、パッケージを選び (0603 など)、定格 (耐圧など) をデータシートから入れて保存します"))
    }

    /// The chip part section. With a kind chosen at the top (capacitor, resistor, coil) it is shown open under its own
    /// heading and the kind is fixed; with no kind chosen it is a folded section with its own kind selector.
    pub(super) fn passive_ui(&mut self, ui: &mut egui::Ui) {
        if self.ptype.is_some() {
            ui.label(egui::RichText::new(format!("かんたん作成: {}をデータシートの数値からつくる", self.pas_kind.label())).strong());
            self.passive_body(ui);
        } else {
            egui::CollapsingHeader::new("チップ部品 (コンデンサ・抵抗・コイル) を、データシートの数値からつくる").id_salt("pas_section").default_open(!self.glyph.is_empty()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    for k in [PassiveKind::Capacitor, PassiveKind::Resistor, PassiveKind::Inductor] {
                        ui.selectable_value(&mut self.pas_kind, k, k.label());
                    }
                });
                self.passive_body(ui);
            });
        }
    }

    fn passive_body(&mut self, ui: &mut egui::Ui) {
        {
            ui.small("容量などの値に、データシートにある ESR・ESL・DCR などを足すと、実物に近いモデルになります。空の欄は入れません。値だけでも作れます");
            let kind = self.pas_kind;
            egui::Grid::new("pas_grid").num_columns(2).show(ui, |ui| {
                let row = |ui: &mut egui::Ui, label: &str, text: &mut String, tip: &str, hint: &str| {
                    ui.label(label).on_hover_text(tip);
                    ui.add(egui::TextEdit::singleline(text).desired_width(120.0).hint_text(hint)).on_hover_text(tip);
                    ui.end_row();
                };
                row(ui, &format!("値 ({})", kind.unit()), &mut self.pas_value, "コンデンサは容量 (例: 100n、4.7u)、抵抗は抵抗値 (例: 10k)、コイルはインダクタンス (例: 10u)", "100n");
                match kind {
                    PassiveKind::Capacitor => {
                        row(ui, "ESR (等価直列抵抗)", &mut self.pas_esr, "データシートの ESR か、インピーダンスの最小値。大きいほどリップルが大きく、発熱します。下の表から求めることもできます", "20m");
                        // for an electrolytic / tantalum only: there the impedance at 100 kHz is about the ESR
                        if self.pas_polar {
                            ui.label("インピーダンス Z (Ω)").on_hover_text("データシートの「インピーダンス」(100kHz の値) を入れて「ESR に入れる」を押すと、上の ESR に写します。電解コンデンサでは、100kHz のインピーダンスはほぼ ESR です (容量の分は、そのころには小さくなっています)。ただし ESR は周波数と温度で大きく変わります (120Hz の tanδ から求めると、もっと大きくなります)");
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(&mut self.pas_z100k).desired_width(80.0).hint_text("0.3"));
                                if ui.small_button("→ ESR に入れる").on_hover_text("この値を ESR の欄に入れます (100kHz のときの値として)").clicked() {
                                    match self.pas_number("インピーダンス", &self.pas_z100k, false) {
                                        Ok(Some(z)) => self.pas_esr = passive::eng(z),
                                        Ok(None) => {}
                                        Err(e) => self.pas_message = Some(Err(e)),
                                    }
                                }
                            });
                            ui.end_row();
                        }
                        row(ui, "ESL (等価直列インダクタンス)", &mut self.pas_esl, "高い周波数で効いてくる直列のインダクタンス。小さいチップほど小さい (0603 で 0.5〜1nH ほど)。自己共振周波数から求めることもできます", "800p");
                        row(ui, "自己共振周波数 (Hz)", &mut self.pas_srf, "容量と ESL が共振する周波数。ESL が空のとき、ここから ESL を求めます。M は MHz (例: 30M)", "30M");
                        row(ui, "DCバイアスで下がる量 (%)", &mut self.pas_bias, "セラミックコンデンサは、直流の電圧をかけると容量が減ります。使う電圧での値をデータシートの「DCバイアス特性」の曲線から読み、減った割合を入れると、その分小さい容量で計算します", "30");
                    }
                    PassiveKind::Resistor => {
                        row(ui, "寄生インダクタンス (H)", &mut self.pas_esl, "高い周波数で効く直列のインダクタンス。チップ抵抗で 1〜2nH ほど", "2n");
                        row(ui, "並列容量 (F)", &mut self.pas_cp, "両端の間の容量。チップ抵抗で 0.05〜0.2pF ほど", "100f");
                    }
                    PassiveKind::Inductor => {
                        row(ui, "DCR (巻線抵抗)", &mut self.pas_dcr, "巻線の直流抵抗。損失と発熱の元になります", "100m");
                        row(ui, "並列容量 (F)", &mut self.pas_cp, "巻線の間の容量。自己共振周波数から求めることもできます", "1p");
                        row(ui, "自己共振周波数 (Hz)", &mut self.pas_srf, "コイルがこれより高い周波数でコンデンサのようになる限界。並列容量が空のとき、ここから求めます。M は MHz", "30M");
                    }
                }
            });
            ui.label("インピーダンスの表 (データシートの周波数特性。1行に「周波数 インピーダンス」。周波数の M は MHz)");
            ui.add(egui::TextEdit::multiline(&mut self.pas_table).desired_rows(4).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace).hint_text(match kind {
                PassiveKind::Capacitor => "100k 0.2\n1M 0.03\n10M 0.05\n100M 0.4",
                PassiveKind::Inductor => "100k 6.5\n1M 63\n10M 600\n40M 2.5k\n100M 900",
                PassiveKind::Resistor => "1M 1k\n100M 1.05k\n500M 1.4k\n1G 2k",
            }))
            .on_hover_text("データシートのグラフから、いくつかの点を読み取って書きます。谷 (コンデンサ) や山 (コイル) の前後も入れると確かです。例: 周波数 1M、インピーダンス 0.05 は「1M 0.05」");
            if ui.button("表から求める").on_hover_text("表に合うように、上の 2 つの値 (ESR と ESL / DCR と並列容量 / 寄生インダクタンスと並列容量) を計算して入れます。「値」を使います").clicked() {
                self.pas_message = Some(self.fit_from_table());
            }
            self.pas_chart(ui);
            if kind == PassiveKind::Inductor {
                self.pas_saturation(ui);
            }
            self.pas_temperature(ui);
            if ui.button("部品を作る (ピンとモデルを入れる)").on_hover_text("ピン、記号、値、モデルを入れます。パッケージは下の「パッケージ」で選びます").clicked() {
                self.pas_message = Some(self.apply_passive());
            }
            match &self.pas_message {
                Some(Ok(m)) => {
                    ui.colored_label(Color32::from_rgb(60, 170, 90), m);
                }
                Some(Err(e)) => {
                    ui.colored_label(Color32::from_rgb(230, 60, 60), e);
                }
                None => {}
            }
        }
    }
}
