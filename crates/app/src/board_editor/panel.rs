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
        let unrouted = self.cached_airwires().len();
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
            // icon-only buttons are packed side by side; with labels they stay one per line
            let wrap = crate::icons::icon_only();
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.y = if wrap { 4.0 } else { ui.spacing().item_spacing.y };
                // the picked tool's settings open right under its button; the rest of the tools follow below
                let at = TOOLS.iter().position(|(t, _, _)| *t == self.tool).filter(|_| matches!(self.tool, Tool::Track | Tool::Via));
                let (head, tail) = TOOLS.split_at(at.map_or(TOOLS.len(), |i| i + 1));
                for part in [head, tail] {
                    if wrap {
                        ui.horizontal_wrapped(|ui| self.tool_buttons(ui, part));
                    } else {
                        self.tool_buttons(ui, part);
                    }
                    if std::ptr::eq(part, head) {
                        self.tool_options(ui);
                    }
                }
            });
            ui.horizontal(|ui| {
                if crate::icons::icon_button(ui, Icon::Undo, "元に戻す", false, !self.undo.is_empty(), "直前の操作を取り消します (Ctrl+Z)").clicked() {
                    self.undo();
                }
                if crate::icons::icon_button(ui, Icon::Redo, "やり直し", false, !self.redo.is_empty(), "取り消した操作をやり直します (Ctrl+Y)").clicked() {
                    self.redo();
                }
            });
            egui::CollapsingHeader::new("作業の設定").default_open(true).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("作業層");
                    crate::icons::help(ui, concat!("画面右上の「層」で切り替えます (L キーでも順に切り替わります)。",
                        "いま配線を置く銅箔の層。部品とシルクは表面か裏面にだけ置けます (内層を選んでいるときは表面)。右上の「層」は、たたんで小さくできます"));
                });
                self.layer_count_ui(ui);
                ui.add(egui::DragValue::new(&mut self.grid).speed(0.05).range(0.0..=5.0).prefix("グリッド ").suffix(" mm")).on_hover_text("カーソルが吸い付く間隔。0 なら吸い付きません。小さいほど細かく置けます (部品の足の間隔は 2.54mm や 1.27mm が多いです)");
                if matches!(self.tool, Tool::PadTh | Tool::PadSmd) {
                    ui.add(egui::DragValue::new(&mut self.pad_size).speed(0.05).range(0.3..=10.0).prefix("パッド径 ").suffix(" mm")).on_hover_text("パッド (部品の足をはんだ付けする銅の丸) の直径");
                    ui.add(egui::DragValue::new(&mut self.pad_drill).speed(0.05).range(0.2..=6.0).prefix("パッド穴 ").suffix(" mm")).on_hover_text("パッドの穴径 (スルーホール)。部品の足の太さ + 0.2〜0.3mm が目安です");
                }
                if self.tool == Tool::Hole {
                    ui.add(egui::DragValue::new(&mut self.hole_drill).speed(0.1).range(0.5..=10.0).prefix("取付穴 ").suffix(" mm")).on_hover_text("ネジ止め用の取付穴の穴径。M3 のネジなら 3.2mm 前後です");
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
                    }).response.on_hover_text("ゾーンでつなぐネット。ふつうは GND にして、基板の余った所を GND の銅で塗ります");
                    ui.add(egui::DragValue::new(&mut self.zone_clearance).speed(0.05).range(0.1..=2.0).prefix("隙間 ").suffix(" mm")).on_hover_text("ゾーンの銅と、ほかのネットの配線・パッドの間をあける距離。小さいと短絡しやすく、大きいと銅が減ります (多くは 0.3〜0.5mm)");
                    ui.checkbox(&mut self.zone_thermal, "スルーホールはサーマルリリーフ").on_hover_text("スルーホールのパッドを、ゾーンに細い十字の足 (サーマルリリーフ) でつなぎます。熱が逃げにくくなり、はんだ付けしやすくなります。オフにすると全面でつながります");
                    ui.checkbox(&mut self.zone_remove_islands, "孤立した銅の破片を消す").on_hover_text("配線で切り分けられて、ゾーンのネットのパッド・ビア・配線のどれにもつながっていない小さな銅 (島) を作らないようにします。島はノイズを拾うアンテナになり、製造中にはがれることもあります");
                }
            });
            self.selection_panel(ui);
            // open while the board is still being set up (no parts or no outline yet), then folded away
            let setup_open = self.board.parts.is_empty() || self.board.outline.len() < 3;
            egui::CollapsingHeader::new("① 配置と外形").default_open(setup_open).show(ui, |ui| {
                if ui.button("回路図から取り込み").on_hover_text("回路図の部品とつながり (ネット) を基板に読み込みます。部品が並び、つなぐべき所に黄色の線 (未配線) が出ます。回路図を直したらもう一度押します").clicked() {
                    self.import_from_schematic(sch);
                }
                let diff = self.schematic_diff(sch).clone();
                if !diff.is_empty() {
                    ui.colored_label(Color32::from_rgb(255, 180, 60), format!("回路図と違います: {}", diff.summary()))
                        .on_hover_text("取り込んだあとに、回路図が変わったか、基板が変わりました。「回路図から取り込み」を押すと、基板の部品の位置と配線は残したまま、値・形・つながりが回路図に合わせて更新されます");
                }
                for n in &self.import_notes {
                    ui.colored_label(Color32::from_rgb(255, 180, 60), n);
                }
                ui.label(format!("未配線: {} 本 (黄色の線)", self.cached_airwires().len()));
                if ui.button("部品を並べて外形を決める").on_hover_text("すべての部品を、つながりの強いものが近くなるように並べ、基板の外形をそれに合わせます。最初の叩き台を作る機能なので、並べたあとに手で動かして整えてください。配線済みの基板では使えません (部品を動かすと配線が合わなくなるため)").clicked() {
                    self.autoplace();
                }
                ui.separator();
                ui.label("基板の外形");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut self.size_w).speed(0.5).range(5.0..=500.0).prefix("幅 ")).on_hover_text("基板の幅 (mm)。「矩形に設定」を押すと外形に反映されます");
                    ui.add(egui::DragValue::new(&mut self.size_h).speed(0.5).range(5.0..=500.0).prefix("高さ ")).on_hover_text("基板の高さ (mm)。「矩形に設定」を押すと外形に反映されます");
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
            });
            egui::CollapsingHeader::new("② 配線").default_open(true).show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("全部").on_hover_text("未配線 (黄色の線) をすべて自動で配線します。先に部品を基板の上に並べておいてください。
    線幅・ビア径は、配線・ビアツールを選ぶと出る設定が使われ、通せなかった接続は黄色の線のまま残ります。気に入らなければ「元に戻す」で取り消せます。
    2層・45°配線で、ルール違反になる配線は自動的にやめます").clicked() {
                        self.autoroute(None);
                    }
                    if ui.button("GND ベタ").on_hover_text("基板の裏面ぜんぶを GND の銅箔 (ベタ) にして、表面の GND パッドを、短い線とビアで裏のベタにつなぎます。GND の長い配線が要らなくなり、配線も楽になります。
    部品を並べて基板の外形を決めたあと、「全部」の自動配線の前に押します (四角い外形の基板で使えます)。気に入らなければ「元に戻す」で取り消せます").clicked() {
                        self.ground_pour();
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
            });
            egui::CollapsingHeader::new("③ 仕上げ (?)").default_open(false).show(ui, |ui| {
                self.panel_pad_entries(ui);
                self.panel_smooth(ui);
            }).header_response.on_hover_text("パッドの根元をそろえる、配線の角を丸める、涙型など、見た目と製造のための仕上げです");
            egui::CollapsingHeader::new("高度な配線 (?)").default_open(false).show(ui, |ui| {
                self.panel_widths(ui);
                self.panel_pairs(ui);
                self.panel_lengths(ui);
            }).header_response.on_hover_text("配線の太さ (電流から)・差動ペア・長さ合わせ");
            egui::CollapsingHeader::new("④ チェックと発注").default_open(true).show(ui, |ui| {
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
                let lacking = pcb_core::drc::thermal_via_checks(&self.board).len();
                if lacking > 0
                    && ui
                        .button(format!("サーマルビアを置く ({lacking} 個の放熱パッド)"))
                        .on_hover_text("放熱パッド (ICの裏のパッドなど) の中に、熱を裏面へ逃がすビアを格子状に置きます。そのあと、裏面に同じネット (たいてい GND) のゾーンを作ると、熱が基板全体に広がります")
                        .clicked()
                {
                    self.checkpoint();
                    let n = pcb_core::drc::add_thermal_vias(&mut self.board);
                    self.status = if n > 0 { format!("サーマルビアを {n} 個置きました。裏面に同じネットのゾーンを作ってください") } else { "サーマルビアを置けませんでした (パッドが小さい、または他の穴に近すぎます)".into() };
                }
                if ui.button("3Dで見る…").on_hover_text("基板を立体で見ます。出来上がりの色 (緑のレジスト・金色のパッド・白いシルク) と、部品の高さの目安が分かります。ドラッグで回せます").clicked() {
                    self.open_view3d();
                }
                if ui.button("発注の準備…").on_hover_text("発注先を選び、チェックと出来上がりの確認をして、製造ファイルを書き出します。発注までの手順も表示します").clicked() {
                    self.open_order();
                }
                ui.label("基板名 (出力ファイル名)");
                ui.text_edit_singleline(&mut self.board.name).on_hover_text("製造ファイル (Gerber など) の名前に使います。半角の英数字がおすすめです");
            });
            egui::CollapsingHeader::new("選択の絞り込みと固定").default_open(false).show(ui, |ui| {
                ui.label("見る物・選べる物のアイコンは、画面の右上、「層」の下にあります").on_hover_text("画面右上の「表示・選択」で、シルク・ゾーン・未配線などの表示と、選べる物 (部品・配線・ビア…) を切り替えます");
                ui.horizontal(|ui| {
                    if ui.small_button("配線・ビアだけ").on_hover_text("配線とビアだけを選べるようにします。部品は動きません").clicked() {
                        self.pick = Pickable { parts: false, tracks: true, vias: true, pads: false, other: false, active_side_only: self.pick.active_side_only };
                        self.multi.retain(|it| matches!(it, Item::Track(_) | Item::Via(_)));
                        if self.selected.is_some_and(|it| !self.pick.allows(&it)) {
                            self.selected = None;
                        }
                    }
                    if ui.small_button("すべて").on_hover_text("すべての物を選べるようにします").clicked() {
                        self.pick = Pickable { active_side_only: self.pick.active_side_only, ..Pickable::ALL };
                    }
                    if ui.small_button("部品を全部固定").on_hover_text("いま基板にあるすべての部品を固定します。配置が終わって、配線に進むときに押します").clicked() {
                        self.board.locked_parts = self.board.parts.iter().map(|p| p.refdes.clone()).collect();
                        self.status = format!("部品 {} 個を固定しました", self.board.parts.len());
                    }
                    if ui.small_button("固定をすべて外す").on_hover_text("固定した部品をすべて動かせるようにします").clicked() {
                        self.board.locked_parts.clear();
                        self.status = "部品の固定をすべて外しました".into();
                    }
                });
            });
            ui.separator();
            if ui.small_button("操作ガイド (F1)").on_hover_text("マウスとキーの操作の一覧を開きます").clicked() {
                self.help_request = Some(crate::help::HelpTab::Board);
            }
        });
    }

    fn tool_buttons(&mut self, ui: &mut egui::Ui, tools: &[(Tool, &'static str, Icon)]) {
        for (t, name, icon) in tools {
            if crate::icons::icon_button(ui, *icon, name, self.tool == *t, true, tool_tip(*t)).clicked() {
                self.tool = *t;
                self.pending = None;
                self.poly.clear();
                // what was selected stays behind: R, B and Delete must not reach it from another tool
                if *t != Tool::Select {
                    self.selected = None;
                    self.multi.clear();
                    self.boxsel = None;
                }
            }
        }
    }

    /// Settings of the tool that is picked, shown right under the toolbar: how a wire is laid for the wire tool,
    /// the size of a via for the via tool (autoroute uses both).
    fn tool_options(&mut self, ui: &mut egui::Ui) {
        match self.tool {
            Tool::Track => {
        ui.label("引き方").on_hover_text("配線ツールで引くときの設定です。線幅・ビアは、自動配線にも使われます");
        ui.checkbox(&mut self.auto_route, "自動経路 (パッドまで探す)").on_hover_text("入れると、配線ツールで、行き先のパッド・ビア・配線の端にカーソルを合わせたとき、出発点からそこまでの経路を、障害物をよけて自動で探して見せます (水色の線)。クリックで、その経路を配線します。探せないとき・行き先が何もない所のときは、いつもの曲げ方の線になります。切ると、押しのけも使わない、手で決めたとおりの配線になります");
        // shoving only works on top of the automatic path, so it is offered only while that is on
        if self.auto_route {
            ui.indent("shove_indent", |ui| {
                ui.checkbox(&mut self.shove, "押しのけ配線").on_hover_text("自動経路で通れないとき・行き先が何もない所のときに、邪魔になる別のネットの配線を脇へ寄せて通します。パッドやビアに固定された配線は動かさず、寄せきれないときは普通に引きます。プレビューが橙色なら押しのけ、赤ならルール違反です");
            });
        }
        ui.horizontal(|ui| {
            ui.label("配線");
            ui.selectable_value(&mut self.route_mode, RouteMode::Angle45, "45°").on_hover_text("配線を 45° の斜めを使って曲げます。基板の標準的な引き方で、角が鈍くなり、きれいで製造もしやすくなります");
            ui.selectable_value(&mut self.route_mode, RouteMode::Orthogonal, "直角").on_hover_text("配線を縦と横だけで曲げます (斜めは使いません)");
            ui.selectable_value(&mut self.route_mode, RouteMode::Free, "自由").on_hover_text("クリックした点どうしを、そのまま直線でつなぎます (どんな角度でも)。鋭い角ができやすいので注意しましょう");
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.pad_lead, "パッドに垂直に出入り").on_hover_text("配線をパッドから出すとき・パッドへ入れるときに、パッドの辺に垂直な短い区間を必ず付けます (そのあとは、いつもの 45° などで行き先へ向かいます)。斜めに入る根元がなくなり、きれいにそろいます。距離が近すぎるときと、丸いパッド、「自由」の配線では何もしません。長いパッドでは、パッドの両端にも吸い付くので、2本を並べて出せます");
            ui.add_enabled(self.pad_lead, egui::DragValue::new(&mut self.pad_lead_len).speed(0.05).range(0.1..=3.0).suffix(" mm")).on_hover_text("パッドの縁から、垂直に出す区間の長さ");
        });
        ui.add(egui::DragValue::new(&mut self.track_width).speed(0.01).range(0.05..=5.0).prefix("線幅 ").suffix(" mm"))
            .on_hover_text("配線の太さ。大きい電流を流す線は太く。発注先の最小線幅 (多くは 0.15〜0.2 mm) を下回らないように");
            }
            Tool::Via => {
        ui.add(egui::DragValue::new(&mut self.via_dia).speed(0.05).range(0.3..=3.0).prefix("ビア径 ").suffix(" mm")).on_hover_text("ビア (表と裏をつなぐ穴) の外径。銅の丸の大きさです。穴径より 0.3〜0.4mm 大きくします。発注先の最小値を下回らないように");
        ui.add(egui::DragValue::new(&mut self.via_drill).speed(0.05).range(0.2..=2.0).prefix("ビア穴 ").suffix(" mm"))
            .on_hover_text("ビアの穴径。小さいほど基板が高くなることがあります。発注先の最小穴径を確認しましょう");
            }
            _ => {}
        }
    }

    pub(super) fn selection_panel(&mut self, ui: &mut egui::Ui) {
        if self.multi.len() > 1 {
            ui.separator();
            ui.label(format!("{} 個を選択中", self.multi.len()));
            if ui.button("選択を解除 (Esc)").on_hover_text("選択をやめます (Esc キーでも同じです)").clicked() {
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
                let r = ui.add(egui::DragValue::new(&mut w).speed(0.01).range(0.05..=5.0).prefix("線幅 ").suffix(" mm")).on_hover_text("選んだ配線の太さ。大きい電流を流す線は太くします");
                if r.drag_started() || r.gained_focus() {
                    self.checkpoint();
                }
                if r.changed() {
                    self.board.tracks[i].width = w;
                }
                if ui.button("このネットの配線を全部消す").on_hover_text("この配線がつながっているネットの、配線とビアをすべて消します (パッドは残り、未配線の線が戻ります)。引き直したいときに使います").clicked() {
                    self.rip_up_selected_net(i);
                    return;
                }
                let mut s = self.board.tracks[i].side;
                ui.horizontal(|ui| {
                    for l in self.board.copper_layers() {
                        ui.selectable_value(&mut s, l, l.label()).on_hover_text("この配線をこの層に移します");
                    }
                });
                if s != self.board.tracks[i].side {
                    self.checkpoint();
                    self.board.tracks[i].side = s;
                }
            }
            Item::Via(i) if i < self.board.vias.len() => {
                let (mut d, mut dr) = (self.board.vias[i].diameter, self.board.vias[i].drill);
                let r1 = ui.add(egui::DragValue::new(&mut d).speed(0.05).range(0.3..=3.0).prefix("径 ").suffix(" mm")).on_hover_text("選んだビアの外径 (銅の丸の大きさ)");
                let r2 = ui.add(egui::DragValue::new(&mut dr).speed(0.05).range(0.2..=2.0).prefix("穴 ").suffix(" mm")).on_hover_text("選んだビアの穴径");
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
                }).response.on_hover_text("このゾーンでつなぐネット");
                let mut c = self.board.zones[i].clearance;
                let r = ui.add(egui::DragValue::new(&mut c).speed(0.05).range(0.1..=2.0).prefix("隙間 ").suffix(" mm")).on_hover_text("このゾーンと、ほかのネットの間をあける距離");
                let mut th = self.board.zones[i].thermal;
                let rt = ui.checkbox(&mut th, "サーマルリリーフ").on_hover_text("スルーホールのパッドを、細い十字の足でゾーンにつなぎます (はんだ付けしやすくなります)");
                let mut ri = self.board.zones[i].remove_islands;
                let rr = ui.checkbox(&mut ri, "孤立した銅の破片を消す").on_hover_text("ゾーンのネットのパッド・ビア・配線のどれにもつながっていない銅の島を作らないようにします (ノイズを拾う・製造中にはがれるのを防ぐ)");
                if net != before || r.drag_started() || r.gained_focus() || rt.changed() || rr.changed() {
                    self.checkpoint();
                }
                self.board.zones[i].remove_islands = ri;
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
                let locked = self.board.is_locked(&r);
                if ui.button(if locked { "固定を外す" } else { "固定する" }).on_hover_text("固定すると、ドラッグ・回転 (R)・面の付け替え (B) ができなくなり、範囲選択に巻き込んで動かす心配もなくなります。配置が済んだ部品に使います。基板ファイルに保存されます").clicked() {
                    self.checkpoint();
                    if locked {
                        self.board.locked_parts.retain(|x| x != &r);
                    } else {
                        self.board.locked_parts.push(r.clone());
                    }
                    self.status = if locked { format!("{r} の固定を外しました") } else { format!("{r} を固定しました") };
                }
                if ui.button(if side == Side::Bottom { "表面に移す (B)" } else { "裏面に移す (B)" }).on_hover_text("部品を反対の面に付け替えます。裏面の部品は左右が反転します (B キーでも)").clicked() {
                    self.flip_selected_part();
                }
            }
            Item::Hole(i) if i < self.board.holes.len() => {
                ui.label(format!("取付穴 φ{:.2} mm", self.board.holes[i].drill));
            }
            _ => {}
        }
        if !matches!(sel, Item::Part(_)) && ui.button("削除 (Delete)").on_hover_text("選んだものを消します (Delete キー)。部品は消せません").clicked() {
            self.delete(sel);
        }
    }
}
