//! Built-in footprint library. Pad positions are relative to the part origin (y up).

use crate::model::{PadShape, Point};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FpPad {
    pub pin: String,
    pub pos: Point,
    pub shape: PadShape,
    pub drill: Option<f64>,
    /// A plated slot `slot` long (`drill` wide); 0 = round. Along x, or along y with `slot_v`.
    #[serde(default)]
    pub slot: f64,
    #[serde(default)]
    pub slot_v: bool,
}

/// A non-plated hole that belongs to the part (a locating peg, a screw hole in a connector's housing).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FpHole {
    pub pos: Point,
    pub drill: f64,
    /// Slot length (0 = round); along x, or along y with `slot_v`.
    #[serde(default)]
    pub slot: f64,
    #[serde(default)]
    pub slot_v: bool,
}

/// A silkscreen line of a footprint (relative to the part origin, y up).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct FpLine {
    pub a: Point,
    pub b: Point,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Footprint {
    pub name: String,
    pub pads: Vec<FpPad>,
    /// The body outline and polarity marks drawn on the silkscreen. Empty = a box around the pads is drawn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub silk: Vec<FpLine>,
    /// The body as the data sheet draws it (size and orientation mark): the 3D view builds the part's box from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<crate::body::BodySpec>,
    #[serde(default)]
    pub holes: Vec<FpHole>,
    /// Height of the part's body above the board (mm), when the data sheet says. The 3D view guesses from the name otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
}

fn pad(pin: &str, x: f64, y: f64, shape: PadShape, drill: Option<f64>) -> FpPad {
    FpPad { pin: pin.to_string(), pos: Point::new(x, y), shape, drill, slot: 0.0, slot_v: false }
}

fn smd2(name: &str, half_pitch: f64, w: f64, h: f64, pins: [&str; 2]) -> Footprint {
    let s = PadShape::Rect { w, h };
    Footprint {
        name: name.into(),
        holes: vec![],
        height: None,
        silk: vec![],
        body: None,
        pads: vec![pad(pins[0], -half_pitch, 0.0, s, None), pad(pins[1], half_pitch, 0.0, s, None)],
    }
}

fn tht2(name: &str, pitch: f64, d: f64, drill: f64, pins: [&str; 2]) -> Footprint {
    let s = PadShape::Circle { d };
    Footprint {
        name: name.into(),
        holes: vec![],
        height: None,
        silk: vec![],
        body: None,
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
        holes: vec![],
        height: None,
        silk: vec![],
        body: None,
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

/// Holes for wires soldered straight into the board (the force and sense leads of a measuring instrument): round pads of 2.2 mm
/// with 1.2 mm holes (wire up to about AWG 20) on the pitch of a header, so a board that had a header there keeps its tracks.
fn wire_pads(name: &str, pins: &[String]) -> Footprint {
    let pitch = 2.54;
    Footprint {
        name: name.into(),
        holes: vec![],
        height: None,
        silk: vec![],
        body: None,
        pads: pins.iter().enumerate().map(|(i, p)| pad(p, 0.0, -(i as f64) * pitch, PadShape::Circle { d: 2.2 }, Some(1.2))).collect(),
    }
}

pub fn names() -> Vec<String> {
    let mut v: Vec<String> = [
        "R_0603", "R_0805", "R_1206", "R_2512", "C_0603", "C_0805", "C_1206", "L_0805", "LED_0805", "D_SOD-123", "R_Axial_10.16", "C_Disc_5.08", "CP_Radial_D5_P2.0",
        "D_DO-35_7.62", "LED_5mm", "TO-92", "SOT-23", "PinHeader_1x2_PM",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    for n in 2..=6 {
        v.push(format!("PinHeader_1x{n}"));
    }
    for n in 2..=6 {
        v.push(format!("WirePad_1x{n}"));
    }
    v
}

pub fn get(name: &str) -> Option<Footprint> {
    let fp = match name {
        "R_0603" | "C_0603" => smd2(name, 0.825, 0.8, 0.95, ["1", "2"]),
        "R_0805" | "C_0805" | "L_0805" => smd2(name, 0.9125, 1.025, 1.4, ["1", "2"]),
        // IPC-7351 nominal land patterns for the larger chips (1/4 W and 1 W resistors)
        "R_1206" | "C_1206" => smd2(name, 1.5, 1.2, 1.8, ["1", "2"]),
        "R_2512" => smd2(name, 2.95, 1.4, 3.4, ["1", "2"]),
        "LED_0805" => smd2(name, 0.9125, 1.025, 1.4, ["K", "A"]),
        "D_SOD-123" => smd2(name, 1.65, 0.91, 1.22, ["K", "A"]),
        "R_Axial_10.16" => tht2(name, 10.16, 1.8, 0.9, ["1", "2"]),
        "C_Disc_5.08" => tht2(name, 5.08, 1.6, 0.8, ["1", "2"]),
        // radial electrolytic: pin 1 (square pad) is the + lead
        "CP_Radial_D5_P2.0" => tht2(name, 2.0, 1.6, 0.8, ["1", "2"]),
        "D_DO-35_7.62" => tht2(name, 7.62, 1.8, 0.9, ["K", "A"]),
        "LED_5mm" => tht2(name, 2.54, 1.8, 0.9, ["K", "A"]),
        // the 18650 cells used to come with the pads of a battery holder (77 mm apart); schematics saved then still name it
        "BatteryHolder_18650" => tht2(name, 77.0, 2.4, 1.2, ["1", "2"]),
        "TO-92" => {
            let s = PadShape::Circle { d: 1.5 };
            Footprint {
                name: name.into(),
                holes: vec![],
                height: None,
                silk: vec![],
                body: None,
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
                holes: vec![],
                height: None,
                silk: vec![],
                body: None,
                pads: vec![pad("B", -0.95, 0.95, s, None), pad("E", 0.95, 0.95, s, None), pad("C", 0.0, -0.95, s, None)],
            }
        }
        "PinHeader_1x2_PM" => header(name, &["+".to_string(), "-".to_string()]),
        _ if name.starts_with("WirePad_1x") => {
            let n: usize = name.strip_prefix("WirePad_1x")?.parse().ok()?;
            if !(1..=40).contains(&n) {
                return None;
            }
            wire_pads(name, &(1..=n).map(|i| i.to_string()).collect::<Vec<_>>())
        }
        _ => {
            let n: usize = name.strip_prefix("PinHeader_1x")?.parse().ok()?;
            if !(1..=40).contains(&n) {
                return None;
            }
            header(name, &(1..=n).map(|i| i.to_string()).collect::<Vec<_>>())
        }
    };
    let mut fp = fp;
    fp.silk = outline(name);
    Some(fp)
}

fn line(a: (f64, f64), b: (f64, f64)) -> FpLine {
    FpLine { a: Point::new(a.0, a.1), b: Point::new(b.0, b.1) }
}

fn rect_lines(hx: f64, hy: f64) -> Vec<FpLine> {
    vec![line((-hx, -hy), (hx, -hy)), line((hx, -hy), (hx, hy)), line((hx, hy), (-hx, hy)), line((-hx, hy), (-hx, -hy))]
}

fn circle_lines(r: f64) -> Vec<FpLine> {
    let n = 32;
    (0..n)
        .map(|i| {
            let (a, b) = (std::f64::consts::TAU * i as f64 / n as f64, std::f64::consts::TAU * (i + 1) as f64 / n as f64);
            line((r * a.cos(), r * a.sin()), (r * b.cos(), r * b.sin()))
        })
        .collect()
}

/// Silkscreen of the through-hole parts whose look tells the polarity or the size: the body of a radial capacitor
/// with its + and - marks, a box capacitor, an axial resistor, a diode with its cathode bar, a 5 mm LED with its
/// flat. Chip parts and the rest have none (a box around the pads is drawn instead).
fn outline(name: &str) -> Vec<FpLine> {
    match name {
        // radial electrolytic: the can, + above the square pad (pin 1) and - above the other lead
        "CP_Radial_D5_P2.0" => {
            let mut v = circle_lines(2.7);
            v.extend([line((-1.25, 1.7), (-0.75, 1.7)), line((-1.0, 1.45), (-1.0, 1.95)), line((0.75, 1.7), (1.25, 1.7))]);
            v
        }
        "C_Disc_5.08" => rect_lines(3.7, 2.0),
        "R_Axial_10.16" => rect_lines(3.2, 1.3),
        // the cathode (K, left) is marked by a bar on the body
        "D_DO-35_7.62" => {
            let mut v = rect_lines(1.9, 0.9);
            v.push(line((-1.2, -0.9), (-1.2, 0.9)));
            v
        }
        // 5 mm LED: the flat side is the cathode (K, left)
        "LED_5mm" => {
            let mut v = circle_lines(2.9);
            v.push(line((-2.5, -1.47), (-2.5, 1.47)));
            v
        }
        _ => vec![],
    }
}

#[cfg(test)]
mod wire_pad_tests {
    use super::*;

    #[test]
    fn wire_pads_are_round_holes_on_the_header_pitch() {
        let fp = get("WirePad_1x5").unwrap();
        assert_eq!(fp.pads.len(), 5);
        assert!(fp.pads.iter().all(|p| p.drill == Some(1.2) && matches!(p.shape, PadShape::Circle { d } if (d - 2.2).abs() < 1e-9)));
        assert!((fp.pads[1].pos.y - fp.pads[0].pos.y + 2.54).abs() < 1e-9);
        assert_eq!(get("C_1206").unwrap().pads.len(), 2);
        assert!(get("WirePad_1x41").is_none() && names().contains(&"WirePad_1x3".to_string()));
    }
}
