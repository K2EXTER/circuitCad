//! Parameter sweep: one part's value stepped through a range, the curves drawn on top of each other.

use super::*;
use sim_core::{param_sweep, sweep_values, ParamAnalysis, ParamResult, ParamSettings, MAX_RUNS};

/// A finished parameter sweep.
pub(super) struct ParamView {
    outcome: Result<ParamResult, Vec<String>>,
    /// Frequency response (dB over Hz) instead of a transient (V over s).
    ac: bool,
    part: String,
    node: String,
    plot: PlotState,
}

impl SchEditor {
    pub fn set_param_settings(&mut self, part: &str, from: &str, to: &str, n: &str, node: &str, ac: bool) {
        self.param_part = part.into();
        self.param_from = from.into();
        self.param_to = to.into();
        self.param_n = n.into();
        self.param_node = node.into();
        self.param_ac = ac;
    }

    pub fn analyze_param(&mut self) {
        self.run_param();
    }

    /// Parts whose value is a plain number (R, C, L): the ones that can be swept.
    fn sweepable(&self) -> Vec<(String, String)> {
        self.sch
            .components
            .iter()
            .filter(|c| matches!(c.kind, Kind::Resistor | Kind::Capacitor | Kind::Inductor))
            .map(|c| (c.refdes.clone(), c.value.clone()))
            .collect()
    }

    /// Picks a part and proposes a range of 1/10 to 10 times its value.
    fn param_choose(&mut self, refdes: &str, value: &str) {
        self.param_part = refdes.into();
        if let Some(v) = parse_value(value.split_whitespace().next().unwrap_or("")).filter(|v| *v > 0.0) {
            self.param_from = format_value(v / 10.0);
            self.param_to = format_value(v * 10.0);
            self.param_log = true;
        }
    }

    pub(super) fn panel_param(&mut self, ui: &mut egui::Ui) {
        ui.small("部品の値を何通りか変えて同じ解析をくり返し、波形を重ねて表示します。「Rを変えるとカットオフがどう動くか」を一度に見られます");
        let parts = self.sweepable();
        let selected_part = match self.selected {
            Some(Sel::Comp(i)) => self.sch.components.get(i).map(|c| (c.refdes.clone(), c.value.clone())),
            _ => None,
        };
        ui.horizontal(|ui| {
            ui.label("部品");
            let mut pick = None;
            let shown = if self.param_part.is_empty() { "選ぶ".to_string() } else { self.param_part.clone() };
            egui::ComboBox::from_id_salt("param_part")
                .width(70.0)
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for (r, v) in &parts {
                        if ui.selectable_label(*r == self.param_part, format!("{r} ({v})")).clicked() {
                            pick = Some((r.clone(), v.clone()));
                        }
                    }
                })
                .response
                .on_hover_text("値を変える部品 (抵抗・コンデンサ・コイル)。選ぶと、今の値の 1/10 〜 10倍が範囲に入ります");
            if pick.is_none()
                && let Some(sel) = selected_part.filter(|(r, _)| parts.iter().any(|p| &p.0 == r))
                && ui.small_button("選択中の部品").on_hover_text("回路図で選んでいる部品をスイープ対象にします").clicked()
            {
                pick = Some(sel);
            }
            if let Some((r, v)) = pick {
                self.param_choose(&r, &v);
            }
        });
        ui.horizontal(|ui| {
            ui.label("から");
            ui.add(egui::TextEdit::singleline(&mut self.param_from).desired_width(48.0)).on_hover_text("最初の値。例: 100, 1k, 10n");
            ui.label("まで");
            ui.add(egui::TextEdit::singleline(&mut self.param_to).desired_width(48.0)).on_hover_text("最後の値");
            ui.label("本数");
            ui.add(egui::TextEdit::singleline(&mut self.param_n).desired_width(24.0)).on_hover_text(format!("値を何通り試すか (最大 {MAX_RUNS})。5 くらいが見やすいです"));
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.param_log, "対数で等間隔").on_hover_text("オンにすると 100, 316, 1k, 3.16k, 10k のように倍率が一定になります。部品の値を振るときは普通こちらが見やすいです。オフだと足し算で等間隔です");
            ui.label("見る点");
            let nets: Vec<String> = extract(&self.sch).into_iter().map(|n| n.name).filter(|n| !n.contains('.')).collect();
            let shown = if self.param_node.is_empty() { "選ぶ".to_string() } else { self.param_node.clone() };
            egui::ComboBox::from_id_salt("param_node")
                .width(70.0)
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for n in &nets {
                        ui.selectable_value(&mut self.param_node, n.clone(), n);
                    }
                })
                .response
                .on_hover_text("波形を見るノード (ネット) の名前。出力にあたる点を選びます");
        });
        ui.horizontal(|ui| {
            ui.label("解析");
            ui.selectable_value(&mut self.param_ac, false, "過渡").on_hover_text("時間変化 (過渡解析の設定を使います)。ステップ応答のなまり方、発振の減り方などを比べられます");
            ui.selectable_value(&mut self.param_ac, true, "AC").on_hover_text("周波数特性 (AC解析の設定を使います)。カットオフ周波数や共振の位置がどう動くかを比べられます");
            if ui.button("スイープを実行").on_hover_text("値を1つずつ入れ替えて解析をくり返し、結果を重ねて表示します。回路そのものは変わりません").clicked() {
                self.run_param();
            }
        });
    }

    pub(super) fn run_param(&mut self) {
        let outcome = self.param_outcome();
        self.status = match &outcome {
            Ok(r) => format!("パラメータスイープ: 完了 ({} 本{})", r.curves.len(), if r.failed.is_empty() { String::new() } else { format!("、{} 本は計算できず", r.failed.len()) }),
            Err(e) => format!("パラメータスイープに失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        let plot = if self.param_ac {
            let (f0, f1) = (parse_value(&self.ac_fstart).unwrap_or(10.0), parse_value(&self.ac_fstop).unwrap_or(1e6));
            PlotState::log(f0.max(1e-30), f1.max(f0 * 1.001))
        } else {
            PlotState::new(parse_value(&self.tran_tstop).unwrap_or(1.0))
        };
        self.param = Some(ParamView { outcome, ac: self.param_ac, part: self.param_part.clone(), node: self.param_node.clone(), plot });
        self.wave_mode = WaveMode::Param;
    }

    fn param_outcome(&self) -> Result<ParamResult, Vec<String>> {
        let temp = match self.temp_c.trim() {
            "" => T_NOM_C,
            t => t.parse::<f64>().map_err(|_| vec![format!("温度「{t}」を数値(℃)で入力してください (例: 27)")])?,
        };
        let part = self.param_part.trim();
        if part.is_empty() {
            return Err(vec!["値を変える部品を選んでください".into()]);
        }
        if self.param_node.trim().is_empty() {
            return Err(vec!["波形を見るノードを選んでください (出力にあたる点)".into()]);
        }
        let (Some(a), Some(b), Some(n)) = (parse_value(&self.param_from), parse_value(&self.param_to), parse_value(&self.param_n)) else {
            return Err(vec!["「から」「まで」「本数」を数値で入力してください (例: 1k, 100k, 5)".into()]);
        };
        if a <= 0.0 || b <= 0.0 {
            return Err(vec!["値は 0 より大きい数にしてください".into()]);
        }
        let analysis = if self.param_ac {
            match (parse_value(&self.ac_fstart), parse_value(&self.ac_fstop), parse_value(&self.ac_ppd)) {
                (Some(f0), Some(f1), Some(p)) => ParamAnalysis::Ac { fstart: f0, fstop: f1, points_per_decade: p.max(0.0) as usize },
                _ => return Err(vec!["AC解析の周波数の設定を数値で入力してください".into()]),
            }
        } else {
            match (parse_value(&self.tran_tstop), parse_value(&self.tran_tstep)) {
                (Some(tstop), Some(tstep)) => ParamAnalysis::Tran { tstop, tstep, uic: self.tran_uic },
                _ => return Err(vec!["過渡解析の終了時間と刻みを数値で入力してください".into()]),
            }
        };
        let values = sweep_values(a, b, n.round().max(1.0) as usize, self.param_log);
        param_sweep(&self.sch, &ParamSettings { refdes: part.into(), values, node: self.param_node.trim().into(), temp_c: temp, analysis }).map_err(|e| vec![e.to_string()])
    }

    pub(super) fn param_wave_panel(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.param.as_mut() else { return };
        let mut close = false;
        let ParamView { outcome, ac, part, node, plot: st } = view;
        ui.horizontal(|ui| {
            ui.strong(format!("パラメータスイープ: {part} を変えたときの V({node})"));
            if ui.button("閉じる").clicked() {
                close = true;
            }
            ui.small(if *ac { "縦軸: ゲイン (dB)。線の色は下の凡例のとおり" } else { "縦軸: 電圧。線の色は下の凡例のとおり" });
        });
        match outcome {
            Err(msgs) => {
                for m in msgs {
                    ui.colored_label(Color32::from_rgb(230, 60, 60), m);
                }
            }
            Ok(res) => {
                for (v, why) in &res.failed {
                    ui.colored_label(Color32::from_rgb(220, 160, 40), format!("{part} = {} は計算できませんでした: {why}", format_value(*v)));
                }
                let names: Vec<String> = res.curves.iter().map(|c| format!("{part}={}", format_value(c.value))).collect();
                ui.horizontal_wrapped(|ui| {
                    for (k, n) in names.iter().enumerate() {
                        ui.label(egui::RichText::new(n).color(PALETTE[k % PALETTE.len()]));
                    }
                });
                let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
                let traces: Vec<Trace> = res.curves.iter().zip(&names).enumerate().map(|(k, (c, n))| Trace { name: n, color: PALETTE[k % PALETTE.len()], t: &c.x, v: &c.y }).collect();
                let style = if *ac { Style { unit: " dB", x_unit: "Hz", x_name: "f", plain_y: true } } else { Style { unit: "V", x_unit: "s", x_name: "t", plain_y: false } };
                plot::draw_group(ui, rect, egui::Id::new("param_plot"), &traces, st, 0, &style, true);
            }
        }
        if close {
            self.param = None;
        }
    }
}
