//! The number of copper layers of the board (2, 4, 6 ...).

use super::*;
use crate::icons::fold_header;

impl BoardEditor {
    pub(super) fn layer_count_ui(&mut self, ui: &mut egui::Ui) {
        let total = self.board.inner_layers as usize + 2;
        ui.horizontal(|ui| {
            ui.label(i18n::tr("銅箔の層数")).on_hover_text(i18n::tr("基板の銅の層の数。2層は表と裏だけ。4層以上は、間に内層が入り、電源やGNDのベタを内層に広げたり、配線の逃げ場にできます。層が増えるほど発注の値段も上がります"));
            let mut pick = total;
            egui::ComboBox::from_id_salt("layer_count").selected_text(i18n::trf!("{total} 層")).show_ui(ui, |ui| {
                for n in [2usize, 4, 6, 8] {
                    ui.selectable_value(&mut pick, n, i18n::trf!("{n} 層"));
                }
            });
            if pick != total {
                self.set_layer_count(pick);
            }
        });
        if total > 2 {
            ui.small(i18n::tr("内層には配線とベタを置けます。スルーホールの部品とビアは、全部の層につながります (ブラインドビアは未対応)"));
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
    /// What is hidden or locked while the box is folded ("2 個を隠し中・部品をロック中"): so a part that will not pick or a
    /// track that is gone is explained without opening the box. The lock on the other side is the usual state and is left out.
    fn view_summary(&self) -> Option<String> {
        let v = self.vis;
        let sheets = !self.board.part_sheets.is_empty();
        let req = pcb_core::requirements::any(&self.board);
        let layers_hidden = self.board.copper_layers().into_iter().filter(|&l| !self.side_visible(l)).count();
        let hidden = [v.parts, v.tracks, v.vias, v.zones, v.silk, v.airwires, v.nets, v.keepouts, v.dimensions, v.drc_marks, v.ruler].iter().filter(|x| !**x).count()
            + usize::from(sheets && !self.show_sheets)
            + usize::from(req && !self.show_req_marks)
            + usize::from(req && !self.show_req_lines)
            + layers_hidden;
        let p = self.pick;
        let locked: Vec<String> = [(p.parts, "部品"), (p.pads, "パッド・穴"), (p.tracks, "配線"), (p.vias, "ビア"), (p.zones, "ゾーン"), (p.silk, "シルク")]
            .iter()
            .filter(|(on, _)| !*on)
            .map(|(_, n)| i18n::tr_dyn(n))
            .collect();
        let mut out = vec![];
        if hidden > 0 {
            out.push(i18n::trf!("{hidden} 個を隠し中"));
        }
        if !locked.is_empty() {
            out.push(i18n::trf!("{}をロック中", locked.join(i18n::tr("・"))));
        }
        (!out.is_empty()).then(|| out.join(i18n::tr("・")))
    }

    /// One row per copper layer: an eye, the layer's colour and its name (a click works on that layer).
    fn layer_rows(&mut self, ui: &mut egui::Ui) {
        use crate::icons::{icon_switch, Icon};
        for l in self.board.copper_layers() {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let mut shown = self.side_visible(l);
                if icon_switch(ui, Icon::Eye, Icon::EyeOff, &mut shown, "この層の銅を表示 / 非表示にします").changed() {
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
                if ui.selectable_value(&mut self.side, l, l.label()).on_hover_text(i18n::tr("配線を置く層をこの層にします (L キーでも順に切り替えられます)")).clicked() {
                    // working on a layer shows it
                    match l {
                        Side::Top => self.vis.top = true,
                        Side::Bottom => self.vis.bottom = true,
                        Side::Inner(n) => self.vis.inner_hidden &= !(1u32 << n.min(31)),
                    }
                }
            });
        }
    }

    /// The things on the board as a table (an eye and a lock per kind), then the helpers (unrouted lines, net names ...) as a
    /// block of picture switches, four to a row, with their names in the hover.
    fn things_rows(&mut self, ui: &mut egui::Ui) {
        use crate::icons::{icon_label, icon_switch, Icon};
        let head = |ui: &mut egui::Ui, text: &str| {
            ui.label(egui::RichText::new(i18n::tr_dyn(text)).small().color(Color32::from_rgb(85, 100, 115)));
        };
        let before = self.pick;
        head(ui, "基板の物");
        egui::Grid::new("view_lock_grid").spacing(Vec2::new(4.0, 2.0)).show(ui, |ui| {
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
                    ui.label(i18n::tr_dyn(name));
                });
            };
            // the switches for all the rows of the table (the lock on the other side is apart)
            let v = self.vis;
            let mut all_shown = v.parts && v.tracks && v.vias && v.zones && v.silk;
            if icon_switch(ui, Icon::Eye, Icon::EyeOff, &mut all_shown, "下の 5 種類 (部品・配線・ビア・ゾーン・シルク) を、まとめて出す / 隠す。全部が出ているときだけ明るくなります").changed() {
                let a = all_shown;
                (self.vis.tracks, self.vis.vias, self.vis.zones, self.vis.silk, self.vis.parts) = (a, a, a, a, a);
            }
            ui.label(i18n::tr("すべて"));
            let p = self.pick;
            let mut all_locked = !(p.parts || p.pads || p.tracks || p.vias || p.zones || p.silk);
            if icon_switch(ui, Icon::Lock, Icon::Unlock, &mut all_locked, "すべての種類を、まとめてロック / 解除します (「反対の面」は別です)。ロックすると、クリックや範囲選択で選べず、ドラッグでも動きません。部品をロックすると、ビアや配線を囲んで選ぶときに部品を巻き込みません").changed() {
                (self.pick.parts, self.pick.pads, self.pick.tracks) = (!all_locked, !all_locked, !all_locked);
                (self.pick.vias, self.pick.zones, self.pick.silk) = (!all_locked, !all_locked, !all_locked);
            }
            ui.end_row();
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
        // the helpers: picture switches (lit = shown), the name first in the hover
        head(ui, "補助の表示");
        let sheets = !self.board.part_sheets.is_empty();
        let req = pcb_core::requirements::any(&self.board);
        let mut items: Vec<(Icon, &str, &str, &mut bool)> = vec![
            (Icon::Airwire, "未配線", "まだつながっていない接続を示す黄色の線を表示します", &mut self.vis.airwires),
            (Icon::Label, "ネット名", "パッドのそばに、回路図でのつながりの名前 (ネット名) を表示します", &mut self.vis.nets),
            (Icon::Keepout, "禁止領域", "禁止領域 (斜線の範囲) を表示します。隠しても領域は効いたままで、DRC は中の配線・ビア・部品を指摘し、自動配線とベタは領域をよけます", &mut self.vis.keepouts),
            (Icon::Dimension, "寸法線", "基板に残した寸法線を表示します。隠しても、DXF と SVG の図には出ます", &mut self.vis.dimensions),
            (Icon::DrcMark, "点検の印", "作業中の点検で見つかったルール違反の印 (赤い ×) を表示します。隠しても点検は続き、左下に件数が出ます。点検そのものを止めるのは「③ 確認する」の「作業中も点検する」です", &mut self.vis.drc_marks),
            (Icon::Ruler, "定規 (mm)", "画面の上と左に、実際の寸法 (mm) の目盛りを出します。オレンジの印がカーソルの位置です", &mut self.vis.ruler),
        ];
        if sheets {
            items.push((Icon::Sheet, "シートの色", "回路図のシート(タブ)ごとに、部品のまわりを色で囲み、グループの角にシート名を出します。どの部品がどのシートのものか、基板の上で見分けられます。シートの一覧は、左のパネルにあります", &mut self.show_sheets));
        }
        if req {
            items.push((Icon::Frame, "要件の組 (枠)", "回路図の「設計の要件」の、近くに置く組を、枠で囲んで表示します (守れていれば緑、広がりすぎならピンク)", &mut self.show_req_marks));
            items.push((Icon::Gap, "要件の距離 (線)", "回路図の「設計の要件」の、離す組を、いちばん近い2つのパッドを結ぶ線と距離で表示します (足りなければピンク)", &mut self.show_req_lines));
        }
        for row in items.chunks_mut(4) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for (icon, name, tip, on) in row.iter_mut() {
                    let tip = format!("{}\n{}", i18n::tr_dyn(name), i18n::tr_dyn(tip));
                    icon_switch(ui, *icon, *icon, on, &tip);
                }
            });
        }
    }

    /// The box floating over the canvas (top right): the copper layers (eye, colour, a click works on the layer), then what to show
    /// and what can be picked. Folded at first, to one line with the working layer and what is hidden or locked.
    pub(super) fn layer_overlay(&mut self, ui: &egui::Ui, rect: Rect) {
        let pos = rect.right_top() + Vec2::new(-14.0, 30.0);
        let id = egui::Id::new("board_view_box_fold");
        egui::Area::new(egui::Id::new("layer_overlay")).order(egui::Order::Foreground).pivot(egui::Align2::RIGHT_TOP).fixed_pos(pos).show(ui.ctx(), |ui| {
            egui::Frame::NONE.fill(Color32::from_rgba_unmultiplied(238, 242, 246, 150)).stroke(Stroke::new(1.0, Color32::from_white_alpha(70))).corner_radius(6.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                // dark text on the whitish glass
                ui.visuals_mut().override_text_color = Some(Color32::from_rgb(25, 35, 45));
                if std::mem::take(&mut self.open_view_box) {
                    ui.ctx().data_mut(|d| d.insert_persisted(id, true));
                }
                let open = ui.ctx().data_mut(|d| *d.get_persisted_mut_or_insert_with(id, || false));
                let title = if open { i18n::tr("表示とロック").to_string() } else { i18n::trf!("表示とロック ・ 作業: {}", self.side.label()) };
                let summary = if open { None } else { self.view_summary() };
                let open = fold_header(ui, id, &title, false, "クリックでたたみます / ひらきます。上は銅の層 (目で表示、名前を押すとその層で作業。L キーでも切り替え)、下は物の種類ごとの目 (画面に出すか) と鍵 (選べないようにロックするか) です。明るい四角が「オン」(見えている / ロック中) です");
                // what is hidden or locked: on a line of its own under the title, within the title's width (beside the title it made the
                // folded box as much wider as the warning was long)
                if let Some(s) = summary {
                    let w = ui.min_rect().width();
                    ui.scope(|ui| {
                        ui.set_max_width(w);
                        ui.add(egui::Label::new(egui::RichText::new(s).small().color(Color32::from_rgb(170, 90, 0))).wrap())
                            .on_hover_text(i18n::tr("隠している物やロックしている物があります。開くと、どれかが分かります"));
                    });
                }
                if open {
                    let w = ui.scope(|ui| {
                        self.layer_rows(ui);
                        ui.separator();
                        self.things_rows(ui);
                    }).response.rect.width();
                    crate::icons::fold_remember_width(ui, id, w);
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How many shapes the keep-outs, the dimensions and the marks of the live check draw.
    fn drawn(e: &BoardEditor) -> usize {
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |ui| {
            let painter = ui.painter().clone();
            e.draw_keepouts(&painter, rect);
            e.draw_dimensions(&painter, rect);
            e.draw_live_drc(&painter, rect, None);
        });
        out.textures_delta.clear();
        out.shapes.len()
    }

    #[test]
    fn the_folded_box_says_what_is_hidden_or_locked() {
        let mut e = BoardEditor::default();
        e.board = pcb_core::Board::new("t", 40.0, 30.0);
        // as it starts (the other side locked is the usual state): nothing to say
        assert_eq!(e.view_summary(), None);
        e.vis.parts = false;
        e.vis.bottom = false;
        e.pick.tracks = false;
        let s = e.view_summary().unwrap();
        assert!(s.contains("2 個を隠し中") && s.contains("配線をロック中"), "{s}");
    }

    /// The width of the folded box as drawn.
    fn folded_width(e: &mut BoardEditor) -> f32 {
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 800.0));
        let mut w = 0.0;
        for _ in 0..3 {
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |ui| e.layer_overlay(ui, rect));
            out.textures_delta.clear();
            w = ctx.memory(|m| m.area_rect(egui::Id::new("layer_overlay"))).map_or(0.0, |r| r.width());
        }
        w
    }

    #[test]
    fn the_folded_box_does_not_widen_for_what_is_hidden() {
        let mut e = BoardEditor::default();
        e.board = pcb_core::Board::new("t", 40.0, 30.0);
        let plain = folded_width(&mut e);
        assert!(plain > 50.0, "{plain}");
        e.vis.parts = false;
        e.vis.bottom = false;
        e.pick.tracks = false;
        e.pick.vias = false;
        assert!(e.view_summary().is_some());
        let with = folded_width(&mut e);
        assert!((with - plain).abs() < 1.0, "the warning goes under the title, in its width: {plain} -> {with}");
    }

    #[test]
    fn the_new_eyes_hide_keepouts_dimensions_and_the_marks_but_the_count_stays() {
        let mut e = BoardEditor::default();
        e.board = pcb_core::Board::new("t", 40.0, 30.0);
        e.fit_view(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0)));
        e.board.keepouts.push(pcb_core::keepout::Keepout::new(vec![Point::new(10.0, 10.0), Point::new(20.0, 10.0), Point::new(20.0, 20.0), Point::new(10.0, 20.0)]));
        e.board.tracks.push(pcb_core::Track { side: Side::Top, a: Point::new(2.0, 15.0), b: Point::new(38.0, 15.0), width: 0.25 });
        pcb_core::dimension::add_outline_dimensions(&mut e.board).unwrap();
        e.settle_live_drc();
        assert!(!e.live_findings().is_empty());
        let all = drawn(&e);
        for (name, f) in [
            ("keepouts", (|e: &mut BoardEditor| e.vis.keepouts = false) as fn(&mut BoardEditor)),
            ("dimensions", |e| e.vis.dimensions = false),
            ("marks", |e| e.vis.drc_marks = false),
        ] {
            let mut e2 = BoardEditor { board: e.board.clone(), ..Default::default() };
            (e2.zoom, e2.pan) = (e.zoom, e.pan);
            e2.settle_live_drc();
            f(&mut e2);
            assert!(drawn(&e2) < all, "{name}");
        }
        // with the marks hidden the check still runs, and the badge says where to show them again
        e.vis.drc_marks = false;
        assert!(!e.live_findings().is_empty());
        // the keep-out still holds for the DRC while hidden
        e.vis.keepouts = false;
        assert!(drc::check(&e.board).iter().any(|v| v.message.contains("禁止領域")));
    }
}
