//! "ADC で測る量の表" (candidate L1): what the ADCs measure, one way per row, kept with the schematic (`Schematic::adc_measures`),
//! edited row by row and worked out on request (`sim_core::adcmeasure`): per way the ADC's input and code, and the reading's noise
//! and stable digits in the measured unit. A meter's ranges, a sensor's reading, a current through a shunt, a multiplexer's
//! channels are all rows of it.

use super::*;
use sch_core::adc_measure::{parse_conditions, AdcMeasure, Measured};
use sim_core::adcmeasure::{read_measure, MeasureReading};

/// The window's state: open, and the last results with the schematic's edit count they were worked out at.
#[derive(Default)]
pub(super) struct AdcMeasureUi {
    pub open: bool,
    results: Vec<(String, Result<MeasureReading, String>)>,
    rev: Option<u64>,
}

const GAINS: [f64; 8] = [1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0];

/// The kinds of thing measured, as the table's menu names them.
const KINDS: [(&str, &str); 4] = [("volts", "電圧"), ("ohms", "抵抗"), ("amps", "電流"), ("part", "部品の値")];

fn kind_of(m: &Measured) -> &'static str {
    match m {
        Measured::Volts { .. } => "volts",
        Measured::Ohms { .. } => "ohms",
        Measured::Amps { .. } => "amps",
        Measured::Part { .. } => "part",
    }
}

/// `m` turned into kind `k`, keeping the nets (or the part) where they fit.
fn with_kind(m: &Measured, k: &str) -> Measured {
    let (plus, minus) = match m {
        Measured::Volts { plus, minus } | Measured::Ohms { plus, minus } | Measured::Amps { plus, minus } => (plus.clone(), minus.clone()),
        Measured::Part { .. } => (String::new(), "GND".into()),
    };
    match k {
        "ohms" => Measured::Ohms { plus, minus },
        "amps" => Measured::Amps { plus, minus },
        "part" => Measured::Part { part: String::new() },
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
            ui.label(egui::RichText::new(i18n::tr("ADC で何をどう測るかを 1 行ずつ書きます (メーターのレンジ、センサーの読み、シャントで測る電流、マルチプレクサのチャンネルなど)。測る物 (2 つのネットの間につなぐ電圧・抵抗・電流か、回路図にある部品の値)・試す値・条件・ADC の入力と利得を入れ、「計算する」で、DC 動作点から ADC の読みと、データシートの雑音を測る量に換算した揺れ・止まって見える桁を出します。表は回路図と一緒に保存されます")).small());
            if parts.is_empty() {
                ui.colored_label(crate::theme::WARN, i18n::tr("回路図に ADC (モデルに adc_bits のある部品。ADS124S08 など) がありません"));
                return;
            }
            let mut remove: Option<usize> = None;
            egui::ScrollArea::both().max_height(420.0).show(ui, |ui| {
                egui::Grid::new("adc_measure_grid").striped(true).num_columns(14).show(ui, |ui| {
                    for (h, tip) in [
                        ("名前", ""),
                        ("測る物", "2 つのネットの間につなぐ電圧・抵抗・電流か、回路図にある部品 (抵抗・電源) の値 (センサー・ブリッジの片側など)"),
                        ("試す値", "試す値 (V・Ω・A か、その部品の単位)。満量程や、ふだんの値にします。10k・1Meg・100m のように書けます"),
                        ("桁", "試す値で、表示が止まって見える桁の数 (log10(試す値 ÷ p-p の揺れ))"),
                        ("揺れ (p-p)", "データシートの雑音を、測る量の単位に換算した揺れ"),
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
                                let clipped = x.main.clipped || x.reference.as_ref().is_some_and(|q| q.clipped);
                                let text = format!("{}V ({})", sim_core::format_value(x.main.vin), x.main.code);
                                if clipped {
                                    ui.colored_label(crate::theme::WARN, text).on_hover_text(i18n::tr("入力が ±基準電圧 / 利得 を超えて、コードが端に張り付いています"));
                                } else if x.slope == 0.0 {
                                    ui.colored_label(crate::theme::WARN, text).on_hover_text(i18n::tr("測る物を変えても読みが変わりません。つなぐ所・条件・ADC の入力を確かめます"));
                                } else {
                                    ui.label(text);
                                }
                            }
                            Some(Err(e)) => {
                                ui.colored_label(crate::theme::ERROR, "✖").on_hover_text(i18n::tr_dyn(e));
                                ui.label("");
                                ui.label("");
                            }
                            None => {
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
                    ui.colored_label(crate::theme::WARN, i18n::tr("回路図が変わりました。もう一度計算します"));
                }
            });
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
    }
}
