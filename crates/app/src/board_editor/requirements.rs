//! The panel's list of the design's requirements that the placement does not meet (the groups and distances the schematic's table gave
//! the board on import; see `pcb_core::requirements`), with a button that selects the parts of a group that is too spread out.

use super::*;
use pcb_core::requirements::{check, FindingKind};

impl BoardEditor {
    /// The groups as frames (green when the farthest two parts are within the limit, red when not, a tag with the name and the numbers) and the
    /// distances to keep as a line between the nearest two pads (green when long enough, red when too short, the distance at the middle).
    pub(super) fn draw_requirement_marks(&self, painter: &egui::Painter, rect: Rect) {
        if (!self.show_req_marks && !self.show_req_lines) || !self.vis.parts || (self.board.place_groups.is_empty() && self.board.keep_apart.is_empty() && self.board.bypass.is_empty()) {
            return;
        }
        let marks = pcb_core::requirements::marks(&self.board);
        let (good, bad) = (Color32::from_rgb(70, 190, 100), Color32::from_rgb(240, 70, 70));
        for g in marks.groups.iter().filter(|_| self.show_req_marks) {
            let col = if g.ok { good } else { bad };
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
            painter.rect_stroke(r, 3.0, Stroke::new(2.0, col), egui::StrokeKind::Outside);
            let text = if g.limit > 0.0 { format!("組「{}」 {:.1} / {} mm", g.name, g.actual, g.limit) } else { format!("組「{}」", g.name) };
            let galley = painter.layout_no_wrap(text, egui::FontId::proportional(12.0), Color32::BLACK);
            let size = galley.size() + Vec2::new(8.0, 3.0);
            let x = r.left().min(rect.right() - size.x - 2.0).max(rect.left());
            let tag = Rect::from_min_size(Pos2::new(x, r.top() - galley.size().y - 6.0), size);
            painter.rect_filled(tag, 3.0, col);
            painter.galley(tag.min + Vec2::new(4.0, 1.5), galley, Color32::BLACK);
        }
        // a bypass capacitor and the IC pin the table says it serves: a thin line (green when near enough, red when far)
        for b in marks.bypass.iter().filter(|_| self.show_req_lines) {
            let col = if b.ok { good } else { bad };
            let (p, q) = (self.to_screen(rect, b.a), self.to_screen(rect, b.b));
            painter.line_segment([p, q], Stroke::new(if b.ok { 1.0 } else { 2.0 }, col));
            painter.circle_stroke(q, 4.0, Stroke::new(1.5, col));
            if !b.ok || self.zoom >= 14.0 {
                let galley = painter.layout_no_wrap(format!("{}→{} {:.1} mm", b.cap, b.ic, b.actual), egui::FontId::proportional(11.0), Color32::BLACK);
                let tag = Rect::from_center_size(p.lerp(q, 0.5), galley.size() + Vec2::new(6.0, 2.0));
                painter.rect_filled(tag, 3.0, col);
                painter.galley(tag.min + Vec2::new(3.0, 1.0), galley, Color32::BLACK);
            }
        }
        for a in marks.apart.iter().filter(|_| self.show_req_lines) {
            let col = if a.ok { good } else { bad };
            let (p, q) = (self.to_screen(rect, a.a), self.to_screen(rect, a.b));
            painter.line_segment([p, q], Stroke::new(if a.ok { 1.0 } else { 2.0 }, col));
            painter.circle_filled(p, 3.0, col);
            painter.circle_filled(q, 3.0, col);
            let text = if a.ok { format!("「{}」 {:.1} mm (下限 {})", a.name, a.actual, a.limit) } else { format!("「{}」 {:.1} mm しかない (下限 {})", a.name, a.actual, a.limit) };
            let galley = painter.layout_no_wrap(text, egui::FontId::proportional(12.0), Color32::BLACK);
            let tag = Rect::from_center_size(p.lerp(q, 0.5), galley.size() + Vec2::new(8.0, 3.0));
            painter.rect_filled(tag, 3.0, col);
            painter.galley(tag.min + Vec2::new(4.0, 1.5), galley, Color32::BLACK);
        }
    }

    pub(super) fn requirements_check(&mut self, ui: &mut egui::Ui) {
        if self.board.place_groups.is_empty() && self.board.keep_apart.is_empty() {
            return;
        }
        let mut both = self.show_req_marks || self.show_req_lines;
        if ui.checkbox(&mut both, "組と離す距離を基板の上に出す").on_hover_text("近くに置く組を枠で囲み (守れていれば緑、広がりすぎなら赤)、離す組の最も近い2つのパッドを線で結んで距離を書きます (足りなければ赤)。回路図の「設計の要件」の表が、基板の上で見えます。枠と線は、右上の「物の表示とロック」で別々に切り替えられます").changed() {
            (self.show_req_marks, self.show_req_lines) = (both, both);
        }
        let rep = check(&self.board);
        let orange = Color32::from_rgb(255, 180, 60);
        if rep.findings.is_empty() {
            ui.label(format!("✔ 設計の要件を満たしています ({} 件を確認)", rep.checked)).on_hover_text("回路図の「設計の要件」の表にある、近くに置く組と、離す組を調べました。上限や下限の0のものは調べません");
        } else {
            ui.colored_label(orange, format!("設計の要件を満たしていない所: {} 件", rep.findings.len())).on_hover_text("回路図の「設計の要件」の表にある、近くに置く組の広がりと、離す組の近さです。部品を動かすか、表の数字を見直してください");
            for f in &rep.findings {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(orange, &f.message);
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
        for note in &self.group_notes {
            ui.label(egui::RichText::new(format!("配置: {note}")).small().color(Color32::GRAY)).on_hover_text("自動配置のとき、組の部品のうち、コピー (同じ形の繰り返し) や IC のブロックにまとまっている部品は、組の塊に入れられません。その部品は、組の上限を距離のコストで守る形になります");
        }
        if !rep.missing.is_empty() {
            ui.label(format!("基板にない名前: {} 件", rep.missing.len())).on_hover_text(format!("表にあるが、基板に部品・パッドがないもの (回路図から取り込み直すと消えることがあります): {}", rep.missing.join("、")));
        }
    }
}
