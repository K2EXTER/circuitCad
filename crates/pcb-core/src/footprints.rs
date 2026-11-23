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
    FpPad { pin: pin.to_string(), pos: Point::new(x, y), shape: shape.smd_default(drill), drill, slot: 0.0, slot_v: false }
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
    wire_pads_sized(name, pins, 2.2, 1.2)
}

/// [`wire_pads`] with the pad diameter and the hole diameter given (mm): a smaller hole for a thin wire.
fn wire_pads_sized(name: &str, pins: &[String], pad_d: f64, drill: f64) -> Footprint {
    let pitch = 2.54;
    Footprint {
        name: name.into(),
        holes: vec![],
        height: None,
        silk: vec![],
        body: None,
        pads: pins.iter().enumerate().map(|(i, p)| pad(p, 0.0, -(i as f64) * pitch, PadShape::Circle { d: pad_d }, Some(drill))).collect(),
    }
}

pub fn names() -> Vec<String> {
    let mut v: Vec<String> = [
        "R_0603", "R_0805", "R_0805_Reflow", "R_1206", "R_2512", "C_0402", "C_0603", "C_0805", "C_0805_Reflow", "C_1206", "C_1210", "L_0805", "LED_0805", "D_SOD-123", "CT_3528-21", "R_Axial_10.16", "C_Disc_5.08", "CP_Radial_D5_P2.0",
        "D_DO-35_7.62", "LED_5mm", "TO-92", "TO-92-3", "SOT-23", "SOT-23-3", "SOT-23-5", "SOT-23-6", "SOT-223", "TO-252", "TO-263", "TO-220-3", "PinHeader_1x2_PM", "Pot_1x3_P2.54",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    for n in 2..=6 {
        v.push(format!("PinHeader_1x{n}"));
    }
    // 2-row headers of the usual sizes (ISP 2x3, JTAG / IDC 2x5, Raspberry Pi 2x20); any 2xN is read by `get`
    for n in [3, 4, 5, 8, 10, 20] {
        v.push(format!("PinHeader_2x{n}"));
    }
    for n in 2..=6 {
        v.push(format!("WirePad_1x{n}"));
    }
    v
}

pub fn get(name: &str) -> Option<Footprint> {
    let fp = match name {
        // 0402 and 1210 chip capacitors (nominal IPC lands)
        "C_0402" => smd2(name, 0.51, 0.54, 0.64, ["1", "2"]),
        "C_1210" => smd2(name, 1.5, 1.2, 2.9, ["1", "2"]),
        "R_0603" | "C_0603" => smd2(name, 0.825, 0.8, 0.95, ["1", "2"]),
        "R_0805" | "C_0805" | "L_0805" => smd2(name, 0.9125, 1.025, 1.4, ["1", "2"]),
        // the reflow land YAGEO recommends for its RC chip resistors (Chip Resistor Surface Mount, Fig. 4 / Table 1: overall 3.0, gap 1.2,
        // pad length 0.9, pad width 1.2). The normal 0805 above has a 0.8 gap and 1.4-wide pads, which is easier to hand-solder. Samsung
        // gives no land for its CL21 capacitors, so the capacitor takes the same numbers.
        "R_0805_Reflow" | "C_0805_Reflow" => smd2(name, 1.05, 0.9, 1.2, ["1", "2"]),
        // IPC-7351 nominal land patterns for the larger chips (1/4 W and 1 W resistors)
        "R_1206" | "C_1206" => smd2(name, 1.5, 1.2, 1.8, ["1", "2"]),
        "R_2512" => smd2(name, 2.95, 1.4, 3.4, ["1", "2"]),
        "LED_0805" => smd2(name, 0.9125, 1.025, 1.4, ["K", "A"]),
        "D_SOD-123" => smd2(name, 1.65, 0.91, 1.22, ["K", "A"]),
        // molded tantalum chip, case B (EIA 3528-21): KEMET's land for density level B ("median", Table 2 of the T491 data sheet):
        // pad 1.80 long x 2.23 wide, gap 1.12. Pin 1 is the + end (the stripe on the body).
        "CT_3528-21" => smd2(name, 1.46, 1.8, 2.23, ["1", "2"]),
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
        // the TO-92 with its pins numbered left to right (2N3904: 1 = E, 2 = B, 3 = C), pin 1 square
        "TO-92-3" => {
            let s = PadShape::Circle { d: 1.5 };
            Footprint { name: name.into(), holes: vec![], height: None, silk: vec![], body: None, pads: vec![pad("1", -2.54, 0.0, PadShape::Rect { w: 1.5, h: 1.5 }, Some(0.8)), pad("2", 0.0, 0.0, s, Some(0.8)), pad("3", 2.54, 0.0, s, Some(0.8))] }
        }
        // the SOT-23 with its pins numbered (1 = B, 2 = E, 3 = C of a transistor; a MOSFET's G, S, D): parts made from a data
        // sheet number their pins
        "SOT-23-3" => {
            let s = PadShape::Rect { w: 0.9, h: 0.8 };
            Footprint { name: name.into(), holes: vec![], height: None, silk: vec![], body: None, pads: vec![pad("1", -0.95, 0.95, s, None), pad("2", 0.95, 0.95, s, None), pad("3", 0.0, -0.95, s, None)] }
        }
        // ICs' small outlines, pins numbered (the lands of the bundled MCP6001 and AMS1117, as KiCad's): SOT-23-5/6 pins 1 to
        // 3 along one side and the rest back along the other
        "SOT-23-5" | "SOT-23-6" => {
            let s = PadShape::Rect { w: 0.6, h: 1.1 };
            let n = if name == "SOT-23-5" { 5 } else { 6 };
            let mut pads = vec![pad("1", -0.95, -1.3, s, None), pad("2", 0.0, -1.3, s, None), pad("3", 0.95, -1.3, s, None), pad("4", 0.95, 1.3, s, None)];
            if n == 5 {
                pads.push(pad("5", -0.95, 1.3, s, None));
            } else {
                pads.extend([pad("5", 0.0, 1.3, s, None), pad("6", -0.95, 1.3, s, None)]);
            }
            Footprint { name: name.into(), holes: vec![], height: None, silk: vec![], body: None, pads }
        }
        // SOT-223: three pins and the tab, which is pin 2 (regulators' output, transistors' collector: KiCad's TabPin2)
        "SOT-223" => {
            let s = PadShape::Rect { w: 2.0, h: 1.5 };
            Footprint {
                name: name.into(),
                holes: vec![],
                height: None,
                silk: vec![],
                body: None,
                pads: vec![pad("1", -3.15, 2.3, s, None), pad("2", -3.15, 0.0, s, None), pad("3", -3.15, -2.3, s, None), pad("2", 3.15, 0.0, PadShape::Rect { w: 2.0, h: 3.8 }, None)],
            }
        }
        // DPAK (TO-252) and D2PAK (TO-263) lying down: the two outer leads and the tab, which is pin 2 (the middle lead is cut),
        // as onsemi's recommended footprints (MC7800/D, CASE 369C and CASE 936, 候補 Q17): DPAK leads 1.60 × 3.00 with their
        // outer edges 6.17 apart, 2.58 from the 5.80 × 6.20 tab; D2PAK leads 1.016 × 3.504 at a 5.080 pitch, the 10.490 × 8.380
        // tab, 16.155 overall. The leads point left, as the SOT-223's.
        "TO-252" | "DPAK" | "TO-263" | "D2PAK" => {
            let (lead, pitch, gap, tab) = if matches!(name, "TO-252" | "DPAK") { ((3.00, 1.60), 4.57, 2.58, (6.20, 5.80)) } else { ((3.504, 1.016), 5.08, 16.155 - 3.504 - 8.380, (8.380, 10.490)) };
            let half = (lead.0 + gap + tab.0) / 2.0;
            let s = PadShape::Rect { w: lead.0, h: lead.1 };
            let lx = -half + lead.0 / 2.0;
            Footprint {
                name: name.into(),
                holes: vec![],
                height: None,
                silk: vec![],
                body: None,
                pads: vec![pad("1", lx, pitch / 2.0, s, None), pad("3", lx, -pitch / 2.0, s, None), pad("2", half - tab.0 / 2.0, 0.0, PadShape::Rect { w: tab.0, h: tab.1 }, None)],
            }
        }
        // TO-220 standing up: three leads 2.54 apart, pin 1 square (the bundled LM7805's)
        "TO-220-3" => {
            let s = PadShape::Circle { d: 2.0 };
            Footprint {
                name: name.into(),
                holes: vec![],
                height: None,
                silk: vec![],
                body: None,
                pads: vec![pad("1", -2.54, 0.0, PadShape::Rect { w: 2.0, h: 2.0 }, Some(1.1)), pad("2", 0.0, 0.0, s, Some(1.1)), pad("3", 2.54, 0.0, s, Some(1.1))],
            }
        }
        "PinHeader_1x2_PM" => header(name, &["+".to_string(), "-".to_string()]),
        // a small potentiometer for a board or a breadboard: three leads in a row 2.54 apart (1, the wiper 2, 3), pin 1 square
        "Pot_1x3_P2.54" => {
            let s = PadShape::Circle { d: 1.8 };
            Footprint {
                name: name.into(),
                holes: vec![],
                height: Some(7.0),
                silk: vec![],
                body: None,
                pads: vec![pad("1", -2.54, 0.0, PadShape::Rect { w: 1.8, h: 1.8 }, Some(1.0)), pad("2", 0.0, 0.0, s, Some(1.0)), pad("3", 2.54, 0.0, s, Some(1.0))],
            }
        }
        // "WirePad_D0.7_P1.6": one pad for a wire, hole 0.7 mm and pad 1.6 mm (the pad is at least 0.4 mm wider than the hole)
        _ if name.starts_with("WirePad_D") => {
            let rest = name.strip_prefix("WirePad_D")?;
            let (d, p) = rest.split_once("_P")?;
            let (drill, pad_d): (f64, f64) = (d.parse().ok()?, p.parse().ok()?);
            if !(0.3..=3.0).contains(&drill) || pad_d < drill + 0.4 || pad_d > 6.0 {
                return None;
            }
            wire_pads_sized(name, &["1".to_string()], pad_d, drill)
        }
        _ if name.starts_with("WirePad_1x") => {
            let n: usize = name.strip_prefix("WirePad_1x")?.parse().ok()?;
            if !(1..=40).contains(&n) {
                return None;
            }
            wire_pads(name, &(1..=n).map(|i| i.to_string()).collect::<Vec<_>>())
        }
        _ if name.starts_with("PinHeader_2x") => {
            let n: usize = name.strip_prefix("PinHeader_2x")?.parse().ok()?;
            if !(1..=40).contains(&n) {
                return None;
            }
            crate::package::PackageSpec::DoubleRow { pins: 2 * n, pitch: 2.54, row_spacing: 2.54, pad_d: 1.7, drill: 1.0 }.build(name).ok()?
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
        // molded tantalum 3528-21: the long edges of the body outside the pads, a bar across the + end (pin 1, left) and a +
        "CT_3528-21" => vec![
            line((-2.36, 1.55), (2.36, 1.55)),
            line((-2.36, -1.55), (2.36, -1.55)),
            line((-2.75, 1.55), (-2.75, -1.55)),
            line((-3.5, 0.0), (-3.0, 0.0)),
            line((-3.25, 0.25), (-3.25, -0.25)),
        ],
        // potentiometer: the square body behind the leads and the knob's slot
        "Pot_1x3_P2.54" => vec![
            line((-4.8, 1.6), (4.8, 1.6)),
            line((4.8, 1.6), (4.8, -8.0)),
            line((4.8, -8.0), (-4.8, -8.0)),
            line((-4.8, -8.0), (-4.8, 1.6)),
            line((-2.0, -3.2), (2.0, -3.2)),
            line((0.0, -5.2), (0.0, -1.2)),
        ],
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
    fn small_ic_outlines_have_numbered_pads() {
        let count = |n: &str| get(n).map(|f| f.pads.len());
        assert_eq!(count("TO-92-3"), Some(3));
        assert_eq!((count("SOT-23-3"), count("SOT-23-5"), count("SOT-23-6"), count("SOT-223"), count("TO-220-3")), (Some(3), Some(5), Some(6), Some(4), Some(3)));
        // the numbered SOT-23 lies as the lettered one
        let (a, b) = (get("SOT-23").unwrap(), get("SOT-23-3").unwrap());
        assert!(a.pads.iter().zip(&b.pads).all(|(p, q)| (p.pos.x - q.pos.x).abs() < 1e-9 && (p.pos.y - q.pos.y).abs() < 1e-9));
        let tab = get("SOT-223").unwrap();
        assert_eq!(tab.pads.iter().filter(|p| p.pin == "2").count(), 2, "the tab is pin 2");
        assert!(get("TO-220-3").unwrap().pads.iter().all(|p| p.drill == Some(1.1)));
        // onsemi's DPAK: the leads' outer edges 6.17 apart, 2.58 from the tab; its D2PAK 16.155 overall
        let d = get("TO-252").unwrap();
        let (l1, l3, t) = (&d.pads[0], &d.pads[1], &d.pads[2]);
        assert!(((l1.pos.y - l3.pos.y).abs() + 1.6 - 6.17).abs() < 1e-9 && t.pin == "2");
        assert!(((t.pos.x - 3.1) - (l1.pos.x + 1.5) - 2.58).abs() < 1e-9);
        let d2 = get("D2PAK").unwrap();
        assert!(((d2.pads[2].pos.x + 8.380 / 2.0) - (d2.pads[0].pos.x - 3.504 / 2.0) - 16.155).abs() < 1e-9);
        for n in ["SOT-23-5", "SOT-23-6", "SOT-223", "TO-220-3"] {
            assert!(names().iter().any(|m| m == n), "{n}");
        }
    }

    #[test]
    fn wire_pads_are_round_holes_on_the_header_pitch() {
        let fp = get("WirePad_1x5").unwrap();
        assert_eq!(fp.pads.len(), 5);
        assert!(fp.pads.iter().all(|p| p.drill == Some(1.2) && matches!(p.shape, PadShape::Circle { d } if (d - 2.2).abs() < 1e-9)));
        assert!((fp.pads[1].pos.y - fp.pads[0].pos.y + 2.54).abs() < 1e-9);
        assert_eq!(get("C_1206").unwrap().pads.len(), 2);
        assert!(get("C_0402").is_some() && get("C_1210").is_some());
        let thin = get("WirePad_D0.7_P1.6").unwrap();
        assert_eq!((thin.pads[0].drill, thin.pads[0].shape.size()), (Some(0.7), (1.6, 1.6)));
        assert!(get("WirePad_D1.0_P1.2").is_none(), "a ring of less than 0.2 mm each side is refused");
        assert!(get("WirePad_1x41").is_none() && names().contains(&"WirePad_1x3".to_string()));
    }

    #[test]
    fn the_reflow_0805_follows_the_yageo_numbers() {
        // Table 1: overall 3.0, gap 1.2, pad length 0.9, pad width 1.2 (overall = gap + 2 x pad length)
        for n in ["R_0805_Reflow", "C_0805_Reflow"] {
            let fp = get(n).unwrap();
            assert!(names().contains(&n.to_string()), "{n} is offered in the picker");
            let (w, h) = fp.pads[0].shape.size();
            assert!((w - 0.9).abs() < 1e-9 && (h - 1.2).abs() < 1e-9, "{n}: {w} x {h}");
            let gap = fp.pads[1].pos.x - fp.pads[0].pos.x - w;
            let overall = fp.pads[1].pos.x - fp.pads[0].pos.x + w;
            assert!((gap - 1.2).abs() < 1e-9 && (overall - 3.0).abs() < 1e-9, "{n}: gap {gap}, overall {overall}");
        }
        // the normal 0805 is not changed
        let n = get("R_0805").unwrap();
        assert!((n.pads[1].pos.x - n.pads[0].pos.x - 1.825).abs() < 1e-9);
    }

    #[test]
    fn the_tantalum_case_b_follows_the_kemet_land_and_marks_the_plus_end() {
        let fp = get("CT_3528-21").unwrap();
        assert!(names().contains(&"CT_3528-21".to_string()));
        let (w, h) = fp.pads[0].shape.size();
        assert!((w - 1.8).abs() < 1e-9 && (h - 2.23).abs() < 1e-9, "{w} x {h}");
        // gap 1.12 between the pads
        let gap = fp.pads[1].pos.x - fp.pads[0].pos.x - w;
        assert!((gap - 1.12).abs() < 1e-9, "{gap}");
        // pin 1 (+) is on the left, and the polarity marks are on that side only, clear of the copper
        assert!(fp.pads[0].pos.x < 0.0 && fp.pads[0].pin == "1");
        let left_only = fp.silk.iter().filter(|l| l.a.x < -2.4 && l.b.x < -2.4).count();
        assert_eq!(left_only, 3, "a bar and a +: {:?}", fp.silk);
        for l in &fp.silk {
            for p in &fp.pads {
                let (hw, hh) = (p.shape.size().0 / 2.0, p.shape.size().1 / 2.0);
                for q in [l.a, l.b] {
                    let inside = (q.x - p.pos.x).abs() < hw + 0.2 && (q.y - p.pos.y).abs() < hh + 0.2;
                    assert!(!inside, "silk point {q:?} is on a pad at {:?}", p.pos);
                }
            }
        }
    }
}
