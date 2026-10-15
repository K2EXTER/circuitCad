//! "配線を整える" section of the board panel: round the corners and step the width changes of the routed tracks.

use super::*;
use pcb_core::smooth::{self, SmoothReport};

impl BoardEditor {
    pub(super) fn panel_smooth(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label("配線を整える").on_hover_text("市販の基板のように、配線の角を丸め、太さが変わる所をなだらかにします。配線は短い線の集まりのまま増えるだけなので、DRC や製造データはそのまま使えます");
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.smooth.corners, "角を丸める").on_hover_text("同じ太さの配線がつながる角を、円弧 (短い線の集まり) にします。角が丸いと電流が流れやすく、高周波でも反射が減ります。直線に近い角や、短くて入らない角は小さくなるか、そのままです");
            ui.add_enabled(self.smooth.corners, egui::DragValue::new(&mut self.smooth.radius).speed(0.05).range(0.1..=5.0).prefix("半径 ").suffix(" mm"))
                .on_hover_text("角の丸みの半径。配線が短いと、入る大きさまで自動で小さくなります");
        });
        ui.checkbox(&mut self.smooth.tapers, "太さの段差をなだらかに").on_hover_text("太い配線と細い配線がつながる所と、配線がパッド・ビアに入る根元に、中間の太さの短い配線を挟みます (パッドより細い配線は、根元がパッドの幅から配線の太さへ徐々に細くなります)。電流が急に絞られず、配線の根元が割れにくくなります");
        if ui.add_enabled(self.smooth.corners || self.smooth.tapers, egui::Button::new("配線を整える")).on_hover_text("全部の配線に上の設定を適用します。パッドやビアに付いた端は動きません。ルール違反が増える所と、接続が切れる所は変えません。元に戻すで取り消せます").clicked() {
            self.smooth_tracks();
        }
    }

    pub fn smooth_tracks(&mut self) {
        self.checkpoint();
        let SmoothReport { corners, tapers } = smooth::smooth_tracks(&mut self.board, &self.smooth);
        self.violations.clear();
        self.status = if corners + tapers == 0 {
            self.undo.pop(); // nothing changed: no undo step
            "整えられる所がありませんでした (角がない、短くて入らない、またはルール違反になる所だけです)".into()
        } else {
            format!("角を {corners} か所丸め、太さの段差を {tapers} か所なだらかにしました")
        };
    }
}
