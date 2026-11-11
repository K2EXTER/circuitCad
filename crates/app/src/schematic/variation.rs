//! Experiments on how a real circuit differs from the drawing: temperature and part tolerances.

use super::*;

/// A measured value with its unit; percentages are not given SI prefixes.
fn fmt_unit(v: f64, unit: &str) -> String {
    if unit == "%" { format!("{v:.1} %") } else { eng(v, unit) }
}

/// Tolerance text ("5" = ±5 %) -> fraction.
pub(super) fn percent(s: &str) -> Option<f64> {
    s.trim().trim_end_matches('%').parse::<f64>().ok().filter(|v| (0.0..100.0).contains(v)).map(|v| v / 100.0)
}

impl SchEditor {
    /// DC operating point at a range of temperatures, shown in the sweep viewer.
    pub(super) fn run_temp_sweep(&mut self) {
        let nums: Vec<Option<f64>> = [&self.temp_from, &self.temp_to, &self.temp_step].iter().map(|s| s.trim().parse::<f64>().ok()).collect();
        let (Some(a), Some(b), Some(st)) = (nums[0], nums[1], nums[2]) else {
            self.status = "開始・終了・刻みを数値で入力してください (例: -20, 80, 5)".into();
            return;
        };
        let outcome = match self.sim_circuit() {
            Err(errs) => Err(errs),
            // the sweep sets the temperature itself: start from the 27 °C circuit
            Ok(_) => match sim_core::from_schematic_with_layout(&self.sch, &self.wiring, &self.couplings) {
                Err(errs) => Err(errs.iter().map(|e| e.to_string()).collect()),
                Ok(c) => temp_sweep(&c, a, b, st).map_err(|e| vec![e.to_string()]),
            },
        };
        let mut selected = BTreeSet::new();
        if let Ok(r) = &outcome {
            for sg in r.signals.iter().filter(|s| s.unit == "V" && !s.name.contains('.')).take(3) {
                selected.insert(sg.name.clone());
            }
        }
        self.apply_lesson_traces("temp", &mut selected, |n| outcome.as_ref().is_ok_and(|r| r.signal(n).is_some()));
        self.status = match &outcome {
            Ok(r) => format!("温度スイープ: 完了 ({} 点, {} 〜 {} ℃)", r.x.len(), a, b),
            Err(e) => format!("温度スイープに失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        let (lo, hi) = if b > a { (a, b) } else { (a - 1.0, a + 1.0) };
        self.sweep = Some(SweepView {
            outcome,
            selected,
            show_diag: false,
            plot: PlotState::linear(lo, hi),
            temp: true,
        });
        self.wave_mode = WaveMode::Sweep;
    }

    /// For screenshots and tests: the variation analysis of `node` (`ac` = the -3 dB frequency, else the DC voltage).
    pub fn analyze_variation(&mut self, node: &str, ac: bool) {
        self.mc_out = node.into();
        self.mc_kind = if ac { McKind::Ac } else { McKind::Dc };
        self.run_mc();
    }

    /// Random part values inside their tolerances; the spread of one node voltage.
    pub(super) fn run_mc(&mut self) {
        let (Some(tol_r), Some(tol_c), Some(tol_l)) = (percent(&self.mc_tol_r), percent(&self.mc_tol_c), percent(&self.mc_tol_l)) else {
            self.status = "許容差は % の数値で入力してください (例: 5)".into();
            return;
        };
        let Ok(runs) = self.mc_runs.trim().parse::<usize>() else {
            self.status = "回数を整数で入力してください (例: 300)".into();
            return;
        };
        self.mc_seed += 1;
        let measure = match self.mc_measure() {
            Ok(m) => m,
            Err(e) => {
                self.status = e;
                return;
            }
        };
        let settings = McSettings { runs, seed: self.mc_seed, tol_r, tol_c, tol_l, output: self.mc_out.trim().to_string(), measure };
        let outcome = if settings.output.is_empty() {
            Err(vec!["測るノードを選んでください".to_string()])
        } else {
            match self.variation_circuit() {
                Err(errs) => Err(errs),
                Ok(c) => monte_carlo(&c, &settings).map_err(|e| vec![e.to_string()]),
            }
        };
        self.status = match &outcome {
            Ok(r) => format!("ばらつき解析: {} 回完了{}", r.values.len(), if r.failed > 0 { format!(" ({} 回は収束せず)", r.failed) } else { String::new() }),
            Err(e) => format!("ばらつき解析に失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        // the limits for the same parts and tolerances: not a chance, a guarantee
        let worst = match (&outcome, self.variation_circuit()) {
            (Ok(_), Ok(c)) => {
                let mut w = sim_core::WcSettings::from(&settings);
                w.tol_v = percent(&self.mc_tol_v).unwrap_or(0.0);
                sim_core::worst_case(&c, &w).ok()
            }
            _ => None,
        };
        self.mc = Some(McView { outcome, worst });
        self.wave_mode = WaveMode::Mc;
    }

    /// The circuit for the variation and worst-case runs: without the models' own min / max when the box is off.
    pub(super) fn variation_circuit(&self) -> Result<sim_core::Circuit, Vec<String>> {
        let mut c = self.sim_circuit()?;
        if !self.mc_spread {
            c.spreads.clear();
        }
        Ok(c)
    }

    /// What the variation, worst-case and sensitivity runs measure, from the choice in the panel.
    pub(super) fn mc_measure(&self) -> Result<McMeasure, String> {
        Ok(match self.mc_kind {
            McKind::Dc => McMeasure::DcVoltage,
            McKind::Ac => {
                let (Some(fstart), Some(fstop), Some(ppd)) = (parse_value(&self.ac_fstart), parse_value(&self.ac_fstop), parse_value(&self.ac_ppd)) else {
                    return Err("AC解析の欄の周波数と点数を数値で入力してください (例: 10, 1Meg, 20)".into());
                };
                McMeasure::Cutoff { fstart, fstop, points_per_decade: ppd.max(1.0) as usize }
            }
            McKind::Tran => {
                let Some((tstop, tstep)) = self.tran_times() else {
                    return Err("過渡解析の欄の終了時間と刻みを数値で入力してください (例: 10m, 10u)".into());
                };
                McMeasure::Tran { tstop, tstep, uic: self.tran_uic, metric: self.mc_metric }
            }
        })
    }

    /// Names of the nodes that can be measured.
    pub(super) fn node_names(&self) -> Vec<String> {
        self.sim_circuit().map(|c| c.names.iter().skip(1).filter(|n| !n.contains('.')).cloned().collect()).unwrap_or_default()
    }

    pub(super) fn mc_wave_panel(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.mc.as_ref() else { return };
        let worst = view.worst.clone();
        let mut close = false;
        ui.horizontal(|ui| {
            crate::widgets::heading_inline(ui, match view.outcome.as_ref().map(|r| r.unit) {
                Ok("Hz") => "ばらつき解析 (部品の誤差でカットオフ周波数がどれだけずれるか)",
                Ok("s") | Ok("%") => "ばらつき解析 (部品の誤差でステップ応答がどれだけずれるか)",
                _ => "ばらつき解析 (部品の誤差で出力がどれだけずれるか)",
            });
            if crate::widgets::close_button(ui, "ばらつき解析の表示") {
                close = true;
            }
        });
        match &view.outcome {
            Err(msgs) => {
                for m in msgs {
                    ui.colored_label(crate::theme::ERROR, i18n::tr_dyn(m));
                }
            }
            Ok(res) => {
                let Some(st) = res.stats() else {
                    ui.colored_label(crate::theme::ERROR, i18n::tr("どの回も収束しませんでした"));
                    return;
                };
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(300.0);
                        egui::Grid::new("mc_stats").num_columns(2).show(ui, |ui| {
                            let row = |ui: &mut egui::Ui, label: &str, val: String, tip: &str| {
                                ui.label(i18n::tr_dyn(label)).on_hover_text(i18n::tr_dyn(tip));
                                ui.label(val).on_hover_text(i18n::tr_dyn(tip));
                                ui.end_row();
                            };
                            row(ui, &format!("{} の設計値", res.output), fmt_unit(res.nominal, res.unit), "部品がすべて表示どおりの値のときの値");
                            row(ui, "平均", fmt_unit(st.mean, res.unit), "ランダムに作った回路の値の平均");
                            row(ui, "最小", fmt_unit(st.min, res.unit), "一番小さかった値");
                            row(ui, "最大", fmt_unit(st.max, res.unit), "一番大きかった値");
                            row(ui, "標準偏差 σ", fmt_unit(st.std, res.unit), "ばらつきの大きさ。約 68% の回路は 平均 ± σ に入ります");
                        });
                        // in percent of the design value, or as a difference when the value is near 0 V (an offset: -33 mV
                        // moving by 80 mV read as "+332 %")
                        let (lo, hi) = (st.min - res.nominal, st.max - res.nominal);
                        if percent_reads(res.nominal, lo, hi) {
                            let (lo, hi) = (lo / res.nominal.abs() * 100.0, hi / res.nominal.abs() * 100.0);
                            ui.label(i18n::trf!("設計値からのずれ: {lo:+.1}% 〜 {hi:+.1}%"));
                        } else {
                            ui.label(i18n::trf!("設計値からのずれ: {} 〜 {}", signed_unit(lo, res.unit), signed_unit(hi, res.unit)))
                                .on_hover_text(i18n::tr("設計値が 0 に近く、ずれのほうが大きいので、% ではなく差で表しています"));
                        }
                        if res.failed > 0 {
                            ui.colored_label(crate::theme::WARN, i18n::trf!("{} 回は計算が収束しませんでした", res.failed));
                        }
                        if let Some(w) = &worst {
                            ui.separator();
                            ui.label(i18n::tr("最悪ケース (全部品が誤差の端にあるとき)")).on_hover_text(i18n::tr("ランダムではなく、結果がいちばん小さく (大きく) なるように、すべての部品を許容差の端に置いて計算した値です。ばらつき解析の最小・最大は「たまたま出た値」ですが、これは「これより外には出ない」値です"));
                            egui::Grid::new("mc_worst").num_columns(2).show(ui, |ui| {
                                for (label, v, corner) in [("最小", w.min, &w.min_corner), ("最大", w.max, &w.max_corner)] {
                                    // "R1 大・R2 小": the words and the separator are translated one by one (the list has any length)
                                    let tip: Vec<String> = corner.iter().map(|(n, h)| format!("{} {}", varied_name(n), if *h { i18n::tr("大") } else { i18n::tr("小") })).collect();
                                    let tip = tip.join(i18n::tr("・"));
                                    let d = v - w.nominal;
                                    let pct = if percent_reads(w.nominal, w.min - w.nominal, w.max - w.nominal) { format!(" ({:+.1}%)", d / w.nominal.abs() * 100.0) } else { format!(" ({})", signed_unit(d, w.unit)) };
                                    ui.label(i18n::tr(label)).on_hover_text(i18n::trf!("そうなる部品の組み合わせ: {}", tip));
                                    ui.label(format!("{}{pct}", fmt_unit(v, w.unit))).on_hover_text(i18n::trf!("そうなる部品の組み合わせ: {}", tip));
                                    ui.end_row();
                                }
                            });
                            ui.small(i18n::trf!("部品が無関係にずれるなら、現実的には {} 〜 {} の範囲に収まります", fmt_unit(w.min_rss, w.unit), fmt_unit(w.max_rss, w.unit))).on_hover_text(i18n::tr("各部品の効きを二乗して足した平方根 (RSS)。全部がいっぺんに最悪になることはまずない、という見積もりです"));
                        }
                        ui.separator();
                        ui.label(i18n::tr("効いている部品")).on_hover_text(i18n::tr("その部品だけが許容差ぶん大きくなったとき、結果がどれだけ変わるか。上にある部品ほど効きます"));
                        egui::Grid::new("mc_sens").num_columns(2).show(ui, |ui| {
                            for (name, dv) in res.sensitivity.iter().take(8) {
                                ui.label(varied_name(name));
                                ui.label(format!("{}{}", if *dv >= 0.0 { "+" } else { "−" }, fmt_unit(dv.abs(), res.unit)));
                                ui.end_row();
                            }
                        });
                        if let Some((top, _)) = res.sensitivity.first() {
                            if top.contains(':') {
                                // a part's own figure (the data sheet's min / max), not a printed tolerance
                                let top = varied_name(top);
                                ui.small(i18n::trf!("{top} が一番効いています。データシートの max の小さい部品 (高精度品) を選ぶか、回路で打ち消す (調整する) と狭くなります"));
                            } else {
                                ui.small(i18n::trf!("{top} の誤差が一番効いています。精度が必要なら、{top} を許容差の小さい部品 (1% など) にしましょう"));
                            }
                        }
                    });
                    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
                    draw_histogram(ui, rect, res, &st, worst.as_ref());
                });
            }
        }
        if close {
            self.mc = None;
        }
    }
}

fn draw_histogram(ui: &egui::Ui, rect: Rect, res: &McResult, st: &sim_core::McStats, worst: Option<&sim_core::WorstCase>) {
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, Color32::from_rgb(18, 22, 18));
    let (lo, hi, bins) = res.histogram(24);
    let peak = bins.iter().copied().max().unwrap_or(1).max(1) as f32;
    let plot = Rect::from_min_max(rect.min + Vec2::new(40.0, 14.0), rect.max - Vec2::new(14.0, 30.0));
    let x_of = |v: f64| {
        // the axis also has to hold the design value
        let (a, b) = (lo.min(res.nominal).min(worst.map_or(f64::MAX, |w| w.min)), hi.max(res.nominal).max(worst.map_or(f64::MIN, |w| w.max)));
        plot.left() + ((v - a) / (b - a).max(1e-15)) as f32 * plot.width()
    };
    let (a, b) = (lo.min(res.nominal).min(worst.map_or(f64::MAX, |w| w.min)), hi.max(res.nominal).max(worst.map_or(f64::MIN, |w| w.max)));
    let bin_w = (hi - lo) / bins.len() as f64;
    for (k, n) in bins.iter().enumerate() {
        let (x0, x1) = (x_of(lo + bin_w * k as f64), x_of(lo + bin_w * (k + 1) as f64));
        let h = *n as f32 / peak * plot.height();
        p.rect_filled(Rect::from_min_max(Pos2::new(x0 + 1.0, plot.bottom() - h), Pos2::new(x1 - 1.0, plot.bottom())), 0.0, Color32::from_rgb(70, 140, 230));
    }
    p.line_segment([plot.left_bottom(), plot.right_bottom()], Stroke::new(1.0, Color32::from_gray(140)));
    let mark = |v: f64, color: Color32, label: &str, above: bool| {
        let x = x_of(v);
        p.line_segment([Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())], Stroke::new(1.5, color));
        let y = if above { plot.top() - 1.0 } else { plot.top() + 11.0 };
        // near the right edge the label goes to the left of its line
        let (dx, al) = if x > plot.center().x + plot.width() * 0.35 { (-3.0, Align2::RIGHT_BOTTOM) } else { (3.0, Align2::LEFT_BOTTOM) };
        p.text(Pos2::new(x + dx, y), al, i18n::tr_dyn(label), FontId::proportional(11.0), color);
    };
    mark(res.nominal, Color32::WHITE, "設計値", true);
    mark(st.mean, Color32::from_rgb(240, 200, 60), "平均", false);
    if let Some(w) = worst {
        mark(w.min, crate::theme::ERROR, "最悪(小)", true);
        mark(w.max, crate::theme::ERROR, "最悪(大)", true);
    }
    let f = FontId::proportional(11.0);
    p.text(Pos2::new(plot.left(), plot.bottom() + 4.0), Align2::LEFT_TOP, fmt_unit(a, res.unit), f.clone(), Color32::from_gray(190));
    p.text(Pos2::new(plot.right(), plot.bottom() + 4.0), Align2::RIGHT_TOP, fmt_unit(b, res.unit), f.clone(), Color32::from_gray(190));
    // round values in between (only the two ends were labelled), with a short tick; one that would crowd an end label is skipped
    let (ticks, _) = plot::nice_ticks(a, b, (plot.width() / 110.0) as usize);
    for t in ticks {
        let x = x_of(t);
        if x - plot.left() < 80.0 || plot.right() - x < 80.0 {
            continue;
        }
        p.line_segment([Pos2::new(x, plot.bottom()), Pos2::new(x, plot.bottom() + 4.0)], Stroke::new(1.0, Color32::from_gray(140)));
        p.text(Pos2::new(x, plot.bottom() + 4.0), Align2::CENTER_TOP, fmt_unit(t, res.unit), f.clone(), Color32::from_gray(150));
    }
    p.text(Pos2::new(rect.left() + 4.0, plot.top()), Align2::LEFT_TOP, i18n::trf!("{peak:.0}回"), f.clone(), Color32::from_gray(190));
    p.text(Pos2::new(rect.left() + 4.0, plot.bottom()), Align2::LEFT_BOTTOM, "0", f, Color32::from_gray(190));
}

/// The name shown for a varied part: `R1` as it is; a model's own figure (`U1:offset`, `Circuit::spreads`) in words.
pub(super) fn varied_name(name: &str) -> String {
    let Some((part, what)) = name.split_once(':') else { return name.to_string() };
    let what = match what {
        "offset" => i18n::tr("入力オフセット電圧"),
        "vout" => i18n::tr("出力電圧"),
        "vref" => i18n::tr("基準電圧"),
        other => other,
    };
    i18n::trf!("{} の{} (データシートの min〜max)", part, what)
}

/// Whether a spread reads well in percent of the design value: not when the value is near 0 and the spread is larger than it.
fn percent_reads(nominal: f64, lo: f64, hi: f64) -> bool {
    nominal.abs() > 1e-12 && lo.abs().max(hi.abs()) <= nominal.abs()
}

/// A difference with its sign and unit (+12.3 mV).
fn signed_unit(d: f64, unit: &str) -> String {
    format!("{}{}", if d >= 0.0 { "+" } else { "−" }, fmt_unit(d.abs(), unit))
}
