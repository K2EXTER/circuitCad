//! "組み立てて、性能を測る": the guide window of the last step of the learning path. It walks from the parts list to
//! the bench measurements, and puts the measurement rows into the spec table, where the typed readings are compared
//! with the simulation (`SchEditor::add_measure_rows`).

use crate::learn::{spaced, NOTE};
use eframe::egui::{self, Color32};

#[derive(Default)]
pub struct MeasureGuide {
    pub open: bool,
}

/// What the learner asked for in the window.
pub enum GuideAction {
    /// Put the measurement rows into the spec table of the current schematic and run the DC analysis.
    AddRows,
}

const WARN: Color32 = Color32::from_rgb(230, 150, 40);

impl MeasureGuide {
    pub fn show(&mut self, ctx: &egui::Context, done: bool, compared: usize) -> Option<GuideAction> {
        if !self.open {
            return None;
        }
        let mut open = true;
        let mut action = None;
        // on the right, so the left panel (where the spec table is) stays in view
        let at = egui::pos2((ctx.content_rect().right() - 580.0).max(0.0), 40.0);
        egui::Window::new("組み立てて、性能を測る").open(&mut open).default_pos(at).default_width(560.0).default_height(700.0).vscroll(true).show(ctx, |ui| {
            ui.label(spaced("自分で設計した電源 (学習パスの「安定化電源の回路図を自分で設計する」の回路) を実際に作って、シミュレーションとの差を調べます。差が出るのは普通のことで、その理由を考えるのが、このステップの中身です。"));
            ui.add_space(4.0);
            ui.colored_label(WARN, "安全: 実験は 12V 以下の直流 (電池や小さな AC アダプタ) だけで行います。商用電源 (100V) の配線は行いません。電源は、つなぐ前に極性 (+ と −) を確かめ、できればヒューズや電流制限をつけます。");
            ui.separator();

            ui.strong("1. 部品表を作る");
            ui.label(spaced("基板の画面の「発注の準備…」で、部品表 (BOM) を書き出せます。どの部品が何個要るかの一覧です。手元にない部品は、値の近いもので置き換えるときに、回路図の値も同じにしておきます (あとで比べるため)。").color(NOTE));
            ui.add_space(4.0);

            ui.strong("2. 組み立てる");
            ui.label(spaced("低い部品 (抵抗・ダイオード) から順につけます。電解コンデンサ・ダイオード・LED には向きがあり、7805 は IN・GND・OUT の並びを、データシートの図で確かめます。電源をつなぐ前に、テスタで入力と GND の間の抵抗を測り、ショートしていないことを確かめます。").color(NOTE));
            ui.add_space(4.0);

            ui.strong("3. 測る項目を表に入れる");
            ui.label(spaced("下のボタンで、いまの回路図の「スペック」表に、測る項目 (入力電圧・出力電圧・負荷の電流) を入れ、DC 動作点を計算します。回路図には、実験でつなぐ負荷 (例: 100Ω) も描いておきます。").color(NOTE));
            if ui
                .button("測る項目を表に入れる")
                .on_hover_text("回路図の「解析」タブの「スペック」表に、入力電圧・出力電圧・電流の行を足して、DC動作点を計算します。回路図に VIN・VOUT という名前 (ラベル) があると、自動で選ばれます")
                .clicked()
            {
                action = Some(GuideAction::AddRows);
            }
            ui.add_space(4.0);

            ui.strong("4. 実機で測って、書き込む");
            ui.label(spaced("電源を入れて、テスタで入力電圧・出力電圧を測ります。負荷をつないだ状態と、はずした状態の両方で測ると、電圧の下がり方 (負荷による変化) が分かります。読んだ値を、表の各行の「実測」欄に書きます (4.93 や 50m のように)。").color(NOTE));
            ui.add_space(4.0);

            ui.strong("5. 差の理由を考える");
            ui.label(spaced("実測を書くと、シミュレーションとの差と、「よく合っています / 少し差があります / 大きく違います」が出ます。差があるときは、「差の理由の手がかり」を開いて、部品の誤差・測り方・つなぎ方のどれかを確かめます。").color(NOTE));
            ui.separator();

            if done {
                ui.colored_label(Color32::from_rgb(60, 160, 90), "✔ 完了しました。実測が 3 項目以上、シミュレーションと比べられています");
            } else {
                ui.label(spaced(&format!("完了の条件: 実測を 3 項目以上、シミュレーションと比べる (いま {compared} / 3)")).color(NOTE));
            }
        });
        if !open {
            self.open = false;
        }
        action
    }
}
