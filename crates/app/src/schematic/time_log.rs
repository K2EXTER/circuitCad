//! "時間の記録" (candidate L2): a meter's log over time (the DMM's readings with the room's temperature, a supply's drift overnight)
//! drawn on its own time axis, apart from the transient waveforms: the column chosen by its name, the temperature under it, the
//! predicted DC voltage of a net as a line to compare with, and per column the mean, the spread, the drift per hour and the
//! temperature coefficient (`sim_core::timelog`). The logs are kept with the sheet (`BenchData::logs`).

use super::bench_data::LogEntry;
use super::*;
use crate::plot;
use sim_core::timelog::{read_log, stats};

/// The window's state.
#[derive(Default)]
pub(super) struct TimeLogUi {
    pub open: bool,
    pick: usize,
    paste: String,
    plot: Option<plot::PlotState>,
    /// The log and the span the plot's x range was set for (a new log resets it).
    plot_for: Option<(usize, usize)>,
}

/// The time axis's unit for a log `span` seconds long: hours, minutes or seconds, and the seconds in one.
fn time_unit(span: f64) -> (&'static str, f64) {
    if span > 2.0 * 3600.0 {
        ("h", 3600.0)
    } else if span > 120.0 {
        ("min", 60.0)
    } else {
        ("s", 1.0)
    }
}

impl SchEditor {
    /// Opens the window.
    pub fn open_time_log(&mut self) {
        self.time_log_ui.open = true;
    }

    /// Adds a log read from `text` (named `name`); returns the message for the status line.
    pub fn add_time_log(&mut self, name: &str, text: &str) -> Result<String, String> {
        let log = read_log(text)?;
        let msg = i18n::trf!("時間の記録を読み込みました: {} 行・{} 列 ({})", log.t.len(), log.names.len(), log.names.join(", ")).to_string();
        // a temperature column is guessed by its name
        let temp = log.names.iter().position(|n| {
            let l = n.to_lowercase();
            l.contains("temp") || l.contains("温度") || l.contains("°c") || l == "t_c"
        });
        let show = (0..log.names.len()).find(|k| Some(*k) != temp).unwrap_or(0);
        self.bench.logs.push(LogEntry { name: name.to_string(), log, show, temp, net: None });
        self.time_log_ui.pick = self.bench.logs.len() - 1;
        self.time_log_ui.open = true;
        Ok(msg)
    }

    /// Adds the log in `text` on to log `k` (the same columns; at its own time when both have clock times); returns the message.
    pub fn append_time_log(&mut self, k: usize, text: &str) -> Result<String, String> {
        let more = read_log(text)?;
        let e = self.bench.logs.get_mut(k).ok_or("足す先の記録がありません")?;
        e.log = sim_core::timelog::append(&e.log, &more)?;
        self.time_log_ui.plot_for = None;
        Ok(i18n::trf!("「{}」に {} 行を足しました (全部で {} 行)", e.name, more.t.len(), e.log.t.len()).to_string())
    }

    pub(super) fn time_log_window(&mut self, ctx: &egui::Context) {
        if !self.time_log_ui.open {
            return;
        }
        let mut open = true;
        let mut load: Option<(String, String)> = None;
        let mut more: Option<String> = None;
        let mut remove = false;
        let predicted: Vec<(String, f64)> = self.bench_voltages().unwrap_or_default();
        egui::Window::new(i18n::tr("時間の記録 (ドリフト・温度)")).open(&mut open).default_pos(crate::widgets::beside_panel()).default_size([760.0, 560.0]).show(ctx, |ui| {
            ui.label(egui::RichText::new(i18n::tr("テスターや自作の DMM の、長い時間の記録 (CSV) を、時刻の軸で見ます。1 行に時刻 (10:00:01・2026-11-21T10:00:01・秒) と値を並べた記録を読み、見出しの行の名前で列を選びます。列ごとの平均・揺れ・1 時間あたりのドリフト・温度の列に対する温度係数を出し、ネットを選ぶと DC 動作点の予測と比べます")).small());
            ui.horizontal(|ui| {
                if ui.button(i18n::tr("CSV を読み込む…")).on_hover_text(i18n::tr("記録のファイル (CSV・TXT) を選んで読み込みます")).clicked()
                    && let Some(p) = rfd::FileDialog::new().add_filter("CSV", &["csv", "txt", "log"]).pick_file()
                {
                    match std::fs::read_to_string(&p) {
                        Ok(t) => load = Some((p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), t)),
                        Err(e) => self.status = i18n::trf!("記録を読めませんでした: {}", e).to_string(),
                    }
                }
                if !self.bench.logs.is_empty() {
                    let names: Vec<String> = self.bench.logs.iter().map(|l| l.name.clone()).collect();
                    let k = self.time_log_ui.pick.min(names.len() - 1);
                    egui::ComboBox::from_id_salt("time_log_pick").selected_text(names[k].clone()).show_ui(ui, |ui| {
                        for (i, n) in names.iter().enumerate() {
                            ui.selectable_value(&mut self.time_log_ui.pick, i, n);
                        }
                    });
                    if ui
                        .button(i18n::tr("続けて足す…"))
                        .on_hover_text(i18n::tr("同じ測り方の次の記録 (次の日・次の回の CSV) を、選んでいる記録の後ろに足します。時刻のある記録どうしは、その時刻の所に置きます (間があいた分も時刻の軸に残るので、日をまたいだドリフトが分かります)。秒の記録は、前の最後の行のすぐ後ろに続けます"))
                        .clicked()
                        && let Some(p) = rfd::FileDialog::new().add_filter("CSV", &["csv", "txt", "log"]).pick_file()
                    {
                        match std::fs::read_to_string(&p) {
                            Ok(t) => more = Some(t),
                            Err(e) => self.status = i18n::trf!("記録を読めませんでした: {}", e).to_string(),
                        }
                    }
                    if ui.small_button("🗑").on_hover_text(i18n::tr("この記録を消します")).clicked() {
                        remove = true;
                    }
                }
            });
            egui::CollapsingHeader::new(i18n::tr("貼り付けて読み込む")).id_salt("time_log_paste").show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(&mut self.time_log_ui.paste).desired_rows(4).hint_text("time,reading,temp\n10:00:00,1.000012,23.1"));
                ui.horizontal(|ui| {
                    if ui.button(i18n::tr("読み込む")).on_hover_text(i18n::tr("新しい記録として読み込みます")).clicked() {
                        load = Some((i18n::trf!("記録 {}", self.bench.logs.len() + 1).to_string(), self.time_log_ui.paste.clone()));
                    }
                    if !self.bench.logs.is_empty() && ui.button(i18n::tr("選んだ記録に続けて足す")).on_hover_text(i18n::tr("貼り付けた行を、選んでいる記録の後ろに足します")).clicked() {
                        more = Some(self.time_log_ui.paste.clone());
                    }
                });
            });
            if self.bench.logs.is_empty() {
                return;
            }
            let k = self.time_log_ui.pick.min(self.bench.logs.len() - 1);
            let e = &mut self.bench.logs[k];
            let names = e.log.names.clone();
            ui.horizontal(|ui| {
                ui.label(i18n::tr("見る列"));
                egui::ComboBox::from_id_salt("time_log_show").selected_text(names[e.show].clone()).show_ui(ui, |ui| {
                    for (i, n) in names.iter().enumerate() {
                        ui.selectable_value(&mut e.show, i, n);
                    }
                });
                ui.label(i18n::tr("温度の列"));
                egui::ComboBox::from_id_salt("time_log_temp").selected_text(e.temp.map(|t| names[t].clone()).unwrap_or_else(|| "—".into())).show_ui(ui, |ui| {
                    ui.selectable_value(&mut e.temp, None, "—");
                    for (i, n) in names.iter().enumerate() {
                        ui.selectable_value(&mut e.temp, Some(i), n);
                    }
                });
                ui.label(i18n::tr("予測と比べるネット")).on_hover_text(i18n::tr("このネットの DC 動作点の電圧を、横の線で重ねます (「DC動作点を解析」の結果)"));
                egui::ComboBox::from_id_salt("time_log_net").selected_text(e.net.clone().unwrap_or_else(|| "—".into())).show_ui(ui, |ui| {
                    ui.selectable_value(&mut e.net, None, "—");
                    for (n, _) in &predicted {
                        ui.selectable_value(&mut e.net, Some(n.clone()), n);
                    }
                });
            });
            let span = e.log.t.last().copied().unwrap_or(0.0);
            let (unit, per) = time_unit(span);
            let t: Vec<f64> = e.log.t.iter().map(|s| s / per).collect();
            let key = (k, e.log.t.len());
            if self.time_log_ui.plot_for != Some(key) || self.time_log_ui.plot.is_none() {
                self.time_log_ui.plot = Some(plot::PlotState::linear(0.0, (span / per).max(1e-9)));
                self.time_log_ui.plot_for = Some(key);
            }
            let st = self.time_log_ui.plot.as_mut().expect("set above");
            let pred = e.net.as_ref().and_then(|n| predicted.iter().find(|(m, _)| m == n)).map(|x| x.1);
            let pred_line: Vec<f64> = pred.map(|p| vec![p; t.len()]).unwrap_or_default();
            let mut traces = vec![plot::Trace { name: &names[e.show], color: egui::Color32::from_rgb(110, 180, 255), t: &t, v: &e.log.cols[e.show], dots: t.len() < 60 }];
            if pred.is_some() {
                traces.push(plot::Trace { name: "予測", color: crate::theme::ok(), t: &t, v: &pred_line, dots: false });
            }
            let h = if e.temp.is_some() { 300.0 } else { 220.0 };
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::hover());
            let style = |name: &'static str| plot::Style { unit: "", x_unit: unit, x_name: name, plain_y: false, db_ref: None };
            match e.temp {
                Some(tc) => {
                    let top = egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.min.y + h * 0.6));
                    let bottom = egui::Rect::from_min_max(egui::pos2(rect.min.x, top.max.y), rect.max);
                    let temp = [plot::Trace { name: &names[tc], color: crate::theme::warn(), t: &t, v: &e.log.cols[tc], dots: t.len() < 60 }];
                    plot::draw_group(ui, top, egui::Id::new("time_log_v"), &traces, st, 0, &style("t"), false);
                    plot::draw_group(ui, bottom, egui::Id::new("time_log_t"), &temp, st, 1, &style("t"), true);
                }
                None => plot::draw_group(ui, rect, egui::Id::new("time_log_v"), &traces, st, 0, &style("t"), true),
            }
            ui.small(i18n::trf!("{} 行・{} {}", e.log.t.len(), format!("{:.2}", span / per), unit));
            if let (Some(p), Some(n)) = (pred, &e.net) {
                let mean = e.log.cols[e.show].iter().sum::<f64>() / e.log.t.len().max(1) as f64;
                let (diff, agree) = judge(p, None, mean, 0.0);
                let color = match agree {
                    Agree::Good => crate::theme::ok(),
                    Agree::Off => crate::theme::warn(),
                    Agree::Far => crate::theme::error(),
                };
                ui.colored_label(color, i18n::trf!("{} の予測 {}V、記録の平均 {}V、差 {}V", n, sim_core::format_value(p), sim_core::format_value(mean), sim_core::format_value(diff)));
            }
            egui::Grid::new("time_log_stats").striped(true).num_columns(6).show(ui, |ui| {
                for (h, tip) in [
                    ("列", ""),
                    ("平均", ""),
                    ("揺れ (p-p)", "いちばん大きい値 − いちばん小さい値"),
                    ("標準偏差", ""),
                    ("ドリフト / 時", "時刻に対する傾き (1 時間あたり)。温度の列を選ぶと、時刻と温度を同時に当てはめて、部屋の温度の変化の分を除きます"),
                    ("温度係数", "温度に対する傾き (1 °C あたり) と、平均に対する ppm/°C。時刻と同時に当てはめるので、ゆっくりしたドリフトの分を除きます (時刻と温度がいっしょに動くだけの記録では分けられず、別々の直線になります)"),
                ] {
                    let r = ui.strong(i18n::tr(h));
                    if !tip.is_empty() {
                        r.on_hover_text(i18n::tr(tip));
                    }
                }
                ui.end_row();
                for s in stats(&e.log, e.temp) {
                    ui.label(&s.name);
                    ui.monospace(sim_core::format_value(s.mean));
                    ui.monospace(sim_core::format_value(s.max - s.min));
                    ui.monospace(sim_core::format_value(s.std));
                    ui.monospace(sim_core::format_value(s.drift_per_hour));
                    ui.monospace(match (s.tempco, s.tempco_ppm) {
                        (Some(tc), Some(ppm)) => format!("{} /°C ({ppm:.1} ppm/°C)", sim_core::format_value(tc)),
                        (Some(tc), None) => format!("{} /°C", sim_core::format_value(tc)),
                        _ => "—".into(),
                    });
                    ui.end_row();
                }
            });
        });
        self.time_log_ui.open = open;
        if remove {
            let k = self.time_log_ui.pick.min(self.bench.logs.len().saturating_sub(1));
            if k < self.bench.logs.len() {
                self.bench.logs.remove(k);
            }
            self.time_log_ui.pick = 0;
            self.time_log_ui.plot_for = None;
        }
        if let Some(text) = more {
            let k = self.time_log_ui.pick.min(self.bench.logs.len().saturating_sub(1));
            self.status = match self.append_time_log(k, &text) {
                Ok(m) => m,
                Err(e) => i18n::trf!("記録を足せませんでした: {}", e).to_string(),
            };
        }
        if let Some((name, text)) = load {
            self.status = match self.add_time_log(&name, &text) {
                Ok(m) => m,
                Err(e) => i18n::trf!("記録を読めませんでした: {}", e).to_string(),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A log is added with its temperature column guessed, kept with the sheet and read back.
    #[test]
    fn a_time_log_is_added_and_kept_with_the_sheet() {
        let mut e = SchEditor::default();
        e.add_time_log("夜通し", "time,reading,温度\n10:00:00,1.0,20\n11:00:00,1.1,21\n").unwrap();
        let l = &e.bench.logs[0];
        assert_eq!((l.show, l.temp), (0, Some(1)));
        let mut sheet = serde_json::json!({});
        with_bench(&mut sheet, &e.bench);
        assert_eq!(take_bench(&mut sheet).logs, e.bench.logs);
        assert_eq!(time_unit(7200.5), ("h", 3600.0));
        assert_eq!(time_unit(600.0), ("min", 60.0));
    }

    /// A second session's log is added on to the first at its own time and kept with the sheet.
    #[test]
    fn a_log_is_added_on_and_kept() {
        let mut e = SchEditor::default();
        e.add_time_log("R1", "time,reading\n10:00:00,1.000\n10:10:00,1.001\n").unwrap();
        e.append_time_log(0, "time,reading\n11:00:00,1.002\n11:10:00,1.003\n").unwrap();
        assert_eq!(e.bench.logs[0].log.t, [0.0, 600.0, 3600.0, 4200.0]);
        assert!(e.append_time_log(0, "time,other\n12:00:00,1\n12:10:00,2\n").is_err(), "another column");
        let mut sheet = serde_json::json!({});
        bench_data::with_bench(&mut sheet, &e.bench);
        let back = bench_data::take_bench(&mut sheet);
        assert_eq!(back.logs[0].log.start, Some(36000.0), "the first row's clock time is kept for the next one");
    }
}
