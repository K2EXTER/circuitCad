//! "設計の要件" window: one table for the whole design (all sheets) of what the board has to do for the circuit: what the nets are for
//! (role, width, clearance, current), which parts belong close together, and what keeps its distance. The board import takes it over
//! (net classes, currents, placement groups, distances); the board editor checks the placement against it.

use super::*;
use design_spec::{Bypass, KeepApart, LinkWidth, Measure, NetRule, PlaceGroup, Waiver, MEASURES, WAIVER_KINDS};
use pcb_core::model::Role;
use std::collections::HashMap;

/// Text boxes of the table, kept while the window is open so that a comma typed at the end of a list is not eaten when the list is read
/// back (the key is the table, the row and the column).
pub(super) type Buffers = HashMap<(u8, usize, u8), String>;

fn list(text: &str) -> Vec<String> {
    text.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

/// A text box that shows `current` until it is edited; returns the new text when it changed.
fn text_cell(ui: &mut egui::Ui, buf: &mut Buffers, key: (u8, usize, u8), current: &str, width: f32, hover: &str) -> Option<String> {
    let t = buf.entry(key).or_insert_with(|| current.to_string());
    // add_sized: a grid column would otherwise shrink the box to what the first frame left over
    let r = ui.add_sized([width, 20.0], egui::TextEdit::singleline(t)).on_hover_text(hover);
    r.changed().then(|| t.clone())
}

/// The reason of a row: a longer sentence, so a box of two lines that wraps.
fn reason_cell(ui: &mut egui::Ui, buf: &mut Buffers, key: (u8, usize, u8), current: &str, width: f32, hover: &str) -> Option<String> {
    let t = buf.entry(key).or_insert_with(|| current.to_string());
    let r = ui.add_sized([width, 62.0], egui::TextEdit::multiline(t).desired_rows(3).hint_text("なぜ? (理由を一言)")).on_hover_text(hover);
    r.changed().then(|| t.clone())
}

fn number_cell(ui: &mut egui::Ui, v: &mut f64, suffix: &str, speed: f64, hover: &str) {
    ui.add(egui::DragValue::new(v).speed(speed).range(0.0..=1000.0).suffix(suffix).max_decimals(3)).on_hover_text(hover);
}

/// The colours of the rows' frames, one after another.
const FRAME_COLORS: [Color32; 6] = [
    Color32::from_rgb(40, 150, 90),
    Color32::from_rgb(60, 110, 220),
    Color32::from_rgb(200, 120, 20),
    Color32::from_rgb(160, 70, 200),
    Color32::from_rgb(40, 160, 170),
    Color32::from_rgb(200, 70, 110),
];

impl SchEditor {
    /// The box round the parts of the sheet with these references, a little outside them (None when none of them is on this sheet).
    fn frame_of(&self, rect: Rect, refs: &[String]) -> Option<Rect> {
        let mut b: Option<(f32, f32, f32, f32)> = None;
        for c in self.sch.components.iter().filter(|c| refs.contains(&c.refdes)) {
            let Some(r) = super::symbols::sheet_box(c.kind, super::symbols::comp_extents(c), c.pos, c.rot) else { continue };
            b = Some(match b {
                None => r,
                Some(o) => (o.0.min(r.0), o.1.max(r.1), o.2.min(r.2), o.3.max(r.3)),
            });
        }
        let (x0, x1, y0, y1) = b?;
        let m = 1.2;
        Some(Rect::from_two_pos(self.to_screen(rect, (x0 - m, y0 - m)), self.to_screen(rect, (x1 + m, y1 + m))))
    }

    /// The groups of the requirements table as coloured frames with their names, and the parts to keep apart as two frames of one colour
    /// ("A" and "B") with the distance: the intent of the design, readable on the sheet without opening the table. Only the parts on the
    /// sheet that is shown (a net named in a distance is not framed).
    pub(super) fn draw_requirement_frames(&self, painter: &egui::Painter, rect: Rect) {
        if !self.req_frames || self.sch.requirements.is_empty() {
            return;
        }
        let tag = |at: Pos2, text: String, col: Color32| {
            let galley = painter.layout_no_wrap(text, FontId::proportional(12.0), Color32::WHITE);
            let size = galley.size() + Vec2::new(8.0, 3.0);
            // (a tag at the right edge of the sheet is moved left so that it can be read)
            let x = at.x.min(rect.right() - size.x - 2.0).max(rect.left());
            let r = Rect::from_min_size(Pos2::new(x, at.y - galley.size().y - 5.0), size);
            painter.rect_filled(r, 3.0, col);
            painter.galley(r.min + Vec2::new(4.0, 1.5), galley, Color32::WHITE);
        };
        let spec = &self.sch.requirements;
        for (k, g) in spec.groups.iter().enumerate() {
            let col = FRAME_COLORS[k % FRAME_COLORS.len()];
            if let Some(r) = self.frame_of(rect, &g.parts).filter(|r| r.intersects(rect)) {
                painter.rect_filled(r, 4.0, col.gamma_multiply(0.07));
                painter.rect_stroke(r, 4.0, Stroke::new(1.5, col), egui::StrokeKind::Inside);
                let text = if g.max_dist > 0.0 { format!("近くに置く: {} ({} mm 以内)", g.name, g.max_dist) } else { format!("近くに置く: {}", g.name) };
                tag(r.left_top(), text, col);
            }
        }
        for (k, a) in spec.apart.iter().enumerate() {
            let col = FRAME_COLORS[(k + spec.groups.len() + 3) % FRAME_COLORS.len()];
            for (side, names) in [("A", &a.a), ("B", &a.b)] {
                if let Some(r) = self.frame_of(rect, names).filter(|r| r.intersects(rect)) {
                    painter.rect_stroke(r, 4.0, Stroke::new(1.5, col), egui::StrokeKind::Inside);
                    // a dashed look: a second, thinner frame inside
                    painter.rect_stroke(r.shrink(3.0), 3.0, Stroke::new(0.7, col.gamma_multiply(0.6)), egui::StrokeKind::Inside);
                    tag(r.left_top(), format!("離す {side}: {} ({} mm 以上)", a.name, a.min_dist), col);
                }
            }
        }
    }

    pub fn open_requirements(&mut self) {
        self.req_open = true;
        self.req_buf.clear();
        self.req_problems = self.sch.requirement_problems();
    }

    /// Run the DC analysis and make the suggestions for the table from the currents of the nets.
    pub fn requirements_from_dc(&mut self) {
        match self.dc_net_currents() {
            Ok(c) => {
                self.req_suggest = design_spec::suggest_from_currents(&self.sch.requirements, &c).into_iter().map(|s| (true, s)).collect();
                self.req_suggest_msg = if self.req_suggest.is_empty() { "表に足せる電流は、ありませんでした (入れる電流は、すべて書いてあります)".into() } else { format!("{} 件の提案があります", self.req_suggest.len()) };
                self.req_currents = c;
            }
            Err(e) => {
                self.req_suggest.clear();
                self.req_suggest_msg = format!("調べられませんでした: {e}");
            }
        }
    }

    pub fn requirements_window(&mut self, ctx: &egui::Context) {
        if !self.req_open {
            return;
        }
        // the sentences about names the circuit does not have: made again now and then (the netlist is not cheap), and when the table changes
        if ctx.cumulative_frame_nr() % 30 == 0 {
            self.req_problems = self.sch.requirement_problems();
        }
        let mut open = true;
        let mut changed = false;
        let (mut run_dc, mut apply_now) = (false, false);
        let (mut run_bp, mut apply_bp) = (false, false);
        egui::Window::new("設計の要件").open(&mut open).default_size([1200.0, 620.0]).vscroll(true).hscroll(true).show(ctx, |ui| {
            ui.label("回路の「こう作りたい」を、ここに表で書いておきます。全シートで1つの表です。基板に取り込むと、ネットのクラス・電流・部品の組・距離のチェックになります。")
                .on_hover_text("回路図の上に記号を置く方式ではなく、表で持ちます。どのネットがどんな役割で、どの部品を近くに置き、どれとどれを離すかを、あとから読み返せます");
            if !self.req_problems.is_empty() {
                ui.separator();
                for p in &self.req_problems {
                    ui.colored_label(egui::Color32::from_rgb(220, 140, 30), format!("⚠ {p}"));
                }
            }
            ui.checkbox(&mut self.req_frames, "回路図の上に枠を出す").on_hover_text("近くに置く組を色つきの枠で囲み、離す組を A・B の二重の枠で囲んで、名前と距離を書きます。この表を開かなくても、回路図を見た人に設計の意図が伝わります。回路図にある部品だけが囲まれます (ネット名で書いた側は囲みません)");
            let unexplained = self.sch.requirements.unexplained();
            if !unexplained.is_empty() {
                ui.colored_label(egui::Color32::from_rgb(220, 140, 30), format!("⚠ 理由が空の行が {} 件あります: {}", unexplained.len(), unexplained.join("、")))
                    .on_hover_text("数字だけでは、あとで読む人に「なぜその値か」が伝わりません。「理由」の欄に、なぜそうするかを一言書いておきましょう (例: ループを小さくしてノイズを減らすため)");
            }
            ui.separator();
            let buf = &mut self.req_buf;
            let spec = &mut self.sch.requirements;
            egui::CollapsingHeader::new(format!("ネットの役割 ({}) (?)", spec.net_rules.len())).id_salt("req_nets").default_open(true).show(ui, |ui| {
                ui.label("同じ役割・同じ太さのネットを、1行にまとめます。基板に取り込むと、行の名前のネットクラスになります。").on_hover_text("ネット名の末尾に * を付けると、その名前で始まるすべてのネットです (例: GN* は GN1〜GN10)。幅と間隔を 0 にすると、基板の既定のままです。電流を書くと、配線の太さの推奨と電圧降下の計算に使われます");
                let mut remove = None;
                egui::Grid::new("req_net_grid").num_columns(8).spacing([6.0, 4.0]).striped(true).show(ui, |ui| {
                    for h in ["名前", "ネット (, 区切り)", "役割", "線幅", "間隔", "電流", "理由", ""] {
                        ui.strong(h);
                    }
                    ui.end_row();
                    for (k, r) in spec.net_rules.iter_mut().enumerate() {
                        if let Some(t) = text_cell(ui, buf, (0, k, 0), &r.name, 120.0, "この行の名前。基板ではネットクラスの名前になります") {
                            r.name = t;
                            changed = true;
                        }
                        if let Some(t) = text_cell(ui, buf, (0, k, 1), &r.nets.join(","), 230.0, "ネット名を , で区切って書きます。末尾が * なら、その名前で始まるネットすべて") {
                            r.nets = list(&t);
                            changed = true;
                        }
                        let now = Role::parse(&r.role).unwrap_or(Role::None);
                        let mut pick = now;
                        egui::ComboBox::from_id_salt(("req_role", k)).selected_text(now.label()).show_ui(ui, |ui| {
                            for role in [Role::None, Role::Power, Role::Switching, Role::Sense, Role::Analog, Role::Logic] {
                                ui.selectable_value(&mut pick, role, role.label());
                            }
                        })
                        .response
                        .on_hover_text("ネットが何のためのものか。自動配線は、大電流 → スイッチングノード → 検出 → アナログ → ロジックの順に引きます。検出のネットは、ビアなしで引きます");
                        if pick != now {
                            r.role = if pick == Role::None { String::new() } else { pick.name().to_string() };
                            changed = true;
                        }
                        let before = (r.width, r.clearance, r.current);
                        number_cell(ui, &mut r.width, " mm", 0.05, "配線の太さ。0 は基板の既定のまま");
                        number_cell(ui, &mut r.clearance, " mm", 0.05, "ほかの銅との間隔。0 は基板の既定のまま (基板の最小値より小さくはなりません)");
                        number_cell(ui, &mut r.current, " A", 0.05, "このネットに流れる電流の目安。0 は指定なし");
                        changed |= before != (r.width, r.clearance, r.current);
                        if let Some(t) = reason_cell(ui, buf, (0, k, 2), &r.note, 370.0, "なぜその値・その役割にするかの理由。数字は「何を」、理由は「なぜ」を伝えます。あとで読み返す人 (自分も) のために書いておきます") {
                            r.note = t;
                            changed = true;
                        }
                        if ui.button("削除").on_hover_text("この行を消します").clicked() {
                            remove = Some(k);
                        }
                        ui.end_row();
                    }
                });
                if let Some(k) = remove {
                    spec.net_rules.remove(k);
                    buf.clear();
                    changed = true;
                }
                if ui.button("＋ ネットの役割を足す").on_hover_text("新しい行を足します").clicked() {
                    spec.net_rules.push(NetRule { name: format!("役割{}", spec.net_rules.len() + 1), ..Default::default() });
                    buf.clear();
                    changed = true;
                }
                egui::CollapsingHeader::new("詳細: 配線の性質 (順番・ビア・層・長さ) (?)").id_salt("req_props").default_open(spec.net_rules.iter().any(|r| r.order.is_some() || !r.vias.is_empty() || !r.layers.is_empty() || r.max_length > 0.0)).show(ui, |ui| {
                    ui.label("役割がきめている「配線の順番」と「ビアを使うか」と、使う層・長さの上限を、行ごとに決めます。").on_hover_text("役割は、性質のひな形です。電源 0・スイッチング 1・検出 2・アナログ 3・ロジック 4・役割なし 5 の順に、自動配線が引きます (役割なしで太いネットは電源と同じ 0)。検出はビアを使いません。ここで直すと、役割は変えずに、この行だけ別の順番にしたり、検出でもビアを許したり、役割なしでもビアを禁じたりできます。「役割のとおり」にすると、また役割が決めます。
・使う層: 自動配線がこの層だけにトラックを引きます (表面なら top、裏面なら bottom、内層は inner1 ... を , で区切る。空ならどの層でも)。ほかの層に配線があると、基板の点検が指摘します
・長さの上限: 1つのネットのトラックの合計 (mm)。0 なら上限なし");
                    egui::Grid::new("req_props_grid").num_columns(5).spacing([10.0, 4.0]).striped(true).show(ui, |ui| {
                        for h in ["行", "配線の順番 (小さいほど先)", "ビア", "使う層 (, 区切り)", "長さの上限"] {
                            ui.strong(h);
                        }
                        ui.end_row();
                        for (k, r) in spec.net_rules.iter_mut().enumerate() {
                            ui.label(&r.name);
                            let role = Role::parse(&r.role).unwrap_or(Role::None);
                            let by_role = if role == Role::None { Role::Power.priority() } else { role.priority() };
                            let shown = match r.order {
                                None => format!("役割のとおり ({by_role})"),
                                Some(o) => o.to_string(),
                            };
                            egui::ComboBox::from_id_salt(("prop_order", k)).selected_text(shown).width(150.0).show_ui(ui, |ui| {
                                if ui.selectable_label(r.order.is_none(), format!("役割のとおり ({by_role})")).clicked() {
                                    r.order = None;
                                    changed = true;
                                }
                                for o in 0..=9u8 {
                                    if ui.selectable_label(r.order == Some(o), o.to_string()).clicked() {
                                        r.order = Some(o);
                                        changed = true;
                                    }
                                }
                            });
                            let via_text = match r.vias.as_str() {
                                "none" => "使わない",
                                "allow" => "使ってよい",
                                _ => if role == Role::Sense { "役割のとおり (使わない)" } else { "役割のとおり (使ってよい)" },
                            };
                            egui::ComboBox::from_id_salt(("prop_vias", k)).selected_text(via_text).width(190.0).show_ui(ui, |ui| {
                                for (key, label) in [("", "役割のとおり"), ("none", "使わない"), ("allow", "使ってよい")] {
                                    if ui.selectable_label(r.vias == key, label).clicked() {
                                        r.vias = key.to_string();
                                        changed = true;
                                    }
                                }
                            });
                            if let Some(s) = text_cell(ui, buf, (6, k, 0), &r.layers.join(","), 130.0, "この行のネットのトラックを引ける層: top (表面)・bottom (裏面)・inner1 ... を , で区切ります。空なら、どの層でも") {
                                r.layers = list(&s).into_iter().map(|x| x.to_ascii_lowercase()).collect();
                                changed = true;
                            }
                            let before = r.max_length;
                            number_cell(ui, &mut r.max_length, " mm", 1.0, "1つのネットのトラックの長さの合計の上限。0 なら上限なし");
                            changed |= r.max_length != before;
                            ui.end_row();
                        }
                    });
                });
                egui::CollapsingHeader::new("詳細: 部品間の線幅 (?)").id_salt("req_links").default_open(spec.net_rules.iter().any(|r| !r.links.is_empty())).show(ui, |ui| {
                    ui.label("上級者向け。ネットの中で、2つの部品の間の線だけ、別の幅にします。").on_hover_text("例: 電池(CB1)から降圧IC(U7)までは太く 0.6 mm、U7 から LDO(U4)までは細く 0.3 mm。自動配線は、ここに書いた2つの部品の間を先に、この幅で引き、ネットの残りをそのあとにつなぎます。幅 0 は、行の幅のままです");
                    for (k, r) in spec.net_rules.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.strong(&r.name);
                            if ui.small_button("＋ 線を足す").on_hover_text("この行のネットに、部品間の線幅を足します").clicked() {
                                r.links.push(LinkWidth { width: r.width.max(0.25), ..Default::default() });
                                buf.clear();
                                changed = true;
                            }
                        });
                        let mut remove = None;
                        for (j, l) in r.links.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                if let Some(t) = text_cell(ui, buf, (3, k * 100 + j, 0), &l.a, 70.0, "1つめの部品の部品番号") {
                                    l.a = t.trim().to_string();
                                    changed = true;
                                }
                                ui.label("—");
                                if let Some(t) = text_cell(ui, buf, (3, k * 100 + j, 1), &l.b, 70.0, "2つめの部品の部品番号") {
                                    l.b = t.trim().to_string();
                                    changed = true;
                                }
                                let before = l.width;
                                number_cell(ui, &mut l.width, " mm", 0.05, "この2つの部品の間の線の幅。0 は行の幅のまま");
                                changed |= before != l.width;
                                if ui.small_button("削除").on_hover_text("この線の設定を消します").clicked() {
                                    remove = Some(j);
                                }
                            });
                        }
                        if let Some(j) = remove {
                            r.links.remove(j);
                            buf.clear();
                            changed = true;
                        }
                    }
                });
            });
            egui::CollapsingHeader::new(format!("近くに置く部品の組 ({}) (?)", spec.groups.len())).id_salt("req_groups").default_open(true).show(ui, |ui| {
                ui.label("端子のそばの保護、小さくしたいループなど、近くにあるべき部品を組にします。").on_hover_text("基板の自動配置は、組の部品を1つの塊にして並べます。基板の画面では、組が上限より広がっていないかを調べます。上限を 0 にすると、チェックはしません (自動配置では塊にします)");
                let mut remove = None;
                egui::Grid::new("req_group_grid").num_columns(5).spacing([6.0, 4.0]).striped(true).show(ui, |ui| {
                    for h in ["名前", "部品 (, 区切り)", "上限", "理由", ""] {
                        ui.strong(h);
                    }
                    ui.end_row();
                    for (k, g) in spec.groups.iter_mut().enumerate() {
                        if let Some(t) = text_cell(ui, buf, (1, k, 0), &g.name, 120.0, "この組の名前") {
                            g.name = t;
                            changed = true;
                        }
                        if let Some(t) = text_cell(ui, buf, (1, k, 1), &g.parts.join(","), 330.0, "部品番号を , で区切って書きます (例: J3A,ZT1,FP,FQ)") {
                            g.parts = list(&t);
                            changed = true;
                        }
                        let before = g.max_dist;
                        number_cell(ui, &mut g.max_dist, " mm", 0.5, "組の中でいちばん離れた2つの部品の、中心どうしの距離の上限。0 はチェックなし");
                        changed |= before != g.max_dist;
                        if let Some(t) = reason_cell(ui, buf, (1, k, 2), &g.note, 440.0, "なぜ近くに置くかの理由 (例: 電流の輪を小さくしてノイズを減らす)") {
                            g.note = t;
                            changed = true;
                        }
                        if ui.button("削除").on_hover_text("この組を消します").clicked() {
                            remove = Some(k);
                        }
                        ui.end_row();
                    }
                });
                if let Some(k) = remove {
                    spec.groups.remove(k);
                    buf.clear();
                    changed = true;
                }
                if ui.button("＋ 組を足す").on_hover_text("新しい組を足します。部品は、あとから書きます").clicked() {
                    spec.groups.push(PlaceGroup { name: format!("組{}", spec.groups.len() + 1), ..Default::default() });
                    buf.clear();
                    changed = true;
                }
            });
            let (suggest, suggest_msg) = (&mut self.req_suggest, &self.req_suggest_msg);
            egui::CollapsingHeader::new("解析から入れる (?)").id_salt("req_from_dc").default_open(!suggest.is_empty()).show(ui, |ui| {
                ui.label("DC解析で、各ネットに流れる電流を調べて、表に入れます。").on_hover_text("電流が空の行には、その行のネットのうち最大の電流 (2桁に切り上げ) を提案します。どの行にも入っていないネットで 0.1 A 以上流れるものは、行の追加を提案します。表に書いてある電流は、そのままです。チェックを外したものは入れません");
                if ui.button("DC解析の電流を調べる").on_hover_text("いまの回路を DC 解析して、ネットごとの電流から、表への提案を作ります。回路が解析できないときは、理由が出ます").clicked() {
                    run_dc = true;
                }
                if !suggest_msg.is_empty() {
                    ui.label(suggest_msg);
                }
                for (on, s) in suggest.iter_mut() {
                    ui.checkbox(on, &s.text);
                }
                if suggest.iter().any(|(on, _)| *on) && ui.button("選んだものを表に入れる").on_hover_text("チェックの付いた提案を、上の表に書き込みます。あとから自由に直せます").clicked() {
                    apply_now = true;
                }
            });
            let (bp, bp_msg) = (&mut self.req_bp_suggest, &self.req_bp_msg);
            egui::CollapsingHeader::new(format!("バイパスの対応 ({}) (?)", spec.bypass.len())).id_salt("req_bypass").default_open(true).show(ui, |ui| {
                ui.label("どのコンデンサが、どの IC のためのものかを書きます。").on_hover_text("電源のネットが同じだと、回路図のつなぎ方だけでは、コンデンサがどの IC のためのものか分かりません (ラベルでつなぐと、置いた場所も当てになりません)。ここに書くと、基板の自動配置がそのコンデンサを、その IC の電源ピンのそばに置き、点検もその IC のピンからの距離を調べます。書いていないコンデンサは、これまでどおり自動で決めます");
                let mut remove = None;
                egui::Grid::new("req_bypass_grid").num_columns(4).spacing([6.0, 4.0]).striped(true).show(ui, |ui| {
                    for h in ["IC", "コンデンサ (, 区切り)", "理由", ""] {
                        ui.strong(h);
                    }
                    ui.end_row();
                    for (k, b) in spec.bypass.iter_mut().enumerate() {
                        if let Some(t) = text_cell(ui, buf, (3, k, 0), &b.ic, 80.0, "IC の部品番号 (例: U3)") {
                            b.ic = t.trim().to_string();
                            changed = true;
                        }
                        if let Some(t) = text_cell(ui, buf, (3, k, 1), &b.caps.join(","), 230.0, "この IC のバイパスコンデンサの部品番号を , で区切って書きます (例: C5,C6)") {
                            b.caps = list(&t);
                            changed = true;
                        }
                        if let Some(t) = reason_cell(ui, buf, (3, k, 2), &b.note, 440.0, "なぜそのコンデンサがその IC のものか (例: 同じシートで、U3 の電源ピン 8 の隣に置く)") {
                            b.note = t;
                            changed = true;
                        }
                        if ui.button("削除").on_hover_text("この行を消します").clicked() {
                            remove = Some(k);
                        }
                        ui.end_row();
                    }
                });
                if let Some(k) = remove {
                    spec.bypass.remove(k);
                    buf.clear();
                    changed = true;
                }
                ui.horizontal(|ui| {
                    if ui.button("＋ 対応を足す").on_hover_text("新しい行を足します").clicked() {
                        spec.bypass.push(Bypass::default());
                        buf.clear();
                        changed = true;
                    }
                    if ui.button("回路から提案を作る").on_hover_text("対応が書かれていないコンデンサについて、どの IC のものかを回路から推測して、提案します。電源のネットを持つ IC のうち、両側の電源を持つもの → 同じシートのもの → コンデンサの数が少ないもの、の順に決めます。提案ごとに、決めた理由が出ます。違うものはチェックを外してください").clicked() {
                        run_bp = true;
                    }
                });
                if !bp_msg.is_empty() {
                    ui.label(bp_msg);
                }
                for (on, s) in bp.iter_mut() {
                    ui.checkbox(on, &s.text);
                }
                if bp.iter().any(|(on, _)| *on) && ui.button("選んだものを表に入れる").on_hover_text("チェックの付いた提案を、上の表に書き込みます。理由の欄は空で入るので、あとで書いてください").clicked() {
                    apply_bp = true;
                }
            });
            egui::CollapsingHeader::new(format!("離す組 ({}) (?)", spec.apart.len())).id_salt("req_apart").default_open(true).show(ui, |ui| {
                ui.label("降圧のスイッチングノードと、基準・ADC・検出線のように、離しておくものどうしを書きます。").on_hover_text("書くのは部品番号か、ネット名です (部品番号が先に見つかれば部品、なければネットの全パッド。末尾の * も使えます)。基板の画面で、いちばん近い2つのパッドの距離が下限より近くないかを調べます");
                let mut remove = None;
                egui::Grid::new("req_apart_grid").num_columns(6).spacing([6.0, 4.0]).striped(true).show(ui, |ui| {
                    for h in ["名前", "A (, 区切り)", "B (, 区切り)", "下限", "理由", ""] {
                        ui.strong(h);
                    }
                    ui.end_row();
                    for (k, a) in spec.apart.iter_mut().enumerate() {
                        if let Some(t) = text_cell(ui, buf, (2, k, 0), &a.name, 200.0, "この行の名前") {
                            a.name = t;
                            changed = true;
                        }
                        if let Some(t) = text_cell(ui, buf, (2, k, 1), &a.a.join(","), 200.0, "部品番号かネット名を , で区切って書きます") {
                            a.a = list(&t);
                            changed = true;
                        }
                        if let Some(t) = text_cell(ui, buf, (2, k, 2), &a.b.join(","), 200.0, "部品番号かネット名を , で区切って書きます") {
                            a.b = list(&t);
                            changed = true;
                        }
                        let before = a.min_dist;
                        number_cell(ui, &mut a.min_dist, " mm", 0.5, "AとBのパッドどうしの、中心間の距離の下限");
                        changed |= before != a.min_dist;
                        if let Some(t) = reason_cell(ui, buf, (2, k, 3), &a.note, 440.0, "なぜ離すかの理由 (例: スイッチングの雑音を帰還に入れない)") {
                            a.note = t;
                            changed = true;
                        }
                        if ui.button("削除").on_hover_text("この行を消します").clicked() {
                            remove = Some(k);
                        }
                        ui.end_row();
                    }
                });
                if let Some(k) = remove {
                    spec.apart.remove(k);
                    buf.clear();
                    changed = true;
                }
                if ui.button("＋ 離す組を足す").on_hover_text("新しい行を足します").clicked() {
                    spec.apart.push(KeepApart { name: format!("離す{}", spec.apart.len() + 1), min_dist: 10.0, ..Default::default() });
                    buf.clear();
                    changed = true;
                }
            });
            egui::CollapsingHeader::new(format!("道筋の測定 ({}) (?)", spec.measures.len())).id_salt("req_measures").default_open(!spec.measures.is_empty()).show(ui, |ui| {
                ui.label("配線した銅の道筋 (トラックとビア) の長さ・ビア数・分かれ目を測って、上限か下限を決めます。").on_hover_text("基板に配線したあとの点検です。測るのは、2つのパッドの間をトラックとビアでたどった道筋で、ベタは通りません (ベタだけでつながっていると、道筋なしになります)。
・長さ: 道筋の長さ (mm)
・ビアの数: 道筋のビアの数 (検出線は 0 に)
・分かれ目: 1つ目のパッドから、2つ目・3つ目のパッドへ行く道筋が、一緒に走る長さ (mm)。電流の線と電圧を測る線が、パッドの根元で分かれているかを見るときに (ケルビン接続: 0.5 mm 以下など)
パッドは「部品.ピン」の形で書きます (例: RS1.1)");
                let mut remove = None;
                egui::Grid::new("req_measures_grid").num_columns(6).spacing([6.0, 4.0]).striped(true).show(ui, |ui| {
                    for h in ["名前", "測るもの", "パッド・ネット (, 区切り)", "上限 / 下限", "理由", ""] {
                        ui.strong(h);
                    }
                    ui.end_row();
                    for (k, m) in spec.measures.iter_mut().enumerate() {
                        if let Some(t) = text_cell(ui, buf, (5, k, 0), &m.name, 120.0, "この測定の名前") {
                            m.name = t;
                            changed = true;
                        }
                        let label = MEASURES.iter().find(|(key, ..)| *key == m.what).map_or("(選ぶ)", |(_, l, ..)| *l);
                        egui::ComboBox::from_id_salt(("measure_what", k)).selected_text(label).width(150.0).show_ui(ui, |ui| {
                            for (key, l, unit, need) in MEASURES {
                                if ui.selectable_label(m.what == *key, format!("{l} ({unit})")).on_hover_text(format!("パッドが {need} 個要ります")).clicked() {
                                    m.what = (*key).to_string();
                                    changed = true;
                                }
                            }
                        });
                        let need = m.kind().map_or(2, |k| k.2);
                        let hint = if m.what == "area" { "銅の面積を測るパッド 1 個 (部品.ピン: 例 Q1.2)。そのパッドにつながった銅 (配線・ビア・ベタ) の面積です。放熱のために、データシートが求める銅の面積があるかを見ます" } else if m.what == "guard" { "1つ目: ガードされる検出のネット、2つ目: ガードのネット (名前か、末尾に * を付けた名前。+ で複数 : 例 A+SA,GRDA)" } else if m.what == "gap" { "間隔を測る2つのネット (名前か、末尾に * を付けた名前。+ で複数。例: SA,SW*)" } else if m.what == "parallel" { "1つ目: 配線の長さを数えるネット、2つ目: それに近づくネット (名前か、末尾に * を付けた名前。+ で複数。例: SA+SB,BKSW)。2つ目の銅が、1つ目の配線の縁から「近さ」以内にある長さを測る" } else if m.what == "thermal_vias" { "ビアを数えるパッド 1 個 (部品.ピン: 例 Q1.2)。そのパッドの中にあるビアの数です。放熱のパッドの下のサーマルビアが足りているかを見ます" } else if m.what == "loop" { "4つのパッド: 1つ目と2つ目 = 行きの道筋 (例: U7.3,CB3.1 = IC の電源ピンからコンデンサのプラスへ)、3つ目と4つ目 = 帰りの道筋 (例: CB3.2,U7.4 = コンデンサのマイナスから IC のグラウンドピンへ)。行きと帰りの配線の長さの合計を測る (小さいほどループが小さい)" } else if m.what == "match" { "4つのパッド: 1つ目と2つ目 = 1本目の道筋、3つ目と4つ目 = 2本目の道筋。2本の配線の長さの差を測る (対になる経路をそろえたいときに。差が小さいほど対称)" } else if m.what == "ring" { "1つ目: 囲まれるパッド (例: U4.8)、2つ目: ガードのネット (名前か、末尾に * を付けた名前。+ で複数 : 例 VCC)。パッドのまわり 360° のうち、縁から「近さ」以内にガードの銅がある割合を測る (配線が出ていく所は隙間になる)" } else if m.what == "resistance" { "2つのパッド (部品.ピン: 例 RS1.1,SW1.2)。その間の配線 (トラックとビア) の直流の抵抗を測る。銅の厚さは 35 µm (1 oz) で計算。ベタは通らない道筋だけを数える" } else if m.what == "under" { "1つ目と2つ目: 調べる道筋の両端のパッド (例: U1.5,J1.2)、3つ目: グラウンドのネット (名前か、末尾に * を付けた名前。例 GND)。その道筋の下 (すぐ上か下の層) に、グラウンドの銅 (ベタ・配線) がある割合。信号の帰り電流の通り道が切れていないかを見る" } else if m.what == "overlap" { "重なりを測る2つのネット (名前か、末尾に * を付けた名前。+ で複数。例: SW,SA+SB)。別の層で上下に重なっている銅の面積。雑音の多い配線の真上や真下を、敏感な配線が通っていないかを見る" } else if m.what == "width" { "配線の太さを調べる道筋の、出発のパッドと行き先のパッド (例: Q1.3,RS1.1)。その道筋の中で一番細い所の太さを測る" } else if m.what == "tap" { "1つ目: 実装用パッド (例: RS1.1)、2つ目: 検出の線の行き先のパッド (例: SS1.2)、3つ目: 抵抗のもう片方のパッド (例: RS1.2)。検出の線がパッドの銅につながる点を、もう片方のパッドに向いた縁から測る (0 = 内側の縁)" } else if need == 3 { "1つ目: 道筋の出発のパッド、2つ目と3つ目: 行き先のパッド (例: RS1.1,Q1.3,UAB.2)" } else { "出発のパッドと、行き先のパッド (例: RS1.1,UAB.2)" };
                        if let Some(t) = text_cell(ui, buf, (5, k, 1), &m.pads.join(","), 230.0, hint) {
                            m.pads = list(&t);
                            changed = true;
                        }
                        if m.what == "gap" || m.what == "parallel" {
                            if let Some(t) = text_cell(ui, buf, (5, k, 3), &m.except.join(","), 230.0, "間隔から除く部品 (コンマ区切り)。その部品のパッドは数えません。pads と書くと、全部のパッドを除いて、配線とビアだけの間隔を見ます。IC のピンどうしは近くて当然なので、配線の間隔を見たいときに使います") {
                                m.except = list(&t);
                                changed = true;
                            }
                        }
                        ui.horizontal(|ui| {
                            let mut min = m.is_min();
                            egui::ComboBox::from_id_salt(("measure_op", k)).selected_text(if min { "以上" } else { "以下" }).width(60.0).show_ui(ui, |ui| {
                                if ui.selectable_label(!min, "以下 (上限)").clicked() {
                                    min = false;
                                }
                                if ui.selectable_label(min, "以上 (下限)").clicked() {
                                    min = true;
                                }
                            });
                            let op = if min { "min" } else { "max" };
                            if m.op != op {
                                m.op = op.to_string();
                                changed = true;
                            }
                            if m.what == "area" {
                                let before = m.reach;
                                number_cell(ui, &mut m.reach, " mm 以内 (0 = 全部)", 0.5, "パッドの中心からこの半径の中の銅だけを数えます。0 なら、つながった銅の全部 (遠くへ伸びる配線も入る)。部品の熱が広がるのは近くの銅なので、10 mm などにすると放熱の目安になります");
                                changed |= m.reach != before;
                            }
                            if m.what == "guard" || m.what == "parallel" || m.what == "ring" {
                                let mut reach = m.guard_reach();
                                let before = reach;
                                number_cell(ui, &mut reach, " mm 以内", 0.1, "ガードの縁が、検出のトラックの縁から、この距離までにあれば「ガードがある」とみなします (空けた隙間 + ガードの幅の目安。0.35 mm の隙間・0.3 mm 幅なら 1 mm)。検出のトラックの両側にあるか、パッドの 2 mm 以内を除いた長さで数えます");
                                if reach != before {
                                    m.reach = reach;
                                    changed = true;
                                }
                            }
                            let before = m.limit;
                            let unit = m.kind().map_or("", |k| match k.1 { "個" => " 個", "%" => " %", "mm²" => " mm²", _ => " mm" });
                            number_cell(ui, &mut m.limit, unit, 0.1, "この値を超えたら (または下回ったら) 指摘します");
                            changed |= m.limit != before;
                        });
                        if let Some(t) = reason_cell(ui, buf, (5, k, 2), &m.note, 320.0, "なぜこの限度か (例: 電流の線と電圧を測る線は、パッドの根元で分けるため)") {
                            m.note = t;
                            changed = true;
                        }
                        if ui.button("削除").on_hover_text("この行を消します").clicked() {
                            remove = Some(k);
                        }
                        ui.end_row();
                    }
                });
                if let Some(k) = remove {
                    spec.measures.remove(k);
                    buf.clear();
                    changed = true;
                }
                if ui.button("＋ 測定を足す").on_hover_text("新しい行を足します").clicked() {
                    spec.measures.push(Measure { name: format!("測定{}", spec.measures.len() + 1), what: "length".into(), op: "max".into(), limit: 10.0, ..Default::default() });
                    buf.clear();
                    changed = true;
                }
            });
            egui::CollapsingHeader::new(format!("承知した例外 ({}) (?)", spec.waivers.len())).id_salt("req_waivers").default_open(!spec.waivers.is_empty()).show(ui, |ui| {
                ui.label("基板の点検で指摘されても、承知の上でそのままにするものを、理由つきで書きます。").on_hover_text("基板の点検 (組の広がり・離す距離・バイパスの距離) の指摘のうち、ここに書いたものは「違反」に数えず、「承知した例外」として理由つきで別に出ます。あとで読む人が、直し忘れではなく承知の上だと分かります。基板の点検の一覧の「承知する」ボタンからも書き込めます");
                let mut remove = None;
                egui::Grid::new("req_waivers_grid").num_columns(5).spacing([6.0, 4.0]).striped(true).show(ui, |ui| {
                    for h in ["種類", "行の名前", "コンデンサ", "理由", ""] {
                        ui.strong(h);
                    }
                    ui.end_row();
                    for (k, w) in spec.waivers.iter_mut().enumerate() {
                        let label = WAIVER_KINDS.iter().find(|(key, _)| *key == w.kind).map_or("(選ぶ)", |(_, l)| *l);
                        egui::ComboBox::from_id_salt(("waiver_kind", k)).selected_text(label).width(90.0).show_ui(ui, |ui| {
                            for (key, l) in WAIVER_KINDS {
                                if ui.selectable_label(w.kind == *key, *l).on_hover_text("どの種類の指摘を承知するか").clicked() {
                                    w.kind = (*key).to_string();
                                    changed = true;
                                }
                            }
                        });
                        if let Some(t) = text_cell(ui, buf, (4, k, 0), &w.name, 160.0, "表の行の名前です。組なら組の名前、離す距離なら行の名前、バイパスなら IC の部品番号") {
                            w.name = t;
                            changed = true;
                        }
                        if w.kind == "bypass" {
                            if let Some(t) = text_cell(ui, buf, (4, k, 1), &w.item, 70.0, "承知するコンデンサの部品番号。空なら、その IC の行のコンデンサすべて") {
                                w.item = t.trim().to_string();
                                changed = true;
                            }
                        } else {
                            ui.label("");
                        }
                        if let Some(t) = reason_cell(ui, buf, (4, k, 2), &w.note, 380.0, "なぜ承知するか (例: 基板が小さく、これ以上近づけられない)。空だと「理由のない行」に数えられます") {
                            w.note = t;
                            changed = true;
                        }
                        if ui.button("削除").on_hover_text("この例外を消します。基板の点検で、また指摘に戻ります").clicked() {
                            remove = Some(k);
                        }
                        ui.end_row();
                    }
                });
                if let Some(k) = remove {
                    spec.waivers.remove(k);
                    buf.clear();
                    changed = true;
                }
                if ui.button("＋ 例外を足す").on_hover_text("新しい行を足します。種類・行の名前・理由を書きます").clicked() {
                    spec.waivers.push(Waiver { kind: "apart".into(), ..Default::default() });
                    buf.clear();
                    changed = true;
                }
            });
        });
        if run_dc {
            self.requirements_from_dc();
        }
        if run_bp {
            self.req_bp_suggest = self.sch.bypass_suggestions().into_iter().map(|s| (true, s)).collect();
            self.req_bp_msg = if self.req_bp_suggest.is_empty() { "提案できるコンデンサは、ありませんでした (すべて書いてあるか、電源につながる IC が見つかりません)".into() } else { format!("{} 件の提案があります", self.req_bp_suggest.len()) };
        }
        if apply_bp {
            for (on, s) in std::mem::take(&mut self.req_bp_suggest) {
                if on {
                    s.apply(&mut self.sch.requirements);
                }
            }
            self.req_buf.clear();
            self.req_bp_suggest = self.sch.bypass_suggestions().into_iter().map(|s| (true, s)).collect();
            self.req_bp_msg = "表に入れました".into();
            changed = true;
        }
        if apply_now {
            for (on, s) in std::mem::take(&mut self.req_suggest) {
                if on {
                    s.apply(&mut self.sch.requirements);
                }
            }
            self.req_buf.clear();
            self.req_suggest = design_spec::suggest_from_currents(&self.sch.requirements, &self.req_currents).into_iter().map(|s| (true, s)).collect();
            self.req_suggest_msg = "表に入れました".into();
            changed = true;
        }
        if changed {
            self.req_problems = self.sch.requirement_problems();
        }
        self.req_open = open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_is_read_back_without_blanks() {
        assert_eq!(list("A, B ,,GN*"), vec!["A", "B", "GN*"]);
        assert!(list(" , ").is_empty());
    }

    #[test]
    fn the_table_survives_a_round_trip_through_the_file() {
        let mut s = Schematic::default();
        s.requirements.groups.push(PlaceGroup { name: "入口".into(), parts: vec!["J3A".into(), "ZT1".into()], max_dist: 40.0, note: "保護".into() });
        s.requirements.net_rules.push(NetRule { name: "検出".into(), nets: vec!["SA".into()], role: "sense".into(), width: 0.25, ..Default::default() });
        let back: Schematic = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.requirements, s.requirements);
        assert!(!serde_json::to_string(&Schematic::default()).unwrap().contains("requirements"), "an empty table is not written");
    }
}
