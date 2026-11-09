//! Keep-out areas in the editor (`pcb_core::keepout`): the tool's shape becomes an area that forbids everything on every layer,
//! the areas are drawn hatched, and the panel lists them with what each forbids, its layers, and a button to take it off.

use super::*;
use pcb_core::keepout::Keepout;

impl BoardEditor {
    /// A drawn polygon becomes a keep-out area (one undo step: `finish_polygon` made the checkpoint).
    pub(super) fn add_keepout(&mut self, polygon: Vec<Point>) {
        self.board.keepouts.push(Keepout::new(polygon));
        let n = self.board.keepouts.len();
        let found = pcb_core::keepout::keepout_checks(&self.board).len();
        let inside = if found > 0 { format!("いま {found} 件の配線・ビア・部品が中にあります (赤い × の所)。") } else { String::new() };
        self.status = format!("禁止領域 {n} を作りました (配線・ビア・ベタ・部品を、全部の層で禁止)。{inside}禁止するものと層は、左の「禁止領域」の一覧で変えられます");
    }

    pub(super) fn draw_keepouts(&self, painter: &egui::Painter, rect: Rect) {
        if !self.vis.keepouts {
            return;
        }
        let col = Color32::from_rgb(235, 90, 90);
        for k in self.board.keepouts.iter().filter(|k| k.polygon.len() >= 3) {
            let pts: Vec<Pos2> = k.polygon.iter().map(|p| self.to_screen(rect, *p)).collect();
            // the outline, dashed
            for i in 0..pts.len() {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                painter.extend(egui::Shape::dashed_line(&[a, b], Stroke::new(1.5, col), 6.0, 4.0));
            }
            // hatching: lines at 45° every 1 mm, clipped to the polygon (the parts of each line inside it)
            let (mut lo, mut hi) = (Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN));
            for p in &k.polygon {
                (lo, hi) = (Point::new(lo.x.min(p.x), lo.y.min(p.y)), Point::new(hi.x.max(p.x), hi.y.max(p.y)));
            }
            let step = 1.0;
            let mut c = lo.x - (hi.y - lo.y);
            while c <= hi.x {
                // the line x - y = c - lo.y, from the bottom of the box to its top, cut where it crosses the polygon's edges
                let (a, b) = (Point::new(c, lo.y), Point::new(c + (hi.y - lo.y), hi.y));
                let mut ts: Vec<f64> = vec![];
                let n = k.polygon.len();
                for i in 0..n {
                    let (p, q) = (k.polygon[i], k.polygon[(i + 1) % n]);
                    if let Some(t) = cross_t(a, b, p, q) {
                        ts.push(t);
                    }
                }
                ts.sort_by(f64::total_cmp);
                for pair in ts.chunks(2) {
                    if let [t0, t1] = pair {
                        let at = |t: f64| Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
                        painter.line_segment([self.to_screen(rect, at(*t0)), self.to_screen(rect, at(*t1))], Stroke::new(1.0, col.gamma_multiply(0.45)));
                    }
                }
                c += step;
            }
        }
    }

    /// The settings of the keep-out tool: how the shape is drawn, and the list of the areas.
    pub(super) fn panel_keepouts(&mut self, ui: &mut egui::Ui) {
        ui.label(i18n::tr("形を描いて確定 (ダブルクリック・Enter、または最初の点をクリック) すると、禁止領域になります"));
        self.poly_mode_ui(ui);
        if self.board.keepouts.is_empty() {
            return;
        }
        ui.separator();
        crate::widgets::section_heading(ui, "禁止領域の一覧");
        let layers = self.board.copper_layers();
        let mut remove = None;
        let mut changed: Option<(usize, Keepout)> = None;
        for (i, k) in self.board.keepouts.iter().enumerate() {
            let mut k2 = k.clone();
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(format!("{}.", i + 1)).strong());
                ui.add(egui::TextEdit::singleline(&mut k2.name).desired_width(70.0).hint_text(i18n::tr("名前"))).on_hover_text(i18n::tr("領域の名前 (違反の文に出ます。空でもよい)"));
                for (flag, text, tip) in [
                    (&mut k2.tracks, "配線", "この中に配線を通さない"),
                    (&mut k2.vias, "ビア", "この中にビアを置かない"),
                    (&mut k2.pours, "ベタ", "この中をベタで埋めない (ベタがよけます)"),
                    (&mut k2.parts, "部品", "この中に部品 (パッドと、パッドを囲む四角) を置かない"),
                ] {
                    ui.checkbox(flag, text).on_hover_text(tip);
                }
                if ui.small_button(i18n::tr("消す")).on_hover_text(i18n::tr("この禁止領域を消します (「元に戻す」で戻せます)")).clicked() {
                    remove = Some(i);
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.small(i18n::tr("層:"));
                let mut all = k2.layers.is_empty();
                if ui.checkbox(&mut all, i18n::tr("全部")).on_hover_text(i18n::tr("全部の銅の層で禁止します。外すと、層を選べます")).changed() {
                    k2.layers = if all { vec![] } else { vec![Side::Top] };
                }
                if !all {
                    for &s in &layers {
                        let mut on = k2.layers.contains(&s);
                        if ui.checkbox(&mut on, s.label()).changed() {
                            if on {
                                k2.layers.push(s);
                            } else {
                                k2.layers.retain(|x| *x != s);
                            }
                        }
                    }
                }
            });
            if &k2 != k {
                changed = Some((i, k2));
            }
        }
        if let Some((i, k)) = changed {
            self.checkpoint_once();
            self.board.keepouts[i] = k;
        }
        if let Some(i) = remove {
            self.checkpoint();
            self.board.keepouts.remove(i);
            self.status = format!("禁止領域 {} を消しました", i + 1);
        }
    }
}

/// Where along `a`-`b` (0..1) it crosses the segment `p`-`q`, if it does.
fn cross_t(a: Point, b: Point, p: Point, q: Point) -> Option<f64> {
    let (rx, ry) = (b.x - a.x, b.y - a.y);
    let (sx, sy) = (q.x - p.x, q.y - p.y);
    let den = rx * sy - ry * sx;
    if den.abs() < 1e-12 {
        return None;
    }
    let t = ((p.x - a.x) * sy - (p.y - a.y) * sx) / den;
    let u = ((p.x - a.x) * ry - (p.y - a.y) * rx) / den;
    // half-open on the polygon's edge, so a line through a corner is cut once
    ((0.0..=1.0).contains(&t) && (0.0..1.0).contains(&u)).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drawn_square_becomes_an_area_the_drc_sees_and_undo_takes_it_back() {
        let mut e = BoardEditor::default();
        e.board = pcb_core::Board::new("t", 40.0, 30.0);
        e.board.tracks.push(pcb_core::Track { side: Side::Top, a: Point::new(2.0, 15.0), b: Point::new(38.0, 15.0), width: 0.25 });
        e.tool = Tool::Keepout;
        for p in [Point::new(10.0, 10.0), Point::new(20.0, 10.0), Point::new(20.0, 20.0), Point::new(10.0, 20.0)] {
            e.polygon_click(p);
        }
        e.finish_polygon();
        assert_eq!(e.board.keepouts.len(), 1);
        assert!(e.status.contains("1 件"), "{}", e.status);
        assert!(pcb_core::drc::check(&e.board).iter().any(|v| v.message.contains("禁止領域")));
        e.undo();
        assert!(e.board.keepouts.is_empty());
    }

    #[test]
    fn a_hatch_line_through_a_square_is_cut_at_its_two_sides() {
        let sq = [Point::new(0.0, 0.0), Point::new(4.0, 0.0), Point::new(4.0, 4.0), Point::new(0.0, 4.0)];
        let (a, b) = (Point::new(-1.0, 0.0), Point::new(3.0, 4.0));
        let ts: Vec<f64> = (0..4).filter_map(|i| cross_t(a, b, sq[i], sq[(i + 1) % 4])).collect();
        assert_eq!(ts.len(), 2, "{ts:?}");
    }
}
