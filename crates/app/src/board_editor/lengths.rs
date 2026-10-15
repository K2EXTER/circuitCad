//! "長さ合わせ (蛇行)" section of the board panel: the length of each routed net, and a serpentine that brings the
//! chosen nets to the same length.

use super::*;
use pcb_core::meander::{self, MeanderStyle};

impl BoardEditor {
    pub(super) fn panel_lengths(&mut self, ui: &mut egui::Ui) {
        let nets: Vec<(String, f64)> = self.board_nets().into_iter().map(|n| (n.clone(), meander::length_of(&self.board, &n))).filter(|(_, l)| *l > 0.0).collect();
        egui::CollapsingHeader::new(format!("長さ合わせ (蛇行) ({})", self.meander_nets.len())).id_salt("meander").default_open(false).show(ui, |ui| {
            ui.small("同時に届いてほしい線 (バス、クロックとデータなど) の長さを、配線の一部に蛇行を入れてそろえます");
            if nets.is_empty() {
                ui.small("配線されたネットがありません");
                return;
            }
            ui.small("そろえるネットを選びます (配線の長さ):");
            for (n, l) in &nets {
                let mut on = self.meander_nets.contains(n);
                if ui.checkbox(&mut on, format!("{n}  {l:.2} mm")).on_hover_text("チェックしたネットの長さをそろえます。長さは、そのネットの配線 (ビアは除く) の長さの合計です").changed() {
                    if on {
                        self.meander_nets.push(n.clone());
                    } else {
                        self.meander_nets.retain(|x| x != n);
                    }
                }
            }
            ui.horizontal(|ui| {
                ui.label("角の形");
                ui.selectable_value(&mut self.meander.style, MeanderStyle::Square, "四角").on_hover_text("蛇行の角を直角にします。いちばん単純で、同じ長さを少ない場所で稼げます");
                ui.selectable_value(&mut self.meander.style, MeanderStyle::Chamfer, "面取り").on_hover_text("角を 45° で切り落とします。直角がなくなり、きれいで製造しやすい形です");
                ui.selectable_value(&mut self.meander.style, MeanderStyle::Round, "丸").on_hover_text("角を丸めます。高周波や音響の線で、反射や角の電界集中を減らしたいときに向きます (長さを稼ぐのに、少し広い場所が要ります)");
            });
            ui.add(egui::DragValue::new(&mut self.meander.height_max).speed(0.1).range(0.5..=10.0).prefix("山の高さ ").suffix(" mm")).on_hover_text("蛇行の山の高さの目安 (上限)。大きいほど山の数が減りますが、周りの配線とぶつかりやすくなります");
            let picked: Vec<String> = nets.iter().map(|(n, _)| n.clone()).filter(|n| self.meander_nets.contains(n)).collect();
            if ui.add_enabled(picked.len() >= 2, egui::Button::new("選んだネットを一番長いものに合わせる")).on_hover_text("選んだネットのうち、いちばん長いものに、ほかのネットの長さをそろえます。短いネットの、まっすぐな長い配線に蛇行を入れます。ルール違反になる所には入れません。元に戻すで取り消せます").clicked() {
                self.match_lengths(&picked, None);
            }
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut self.meander_target).speed(0.5).range(0.0..=1000.0).prefix("目標 ").suffix(" mm"));
                if ui.add_enabled(!picked.is_empty() && self.meander_target > 0.0, egui::Button::new("指定の長さに合わせる")).on_hover_text("選んだネットすべてを、指定した長さにします (いちばん長いネットがそれより長いときは、いちばん長いものに合わせます)").clicked() {
                    self.match_lengths(&picked, Some(self.meander_target));
                }
            });
        });
    }

    pub(super) fn match_lengths(&mut self, nets: &[String], target: Option<f64>) {
        self.checkpoint();
        let report = meander::match_nets(&mut self.board, nets, target, &self.meander);
        let ok = report.nets.iter().filter(|(_, _, r)| r.as_ref().is_ok_and(|a| *a > 0.0)).count();
        let failed: Vec<String> = report.nets.iter().filter_map(|(n, _, r)| r.as_ref().err().map(|e| format!("{n}: {e}"))).collect();
        self.violations.clear();
        if ok == 0 && failed.is_empty() {
            self.undo.pop(); // nothing changed: no undo step
            self.status = format!("長さはもうそろっています (目標 {:.2} mm)", report.target);
        } else if ok == 0 {
            self.undo.pop();
            self.status = format!("長さを合わせられませんでした。{}", failed.join(" / "));
        } else {
            let tail = if failed.is_empty() { String::new() } else { format!("。できなかったもの: {}", failed.join(" / ")) };
            self.status = format!("長さ合わせ: 目標 {:.2} mm に、{ok} 本のネットを蛇行で合わせました{tail}", report.target);
        }
    }
}
