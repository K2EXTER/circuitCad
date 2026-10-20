//! The align buttons shown while several parts are selected (計算は `pcb_core::align`).

use super::*;
use pcb_core::align::{align_parts, Align};

impl BoardEditor {
    /// Eight buttons: edges, centres and equal gaps of the selected parts. Shown under the "N 個を選択中" line.
    pub(super) fn panel_align(&mut self, ui: &mut egui::Ui) {
        let parts: Vec<String> = self.multi.iter().filter_map(|it| if let Item::Part(i) = it { self.board.parts.get(*i).map(|p| p.refdes.clone()) } else { None }).collect();
        if parts.len() < 2 {
            return;
        }
        ui.label("整列 (?)").on_hover_text("選んだ部品の位置をそろえます。端や中心は、パッドを囲んだ四角が基準です。固定した部品は動きません。配線は動かないので、配線のあとに使うと、つながりが切れることがあります (元に戻すで戻せます)");
        let buttons: [(Align, &str, &str); 8] = [
            (Align::Left, "左揃え", "いちばん左の部品に、左端をそろえます"),
            (Align::Right, "右揃え", "いちばん右の部品に、右端をそろえます"),
            (Align::Top, "上揃え", "いちばん上の部品に、上端をそろえます"),
            (Align::Bottom, "下揃え", "いちばん下の部品に、下端をそろえます"),
            (Align::CenterX, "縦の線に中心", "全体の真ん中の縦の線に、部品の中心をそろえます (縦に並べたいとき)"),
            (Align::CenterY, "横の線に中心", "全体の真ん中の横の線に、部品の中心をそろえます (横に1列に並べたいとき)"),
            (Align::SpaceX, "横に等間隔", "いちばん左と右の部品は動かさず、間の部品の間隔を同じにします (3 個以上)"),
            (Align::SpaceY, "縦に等間隔", "いちばん上と下の部品は動かさず、間の部品の間隔を同じにします (3 個以上)"),
        ];
        ui.horizontal_wrapped(|ui| {
            for (how, label, tip) in buttons {
                if ui.button(label).on_hover_text(tip).clicked() {
                    self.align_selected(&parts, how);
                }
            }
        });
    }

    fn align_selected(&mut self, parts: &[String], how: Align) {
        self.checkpoint();
        match align_parts(&mut self.board, parts, how) {
            Ok(r) if r.moved > 0 => {
                self.violations.clear();
                let locked = if r.locked > 0 { format!("固定した {} 個は動かしていません。", r.locked) } else { String::new() };
                let tracks = if self.board.tracks.is_empty() { String::new() } else { "配線は動いていないので、DRC で確認しましょう。".into() };
                self.status = format!("整列: {} 個の部品を動かしました。{locked}{tracks}", r.moved);
            }
            Ok(_) => {
                self.undo.pop();
                self.status = "もうそろっています (動かす部品がありませんでした)".into();
            }
            Err(e) => {
                self.undo.pop();
                self.status = e;
            }
        }
    }
}
