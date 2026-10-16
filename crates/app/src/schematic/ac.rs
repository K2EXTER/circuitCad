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
        let ghost = self.ac.as_ref().and_then(|v| v.ghost.clone());
        self.ac = Some(AcView { outcome, selected, plot: PlotState::log(lo.max(1e-30), hi), ghost, signature: self.signature() });
        self.wave_mode = WaveMode::Ac;
    }

    pub(super) fn ac_wave_panel(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.ac.as_mut() else { return };
        let mut close = false;
        let mut start_probe = false;
        let mut open_help = false;
        ui.horizontal(|ui| {
            ui.strong("AC解析 (ボード線図)").on_hover_text("ゲインは 0dB = 1 V(または 1 A)。入力の電源を「AC 1」にすると、そのまま伝達関数になります");
            if ui.button("閉じる").clicked() {
                close = true;
            }
            if ui.small_button("操作").on_hover_text("波形のズーム・測定カーソル・信号の選び方を開きます").clicked() {
                open_help = true;
            }
        });
        let AcView { outcome, selected, plot: st, ghost, .. } = view;
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
                        ui.horizontal(|ui| {
                            if ui.button("いまの特性を残す").on_hover_text("選んでいる特性のコピーを、うすい色で残します。部品の値を変えて計算し直すと、変える前 (うすい線) と後 (こい線) を重ねて比べられます").clicked() {
                                let mut sigs = vec![];
                                for sg in order.iter().filter(|n| selected.contains(**n)).filter_map(|n| res.signal(n)) {
                                    let name = format!("{} (前)", sg.name);
                                    sigs.push(GhostSig { base: sg.name.clone(), name: name.clone(), group: 0, v: sg.db() });
                                    sigs.push(GhostSig { base: sg.name.clone(), name, group: 1, v: sg.phase_deg() });
                                }
                                *ghost = Some(Ghost { x: res.freq.clone(), sigs });
                            }
                            if ghost.is_some() && ui.button("消す").on_hover_text("残してある「前」の特性を消します").clicked() {
                                *ghost = None;
                            }
                        });
                        // -3dB point: one column per signal
                        let corner: Vec<(String, String)> = order
                            .iter()
                            .filter(|n| selected.contains(**n))
                            .take(4)
                            .filter_map(|name| {
                                let sg = res.signal(name).filter(|s| s.unit == "V")?;
                                let txt = minus_3db(&res.freq, &sg.mag()).map_or_else(|| "範囲内にありません".to_string(), |f| eng(f, "Hz"));
                                Some((name.to_string(), txt))
                            })
                            .collect();
                        if !corner.is_empty() {
                            egui::Grid::new("ac_corner_grid").striped(true).spacing([10.0, 2.0]).show(ui, |ui| {
                                ui.label("");
                                for (name, _) in &corner {
                                    ui.strong(name);
                                }
                                ui.end_row();
                                ui.label("-3dB点").on_hover_text("ゲインの最大値から 3dB (約 0.7倍) 下がる周波数です。フィルタの「カットオフ周波数」にあたります");
                                for (_, txt) in &corner {
                                    ui.label(txt);
                                }
                                ui.end_row();
                            });
                        }
                        egui::CollapsingHeader::new("安定性の余裕 (位相余裕・ゲイン余裕)").show(ui, |ui| {
                            let margins: Vec<(String, sim_core::Margins)> = order
                                .iter()
                                .filter(|n| selected.contains(**n))
                                .take(3)
                                .filter_map(|name| {
                                    let sg = res.signal(name).filter(|s| s.unit == "V")?;
                                    Some((name.to_string(), stability_margins(&res.freq, &sg.db(), &sg.phase_deg())?))
                                })
                                .collect();
                            if !margins.is_empty() {
                                // one column per signal; the colour says whether the margin is enough
                                let green = Color32::from_rgb(90, 190, 110);
                                let amber = Color32::from_rgb(220, 160, 40);
                                let red = Color32::from_rgb(230, 60, 60);
                                egui::Grid::new("ac_margin_grid").striped(true).spacing([10.0, 2.0]).show(ui, |ui| {
                                    ui.label("");
                                    for (name, _) in &margins {
                                        ui.strong(name);
                                    }
                                    ui.end_row();
                                    ui.label("位相余裕").on_hover_text("ゲインが 0dB になる周波数で、位相が -180° までにあと何度残っているか。45° 以上が目安で、小さいとステップ応答で行き過ぎや振動が出ます");
                                    for (_, m) in &margins {
                                        match m.phase_margin.filter(|_| m.gain_crossover.is_some()) {
                                            Some(pm) => {
                                                let (word, col) = if pm >= 45.0 { ("十分", green) } else if pm >= 30.0 { ("やや不足", amber) } else if pm > 0.0 { ("不足", Color32::from_rgb(230, 90, 70)) } else { ("不安定", red) };
                                                let cell = ui.label(egui::RichText::new(format!("{pm:.1}° ({word})")).color(col));
                                                if word == "不足" {
                                                    cell.on_hover_text("不足: 振動しやすい状態です");
                                                }
                                            }
                                            None => {
                                                ui.label("—").on_hover_text("ゲイン 0dB の交点がありません (ゲインが最初から 1 未満、または範囲内で下がりきらない)");
                                            }
                                        }
                                    }
                                    ui.end_row();
                                    ui.label("  その周波数").on_hover_text("ゲインが 0dB になる周波数 (ゲイン交点)");
                                    for (_, m) in &margins {
                                        ui.label(m.gain_crossover.map_or_else(|| "—".to_string(), |f| eng(f, "Hz")));
                                    }
                                    ui.end_row();
                                    ui.label("ゲイン余裕").on_hover_text("位相が -180° になる周波数で、ゲインが 0dB まであと何 dB 残っているか。6dB 以上が目安です");
                                    for (_, m) in &margins {
                                        match (m.gain_margin, m.phase_crossover) {
                                            (Some(gm), Some(_)) => {
                                                let col = if gm >= 6.0 { green } else if gm > 0.0 { Color32::from_rgb(230, 90, 70) } else { red };
                                                ui.label(egui::RichText::new(format!("{gm:.1} dB")).color(col));
                                            }
                                            _ => {
                                                ui.label("∞").on_hover_text("位相が -180° に達しません (ゲイン余裕は無限大)");
                                            }
                                        }
                                    }
                                    ui.end_row();
                                    ui.label("  その周波数").on_hover_text("位相が -180° になる周波数 (位相交点)");
                                    for (_, m) in &margins {
                                        ui.label(m.phase_crossover.map_or_else(|| "—".to_string(), |f| eng(f, "Hz")));
                                    }
                                    ui.end_row();
                                });
                                if margins.iter().any(|(_, m)| m.inverted) {
                                    ui.small("位相が 180° から始まる信号は、反転増幅として 180° ずらして計算しました").on_hover_text("反転入力に信号を入れて出力を測ると、位相は 180° から始まります。負帰還では反転は織り込み済みなので、ずらして評価します");
                                }
                                for (name, m) in margins.iter().filter(|(_, m)| m.gain_crossover.is_some() || m.phase_crossover.is_some()) {
                                    let label = if margins.iter().filter(|(_, m)| m.gain_crossover.is_some() || m.phase_crossover.is_some()).count() > 1 { format!("{name} の交点にカーソルを置く") } else { "交点にカーソルを置く".to_string() };
                                    if ui.small_button(label).on_hover_text("波形上のカーソル A をゲイン交点 (0dB)、B を位相交点 (-180°) に置きます").clicked() {
                                        st.cursors = [m.gain_crossover, m.phase_crossover];
                                    }
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
                    let mut mag: Vec<Trace> = chosen.iter().enumerate().map(|(k, (n, db, _))| Trace { name: n, color: PALETTE[k % PALETTE.len()], t: &res.freq, v: db }).collect();
                    let mut phase: Vec<Trace> = chosen.iter().enumerate().map(|(k, (n, _, ph))| Trace { name: n, color: PALETTE[k % PALETTE.len()], t: &res.freq, v: ph }).collect();
                    // kept "before" curves go first, so they are painted behind the live ones
                    if let Some(gh) = ghost.as_ref() {
                        for gs in gh.sigs.iter().rev() {
                            let Some(k) = chosen.iter().position(|(n, _, _)| *n == gs.base) else { continue };
                            let tr = Trace { name: &gs.name, color: plot::ghost_color(PALETTE[k % PALETTE.len()]), t: &gh.x, v: &gs.v };
                            if gs.group == 0 { mag.insert(0, tr) } else { phase.insert(0, tr) }
                        }
                    }
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
