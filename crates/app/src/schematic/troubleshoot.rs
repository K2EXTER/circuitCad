//! "症状から原因を探す": voltages measured on a circuit that does not work, matched against every single fault (see
//! `sim_core::troubleshoot`). The faults are solved once per circuit on another thread; the readings can then be changed and
//! matched again at once.

use super::*;
use sim_core::{fault_hypotheses, troubleshoot, Diagnosis, Hypothesis, Reading};
use std::sync::Arc;

/// The state of the panel: the readings as typed, the solved faults and the last diagnosis.
#[derive(Default)]
pub(super) struct DiagState {
    /// (node, volts as typed).
    pub(super) rows: Vec<(String, String)>,
    /// The faults solved for a circuit: its signature, whether the rare ones are in, the hypotheses.
    hyps: Option<(String, bool, Arc<Vec<Hypothesis>>)>,
    /// Solving on another thread.
    job: Option<std::sync::mpsc::Receiver<Result<Vec<Hypothesis>, String>>>,
    job_key: Option<(String, bool)>,
    pub(super) result: Option<Diagnosis>,
    error: Option<String>,
}

impl SchEditor {
    /// Sets the readings (command line `--diagnose OUT=0,MID=5`).
    pub fn set_diag_readings(&mut self, text: &str) {
        self.diag.rows = text.split(',').filter_map(|kv| kv.split_once('=')).map(|(n, v)| (n.trim().to_string(), v.trim().to_string())).collect();
    }

    /// The analysis tab with only the section `key` open (command line, to show a result in its section).
    pub fn show_analysis_section(&mut self, key: &'static str) {
        self.panel_tab = PanelTab::Analyze;
        self.focus_section = Some(key);
        self.sec_gen += 1;
    }

    /// Solves the faults on this thread and matches the readings (command line, tests).
    pub fn analyze_diagnosis(&mut self) {
        let sig = self.signature();
        if self.diag.hyps.as_ref().is_none_or(|(s, r, _)| *s != sig || *r != self.scan_rare) {
            let temp = self.fault_temp_value();
            match fault_hypotheses(&self.sch, temp, self.scan_rare) {
                Ok(h) => self.diag.hyps = Some((sig, self.scan_rare, Arc::new(h))),
                Err(e) => {
                    self.diag.error = Some(format!("回路を計算できないため、故障を試せません: {e}"));
                    return;
                }
            }
        }
        self.match_readings();
    }

    fn fault_temp_value(&self) -> f64 {
        self.temp_c.trim().parse().unwrap_or(T_NOM_C)
    }

    /// The readings that can be read as numbers; the rows that cannot are reported.
    fn diag_readings(&self) -> Result<Vec<Reading>, String> {
        let mut out = vec![];
        for (node, v) in &self.diag.rows {
            if node.trim().is_empty() && v.trim().is_empty() {
                continue;
            }
            let Some(volts) = parse_value(v.trim()) else {
                return Err(format!("{node} の電圧「{v}」を数値にできません (4.93 や 50m のように書きます)"));
            };
            out.push(Reading { node: node.trim().trim_start_matches("V(").trim_end_matches(')').to_string(), volts });
        }
        if out.is_empty() {
            return Err("測った点と電圧を 1 つ以上入れてください".into());
        }
        Ok(out)
    }

    fn match_readings(&mut self) {
        self.diag.error = None;
        let Some((_, _, hyps)) = &self.diag.hyps else { return };
        match self.diag_readings() {
            Ok(r) => {
                let d = troubleshoot(hyps, &r);
                self.status = match d.suspects.first() {
                    Some(s) if !d.unexplained => {
                        let h = &hyps[s.index];
                        format!("故障診断: いちばん疑わしいのは {} ({:.0}%)", if h.element.is_empty() { h.label.clone() } else { format!("{} の{}", h.element, h.label) }, s.probability * 100.0)
                    }
                    _ => "故障診断: 1 つの故障ではうまく説明できません".into(),
                };
                self.diag.result = Some(d);
            }
            Err(e) => {
                self.diag.error = Some(e);
                self.diag.result = None;
            }
        }
    }

    /// Starts solving the faults on another thread (or matches at once when they are solved for this circuit already).
    fn start_diagnosis(&mut self) {
        let sig = self.signature();
        let key = (sig.clone(), self.scan_rare);
        if self.diag.hyps.as_ref().is_some_and(|(s, r, _)| *s == key.0 && *r == key.1) {
            self.match_readings();
            return;
        }
        if self.diag.job.is_some() {
            return;
        }
        let (sch, temp, rare) = (self.sch.clone(), self.fault_temp_value(), self.scan_rare);
        let (tx, rx) = std::sync::mpsc::channel();
        let options = sim_core::options();
        std::thread::spawn(move || {
            sim_core::set_options(options);
            let _ = tx.send(fault_hypotheses(&sch, temp, rare).map_err(|e| e.to_string()));
        });
        self.diag.job = Some(rx);
        self.diag.job_key = Some(key);
        self.status = "故障診断: 起こりうる故障を全部計算しています…".into();
    }

    fn poll_diagnosis(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.diag.job else { return };
        match rx.try_recv() {
            Ok(Ok(h)) => {
                let (sig, rare) = self.diag.job_key.take().unwrap_or_default();
                self.diag.hyps = Some((sig, rare, Arc::new(h)));
                self.diag.job = None;
                self.match_readings();
            }
            Ok(Err(e)) => {
                self.diag.error = Some(format!("回路を計算できないため、故障を試せません: {e}"));
                self.diag.job = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.diag.job = None,
        }
    }

    /// The DC voltage readings of the spec table ("実測" of the DC voltage rows), as readings (the first board when several).
    fn readings_from_specs(&self) -> Vec<(String, String)> {
        self.spec_items()
            .iter()
            .filter(|s| s.kind.starts_with("dc_voltage") && !s.measured.trim().is_empty())
            .filter_map(|s| {
                let first = *sim_core::split_readings(&s.measured, parse_value)?.first()?;
                let node = s.target.trim().trim_start_matches("V(").trim_end_matches(')').to_string();
                Some((node, format_value(first)))
            })
            .collect()
    }

    pub(super) fn panel_troubleshoot(&mut self, ui: &mut egui::Ui) {
        self.poll_diagnosis(ui.ctx());
        let nodes = self.node_names();
        ui.small(i18n::tr("動かない回路で、テスターで測った電圧 (GND から) を入れます。起こりうる故障を 1 つずつ全部計算して、測った値をいちばんよく説明する故障を探し、次に測るとよい点を教えます"));
        let mut remove = None;
        egui::Grid::new("diag_rows").num_columns(3).spacing([6.0, 3.0]).show(ui, |ui| {
            for (k, (node, v)) in self.diag.rows.iter_mut().enumerate() {
                let shown = if node.is_empty() { i18n::tr("点を選ぶ").to_string() } else { node.clone() };
                egui::ComboBox::from_id_salt(("diag_node", k)).width(70.0).selected_text(shown).show_ui(ui, |ui| {
                    for n in &nodes {
                        ui.selectable_value(node, n.clone(), n);
                    }
                });
                let bad = !v.trim().is_empty() && parse_value(v.trim()).is_none();
                let mut edit = egui::TextEdit::singleline(v).desired_width(56.0).hint_text("V");
                if bad {
                    edit = edit.text_color(crate::theme::ERROR);
                }
                ui.add(edit).on_hover_text(i18n::tr("その点の電圧 (V)。4.93 や 50m のように書きます"));
                if ui.small_button("×").on_hover_text(i18n::tr("この行を消します")).clicked() {
                    remove = Some(k);
                }
                ui.end_row();
            }
        });
        if let Some(k) = remove {
            self.diag.rows.remove(k);
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button(i18n::tr("+ 測った点")).on_hover_text(i18n::tr("測った点を 1 行足します")).clicked() {
                self.diag.rows.push((String::new(), String::new()));
            }
            let from_specs = self.readings_from_specs();
            if !from_specs.is_empty() && ui.button(i18n::tr("スペック表の実測から")).on_hover_text(i18n::tr("スペック表の DC電圧の行に入れた実測を、測った点として取り込みます (何枚か入れてあるときは 1 枚目)")).clicked() {
                for (n, v) in from_specs {
                    match self.diag.rows.iter_mut().find(|(m, _)| m.eq_ignore_ascii_case(&n)) {
                        Some(row) => row.1 = v,
                        None => self.diag.rows.push((n, v)),
                    }
                }
            }
            ui.checkbox(&mut self.scan_rare, i18n::tr("まれな故障も含める")).on_hover_text(i18n::tr("値が 10 倍になる・逆向きにつないだ、など、めったに起きない故障も候補に入れます (小さな重みで)"));
        });
        let busy = self.diag.job.is_some();
        let label = if busy { "計算中…" } else { "原因の候補を探す" };
        if self.lit(crate::widgets::primary_button_wide(ui, !busy, label), "原因の候補を探す").on_hover_text(i18n::tr("はじめは、起こりうる故障を全部計算します (大きな回路では時間がかかります)。2 回目からは、測った値を変えてもすぐ答えが出ます")).clicked() {
            self.start_diagnosis();
        }
        if let Some(e) = &self.diag.error {
            ui.colored_label(crate::theme::ERROR, i18n::tr_dyn(e));
        }
        let (Some(d), Some((_, _, hyps))) = (&self.diag.result, &self.diag.hyps) else { return };
        let hyps = hyps.clone();
        // a merged line ("断線 / ピン 2 (B) を断線 / ...") shows its first way and a count; the hover has them all
        let name = |i: usize| {
            let h = &hyps[i];
            let ways: Vec<&str> = h.label.split(" / ").collect();
            let first = i18n::tr_dyn(ways[0]);
            let more = if ways.len() > 1 { i18n::trf!(" (ほか {} 通り)", ways.len() - 1) } else { String::new() };
            if h.element.is_empty() { format!("{first}{more}") } else { format!("{} {first}{more}", h.element) }
        };
        let all_ways = |i: usize| hyps[i].label.split(" / ").map(i18n::tr_dyn).collect::<Vec<_>>().join("\n");
        for n in &d.unknown_nodes {
            ui.colored_label(crate::theme::WARN, i18n::trf!("{n} という点は回路にありません"));
        }
        if d.unexplained {
            ui.colored_label(crate::theme::WARN, i18n::tr("どの故障 1 つでも、測った値をうまく説明できません。故障が 2 つ以上ある・測り間違い (テスターの GND の当て先)・モデルが実物と違う、を疑います"));
        }
        ui.label(egui::RichText::new(i18n::tr("疑わしい順")).strong()).on_hover_text(i18n::tr("測った値に合う度合いと、その故障の起こりやすさ (よくある・ときどき・まれ) から出した割合です。クリックすると、回路図でその部品を選びます"));
        let mut pick = None;
        egui::Grid::new("diag_suspects").num_columns(2).spacing([8.0, 2.0]).show(ui, |ui| {
            for s in d.suspects.iter().take(8) {
                let h = &hyps[s.index];
                let fit = s.fit.iter().map(|(n, p, m)| i18n::trf!("{n}: この故障なら {} / 測った値 {}", eng(*p, "V"), eng(*m, "V"))).collect::<Vec<_>>().join("\n");
                let tip = format!("{}\n\n{fit}", all_ways(s.index));
                let text = egui::RichText::new(name(s.index));
                let text = if s.worst_miss > 3.0 { text.weak() } else { text };
                if ui.add(egui::Label::new(text).sense(Sense::click())).on_hover_text(&tip).clicked() && !h.element.is_empty() {
                    pick = Some(h.element.clone());
                }
                let (rect, _) = ui.allocate_exact_size(egui::vec2(90.0, 12.0), Sense::hover());
                ui.painter().rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(90.0 * s.probability as f32, 12.0)), 2.0, crate::theme::INFO);
                ui.painter().text(rect.right_center(), egui::Align2::RIGHT_CENTER, format!("{:.0}%", s.probability * 100.0), FontId::proportional(11.0), ui.visuals().text_color());
                ui.end_row();
            }
        });
        if d.left > 1.2 {
            if let Some(n) = d.next.first() {
                ui.label(egui::RichText::new(i18n::trf!("次に測るとよい点: {} (候補が約 {:.1} 個に絞れます)", n.node, n.left_after)).strong())
                    .on_hover_text(i18n::tr("いまの候補が、この点ではそれぞれ違う電圧になるので、測ると見分けられます"));
                for (i, v) in &n.predictions {
                    ui.small(i18n::trf!("  {} なら {}", name(*i), eng(*v, "V")));
                }
                if d.next.len() > 1 {
                    let others: Vec<String> = d.next[1..].iter().map(|x| format!("{} ({:.1})", x.node, x.left_after)).collect();
                    ui.small(i18n::trf!("ほかの候補: {}", others.join("、")));
                }
            } else {
                ui.small(i18n::tr("これ以上は DC の電圧では見分けられません。部品を外して測る・抵抗値を測る・波形を見る、などで確かめます"));
            }
        } else if !d.unexplained {
            ui.small(i18n::tr("候補はほぼ 1 つに絞れました。その部品を外して確かめるか、交換してみましょう"));
        }
        if let Some(p) = pick {
            self.select_part(&p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zener_regulator_with_no_output_points_at_its_series_resistor() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/zener_regulator.schjson");
        let mut e = SchEditor::default();
        e.load_path(path);
        // VIN is fine (9 V) but VOUT is dead: R1 open (or D1 shorted) explains it
        e.set_diag_readings("VIN=9,VOUT=0");
        e.analyze_diagnosis();
        let d = e.diag.result.as_ref().expect("diagnosed");
        let hyps = &e.diag.hyps.as_ref().unwrap().2;
        let top: Vec<String> = d.suspects.iter().take(2).map(|s| format!("{} {}", hyps[s.index].element, hyps[s.index].label)).collect();
        assert!(top.iter().any(|t| t.starts_with("R1") || t.starts_with("D1")), "{top:?}");
        assert!(e.status.contains("故障診断"), "{}", e.status);
        // a reading that is not a number is reported, not guessed
        e.set_diag_readings("VOUT=abc");
        e.analyze_diagnosis();
        assert!(e.diag.error.as_ref().is_some_and(|x| x.contains("数値")));
    }
}
