//! The "温度の時間変化" block under the "自己発熱" section: the temperature of chosen parts over time, from a transient run
//! (`sim_core::heat_tran`).

use super::*;

/// What the last run found.
pub(crate) struct HeatTranView {
    pub ambient: f64,
    pub tstop: f64,
    pub time: Vec<f64>,
    pub parts: Vec<sim_core::HeatTranPart>,
    /// When the shifts below are taken, and (signal, volts cold, volts warm).
    pub moved_at: f64,
    pub moved: Vec<(String, f64, f64)>,
    /// Each part's highest temperature against its limit (`sim_core::check_peak_temperatures`).
    pub checks: Vec<sim_core::TempCheck>,
}

/// What the 3D view's heat colours show (E9): the steady self-heating run ("自己発熱"), or from the temperature-over-time run each
/// part's highest temperature, or the temperatures at one moment (seconds).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(crate) enum TranPaint {
    #[default]
    Steady,
    Peak,
    At(f64),
}

impl HeatTranView {
    /// Each part's (name, temperature °C, power W): at its peak (`at` = `None`: the highest rise and the peak power, each at its own
    /// moment), or at the moment `at` (seconds; the rise and the power read off the run between its two nearest points).
    pub(crate) fn temps(&self, at: Option<f64>) -> Vec<(String, f64, f64)> {
        let read = |w: &[f64], t: f64| -> f64 {
            let k = self.time.partition_point(|&x| x < t);
            if k == 0 {
                return w.first().copied().unwrap_or(0.0);
            }
            if k >= self.time.len() {
                return w.last().copied().unwrap_or(0.0);
            }
            let (t0, t1) = (self.time[k - 1], self.time[k]);
            let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 1.0 };
            w[k - 1] + (w[k] - w[k - 1]) * f
        };
        self.parts
            .iter()
            .map(|p| match at {
                None => (p.name.clone(), self.ambient + p.rise_peak, p.power_peak),
                Some(t) => (p.name.clone(), self.ambient + read(&p.rise, t), read(&p.power, t)),
            })
            .collect()
    }

    /// The words under the 3D view's colour bar for `paint`.
    pub(crate) fn paint_source(paint: TranPaint) -> String {
        match paint {
            TranPaint::Steady => String::new(),
            TranPaint::Peak => i18n::tr("時間変化\n最高").to_string(),
            TranPaint::At(t) => i18n::trf!("時間変化\n{t:.3} 秒"),
        }
    }
}

/// "Q1:RS1:150, ..." -> (part, part, K/W) per link.
fn parse_links(text: &str) -> Result<Vec<(String, String, f64)>, String> {
    let mut links = vec![];
    for spec in text.split(',').map(str::trim).filter(|x| !x.is_empty()) {
        let w: Vec<&str> = spec.split(':').collect();
        let [a, b, th] = w.as_slice() else { return Err(format!("結合「{spec}」は 部品:部品:K/W の形で入力してください (例: Q1:RS1:150)")) };
        let theta = sim_core::parse_value(th).filter(|x| *x > 0.0 && x.is_finite()).ok_or_else(|| format!("結合「{spec}」の熱抵抗は 0 より大きい数 (K/W) にしてください"))?;
        links.push((a.trim().to_string(), b.trim().to_string(), theta));
    }
    Ok(links)
}

fn links_text(links: &[(String, String, f64)]) -> String {
    links.iter().map(|(a, b, t)| format!("{a}:{b}:{t}")).collect::<Vec<_>>().join(", ")
}

impl SchEditor {
    /// Keeps the links in the schematic (so the file has them); the text box shows the same.
    fn store_links(&mut self, links: Vec<(String, String, f64)>) {
        if self.sch.thermal_links != links {
            self.sch.thermal_links = links.clone();
        }
        self.heat.tran_links_seen = links;
    }

    pub(crate) fn run_heat_tran(&mut self) -> Result<HeatTranView, String> {
        let ambient: f64 = self.heat.ambient.trim().parse().ok().filter(|t: &f64| *t > -273.0 && *t < 1000.0).ok_or("周囲の温度を数値(℃)で入力してください (例: 27)")?;
        let tstop = sim_core::parse_value(self.heat.tran_stop.trim()).filter(|t| *t > 0.0 && t.is_finite()).ok_or("時間を、0 より大きい数で入力してください (例: 0.6 や 600m。秒)")?;
        let refs: Vec<String> = self.heat.tran_parts.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
        if refs.is_empty() {
            return Err("温度を見たい部品の名前を入力してください (例: Q1,RS1。コンマで区切る)".into());
        }
        let parsed = parse_links(&self.heat.tran_links)?;
        // what was run is what the file keeps
        self.store_links(parsed.clone());
        let links: Vec<sim_core::ThermalLink> = parsed.into_iter().map(|(a, b, theta)| sim_core::ThermalLink { a, b, theta }).collect();
        let c = sim_core::from_schematic_with_layout(&self.sch, &self.wiring, &self.couplings).map_err(|e| e.first().map(|x| x.to_string()).unwrap_or_default())?;
        let mut nodes = sim_core::thermal_nodes_from_schematic(&self.sch, &c, &refs, &self.copper_g())?;
        let links = self.tran_nodes_on_board(&c, &mut nodes, links, ambient)?;
        let r = sim_core::heat_tran(&c, &nodes, &links, ambient, &sim_core::TranSettings { tstop, tstep: sim_core::auto_tstep(tstop), uic: false }).map_err(|e| e.to_string())?;
        let (moved_at, moved) = r.moved(6);
        let checks = sim_core::check_peak_temperatures(&self.sch, &r.parts, ambient);
        Ok(HeatTranView { ambient, tstop, time: r.time.clone(), parts: r.parts.clone(), moved_at, moved, checks })
    }

    pub(super) fn panel_heat_tran(&mut self, ui: &mut egui::Ui) {
        // a file that was opened (or an undo) brings other links than the box shows: the box follows the schematic
        if self.sch.thermal_links != self.heat.tran_links_seen {
            self.heat.tran_links = links_text(&self.sch.thermal_links);
            self.heat.tran_links_seen = self.sch.thermal_links.clone();
        }
        ui.separator();
        ui.label(egui::RichText::new(i18n::tr("温度の時間変化 (短い通電)")).strong())
            .on_hover_text(i18n::tr("過渡解析の電力から、部品の温度が時間とともにどう上がるかを計算します。短い通電では、定常の温度より低くなります (熱容量があるため)。2つの部品が基板の銅を通じてお互いを温める分も入れられます"));
        egui::Grid::new("htran_inputs").num_columns(2).show(ui, |ui| {
            ui.label(i18n::tr("部品"));
            ui.add(egui::TextEdit::singleline(&mut self.heat.tran_parts).desired_width(150.0).hint_text("Q1,RS1"))
                .on_hover_text(i18n::tr("温度を見たい部品の名前 (コンマ区切り)。抵抗・ダイオード・トランジスタが入った部品 (ライブラリ部品も) が対象です"));
            ui.end_row();
            ui.label(i18n::tr("時間"));
            ui.add(egui::TextEdit::singleline(&mut self.heat.tran_stop).desired_width(60.0)).on_hover_text(i18n::tr("計算する長さ (秒)。電源のパルスや波形は、この間の動きを使います (例: 0.6 や 600m)"));
            ui.end_row();
            ui.label(i18n::tr("結合"));
            let links_box = ui.add(egui::TextEdit::singleline(&mut self.heat.tran_links).desired_width(150.0).hint_text("Q1:RS1:150")).on_hover_text(
                i18n::tr("2つの部品の間の熱抵抗 (K/W)。部品:部品:K/W の形で、コンマで複数。基板の銅を通って熱が行き来します。空欄なら、お互いは温め合いません。値の目安は、距離が遠いほど大きく (数百 K/W)、同じ銅でつながっていれば小さくなります。データシートにはないので、推測で入れてください"),
            );
            // leaving the box with links that read is enough to keep them (a half-typed one is kept as text only)
            if links_box.lost_focus()
                && let Ok(l) = parse_links(&self.heat.tran_links)
            {
                self.store_links(l);
            }
            ui.end_row();
        });
        if self
            .lit(ui.button(i18n::tr("温度の変化を計算")), "温度の変化を計算")
            .on_hover_text(i18n::tr("過渡解析を実行して、各部品の電力から温度の上がり方を出します。温度係数のある抵抗と半導体は、温まった温度で値・モデルを決め直して計算し直します"))
            .clicked()
        {
            self.heat.tran = Some(self.run_heat_tran());
            if let Some(Err(e)) = &self.heat.tran {
                self.status = format!("温度の変化の計算に失敗: {e}");
            }
        }
        let Some(view) = self.heat.tran.take() else { return };
        match &view {
            Err(e) => {
                ui.colored_label(crate::theme::error(), e).on_hover_text(i18n::tr("部品の名前・時間・電源を確認してください。過渡解析が通らない回路は計算できません"));
            }
            Ok(v) => {
                let mut edits: Vec<(String, f64, f64)> = vec![];
                let mut limit_edits: Vec<(String, f64)> = vec![];
                let mut case_edits: Vec<(String, super::selfheat::CaseHeat)> = vec![];
                let own = self.own_thetas();
                egui::Grid::new("htran_grid").num_columns(7).striped(true).spacing([6.0, 4.0]).show(ui, |ui| {
                    for (h, tip) in [("部品", ""), ("電力(最大)", ""), ("上昇(最大)", ""), ("θ K/W", "熱抵抗 (K/W)"), ("熱容量 J/K", ""), ("定常なら", ""), ("上限 ℃", "部品の最高温度 (℃)")] {
                        let r = ui.label(egui::RichText::new(h).small().strong());
                        if !tip.is_empty() {
                            r.on_hover_text(i18n::tr(tip));
                        }
                    }
                    ui.end_row();
                    for (k, p) in v.parts.iter().enumerate() {
                        let stages = if p.stages > 1 { i18n::trf!("。過渡熱抵抗 (Zth) の {} 段で計算 (部品作成の「データシートの情報」で入れたもの)", p.stages) } else { String::new() };
                        ui.label(egui::RichText::new(&p.name).color(curve_color(k)))
                            .on_hover_text(format!("{}{stages}", i18n::trf!("最高 {:.1} ℃ (周囲 {} ℃ + {:.1} K、{:.3} 秒)。電力をかけている間の平均 {:.1} ℃", v.ambient + p.rise_peak, v.ambient, p.rise_peak, p.peak_time, p.temp_working_c)));
                        ui.label(fmt_power(p.power_peak));
                        let check = v.checks.iter().find(|c| c.name == p.name);
                        let rise_col = match check.and_then(|c| c.level) {
                            Some(sim_core::RatingLevel::Over) => crate::theme::error(),
                            Some(sim_core::RatingLevel::Caution) => crate::theme::warn(),
                            None => ui.visuals().text_color(),
                        };
                        let neighbours = i18n::trf!(
                            "相手の部品から受けた熱: {:+.2} K (結合を外したときとの差。熱を逃がす側はマイナス)。値の変化 {:+.0} ppm",
                            p.rise_peak - p.rise_alone,
                            p.change * 1e6
                        );
                        let tip = check.map_or(neighbours.clone(), |c| format!("{}
{neighbours}", i18n::tr_dyn(&c.message)));
                        ui.label(egui::RichText::new(format!("{:.1} K", p.rise_peak)).color(rise_col)).on_hover_text(tip);
                        let (mut theta, mut cth) = (own.get(&p.name).copied().unwrap_or(p.theta), p.cth);
                        let mut case = self.case_heat(&p.name);
                        let a = super::selfheat::theta_cell(ui, &mut theta, p.theta, self.heat_copper.get(&p.name), &mut case, self.board_heat_parts.contains(&p.name));
                        if case.changed {
                            case_edits.push((p.name.clone(), case));
                        }
                        let b = ui
                            .add(egui::DragValue::new(&mut cth).range(0.0..=100.0).speed(0.005).max_decimals(3))
                            .on_hover_text(i18n::tr("熱容量 (J/K): 部品とそのまわりの銅を 1 K 温めるのに要る熱。短い通電の温度の上がりは、これでほぼ決まります (上昇 ≒ 電力 × 時間 ÷ 熱容量)。データシートに数字がないことが多いので、推測の値です。0 だと定常の温度になります"));
                        if a.changed() || b.changed() {
                            edits.push((p.name.clone(), theta, cth));
                        }
                        ui.label(format!("{:.0} K", p.power_peak * p.theta)).on_hover_text(i18n::tr("同じ電力を流し続けたとき (定常) の上昇。通電が短いほど、最大の上昇はこれよりずっと低くなります"));
                        if let Some(c) = check {
                            let mut t = c.limit.t_max;
                            let guess = c.limit.source == sim_core::LimitSource::Guess;
                            let r = ui.add(egui::DragValue::new(&mut t).range(40.0..=400.0).speed(1.0).max_decimals(0).suffix(if guess { "?" } else { "" })).on_hover_text(i18n::tr("部品の中が達してよい最高の温度 (半導体は接合部の Tj の最大、抵抗は抵抗体の上限)。いちばん熱い瞬間の温度と比べます。「?」は部品の種類からの目安です。データシートの値に直してください。変えたら、もう一度計算してください"));
                            if r.changed() {
                                limit_edits.push((p.name.clone(), t));
                            }
                        } else {
                            ui.label("");
                        }
                        ui.end_row();
                    }
                });
                for (name, theta, cth) in edits {
                    if let Some(c) = self.sch.components.iter_mut().find(|c| c.refdes == name) {
                        c.theta = theta;
                        c.cth = cth;
                    }
                }
                self.apply_case_heat(case_edits);
                for (name, t) in limit_edits {
                    if let Some(c) = self.sch.components.iter_mut().find(|c| c.refdes == name) {
                        c.t_max = t;
                    }
                }
                for c in v.checks.iter().filter(|c| c.level.is_some()) {
                    let col = if c.level == Some(sim_core::RatingLevel::Over) { crate::theme::error() } else { crate::theme::warn() };
                    ui.colored_label(col, format!("⚠ {}", i18n::tr_dyn(&c.message))).on_hover_text(i18n::tr_dyn(&c.hint));
                }
                draw_rise_chart(ui, v, match self.heat.paint {
                    TranPaint::At(t) => Some(t),
                    _ => None,
                });
                self.paint_row(ui, v);
                if !v.moved.is_empty() {
                    ui.label(egui::RichText::new(i18n::trf!("温まって動いた電圧 ({:.3} 秒のとき)", v.moved_at)).small().strong()).on_hover_text(i18n::tr("冷えているときの電圧と、温まったあとの電圧の差。大きい順"));
                    for (n, a, b) in &v.moved {
                        ui.label(egui::RichText::new(format!("{n}: {:.6} → {:.6} ({:+.3} mV)", a, b, (b - a) * 1e3)).small());
                    }
                }
                ui.label(egui::RichText::new(i18n::tr("熱容量と、部品どうしの結合は推測の値です。数字の幅を見るために、変えて比べてください")).small())
                    .on_hover_text(i18n::tr("1つの部品を、熱容量と熱抵抗の1段として扱った簡単な近似です。基板の中の熱の広がりまでは計算していません"));
            }
        }
        self.heat.tran = Some(view);
    }
}

impl SchEditor {
    /// The row choosing what the 3D view's heat colours show (the steady run, the peaks of this run, or a moment of it on a slider) and
    /// the button that opens the 3D view with them.
    fn paint_row(&mut self, ui: &mut egui::Ui, v: &HeatTranView) {
        ui.horizontal_wrapped(|ui| {
            ui.label(i18n::tr("3D の温度の色:")).on_hover_text(i18n::tr("基板の 3D の窓で「温度の色」にしたとき、部品をどの温度で塗るか"));
            let p = &mut self.heat.paint;
            if ui.radio(*p == TranPaint::Steady, i18n::tr("定常")).on_hover_text(i18n::tr("上の「自己発熱」の、電力を流し続けたときの温度で塗ります")).clicked() {
                *p = TranPaint::Steady;
            }
            if ui.radio(*p == TranPaint::Peak, i18n::tr("最高")).on_hover_text(i18n::tr("この計算の中で、部品ごとにいちばん熱くなった温度で塗ります (部品ごとに時刻は違います)")).clicked() {
                *p = TranPaint::Peak;
            }
            let at = matches!(p, TranPaint::At(_));
            if ui.radio(at, i18n::tr("時刻")).on_hover_text(i18n::tr("選んだ時刻の温度で塗ります。スライダーを動かすと、部品が温まっていく (冷えていく) 様子が 3D で見えます")).clicked() && !at {
                *p = TranPaint::At(v.tstop);
            }
            if let TranPaint::At(t) = p {
                let mut x = t.clamp(0.0, v.tstop);
                if ui.add(egui::Slider::new(&mut x, 0.0..=v.tstop).suffix(" s").max_decimals(4)).on_hover_text(i18n::tr("塗る時刻 (秒)。グラフの縦の線がその時刻です")).changed() {
                    *t = x;
                }
            }
            if ui.button(i18n::tr("3D で見る")).on_hover_text(i18n::tr("基板の画面の 3D の窓を開いて、部品をこの温度の色で塗ります (回路図の部品番号と同じ部品を塗ります。基板に無い部品は描けません)")).clicked() {
                if self.heat.paint == TranPaint::Steady {
                    self.heat.paint = TranPaint::Peak;
                }
                self.heat.show_3d = true;
            }
        });
    }
}

fn fmt_power(w: f64) -> String {
    if w >= 1.0 { format!("{w:.2} W") } else if w >= 1e-3 { format!("{:.1} mW", w * 1e3) } else { format!("{:.1} µW", w * 1e6) }
}

fn curve_color(k: usize) -> egui::Color32 {
    crate::theme::series_color(k)
}

/// The temperature rise of each part over time, as lines.
fn draw_rise_chart(ui: &mut egui::Ui, v: &HeatTranView, at: Option<f64>) {
    // the limit (as a rise over the surroundings) of each part that comes near it or past it is drawn too, so the scale takes it in
    let limits: Vec<(usize, f64)> = v
        .parts
        .iter()
        .enumerate()
        .filter_map(|(k, p)| v.checks.iter().find(|c| c.name == p.name && c.level.is_some()).map(|c| (k, c.limit.t_max - v.ambient)))
        .filter(|(_, l)| *l > 0.0)
        .collect();
    let top = v.parts.iter().map(|p| p.rise_peak).chain(limits.iter().map(|l| l.1)).fold(0.0, f64::max).max(1e-9);
    let size = egui::vec2(ui.available_width().clamp(180.0, 420.0), 120.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
    resp.on_hover_text(i18n::tr("各部品の、周囲の温度からの上昇 (K) の時間変化。横軸は時間、縦軸は上昇。上限に近づいた・超えた部品は、上限を同じ色の点線で描きます"));
    let plot = rect.shrink2(egui::vec2(34.0, 14.0));
    let painter = ui.painter_at(rect);
    let weak = ui.visuals().weak_text_color();
    painter.rect_stroke(plot, 0.0, egui::Stroke::new(1.0, weak), egui::StrokeKind::Inside);
    let font = egui::FontId::proportional(10.0);
    painter.text(egui::pos2(plot.left() - 3.0, plot.top()), egui::Align2::RIGHT_TOP, format!("{top:.1}K"), font.clone(), weak);
    painter.text(egui::pos2(plot.left() - 3.0, plot.bottom()), egui::Align2::RIGHT_BOTTOM, "0", font.clone(), weak);
    painter.text(egui::pos2(plot.right(), plot.bottom() + 2.0), egui::Align2::RIGHT_TOP, format!("{:.3} s", v.tstop), font, weak);
    for (k, p) in v.parts.iter().enumerate() {
        let pts: Vec<egui::Pos2> =
            v.time.iter().zip(&p.rise).map(|(t, u)| egui::pos2(plot.left() + (t / v.tstop) as f32 * plot.width(), plot.bottom() - (u / top) as f32 * plot.height())).collect();
        painter.add(egui::Shape::line(pts, egui::Stroke::new(1.5, curve_color(k))));
    }
    // the moment the 3D view is painted at
    if let Some(t) = at {
        let x = plot.left() + (t / v.tstop).clamp(0.0, 1.0) as f32 * plot.width();
        painter.line_segment([egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())], egui::Stroke::new(1.0, ui.visuals().text_color()));
    }
    for (k, lim) in limits {
        let y = plot.bottom() - (lim / top) as f32 * plot.height();
        painter.add(egui::Shape::dashed_line(&[egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)], egui::Stroke::new(1.0, curve_color(k)), 5.0, 4.0));
        painter.text(egui::pos2(plot.right() - 2.0, y - 1.0), egui::Align2::RIGHT_BOTTOM, i18n::tr("上限"), egui::FontId::proportional(10.0), curve_color(k));
    }
}
