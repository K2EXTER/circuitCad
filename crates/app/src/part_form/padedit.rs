//! "絵で直す": the pads of the part as a list the learner edits on a picture of the footprint, in the "④ 部品の形" tab. Click a
//! pad to pick it, drag it (or the arrow keys) to move it on a grid, change its shape and size in the boxes below, add a pad
//! next to it or take it away; Ctrl+Z goes back. While this is on, the pads are this list (`PackageSpec::Explicit`) and the
//! dimension boxes of the tab are not used.

use super::*;

/// The grids a pad snaps to while it is dragged or moved with the arrow keys, in mm.
pub(super) const SNAPS: &[f64] = &[0.01, 0.05, 0.1, 0.25, 0.635, 1.27];

/// The shapes offered, as the words of a `PadSpec` and their labels.
const SHAPES: &[(&str, &str, &str)] = &[
    ("roundrect", "角丸", "角を丸めた四角。面実装のパッドのふつうの形 (角の半径は短い辺の 25% が既定)"),
    ("rect", "四角", "角のとがった四角。1 番ピンの印にするときなど"),
    ("oval", "長円", "両端が丸い形。リード部品の穴のパッドに"),
    ("circle", "円", "丸いパッド。幅が直径です"),
];

#[derive(Clone, Debug, PartialEq)]
pub struct PadEdit {
    pub pads: Vec<PadSpec>,
    pub holes: Vec<HoleSpec>,
    /// The pad picked (an index into `pads`).
    pub sel: Option<usize>,
    /// The grid in mm (one of `SNAPS`).
    pub snap: f64,
    /// Earlier states, the last one on top.
    undo: Vec<(Vec<PadSpec>, Vec<HoleSpec>)>,
    /// While a pad is dragged: where it was grabbed (mm from its centre) and the view kept still meanwhile.
    drag: Option<(f64, f64)>,
    frozen: Option<View>,
    /// Where the picture was drawn last and how (the tests point at a pad with it).
    shown: Option<(Rect, View)>,
}

/// Which part of the footprint the picture shows: the middle and the pixels per mm.
#[derive(Clone, Copy, Debug, PartialEq)]
struct View {
    cx: f64,
    cy: f64,
    scale: f32,
}

/// `v` on the grid of `step` mm (rounded off past 1e-6 so 0.1 + 0.05 shows as 0.15).
pub(super) fn snap(v: f64, step: f64) -> f64 {
    if step <= 0.0 {
        return v;
    }
    ((v / step).round() * step * 1e6).round() / 1e6
}

/// The next free pin number: one past the highest number used (or "1"); names that are not numbers are skipped.
pub(super) fn next_pin(pads: &[PadSpec]) -> String {
    (pads.iter().filter_map(|p| p.pin.trim().parse::<u32>().ok()).max().unwrap_or(0) + 1).to_string()
}

impl PadEdit {
    /// The pads of a package as a list to edit.
    pub fn from_package(pkg: &PackageSpec, name: &str) -> Result<PadEdit, String> {
        let fp = pkg.build(name)?;
        let pads = fp.pads.iter().map(|p| parse::pad_spec(&fp, p)).collect();
        let holes = fp.holes.iter().map(|h| HoleSpec { x: h.pos.x, y: h.pos.y, drill: h.drill, slot: h.slot, slot_v: h.slot_v }).collect();
        Ok(PadEdit { pads, holes, sel: None, snap: 0.05, undo: vec![], drag: None, frozen: None, shown: None })
    }

    /// The pads as the part's package.
    pub fn package(&self) -> PackageSpec {
        PackageSpec::Explicit { pads: self.pads.clone(), holes: self.holes.clone() }
    }

    /// Keep the state before a change (nothing is kept twice in a row).
    pub fn checkpoint(&mut self) {
        let now = (self.pads.clone(), self.holes.clone());
        if self.undo.last() != Some(&now) {
            self.undo.push(now);
        }
    }

    pub fn can_undo(&self) -> bool {
        self.undo.last().is_some_and(|u| *u != (self.pads.clone(), self.holes.clone())) || self.undo.len() > 1
    }

    /// Back to the state before the last change.
    pub fn undo(&mut self) {
        while let Some((p, h)) = self.undo.pop() {
            if p != self.pads || h != self.holes {
                (self.pads, self.holes) = (p, h);
                break;
            }
        }
        self.sel = self.sel.filter(|&i| i < self.pads.len());
    }

    /// Move pad `i` so its centre is at (x, y) on the grid.
    pub fn move_to(&mut self, i: usize, x: f64, y: f64) {
        if let Some(p) = self.pads.get_mut(i) {
            (p.x, p.y) = (snap(x, self.snap), snap(y, self.snap));
        }
    }

    /// A copy of the picked pad (or of the last one) beside it, with the next pin number, and pick it. In a row of pads the
    /// copy goes on at the row's pitch; a lone pad gets it one pad width plus 0.5 mm to the right.
    pub fn add(&mut self) {
        self.checkpoint();
        let Some(i) = self.sel.or(self.pads.len().checked_sub(1)) else {
            self.pads.push(PadSpec { pin: "1".into(), x: 0.0, y: 0.0, shape: "roundrect".into(), w: 1.0, h: 0.6, r: 0.0, drill: None, slot: 0.0, slot_v: false });
            self.sel = Some(0);
            return;
        };
        let src = self.pads[i].clone();
        // the nearest pad of the same size: the step from it to this one is the row's pitch
        let step = self
            .pads
            .iter()
            .enumerate()
            .filter(|(j, q)| *j != i && (q.w - src.w).abs() < 1e-9 && (q.h - src.h).abs() < 1e-9)
            .map(|(_, q)| (src.x - q.x, src.y - q.y))
            .min_by(|a, b| a.0.hypot(a.1).total_cmp(&b.0.hypot(b.1)))
            .unwrap_or((src.w + 0.5, 0.0));
        let mut p = src;
        p.pin = next_pin(&self.pads);
        (p.x, p.y) = (snap(p.x + step.0, 0.001), snap(p.y + step.1, 0.001));
        self.pads.push(p);
        self.sel = Some(self.pads.len() - 1);
    }

    /// Take the picked pad away.
    pub fn remove(&mut self) {
        if let Some(i) = self.sel.filter(|&i| i < self.pads.len()) {
            self.checkpoint();
            self.pads.remove(i);
            self.sel = None;
        }
    }

    /// The pad under (x, y) mm, the one drawn last (on top) first.
    pub fn pad_at(&self, x: f64, y: f64) -> Option<usize> {
        self.pads.iter().enumerate().rev().find(|(_, p)| p.pad_shape().is_ok_and(|s| s.contains(x - p.x, y - p.y, 0.0))).map(|(i, _)| i)
    }

    /// The box round the pads and holes: (x0, y0, x1, y1) mm.
    fn bounds(&self) -> (f64, f64, f64, f64) {
        let mut b = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        let mut add = |x: f64, y: f64, hw: f64, hh: f64| b = (b.0.min(x - hw), b.1.min(y - hh), b.2.max(x + hw), b.3.max(y + hh));
        for p in &self.pads {
            let (w, h) = p.pad_shape().map(|s| s.size()).unwrap_or((p.w, p.h));
            add(p.x, p.y, w / 2.0, h / 2.0);
        }
        for h in &self.holes {
            add(h.x, h.y, (h.drill.max(h.slot)) / 2.0, (h.drill.max(h.slot)) / 2.0);
        }
        if b.0 > b.2 {
            (-2.0, -2.0, 2.0, 2.0)
        } else {
            b
        }
    }
}

impl PartForm {
    /// Start editing the pads on the picture, from the pads the form makes now.
    pub(super) fn start_pad_edit(&mut self) {
        match self.to_def().and_then(|d| PadEdit::from_package(&d.package, self.name.trim())) {
            Ok(e) => self.pad_edit = Some(e),
            Err(e) => self.pad_edit_error = Some(e),
        }
    }

    /// `--form-pad-edit [pin]`: start the editor with that pin's pad picked (for a screenshot).
    pub fn pad_edit_from_args(&mut self, pin: Option<&str>) {
        self.start_pad_edit();
        if let (Some(e), Some(pin)) = (self.pad_edit.as_mut(), pin) {
            e.sel = e.pads.iter().position(|p| p.pin == pin);
        }
    }

    /// The editor: the notice and the way back, the picture, the boxes of the picked pad and the buttons.
    pub(super) fn pad_edit_ui(&mut self, ui: &mut egui::Ui) {
        let Some(mut e) = self.pad_edit.take() else { return };
        let mut stop = false;
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(crate::theme::warn(), i18n::tr("パッドを絵で直しています (下の寸法・標準の形・追加のパッドの欄は使われません)。"));
            if ui.button(i18n::tr("寸法に戻す")).on_hover_text(i18n::tr("絵で直したパッドを捨てて、寸法の欄 (または標準の形) から作るのに戻します")).clicked() {
                stop = true;
            }
        });
        ui.small(i18n::tr("パッドをクリックで選び、ドラッグで動かします (矢印キーでも 1 目盛り)。座標は部品の中心から mm、右が +X・上が +Y。Ctrl+Z で元に戻します"));
        canvas(ui, &mut e);
        fields(ui, &mut e);
        self.pad_edit = (!stop).then_some(e);
    }
}

/// The picture: pads (pin 1 orange, the picked one outlined), holes, the centre cross, and the pointer's position.
fn canvas(ui: &mut egui::Ui, e: &mut PadEdit) {
    let (resp, painter) = ui.allocate_painter(Vec2::new(ui.available_width().min(460.0), 260.0), Sense::click_and_drag());
    let r = resp.rect;
    painter.rect_filled(r, 2.0, Color32::from_rgb(18, 22, 18));
    let view = e.frozen.unwrap_or_else(|| {
        let (x0, y0, x1, y1) = e.bounds();
        let scale = ((r.width() as f64 - 40.0) / (x1 - x0).max(0.5)).min((r.height() as f64 - 40.0) / (y1 - y0).max(0.5)).min(80.0) as f32;
        View { cx: (x0 + x1) / 2.0, cy: (y0 + y1) / 2.0, scale }
    });
    e.shown = Some((r, view));
    let c = r.center();
    let to = |x: f64, y: f64| Pos2::new(c.x + ((x - view.cx) as f32) * view.scale, c.y - ((y - view.cy) as f32) * view.scale);
    let from = |p: Pos2| (view.cx + ((p.x - c.x) / view.scale) as f64, view.cy - ((p.y - c.y) / view.scale) as f64);
    let painter = painter.with_clip_rect(r);
    // the part's centre (the origin of the coordinates)
    let o = to(0.0, 0.0);
    for (a, b) in [(Vec2::new(-6.0, 0.0), Vec2::new(6.0, 0.0)), (Vec2::new(0.0, -6.0), Vec2::new(0.0, 6.0))] {
        painter.line_segment([o + a, o + b], Stroke::new(1.0, Color32::from_gray(110)));
    }
    for (i, p) in e.pads.iter().enumerate() {
        let Ok(shape) = p.pad_shape() else { continue };
        let (w, h) = shape.size();
        let ctr = to(p.x, p.y);
        let size = Vec2::new(w as f32 * view.scale, h as f32 * view.scale);
        let col = if p.pin == "1" { Color32::from_rgb(240, 120, 60) } else { crate::theme::error() };
        let rect = Rect::from_center_size(ctr, size);
        match shape {
            pcb_core::PadShape::Circle { .. } => {
                painter.circle_filled(ctr, size.x / 2.0, col);
            }
            _ => {
                painter.rect_filled(rect, shape.corner_radius() as f32 * view.scale, col);
            }
        }
        if let Some(d) = p.drill {
            let (a, b) = pcb_core::slot_ends(pcb_core::Point::new(p.x, p.y), d, p.slot, p.slot_v);
            crate::board_editor::capsule(&painter, to(a.x, a.y), to(b.x, b.y), d as f32 * view.scale / 2.0, Color32::from_rgb(18, 22, 18), None);
        }
        if e.sel == Some(i) {
            painter.rect_stroke(rect.expand(3.0), 2.0, Stroke::new(2.0, Color32::from_rgb(255, 220, 60)), egui::StrokeKind::Outside);
        }
        painter.text(ctr, Align2::CENTER_CENTER, &p.pin, FontId::proportional(11.0), Color32::WHITE);
    }
    for h in &e.holes {
        let (a, b) = pcb_core::slot_ends(pcb_core::Point::new(h.x, h.y), h.drill, h.slot, h.slot_v);
        crate::board_editor::capsule(&painter, to(a.x, a.y), to(b.x, b.y), h.drill as f32 * view.scale / 2.0, Color32::TRANSPARENT, Some(Stroke::new(1.5, Color32::LIGHT_GRAY)));
    }
    painter.rect_stroke(r, 2.0, Stroke::new(1.0, Color32::from_gray(80)), egui::StrokeKind::Inside);

    // pick, drag
    if let Some(pos) = resp.interact_pointer_pos() {
        let (x, y) = from(pos);
        if resp.drag_started() || resp.clicked() {
            // where the button went down: a drag starts only after the pointer has moved a few pixels
            let (px, py) = from(ui.input(|i| i.pointer.press_origin()).unwrap_or(pos));
            e.sel = e.pad_at(px, py);
            if resp.drag_started()
                && let Some(i) = e.sel
            {
                e.checkpoint();
                e.drag = Some((px - e.pads[i].x, py - e.pads[i].y));
                e.frozen = Some(view);
            }
        }
        if let (Some((gx, gy)), Some(i), true) = (e.drag, e.sel, resp.dragged()) {
            e.move_to(i, x - gx, y - gy);
        }
    }
    if resp.drag_stopped() {
        e.drag = None;
        e.frozen = None;
    }
    // the arrow keys move the picked pad by one grid step; Ctrl+Z goes back
    if resp.hovered() || resp.has_focus() {
        let (mut dx, mut dy) = (0.0, 0.0);
        let undo = ui.input_mut(|i| {
            for (k, d) in [(egui::Key::ArrowLeft, (-1.0, 0.0)), (egui::Key::ArrowRight, (1.0, 0.0)), (egui::Key::ArrowUp, (0.0, 1.0)), (egui::Key::ArrowDown, (0.0, -1.0))] {
                if i.consume_key(egui::Modifiers::NONE, k) {
                    (dx, dy) = d;
                }
            }
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)
        });
        if let (Some(i), true) = (e.sel, dx != 0.0 || dy != 0.0) {
            e.checkpoint();
            let (x, y) = (e.pads[i].x + dx * e.snap, e.pads[i].y + dy * e.snap);
            e.move_to(i, x, y);
        }
        if undo {
            e.undo();
        }
    }
    let at = resp.hover_pos().map(from);
    let text = match (e.drag.and(e.sel), at) {
        (Some(i), _) => i18n::trf!("ピン {}: X {:.3}  Y {:.3} mm", e.pads[i].pin, e.pads[i].x, e.pads[i].y),
        (None, Some((x, y))) => format!("X {x:.2}  Y {y:.2} mm"),
        _ => String::new(),
    };
    painter.text(r.left_bottom() + Vec2::new(6.0, -6.0), Align2::LEFT_BOTTOM, text, FontId::proportional(11.0), Color32::from_gray(200));
}

/// The boxes of the picked pad, the grid and the buttons.
fn fields(ui: &mut egui::Ui, e: &mut PadEdit) {
    ui.horizontal(|ui| {
        if ui.button(i18n::tr("パッドを足す")).on_hover_text(i18n::tr("選んだパッドを写して、となりに置きます (並んだパッドなら同じ間隔で次へ)。ピン番号は次の番号になります。何も選んでいなければ最後のパッドを写します")).clicked() {
            e.add();
        }
        if crate::widgets::unless(ui, (!e.sel.is_some()).then_some("先にパッドを選んでください"), egui::Button::new(i18n::tr("消す"))).on_hover_text(i18n::tr("選んだパッドを消します")).clicked() {
            e.remove();
        }
        if crate::widgets::unless(ui, (!e.can_undo()).then_some("戻す操作がありません"), egui::Button::new(i18n::tr("元に戻す"))).on_hover_text(i18n::tr("ひとつ前に戻します (Ctrl+Z)")).clicked() {
            e.undo();
        }
        ui.separator();
        ui.label(i18n::tr("目盛り"));
        egui::ComboBox::from_id_salt("pad_snap").selected_text(format!("{} mm", e.snap)).width(80.0).show_ui(ui, |ui| {
            for s in SNAPS {
                ui.selectable_value(&mut e.snap, *s, format!("{s} mm"));
            }
        }).response.on_hover_text(i18n::tr("ドラッグと矢印キーで動かすときの目盛りです。データシートの寸法の細かさに合わせます (0.635 と 1.27 は 1.27 mm ピッチの半分と 1 つ)"));
    });
    let Some(i) = e.sel.filter(|&i| i < e.pads.len()) else {
        ui.small(i18n::tr("パッドを選ぶと、ここで形と大きさを数字で直せます"));
        return;
    };
    let before = (e.pads.clone(), e.holes.clone());
    let touched = std::cell::Cell::new(false);
    let track = |r: egui::Response| {
        if r.drag_started() || r.gained_focus() || r.changed() && !r.dragged() && !r.has_focus() {
            touched.set(true);
        }
        r
    };
    let p = &mut e.pads[i];
    egui::Grid::new("pad_edit_fields").num_columns(4).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label(i18n::tr("ピン番号"));
        track(ui.add(egui::TextEdit::singleline(&mut p.pin).desired_width(60.0))).on_hover_text(i18n::tr("「③ ピン」の表の番号と同じにします。シールドの脚など、同じ番号のパッドを何個も置けます"));
        ui.label(i18n::tr("形"));
        let shown = SHAPES.iter().find(|s| s.0 == p.shape).map_or(p.shape.clone(), |s| i18n::tr(s.1).to_string());
        egui::ComboBox::from_id_salt("pad_shape").selected_text(shown).show_ui(ui, |ui| {
            for (key, label, tip) in SHAPES {
                if ui.selectable_label(p.shape == *key, i18n::tr(label)).on_hover_text(i18n::tr(tip)).clicked() && p.shape != *key {
                    touched.set(true);
                    p.shape = key.to_string();
                }
            }
        });
        ui.end_row();
        ui.label("X");
        track(ui.add(egui::DragValue::new(&mut p.x).speed(0.01).max_decimals(3).suffix(" mm"))).on_hover_text(i18n::tr("パッドの中心の横の位置。部品の中心から右が +"));
        ui.label("Y");
        track(ui.add(egui::DragValue::new(&mut p.y).speed(0.01).max_decimals(3).suffix(" mm"))).on_hover_text(i18n::tr("パッドの中心の縦の位置。部品の中心から上が +"));
        ui.end_row();
        let circle = p.shape == "circle";
        ui.label(i18n::tr(if circle { "直径" } else { "幅" }));
        track(ui.add(egui::DragValue::new(&mut p.w).speed(0.01).range(0.05..=50.0).max_decimals(3).suffix(" mm"))).on_hover_text(i18n::tr("パッドの横の大きさ (円は直径)"));
        if !circle {
            ui.label(i18n::tr("高さ"));
            if p.h <= 0.0 {
                p.h = p.w;
            }
            track(ui.add(egui::DragValue::new(&mut p.h).speed(0.01).range(0.05..=50.0).max_decimals(3).suffix(" mm"))).on_hover_text(i18n::tr("パッドの縦の大きさ"));
        }
        ui.end_row();
        if p.shape == "roundrect" {
            ui.label(i18n::tr("角の丸み"));
            let mut pct = if p.r > 0.0 { p.r * 100.0 } else { pcb_core::ROUNDRECT_DEFAULT * 100.0 };
            let resp = track(ui.add(egui::DragValue::new(&mut pct).speed(0.5).range(1.0..=50.0).max_decimals(0).suffix(" %")));
            resp.on_hover_text(i18n::tr("角の半径を、短い辺に対する割合で。25 % が KiCad と IPC-7351 の既定、50 % で両端が半円になります"));
            p.r = if (pct / 100.0 - pcb_core::ROUNDRECT_DEFAULT).abs() < 1e-9 { 0.0 } else { pct / 100.0 };
            ui.end_row();
        }
        ui.label(i18n::tr("穴"));
        let mut thru = p.drill.is_some();
        if track(ui.checkbox(&mut thru, i18n::tr("めっきの穴"))).on_hover_text(i18n::tr("リード部品の足を通す穴を開けます (表と裏の両方にパッドができます)。外すと面実装のパッドです")).changed() {
            p.drill = thru.then(|| (p.w.min(if p.h > 0.0 { p.h } else { p.w }) * 0.6 * 100.0).round() / 100.0);
            if !thru {
                p.slot = 0.0;
            }
        }
        if let Some(d) = p.drill.as_mut() {
            track(ui.add(egui::DragValue::new(d).speed(0.01).range(0.1..=10.0).max_decimals(2).prefix(i18n::tr("穴径 ")).suffix(" mm"))).on_hover_text(i18n::tr("穴の直径。足の太さより 0.2〜0.4 mm 大きくします"));
        }
        ui.end_row();
    });
    if touched.get() {
        // the state before this change goes on the stack once, when the change starts
        let now = (e.pads.clone(), e.holes.clone());
        let mut keep = e.clone();
        (keep.pads, keep.holes) = before;
        keep.checkpoint();
        e.undo = keep.undo;
        (e.pads, e.holes) = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chip_form() -> PartForm {
        let mut f = PartForm::default();
        f.name = "TEST".into();
        f.pins_text = "1 A
2 K".into();
        f.pkg = PkgKind::Chip2;
        f
    }

    #[test]
    fn the_grid_and_the_next_pin_number() {
        assert_eq!(snap(0.1234, 0.05), 0.1);
        assert_eq!(snap(0.1 + 0.05, 0.05), 0.15);
        assert_eq!(snap(-1.27 * 1.4, 0.635), -1.905);
        let pad = |pin: &str| PadSpec { pin: pin.into(), x: 0.0, y: 0.0, shape: "rect".into(), w: 1.0, h: 1.0, r: 0.0, drill: None, slot: 0.0, slot_v: false };
        assert_eq!(next_pin(&[pad("1"), pad("7"), pad("SH")]), "8");
        assert_eq!(next_pin(&[pad("SH")]), "1");
    }

    #[test]
    fn the_edited_pads_are_saved_as_the_package() {
        let mut f = chip_form();
        f.start_pad_edit();
        let e = f.pad_edit.as_mut().expect("started");
        assert_eq!(e.pads.len(), 2);
        // a chip's pads come in rounded
        assert_eq!(e.pads[0].shape, "roundrect");
        // move pin 2 on the grid, make it a sharp square, add a third next to it at the same pitch
        e.checkpoint();
        e.move_to(1, 1.234, 0.51);
        assert_eq!((e.pads[1].x, e.pads[1].y), (1.25, 0.5));
        e.pads[1].shape = "rect".into();
        e.sel = Some(1);
        e.add();
        let (a, b, c) = (e.pads[0].clone(), e.pads[1].clone(), e.pads[2].clone());
        assert_eq!(c.pin, "3");
        assert!(((c.x - b.x) - (b.x - a.x)).abs() < 1e-9 && ((c.y - b.y) - (b.y - a.y)).abs() < 1e-9, "{a:?} {b:?} {c:?}");
        let def = f.to_def().unwrap();
        let fp = def.package.build("TEST").unwrap();
        assert_eq!(fp.pads.len(), 3);
        assert_eq!(fp.pads[1].shape, pcb_core::PadShape::Rect { w: b.w, h: b.h });
        assert!(matches!(fp.pads[0].shape, pcb_core::PadShape::RoundRect { .. }));
        // the dimension boxes no longer change the pads
        f.pitch = 9.0;
        assert_eq!(f.to_def().unwrap().package.build("TEST").unwrap().pads[1].pos.x, 1.25);
        // "寸法に戻す"
        f.pad_edit = None;
        assert_eq!(f.to_def().unwrap().package.build("TEST").unwrap().pads.len(), 2);
    }

    #[test]
    fn remove_and_undo() {
        let mut f = chip_form();
        f.start_pad_edit();
        let e = f.pad_edit.as_mut().unwrap();
        let first = e.pads.clone();
        assert!(!e.can_undo());
        e.sel = e.pad_at(e.pads[1].x, e.pads[1].y);
        assert_eq!(e.sel, Some(1));
        e.remove();
        assert_eq!(e.pads.len(), 1);
        assert!(e.can_undo());
        e.undo();
        assert_eq!(e.pads, first);
        // nothing under an empty spot
        assert_eq!(e.pad_at(50.0, 50.0), None);
    }

    /// Dragging a pad with the mouse moves it on the grid, and Ctrl+Z puts it back.
    #[test]
    fn a_pad_is_dragged_on_the_picture() {
        let mut f = chip_form();
        f.start_pad_edit();
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 700.0));
        let frame = |f: &mut PartForm, events: Vec<egui::Event>| {
            let out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| f.pad_edit_ui(ui));
            let mut out = out;
            out.textures_delta.clear();
        };
        frame(&mut f, vec![]);
        let (r, view) = f.pad_edit.as_ref().unwrap().shown.unwrap();
        let to = |x: f64, y: f64| Pos2::new(r.center().x + ((x - view.cx) as f32) * view.scale, r.center().y - ((y - view.cy) as f32) * view.scale);
        let (x0, y0) = { let p = &f.pad_edit.as_ref().unwrap().pads[1]; (p.x, p.y) };
        let start = to(x0, y0);
        let end = start + Vec2::new(view.scale * 0.5, -view.scale * 0.3); // 0.5 mm right, 0.3 mm up
        let button = |pos: Pos2, pressed: bool| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        frame(&mut f, vec![egui::Event::PointerMoved(start)]);
        frame(&mut f, vec![button(start, true)]);
        for k in 1..=6 {
            frame(&mut f, vec![egui::Event::PointerMoved(start + (end - start) * (k as f32 / 6.0))]);
        }
        frame(&mut f, vec![button(end, false)]);
        let e = f.pad_edit.as_ref().unwrap();
        assert_eq!(e.sel, Some(1));
        let p = &e.pads[1];
        assert!((p.x - snap(x0 + 0.5, 0.05)).abs() < 1e-9 && (p.y - snap(y0 + 0.3, 0.05)).abs() < 1e-9, "({}, {}) from ({x0}, {y0})", p.x, p.y);
        frame(&mut f, vec![egui::Event::Key { key: egui::Key::Z, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND }]);
        let p = &f.pad_edit.as_ref().unwrap().pads[1];
        assert_eq!((p.x, p.y), (x0, y0));
    }
}
