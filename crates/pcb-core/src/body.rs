//! The body of a part as the data sheet draws it (a rectangle or a circle, with an orientation mark),
//! turned into silkscreen lines of its footprint. The lines stay clear of the pads: the part of an outline
//! that would run over a pad is left out.

use crate::footprints::{FpLine, FpPad};
use crate::model::{PadShape, Point};
use serde::{Deserialize, Serialize};

/// What marks the orientation of the part on its body.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyMark {
    #[default]
    None,
    /// A dot inside the body corner next to the marked pin (pin 1 of an IC).
    Dot,
    /// A bar across the body at the marked pin's end (the cathode of a diode).
    Bar,
    /// A + next to the marked pin (the plus lead of a polarised capacitor).
    Plus,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BodySpec {
    /// "rect" or "circle".
    #[serde(default = "default_shape")]
    pub shape: String,
    /// Width along x; the diameter of a circle.
    pub w: f64,
    /// Length along y; ignored for a circle.
    #[serde(default)]
    pub h: f64,
    /// Centre of the body relative to the part origin.
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default)]
    pub mark: BodyMark,
    /// The pin the mark belongs to; empty = the first pad.
    #[serde(default)]
    pub mark_pin: String,
}

fn default_shape() -> String {
    "rect".into()
}

impl BodySpec {
    pub fn validate(&self, pin_names: &[&str]) -> Result<(), String> {
        if self.shape != "rect" && self.shape != "circle" {
            return Err(format!("本体の形「{}」は rect か circle にしてください", self.shape));
        }
        if !(self.w.is_finite() && self.w > 0.0) {
            return Err("本体の幅 (円は直径) は 0 より大きい値にしてください".into());
        }
        if self.shape == "rect" && !(self.h.is_finite() && self.h > 0.0) {
            return Err("本体の長さは 0 より大きい値にしてください".into());
        }
        if !self.mark_pin.is_empty() && !pin_names.contains(&self.mark_pin.as_str()) {
            return Err(format!("向きの印のピン「{}」が、ピン表にありません", self.mark_pin));
        }
        Ok(())
    }
}

/// Distance from the pad's copper that silk keeps (mm).
const PAD_CLEARANCE: f64 = 0.2;

fn near_pad(pads: &[FpPad], p: Point) -> bool {
    pads.iter().any(|pad| {
        let (w, h) = pad.shape.size();
        match pad.shape {
            PadShape::Circle { d } => p.dist(pad.pos) <= d / 2.0 + PAD_CLEARANCE,
            _ => (p.x - pad.pos.x).abs() <= w / 2.0 + PAD_CLEARANCE && (p.y - pad.pos.y).abs() <= h / 2.0 + PAD_CLEARANCE,
        }
    })
}

/// A line cut where it would run over a pad: the pieces that stay clear.
fn clipped(pads: &[FpPad], a: Point, b: Point, out: &mut Vec<FpLine>) {
    let len = a.dist(b);
    if len < 1e-9 {
        return;
    }
    let steps = ((len / 0.05).ceil() as usize).max(1);
    let at = |i: usize| Point::new(a.x + (b.x - a.x) * i as f64 / steps as f64, a.y + (b.y - a.y) * i as f64 / steps as f64);
    let mut start: Option<usize> = None;
    for i in 0..=steps {
        let clear = !near_pad(pads, at(i));
        match (clear, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                if i - 1 > s {
                    out.push(FpLine { a: at(s), b: at(i - 1) });
                }
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start
        && steps > s {
            out.push(FpLine { a: at(s), b: at(steps) });
        }
}

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

/// The silkscreen lines for `body` on a part with these pads.
pub fn body_lines(body: &BodySpec, pads: &[FpPad]) -> Vec<FpLine> {
    let (cx, cy) = (body.x, body.y);
    let circle = body.shape == "circle";
    let (hw, hh) = if circle { (body.w / 2.0, body.w / 2.0) } else { (body.w / 2.0, body.h / 2.0) };
    let mut out = vec![];
    // the outline
    if circle {
        let n = 64;
        for i in 0..n {
            let (t0, t1) = (std::f64::consts::TAU * i as f64 / n as f64, std::f64::consts::TAU * (i + 1) as f64 / n as f64);
            clipped(pads, pt(cx + hw * t0.cos(), cy + hw * t0.sin()), pt(cx + hw * t1.cos(), cy + hw * t1.sin()), &mut out);
        }
    } else {
        let c = [pt(cx - hw, cy - hh), pt(cx + hw, cy - hh), pt(cx + hw, cy + hh), pt(cx - hw, cy + hh)];
        for i in 0..4 {
            clipped(pads, c[i], c[(i + 1) % 4], &mut out);
        }
    }
    // the mark: at the pad it belongs to
    let Some(marked) = pads.iter().find(|p| body.mark_pin.is_empty() || p.pin == body.mark_pin).or(pads.first()) else { return out };
    let (dx, dy) = (marked.pos.x - cx, marked.pos.y - cy);
    // which side of the body the marked pad is on
    let (ux, uy) = if dx.abs() >= dy.abs() { (dx.signum(), 0.0) } else { (0.0, dy.signum()) };
    match body.mark {
        BodyMark::None => {}
        BodyMark::Dot => {
            // inside the body corner nearest to the pad
            let (sx, sy) = (if dx >= 0.0 { 1.0 } else { -1.0 }, if dy >= 0.0 { 1.0 } else { -1.0 });
            let inset = (0.2 * hw.min(hh)).clamp(0.3, 0.7);
            let (px, py) = (cx + sx * (hw - inset - 0.1), cy + sy * (hh - inset - 0.1));
            let r = (0.12 * hw.min(hh)).clamp(0.12, 0.25);
            let n = 12;
            for i in 0..n {
                let (t0, t1) = (std::f64::consts::TAU * i as f64 / n as f64, std::f64::consts::TAU * (i + 1) as f64 / n as f64);
                out.push(FpLine { a: pt(px + r * t0.cos(), py + r * t0.sin()), b: pt(px + r * t1.cos(), py + r * t1.sin()) });
            }
        }
        BodyMark::Bar => {
            // a bar parallel to the body edge nearest to the pad, a little inside it
            let extent = if ux != 0.0 { hw } else { hh };
            let inset = (0.3 * extent).clamp(0.3, 0.8).min(extent * 0.8);
            let d = extent - inset;
            if ux != 0.0 {
                let half = if circle { (hw * hw - d * d).max(0.0).sqrt() } else { hh };
                clipped(pads, pt(cx + ux * d, cy - half), pt(cx + ux * d, cy + half), &mut out);
            } else {
                let half = if circle { (hw * hw - d * d).max(0.0).sqrt() } else { hw };
                clipped(pads, pt(cx - half, cy + uy * d), pt(cx + half, cy + uy * d), &mut out);
            }
        }
        BodyMark::Plus => {
            // beside the marked pad's lead, inside the body; the arms are 0.5 mm
            let (px, py) = if ux != 0.0 { (cx + ux * 0.6 * hw, cy + 0.55 * hh) } else { (cx - 0.55 * hw, cy + uy * 0.6 * hh) };
            let a = 0.25;
            out.push(FpLine { a: pt(px - a, py), b: pt(px + a, py) });
            out.push(FpLine { a: pt(px, py - a), b: pt(px, py + a) });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(pin: &str, x: f64, y: f64, w: f64, h: f64) -> FpPad {
        FpPad { pin: pin.into(), pos: pt(x, y), shape: PadShape::Rect { w, h }, drill: None, slot: 0.0, slot_v: false }
    }

    /// A SOIC-like body between two rows of pads: the outline is cut where it reaches a pad, and the pin 1 dot
    /// sits inside the corner next to pad 1.
    #[test]
    fn a_body_between_pads_keeps_clear_of_them() {
        let pads = vec![pad("1", -2.7, 1.9, 1.55, 0.6), pad("2", -2.7, 0.63, 1.55, 0.6), pad("3", 2.7, 0.63, 1.55, 0.6), pad("4", 2.7, 1.9, 1.55, 0.6)];
        let body = BodySpec { shape: "rect".into(), w: 3.9, h: 4.9, x: 0.0, y: 1.27, mark: BodyMark::Dot, mark_pin: String::new() };
        let lines = body_lines(&body, &pads);
        assert!(lines.len() > 4, "{}", lines.len());
        for l in &lines {
            for p in [l.a, l.b] {
                assert!(!near_pad(&pads, p) || (l.a.dist(l.b) < 1e-9), "{p:?} is over a pad");
            }
        }
        // the dot is near the top left corner (pad 1 is at the top left)
        let dot: Vec<&FpLine> = lines.iter().filter(|l| l.a.dist(l.b) < 0.3 && l.a.x < 0.0 && l.a.y > 2.5).collect();
        assert!(dot.len() >= 8, "{}", dot.len());
    }

    #[test]
    fn a_circle_with_a_bar_and_a_plus() {
        let pads = vec![
            FpPad { pin: "1".into(), pos: pt(-1.0, 0.0), shape: PadShape::Rect { w: 1.6, h: 1.6 }, drill: Some(0.8), slot: 0.0, slot_v: false },
            FpPad { pin: "2".into(), pos: pt(1.0, 0.0), shape: PadShape::Circle { d: 1.6 }, drill: Some(0.8), slot: 0.0, slot_v: false },
        ];
        let circle = BodySpec { shape: "circle".into(), w: 5.4, h: 0.0, x: 0.0, y: 0.0, mark: BodyMark::Plus, mark_pin: "1".into() };
        let lines = body_lines(&circle, &pads);
        assert!(lines.len() > 60, "{}", lines.len());
        // the + is on the pin 1 side (left)
        assert!(lines.iter().rev().take(2).all(|l| l.a.x < 0.0 && l.b.x < 0.0));
        // a bar on the pin 1 side is a chord of the circle
        let bar = body_lines(&BodySpec { mark: BodyMark::Bar, ..circle.clone() }, &pads);
        let chord = bar.last().unwrap();
        assert!(chord.a.x < 0.0 && (chord.a.x - chord.b.x).abs() < 1e-9 && chord.a.y != chord.b.y, "{chord:?}");
        // wrong inputs are refused with a message
        assert!(BodySpec { w: 0.0, ..circle.clone() }.validate(&["1", "2"]).is_err());
        assert!(BodySpec { mark_pin: "9".into(), ..circle.clone() }.validate(&["1", "2"]).is_err());
        assert!(circle.validate(&["1", "2"]).is_ok());
    }
}
