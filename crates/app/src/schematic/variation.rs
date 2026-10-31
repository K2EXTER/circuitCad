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
        let measure = match self.mc_kind {
            McKind::Dc => McMeasure::DcVoltage,
            McKind::Ac => {
                let (Some(fstart), Some(fstop), Some(ppd)) = (parse_value(&self.ac_fstart), parse_value(&self.ac_fstop), parse_value(&self.ac_ppd)) else {
                    self.status = "AC解析の欄の周波数と点数を数値で入力してください (例: 10, 1Meg, 20)".into();
                    return;
                };
                McMeasure::Cutoff { fstart, fstop, points_per_decade: ppd.max(1.0) as usize }
            }
            McKind::Tran => {
                let Some((tstop, tstep)) = self.tran_times() else {
                    self.status = "過渡解析の欄の終了時間と刻みを数値で入力してください (例: 10m, 10u)".into();
                    return;
                };
                McMeasure::Tran { tstop, tstep, uic: self.tran_uic, metric: self.mc_metric }
            }
        };
        let settings = McSettings { runs, seed: self.mc_seed, tol_r, tol_c, tol_l, output: self.mc_out.trim().to_string(), measure };
        let outcome = if settings.output.is_empty() {
            Err(vec!["測るノードを選んでください".to_string()])
        } else {
            match self.sim_circuit() {
                Err(errs) => Err(errs),
                Ok(c) => monte_carlo(&c, &settings).map_err(|e| vec![e.to_string()]),
            }
        };
        self.status = match &outcome {
            Ok(r) => format!("ばらつき解析: {} 回完了{}", r.values.len(), if r.failed > 0 { format!(" ({} 回は収束せず)", r.failed) } else { String::new() }),
            Err(e) => format!("ばらつき解析に失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        // the limits for the same parts and tolerances: not a chance, a guarantee
        let worst = match (&outcome, self.sim_circuit()) {
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

    /// Names of the nodes that can be measured.
    pub(super) fn node_names(&self) -> Vec<String> {
        self.sim_circuit().map(|c| c.names.iter().skip(1).filter(|n| !n.contains('.')).cloned().collect()).unwrap_or_default()
    }

    pub(super) fn mc_wave_panel(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.mc.as_ref() else { return };
        let worst = view.worst.clone();
        let mut close = false;
        ui.horizontal(|ui| {
            ui.strong(match view.outcome.as_ref().map(|r| r.unit) {
                Ok("Hz") => "ばらつき解析 (部品の誤差でカットオフ周波数がどれだけずれるか)",
                Ok("s") | Ok("%") => "ばらつき解析 (部品の誤差でステップ応答がどれだけずれるか)",
                _ => "ばらつき解析 (部品の誤差で出力がどれだけずれるか)",
            });
            if ui.button("閉じる").clicked() {
                close = true;
            }
        });
        match &view.outcome {
            Err(msgs) => {
                for m in msgs {
                    ui.colored_label(Color32::from_rgb(230, 60, 60), m);
                }
            }
            Ok(res) => {
                let Some(st) = res.stats() else {
                    ui.colored_label(Color32::from_rgb(230, 60, 60), "どの回も収束しませんでした");
                    return;
                };
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(300.0);
                        egui::Grid::new("mc_stats").num_columns(2).show(ui, |ui| {
                            let row = |ui: &mut egui::Ui, label: &str, val: String, tip: &str| {
                                ui.label(label).on_hover_text(tip);
                                ui.label(val).on_hover_text(tip);
                                ui.end_row();
                            };
                            row(ui, &format!("{} の設計値", res.output), fmt_unit(res.nominal, res.unit), "部品がすべて表示どおりの値のときの値");
                            row(ui, "平均", fmt_unit(st.mean, res.unit), "ランダムに作った回路の値の平均");
                            row(ui, "最小", fmt_unit(st.min, res.unit), "一番小さかった値");
                            row(ui, "最大", fmt_unit(st.max, res.unit), "一番大きかった値");
                            row(ui, "標準偏差 σ", fmt_unit(st.std, res.unit), "ばらつきの大きさ。約 68% の回路は 平均 ± σ に入ります");
                        });
                        if res.nominal.abs() > 1e-12 {
                            let lo = (st.min - res.nominal) / res.nominal.abs() * 100.0;
                            let hi = (st.max - res.nominal) / res.nominal.abs() * 100.0;
                            ui.label(format!("設計値からのずれ: {lo:+.1}% 〜 {hi:+.1}%"));
                        }
                        if res.failed > 0 {
                            ui.colored_label(Color32::from_rgb(220, 160, 40), format!("{} 回は計算が収束しませんでした", res.failed));
                        }
                        if let Some(w) = &worst {
                            ui.separator();
                            ui.label("最悪ケース (全部品が誤差の端にあるとき)").on_hover_text("ランダムではなく、結果がいちばん小さく (大きく) なるように、すべての部品を許容差の端に置いて計算した値です。ばらつき解析の最小・最大は「たまたま出た値」ですが、これは「これより外には出ない」値です");
                            egui::Grid::new("mc_worst").num_columns(2).show(ui, |ui| {
                                for (label, v, corner) in [("最小", w.min, &w.min_corner), ("最大", w.max, &w.max_corner)] {
                                    let tip: Vec<String> = corner.iter().map(|(n, h)| format!("{n}{}", if *h { " 大" } else { " 小" })).collect();
                                    let pct = if w.nominal.abs() > 1e-12 { format!(" ({:+.1}%)", (v - w.nominal) / w.nominal.abs() * 100.0) } else { String::new() };
                                    ui.label(label).on_hover_text(format!("そうなる部品の組み合わせ: {}", tip.join("・")));
                                    ui.label(format!("{}{pct}", fmt_unit(v, w.unit))).on_hover_text(format!("そうなる部品の組み合わせ: {}", tip.join("・")));
                                    ui.end_row();
                                }
                            });
                            ui.small(format!("部品が無関係にずれるなら、現実的には {} 〜 {} の範囲に収まります", fmt_unit(w.min_rss, w.unit), fmt_unit(w.max_rss, w.unit))).on_hover_text("各部品の効きを二乗して足した平方根 (RSS)。全部がいっぺんに最悪になることはまずない、という見積もりです");
                        }
                        ui.separator();
                        ui.label("効いている部品").on_hover_text("その部品だけが許容差ぶん大きくなったとき、結果がどれだけ変わるか。上にある部品ほど効きます");
                        egui::Grid::new("mc_sens").num_columns(2).show(ui, |ui| {
                            for (name, dv) in res.sensitivity.iter().take(8) {
                                ui.label(name);
                                ui.label(format!("{}{}", if *dv >= 0.0 { "+" } else { "−" }, fmt_unit(dv.abs(), res.unit)));
                                ui.end_row();
                            }
                        });
                        if let Some((top, _)) = res.sensitivity.first() {
                            ui.small(format!("{top} の誤差が一番効いています。精度が必要なら、{top} を許容差の小さい部品 (1% など) にしましょう"));
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
        p.text(Pos2::new(x + dx, y), al, label, FontId::proportional(11.0), color);
    };
    mark(res.nominal, Color32::WHITE, "設計値", true);
    mark(st.mean, Color32::from_rgb(240, 200, 60), "平均", false);
    if let Some(w) = worst {
        mark(w.min, Color32::from_rgb(230, 80, 80), "最悪(小)", true);
        mark(w.max, Color32::from_rgb(230, 80, 80), "最悪(大)", true);
    }
    let f = FontId::proportional(11.0);
    p.text(Pos2::new(plot.left(), plot.bottom() + 4.0), Align2::LEFT_TOP, fmt_unit(a, res.unit), f.clone(), Color32::from_gray(190));
    p.text(Pos2::new(plot.right(), plot.bottom() + 4.0), Align2::RIGHT_TOP, fmt_unit(b, res.unit), f.clone(), Color32::from_gray(190));
    p.text(Pos2::new(rect.left() + 4.0, plot.top()), Align2::LEFT_TOP, format!("{peak:.0}回"), f.clone(), Color32::from_gray(190));
    p.text(Pos2::new(rect.left() + 4.0, plot.bottom()), Align2::LEFT_BOTTOM, "0", f, Color32::from_gray(190));
}
