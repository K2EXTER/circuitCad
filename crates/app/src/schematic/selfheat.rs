//! The "自己発熱" section of the analysis tab: the resistors warmed by their own power (`sim_core::self_heat`).

use super::*;

/// What the last run found.
pub(super) struct HeatView {
    pub ambient: f64,
    pub parts: Vec<sim_core::HeatedPart>,
    /// (net, volts cold, volts warm) for the nets that moved the most.
    pub nodes: Vec<(String, f64, f64)>,
}

impl SchEditor {
    pub(super) fn run_self_heat(&mut self) -> Result<HeatView, String> {
        let ambient: f64 = self.heat_ambient.trim().parse().ok().filter(|t: &f64| *t > -273.0 && *t < 1000.0).ok_or("周囲の温度を数値(℃)で入力してください (例: 27)")?;
        let c = sim_core::from_schematic_with_layout(&self.sch, &self.wiring, &self.couplings).map_err(|e| e.first().map(|x| x.to_string()).unwrap_or_default())?;
        let heaters = sim_core::heaters_from_schematic(&self.sch, &c);
        let r = sim_core::self_heat(&c, &heaters, ambient).map_err(|e| e.to_string())?;
        let mut parts: Vec<_> = r.parts.iter().filter(|p| p.power > 1e-9).cloned().collect();
        parts.sort_by(|a, b| b.rise.total_cmp(&a.rise));
        let mut nodes: Vec<(String, f64, f64)> = r.warm.voltages.iter().filter(|(n, _)| !n.contains('.')).filter_map(|(n, v)| r.cold.voltage(n).map(|c0| (n.clone(), c0, *v))).filter(|(_, a, b)| (a - b).abs() > 1e-9).collect();
        nodes.sort_by(|a, b| (b.2 - b.1).abs().total_cmp(&(a.2 - a.1).abs()));
        nodes.truncate(8);
        Ok(HeatView { ambient, parts, nodes })
    }

    pub(super) fn panel_selfheat(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("周囲の温度");
            ui.add(egui::TextEdit::singleline(&mut self.heat_ambient).desired_width(40.0)).on_hover_text("部品のまわりの空気の温度 (℃)。部品の温度は、これに「電力 × 熱抵抗」を足したものになります");
            ui.label("℃");
        });
        if self.lit(ui.button("自己発熱を計算"), "自己発熱を計算").on_hover_text("電流が流れる抵抗は、自分の電力で温まります。温まると値が変わる抵抗 (温度係数のある部品) は、電流も変わるので、落ち着くまでくり返し計算します。直流の定常状態です (短いパルスでは、これより低くなります)").clicked() {
            self.heat_view = Some(self.run_self_heat());
            if let Some(Err(e)) = &self.heat_view {
                self.status = format!("自己発熱の計算に失敗: {e}");
            }
        }
        let Some(view) = self.heat_view.take() else {
            ui.label(egui::RichText::new("電流が流れる抵抗の温度上昇と、それで値がどれだけ動くかを見ます").small());
            return;
        };
        match &view {
            Err(e) => {
                ui.colored_label(egui::Color32::from_rgb(230, 90, 70), e).on_hover_text("電源や電流の流れ方を確認してください。DC解析が収束しない回路は計算できません");
            }
            Ok(v) => {
                if v.parts.is_empty() {
                    ui.label("電力が出ている抵抗はありません");
                }
                let mut edits: Vec<(String, f64, f64)> = vec![];
                egui::Grid::new("selfheat_grid").num_columns(6).striped(true).show(ui, |ui| {
                    for h in ["部品", "電力", "上昇", "値の変化", "熱抵抗 K/W", "係数 ppm/K"] {
                        ui.label(egui::RichText::new(h).small().strong());
                    }
                    ui.end_row();
                    for p in &v.parts {
                        ui.label(&p.name).on_hover_text(format!("温度 {:.1} ℃ (周囲 {} ℃ + {:.1} K)", p.temp_c, v.ambient, p.rise));
                        ui.label(fmt_power(p.power));
                        ui.label(format!("{:.1} K", p.rise)).on_hover_text("周囲の温度より、これだけ高くなります");
                        ui.label(format!("{:+.0} ppm", p.change * 1e6)).on_hover_text("温まったことで、抵抗値が冷えているときよりどれだけ変わったか (100 ppm = 0.01 %)。温度係数が 0 なら変わりません");
                        let (mut theta, mut tc) = (p.theta, p.tc1 * 1e6);
                        let a = ui.add(egui::DragValue::new(&mut theta).range(0.0..=1000.0).speed(0.5)).on_hover_text("熱抵抗: 抵抗体から空気までの熱の伝わりにくさ (K/W)。メーカーの値 (抵抗体〜端子間) に、基板の銅箔での放熱ぶんが足されます。放熱の良い大きな銅箔・厚い銅箔・多層にすると小さくなります。変えたら、もう一度計算してください");
                        let b = ui.add(egui::DragValue::new(&mut tc).range(-5000.0..=5000.0).speed(1.0)).on_hover_text("温度係数: 1 K 温まると抵抗値が何 ppm 変わるか。データシートの値 (シャント抵抗なら ±25 〜 ±100 ppm/K など) を入れます");
                        if a.changed() || b.changed() {
                            edits.push((p.name.clone(), theta, tc));
                        }
                        ui.end_row();
                    }
                });
                for (name, theta, tc) in edits {
                    if let Some(c) = self.sch.components.iter_mut().find(|c| c.refdes == name) {
                        c.theta = theta;
                        c.tc_ppm = tc;
                    }
                }
                if !v.nodes.is_empty() {
                    ui.label(egui::RichText::new("温まって動いたところ").small().strong()).on_hover_text("冷えているときの電圧と、温まったあとの電圧の差。大きい順");
                    for (n, a, b) in &v.nodes {
                        ui.label(egui::RichText::new(format!("{n}: {:.6} V → {:.6} V ({:+.3} mV)", a, b, (b - a) * 1e3)).small());
                    }
                }
                ui.label(egui::RichText::new("熱抵抗を入れていない部品は、パッケージの大きさからの目安 (定格電力から) です。データシートの値に直してください").small()).on_hover_text("チップ抵抗は、周囲 70 ℃ で定格の電力をかけると 155 ℃ になるものとして、θ ≈ 85 K ÷ 定格電力 としています");
            }
        }
        self.heat_view = Some(view);
    }
}

fn fmt_power(w: f64) -> String {
    if w >= 1.0 { format!("{w:.2} W") } else if w >= 1e-3 { format!("{:.1} mW", w * 1e3) } else { format!("{:.1} µW", w * 1e6) }
}
