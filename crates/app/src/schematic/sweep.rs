//! DC sweep (input-to-output transfer curve).

use super::*;

impl SchEditor {
    pub fn analyze_sweep(&mut self) {
        self.run_sweep();
    }

    pub fn set_sweep_settings(&mut self, src: &str, start: &str, stop: &str, step: &str) {
        self.sweep_src = src.into();
        self.sweep_start = start.into();
        self.sweep_stop = stop.into();
        self.sweep_step = step.into();
    }

    pub(super) fn run_sweep(&mut self) {
        let (Some(a), Some(b), Some(st)) = (parse_value(&self.sweep_start), parse_value(&self.sweep_stop), parse_value(&self.sweep_step)) else {
            self.status = "開始・終了・刻みを数値で入力してください (例: 0, 5, 0.05)".into();
            return;
        };
        let src = self.sweep_src.trim().to_string();
        let outcome = if src.is_empty() {
            Err(vec!["スイープする電源の番号を入力してください (例: V2)。入力にあたる電源です".to_string()])
        } else {
            match self.sim_circuit() {
                Err(errs) => Err(errs),
                Ok(c) => dc_sweep(&c, &DcSweepSettings { source: src.clone(), start: a, stop: b, step: st }).map_err(|e| vec![e.to_string()]),
            }
        };
        let mut selected = BTreeSet::new();
        if let Ok(r) = &outcome {
            // the output of an inverter is the most interesting signal: prefer a falling curve
            let falling = r.signals.iter().filter(|s| s.unit == "V" && !s.name.contains('.')).find(|s| inverter_metrics(&r.x, &s.values).is_some());
            match falling {
                Some(s) => {
                    selected.insert(s.name.clone());
                }
                None => {
                    for s in r.signals.iter().filter(|s| s.unit == "V" && !s.name.contains('.')).take(3) {
                        selected.insert(s.name.clone());
                    }
                }
            }
        }
        self.status = match &outcome {
            Ok(r) => format!("DCスイープ: 完了 ({} 点, {} = {} 〜 {})", r.x.len(), src, a, b),
            Err(e) => format!("DCスイープに失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        let (lo, hi) = if (b - a).abs() > 1e-12 { (a.min(b), a.max(b)) } else { (a - 1.0, a + 1.0) };
        self.sweep = Some(SweepView { outcome, selected, show_diag: true, plot: PlotState { x: (lo, hi), full_x: (lo, hi), log_x: false, y: [(-1.0, 1.0); 2], auto_y: [true; 2], cursors: [None; 2] }, temp: false });
        self.wave_mode = WaveMode::Sweep;
    }

    pub(super) fn sweep_wave_panel(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.sweep.as_mut() else { return };
        let mut close = false;
        ui.horizontal(|ui| {
            ui.strong(if view.temp { "温度スイープ (温度を変えたときの動作点)" } else { "DCスイープ (入力を変えたときの出力)" });
            if ui.button("閉じる").clicked() {
                close = true;
            }
            ui.small(if view.temp { "横軸: 回路の温度 (℃)。縦軸: 各ノードの電圧 (上) と電流 (下)" } else { "横軸: スイープした電源の電圧。縦軸: 各ノードの電圧 (上) と電流 (下)" });
        });
        let SweepView { outcome, selected, show_diag, plot: st, temp } = view;
        let temp = *temp;
        match outcome {
            Err(msgs) => {
                for m in msgs {
                    ui.colored_label(Color32::from_rgb(230, 60, 60), m);
                }
            }
            Ok(res) => {
                for w in &res.warnings {
                    ui.colored_label(Color32::from_rgb(220, 160, 40), w);
                }
                let names: Vec<&DcSweepSignal> = res.signals.iter().filter(|s| !s.name.contains('.')).collect();
                // key numbers of the first selected voltage that is an inverting curve
                let metrics = names.iter().filter(|s| s.unit == "V" && selected.contains(&s.name)).find_map(|s| inverter_metrics(&res.x, &s.values).map(|m| (s.name.clone(), m)));
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(260.0);
                        if let Some((name, m)) = &metrics {
                            ui.colored_label(Color32::from_rgb(220, 150, 40), format!("{name} は反転する特性です"));
                            egui::Grid::new("inv_metrics").num_columns(2).show(ui, |ui| {
                                let row = |ui: &mut egui::Ui, label: &str, val: String, tip: &str| {
                                    ui.label(label).on_hover_text(tip);
                                    ui.label(val).on_hover_text(tip);
                                    ui.end_row();
                                };
                                row(ui, "VOH (出力 High)", eng(m.voh, "V"), "入力が Low のときの出力電圧。電源電圧に近いほど良い High です");
                                row(ui, "VOL (出力 Low)", eng(m.vol, "V"), "入力が High のときの出力電圧。0 V に近いほど良い Low です");
                                row(ui, "Vth (切替電圧)", eng(m.vth, "V"), "入力と出力が等しくなる電圧。ここを境に出力の High/Low が入れ替わります。電源の半分が理想です");
                                row(ui, "VIL", eng(m.vil, "V"), "これより低い入力は確実に Low と見なされます (利得が -1 になる点)");
                                row(ui, "VIH", eng(m.vih, "V"), "これより高い入力は確実に High と見なされます (利得が -1 になる点)");
                                row(ui, "NML (Lowの余裕)", eng(m.nml(), "V"), "ノイズマージン = VIL − VOL。Low の信号に、これだけノイズが乗っても誤動作しません");
                                row(ui, "NMH (Highの余裕)", eng(m.nmh(), "V"), "ノイズマージン = VOH − VIH。High の信号に、これだけノイズが落ち込んでも誤動作しません");
                                row(ui, "最大利得", format!("{:.1}", m.max_gain), "切り替わり部分の傾き。大きい (絶対値) ほどスパッと切り替わります");
                            });
                            let ratio = (m.nml().min(m.nmh())) / (m.voh - m.vol).max(1e-12);
                            ui.small(if ratio > 0.35 {
                                "ノイズマージンは十分です。ロジックとして安心して使えます。"
                            } else if ratio > 0.15 {
                                "ノイズマージンは小さめです。ノイズの多い所では誤動作に注意しましょう。"
                            } else {
                                "ノイズマージンがほとんどありません。回路や部品の組み合わせを見直しましょう。"
                            });
                        } else {
                            ui.small("電圧の信号を選ぶと、反転する (インバーターの) 特性なら VOH / VOL / 切替電圧 / ノイズマージンを計算します");
                        }
                        if !temp {
                            ui.checkbox(show_diag, "「出力 = 入力」の線").on_hover_text("この線と曲線が交わる所が切替電圧 Vth です");
                        }
                        if ui.button("選択をクリア").clicked() {
                            selected.clear();
                        }
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            let sel_order: Vec<&str> = names.iter().filter(|s| selected.contains(&s.name)).map(|s| s.name.as_str()).collect();
                            for sg in &names {
                                let mut on = selected.contains(&sg.name);
                                let color = sel_order.iter().position(|n| *n == sg.name).map(|i| PALETTE[i % PALETTE.len()]);
                                let text = match color {
                                    Some(c) if on => egui::RichText::new(&sg.name).color(c),
                                    _ => egui::RichText::new(&sg.name),
                                };
                                if ui.checkbox(&mut on, text).changed() {
                                    if on {
                                        selected.insert(sg.name.clone());
                                    } else {
                                        selected.remove(&sg.name);
                                    }
                                }
                            }
                        });
                    });
                    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
                    let chosen: Vec<(usize, &DcSweepSignal)> = names.iter().filter(|s| selected.contains(&s.name)).enumerate().map(|(k, s)| (k, *s)).collect();
                    if chosen.is_empty() {
                        ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(18, 22, 18));
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "左の一覧から信号を選んでください", FontId::proportional(14.0), Color32::from_gray(150));
                        return;
                    }
                    let mut volts: Vec<Trace> = chosen.iter().filter(|(_, s)| s.unit == "V").map(|(k, s)| Trace { name: &s.name, color: PALETTE[*k % PALETTE.len()], t: &res.x, v: &s.values }).collect();
                    if *show_diag && !temp && !volts.is_empty() {
                        volts.push(Trace { name: "出力=入力", color: Color32::from_gray(120), t: &res.x, v: &res.x });
                    }
                    let amps: Vec<Trace> = chosen.iter().filter(|(_, s)| s.unit == "A").map(|(k, s)| Trace { name: &s.name, color: PALETTE[*k % PALETTE.len()], t: &res.x, v: &s.values }).collect();
                    let x_unit = if temp { "℃" } else { "V" };
                    let vs = Style { unit: "V", x_unit, x_name: &res.source, plain_y: false };
                    let is = Style { unit: "A", x_unit, x_name: &res.source, plain_y: false };
                    match (volts.is_empty(), amps.is_empty()) {
                        (false, false) => {
                            let h = rect.height() / 2.0;
                            let top = Rect::from_min_size(rect.min, Vec2::new(rect.width(), h - 2.0));
                            let bottom = Rect::from_min_size(Pos2::new(rect.left(), rect.top() + h), Vec2::new(rect.width(), h - 2.0));
                            plot::draw_group(ui, top, egui::Id::new("sweep_v"), &volts, st, 0, &vs, false);
                            plot::draw_group(ui, bottom, egui::Id::new("sweep_i"), &amps, st, 1, &is, true);
                        }
                        (false, true) => plot::draw_group(ui, rect, egui::Id::new("sweep_v"), &volts, st, 0, &vs, true),
                        _ => plot::draw_group(ui, rect, egui::Id::new("sweep_i"), &amps, st, 1, &is, true),
                    }
                });
            }
        }
        if close {
            self.sweep = None;
        }
    }
}
