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
}

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
mod devices;
mod micro;
mod supply;

#[cfg(test)]
pub(super) use conditions::value_of;

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
        (lesson.setup)(self);
        self.lesson = Some(ActiveLesson { idx, shown: None, done: vec![false; lesson.steps.len()] });
        self.status = format!("レッスン「{}」を始めました。右の案内に沿って操作してみましょう", lesson.title);
    }

    /// The id of the running lesson once every step is ticked (for the progress record).
    pub fn finished_lesson(&self) -> Option<&'static str> {
        let a = self.lesson.as_ref()?;
        a.done.iter().all(|d| *d).then(|| LESSONS.get(a.idx).map(|l| l.id)).flatten()
    }

    /// Tick off the step the learner is on, once its condition holds.
    pub(super) fn update_lesson(&mut self) {
        let Some(active) = self.lesson.as_ref() else { return };
        let Some(lesson) = LESSONS.get(active.idx) else { return };
        let Some(cur) = active.done.iter().position(|d| !d) else { return };
        if (lesson.steps[cur].done)(self)
            && let Some(a) = self.lesson.as_mut() {
                a.done[cur] = true;
            }
        // open the page of the left panel that the step on hand works on
        let Some(active) = self.lesson.as_mut() else { return };
        if let Some(next) = active.done.iter().position(|d| !d)
            && active.shown != Some(next) {
                active.shown = Some(next);
                self.panel_tab = lesson.steps[next].tab;
            }
    }

    /// Floating guide window for the running lesson.
    pub fn lesson_window(&mut self, ctx: &egui::Context) {
        self.update_lesson();
        let Some(active) = self.lesson.as_ref() else { return };
        let Some(lesson) = LESSONS.get(active.idx) else { return };
        let cur = active.done.iter().position(|d| !d);
        let done = active.done.clone();
        let idx = active.idx;
        let mut close = false;
        let mut restart = false;
        let mut next = None;
        let mut build = None;
        egui::Window::new(format!("レッスン: {}", lesson.title)).default_width(360.0).anchor(Align2::RIGHT_TOP, Vec2::new(-8.0, 40.0)).show(ctx, |ui| {
            ui.label(lesson.goal);
            ui.small(format!("進み具合: {} / {}", done.iter().filter(|d| **d).count(), done.len()));
            ui.separator();
            for (k, step) in lesson.steps.iter().enumerate() {
                let (mark, color) = if done[k] {
                    ("✔", Color32::from_rgb(60, 160, 90))
                } else if Some(k) == cur {
                    ("▶", Color32::from_rgb(220, 150, 40))
                } else {
                    ("○", Color32::from_gray(140))
                };
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(color, mark);
                    ui.label(egui::RichText::new(step.title).strong().color(if Some(k) == cur { color } else { ui.visuals().text_color() }));
                });
                if Some(k) == cur {
                    ui.indent(("lesson_detail", k), |ui| {
                        ui.label(step.detail);
                    });
                }
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
                ui.colored_label(Color32::from_rgb(60, 160, 90), "レッスン完了!");
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
                if ui.button("閉じる").clicked() {
                    close = true;
                }
            });
        });
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

