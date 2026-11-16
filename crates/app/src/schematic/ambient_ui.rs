//! "周囲の温度を振る" under the "自己発熱" section (candidate H8): the ambient swept with the self-heating (`sim_core::ambient_sweep`),
//! summed up as the worst case of each part: its hottest, the ambient where it reaches its limit, and the nets that move.

use super::*;

impl SchEditor {
    pub(crate) fn run_ambient_sweep(&mut self) -> Result<sim_core::AmbientSweep, String> {
        let num = |s: &str, what: &str| s.trim().parse::<f64>().ok().filter(|x| x.is_finite()).ok_or_else(|| i18n::trf!("{} を数値 (℃) で入力してください", what));
        let (t0, t1, step) = (num(&self.amb_from, "開始")?, num(&self.amb_to, "終了")?, num(&self.amb_step, "刻み")?);
        let c = sim_core::from_schematic_with_layout(&self.sch, &self.wiring, &self.couplings).map_err(|e| e.first().map(|x| x.to_string()).unwrap_or_default())?;
        let heaters = sim_core::heaters_from_schematic(&self.sch, &c, &self.copper_g());
        // with the board carried over, its matrix at the middle of the range (its h hardly moves with the air)
        let (heaters, mutual) = self.heaters_on_board(&c, heaters, 0.5 * (t0 + t1))?;
        sim_core::ambient_sweep(&self.sch, &c, &heaters, &mutual, t0, t1, step).map_err(|e| e.to_string())
    }

    pub(super) fn panel_ambient_sweep(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label(egui::RichText::new(i18n::tr("周囲の温度を振る (最悪ケース)")).strong()).on_hover_text(i18n::tr(
            "周囲の温度を変えながら、その温度ごとに自己発熱を計算します。部品は「周囲 + 自分の熱の上昇」になるので、周囲が上がるほど上限までの余裕が減ります。温まると電流が増える回路 (エミッタ抵抗の無いトランジスタなど) は、周囲が高いほど上昇も大きくなります。ふつうの温度スイープ (温度とばらつき) は、部品を周囲と同じ温度にします",
        ));
        ui.horizontal(|ui| {
            for (s, tip) in [(&mut self.amb_from, "開始 (℃)"), (&mut self.amb_to, "終了 (℃)"), (&mut self.amb_step, "刻み (K)")] {
                ui.add(egui::TextEdit::singleline(s).desired_width(36.0)).on_hover_text(i18n::tr(tip));
            }
            ui.label("℃");
            if ui.button(i18n::tr("周囲の温度を振る")).on_hover_text(i18n::tr("例: −20 〜 85 ℃ を 5 K 刻み。製品の使う温度の範囲 (民生 0〜70 ℃、産業 −40〜85 ℃ など) にします")).clicked() {
                self.amb_view = Some(self.run_ambient_sweep());
            }
        });
        let Some(view) = &self.amb_view else { return };
        let r = match view {
            Ok(r) => r,
            Err(e) => {
                ui.colored_label(crate::theme::ERROR, e);
                return;
            }
        };
        if let Some((t, why)) = &r.runaway {
            ui.colored_label(crate::theme::ERROR, i18n::trf!("周囲 {:.0} ℃ で落ち着きませんでした (熱暴走): {}", t, why));
        }
        egui::Grid::new("ambient_worst").striped(true).num_columns(5).show(ui, |ui| {
            for (h, tip) in [("部品", ""), ("いちばん熱い", "範囲の中で、部品がいちばん熱くなる温度と、そのときの周囲の温度"), ("上限", "部品の最高温度 (「?」は種類からの目安)"), ("上限に届く周囲", "部品が上限の温度に届く周囲の温度 (点の間は直線で)。これより暑い所では使えません"), ("上昇", "周囲の温度より、どれだけ熱いか (範囲の低い端 → 高い端)。温まると電力が増える部品は、高い端で大きくなります")] {
                let l = ui.label(egui::RichText::new(i18n::tr(h)).small().strong());
                if !tip.is_empty() {
                    l.on_hover_text(i18n::tr(tip));
                }
            }
            ui.end_row();
            for w in &r.worst {
                ui.label(&w.name);
                let col = if w.over_from.is_some() {
                    crate::theme::ERROR
                } else if w.caution_from.is_some() {
                    crate::theme::WARN
                } else {
                    ui.visuals().text_color()
                };
                ui.label(egui::RichText::new(format!("{:.0} ℃ ({:.0})", w.hottest_c, w.at_ambient)).color(col));
                ui.label(format!("{:.0}{}", w.t_max, if w.guessed_limit { "?" } else { "" }));
                ui.label(match w.over_from {
                    Some(t) => egui::RichText::new(format!("{t:.0} ℃")).color(crate::theme::ERROR),
                    None => egui::RichText::new(i18n::tr("届かない")).weak(),
                });
                ui.label(format!("{:.1} → {:.1} K", w.rise_low, w.rise_high));
                ui.end_row();
            }
        });
        for w in r.worst.iter().filter(|w| w.derated_over) {
            ui.colored_label(crate::theme::WARN, i18n::trf!("⚠ {} は、周囲が高いところで、温度で下げた定格電力を超えます", w.name));
        }
        if !r.nets.is_empty() {
            ui.label(egui::RichText::new(i18n::tr("周囲の温度で動くところ")).small().strong()).on_hover_text(i18n::tr("範囲の低い端と高い端での電圧。大きく動く順"));
            for n in &r.nets {
                ui.label(egui::RichText::new(format!("{}: {:.4} V → {:.4} V ({:+.1} mV)", n.net, n.at_low, n.at_high, (n.at_high - n.at_low) * 1e3)).small());
            }
        }
        draw_ambient_chart(ui, r);
    }
}

/// Each part's temperature against the ambient, with its limit dashed.
fn draw_ambient_chart(ui: &mut egui::Ui, r: &sim_core::AmbientSweep) {
    let (Some(first), Some(last)) = (r.points.first(), r.points.last()) else { return };
    if r.points.len() < 2 {
        return;
    }
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(260.0), 140.0), egui::Sense::hover());
    let plot = egui::Rect::from_min_max(rect.min + egui::vec2(40.0, 6.0), rect.max - egui::vec2(8.0, 16.0));
    let painter = ui.painter_at(rect);
    let weak = ui.visuals().weak_text_color();
    painter.rect_stroke(plot, 0.0, egui::Stroke::new(1.0, weak), egui::StrokeKind::Inside);
    let (a0, a1) = (first.ambient, last.ambient);
    let lo = a0.min(r.points.iter().flat_map(|p| p.parts.iter().map(|x| x.temp_c)).fold(a0, f64::min));
    let hi = r.points.iter().flat_map(|p| p.parts.iter().map(|x| x.temp_c)).fold(a1, f64::max).max(r.worst.iter().map(|w| w.t_max).fold(f64::MIN, f64::max).min(lo + 4.0 * (a1 - lo).max(10.0)));
    let x = |t: f64| plot.left() + plot.width() * ((t - a0) / (a1 - a0).max(1e-9)) as f32;
    let y = |c: f64| plot.bottom() - plot.height() * ((c - lo) / (hi - lo).max(1e-9)) as f32;
    let font = egui::FontId::proportional(11.0);
    painter.text(egui::pos2(plot.left() - 3.0, plot.top()), egui::Align2::RIGHT_TOP, format!("{hi:.0}℃"), font.clone(), weak);
    painter.text(egui::pos2(plot.left() - 3.0, plot.bottom()), egui::Align2::RIGHT_BOTTOM, format!("{lo:.0}℃"), font.clone(), weak);
    painter.text(egui::pos2(plot.left(), plot.bottom() + 2.0), egui::Align2::LEFT_TOP, i18n::trf!("周囲 {a0:.0} ℃"), font.clone(), weak);
    painter.text(egui::pos2(plot.right(), plot.bottom() + 2.0), egui::Align2::RIGHT_TOP, format!("{a1:.0} ℃"), font.clone(), weak);
    for (k, w) in r.worst.iter().enumerate() {
        let col = crate::theme::series_color(k);
        let pts: Vec<egui::Pos2> = r.points.iter().filter_map(|p| p.parts.iter().find(|hp| hp.name == w.name).map(|hp| egui::pos2(x(p.ambient), y(hp.temp_c)))).collect();
        painter.add(egui::Shape::line(pts, egui::Stroke::new(1.5, col)));
        if w.t_max <= hi {
            let yl = y(w.t_max);
            painter.add(egui::Shape::dashed_line(&[egui::pos2(plot.left(), yl), egui::pos2(plot.right(), yl)], egui::Stroke::new(1.0, col), 5.0, 4.0));
        }
        if let Some(p) = r.points.last().and_then(|p| p.parts.iter().find(|x| x.name == w.name)) {
            painter.text(egui::pos2(plot.right() - 2.0, (y(p.temp_c) - 1.0).max(plot.top() + 12.0)), egui::Align2::RIGHT_BOTTOM, &w.name, font.clone(), col);
        }
    }
}
