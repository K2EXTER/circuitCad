//! "電源の予算表" (candidate N2): every supply rail at the DC operating point as a tree (battery → 5 V → 3.3 V), with what each
//! gives, its rated current, how much more it could give and what stops it, and a regulator's heat and efficiency
//! (`sim_core::budget::power_budget`).

use super::*;
use sim_core::budget::{power_budget, Rail};

impl SchEditor {
    /// Solves the DC operating point and makes the budget of its rails.
    pub fn run_budget(&mut self) {
        let ambient = self.temp_c.trim().parse::<f64>().unwrap_or(T_NOM_C);
        let result = (|| {
            let c = self.sim_circuit().map_err(|e| e.join(" / "))?;
            let dc = dc_op(&c).map_err(|e| format!("DC解析ができませんでした: {e}"))?;
            Ok(power_budget(&self.sch, &c, &dc, ambient))
        })();
        self.status = match &result {
            Ok(r) if r.is_empty() => "電源の予算表: 電源のレールが見つかりませんでした".to_string(),
            Ok(r) => {
                let tight: Vec<&str> = r.iter().filter(|x| x.tight()).map(|x| x.net.as_str()).collect();
                if tight.is_empty() {
                    format!("電源の予算表: レール {} 本、どれも余裕があります", r.len())
                } else {
                    format!("電源の予算表: 余裕が少ないレールがあります ({})", tight.join("、"))
                }
            }
            Err(e) => format!("電源の予算表を出せませんでした: {e}"),
        };
        self.budget = Some(result);
    }

    pub(super) fn panel_budget(&mut self, ui: &mut egui::Ui) {
        if ui
            .button(i18n::tr("DC動作点を解いて、電源の予算表を出す"))
            .on_hover_text(i18n::tr("電源のレール (電圧源の + 側と、レギュレータの出力) ごとに、流れている電流・部品の定格・あと何 mA 足せるか・レギュレータの発熱と効率を、つながりの順 (電池 → 5V → 3.3V) に並べます"))
            .clicked()
        {
            self.run_budget();
        }
        let Some(view) = &self.budget else { return };
        let rails = match view {
            Ok(r) => r,
            Err(e) => {
                ui.colored_label(crate::theme::error(), i18n::tr_dyn(e));
                return;
            }
        };
        if rails.is_empty() {
            ui.label(egui::RichText::new(i18n::tr("電源のレールが見つかりません。− が GND の電圧源の + 側と、IN と OUT のピンがあるレギュレータの出力を、レールとして数えます")).small());
            return;
        }
        let amps = |a: f64| format!("{}A", sim_core::format_value(a));
        egui::Grid::new("budget_grid").num_columns(6).striped(true).spacing([12.0, 3.0]).show(ui, |ui| {
            for h in ["レール", "電圧", "電流", "定格", "あと", "発熱・温度"] {
                ui.label(egui::RichText::new(i18n::tr(h)).strong());
            }
            ui.end_row();
            for r in rails {
                let indent = if r.depth == 0 { String::new() } else { format!("{}└ ", "　".repeat(r.depth - 1)) };
                ui.label(format!("{indent}{} ({})", r.net, r.source)).on_hover_text(loads_text(r));
                ui.label(format!("{}V", sim_core::format_value(r.volts)));
                ui.label(amps(r.amps)).on_hover_text(i18n::tr("このレールの電源 (電圧源・レギュレータ) が出している電流"));
                match r.rated {
                    Some(i) => ui.label(amps(i)).on_hover_text(i18n::tr("部品のデータの最大電流 (データシートの定格)")),
                    None => ui.label("—").on_hover_text(i18n::tr("部品のデータに定格の電流がありません (理想の電圧源など)")),
                };
                match r.headroom {
                    Some((h, why)) => {
                        let color = if h < 0.0 { crate::theme::error() } else if r.tight() { crate::theme::warn() } else { crate::theme::ok() };
                        let text = if h < 0.0 { i18n::trf!("{} 超過", amps(-h)) } else { amps(h) };
                        ui.label(egui::RichText::new(text).color(color)).on_hover_text(i18n::trf!("このレールに、あと何 A つなげるか。決めているのは「{}」です", i18n::tr(why.label())));
                    }
                    None => {
                        ui.label("—").on_hover_text(i18n::tr("電源の部品に定格の電流がないので、決められません"));
                    }
                }
                match (r.heat_w, r.temp_c) {
                    (Some(p), Some(t)) => {
                        let hot = r.t_max.is_some_and(|m| t > m - 20.0);
                        let eff = r.efficiency.map_or(String::new(), |e| format!("・効率 {:.0} %", e * 100.0));
                        let text = format!("{}W・{:.0}℃{eff}", sim_core::format_value(p), t);
                        ui.label(egui::RichText::new(text).color(if hot { crate::theme::warn() } else { ui.visuals().text_color() })).on_hover_text(i18n::trf!(
                            "レギュレータが熱にする電力と、そのときの部品の温度の目安 (上限 {}℃)。リニアレギュレータは (入力 − 出力) × 電流 が熱になります。温度は部品の θ (データシートの θJA か、パッケージの目安) から出し、基板の銅で下がります (「自己発熱」の解析で詳しく)",
                            r.t_max.map_or("?".to_string(), |m| format!("{m:.0}"))
                        ));
                    }
                    _ => {
                        ui.label("");
                    }
                }
                ui.end_row();
            }
        });
        for r in rails.iter().filter(|r| r.headroom.is_some_and(|(h, _)| h < 0.0) || r.tight()) {
            let why = r.headroom.map_or("", |(_, w)| w.label());
            ui.label(egui::RichText::new(i18n::trf!("・{} は余裕が少ないです (決めているのは「{}」)。負荷を減らすか、電流の大きいレギュレータ・放熱・入力の電圧を下げる、を考えましょう", r.net, i18n::tr(why))).small().color(crate::theme::warn()));
        }
        ui.label(egui::RichText::new(i18n::tr("・レールの名前にマウスを乗せると、そのレールから電流をとっている部品が出ます。DC の値なので、パルスで流れる電流 (モーターの起動・無線の送信) は入りません")).small().color(crate::learn::NOTE));
    }
}

/// The loads of a rail, largest first, for the tooltip.
fn loads_text(r: &Rail) -> String {
    let mut lines = vec![i18n::trf!("{} から電流をとっている部品:", r.net)];
    for l in &r.loads {
        let share = if r.amps > 0.0 { l.amps / r.amps * 100.0 } else { 0.0 };
        let feeds = if l.feeds_rail { i18n::tr(" (下のレールのレギュレータ)").to_string() } else { String::new() };
        lines.push(format!("  {} {}A ({share:.0} %){feeds}", l.part, sim_core::format_value(l.amps)));
    }
    if r.loads.is_empty() {
        lines.push(i18n::tr("  (なし)").to_string());
    }
    if let Some((v, i)) = r.input {
        lines.push(i18n::trf!("入力: {}V・{}A", sim_core::format_value(v), sim_core::format_value(i)));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sample: 9 V → LM7805 → 5 V → MCP1700 → 3.3 V, each with a load; the table draws, and the 5 V rail's heat is what stops it.
    #[test]
    fn the_sample_makes_a_tree_of_three_rails() {
        let mut e = SchEditor::default();
        e.load_path(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/power_budget.schjson"));
        e.run_budget();
        let rails = e.budget.as_ref().unwrap().as_ref().unwrap();
        let names: Vec<(&str, usize)> = rails.iter().map(|r| (r.net.as_str(), r.depth)).collect();
        assert_eq!(names, [("VIN", 0), ("5V", 1), ("3V3", 2)], "{rails:?}");
        assert_eq!(rails[2].parent, Some(1));
        // no ERC findings: the sample carries the capacitors the data sheets ask for
        assert!(sch_core::erc(&e.sch).is_empty(), "{:?}", sch_core::erc(&e.sch));
        let ctx = egui::Context::default();
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 900.0))), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| e.panel_budget(ui));
        out.textures_delta.clear();
    }
}
