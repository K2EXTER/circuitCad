use super::*;

impl PartForm {
    /// The right-hand side: the symbol and the footprint drawn to scale, what is wrong, and the save button.
    pub(super) fn preview_pane(&mut self, ui: &mut egui::Ui, built: &Result<PartDef, String>) -> Option<PartDef> {
        let def = match built {
            Ok(d) => d,
            Err(e) => {
                ui.colored_label(crate::theme::ERROR, i18n::tr_dyn(e));
                return None;
            }
        };
        crate::widgets::section_heading(ui, "回路図の記号");
        symbol_preview(ui, def);
        crate::widgets::section_heading(ui, "部品の形 (実寸比)");
        if let Ok(fp) = def.footprint() {
            footprint_preview(ui, &fp);
            if let Some(h) = fp.height {
                ui.small(i18n::trf!("本体の高さ {h:.2} mm"));
            }
        }
        let issues: Vec<Issue> = def.validate();
        // what stops the save, then what is only worth a look, each under its own small heading; an "error" here only
        // means "cannot be saved yet" (a blank form starts with some), so it is worded as a to-do
        ui.add_space(4.0);
        if issues.is_empty() {
            ui.colored_label(crate::theme::OK, i18n::tr("✔ 問題は見つかりません"));
        }
        egui::ScrollArea::vertical().id_salt("part_form_issues").max_height(130.0).auto_shrink([false, true]).show(ui, |ui| {
            for (error, head, color) in [(true, "保存の前に直すこと", crate::theme::ERROR), (false, "注意", crate::theme::WARN)] {
                let lines: Vec<&Issue> = issues.iter().filter(|i| i.error == error).collect();
                if lines.is_empty() {
                    continue;
                }
                ui.label(egui::RichText::new(i18n::tr(head)).small().strong().color(color));
                for i in lines {
                    ui.horizontal_top(|ui| {
                        ui.colored_label(color, "•");
                        ui.add(egui::Label::new(egui::RichText::new(i18n::tr_dyn(&i.message)).color(color)).wrap());
                    });
                }
            }
        });
        let ok = !def.has_errors();
        let replaces = self.editing() == Some(def.name.as_str());
        let (label, tip) = if replaces {
            ("保存して上書き", "「{}」をこの内容で書き換えます。すでに回路図に置いた部品は、そのままです (新しく置く部品から変わります)。名前を変えて押すと、別の部品として追加します。エラーが残っていると押せません")
        } else {
            ("保存してライブラリに追加", "この部品をライブラリに保存して、すぐに回路図で使えるようにします。エラーが残っていると押せません")
        };
        if let Some(n) = self.editing().filter(|_| !replaces) {
            ui.small(i18n::trf!("「{n}」から名前を変えたので、別の部品として追加します (元の部品は残ります)"));
        }
        if crate::widgets::primary_button_wide(ui, ok, label).on_hover_text(i18n::tr(tip).replace("{}", &def.name)).clicked() {
            return Some(def.clone());
        }
        None
    }
}

/// The schematic symbol of the part, as it will be drawn on the sheet.
pub(super) fn symbol_preview(ui: &mut egui::Ui, def: &PartDef) {
    let sym = def.symbol();
    let lines = crate::schematic::symbol_lines(&sym);
    let (resp, painter) = ui.allocate_painter(Vec2::new(ui.available_width().min(370.0), 140.0), Sense::hover());
    let r = resp.rect;
    painter.rect_filled(r, 2.0, Color32::from_rgb(248, 245, 236));
    if lines.is_empty() {
        return;
    }
    let labels = crate::schematic::symbol_pin_labels(&sym);
    // the pin names count too: a transistor's C and E sit at the ends of its leads and were drawn off the box
    let lines_bottom = lines.iter().flat_map(|(a, b)| [a[1], b[1]]).fold(f32::MIN, f32::max);
    let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for (a, b) in lines.iter().map(|(a, b)| (a, b)).chain(labels.iter().map(|(_, p)| (p, p))) {
        for p in [a, b] {
            x0 = x0.min(p[0]);
            x1 = x1.max(p[0]);
            y0 = y0.min(p[1]);
            y1 = y1.max(p[1]);
        }
    }
    // room for a label above the top end and below the bottom end (labels sit outside the lead ends)
    let scale = ((r.width() - 40.0) / (x1 - x0).max(1.0)).min((r.height() - 44.0) / (y1 - y0).max(1.0)).min(26.0);
    let c = r.center();
    let to = |p: [f32; 2]| Pos2::new(c.x + (p[0] - (x0 + x1) / 2.0) * scale, c.y + (p[1] - (y0 + y1) / 2.0) * scale);
    for (a, b) in &lines {
        painter.line_segment([to(*a), to(*b)], Stroke::new(1.6, Color32::from_rgb(120, 30, 30)));
    }
    let painter = painter.with_clip_rect(r);
    for (name, p) in labels {
        // a pin at the bottom end (a transistor's E) gets its name under the lead, not over it
        // (the label point sits 0.2 above the pin tip, see `symbol_pin_labels`)
        let tip = [p[0], p[1] + 0.2];
        let (at, align) = if tip[1] >= lines_bottom - 1e-3 { (tip, Align2::CENTER_TOP) } else { (p, Align2::CENTER_BOTTOM) };
        painter.text(to(at), align, name, FontId::proportional(11.0), Color32::from_gray(70));
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
        let col = if p.pin == "1" { Color32::from_rgb(240, 120, 60) } else { crate::theme::ERROR };
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
