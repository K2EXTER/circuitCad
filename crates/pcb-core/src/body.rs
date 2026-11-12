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
    /// A leaded resistor lying down with colour bands: the 3D view draws it as a cylinder with its bands, from the part's value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_code: Option<ColorCode>,
}

/// The colour bands printed on a leaded resistor (IEC 60062): how many, and the tolerance they end with.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColorCode {
    /// 4 (two figures, multiplier, tolerance) or 5 (three figures, multiplier, tolerance).
    pub bands: u8,
    pub tolerance_pct: f64,
    /// The paint of the body (a carbon film's beige, a metal film's blue).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<[u8; 3]>,
}

/// One colour of the code: a figure 0 to 9, or gold / silver (multiplier 0.1 / 0.01, tolerance 5 % / 10 %).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BandColor {
    Digit(u8),
    Gold,
    Silver,
}

impl BandColor {
    /// The colour to draw.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            BandColor::Digit(0) => [25, 25, 25],
            BandColor::Digit(1) => [125, 70, 30],
            BandColor::Digit(2) => [210, 35, 35],
            BandColor::Digit(3) => [240, 135, 25],
            BandColor::Digit(4) => [240, 210, 40],
            BandColor::Digit(5) => [40, 155, 70],
            BandColor::Digit(6) => [40, 90, 210],
            BandColor::Digit(7) => [145, 70, 190],
            BandColor::Digit(8) => [135, 135, 135],
            BandColor::Digit(_) => [245, 245, 245],
            BandColor::Gold => [200, 160, 50],
            BandColor::Silver => [190, 190, 200],
        }
    }

    /// The colour's name, as it is taught: 黒 0, 茶 1, 赤 2 … 白 9, 金, 銀.
    pub fn name(self) -> &'static str {
        match self {
            BandColor::Digit(0) => "黒",
            BandColor::Digit(1) => "茶",
            BandColor::Digit(2) => "赤",
            BandColor::Digit(3) => "橙",
            BandColor::Digit(4) => "黄",
            BandColor::Digit(5) => "緑",
            BandColor::Digit(6) => "青",
            BandColor::Digit(7) => "紫",
            BandColor::Digit(8) => "灰",
            BandColor::Digit(_) => "白",
            BandColor::Gold => "金",
            BandColor::Silver => "銀",
        }
    }
}

impl ColorCode {
    /// The bands of a resistor of `ohms`, first (nearest pin 1) to last. None when the value does not fit the figures (4.75 kΩ
    /// in a 4-band code) or the multiplier is out of the code's range.
    pub fn bands_for(&self, ohms: f64) -> Option<Vec<BandColor>> {
        if !(ohms.is_finite() && ohms > 0.0) || !(self.bands == 4 || self.bands == 5) {
            return None;
        }
        let figures = (self.bands - 2) as i32;
        let exp = ohms.log10().floor() as i32;
        let power = exp - (figures - 1);
        let m = (ohms / 10f64.powi(power)).round() as u32;
        if (m as f64 * 10f64.powi(power) / ohms - 1.0).abs() > 1e-6 || m >= 10u32.pow(figures as u32) {
            return None;
        }
        let mut v: Vec<BandColor> = format!("{m:0width$}", width = figures as usize).bytes().map(|b| BandColor::Digit(b - b'0')).collect();
        v.push(match power {
            -1 => BandColor::Gold,
            -2 => BandColor::Silver,
            p if (0..=9).contains(&p) => BandColor::Digit(p as u8),
            _ => return None,
        });
        let tol = self.tolerance_pct;
        let close = |x: f64| (tol - x).abs() < 1e-9;
        v.push(if close(1.0) {
            BandColor::Digit(1)
        } else if close(2.0) {
            BandColor::Digit(2)
        } else if close(0.5) {
            BandColor::Digit(5)
        } else if close(0.25) {
            BandColor::Digit(6)
        } else if close(0.1) {
            BandColor::Digit(7)
        } else if close(5.0) {
            BandColor::Gold
        } else if close(10.0) {
            BandColor::Silver
        } else {
            return None;
        });
        Some(v)
    }
}

/// Where along a banded resistor's body each band sits (its middle, as a share of the length from the pin 1 end): the tolerance
/// band stands apart from the others so the code reads from the other end.
pub fn band_places(n: usize) -> &'static [f64] {
    if n == 5 { &[0.15, 0.27, 0.39, 0.51, 0.81] } else { &[0.18, 0.32, 0.46, 0.79] }
}

/// A resistance as a part's value is written: `220`, `4.75k`, `1Meg`, `1M` (mega), `2.2`, `4k7`, `100Ω`.
pub fn parse_ohms(text: &str) -> Option<f64> {
    let s = text.trim().trim_end_matches(['Ω', 'Ω']).trim();
    let end = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    let (num, rest) = s.split_at(end);
    let lower = rest.to_ascii_lowercase();
    let (mult, tail) = if lower.starts_with("meg") {
        (1e6, &rest[3..])
    } else {
        match rest.chars().next() {
            Some('k' | 'K') => (1e3, &rest[1..]),
            Some('M') => (1e6, &rest[1..]),
            Some('G') => (1e9, &rest[1..]),
            Some('R' | 'r') => (1.0, &rest[1..]),
            None => (1.0, ""),
            _ => return None,
        }
    };
    // `4k7`: the letter is the decimal point
    let x: f64 = if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) && !num.contains('.') {
        format!("{num}.{tail}").parse().ok()?
    } else if tail.is_empty() {
        num.parse().ok()?
    } else {
        return None;
    };
    Some(x * mult).filter(|v| v.is_finite() && *v > 0.0)
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
        let body = BodySpec { shape: "rect".into(), w: 3.9, h: 4.9, x: 0.0, y: 1.27, mark: BodyMark::Dot, mark_pin: String::new(), color_code: None };
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
        let circle = BodySpec { shape: "circle".into(), w: 5.4, h: 0.0, x: 0.0, y: 0.0, mark: BodyMark::Plus, mark_pin: "1".into(), color_code: None };
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

    #[test]
    fn the_colour_code_reads_as_it_is_taught() {
        use super::BandColor::*;
        let four = ColorCode { bands: 4, tolerance_pct: 5.0, body: None };
        let five = ColorCode { bands: 5, tolerance_pct: 1.0, body: None };
        // 220 Ω ±5 %: red red brown gold; 4.7 kΩ: yellow violet red gold; 2.2 Ω: red red gold gold
        assert_eq!(four.bands_for(220.0), Some(vec![Digit(2), Digit(2), Digit(1), Gold]));
        assert_eq!(four.bands_for(4700.0), Some(vec![Digit(4), Digit(7), Digit(2), Gold]));
        assert_eq!(four.bands_for(2.2), Some(vec![Digit(2), Digit(2), Gold, Gold]));
        assert_eq!(four.bands_for(4750.0), None, "three figures do not fit four bands");
        // 4.75 kΩ ±1 %: yellow violet green brown brown; 10 Ω: brown black black gold brown
        assert_eq!(five.bands_for(4750.0), Some(vec![Digit(4), Digit(7), Digit(5), Digit(1), Digit(1)]));
        assert_eq!(five.bands_for(10.0), Some(vec![Digit(1), Digit(0), Digit(0), Gold, Digit(1)]));
        assert_eq!(Digit(7).name(), "紫");
        assert_eq!(parse_ohms("4.75k"), Some(4750.0));
        assert_eq!(parse_ohms("1Meg"), Some(1e6));
        assert_eq!(parse_ohms("1M"), Some(1e6));
        assert_eq!(parse_ohms("4k7"), Some(4700.0));
        assert_eq!(parse_ohms("2.2"), Some(2.2));
        assert_eq!(parse_ohms("100Ω"), Some(100.0));
        assert_eq!(parse_ohms("10u"), None);
    }
}
