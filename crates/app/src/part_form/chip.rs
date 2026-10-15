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

    /// Fit ESR and ESL to the impedance table typed in the form.
    pub(super) fn fit_from_table(&mut self) -> Result<String, String> {
        let c = self.pas_number("容量", &self.pas_value, false)?.ok_or("先に上の「値」に容量を入れてください (表の容量。DCバイアス時の表ならそのときの容量)")?;
        let mut pts = vec![];
        for (n, line) in self.pas_table.lines().enumerate().filter(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with('#')) {
            let t: Vec<&str> = line.split(|c: char| c.is_whitespace() || c == ',' || c == '|').filter(|s| !s.is_empty()).collect();
            let (f, z) = match (t.first().and_then(|s| parse_freq(s)), t.get(1).and_then(|s| parse_ohm(s))) {
                (Some(f), Some(z)) => (f, z),
                _ => return Err(format!("インピーダンスの表 {}行目: 「周波数 インピーダンス」の形で入力してください (例: 1M 0.05)", n + 1)),
            };
            pts.push((f, z));
        }
        let fit = passive::fit_cap(c, &pts)?;
        self.pas_esr = passive::eng(fit.esr);
        self.pas_esl = passive::eng(fit.esl);
        let mut m = format!("ESR = {}Ω、ESL = {}H と求めました (共振 {}Hz、表との差は最大 {:.0}%)。上の欄に入れたので、「部品を作る」を押すとモデルに入ります", self.pas_esr, self.pas_esl, passive::eng(fit.srf), fit.max_error_pct);
        for w in &fit.warnings {
            m.push_str(&format!("\n注意: {w}"));
        }
        Ok(m)
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
            if kind == PassiveKind::Capacitor {
                ui.label("インピーダンスの表 (データシートの周波数特性。1行に「周波数 インピーダンス」。周波数の M は MHz)");
                ui.add(egui::TextEdit::multiline(&mut self.pas_table).desired_rows(4).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace).hint_text("100k 0.2\n1M 0.03\n10M 0.05\n100M 0.4"))
                    .on_hover_text("データシートのグラフから、いくつかの点を読み取って書きます。谷 (一番低い所) の前後も入れると確かです。例: 周波数 1M、インピーダンス 0.05 は「1M 0.05」");
                if ui.button("表から ESR と ESL を求める").on_hover_text("表に合うように、ESR と ESL を計算して上の欄に入れます。「値」の容量を使います").clicked() {
                    self.pas_message = Some(self.fit_from_table());
                }
            }
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
