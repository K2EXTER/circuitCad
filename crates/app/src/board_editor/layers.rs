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

impl BoardEditor {
    /// The layer switcher floating over the canvas (top right): folds down to the name of the working layer,
    /// unfolds to one row per copper layer with an eye (show / hide) and a radio (work on this layer).
    pub(super) fn layer_overlay(&mut self, ui: &egui::Ui, rect: Rect) {
        let layers = self.board.copper_layers();
        let pos = rect.right_top() + Vec2::new(-14.0, 30.0);
        let id = egui::Id::new("layer_overlay_fold");
        egui::Area::new(egui::Id::new("layer_overlay")).order(egui::Order::Foreground).pivot(egui::Align2::RIGHT_TOP).fixed_pos(pos).show(ui.ctx(), |ui| {
            egui::Frame::NONE.fill(Color32::from_rgba_unmultiplied(238, 242, 246, 150)).stroke(Stroke::new(1.0, Color32::from_white_alpha(70))).corner_radius(6.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                // dark text on the whitish glass
                ui.visuals_mut().override_text_color = Some(Color32::from_rgb(25, 35, 45));
                let state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
                let title = if state.is_open() { "層".to_string() } else { format!("層: {}", self.side.label()) };
                state
                    .show_header(ui, |ui| {
                        ui.label(title).on_hover_text("三角でたたみます / ひらきます。L キーでも作業層を順に切り替えられます");
                    })
                    .body(|ui| {
                        for l in layers.iter().copied() {
                            ui.horizontal(|ui| {
                                let mut shown = self.side_visible(l);
                                if ui.checkbox(&mut shown, "").on_hover_text("この層の銅を表示 / 非表示にします").changed() {
                                    match l {
                                        Side::Top => self.vis.top = shown,
                                        Side::Bottom => self.vis.bottom = shown,
                                        Side::Inner(n) => {
                                            let bit = 1u32 << n.min(31);
                                            self.vis.inner_hidden = if shown { self.vis.inner_hidden & !bit } else { self.vis.inner_hidden | bit };
                                        }
                                    }
                                }
                                let (r, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                                ui.painter().rect_filled(r, 2.0, self.side_color(l));
                                if ui.selectable_value(&mut self.side, l, l.label()).on_hover_text("配線を置く層をこの層にします").clicked() {
                                    // working on a layer shows it
                                    match l {
                                        Side::Top => self.vis.top = true,
                                        Side::Bottom => self.vis.bottom = true,
                                        Side::Inner(n) => self.vis.inner_hidden &= !(1u32 << n.min(31)),
                                    }
                                }
                            });
                        }
                    });
            });
        });
    }
}
