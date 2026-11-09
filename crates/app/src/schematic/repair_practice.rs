//! Repair practice with a virtual meter: the schematic is the healthy design, and a "built board" of it has one fault that the
//! learner does not see. Clicking a wire or a pin measures the board there (the faulty circuit's voltage, with a meter's small
//! spread). The learner names the part and how it is broken; the answer is checked, and the order of measuring that the
//! troubleshooting (`sim_core::troubleshoot`) would have used is shown beside the learner's own.

use super::*;
use sim_core::{fault_hypotheses, troubleshoot, Hypothesis, Reading};
use std::sync::Arc;

/// One practice: the faults of the circuit, the one that was put in, and what the learner did.
pub(super) struct RepairPractice {
    hyps: Arc<Vec<Hypothesis>>,
    /// Index of the fault in `hyps` (never 0, the healthy circuit).
    pub(super) fault: usize,
    /// The point where the trouble shows (named in the task).
    pub(super) symptom: String,
    seed: u64,
    /// (node, volts) in the order measured.
    pub(super) readings: Vec<(String, f64)>,
    /// Clicks on the sheet measure.
    pub(super) probing: bool,
    answer_part: String,
    answer_way: usize,
    pub(super) wrong: u32,
    pub(super) hints: u32,
    pub(super) solved: bool,
    message: Option<(bool, String)>,
}

/// A small, repeatable meter spread for a reading: up to ±(0.3 % + 2 mV).
fn meter(v: f64, seed: u64, node: &str) -> f64 {
    let mut h = seed ^ 0x9e37_79b9_7f4a_7c15;
    for b in node.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
    }
    let u = ((h >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0;
    let v = v + u * (0.003 * v.abs() + 0.002);
    (v * 1000.0).round() / 1000.0
}

impl RepairPractice {
    /// The voltage the board shows at `node` (None: no such point).
    fn read(&self, node: &str) -> Option<f64> {
        self.hyps[self.fault].voltage(node).map(|v| meter(v, self.seed, node))
    }

    /// Two faults that give the same voltage at every point cannot be told apart by measuring: either is a right answer.
    fn same_as_fault(&self, i: usize) -> bool {
        let (a, b) = (&self.hyps[i], &self.hyps[self.fault]);
        i == self.fault || (i != 0 && a.voltages.iter().all(|(n, v)| b.voltage(n).is_some_and(|w| (v - w).abs() <= sim_core::reading_spread(w))))
    }

    /// Checks an answer (index into the hypotheses) and says why it is right or not.
    pub(super) fn answer(&mut self, i: usize) {
        if self.same_as_fault(i) {
            self.solved = true;
            self.probing = false;
            let best = self.best_order();
            let truth = &self.hyps[self.fault];
            self.message = Some((true, format!(
                "正解: {} の {}。あなたは {} 回測りました。故障診断の順なら {} 回 ({})",
                truth.element,
                truth.label.split(" / ").next().unwrap_or(""),
                self.readings.len(),
                best.len(),
                best.join(" → ")
            )));
        } else {
            self.wrong += 1;
            let h = &self.hyps[i];
            // why it is not that one: a reading it does not fit
            let miss = self.readings.iter().find(|(n, v)| h.voltage(n).is_some_and(|p| (p - v).abs() > 2.0 * sim_core::reading_spread(*v)));
            // one whole sentence each, so the English table finds it
            let text = match miss {
                Some((n, v)) => format!("違います。その故障なら {n} は {} になるはずですが、測った値は {} です", eng(h.voltage(n).unwrap_or(0.0), "V"), eng(*v, "V")),
                None => "違います。いままで測った点では、その故障とも合いますが、ほかの点が合いません。まだ測っていない点を測ってみましょう".into(),
            };
            self.message = Some((false, text));
        }
    }

    /// How the troubleshooting would have measured: from the symptom, always the point it suggests, until one cause is left.
    fn best_order(&self) -> Vec<String> {
        let mut readings = vec![Reading { node: self.symptom.clone(), volts: self.read(&self.symptom).unwrap_or(0.0) }];
        let mut order = vec![self.symptom.clone()];
        for _ in 0..8 {
            let d = troubleshoot(&self.hyps, &readings);
            if d.suspects.first().is_some_and(|s| s.probability > 0.8) || d.left < 1.3 {
                break;
            }
            let Some(n) = d.next.first() else { break };
            readings.push(Reading { node: n.node.clone(), volts: self.read(&n.node).unwrap_or(0.0) });
            order.push(n.node.clone());
        }
        order
    }
}

impl SchEditor {
    /// Starts a practice on the sheet as it is: the faults are solved, and one that shows at `symptom` is put in "the board".
    pub(super) fn start_repair(&mut self, symptom: &str, seed: u64) {
        let hyps = match fault_hypotheses(&self.sch, T_NOM_C, false) {
            Ok(h) => h,
            Err(e) => {
                self.status = format!("修理の練習を始められません: {e}");
                return;
            }
        };
        let healthy = hyps[0].voltage(symptom).unwrap_or(0.0);
        // a fault the learner can notice: it moves the symptom point clearly, and it is not one that hardly ever happens
        let visible: Vec<usize> = (1..hyps.len())
            .filter(|&i| hyps[i].prior >= 0.3 && hyps[i].voltage(symptom).is_some_and(|v| (v - healthy).abs() > 0.3_f64.max(0.1 * healthy.abs())))
            .collect();
        let Some(&fault) = visible.get((seed % visible.len().max(1) as u64) as usize) else {
            self.status = "この回路には、出力に表れる故障がありません".into();
            return;
        };
        self.repair = Some(RepairPractice { hyps: Arc::new(hyps), fault, symptom: symptom.into(), seed, readings: vec![], probing: true, answer_part: String::new(), answer_way: 0, wrong: 0, hints: 0, solved: false, message: None });
        self.status = "修理の練習: テスターを当てる点 (配線かピン) をクリックすると、その点の電圧を測ります".into();
    }

    /// A click on the sheet while probing: the voltage of the board at that net.
    pub(super) fn repair_probe(&mut self, pt: Pt) {
        let Some(net) = net_at(&self.sch, pt) else {
            self.status = "配線かピンの上をクリックすると、そこを測ります".into();
            return;
        };
        let Some(r) = self.repair.as_mut() else { return };
        match r.read(&net) {
            Some(v) => {
                if !r.readings.iter().any(|(n, _)| *n == net) {
                    r.readings.push((net.clone(), v));
                }
                self.status = format!("テスター: {net} は {}", eng(v, "V"));
            }
            None => self.status = format!("{net} は測れません"),
        }
    }

    /// The practice's part of the lesson window: what to do, the meter's readings, the answer.
    pub(super) fn repair_ui(&mut self, ui: &mut egui::Ui) {
        let Some(r) = self.repair.as_mut() else { return };
        ui.separator();
        ui.label(egui::RichText::new(i18n::tr("テスター")).strong());
        ui.horizontal_wrapped(|ui| {
            let label = if r.probing { i18n::tr("測っています (クリックで測る)") } else { i18n::tr("テスターを当てる") };
            if ui.selectable_label(r.probing, label).on_hover_text(i18n::tr("オンのあいだ、回路図の配線かピンをクリックすると、作った基板のその点の電圧 (GND から) を測ります。回路図は正しい設計のままで、壊れているのは基板のほうです")).clicked() {
                r.probing = !r.probing;
            }
            ui.small(i18n::trf!("測った回数: {}", r.readings.len()));
        });
        if r.readings.is_empty() {
            ui.small(i18n::trf!("まず、{} を測ってみましょう", r.symptom));
        } else {
            egui::Grid::new("repair_readings").num_columns(2).spacing([10.0, 1.0]).show(ui, |ui| {
                for (n, v) in &r.readings {
                    ui.label(n);
                    ui.label(eng(*v, "V"));
                    ui.end_row();
                }
            });
        }
        if r.solved {
            return;
        }
        // the answer: a part, then how it is broken
        let mut parts: Vec<String> = r.hyps.iter().skip(1).map(|h| h.element.clone()).collect();
        parts.sort();
        parts.dedup();
        ui.horizontal_wrapped(|ui| {
            ui.label(i18n::tr("原因"));
            let shown = if r.answer_part.is_empty() { i18n::tr("部品").to_string() } else { r.answer_part.clone() };
            egui::ComboBox::from_id_salt("repair_part").width(60.0).selected_text(shown).show_ui(ui, |ui| {
                for p in &parts {
                    if ui.selectable_label(r.answer_part == *p, p).clicked() {
                        r.answer_part = p.clone();
                        r.answer_way = 0;
                    }
                }
            });
            let ways: Vec<usize> = (1..r.hyps.len()).filter(|&i| r.hyps[i].element == r.answer_part).collect();
            if !ways.is_empty() {
                let way_name = |i: usize| i18n::tr_dyn(r.hyps[i].label.split(" / ").next().unwrap_or(""));
                let cur = ways.get(r.answer_way).copied().unwrap_or(ways[0]);
                egui::ComboBox::from_id_salt("repair_way").width(150.0).selected_text(way_name(cur)).show_ui(ui, |ui| {
                    for (k, &i) in ways.iter().enumerate() {
                        ui.selectable_value(&mut r.answer_way, k, way_name(i));
                    }
                });
            }
        });
        ui.horizontal_wrapped(|ui| {
            let ways: Vec<usize> = (1..r.hyps.len()).filter(|&i| r.hyps[i].element == r.answer_part).collect();
            let chosen = ways.get(r.answer_way).copied();
            if ui.add_enabled(chosen.is_some(), egui::Button::new(i18n::tr("これが原因"))).on_hover_text(i18n::tr("選んだ部品と壊れ方を答えます。外れても、続けて測れます")).clicked()
                && let Some(i) = chosen
            {
                r.answer(i);
            }
            if ui.small_button(i18n::tr("ヒント")).on_hover_text(i18n::tr("いま測った値から、次に測るとよい点を教えます (使った回数は記録されます)")).clicked() {
                r.hints += 1;
                let readings: Vec<Reading> = r.readings.iter().map(|(n, v)| Reading { node: n.clone(), volts: *v }).collect();
                let text = if readings.is_empty() {
                    format!("まず、症状の出ている {} を測ります", r.symptom)
                } else {
                    match troubleshoot(&r.hyps, &readings).next.first() {
                        Some(n) => format!("次は {} を測ると、候補がよく絞れます", n.node),
                        None => "測った値で、もう答えを選べます。値が正しい回路 (DC動作点) と違う所の近くの部品を疑います".into(),
                    }
                };
                r.message = Some((true, text));
            }
        });
        if let Some((good, m)) = &r.message {
            ui.colored_label(if *good { crate::theme::OK } else { crate::theme::WARN }, i18n::tr_dyn(m));
        }
    }

    /// Measures the named nets (command line `--probe MID,VIN`, for screenshots).
    pub fn probe_nets(&mut self, nets: &str) {
        for net in nets.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            let at = self.sch.components.iter().flat_map(|c| c.pins()).map(|(_, p)| p).find(|p| net_at(&self.sch, *p).as_deref() == Some(net));
            if let Some(p) = at {
                self.repair_probe(p);
            }
        }
    }

    /// Ends probing (Esc or a right click on the sheet).
    pub(super) fn stop_repair_probe(&mut self) -> bool {
        match self.repair.as_mut() {
            Some(r) if r.probing => {
                r.probing = false;
                self.status = "テスターを外しました".into();
                true
            }
            _ => false,
        }
    }

    pub(super) fn repair_probing(&self) -> bool {
        self.repair.as_ref().is_some_and(|r| r.probing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_practice(file: &str, symptom: &str, seed: u64) -> SchEditor {
        let mut e = SchEditor::default();
        e.load_path(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples").join(file));
        e.start_repair(symptom, seed);
        e
    }

    #[test]
    fn a_practice_hides_a_visible_fault_and_the_meter_reads_the_broken_board() {
        for seed in 0..6 {
            let e = with_practice("series_regulator.schjson", "VOUT", seed);
            let r = e.repair.as_ref().expect("started");
            let ok = r.hyps[0].voltage("VOUT").unwrap();
            let v = r.read("VOUT").unwrap();
            assert!((v - ok).abs() > 0.3, "seed {seed}: the fault shows at VOUT ({ok} → {v})");
            assert!(r.hyps[r.fault].prior >= 0.3);
            // the schematic itself is untouched: it is the healthy design
            assert!(e.sch.components.iter().all(|c| c.value != "0"));
        }
    }

    #[test]
    fn the_right_answer_is_accepted_and_compared_with_the_best_order() {
        let mut e = with_practice("zener_regulator.schjson", "VOUT", 3);
        let r = e.repair.as_mut().unwrap();
        r.readings.push(("VOUT".into(), r.read("VOUT").unwrap()));
        let truth = r.fault;
        assert!(r.same_as_fault(truth));
        assert!(!r.same_as_fault(0), "the healthy circuit is never the answer");
        let best = r.best_order();
        assert_eq!(best[0], "VOUT");
        assert!(best.len() <= 4, "{best:?}");
    }

    /// A point on the sheet that is on `net` (a pin of a part on it).
    fn point_on(e: &SchEditor, net: &str) -> Pt {
        e.sch.components.iter().flat_map(|c| c.pins()).map(|(_, p)| p).find(|p| net_at(&e.sch, *p).as_deref() == Some(net)).expect("a pin on the net")
    }

    #[test]
    fn every_repair_lesson_walks_from_the_symptom_to_the_answer() {
        for (id, symptom) in [("repair_divider", "MID"), ("repair_zener", "VOUT"), ("repair_led", "COL"), ("repair_regulator", "VOUT")] {
            let mut e = SchEditor::default();
            let idx = super::super::lessons::lesson_ids().iter().position(|l| *l == id).unwrap();
            e.start_lesson(idx);
            assert!(e.repair.is_some(), "{id}: the practice starts with the lesson");
            // a fixed fault for the test
            e.start_repair(symptom, 1);
            let at = point_on(&e, symptom);
            e.repair_probe(at);
            e.update_lesson();
            e.run_dc();
            e.update_lesson();
            // a wrong answer first (the healthy circuit's part with a fault that does not fit), then the right one
            let fault = e.repair.as_ref().unwrap().fault;
            let wrong = (1..e.repair.as_ref().unwrap().hyps.len()).find(|&i| !e.repair.as_ref().unwrap().same_as_fault(i)).unwrap();
            e.repair.as_mut().unwrap().answer(wrong);
            assert!(!e.repair.as_ref().unwrap().solved && e.repair.as_ref().unwrap().wrong == 1, "{id}");
            e.repair.as_mut().unwrap().answer(fault);
            e.update_lesson();
            assert!(e.finished_lesson() == Some(id), "{id}: every step ticked: {:?}", e.lesson.as_ref().map(|l| l.done.clone()));
            assert!(e.repair.as_ref().unwrap().message.as_ref().is_some_and(|(ok, m)| *ok && m.contains("正解")), "{id}");
        }
    }
}
