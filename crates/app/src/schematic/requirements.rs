//! "設計の要件" window: one table for the whole design (all sheets) of what the board has to do for the circuit: what the nets are for
//! (role, width, clearance, current), which parts belong close together, and what keeps its distance. The board import takes it over
//! (net classes, currents, placement groups, distances); the board editor checks the placement against it.

use super::*;
use design_spec::{KeepApart, LinkWidth, NetRule, PlaceGroup};
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

fn number_cell(ui: &mut egui::Ui, v: &mut f64, suffix: &str, speed: f64, hover: &str) {
    ui.add(egui::DragValue::new(v).speed(speed).range(0.0..=1000.0).suffix(suffix).max_decimals(3)).on_hover_text(hover);
}

impl SchEditor {
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
        egui::Window::new("設計の要件").open(&mut open).default_size([1040.0, 520.0]).vscroll(true).hscroll(true).show(ctx, |ui| {
            ui.label("回路の「こう作りたい」を、ここに表で書いておきます。全シートで1つの表です。基板に取り込むと、ネットのクラス・電流・部品の組・距離のチェックになります。")
                .on_hover_text("回路図の上に記号を置く方式ではなく、表で持ちます。どのネットがどんな役割で、どの部品を近くに置き、どれとどれを離すかを、あとから読み返せます");
            if !self.req_problems.is_empty() {
                ui.separator();
                for p in &self.req_problems {
                    ui.colored_label(egui::Color32::from_rgb(220, 140, 30), format!("⚠ {p}"));
                }
            }
            ui.separator();
            let buf = &mut self.req_buf;
            let spec = &mut self.sch.requirements;
            egui::CollapsingHeader::new(format!("ネットの役割 ({}) (?)", spec.net_rules.len())).id_salt("req_nets").default_open(true).show(ui, |ui| {
                ui.label("同じ役割・同じ太さのネットを、1行にまとめます。基板に取り込むと、行の名前のネットクラスになります。").on_hover_text("ネット名の末尾に * を付けると、その名前で始まるすべてのネットです (例: GN* は GN1〜GN10)。幅と間隔を 0 にすると、基板の既定のままです。電流を書くと、配線の太さの推奨と電圧降下の計算に使われます");
                let mut remove = None;
                egui::Grid::new("req_net_grid").num_columns(8).spacing([6.0, 4.0]).striped(true).show(ui, |ui| {
                    for h in ["名前", "ネット (, 区切り)", "役割", "線幅", "間隔", "電流", "メモ", ""] {
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
                        if let Some(t) = text_cell(ui, buf, (0, k, 2), &r.note, 260.0, "なぜそうするかの一言。あとで読み返す人のためのメモ") {
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
                    for h in ["名前", "部品 (, 区切り)", "上限", "メモ", ""] {
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
                        if let Some(t) = text_cell(ui, buf, (1, k, 2), &g.note, 300.0, "なぜ近くに置くかの一言") {
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
            egui::CollapsingHeader::new(format!("離す組 ({}) (?)", spec.apart.len())).id_salt("req_apart").default_open(true).show(ui, |ui| {
                ui.label("降圧のスイッチングノードと、基準・ADC・検出線のように、離しておくものどうしを書きます。").on_hover_text("書くのは部品番号か、ネット名です (部品番号が先に見つかれば部品、なければネットの全パッド。末尾の * も使えます)。基板の画面で、いちばん近い2つのパッドの距離が下限より近くないかを調べます");
                let mut remove = None;
                egui::Grid::new("req_apart_grid").num_columns(6).spacing([6.0, 4.0]).striped(true).show(ui, |ui| {
                    for h in ["名前", "A (, 区切り)", "B (, 区切り)", "下限", "メモ", ""] {
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
                        if let Some(t) = text_cell(ui, buf, (2, k, 3), &a.note, 260.0, "なぜ離すかの一言") {
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
        });
        if run_dc {
            self.requirements_from_dc();
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
