use super::*;

impl BoardEditor {
    pub fn view3d_window(&mut self, ctx: &egui::Context) {
        if !self.view3d.open {
            return;
        }
        if self.view3d.stale {
            self.view3d.stale = false;
            self.view3d.generation += 1;
            match Scene::build(&self.board, ctx, self.view3d.mask, self.view3d.silk) {
                Ok(mut s) => {
                    s.set_others(&self.view3d.others);
                    self.view3d.scene = Some(s);
                    self.view3d.error = None;
                    self.apply_net_glow();
                }
                Err(e) => {
                    self.view3d.scene = None;
                    self.view3d.error = Some(e);
                }
            }
        }
        // a track, pad or via picked in the editor lights up its net here (once per pick, so the list stays usable)
        let sel_net: Option<String> = {
            let b = &self.board;
            match self.selected {
                Some(Item::Track(i)) if i < b.tracks.len() => self.cached_connectivity().track_net(i).map(str::to_string),
                Some(Item::Pad(i)) if i < b.pads.len() => self.cached_connectivity().pad_net(i).map(str::to_string),
                Some(Item::Via(i)) if i < b.vias.len() => self.cached_connectivity().via_net(i).map(str::to_string),
                // nothing picked here: what was picked in the schematic
                _ => self.linked.net.clone(),
            }
        };
        let mut new_net: Option<Option<String>> = None;
        if sel_net.is_some() && sel_net != self.view3d.followed {
            new_net = Some(sel_net.clone());
        }
        self.view3d.followed = sel_net;
        if self.view3d.scene.is_some() {
            let (on, top, bottom) = (self.view3d.check_height, self.view3d.limit_top, self.view3d.limit_bottom);
            if self.view3d.scene.as_mut().is_some_and(|sc| sc.set_limits(on, top, bottom)) {
                self.view3d.generation += 1;
            }
            let spec = on.then(|| pcb_core::case::CaseSpec { inner_w: self.view3d.case_w, inner_d: self.view3d.case_d, top, bottom, clearance: 0.5 });
            if self.view3d.scene.as_mut().is_some_and(|sc| sc.set_case(spec, &self.board)) {
                self.view3d.generation += 1;
            }
            self.sync_heat_map();
            let temps = if self.view3d.heat_on { self.view3d.temps.clone() } else { None };
            if self.view3d.scene.as_mut().is_some_and(|sc| sc.set_heat(temps.as_ref())) {
                self.view3d.generation += 1;
            }
        }
        let mut open = true;
        let mut retex = false;
        let mut stale = false;
        // a change to the case read from its STEP (`Some(None)`: taken away), made after the window so it can be undone
        let mut case_change: Option<Option<pcb_core::case_model::CaseModel>> = None;
        let mut want_help = false;
        let mut save_png = false;
        let mut note: Option<String> = None;
        let mut pick: Option<usize> = None;
        let selected = if let Some(Item::Part(i)) = self.selected {
            Some(i)
        } else {
            // the part picked in the schematic is shown as the selected one
            self.linked.part.as_deref().and_then(|r| self.board.parts.iter().position(|p| p.refdes == r))
        };
        let v = &mut self.view3d;
        // beside the left panel, like the other windows (with no position it opened over the panel)
        egui::Window::new(i18n::tr("3Dで見る")).open(&mut open).default_pos(crate::widgets::beside_panel()).default_size([900.0, 680.0]).show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button(i18n::tr("上から")).on_hover_text(i18n::tr("真上から見ます (部品の配置・配線の確認)")).clicked() {
                    (v.yaw, v.pitch, v.pan) = (0.0, std::f32::consts::FRAC_PI_2, Vec2::ZERO);
                }
                if ui.button(i18n::tr("斜め")).on_hover_text(i18n::tr("斜め上から見ます (部品の高さや立体感)")).clicked() {
                    (v.yaw, v.pitch, v.pan) = (0.0, 0.9, Vec2::ZERO);
                }
                if ui.button(i18n::tr("横から")).on_hover_text(i18n::tr("真横から見ます (部品の高さや、ケースに入る高さの確認)")).clicked() {
                    (v.yaw, v.pitch, v.pan) = (0.0, 0.12, Vec2::ZERO);
                }
                if ui.button(i18n::tr("裏から")).on_hover_text(i18n::tr("基板を裏返して真下から見ます (裏面の部品と配線)")).clicked() {
                    (v.yaw, v.pitch, v.pan) = (0.0, -std::f32::consts::FRAC_PI_2, Vec2::ZERO);
                }
                ui.separator();
                ui.checkbox(&mut v.show_parts, i18n::tr("部品")).on_hover_text(i18n::tr("部品の本体を表示します。形と高さはフットプリントからの目安です"));
                ui.checkbox(&mut v.show_names, i18n::tr_ctx("3D", "部品名")).on_hover_text(i18n::tr("部品番号 (R1 など) を表示します"));
                if ui.checkbox(&mut v.mask, i18n::tr("ソルダーレジスト")).on_hover_text(i18n::tr("基板の緑色の保護膜。オフにすると、銅箔の配線がそのまま見えます")).changed() {
                    retex = true;
                }
                if ui.checkbox(&mut v.silk, i18n::tr("シルク")).on_hover_text(i18n::tr("白い印刷 (部品の枠や文字)")).changed() {
                    retex = true;
                }
                ui.checkbox(&mut v.check_height, i18n::tr("ケース高さ")).on_hover_text(i18n::tr("ケースに入る高さを調べます。基板の面からこの高さを超える部品は赤くなり、高さの上限が橙の枠で見えます。0 にした面は調べません"));
                if v.check_height {
                    ui.add(egui::DragValue::new(&mut v.limit_top).range(0.0..=200.0).speed(0.1).prefix(i18n::tr("表 ")).suffix(" mm")).on_hover_text(i18n::tr("表側の、基板の面からケースまでの高さ (部品に使える高さ)"));
                    ui.add(egui::DragValue::new(&mut v.limit_bottom).range(0.0..=200.0).speed(0.1).prefix(i18n::tr("裏 ")).suffix(" mm")).on_hover_text(i18n::tr("裏側の、基板の面からケースまでの高さ"));
                    ui.add(egui::DragValue::new(&mut v.case_w).range(0.0..=500.0).speed(0.5).prefix(i18n::tr("内寸 幅 ")).suffix(" mm")).on_hover_text(i18n::tr("ケースの内側の幅。基板が入るかを調べます (縁との隙間は 0.5 mm を見込みます)。0 にすると調べません。青い枠がケースの内側です"));
                    ui.add(egui::DragValue::new(&mut v.case_d).range(0.0..=500.0).speed(0.5).prefix(i18n::tr("奥行 ")).suffix(" mm")).on_hover_text(i18n::tr("ケースの内側の奥行 (幅と合わせて、基板の底面が入るかを調べます)"));
                }
                ui.checkbox(&mut v.heat_on, i18n::tr("温度の色")).on_hover_text(i18n::tr("部品を温度の色で塗ります (サーマルカメラのような見え方)。温度は回路図の「自己発熱」の計算の結果で、電力 × 熱抵抗から出した目安です。基板の「熱の地図」を計算してあれば、基板の表と裏もその温度で塗り、部品はその地図の温度になります (時間の変化の時刻を選んでいれば、その時刻)。青いほど周囲の温度に近く、黄色・白いほど熱い所です。白の見た目 (GPU) と合わせると見やすくなります"));
                ui.checkbox(&mut v.spin, i18n::tr("自動回転")).on_hover_text(i18n::tr("ゆっくり回して、いろいろな向きから見せます"));
                if gpu::ready() {
                    ui.checkbox(&mut v.use_gpu, i18n::tr("GPU で描く")).on_hover_text(i18n::tr("グラフィックス (GPU) で描きます。部品の形が細かくても軽く回せます。表示がおかしいときは外すと、これまでの描き方 (CPU) に戻ります"));
                    if v.use_gpu {
                        ui.selectable_value(&mut v.look, gpu::Look::Color, i18n::tr("色")).on_hover_text(i18n::tr("基板の絵と部品の色で描きます"));
                        ui.selectable_value(&mut v.look, gpu::Look::Clay, i18n::tr("白")).on_hover_text(i18n::tr("基板も部品も白で描きます。形と凹凸が見やすく、熱の色を重ねるときの土台になります (ケースの高さを超えた部品の赤と、ネットの強調は残ります)"));
                        ui.checkbox(&mut v.ao, i18n::tr("陰 (AO)")).on_hover_text(i18n::tr("面と面が寄り合う所・部品の根元・凹んだ所を暗くします (アンビエントオクルージョン)。立体の重なりが分かりやすくなります"));
                    }
                }
                if ui.button(i18n::tr("更新")).on_hover_text(i18n::tr("基板エディタで変更した内容を反映します")).clicked() {
                    stale = true;
                }
                if ui.add_enabled(selected.is_some(), egui::Button::new(i18n::tr("選択部品へ寄る"))).on_hover_text(i18n::tr("基板エディタで選んでいる部品に近づきます。裏面の部品なら基板を裏返して見ます。ダブルクリックでも、部品の上なら寄れます")).clicked() {
                    v.focus = selected;
                }
                if ui.button(i18n::tr("PNG保存")).on_hover_text(i18n::tr("いま見えている向きの絵を画像ファイルに保存します (発注前の確認や、人に見せるときに)")).clicked() {
                    save_png = true;
                }
                if let Some(sc) = &v.scene {
                    let shown = v.net.clone().unwrap_or_else(|| "なし".into());
                    egui::ComboBox::from_id_salt("view3d_net").selected_text(i18n::trf!("ネット強調: {shown}")).show_ui(ui, |ui| {
                        if ui.selectable_label(v.net.is_none(), i18n::tr("なし")).clicked() {
                            new_net = Some(None);
                        }
                        for n in &sc.nets {
                            if ui.selectable_label(v.net.as_deref() == Some(n.as_str()), n).clicked() {
                                new_net = Some(Some(n.clone()));
                            }
                        }
                    }).response.on_hover_text(i18n::tr("選んだネットの銅 (配線・パッド・ビア) を桃色に光らせます。基板エディタで配線やパッドを選んでも、そのネットが光ります"));
                }
                if ui.small_button(i18n::tr("操作")).on_hover_text(i18n::tr("回す・動かす・拡大の操作を開きます")).clicked() {
                    want_help = true;
                }
            });
            // (the rotate / pan / zoom operations are in the guide)
            if let Some(e) = &v.error {
                ui.colored_label(crate::theme::ERROR, e);
                return;
            }
            let Some(scene) = &v.scene else { return };
            let (w, h) = (scene.hi.x - scene.lo.x, scene.hi.y - scene.lo.y);
            let mut info = i18n::trf!("基板 {w:.1} × {h:.1} × {BOARD_THICKNESS} mm");
            if let Some((r, t)) = &scene.tallest[0] {
                info += &i18n::trf!(" / 表側の最も高い部品 {r} (基板の面から {t:.1} mm)");
            }
            if let Some((r, t)) = &scene.tallest[1] {
                info += &i18n::trf!(" / 裏側 {r} ({t:.1} mm)");
            }
            ui.small(info);
            if v.heat_on {
                match &v.temps {
                    None => {
                        ui.horizontal(|ui| {
                            ui.colored_label(crate::theme::WARN, i18n::tr("温度がまだありません。回路図の自己発熱を計算すると、部品が温度の色になります"));
                            if ui.button(i18n::tr("自己発熱を計算")).on_hover_text(i18n::tr("回路図の直流の動作点から、部品ごとの電力と温度を計算します (回路図の解析タブの「自己発熱」と同じ。周囲の温度もそこで変えられます)")).clicked() {
                                v.heat_request = true;
                            }
                        });
                    }
                    Some(t) => {
                        let mut hot: Vec<&(String, f64, f64)> = t.parts.iter().collect();
                        hot.sort_by(|a, b| b.1.total_cmp(&a.1));
                        let names: Vec<String> = hot.iter().take(4).map(|p| format!("{} {:.0} ℃", p.0, p.1)).collect();
                        let missing = t.parts.iter().filter(|p| !scene.parts.iter().any(|q| q.0 == p.0)).count();
                        ui.horizontal_wrapped(|ui| {
                            ui.small(if names.is_empty() { i18n::trf!("周囲 {:.0} ℃。電力が出ている部品はありません", t.ambient) } else { i18n::trf!("周囲 {:.0} ℃。熱い順: {}", t.ambient, names.join(", ")) });
                            if missing > 0 {
                                ui.small(i18n::trf!("(基板に無い部品 {missing} 個は描けません)"));
                            }
                            if ui.small_button(i18n::tr("計算し直す")).on_hover_text(i18n::tr("回路図を変えたあとに、自己発熱をもう一度計算します")).clicked() {
                                v.heat_request = true;
                            }
                        });
                    }
                }
            }
            if v.check_height && v.case_w > 0.0 && v.case_d > 0.0 {
                let r = pcb_core::case::check(&self.board, &pcb_core::case::CaseSpec { inner_w: v.case_w, inner_d: v.case_d, clearance: 0.5, ..Default::default() });
                if r.fits {
                    let how = if r.turned { " (基板を 90° 回すと)" } else { "" };
                    ui.colored_label(crate::theme::OK, i18n::trf!("✔ 基板 {:.1} × {:.1} mm はケースの底に入ります{how}。余り: {:.1} × {:.1} mm", r.board_w, r.board_d, r.spare_w, r.spare_d));
                } else {
                    ui.colored_label(crate::theme::ERROR, i18n::trf!("⚠ 基板 {:.1} × {:.1} mm はケースの底 ({:.1} × {:.1} mm、縁の隙間込み) に入りません (足りない分: 幅 {:.1} / 奥行 {:.1} mm)", r.board_w, r.board_d, v.case_w - 1.0, v.case_d - 1.0, (-r.spare_w).max(0.0), (-r.spare_d).max(0.0)));
                }
                // the screw posts (orange, red when they run into something) and the openings in the walls (pink)
                if let Some(ins) = scene.case.as_ref().map(|w| &w.inside) {
                    ui.horizontal_wrapped(|ui| {
                        if let Some(p) = ins.posts.first() {
                            ui.small(i18n::trf!("ねじ柱 {} 本 ({}、高さ {:.1} mm、外径 {:.1} mm・下穴 {:.1} mm)", ins.posts.len(), p.screw, p.height, p.outer_d, p.pilot_d))
                                .on_hover_text(i18n::tr("基板の取り付け穴の真下に立てる柱です (橙)。高さは「裏」の高さ (無ければ 5 mm)。ねじは穴の径から決めています。下穴はタッピングねじ用です"));
                        }
                        if !ins.openings.is_empty() {
                            let names: Vec<String> = ins.openings.iter().map(|o| format!("{} ({})", o.refdes, i18n::tr(o.wall.label()))).collect();
                            ui.small(i18n::trf!("壁の穴: {}", names.join(", "))).on_hover_text(i18n::tr("基板の縁のコネクタ (USB・DC ジャック・端子台) と、縁からはみ出す部品のために、壁にあける穴です (桃色の四角)。まわりに 0.5 mm の余裕を足しています"));
                        }
                        if ui.small_button(i18n::tr("ケースの図面 (DXF) を保存…")).on_hover_text(i18n::tr("ケースの底 (柱の位置・外径・下穴) と、まわりに開いた 4 枚の壁 (内側から見た穴の位置と大きさ) の図面です。ケースの CAD や、市販のケースに穴をあける型紙に使えます")).clicked() {
                            let spec = pcb_core::case::CaseSpec { inner_w: v.case_w, inner_d: v.case_d, top: v.limit_top, bottom: v.limit_bottom, clearance: 0.5 };
                            let name = format!("{}_case.dxf", if self.board.name.is_empty() { "board" } else { &self.board.name });
                            if let Some(path) = rfd::FileDialog::new().add_filter("DXF", &["dxf"]).set_file_name(name).save_file() {
                                self.status = match pcb_core::case::drawing(&self.board, &spec).and_then(|t| std::fs::write(&path, t).map_err(|e| e.to_string())) {
                                    Ok(()) => i18n::trf!("ケースの図面を保存しました: {}", path.display()),
                                    Err(e) => i18n::trf!("ケースの図面を保存できません: {e}"),
                                };
                            }
                        }
                    });
                    for p in ins.posts.iter().filter(|p| p.problem.is_some()) {
                        ui.colored_label(crate::theme::WARN, i18n::trf!("⚠ 穴 ({:.1}, {:.1}) の柱: {}", p.hole.x, p.hole.y, i18n::tr_dyn(p.problem.as_deref().unwrap_or(""))));
                    }
                    for n in &ins.notes {
                        ui.colored_label(crate::theme::WARN, i18n::tr_dyn(n));
                    }
                }
            }
            if v.check_height {
                let over = scene.over_list();
                if over.is_empty() {
                    ui.colored_label(crate::theme::OK, i18n::tr("✔ どの部品もケースの高さに収まっています"));
                } else {
                    let names: Vec<String> = over.iter().take(6).map(|(r, o)| format!("{r} (+{o:.1} mm)")).collect();
                    let more = if over.len() > 6 { format!(" ほか {} 個", over.len() - 6) } else { String::new() };
                    ui.colored_label(crate::theme::ERROR, i18n::trf!("⚠ ケースの高さを超える部品 (赤): {}{more}", names.join(", ")));
                }
            }
            // the case read from its STEP: read / move / turn / take away, and what it runs into
            ui.horizontal_wrapped(|ui| {
                if ui.small_button(i18n::tr("ケースの STEP を読む…")).on_hover_text(i18n::tr("ケースを設計した CAD の STEP を読み、基板のまわりに置きます (箱の真ん中を基板の真ん中に、底を基板の 5 mm 下に)。ケースは鋭い辺の線 (水色) で描き、ぶつかる部品は赤くなります。位置は下の数字で合わせます")).clicked()
                    && let Some(path) = rfd::FileDialog::new().add_filter("STEP", &["step", "stp", "STEP", "STP"]).pick_file()
                {
                    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| pcb_core::case_model::CaseModel::from_step(&b, &name, &self.board)) {
                        Ok(c) => case_change = Some(Some(c)),
                        Err(e) => self.status = i18n::trf!("ケースの STEP を読めません: {e}"),
                    }
                }
                if let Some(c) = self.board.case3d.clone() {
                    let s = c.size();
                    ui.small(i18n::trf!("{} ({:.1} × {:.1} × {:.1} mm)", c.file, s[0], s[1], s[2]));
                    let mut moved = c.clone();
                    for (k, label) in [(0, "x "), (1, "y "), (2, "z ")] {
                        ui.add(egui::DragValue::new(&mut moved.offset[k]).speed(0.1).prefix(label).suffix(" mm")).on_hover_text(i18n::tr("ケースの位置 (ケースの STEP の原点を、基板の座標のどこに置くか)。z は基板の裏の面が 0 です"));
                    }
                    if ui.small_button(i18n::tr("90° 回す")).on_hover_text(i18n::tr("ケースを上から見て左回りに 90° 回します")).clicked() {
                        moved.rot = (moved.rot + 1) % 4;
                    }
                    if moved != c {
                        case_change = Some(Some(moved));
                    }
                    if ui.small_button(i18n::tr("外す")).on_hover_text(i18n::tr("読んだケースを基板から外します")).clicked() {
                        case_change = Some(None);
                    }
                }
            });
            if self.board.case3d.is_some() {
                let names: Vec<&str> = scene.case_hits.iter().enumerate().filter(|(_, h)| **h).filter_map(|(i, _)| self.board.parts.get(i).map(|p| p.refdes.as_str())).collect();
                if names.is_empty() && !scene.case_board_hit {
                    ui.colored_label(crate::theme::OK, i18n::tr("✔ ケースにぶつかる部品はありません"));
                } else {
                    let mut what = names.join(", ");
                    if scene.case_board_hit {
                        what = if what.is_empty() { i18n::tr("基板").to_string() } else { format!("{} {what}", i18n::tr("基板,")) };
                    }
                    ui.colored_label(crate::theme::ERROR, i18n::trf!("⚠ ケースにぶつかるかもしれません (赤): {what}"))
                        .on_hover_text(i18n::tr("部品は 3D で描く箱で調べるので、壁のすぐそばの丸い部品は、実際には当たらなくても出ることがあります。足が床に当たるときは、柱を高くするかケースを下げます"));
                }
            }
            let size = ui.available_size().max(Vec2::new(300.0, 240.0));
            let (resp, painter) = ui.allocate_painter(size, Sense::click_and_drag());
            painter.rect_filled(resp.rect, 4.0, Color32::from_rgb(34, 38, 46));
            if resp.dragged_by(egui::PointerButton::Primary) {
                let d = resp.drag_delta();
                v.yaw -= d.x * 0.01;
                v.pitch = (v.pitch + d.y * 0.01).clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2);
            }
            if resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle) {
                v.pan += resp.drag_delta();
            }
            if resp.double_clicked() {
                if let Some(i) = v.hover.filter(|_| v.show_parts) {
                    v.focus = Some(i);
                } else {
                    (v.yaw, v.pitch, v.zoom, v.pan) = (0.0, 0.9, 1.0, Vec2::ZERO);
                }
            }
            if let Some(i) = v.focus.take()
                && let Some((pitch, zoom, pan)) = focus_on(scene, i, resp.rect, v.yaw, v.pitch)
            {
                (v.pitch, v.zoom, v.pan) = (pitch, zoom, pan);
            }
            let mut moving = resp.dragged();
            if resp.dragged() || resp.double_clicked() {
                v.touched = true;
            }
            if resp.hovered() {
                let scroll = crate::prefs::zoom_scroll(ui);
                if scroll != 0.0 {
                    moving = true;
                    v.touched = true;
                    v.zoom = (v.zoom * (1.0 + scroll * 0.002)).clamp(0.2, MAX_ZOOM);
                }
            }
            if v.spin {
                v.yaw += ui.input(|i| i.stable_dt) * 0.5;
                ui.ctx().request_repaint();
            }
            let cam = camera(scene, resp.rect, v.yaw, v.pitch, v.zoom, v.pan);
            let on_gpu = v.use_gpu && gpu::ready();
            if on_gpu {
                // the GPU draws every frame; its vertex data is made again only when the scene or what shows in it changes
                if v.gpu_geometry.as_ref().is_none_or(|g| g.key() != (scene.serial, v.generation)) {
                    v.gpu_geometry = Some(std::sync::Arc::new(gpu::Geometry::build(scene, v.generation)));
                }
                if let Some(g) = &v.gpu_geometry {
                    let frame = gpu::Frame { geometry: g.clone(), cam, rect: resp.rect, ppp: ui.ctx().pixels_per_point(), show_parts: v.show_parts, hover: v.hover, selected, look: v.look, ao: v.ao };
                    painter.add(eframe::egui_wgpu::Callback::new_paint_callback(resp.rect, gpu::Paint(frame)));
                }
                v.cache = None;
            }
            // the picture: drawn at twice the size and averaged (release builds), and kept while nothing in it changes
            let quality = if moving { Quality::Draft } else { Quality::Fine(if cfg!(debug_assertions) { 1 } else { 2 }) };
            let size_px = [resp.rect.width().round().max(1.0) as usize, resp.rect.height().round().max(1.0) as usize];
            let key = PictureKey {
                camera: [v.yaw.to_bits(), v.pitch.to_bits(), v.zoom.to_bits(), v.pan.x.to_bits(), v.pan.y.to_bits()],
                size: size_px,
                parts: v.show_parts,
                hover: v.hover,
                selected,
                generation: v.generation,
                draft: moving,
            };
            if moving {
                // one more frame once it stops, to draw the full picture
                ui.ctx().request_repaint();
            }
            if !on_gpu && v.cache.as_ref().is_none_or(|c| c.key != key) {
                let (pixels, ids, w, h) = snapshot(scene, resp.rect, &cam, v.show_parts, quality, v.hover, selected);
                let rgba: Vec<u8> = pixels.iter().flatten().copied().collect();
                let tex = ui.ctx().load_texture("board3d_view", egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba), egui::TextureOptions::LINEAR);
                v.cache = Some(Cached { key, tex, ids, size: [w, h] });
            }
            if save_png {
                // a clean picture (no outlines), drawn at twice the size
                let (pixels, _, w, h) = snapshot(scene, resp.rect, &cam, v.show_parts, Quality::Fine(2), None, None);
                if let Some(path) = rfd::FileDialog::new().add_filter("PNG画像", &["png"]).set_file_name("board3d.png").save_file() {
                    let rgba: Vec<u8> = pixels.iter().flatten().copied().collect();
                    note = Some(match image::save_buffer(&path, &rgba, w as u32, h as u32, image::ColorType::Rgba8) {
                        Ok(()) => format!("3D表示を保存しました: {}", path.display()),
                        Err(e) => format!("3D表示を保存できませんでした: {e} (保存先のフォルダに書き込めるか確認してください)"),
                    });
                }
            }
            let mut hover = None;
            if on_gpu {
                hover = resp.hover_pos().and_then(|p| gpu::pick(scene, &cam, p, v.show_parts));
            } else if let Some(c) = &v.cache {
                painter.image(c.tex.id(), resp.rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                if let Some(p) = resp.hover_pos()
                    && let Some(k) = sample_at(resp.rect, c.size, p)
                {
                    let id = c.ids[k];
                    hover = (id != NO_PART && v.show_parts).then_some(id as usize);
                }
            }
            if v.heat_on
                && let Some(t) = &v.temps
            {
                heat::legend(&painter, resp.rect, t);
            }
            if !v.touched {
                // what to do with the picture, until the first time it is turned or zoomed
                let lines = ["ドラッグ: 回す  /  右ドラッグ: 動かす  /  ホイール: 拡大・縮小  /  ダブルクリック: 元の向きに戻す", "上の「ケース高さ」を入れると、ケースに入らない高さの部品が赤くなります"];
                let font = egui::FontId::proportional(13.0);
                let mut y = resp.rect.bottom() - 10.0;
                for line in lines.iter().rev() {
                    let g = painter.layout_no_wrap(i18n::tr(line).to_string(), font.clone(), Color32::WHITE);
                    let at = Pos2::new(resp.rect.left() + 12.0, y - g.size().y);
                    painter.rect_filled(Rect::from_min_size(at - Vec2::new(6.0, 3.0), g.size() + Vec2::new(12.0, 6.0)), 4.0, Color32::from_black_alpha(150));
                    y = at.y - 8.0;
                    painter.galley(at, g, Color32::WHITE);
                }
            }
            if hover != v.hover {
                // the part under the pointer changed: draw it brighter in the next picture
                v.hover = hover;
                ui.ctx().request_repaint();
            }
            if v.show_names {
                let near_side = cam.pitch.sin() <= 0.0;
                for (i, (name, ..)) in scene.parts.iter().enumerate() {
                    if let Some(b) = scene.boxes.iter().filter(|b| b.part == i && b.bottom == near_side).max_by(|a, b| a.hi[2].total_cmp(&b.hi[2])) {
                        let z = if near_side { b.lo[2] } else { b.hi[2] };
                        let (pos, _) = cam.project([(b.lo[0] + b.hi[0]) / 2.0, (b.lo[1] + b.hi[1]) / 2.0, z]);
                        painter.text(pos, egui::Align2::CENTER_CENTER, name, egui::FontId::proportional(11.0), Color32::WHITE);
                    }
                }
            }
            if let Some(i) = hover {
                if let Some((r, val, fp, hh)) = scene.parts.get(i) {
                    egui::Area::new(egui::Id::new("view3d_tip")).order(egui::Order::Tooltip).fixed_pos(resp.hover_pos().unwrap_or_default() + Vec2::new(14.0, 14.0)).show(ui.ctx(), |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.strong(r);
                            ui.label(i18n::trf!("値: {}", if val.is_empty() { "—" } else { val }));
                            ui.label(i18n::trf!("フットプリント: {}", if fp.is_empty() { "—" } else { fp }));
                            ui.label(i18n::trf!("高さの目安: {hh:.1} mm"));
                            if v.heat_on
                                && let Some(t) = &v.temps
                            {
                                match scene.temps.get(i).copied().flatten() {
                                    Some((tc, w)) => ui.label(i18n::trf!("温度の目安: {tc:.1} ℃ (電力 {}、周囲 +{:.1} K)", fmt_watts(w), tc - t.ambient)),
                                    None => ui.label(i18n::trf!("温度の目安: 周囲と同じ {:.0} ℃ (電力が出ていない、または回路図に無い部品)", t.ambient)),
                                };
                            }
                            if let Some(o) = scene.over.get(i).filter(|o| **o > 1e-6) {
                                ui.colored_label(crate::theme::ERROR, i18n::trf!("ケースの高さを {o:.1} mm 超えています"));
                            }
                        });
                    });
                }
                if resp.clicked_by(egui::PointerButton::Primary) {
                    pick = Some(i);
                }
            }
        });
        if let Some(n) = new_net {
            self.view3d.net = n;
            self.apply_net_glow();
        }
        if let Some(n) = note {
            self.status = n;
        }
        if retex && let Some(s) = &mut self.view3d.scene {
            s.img = Scene::images(&s.pv, self.view3d.mask, self.view3d.silk);
            self.view3d.generation += 1;
            // the heat map is painted on the new pictures again
            self.view3d.board_heat = None;
        }
        if let Some(i) = pick {
            self.selected = Some(Item::Part(i));
            self.multi.clear();
            if let Some(p) = self.board.parts.get(i) {
                self.status = format!("3D表示から {} を選びました (基板エディタで選択中)", p.refdes);
            }
        }
        if want_help {
            self.help_request = Some(crate::help::HelpTab::View3d);
        }
        if let Some(c) = case_change {
            // one undo step for a whole drag of a position
            self.checkpoint_once();
            self.board.case3d = c;
            // only the case is drawn again (not the board's pictures)
            if let Some(sc) = self.view3d.scene.as_mut() {
                sc.set_case_model(&self.board);
                self.view3d.generation += 1;
            }
        }
        if stale {
            self.view3d.stale = true;
        }
        if !open {
            self.view3d.open = false;
            self.view3d.scene = None;
        }
    }
}

/// A power for the tooltip (W, mW or µW).
fn fmt_watts(w: f64) -> String {
    if w >= 1.0 { format!("{w:.2} W") } else if w >= 1e-3 { format!("{:.1} mW", w * 1e3) } else { format!("{:.1} µW", w * 1e6) }
}
