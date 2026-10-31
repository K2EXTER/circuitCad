//! The "温度の時間変化" block under the "自己発熱" section: the temperature of chosen parts over time, from a transient run
//! (`sim_core::heat_tran`).

use super::*;

/// What the last run found.
pub(super) struct HeatTranView {
    pub ambient: f64,
    pub tstop: f64,
    pub time: Vec<f64>,
    pub parts: Vec<sim_core::HeatTranPart>,
    /// When the shifts below are taken, and (signal, volts cold, volts warm).
    pub moved_at: f64,
    pub moved: Vec<(String, f64, f64)>,
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
        self.htran_links_seen = links;
    }

    pub(super) fn run_heat_tran(&mut self) -> Result<HeatTranView, String> {
        let ambient: f64 = self.heat_ambient.trim().parse().ok().filter(|t: &f64| *t > -273.0 && *t < 1000.0).ok_or("周囲の温度を数値(℃)で入力してください (例: 27)")?;
        let tstop = sim_core::parse_value(self.htran_stop.trim()).filter(|t| *t > 0.0 && t.is_finite()).ok_or("時間を、0 より大きい数で入力してください (例: 0.6 や 600m。秒)")?;
        let refs: Vec<String> = self.htran_parts.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
        if refs.is_empty() {
            return Err("温度を見たい部品の名前を入力してください (例: Q1,RS1。コンマで区切る)".into());
        }
        let parsed = parse_links(&self.htran_links)?;
        // what was run is what the file keeps
        self.store_links(parsed.clone());
        let links: Vec<sim_core::ThermalLink> = parsed.into_iter().map(|(a, b, theta)| sim_core::ThermalLink { a, b, theta }).collect();
        let c = sim_core::from_schematic_with_layout(&self.sch, &self.wiring, &self.couplings).map_err(|e| e.first().map(|x| x.to_string()).unwrap_or_default())?;
        let nodes = sim_core::thermal_nodes_from_schematic(&self.sch, &c, &refs)?;
        let r = sim_core::heat_tran(&c, &nodes, &links, ambient, &sim_core::TranSettings { tstop, tstep: sim_core::auto_tstep(tstop), uic: false }).map_err(|e| e.to_string())?;
        let (moved_at, moved) = r.moved(6);
        Ok(HeatTranView { ambient, tstop, time: r.time.clone(), parts: r.parts.clone(), moved_at, moved })
    }

    pub(super) fn panel_heat_tran(&mut self, ui: &mut egui::Ui) {
        // a file that was opened (or an undo) brings other links than the box shows: the box follows the schematic
        if self.sch.thermal_links != self.htran_links_seen {
            self.htran_links = links_text(&self.sch.thermal_links);
            self.htran_links_seen = self.sch.thermal_links.clone();
        }
        ui.separator();
        ui.label(egui::RichText::new("温度の時間変化 (短い通電)").strong())
            .on_hover_text("過渡解析の電力から、部品の温度が時間とともにどう上がるかを計算します。短い通電では、定常の温度より低くなります (熱容量があるため)。2つの部品が基板の銅を通じてお互いを温める分も入れられます");
        egui::Grid::new("htran_inputs").num_columns(2).show(ui, |ui| {
            ui.label("部品");
            ui.add(egui::TextEdit::singleline(&mut self.htran_parts).desired_width(150.0).hint_text("Q1,RS1"))
                .on_hover_text("温度を見たい部品の名前 (コンマ区切り)。抵抗・ダイオード・トランジスタが入った部品 (ライブラリ部品も) が対象です");
            ui.end_row();
            ui.label("時間");
            ui.add(egui::TextEdit::singleline(&mut self.htran_stop).desired_width(60.0)).on_hover_text("計算する長さ (秒)。電源のパルスや波形は、この間の動きを使います (例: 0.6 や 600m)");
            ui.end_row();
            ui.label("結合");
            let links_box = ui.add(egui::TextEdit::singleline(&mut self.htran_links).desired_width(150.0).hint_text("Q1:RS1:150")).on_hover_text(
                "2つの部品の間の熱抵抗 (K/W)。部品:部品:K/W の形で、コンマで複数。基板の銅を通って熱が行き来します。空欄なら、お互いは温め合いません。値の目安は、距離が遠いほど大きく (数百 K/W)、同じ銅でつながっていれば小さくなります。データシートにはないので、推測で入れてください",
            );
            // leaving the box with links that read is enough to keep them (a half-typed one is kept as text only)
            if links_box.lost_focus()
                && let Ok(l) = parse_links(&self.htran_links)
            {
                self.store_links(l);
            }
            ui.end_row();
        });
        if self
            .lit(ui.button("温度の変化を計算"), "温度の変化を計算")
            .on_hover_text("過渡解析を実行して、各部品の電力から温度の上がり方を出します。温度係数のある抵抗と半導体は、温まった温度で値・モデルを決め直して計算し直します")
            .clicked()
        {
            self.htran = Some(self.run_heat_tran());
            if let Some(Err(e)) = &self.htran {
                self.status = format!("温度の変化の計算に失敗: {e}");
            }
        }
        let Some(view) = self.htran.take() else { return };
        match &view {
            Err(e) => {
                ui.colored_label(egui::Color32::from_rgb(230, 90, 70), e).on_hover_text("部品の名前・時間・電源を確認してください。過渡解析が通らない回路は計算できません");
            }
            Ok(v) => {
                let mut edits: Vec<(String, f64, f64)> = vec![];
                egui::Grid::new("htran_grid").num_columns(6).striped(true).show(ui, |ui| {
                    for h in ["部品", "電力(最大)", "上昇(最大)", "熱抵抗 K/W", "熱容量 J/K", "定常なら"] {
                        ui.label(egui::RichText::new(h).small().strong());
                    }
                    ui.end_row();
                    for (k, p) in v.parts.iter().enumerate() {
                        ui.label(egui::RichText::new(&p.name).color(curve_color(k)))
                            .on_hover_text(format!("最高 {:.1} ℃ (周囲 {} ℃ + {:.1} K、{:.3} 秒)。電力をかけている間の平均 {:.1} ℃", v.ambient + p.rise_peak, v.ambient, p.rise_peak, p.peak_time, p.temp_working_c));
                        ui.label(fmt_power(p.power_peak));
                        ui.label(format!("{:.1} K", p.rise_peak)).on_hover_text(format!(
                            "相手の部品から受けた熱: {:+.2} K (結合を外したときとの差。熱を逃がす側はマイナス)。値の変化 {:+.0} ppm",
                            p.rise_peak - p.rise_alone,
                            p.change * 1e6
                        ));
                        let (mut theta, mut cth) = (p.theta, p.cth);
                        let a = ui.add(egui::DragValue::new(&mut theta).range(0.1..=1000.0).speed(0.5)).on_hover_text("熱抵抗 (K/W): 部品から空気までの熱の伝わりにくさ。データシートの値 (基板の銅の面積で変わります) を入れます。変えたら、もう一度計算してください");
                        let b = ui
                            .add(egui::DragValue::new(&mut cth).range(0.0..=100.0).speed(0.005).max_decimals(3))
                            .on_hover_text("熱容量 (J/K): 部品とそのまわりの銅を 1 K 温めるのに要る熱。短い通電の温度の上がりは、これでほぼ決まります (上昇 ≒ 電力 × 時間 ÷ 熱容量)。データシートに数字がないことが多いので、推測の値です。0 だと定常の温度になります");
                        if a.changed() || b.changed() {
                            edits.push((p.name.clone(), theta, cth));
                        }
                        ui.label(format!("{:.0} K", p.power_peak * p.theta)).on_hover_text("同じ電力を流し続けたとき (定常) の上昇。通電が短いほど、最大の上昇はこれよりずっと低くなります");
                        ui.end_row();
                    }
                });
                for (name, theta, cth) in edits {
                    if let Some(c) = self.sch.components.iter_mut().find(|c| c.refdes == name) {
                        c.theta = theta;
                        c.cth = cth;
                    }
                }
                draw_rise_chart(ui, v);
                if !v.moved.is_empty() {
                    ui.label(egui::RichText::new(format!("温まって動いた電圧 ({:.3} 秒のとき)", v.moved_at)).small().strong()).on_hover_text("冷えているときの電圧と、温まったあとの電圧の差。大きい順");
                    for (n, a, b) in &v.moved {
                        ui.label(egui::RichText::new(format!("{n}: {:.6} → {:.6} ({:+.3} mV)", a, b, (b - a) * 1e3)).small());
                    }
                }
                ui.label(egui::RichText::new("熱容量と、部品どうしの結合は推測の値です。数字の幅を見るために、変えて比べてください").small())
                    .on_hover_text("1つの部品を、熱容量と熱抵抗の1段として扱った簡単な近似です。基板の中の熱の広がりまでは計算していません");
            }
        }
        self.htran = Some(view);
    }
}

fn fmt_power(w: f64) -> String {
    if w >= 1.0 { format!("{w:.2} W") } else if w >= 1e-3 { format!("{:.1} mW", w * 1e3) } else { format!("{:.1} µW", w * 1e6) }
}

fn curve_color(k: usize) -> egui::Color32 {
    const COLORS: [egui::Color32; 5] = [
        egui::Color32::from_rgb(240, 150, 60),
        egui::Color32::from_rgb(90, 170, 240),
        egui::Color32::from_rgb(120, 210, 120),
        egui::Color32::from_rgb(220, 110, 200),
        egui::Color32::from_rgb(220, 210, 90),
    ];
    COLORS[k % COLORS.len()]
}

/// The temperature rise of each part over time, as lines.
fn draw_rise_chart(ui: &mut egui::Ui, v: &HeatTranView) {
    let top = v.parts.iter().map(|p| p.rise_peak).fold(0.0, f64::max).max(1e-9);
    let size = egui::vec2(ui.available_width().clamp(180.0, 420.0), 120.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
    resp.on_hover_text("各部品の、周囲の温度からの上昇 (K) の時間変化。横軸は時間、縦軸は上昇");
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
}
