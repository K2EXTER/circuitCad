//! The align buttons shown while several parts are selected (計算は `pcb_core::align`).

use super::*;
use pcb_core::align::{align_parts, Align};

/// An align button: what it does, its icon, its name and its hover text.
type AlignButton = (Align, crate::icons::Icon, &'static str, &'static str);

impl BoardEditor {
    /// Eight buttons: edges, centres and equal gaps of the selected parts. Shown under the "N 個を選択中" line.
    pub(super) fn panel_align(&mut self, ui: &mut egui::Ui) {
        let parts: Vec<String> = self.multi.iter().filter_map(|it| if let Item::Part(i) = it { self.board.parts.get(*i).map(|p| p.refdes.clone()) } else { None }).collect();
        if parts.len() < 2 {
            return;
        }
        ui.label(i18n::tr("整列 (?)")).on_hover_text(i18n::tr("選んだ部品の位置をそろえます。端や中心は、パッドを囲んだ四角が基準です。固定した部品は動きません。配線は動かないので、配線のあとに使うと、つながりが切れることがあります (元に戻すで戻せます)"));
        // edges and centres on one row, gaps and lines on the next (icons, the name and what it does on hover); the order of the
        // pins they are wired to with its name too (it has no picture people know from drawing programs)
        use crate::icons::{icon_square, Icon};
        let rows: [(&str, &[AlignButton]); 2] = [
            (
                "揃える",
                &[
                    (Align::Left, Icon::AlignLeft, "左揃え", "いちばん左の部品に、左端をそろえます"),
                    (Align::CenterX, Icon::AlignCenterX, "縦の線に中心", "全体の真ん中の縦の線に、部品の中心をそろえます (縦に並べたいとき)"),
                    (Align::Right, Icon::AlignRight, "右揃え", "いちばん右の部品に、右端をそろえます"),
                    (Align::Top, Icon::AlignTop, "上揃え", "いちばん上の部品に、上端をそろえます"),
                    (Align::CenterY, Icon::AlignCenterY, "横の線に中心", "全体の真ん中の横の線に、部品の中心をそろえます (横に1列に並べたいとき)"),
                    (Align::Bottom, Icon::AlignBottom, "下揃え", "いちばん下の部品に、下端をそろえます"),
                ],
            ),
            (
                "並べる",
                &[
                    (Align::SpaceX, Icon::SpaceX, "横に等間隔", "いちばん左と右の部品は動かさず、間の部品の間隔を同じにします (3 個以上)"),
                    (Align::SpaceY, Icon::SpaceY, "縦に等間隔", "いちばん上と下の部品は動かさず、間の部品の間隔を同じにします (3 個以上)"),
                    (Align::RowX, Icon::PackX, "横に詰めて並べる", "今の左からの順で、いちばん左の部品から右へ、1 mm ずつ空けて横一列に並べます (中心は最初の部品の横の線に)"),
                    (Align::RowY, Icon::PackY, "縦に詰めて並べる", "今の上からの順で、いちばん上の部品から下へ、1 mm ずつ空けて縦一列に並べます (中心は最初の部品の縦の線に)"),
                ],
            ),
        ];
        let mut chosen: Option<Align> = None;
        for (i, (name, buttons)) in rows.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(i18n::tr(name));
                for (k, (how, icon, label, tip)) in buttons.iter().enumerate() {
                    // a little room between the three of the vertical line and the three of the horizontal one
                    if i == 0 && k == 3 {
                        ui.add_space(6.0);
                    }
                    if icon_square(ui, *icon, label, true, tip).clicked() {
                        chosen = Some(*how);
                    }
                }
                if i == 1 && crate::icons::icon_button_key(ui, Icon::NetOrder, "つながる順に並べる", "", false, true, "つながる相手の IC やコネクタのピンの並びの順に、1 mm ずつ空けて一列に並べます。コネクタのピンの順に直列抵抗を、ドライバの出力の順に LED の抵抗を、のように使うと、配線が交わりません。列の真ん中は相手のピンの真ん中の向かいに、外形の中に置きます (相手のピンが横に並んでいれば横一列、縦なら縦一列)").clicked() {
                    chosen = Some(Align::NetOrder);
                }
            });
        }
        if let Some(how) = chosen {
            self.align_selected(&parts, how);
        }
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
