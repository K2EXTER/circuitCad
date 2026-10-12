//! The number of copper layers of the board (2, 4, 6 ...).

use super::*;

impl BoardEditor {
    pub(super) fn layer_count_ui(&mut self, ui: &mut egui::Ui) {
        let total = self.board.inner_layers as usize + 2;
        ui.horizontal(|ui| {
            ui.label("銅箔の層数").on_hover_text("基板の銅の層の数。2層は表と裏だけ。4層以上は、間に内層が入り、電源やGNDのベタを内層に広げたり、配線の逃げ場にできます。層が増えるほど発注の値段も上がります");
            let mut pick = total;
            egui::ComboBox::from_id_salt("layer_count").selected_text(format!("{total} 層")).show_ui(ui, |ui| {
                for n in [2usize, 4, 6, 8] {
                    ui.selectable_value(&mut pick, n, format!("{n} 層"));
                }
            });
            if pick != total {
                self.set_layer_count(pick);
            }
        });
        if total > 2 {
            ui.small("内層には配線とベタを置けます。スルーホールの部品とビアは、全部の層につながります (ブラインドビアは未対応)");
        }
    }

    /// Change the number of copper layers. Refuses (and says why) when copper would be lost.
    pub(super) fn set_layer_count(&mut self, total: usize) {
        let inner = (total.max(2) - 2).min(250) as u8;
        let used = |s: Side| matches!(s, Side::Inner(n) if n > inner);
        let n_tracks = self.board.tracks.iter().filter(|t| used(t.side)).count();
        let n_zones = self.board.zones.iter().filter(|z| used(z.side)).count();
        if n_tracks + n_zones > 0 {
            self.status = format!("{total} 層にすると消える内層に、配線 {n_tracks} 本とベタ {n_zones} 個があります。先に消すか、ほかの層へ移してください");
            return;
        }
        self.checkpoint();
        self.board.inner_layers = inner;
        if used(self.side) {
            self.side = Side::Top;
        }
        self.status = format!("銅箔を {total} 層にしました");
    }
}
