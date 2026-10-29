//! The panel's list of the design's requirements that the placement does not meet (the groups and distances the schematic's table gave
//! the board on import; see `pcb_core::requirements`), with a button that selects the parts of a group that is too spread out.

use super::*;
use pcb_core::requirements::{check, FindingKind};

/// Dark outline under the requirement frames and lines, so they stand out from the copper colours (top red, inner green, bottom blue).
const HALO: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 200);

impl BoardEditor {
    /// The groups as frames (green when the farthest two parts are within the limit, pink when not, a tag with the name and the numbers) and the
    /// distances to keep as a line between the nearest two pads (green when long enough, pink when too short, the distance at the middle).
    pub(super) fn draw_requirement_marks(&self, painter: &egui::Painter, rect: Rect) {
        if (!self.show_req_marks && !self.show_req_lines) || !self.vis.parts || !pcb_core::requirements::any(&self.board) {
            return;
        }
        let marks = pcb_core::requirements::marks(&self.board);
        let (good, bad) = (Color32::from_rgb(70, 190, 100), Color32::from_rgb(255, 60, 200));
        // not kept, but the table accepts it (a waiver with its reason)
        let accepted = Color32::from_rgb(210, 190, 90);
        // a dark halo under every line and frame: the top copper is red and the inner layers green, so the colour alone is not enough
        for g in marks.groups.iter().filter(|_| self.show_req_marks) {
            let col = if g.ok { good } else if g.waived { accepted } else { bad };
            let (mut lo, mut hi) = (Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN));
            for (a, b) in g.parts.iter().filter_map(|r| self.part_box(r)) {
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
            painter.rect_stroke(r, 3.0, Stroke::new(4.5, HALO), egui::StrokeKind::Outside);
            painter.rect_stroke(r, 3.0, Stroke::new(2.0, col), egui::StrokeKind::Outside);
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
            painter.line_segment([p, q], Stroke::new(if b.ok { 3.0 } else { 4.5 }, HALO));
            painter.line_segment([p, q], Stroke::new(if b.ok { 1.0 } else { 2.0 }, col));
            painter.circle_stroke(q, 4.0, Stroke::new(3.5, HALO));
            painter.circle_stroke(q, 4.0, Stroke::new(1.5, col));
            if !b.ok || self.zoom >= 14.0 {
                let galley = painter.layout_no_wrap(format!("{}→{} {:.1} mm{}", b.cap, b.ic, b.actual, if b.waived && !b.ok { " (承知)" } else { "" }), egui::FontId::proportional(11.0), Color32::BLACK);
                let tag = Rect::from_center_size(p.lerp(q, 0.5), galley.size() + Vec2::new(6.0, 2.0));
                painter.rect_filled(tag, 3.0, col);
                painter.galley(tag.min + Vec2::new(3.0, 1.0), galley, Color32::BLACK);
            }
        }
        // the copper of a connection that the table limits: the way along the tracks (two ways and the place they part for a fork)
        // the ones that are fine first, so that what is wrong lies on top (and the tags of one place sit in a column, not on each other)
        let measured = self.cached_measure_marks();
        let mut order: Vec<(usize, &pcb_core::requirements::MeasureMark)> = measured.iter().enumerate().filter(|_| self.show_req_lines).collect();
        order.sort_by_key(|(_, m)| !m.ok);
        for (row, m) in order {
            let col = if m.ok { good } else if m.waived { accepted } else { bad };
            let wide = if m.ok { 1.5 } else { 3.0 };
            for way in &m.ways {
                let pts: Vec<Pos2> = way.iter().map(|p| self.to_screen(rect, *p)).collect();
                for w in pts.windows(2) {
                    painter.line_segment([w[0], w[1]], Stroke::new(wide + 2.0, HALO));
                }
                for w in pts.windows(2) {
                    painter.line_segment([w[0], w[1]], Stroke::new(wide, col));
                }
            }
            if let Some(at) = m.at {
                let c = self.to_screen(rect, at);
                painter.circle_stroke(c, 7.0, Stroke::new(4.0, HALO));
                painter.circle_stroke(c, 7.0, Stroke::new(2.0, col));
            }
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
            painter.line_segment([p, q], Stroke::new(if a.ok { 3.0 } else { 4.5 }, HALO));
            painter.line_segment([p, q], Stroke::new(if a.ok { 1.0 } else { 2.0 }, col));
            for e in [p, q] {
                painter.circle_filled(e, 4.5, HALO);
                painter.circle_filled(e, 3.0, col);
            }
            let text = if a.ok { format!("「{}」 {:.1} mm (下限 {})", a.name, a.actual, a.limit) } else { format!("「{}」 {:.1} mm しかない (下限 {}){}", a.name, a.actual, a.limit, if a.waived { " (承知)" } else { "" }) };
            let galley = painter.layout_no_wrap(text, egui::FontId::proportional(12.0), Color32::BLACK);
            let tag = Rect::from_center_size(p.lerp(q, 0.5), galley.size() + Vec2::new(8.0, 3.0));
            painter.rect_filled(tag, 3.0, col);
            painter.galley(tag.min + Vec2::new(4.0, 1.5), galley, Color32::BLACK);
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
        let rep = check(&self.board);
        let orange = Color32::from_rgb(255, 180, 60);
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
                        ui.label(egui::RichText::new(&f.message).color(Color32::from_rgb(210, 190, 90)));
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
