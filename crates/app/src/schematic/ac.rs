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
        self.ac = Some(AcView { outcome, selected, plot: PlotState::log(lo.max(1e-30), hi) });
        self.wave_mode = WaveMode::Ac;
    }

    pub(super) fn ac_wave_panel(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.ac.as_mut() else { return };
        let mut close = false;
        ui.horizontal(|ui| {
            ui.strong("AC解析 (ボード線図)");
            if ui.button("閉じる").clicked() {
                close = true;
            }
            ui.small("ゲインは 0dB = 1 V(または 1 A)。入力の電源を「AC 1」にすると、そのまま伝達関数になります");
        });
        let AcView { outcome, selected, plot: st } = view;
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
                ui.horizontal_top(|ui| {
                    let order: Vec<&str> = res.signals.iter().filter(|s| !s.name.contains('.')).map(|s| s.name.as_str()).collect();
                    ui.vertical(|ui| {
                        ui.set_width(170.0);
                        if ui.button("選択をクリア").clicked() {
                            selected.clear();
                        }
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
        if close {
            self.ac = None;
        }
    }
}
