//! "ADC で測る量の表" (candidate L1): what the ADCs measure, one way per row, kept with the schematic (`Schematic::adc_measures`),
//! edited row by row and worked out on request (`sim_core::adcmeasure`): per way the ADC's input and code, and the reading's noise
//! and stable digits in the measured unit. A meter's ranges, a sensor's reading, a current through a shunt, a multiplexer's
//! channels are all rows of it. Each row also gets its error estimate (the resistors' tolerances and temperature coefficients,
//! the leaks of the other parts), and a temperature sensor (an NTC, a resistance thermometer) reads in °C.

use super::*;
use sch_core::adc_measure::{parse_conditions, AdcMeasure, Measured, DEFAULT_DTEMP, DEFAULT_TC_PPM, DEFAULT_TOL_PCT};
use sim_core::adcmeasure::{read_measure, MeasureErrors, MeasureReading};

/// The window's state: open, and the last results with the schematic's edit count they were worked out at.
#[derive(Default)]
pub(super) struct AdcMeasureUi {
    pub open: bool,
    results: Vec<(String, Result<MeasureReading, String>)>,
    rev: Option<u64>,
}

const GAINS: [f64; 8] = [1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0];

/// The kinds of thing measured, as the table's menu names them.
const KINDS: [(&str, &str); 5] = [("volts", "電圧"), ("ohms", "抵抗"), ("amps", "電流"), ("part", "部品の値"), ("temp", "温度センサー")];

fn kind_of(m: &Measured) -> &'static str {
    match m {
        Measured::Volts { .. } => "volts",
        Measured::Ohms { .. } => "ohms",
        Measured::Amps { .. } => "amps",
        Measured::Part { .. } => "part",
        Measured::Temp { .. } => "temp",
    }
}

/// A value with its unit, signed.
fn signed(x: f64, unit: &str) -> String {
    format!("{}{}{unit}", if x < 0.0 { "−" } else { "+" }, sim_core::format_value(x.abs()))
}

/// The error cell: the tolerances, the temperature coefficients (each as their root of the sum of squares) and the leaks.
fn error_cell(e: &MeasureErrors, unit: &str) -> String {
    format!("±{}{unit} / ±{}{unit} / {}", sim_core::format_value(e.tol_rss), sim_core::format_value(e.temp_rss), if e.leaks.is_empty() { "—".to_string() } else { signed(e.leak_sum, unit) })
}

/// The error cell's hover: every resistor and leak.
fn error_detail(e: &MeasureErrors, unit: &str, dtemp: f64) -> String {
    let mut t = i18n::trf!(
        "公差 (校正の前): 合計 ±{}{unit}・2 乗和 ±{}{unit}\n温度係数 (±{} K): 合計 ±{}{unit}・2 乗和 ±{}{unit}\n",
        sim_core::format_value(e.tol_sum),
        sim_core::format_value(e.tol_rss),
        dtemp,
        sim_core::format_value(e.temp_sum),
        sim_core::format_value(e.temp_rss)
    )
    .to_string();
    for p in &e.parts {
        let whose = if p.own { "" } else { i18n::tr(" (表の値)") };
        t += &i18n::trf!("  {}: ±{} %・{} ppm/K{whose} → {} / {}\n", p.part, p.tol * 100.0, p.tc_ppm, signed(p.tol_err, unit), signed(p.temp_err, unit));
    }
    if e.leaks.is_empty() {
        t += i18n::tr("漏れ: 読みに効く部品はありません");
    } else {
        t += &i18n::trf!("漏れ (その部品が無いときとの差): 合計 {}\n", signed(e.leak_sum, unit));
        for l in &e.leaks {
            t += &format!("  {}: {}\n", l.part, signed(l.err, unit));
        }
    }
    t.trim_end().to_string()
}

/// `m` turned into kind `k`, keeping the nets (or the part) where they fit.
fn with_kind(m: &Measured, k: &str) -> Measured {
    let (plus, minus) = match m {
        Measured::Volts { plus, minus } | Measured::Ohms { plus, minus } | Measured::Amps { plus, minus } => (plus.clone(), minus.clone()),
        Measured::Part { .. } | Measured::Temp { .. } => (String::new(), "GND".into()),
    };
    match k {
        "ohms" => Measured::Ohms { plus, minus },
        "amps" => Measured::Amps { plus, minus },
        "part" => Measured::Part { part: String::new() },
        "temp" => Measured::Temp { part: String::new(), r0: 10e3, b: 3950.0, alpha: 0.0 },
        _ => Measured::Volts { plus, minus },
    }
}

impl SchEditor {
    /// A new row from the ADC's setup chosen in the part's settings (or its default).
    fn new_adc_measure(&self) -> Option<AdcMeasure> {
        let (adc, _, pins) = sim_core::adcread::adc_parts(&self.sch).into_iter().next()?;
        let s = self.adc_setups.get(&adc).cloned().unwrap_or_else(|| sim_core::adcread::default_setup(&pins));
        Some(AdcMeasure {
            name: i18n::trf!("測り方 {}", self.sch.adc_measures.len() + 1).to_string(),
            measured: Measured::Volts { plus: String::new(), minus: "GND".into() },
            at: 1.0,
            conditions: vec![],
            adc,
            pos: s.pos,
            neg: s.neg,
            ref_pos: String::new(),
            ref_neg: String::new(),
            gain: s.gain,
            rate: s.rate,
            low_latency: s.filter == sim_core::adcread::AdcFilter::LowLatency,
            chop: s.chop,
            tol_pct: self.sch.adc_measures.first().map_or(DEFAULT_TOL_PCT, |m| m.tol_pct),
            tc_ppm: self.sch.adc_measures.first().map_or(DEFAULT_TC_PPM, |m| m.tc_ppm),
            dtemp: self.sch.adc_measures.first().map_or(DEFAULT_DTEMP, |m| m.dtemp),
        })
    }

    pub(super) fn adc_measure_window(&mut self, ctx: &egui::Context) {
        if !self.adc_measure_ui.open {
            return;
        }
        let mut open = true;
        let mut run = false;
        let parts = sim_core::adcread::adc_parts(&self.sch);
        let flat = self.sch.flat();
        let nets: Vec<String> = sch_core::extract(&flat).into_iter().map(|n| n.name).filter(|n| !n.starts_with('$') && !n.starts_with("N$")).collect();
        // the parts whose value can be moved (a sensor, a bridge's arm): resistors and sources
        let valued: Vec<String> = flat.components.iter().filter(|c| matches!(c.kind, Kind::Resistor | Kind::VSource | Kind::ISource)).map(|c| c.refdes.clone()).collect();
        // the parts a temperature sensor can be: two-pin parts drawn as a resistor
        let sensors: Vec<String> = flat.components.iter().filter(|c| c.kind == Kind::Resistor || c.custom.as_ref().is_some_and(|x| x.glyph == "resistor" && x.pins.len() == 2)).map(|c| c.refdes.clone()).collect();
        let sch = self.sch.clone();
        // the parts a condition can switch: latching relays and switches
        let switching: Vec<String> = flat
            .components
            .iter()
            .filter(|c| c.custom.as_ref().and_then(|s| s.sim.as_ref()).is_some_and(|m| {
                let t = m.text.to_ascii_lowercase();
                t.contains(" setp ") || t.lines().any(|l| l.trim_start().starts_with("vpress") || l.trim_start().starts_with("vpos"))
            }))
            .map(|c| c.refdes.clone())
            .collect();
        let names: std::collections::BTreeMap<String, String> = flat.components.iter().filter_map(|c| Some((c.refdes.clone(), c.custom.as_ref()?.name.clone()))).collect();
        let fresh = self.adc_measure_ui.rev == Some(self.rev);
        egui::Window::new(i18n::tr("ADC で測る量の表")).open(&mut open).default_pos(crate::widgets::beside_panel()).default_width(1350.0).show(ctx, |ui| {
            ui.label(egui::RichText::new(i18n::tr("ADC で何をどう測るかを 1 行ずつ書きます (メーターのレンジ、センサーの読み、シャントで測る電流、マルチプレクサのチャンネルなど)。測る物 (2 つのネットの間につなぐ電圧・抵抗・電流か、回路図にある部品の値)・試す値・条件・ADC の入力と利得を入れ、「計算する」で、DC 動作点から ADC の読みと、データシートの雑音を測る量に換算した揺れ・止まって見える桁、部品の公差・温度係数・漏れによる誤差を出します。表は回路図と一緒に保存されます")).small());
            if parts.is_empty() {
                ui.colored_label(crate::theme::warn(), i18n::tr("回路図に ADC (モデルに adc_bits のある部品。ADS124S08 など) がありません"));
                return;
            }
            let mut remove: Option<usize> = None;
            egui::ScrollArea::both().max_height(420.0).show(ui, |ui| {
                egui::Grid::new("adc_measure_grid").striped(true).num_columns(15).show(ui, |ui| {
                    for (h, tip) in [
                        ("名前", ""),
                        ("測る物", "2 つのネットの間につなぐ電圧・抵抗・電流か、回路図にある部品 (抵抗・電源) の値 (センサー・ブリッジの片側など)"),
                        ("試す値", "試す値 (V・Ω・A か、その部品の単位)。満量程や、ふだんの値にします。10k・1Meg・100m のように書けます"),
                        ("桁", "試す値で、表示が止まって見える桁の数 (log10(試す値 ÷ p-p の揺れ))"),
                        ("揺れ (p-p)", "データシートの雑音を、測る量の単位に換算した揺れ"),
                        ("誤差 (公差 / 温度 / 漏れ)", "抵抗の公差 (校正の前) と、温度係数 × 温度の幅による誤差 (どちらも部品ごとの 2 乗和)、ほかの部品の漏れ (クランプのダイオード・開いたリレーの接点・ADC の入力電流) の合計。測る量の単位で。カーソルを重ねると部品ごとの内訳"),
                        ("読み", "ADC の入力の電圧とコード"),
                        ("つなぐ所", "測る物をつなぐ + 側と − 側のネット (部品の値のときは、その部品)"),
                        ("条件", "この測り方のときの部品の状態。部品番号だけ (K1・SW2) で、状態を保つリレーはセット、スイッチは押す (切り替えは B 側)。R5=10k・V3=1.2 で部品の値を置き換え (マルチプレクサの選択の電圧など)。カンマで区切ります"),
                        ("入力 +", ""),
                        ("入力 −", ""),
                        ("比の入力", "比で測るときの 2 つ目の入力 (基準抵抗の両端、ブリッジの電源など)。— は比を使わない"),
                        ("利得", ""),
                        ("レート", "データレート (SPS)"),
                    ] {
                        let r = ui.strong(i18n::tr(h));
                        if !tip.is_empty() {
                            r.on_hover_text(i18n::tr(tip));
                        }
                    }
                    ui.label("");
                    ui.end_row();
                    let results = &self.adc_measure_ui.results;
                    for (k, m) in self.sch.adc_measures.iter_mut().enumerate() {
                        let pins = parts.iter().find(|p| p.0 == m.adc).map(|p| p.2.clone()).unwrap_or_default();
                        let pin_name = |n: &str| pins.iter().find(|(p, _)| p == n).map(|x| x.1.clone()).unwrap_or_else(|| "—".into());
                        ui.add_sized([90.0, 20.0], egui::TextEdit::singleline(&mut m.name));
                        let mut kind = kind_of(&m.measured);
                        egui::ComboBox::from_id_salt(("mk", k)).width(70.0).selected_text(i18n::tr(KINDS.iter().find(|x| x.0 == kind).map(|x| x.1).unwrap_or(""))).show_ui(ui, |ui| {
                            for (key, label) in KINDS {
                                ui.selectable_value(&mut kind, key, i18n::tr(label));
                            }
                        });
                        if kind != kind_of(&m.measured) {
                            m.measured = with_kind(&m.measured, kind);
                        }
                        let mut at = sim_core::format_value(m.at);
                        if ui.add(egui::TextEdit::singleline(&mut at).desired_width(55.0)).changed()
                            && let Some(v) = sim_core::parse_value(&at)
                        {
                            m.at = v;
                        }
                        match results.iter().find(|x| x.0 == m.name).map(|x| &x.1).filter(|_| fresh) {
                            Some(Ok(x)) => {
                                ui.label(egui::RichText::new(x.digits.map(|d| format!("{d:.1}")).unwrap_or_else(|| "—".into())).strong());
                                ui.label(x.noise_pp.map(|p| format!("{}{}", sim_core::format_value(p), x.unit)).unwrap_or_else(|| "—".into()));
                                match &x.errors {
                                    Some(e) => {
                                        ui.label(error_cell(e, x.unit)).on_hover_text(error_detail(e, x.unit, m.dtemp));
                                    }
                                    None => {
                                        ui.label("—");
                                    }
                                }
                                let clipped = x.main.clipped || x.reference.as_ref().is_some_and(|q| q.clipped);
                                let text = format!("{}V ({})", sim_core::format_value(x.main.vin), x.main.code);
                                if clipped {
                                    ui.colored_label(crate::theme::warn(), text).on_hover_text(i18n::tr("入力が ±基準電圧 / 利得 を超えて、コードが端に張り付いています"));
                                } else if x.slope == 0.0 {
                                    ui.colored_label(crate::theme::warn(), text).on_hover_text(i18n::tr("測る物を変えても読みが変わりません。つなぐ所・条件・ADC の入力を確かめます"));
                                } else {
                                    ui.label(text);
                                }
                            }
                            Some(Err(e)) => {
                                ui.colored_label(crate::theme::error(), "✖").on_hover_text(i18n::tr_dyn(e));
                                ui.label("");
                                ui.label("");
                                ui.label("");
                            }
                            None => {
                                ui.label("");
                                ui.label("");
                                ui.label("");
                                ui.label("");
                            }
                        }
                        ui.horizontal(|ui| match &mut m.measured {
                            Measured::Volts { plus, minus } | Measured::Ohms { plus, minus } | Measured::Amps { plus, minus } => {
                                for (slot, id) in [(plus, "mp"), (minus, "mm")] {
                                    egui::ComboBox::from_id_salt((id, k)).width(70.0).selected_text(if slot.is_empty() { "—".to_string() } else { slot.clone() }).show_ui(ui, |ui| {
                                        for n in &nets {
                                            ui.selectable_value(slot, n.clone(), n);
                                        }
                                    });
                                }
                            }
                            Measured::Part { part } => {
                                egui::ComboBox::from_id_salt(("mpart", k)).width(146.0).selected_text(if part.is_empty() { "—".to_string() } else { part.clone() }).show_ui(ui, |ui| {
                                    for n in &valued {
                                        ui.selectable_value(part, n.clone(), n);
                                    }
                                });
                            }
                            Measured::Temp { part, r0, b, alpha } => {
                                let before = part.clone();
                                egui::ComboBox::from_id_salt(("mtemp", k)).width(50.0).selected_text(if part.is_empty() { "—".to_string() } else { part.clone() }).show_ui(ui, |ui| {
                                    for n in &sensors {
                                        ui.selectable_value(part, n.clone(), n);
                                    }
                                });
                                // an NTC from the library brings its own R25 and B
                                if *part != before
                                    && let Some((r, bb)) = sim_core::adcmeasure::sensor_law(&sch, part)
                                {
                                    (*r0, *b, *alpha) = (r, bb, 0.0);
                                }
                                let mut r = sim_core::format_value(*r0);
                                if ui.add(egui::TextEdit::singleline(&mut r).desired_width(36.0)).on_hover_text(i18n::tr("基準の抵抗: NTC は 25 °C の値 (R25)、白金などの測温抵抗体は 0 °C の値 (Pt100 は 100)")).changed()
                                    && let Some(v) = sim_core::parse_value(&r)
                                {
                                    *r0 = v;
                                }
                                let mut law = if *alpha != 0.0 { format!("{alpha}") } else { format!("{b}") };
                                if ui.add(egui::TextEdit::singleline(&mut law).desired_width(44.0)).on_hover_text(i18n::tr("NTC の B 定数 (K。3950 など) か、測温抵抗体の温度係数 (1/K。Pt100 は 0.00385)。1 より小さい数は温度係数として読みます。ライブラリの NTC を選ぶと、そのモデルから入ります")).changed()
                                    && let Some(v) = sim_core::parse_value(&law)
                                {
                                    (*b, *alpha) = if v.abs() < 1.0 { (0.0, v) } else { (v, 0.0) };
                                }
                            }
                        });
                        let mut cond = m.conditions.join(", ");
                        let resp = ui.add_sized([110.0, 20.0], egui::TextEdit::singleline(&mut cond));
                        if resp.changed() {
                            m.conditions = parse_conditions(&cond);
                        }
                        resp.on_hover_text(i18n::trf!("状態を変えられる部品: {}", if switching.is_empty() { "—".to_string() } else { switching.join(", ") }));
                        for (slot, id) in [(&mut m.pos, "ip"), (&mut m.neg, "in")] {
                            egui::ComboBox::from_id_salt((id, k)).width(60.0).selected_text(pin_name(slot)).show_ui(ui, |ui| {
                                for (p, n) in &pins {
                                    ui.selectable_value(slot, p.clone(), n);
                                }
                            });
                        }
                        ui.horizontal(|ui| {
                            for (slot, id) in [(&mut m.ref_pos, "qp"), (&mut m.ref_neg, "qn")] {
                                egui::ComboBox::from_id_salt((id, k)).width(55.0).selected_text(pin_name(slot)).show_ui(ui, |ui| {
                                    ui.selectable_value(slot, String::new(), "—");
                                    for (p, n) in &pins {
                                        ui.selectable_value(slot, p.clone(), n);
                                    }
                                });
                            }
                        });
                        egui::ComboBox::from_id_salt(("mg", k)).width(40.0).selected_text(format!("{}", m.gain)).show_ui(ui, |ui| {
                            for g in GAINS {
                                ui.selectable_value(&mut m.gain, g, format!("{g}"));
                            }
                        });
                        let rates = sim_core::adcread::noise_rates(names.get(&m.adc).map(String::as_str).unwrap_or(""));
                        egui::ComboBox::from_id_salt(("mr", k)).width(50.0).selected_text(format!("{}", m.rate)).show_ui(ui, |ui| {
                            for v in if rates.is_empty() { vec![m.rate] } else { rates } {
                                ui.selectable_value(&mut m.rate, v, format!("{v}"));
                            }
                        });
                        if ui.small_button("🗑").on_hover_text(i18n::tr("この行を消します")).clicked() {
                            remove = Some(k);
                        }
                        ui.end_row();
                    }
                });
            });
            if let Some(k) = remove {
                self.sch.adc_measures.remove(k);
            }
            ui.horizontal(|ui| {
                if ui.button(i18n::tr("＋ 行を足す")).on_hover_text(i18n::tr("ADC の部品の設定で選んだ入力・利得・レートから、行を 1 つ足します")).clicked()
                    && let Some(m) = self.new_adc_measure()
                {
                    self.sch.adc_measures.push(m);
                }
                if ui.button(i18n::tr("計算する")).on_hover_text(i18n::tr("行ごとに、条件を入れ、試す値をつないで DC 動作点を 2 回解き (試す値と 0.1 % 多い値)、ADC の読みと揺れを出します。回路図に電源が無ければ、コネクタに来る 5V・3V3 のような名前の線から入れます")).clicked() {
                    run = true;
                }
                if !fresh && !self.adc_measure_ui.results.is_empty() {
                    ui.colored_label(crate::theme::warn(), i18n::tr("回路図が変わりました。もう一度計算します"));
                }
            });
            // the figures of the resistors that carry none of their own, and the temperature span, for every row
            if let Some(first) = self.sch.adc_measures.first().cloned() {
                let (mut tol, mut tc, mut dt) = (first.tol_pct, first.tc_ppm, first.dtemp);
                ui.horizontal(|ui| {
                    ui.label(i18n::tr("誤差の見積もり:")).on_hover_text(i18n::tr("ライブラリの抵抗 (データシートの許容差・温度係数を持つもの) と、部品の設定で温度係数を入れた抵抗は、その値を使います。ほかの抵抗は、ここの値です"));
                    ui.label(i18n::tr("値の無い抵抗 ±"));
                    ui.add(egui::DragValue::new(&mut tol).range(0.0..=20.0).speed(0.01).suffix(" %"));
                    ui.add(egui::DragValue::new(&mut tc).range(0.0..=5000.0).speed(1.0).suffix(" ppm/K"));
                    ui.label(i18n::tr("温度の幅 ±")).on_hover_text(i18n::tr("校正したときの温度から、使うときの温度がどれだけ離れるか"));
                    ui.add(egui::DragValue::new(&mut dt).range(0.0..=100.0).speed(0.5).suffix(" K"));
                });
                if (tol, tc, dt) != (first.tol_pct, first.tc_ppm, first.dtemp) {
                    for m in self.sch.adc_measures.iter_mut() {
                        (m.tol_pct, m.tc_ppm, m.dtemp) = (tol, tc, dt);
                    }
                }
            }
            if fresh && let Some(fed) = self.adc_measure_ui.results.iter().find_map(|x| x.1.as_ref().ok()).map(|x| x.done.iter().filter(|f| f.ends_with(" V") && f.contains(" = ")).cloned().collect::<Vec<_>>()).filter(|f| !f.is_empty()) {
                ui.small(i18n::trf!("回路図に電源がないので、コネクタの線から入れました: {}", fed.join("、")));
            }
        });
        self.adc_measure_ui.open = open;
        if run {
            self.run_adc_measures();
        }
    }

    /// Work out every row of the table.
    fn run_adc_measures(&mut self) {
        self.adc_measure_ui.results = self.sch.adc_measures.iter().map(|m| (m.name.clone(), read_measure(&self.sch, m))).collect();
        self.adc_measure_ui.rev = Some(self.rev);
    }

    /// Open the table worked out (`--adc-measures`).
    pub fn open_adc_measures(&mut self) {
        self.adc_measure_ui.open = true;
        self.run_adc_measures();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kind_keeps_the_nets_where_it_can() {
        let v = Measured::Volts { plus: "HI".into(), minus: "LO".into() };
        assert_eq!(with_kind(&v, "amps"), Measured::Amps { plus: "HI".into(), minus: "LO".into() });
        assert_eq!(with_kind(&v, "part"), Measured::Part { part: String::new() });
        assert_eq!(kind_of(&with_kind(&Measured::Part { part: "R1".into() }, "ohms")), "ohms");
        assert_eq!(kind_of(&with_kind(&v, "temp")), "temp");
    }

    #[test]
    fn the_error_cell_reads_tolerance_temperature_and_leaks() {
        let e = MeasureErrors { tol_rss: 0.0127, temp_rss: 0.0064, leaks: vec![sim_core::adcmeasure::Leak { part: "D1".into(), err: -1e-6 }], leak_sum: -1e-6, ..Default::default() };
        assert_eq!(error_cell(&e, "V"), "±12.7mV / ±6.4mV / −1uV");
    }
}
