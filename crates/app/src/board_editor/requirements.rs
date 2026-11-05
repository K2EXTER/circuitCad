//! The panel's list of the design's requirements that the placement does not meet (the groups and distances the schematic's table gave
//! the board on import; see `pcb_core::requirements`), with a button that selects the parts of a group that is too spread out.

use super::*;
use pcb_core::requirements::FindingKind;

/// Dark outline under the requirement frames and lines, so they stand out from the copper colours (top red, inner green, bottom blue).
const HALO: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 200);

/// The ends of a line pushed out by 2 pixels: the dark strip under a line has flat ends, and the round end of the coloured one must not reach past it.
fn longer(pts: &[Pos2]) -> Vec<Pos2> {
    let mut v = pts.to_vec();
    if let [a, b, ..] = pts {
        v[0] = *a + (*a - *b).normalized() * 2.0;
    }
    if let [.., a, b] = pts {
        let n = v.len();
        v[n - 1] = *b + (*b - *a).normalized() * 2.0;
    }
    v
}

impl BoardEditor {
    /// The groups as frames (green when the farthest two parts are within the limit, pink when not, a tag with the name and the numbers) and the
    /// distances to keep as a line between the nearest two pads (green when long enough, pink when too short, the distance at the middle).
    pub(super) fn draw_requirement_marks(&self, painter: &egui::Painter, rect: Rect) {
        if (!self.show_req_marks && !self.show_req_lines) || !self.vis.parts || !pcb_core::requirements::any(&self.board) {
            return;
        }
        let marks = self.cached_marks();
        let boxes = self.part_boxes();
        let (good, bad) = (crate::theme::OK, Color32::from_rgb(255, 60, 200));
        // not kept, but the table accepts it (a waiver with its reason)
        let accepted = crate::theme::WARN;
        // the frames and lines go into one mesh and are handed over before each tag, so that the tags lie over them
        let mut mesh = egui::Mesh::default();
        // a dark halo under every line and frame: the top copper is red and the inner layers green, so the colour alone is not enough
        for g in marks.groups.iter().filter(|_| self.show_req_marks) {
            let col = if g.ok { good } else if g.waived { accepted } else { bad };
            let (mut lo, mut hi) = (Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN));
            for (a, b) in g.parts.iter().filter_map(|r| boxes.get(r.as_str()).copied()) {
                lo = Point::new(lo.x.min(a.x), lo.y.min(a.y));
                hi = Point::new(hi.x.max(b.x), hi.y.max(b.y));
            }
            if lo.x > hi.x {
                continue;
            }
            let m = 0.8;
            let r = Rect::from_two_pos(self.to_screen(rect, Point::new(lo.x - m, lo.y - m)), self.to_screen(rect, Point::new(hi.x + m, hi.y + m)));
            if !r.intersects(rect) {
                continue;
            }
            ribbon::add_frame(&mut mesh, r.expand(2.25), 4.5, HALO);
            ribbon::add_frame(&mut mesh, r.expand(1.0), 2.0, col);
            ribbon::flush(painter, &mut mesh);
            let text = if g.limit > 0.0 { format!("組「{}」 {:.1} / {} mm{}", g.name, g.actual, g.limit, if g.waived && !g.ok { " (承知)" } else { "" }) } else { format!("組「{}」", g.name) };
            let galley = painter.layout_no_wrap(text, egui::FontId::proportional(12.0), Color32::BLACK);
            let size = galley.size() + Vec2::new(8.0, 3.0);
            let x = r.left().min(rect.right() - size.x - 2.0).max(rect.left());
            let tag = Rect::from_min_size(Pos2::new(x, r.top() - galley.size().y - 6.0), size);
            painter.rect_filled(tag, 3.0, col);
            painter.galley(tag.min + Vec2::new(4.0, 1.5), galley, Color32::BLACK);
        }
        // a bypass capacitor and the IC pin the table says it serves: a thin line (green when near enough, red when far)
        for b in marks.bypass.iter().filter(|_| self.show_req_lines) {
            let col = if b.ok { good } else if b.waived { accepted } else { bad };
            let (p, q) = (self.to_screen(rect, b.a), self.to_screen(rect, b.b));
            ribbon::add_flat_chain(&mut mesh, &longer(&[p, q]), if b.ok { 3.0 } else { 4.5 }, HALO);
            ribbon::add_chain(&mut mesh, &[p, q], if b.ok { 1.0 } else { 2.0 }, col);
            ribbon::add_ring(&mut mesh, q, 4.0, 3.5, HALO, true);
            ribbon::add_ring(&mut mesh, q, 4.0, 1.5, col, false);
            ribbon::flush(painter, &mut mesh);
            if !b.ok || self.zoom >= 14.0 {
                let galley = painter.layout_no_wrap(format!("{}→{} {:.1} mm{}", b.cap, b.ic, b.actual, if b.waived && !b.ok { " (承知)" } else { "" }), egui::FontId::proportional(11.0), Color32::BLACK);
                let tag = Rect::from_center_size(p.lerp(q, 0.5), galley.size() + Vec2::new(6.0, 2.0));
                painter.rect_filled(tag, 3.0, col);
                painter.galley(tag.min + Vec2::new(3.0, 1.0), galley, Color32::BLACK);
            }
        }
        // the copper of a connection that the table limits: the way along the tracks (two ways and the place they part for a fork)
        // the ones that are fine first, so that what is wrong lies on top (and the tags of one place sit in a column, not on each other)
        let answer = self.cached_requirements();
        let measured = &answer.1;
        let mut order: Vec<(usize, &pcb_core::requirements::MeasureMark)> = measured.iter().enumerate().filter(|_| self.show_req_lines).collect();
        order.sort_by_key(|(_, m)| !m.ok);
        for (row, m) in order {
            let col = if m.ok { good } else if m.waived { accepted } else { bad };
            let wide = if m.ok { 1.5 } else { 3.0 };
            for way in &m.ways {
                // a way is one strip (the halo first, then the colour), not a line for every piece
                let pts: Vec<Pos2> = way.iter().map(|p| self.to_screen(rect, *p)).collect();
                ribbon::add_flat_chain(&mut mesh, &longer(&pts), wide + 2.0, HALO);
                ribbon::add_chain(&mut mesh, &pts, wide, col);
            }
            if let Some(at) = m.at {
                let c = self.to_screen(rect, at);
                ribbon::add_ring(&mut mesh, c, 7.0, 4.0, HALO, true);
                ribbon::add_ring(&mut mesh, c, 7.0, 2.0, col, false);
            }
            ribbon::flush(painter, &mut mesh);
            if !m.ok || self.zoom >= 14.0 {
                let anchor = m.at.or_else(|| m.ways.first().and_then(|w| w.get(w.len() / 2).copied()));
                if let Some(a) = anchor {
                    let text = format!("{}: {:.*} {}{}", m.name, match m.unit.as_str() { "個" => 0, "%" => 1, _ => 2 }, m.value, m.unit, if m.waived && !m.ok { " (承知)" } else { "" });
                    let galley = painter.layout_no_wrap(text, egui::FontId::proportional(11.0), Color32::BLACK);
                    let tag = Rect::from_min_size(self.to_screen(rect, a) + Vec2::new(8.0, -8.0 + 15.0 * (row % 6) as f32), galley.size() + Vec2::new(6.0, 2.0));
                    painter.rect_filled(tag, 3.0, col);
                    painter.galley(tag.min + Vec2::new(3.0, 1.0), galley, Color32::BLACK);
                }
            }
        }
        for a in marks.apart.iter().filter(|_| self.show_req_lines) {
            let col = if a.ok { good } else if a.waived { accepted } else { bad };
            let (p, q) = (self.to_screen(rect, a.a), self.to_screen(rect, a.b));
            ribbon::add_flat_chain(&mut mesh, &longer(&[p, q]), if a.ok { 3.0 } else { 4.5 }, HALO);
            ribbon::add_chain(&mut mesh, &[p, q], if a.ok { 1.0 } else { 2.0 }, col);
            // a one-point chain is a disc
            for e in [p, q] {
                ribbon::add_chain(&mut mesh, &[e], 9.0, HALO);
                ribbon::add_chain(&mut mesh, &[e], 6.0, col);
            }
            ribbon::flush(painter, &mut mesh);
            let text = if a.ok { format!("「{}」 {:.1} mm (下限 {})", a.name, a.actual, a.limit) } else { format!("「{}」 {:.1} mm しかない (下限 {}){}", a.name, a.actual, a.limit, if a.waived { " (承知)" } else { "" }) };
            let galley = painter.layout_no_wrap(text, egui::FontId::proportional(12.0), Color32::BLACK);
            let tag = Rect::from_center_size(p.lerp(q, 0.5), galley.size() + Vec2::new(8.0, 3.0));
            painter.rect_filled(tag, 3.0, col);
            painter.galley(tag.min + Vec2::new(4.0, 1.5), galley, Color32::BLACK);
        }
    }

    /// The schematic's table, row by row, with whether the board has taken each row (read only: it is edited in the schematic, so
    /// that the table has one owner). A net rule counts as taken when a class of its name holds nets of the board.
    pub(super) fn requirements_table(&mut self, ui: &mut egui::Ui, spec: &design_spec::DesignSpec) {
        if spec.net_rules.is_empty() && spec.groups.is_empty() && spec.apart.is_empty() && spec.bypass.is_empty() && spec.measures.is_empty() {
            return;
        }
        let b = &self.board;
        // (kind, name, what it says, taken, why)
        let mut rows: Vec<(&str, String, String, bool, &str)> = vec![];
        for r in &spec.net_rules {
            let got = !r.name.is_empty() && b.net_classes.iter().any(|c| c.name == r.name && !c.nets.is_empty());
            let mut what = r.nets.join(", ");
            if r.width > 0.0 {
                what += &format!(" / 幅 {} mm", r.width);
            }
            if r.current > 0.0 {
                what += &format!(" / {} A", r.current);
            }
            rows.push(("ネット", r.name.clone(), what, got, &r.note));
        }
        for g in &spec.groups {
            let what = format!("{}{}", g.parts.join(", "), if g.max_dist > 0.0 { format!(" / {} mm 以内", g.max_dist) } else { String::new() });
            rows.push(("組", g.name.clone(), what, b.place_groups.iter().any(|x| x == g), &g.note));
        }
        for a in &spec.apart {
            let what = format!("{} と {} / {} mm 以上", a.a.join(", "), a.b.join(", "), a.min_dist);
            rows.push(("離す", a.name.clone(), what, b.keep_apart.iter().any(|x| x == a), &a.note));
        }
        for x in &spec.bypass {
            rows.push(("バイパス", x.ic.clone(), x.caps.join(", "), b.bypass.iter().any(|y| y == x), &x.note));
        }
        for m in &spec.measures {
            let what = format!("{} {} {}", m.what, if m.is_min() { "≥" } else { "≤" }, m.limit);
            rows.push(("測定", m.name.clone(), what, b.measures.iter().any(|y| y == m), &m.note));
        }
        let lost = rows.iter().filter(|r| !r.3).count();
        let title = if lost == 0 { format!("設計の要件の表 ({} 行)", rows.len()) } else { format!("設計の要件の表 ({} 行、基板に届かない {lost} 行)", rows.len()) };
        let mut open_sch = false;
        egui::CollapsingHeader::new(title).id_salt("req_table").default_open(lost > 0).show(ui, |ui| {
            for (kind, name, what, got, why) in &rows {
                ui.horizontal_wrapped(|ui| {
                    if *got {
                        ui.colored_label(crate::theme::OK, "✔").on_hover_text("基板に取り込まれています");
                    } else {
                        ui.colored_label(crate::theme::ERROR, "✖").on_hover_text("基板に届いていません。ネットのパッドが基板にない、部品を取り込んでいない、などが考えられます。名前を確かめて、回路図から取り込み直してください");
                    }
                    ui.label(format!("{kind}「{name}」")).on_hover_text(if why.trim().is_empty() { "(理由が書かれていません)" } else { why });
                    ui.label(egui::RichText::new(what).small().color(Color32::GRAY));
                });
            }
            if ui.small_button("回路図で表を開く").on_hover_text("表は回路図の側で直します (基板と回路図で食い違わないように、ここは見るだけです)。直すと基板に自動で取り込まれます").clicked() {
                open_sch = true;
            }
        });
        if open_sch {
            self.open_sch_requirements = true;
        }
    }

    pub(super) fn requirements_check(&mut self, ui: &mut egui::Ui) {
        if !pcb_core::requirements::any(&self.board) {
            return;
        }
        let mut both = self.show_req_marks || self.show_req_lines;
        if ui.checkbox(&mut both, "組と離す距離を基板の上に出す").on_hover_text("近くに置く組を枠で囲み (守れていれば緑、広がりすぎならピンク)、離す組の最も近い2つのパッドを線で結んで距離を書きます (足りなければピンク)。回路図の「設計の要件」の表が、基板の上で見えます。枠と線は、右上の「物の表示とロック」で別々に切り替えられます").changed() {
            (self.show_req_marks, self.show_req_lines) = (both, both);
        }
        let answer = self.cached_requirements();
        let rep = &answer.0;
        let orange = crate::theme::WARN;
        if rep.findings.is_empty() {
            ui.label(format!("✔ 設計の要件を満たしています ({} 件を確認)", rep.checked)).on_hover_text("回路図の「設計の要件」の表にある、近くに置く組と、離す組を調べました。上限や下限の0のものは調べません");
        } else {
            ui.colored_label(orange, format!("設計の要件を満たしていない所: {} 件", rep.findings.len())).on_hover_text("回路図の「設計の要件」の表にある、近くに置く組の広がりと、離す組の近さです。部品を動かすか、表の数字を見直してください");
            ui.horizontal(|ui| {
                ui.label("承知する理由");
                ui.add(egui::TextEdit::singleline(&mut self.waive_reason).hint_text("なぜこのままでよいか").desired_width(220.0))
                    .on_hover_text("下の「承知する」を押すときの理由です。回路図の「設計の要件」の表の「承知した例外」に、理由つきで書き込まれます。理由が空だと押せません (あとで読む人が、なぜかを知るため)");
            });
            for f in &rep.findings {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(orange, &f.message);
                    let can = !self.waive_reason.trim().is_empty();
                    if ui.add_enabled(can, egui::Button::new("承知する").small()).on_hover_text("この指摘を、理由つきで「承知した例外」にします。点検の件数に数えず、基板の上では黄土色で出ます。理由を上の欄に書いてから押します。回路図の側の表にも書き込まれます").clicked() {
                        let w = design_spec::Waiver { kind: f.kind.key().into(), name: f.name.clone(), item: f.item.clone(), note: self.waive_reason.trim().to_string() };
                        self.checkpoint();
                        self.board.waivers.push(w.clone());
                        self.waiver_request = Some((true, w));
                        self.status = "承知した例外にしました。回路図の表にも書きました".into();
                    }
                    if f.kind == FindingKind::Group && ui.small_button("組を選ぶ").on_hover_text("この組の部品をまとめて選びます。どこに散らばっているかが見えます").clicked() {
                        if let Some(g) = self.board.place_groups.iter().find(|g| g.name == f.name) {
                            self.multi = self.board.parts.iter().enumerate().filter(|(_, p)| g.parts.contains(&p.refdes)).map(|(i, _)| Item::Part(i)).collect();
                            self.selected = None;
                            self.status = format!("組「{}」の部品 {} 個を選びました", g.name, self.multi.len());
                        }
                    }
                });
            }
        }
        if !rep.waived.is_empty() {
            egui::CollapsingHeader::new(format!("承知した例外 ({})", rep.waived.len())).id_salt("req_waived").default_open(false).show(ui, |ui| {
                for (f, why) in &rep.waived {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(&f.message).color(crate::theme::WARN));
                        ui.label(egui::RichText::new(format!("理由: {}", if why.trim().is_empty() { "(書かれていません)" } else { why })).small());
                        if ui.small_button("承知をやめる").on_hover_text("この例外を取り消して、また指摘に戻します。回路図の表からも消えます").clicked() {
                            let w = design_spec::Waiver { kind: f.kind.key().into(), name: f.name.clone(), item: f.item.clone(), note: String::new() };
                            self.checkpoint();
                            self.board.waivers.retain(|x| !(x.kind == w.kind && x.name == w.name && (x.item.is_empty() || x.item == w.item)));
                            self.waiver_request = Some((false, w));
                        }
                    });
                }
            });
        }
        for note in &self.group_notes {
            ui.label(egui::RichText::new(format!("配置: {note}")).small().color(Color32::GRAY)).on_hover_text("自動配置のとき、組の部品のうち、コピー (同じ形の繰り返し) や IC のブロックにまとまっている部品は、組の塊に入れられません。その部品は、組の上限を距離のコストで守る形になります");
        }
        if !rep.missing.is_empty() {
            ui.label(format!("基板にない名前: {} 件", rep.missing.len())).on_hover_text(format!("表にあるが、基板に部品・パッドがないもの (回路図から取り込み直すと消えることがあります): {}", rep.missing.join("、")));
        }
    }
}
