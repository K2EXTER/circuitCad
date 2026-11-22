//! The controls of a running digital circuit (as the band over the app's canvas): run / pause, one clock, one event, the clock's
//! speed, slow motion; and what it found: the glitches, the flip-flops' timing violations, the truth table.

use crate::view::level_colour;
use eframe::egui;
use i18n::{tr, trf};
use sim_core::live_logic::{LiveLogic, Truth, HZ_RANGE};
use sim_core::logic_level::Level;

/// The buttons, wrapped onto more lines on a narrow screen.
pub fn controls(ui: &mut egui::Ui, ll: &mut LiveLogic) {
    ui.horizontal_wrapped(|ui| {
        let (icon, tip) = if ll.running { ("⏸", tr("一時停止: 時間を止めます。止めている間は「1 クロック」「1 イベント」で少しずつ進めます")) } else { ("▶", tr("動かす: 時間を進めます。クロックはつまみの速さで変わります")) };
        if ui.button(icon).on_hover_text(tip).clicked() {
            ll.running = !ll.running;
        }
        if ui
            .add_enabled(!ll.clocks.is_empty(), egui::Button::new(tr("1 クロック")))
            .on_hover_text(tr("クロックを 1 回 (上がって下がる) だけ進めて、一時停止します。カウンタが 1 つ進むのを確かめられます"))
            .on_disabled_hover_text(tr("この回路にはクロック (パルスの電源) がありません"))
            .clicked()
        {
            ll.step_clock();
        }
        if ui.button(tr("1 イベント")).on_hover_text(tr("次にゲートの出力が変わる時刻まで進めて、一時停止します。ゲートの遅れ (数 ns) の順に、信号が 1 段ずつ伝わるのを見られます")).clicked() {
            ll.running = false;
            ll.step_event();
        }
        if !ll.clocks.is_empty() {
            ui.add(egui::Slider::new(&mut ll.hz, HZ_RANGE.0..=HZ_RANGE.1).logarithmic(true).suffix(" Hz").max_decimals(1))
                .on_hover_text(tr("クロックの速さ (1 秒に何回)。目で追える速さにしてあります。実物の回路は MHz で動きます"));
        }
        ui.checkbox(&mut ll.slow, tr("ゆっくり")).on_hover_text(tr("ゲートの遅れ (10ns ほど) を 1 秒ほどに引き延ばします。入力を変えると、信号がゲートを 1 段ずつ伝わり、一瞬だけ違う値 (グリッチ) が出るのも見えます"));
        let t = sim_core::format_time(ll.time());
        let state = if let Some(e) = &ll.error {
            e.clone()
        } else if !ll.settled() {
            trf!("伝わっています (時刻 {})", t)
        } else if ll.running {
            trf!("時刻 {}", t)
        } else {
            trf!("一時停止中 (時刻 {})", t)
        };
        ui.weak(state);
    });
}

/// The last glitch and the timing violations, each a line.
pub fn findings(ui: &mut egui::Ui, ll: &LiveLogic) {
    if let Some(g) = ll.glitches.last() {
        let level = if g.high { "High" } else { "Low" };
        ui.colored_label(egui::Color32::from_rgb(255, 170, 60), trf!("グリッチ: {} が {} だけ {} になりました (線が赤く光っています)", g.net, sim_core::format_time(g.width), level))
            .on_hover_text(tr("ゲートの入力が少しずれて変わると、出力に一瞬だけ違う値が出ます (グリッチ)。この線をクロックやクリアにつなぐと、数えたり消えたりしてしまいます。「ゆっくり」で、どの順に変わったかを見られます"));
    }
    for v in ll.races.iter().take(3) {
        let what = match v.kind {
            sim_core::TimingKind::Setup => tr("セットアップ時間の違反 (クロックの直前にデータが変わった)"),
            sim_core::TimingKind::Hold => tr("ホールド時間の違反 (クロックの直後にデータが変わった)"),
            sim_core::TimingKind::PulseWidth => tr("パルス幅の違反 (クロックが短すぎる)"),
        };
        ui.colored_label(egui::Color32::LIGHT_RED, trf!("競合: {}", trf!("{}: {} ({} 回)", v.cell, what, v.count)));
    }
}

/// The truth table, a row per combination; the rows not tried yet are blank, a cell that has shown both 0 and 1 is "記憶".
pub fn truth_table(ui: &mut egui::Ui, t: &Truth) {
    let n = t.ins.len();
    let head = if t.complete() { tr("真理値表 (全部埋まりました)").to_string() } else { trf!("真理値表 ({}/{} 行)", t.rows.len(), 1 << n) };
    ui.label(egui::RichText::new(head).strong()).on_hover_text(tr("入力の組み合わせを全部試すと、表が完成します。同じ入力で前と違う出力になった所は「記憶」(前の状態で決まる) と出ます"));
    egui::Grid::new("live_truth").striped(true).spacing(egui::vec2(10.0, 0.0)).show(ui, |ui| {
        for name in t.ins.iter().chain(&t.outs) {
            ui.label(egui::RichText::new(name).monospace().strong());
        }
        ui.end_row();
        for key in 0..(1u32 << n) {
            for b in (0..n).rev() {
                ui.label(egui::RichText::new(if key >> b & 1 == 1 { "1" } else { "0" }).monospace());
            }
            match t.rows.get(&key) {
                Some(outs) => {
                    for o in outs {
                        let (s, c) = match o {
                            Some(true) => ("1", level_colour(Level::High)),
                            Some(false) => ("0", level_colour(Level::Low)),
                            None => ("記憶", level_colour(Level::Mid)),
                        };
                        ui.label(egui::RichText::new(tr(s)).monospace().color(c));
                    }
                }
                None => {
                    for _ in &t.outs {
                        ui.label("·");
                    }
                }
            }
            ui.end_row();
        }
    });
}
