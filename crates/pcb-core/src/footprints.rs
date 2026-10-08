//! Built-in footprint library. Pad positions are relative to the part origin (y up).

use crate::model::{PadShape, Point};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FpPad {
    pub pin: String,
    pub pos: Point,
    pub shape: PadShape,
    pub drill: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Footprint {
    pub name: String,
    pub pads: Vec<FpPad>,
}

fn pad(pin: &str, x: f64, y: f64, shape: PadShape, drill: Option<f64>) -> FpPad {
    FpPad { pin: pin.to_string(), pos: Point::new(x, y), shape, drill }
}

fn smd2(name: &str, half_pitch: f64, w: f64, h: f64, pins: [&str; 2]) -> Footprint {
    let s = PadShape::Rect { w, h };
    Footprint {
        name: name.into(),
        pads: vec![pad(pins[0], -half_pitch, 0.0, s, None), pad(pins[1], half_pitch, 0.0, s, None)],
    }
}

fn tht2(name: &str, pitch: f64, d: f64, drill: f64, pins: [&str; 2]) -> Footprint {
    let s = PadShape::Circle { d };
    Footprint {
        name: name.into(),
        pads: vec![
            pad(pins[0], -pitch / 2.0, 0.0, PadShape::Rect { w: d, h: d }, Some(drill)),
            pad(pins[1], pitch / 2.0, 0.0, s, Some(drill)),
        ],
    }
}

fn header(name: &str, pins: &[String]) -> Footprint {
    let pitch = 2.54;
    Footprint {
        name: name.into(),
        pads: pins
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let shape = if i == 0 { PadShape::Rect { w: 1.7, h: 1.7 } } else { PadShape::Circle { d: 1.7 } };
                pad(p, 0.0, -(i as f64) * pitch, shape, Some(1.0))
            })
            .collect(),
    }
}

pub fn names() -> Vec<String> {
    let mut v: Vec<String> = [
        "R_0603", "R_0805", "C_0603", "C_0805", "L_0805", "LED_0805", "D_SOD-123", "R_Axial_10.16", "C_Disc_5.08",
        "D_DO-35_7.62", "LED_5mm", "TO-92", "SOT-23", "PinHeader_1x2_PM",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    for n in 2..=6 {
        v.push(format!("PinHeader_1x{n}"));
    }
    v
}

pub fn get(name: &str) -> Option<Footprint> {
    let fp = match name {
        "R_0603" | "C_0603" => smd2(name, 0.825, 0.8, 0.95, ["1", "2"]),
        "R_0805" | "C_0805" | "L_0805" => smd2(name, 0.9125, 1.025, 1.4, ["1", "2"]),
        "LED_0805" => smd2(name, 0.9125, 1.025, 1.4, ["K", "A"]),
        "D_SOD-123" => smd2(name, 1.65, 0.91, 1.22, ["K", "A"]),
        "R_Axial_10.16" => tht2(name, 10.16, 1.8, 0.9, ["1", "2"]),
        "C_Disc_5.08" => tht2(name, 5.08, 1.6, 0.8, ["1", "2"]),
        "D_DO-35_7.62" => tht2(name, 7.62, 1.8, 0.9, ["K", "A"]),
        "LED_5mm" => tht2(name, 2.54, 1.8, 0.9, ["K", "A"]),
        "TO-92" => {
            let s = PadShape::Circle { d: 1.5 };
            Footprint {
                name: name.into(),
                pads: vec![
                    pad("E", -2.54, 0.0, PadShape::Rect { w: 1.5, h: 1.5 }, Some(0.8)),
                    pad("B", 0.0, 0.0, s, Some(0.8)),
                    pad("C", 2.54, 0.0, s, Some(0.8)),
                ],
            }
        }
        "SOT-23" => {
            let s = PadShape::Rect { w: 0.9, h: 0.8 };
            Footprint {
                name: name.into(),
                pads: vec![pad("B", -0.95, 0.95, s, None), pad("E", 0.95, 0.95, s, None), pad("C", 0.0, -0.95, s, None)],
            }
        }
        "PinHeader_1x2_PM" => header(name, &["+".to_string(), "-".to_string()]),
        _ => {
            let n: usize = name.strip_prefix("PinHeader_1x")?.parse().ok()?;
            if !(1..=40).contains(&n) {
                return None;
            }
            header(name, &(1..=n).map(|i| i.to_string()).collect::<Vec<_>>())
        }
    };
    Some(fp)
}
