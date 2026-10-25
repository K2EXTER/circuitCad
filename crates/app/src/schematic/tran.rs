//! Transient analysis and the oscilloscope-style viewer.

use super::*;

/// The bands (a fraction of the step) of the settling-time rows under "ステップ応答の指標".
const SETTLE_BANDS: [f64; 4] = [1e-2, 1e-3, 1e-4, 3e-5];

/// Signals the viewer lists: the nets and currents, and the pins' wire nodes `V(REF.PIN.wire)` of a post-layout run (the copper's
/// inductance and capacitance act between the net and that node). Other names with a dot are the insides of parts.
fn shown_signal(name: &str) -> bool {
    !name.contains('.') || name.ends_with(".wire)")
}

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

    /// Picks the signals of the power box (command line `--power <in V> <in I> <out V> <out I>`).
    pub fn set_power_signals(&mut self, names: [String; 4]) {
        if let Some(v) = self.tran.as_mut() {
            v.power = names;
        }
    }

    /// Picks the nets whose waveforms are drawn (command line `--signals CLK,Q1,Q2`).
    pub fn set_tran_signals(&mut self, nets: &str) {
        if let Some(v) = self.tran.as_mut() {
            v.selected = nets.split(',').filter(|n| !n.is_empty()).map(|n| format!("V({n})")).collect();
        }
    }

    #[cfg(test)]
    /// Largest value of a signal (`V(R2.1.wire)`) in the last transient run.
    pub fn tran_signal_max(&self, name: &str) -> Option<f64> {
        let r = self.tran.as_ref()?.outcome.as_ref().ok()?;
        r.signal(name).map(|s| s.values.iter().copied().fold(f64::NEG_INFINITY, f64::max))
    }

    pub fn set_tran_settings(&mut self, tstop: &str, tstep: &str) {
        self.tran_tstop = tstop.into();
        self.tran_tstep = tstep.into();
    }

    /// (end time, largest step) from the two fields; a blank step is chosen from the end time (`sim_core::auto_tstep`).
    pub(super) fn tran_times(&self) -> Option<(f64, f64)> {
        let tstop = parse_value(&self.tran_tstop)?;
        let tstep = if self.tran_tstep.trim().is_empty() { sim_core::auto_tstep(tstop) } else { parse_value(&self.tran_tstep)? };
        Some((tstop, tstep))
    }

    pub(super) fn run_tran(&mut self) {
        let Some((tstop, tstep)) = self.tran_times() else {
            self.status = "終了時間を数値で入力してください (例: 10m)。刻みは空欄なら自動で、入れるなら数値で (例: 10u)".into();
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
        // the capacitors' ripple current and life, from the second half of the run
        let ambient = self.temp_c.trim().parse::<f64>().ok().filter(|v| *v > -273.0 && *v < 1000.0).unwrap_or(T_NOM_C);
        let parts = outcome.as_ref().map(|r| sim_core::ripple_reports(&self.with_library_ratings(), r, ambient)).unwrap_or_default();
        let ghost = self.tran.as_ref().and_then(|v| v.ghost.clone());
        let power = self.tran.as_ref().map(|v| v.power.clone()).unwrap_or_default();
        self.tran = Some(TranView {
            ghost,
            power,
            parts,
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
        let mut open_help = false;
        let mut longer = false;
        let TranView { outcome, selected, plot: st, spectrum, spec_t0, spec_t1, spec_plot, spec_cache, ghost, power, parts, .. } = view;
        ui.horizontal(|ui| {
            ui.strong("過渡解析の波形");
            if ui.button("閉じる").clicked() {
                close = true;
            }
            ui.checkbox(spectrum, "スペクトル表示").on_hover_text("波形を周波数ごとの成分 (スペクトル) に分けて表示します。ノイズやリップルの周波数、ひずみを調べるときに使います");
            if *spectrum {
                ui.label("解析範囲");
                let tip = "スペクトルを計算する時間の範囲 (秒)。立ち上がり直後を除いた範囲を指定してください。縦軸は 0 dB = 1 V (ピーク値)";
                ui.add(egui::TextEdit::singleline(spec_t0).desired_width(70.0)).on_hover_text(tip);
                ui.label("〜");
                ui.add(egui::TextEdit::singleline(spec_t1).desired_width(70.0)).on_hover_text(tip);
                ui.label("秒");
            } else {
                if ui.small_button("操作").on_hover_text("波形のズーム・測定カーソル・信号の選び方を開きます").clicked() {
                    open_help = true;
                }
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
                    // "starting from all zero" is the normal way into an oscillator: a calm note, not a warning
                    if w.contains("普通のことです") {
                        ui.colored_label(Color32::from_rgb(130, 170, 210), format!("ⓘ {w}"));
                    } else {
                        ui.colored_label(Color32::from_rgb(220, 160, 40), w);
                    }
                }
                // a picture that does not move in its second half: the end time may be too short for what the circuit does (a blinker
                // with a period of a second, seen for 20 ms, is a flat line)
                let half = res.time.last().copied().unwrap_or(0.0) * 0.5;
                let seen: Vec<_> = res.signals.iter().filter(|s| s.unit == "V" && shown_signal(&s.name) && selected.contains(&s.name)).collect();
                // flat = in the second half no signal moves by more than 2% of the largest voltage on the picture
                let scale = seen.iter().flat_map(|sg| sg.values.iter()).fold(0.0f64, |m, v| m.max(v.abs()));
                let flat = !seen.is_empty()
                    && seen.iter().all(|sg| {
                        let tail: Vec<f64> = res.time.iter().zip(&sg.values).filter(|(t, _)| **t >= half).map(|(_, v)| *v).collect();
                        let (lo, hi) = tail.iter().fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
                        !tail.is_empty() && hi - lo < 0.02 * scale + 1e-3
                    });
                if flat {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new("波形が後半ほとんど動いていません。回路が落ち着いたのかもしれませんが、点滅や充放電が見たいときは、終了時間が短すぎる (動きが始まる前に終わっている) ことがあります。").size(12.0));
                        if ui.small_button("終了時間を10倍にして計算し直す").on_hover_text("終了時間の欄を10倍にして、もう一度過渡解析を実行します").clicked() {
                            longer = true;
                        }
                    });
                }
                if !parts.is_empty() {
                    egui::CollapsingHeader::new("部品の定格 (リップル電流・寿命)").default_open(true).show(ui, |ui| {
                        for r in parts.iter() {
                            let color = match r.level {
                                Some(sim_core::RatingLevel::Over) => Color32::from_rgb(230, 60, 60),
                                Some(sim_core::RatingLevel::Caution) => Color32::from_rgb(220, 160, 40),
                                None => Color32::from_rgb(60, 170, 90),
                            };
                            ui.colored_label(color, r.message());
                            if !r.hint().is_empty() {
                                ui.small(r.hint());
                            }
                            if let Some(t) = r.life_text() {
                                ui.label(t);
                            }
                        }
                        crate::icons::help_small(ui, "リップル電流と寿命の見積もり", "リップル電流は、計算の後半の波形の交流分の実効値です。寿命は「温度が 10℃ 下がると 2 倍」の目安で、データシートの条件 (定格電圧・定格リップル電流) での値から見積もります。実際の寿命は、使い方で大きく変わります");
                    });
                }
                if !*spectrum {
                    // scale controls for the groups that are on screen (voltage, current)
                    let units: Vec<&str> = res.signals.iter().filter(|s| shown_signal(&s.name) && selected.contains(&s.name)).map(|s| s.unit).collect();
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
                    let order: Vec<&str> = res.signals.iter().filter(|s| shown_signal(&s.name)).map(|s| s.name.as_str()).collect();
                    ui.vertical(|ui| {
                        // wide enough for a column per signal (the table scrolled sideways and hid the last one)
                        let columns = order.iter().filter(|n| selected.contains(**n)).take(6).count();
                        ui.set_width((100.0 + 80.0 * columns as f32).clamp(250.0, 520.0));
                        ui.horizontal(|ui| {
                            if ui.button("選択をクリア").clicked() {
                                selected.clear();
                            }
                            if ui.button("回路図で選ぶ").on_hover_text("押してから回路図をクリックします。ピンや配線 = その点の電圧、部品の本体 (Shift+ピンでも可) = その部品の電流の波形が追加されます。何か所でも続けて選べます (Esc か右クリックで終了)").clicked() {
                                start_probe = true;
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui.button("いまの波形を残す").on_hover_text("選んでいる波形のコピーを、うすい色で残します。部品の値を変えて計算し直すと、変える前 (うすい線) と後 (こい線) を重ねて比べられます").clicked() {
                                let sigs = order
                                    .iter()
                                    .filter(|n| selected.contains(**n))
                                    .filter_map(|n| res.signal(n))
                                    .map(|sg| GhostSig { base: sg.name.clone(), name: format!("{} (前)", sg.name), group: if sg.unit == "V" { 0 } else { 1 }, v: sg.values.clone() })
                                    .collect();
                                *ghost = Some(Ghost { x: res.time.clone(), sigs });
                            }
                            if ghost.is_some() && ui.button("消す").on_hover_text("残してある「前」の波形を消します").clicked() {
                                *ghost = None;
                            }
                        });
                        // mean and AC RMS (= noise / ripple amplitude) over the visible window
                        let (w0, w1) = if *spectrum {
                            (parse_value(spec_t0).unwrap_or(0.0), parse_value(spec_t1).unwrap_or(f64::MAX))
                        } else {
                            st.x
                        };
                        // one column per selected signal, one row per figure (so the narrow panel reads as a table)
                        let stats: Vec<_> = order
                            .iter()
                            .filter(|n| selected.contains(**n))
                            .take(6)
                            .filter_map(|name| {
                                let sg = res.signal(name)?;
                                let sx = signal_stats(&res.time, &sg.values, w0, w1)?;
                                Some((name.to_string(), sg.unit, sx, wave_measure(&res.time, &sg.values, w0, w1)))
                            })
                            .collect();
                        if !stats.is_empty() {
                            egui::ScrollArea::horizontal().id_salt("wave_stats").show(ui, |ui| {
                                egui::Grid::new("wave_stats_grid").striped(true).spacing([10.0, 2.0]).show(ui, |ui| {
                                    ui.label("");
                                    for (name, ..) in &stats {
                                        ui.strong(name);
                                    }
                                    ui.end_row();
                                    let dash = || "—".to_string();
                                    type Stat<'a> = (String, &'a str, sim_core::Stats, Option<sim_core::WaveMeasure>);
                                    type Column<'a> = (&'a str, &'a str, Box<dyn Fn(&Stat<'_>) -> String + 'a>);
                                    let rows: [Column; 8] = [
                                        ("平均", "表示範囲の平均値です", Box::new(|s| format!("{}{}", plot::axis_label(s.2.mean, s.2.mean.abs() / 1000.0), s.1))),
                                        ("AC実効値", "平均を引いた交流分の実効値 (ノイズ・リップルの大きさ)", Box::new(|s| format!("{}{}", plot::axis_label(s.2.rms_ac, s.2.rms_ac.abs() / 1000.0), s.1))),
                                        ("最小", "表示中の範囲での最小値です。ズームすると、見ている部分だけの値になります", Box::new(|s| s.3.as_ref().map_or_else(dash, |m| eng(m.min, s.1)))),
                                        ("最大", "表示中の範囲での最大値です。ズームすると、見ている部分だけの値になります", Box::new(|s| s.3.as_ref().map_or_else(dash, |m| eng(m.max, s.1)))),
                                        ("振幅 (p-p)", "最大と最小の差です", Box::new(|s| s.3.as_ref().map_or_else(dash, |m| eng(m.pp, s.1)))),
                                        ("周波数", "中間の高さを下から上へ横切る間隔から求めています。2周期以上が範囲に入っているときだけ出ます", Box::new(|s| s.3.as_ref().and_then(|m| m.freq).map_or_else(dash, |f| eng(f, "Hz")))),
                                        ("周期", "周波数の逆数です", Box::new(|s| s.3.as_ref().and_then(|m| m.freq).map_or_else(dash, |f| eng(1.0 / f, "s")))),
                                        ("デューティ比", "1周期のうち、中間の高さより上にいる時間の割合です", Box::new(|s| s.3.as_ref().and_then(|m| m.duty).map_or_else(dash, |d| format!("{:.1} %", d * 100.0)))),
                                    ];
                                    for (label, tip, f) in rows.iter() {
                                        let cells: Vec<String> = stats.iter().map(f).collect();
                                        // a row with nothing to show for any signal (no frequency, no duty) is left out
                                        if cells.iter().all(|c| c == "—") {
                                            continue;
                                        }
                                        ui.label(*label).on_hover_text(*tip);
                                        for c in cells {
                                            ui.label(c);
                                        }
                                        ui.end_row();
                                    }
                                });
                            });
                        }
                        egui::CollapsingHeader::new("ステップ応答の指標").show(ui, |ui| {
                            ui.small("0 から始まって一定の値に落ち着く波形用 (電源が 0V→5V に切り替わるなど)");
                            // one column per signal, one row per figure (same layout as the table above)
                            let mut metrics = vec![];
                            let mut settling = vec![];
                            let mut failed = vec![];
                            for name in order.iter().filter(|n| selected.contains(**n)).take(3) {
                                let Some(sg) = res.signal(name).filter(|s| s.unit == "V") else { continue };
                                match step_metrics(&res.time, &sg.values) {
                                    Ok(m) => {
                                        // how long it takes to stay within a tight band, counted from the moment it starts to move
                                        let step = (m.final_value - m.initial).abs();
                                        let start = res.time.iter().zip(&sg.values).find(|(_, v)| (**v - m.initial).abs() > 0.005 * step).map_or(0.0, |(t, _)| *t);
                                        settling.push(settling_times(&res.time, &sg.values, start, &SETTLE_BANDS).ok());
                                        metrics.push((name.to_string(), m));
                                    }
                                    Err(why) => failed.push(format!("{name}: {why}")),
                                }
                            }
                            if !metrics.is_empty() {
                                egui::Grid::new("step_metrics_grid").striped(true).spacing([10.0, 2.0]).show(ui, |ui| {
                                    let dash = || "—".to_string();
                                    ui.label("");
                                    for (name, _) in &metrics {
                                        ui.strong(name);
                                    }
                                    ui.end_row();
                                    ui.label("最終値").on_hover_text("波形が落ち着いた先の値です");
                                    for (_, m) in &metrics {
                                        ui.label(eng(m.final_value, "V"));
                                    }
                                    ui.end_row();
                                    ui.label("立ち上がり").on_hover_text("10% から 90% まで変わる時間");
                                    for (_, m) in &metrics {
                                        ui.label(m.rise_time.map_or_else(dash, |t| eng(t, "s")));
                                    }
                                    ui.end_row();
                                    ui.label("行き過ぎ").on_hover_text("最終値を超えた量。0% なら滑らかに近づいています");
                                    for (_, m) in &metrics {
                                        ui.label(format!("{:.1} %", m.overshoot_pct));
                                    }
                                    ui.end_row();
                                    ui.label("整定").on_hover_text("10% に達してから ±2% に収まるまでの時間");
                                    for (_, m) in &metrics {
                                        ui.label(m.settling_time.map_or_else(dash, |t| eng(t, "s")));
                                    }
                                    ui.end_row();
                                    // tighter bands: how long until the waveform is within ±1 %, ±0.1 %, ... of the final value for good
                                    for (k, band) in SETTLE_BANDS.iter().enumerate() {
                                        ui.label(format!("±{} %", (band * 1e5).round() / 1e3)).on_hover_text("波形が動き始めてから、最終値との差がこの幅 (ステップの大きさに対する割合) に収まって、そのままになるまでの時間です。精密な測定では、この時間だけ待ってから読みます。解析の終わりまでに収まらないときは「—」です");
                                        for s in &settling {
                                            ui.label(s.as_ref().and_then(|s| s.times[k].1).map_or_else(dash, |t| eng(t, "s")));
                                        }
                                        ui.end_row();
                                    }
                                });
                            }
                            for why in failed {
                                ui.small(why);
                            }
                            if !order.iter().any(|n| selected.contains(*n)) {
                                ui.small("下の一覧で信号を選んでください");
                            }
                        });
                        egui::CollapsingHeader::new("電力と効率").default_open(power.iter().any(|p| !p.is_empty())).show(ui, |ui| power_ui::power_ui(ui, res, &order, st.x, power));
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
                    // kept "before" waveforms go first, so they are painted behind the live ones
                    if let Some(gh) = ghost.as_ref() {
                        for gs in gh.sigs.iter().rev() {
                            let Some(k) = order.iter().filter(|n| selected.contains(**n)).position(|n| *n == gs.base) else { continue };
                            if !groups[gs.group].is_empty() {
                                groups[gs.group].insert(0, Trace { name: &gs.name, color: plot::ghost_color(PALETTE[k % PALETTE.len()]), t: &gh.x, v: &gs.v });
                            }
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
        if open_help {
            self.help_request = Some(crate::help::HelpTab::Waveform);
        }
        if start_probe {
            self.start_probe();
        }
        if longer && let Some((tstop, _)) = self.tran_times() {
            self.tran_tstop = sim_core::format_value(tstop * 10.0);
            self.run_tran();
        }
        if close {
            self.tran = None;
        }
    }
}
