//! The "シミュレーション設定" window: solver accuracy and convergence aids (SPICE's .options).

use super::*;
use sim_core::{set_options, SimOptions};

const RELTOLS: [(f64, &str); 4] = [(1e-3, "1e-3 (ゆるい・速い)"), (1e-4, "1e-4"), (1e-6, "1e-6 (標準)"), (1e-9, "1e-9 (とても厳しい)")];
const VNTOLS: [(f64, &str); 3] = [(1e-6, "1µV"), (1e-8, "10nV (標準)"), (1e-10, "0.1nV")];
const ABSTOLS: [(f64, &str); 3] = [(1e-9, "1nA"), (1e-12, "1pA (標準)"), (1e-15, "1fA")];
const GMINS: [(f64, &str); 4] = [(1e-9, "1e-9 S (収束しやすい)"), (1e-10, "1e-10 S"), (1e-12, "1e-12 S (標準)"), (1e-14, "1e-14 S (高精度)")];

fn pick(ui: &mut egui::Ui, id: &str, label: &str, value: &mut f64, items: &[(f64, &str)], tip: &str) {
    ui.label(label).on_hover_text(tip);
    let text = items.iter().find(|(v, _)| (*v / *value - 1.0).abs() < 1e-9).map_or_else(|| format!("{value:e}"), |(_, t)| t.to_string());
    egui::ComboBox::from_id_salt(id).selected_text(text).show_ui(ui, |ui| {
        for (v, t) in items {
            ui.selectable_value(value, *v, *t);
        }
    }).response.on_hover_text(tip);
    ui.end_row();
}

impl SchEditor {
    /// Applies the settings to the solver (every frame, so analyses always see them) and shows the window.
    pub fn settings_window(&mut self, ctx: &egui::Context) {
        set_options(self.sim_opts);
        if !self.show_sim_opts {
            return;
        }
        let mut open = true;
        let mut o = self.sim_opts;
        egui::Window::new("シミュレーション設定").open(&mut open).default_width(380.0).show(ctx, |ui| {
            ui.small("ふだんは変えなくて大丈夫です。「収束しませんでした」と出たとき、または結果の精度を上げたいときに調整します。");
            ui.horizontal(|ui| {
                if ui.button("標準").on_hover_text("初期値に戻します").clicked() {
                    o = SimOptions::default();
                }
                if ui.button("収束しやすく").on_hover_text("許容誤差をゆるめ、反復回数を増やし、gmin を大きくします。「収束しませんでした」と出るときに試しましょう。精度は少し落ちます").clicked() {
                    o = SimOptions { reltol: 1e-4, vntol: 1e-6, abstol: 1e-9, gmin: 1e-9, max_iter: 500 };
                }
                if ui.button("高精度").on_hover_text("許容誤差を厳しくします。計算が遅くなり、収束しにくくなることもあります").clicked() {
                    o = SimOptions { reltol: 1e-9, vntol: 1e-10, abstol: 1e-15, gmin: 1e-14, max_iter: 150 };
                }
            });
            ui.separator();
            egui::Grid::new("sim_opts").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                pick(ui, "reltol", "相対許容誤差 (reltol)", &mut o.reltol, &RELTOLS, "反復の前後で値の変化がこの割合より小さくなったら「答えが定まった」と判断します。小さいほど正確ですが、反復が増えます");
                pick(ui, "vntol", "電圧の許容誤差 (vntol)", &mut o.vntol, &VNTOLS, "電圧がこの値以下しか変わらなければ収束とみなします。ごく小さい信号を扱うときは小さくします");
                pick(ui, "abstol", "電流の許容誤差 (abstol)", &mut o.abstol, &ABSTOLS, "電流がこの値以下しか変わらなければ収束とみなします。pA 級の漏れ電流を見たいときは小さくします");
                pick(ui, "gmin", "最小コンダクタンス (gmin)", &mut o.gmin, &GMINS, "すべてのノードとGNDの間に入れるごく小さな導通です。回路が浮いていても計算できるようにする代わりに、ごくわずかに結果をずらします。大きいほど収束しやすく、小さいほど正確です");
                ui.label("反復回数の上限").on_hover_text("1回の計算でやり直す最大回数です。増やすと収束しにくい回路が解けることがありますが、解けないときは時間がかかります");
                let mut n = o.max_iter as u32;
                ui.add(egui::DragValue::new(&mut n).range(10..=10_000).speed(5.0)).on_hover_text("10〜10000");
                o.max_iter = n as usize;
                ui.end_row();
            });
            ui.separator();
            ui.small("変えたあとは、解析をもう一度実行してください (「自動で再計算」がオンなら自動で反映されます)。");
        });
        self.sim_opts = o;
        if !open {
            self.show_sim_opts = false;
        }
    }
}
