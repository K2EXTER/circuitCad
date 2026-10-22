use super::*;

impl PartForm {
    /// The right-hand side: the symbol and the footprint drawn to scale, what is wrong, and the save button.
    pub(super) fn preview_pane(&mut self, ui: &mut egui::Ui, built: &Result<PartDef, String>) -> Option<PartDef> {
        let def = match built {
            Ok(d) => d,
            Err(e) => {
                ui.colored_label(Color32::from_rgb(230, 60, 60), e);
                return None;
            }
        };
        ui.label(egui::RichText::new("回路図の記号").strong());
        symbol_preview(ui, def);
        ui.label(egui::RichText::new("基板の形 (実寸比)").strong());
        if let Ok(fp) = def.footprint() {
            footprint_preview(ui, &fp);
            if let Some(h) = fp.height {
                ui.small(format!("本体の高さ {h:.2} mm"));
            }
        }
        let issues: Vec<Issue> = def.validate();
        egui::ScrollArea::vertical().id_salt("part_form_issues").max_height(110.0).auto_shrink([false, true]).show(ui, |ui| {
            if issues.iter().any(|i| i.error) {
                ui.small("赤い項目を直すと、保存できます");
            }
            if issues.is_empty() {
                ui.colored_label(Color32::from_rgb(60, 170, 90), "✔ 問題は見つかりません");
            }
            for i in &issues {
                // an "error" here only means "cannot be saved yet": a blank form starts with some, so it is worded as a to-do
                let (c, mark) = if i.error { (Color32::from_rgb(215, 90, 70), "保存前に直す") } else { (Color32::from_rgb(220, 160, 40), "注意") };
                ui.colored_label(c, format!("{mark}: {}", i.message));
            }
        });
        let ok = !def.has_errors();
        if ui.add_enabled(ok, egui::Button::new(egui::RichText::new("保存してライブラリに追加").size(15.0))).on_hover_text("この部品をライブラリに保存して、すぐに回路図で使えるようにします。エラーが残っていると押せません").clicked() {
            return Some(def.clone());
        }
        None
    }
}

/// The schematic symbol of the part, as it will be drawn on the sheet.
pub(super) fn symbol_preview(ui: &mut egui::Ui, def: &PartDef) {
    let sym = def.symbol();
    let lines = crate::schematic::symbol_lines(&sym);
    let (resp, painter) = ui.allocate_painter(Vec2::new(ui.available_width().min(370.0), 120.0), Sense::hover());
    let r = resp.rect;
    painter.rect_filled(r, 2.0, Color32::from_rgb(248, 245, 236));
    if lines.is_empty() {
        return;
    }
    let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for (a, b) in &lines {
        for p in [a, b] {
            x0 = x0.min(p[0]);
            x1 = x1.max(p[0]);
            y0 = y0.min(p[1]);
            y1 = y1.max(p[1]);
        }
    }
    let scale = ((r.width() - 40.0) / (x1 - x0).max(1.0)).min((r.height() - 24.0) / (y1 - y0).max(1.0)).min(26.0);
    let c = r.center();
    let to = |p: [f32; 2]| Pos2::new(c.x + (p[0] - (x0 + x1) / 2.0) * scale, c.y + (p[1] - (y0 + y1) / 2.0) * scale);
    for (a, b) in &lines {
        painter.line_segment([to(*a), to(*b)], Stroke::new(1.6, Color32::from_rgb(120, 30, 30)));
    }
    for (name, p) in crate::schematic::symbol_pin_labels(&sym) {
        painter.text(to(p), Align2::CENTER_BOTTOM, name, FontId::proportional(10.0), Color32::from_gray(90));
    }
}

pub(super) fn footprint_preview(ui: &mut egui::Ui, fp: &pcb_core::footprints::Footprint) {
    let (resp, painter) = ui.allocate_painter(Vec2::new(ui.available_width().min(420.0), 170.0), Sense::hover());
    let r = resp.rect;
    painter.rect_filled(r, 2.0, Color32::from_rgb(18, 22, 18));
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for p in &fp.pads {
        let (w, h) = p.shape.size();
        x0 = x0.min(p.pos.x - w / 2.0);
        x1 = x1.max(p.pos.x + w / 2.0);
        y0 = y0.min(p.pos.y - h / 2.0);
        y1 = y1.max(p.pos.y + h / 2.0);
    }
    for l in &fp.silk {
        for q in [l.a, l.b] {
            x0 = x0.min(q.x);
            x1 = x1.max(q.x);
            y0 = y0.min(q.y);
            y1 = y1.max(q.y);
        }
    }
    for h in &fp.holes {
        let (a, b) = pcb_core::slot_ends(h.pos, h.drill, h.slot, h.slot_v);
        x0 = x0.min(a.x.min(b.x) - h.drill / 2.0);
        x1 = x1.max(a.x.max(b.x) + h.drill / 2.0);
        y0 = y0.min(a.y.min(b.y) - h.drill / 2.0);
        y1 = y1.max(a.y.max(b.y) + h.drill / 2.0);
    }
    let scale = ((r.width() as f64 - 20.0) / (x1 - x0).max(0.1)).min((r.height() as f64 - 20.0) / (y1 - y0).max(0.1)) as f32;
    let c = r.center();
    let to = |x: f64, y: f64| Pos2::new(c.x + ((x - (x0 + x1) / 2.0) as f32) * scale, c.y - ((y - (y0 + y1) / 2.0) as f32) * scale);
    for p in &fp.pads {
        let (w, h) = p.shape.size();
        let ctr = to(p.pos.x, p.pos.y);
        let size = Vec2::new(w as f32 * scale, h as f32 * scale);
        let col = if p.pin == "1" { Color32::from_rgb(240, 120, 60) } else { Color32::from_rgb(220, 60, 60) };
        match p.shape {
            pcb_core::PadShape::Circle { .. } => {
                painter.circle_filled(ctr, size.x / 2.0, col);
            }
            _ => {
                painter.rect_filled(Rect::from_center_size(ctr, size), 0.0, col);
            }
        }
        if let Some(d) = p.drill {
            let (a, b) = pcb_core::slot_ends(p.pos, d, p.slot, p.slot_v);
            crate::board_editor::capsule(&painter, to(a.x, a.y), to(b.x, b.y), d as f32 * scale / 2.0, Color32::from_rgb(18, 22, 18), None);
        }
        painter.text(ctr, Align2::CENTER_CENTER, &p.pin, FontId::proportional(11.0), Color32::WHITE);
    }
    // holes of the part (no copper): an outline only
    for h in &fp.holes {
        let (a, b) = pcb_core::slot_ends(h.pos, h.drill, h.slot, h.slot_v);
        crate::board_editor::capsule(&painter, to(a.x, a.y), to(b.x, b.y), h.drill as f32 * scale / 2.0, Color32::TRANSPARENT, Some(Stroke::new(1.5, Color32::LIGHT_GRAY)));
    }
    // the body outline and orientation mark (silkscreen)
    for l in &fp.silk {
        painter.line_segment([to(l.a.x, l.a.y), to(l.b.x, l.b.y)], Stroke::new(1.3, Color32::from_gray(235)));
    }
    painter.rect_stroke(r, 2.0, Stroke::new(1.0, Color32::from_gray(80)), egui::StrokeKind::Inside);
}
