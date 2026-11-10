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
            Ok(c) => ac_analysis(&c, &AcSettings { fstart: f0, fstop: f1, sweep: AcSweep::Decade(ppd.max(0.0) as usize) }).map_err(|e| dc::small_signal_error(&c, e)),
        };
        let mut selected = BTreeSet::new();
        if let Ok(r) = &outcome {
            // the first three voltages that answer to the AC source: a node nothing AC reaches (the supply, a node behind a DC-only
            // source) is a flat line at -infinity dB that stretches the graph, so it is left unselected (all silent: the first three)
            let volts: Vec<_> = r.signals.iter().filter(|s| s.unit == "V" && !s.name.contains('.')).collect();
            let responds = |s: &&sim_core::AcSignal| s.mag().iter().any(|m| *m > 1e-9);
            let live: Vec<_> = volts.iter().filter(|s| responds(s)).collect();
            let pick: Vec<_> = if live.is_empty() { volts.iter().take(3).collect() } else { live.into_iter().take(3).collect() };
            for sg in pick {
                selected.insert(sg.name.clone());
            }
        }
        self.apply_lesson_traces("ac", &mut selected, |n| outcome.as_ref().is_ok_and(|r| r.signal(n).is_some()));
        let skin = if !self.wiring.is_empty() && f1 > 1e6 { " / 注意: 基板の配線の抵抗は直流の値です。約 300 kHz 以上では表皮効果で増えます (10 MHz で約 1.4〜1.9 倍、100 MHz で約 3.5〜4.3 倍)" } else { "" };
        self.status = match &outcome {
            Ok(r) => format!("AC解析: 完了 ({} 点){skin}", r.freq.len()),
            Err(e) => format!("AC解析に失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        let (lo, hi) = if f1 > f0 * 1.0001 { (f0, f1) } else { (f0 / 2.0, f0 * 2.0) };
        let ghost = self.ac.as_ref().and_then(|v| v.ghost.clone());
        self.ac = Some(AcView { outcome, selected, plot: PlotState::log(lo.max(1e-30), hi), ghost, signature: self.signature(), loop_run: false, pz_run: false, psrr_run: false, notes: vec![] });
        self.wave_mode = WaveMode::Ac;
    }

    pub(super) fn ac_wave_panel(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.ac.as_mut() else { return };
        let (bench, bench_ui) = (&mut self.bench.ac, &mut self.bench_ui);
        let mut close = false;
        let mut start_probe = false;
        let mut open_help = false;
        ui.horizontal(|ui| {
            crate::widgets::heading_inline(ui, if view.psrr_run { "電源除去比 (電源の揺れが届く割合)" } else if view.loop_run { "ループゲイン (帰還を1周したゲイン)" } else if view.pz_run { "伝達関数と、極・零から作った当てはめ" } else { "AC解析 (ボード線図)" }).on_hover_text(if view.psrr_run { i18n::tr("選んだ電源にだけ 1 V の揺れを入れたとき、各点に届く揺れの大きさです。-60 dB なら 1 V の揺れが 1 mV に (除去比 60 dB)。周波数が高い所で悪くなるのが普通で、そこはコンデンサで吸います") } else if view.pz_run { i18n::tr("H = 出力 ÷ 入力。「当てはめ」が回路の応答にぴったり重なっていれば、見つかった極と零で、この応答を説明できています。左の「解析」タブの「極と零」に、極と零の周波数の一覧があります") } else if view.loop_run { i18n::tr("0 dB = 1 倍。0 dB を横切る所の位相が -180° まで余裕があれば安定です (位相余裕 45° 以上が目安)。T が本当のループゲインで、Tv・Ti は電圧だけ・電流だけを入れたときの値 (T とずれる所は、切った所の両側のインピーダンスが違う所です)") } else { i18n::tr("ゲインは 0dB = 1 V(または 1 A)。入力の電源を「AC 1」にすると、そのまま伝達関数になります") });
            if crate::widgets::close_button(ui, "波形") {
                close = true;
            }
            if ui.small_button(i18n::tr("操作")).on_hover_text(i18n::tr("波形のズーム・測定カーソル・信号の選び方を開きます")).clicked() {
                open_help = true;
            }
        });
        let AcView { outcome, selected, plot: st, ghost, psrr_run, pz_run, notes, .. } = view;
        match outcome {
            Err(msgs) => {
                for m in msgs {
                    ui.colored_label(crate::theme::ERROR, i18n::tr_dyn(m));
                }
            }
            Ok(res) => {
                for n in notes.iter() {
                    // a note that says the result cannot be used is in the warning colour
                    if n.starts_with('▲') {
                        ui.colored_label(crate::theme::WARN, i18n::tr_dyn(n));
                    } else {
                        ui.label(i18n::tr_dyn(n));
                    }
                }
                crate::widgets::warning_lines(ui, "ac_warn", &res.warnings, |_| false);
                plot::scale_bar(ui, st, "Hz", &[(0, "dB"), (1, "°")]);
                ui.horizontal_top(|ui| {
                    let order: Vec<&str> = res.signals.iter().filter(|s| !s.name.contains('.')).map(|s| s.name.as_str()).collect();
                    ui.vertical(|ui| {
                        // same layout as the transient viewer: one bar of buttons, the signals side by side under it, then the
                        // tables; the column scrolls as one so nothing is cut off at the panel's bottom
                        ui.set_width(360.0);
                        egui::ScrollArea::vertical().id_salt("ac_left").auto_shrink([false, false]).show(ui, |ui| {
                        let bar = crate::widgets::segment_row(ui, &[("回路図で選ぶ", true), ("選択をクリア", !selected.is_empty()), (if ghost.is_some() { "前の特性を消す" } else { "いまの特性を残す" }, true)]);
                        if bar[0].clicked() {
                            start_probe = true;
                        }
                        bar[0].clone().on_hover_text(i18n::tr("押してから回路図をクリックします。ピンや配線 = その点の電圧、部品の本体 (Shift+ピンでも可) = その部品の電流の周波数特性が追加されます。何か所でも続けて選べます (Esc か右クリックで終了)"));
                        if bar[1].clicked() {
                            selected.clear();
                        }
                        bar[1].clone().on_hover_text(i18n::tr("選んだ信号を、すべて外します"));
                        if bar[2].clicked() {
                            if ghost.is_some() {
                                *ghost = None;
                            } else {
                                let mut sigs = vec![];
                                for sg in order.iter().filter(|n| selected.contains(**n)).filter_map(|n| res.signal(n)) {
                                    let name = format!("{} (前)", sg.name);
                                    sigs.push(GhostSig { base: sg.name.clone(), name: name.clone(), group: 0, v: sg.db() });
                                    sigs.push(GhostSig { base: sg.name.clone(), name, group: 1, v: sg.phase_deg() });
                                }
                                *ghost = Some(Ghost { x: res.freq.clone(), sigs });
                            }
                        }
                        bar[2].clone().on_hover_text(i18n::tr("選んでいる特性のコピーを、うすい色で残します。部品の値を変えて計算し直すと、変える前 (うすい線) と後 (こい線) を重ねて比べられます。残した後は、もう一度押すと消えます"));
                        wave::signal_list(ui, "ac", &order, selected);
                        ui.add_space(crate::theme::SPACE_S);
                        // -3dB point: one column per signal
                        let corner: Vec<(String, String)> = order
                            .iter()
                            .filter(|n| selected.contains(**n))
                            .take(4)
                            .filter_map(|name| {
                                let sg = res.signal(name).filter(|s| s.unit == "V")?;
                                let txt = minus_3db(&res.freq, &sg.mag()).map_or_else(|| "—".to_string(), |f| eng(f, "Hz"));
                                Some((name.to_string(), txt))
                            })
                            .collect();
                        // a ripple share has no cutoff to speak of
                        // (nor a result the note above calls unusable)
                        let unusable = notes.iter().any(|n| n.starts_with('▲'));
                        if !corner.is_empty() && !*psrr_run && !unusable {
                            egui::Grid::new("ac_corner_grid").striped(true).spacing([10.0, 2.0]).show(ui, |ui| {
                                ui.label("");
                                for (name, _) in &corner {
                                    ui.strong(i18n::tr_dyn(name));
                                }
                                ui.end_row();
                                ui.label(i18n::tr("-3dB点")).on_hover_text(i18n::tr("ゲインの最大値から 3dB (約 0.7倍) 下がる周波数です。フィルタの「カットオフ周波数」にあたります"));
                                for (_, txt) in &corner {
                                    if txt == "—" {
                                        // the input itself, or a curve that never falls 3 dB inside the range
                                        ui.label(txt).on_hover_text(i18n::tr("計算した範囲の中で、最大値から 3dB 下がる所がありません (入力そのものや、平らなままの信号)"));
                                    } else {
                                        ui.label(txt);
                                    }
                                }
                                ui.end_row();
                            });
                        }
                        // a ripple share or a fitted transfer function has no loop to be stable or not
                        if !*psrr_run && !*pz_run {
                        egui::CollapsingHeader::new(i18n::tr("安定性の余裕 (位相余裕・ゲイン余裕)")).show(ui, |ui| {
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
                                let amber = crate::theme::WARN;
                                let red = crate::theme::ERROR;
                                egui::Grid::new("ac_margin_grid").striped(true).spacing([10.0, 2.0]).show(ui, |ui| {
                                    ui.label("");
                                    for (name, _) in &margins {
                                        ui.strong(i18n::tr_dyn(name));
                                    }
                                    ui.end_row();
                                    ui.label(i18n::tr("位相余裕")).on_hover_text(i18n::tr("ゲインが 0dB になる周波数で、位相が -180° までにあと何度残っているか。45° 以上が目安で、小さいとステップ応答で行き過ぎや振動が出ます"));
                                    for (_, m) in &margins {
                                        match m.phase_margin.filter(|_| m.gain_crossover.is_some()) {
                                            Some(pm) => {
                                                let (word, col) = if pm >= 45.0 { ("十分", green) } else if pm >= 30.0 { ("やや不足", amber) } else if pm > 0.0 { ("不足", crate::theme::ERROR) } else { ("不安定", red) };
                                                let cell = ui.label(egui::RichText::new(format!("{pm:.1}° ({word})")).color(col));
                                                if word == "不足" {
                                                    cell.on_hover_text(i18n::tr("不足: 振動しやすい状態です"));
                                                }
                                            }
                                            None => {
                                                ui.label("—").on_hover_text(i18n::tr("ゲイン 0dB の交点がありません (ゲインが最初から 1 未満、または範囲内で下がりきらない)"));
                                            }
                                        }
                                    }
                                    ui.end_row();
                                    ui.label(i18n::tr("  その周波数")).on_hover_text(i18n::tr("ゲインが 0dB になる周波数 (ゲイン交点)"));
                                    for (_, m) in &margins {
                                        ui.label(m.gain_crossover.map_or_else(|| "—".to_string(), |f| eng(f, "Hz")));
                                    }
                                    ui.end_row();
                                    ui.label(i18n::tr("ゲイン余裕")).on_hover_text(i18n::tr("位相が -180° になる周波数で、ゲインが 0dB まであと何 dB 残っているか。6dB 以上が目安です"));
                                    for (_, m) in &margins {
                                        match (m.gain_margin, m.phase_crossover) {
                                            (Some(gm), Some(_)) => {
                                                let col = if gm >= 6.0 { green } else if gm > 0.0 { crate::theme::ERROR } else { red };
                                                ui.label(egui::RichText::new(format!("{gm:.1} dB")).color(col));
                                            }
                                            _ => {
                                                ui.label("∞").on_hover_text(i18n::tr("位相が -180° に達しません (ゲイン余裕は無限大)"));
                                            }
                                        }
                                    }
                                    ui.end_row();
                                    ui.label(i18n::tr("  その周波数")).on_hover_text(i18n::tr("位相が -180° になる周波数 (位相交点)"));
                                    for (_, m) in &margins {
                                        ui.label(m.phase_crossover.map_or_else(|| "—".to_string(), |f| eng(f, "Hz")));
                                    }
                                    ui.end_row();
                                });
                                if margins.iter().any(|(_, m)| m.inverted) {
                                    ui.small(i18n::tr("位相が 180° から始まる信号は、反転増幅として 180° ずらして計算しました")).on_hover_text(i18n::tr("反転入力に信号を入れて出力を測ると、位相は 180° から始まります。負帰還では反転は織り込み済みなので、ずらして評価します"));
                                }
                                for (name, m) in margins.iter().filter(|(_, m)| m.gain_crossover.is_some() || m.phase_crossover.is_some()) {
                                    let label = if margins.iter().filter(|(_, m)| m.gain_crossover.is_some() || m.phase_crossover.is_some()).count() > 1 { format!("{name} の交点にカーソルを置く") } else { "交点にカーソルを置く".to_string() };
                                    if ui.small_button(label).on_hover_text(i18n::tr("波形上のカーソル A をゲイン交点 (0dB)、B を位相交点 (-180°) に置きます")).clicked() {
                                        st.cursors = [m.gain_crossover, m.phase_crossover];
                                    }
                                }
                            }
                        });
                        }
                        let shown: Vec<&str> = order.iter().copied().filter(|n| selected.contains(*n)).collect();
                        let sim = |name: &str| res.signal(name).map(|sg| (res.freq.clone(), sg.db()));
                        bench_data::bench_controls(ui, bench, bench_ui, true, &order, &shown, &sim);
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
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, i18n::tr("左の一覧から信号を選んでください"), FontId::proportional(14.0), Color32::from_gray(150));
                        return;
                    }
                    let mut mag: Vec<Trace> = chosen.iter().enumerate().map(|(k, (n, db, _))| Trace { name: n, color: PALETTE[k % PALETTE.len()], t: &res.freq, v: db, dots: false }).collect();
                    let mut phase: Vec<Trace> = chosen.iter().enumerate().map(|(k, (n, _, ph))| Trace { name: n, color: PALETTE[k % PALETTE.len()], t: &res.freq, v: ph, dots: false }).collect();
                    // kept "before" curves go first, so they are painted behind the live ones
                    if let Some(gh) = ghost.as_ref() {
                        for gs in gh.sigs.iter().rev() {
                            let Some(k) = chosen.iter().position(|(n, _, _)| *n == gs.base) else { continue };
                            let tr = Trace { name: &gs.name, color: plot::ghost_color(PALETTE[k % PALETTE.len()]), t: &gh.x, v: &gs.v, dots: false };
                            if gs.group == 0 { mag.insert(0, tr) } else { phase.insert(0, tr) }
                        }
                    }
                    // bench points on top, in the colour of the signal they are compared with
                    let shown: Vec<&str> = chosen.iter().map(|(n, _, _)| *n).collect();
                    for c in bench.iter() {
                        let color = bench_data::bench_color(&shown, &c.signal);
                        mag.push(Trace { name: &c.name, color, t: &c.x, v: &c.y, dots: true });
                        if !c.phase.is_empty() {
                            phase.push(Trace { name: &c.name, color, t: &c.x, v: &c.phase, dots: true });
                        }
                    }
                    let h = rect.height() / 2.0;
                    let top = Rect::from_min_size(rect.min, Vec2::new(rect.width(), h - 2.0));
                    let bottom = Rect::from_min_size(Pos2::new(rect.left(), rect.top() + h), Vec2::new(rect.width(), h - 2.0));
                    let ms = Style { unit: " dB", x_unit: "Hz", x_name: "f", plain_y: true, db_ref: None };
                    let ps = Style { unit: "°", x_unit: "Hz", x_name: "f", plain_y: true, db_ref: None };
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
