//! Transient analysis and the oscilloscope-style viewer.

use super::*;

impl SchEditor {
    /// Switch the waveform viewer to the amplitude spectrum (command line `--spectrum`).
    pub fn show_spectrum(&mut self) {
        if let Some(v) = self.tran.as_mut() {
            v.spectrum = true;
        }
    }

    pub fn analyze_tran(&mut self) {
        self.run_tran();
    }

    pub fn set_tran_settings(&mut self, tstop: &str, tstep: &str) {
        self.tran_tstop = tstop.into();
        self.tran_tstep = tstep.into();
    }

    pub(super) fn run_tran(&mut self) {
        let (Some(tstop), Some(tstep)) = (parse_value(&self.tran_tstop), parse_value(&self.tran_tstep)) else {
            self.status = "終了時間と時間刻みを数値で入力してください (例: 10m, 10u)".into();
            return;
        };
        let outcome = match self.sim_circuit() {
            Err(errs) => Err(errs),
            Ok(c) => transient(&c, &TranSettings { tstop, tstep, uic: self.tran_uic }).map_err(|e| vec![e.to_string()]),
        };
        let mut selected = BTreeSet::new();
        if let Ok(r) = &outcome {
            for sg in r.signals.iter().filter(|s| s.unit == "V" && !s.name.contains('.')).take(4) {
                selected.insert(sg.name.clone());
            }
        }
        self.status = match &outcome {
            Ok(r) => format!("過渡解析: 完了 ({} 点, 再試行 {} 回)", r.time.len(), r.retries),
            Err(e) => format!("過渡解析に失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        self.tran = Some(TranView {
            outcome,
            selected,
            plot: PlotState::new(tstop),
            spectrum: false,
            spec_t0: format!("{:.6e}", tstop * 0.1),
            spec_t1: format!("{:.6e}", tstop),
            spec_plot: None,
            spec_cache: None,
            signature: self.signature(),
        });
        self.wave_mode = WaveMode::Tran;
    }

    /// Transient waveforms.
    pub(super) fn tran_wave_panel(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.tran.as_mut() else { return };
        let mut close = false;
        let mut start_probe = false;
        let TranView { outcome, selected, plot: st, spectrum, spec_t0, spec_t1, spec_plot, spec_cache, .. } = view;
        ui.horizontal(|ui| {
            ui.strong("過渡解析の波形");
            if ui.button("閉じる").clicked() {
                close = true;
            }
            ui.checkbox(spectrum, "スペクトル表示");
            if *spectrum {
                ui.label("解析範囲");
                ui.add(egui::TextEdit::singleline(spec_t0).desired_width(70.0));
                ui.label("〜");
                ui.add(egui::TextEdit::singleline(spec_t1).desired_width(70.0));
                ui.small("秒 / 0 dB = 1 V(ピーク値)。立ち上がり直後を除いた範囲を指定してください");
            } else {
                ui.small("ドラッグ: 移動 / ホイール: 時間軸ズーム (Ctrl: 縦軸) / 縦軸の数字の上でホイール: 縦軸だけ拡大縮小 / 横軸の数字の上でホイール: 横軸だけ拡大縮小 / クリック: 測定カーソル A→B (2点の差と周期が出ます) / ダブルクリック: 全体表示・カーソル解除 / 統計値は表示中の範囲");
            }
        });
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
                if !*spectrum {
                    // scale controls for the groups that are on screen (voltage, current)
                    let units: Vec<&str> = res.signals.iter().filter(|s| !s.name.contains('.') && selected.contains(&s.name)).map(|s| s.unit).collect();
                    let mut groups: Vec<(usize, &str)> = vec![];
                    if units.contains(&"V") {
                        groups.push((0, "V"));
                    }
                    if units.iter().any(|u| *u != "V") {
                        groups.push((1, "A"));
                    }
                    plot::scale_bar(ui, st, "s", &groups);
                }
                ui.horizontal_top(|ui| {
                    let order: Vec<&str> = res.signals.iter().filter(|s| !s.name.contains('.')).map(|s| s.name.as_str()).collect();
                    ui.vertical(|ui| {
                        ui.set_width(170.0);
                        ui.horizontal(|ui| {
                            if ui.button("選択をクリア").clicked() {
                                selected.clear();
                            }
                            if ui.button("回路図で選ぶ").on_hover_text("押してから回路図をクリックします。ピンや配線 = その点の電圧、部品の本体 (Shift+ピンでも可) = その部品の電流の波形が追加されます。何か所でも続けて選べます (Esc か右クリックで終了)").clicked() {
                                start_probe = true;
                            }
                        });
                        // mean and AC RMS (= noise / ripple amplitude) over the visible window
                        let (w0, w1) = if *spectrum {
                            (parse_value(spec_t0).unwrap_or(0.0), parse_value(spec_t1).unwrap_or(f64::MAX))
                        } else {
                            st.x
                        };
                        for name in order.iter().filter(|n| selected.contains(**n)).take(6) {
                            if let Some(sg) = res.signal(name)
                                && let Some(sx) = signal_stats(&res.time, &sg.values, w0, w1) {
                                    let u = sg.unit;
                                    ui.small(format!("{name}\n  平均 {}{u}  AC実効値 {}{u}", plot::axis_label(sx.mean, sx.mean.abs() / 1000.0), plot::axis_label(sx.rms_ac, sx.rms_ac.abs() / 1000.0)));
                                    if let Some(m) = wave_measure(&res.time, &sg.values, w0, w1) {
                                        ui.small(format!("  最小 {}  最大 {}
  振幅(p-p) {}", eng(m.min, u), eng(m.max, u), eng(m.pp, u))).on_hover_text("表示中の範囲での値です。ズームすると、見ている部分だけの値になります");
                                        if let Some(f) = m.freq {
                                            ui.small(format!("  周波数 {}  周期 {}", eng(f, "Hz"), eng(1.0 / f, "s"))).on_hover_text("中間の高さを下から上へ横切る間隔から求めています。2周期以上が範囲に入っているときだけ表示されます");
                                        }
                                        if let Some(d) = m.duty {
                                            ui.small(format!("  デューティ比 {:.1} %", d * 100.0)).on_hover_text("1周期のうち、中間の高さより上にいる時間の割合です");
                                        }
                                    }
                                }
                        }
                        egui::CollapsingHeader::new("ステップ応答の指標").show(ui, |ui| {
                            ui.small("0 から始まって一定の値に落ち着く波形用 (電源が 0V→5V に切り替わるなど)");
                            for name in order.iter().filter(|n| selected.contains(**n)).take(3) {
                                let Some(sg) = res.signal(name).filter(|s| s.unit == "V") else { continue };
                                match step_metrics(&res.time, &sg.values) {
                                    Ok(m) => {
                                        ui.small(format!("{name} (最終値 {})", eng(m.final_value, "V")));
                                        let dash = "—".to_string();
                                        ui.small(format!("  立ち上がり {}", m.rise_time.map_or(dash.clone(), |t| eng(t, "s")))).on_hover_text("10% から 90% まで変わる時間");
                                        ui.small(format!("  行き過ぎ {:.1} %", m.overshoot_pct)).on_hover_text("最終値を超えた量。0% なら滑らかに近づいています");
                                        ui.small(format!("  整定 {}", m.settling_time.map_or(dash, |t| eng(t, "s")))).on_hover_text("10% に達してから ±2% に収まるまでの時間");
                                    }
                                    Err(why) => {
                                        ui.small(format!("{name}: {why}"));
                                    }
                                }
                            }
                            if !order.iter().any(|n| selected.contains(*n)) {
                                ui.small("下の一覧で信号を選んでください");
                            }
                        });
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for name in &order {
                                let mut on = selected.contains(*name);
                                let color = order
                                    .iter()
                                    .filter(|n| selected.contains(**n))
                                    .position(|n| n == name)
                                    .map(|i| PALETTE[i % PALETTE.len()]);
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
                    if *spectrum {
                        let (Some(t0), Some(t1)) = (parse_value(spec_t0), parse_value(spec_t1)) else {
                            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "解析範囲を数値で入力してください", FontId::proportional(14.0), Color32::from_rgb(230, 60, 60));
                            return;
                        };
                        let key = format!("{t0:e}|{t1:e}|{selected:?}");
                        if spec_cache.as_ref().map(|c| &c.0) != Some(&key) {
                            let list: Vec<(String, Spectrum)> = order
                                .iter()
                                .filter(|n| selected.contains(**n))
                                .filter_map(|n| res.signal(n).and_then(|sg| fft_spectrum(&res.time, &sg.values, t0, t1)).map(|sp| (n.to_string(), sp)))
                                .collect();
                            *spec_plot = list.first().map(|(_, sp)| PlotState::log(sp.freq[0], *sp.freq.last().unwrap()));
                            *spec_cache = Some((key, list));
                        }
                        let list = &spec_cache.as_ref().unwrap().1;
                        let (Some(sp_state), false) = (spec_plot.as_mut(), list.is_empty()) else {
                            ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(18, 22, 18));
                            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "左の一覧から信号を選んでください (範囲が不正な場合も表示されません)", FontId::proportional(14.0), Color32::from_gray(150));
                            return;
                        };
                        let dbs: Vec<Vec<f64>> = list.iter().map(|(_, sp)| sp.amp.iter().map(|a| 20.0 * a.max(1e-12).log10()).collect()).collect();
                        let traces: Vec<Trace> = list
                            .iter()
                            .zip(&dbs)
                            .enumerate()
                            .map(|(k, ((n, sp), db))| Trace { name: n, color: PALETTE[k % PALETTE.len()], t: &sp.freq, v: db })
                            .collect();
                        let style = Style { unit: " dB", x_unit: "Hz", x_name: "f", plain_y: true };
                        plot::draw_group(ui, rect, egui::Id::new("spectrum"), &traces, sp_state, 0, &style, true);
                        return;
                    }
                    // traces per unit group, colours assigned in list order
                    let mut groups: [Vec<Trace>; 2] = [vec![], vec![]];
                    let mut k = 0;
                    for name in &order {
                        if !selected.contains(*name) {
                            continue;
                        }
                        if let Some(sg) = res.signal(name) {
                            let g = if sg.unit == "V" { 0 } else { 1 };
                            groups[g].push(Trace { name: &sg.name, color: PALETTE[k % PALETTE.len()], t: &res.time, v: &sg.values });
                            k += 1;
                        }
                    }
                    let used: Vec<usize> = (0..2).filter(|g| !groups[*g].is_empty()).collect();
                    if used.is_empty() {
                        ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(18, 22, 18));
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "左の一覧から波形を選んでください", FontId::proportional(14.0), Color32::from_gray(150));
                    }
                    let h = rect.height() / used.len().max(1) as f32;
                    for (n, g) in used.iter().enumerate() {
                        let sub = Rect::from_min_size(Pos2::new(rect.left(), rect.top() + h * n as f32), Vec2::new(rect.width(), h - 2.0));
                        let style = Style { unit: if *g == 0 { "V" } else { "A" }, x_unit: "s", x_name: "t", plain_y: false };
                        plot::draw_group(ui, sub, egui::Id::new(("wave", *g)), &groups[*g], st, *g, &style, n + 1 == used.len());
                    }
                });
            }
        }
        if start_probe {
            self.start_probe();
        }
        if close {
            self.tran = None;
        }
    }
}
