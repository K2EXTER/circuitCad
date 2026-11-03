//! Guided lessons: a sample circuit plus a short list of things to try. Each step is ticked off
//! by itself when the editor reaches the matching state, so the learner is never asked to press
//! a "next" button after doing what was asked.

use super::*;

pub(super) struct ActiveLesson {
    pub(super) idx: usize,
    /// The step whose left-panel page has been shown already.
    pub(super) shown: Option<usize>,
    /// Steps that have been completed (once done, a step stays done).
    pub(super) done: Vec<bool>,
    /// The step on hand and the circuit as they were when the learner last did something, and when (seconds of the UI clock):
    /// the hint of what is missing comes after a quiet time.
    watch: Option<(Option<usize>, String, f64)>,
    /// The option the learner chose for the question of each step (if the step has one and it was answered).
    pub(super) predicted: Vec<Option<usize>>,
    /// How each guess turned out, written when the step was done.
    pub(super) verdicts: Vec<Option<predict::Verdict>>,
}

/// Stands for "no guess": the learner went on without answering the question of a step.
const NO_GUESS: usize = usize::MAX;

/// Seconds the learner has to be quiet on a step before the lesson window says what its condition is missing.
const HINT_AFTER: f64 = 25.0;

struct Step {
    /// Which page of the left panel this step works on.
    tab: PanelTab,
    title: &'static str,
    detail: &'static str,
    done: fn(&SchEditor) -> bool,
}

struct Lesson {
    /// Stable name used to record progress and by the learning paths (never reuse or rename).
    id: &'static str,
    title: &'static str,
    goal: &'static str,
    summary: &'static str,
    /// Where the simulation differs from the real parts: shown as "実物との違い".
    limits: &'static [&'static str],
    /// "実機で確かめよう": what to prepare, how to wire and measure, what to expect, and the safety notes (empty = none yet).
    real: &'static [&'static str],
    /// The sample circuit (embedded, so lessons work without the docs folder).
    circuit: &'static str,
    setup: fn(&mut SchEditor),
    steps: &'static [Step],
}

mod analog;
mod basics;
mod conditions;
mod debug;
mod devices;
mod micro;
mod models;
mod parts;
mod predict;
mod signals;
mod supply;

pub(super) use conditions::Unmet;

#[cfg(test)]
pub(super) use predict::{for_step as prediction_for_step, PREDICTIONS};

#[cfg(test)]
pub(super) use conditions::{dc_fresh_for_test, value_between_for_test, value_of};

const LESSONS: &[Lesson] = &[
    basics::DIVIDER,
    basics::LED,
    basics::RC_LOWPASS,
    basics::BJT_SWITCH,
    basics::NE555,
    basics::RLC,
    devices::TP4056,
    devices::OPAMP_NONINV,
    devices::RECTIFIER,
    devices::COIN_CELL,
    devices::BUCK,
    devices::MOSFET,
    analog::PNP_HIGHSIDE,
    analog::OPAMP_INV,
    analog::DIODE_CURVE,
    analog::ZENER,
    analog::EMITTER_FOLLOWER,
    supply::SERIES_REGULATOR,
    supply::REGULATOR_78XX,
    supply::HEAT,
    supply::PROTECTION,
    supply::COIL_LAYER_SHORT,
    supply::VARIABLE_SUPPLY,
    micro::CRYSTAL,
    micro::RESET,
    micro::DECOUPLING,
    micro::PULLUP,
    micro::PIN_LED,
    micro::LEVELS,
    micro::SERIAL,
    micro::DATASHEET,
    micro::USB_POWER,
    micro::USB_SPEED,
    micro::USB_SIGNAL,
    micro::PWM,
    supply::LDO,
    supply::TL431,
    micro::FLYBACK,
    micro::DEBOUNCE,
    micro::TEMP_SENSORS,
    micro::LIGHT_SENSORS,
    micro::CURRENT_SENSORS,
    micro::ADC_INPUT,
    micro::MEGA_POWER,
    micro::CRYSTAL_LOAD,
    micro::CH340G,
    models::EQUIV_MODEL,
    signals::POLES,
    signals::PHASE_MARGIN,
    signals::NOISE,
    debug::DEBUG,
    parts::RESISTOR,
    parts::CAPACITOR,
    parts::INDUCTOR,
    parts::DIODE_BASICS,
    parts::LED_BASICS,
    parts::ZENER_BASICS,
    parts::TRANSISTOR,
    parts::MOSFET_BASICS,
    parts::OPAMP_BASICS,
    parts::THERMISTOR_BASICS,
    parts::PHOTOTRANSISTOR_BASICS,
    parts::PHOTOCOUPLER_BASICS,
    parts::REGULATOR_BASICS,
    parts::TRANSFORMER_BASICS,
    parts::FUSE_BASICS,
    parts::VARISTOR_BASICS,
    parts::RELAY_BASICS,
    parts::CRYSTAL_BASICS,
    parts::SPEAKER_BASICS,
    parts::LOGIC_BASICS,
];

/// The "実機で確かめよう" notes of lesson `idx`.
#[cfg(test)]
pub(super) fn lesson_real(idx: usize) -> &'static [&'static str] {
    LESSONS.get(idx).map_or(&[], |l| l.real)
}

/// The "実物との違い" notes of lesson `idx`.
#[cfg(test)]
pub(super) fn lesson_limits(idx: usize) -> &'static [&'static str] {
    LESSONS.get(idx).map_or(&[], |l| l.limits)
}

pub fn lesson_ids() -> Vec<&'static str> {
    LESSONS.iter().map(|l| l.id).collect()
}

pub fn lesson_titles() -> Vec<&'static str> {
    LESSONS.iter().map(|l| l.title).collect()
}

impl SchEditor {
    /// Replace the sheet by a sample circuit (used by the board lesson). False if it cannot be read.
    pub fn load_sample(&mut self, json: &str) -> bool {
        let Ok(sch) = serde_json::from_str::<Schematic>(json) else { return false };
        self.new_file();
        self.sch = sch;
        self.reset_history();
        self.fit_pending = true;
        // a finished circuit is for looking at: a click on it must not drop the part the blank sheet starts with
        self.tool = Tool::Select;
        true
    }

    /// Open a built-in sample circuit (as an unsaved sheet, so "保存" asks for a name and never overwrites the sample).
    pub fn open_sample(&mut self, title: &str, json: &str) -> bool {
        if !self.load_sample(json) {
            return false;
        }
        self.status = format!("サンプルを開きました: {title}");
        true
    }

    /// Load a lesson's circuit and start tracking its steps.
    pub fn start_lesson(&mut self, idx: usize) {
        let Some(lesson) = LESSONS.get(idx) else { return };
        let Ok(sch) = serde_json::from_str::<Schematic>(lesson.circuit) else {
            self.status = "レッスンの回路を読み込めませんでした".into();
            return;
        };
        self.new_file();
        self.sch = sch;
        self.reset_history();
        self.lesson_base = self.sch.clone();
        self.fit_pending = true;
        // the app starts with the resistor ready to place: in a lesson a click on the circuit would add one
        self.tool = Tool::Select;
        (lesson.setup)(self);
        self.lesson = Some(ActiveLesson { idx, shown: None, done: vec![false; lesson.steps.len()], watch: None, predicted: vec![None; lesson.steps.len()], verdicts: vec![None; lesson.steps.len()] });
        self.status = format!("レッスン「{}」を始めました。右の案内に沿って操作してみましょう", lesson.title);
    }

    /// The id of the running lesson once every step is ticked (for the progress record).
    pub fn finished_lesson(&self) -> Option<&'static str> {
        let a = self.lesson.as_ref()?;
        a.done.iter().all(|d| *d).then(|| LESSONS.get(a.idx).map(|l| l.id)).flatten()
    }

    /// The text of the step the learner is on (the first that is not ticked), while a lesson runs.
    fn current_step_detail(&self) -> Option<&'static str> {
        let a = self.lesson.as_ref()?;
        let lesson = LESSONS.get(a.idx)?;
        let cur = a.done.iter().position(|d| !d)?;
        lesson.steps.get(cur).map(|s| s.detail)
    }

    /// Makes the button the current lesson step names pulse: a button whose label is in 「」 in the step's text is the one to
    /// press now, and it is hard to find among ten sections of the left panel. Returns the response as it was given.
    pub(super) fn lit(&self, r: egui::Response, label: &str) -> egui::Response {
        let Some(detail) = self.current_step_detail() else { return r };
        if !detail.contains(&format!("「{label}」")) {
            return r;
        }
        let t = r.ctx.input(|i| i.time);
        let a = 0.55 + 0.45 * (t * 4.0).sin().abs();
        let color = egui::Color32::from_rgb(255, 170, 40).gamma_multiply(a as f32);
        r.ctx.layer_painter(r.layer_id).rect_stroke(r.rect.expand(2.0), 4.0, egui::Stroke::new(2.5, color), egui::StrokeKind::Outside);
        r.ctx.request_repaint_after(std::time::Duration::from_millis(50));
        r
    }

    /// Tick off the step the learner is on, once its condition holds.
    pub(super) fn update_lesson(&mut self) {
        let Some(active) = self.lesson.as_ref() else { return };
        let Some(lesson) = LESSONS.get(active.idx) else { return };
        let Some(cur) = active.done.iter().position(|d| !d) else { return };
        self.lesson_unmet.replace(None);
        if (lesson.steps[cur].done)(self) {
            // a guess that was made before the step is judged by the number the simulation gives now
            let chosen = self.lesson.as_ref().and_then(|a| a.predicted[cur]).filter(|c| *c != NO_GUESS);
            let verdict = predict::for_step(lesson.id, cur).zip(chosen).and_then(|(p, c)| (p.measure)(self).map(|v| p.verdict(c, v)));
            if let Some(a) = self.lesson.as_mut() {
                a.done[cur] = true;
                a.verdicts[cur] = verdict;
            }
        }
        // open the page of the left panel that the step on hand works on
        let Some(active) = self.lesson.as_mut() else { return };
        if let Some(next) = active.done.iter().position(|d| !d)
            && active.shown != Some(next) {
                active.shown = Some(next);
                self.panel_tab = lesson.steps[next].tab;
            }
    }

    /// What the step on hand is missing, once the learner has done nothing for [`HINT_AFTER`] seconds (any change of the circuit or of the step
    /// starts the wait again). `None` before that, or when the condition did not say.
    pub(super) fn stuck_hint(&mut self, ctx: &egui::Context, cur: Option<usize>) -> Option<String> {
        let now = ctx.input(|i| i.time);
        let sig = self.signature_cached().to_string();
        let unmet = self.lesson_unmet.borrow().clone();
        let active = self.lesson.as_mut()?;
        let since = match &active.watch {
            Some((step, s, t)) if *step == cur && *s == sig => *t,
            _ => {
                active.watch = Some((cur, sig, now));
                now
            }
        };
        let unmet = unmet.filter(|_| cur.is_some())?;
        let quiet = now - since;
        if quiet < HINT_AFTER {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(HINT_AFTER - quiet + 0.05));
            return None;
        }
        Some(unmet.message())
    }

    /// Floating guide window for the running lesson.
    pub fn lesson_window(&mut self, ctx: &egui::Context) {
        self.update_lesson();
        let Some(active) = self.lesson.as_ref() else { return };
        let Some(lesson) = LESSONS.get(active.idx) else { return };
        let cur = active.done.iter().position(|d| !d);
        let done = active.done.clone();
        let predicted = active.predicted.clone();
        let verdicts = active.verdicts.clone();
        let idx = active.idx;
        let mut close = false;
        let mut restart = false;
        let mut next = None;
        let mut build = None;
        let mut pick: Option<(usize, usize)> = None;
        let hint = self.stuck_hint(ctx, cur);
        // the repair lesson says what was wrong once the learner has mended it
        let cause = self.debug_cause().filter(|_| lesson.id == "debug_circuit" && done.get(1) == Some(&true));
        egui::Window::new(format!("レッスン: {}", lesson.title)).default_width(440.0).anchor(Align2::RIGHT_TOP, Vec2::new(-8.0, 40.0)).show(ctx, |ui| {
            ui.label(lesson.goal);
            ui.small(format!("進み具合: {} / {}", done.iter().filter(|d| **d).count(), done.len()));
            ui.separator();
            for (k, step) in lesson.steps.iter().enumerate() {
                let (mark, color) = if done[k] {
                    ("✔", crate::theme::OK)
                } else if Some(k) == cur {
                    ("▶", crate::theme::WARN)
                } else {
                    ("○", Color32::from_gray(140))
                };
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(color, mark);
                    ui.label(egui::RichText::new(step.title).strong().color(if Some(k) == cur { color } else { ui.visuals().text_color() }));
                });
                let prediction = predict::for_step(lesson.id, k);
                if Some(k) == cur {
                    ui.indent(("lesson_detail", k), |ui| {
                        // the guess comes before the doing: the question first; the step's text (which tells the answer) after it
                        if let Some(p) = prediction.filter(|_| predicted[k].is_none()) {
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                ui.label(egui::RichText::new("予測してみよう").strong().color(crate::theme::WARN));
                                ui.label(p.question);
                                for (o, (label, _)) in p.options.iter().enumerate() {
                                    if ui.button(*label).on_hover_text("やる前に、結果を予測します。当たっても外れても、やったあとに、理由が出ます").clicked() {
                                        pick = Some((k, o));
                                    }
                                }
                                if ui.small_button("予測せずに進める").on_hover_text("予測を飛ばして、手順の説明を読みます").clicked() {
                                    pick = Some((k, NO_GUESS));
                                }
                            });
                        } else {
                            ui.label(step.detail);
                            if let (Some(p), Some(o)) = (prediction, predicted[k].filter(|o| *o != NO_GUESS)) {
                                ui.small(format!("あなたの予測: 「{}」。手順のとおりにやって、確かめましょう", p.options[o].0));
                            }
                        }
                    });
                }
                if let (Some(p), Some(v)) = (prediction, verdicts[k].as_ref()) {
                    ui.indent(("lesson_verdict", k), |ui| {
                        if v.right {
                            ui.colored_label(crate::theme::OK, format!("✔ 予測が当たりました。実際は {} でした", p.show(v.actual)));
                        } else {
                            ui.colored_label(crate::theme::WARN, format!("✘ 予測とは違いました (あなたの予測: 「{}」)。実際は {} でした", p.options[v.chosen].0, p.show(v.actual)));
                        }
                        ui.small(p.explain);
                    });
                }
            }
            if let Some(text) = hint.as_ref() {
                ui.add_space(2.0);
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(crate::theme::WARN, "ヒント");
                    ui.label(text);
                });
            }
            if let Some(cause) = cause {
                ui.add_space(2.0);
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(crate::theme::OK, "仕込まれていた故障");
                    ui.label(cause);
                });
            }
            ui.separator();
            // opened by itself once the lesson is finished (the id changes, so egui opens it again)
            egui::CollapsingHeader::new("実物との違い (このモデルの限界)")
                .id_salt(("limits", idx, cur.is_none()))
                .default_open(cur.is_none())
                .show(ui, |ui| {
                    ui.small("シミュレーションは、部品を単純なモデルにして計算します。実物の回路と、次の点が違います");
                    for l in lesson.limits {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("・");
                            ui.label(*l);
                        });
                    }
                });
            if let Some(cid) = challenge_of_lesson(lesson.id) {
                ui.separator();
                ui.label(egui::RichText::new("自分で作ってみよう").strong());
                ui.label("見るだけでなく、目標だけを示して、自分で部品を置き、配線し、値を決める練習です。作ったあとは、判定ボタンで、動きを調べます");
                if ui.button("チャレンジを始める").on_hover_text("いまの回路を空にして、目標から自分で作ります。あとで、このレッスンに戻れます").clicked() {
                    build = Some(cid);
                }
            }
            if !lesson.real.is_empty() {
                egui::CollapsingHeader::new("実機で確かめよう (実験)").id_salt(("real", idx)).default_open(false).show(ui, |ui| {
                    ui.label("シミュレーションの結果を、実際の部品で確かめます。安全のため、12V 以下の低い電圧だけで行い、商用電源 (100V) は使いません");
                    for l in lesson.real {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("・");
                            ui.label(*l);
                        });
                    }
                });
            }
            if cur.is_none() {
                ui.separator();
                ui.colored_label(crate::theme::OK, "レッスン完了!");
                ui.label(lesson.summary);
                if idx + 1 < LESSONS.len() && ui.button(format!("次のレッスンへ: {}", LESSONS[idx + 1].title)).on_hover_text("このレッスンを終えて、次のレッスンのサンプル回路を開きます").clicked() {
                    next = Some(idx + 1);
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("最初の回路に戻す").on_hover_text("回路を、このレッスンの最初の状態にします").clicked() {
                    restart = true;
                }
                if crate::widgets::close_button(ui, "レッスンの窓") {
                    close = true;
                }
            });
        });
        if let Some((k, o)) = pick
            && let Some(a) = self.lesson.as_mut() {
                a.predicted[k] = Some(o);
            }
        if let Some(id) = build {
            self.start_challenge(id, 0);
            return;
        }
        if let Some(n) = next {
            self.start_lesson(n);
        } else if restart {
            self.start_lesson(idx);
        } else if close {
            self.lesson = None;
        }
    }
}


/// The button that the current step names in 「」 is the one that pulses; every step of every lesson that names a panel button names
/// one that exists (a renamed button would otherwise stop glowing without a sound).
#[cfg(test)]
mod lit_tests {
    use super::*;

    #[test]
    fn the_step_text_names_the_button_to_press() {
        let mut e = SchEditor::default();
        assert!(e.current_step_detail().is_none(), "no lesson, nothing to light");
        let idx = LESSONS.iter().position(|l| l.id == "divider").unwrap();
        e.start_lesson(idx);
        let detail = e.current_step_detail().unwrap();
        assert!(detail.contains("「DC動作点を解析」"), "{detail}");
    }
}
