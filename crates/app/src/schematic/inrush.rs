//! "突入電流" (candidate T3): the current each supply gives the moment it is switched on, while the capacitors charge from nothing,
//! against the parts on the way (`sim_core::inrush`).

use super::*;
use sim_core::inrush::{inrush, Inrush, InrushSettings};

/// The boxes of the section (as typed) and the last answer.
pub(crate) struct InrushUi {
    pub(super) r_source: String,
    pub(super) tstop: String,
    pub(super) view: Option<Result<(Vec<Inrush>, f64), String>>,
}

impl Default for InrushUi {
    fn default() -> Self {
        Self { r_source: "0.1".into(), tstop: "20m".into(), view: None }
    }
}

impl SchEditor {
    pub fn run_inrush(&mut self) {
        let result = (|| {
            let r_source = parse_value(&self.inrush.r_source).filter(|r| *r >= 0.0).ok_or("電源の抵抗を 0 以上の数 (Ω) で入れてください (例: 0.1)")?;
            let tstop = parse_value(&self.inrush.tstop).filter(|t| *t > 0.0).ok_or("見る時間を 0 より大きい数 (秒) で入れてください (例: 20m)")?;
            let c = self.sim_circuit().map_err(|e| e.join(" / "))?;
            inrush(&self.sch, &c, &InrushSettings { r_source, tstop }).map(|v| (v, r_source))
        })();
        self.status = match &result {
            Ok((v, _)) => format!("突入電流: {}", v.iter().map(|x| format!("{} ピーク {}A", x.source, sim_core::format_value(x.peak))).collect::<Vec<_>>().join("、")),
            Err(e) => format!("突入電流を出せませんでした: {e}"),
        };
        self.inrush.view = Some(result);
    }

    pub(super) fn panel_inrush(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(i18n::tr("電源の抵抗"));
            ui.add(egui::TextEdit::singleline(&mut self.inrush.r_source).desired_width(44.0)).on_hover_text(i18n::tr("電源と配線の抵抗 (Ω)。理想の電源は抵抗 0 なので、ここで実物に近づけます。AC アダプタとケーブルで 0.1〜0.5Ω、実験用電源のリード線で 0.05Ω くらい。小さいほど突入電流は大きくなります"));
            ui.label("Ω");
            ui.label(i18n::tr("見る時間"));
            ui.add(egui::TextEdit::singleline(&mut self.inrush.tstop).desired_width(44.0)).on_hover_text(i18n::tr("電源を入れてから何秒までを見るか (例: 20m)。コンデンサが充電し終わり、電流が落ち着くまで"));
            ui.label("s");
        });
        if ui.button(i18n::tr("電源を入れて、突入電流を見る")).on_hover_text(i18n::tr("コンデンサが空の状態で電源を入れる過渡解析をして、電源から流れる電流のピーク・I²t (ヒューズや接点を温める量)・続く時間と、通り道の部品の電流を、部品の定格と比べます")).clicked() {
            self.run_inrush();
        }
        let Some(view) = &self.inrush.view else { return };
        let (list, r_source) = match view {
            Ok((v, r)) => (v, *r),
            Err(e) => {
                ui.colored_label(crate::theme::error(), i18n::tr_dyn(e));
                return;
            }
        };
        let amps = |a: f64| format!("{}A", sim_core::format_value(a));
        for x in list {
            ui.separator();
            let color = if x.large() || x.parts.iter().any(|p| p.over()) { crate::theme::warn() } else { crate::theme::ok() };
            ui.label(egui::RichText::new(i18n::trf!("{} ({}V): ピーク {}", x.source, sim_core::format_value(x.volts), amps(x.peak))).strong().color(color));
            egui::Grid::new(("inrush_grid", &x.source)).num_columns(2).spacing([16.0, 3.0]).show(ui, |ui| {
                ui.label(i18n::tr("落ち着いたときの電流"));
                ui.label(amps(x.steady)).on_hover_text(i18n::tr("DC 動作点の電流。ピークがこの何倍かが、突入の大きさです"));
                ui.end_row();
                ui.label(i18n::tr("I²t"));
                ui.label(format!("{} A²s", sim_core::format_value(x.i2t))).on_hover_text(i18n::tr("電流の 2 乗を時間で積んだもの。ヒューズが溶ける・接点が傷むのは、この量で決まります。ヒューズのデータシートの「溶断 I²t」より十分小さいこと"));
                ui.end_row();
                ui.label(i18n::tr("続く時間"));
                ui.label(format!("{}s", sim_core::format_value(x.surge_s))).on_hover_text(i18n::tr("電流が、落ち着いたときの 2 倍とピークの 1 割のどちらよりも大きい間"));
                ui.end_row();
            });
            wave_plot(ui, x);
            for p in &x.parts {
                let text = match p.rated {
                    Some(r) => i18n::trf!("  {}: ピーク {} (定格 {})", p.part, amps(p.peak), amps(r)),
                    None => i18n::trf!("  {}: ピーク {}", p.part, amps(p.peak)),
                };
                let c = if p.over() { crate::theme::warn() } else { ui.visuals().text_color() };
                ui.label(egui::RichText::new(text).small().color(c));
            }
            if x.did_not_recover() {
                ui.label(egui::RichText::new(i18n::trf!("・電源を入れたあと、電流が落ち着いたときの値 ({}) に戻りません (終わりで {})。ヒューズが切れたか、何かがラッチした可能性があります", amps(x.steady), amps(x.end))).small().color(crate::theme::error()));
            }
            for p in x.parts.iter().filter(|p| p.over()) {
                ui.label(egui::RichText::new(i18n::trf!("・{} の電流が定格を超えます。ヒューズならスローブロー (タイムラグ) にするか、突入を抑えます。ダイオードはデータシートのサージ電流 (IFSM) と比べます", p.part)).small().color(crate::theme::warn()));
            }
            if x.large() {
                let ohms = x.limit_ohms(x.target(), r_source);
                ui.label(
                    egui::RichText::new(i18n::trf!(
                        "・ピークが、落ち着いたときの {:.0} 倍です。抑えるには、電源の入口に NTC サーミスタ (冷えているとき約 {}Ω で、温まると下がる) か抵抗を直列に入れる、レギュレータのソフトスタートを使う、入口のコンデンサを小さくする、があります",
                        x.peak / x.steady.max(1e-3),
                        sim_core::format_value(ohms)
                    ))
                    .small()
                    .color(crate::learn::NOTE),
                );
            }
        }
    }
}

/// The supply current over the first stretch of the run (three times the surge, at least a tenth of the run), as a small plot.
fn wave_plot(ui: &mut egui::Ui, x: &Inrush) {
    let (t, i) = &x.wave;
    let total = t.last().copied().unwrap_or(0.0);
    if t.len() < 2 || total <= 0.0 || x.peak <= 0.0 {
        return;
    }
    let span = (3.0 * x.surge_s).max(0.1 * total).min(total);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().min(360.0), 80.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_stroke(rect, 2.0, egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color), egui::StrokeKind::Inside);
    let pts: Vec<egui::Pos2> = t
        .iter()
        .zip(i)
        .filter(|(tt, _)| **tt <= span)
        .map(|(tt, ii)| egui::pos2(rect.left() + (*tt / span) as f32 * rect.width(), rect.bottom() - 4.0 - (ii.abs() / x.peak) as f32 * (rect.height() - 8.0)))
        .collect();
    p.add(egui::Shape::line(pts, egui::Stroke::new(1.5, crate::theme::warn())));
    let small = egui::FontId::proportional(10.0);
    p.text(rect.left_top() + egui::vec2(4.0, 2.0), egui::Align2::LEFT_TOP, format!("{}A", sim_core::format_value(x.peak)), small.clone(), ui.visuals().weak_text_color());
    p.text(rect.right_bottom() - egui::vec2(4.0, 2.0), egui::Align2::RIGHT_BOTTOM, format!("{}s", sim_core::format_value(span)), small, ui.visuals().weak_text_color());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sample: 12 V → 1N4007 → 1000 µF → LM7805: a large peak, the diode over its 1 A, and the section draws.
    #[test]
    fn the_sample_shows_a_large_inrush() {
        let mut e = SchEditor::default();
        e.load_path(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/inrush.schjson"));
        e.run_inrush();
        let (list, _) = e.inrush.view.as_ref().unwrap().as_ref().unwrap();
        let x = &list[0];
        assert!(x.large(), "{x:?}");
        assert!(x.parts.iter().any(|p| p.part == "D1" && p.over()), "{:?}", x.parts);
        let ctx = egui::Context::default();
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 900.0))), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| e.panel_inrush(ui));
        out.textures_delta.clear();
    }
}
