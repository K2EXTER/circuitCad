//! The left side panel, the selection panel and the context hint.

use super::*;

impl BoardEditor {
    /// A tip for what to do next on the board.
    pub fn hint(&self) -> String {
        if self.board.parts.is_empty() {
            return "まず回路図タブで回路を作り、ここで「回路図から取り込み」を押すと、部品が基板に読み込まれます".into();
        }
        if self.board.outline.len() >= 3 && self.board.tracks.is_empty() && self.board.parts.iter().any(|p| self.board.pads.iter().any(|d| d.refdes == p.refdes && !inside_polygon(&self.board.outline, d.pos))) {
            return "部品が基板の外にあります。左の「部品を並べて外形を決める」で自動的に並べるか、部品をドラッグして基板の中に置きましょう".into();
        }
        if self.board.bounds().is_none() || self.board.outline.is_empty() {
            return "基板の外形を決めましょう。左の「矩形に設定」か、ツールの「基板外形」で描きます。発注にはこの外形が必要です".into();
        }
        let unrouted = ratsnest(&self.board).len();
        if unrouted > 0 {
            return format!("黄色の線 (未配線 {unrouted} 本) が残っています。「配線」ツールで手でつなぐか、左の「自動配線」に任せましょう。広い GND は「銅箔ゾーン」にすると簡単です");
        }
        if self.violations.is_empty() {
            return "配線が終わりました。「DRC を実行」でルール違反がないか確認し、問題なければ「発注の準備」で最終チェックをして、発注用データ (ガーバー) を作れます".into();
        }
        "DRC の指摘は、クリックするとその場所に飛びます。直したらもう一度 DRC を実行しましょう。0 件になれば発注データを書き出せます".into()
    }

    pub fn side_panel(&mut self, ui: &mut egui::Ui, sch: &SchEditor) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("基板パターン");
            for (t, name) in TOOLS {
                if ui.selectable_label(self.tool == *t, *name).on_hover_text(tool_tip(*t)).clicked() {
                    self.tool = *t;
                    self.pending = None;
                    self.poly.clear();
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("作業層 (L)").on_hover_text("いま配線や部品を置く面。基板は表と裏の2層に銅を張れます");
                ui.selectable_value(&mut self.side, Side::Top, "表面");
                ui.selectable_value(&mut self.side, Side::Bottom, "裏面");
            });
            ui.horizontal(|ui| {
                ui.label("配線");
                ui.selectable_value(&mut self.route_mode, RouteMode::Angle45, "45°");
                ui.selectable_value(&mut self.route_mode, RouteMode::Orthogonal, "直角");
                ui.selectable_value(&mut self.route_mode, RouteMode::Free, "自由");
            });
            ui.small("配線中: V = ビアで層を切替 / Space = 曲がり方を反転");
            ui.add(egui::DragValue::new(&mut self.grid).speed(0.05).range(0.0..=5.0).prefix("グリッド ").suffix(" mm"));
            ui.add(egui::DragValue::new(&mut self.track_width).speed(0.01).range(0.05..=5.0).prefix("線幅 ").suffix(" mm"))
                .on_hover_text("配線の太さ。大きい電流を流す線は太く。発注先の最小線幅 (多くは 0.15〜0.2 mm) を下回らないように");
            ui.add(egui::DragValue::new(&mut self.via_dia).speed(0.05).range(0.3..=3.0).prefix("ビア径 ").suffix(" mm"));
            ui.add(egui::DragValue::new(&mut self.via_drill).speed(0.05).range(0.2..=2.0).prefix("ビア穴 ").suffix(" mm"))
                .on_hover_text("ビアの穴径。小さいほど基板が高くなることがあります。発注先の最小穴径を確認しましょう");
            if matches!(self.tool, Tool::PadTh | Tool::PadSmd) {
                ui.add(egui::DragValue::new(&mut self.pad_size).speed(0.05).range(0.3..=10.0).prefix("パッド径 ").suffix(" mm"));
                ui.add(egui::DragValue::new(&mut self.pad_drill).speed(0.05).range(0.2..=6.0).prefix("パッド穴 ").suffix(" mm"));
            }
            if self.tool == Tool::Hole {
                ui.add(egui::DragValue::new(&mut self.hole_drill).speed(0.1).range(0.5..=10.0).prefix("取付穴 ").suffix(" mm"));
            }

            ui.separator();
            ui.label("表示");
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut self.vis.top, "表面");
                ui.checkbox(&mut self.vis.bottom, "裏面");
                ui.checkbox(&mut self.vis.silk, "シルク");
                ui.checkbox(&mut self.vis.zones, "ゾーン");
                ui.checkbox(&mut self.vis.airwires, "未配線");
                ui.checkbox(&mut self.vis.nets, "ネット名");
                ui.checkbox(&mut self.vis.ruler, "定規 (mm)").on_hover_text("画面の上と左に、実際の寸法 (mm) の目盛りを出します。拡大すると目盛りが細かくなります。オレンジの印がカーソルの位置です");
            });

            ui.separator();
            ui.label("基板外形");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut self.size_w).speed(0.5).range(5.0..=500.0).prefix("幅 "));
                ui.add(egui::DragValue::new(&mut self.size_h).speed(0.5).range(5.0..=500.0).prefix("高さ "));
                if ui.button("矩形に設定").on_hover_text("上の幅と高さで基板の外形を作ります。発注するときの基板サイズになります。右の「角R」が 0 より大きいと、角が丸くなります").clicked() {
                    self.checkpoint();
                    let (lo, _) = self.board.bounds().unwrap_or((Point::default(), Point::default()));
                    self.set_outline_box(lo.x, lo.y, lo.x + self.size_w, lo.y + self.size_h);
                }
            });
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut self.size_r).speed(0.1).range(0.0..=100.0).prefix("角R ").suffix(" mm"))
                    .on_hover_text("角の丸み (半径)。0 なら角ばった形です。ケースに入れる基板や、手で持つ基板は、角を丸めると角が欠けにくく、安全です。大きすぎる値は、入る大きさまで自動で小さくなります");
                if ui.button("円・長円に設定").on_hover_text("上の幅と高さに収まる円 (幅=高さ) や長円の外形にします。丸い基板やボタン電池サイズの基板に").clicked() {
                    self.checkpoint();
                    let (lo, _) = self.board.bounds().unwrap_or((Point::default(), Point::default()));
                    self.board.outline = pcb_core::outline::ellipse(lo.x, lo.y, lo.x + self.size_w, lo.y + self.size_h);
                    self.status = "基板の外形を円・長円にしました".into();
                }
            });
            if ui.button("いまの外形の角を丸める").on_hover_text("いまの基板の外形 (矩形でも、多角形でも) のすべての角を、上の「角R」の半径で丸めます。辺が短くて入らない角は、入る大きさまで小さくなります (元に戻すで取り消せます)").clicked() {
                self.round_outline(self.size_r);
            }
            if let Some((lo, hi)) = self.board.bounds() {
                ui.small(format!("現在 {:.1} × {:.1} mm ({}点)", hi.x - lo.x, hi.y - lo.y, self.board.outline.len()));
            }

            if self.tool == Tool::Zone {
                ui.separator();
                ui.label("ゾーンの設定 (多角形を描き、ダブルクリックか Enter で確定)");
                let nets = self.nets();
                egui::ComboBox::from_label("ネット").selected_text(self.zone_net.clone()).show_ui(ui, |ui| {
                    for n in &nets {
                        ui.selectable_value(&mut self.zone_net, n.clone(), n);
                    }
                    if !nets.iter().any(|n| n == "GND") {
                        ui.selectable_value(&mut self.zone_net, "GND".to_string(), "GND");
                    }
                });
                ui.add(egui::DragValue::new(&mut self.zone_clearance).speed(0.05).range(0.1..=2.0).prefix("隙間 ").suffix(" mm"));
                ui.checkbox(&mut self.zone_thermal, "スルーホールはサーマルリリーフ");
            }

            self.selection_panel(ui);

            ui.separator();
            if ui.button("回路図から取り込み").on_hover_text("回路図の部品とつながり (ネット) を基板に読み込みます。部品が並び、つなぐべき所に黄色の線 (未配線) が出ます。回路図を直したらもう一度押します").clicked() {
                self.import_from_schematic(sch);
            }
            for n in &self.import_notes {
                ui.colored_label(Color32::from_rgb(255, 180, 60), n);
            }
            ui.label(format!("未配線: {} 本 (黄色の線)", ratsnest(&self.board).len()));
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("元に戻す")).clicked() {
                    self.undo();
                }
                if ui.add_enabled(!self.redo.is_empty(), egui::Button::new("やり直し")).clicked() {
                    self.redo();
                }
            });
            ui.label("基板名 (出力ファイル名)");
            ui.text_edit_singleline(&mut self.board.name);

            ui.separator();
            if ui.button("3Dで見る…").on_hover_text("基板を立体で見ます。出来上がりの色 (緑のレジスト・金色のパッド・白いシルク) と、部品の高さの目安が分かります。ドラッグで回せます").clicked() {
                self.open_view3d();
            }
            if ui.button("発注の準備…").on_hover_text("発注先を選び、チェックと出来上がりの確認をして、製造ファイルを書き出します。発注までの手順も表示します").clicked() {
                self.open_order();
            }
            ui.separator();
            ui.label("自動配置");
            if ui.button("部品を並べて外形を決める").on_hover_text("すべての部品を、つながりの強いものが近くなるように並べ、基板の外形をそれに合わせます。最初の叩き台を作る機能なので、並べたあとに手で動かして整えてください。配線済みの基板では使えません (部品を動かすと配線が合わなくなるため)").clicked() {
                self.autoplace();
            }
            ui.separator();
            ui.label("自動配線");
            ui.horizontal(|ui| {
                if ui.button("全部").on_hover_text("未配線 (黄色の線) をすべて自動で配線します。先に部品を基板の上に並べておいてください。
線幅・ビア径は下の設定が使われ、通せなかった接続は黄色の線のまま残ります。気に入らなければ「元に戻す」で取り消せます").clicked() {
                    self.autoroute(None);
                }
                let sel_net = match self.selected {
                    Some(Item::Pad(i)) => self.board.pads.get(i).map(|p| p.net.clone()).filter(|n| !n.is_empty()),
                    _ => None,
                };
                let label = sel_net.as_ref().map_or("選択したネットだけ".to_string(), |n| format!("{n} だけ"));
                if ui.add_enabled(sel_net.is_some(), egui::Button::new(label)).on_hover_text("パッドを選ぶと、そのパッドのネットだけを自動配線します。重要な線は手で引き、残りを任せる使い方ができます").clicked()
                    && let Some(n) = sel_net {
                        self.autoroute(Some(&n));
                    }
            });
            ui.small("2層・45°配線。ルール違反になる配線は自動的にやめます");
            ui.separator();
            self.panel_widths(ui);
            ui.separator();
            if ui.button("DRC を実行").on_hover_text("設計ルールチェック。線同士が近すぎる、銅が基板の端にはみ出している、などを調べます。発注前に必ず実行しましょう").clicked() {
                self.run_drc();
            }
            let mut jump = None;
            for v in &self.violations {
                if ui.link(format!("({:.1}, {:.1}) {}", v.at.x, v.at.y, v.message)).clicked() {
                    jump = Some(v.at);
                }
            }
            if let Some(at) = jump {
                self.pan = Vec2::new(at.x as f32, at.y as f32);
                self.zoom = self.zoom.max(25.0);
            }
            ui.separator();
            if ui.small_button("操作ガイド (F1)").on_hover_text("マウスとキーの操作の一覧を開きます").clicked() {
                self.help_request = Some(crate::help::HelpTab::Board);
            }
        });
    }

    pub(super) fn selection_panel(&mut self, ui: &mut egui::Ui) {
        if self.multi.len() > 1 {
            ui.separator();
            ui.label(format!("{} 個を選択中", self.multi.len()));
            if ui.button("選択を解除 (Esc)").clicked() {
                self.multi.clear();
            }
            return;
        }
        let Some(sel) = self.selected else { return };
        ui.separator();
        ui.label("選択中");
        match sel {
            Item::Track(i) if i < self.board.tracks.len() => {
                let mut w = self.board.tracks[i].width;
                let r = ui.add(egui::DragValue::new(&mut w).speed(0.01).range(0.05..=5.0).prefix("線幅 ").suffix(" mm"));
                if r.drag_started() || r.gained_focus() {
                    self.checkpoint();
                }
                if r.changed() {
                    self.board.tracks[i].width = w;
                }
                let mut s = self.board.tracks[i].side;
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut s, Side::Top, "表面");
                    ui.selectable_value(&mut s, Side::Bottom, "裏面");
                });
                if s != self.board.tracks[i].side {
                    self.checkpoint();
                    self.board.tracks[i].side = s;
                }
            }
            Item::Via(i) if i < self.board.vias.len() => {
                let (mut d, mut dr) = (self.board.vias[i].diameter, self.board.vias[i].drill);
                let r1 = ui.add(egui::DragValue::new(&mut d).speed(0.05).range(0.3..=3.0).prefix("径 ").suffix(" mm"));
                let r2 = ui.add(egui::DragValue::new(&mut dr).speed(0.05).range(0.2..=2.0).prefix("穴 ").suffix(" mm"));
                if r1.drag_started() || r1.gained_focus() || r2.drag_started() || r2.gained_focus() {
                    self.checkpoint();
                }
                if r1.changed() || r2.changed() {
                    self.board.vias[i].diameter = d;
                    self.board.vias[i].drill = dr;
                }
            }
            Item::Zone(i) if i < self.board.zones.len() => {
                let nets = self.nets();
                let mut net = self.board.zones[i].net.clone();
                let before = net.clone();
                egui::ComboBox::from_label("ゾーンのネット").selected_text(net.clone()).show_ui(ui, |ui| {
                    for n in &nets {
                        ui.selectable_value(&mut net, n.clone(), n);
                    }
                });
                let mut c = self.board.zones[i].clearance;
                let r = ui.add(egui::DragValue::new(&mut c).speed(0.05).range(0.1..=2.0).prefix("隙間 ").suffix(" mm"));
                let mut th = self.board.zones[i].thermal;
                let rt = ui.checkbox(&mut th, "サーマルリリーフ");
                if net != before || r.drag_started() || r.gained_focus() || rt.changed() {
                    self.checkpoint();
                }
                let z = &mut self.board.zones[i];
                z.net = net;
                if r.changed() {
                    z.clearance = c;
                }
                z.thermal = th;
            }
            Item::Part(i) if i < self.board.parts.len() => {
                let p = &self.board.parts[i];
                let (r, side) = (p.refdes.clone(), p.side);
                ui.label(format!("{}  {}  [{}]  {}", p.refdes, p.value, p.footprint, if side == Side::Bottom { "裏面実装" } else { "表面実装" }));
                let _ = r;
                if ui.button(if side == Side::Bottom { "表面に移す (B)" } else { "裏面に移す (B)" }).clicked() {
                    self.flip_selected_part();
                }
            }
            Item::Hole(i) if i < self.board.holes.len() => {
                ui.label(format!("取付穴 φ{:.2} mm", self.board.holes[i].drill));
            }
            _ => {}
        }
        if !matches!(sel, Item::Part(_)) && ui.button("削除 (Delete)").clicked() {
            self.delete(sel);
        }
    }
}
