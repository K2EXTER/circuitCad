//! Parametric package (land pattern) generators, so a footprint can be described by the
//! few dimensions printed in a datasheet's "recommended land pattern" drawing.

use crate::footprints::{Footprint, FpHole, FpPad};
use crate::model::{PadShape, Point};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PackageSpec {
    /// Two SMD pads (chip R/C/L/LED, SOD diodes). `pitch` is centre to centre.
    Chip2 { pitch: f64, pad_len: f64, pad_width: f64 },
    /// Two rows (SOIC, SSOP, TSSOP, DIP). Pin 1 top-left, counter-clockwise.
    /// `row_spacing` is the centre-to-centre distance between the rows.
    /// `pad_len` is along the row-to-row direction, `pad_width` along the row.
    /// With `drill` the pads are through-hole (DIP).
    Dual {
        pins: usize,
        pitch: f64,
        row_spacing: f64,
        pad_len: f64,
        pad_width: f64,
        #[serde(default)]
        drill: Option<f64>,
    },
    /// Four sides (QFP, QFN without exposed pad). Pin 1 top-left, counter-clockwise.
    /// `span` is the centre-to-centre distance between opposite pad rows.
    Quad { pins: usize, pitch: f64, span: f64, pad_len: f64, pad_width: f64 },
    /// One row of through-hole pins (headers, TO-92 style strips). Pin 1 square.
    SingleRow { pins: usize, pitch: f64, pad_d: f64, drill: f64 },
    /// Anything else: list every pad.
    /// `holes` are non-plated holes of the part (locating pegs); a pad with `drill` and `slot` is a plated slot.
    Explicit {
        pads: Vec<PadSpec>,
        #[serde(default)]
        holes: Vec<HoleSpec>,
    },
}

fn default_shape() -> String {
    "rect".into()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PadSpec {
    pub pin: String,
    pub x: f64,
    /// Up is positive.
    pub y: f64,
    /// "rect" | "circle" | "oval"
    #[serde(default = "default_shape")]
    pub shape: String,
    pub w: f64,
    /// Ignored for circles.
    #[serde(default)]
    pub h: f64,
    #[serde(default)]
    pub drill: Option<f64>,
    /// Slot length of a plated slot (`drill` wide); 0 = round hole.
    #[serde(default)]
    pub slot: f64,
    /// The slot runs along y instead of x.
    #[serde(default)]
    pub slot_v: bool,
}

/// A non-plated hole of a part (position relative to the part origin, y up).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HoleSpec {
    pub x: f64,
    pub y: f64,
    pub drill: f64,
    #[serde(default)]
    pub slot: f64,
    #[serde(default)]
    pub slot_v: bool,
}

fn fp_pad(pin: String, x: f64, y: f64, shape: PadShape, drill: Option<f64>) -> FpPad {
    FpPad { pin, pos: Point::new(x, y), shape, drill, slot: 0.0, slot_v: false }
}

fn positive(name: &str, v: f64) -> Result<(), String> {
    if v.is_finite() && v > 0.0 { Ok(()) } else { Err(format!("{name} は正の値にしてください")) }
}

/// A slot needs a hole to be cut from and must be at least as long as it is wide.
fn check_slot(what: &str, drill: Option<f64>, slot: f64) -> Result<(), String> {
    if slot == 0.0 {
        return Ok(());
    }
    match drill {
        None => Err(format!("{what}: slot は穴のあるパッドにだけ付けられます (drill を指定してください)")),
        Some(d) if slot < d => Err(format!("{what}: slot (長穴の長さ {slot}) は drill (穴の幅 {d}) 以上にしてください")),
        Some(_) => Ok(()),
    }
}

impl PackageSpec {
    pub fn build(&self, name: &str) -> Result<Footprint, String> {
        let mut fp_holes: Vec<FpHole> = vec![];
        let pads = match self {
            PackageSpec::Chip2 { pitch, pad_len, pad_width } => {
                positive("pitch", *pitch)?;
                positive("pad_len", *pad_len)?;
                positive("pad_width", *pad_width)?;
                let s = PadShape::Rect { w: *pad_len, h: *pad_width };
                vec![fp_pad("1".into(), -pitch / 2.0, 0.0, s, None), fp_pad("2".into(), pitch / 2.0, 0.0, s, None)]
            }
            PackageSpec::Dual { pins, pitch, row_spacing, pad_len, pad_width, drill } => {
                if *pins < 2 || pins % 2 != 0 {
                    return Err("dual のピン数は2以上の偶数にしてください".into());
                }
                for (n, v) in [("pitch", pitch), ("row_spacing", row_spacing), ("pad_len", pad_len), ("pad_width", pad_width)] {
                    positive(n, *v)?;
                }
                let half = pins / 2;
                let shape = match drill {
                    Some(_) if (pad_len - pad_width).abs() < 1e-9 => PadShape::Circle { d: *pad_len },
                    Some(_) => PadShape::Oval { w: *pad_len, h: *pad_width },
                    None => PadShape::Rect { w: *pad_len, h: *pad_width },
                };
                let y = |i: usize| ((half as f64 - 1.0) / 2.0 - i as f64) * pitch;
                let mut v = vec![];
                for i in 0..half {
                    v.push(fp_pad((i + 1).to_string(), -row_spacing / 2.0, y(i), shape, *drill));
                }
                for i in 0..half {
                    // right column, bottom to top
                    v.push(fp_pad((half + i + 1).to_string(), row_spacing / 2.0, y(half - 1 - i), shape, *drill));
                }
                v
            }
            PackageSpec::Quad { pins, pitch, span, pad_len, pad_width } => {
                if *pins < 4 || pins % 4 != 0 {
                    return Err("quad のピン数は4の倍数にしてください".into());
                }
                for (n, v) in [("pitch", pitch), ("span", span), ("pad_len", pad_len), ("pad_width", pad_width)] {
                    positive(n, *v)?;
                }
                let n = pins / 4;
                let c = (n as f64 - 1.0) / 2.0;
                let (lr, tb) = (PadShape::Rect { w: *pad_len, h: *pad_width }, PadShape::Rect { w: *pad_width, h: *pad_len });
                let h = span / 2.0;
                let mut v = vec![];
                let mut k = 0;
                let mut push = |x: f64, y: f64, s: PadShape| {
                    k += 1;
                    v.push(fp_pad(k.to_string(), x, y, s, None));
                };
                for i in 0..n {
                    push(-h, (c - i as f64) * pitch, lr); // left, downwards
                }
                for i in 0..n {
                    push((-c + i as f64) * pitch, -h, tb); // bottom, left to right
                }
                for i in 0..n {
                    push(h, (-c + i as f64) * pitch, lr); // right, upwards
                }
                for i in 0..n {
                    push((c - i as f64) * pitch, h, tb); // top, right to left
                }
                v
            }
            PackageSpec::SingleRow { pins, pitch, pad_d, drill } => {
                if *pins < 1 {
                    return Err("pins は1以上".into());
                }
                for (n, v) in [("pitch", pitch), ("pad_d", pad_d), ("drill", drill)] {
                    positive(n, *v)?;
                }
                if drill >= pad_d {
                    return Err("drill は pad_d より小さくしてください".into());
                }
                (0..*pins)
                    .map(|i| {
                        let s = if i == 0 { PadShape::Rect { w: *pad_d, h: *pad_d } } else { PadShape::Circle { d: *pad_d } };
                        fp_pad((i + 1).to_string(), 0.0, -(i as f64) * pitch, s, Some(*drill))
                    })
                    .collect()
            }
            PackageSpec::Explicit { pads, holes } => {
                if pads.is_empty() {
                    return Err("pads が空です".into());
                }
                let mut v = vec![];
                for p in pads {
                    positive("w", p.w)?;
                    let shape = match p.shape.as_str() {
                        "rect" => PadShape::Rect { w: p.w, h: if p.h > 0.0 { p.h } else { p.w } },
                        "oval" => PadShape::Oval { w: p.w, h: if p.h > 0.0 { p.h } else { p.w } },
                        "circle" => PadShape::Circle { d: p.w },
                        other => return Err(format!("未知のパッド形状: {other}")),
                    };
                    let mut pad = fp_pad(p.pin.clone(), p.x, p.y, shape, p.drill);
                    check_slot(&p.pin, p.drill, p.slot)?;
                    pad.slot = p.slot;
                    pad.slot_v = p.slot_v;
                    v.push(pad);
                }
                for h in holes {
                    positive("holes の drill", h.drill)?;
                    check_slot("holes", Some(h.drill), h.slot)?;
                    fp_holes.push(FpHole { pos: Point::new(h.x, h.y), drill: h.drill, slot: h.slot, slot_v: h.slot_v });
                }
                v
            }
        };
        Ok(Footprint { name: name.to_string(), pads, holes: fp_holes, height: None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(fp: &Footprint, pin: &str) -> (f64, f64) {
        let p = fp.pads.iter().find(|p| p.pin == pin).unwrap().pos;
        (p.x, p.y)
    }

    #[test]
    fn soic8_pin_numbering_is_counter_clockwise_from_top_left() {
        let fp = PackageSpec::Dual { pins: 8, pitch: 1.27, row_spacing: 5.4, pad_len: 1.55, pad_width: 0.6, drill: None }
            .build("SOIC-8")
            .unwrap();
        assert_eq!(fp.pads.len(), 8);
        assert_eq!(pos(&fp, "1"), (-2.7, 1.905));
        assert_eq!(pos(&fp, "4"), (-2.7, -1.905));
        assert_eq!(pos(&fp, "5"), (2.7, -1.905));
        assert_eq!(pos(&fp, "8"), (2.7, 1.905));
    }

    #[test]
    fn dip_is_through_hole_with_round_pads() {
        let fp = PackageSpec::Dual { pins: 8, pitch: 2.54, row_spacing: 7.62, pad_len: 1.6, pad_width: 1.6, drill: Some(0.8) }
            .build("DIP-8")
            .unwrap();
        assert_eq!(fp.pads[0].shape, PadShape::Circle { d: 1.6 });
        assert_eq!(fp.pads[0].drill, Some(0.8));
    }

    #[test]
    fn qfp_corners() {
        let fp = PackageSpec::Quad { pins: 32, pitch: 0.8, span: 8.4, pad_len: 1.6, pad_width: 0.5 }.build("TQFP-32").unwrap();
        assert_eq!(fp.pads.len(), 32);
        let (x1, y1) = pos(&fp, "1");
        assert!(x1 < 0.0 && y1 > 0.0); // top of the left side
        let (x8, y8) = pos(&fp, "8");
        assert!(x8 < 0.0 && y8 < 0.0); // bottom of the left side
        let (x9, y9) = pos(&fp, "9");
        assert!(x9 < 0.0 && y9 < 0.0); // start of the bottom side (left)
        let (x17, y17) = pos(&fp, "17");
        assert!(x17 > 0.0 && y17 < 0.0); // start of the right side (bottom)
        let (x25, y25) = pos(&fp, "25");
        assert!(x25 > 0.0 && y25 > 0.0); // start of the top side (right)
    }

    #[test]
    fn invalid_specs_are_rejected() {
        assert!(PackageSpec::Dual { pins: 7, pitch: 1.0, row_spacing: 5.0, pad_len: 1.0, pad_width: 0.5, drill: None }.build("x").is_err());
        assert!(PackageSpec::Chip2 { pitch: -1.0, pad_len: 1.0, pad_width: 1.0 }.build("x").is_err());
        assert!(PackageSpec::SingleRow { pins: 2, pitch: 2.54, pad_d: 1.0, drill: 1.2 }.build("x").is_err());
    }

    #[test]
    fn json_round_trip() {
        let j = r#"{"type":"dual","pins":8,"pitch":1.27,"row_spacing":5.4,"pad_len":1.55,"pad_width":0.6}"#;
        let spec: PackageSpec = serde_json::from_str(j).unwrap();
        assert!(matches!(spec, PackageSpec::Dual { drill: None, .. }));
        let e = r#"{"type":"explicit","pads":[{"pin":"A","x":0,"y":0,"shape":"circle","w":1.2,"drill":0.6}]}"#;
        assert_eq!(serde_json::from_str::<PackageSpec>(e).unwrap().build("x").unwrap().pads[0].shape, PadShape::Circle { d: 1.2 });
    }
}
