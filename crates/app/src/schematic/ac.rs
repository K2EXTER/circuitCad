//! AC (frequency response) analysis and its Bode plot.

use super::*;

impl SchEditor {
    pub fn analyze_ac(&mut self) {
        self.run_ac();
    }

    pub fn set_ac_settings(&mut self, f0: &str, f1: &str, ppd: &str) {
        self.ac_fstart = f0.into();
        self.ac_fstop = f1.into();
        self.ac_ppd = ppd.into();
    }

    pub(super) fn run_ac(&mut self) {
        let (Some(f0), Some(f1), Some(ppd)) = (parse_value(&self.ac_fstart), parse_value(&self.ac_fstop), parse_value(&self.ac_ppd)) else {
            self.status = "開始・終了周波数と点数/デケードを数値で入力してください (例: 10, 1Meg, 20)".into();
            return;
        };
        let outcome = match self.sim_circuit() {
            Err(errs) => Err(errs),
            Ok(c) => ac_analysis(&c, &AcSettings { fstart: f0, fstop: f1, sweep: AcSweep::Decade(ppd.max(0.0) as usize) }).map_err(|e| vec![e.to_string()]),
        };
        let mut selected = BTreeSet::new();
        if let Ok(r) = &outcome {
            for sg in r.signals.iter().filter(|s| s.unit == "V" && !s.name.contains('.')).take(3) {
                selected.insert(sg.name.clone());
            }
        }
        self.status = match &outcome {
            Ok(r) => format!("AC解析: 完了 ({} 点)", r.freq.len()),
            Err(e) => format!("AC解析に失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        let (lo, hi) = if f1 > f0 * 1.0001 { (f0, f1) } else { (f0 / 2.0, f0 * 2.0) };
        self.ac = Some(AcView { outcome, selected, plot: PlotState::log(lo.max(1e-30), hi), signature: self.signature() });
        self.wave_mode = WaveMode::Ac;
    }

    pub(super) fn ac_wave_panel(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.ac.as_mut() else { return };
        let mut close = false;
        let mut start_probe = false;
        let mut open_help = false;
        ui.horizontal(|ui| {
            ui.strong("AC解析 (ボード線図)");
            if ui.button("閉じる").clicked() {
                close = true;
            }
            if ui.small_button("操作").on_hover_text("波形のズーム・測定カーソル・信号の選び方を開きます").clicked() {
                open_help = true;
            }
            ui.small("ゲインは 0dB = 1 V(または 1 A)。入力の電源を「AC 1」にすると、そのまま伝達関数になります");
        });
        let AcView { outcome, selected, plot: st, .. } = view;
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
                plot::scale_bar(ui, st, "Hz", &[(0, "dB"), (1, "°")]);
                ui.horizontal_top(|ui| {
                    let order: Vec<&str> = res.signals.iter().filter(|s| !s.name.contains('.')).map(|s| s.name.as_str()).collect();
                    ui.vertical(|ui| {
                        ui.set_width(170.0);
                        ui.horizontal(|ui| {
                            if ui.button("選択をクリア").clicked() {
                                selected.clear();
                            }
                            if ui.button("回路図で選ぶ").on_hover_text("押してから回路図をクリックします。ピンや配線 = その点の電圧、部品の本体 (Shift+ピンでも可) = その部品の電流の周波数特性が追加されます。何か所でも続けて選べます (Esc か右クリックで終了)").clicked() {
                                start_probe = true;
                            }
                        });
                        for name in order.iter().filter(|n| selected.contains(**n)).take(4) {
                            if let Some(sg) = res.signal(name).filter(|s| s.unit == "V") {
                                let txt = match minus_3db(&res.freq, &sg.mag()) {
                                    Some(f) => eng(f, "Hz"),
                                    None => "範囲内にありません".into(),
                                };
                                ui.small(format!("{name}
  -3dB点 {txt}")).on_hover_text("ゲインの最大値から 3dB (約 0.7倍) 下がる周波数です。フィルタの「カットオフ周波数」にあたります");
                            }
                        }
                        egui::CollapsingHeader::new("安定性の余裕 (位相余裕・ゲイン余裕)").show(ui, |ui| {
                            ui.small("帰還ループを1か所で切って、1周したときのゲイン (一巡伝達関数) を測った波形に使います。選んだ信号を、そのゲインとして扱います");
                            for name in order.iter().filter(|n| selected.contains(**n)).take(3) {
                                let Some(sg) = res.signal(name).filter(|s| s.unit == "V") else { continue };
                                let Some(m) = stability_margins(&res.freq, &sg.db(), &sg.phase_deg()) else { continue };
                                ui.small(egui::RichText::new(*name).strong());
                                if m.inverted {
                                    ui.small("  位相が 180° から始まるので、反転増幅として 180° ずらして計算しました").on_hover_text("反転入力に信号を入れて出力を測ると、位相は 180° から始まります。負帰還では反転は織り込み済みなので、ずらして評価します");
                                }
                                match (m.gain_crossover, m.phase_margin) {
                                    (Some(f), Some(pm)) => {
                                        let (word, col) = if pm >= 45.0 { ("十分", Color32::from_rgb(90, 190, 110)) } else if pm >= 30.0 { ("やや不足", Color32::from_rgb(220, 160, 40)) } else if pm > 0.0 { ("不足: 振動しやすい", Color32::from_rgb(230, 90, 70)) } else { ("不安定", Color32::from_rgb(230, 60, 60)) };
                                        ui.small(egui::RichText::new(format!("  位相余裕 {pm:.1}° ({word})  @ {}", eng(f, "Hz"))).color(col)).on_hover_text("ゲインが 0dB になる周波数で、位相が -180° までにあと何度残っているか。45° 以上が目安で、小さいとステップ応答で行き過ぎや振動が出ます");
                                    }
                                    _ => {
                                        ui.small("  ゲイン 0dB の交点がありません (ゲインが最初から 1 未満、または範囲内で下がりきらない)");
                                    }
                                }
                                match (m.phase_crossover, m.gain_margin) {
                                    (Some(f), Some(gm)) => {
                                        let col = if gm >= 6.0 { Color32::from_rgb(90, 190, 110) } else if gm > 0.0 { Color32::from_rgb(230, 90, 70) } else { Color32::from_rgb(230, 60, 60) };
                                        ui.small(egui::RichText::new(format!("  ゲイン余裕 {gm:.1} dB  @ {}", eng(f, "Hz"))).color(col)).on_hover_text("位相が -180° になる周波数で、ゲインが 0dB まであと何 dB 残っているか。6dB 以上が目安で、負だと発振します");
                                    }
                                    _ => {
                                        ui.small("  位相が -180° に達しません (ゲイン余裕は無限大)");
                                    }
                                }
                                if ui.small_button("交点にカーソルを置く").on_hover_text("波形上のカーソル A をゲイン交点 (0dB)、B を位相交点 (-180°) に置きます").clicked() {
                                    st.cursors = [m.gain_crossover, m.phase_crossover];
                                }
                            }
                        });
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for name in &order {
                                let mut on = selected.contains(*name);
                                let color = order.iter().filter(|n| selected.contains(**n)).position(|n| n == name).map(|i| PALETTE[i % PALETTE.len()]);
                                let text = match color {
                                    Some(c) if on => egui::RichText::new(*name).color(c),
                                    _ => egui::RichText::new(*name),
                                };
                                if ui.checkbox(&mut on, text).changed() {
                                    if on {
                                        selected.insert(name.to_string());
                                    } else {
                                        selected.remove(*name);
                                    }
                                }
                            }
                        });
                    });
                    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
                    let chosen: Vec<(&str, Vec<f64>, Vec<f64>)> = order
                        .iter()
                        .filter(|n| selected.contains(**n))
                        .filter_map(|n| res.signal(n).map(|sg| (*n, sg.db(), sg.phase_deg())))
                        .collect();
                    if chosen.is_empty() {
                        ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(18, 22, 18));
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "左の一覧から信号を選んでください", FontId::proportional(14.0), Color32::from_gray(150));
                        return;
                    }
                    let mag: Vec<Trace> = chosen.iter().enumerate().map(|(k, (n, db, _))| Trace { name: n, color: PALETTE[k % PALETTE.len()], t: &res.freq, v: db }).collect();
                    let phase: Vec<Trace> = chosen.iter().enumerate().map(|(k, (n, _, ph))| Trace { name: n, color: PALETTE[k % PALETTE.len()], t: &res.freq, v: ph }).collect();
                    let h = rect.height() / 2.0;
                    let top = Rect::from_min_size(rect.min, Vec2::new(rect.width(), h - 2.0));
                    let bottom = Rect::from_min_size(Pos2::new(rect.left(), rect.top() + h), Vec2::new(rect.width(), h - 2.0));
                    let ms = Style { unit: " dB", x_unit: "Hz", x_name: "f", plain_y: true };
                    let ps = Style { unit: "°", x_unit: "Hz", x_name: "f", plain_y: true };
                    plot::draw_group(ui, top, egui::Id::new("bode_mag"), &mag, st, 0, &ms, false);
                    plot::draw_group(ui, bottom, egui::Id::new("bode_phase"), &phase, st, 1, &ps, true);
                });
            }
        }
        if open_help {
            self.help_request = Some(crate::help::HelpTab::Waveform);
        }
        if start_probe {
            self.start_probe();
        }
        if close {
            self.ac = None;
        }
    }
}
