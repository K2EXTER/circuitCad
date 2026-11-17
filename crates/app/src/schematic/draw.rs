//! Painting the sheet: grid, wires, parts, labels and the analysis overlays.

use super::*;

/// Below this a part's DC current is not written on the sheet (it reads as 0 and only adds clutter).
pub(super) const MIN_SHOWN_CURRENT: f64 = 1e-9;

/// The tiers of text on the sheet (`docs/code-notes/app/ui-rules.md`, 2026-11-17). Each follows the zoom, up to
/// `TEXT_MAX`. Main: the voltages, currents and net names, always drawn (at least `MAIN_MIN`). Sub: the parts' names and
/// values and the pin names inside a box, left out when smaller than `SUB_MIN`. The pin numbers are the third tier, drawn in
/// the single-stroke font at the drawing's own scale (`pin_number`).
#[derive(Clone, Copy)]
pub(super) enum Tier {
    Main,
    Sub,
}

const TEXT_MAX: f32 = 18.0;
const MAIN_MIN: f32 = 10.0;
const SUB_MIN: f32 = 9.0;
/// Below this height (px) the strokes of a pin number run together: it is left out.
const PIN_NUMBER_MIN: f32 = 4.0;

impl SchEditor {
    /// The font of a tier at this zoom, `ratio` times the grid step; `None` when a sub text would be too small to read.
    pub(super) fn tier_font(&self, painter: &egui::Painter, tier: Tier, ratio: f32) -> Option<FontId> {
        let s = self.zoom * ratio;
        let s = match tier {
            Tier::Main => s.clamp(MAIN_MIN, TEXT_MAX),
            Tier::Sub if s < SUB_MIN => return None,
            Tier::Sub => s.min(TEXT_MAX),
        };
        Some(crate::theme::canvas_font(painter, s))
    }

    /// The main text's font (it is always drawn).
    pub(super) fn main_font(&self, painter: &egui::Painter) -> FontId {
        self.tier_font(painter, Tier::Main, 0.6).unwrap_or_else(|| FontId::proportional(MAIN_MIN))
    }

    /// A pin number in the single-stroke font, centred on `at`, scaled with the drawing like the symbol's lines: it shrinks
    /// with the part instead of staying a fixed size over it, and its strokes never drop out the way a small font's do.
    pub(super) fn pin_number(&self, painter: &egui::Painter, at: Pos2, text: &str) {
        let h = self.zoom * 0.3;
        if h < PIN_NUMBER_MIN {
            return;
        }
        let w = pcb_core::strokefont::text_width(text, h as f64) as f32;
        let at = at + Vec2::new(-w / 2.0, h / 2.0);
        let stroke = Stroke::new((h / 9.0).clamp(0.8, 1.6), crate::canvas_theme::col(crate::canvas_theme::Role::PinNumber));
        for [a, b] in pcb_core::strokefont::text_strokes(text, h as f64, false) {
            painter.line_segment([Pos2::new(at.x + a.x as f32, at.y - a.y as f32), Pos2::new(at.x + b.x as f32, at.y - b.y as f32)], stroke);
        }
    }

    pub(super) fn draw_grid(&self, painter: &egui::Painter, rect: Rect) {
        // grid dots
        let tl = self.to_grid(rect, rect.left_top());
        let br = self.to_grid(rect, rect.right_bottom());
        if self.zoom >= 10.0 {
            // one mesh of little squares instead of a circle shape per dot (a big sheet has thousands)
            let mut dots = egui::epaint::Mesh::default();
            for x in (tl.0.floor() as i32)..=(br.0.ceil() as i32) {
                for y in (tl.1.floor() as i32)..=(br.1.ceil() as i32) {
                    dots.add_colored_rect(Rect::from_center_size(self.to_screen(rect, (x as f32, y as f32)), egui::Vec2::splat(1.8)), crate::canvas_theme::col(crate::canvas_theme::Role::Grid));
                }
            }
            painter.add(egui::Shape::mesh(dots));
        }
    }

    pub(super) fn draw_wires(&self, painter: &egui::Painter, rect: Rect) {
        // wires (tinted by the net voltage when a DC result is showing)
        let live_dc = self.dc.as_ref().filter(|d| d.outcome.is_ok());
        let scale = live_dc.filter(|_| self.show_v).map(|d| self.volt_scale_of(d));
        // a result from before the last change keeps its colours, faint
        let faint = self.dc_stale();
        for (i, w) in self.sch.wires.iter().enumerate() {
            let sel = self.is_sel(Sel::Wire(i));
            let tint = live_dc.zip(scale).and_then(|(d, (r, range))| {
                let c = step_colour(volt_step(self.wire_voltage(d, i)? - r, range, self.vscale.decades));
                Some(if faint { c.gamma_multiply(0.45) } else { c })
            });
            painter.line_segment(
                [self.to_screen(rect, (w.a.0 as f32, w.a.1 as f32)), self.to_screen(rect, (w.b.0 as f32, w.b.1 as f32))],
                Stroke::new(if sel { 3.0 } else { 2.2 }, if sel { crate::canvas_theme::col(crate::canvas_theme::Role::Selection) } else { tint.unwrap_or(crate::canvas_theme::col(crate::canvas_theme::Role::Wire)) }),
            );
        }
    }

    /// Bus lines: thick and blue, apart from the wires (they connect nothing).
    pub(super) fn draw_buses(&self, painter: &egui::Painter, rect: Rect) {
        for (i, w) in self.sch.buses.iter().enumerate() {
            let sel = self.is_sel(Sel::Bus(i));
            painter.line_segment(
                [self.to_screen(rect, (w.a.0 as f32, w.a.1 as f32)), self.to_screen(rect, (w.b.0 as f32, w.b.1 as f32))],
                Stroke::new(if sel { 6.0 } else { 5.0 }, if sel { crate::canvas_theme::col(crate::canvas_theme::Role::Selection) } else { crate::canvas_theme::col(crate::canvas_theme::Role::Bus) }),
            );
        }
    }

    pub(super) fn draw_junctions(&self, painter: &egui::Painter, rect: Rect) {
        // junction dots
        let pins: Vec<Pt> = self.sch.components.iter().flat_map(|c| c.pins().into_iter().map(|p| p.1)).collect();
        for w in &self.sch.wires {
            for p in [w.a, w.b] {
                let ends = self.sch.wires.iter().map(|x| (x.a == p) as u32 + (x.b == p) as u32).sum::<u32>()
                    + pins.iter().filter(|&&q| q == p).count() as u32;
                let on_other = self.sch.wires.iter().any(|x| x.a != p && x.b != p && on_segment(p, x.a, x.b));
                if ends >= 3 || on_other {
                    painter.circle_filled(self.to_screen(rect, (p.0 as f32, p.1 as f32)), 3.5, crate::canvas_theme::col(crate::canvas_theme::Role::Wire));
                }
            }
        }
    }

    pub(super) fn draw_components(&self, painter: &egui::Painter, rect: Rect) {
        // components
        for (i, c) in self.sch.components.iter().enumerate() {
            let sel = self.is_sel(Sel::Comp(i));
            let col = if sel { crate::canvas_theme::col(crate::canvas_theme::Role::Selection) } else { crate::canvas_theme::col(crate::canvas_theme::Role::Part) };
            self.draw_component(painter, rect, c, col);
            for (_, p) in c.pins() {
                painter.rect_filled(
                    Rect::from_center_size(self.to_screen(rect, (p.0 as f32, p.1 as f32)), Vec2::splat(4.0)),
                    0.0,
                    crate::canvas_theme::col(crate::canvas_theme::Role::Part),
                );
            }
            // the pins left open on purpose: a cross over the pin
            for (pin, p) in c.pins() {
                if self.sch.is_no_connect(&c.refdes, &pin) {
                    let at = self.to_screen(rect, (p.0 as f32, p.1 as f32));
                    let s = Stroke::new(1.8, crate::canvas_theme::col(crate::canvas_theme::Role::NoConnect));
                    painter.line_segment([at + Vec2::new(-4.5, -4.5), at + Vec2::new(4.5, 4.5)], s);
                    painter.line_segment([at + Vec2::new(-4.5, 4.5), at + Vec2::new(4.5, -4.5)], s);
                }
            }
            let f = self.main_font(painter);
            let (x0, x1, y0, y1) = comp_extents(c);
            if c.kind.is_power_symbol() {
                if c.kind == Kind::Power {
                    painter.text(self.to_screen(rect, (c.pos.0 as f32, c.pos.1 as f32 - 0.8)), Align2::CENTER_BOTTOM, &c.value, f.clone(), crate::canvas_theme::col(crate::canvas_theme::Role::Part));
                }
            } else if let Some(f) = self.tier_font(painter, Tier::Sub, 0.5) {
                let name_col = crate::canvas_theme::col(crate::canvas_theme::Role::PartName);
                // the part's box on the sheet after rotation; a part taller than wide has its pins at the top and bottom
                let (bx0, bx1, by0, by1) = sheet_box(c.kind, (x0, x1, y0, y1), c.pos, c.rot)
                    .unwrap_or((c.pos.0 as f32 - 2.0, c.pos.0 as f32 + 2.0, c.pos.1 as f32 - 1.0, c.pos.1 as f32 + 1.0));
                // a part with pins down its sides (an IC) has its wires and net labels left and right of the body: its name and
                // value go above and below, like a wide part's
                if by1 - by0 <= bx1 - bx0 || side_pinned(c, bx0, bx1) {
                    let cx = (bx0 + bx1) / 2.0;
                    // a pin on the centre line of the top / bottom edge has its wire running through the centred text:
                    // put the text just left of that wire instead (the voltages are drawn right of the pins)
                    let wire_at = |y: f32| pin_on_edge(c, y, cx);
                    // the arrows of an LED rise 1.5 above its centre (or hang 1.5 below when it is turned upside down)
                    let (arrows_up, arrows_down) = (c.kind == Kind::Led && c.rot % 4 == 0, c.kind == Kind::Led && c.rot % 4 == 2);
                    let name_y = by0 - if arrows_up { 1.2 } else { 0.3 };
                    let value_y = by1 + if arrows_down { 1.2 } else { 0.3 };
                    // a four-pin crystal has its two case pins (and their wires) on one side: name and value both go on the other
                    let case_pins = c.custom.as_ref().is_some_and(|s| s.pins.len() == 4 && super::glyphs::body_glyph(s) == Some("crystal")) && c.rot % 2 == 0;
                    if case_pins {
                        let up = c.rot % 4 == 0;
                        let y = if up { name_y } else { value_y };
                        let (left, right) = if up { (Align2::RIGHT_BOTTOM, Align2::LEFT_BOTTOM) } else { (Align2::RIGHT_TOP, Align2::LEFT_TOP) };
                        painter.text(self.to_screen(rect, (cx - 0.3, y)), left, &c.refdes, f.clone(), name_col);
                        painter.text(self.to_screen(rect, (cx + 0.3, y)), right, &c.value, f.clone(), crate::canvas_theme::col(crate::canvas_theme::Role::Value));
                    } else if wire_at(by0) {
                        painter.text(self.to_screen(rect, (cx - 0.4, name_y)), Align2::RIGHT_BOTTOM, &c.refdes, f.clone(), name_col);
                    } else {
                        painter.text(self.to_screen(rect, (cx, name_y)), Align2::CENTER_BOTTOM, &c.refdes, f.clone(), name_col);
                    }
                    if !case_pins {
                        if wire_at(by1) {
                            painter.text(self.to_screen(rect, (cx - 0.4, value_y)), Align2::RIGHT_TOP, &c.value, f, crate::canvas_theme::col(crate::canvas_theme::Role::Value));
                        } else {
                            painter.text(self.to_screen(rect, (cx, value_y)), Align2::CENTER_TOP, &c.value, f, crate::canvas_theme::col(crate::canvas_theme::Role::Value));
                        }
                    }
                } else {
                    // name and value go left of the body, level with its middle, so they stay clear of the pin
                    // voltages (right of each pin) and the wires that run along the pins' line
                    let (left, mid) = (bx0 - 0.4, (by0 + by1) / 2.0);
                    painter.text(self.to_screen(rect, (left, mid)), Align2::RIGHT_BOTTOM, &c.refdes, f.clone(), name_col);
                    painter.text(self.to_screen(rect, (left, mid)), Align2::RIGHT_TOP, &c.value, f, crate::canvas_theme::col(crate::canvas_theme::Role::Value));
                }
            }
        }
    }

    pub(super) fn draw_labels(&self, painter: &egui::Painter, rect: Rect) {
        for (i, l) in self.sch.labels.iter().enumerate() {
            let sel = self.is_sel(Sel::Label(i));
            let p = self.to_screen(rect, (l.pos.0 as f32, l.pos.1 as f32));
            painter.circle_filled(p, 3.0, crate::canvas_theme::col(crate::canvas_theme::Role::Wire));
            painter.text(
                p + Vec2::new(5.0, -3.0),
                Align2::LEFT_BOTTOM,
                &l.text,
                self.main_font(painter),
                if sel { crate::canvas_theme::col(crate::canvas_theme::Role::Selection) } else if sch_core::is_bus_label(&l.text) { crate::canvas_theme::col(crate::canvas_theme::Role::Bus) } else { crate::canvas_theme::col(crate::canvas_theme::Role::NetLabel) },
            );
        }
    }

    /// The net label a pin's voltage must keep clear of, and where the voltage goes: after the label's name, before its dot, or
    /// (for a pin on the top or bottom edge, whose neighbours are close) stacked above the name. The label sits on the pin point
    /// itself, or at the far end of a short level wire from the pin (to the right: after the name; to the left: before the dot).
    fn label_anchor(&self, pt: Pt) -> Option<(&sch_core::Label, VoltageSpot)> {
        if let Some(l) = self.sch.labels.iter().find(|l| l.pos == pt) {
            let part_is_right = self.sch.wires.iter().any(|w| (w.a == pt && w.b.1 == pt.1 && w.b.0 > pt.0) || (w.b == pt && w.a.1 == pt.1 && w.a.0 > pt.0))
                || self.sch.components.iter().any(|c| c.pins().iter().any(|p| p.1 == pt) && pin_left_of_body(c, pt));
            let on_top_or_bottom = self.sch.components.iter().any(|c| c.pins().iter().any(|p| p.1 == pt) && pin_beyond_body_vertically(c, pt));
            let spot = if on_top_or_bottom {
                VoltageSpot::AboveName
            } else if part_is_right {
                VoltageSpot::BeforeDot
            } else {
                VoltageSpot::AfterName
            };
            return Some((l, spot));
        }
        self.sch.wires.iter().find_map(|w| {
            let other = if w.a == pt { w.b } else if w.b == pt { w.a } else { return None };
            if other.1 != pt.1 || (other.0 - pt.0).abs() > 6 {
                return None;
            }
            self.sch.labels.iter().find(|l| l.pos == other).map(|l| (l, if other.0 > pt.0 { VoltageSpot::AfterName } else { VoltageSpot::BeforeDot }))
        })
    }

    /// Where each net label's name is drawn, on the screen.
    fn label_rects(&self, painter: &egui::Painter, rect: Rect) -> Vec<Rect> {
        self.sch
            .labels
            .iter()
            .map(|l| {
                let g = painter.layout_no_wrap(l.text.clone(), self.main_font(painter), Color32::BLACK);
                let at = self.to_screen(rect, (l.pos.0 as f32, l.pos.1 as f32)) + Vec2::new(5.0, -3.0);
                Rect::from_min_size(Pos2::new(at.x, at.y - g.size().y), g.size())
            })
            .collect()
    }

    pub(super) fn draw_voltages(&self, painter: &egui::Painter, rect: Rect) {
        if self.show_v
            && let Some(dc) = self.dc.as_ref() {
                let painter = &self.result_painter(painter);
                let f = self.main_font(painter);
                for (pt, v) in &dc.marks {
                    let galley = painter.layout_no_wrap(eng(*v, "V"), f.clone(), crate::canvas_theme::col(crate::canvas_theme::Role::Voltage));
                    // a pin that has a net label on it or at the end of a short level wire: the voltage goes beside the label
                    // (after its name, or before its dot), not on top of it
                    // a power symbol (VCC, 5V ...) on the pin has its value drawn above it: the voltage stacks above that
                    let power = self.sch.components.iter().find(|c| c.kind == Kind::Power && c.pos == *pt);
                    let top_left = if let Some(c) = power {
                        let name_font = self.main_font(painter);
                        let name_h = painter.layout_no_wrap(c.value.clone(), name_font, Color32::BLACK).size().y;
                        let at = self.to_screen(rect, (c.pos.0 as f32, c.pos.1 as f32 - 0.8));
                        Pos2::new(at.x - galley.size().x / 2.0, at.y - name_h - 1.0 - galley.size().y)
                    } else { match self.label_anchor(*pt) {
                        Some((l, spot)) => {
                            let at = self.to_screen(rect, (l.pos.0 as f32, l.pos.1 as f32));
                            let name = painter.layout_no_wrap(l.text.clone(), self.main_font(painter), Color32::BLACK).size();
                            match spot {
                                VoltageSpot::AfterName => Pos2::new(at.x + 5.0 + name.x + 6.0, at.y - 3.0 - galley.size().y),
                                VoltageSpot::BeforeDot => Pos2::new(at.x - 6.0 - galley.size().x, at.y - 3.0 - galley.size().y),
                                VoltageSpot::AboveName => Pos2::new(at.x + 5.0, at.y - 3.0 - name.y - 1.0 - galley.size().y),
                            }
                        }
                        None => self.to_screen(rect, (pt.0 as f32, pt.1 as f32)) + Vec2::new(5.0, -4.0 - galley.size().y),
                    } };
                    painter.rect_filled(Rect::from_min_size(top_left, galley.size()).expand(1.0), 2.0, crate::canvas_theme::col(crate::canvas_theme::Role::TextChip));
                    painter.galley(top_left, galley, crate::canvas_theme::col(crate::canvas_theme::Role::Voltage));
                }
            }
    }

    pub(super) fn draw_currents(&self, painter: &egui::Painter, rect: Rect) {
        if self.show_i
            && let Some(dc) = self.dc.as_ref()
            && let Ok(res) = &dc.outcome {
                let painter = &self.result_painter(painter);
                let f = self.main_font(painter);
                let col = crate::canvas_theme::col(crate::canvas_theme::Role::Current);
                let label_rects = self.label_rects(painter, rect);
                for c in self.sch.components.iter().filter(|c| !c.kind.is_power_symbol()) {
                    let Some(el) = res.element(&c.refdes) else { continue };
                    // no current worth reading: a capacitor at DC, a part on an idle pin. "0 A" under every part of a big sheet
                    // buried the currents that matter; 1 nA still keeps an op-amp's input bias (tens of nA)
                    if el.current.abs() < MIN_SHOWN_CURRENT {
                        continue;
                    }
                    let ext = comp_extents(c);
                    let Some((x0, x1, _, y1)) = sheet_box(c.kind, ext, c.pos, c.rot) else { continue };
                    // below the part's value text
                    let at = self.to_screen(rect, ((x0 + x1) / 2.0, y1 + 1.3));
                    let galley = painter.layout_no_wrap(eng(el.current, "A"), f.clone(), col);
                    // a wire leaves the middle of the bottom edge: the text goes left of it
                    let tall = (y1 - sheet_box(c.kind, ext, c.pos, c.rot).map_or(0.0, |b| b.2)) > (x1 - x0);
                    let wire_below = !tall && pin_on_edge(c, y1, (x0 + x1) / 2.0);
                    let (w, h) = (galley.size().x, galley.size().y);
                    let preferred = if wire_below { at - Vec2::new(w + 6.0, 0.0) } else { at - Vec2::new(w / 2.0, 0.0) };
                    // when that spot lies over a net label (a source's "GND" just below it), try left of the wire, then right of it
                    let spots = [preferred, at - Vec2::new(w + 6.0, 0.0), at + Vec2::new(6.0, 0.0)];
                    let rects: Vec<Rect> = spots.iter().map(|p| Rect::from_min_size(*p, Vec2::new(w, h))).collect();
                    let pos = spots[first_clear(&rects, &label_rects)];
                    painter.rect_filled(Rect::from_min_size(pos, galley.size()).expand(1.0), 2.0, crate::canvas_theme::col(crate::canvas_theme::Role::TextChip));
                    painter.galley(pos, galley, col);
                    // arrow along the part: the direction the (conventional) current flows
                    let pins = c.pins();
                    if pins.len() == 2 && el.current.abs() > 1e-12 {
                        let (p1, p2) = (self.to_screen(rect, (pins[0].1.0 as f32, pins[0].1.1 as f32)), self.to_screen(rect, (pins[1].1.0 as f32, pins[1].1.1 as f32)));
                        let (from, to) = if el.current >= 0.0 { (p1, p2) } else { (p2, p1) };
                        let dir = (to - from).normalized();
                        let tip = from + (to - from) * 0.5 + dir * 6.0;
                        let side = Vec2::new(-dir.y, dir.x) * 4.0;
                        painter.add(egui::Shape::convex_polygon(vec![tip, tip - dir * 9.0 + side, tip - dir * 9.0 - side], col, Stroke::NONE));
                    }
                }
            }
    }

    pub(super) fn draw_problems(&self, painter: &egui::Painter, rect: Rect) {
        for (lo, hi) in &self.sch.erc_skip {
            let r = Rect::from_two_pos(self.to_screen(rect, (lo.0 as f32, lo.1 as f32)), self.to_screen(rect, (hi.0 as f32, hi.1 as f32)));
            painter.rect(r, 4.0, (crate::canvas_theme::col(crate::canvas_theme::Role::Grid)).gamma_multiply(0.25), Stroke::new(1.2, crate::canvas_theme::col(crate::canvas_theme::Role::PinNumber)), egui::StrokeKind::Inside);
            painter.text(r.left_top() + Vec2::new(4.0, 2.0), Align2::LEFT_TOP, i18n::tr("描きかけ (ERC対象外)"), FontId::proportional(11.0), crate::canvas_theme::col(crate::canvas_theme::Role::PinNumber));
        }
        for (k, pr) in self.problems.iter().enumerate() {
            if let Some(a) = pr.at.filter(|_| pr.sheet == self.sch.active_sheet) {
                let color = match pr.severity {
                    Severity::Error => crate::canvas_theme::col(crate::canvas_theme::Role::Error),
                    Severity::Warning => crate::canvas_theme::col(crate::canvas_theme::Role::Warning),
                };
                let focus = self.erc_focus == Some(k);
                painter.circle_stroke(self.to_screen(rect, (a.0 as f32, a.1 as f32)), if focus { 16.0 } else { 11.0 }, Stroke::new(if focus { 3.5 } else { 2.0 }, color));
            }
        }
    }

    pub(super) fn draw_noise_overlay(&self, painter: &egui::Painter, rect: Rect) {
        for (pt, is_ref) in &self.noise_marks {
            let c = self.to_screen(rect, (pt.0 as f32, pt.1 as f32));
            let col = if *is_ref { Color32::from_rgb(40, 140, 40) } else { Color32::from_rgb(220, 90, 0) };
            painter.circle_stroke(c, 10.0, Stroke::new(2.5, col));
            painter.text(c + Vec2::new(12.0, -12.0), Align2::LEFT_BOTTOM, if *is_ref { i18n::tr("基準") } else { i18n::tr("出力") }, FontId::proportional(12.0), col);
        }
        if self.noise_pick {
            let banner = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 26.0));
            painter.rect_filled(banner, 0.0, Color32::from_rgba_unmultiplied(220, 90, 0, 230));
            painter.text(
                banner.left_center() + Vec2::new(10.0, 0.0),
                Align2::LEFT_CENTER,
                if self.loop_pick { i18n::tr("ループを切る部品の本体をクリック (ピン・配線: 出力側のネット / Esc: 中止)") } else if self.pz_pick { i18n::tr("伝達関数の出力にしたいネットをクリック (Shift+クリック: 入力のネット / Esc: 中止)") } else if self.z_pick { i18n::tr("インピーダンスを見たいネットをクリック (Shift+クリック: 基準のネット / Esc: 中止)") } else { i18n::tr("ノイズを測りたいノードをクリック (Shift+クリック: 基準ノード / Esc: 中止)") },
                FontId::proportional(14.0),
                Color32::WHITE,
            );
        }
    }

    pub(super) fn draw_noise_hover(&mut self, painter: &egui::Painter, snapped: Pt, sp: Pos2) {
        if self.noise_pick {
            if self.pick_hover.as_ref().map(|(p, _)| *p) != Some(snapped) {
                self.pick_hover = Some((snapped, net_at(&self.sch, snapped)));
            }
            if let Some((_, name)) = &self.pick_hover {
                let (txt, col) = match name {
                    Some(n) => (format!("ノード {n}"), Color32::from_rgb(220, 90, 0)),
                    None => ("(接続なし)".to_string(), Color32::from_gray(130)),
                };
                painter.circle_stroke(sp, 7.0, Stroke::new(2.0, col));
                let g = painter.layout_no_wrap(txt, self.main_font(painter), Color32::WHITE);
                let r = Rect::from_min_size(sp + Vec2::new(12.0, 10.0), g.size()).expand(3.0);
                painter.rect_filled(r, 3.0, Color32::from_rgba_unmultiplied(40, 40, 40, 220));
                painter.galley(r.min + Vec2::new(3.0, 3.0), g, col);
            }
        }
    }

    pub(super) fn draw_tool_ghost(&self, painter: &egui::Painter, rect: Rect, snapped: Pt, sp: Pos2) {
        match self.tool {
            _ if self.noise_pick => {}
            Tool::Place => {
                let ghost = (crate::canvas_theme::col(crate::canvas_theme::Role::Part)).gamma_multiply(0.5);
                match self.place_lib.and_then(|k| self.library.get(k)) {
                    Some(d) => self.draw_custom(painter, rect, &d.symbol(), snapped, self.rot, ghost),
                    None => self.draw_symbol(painter, rect, self.place, snapped, self.rot, ghost),
                }
            }
            Tool::Wire => {
                painter.circle_stroke(sp, 4.0, Stroke::new(1.5, crate::canvas_theme::col(crate::canvas_theme::Role::Wire)));
                if let Some(a) = self.pending {
                    let a_s = self.to_screen(rect, (a.0 as f32, a.1 as f32));
                    let m = self.to_screen(rect, (snapped.0 as f32, a.1 as f32));
                    let st = Stroke::new(1.5, (crate::canvas_theme::col(crate::canvas_theme::Role::Wire)).gamma_multiply(0.6));
                    painter.line_segment([a_s, m], st);
                    painter.line_segment([m, sp], st);
                }
            }
            Tool::Bus => {
                painter.circle_stroke(sp, 5.0, Stroke::new(2.5, crate::canvas_theme::col(crate::canvas_theme::Role::Bus)));
                if let Some(a) = self.pending {
                    let a_s = self.to_screen(rect, (a.0 as f32, a.1 as f32));
                    let m = self.to_screen(rect, (snapped.0 as f32, a.1 as f32));
                    let st = Stroke::new(3.0, (crate::canvas_theme::col(crate::canvas_theme::Role::Bus)).gamma_multiply(0.55));
                    painter.line_segment([a_s, m], st);
                    painter.line_segment([m, sp], st);
                }
            }
            Tool::Label => {
                painter.circle_stroke(sp, 4.0, Stroke::new(1.5, crate::canvas_theme::col(crate::canvas_theme::Role::NetLabel)));
            }
            Tool::Select => {}
        }
    }
}

/// Where a pin's voltage goes next to the pin's net label.
#[derive(Clone, Copy, Debug, PartialEq)]
enum VoltageSpot {
    AfterName,
    BeforeDot,
    AboveName,
}

/// Whether the pin at `pt` of `c` is on the top or bottom edge of the part's box: away from its left and right edges (the box
/// reaches out to the pin ends, so a side pin lies on an edge and a top pin lies between them).
fn pin_beyond_body_vertically(c: &Component, pt: Pt) -> bool {
    let Some((bx0, bx1, _, _)) = sheet_box(c.kind, comp_extents(c), c.pos, c.rot) else { return false };
    (pt.0 as f32) > bx0 + 0.6 && (pt.0 as f32) < bx1 - 0.6
}

/// Whether the pin at `pt` of `c` sits left of the middle of the part's body (its lead runs to the right, into the part).
fn pin_left_of_body(c: &Component, pt: Pt) -> bool {
    let Some((bx0, bx1, _, _)) = sheet_box(c.kind, comp_extents(c), c.pos, c.rot) else { return false };
    (pt.0 as f32) < (bx0 + bx1) / 2.0 - 0.5
}

/// A part with at least three pins on its left or right edge (an IC): the space beside the body is taken by wires and labels.
pub(super) fn side_pinned(c: &Component, bx0: f32, bx1: f32) -> bool {
    let pins = c.pins();
    let on = |x: f32| pins.iter().filter(|p| (p.1.0 as f32 - x).abs() < 0.6).count();
    on(bx0) >= 3 || on(bx1) >= 3
}

/// The index of the first candidate that touches none of the obstacles (0 when every one does: the preferred spot stays).
fn first_clear(candidates: &[Rect], obstacles: &[Rect]) -> usize {
    candidates.iter().position(|c| !obstacles.iter().any(|o| o.intersects(*c))).unwrap_or(0)
}

/// Whether a pin of `c` sits on the horizontal line `y` close to the x of the part's middle: a wire leaves the part there.
fn pin_on_edge(c: &Component, y: f32, cx: f32) -> bool {
    c.pins().iter().any(|p| (p.1.1 as f32 - y).abs() < 0.1 && (p.1.0 as f32 - cx).abs() < 1.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f32, y: f32) -> Rect {
        Rect::from_min_size(Pos2::new(x, y), Vec2::new(10.0, 10.0))
    }

    #[test]
    fn the_first_spot_that_touches_no_label_wins() {
        let labels = [r(0.0, 0.0)];
        // the preferred spot is on a label; the second is clear
        assert_eq!(first_clear(&[r(5.0, 5.0), r(30.0, 0.0), r(60.0, 0.0)], &labels), 1);
        // nothing is in the way: the preferred spot stays
        assert_eq!(first_clear(&[r(40.0, 0.0), r(60.0, 0.0)], &labels), 0);
        // every spot is blocked: still the preferred one
        assert_eq!(first_clear(&[r(2.0, 2.0), r(4.0, 4.0)], &labels), 0);
        // rectangles that only touch along an edge are not in the way of each other in practice
        assert_eq!(first_clear(&[r(0.0, 0.0)], &[]), 0);
    }

    #[test]
    fn an_ic_with_pins_down_both_sides_counts_as_side_pinned_but_a_resistor_does_not() {
        let mut s = sch_core::Schematic::default();
        let r1 = s.add(Kind::Resistor, (0, 0), 1);
        let (x0, x1, y0, y1) = comp_extents(&s.components[r1]);
        let b = sheet_box(Kind::Resistor, (x0, x1, y0, y1), (0, 0), 1).unwrap();
        assert!(!side_pinned(&s.components[r1], b.0, b.1), "a vertical resistor: pins top and bottom");
        let q = s.add(Kind::Npn, (20, 0), 0);
        let (x0, x1, y0, y1) = comp_extents(&s.components[q]);
        let b = sheet_box(Kind::Npn, (x0, x1, y0, y1), (20, 0), 0).unwrap();
        assert!(!side_pinned(&s.components[q], b.0, b.1), "a transistor has two pins on its right edge, not three");
        let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "ULN2803A").unwrap();
        let u = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (40, 10), 0);
        let (x0, x1, y0, y1) = comp_extents(&s.components[u]);
        let b = sheet_box(s.components[u].kind, (x0, x1, y0, y1), (40, 10), 0).unwrap();
        assert!(side_pinned(&s.components[u], b.0, b.1), "ULN2803A: eight pins on each side");
    }
}
