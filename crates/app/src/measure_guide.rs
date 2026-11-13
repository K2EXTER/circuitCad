//! "組み立てて、性能を測る": the guide window of the last step of the learning path. It walks from the parts list to
//! the bench measurements, and puts the measurement rows into the spec table, where the typed readings are compared
//! with the simulation (`SchEditor::add_measure_rows`).

use crate::learn::{spaced, NOTE};
use eframe::egui::{self, Color32};

#[derive(Default)]
pub struct MeasureGuide {
    pub open: bool,
    /// The step the learner opened (None: the first one not yet done).
    step_open: Option<usize>,
    /// Bumped when a step is opened, so the others fold (their ids change; the opened one is the default open).
    round: u32,
    /// What was done the last frame, so a step that has just been finished hands over to the next one.
    was_done: [bool; 5],
}

/// What the learner asked for in the window.
pub enum GuideAction {
    /// Put the measurement rows into the spec table of the current schematic and run the DC analysis.
    AddRows,
}

const WARN: Color32 = crate::theme::WARN;

/// The ticks of the two steps the program cannot see (the parts list made, the board built), kept with the window's state.
fn ticked_id(step: usize) -> egui::Id {
    egui::Id::new(("measure_guide_ticked", step))
}

impl MeasureGuide {
    /// `rows`: the rows of the spec table; `compared`: how many have a reading compared with the simulation; `done`: the step is
    /// finished (three readings).
    pub fn show(&mut self, ctx: &egui::Context, done: bool, compared: usize, rows: usize) -> Option<GuideAction> {
        if !self.open {
            return None;
        }
        let mut open = true;
        let mut action = None;
        let ticked = |step: usize| ctx.data_mut(|d| *d.get_persisted_mut_or_insert_with(ticked_id(step), || false));
        // what is done: 1 and 2 by the learner's tick, 3 when the table has rows, 4 when a reading is in, 5 when three are compared
        // (once the step is finished it stays finished, whatever schematic is open now)
        let state = if done { [true; 5] } else { [ticked(0), ticked(1), rows > 0, compared > 0, false] };
        let current = state.iter().position(|d| !d).unwrap_or(4);
        let open_step = self.step_open.unwrap_or(current);
        let round = self.round;
        egui::Window::new(i18n::tr("組み立てて、性能を測る")).open(&mut open).default_pos(crate::widgets::beside_panel()).default_width(560.0).default_height(640.0).vscroll(true).show(ctx, |ui| {
            ui.label(spaced("自分で設計した電源 (学習パスの「安定化電源の回路図を自分で設計する」の回路) を実際に作って、シミュレーションとの差を調べます。差が出るのは普通のことで、その理由を考えるのが、このステップの中身です。"));
            ui.add_space(4.0);
            ui.colored_label(WARN, i18n::tr("安全: 実験は 12V 以下の直流 (電池や小さな AC アダプタ) だけで行います。商用電源 (100V) の配線は行いません。電源は、つなぐ前に極性 (+ と −) を確かめ、できればヒューズや電流制限をつけます。"));
            ui.separator();
            let mut opened = None;
            // one step open at a time; a step that opens closes the others
            let mut step = |ui: &mut egui::Ui, k: usize, title: &str, body: &mut dyn FnMut(&mut egui::Ui)| {
                let id = egui::Id::new(("measure_step", k, round));
                let was = egui::collapsing_header::CollapsingState::load(ui.ctx(), id).map_or(open_step == k, |s| s.is_open());
                crate::widgets::section_header_marked(ui, id, open_step == k, title, "", None, state[k], |ui| body(ui));
                if !was && egui::collapsing_header::CollapsingState::load(ui.ctx(), id).is_some_and(|s| s.is_open()) {
                    opened = Some(k);
                }
            };
            let tick = |ui: &mut egui::Ui, k: usize, text: &str| {
                let mut on = ui.ctx().data_mut(|d| *d.get_persisted_mut_or_insert_with(ticked_id(k), || false));
                if ui.checkbox(&mut on, i18n::tr_dyn(text)).on_hover_text(i18n::tr("済んだら印をつけます。次の手順が開きます")).changed() {
                    ui.ctx().data_mut(|d| d.insert_persisted(ticked_id(k), on));
                }
            };
            step(ui, 0, "1. 部品表を作る", &mut |ui| {
                ui.label(spaced("基板の画面の「発注の準備…」で、部品表 (BOM) を書き出せます。どの部品が何個要るかの一覧です。手元にない部品は、値の近いもので置き換えるときに、回路図の値も同じにしておきます (あとで比べるため)。").color(NOTE));
                tick(ui, 0, "部品をそろえた");
            });
            step(ui, 1, "2. 組み立てる", &mut |ui| {
                ui.label(spaced("低い部品 (抵抗・ダイオード) から順につけます。電解コンデンサ・ダイオード・LED には向きがあり、7805 は IN・GND・OUT の並びを、データシートの図で確かめます。電源をつなぐ前に、テスタで入力と GND の間の抵抗を測り、ショートしていないことを確かめます。").color(NOTE));
                tick(ui, 1, "組み立てて、ショートがないのを確かめた");
            });
            step(ui, 2, "3. 測る項目を表に入れる", &mut |ui| {
                ui.label(spaced("下のボタンで、いまの回路図の「スペック」表に、測る項目 (入力電圧・出力電圧・負荷の電流) を入れ、DC 動作点を計算します。回路図には、実験でつなぐ負荷 (例: 100Ω) も描いておきます。").color(NOTE));
                if ui
                    .button(i18n::tr("測る項目を表に入れる"))
                    .on_hover_text(i18n::tr("回路図の「解析」タブの「スペック」表に、入力電圧・出力電圧・電流の行を足して、DC動作点を計算します。回路図に VIN・VOUT という名前 (ラベル) があると、自動で選ばれます"))
                    .clicked()
                {
                    action = Some(GuideAction::AddRows);
                }
            });
            step(ui, 3, "4. 実機で測って、書き込む", &mut |ui| {
                ui.label(spaced("電源を入れて、テスタで入力電圧・出力電圧を測ります。負荷をつないだ状態と、はずした状態の両方で測ると、電圧の下がり方 (負荷による変化) が分かります。読んだ値を、表の各行の「実測」欄に書きます (4.93 や 50m のように)。").color(NOTE));
            });
            step(ui, 4, "5. 差の理由を考える", &mut |ui| {
                ui.label(spaced("実測を書くと、シミュレーションとの差と、「よく合っています / 少し差があります / 大きく違います」が出ます。差があるときは、「差の理由の手がかり」を開いて、部品の誤差・測り方・つなぎ方のどれかを確かめます。").color(NOTE));
            });
            ui.separator();
            if done {
                ui.colored_label(crate::theme::OK, i18n::tr("✔ 完了しました。実測が 3 項目以上、シミュレーションと比べられています"));
            } else {
                ui.label(spaced(&format!("完了の条件: 実測を 3 項目以上、シミュレーションと比べる (いま {compared} / 3)")).color(NOTE));
            }
            if let Some(k) = opened {
                self.step_open = Some(k);
                self.round += 1;
            }
        });
        // the open step has just been finished: the next one opens by itself (a finished step opened again to read stays open)
        if self.step_open.is_some_and(|k| state[k] && !self.was_done[k]) {
            self.step_open = None;
            self.round += 1;
        }
        self.was_done = state;
        if !open {
            self.open = false;
        }
        action
    }
}
