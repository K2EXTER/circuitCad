//! The "自己発熱" section of the analysis tab: the resistors, diodes and transistors warmed by their own power (`sim_core::self_heat`).

use super::*;

/// What the last run found.
pub(crate) struct HeatView {
    pub ambient: f64,
    pub parts: Vec<sim_core::HeatedPart>,
    /// (net, volts cold, volts warm) for the nets that moved the most.
    pub nodes: Vec<(String, f64, f64)>,
    /// Each warmed part against its temperature limit (`sim_core::check_temperatures`), the worst first.
    pub checks: Vec<sim_core::TempCheck>,
    /// With the board carried over: the pads' wiring that the board's heat made most resistive (the hottest first).
    pub wiring_warm: Vec<pcb_core::thermal::WarmWiring>,
}

impl SchEditor {
    /// The heat sources of the board's temperature map from a self-heating run: each warmed part's power, its own θ, θjc and heat sink;
    /// with the run's temperatures (to compare) and the air's.
    #[allow(clippy::type_complexity)]
    pub(crate) fn thermal_sources(&self, v: &HeatView) -> (Vec<pcb_core::thermal::HeatSource>, Vec<(String, f64)>, f64) {
        let src = v
            .parts
            .iter()
            .filter_map(|p| {
                let c = self.sch.components.iter().find(|c| c.refdes == p.name)?;
                let mut s = pcb_core::thermal::HeatSource::new(&p.name, p.power, sim_core::own_theta(c), sim_core::theta_jc(c).0, c.heatsink);
                s.cth = c.cth;
                Some(s)
            })
            .collect();
        (src, v.parts.iter().map(|p| (p.name.clone(), p.temp_c)).collect(), v.ambient)
    }

    pub(crate) fn run_self_heat(&mut self) -> Result<HeatView, String> {
        let ambient: f64 = self.heat_ambient.trim().parse().ok().filter(|t: &f64| *t > -273.0 && *t < 1000.0).ok_or("周囲の温度を数値(℃)で入力してください (例: 27)")?;
        let c = sim_core::from_schematic_with_layout(&self.sch, &self.wiring, &self.couplings).map_err(|e| e.first().map(|x| x.to_string()).unwrap_or_default())?;
        let heaters = sim_core::heaters_from_schematic(&self.sch, &c, &self.copper_g());
        let (heaters, mutual) = self.heaters_on_board(&c, heaters, ambient)?;
        let first = sim_core::self_heat_coupled(&c, &heaters, ambient, &mutual).map_err(|e| e.to_string())?;
        // with the board carried over, its copper is as warm as the map says: once more on the warmer wiring (the cold answer stays the
        // one on copper at 25 °C)
        let mut wiring_warm = vec![];
        let r = match self.warm_wiring(&first.parts, ambient) {
            Some((wiring, warm)) if warm.iter().any(|w| (w.factor - 1.0).abs() > 1e-4) => {
                let c2 = sim_core::from_schematic_with_layout(&self.sch, &wiring, &self.couplings).map_err(|e| e.first().map(|x| x.to_string()).unwrap_or_default())?;
                let mut again = sim_core::self_heat_coupled(&c2, &heaters, ambient, &mutual).map_err(|e| e.to_string())?;
                again.cold = first.cold.clone();
                wiring_warm = warm;
                wiring_warm.sort_by(|a, b| b.temp_c.total_cmp(&a.temp_c));
                wiring_warm.truncate(6);
                again
            }
            _ => first,
        };
        let mut parts: Vec<_> = r.parts.iter().filter(|p| p.power > 1e-9).cloned().collect();
        parts.sort_by(|a, b| b.rise.total_cmp(&a.rise));
        let mut nodes: Vec<(String, f64, f64)> = r.warm.voltages.iter().filter(|(n, _)| !n.contains('.')).filter_map(|(n, v)| r.cold.voltage(n).map(|c0| (n.clone(), c0, *v))).filter(|(_, a, b)| (a - b).abs() > 1e-9).collect();
        nodes.sort_by(|a, b| (b.2 - b.1).abs().total_cmp(&(a.2 - a.1).abs()));
        nodes.truncate(8);
        let checks = sim_core::check_temperatures(&self.sch, &parts, ambient);
        Ok(HeatView { ambient, parts, nodes, checks, wiring_warm })
    }

    pub(super) fn panel_selfheat(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(i18n::tr("周囲の温度"));
            ui.add(egui::TextEdit::singleline(&mut self.heat_ambient).desired_width(40.0)).on_hover_text(i18n::tr("部品のまわりの空気の温度 (℃)。部品の温度は、これに「電力 × 熱抵抗」を足したものになります"));
            ui.label("℃");
        });
        if self.lit(crate::widgets::primary_button_wide(ui, true, "自己発熱を計算"), "自己発熱を計算").on_hover_text(i18n::tr("電流が流れる抵抗・ダイオード・LED・トランジスタは、自分の電力で温まります。温まると値が変わる抵抗 (温度係数のある部品) や、温まると電圧が下がるダイオード・トランジスタ (1 ℃ で約 2 mV) は、電流も変わるので、落ち着くまでくり返し計算します。直流の定常状態です (短いパルスでは、これより低くなります)")).clicked() {
            self.heat_view = Some(self.run_self_heat());
            if let Some(Err(e)) = &self.heat_view {
                self.status = format!("自己発熱の計算に失敗: {e}");
            }
        }
        let Some(view) = self.heat_view.take() else {
            ui.label(egui::RichText::new(i18n::tr("電流が流れる部品 (抵抗・ダイオード・トランジスタ) の温度上昇と、それで値や電圧がどれだけ動くかを見ます")).small());
            return;
        };
        match &view {
            Err(e) => {
                ui.colored_label(crate::theme::ERROR, e).on_hover_text(i18n::tr("電源や電流の流れ方を確認してください。DC解析が収束しない回路は計算できません"));
            }
            Ok(v) => {
                if v.parts.is_empty() {
                    ui.label(i18n::tr("電力が出ている部品はありません"));
                }
                let mut edits: Vec<(String, f64, f64)> = vec![];
                let mut limit_edits: Vec<(String, f64)> = vec![];
                let mut case_edits: Vec<(String, CaseHeat)> = vec![];
                let own = self.own_thetas();
                egui::Grid::new("selfheat_grid").num_columns(7).striped(true).spacing([6.0, 4.0]).show(ui, |ui| {
                    for (h, tip) in [("部品", ""), ("電力", ""), ("上昇", ""), ("値の変化", ""), ("θ K/W", "熱抵抗 (K/W)"), ("係数", "温度係数 (ppm/K)"), ("上限 ℃", "部品の最高温度 (℃)")] {
                        let r = ui.label(egui::RichText::new(h).small().strong());
                        if !tip.is_empty() {
                            r.on_hover_text(i18n::tr(tip));
                        }
                    }
                    ui.end_row();
                    for p in &v.parts {
                        ui.label(&p.name).on_hover_text(i18n::trf!("温度 {:.1} ℃ (周囲 {} ℃ + {:.1} K)", p.temp_c, v.ambient, p.rise));
                        ui.label(fmt_power(p.power));
                        let check = v.checks.iter().find(|c| c.name == p.name);
                        let rise_col = match check.and_then(|c| c.level) {
                            Some(sim_core::RatingLevel::Over) => crate::theme::ERROR,
                            Some(sim_core::RatingLevel::Caution) => crate::theme::WARN,
                            None => ui.visuals().text_color(),
                        };
                        let mut tip = check.map_or_else(|| i18n::tr("周囲の温度より、これだけ高くなります").to_string(), |c| i18n::tr_dyn(&c.message));
                        if p.from_others > 0.05 {
                            tip.push_str(&format!("
{}", i18n::trf!("うち {:.1} K は、基板を通ってほかの部品から受けた熱です", p.from_others)));
                        }
                        ui.label(egui::RichText::new(format!("{:.1} K", p.rise)).color(rise_col)).on_hover_text(tip);
                        // a diode or transistor alone: its voltage moves, not a resistance
                        let semi = self.sch.components.iter().any(|c| c.refdes == p.name && matches!(c.kind, sch_core::Kind::Diode | sch_core::Kind::Led | sch_core::Kind::Npn));
                        if let Some((cold, warm)) = p.junction_v {
                            ui.label(format!("{:+.0} mV", (warm - cold) * 1e3)).on_hover_text(i18n::trf!("温まって、この部品の電圧 (ダイオードは順方向の電圧、トランジスタはコレクタ・エミッタ間) が {cold:.4} V から {warm:.4} V になりました。シリコンの接合は、1 ℃ 温まると約 2 mV 下がります"));
                        } else if semi || (p.junctions > 0 && p.tc1 == 0.0 && p.tc2 == 0.0) {
                            ui.label("—").on_hover_text(i18n::tr("中のダイオード・トランジスタは、この部品の温度で計算しています (電圧の変化は「温まって動いたところ」を見てください)"));
                        } else {
                            ui.label(format!("{:+.0} ppm", p.change * 1e6)).on_hover_text(i18n::tr("温まったことで、抵抗値が冷えているときよりどれだけ変わったか (100 ppm = 0.01 %)。温度係数が 0 なら変わりません"));
                        }
                        let (mut theta, mut tc) = (own.get(&p.name).copied().unwrap_or(p.theta), p.tc1 * 1e6);
                        let mut case = self.case_heat(&p.name);
                        let a = theta_cell(ui, &mut theta, p.theta, self.heat_copper.get(&p.name), &mut case, self.board_heat_parts.contains(&p.name));
                        if case.changed {
                            case_edits.push((p.name.clone(), case));
                        }
                        let b = if semi {
                            ui.label("—").on_hover_text(i18n::tr("温度係数は抵抗のものです。ダイオード・トランジスタの温度による変化は、モデルの決まり (飽和電流・しきい値の温度の式) で入ります"))
                        } else {
                            ui.add(egui::DragValue::new(&mut tc).range(-5000.0..=5000.0).speed(1.0).max_decimals(0)).on_hover_text(i18n::tr("温度係数: 1 K 温まると抵抗値が何 ppm 変わるか。データシートの値 (シャント抵抗なら ±25 〜 ±100 ppm/K など) を入れます"))
                        };
                        if a.changed() || b.changed() {
                            edits.push((p.name.clone(), theta, tc));
                        }
                        if let Some(c) = check {
                            let mut t = c.limit.t_max;
                            let guess = c.limit.source == sim_core::LimitSource::Guess;
                            let r = ui.add(egui::DragValue::new(&mut t).range(40.0..=400.0).speed(1.0).max_decimals(0).suffix(if guess { "?" } else { "" })).on_hover_text(i18n::tr("部品の中が達してよい最高の温度 (半導体は接合部の Tj の最大、抵抗は抵抗体の上限)。「?」は部品の種類からの目安です。データシートの値に直してください。変えたら、もう一度計算してください"));
                            if r.changed() {
                                limit_edits.push((p.name.clone(), t));
                            }
                        } else {
                            ui.label("");
                        }
                        ui.end_row();
                    }
                });
                for (name, t) in limit_edits {
                    if let Some(c) = self.sch.components.iter_mut().find(|c| c.refdes == name) {
                        c.t_max = t;
                    }
                }
                self.apply_case_heat(case_edits);
                // the parts near or past their limit, with what to do
                for c in v.checks.iter().filter(|c| c.level.is_some()) {
                    let col = if c.level == Some(sim_core::RatingLevel::Over) { crate::theme::ERROR } else { crate::theme::WARN };
                    ui.colored_label(col, format!("⚠ {}", i18n::tr_dyn(&c.message))).on_hover_text(i18n::tr_dyn(&c.hint));
                }
                for (name, theta, tc) in edits {
                    if let Some(c) = self.sch.components.iter_mut().find(|c| c.refdes == name) {
                        c.theta = theta;
                        c.tc_ppm = tc;
                    }
                }
                if !v.nodes.is_empty() {
                    ui.label(egui::RichText::new(i18n::tr("温まって動いたところ")).small().strong()).on_hover_text(i18n::tr("冷えているときの電圧と、温まったあとの電圧の差。大きい順"));
                    for (n, a, b) in &v.nodes {
                        ui.label(egui::RichText::new(format!("{n}: {:.6} V → {:.6} V ({:+.3} mV)", a, b, (b - a) * 1e3)).small());
                    }
                }
                if let Some(w) = v.wiring_warm.first() {
                    let list: Vec<String> = v.wiring_warm.iter().take(3).map(|w| format!("{}.{} {:.0} ℃ (+{:.1} %)", w.refdes, w.pin, w.temp_c, (w.factor - 1.0) * 100.0)).collect();
                    // the normal size: at the small one this font draws 8 much like 0
                    ui.label(i18n::trf!("配線も温まって抵抗が増えています: {}", list.join("、")))
                        .on_hover_text(i18n::trf!("銅の抵抗は 1 ℃ で約 0.39 % 増えます。基板の熱の地図で、パッドの配線 (電源のパッドからの道すじ) の平均の温度を出し、その抵抗で計算し直しました。いちばん熱いのは {}.{} の配線 ({} ネット)。上の「温まって動いたところ」に、その分も入っています", w.refdes, w.pin, w.net));
                }
                if !self.board_heat_parts.is_empty() {
                    ui.label(egui::RichText::new(i18n::trf!("基板の熱の地図から: {} (基板を「シミュレーションに反映」しています。部品どうしが基板を通って温め合う分も入っています)", self.board_heat_parts.join(", "))).small().color(crate::theme::OK))
                        .on_hover_text(i18n::tr("電力の出ている部品 (1 mW 以上、最大 24 個) を基板の升目に置き、部品ごとに 1 W を入れて解いた熱抵抗の表 (行列) を使っています。ほかの部品は、熱抵抗の式 (銅の面積から) のままです"));
                }
                ui.label(egui::RichText::new(i18n::tr("熱抵抗を入れていない部品は、パッケージの大きさからの目安 (定格電力から) です。データシートの値 (θJA) に直してください")).small()).on_hover_text(i18n::tr("チップ抵抗は、周囲 70 ℃ で定格の電力をかけると 155 ℃ になるものとして、θ ≈ 85 K ÷ 定格電力 としています"));
            }
        }
        self.heat_view = Some(view);
    }
}

impl SchEditor {
    /// The board copper's conductance (W/K) beside each part's θ, for `sim_core` (empty unless the board was carried over).
    pub(super) fn copper_g(&self) -> std::collections::BTreeMap<String, f64> {
        self.heat_copper.iter().map(|(k, v)| (k.clone(), v.0)).collect()
    }

    /// Each part's own θ (K/W, smallest pads): what it was given, or the guess from its package.
    pub(super) fn own_thetas(&self) -> std::collections::BTreeMap<String, f64> {
        self.sch.components.iter().map(|c| (c.refdes.clone(), sim_core::own_theta(c))).collect()
    }

    /// A part's θjc and heat sink, for the θ cell.
    pub(super) fn case_heat(&self, name: &str) -> CaseHeat {
        self.sch.components.iter().find(|c| c.refdes == name).map_or(CaseHeat { jc: 0.0, jc_guess: true, sink: 0.0, changed: false }, |c| {
            let (jc, jc_guess) = sim_core::theta_jc(c);
            CaseHeat { jc, jc_guess, sink: c.heatsink, changed: false }
        })
    }

    /// θjc and heat sinks changed in a heat table.
    pub(super) fn apply_case_heat(&mut self, edits: Vec<(String, CaseHeat)>) {
        for (name, h) in edits {
            if let Some(c) = self.sch.components.iter_mut().find(|c| c.refdes == name) {
                if !h.jc_guess {
                    c.theta_jc = h.jc;
                }
                c.heatsink = h.sink;
            }
        }
    }
}

/// A part's way from the junction to its case (θjc) and its heat sink, as the θ cell shows and edits them.
pub(super) struct CaseHeat {
    pub jc: f64,
    /// θjc is the guess (a share of θ), not a number from the data sheet.
    pub jc_guess: bool,
    /// The heat sink's θ (K/W, case to air, the mounting included); 0 = none.
    pub sink: f64,
    pub changed: bool,
}

/// The θ cell of a heat table: the part's own θ to edit, when the board's copper or a heat sink helps the θ it has with them, and a
/// menu with its θjc and heat sink (`case`, `changed` set when either was edited).
pub(super) fn theta_cell(ui: &mut egui::Ui, own: &mut f64, with_copper: f64, copper: Option<&(f64, f64)>, case: &mut CaseHeat, board: bool) -> egui::Response {
    ui.horizontal(|ui| {
        let r = ui.add(egui::DragValue::new(own).range(0.1..=1000.0).speed(0.5).max_decimals(1)).on_hover_text(i18n::tr("部品の熱抵抗 (K/W): 部品から空気までの熱の伝わりにくさ。データシートの「最小のパッド」の値を入れます (入れないと、パッケージの定格電力からの目安)。基板を回路図に反映していると、つながった銅の放熱ぶんが自動で足されます。変えたら、もう一度計算してください"));
        if board {
            let tip = i18n::trf!(
                "基板の熱の地図 (升目の計算) から求めた熱抵抗: {:.0} K/W。基板の銅 (ベタ・配線・ビア) と、表と裏の面から空気への対流と放射を解いた値で、θjc {:.1} K/W と放熱板も入っています。ほかの部品から受ける熱は、上昇の欄に足してあります。目安で、風やケースで ±20〜30 % ずれます",
                with_copper,
                case.jc
            );
            ui.label(egui::RichText::new(format!("→ {with_copper:.0}")).small().color(crate::theme::OK)).on_hover_text(tip);
        } else if copper.is_some() || case.sink > 0.0 {
            let mut tip = vec![i18n::trf!("銅と放熱板を入れた熱抵抗: {:.0} K/W = θjc {:.1} + (ケース → 空気 {:.1} と、銅・放熱板の並列)", with_copper, case.jc, *own - case.jc)];
            if let Some((_, mm2)) = copper {
                tip.push(i18n::trf!(
                    "ピンにつながった銅 (部品の中心から 20 mm 以内、パッドのほかに {:.0} mm²) が熱を広げます。銅 35 µm (1 oz) として、SOT-223 のデータシートの曲線 (最小のパッド 110 K/W、1 in² で 42 K/W、2 oz、θjc 12 K/W) から見積もった目安です。正確には実物で測ってください",
                    mm2
                ));
            }
            if case.sink > 0.0 {
                tip.push(i18n::trf!("放熱板 {:.1} K/W (ケース → 空気、取り付けを含む)", case.sink));
            }
            ui.label(egui::RichText::new(format!("→ {with_copper:.0}")).small().color(crate::theme::OK)).on_hover_text(tip.join("\n"));
        }
        ui.menu_button(if case.sink > 0.0 { i18n::tr("放熱✓") } else { i18n::tr("放熱") }, |ui| {
            ui.label(egui::RichText::new(i18n::tr("ケースと放熱板")).strong());
            ui.label(egui::RichText::new(i18n::tr("熱は 接合部 → ケース → 空気 と流れます。基板の銅と放熱板は、ケースから先の道を増やすだけなので、θjc の分は残ります")).small());
            egui::Grid::new("case_heat").num_columns(2).show(ui, |ui| {
                ui.label(i18n::tr("θjc K/W")).on_hover_text(i18n::tr("接合部 (部品の中) からケース (パッケージの外側・放熱タブ) までの熱抵抗。データシートの「Junction to Case」(RθJC)。「?」は θ の 1 割ほどの目安です (銅の式を合わせた SOT-223 の部品の割合)"));
                let r = ui.add(egui::DragValue::new(&mut case.jc).range(0.0..=*own).speed(0.1).max_decimals(2).suffix(if case.jc_guess { "?" } else { "" }));
                if r.changed() {
                    case.jc_guess = false;
                    case.changed = true;
                }
                ui.end_row();
                ui.label(i18n::tr("放熱板 K/W")).on_hover_text(i18n::tr("放熱板のケースから空気までの熱抵抗 (放熱板のデータシートの値に、取り付け (グリス・シート) の 0.5〜1 K/W を足したもの)。0 で放熱板なし。小さいほどよく冷えます"));
                if ui.add(egui::DragValue::new(&mut case.sink).range(0.0..=200.0).speed(0.1).max_decimals(1)).changed() {
                    case.changed = true;
                }
                ui.end_row();
            });
            ui.label(egui::RichText::new(i18n::tr("変えたら、もう一度計算してください")).small().weak());
        })
        .response
        .on_hover_text(i18n::tr("θjc (接合部 → ケース) と放熱板"));
        r
    })
    .inner
}

fn fmt_power(w: f64) -> String {
    if w >= 1.0 { format!("{w:.2} W") } else if w >= 1e-3 { format!("{:.1} mW", w * 1e3) } else { format!("{:.1} µW", w * 1e6) }
}
