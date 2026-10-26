//! The number of copper layers of the board (2, 4, 6 ...).

use super::*;
use crate::icons::fold_header;

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
    /// What to show and what can be picked, as a table (one row per kind of thing: an eye and a lock) on a glass strip over the canvas
    /// (under the layer switcher, same Area). Folds down to a small title.
    fn view_frame(&mut self, ui: &mut egui::Ui) -> f32 {
        use crate::icons::{icon_label, icon_switch, Icon};
        let id = egui::Id::new("view_overlay_fold");
        {
            egui::Frame::NONE.fill(Color32::from_rgba_unmultiplied(238, 242, 246, 150)).stroke(Stroke::new(1.0, Color32::from_white_alpha(70))).corner_radius(6.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                ui.visuals_mut().override_text_color = Some(Color32::from_rgb(25, 35, 45));
                if fold_header(ui, id, "物の表示とロック", true, "クリックでたたみます / ひらきます。種類ごとに、目は「画面に出すか」、鍵は「選べないようにロックするか」です。明るい四角が「オン」(見えている / ロック中)です") {
                    let w = ui.scope(|ui| {
                        let before = self.pick;
                        // (icon, name, tip, eye, lock): None where a kind has no such switch
                        egui::Grid::new("view_lock_grid").spacing(Vec2::new(4.0, 2.0)).show(ui, |ui| {
                            let v = self.vis;
                            let mut all_shown = v.parts && v.tracks && v.vias && v.zones && v.silk && v.airwires && v.nets && v.ruler;
                            if icon_switch(ui, Icon::Eye, Icon::EyeOff, &mut all_shown, "すべての表示を、まとめて出す / 隠す (下の目の一括操作)。いまは全部が出ているときだけ明るくなります").changed() {
                                (self.vis.tracks, self.vis.vias, self.vis.zones, self.vis.silk) = (all_shown, all_shown, all_shown, all_shown);
                                (self.vis.airwires, self.vis.nets, self.vis.ruler) = (all_shown, all_shown, all_shown);
                                self.vis.parts = all_shown;
                            }
                            ui.label("すべて");
                            let p = self.pick;
                            let mut all_locked = !(p.parts || p.pads || p.tracks || p.vias || p.zones || p.silk);
                            if icon_switch(ui, Icon::Lock, Icon::Unlock, &mut all_locked, "すべての種類を、まとめてロック / 解除します (「反対の面」は別です)。ロックすると、クリックや範囲選択で選べず、ドラッグでも動きません。部品をロックすると、ビアや配線を囲んで選ぶときに部品を巻き込みません").changed() {
                                (self.pick.parts, self.pick.pads, self.pick.tracks) = (!all_locked, !all_locked, !all_locked);
                                (self.pick.vias, self.pick.zones, self.pick.silk) = (!all_locked, !all_locked, !all_locked);
                            }
                            ui.end_row();
                            let lock = |ui: &mut egui::Ui, pick: &mut bool, tip: &str| {
                                let mut locked = !*pick;
                                if icon_switch(ui, Icon::Lock, Icon::Unlock, &mut locked, tip).changed() {
                                    *pick = !locked;
                                }
                            };
                            let eye = |ui: &mut egui::Ui, shown: &mut bool, tip: &str| {
                                icon_switch(ui, Icon::Eye, Icon::EyeOff, shown, tip);
                            };
                            let none = |ui: &mut egui::Ui| {
                                ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover());
                            };
                            let kind = |ui: &mut egui::Ui, icon: Icon, name: &str| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 2.0;
                                    icon_label(ui, icon, name);
                                    ui.label(name);
                                });
                            };
                            eye(ui, &mut self.vis.parts, "部品 (部品のパッド、外形、部品番号) を表示します。隠すと、部品の下の配線やベタが見やすくなります。隠している間も、配線やビアは、そのまま選べます");
                            kind(ui, Icon::Part, "部品");
                            lock(ui, &mut self.pick.parts, "部品をロックします。選べず、ドラッグでも動きません (範囲選択になります)");
                            ui.end_row();
                            none(ui);
                            kind(ui, Icon::PadTh, "パッド・穴");
                            lock(ui, &mut self.pick.pads, "部品に属さないパッドと取付穴をロックします");
                            ui.end_row();
                            eye(ui, &mut self.vis.tracks, "配線 (トラック) を表示します");
                            kind(ui, Icon::Wire, "配線");
                            lock(ui, &mut self.pick.tracks, "配線をロックします。選べず、動かせません");
                            ui.end_row();
                            eye(ui, &mut self.vis.vias, "ビアを表示します");
                            kind(ui, Icon::Via, "ビア");
                            lock(ui, &mut self.pick.vias, "ビアをロックします。選べず、動かせません");
                            ui.end_row();
                            eye(ui, &mut self.vis.zones, "ゾーン (銅を面で塗った所) を表示します");
                            kind(ui, Icon::Zone, "ゾーン");
                            lock(ui, &mut self.pick.zones, "ゾーン (ベタ) をロックします");
                            ui.end_row();
                            eye(ui, &mut self.vis.silk, "シルク (白い印刷の文字と枠) を表示します");
                            kind(ui, Icon::Silk, "シルク");
                            lock(ui, &mut self.pick.silk, "部品に属さないシルク線をロックします");
                            ui.end_row();
                            eye(ui, &mut self.vis.airwires, "未配線 (まだつながっていない接続を示す黄色の線) を表示します");
                            kind(ui, Icon::Airwire, "未配線");
                            none(ui);
                            ui.end_row();
                            eye(ui, &mut self.vis.nets, "パッドのそばに、回路図でのつながりの名前 (ネット名) を表示します");
                            kind(ui, Icon::Label, "ネット名");
                            none(ui);
                            ui.end_row();
                            eye(ui, &mut self.vis.ruler, "画面の上と左に、実際の寸法 (mm) の目盛りを出します。オレンジの印がカーソルの位置です");
                            kind(ui, Icon::Ruler, "定規 (mm)");
                            none(ui);
                            ui.end_row();
                            none(ui);
                            kind(ui, Icon::Layers, "反対の面");
                            icon_switch(ui, Icon::Lock, Icon::Unlock, &mut self.pick.active_side_only, "いま作業している面 (枠の色) と反対の面にある物をロックします。薄く表示している反対の面を誤って選ばなくなります。ビアと、貫通するスルーホール部品は、どちらの面でも選べます");
                            ui.end_row();
                        });
                        if before.parts != self.pick.parts || before.tracks != self.pick.tracks || before.vias != self.pick.vias || before.pads != self.pick.pads || before.zones != self.pick.zones || before.silk != self.pick.silk {
                            self.multi.retain(|it| self.pick.allows(it));
                            if self.selected.is_some_and(|it| !self.pick.allows(&it)) {
                                self.selected = None;
                            }
                        }
                    }).response.rect.width();
                    crate::icons::fold_remember_width(ui, id, w);
                }
            })
            .response
            .rect
            .width()
        }
    }

    /// The layer switcher floating over the canvas (top right): folds down to the name of the working layer,
    /// unfolds to one row per copper layer with an eye (show / hide) and a radio (work on this layer).
    pub(super) fn layer_overlay(&mut self, ui: &egui::Ui, rect: Rect) {
        let layers = self.board.copper_layers();
        let pos = rect.right_top() + Vec2::new(-14.0, 30.0);
        let id = egui::Id::new("layer_overlay_fold");
        egui::Area::new(egui::Id::new("layer_overlay")).order(egui::Order::Foreground).pivot(egui::Align2::RIGHT_TOP).fixed_pos(pos).show(ui.ctx(), |ui| ui.vertical(|ui| {
            // Both panels hug the right edge. The Area must be exactly as wide as the wider panel: a right-aligned
            // layout would make it as wide as the free space, and that empty strip would take the canvas's clicks.
            let (wl_id, wv_id) = (egui::Id::new("layer_overlay_w"), egui::Id::new("view_overlay_w"));
            let (wl, wv) = ui.ctx().data_mut(|d| (d.get_temp::<f32>(wl_id).unwrap_or(0.0), d.get_temp::<f32>(wv_id).unwrap_or(0.0)));
            let widest = wl.max(wv);
            let gap = ui.spacing().item_spacing.x;
            let drawn_l = ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space((widest - wl).max(0.0));
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    egui::Frame::NONE.fill(Color32::from_rgba_unmultiplied(238, 242, 246, 150)).stroke(Stroke::new(1.0, Color32::from_white_alpha(70))).corner_radius(6.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                // dark text on the whitish glass
                ui.visuals_mut().override_text_color = Some(Color32::from_rgb(25, 35, 45));
                let open = ui.ctx().data_mut(|d| *d.get_persisted_mut_or_insert_with(id, || false));
                let title = if open { "層".to_string() } else { format!("層: {}", self.side.label()) };
                if fold_header(ui, id, &title, false, "クリックでたたみます / ひらきます。L キーでも作業層を順に切り替えられます") {
                    let w = ui.scope(|ui| {
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
                    }).response.rect.width();
                    crate::icons::fold_remember_width(ui, id, w);
                }
            }).response.rect.width()
                }).inner
            }).inner;
            let drawn_v = ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space((widest - wv).max(0.0));
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    self.view_frame(ui)
                }).inner
            }).inner;
            ui.ctx().data_mut(|d| {
                d.insert_temp(wl_id, drawn_l);
                d.insert_temp(wv_id, drawn_v);
            });
            if (drawn_l - wl).abs() > 0.5 || (drawn_v - wv).abs() > 0.5 {
                ui.ctx().request_repaint(); // the padding follows the new widths
            }
        }));
    }
}
