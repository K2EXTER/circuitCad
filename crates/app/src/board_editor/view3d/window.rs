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
                Ok(s) => {
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
                _ => None,
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
        }
        let mut open = true;
        let mut retex = false;
        let mut stale = false;
        let mut want_help = false;
        let mut save_png = false;
        let mut note: Option<String> = None;
        let mut pick: Option<usize> = None;
        let selected = if let Some(Item::Part(i)) = self.selected { Some(i) } else { None };
        let v = &mut self.view3d;
        egui::Window::new("3Dで見る").open(&mut open).default_size([900.0, 680.0]).show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("上から").on_hover_text("真上から見ます (部品の配置・配線の確認)").clicked() {
                    (v.yaw, v.pitch, v.pan) = (0.0, std::f32::consts::FRAC_PI_2, Vec2::ZERO);
                }
                if ui.button("斜め").on_hover_text("斜め上から見ます (部品の高さや立体感)").clicked() {
                    (v.yaw, v.pitch, v.pan) = (0.0, 0.9, Vec2::ZERO);
                }
                if ui.button("横から").on_hover_text("真横から見ます (部品の高さや、ケースに入る高さの確認)").clicked() {
                    (v.yaw, v.pitch, v.pan) = (0.0, 0.12, Vec2::ZERO);
                }
                if ui.button("裏から").on_hover_text("基板を裏返して真下から見ます (裏面の部品と配線)").clicked() {
                    (v.yaw, v.pitch, v.pan) = (0.0, -std::f32::consts::FRAC_PI_2, Vec2::ZERO);
                }
                ui.separator();
                ui.checkbox(&mut v.show_parts, "部品").on_hover_text("部品の本体を表示します。形と高さはフットプリントからの目安です");
                ui.checkbox(&mut v.show_names, "部品名").on_hover_text("部品番号 (R1 など) を表示します");
                if ui.checkbox(&mut v.mask, "ソルダーレジスト").on_hover_text("基板の緑色の保護膜。オフにすると、銅箔の配線がそのまま見えます").changed() {
                    retex = true;
                }
                if ui.checkbox(&mut v.silk, "シルク").on_hover_text("白い印刷 (部品の枠や文字)").changed() {
                    retex = true;
                }
                ui.checkbox(&mut v.check_height, "ケース高さ").on_hover_text("ケースに入る高さを調べます。基板の面からこの高さを超える部品は赤くなり、高さの上限が橙の枠で見えます。0 にした面は調べません");
                if v.check_height {
                    ui.add(egui::DragValue::new(&mut v.limit_top).range(0.0..=200.0).speed(0.1).prefix("表 ").suffix(" mm")).on_hover_text("表側の、基板の面からケースまでの高さ (部品に使える高さ)");
                    ui.add(egui::DragValue::new(&mut v.limit_bottom).range(0.0..=200.0).speed(0.1).prefix("裏 ").suffix(" mm")).on_hover_text("裏側の、基板の面からケースまでの高さ");
                }
                ui.checkbox(&mut v.spin, "自動回転").on_hover_text("ゆっくり回して、いろいろな向きから見せます");
                if ui.button("更新").on_hover_text("基板エディタで変更した内容を反映します").clicked() {
                    stale = true;
                }
                if ui.add_enabled(selected.is_some(), egui::Button::new("選択部品へ寄る")).on_hover_text("基板エディタで選んでいる部品に近づきます。裏面の部品なら基板を裏返して見ます。ダブルクリックでも、部品の上なら寄れます").clicked() {
                    v.focus = selected;
                }
                if ui.button("PNG保存").on_hover_text("いま見えている向きの絵を画像ファイルに保存します (発注前の確認や、人に見せるときに)").clicked() {
                    save_png = true;
                }
                if let Some(sc) = &v.scene {
                    let shown = v.net.clone().unwrap_or_else(|| "なし".into());
                    egui::ComboBox::from_id_salt("view3d_net").selected_text(format!("ネット強調: {shown}")).show_ui(ui, |ui| {
                        if ui.selectable_label(v.net.is_none(), "なし").clicked() {
                            new_net = Some(None);
                        }
                        for n in &sc.nets {
                            if ui.selectable_label(v.net.as_deref() == Some(n.as_str()), n).clicked() {
                                new_net = Some(Some(n.clone()));
                            }
                        }
                    }).response.on_hover_text("選んだネットの銅 (配線・パッド・ビア) を桃色に光らせます。基板エディタで配線やパッドを選んでも、そのネットが光ります");
                }
                if ui.small_button("操作").on_hover_text("回す・動かす・拡大の操作を開きます").clicked() {
                    want_help = true;
                }
            });
            // (the rotate / pan / zoom operations are in the guide)
            if let Some(e) = &v.error {
                ui.colored_label(Color32::from_rgb(230, 60, 60), e);
                return;
            }
            let Some(scene) = &v.scene else { return };
            let (w, h) = (scene.hi.x - scene.lo.x, scene.hi.y - scene.lo.y);
            let mut info = format!("基板 {w:.1} × {h:.1} × {BOARD_THICKNESS} mm");
            if let Some((r, t)) = &scene.tallest[0] {
                info += &format!(" / 表側の最も高い部品 {r} (基板の面から {t:.1} mm)");
            }
            if let Some((r, t)) = &scene.tallest[1] {
                info += &format!(" / 裏側 {r} ({t:.1} mm)");
            }
            ui.small(info);
            if v.check_height {
                let over = scene.over_list();
                if over.is_empty() {
                    ui.colored_label(Color32::from_rgb(70, 190, 100), "✔ どの部品もケースの高さに収まっています");
                } else {
                    let names: Vec<String> = over.iter().take(6).map(|(r, o)| format!("{r} (+{o:.1} mm)")).collect();
                    let more = if over.len() > 6 { format!(" ほか {} 個", over.len() - 6) } else { String::new() };
                    ui.colored_label(Color32::from_rgb(230, 70, 60), format!("⚠ ケースの高さを超える部品 (赤): {}{more}", names.join(", ")));
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
            if resp.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll != 0.0 {
                    moving = true;
                    v.zoom = (v.zoom * (1.0 + scroll * 0.002)).clamp(0.2, MAX_ZOOM);
                }
            }
            if v.spin {
                v.yaw += ui.input(|i| i.stable_dt) * 0.5;
                ui.ctx().request_repaint();
            }
            let cam = camera(scene, resp.rect, v.yaw, v.pitch, v.zoom, v.pan);
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
            if v.cache.as_ref().is_none_or(|c| c.key != key) {
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
            if let Some(c) = &v.cache {
                painter.image(c.tex.id(), resp.rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                if let Some(p) = resp.hover_pos()
                    && let Some(k) = sample_at(resp.rect, c.size, p)
                {
                    let id = c.ids[k];
                    hover = (id != NO_PART && v.show_parts).then_some(id as usize);
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
                            ui.label(format!("値: {}", if val.is_empty() { "—" } else { val }));
                            ui.label(format!("フットプリント: {}", if fp.is_empty() { "—" } else { fp }));
                            ui.label(format!("高さの目安: {hh:.1} mm"));
                            if let Some(o) = scene.over.get(i).filter(|o| **o > 1e-6) {
                                ui.colored_label(Color32::from_rgb(230, 70, 60), format!("ケースの高さを {o:.1} mm 超えています"));
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
        if stale {
            self.view3d.stale = true;
        }
        if !open {
            self.view3d.open = false;
            self.view3d.scene = None;
        }
    }
}
