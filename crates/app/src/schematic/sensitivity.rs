//! Sensitivity: every part moved by ±1 % on its own, the parts listed by how much they move the measured result.

use super::*;
use sim_core::{sensitivity, SensSettings, Sensitivity};

/// A finished sensitivity run.
pub(super) struct SensView {
    pub(super) outcome: Result<Sensitivity, Vec<String>>,
}

/// Below this elasticity (%/%) a part counts as not mattering.
const SMALL: f64 = 0.01;
/// The longest list drawn; the rest are counted.
const SHOWN: usize = 40;

fn unit_of_part(name: &str) -> &'static str {
    match name.chars().next().map(|c| c.to_ascii_uppercase()) {
        Some('R') => "Ω",
        Some('C') => "F",
        Some('L') => "H",
        Some('V') => "V",
        Some('I') => "A",
        _ => "",
    }
}

fn fmt_result(v: f64, unit: &str) -> String {
    if unit == "%" { format!("{v:.2} %") } else { eng(v, unit) }
}

impl SchEditor {
    pub fn analyze_sensitivity(&mut self, node: &str, ac: bool) {
        self.mc_out = node.into();
        self.mc_kind = if ac { McKind::Ac } else { McKind::Dc };
        self.run_sens();
    }

    /// Every part moved by 1 % up and down on its own: which ones set the measured result.
    pub(super) fn run_sens(&mut self) {
        let outcome = (|| {
            let measure = self.mc_measure().map_err(|e| vec![e])?;
            let output = self.mc_out.trim().to_string();
            if output.is_empty() {
                return Err(vec!["測る点を選んでください".to_string()]);
            }
            let c = self.sim_circuit()?;
            sensitivity(&c, &SensSettings { output, measure, step: 0.01 }).map_err(|e| vec![e.to_string()])
        })();
        self.status = match &outcome {
            Ok(r) => match r.parts.first() {
                Some(p) => format!("感度: 完了 ({} 個の部品。いちばん効くのは {})", r.parts.len(), p.name),
                None => "感度: 完了".into(),
            },
            Err(e) => format!("感度の計算に失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        self.sens = Some(SensView { outcome });
        self.wave_mode = WaveMode::Sens;
    }

    pub(super) fn sens_wave_panel(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.sens.as_ref() else { return };
        let mut close = false;
        let mut pick: Option<String> = None;
        ui.horizontal(|ui| {
            crate::widgets::heading_inline(ui, i18n::tr("感度 (どの部品が結果を決めているか)"));
            if crate::widgets::close_x(ui, "感度の表示") {
                close = true;
            }
        });
        match &view.outcome {
            Err(msgs) => {
                for m in msgs {
                    ui.colored_label(crate::theme::ERROR, m);
                }
            }
            Ok(res) => {
                let what = match res.unit {
                    "Hz" => i18n::trf!("{} のカットオフ周波数", res.output),
                    "V" => i18n::trf!("V({})", res.output),
                    _ => i18n::trf!("{} のステップ応答", res.output),
                };
                ui.label(i18n::trf!("{what} = {} のとき、部品を 1 つずつ ±{:.0}% 動かして、結果がどれだけ動くかを見ました", fmt_result(res.nominal, res.unit), res.step * 100.0));
                // the headline: the part that matters most, said as a sentence
                if let Some(p) = res.parts.first() {
                    match p.elasticity {
                        Some(e) if e.abs() >= SMALL => {
                            let dir = if e * res.nominal.signum() >= 0.0 { i18n::tr("上がり") } else { i18n::tr("下がり") };
                            ui.label(egui::RichText::new(i18n::trf!("{} が 1% 大きいと、結果は {:.2}% {dir}ます。精度が要るなら、まず {} を誤差の小さい部品にします", p.name, e.abs(), p.name)).strong());
                        }
                        _ => {
                            ui.label(i18n::tr("どの部品を動かしても、結果はほとんど動きません。値を決めているのは IC やダイオードのような、値を動かせない部品です"));
                        }
                    }
                }
                let quiet = res.parts.iter().filter(|p| p.elasticity.is_some_and(|e| e.abs() < SMALL)).count();
                if quiet > 0 {
                    ui.small(i18n::trf!("{quiet} 個の部品は、ほとんど効きません (1% 動かして結果が 0.01% 未満)。安い部品でかまいません"));
                }
                if !res.unsolved.is_empty() {
                    ui.colored_label(crate::theme::WARN, i18n::trf!("動かすと計算できなかった部品: {}", res.unsolved.join(", ")));
                }
                let biggest = res.parts.iter().filter_map(|p| p.elasticity).fold(0.0f64, |m, e| m.max(e.abs())).max(1.0);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    egui::Grid::new("sens_rows").num_columns(5).striped(true).show(ui, |ui| {
                        ui.label(i18n::tr("部品")).on_hover_text(i18n::tr("クリックすると、回路図でその部品を選びます"));
                        ui.label(i18n::tr("値"));
                        ui.label(i18n::tr("1% 大きいと")).on_hover_text(i18n::tr("その部品だけを 1% 大きくしたとき、結果がどれだけ変わるか (結果の単位で)"));
                        ui.label(i18n::tr("効き (%/%)")).on_hover_text(i18n::tr("部品が 1% 変わると結果が何 % 変わるか。1 なら同じだけ、0.5 なら半分、マイナスなら逆向きに動きます。種類の違う部品 (抵抗と電源など) を、同じものさしで比べられます"));
                        ui.label("");
                        ui.end_row();
                        for p in res.parts.iter().take(SHOWN) {
                            let quiet = p.elasticity.is_none_or(|e| e.abs() < SMALL);
                            let name = if quiet { egui::RichText::new(&p.name).weak() } else { egui::RichText::new(&p.name) };
                            if ui.add(egui::Label::new(name).sense(Sense::click())).on_hover_text(i18n::tr("クリックすると、回路図でその部品を選びます")).clicked() {
                                pick = Some(p.name.clone());
                            }
                            ui.label(eng(p.value, unit_of_part(&p.name)));
                            let sign = if p.per_percent >= 0.0 { "+" } else { "−" };
                            ui.label(format!("{sign}{}", fmt_result(p.per_percent.abs(), res.unit)));
                            let el = p.elasticity.map_or("—".to_string(), |e| format!("{e:+.3}"));
                            let r = ui.label(el);
                            if p.bent {
                                r.on_hover_text(i18n::tr("大きくしたときと小さくしたときで、動く量が違います (まっすぐに動かない)。大きく動かすと、この数字からは外れます"));
                            }
                            // a bar from the middle: right for "grows with the part", left for "falls"
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(160.0, 14.0), Sense::hover());
                            let mid = rect.center().x;
                            ui.painter().line_segment([egui::pos2(mid, rect.top()), egui::pos2(mid, rect.bottom())], ui.visuals().widgets.noninteractive.bg_stroke);
                            if let Some(e) = p.elasticity {
                                let w = (e / biggest).clamp(-1.0, 1.0) as f32 * rect.width() / 2.0;
                                let bar = egui::Rect::from_min_max(egui::pos2(mid.min(mid + w), rect.top() + 2.0), egui::pos2(mid.max(mid + w), rect.bottom() - 2.0));
                                ui.painter().rect_filled(bar, 1.0, if e >= 0.0 { crate::theme::INFO } else { crate::theme::HIGHLIGHT });
                            }
                            ui.end_row();
                        }
                    });
                    if res.parts.len() > SHOWN {
                        ui.small(i18n::trf!("ほか {} 個 (効きの小さい部品)", res.parts.len() - SHOWN));
                    }
                });
            }
        }
        if close {
            self.sens = None;
        }
        if let Some(name) = pick {
            self.select_part(&name);
        }
    }
}
