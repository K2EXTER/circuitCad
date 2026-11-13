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
        self.apply_lesson_traces("tran", &mut selected, |n| outcome.as_ref().is_ok_and(|r| r.signal(n).is_some()));
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
            spec_t0: sim_core::format_value(tstop * 0.1),
            spec_t1: sim_core::format_value(tstop),
            spec_plot: None,
            spec_cache: None,
            thd_f0: String::new(),
            signature: self.signature(),
        });
        self.wave_mode = WaveMode::Tran;
    }

    /// Transient waveforms.
    pub(super) fn tran_wave_panel(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.tran.as_mut() else { return };
        let (bench, bench_ui) = (&mut self.bench.tran, &mut self.bench_ui);
        let mut close = false;
        let mut start_probe = false;
        let mut open_help = false;
        let mut longer = false;
        let TranView { outcome, selected, plot: st, spectrum, spec_t0, spec_t1, spec_plot, spec_cache, thd_f0, ghost, power, parts, .. } = view;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(i18n::tr("過渡解析の波形")).strong().color(crate::theme::INFO));
            if crate::widgets::close_x(ui, "波形") {
                close = true;
            }
            ui.checkbox(spectrum, i18n::tr("スペクトル表示")).on_hover_text(i18n::tr("波形を周波数ごとの成分 (スペクトル) に分けて表示します。ノイズやリップルの周波数、ひずみを調べるときに使います"));
            if *spectrum {
                ui.label(i18n::tr("解析範囲"));
                let tip = "スペクトルを計算する時間の範囲 (秒)。立ち上がり直後を除いた範囲を指定してください。縦軸は各周波数の成分の振幅 (ピーク値の V。目盛りは対数)";
                ui.add(egui::TextEdit::singleline(spec_t0).desired_width(70.0)).on_hover_text(tip);
                ui.label("〜");
                ui.add(egui::TextEdit::singleline(spec_t1).desired_width(70.0)).on_hover_text(tip);
                ui.label(i18n::tr("秒"));
            } else {
                if ui.small_button(i18n::tr("操作")).on_hover_text(i18n::tr("波形のズーム・測定カーソル・信号の選び方を開きます")).clicked() {
                    open_help = true;
                }
                // the range of the groups that are on screen (voltage, current), as one small button
                if let Ok(res) = &*outcome {
                    let units: Vec<&str> = res.signals.iter().filter(|s| shown_signal(&s.name) && selected.contains(&s.name)).map(|s| s.unit).collect();
                    let mut groups: Vec<(usize, &str)> = vec![];
                    if units.contains(&"V") {
                        groups.push((0, "V"));
                    }
                    if units.iter().any(|u| *u != "V") {
                        groups.push((1, "A"));
                    }
                    plot::range_button(ui, st, "s", &groups);
                }
            }
        });
        match outcome {
            Err(msgs) => {
                for m in msgs {
                    ui.colored_label(crate::theme::ERROR, i18n::tr_dyn(m));
                }
            }
            Ok(res) => {
                // "starting from all zero" is the normal way into an oscillator: a calm note, not a warning
                crate::widgets::warning_lines(ui, "tran_warn", &res.warnings, |w| w.contains("普通のことです"));
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
                    // one short line; the reasoning sits behind the (?)
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(crate::theme::WARN, i18n::tr("▲ 後半ほとんど動いていません"));
                        crate::icons::help(ui, "回路が落ち着いたのかもしれませんが、点滅や充放電が見たいときは、終了時間が短すぎる (動きが始まる前に終わっている) ことがあります。");
                        if ui.button(i18n::tr("終了時間を10倍にして計算し直す")).on_hover_text(i18n::tr("終了時間の欄を10倍にして、もう一度過渡解析を実行します")).clicked() {
                            longer = true;
                        }
                    });
                }
                if !parts.is_empty() {
                    // open at first only when a part is past or near its rating: all-green lines are not worth the plot's height
                    egui::CollapsingHeader::new(i18n::tr("部品の定格 (リップル電流・寿命)")).default_open(parts.iter().any(|r| r.level.is_some())).show(ui, |ui| {
                        for r in parts.iter() {
                            let color = match r.level {
                                Some(sim_core::RatingLevel::Over) => crate::theme::ERROR,
                                Some(sim_core::RatingLevel::Caution) => crate::theme::WARN,
                                None => crate::theme::OK,
                            };
                            ui.colored_label(color, i18n::tr_dyn(&r.message()));
                            if !r.hint().is_empty() {
                                ui.small(i18n::tr_dyn(&r.hint()));
                            }
                            if let Some(t) = r.life_text() {
                                ui.label(t);
                            }
                        }
                        crate::icons::help_small(ui, "リップル電流と寿命の見積もり", "リップル電流は、計算の後半の波形の交流分の実効値です。寿命は「温度が 10℃ 下がると 2 倍」の目安で、データシートの条件 (定格電圧・定格リップル電流) での値から見積もります。実際の寿命は、使い方で大きく変わります");
                    });
                }
                ui.horizontal_top(|ui| {
                    let order: Vec<&str> = res.signals.iter().filter(|s| shown_signal(&s.name)).map(|s| s.name.as_str()).collect();
                    ui.vertical(|ui| {
                        // wide enough for a column per signal (the table scrolled sideways and hid the last one) and for the
                        // four figure tabs on one line
                        let columns = order.iter().filter(|n| selected.contains(**n)).take(6).count();
                        ui.set_width((100.0 + 80.0 * columns as f32).clamp(440.0, 580.0));
                        // the column scrolls as one, so the signal list is never cut off below the panel
                        egui::ScrollArea::vertical().id_salt("tran_left").auto_shrink([false, false]).show(ui, |ui| {
                        let bar = crate::widgets::segment_row(ui, &[("回路図で選ぶ", true), ("選択をクリア", !selected.is_empty()), (if ghost.is_some() { "前の波形を消す" } else { "いまの波形を残す" }, true)]);
                        if bar[0].clicked() {
                            start_probe = true;
                        }
                        bar[0].clone().on_hover_text(i18n::tr("押してから回路図をクリックします。ピンや配線 = その点の電圧、部品の本体 (Shift+ピンでも可) = その部品の電流の波形が追加されます。何か所でも続けて選べます (Esc か右クリックで終了)"));
                        if bar[1].clicked() {
                            selected.clear();
                        }
                        bar[1].clone().on_hover_text(i18n::tr("選んだ信号を、すべて外します"));
                        if bar[2].clicked() {
                            if ghost.is_some() {
                                *ghost = None;
                            } else {
                                let sigs = order
                                    .iter()
                                    .filter(|n| selected.contains(**n))
                                    .filter_map(|n| res.signal(n))
                                    .map(|sg| GhostSig { base: sg.name.clone(), name: format!("{} (前)", sg.name), group: if sg.unit == "V" { 0 } else { 1 }, v: sg.values.clone() })
                                    .collect();
                                *ghost = Some(Ghost { x: res.time.clone(), sigs });
                            }
                        }
                        bar[2].clone().on_hover_text(i18n::tr("選んでいる波形のコピーを、うすい色で残します。部品の値を変えて計算し直すと、変える前 (うすい線) と後 (こい線) を重ねて比べられます。残した後は、もう一度押すと消えます"));
                        // the signals to show, side by side so a short list takes a line or two (it used to sit at the bottom and
                        // was cut off by the panel's edge)
                        wave::signal_list(ui, "tran", &order, selected);
                        // which table to show: the basic figures or one of the detailed ones, one at a time (they used to be a table
                        // and four folds stacked under it, which pushed everything below off the panel)
                        let metric_id = egui::Id::new("tran_metric");
                        let mut metric: usize = ui.data_mut(|d| *d.get_persisted_mut_or_insert_with(metric_id, || if power.iter().any(|p| !p.is_empty()) { 4 } else { 0 }));
                        ui.add_space(crate::theme::SPACE_S);
                        let tabs = crate::widgets::segment_bar(ui, &[("基本", true), ("ひずみ率", true), ("ステップ応答", true), ("負荷の急変", true), ("電力と効率", true)], Some(metric));
                        let tips = [
                            "平均・実効値・最小・最大・周波数など、表示している範囲の基本の値",
                            "ひずみ率 (THD): 正弦波がどれだけゆがんだか",
                            "ステップ応答の指標: 0 から一定の値に落ち着く波形の、立ち上がり・行き過ぎ・整定の時間",
                            "負荷の急変 (落ち込みと戻り): 負荷を急に変えたときの電圧の落ち込みと、戻るまでの時間",
                            "電力と効率: 入力と出力の平均電力と効率、損失",
                        ];
                        for (k, (tab, tip)) in tabs.into_iter().zip(tips).enumerate() {
                            if tab.clicked() {
                                metric = k;
                            }
                            tab.on_hover_text(i18n::tr(tip));
                        }
                        ui.data_mut(|d| d.insert_persisted(metric_id, metric));
                        ui.add_space(crate::theme::SPACE_S);
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
                        if metric == 0 && stats.is_empty() {
                            ui.small(i18n::tr("上の一覧で信号を選んでください"));
                        }
                        if metric == 0 && !stats.is_empty() {
                            egui::ScrollArea::horizontal().id_salt("wave_stats").show(ui, |ui| {
                                egui::Grid::new("wave_stats_grid").striped(true).spacing([16.0, 2.0]).show(ui, |ui| {
                                    ui.label("");
                                    for (name, ..) in &stats {
                                        ui.strong(name);
                                    }
                                    ui.end_row();
                                    let dash = || "—".to_string();
                                    type Stat<'a> = (String, &'a str, sim_core::Stats, Option<sim_core::WaveMeasure>);
                                    type Column<'a> = (&'a str, &'a str, Box<dyn Fn(&Stat<'_>) -> String + 'a>);
                                    // a ripple or swing a million times smaller than the signal is the solver's rounding, not the
                                    // circuit: show it as 0 (a steady 12 V read "AC実効値 0.04441pV")
                                    let size = |s: &Stat<'_>| s.3.as_ref().map_or(0.0, |m| m.min.abs().max(m.max.abs())).max(s.2.mean.abs());
                                    let small = |x: f64, s: &Stat<'_>| eng(if x.abs() <= size(s) * 1e-6 { 0.0 } else { x }, s.1);
                                    let rows: [Column; 8] = [
                                        ("平均", "表示範囲の平均値です", Box::new(|s| eng(s.2.mean, s.1))),
                                        ("AC実効値", "平均を引いた交流分の実効値 (ノイズ・リップルの大きさ)", Box::new(move |s| small(s.2.rms_ac, s))),
                                        ("最小", "表示中の範囲での最小値です。ズームすると、見ている部分だけの値になります", Box::new(|s| s.3.as_ref().map_or_else(dash, |m| eng(m.min, s.1)))),
                                        ("最大", "表示中の範囲での最大値です。ズームすると、見ている部分だけの値になります", Box::new(|s| s.3.as_ref().map_or_else(dash, |m| eng(m.max, s.1)))),
                                        ("振幅 (p-p)", "最大と最小の差です", Box::new(move |s| s.3.as_ref().map_or_else(dash, |m| small(m.pp, s)))),
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
                                        ui.label(i18n::tr(label)).on_hover_text(i18n::tr(tip));
                                        for c in cells {
                                            ui.label(c);
                                        }
                                        ui.end_row();
                                    }
                                });
                            });
                        }
                        if metric == 1 {
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(i18n::tr("基本周波数"));
                                ui.add(egui::TextEdit::singleline(thd_f0).hint_text(i18n::tr("自動")).desired_width(60.0)).on_hover_text(i18n::tr("入力の正弦波の周波数 (例 1k)。空欄なら波形から自動で見つけます。範囲は、ちょうど整数周期ぶんに切って計算します"));
                            });
                            ui.small(if *spectrum { i18n::tr("スペクトルの「解析範囲」の時間で計算します") } else { i18n::tr("いま表示している時間の範囲で計算します。立ち上がりを除いて、数周期〜数十周期が入るようにします") });
                            let f0 = if thd_f0.trim().is_empty() { None } else { parse_value(thd_f0) };
                            if f0.is_none() && !thd_f0.trim().is_empty() {
                                ui.colored_label(crate::theme::ERROR, i18n::tr("基本周波数を数値で入力してください (例: 1k)"));
                            } else {
                                let mut results = vec![];
                                let mut failed = vec![];
                                for name in order.iter().filter(|n| selected.contains(**n)).take(3) {
                                    let Some(sg) = res.signal(name).filter(|s| s.unit == "V") else { continue };
                                    match sim_core::thd(&res.time, &sg.values, f0, w0, w1, 10) {
                                        Ok(t) => results.push((name.to_string(), t)),
                                        Err(why) => failed.push(format!("{name}: {why}")),
                                    }
                                }
                                if !results.is_empty() {
                                    egui::Grid::new("thd_grid").striped(true).spacing([16.0, 2.0]).show(ui, |ui| {
                                        ui.label("");
                                        for (name, _) in &results {
                                            ui.strong(name);
                                        }
                                        ui.end_row();
                                        let pct = |r: f64| if r >= 0.01 { format!("{:.2} %", r * 100.0) } else { format!("{:.4} %", r * 100.0) };
                                        let rows: [(&str, &str, Box<dyn Fn(&sim_core::Thd) -> String>); 5] = [
                                            ("基本周波数", "基本波の周波数です", Box::new(|t| eng(t.f0, "Hz"))),
                                            ("THD", "全高調波ひずみ率: 2次以上の高調波を二乗して足した平方根 ÷ 基本波の振幅。小さいほど元の正弦波に近く、オーディオアンプなら 0.1% 以下が目安です", Box::new(|t| format!("{} ({:.1} dB)", pct(t.thd), 20.0 * t.thd.max(1e-12).log10()))),
                                            ("THD+N", "高調波に、ノイズや他の成分も足したもの (基本波以外のすべて ÷ 基本波)", Box::new(|t| pct(t.thd_n))),
                                            ("2次", "基本周波数の2倍の成分 (基本波との比)。波の上下が非対称にゆがむと出ます", Box::new(|t| format!("{:.1} dB", 20.0 * (t.harmonics[1].amp / t.fundamental()).max(1e-12).log10()))),
                                            ("3次", "基本周波数の3倍の成分 (基本波との比)。波の頭が上下とも削られる (クリップ) と出ます", Box::new(|t| format!("{:.1} dB", 20.0 * (t.harmonics[2].amp / t.fundamental()).max(1e-12).log10()))),
                                        ];
                                        for (label, tip, f) in rows.iter() {
                                            ui.label(i18n::tr(label)).on_hover_text(i18n::tr(tip));
                                            for (_, t) in &results {
                                                ui.label(f(t));
                                            }
                                            ui.end_row();
                                        }
                                    });
                                    if results.iter().any(|(_, t)| t.points_per_period < 40.0) {
                                        ui.colored_label(crate::theme::WARN, i18n::tr("1周期の波形の点が少なく、点を結ぶ直線のひずみが入っています。過渡解析の「刻み」を小さくして計算し直してください"));
                                    }
                                }
                                for f in &failed {
                                    ui.small(f);
                                }
                            }
                        });
                        }
                        if metric == 2 {
                        ui.vertical(|ui| {
                            ui.small(i18n::tr("0 から始まって一定の値に落ち着く波形用 (電源が 0V→5V に切り替わるなど)"));
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
                                egui::Grid::new("step_metrics_grid").striped(true).spacing([16.0, 2.0]).show(ui, |ui| {
                                    let dash = || "—".to_string();
                                    ui.label("");
                                    for (name, _) in &metrics {
                                        ui.strong(name);
                                    }
                                    ui.end_row();
                                    ui.label(i18n::tr("最終値")).on_hover_text(i18n::tr("波形が落ち着いた先の値です"));
                                    for (_, m) in &metrics {
                                        ui.label(eng(m.final_value, "V"));
                                    }
                                    ui.end_row();
                                    ui.label(i18n::tr("立ち上がり")).on_hover_text(i18n::tr("10% から 90% まで変わる時間"));
                                    for (_, m) in &metrics {
                                        ui.label(m.rise_time.map_or_else(dash, |t| eng(t, "s")));
                                    }
                                    ui.end_row();
                                    ui.label(i18n::tr("行き過ぎ")).on_hover_text(i18n::tr("最終値を超えた量。0% なら滑らかに近づいています"));
                                    for (_, m) in &metrics {
                                        ui.label(format!("{:.1} %", m.overshoot_pct));
                                    }
                                    ui.end_row();
                                    ui.label(i18n::tr("整定")).on_hover_text(i18n::tr("10% に達してから ±2% に収まるまでの時間"));
                                    for (_, m) in &metrics {
                                        ui.label(m.settling_time.map_or_else(dash, |t| eng(t, "s")));
                                    }
                                    ui.end_row();
                                    // tighter bands: how long until the waveform is within ±1 %, ±0.1 %, ... of the final value for good
                                    for (k, band) in SETTLE_BANDS.iter().enumerate() {
                                        ui.label(format!("±{} %", (band * 1e5).round() / 1e3)).on_hover_text(i18n::tr("波形が動き始めてから、最終値との差がこの幅 (ステップの大きさに対する割合) に収まって、そのままになるまでの時間です。精密な測定では、この時間だけ待ってから読みます。解析の終わりまでに収まらないときは「—」です"));
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
                                ui.small(i18n::tr("上の一覧で信号を選んでください"));
                            }
                        });
                        }
                        if metric == 3 {
                        ui.vertical(|ui| {
                            ui.small(i18n::tr("電流源を PULSE にして、負荷を急に変えたとき用。変わる所 (1 回分) だけが入るように拡大してから読みます"));
                            let mut rows = vec![];
                            let mut failed = vec![];
                            for name in order.iter().filter(|n| selected.contains(**n)).take(3) {
                                let Some(sg) = res.signal(name).filter(|s| s.unit == "V") else { continue };
                                match sim_core::load_step(&res.time, &sg.values, st.x.0, st.x.1) {
                                    Ok(m) => rows.push((name.to_string(), m)),
                                    Err(why) => failed.push(format!("{name}: {}", i18n::tr_dyn(&why))),
                                }
                            }
                            // a supply that does not move is not worth a line when another signal has the figures
                            if rows.is_empty() {
                                for f in &failed {
                                    ui.small(f);
                                }
                            }
                            if !rows.is_empty() {
                                egui::Grid::new("load_step_grid").striped(true).spacing([16.0, 2.0]).show(ui, |ui| {
                                    let pct = |d: f64, base: f64| if base.abs() > 1e-12 { format!(" ({:+.2}%)", d / base.abs() * 100.0) } else { String::new() };
                                    ui.label("");
                                    for (name, _) in &rows {
                                        ui.strong(name);
                                    }
                                    ui.end_row();
                                    ui.label(i18n::tr("変わる前")).on_hover_text(i18n::tr("表示している範囲の最初の電圧 (はじめの 1/20 の平均)"));
                                    for (_, m) in &rows {
                                        ui.label(eng(m.before, "V"));
                                    }
                                    ui.end_row();
                                    ui.label(i18n::tr("いちばん離れた所")).on_hover_text(i18n::tr("負荷が変わった直後に、変わる前の電圧からいちばん離れた量。マイナスなら落ち込み、プラスなら跳ね上がりです。出力のコンデンサを大きくするか、ESR の小さいものにすると小さくなります"));
                                    for (_, m) in &rows {
                                        ui.label(format!("{}{}", eng(m.excursion(), "V"), pct(m.excursion(), m.before)));
                                    }
                                    ui.end_row();
                                    ui.label(i18n::tr("落ち着いた先")).on_hover_text(i18n::tr("表示している範囲の最後の電圧 (おわりの 1/10 の平均) と、変わる前との差。負荷で電圧がどれだけ動くか (ロードレギュレーション) です"));
                                    for (_, m) in &rows {
                                        ui.label(format!("{}{}", eng(m.after, "V"), pct(m.after - m.before, m.before)));
                                    }
                                    ui.end_row();
                                    ui.label(i18n::tr("戻るまで")).on_hover_text(i18n::tr("電圧が動き始めてから、落ち着いた先との差が、いちばん離れた量の 1/10 以内に収まって、そのままになるまでの時間です。範囲の終わりまでに収まらないときは「—」です"));
                                    for (_, m) in &rows {
                                        ui.label(m.recovery.map_or_else(|| "—".to_string(), |t| eng(t, "s")));
                                    }
                                    ui.end_row();
                                });
                            }
                        });
                        }
                        if metric == 4 {
                            power_ui::power_ui(ui, res, &order, st.x, power);
                        }
                        // measured readings on top: used less often than the figures, so it comes last
                        {
                            let volts: Vec<&str> = order.iter().copied().filter(|n| res.signal(n).is_some_and(|s| s.unit == "V")).collect();
                            let shown: Vec<&str> = order.iter().copied().filter(|n| selected.contains(*n)).collect();
                            let sim = |name: &str| res.signal(name).map(|sg| (res.time.clone(), sg.values.clone()));
                            bench_data::bench_controls(ui, bench, bench_ui, false, &volts, &shown, &sim);
                        }
                        });
                    });
                    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
                    if *spectrum {
                        let (Some(t0), Some(t1)) = (parse_value(spec_t0), parse_value(spec_t1)) else {
                            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, i18n::tr("解析範囲を数値で入力してください"), FontId::proportional(14.0), crate::theme::ERROR);
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
                            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, i18n::tr("左の一覧から信号を選んでください (範囲が不正な場合も表示されません)"), FontId::proportional(14.0), Color32::from_gray(150));
                            return;
                        };
                        let dbs: Vec<Vec<f64>> = list.iter().map(|(_, sp)| sp.amp.iter().map(|a| 20.0 * a.max(1e-12).log10()).collect()).collect();
                        let traces: Vec<Trace> = list
                            .iter()
                            .zip(&dbs)
                            .enumerate()
                            .map(|(k, ((n, sp), db))| Trace { name: n, color: PALETTE[k % PALETTE.len()], t: &sp.freq, v: db, dots: false })
                            .collect();
                        // the amplitude itself (peak volts) on a log scale: the curve is dB with 0 dB = 1 V
                        let style = Style { unit: " dB", x_unit: "Hz", x_name: "f", plain_y: true, db_ref: Some((1.0, "V")) };
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
                            groups[g].push(Trace { name: &sg.name, color: PALETTE[k % PALETTE.len()], t: &res.time, v: &sg.values, dots: false });
                            k += 1;
                        }
                    }
                    // kept "before" waveforms go first, so they are painted behind the live ones
                    if let Some(gh) = ghost.as_ref() {
                        for gs in gh.sigs.iter().rev() {
                            let Some(k) = order.iter().filter(|n| selected.contains(**n)).position(|n| *n == gs.base) else { continue };
                            if !groups[gs.group].is_empty() {
                                groups[gs.group].insert(0, Trace { name: &gs.name, color: plot::ghost_color(PALETTE[k % PALETTE.len()]), t: &gh.x, v: &gs.v, dots: false });
                            }
                        }
                    }
                    // bench recordings over the voltages, in the colour of the signal they are compared with
                    let shown: Vec<&str> = order.iter().copied().filter(|n| selected.contains(*n)).collect();
                    let shifted: Vec<Vec<f64>> = bench.iter().map(|c| c.shifted_x()).collect();
                    for (c, x) in bench.iter().zip(&shifted) {
                        groups[0].push(Trace { name: &c.name, color: bench_data::bench_color(&shown, &c.signal), t: x, v: &c.y, dots: c.x.len() <= 60 });
                    }
                    let used: Vec<usize> = (0..2).filter(|g| !groups[*g].is_empty()).collect();
                    if used.is_empty() {
                        ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(18, 22, 18));
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, i18n::tr("左の一覧から波形を選んでください"), FontId::proportional(14.0), Color32::from_gray(150));
                    }
                    let h = rect.height() / used.len().max(1) as f32;
                    for (n, g) in used.iter().enumerate() {
                        let sub = Rect::from_min_size(Pos2::new(rect.left(), rect.top() + h * n as f32), Vec2::new(rect.width(), h - 2.0));
                        let style = Style { unit: if *g == 0 { "V" } else { "A" }, x_unit: "s", x_name: "t", plain_y: false, db_ref: None };
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
