//! Drawing the parts, the wires and the ratsnest lines on the board.

use super::*;

/// A ratsnest line: a net's two leads that the board leaves apart, joined by a dashed line with a ring at each end.
pub(super) fn rats_line(painter: &egui::Painter, a: Pos2, b: Pos2, pitch: f32) {
    let c = Color32::from_rgb(255, 214, 60);
    painter.extend(egui::Shape::dashed_line(&[a, b], Stroke::new((pitch * 0.12).max(1.2), c), pitch * 0.5, pitch * 0.3));
    for p in [a, b] {
        painter.circle_stroke(p, pitch * 0.32, Stroke::new(1.5, c));
    }
}

/// A jumper wire: a gentle arc between two holes, with its ends.
pub(super) fn wire_points(a: Pos2, b: Pos2, pitch: f32) -> Vec<Pos2> {
    let mid = a + (b - a) / 2.0 - Vec2::new(0.0, ((b - a).length() * 0.15).min(pitch * 1.5));
    (0..=12)
        .map(|i| {
            let t = i as f32 / 12.0;
            let u = 1.0 - t;
            Pos2::new(u * u * a.x + 2.0 * u * t * mid.x + t * t * b.x, u * u * a.y + 2.0 * u * t * mid.y + t * t * b.y)
        })
        .collect()
}

pub(super) fn draw_wire(painter: &egui::Painter, a: Pos2, b: Pos2, c: Color32, pitch: f32) {
    let pts = wire_points(a, b, pitch);
    painter.add(egui::Shape::line(pts, Stroke::new((pitch * 0.22).max(2.0), c)));
    for p in [a, b] {
        painter.circle_filled(p, pitch * 0.16, Color32::from_gray(200));
    }
}

pub(super) fn part_rect(p: &sch_core::breadboard::Placed, to: impl Fn(Hole) -> Pos2, pitch: f32) -> Rect {
    let mut r = Rect::NOTHING;
    for (_, h) in &p.leads {
        r = r.union(Rect::from_center_size(to(*h), Vec2::splat(pitch * 0.9)));
    }
    r
}

/// Whether circuit element `element` belongs to part `refdes`: the part's own element, or one inside it (`VR1.A`).
pub(super) fn element_of(element: &str, refdes: &str) -> bool {
    element.strip_prefix(refdes).is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

/// Draws a part and returns the area it takes (for the pointer).
pub(super) fn draw_part(painter: &egui::Painter, p: &sch_core::breadboard::Placed, comp: Option<&sch_core::Component>, to: impl Fn(Hole) -> Pos2 + Copy, pitch: f32, font: &egui::FontId, lit: bool) -> Rect {
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
            if kind == Some(sch_core::Kind::Pot) {
                // a small blue trimmer: the square body, the round knob and its slot pointing where it is turned (from 7 to 5 o'clock)
                let body = Rect::from_min_max(Pos2::new(r.min.x, r.max.y - pitch * 0.2), Pos2::new(r.max.x, r.max.y + pitch * 1.1));
                painter.rect(body, pitch * 0.2, Color32::from_rgb(40, 90, 170), outline, egui::StrokeKind::Middle);
                let c = body.center();
                let rad = body.width().min(body.height()) * 0.42;
                painter.circle(c, rad, Color32::from_gray(235), Stroke::new(1.0, Color32::from_gray(60)));
                let pos = sch_core::pot_position(value) as f32;
                // 0 % at the lower left (pin 1's side), 100 % at the lower right; screen angles run clockwise
                let a = (135.0 + 270.0 * pos).to_radians();
                let d = Vec2::new(a.cos(), a.sin()) * rad * 0.85;
                painter.line_segment([c - d * 0.3, c + d], Stroke::new((pitch * 0.15).max(2.0), Color32::from_gray(40)));
                painter.text(Pos2::new(c.x, body.bottom() + pitch * 0.35), egui::Align2::CENTER_CENTER, format!("{} {:.0}%", p.refdes, pos * 100.0), font.clone(), Color32::from_gray(30));
                return r.union(body);
            }
            let fill = if matches!(kind, Some(sch_core::Kind::Header(_))) { Color32::from_rgb(30, 30, 30) } else { Color32::from_rgb(40, 40, 44) };
            painter.rect(body, pitch * 0.25, fill, outline, egui::StrokeKind::Middle);
            painter.text(body.center(), egui::Align2::CENTER_CENTER, &p.refdes, font.clone(), Color32::WHITE);
            r.union(body)
        }
        Look::TwoLead => {
            let Some((a, b)) = looks::ends(&p.leads) else { return Rect::NOTHING };
            let body = looks::draw_two_lead(painter, to(a), to(b), comp, pitch, outline);
            let dir = to(b) - to(a);
            let label_at = body.center() + if dir.x.abs() >= dir.y.abs() { Vec2::new(0.0, -(body.height() / 2.0 + pitch * 0.35)) } else { Vec2::new(body.width() / 2.0 + pitch * 0.45, 0.0) };
            painter.text(label_at, egui::Align2::CENTER_CENTER, &p.refdes, font.clone(), Color32::from_gray(30));
            body.expand(pitch * 0.2)
        }
    }
}
