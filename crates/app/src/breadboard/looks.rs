//! How a two-lead part looks on the boards of the proto tab (breadboard, stripboard, perfboard), drawn from above as the real
//! part: a resistor's beige body with its colour bands, a diode's black body (orange glass for a 1N4148) with the band at the
//! cathode, an LED's dome in its colour with the flat at the cathode, a ceramic capacitor's disc, an electrolytic's can with the
//! stripe on its minus side, an inductor's green body.

use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke, Vec2};
use sch_core::{Component, Kind};

/// The lead going to the cathode (a diode's or LED's K) or the minus (an electrolytic's −), else the second: (first, second).
pub(super) fn ends<P: Copy>(leads: &[(String, P)]) -> Option<(P, P)> {
    let minus = |pin: &str| matches!(pin.to_ascii_uppercase().as_str(), "K" | "-" | "−" | "C");
    let (a, b) = (leads.first()?, leads.get(1)?);
    Some(if minus(&a.0) && !minus(&b.0) { (b.1, a.1) } else { (a.1, b.1) })
}

/// An LED's colour from its value or model (red, green, blue, yellow, orange, white; red when it does not say).
fn led_colour(c: Option<&Component>) -> Color32 {
    let text = c.map(|c| format!("{} {} {}", c.value, c.footprint, c.custom.as_ref().map_or("", |s| s.name.as_str()))).unwrap_or_default().to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| text.contains(w));
    if has(&["green", "grn", "緑"]) {
        Color32::from_rgb(60, 200, 80)
    } else if has(&["blue", "blu", "青"]) {
        Color32::from_rgb(60, 110, 240)
    } else if has(&["yellow", "yel", "黄"]) {
        Color32::from_rgb(245, 215, 40)
    } else if has(&["orange", "橙"]) {
        Color32::from_rgb(250, 140, 30)
    } else if has(&["white", "wht", "白"]) {
        Color32::from_rgb(235, 235, 225)
    } else {
        Color32::from_rgb(225, 45, 45)
    }
}

/// An electrolytic capacitor (a polarised footprint, or 10 µF and up), not a ceramic one.
fn electrolytic(c: Option<&Component>) -> bool {
    let Some(c) = c else { return false };
    let fp = c.footprint.to_ascii_uppercase();
    fp.starts_with("CP") || fp.contains("ELEC") || fp.contains("POLAR") || sim_core::parse_value(&c.value).is_some_and(|v| v >= 9.9e-6)
}

/// A rectangle along the line from its centre `c`: `half_len` along `u`, `half_th` across.
fn quad(c: Pos2, u: Vec2, half_len: f32, half_th: f32) -> Vec<Pos2> {
    let n = u.rot90();
    vec![c - u * half_len - n * half_th, c + u * half_len - n * half_th, c + u * half_len + n * half_th, c - u * half_len + n * half_th]
}

/// A capsule (a resistor's or diode's body) along `u` round centre `c`.
fn capsule(painter: &egui::Painter, c: Pos2, u: Vec2, half_len: f32, half_th: f32, fill: Color32, outline: Stroke) {
    let ends = [c - u * (half_len - half_th), c + u * (half_len - half_th)];
    for e in ends {
        painter.circle(e, half_th, fill, outline);
    }
    painter.add(Shape::convex_polygon(quad(c, u, half_len - half_th, half_th), fill, Stroke::NONE));
    let n = u.rot90() * half_th;
    for s in [1.0, -1.0] {
        painter.line_segment([ends[0] + n * s, ends[1] + n * s], outline);
    }
    // a shine along the top
    painter.line_segment([ends[0] - n * 0.45, ends[1] - n * 0.45], Stroke::new((half_th * 0.25).max(1.0), Color32::from_white_alpha(70)));
}

/// A capsule's centre, direction, half length and half thickness.
type Body = (Pos2, Vec2, f32, f32);

/// A band across a capsule at `t` (0 at the first end, 1 at the second).
fn band(painter: &egui::Painter, (c, u, half_len, half_th): Body, t: f32, width: f32, colour: Color32) {
    let at = c + u * (half_len * (2.0 * t - 1.0));
    painter.add(Shape::convex_polygon(quad(at, u, width / 2.0, half_th * 0.98), colour, Stroke::NONE));
}

/// Draws a two-lead part from `a` to `b` (`b` at the cathode or the minus: `ends`), its leads first; the body's rectangle is
/// returned (for the name beside it and the pointer).
pub(super) fn draw_two_lead(painter: &egui::Painter, a: Pos2, b: Pos2, comp: Option<&Component>, pitch: f32, outline: Stroke) -> Rect {
    let lead = Stroke::new((pitch * 0.11).max(1.3), Color32::from_gray(175));
    painter.line_segment([a, b], lead);
    let d = b - a;
    let u = if d.length() > 0.1 { d.normalized() } else { Vec2::X };
    let mid = a + d / 2.0;
    let half_len = (d.length() * 0.33).clamp(pitch * 0.5, pitch * 1.2);
    let half_th = pitch * 0.27;
    let bounds = |r: f32| Rect::from_center_size(mid, Vec2::splat(r * 2.0));
    let capsule_bounds = Rect::from_points(&quad(mid, u, half_len, half_th));
    match comp.map(|c| c.kind) {
        Some(Kind::Resistor) => {
            capsule(painter, mid, u, half_len, half_th, Color32::from_rgb(222, 196, 150), outline);
            if let Some(bs) = super::bands(comp.map_or("", |c| c.value.as_str())) {
                for (i, c) in bs.iter().enumerate() {
                    // three bands from the first end, the tolerance's apart at the other
                    let t = if i < 3 { 0.24 + 0.16 * i as f32 } else { 0.8 };
                    band(painter, (mid, u, half_len, half_th), t, pitch * 0.13, *c);
                }
            }
            capsule_bounds
        }
        Some(Kind::Diode) => {
            let glass = comp.is_some_and(|c| c.value.contains("4148") || c.value.to_ascii_uppercase().contains("LL41"));
            let (body, mark) = if glass { (Color32::from_rgb(235, 120, 40).gamma_multiply(0.9), Color32::from_gray(25)) } else { (Color32::from_rgb(32, 32, 36), Color32::from_gray(205)) };
            capsule(painter, mid, u, half_len * 0.85, half_th * 0.9, body, outline);
            band(painter, (mid, u, half_len * 0.85, half_th * 0.9), 0.82, pitch * 0.16, mark);
            capsule_bounds
        }
        Some(Kind::Led) => {
            let colour = led_colour(comp);
            let r = pitch * 0.55;
            // the rim (the flange), the dome over it, a shine, and the rim's flat on the cathode's side
            painter.circle(mid, r, colour.linear_multiply(0.55), outline);
            painter.circle_filled(mid, r * 0.8, colour);
            painter.circle_filled(mid - Vec2::splat(r * 0.3), r * 0.22, Color32::from_white_alpha(150));
            let n = u * r * 0.82;
            painter.line_segment([mid + n + n.rot90() * 0.62, mid + n - n.rot90() * 0.62], Stroke::new(2.0, Color32::from_gray(35)));
            bounds(r)
        }
        Some(Kind::Capacitor) if electrolytic(comp) => {
            let r = pitch * 0.62;
            let can = Color32::from_rgb(40, 70, 160);
            painter.circle(mid, r, can, outline);
            // the stripe down the minus side, and the top's vent
            let toward = u.angle();
            let pts: Vec<Pos2> = (0..=12)
                .map(|k| toward - 0.75 + 1.5 * k as f32 / 12.0)
                .map(|t| mid + Vec2::angled(t) * r)
                .chain((0..=12).rev().map(|k| toward - 0.75 + 1.5 * k as f32 / 12.0).map(|t| mid + Vec2::angled(t) * r * 0.68))
                .collect();
            painter.add(Shape::Path(egui::epaint::PathShape { points: pts, closed: true, fill: Color32::from_gray(200), stroke: Stroke::NONE.into() }));
            painter.text(mid + u * r * 0.84, egui::Align2::CENTER_CENTER, "−", egui::FontId::proportional((pitch * 0.4).max(7.0)), can);
            let v = r * 0.35;
            for w in [Vec2::new(v, 0.0), Vec2::new(0.0, v)] {
                painter.line_segment([mid - w, mid + w], Stroke::new(1.0, Color32::from_gray(170)));
            }
            bounds(r)
        }
        Some(Kind::Capacitor) => {
            // a ceramic disc, its value's code on it (104 = 100 nF)
            let r = pitch * 0.42;
            painter.circle(mid, r, Color32::from_rgb(232, 160, 50), outline);
            painter.circle_filled(mid - Vec2::splat(r * 0.3), r * 0.2, Color32::from_white_alpha(80));
            if let Some(code) = comp.and_then(|c| sim_core::parse_value(&c.value)).and_then(cap_code) {
                painter.text(mid, egui::Align2::CENTER_CENTER, code, egui::FontId::monospace((pitch * 0.3).clamp(6.0, 10.0)), Color32::from_gray(40));
            }
            bounds(r)
        }
        Some(Kind::Inductor) => {
            capsule(painter, mid, u, half_len, half_th, Color32::from_rgb(70, 140, 85), outline);
            for t in [0.3, 0.45, 0.6] {
                band(painter, (mid, u, half_len, half_th), t, pitch * 0.1, Color32::from_gray(200));
            }
            capsule_bounds
        }
        _ => {
            capsule(painter, mid, u, half_len, half_th, Color32::from_rgb(60, 60, 70), outline);
            capsule_bounds
        }
    }
}

/// A ceramic capacitor's three-digit code in pF: 100 nF → 104, 22 pF → 220, 4.7 nF → 472 (none past 9.9 µF or below 1 pF).
fn cap_code(f: f64) -> Option<String> {
    let pf = f * 1e12;
    if !(1.0..1e7).contains(&pf) {
        return None;
    }
    let exp = (pf.log10().floor() as i32 - 1).max(0);
    let digits = (pf / 10f64.powi(exp)).round() as i32;
    let (digits, exp) = if digits >= 100 { (digits / 10, exp + 1) } else { (digits, exp) };
    Some(format!("{digits:02}{exp}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ceramic_capacitor_is_marked_with_its_code() {
        for (f, code) in [(100e-9, Some("104")), (22e-12, Some("220")), (4.7e-9, Some("472")), (1e-6, Some("105")), (10e-12, Some("100")), (0.5e-12, None)] {
            assert_eq!(cap_code(f).as_deref(), code, "{f}");
        }
    }

    #[test]
    fn the_cathode_end_is_found_by_its_pin() {
        assert_eq!(ends(&[("K".to_string(), 1), ("A".to_string(), 2)]), Some((2, 1)));
        assert_eq!(ends(&[("A".to_string(), 1), ("K".to_string(), 2)]), Some((1, 2)));
        assert_eq!(ends(&[("1".to_string(), 1), ("2".to_string(), 2)]), Some((1, 2)));
    }
}
