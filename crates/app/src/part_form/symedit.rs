//! "記号のピン配置": the box symbol drawn large on the "③ ピン" tab, where a pin is dragged to another side or to another
//! place on its side, and a right click leaves empty places before a pin (to set the supply apart from the signals, as
//! data sheets draw them). Everything is written back into the pin text (side word, order of the lines, `gap=N`), so the
//! table, the paste box and the saved part keep the one form they had.

use super::pintable::{rows_of, text_of, PinRow};
use super::*;
use sch_core::SymLayout;

/// Where a pin dropped at `g` (symbol grid units, y down) goes: the side of the box it is nearest to, and its place
/// among the other pins of that side (0 = before the first one).
pub(super) fn drop_at(lay: &SymLayout, dragged: &str, g: (f32, f32)) -> (PinSide, usize) {
    let (hw, hh) = (lay.hw.max(1) as f32, lay.hh.max(1) as f32);
    let side = if g.0.abs() > hw + 0.5 || g.1.abs() > hh + 0.5 {
        // outside the box: the side it is beyond (the farther one when beyond a corner)
        if g.0.abs() - hw >= g.1.abs() - hh {
            if g.0 < 0.0 { PinSide::Left } else { PinSide::Right }
        } else if g.1 < 0.0 {
            PinSide::Top
        } else {
            PinSide::Bottom
        }
    } else if hw - g.0.abs() <= hh - g.1.abs() {
        if g.0 < 0.0 { PinSide::Left } else { PinSide::Right }
    } else if g.1 < 0.0 {
        PinSide::Top
    } else {
        PinSide::Bottom
    };
    let along = |p: (i32, i32)| if matches!(side, PinSide::Left | PinSide::Right) { p.1 as f32 } else { p.0 as f32 };
    let at = if matches!(side, PinSide::Left | PinSide::Right) { g.1 } else { g.0 };
    let before = lay.pins.iter().filter(|p| p.side == side && p.number != dragged).filter(|p| along(p.tip) < at).count();
    (side, before)
}

fn side_name(s: PinSide) -> &'static str {
    match s {
        PinSide::Left => "left",
        PinSide::Right => "right",
        PinSide::Top => "top",
        PinSide::Bottom => "bottom",
    }
}

/// The rows with every side written out as the symbol draws it now: an automatic side ("前半が左") would move when
/// another pin moves, so the first drag fixes them all where they are.
fn fixed_sides(rows: &mut [PinRow], lay: &SymLayout) {
    for r in rows.iter_mut().filter(|r| r.side.is_empty()) {
        if let Some(p) = lay.pins.iter().find(|p| p.number == r.number) {
            r.side = side_name(p.side).into();
        }
    }
}

/// Moves pin `number` to `side`, before the `before`-th other pin of that side (after the last when there are fewer).
/// The order of the lines is the order on each side, so the row moves in the list. A moved pin starts without a gap.
pub(super) fn move_pin(rows: &mut Vec<PinRow>, lay: &SymLayout, number: &str, side: PinSide, before: usize) {
    fixed_sides(rows, lay);
    let Some(from) = rows.iter().position(|r| r.number == number) else { return };
    let mut row = rows.remove(from);
    row.side = side_name(side).into();
    row.gap = 0;
    let on_side: Vec<usize> = rows.iter().enumerate().filter(|(_, r)| r.side == row.side).map(|(i, _)| i).collect();
    let at = match on_side.get(before) {
        Some(i) => *i,
        None => on_side.last().map_or(rows.len(), |i| i + 1),
    };
    rows.insert(at, row);
}

impl PartForm {
    /// The symbol editor of the pin tab.
    pub(super) fn symbol_layout_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(i18n::tr("記号のピン配置")).strong());
            crate::icons::help(ui, "回路図の記号で、ピンをどの辺のどこに出すかを、絵の上で決めます。ピンをドラッグすると、別の辺や別の順番に動きます (表の行の順番と「向き」も変わります)。ピンを右クリックすると、その前に空きを入れられます (電源ピンを信号から離す、データシートの絵に合わせる など)。ダイオードやトランジスタのような決まった形の記号は、ピンの辺が合わないと箱で描かれます");
        });
        let def = match self.to_def() {
            Ok(d) => d,
            Err(_) => {
                ui.small(i18n::tr("ピン表のまちがいを直すと、ここに記号が出ます"));
                return;
            }
        };
        let sym = def.symbol();
        let lay = sym.layout();
        if lay.pins.is_empty() {
            return;
        }
        // the extent in grid units: the box, the leads (2 units) and room for the numbers
        let (ex, ey) = (lay.hw as f32 + 3.0, lay.hh as f32 + 3.0);
        let w = ui.available_width().min(560.0);
        let scale = ((w - 40.0) / (2.0 * ex)).min(320.0 / (2.0 * ey)).clamp(6.0, 22.0);
        let h = 2.0 * ey * scale + 10.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2.0, Color32::from_rgb(248, 245, 236));
        let c = rect.center();
        let to = |p: (f32, f32)| Pos2::new(c.x + p.0 * scale, c.y + p.1 * scale);
        let from = |q: Pos2| ((q.x - c.x) / scale, (q.y - c.y) / scale);
        let ink = Color32::from_rgb(120, 30, 30);
        // the symbol as the sheet draws it (a box, or the standard shape of a diode, a transistor ...)
        for (a, b) in crate::schematic::symbol_lines(&sym) {
            painter.line_segment([to((a[0], a[1])), to((b[0], b[1]))], Stroke::new(1.6, ink));
        }
        // a standard shape has no box to write the names in (its body is 0 wide): they go by the lead's end
        let boxed = lay.hw > 0;
        let font = FontId::proportional(11.0);
        let mut moved: Option<(String, PinSide, usize)> = None;
        let mut gap_change: Option<(String, i32)> = None;
        for (i, p) in lay.pins.iter().enumerate() {
            let tip = to((p.tip.0 as f32, p.tip.1 as f32));
            let edge = to((p.edge.0 as f32, p.edge.1 as f32));
            // a standard shape's lead ends at its drawing, not at the body's centre (`edge` is the centre when the body
            // is 0 wide): there only the outer part of the lead, one grid step long, is the pin's own
            let inner = if boxed { edge } else { tip + (edge - tip).normalized() * scale };
            let hit = Rect::from_two_pos(tip, inner).expand(scale * 0.7);
            let resp = ui.interact(hit, ui.id().with(("sym_pin", i)), Sense::click_and_drag());
            let hot = resp.hovered() || resp.dragged();
            if boxed || hot {
                painter.line_segment([tip, inner], Stroke::new(if hot { 3.0 } else { 1.6 }, if hot { crate::theme::ACCENT } else { ink }));
            }
            // the name inside the box next to its lead, the number by the lead outside
            let (name_at, name_align, num_align) = match p.side {
                PinSide::Left => (edge + Vec2::new(4.0, 0.0), Align2::LEFT_CENTER, Align2::CENTER_BOTTOM),
                PinSide::Right => (edge - Vec2::new(4.0, 0.0), Align2::RIGHT_CENTER, Align2::CENTER_BOTTOM),
                PinSide::Top => (edge + Vec2::new(0.0, 4.0), Align2::CENTER_TOP, Align2::LEFT_CENTER),
                PinSide::Bottom => (edge - Vec2::new(0.0, 4.0), Align2::CENTER_BOTTOM, Align2::LEFT_CENTER),
            };
            if boxed {
                painter.text(name_at, name_align, &p.name, font.clone(), Color32::from_gray(40));
            } else {
                let (at, align) = match p.side {
                    PinSide::Left => (tip - Vec2::new(4.0, 0.0), Align2::RIGHT_CENTER),
                    PinSide::Right => (tip + Vec2::new(4.0, 0.0), Align2::LEFT_CENTER),
                    PinSide::Top => (tip - Vec2::new(0.0, 3.0), Align2::CENTER_BOTTOM),
                    PinSide::Bottom => (tip + Vec2::new(0.0, 3.0), Align2::CENTER_TOP),
                };
                painter.text(at, align, &p.name, font.clone(), Color32::from_gray(40));
            }
            let mid = tip + (inner - tip) / 2.0;
            let num_at = if matches!(p.side, PinSide::Left | PinSide::Right) { mid - Vec2::new(0.0, 2.0) } else { mid + Vec2::new(3.0, 0.0) };
            painter.text(num_at, num_align, &p.number, FontId::proportional(10.0), Color32::from_gray(110));
            let gap = sym.pins.iter().find(|s| s.number == p.number).map_or(0, |s| s.gap);
            if let Some(q) = resp.interact_pointer_pos().filter(|_| resp.dragged()) {
                let (side, before) = drop_at(&lay, &p.number, from(q));
                draw_drop_mark(&painter, &lay, &p.number, side, before, &to);
                painter.circle_filled(q, 4.0, crate::theme::ACCENT);
            }
            if let Some(q) = resp.interact_pointer_pos().filter(|_| resp.drag_stopped()) {
                let (side, before) = drop_at(&lay, &p.number, from(q));
                moved = Some((p.number.clone(), side, before));
            }
            let resp = resp.on_hover_text(i18n::trf!("ピン {} ({}): ドラッグで別の辺・順番へ。右クリックで前に空きを入れます", p.number, p.name));
            resp.context_menu(|ui| {
                if ui.button(i18n::tr("この前に空きを1つ入れる")).on_hover_text(i18n::tr("同じ辺で、このピンの前 (左右の辺なら上、上下の辺なら左) に、ピン1本分の空きを入れます")).clicked() {
                    gap_change = Some((p.number.clone(), 1));
                    ui.close();
                }
                if ui.add_enabled(gap > 0, egui::Button::new(i18n::tr("この前の空きを1つ減らす"))).on_hover_text(i18n::tr("このピンの前に入れた空きを、1つ減らします")).clicked() {
                    gap_change = Some((p.number.clone(), -1));
                    ui.close();
                }
            });
        }
        let mut rows = rows_of(&self.pins_text);
        let before = rows.clone();
        if let Some((number, side, at)) = moved {
            move_pin(&mut rows, &lay, &number, side, at);
        }
        if let Some((number, d)) = gap_change {
            // an automatic side would move when the box grows: fix them as drawn, like a drag does
            fixed_sides(&mut rows, &lay);
            if let Some(r) = rows.iter_mut().find(|r| r.number == number) {
                r.gap = (r.gap as i32 + d).clamp(0, MAX_GAP as i32) as u8;
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.small(i18n::tr("ピンをドラッグで動かす・右クリックで空き"));
            let custom = rows.iter().any(|r| !r.side.is_empty() || r.gap > 0);
            if ui.add_enabled(custom, egui::Button::new(i18n::tr("自動の配置に戻す")).small()).on_hover_text(i18n::tr("向きと空きを全部消して、自動の配置 (前半が左、後半が右) に戻します。ピンの行の順番はそのままです")).clicked() {
                for r in rows.iter_mut() {
                    r.side.clear();
                    r.gap = 0;
                }
            }
        });
        if rows != before {
            self.pins_text = text_of(&rows);
        }
    }
}

/// The place a dragged pin would go: a short bar where its lead will be, between the pins it falls between.
fn draw_drop_mark(painter: &egui::Painter, lay: &SymLayout, dragged: &str, side: PinSide, before: usize, to: &dyn Fn((f32, f32)) -> Pos2) {
    let vertical = matches!(side, PinSide::Left | PinSide::Right);
    let along: Vec<f32> = lay.pins.iter().filter(|p| p.side == side && p.number != dragged).map(|p| if vertical { p.tip.1 as f32 } else { p.tip.0 as f32 }).collect();
    let at = match (before.checked_sub(1).and_then(|i| along.get(i)), along.get(before)) {
        (Some(a), Some(b)) => (a + b) / 2.0,
        (Some(a), None) => a + 1.0,
        (None, Some(b)) => b - 1.0,
        (None, None) => 0.0,
    };
    let (hw, hh) = (lay.hw as f32, lay.hh as f32);
    let (a, b) = match side {
        PinSide::Left => ((-hw - 2.0, at), (-hw, at)),
        PinSide::Right => ((hw, at), (hw + 2.0, at)),
        PinSide::Top => ((at, -hh - 2.0), (at, -hh)),
        PinSide::Bottom => ((at, hh), (at, hh + 2.0)),
    };
    painter.line_segment([to(a), to(b)], Stroke::new(3.0, crate::theme::WARN));
}
