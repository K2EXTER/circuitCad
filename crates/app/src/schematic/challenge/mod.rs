//! "作ってみよう": a goal instead of a finished circuit. The learner places and wires the parts, and a
//! "判定する" button judges the circuit by what it does (it is simulated with loads, inputs and
//! frequencies of the checker's own choosing), not by comparing it with one model answer. A failed
//! check says what to look at. Each challenge is plain data: its start sheet and its list of checks.

use super::*;

/// One line of the checklist after "判定する".
pub(super) struct CheckResult {
    pub label: String,
    /// None = not judged (an earlier check failed first).
    pub ok: Option<bool>,
    pub detail: String,
}

/// The challenge being worked on.
pub(super) struct ActiveChallenge {
    pub id: &'static str,
    /// 0 = the parts are placed, only wiring is left; 1 = place the parts too.
    pub level: u8,
    pub results: Vec<CheckResult>,
    pub hints: usize,
    pub passed: bool,
}

/// Which part a current check looks at.
#[derive(Clone, Copy)]
enum Who {
    Refdes(&'static str),
    /// The first part of this kind on the sheet (e.g. the LED).
    Kind(Kind),
}

/// One thing the judge tests. `set` = (part number, value) pairs applied to a copy of the learner's circuit
/// first (e.g. a different input voltage); `load` = a resistor from VOUT to GND the checker adds.
enum Check {
    /// No ERC error (wiring slips).
    Erc,
    /// A net with this name exists (the learner names the output so the judge can measure it).
    Named { net: &'static str },
    /// The voltage of `net` is within [lo, hi]. `explain` maps a measured value to advice ({v} = the value).
    /// `gate`: if this fails, the later checks are not judged yet.
    Voltage { label: &'static str, net: &'static str, load: Option<&'static str>, set: &'static [(&'static str, &'static str)], lo: f64, hi: f64, explain: &'static [(f64, f64, &'static str)], gate: bool },
    /// The voltage of `net` stays within [lo, hi] while source `vary.0` is set to `vary.1` and to `vary.2`.
    Stay { label: &'static str, net: &'static str, load: Option<&'static str>, vary: (&'static str, &'static str, &'static str), lo: f64, hi: f64, hint: &'static str },
    /// A current (absolute, in amperes) is within [lo, hi].
    Current { label: &'static str, who: Who, set: &'static [(&'static str, &'static str)], lo: f64, hi: f64, explain: &'static [(f64, f64, &'static str)] },
    /// AC gain of the net VOUT at `freq` (dB, against the 1 V AC source of the circuit) is within [lo, hi].
    Gain { label: &'static str, freq: f64, lo: f64, hi: f64, explain: &'static [(f64, f64, &'static str)] },
    /// The sheet has at least `min` parts of this kind (e.g. the two capacitors a regulator needs).
    Has { label: &'static str, kind: Kind, min: usize, hint: &'static str },
    /// No part is past its rating (judged with the given load / settings).
    Ratings { label: &'static str, load: Option<&'static str>, set: &'static [(&'static str, &'static str)] },
}

struct Challenge {
    id: &'static str,
    /// The lesson this challenge belongs to.
    lesson: &'static str,
    title: &'static str,
    goal: &'static str,
    /// From a nudge to nearly the answer; shown one at a time.
    hints: &'static [&'static str],
    /// A circuit that passes, as a sample file; `drop` / `values` / `labels` turn it into the answer.
    solution: &'static str,
    drop: &'static [&'static str],
    values: &'static [(&'static str, &'static str)],
    labels: &'static [(&'static str, &'static str, &'static str)],
    /// Net names of the sample to rename (so that the judged output net is called VOUT).
    rename: &'static [(&'static str, &'static str)],
    /// Level 0 / level 1 start sheets: the parts kept (with their positions) and the starting values.
    parts0: &'static [(&'static str, Pt)],
    parts1: &'static [(&'static str, Pt)],
    start_values: &'static [(&'static str, &'static str)],
    /// Ground symbols: (part, pin, dx, dy) from that pin, for level 0 / level 1.
    grounds0: &'static [(&'static str, &'static str, i32, i32)],
    grounds1: &'static [(&'static str, &'static str, i32, i32)],
    checks: &'static [Check],
}

mod basic;
mod judge;
mod power;
#[cfg(test)]
mod tests;
mod window;

use judge::*;

const NO: &[(&str, &str)] = &[];

const CHALLENGES: &[Challenge] = &[
    basic::ZENER,
    basic::DIVIDER,
    basic::LED,
    basic::RC_LOWPASS,
    basic::EMITTER_FOLLOWER,
    basic::OPAMP_INV,
    power::SERIES_REGULATOR,
    power::REGULATOR_78XX,
    power::DRAW_PSU,
];

fn challenge(id: &str) -> Option<&'static Challenge> {
    CHALLENGES.iter().find(|c| c.id == id)
}

/// Whether a challenge of this id exists (items of a learning path that are only a challenge).
#[cfg(test)]
pub fn challenge_exists(id: &str) -> bool {
    challenge(id).is_some()
}

/// The id of the challenge of a lesson, if it has one.
pub fn challenge_of_lesson(lesson: &str) -> Option<&'static str> {
    CHALLENGES.iter().find(|c| c.lesson == lesson).map(|c| c.id)
}

/// The key under which a passed challenge is recorded (see `learn::Progress`).
pub fn progress_key(id: &str) -> String {
    format!("{id}+build")
}

impl SchEditor {
    /// Starts challenge `id` at `level` on a fresh sheet (the running lesson is put away).
    pub fn start_challenge(&mut self, id: &str, level: u8) {
        let Some(ch) = challenge(id) else { return };
        self.new_file();
        self.sch = starting_sheet(ch, level);
        self.reset_history();
        self.fit_pending = true;
        self.lesson = None;
        self.panel_tab = PanelTab::Draw;
        self.challenge = Some(ActiveChallenge { id: ch.id, level, results: vec![], hints: 0, passed: false });
        self.status = format!("「{}」を始めました。回路を作って、「判定する」を押してください", ch.title);
    }

    /// The key of the challenge just passed, for the progress record.
    pub fn finished_challenge(&self) -> Option<String> {
        self.challenge.as_ref().filter(|c| c.passed).map(|c| progress_key(c.id))
    }

    /// "判定する": judges the sheet and keeps the result for the window.
    pub fn run_judge(&mut self) {
        let results = self.judge_challenge();
        let passed = !results.is_empty() && results.iter().all(|r| r.ok == Some(true));
        if let Some(a) = self.challenge.as_mut() {
            a.results = results;
            a.passed = passed;
        }
        self.status = if passed { "判定: すべて合格です".into() } else { "判定: 直すところがあります (✖ の項目を見てください)".into() };
    }
}
