//! The prototype tab ("試作", candidate B6 ①): the schematic laid out on a solderless breadboard. The layout itself (which hole each
//! lead goes in, the jumpers, the supplies off the board) and the check against the schematic are `sch_core::breadboard`; this draws
//! the board, lights up the strip under the pointer, lets a part be dragged to other holes, and lists what the holes join that the
//! schematic does not (or the other way round). The layout is the sheet's (saved with it once the person moved something or asked for
//! a layout, `SheetSettings::breadboard`); until then it is laid out afresh from the schematic. Notes: `docs/code-notes/app/breadboard.md`.

use crate::learn::{spaced, NOTE};
use crate::schematic::SchEditor;
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use sch_core::breadboard::{auto_layout, check, moved, Hole, Layout, Look, Problem, Rail};
use std::collections::BTreeSet;

/// What the person is dragging: the part, and the hole the pointer started on.
struct Drag {
    refdes: String,
    from: Hole,
}

#[derive(Default)]
pub struct BreadboardView {
    /// The layout made from the schematic when none is saved, and the schematic's edit count it was made at.
    auto: Option<(u64, usize, Layout)>,
    drag: Option<Drag>,
    /// The part picked on the board or in the list.
    pub picked: Option<String>,
}

const BOARD: Color32 = Color32::from_rgb(236, 234, 226);
const HOLE: Color32 = Color32::from_rgb(70, 70, 74);
const RED: Color32 = Color32::from_rgb(214, 56, 50);
const BLUE: Color32 = Color32::from_rgb(52, 98, 196);
const LIT: Color32 = crate::theme::HIGHLIGHT;
const BAD: Color32 = crate::theme::ERROR;

/// Where a hole is drawn, in pitches: x from the left edge (four pitches left of the first column for the supplies off the board),
/// y from the top rail.
fn at(h: Hole) -> Vec2 {
    let x = 5.0 + h.col() as f32;
    let y = match h {
        Hole::Rail { rail: Rail::TopPlus, .. } => 0.0,
        Hole::Rail { rail: Rail::TopMinus, .. } => 1.0,
        Hole::Strip { row, .. } if row < 5 => 2.6 + row as f32,
        Hole::Strip { row, .. } => 4.4 + row as f32,
        Hole::Rail { rail: Rail::BottomMinus, .. } => 15.0,
        Hole::Rail { rail: Rail::BottomPlus, .. } => 16.0,
    };
    Vec2::new(x, y)
}

const HEIGHT: f32 = 16.0;

fn rail_colour(r: Rail) -> Color32 {
    match r {
        Rail::TopPlus | Rail::BottomPlus => RED,
        _ => BLUE,
    }
}

/// The colour of a jumper: red for the supply, black for ground, else one of a few by the net's name.
fn wire_colour(net: &str, l: &Layout) -> Color32 {
    let on_rail = |k: &str| l.rails.get(k).is_some_and(|n| n == net);
    if on_rail("TopMinus") || on_rail("BottomMinus") {
        return Color32::from_gray(30);
    }
    if on_rail("TopPlus") || on_rail("BottomPlus") {
        return RED;
    }
    const OTHERS: [Color32; 5] = [Color32::from_rgb(40, 140, 60), Color32::from_rgb(230, 170, 30), Color32::from_rgb(40, 110, 200), Color32::from_rgb(150, 70, 170), Color32::from_rgb(230, 110, 40)];
    let h = net.bytes().fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize));
    OTHERS[h % OTHERS.len()]
}

/// The colour bands of a resistor value (two digits, the multiplier, gold for 5 %).
fn bands(value: &str) -> Option<[Color32; 4]> {
    const C: [Color32; 10] = [
        Color32::from_rgb(20, 20, 20),
        Color32::from_rgb(120, 70, 30),
        Color32::from_rgb(210, 40, 40),
        Color32::from_rgb(240, 130, 30),
        Color32::from_rgb(240, 220, 40),
        Color32::from_rgb(40, 160, 60),
        Color32::from_rgb(40, 90, 210),
        Color32::from_rgb(140, 60, 180),
        Color32::from_rgb(130, 130, 130),
        Color32::from_rgb(250, 250, 250),
    ];
    let v = sim_core::parse_value(value).filter(|v| *v >= 1.0 && v.is_finite())?;
    let exp = v.log10().floor() as i32 - 1;
    let digits = (v / 10f64.powi(exp)).round() as i32;
    let (a, b) = ((digits / 10).clamp(0, 9) as usize, (digits % 10).clamp(0, 9) as usize);
    let m = exp.clamp(0, 9) as usize;
    Some([C[a], C[b], C[m], Color32::from_rgb(200, 160, 60)])
}

impl BreadboardView {
    /// The layout shown: the sheet's, else one made from the schematic (made again when the schematic changes).
    fn layout(&mut self, sch: &SchEditor) -> Layout {
        if let Some(l) = &sch.breadboard {
            return l.clone();
        }
        let key = (sch.rev, sch.sch.components.len());
        if !self.auto.as_ref().is_some_and(|(r, n, _)| (*r, *n) == key) {
            self.auto = Some((key.0, key.1, auto_layout(&sch.sch)));
        }
        self.auto.as_ref().map(|(_, _, l)| l.clone()).unwrap_or_default()
    }

    /// The left panel: what this is, the differences from the schematic, the parts and where their leads go.
    pub fn panel(&mut self, ui: &mut egui::Ui, sch: &mut SchEditor) {
        let l = self.layout(sch);
        crate::widgets::section_heading(ui, i18n::tr("ブレッドボード"));
        ui.label(spaced("回路図の部品を、ブレッドボードのどの穴に差すかを並べました。縦に並んだ 5 つの穴 (a〜e、f〜j) は中でつながっていて、上下の赤と青の列は電源とGNDです。部品をドラッグすると差し替えられます。").color(NOTE));
        let problems = check(&sch.sch, &l);
        ui.add_space(4.0);
        if problems.is_empty() {
            ui.colored_label(crate::theme::OK, i18n::tr("✔ 回路図どおりにつながっています"));
        } else {
            ui.colored_label(BAD, i18n::trf!("回路図と違う所が {} 件あります", problems.len()));
            for p in &problems {
                ui.label(egui::RichText::new(format!("・{}", i18n::tr_dyn(&p.message()))).color(BAD));
            }
        }
        ui.horizontal(|ui| {
            if ui.button(i18n::tr("自動で並べ直す")).on_hover_text(i18n::tr("回路図から、部品の差し方を並べ直します (IC は中央の溝をまたぎ、電源とGNDは上下の列、同じネットの足は同じ縦の列)。手で動かした分は消えます")).clicked() {
                sch.breadboard = Some(auto_layout(&sch.sch));
            }
            if sch.breadboard.is_none() {
                ui.label(egui::RichText::new(i18n::tr("(自動で並べた形)")).color(NOTE)).on_hover_text(i18n::tr("まだ手で動かしていないので、回路図を変えると並べ直されます。部品を動かすと、その並べ方が回路図のファイルに入ります"));
            }
        });
        ui.separator();
        ui.label(egui::RichText::new(i18n::tr("部品と差す穴")).strong());
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("bb_parts").num_columns(2).striped(true).spacing([10.0, 2.0]).show(ui, |ui| {
                for p in &l.parts {
                    let picked = self.picked.as_deref() == Some(p.refdes.as_str());
                    if ui.selectable_label(picked, egui::RichText::new(&p.refdes).strong()).on_hover_text(i18n::tr("選ぶと、ブレッドボードの上でこの部品が光ります")).clicked() {
                        self.picked = if picked { None } else { Some(p.refdes.clone()) };
                    }
                    ui.label(p.leads.iter().map(|(pin, h)| format!("{pin}:{}", h.name())).collect::<Vec<_>>().join("  "));
                    ui.end_row();
                }
            });
            if !l.off_board.is_empty() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(i18n::tr("ブレッドボードの外 (電源・電池・信号源)")).strong()).on_hover_text(i18n::tr("実験用の電源・電池・発振器などは、ブレッドボードの外に置いて、線でつなぎます"));
                for o in &l.off_board {
                    ui.label(format!("{}  {}", o.refdes, o.pins.iter().map(|(pin, net, h)| format!("{pin} ({net}) → {}", h.name())).collect::<Vec<_>>().join("、")));
                }
            }
        });
    }

    /// The board. `focus`: the part picked in the schematic, lit here too.
    pub fn canvas(&mut self, ui: &mut egui::Ui, sch: &mut SchEditor, focus: Option<&str>) {
        let mut l = self.layout(sch);
        let (response, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
        let rect = response.rect;
        painter.rect_filled(rect, 0.0, crate::canvas_theme::col(crate::canvas_theme::Role::Bg));
        if l.parts.is_empty() && l.off_board.is_empty() {
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, i18n::tr("回路図に部品がありません。回路図の画面で部品を置いて配線すると、ここにブレッドボードの差し方が出ます"), egui::FontId::proportional(16.0), Color32::from_gray(200));
            return;
        }
        let width = 5.0 + l.cols as f32 + 1.0;
        let pitch = ((rect.width() - 40.0) / width).min((rect.height() - 60.0) / (HEIGHT + 2.0)).clamp(6.0, 28.0);
        let origin = Pos2::new(rect.left() + (rect.width() - width * pitch) / 2.0, rect.top() + 40.0 + pitch);
        let to = |h: Hole| origin + at(h) * pitch;
        let cols = l.cols;
        let hole_at = |p: Pos2| -> Option<Hole> {
            let v = (p - origin) / pitch;
            let col = (v.x - 5.0).round();
            if col < 0.0 || col >= cols as f32 {
                return None;
            }
            let col = col as u16;
            let cands = [Hole::Rail { rail: Rail::TopPlus, col }, Hole::Rail { rail: Rail::TopMinus, col }, Hole::Rail { rail: Rail::BottomMinus, col }, Hole::Rail { rail: Rail::BottomPlus, col }];
            cands.into_iter().chain((0..10).map(|row| Hole::Strip { col, row })).find(|h| (at(*h).y - v.y).abs() < 0.5)
        };
        // dragging: the part follows the pointer by whole holes; let go to keep it there
        if response.drag_started()
            && let Some(p) = response.interact_pointer_pos()
            && let Some(h) = hole_at(p)
            && let Some(part) = l.parts.iter().find(|x| part_rect(x, to, pitch).expand(pitch * 0.6).contains(p))
        {
            self.drag = Some(Drag { refdes: part.refdes.clone(), from: h });
            self.picked = Some(part.refdes.clone());
        }
        let mut dragging = false;
        if let Some(d) = &self.drag
            && let Some(p) = response.interact_pointer_pos().or(ui.ctx().pointer_latest_pos())
            && let Some(h) = hole_at(p)
        {
            let dcol = h.col() as i32 - d.from.col() as i32;
            // rows count between strip holes (a step across the groove is one row); a rail lead only moves along its rail
            let drow = match (d.from, h) {
                (Hole::Strip { row: a, .. }, Hole::Strip { row: b, .. }) => b as i32 - a as i32,
                _ => 0,
            };
            if let Some(m) = moved(&l, &d.refdes, dcol, drow) {
                if response.drag_stopped() {
                    sch.breadboard = Some(m.clone());
                } else {
                    dragging = true;
                }
                l = m;
            }
        }
        if response.drag_stopped() {
            self.drag = None;
        }
        let problems = check(&sch.sch, &l);
        // the leads named in a problem, drawn with a red ring
        let mut wrong: BTreeSet<Hole> = BTreeSet::new();
        for p in &problems {
            let names: Vec<String> = match p {
                Problem::Short { at, .. } => at.clone(),
                Problem::Open { apart, .. } => apart.iter().flat_map(|a| a.split(',').map(String::from).collect::<Vec<_>>()).collect(),
                Problem::SameHole { leads, .. } => leads.clone(),
                Problem::Missing { .. } => vec![],
            };
            for n in names {
                let Some((r, pin)) = n.split_once('.') else { continue };
                for part in l.parts.iter().filter(|x| x.refdes == r) {
                    wrong.extend(part.leads.iter().filter(|(p2, _)| p2 == pin).map(|(_, h)| *h));
                }
            }
        }
        // the board
        let board = Rect::from_min_max(origin + Vec2::new(4.3, -0.8) * pitch, origin + Vec2::new(width - 0.3, HEIGHT + 0.8) * pitch);
        painter.rect_filled(board, pitch * 0.4, BOARD);
        painter.rect_filled(Rect::from_min_max(origin + Vec2::new(4.6, 7.6) * pitch, origin + Vec2::new(width - 0.6, 9.4) * pitch), 0.0, Color32::from_rgb(214, 212, 204));
        for (y, c) in [(-0.5, RED), (1.5, BLUE), (14.5, BLUE), (16.5, RED)] {
            painter.line_segment([origin + Vec2::new(4.6, y) * pitch, origin + Vec2::new(width - 0.6, y) * pitch], Stroke::new(1.5, c));
        }
        // the strip (or rail) under the pointer, lit
        let hover = response.hover_pos().and_then(hole_at);
        let lit_node = hover.map(|h| h.node());
        let small = egui::FontId::proportional((pitch * 0.55).clamp(8.0, 12.0));
        for row in 0..10u8 {
            let y = at(Hole::Strip { col: 0, row }).y;
            painter.text(origin + Vec2::new(4.6, y) * pitch, egui::Align2::CENTER_CENTER, ((b'a' + row) as char).to_string(), small.clone(), Color32::from_gray(120));
        }
        for col in 0..l.cols {
            if col % 5 == 4 || col == 0 {
                painter.text(origin + Vec2::new(5.0 + col as f32, 2.0) * pitch, egui::Align2::CENTER_CENTER, (col + 1).to_string(), small.clone(), Color32::from_gray(120));
            }
            let holes = (0..10).map(|row| Hole::Strip { col, row }).chain([Rail::TopPlus, Rail::TopMinus, Rail::BottomMinus, Rail::BottomPlus].map(|rail| Hole::Rail { rail, col }));
            for h in holes {
                let p = to(h);
                let lit = lit_node == Some(h.node());
                if lit {
                    painter.rect_filled(Rect::from_center_size(p, Vec2::splat(pitch * 0.8)), 2.0, LIT.gamma_multiply(0.6));
                }
                painter.rect_filled(Rect::from_center_size(p, Vec2::splat(pitch * 0.32)), 1.0, if lit { Color32::from_gray(20) } else { HOLE });
            }
        }
        for (key, net) in &l.rails {
            let (y, rail) = match key.as_str() {
                "TopPlus" => (0.0, Rail::TopPlus),
                "TopMinus" => (1.0, Rail::TopMinus),
                "BottomMinus" => (15.0, Rail::BottomMinus),
                _ => (16.0, Rail::BottomPlus),
            };
            painter.text(origin + Vec2::new(width - 0.1, y) * pitch, egui::Align2::LEFT_CENTER, net, small.clone(), rail_colour(rail));
        }
        // the wires, then the parts on top
        for w in &l.wires {
            draw_wire(&painter, to(w.a), to(w.b), wire_colour(&w.net, &l), pitch);
        }
        let font = egui::FontId::proportional((pitch * 0.6).clamp(9.0, 14.0));
        for (k, o) in l.off_board.iter().enumerate() {
            let center = origin + Vec2::new(2.0, 3.0 + 4.0 * k as f32) * pitch;
            let r = Rect::from_center_size(center, Vec2::new(3.2, 2.4) * pitch);
            painter.rect(r, 3.0, Color32::from_rgb(60, 64, 72), Stroke::new(1.0, Color32::from_gray(160)), egui::StrokeKind::Middle);
            let value = sch.sch.components.iter().find(|c| c.refdes == o.refdes).map(|c| c.value.clone()).unwrap_or_default();
            painter.text(center - Vec2::new(0.0, 0.4 * pitch), egui::Align2::CENTER_CENTER, &o.refdes, font.clone(), Color32::WHITE);
            painter.text(center + Vec2::new(0.0, 0.5 * pitch), egui::Align2::CENTER_CENTER, short(&value, 10), small.clone(), Color32::from_gray(200));
            for (i, (pin, net, h)) in o.pins.iter().enumerate() {
                let from = r.right_center() + Vec2::new(0.0, (i as f32 - 0.5) * 0.8 * pitch);
                draw_wire(&painter, from, to(*h), wire_colour(net, &l), pitch);
                painter.text(from + Vec2::new(-0.2 * pitch, 0.0), egui::Align2::RIGHT_CENTER, pin, small.clone(), Color32::WHITE);
            }
        }
        let focus = focus.map(String::from);
        let mut hovered_part = None;
        for p in &l.parts {
            let lit = self.picked.as_deref() == Some(p.refdes.as_str()) || focus.as_deref() == Some(p.refdes.as_str()) || self.drag.as_ref().is_some_and(|d| d.refdes == p.refdes);
            let comp = sch.sch.components.iter().find(|c| c.refdes == p.refdes);
            let rect = draw_part(&painter, p, comp, to, pitch, &font, lit);
            if response.hover_pos().is_some_and(|h| rect.contains(h)) {
                hovered_part = Some((p.refdes.clone(), comp.map(|c| c.value.clone()).unwrap_or_default()));
            }
        }
        for h in &wrong {
            painter.circle_stroke(to(*h), pitch * 0.55, Stroke::new(2.5, BAD));
        }
        // what is under the pointer
        if !dragging {
            if let Some((r, v)) = &hovered_part {
                response.clone().on_hover_text(i18n::trf!("{r} {v}\nドラッグで差し替え"));
            } else if let Some(h) = hover {
                let here: Vec<String> = l.parts.iter().flat_map(|p| p.leads.iter().filter(|(_, lh)| lh.node() == h.node()).map(move |(pin, _)| format!("{}.{pin}", p.refdes))).collect();
                let tip = if here.is_empty() { h.name() } else { format!("{}: {}", h.name(), here.join(", ")) };
                response.clone().on_hover_text(tip);
            }
        }
        if response.clicked() {
            self.picked = hovered_part.map(|(r, _)| r);
        }
        let legend = i18n::tr("黄: ポインタの下の穴とつながっている穴　赤い丸: 回路図と違う足　部品はドラッグで差し替え");
        painter.text(rect.left_top() + Vec2::new(12.0, 10.0), egui::Align2::LEFT_TOP, legend, egui::FontId::proportional(13.0), Color32::from_gray(210));
    }

    /// The status line's hint.
    pub fn hint(&self) -> String {
        "縦の 5 つの穴は中でつながっています。ポインタを穴に合わせると、つながっている穴が光ります。部品をドラッグすると差し替えられ、回路図と違うつなぎ方は赤い丸と左の一覧で分かります".into()
    }
}

fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n).collect::<String>()) }
}

/// A jumper wire: a gentle arc between two holes, with its ends.
fn draw_wire(painter: &egui::Painter, a: Pos2, b: Pos2, c: Color32, pitch: f32) {
    let mid = a + (b - a) / 2.0 - Vec2::new(0.0, ((b - a).length() * 0.15).min(pitch * 1.5));
    let pts: Vec<Pos2> = (0..=12).map(|i| {
        let t = i as f32 / 12.0;
        let u = 1.0 - t;
        Pos2::new(u * u * a.x + 2.0 * u * t * mid.x + t * t * b.x, u * u * a.y + 2.0 * u * t * mid.y + t * t * b.y)
    }).collect();
    painter.add(egui::Shape::line(pts, Stroke::new((pitch * 0.22).max(2.0), c)));
    for p in [a, b] {
        painter.circle_filled(p, pitch * 0.16, Color32::from_gray(200));
    }
}

fn part_rect(p: &sch_core::breadboard::Placed, to: impl Fn(Hole) -> Pos2, pitch: f32) -> Rect {
    let mut r = Rect::NOTHING;
    for (_, h) in &p.leads {
        r = r.union(Rect::from_center_size(to(*h), Vec2::splat(pitch * 0.9)));
    }
    r
}

/// Draws a part and returns the area it takes (for the pointer).
fn draw_part(painter: &egui::Painter, p: &sch_core::breadboard::Placed, comp: Option<&sch_core::Component>, to: impl Fn(Hole) -> Pos2 + Copy, pitch: f32, font: &egui::FontId, lit: bool) -> Rect {
    let outline = if lit { Stroke::new(2.5, LIT) } else { Stroke::new(1.0, Color32::from_gray(40)) };
    let kind = comp.map(|c| c.kind);
    let value = comp.map(|c| c.value.as_str()).unwrap_or("");
    let lead = Stroke::new((pitch * 0.12).max(1.5), Color32::from_gray(150));
    match p.look {
        Look::Dip => {
            let r = part_rect(p, to, pitch).shrink(pitch * 0.15);
            let body = Rect::from_min_max(Pos2::new(r.left(), r.top() + pitch * 0.25), Pos2::new(r.right(), r.bottom() - pitch * 0.25));
            for (_, h) in &p.leads {
                let c = to(*h);
                painter.rect_filled(Rect::from_center_size(c, Vec2::splat(pitch * 0.35)), 1.0, Color32::from_gray(190));
            }
            painter.rect(body, 2.0, Color32::from_rgb(34, 34, 38), outline, egui::StrokeKind::Middle);
            painter.circle_filled(Pos2::new(body.left() + pitch * 0.15, body.center().y), pitch * 0.3, BOARD.gamma_multiply(0.6));
            painter.text(body.center(), egui::Align2::CENTER_CENTER, format!("{} {}", p.refdes, short(value, 12)), font.clone(), Color32::WHITE);
            r
        }
        Look::Row => {
            let r = part_rect(p, to, pitch);
            // the body over the groove, below the leads in row e
            let body = Rect::from_min_max(Pos2::new(r.min.x, r.max.y - pitch * 0.2), Pos2::new(r.max.x, r.max.y + pitch * 1.1));
            for (_, h) in &p.leads {
                painter.line_segment([to(*h), Pos2::new(to(*h).x, body.top())], lead);
            }
            let fill = if matches!(kind, Some(sch_core::Kind::Header(_))) { Color32::from_rgb(30, 30, 30) } else { Color32::from_rgb(40, 40, 44) };
            painter.rect(body, pitch * 0.25, fill, outline, egui::StrokeKind::Middle);
            painter.text(body.center(), egui::Align2::CENTER_CENTER, &p.refdes, font.clone(), Color32::WHITE);
            r.union(body)
        }
        Look::TwoLead => {
            let (Some((_, a)), Some((_, b))) = (p.leads.first(), p.leads.get(1)) else { return Rect::NOTHING };
            let (pa, pb) = (to(*a), to(*b));
            painter.line_segment([pa, pb], lead);
            let mid = pa + (pb - pa) / 2.0;
            let dir = (pb - pa).normalized();
            let len = ((pb - pa).length() * 0.55).clamp(pitch * 0.8, pitch * 2.2);
            let body = Rect::from_center_size(mid, if dir.x.abs() >= dir.y.abs() { Vec2::new(len, pitch * 0.55) } else { Vec2::new(pitch * 0.55, len) });
            match kind {
                Some(sch_core::Kind::Resistor) => {
                    painter.rect(body, pitch * 0.25, Color32::from_rgb(222, 196, 150), outline, egui::StrokeKind::Middle);
                    if let Some(bs) = bands(value) {
                        for (i, c) in bs.iter().enumerate() {
                            let t = 0.25 + 0.17 * i as f32 - 0.5;
                            let at = body.center() + if dir.x.abs() >= dir.y.abs() { Vec2::new(t * body.width() * dir.x.signum(), 0.0) } else { Vec2::new(0.0, t * body.height() * dir.y.signum()) };
                            let size = if dir.x.abs() >= dir.y.abs() { Vec2::new(pitch * 0.12, body.height()) } else { Vec2::new(body.width(), pitch * 0.12) };
                            painter.rect_filled(Rect::from_center_size(at, size), 0.0, *c);
                        }
                    }
                }
                Some(sch_core::Kind::Led) => {
                    painter.circle(mid, pitch * 0.55, Color32::from_rgb(230, 60, 60).gamma_multiply(0.85), outline);
                    // the cathode's flat side
                    let k = to(*b);
                    let n = (k - mid).normalized() * pitch * 0.45;
                    painter.line_segment([mid + n + n.rot90() * 0.9, mid + n - n.rot90() * 0.9], Stroke::new(2.0, Color32::from_gray(40)));
                    painter.text(mid, egui::Align2::CENTER_CENTER, &p.refdes, font.clone(), Color32::WHITE);
                    return body.union(Rect::from_center_size(mid, Vec2::splat(pitch * 1.2)));
                }
                Some(sch_core::Kind::Diode) => {
                    painter.rect(body, pitch * 0.15, Color32::from_rgb(30, 30, 30), outline, egui::StrokeKind::Middle);
                    // the band at the cathode
                    let k = body.center() + (to(*b) - mid).normalized() * (len * 0.35);
                    let size = if dir.x.abs() >= dir.y.abs() { Vec2::new(pitch * 0.14, body.height()) } else { Vec2::new(body.width(), pitch * 0.14) };
                    painter.rect_filled(Rect::from_center_size(k, size), 0.0, Color32::from_gray(210));
                }
                Some(sch_core::Kind::Capacitor) => {
                    painter.rect(body, pitch * 0.25, Color32::from_rgb(90, 140, 210), outline, egui::StrokeKind::Middle);
                }
                Some(sch_core::Kind::Inductor) => {
                    painter.rect(body, pitch * 0.25, Color32::from_rgb(80, 150, 90), outline, egui::StrokeKind::Middle);
                }
                _ => {
                    painter.rect(body, pitch * 0.2, Color32::from_rgb(60, 60, 70), outline, egui::StrokeKind::Middle);
                }
            }
            let label_at = body.center() + if dir.x.abs() >= dir.y.abs() { Vec2::new(0.0, -pitch * 0.75) } else { Vec2::new(pitch * 0.9, 0.0) };
            painter.text(label_at, egui::Align2::CENTER_CENTER, &p.refdes, font.clone(), Color32::from_gray(30));
            body.expand(pitch * 0.2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{App, Mode};

    #[test]
    fn resistor_bands_read_the_value() {
        let b = bands("4.7k").unwrap();
        // yellow, violet, red (x100), gold
        assert_eq!(b[0], Color32::from_rgb(240, 220, 40));
        assert_eq!(b[1], Color32::from_rgb(140, 60, 180));
        assert_eq!(b[2], Color32::from_rgb(210, 40, 40));
        assert_eq!(bands("10").unwrap()[2], Color32::from_rgb(20, 20, 20), "10 = brown black black");
        assert!(bands("x").is_none());
    }

    #[test]
    fn the_tab_draws_a_sample_and_a_drag_is_kept_with_the_sheet() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let mut app = App::default();
        app.sch.load_path(root.join("ne555_blinker.schjson"));
        app.mode = Mode::Proto;
        let ctx = egui::Context::default();
        let mut f = eframe::Frame::_new_kittest();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
        let mut run = |app: &mut App, events: Vec<egui::Event>| {
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
            out.textures_delta.clear();
        };
        run(&mut app, vec![]);
        assert!(app.sch.breadboard.is_none(), "looking does not change the sheet");
        assert!(!app.sch.dirty());
        // the layout kept with the sheet once it is asked for, and read back from the file
        app.sch.breadboard = Some(auto_layout(&app.sch.sch));
        assert!(app.sch.dirty());
        let mut v = serde_json::json!({});
        crate::schematic::with_settings(&mut v, &app.sch.sheet_settings());
        let back = crate::schematic::take_settings(&mut v);
        assert_eq!(back.breadboard, app.sch.breadboard);
        run(&mut app, vec![]);
        // an empty schematic says what to do
        let mut empty = App::default();
        empty.mode = Mode::Proto;
        run(&mut empty, vec![]);
    }
}
