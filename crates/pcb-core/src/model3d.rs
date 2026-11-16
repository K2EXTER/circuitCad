//! Rough 3D shapes for the board preview: bodies and leads for every part, sized from its pads and
//! a height guessed from the footprint. They show how the board will look and help to spot parts
//! that collide with a case; they are not real component models.

use crate::model::*;
use std::collections::BTreeSet;

/// FR-4 thickness used for the preview (mm). The usual 2-layer board is 1.6 mm.
pub const BOARD_THICKNESS: f64 = 1.6;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BodyShape {
    Box,
    /// A cylinder along this axis (0 = x, 1 = y, 2 = z); the box's other two sizes give its width.
    Cylinder(u8),
}

/// One solid of a part: `lo`..`hi` in board coordinates (z = 0 is the bottom face, z = `BOARD_THICKNESS` the top).
#[derive(Clone, Debug, PartialEq)]
pub struct PartBox {
    pub refdes: String,
    /// Index into `Board::parts`.
    pub part: usize,
    pub lo: [f64; 3],
    pub hi: [f64; 3],
    pub color: [u8; 3],
    pub bottom: bool,
    pub shape: BodyShape,
}

/// What a part looks like, as far as the preview can tell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// A chip with end caps (0603, 0805, SOD-123 ...).
    Chip,
    /// A leaded body lying on the board (axial resistor, diode).
    Axial,
    /// An upright can (electrolytic, disc capacitor).
    Can,
    /// A 5 mm LED.
    Led,
    /// A pin header: plastic body and long pins.
    Header,
    /// A TO-92 (the small black transistor with a flat face): the body across the row of its three legs.
    To92,
    /// A TO-220 standing on the board: plastic body with a metal tab above it.
    To220,
    /// A surface-mount IC or transistor with legs.
    SmdIc,
    /// A through-hole IC (DIP).
    Dip,
    /// A connector in a metal shell (USB, DC jack, RJ45): a plastic base with the shell over it.
    Shell,
    /// A screw terminal block: a green block with a screw on top of each terminal.
    Terminal,
    /// A mains / EI transformer: the iron core with the coil showing at the ends.
    Transformer,
    /// A round buzzer: a black can with the sound hole in the top.
    Buzzer,
    /// A holder for a cylindrical cell (AA, AAA, 18650): an open tray with the cell lying in it.
    BatteryTray,
    /// A coin cell holder (CR2032 ...): a round base, the coin and the spring clip over it.
    CoinHolder,
    /// A holder for a 5 x 20 mm glass fuse: two clips with the fuse lying in them.
    FuseHolder,
    /// An HC-49 quartz crystal: a metal can with round ends.
    Crystal,
    /// A relay: a closed box with a printed label on top.
    Relay,
    /// A tactile push switch: a square body, a metal plate and a round button.
    Tact,
    /// A slide switch: a body with a knob on top.
    Slide,
    /// A DIP switch: a body between the two rows of pins with a little lever for every switch.
    DipSwitch,
    /// Anything else: one box.
    Other,
}

fn is_through_hole(fp: &str) -> bool {
    ["axial", "disc", "dip", "to-92", "to-220", "pinheader", "5mm", "do-", "transformer", "radial"].iter().any(|k| fp.contains(k))
}

/// Does this battery holder footprint (lower case) hold a coin cell? KiCad names them by the cell (CR2032) or by the size
/// (Keystone_3034_1x20mm, Keystone_1058_1x2032).
fn is_coin_holder(fp: &str) -> bool {
    ["cr2", "cr1", "coin", "x20mm", "x2032", "x12mm", "x16mm", "x1220"].iter().any(|k| fp.contains(k))
}

/// The diameter of the cell a holder takes (mm), from its footprint name (lower case).
fn cell_diameter(fp: &str) -> Option<f64> {
    if is_coin_holder(fp) {
        return Some(if fp.contains("1220") || fp.contains("12mm") { 12.5 } else if fp.contains("16") { 16.0 } else { 20.0 });
    }
    // longer names first: "aaa" contains "aa"
    [("18650", 18.4), ("aaa", 10.5), ("aa", 14.5)].iter().find(|(k, _)| fp.contains(k)).map(|r| r.1)
}

/// The height of a tactile switch written in its footprint name ("SW_PUSH_6mm_H5mm"), mm above the board.
fn height_in_name(fp: &str) -> Option<f64> {
    fp.match_indices("_h").find_map(|(at, _)| {
        let rest = &fp[at + 2..];
        rest[..rest.find("mm")?].parse::<f64>().ok().filter(|h| (1.0..=30.0).contains(h))
    })
}

fn prefix(refdes: &str) -> char {
    refdes.trim_start_matches(|c: char| !c.is_ascii_alphabetic()).chars().next().map(|c| c.to_ascii_uppercase()).unwrap_or('?')
}

/// The kind of part from its reference designator and footprint name.
pub fn part_kind(refdes: &str, footprint: &str) -> Kind {
    let fp = footprint.to_ascii_lowercase();
    let th = is_through_hole(&fp);
    // a pin header is a pin header whatever its designator says (a source or a probe point on a header is V1, TP1 ...)
    if fp.contains("pinheader") {
        return Kind::Header;
    }
    if fp.contains("to-220") {
        return Kind::To220;
    }
    if fp.contains("to-92") {
        return Kind::To92;
    }
    if fp.contains("terminalblock") {
        return Kind::Terminal;
    }
    if fp.contains("transformer") {
        return Kind::Transformer;
    }
    if fp.contains("buzzer") {
        return Kind::Buzzer;
    }
    if fp.contains("batteryholder") {
        return if is_coin_holder(&fp) { Kind::CoinHolder } else { Kind::BatteryTray };
    }
    if fp.contains("5x20") || fp.contains("fuseholder") {
        return Kind::FuseHolder;
    }
    if fp.contains("hc49") || fp.contains("hc-49") {
        return Kind::Crystal;
    }
    if fp.contains("relay") {
        return Kind::Relay;
    }
    // a DIP switch footprint also says "slide" (KiCad's SW_DIP_SPSTx04_Slide_...), so it is asked first
    if fp.contains("sw_dip") {
        return Kind::DipSwitch;
    }
    if fp.contains("sw_push") || fp.contains("tact") {
        return Kind::Tact;
    }
    if fp.contains("sw_slide") {
        return Kind::Slide;
    }
    // a connector is told by its footprint (and not by a letter alone: a USB protection IC is not a connector)
    if matches!(prefix(refdes), 'J' | 'P' | 'X') && ["usb", "jack", "barrel", "rj45"].iter().any(|k| fp.contains(k)) {
        return Kind::Shell;
    }
    match prefix(refdes) {
        'R' | 'L' | 'C' | 'D' if fp.contains("5mm") => Kind::Led,
        'R' if fp.contains("axial") => Kind::Axial,
        'D' if fp.contains("do-") => Kind::Axial,
        'C' if th => Kind::Can,
        'R' | 'C' | 'L' | 'D' if !th => Kind::Chip,
        'J' | 'P' | 'H' => Kind::Header,
        'U' | 'Q' if fp.contains("dip") => Kind::Dip,
        'U' | 'Q' if !th => Kind::SmdIc,
        _ => Kind::Other,
    }
}

/// Footprints (a part of the name, in lower case) of parts that stand tall, with the typical height of the body in mm and its colour.
/// Estimates from the usual packages: the real part may be a few mm different, so give the footprint a `height` when it matters.
const TALL_FOOTPRINTS: &[(&str, f64, [u8; 3])] = &[
    // connectors with a metal shell; the sizes are the usual ones of each family (the maker's drawing may differ by a few mm)
    ("usb_micro", 2.8, [190, 190, 200]),
    ("usb_c", 3.3, [190, 190, 200]),
    ("usb_b", 11.0, [190, 190, 200]),
    ("usb", 7.0, [190, 190, 200]),
    ("barrel", 11.0, [40, 40, 44]),
    ("jack", 10.0, [40, 40, 44]),
    ("buzzer", 9.0, [30, 30, 34]),
    ("xtal_3225", 0.9, [190, 190, 200]),
    ("terminalblock", 10.0, [30, 110, 60]),
    ("transformer", 25.0, [60, 60, 70]),
    ("batteryholder_18650", 21.0, [40, 40, 44]),
    ("batteryholder_aaa", 12.5, [40, 40, 44]),
    ("batteryholder_aa", 16.0, [40, 40, 44]),
    ("batteryholder_cr2032", 5.0, [150, 150, 158]),
    ("5x20", 9.0, [200, 190, 150]),
    ("fuseholder", 9.0, [200, 190, 150]),
    // HC-49: the tall can (HC-49/U, 13.5 mm), the low one (HC-49/S, KiCad's HC49-4H, 3.5 mm) and the one lying on a plastic base (SMD)
    ("hc49-4h", 3.5, [196, 198, 204]),
    ("hc-49s", 3.5, [196, 198, 204]),
    ("hc49s", 3.5, [196, 198, 204]),
    ("smd_hc49", 4.3, [196, 198, 204]),
    ("hc49", 13.5, [196, 198, 204]),
    ("hc-49", 13.5, [196, 198, 204]),
    // relays: the small signal relay (Omron G6K), the 10 A power relays (Omron G5LE, the blue Songle SRD) and the rest
    ("g6k", 5.2, [30, 30, 34]),
    ("g5le", 18.8, [30, 30, 34]),
    ("srd", 15.3, [40, 80, 170]),
    ("relay", 15.0, [30, 30, 34]),
    // switches: a DIP switch first (its name also says "slide")
    ("sw_dip", 5.0, [40, 80, 170]),
    ("sw_push", 4.3, [30, 30, 34]),
    ("tact", 4.3, [30, 30, 34]),
    ("sw_slide", 7.0, [196, 198, 204]),
];

/// How far a TO-220's metal tab stands above its plastic body (mm).
const TO220_TAB: f64 = 6.5;

/// Body height (mm above the board) and colour guessed from the reference designator and footprint.
pub fn part_style(refdes: &str, footprint: &str) -> (f64, [u8; 3]) {
    let fp = footprint.to_ascii_lowercase();
    let th = is_through_hole(&fp);
    if fp.contains("pinheader") {
        return (2.5, [24, 24, 26]);
    }
    // parts that stand tall by their footprint, whatever they are called: typical heights, for the case check (a footprint's
    // own `height` wins over these; they are estimates, not a maker's drawing)
    if let Some(r) = TALL_FOOTPRINTS.iter().find(|(key, _, _)| fp.contains(key)) {
        // a tactile switch says its height in the name (SW_PUSH_6mm_H5mm)
        let h = if matches!(part_kind(refdes, footprint), Kind::Tact) { height_in_name(&fp).unwrap_or(r.1) } else { r.1 };
        return (h, r.2);
    }
    // a battery holder by another name (Keystone_3034_1x20mm): low for a coin, as tall as the cell otherwise
    if fp.contains("batteryholder") {
        return match cell_diameter(&fp) {
            Some(_) if is_coin_holder(&fp) => (5.0, [150, 150, 158]),
            Some(d) => (d + 1.5, [40, 40, 44]),
            None => (16.0, [40, 40, 44]),
        };
    }
    match prefix(refdes) {
        'R' if th => (2.4, [196, 160, 110]),
        'R' => (0.55, [30, 30, 32]),
        'C' if th => (4.5, [200, 130, 40]),
        'C' => (0.9, [160, 120, 70]),
        'L' => (2.5, [90, 90, 100]),
        'D' if fp.contains("5mm") || fp.contains("led") => (if fp.contains("5mm") { 8.5 } else { 0.8 }, [220, 60, 50]),
        'D' if th => (2.7, [30, 30, 32]),
        'D' => (1.0, [36, 36, 40]),
        'Q' | 'U' if fp.contains("to-220") => (9.3, [40, 40, 44]),
        'Q' if fp.contains("to-92") => (4.5, [32, 32, 36]),
        'Q' => (1.1, [36, 36, 40]),
        'U' if fp.contains("dip") => (3.3, [28, 28, 32]),
        'U' if fp.contains("to-92") => (4.5, [32, 32, 36]),
        'U' if fp.contains("tqfp") || fp.contains("qfn") => (1.2, [30, 30, 34]),
        'U' => (1.6, [30, 30, 34]),
        'J' | 'P' | 'H' => (2.5, [24, 24, 26]),
        'T' => (15.0, [70, 70, 80]),
        'Y' | 'X' => (3.5, [190, 190, 200]),
        _ if th => (3.0, [60, 60, 66]),
        _ => (2.0, [90, 90, 96]),
    }
}

/// Total height above the board including the leads that stick out (pins of a header), for case checks.
pub fn part_total_height(refdes: &str, footprint: &str) -> f64 {
    let (h, _) = part_style(refdes, footprint);
    match part_kind(refdes, footprint) {
        Kind::Header => h + 6.0,
        Kind::To220 => h + TO220_TAB,
        _ => h,
    }
}

/// `part_style` with the body height of the footprint when it says one (a library part's data sheet value).
fn style_in(board: &Board, part: &Part) -> (f64, [u8; 3]) {
    let (h, color) = part_style(&part.refdes, &part.footprint);
    match board.footprint(&part.footprint).and_then(|f| f.height).filter(|h| h.is_finite() && *h > 0.0) {
        Some(given) => (given, color),
        None => (h, color),
    }
}

/// `part_total_height` for a part on a board: the footprint's own height wins over the guess.
pub fn part_height_on(board: &Board, part: &Part) -> f64 {
    let (h, _) = style_in(board, part);
    match part_kind(&part.refdes, &part.footprint) {
        Kind::Header => h + 6.0,
        Kind::To220 if board.footprint(&part.footprint).and_then(|f| f.height).is_none() => h + TO220_TAB,
        _ => h,
    }
}

/// The pad that carries one of the pin names (the first name wins), for polarity and pin 1 marks.
fn pad_named<'a>(pads: &[&'a Pad], names: &[&str]) -> Option<&'a Pad> {
    names.iter().find_map(|n| pads.iter().copied().find(|p| p.pin == *n))
}

/// Is the capacitor of this footprint polarized (an electrolytic)? By the footprint name or a "+" pin.
fn is_polarized(footprint: &str, pads: &[&Pad]) -> bool {
    let fp = footprint.to_ascii_lowercase();
    fp.contains("cp_") || fp.contains("radial") || fp.contains("elec") || pads.iter().any(|p| p.pin == "+")
}

const MARK: [u8; 3] = [226, 226, 230];
const CATHODE_LED: [u8; 3] = [70, 190, 100];

const SILVER: [u8; 3] = [196, 198, 204];
const GOLD: [u8; 3] = [214, 176, 58];

struct Solid {
    /// Offsets along x, y and above the board surface (z), low and high.
    lo: [f64; 3],
    hi: [f64; 3],
    color: [u8; 3],
    shape: BodyShape,
}

fn solid(lo: [f64; 3], hi: [f64; 3], color: [u8; 3], shape: BodyShape) -> Solid {
    Solid { lo, hi, color, shape }
}

fn b_box() -> BodyShape {
    BodyShape::Box
}

/// A pale dot on the top of an IC body, in the corner next to pin 1 (the pad named "1"), like the printed dot or
/// notch of the real package. Nothing when the part has no pin named "1".
fn pin1_dot(pads: &[&Pad], lo: [f64; 2], hi: [f64; 2], top: f64) -> Option<Solid> {
    let p1 = pad_named(pads, &["1"])?;
    let (cx, cy) = ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0);
    let (hw, hh) = ((hi[0] - lo[0]) / 2.0, (hi[1] - lo[1]) / 2.0);
    if hw <= 0.0 || hh <= 0.0 {
        return None;
    }
    let r = (hw.min(hh) * 0.16).clamp(0.2, 0.6);
    // towards the corner on pin 1's side; an axis the pad is centred on stays centred
    let towards = |off: f64, half: f64| if off.abs() < half * 0.2 { 0.0 } else { off.signum() * (half - r - 0.25).max(0.0) };
    let (x, y) = (cx + towards(p1.pos.x - cx, hw), cy + towards(p1.pos.y - cy, hh));
    Some(solid([x - r, y - r, top], [x + r, y + r, top + 0.05], MARK, BodyShape::Cylinder(2)))
}

/// The body the data sheet gives (size, orientation mark): a box or a cylinder of its height, with the mark on top.
/// `local` positions are in the footprint's frame; a bottom part is mirrored and the part is turned by `rot`.
fn data_sheet_body(part: &Part, pads: &[&Pad], body: &crate::body::BodySpec, h: f64, color: [u8; 3]) -> Vec<Solid> {
    use crate::body::BodyMark;
    if let Some(code) = body.color_code.as_ref().filter(|_| pads.len() >= 2) {
        return banded_resistor(part, pads, body, code, color);
    }
    let bottom = part.side == Side::Bottom;
    let place = |x: f64, y: f64| {
        let local = if bottom { Point::new(-x, y) } else { Point::new(x, y) };
        let r = crate::parts::rotate(local, part.rot);
        (part.pos.x + r.x, part.pos.y + r.y)
    };
    let circle = body.shape == "circle";
    let (w, l) = if circle { (body.w, body.w) } else { (body.w, body.h) };
    let (w, l) = if part.rot % 2 == 1 { (l, w) } else { (w, l) };
    let (cx, cy) = place(body.x, body.y);
    // surface-mount parts sit a little above the pads, leaded ones on the board
    let z0 = if pads.iter().any(|p| p.drill.is_some()) { 0.0 } else { 0.15 };
    let top = z0 + h;
    let main = solid([cx - w / 2.0, cy - l / 2.0, z0], [cx + w / 2.0, cy + l / 2.0, top], color, if circle { BodyShape::Cylinder(2) } else { BodyShape::Box });
    let mut v = vec![main];
    let marked = pads.iter().copied().find(|p| body.mark_pin.is_empty() || p.pin == body.mark_pin).or(pads.first().copied());
    if let Some(m) = marked {
        let (dx, dy) = (m.pos.x - cx, m.pos.y - cy);
        let (ux, uy) = if dx.abs() >= dy.abs() { (dx.signum(), 0.0) } else { (0.0, dy.signum()) };
        match body.mark {
            BodyMark::Dot => {
                let (hw, hh) = (w / 2.0, l / 2.0);
                let r = (hw.min(hh) * 0.16).clamp(0.2, 0.6);
                let towards = |off: f64, half: f64| if off.abs() < half * 0.2 { 0.0 } else { off.signum() * (half - r - 0.25).max(0.0) };
                let (x, y) = (cx + towards(dx, hw), cy + towards(dy, hh));
                v.push(solid([x - r, y - r, top], [x + r, y + r, top + 0.05], MARK, BodyShape::Cylinder(2)));
            }
            BodyMark::Bar => {
                // a pale band across the top at the marked pad's end
                let band = ((if ux != 0.0 { w } else { l }) * 0.14).clamp(0.3, 1.0);
                if ux != 0.0 {
                    let (a, b) = if ux > 0.0 { (cx + w / 2.0 - band, cx + w / 2.0) } else { (cx - w / 2.0, cx - w / 2.0 + band) };
                    v.push(solid([a, cy - l / 2.0, top], [b, cy + l / 2.0, top + 0.05], MARK, BodyShape::Box));
                } else {
                    let (a, b) = if uy > 0.0 { (cy + l / 2.0 - band, cy + l / 2.0) } else { (cy - l / 2.0, cy - l / 2.0 + band) };
                    v.push(solid([cx - w / 2.0, a, top], [cx + w / 2.0, b, top + 0.05], MARK, BodyShape::Box));
                }
            }
            // a round can marked + (an electrolytic): the pale stripe down its side on the minus lead's side, as the sleeve is
            // printed; the + sign itself is on the silkscreen
            BodyMark::Plus if circle => {
                let (r, sw, out) = (w / 2.0, w * 0.22, 0.06);
                // the side away from the + pad
                let (sx, sy) = (-ux, -uy);
                v.push(if sx != 0.0 {
                    let (a, b) = if sx < 0.0 { (cx - r - out, cx - r + 0.4) } else { (cx + r - 0.4, cx + r + out) };
                    solid([a, cy - sw / 2.0, z0], [b, cy + sw / 2.0, top], MARK, BodyShape::Box)
                } else {
                    let (a, b) = if sy < 0.0 { (cy - r - out, cy - r + 0.4) } else { (cy + r - 0.4, cy + r + out) };
                    solid([cx - sw / 2.0, a, z0], [cx + sw / 2.0, b, top], MARK, BodyShape::Box)
                });
            }
            BodyMark::Plus | BodyMark::None => {}
        }
    }
    v
}

/// A leaded resistor lying on the board with its colour bands (the code of its value, read from pin 1's end): the body as a
/// cylinder between the two holes `body.w` long and `body.h` across, the lead wire through it, and a ring for every band.
fn banded_resistor(part: &Part, pads: &[&Pad], body: &crate::body::BodySpec, code: &crate::body::ColorCode, color: [u8; 3]) -> Vec<Solid> {
    let first = pads.iter().copied().find(|p| p.pin == "1").unwrap_or(pads[0]);
    let other = pads.iter().copied().find(|p| !std::ptr::eq(*p, first)).unwrap_or(pads[1]);
    let along_x = (other.pos.x - first.pos.x).abs() >= (other.pos.y - first.pos.y).abs();
    let (cx, cy) = ((first.pos.x + other.pos.x) / 2.0, (first.pos.y + other.pos.y) / 2.0);
    let (len, dia) = (body.w, body.h.max(0.5));
    let z0 = 0.3;
    let zc = z0 + dia / 2.0;
    // the axis runs from pin 1 (s = -1 when pin 1 is on the low side)
    let (c_axis, p1_axis) = if along_x { (cx, first.pos.x) } else { (cy, first.pos.y) };
    let s = if p1_axis < c_axis { -1.0 } else { 1.0 };
    let span = |a: f64, b: f64, r: f64, col: [u8; 3]| -> Solid {
        let (a, b) = (a.min(b), a.max(b));
        if along_x {
            solid([a, cy - r, zc - r], [b, cy + r, zc + r], col, BodyShape::Cylinder(0))
        } else {
            solid([cx - r, a, zc - r], [cx + r, b, zc + r], col, BodyShape::Cylinder(1))
        }
    };
    let wire = 0.3;
    let (lead_a, lead_b) = if along_x { (first.pos.x, other.pos.x) } else { (first.pos.y, other.pos.y) };
    let paint = code.body.unwrap_or(color);
    let mut v = vec![span(lead_a, lead_b, wire, SILVER), span(c_axis - len / 2.0, c_axis + len / 2.0, dia / 2.0, paint)];
    if let Some(bands) = crate::body::parse_ohms(&part.value).and_then(|ohms| code.bands_for(ohms)) {
        let width = len * 0.085;
        for (band, f) in bands.iter().zip(crate::body::band_places(bands.len())) {
            let mid = c_axis + s * (len / 2.0 - f * len);
            v.push(span(mid - width / 2.0, mid + width / 2.0, dia / 2.0 + 0.04, band.rgb()));
        }
    }
    v
}

/// All the solids of one part, in board x / y and height above the board surface.
fn solids(board: &Board, part: &Part) -> Vec<Solid> {
    let pads: Vec<&Pad> = board.pads.iter().filter(|p| p.refdes == part.refdes).collect();
    if pads.is_empty() {
        return vec![];
    }
    // a library part with the body from its data sheet: the box of that size and height
    if let Some(body) = board.footprint(&part.footprint).and_then(|f| f.body).filter(|b| b.w > 0.0 && (b.shape == "circle" || b.h > 0.0)) {
        let (h, color) = style_in(board, part);
        return data_sheet_body(part, &pads, &body, h, color);
    }
    let (mut lo, mut hi) = (Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN));
    for p in &pads {
        let (w, h) = p.shape.size();
        lo = Point::new(lo.x.min(p.pos.x - w / 2.0), lo.y.min(p.pos.y - h / 2.0));
        hi = Point::new(hi.x.max(p.pos.x + w / 2.0), hi.y.max(p.pos.y + h / 2.0));
    }
    let (kind, (h, color)) = (part_kind(&part.refdes, &part.footprint), style_in(board, part));
    let fpl = part.footprint.to_ascii_lowercase();
    let prefix = prefix(&part.refdes);
    // the cathode (diode, LED) and the minus lead (electrolytic) of a polarized part
    let cathode = pad_named(&pads, &["K"]);
    let minus = if is_polarized(&part.footprint, &pads) { pad_named(&pads, &["-", "2"]) } else { None };
    let (cx, cy) = ((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
    let (sx, sy) = (hi.x - lo.x, hi.y - lo.y);
    // the long way across the pads
    let along_x = sx >= sy;
    let b = BodyShape::Box;
    match kind {
        Kind::Chip => {
            // body in the middle, a silver cap at each end
            let (len, wid) = if along_x { (sx, sy) } else { (sy, sx) };
            // the real chip is about as long as the distance between the pad centres: the outer half of
            // each pad (where the track meets it) stays visible, not buried under the body
            let centres: Vec<f64> = pads.iter().map(|p| if along_x { p.pos.x } else { p.pos.y }).collect();
            let span = centres.iter().cloned().fold(f64::MIN, f64::max) - centres.iter().cloned().fold(f64::MAX, f64::min);
            let len = if span > 0.0 { (span + 0.1).min(len) } else { len };
            let wid = wid * 0.8;
            let cap = len * 0.2;
            let mk = |a0: f64, a1: f64, c: [u8; 3], hh: f64| {
                if along_x { solid([cx - len / 2.0 + a0, cy - wid / 2.0, 0.0], [cx - len / 2.0 + a1, cy + wid / 2.0, hh], c, b) } else { solid([cx - wid / 2.0, cy - len / 2.0 + a0, 0.0], [cx + wid / 2.0, cy - len / 2.0 + a1, hh], c, b) }
            };
            // a diode's cathode end is white (the printed band), an LED's green
            let mark = if prefix == 'D' && !part.footprint.to_ascii_lowercase().contains("led") { MARK } else { CATHODE_LED };
            let (mut c0, mut c1) = (SILVER, SILVER);
            if let Some(k) = cathode.filter(|_| prefix == 'D') {
                let at = if along_x { k.pos.x } else { k.pos.y };
                let mid = if along_x { cx } else { cy };
                if at < mid {
                    c0 = mark;
                } else {
                    c1 = mark;
                }
            }
            vec![mk(cap, len - cap, color, h), mk(0.0, cap, c0, h), mk(len - cap, len, c1, h)]
        }
        Kind::Axial => {
            let (len, dia) = (if along_x { sx } else { sy } * 0.55, h);
            let (a, c) = if along_x { ([cx - len / 2.0, cy - dia / 2.0, 0.3], [cx + len / 2.0, cy + dia / 2.0, 0.3 + dia]) } else { ([cx - dia / 2.0, cy - len / 2.0, 0.3], [cx + dia / 2.0, cy + len / 2.0, 0.3 + dia]) };
            let wire = 0.45;
            let lead = if along_x { solid([lo.x + 0.6, cy - wire / 2.0, 0.3 + dia / 2.0 - wire / 2.0], [hi.x - 0.6, cy + wire / 2.0, 0.3 + dia / 2.0 + wire / 2.0], SILVER, b) } else { solid([cx - wire / 2.0, lo.y + 0.6, 0.3 + dia / 2.0 - wire / 2.0], [cx + wire / 2.0, hi.y - 0.6, 0.3 + dia / 2.0 + wire / 2.0], SILVER, b) };
            let mut v = vec![lead, solid(a, c, color, BodyShape::Cylinder(if along_x { 0 } else { 1 }))];
            if let Some(k) = cathode.filter(|_| prefix == 'D') {
                // the printed band round the cathode end of the body
                let (band, grow) = (len * 0.16, 0.04);
                let to_low = if along_x { k.pos.x < cx } else { k.pos.y < cy };
                let (b0, b1) = if to_low { (0.0, band) } else { (len - band, len) };
                let (lo_a, hi_a) = if along_x { (cx - len / 2.0 + b0, cx - len / 2.0 + b1) } else { (cy - len / 2.0 + b0, cy - len / 2.0 + b1) };
                let (ca, cb) = (a, c);
                let band_solid = if along_x { solid([lo_a, ca[1] - grow, ca[2] - grow], [hi_a, cb[1] + grow, cb[2] + grow], MARK, BodyShape::Cylinder(0)) } else { solid([ca[0] - grow, lo_a, ca[2] - grow], [cb[0] + grow, hi_a, cb[2] + grow], MARK, BodyShape::Cylinder(1)) };
                v.push(band_solid);
            }
            v
        }
        Kind::Can => {
            let dia = (sx.max(sy) * 2.0).max(5.0);
            let mut v = vec![solid([cx - dia / 2.0, cy - dia / 2.0, 0.0], [cx + dia / 2.0, cy + dia / 2.0, h], color, BodyShape::Cylinder(2))];
            if let Some(m) = minus {
                // the light stripe down the side of the can, on the minus lead's side
                let (dx, dy) = (m.pos.x - cx, m.pos.y - cy);
                let (r, w, out) = (dia / 2.0, dia * 0.22, 0.06);
                v.push(if dx.abs() >= dy.abs() {
                    let (a, b) = if dx < 0.0 { (cx - r - out, cx - r + 0.4) } else { (cx + r - 0.4, cx + r + out) };
                    solid([a, cy - w / 2.0, 0.0], [b, cy + w / 2.0, h], MARK, b_box())
                } else {
                    let (a, b) = if dy < 0.0 { (cy - r - out, cy - r + 0.4) } else { (cy + r - 0.4, cy + r + out) };
                    solid([cx - w / 2.0, a, 0.0], [cx + w / 2.0, b, h], MARK, b_box())
                });
            }
            v
        }
        Kind::Led => {
            let dia = 5.0;
            let mut v = vec![
                solid([cx - dia / 2.0 - 0.3, cy - dia / 2.0 - 0.3, 0.0], [cx + dia / 2.0 + 0.3, cy + dia / 2.0 + 0.3, 1.2], color, BodyShape::Cylinder(2)),
                solid([cx - dia / 2.0, cy - dia / 2.0, 1.2], [cx + dia / 2.0, cy + dia / 2.0, 5.5], color, BodyShape::Cylinder(2)),
                solid([cx - dia * 0.35, cy - dia * 0.35, 5.5], [cx + dia * 0.35, cy + dia * 0.35, h], color, BodyShape::Cylinder(2)),
            ];
            if let Some(k) = cathode {
                // the flat on the cathode side of the rim: a pale patch sticking out a little
                let (r, w, out) = (dia / 2.0 + 0.3, 2.2, 0.06);
                let (dx, dy) = (k.pos.x - cx, k.pos.y - cy);
                v.push(if dx.abs() >= dy.abs() {
                    let (a, b) = if dx < 0.0 { (cx - r - out, cx - r + 0.3) } else { (cx + r - 0.3, cx + r + out) };
                    solid([a, cy - w / 2.0, 0.0], [b, cy + w / 2.0, 1.2], MARK, b_box())
                } else {
                    let (a, b) = if dy < 0.0 { (cy - r - out, cy - r + 0.3) } else { (cy + r - 0.3, cy + r + out) };
                    solid([cx - w / 2.0, a, 0.0], [cx + w / 2.0, b, 1.2], MARK, b_box())
                });
            }
            v
        }
        Kind::Header => {
            let mut v = vec![solid([lo.x - 0.5, lo.y - 0.5, 0.0], [hi.x + 0.5, hi.y + 0.5, h], color, b)];
            for p in &pads {
                v.push(solid([p.pos.x - 0.32, p.pos.y - 0.32, 0.0], [p.pos.x + 0.32, p.pos.y + 0.32, h + 6.0], GOLD, b));
            }
            v
        }
        Kind::To92 => {
            // 4.8 mm across the legs, 3.8 mm deep, the legs a little way up into the body
            let (len, thick) = (4.8, 3.8);
            let (a, p) = if along_x { ([cx - len / 2.0, cy - thick / 2.0], [cx + len / 2.0, cy + thick / 2.0]) } else { ([cx - thick / 2.0, cy - len / 2.0], [cx + thick / 2.0, cy + len / 2.0]) };
            let mut v = vec![solid([a[0], a[1], 0.8], [p[0], p[1], h], color, b)];
            for q in &pads {
                v.push(solid([q.pos.x - 0.25, q.pos.y - 0.25, 0.0], [q.pos.x + 0.25, q.pos.y + 0.25, 0.9], SILVER, b));
            }
            v
        }
        Kind::To220 => {
            // the plastic body across the row of the legs, the metal tab on top of it (which side the tab faces is a guess)
            let (len, thick) = (10.0, 4.5);
            let mk = |a0: f64, a1: f64, p0: f64, p1: f64, z0: f64, z1: f64, c: [u8; 3]| {
                if along_x { solid([cx + a0, cy + p0, z0], [cx + a1, cy + p1, z1], c, b) } else { solid([cx + p0, cy + a0, z0], [cx + p1, cy + a1, z1], c, b) }
            };
            let mut v = vec![mk(-len / 2.0, len / 2.0, -thick / 2.0, thick / 2.0, 0.5, h, color)];
            v.push(mk(-len / 2.0, len / 2.0, -thick / 2.0, -thick / 2.0 + 1.3, h, h + TO220_TAB, SILVER));
            for p in &pads {
                v.push(solid([p.pos.x - 0.35, p.pos.y - 0.35, 0.0], [p.pos.x + 0.35, p.pos.y + 0.35, 0.5], SILVER, b));
            }
            v
        }
        Kind::SmdIc => {
            // legs under the body, the body lifted a little
            let leg_h = 0.25;
            let mut v: Vec<Solid> = pads
                .iter()
                .map(|p| {
                    let (w, hh) = p.shape.size();
                    solid([p.pos.x - w / 2.0, p.pos.y - hh / 2.0, 0.0], [p.pos.x + w / 2.0, p.pos.y + hh / 2.0, leg_h], SILVER, b)
                })
                .collect();
            let pad_len = pads.iter().map(|p| p.shape.size().0.max(p.shape.size().1)).sum::<f64>() / pads.len() as f64;
            let s = (pad_len * 0.35).min(sx.min(sy) * 0.25);
            v.push(solid([lo.x + s, lo.y + s, leg_h], [hi.x - s, hi.y - s, leg_h + h], color, b));
            v.extend(pin1_dot(&pads, [lo.x + s, lo.y + s], [hi.x - s, hi.y - s], leg_h + h));
            v
        }
        Kind::Dip => {
            // two rows of pins: the plastic sits between them
            let xs: BTreeSet<i64> = pads.iter().map(|p| (p.pos.x * 100.0).round() as i64).collect();
            let (shx, shy) = if xs.len() <= 2 && sx > 6.0 { (1.4, 0.2) } else { (0.2, 1.4) };
            let (blo, bhi) = ([lo.x + shx, lo.y + shy], [hi.x - shx, hi.y - shy]);
            let mut v = vec![solid([blo[0], blo[1], 0.0], [bhi[0], bhi[1], h], color, b)];
            v.extend(pin1_dot(&pads, blo, bhi, h));
            v
        }
        Kind::Shell => {
            // a low plastic base the size of the footprint, the metal shell over it (a little inside, so the base shows round the foot)
            let base = (h * 0.12).clamp(0.8, 1.6);
            let s = 0.3_f64.min(sx.min(sy) * 0.1);
            let (slo, shi) = ([lo.x + s, lo.y + s], [hi.x - s, hi.y - s]);
            let mut v = vec![solid([lo.x, lo.y, 0.0], [hi.x, hi.y, base], [30, 30, 34], b), solid([slo[0], slo[1], base], [shi[0], shi[1], h], color, b)];
            let round = ["jack", "barrel"].iter().any(|k| fpl.contains(k));
            v.extend(board.bounds().map(|(blo, bhi)| shell_mouth(slo, shi, [base, h], [cx, cy], (blo, bhi), round)));
            v
        }
        Kind::Terminal => {
            // the block with a screw (and its slot) on top of each terminal
            let mut v = vec![solid([lo.x - 0.5, lo.y - 0.5, 0.0], [hi.x + 0.5, hi.y + 0.5, h], color, b)];
            for p in &pads {
                let r = (sx.min(sy).max(3.5) * 0.4).min(2.2);
                v.push(solid([p.pos.x - r, p.pos.y - r, h], [p.pos.x + r, p.pos.y + r, h + 0.5], [150, 150, 158], BodyShape::Cylinder(2)));
                v.push(solid([p.pos.x - r * 0.8, p.pos.y - 0.2, h + 0.5], [p.pos.x + r * 0.8, p.pos.y + 0.2, h + 0.55], [40, 40, 44], b));
            }
            v
        }
        Kind::Transformer => {
            // the iron core (a little narrower along the pads' long side) with the coil standing out at its two ends
            let (core_lo, core_hi) = if along_x { ([lo.x + sx * 0.12, lo.y], [hi.x - sx * 0.12, hi.y]) } else { ([lo.x, lo.y + sy * 0.12], [hi.x, hi.y - sy * 0.12]) };
            vec![solid([lo.x, lo.y, 0.0], [hi.x, hi.y, h * 0.7], [150, 92, 50], b), solid([core_lo[0], core_lo[1], 0.0], [core_hi[0], core_hi[1], h], color, b)]
        }
        Kind::Buzzer => {
            // the diameter the name says (Buzzer_D12_P7.6), else a guess from the pads
            let named = fpl.match_indices("_d").find_map(|(at, _)| {
                let rest = &fpl[at + 2..];
                rest[..rest.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(rest.len())].parse::<f64>().ok().filter(|d| (5.0..=40.0).contains(d))
            });
            let dia = named.unwrap_or((sx.max(sy) * 1.6).max(9.0));
            vec![solid([cx - dia / 2.0, cy - dia / 2.0, 0.0], [cx + dia / 2.0, cy + dia / 2.0, h], color, BodyShape::Cylinder(2)), solid([cx - 1.2, cy - 1.2, h], [cx + 1.2, cy + 1.2, h + 0.05], [8, 8, 10], BodyShape::Cylinder(2))]
        }
        Kind::BatteryTray => battery_tray(&pads, &fpl, [cx, cy], along_x, h, color),
        Kind::CoinHolder => coin_holder(&pads, &fpl, [cx, cy], along_x, h),
        Kind::FuseHolder => fuse_holder(&pads, [cx, cy], along_x, h),
        Kind::Crystal => {
            // the can is longer than its two legs are apart: the size of the package, round at both ends
            let (len, wid) = if fpl.contains("smd") { (11.4, 4.7) } else { (11.0, 4.65) };
            let (r, a) = (wid / 2.0, len / 2.0 - wid / 2.0);
            let f = Frame { c: [cx, cy], along_x };
            vec![f.at([-a, a], [-r, r], [0.0, h], color, b), f.at([-a - r, -a + r], [-r, r], [0.0, h], color, BodyShape::Cylinder(2)), f.at([a - r, a + r], [-r, r], [0.0, h], color, BodyShape::Cylinder(2))]
        }
        Kind::Relay => {
            // the closed box a little bigger than its pins, with the printed label on top
            let m = 2.0;
            let (blo, bhi) = ([lo.x - m, lo.y - m], [hi.x + m, hi.y + m]);
            let (iw, ih) = ((bhi[0] - blo[0]) * 0.2, (bhi[1] - blo[1]) * 0.25);
            vec![solid([blo[0], blo[1], 0.0], [bhi[0], bhi[1], h], color, b), solid([blo[0] + iw, blo[1] + ih, h], [bhi[0] - iw, bhi[1] - ih, h + 0.05], MARK, b)]
        }
        Kind::Tact => {
            // the square body (6 x 6 or 12 x 12 by the name, else as big as the pads), the metal plate, the round button
            let side = if fpl.contains("12mm") { 12.0 } else if fpl.contains("6mm") || sx.max(sy) >= 6.0 { 6.0 } else { (sx.max(sy) * 0.8).max(2.0) };
            let h = h.max(1.5);
            let (body, plate) = (h * 0.7, h * 0.7 + 0.25);
            let (q, k) = (side / 2.0, side * 0.55 / 2.0);
            vec![
                solid([cx - q, cy - q, 0.0], [cx + q, cy + q, body], [30, 30, 34], b),
                solid([cx - q, cy - q, body], [cx + q, cy + q, plate], SILVER, b),
                solid([cx - k, cy - k, plate], [cx + k, cy + k, h], color, BodyShape::Cylinder(2)),
            ]
        }
        Kind::Slide => {
            // the body along the row of pins, the knob standing on it, pushed to one end
            let f = Frame { c: [cx, cy], along_x };
            let (len, wid) = (if along_x { sx } else { sy } + 1.0, (if along_x { sy } else { sx }).max(3.5));
            let top = h * 0.55;
            let (kl, kw) = (len * 0.18, wid * 0.4);
            vec![f.at([-len / 2.0, len / 2.0], [-wid / 2.0, wid / 2.0], [0.0, top], color, b), f.at([-len / 6.0 - kl / 2.0, -len / 6.0 + kl / 2.0], [-kw / 2.0, kw / 2.0], [top, h], [30, 30, 34], b)]
        }
        Kind::DipSwitch => {
            // the body between the two rows of pins; a white lever for every pair of pins across
            let xs: BTreeSet<i64> = pads.iter().map(|p| (p.pos.x * 100.0).round() as i64).collect();
            let ys: BTreeSet<i64> = pads.iter().map(|p| (p.pos.y * 100.0).round() as i64).collect();
            let rows_along_y = xs.len() <= 2 && ys.len() > 1;
            let (shx, shy) = if rows_along_y { (1.4, -0.3) } else { (-0.3, 1.4) };
            let (blo, bhi) = ([lo.x + shx, lo.y + shy], [hi.x - shx, hi.y - shy]);
            let top = (h - 0.6).max(1.0);
            let mut v = vec![solid([blo[0], blo[1], 0.0], [bhi[0], bhi[1], top], color, b)];
            let (across, at) = if rows_along_y { (bhi[0] - blo[0], ys) } else { (bhi[1] - blo[1], xs) };
            let (kl, kw) = (across * 0.3, 0.9);
            for p in at {
                let p = p as f64 / 100.0;
                // the lever sits towards the ON side (pin 1's row)
                let (k0, k1) = (-kl * 0.1, kl * 0.9);
                v.push(if rows_along_y {
                    solid([cx + k0 - kl / 2.0, p - kw / 2.0, top], [cx + k1 - kl / 2.0, p + kw / 2.0, h], MARK, b)
                } else {
                    solid([p - kw / 2.0, cy + k0 - kl / 2.0, top], [p + kw / 2.0, cy + k1 - kl / 2.0, h], MARK, b)
                });
            }
            v
        }
        Kind::Other => vec![solid([lo.x - 0.1, lo.y - 0.1, 0.0], [hi.x + 0.1, hi.y + 0.1, h], color, b)],
    }
}

/// Positions along and across the long way of a part (its centre `c`), for the parts drawn along the row of their pins.
struct Frame {
    c: [f64; 2],
    along_x: bool,
}

impl Frame {
    /// A solid `a` along the part, `p` across it and `z` above the board.
    fn at(&self, a: [f64; 2], p: [f64; 2], z: [f64; 2], color: [u8; 3], shape: BodyShape) -> Solid {
        if self.along_x {
            solid([self.c[0] + a[0], self.c[1] + p[0], z[0]], [self.c[0] + a[1], self.c[1] + p[1], z[1]], color, shape)
        } else {
            solid([self.c[0] + p[0], self.c[1] + a[0], z[0]], [self.c[0] + p[1], self.c[1] + a[1], z[1]], color, shape)
        }
    }

    /// A cylinder lying along the part.
    fn axis(&self) -> BodyShape {
        BodyShape::Cylinder(if self.along_x { 0 } else { 1 })
    }

    /// Where a pad is along the part, from the centre.
    fn along(&self, p: &Pad) -> f64 {
        if self.along_x { p.pos.x - self.c[0] } else { p.pos.y - self.c[1] }
    }
}

/// The distance between the outermost pad centres along the part.
fn pad_span(pads: &[&Pad], f: &Frame) -> f64 {
    let at: Vec<f64> = pads.iter().map(|p| f.along(p)).collect();
    at.iter().cloned().fold(f64::MIN, f64::max) - at.iter().cloned().fold(f64::MAX, f64::min)
}

/// Which way the + end is along the part (pin "+", else pin 1): 1 or -1.
fn plus_end(pads: &[&Pad], f: &Frame) -> f64 {
    pad_named(pads, &["+", "1"]).map(|p| if f.along(p) < 0.0 { -1.0 } else { 1.0 }).unwrap_or(1.0)
}

const CELL: [u8; 3] = [50, 100, 170];
const HOLDER: [u8; 3] = [40, 40, 44];
const GLASS: [u8; 3] = [200, 222, 230];

/// An open tray between the two contacts with the cell lying in it: the + end has the silver cap and the little nub.
fn battery_tray(pads: &[&Pad], fpl: &str, c: [f64; 2], along_x: bool, h: f64, color: [u8; 3]) -> Vec<Solid> {
    let f = Frame { c, along_x };
    let b = BodyShape::Box;
    let span = pad_span(pads, &f);
    let floor = 1.0;
    let dia = cell_diameter(fpl).unwrap_or(h - 1.5).min(h - floor).max(3.0);
    let (len, wid) = (span + 4.0, dia + 2.4);
    let (l2, w2, wall) = (len / 2.0, wid / 2.0, 1.2);
    let mut v = vec![
        f.at([-l2, l2], [-w2, w2], [0.0, floor], color, b),
        f.at([-l2, l2], [-w2, -w2 + wall], [floor, floor + dia * 0.6], color, b),
        f.at([-l2, l2], [w2 - wall, w2], [floor, floor + dia * 0.6], color, b),
        f.at([-l2, -l2 + 1.5], [-w2, w2], [floor, h], color, b),
        f.at([l2 - 1.5, l2], [-w2, w2], [floor, h], color, b),
    ];
    // the cell between the end walls: the sleeve, the flat minus end and the + end with its nub
    // `u` runs towards the + end; the cell fills the inside of the tray (len - 3), the last 1 mm of it is the nub
    let (inside, r, zc) = ((len - 3.0) / 2.0, dia / 2.0, floor + dia / 2.0);
    let s = plus_end(pads, &f);
    let along = |u0: f64, u1: f64| [(s * u0).min(s * u1), (s * u0).max(s * u1)];
    let round = |rr: f64| ([-rr, rr], [zc - rr, zc + rr]);
    let (sleeve, cap, nub) = (round(r), round(r + 0.03), round(r * 0.35));
    v.push(f.at(along(-inside + 0.8, inside - 2.0), sleeve.0, sleeve.1, CELL, f.axis()));
    v.push(f.at(along(-inside, -inside + 0.8), cap.0, cap.1, SILVER, f.axis()));
    v.push(f.at(along(inside - 2.0, inside - 1.0), cap.0, cap.1, SILVER, f.axis()));
    v.push(f.at(along(inside - 1.0, inside), nub.0, nub.1, SILVER, f.axis()));
    v
}

/// A coin cell holder: the round black base, the coin in it and the spring clip from the + contact over the coin.
fn coin_holder(pads: &[&Pad], fpl: &str, c: [f64; 2], along_x: bool, h: f64) -> Vec<Solid> {
    let f = Frame { c, along_x };
    let dia = cell_diameter(fpl).unwrap_or(20.0);
    let (r, base_r, floor) = (dia / 2.0, dia / 2.0 + 1.25, 1.0_f64);
    let coin_top = (floor + 3.2).min(h - 0.3);
    let round = BodyShape::Cylinder(2);
    let mut v = vec![f.at([-base_r, base_r], [-base_r, base_r], [0.0, floor], HOLDER, round), f.at([-r, r], [-r, r], [floor, coin_top], SILVER, round)];
    let s = plus_end(pads, &f);
    let reach = (pad_span(pads, &f) / 2.0).max(r);
    let a = [(s * reach).min(0.0), (s * reach).max(0.0)];
    v.push(f.at(a, [-dia * 0.15, dia * 0.15], [coin_top, h.max(coin_top + 0.2)], SILVER, BodyShape::Box));
    v
}

/// Two clips standing at the contacts with the 5 x 20 mm glass fuse lying in them: silver caps at its ends, glass between.
fn fuse_holder(pads: &[&Pad], c: [f64; 2], along_x: bool, h: f64) -> Vec<Solid> {
    let f = Frame { c, along_x };
    let b = BodyShape::Box;
    let (r, half, cap) = (2.5, 10.0, 5.0);
    let zc = (h - r - 0.1).max(r);
    let z = [zc - r, zc + r];
    let at = pad_span(pads, &f) / 2.0;
    let mut v = vec![];
    for s in [-1.0, 1.0] {
        // a clip from a little inside its contact out past it, as high as the middle of the fuse
        let (a0, a1) = (s * (at - 3.0), s * (at + 1.5));
        v.push(f.at([a0.min(a1), a0.max(a1)], [-3.5, 3.5], [0.0, zc], [200, 190, 150], b));
        let (c0, c1) = (s * (half - cap), s * half);
        v.push(f.at([c0.min(c1), c0.max(c1)], [-r, r], z, SILVER, f.axis()));
    }
    v.push(f.at([-(half - cap), half - cap], [-r * 0.96, r * 0.96], [zc - r * 0.96, zc + r * 0.96], GLASS, f.axis()));
    v
}

/// The opening of a connector in a metal shell, on the side that faces the nearest board edge (the plug goes in from the
/// edge): a dark round hole for a DC jack, a dark slot for USB and the like.
fn shell_mouth(lo: [f64; 2], hi: [f64; 2], z: [f64; 2], c: [f64; 2], (blo, bhi): (Point, Point), round: bool) -> Solid {
    const HOLE: [u8; 3] = [8, 8, 10];
    let gaps = [(c[0] - blo.x, 0, -1.0), (bhi.x - c[0], 0, 1.0), (c[1] - blo.y, 1, -1.0), (bhi.y - c[1], 1, 1.0)];
    let (_, axis, dir) = gaps.iter().copied().min_by(|a, b| a.0.total_cmp(&b.0)).unwrap();
    let face = if dir < 0.0 { lo[axis] } else { hi[axis] };
    // from a little outside the face to a little inside it
    let (n0, n1) = ((face + dir * 0.06).min(face - dir * 0.3), (face + dir * 0.06).max(face - dir * 0.3));
    let other = 1 - axis;
    let (mid, width, height, zc) = ((lo[other] + hi[other]) / 2.0, hi[other] - lo[other], z[1] - z[0], (z[0] + z[1]) / 2.0);
    let (half_w, half_h) = if round { let d = 0.6 * width.min(height) / 2.0; (d, d) } else { (0.35 * width, 0.25 * height) };
    let (mut l, mut h) = ([0.0; 3], [0.0; 3]);
    l[axis] = n0;
    h[axis] = n1;
    l[other] = mid - half_w;
    h[other] = mid + half_w;
    l[2] = zc - half_h;
    h[2] = zc + half_h;
    solid(l, h, HOLE, if round { BodyShape::Cylinder(axis as u8) } else { BodyShape::Box })
}

/// The solids of every part that has pads. Bottom-side parts hang below the board.
pub fn part_boxes(board: &Board) -> Vec<PartBox> {
    let mut out = vec![];
    for (i, part) in board.parts.iter().enumerate() {
        // a part with a mesh of its own (a maker's STEP) is drawn from `part_meshes`
        if board.models3d.contains_key(&part.footprint) || crate::assembly::is_bare_copper(&part.footprint) {
            continue;
        }
        let bottom = part.side == Side::Bottom;
        for s in solids(board, part) {
            let (z0, z1) = if bottom { (-s.hi[2], -s.lo[2]) } else { (BOARD_THICKNESS + s.lo[2], BOARD_THICKNESS + s.hi[2]) };
            out.push(PartBox { refdes: part.refdes.clone(), part: i, lo: [s.lo[0], s.lo[1], z0], hi: [s.hi[0], s.hi[1], z1], color: s.color, bottom, shape: s.shape });
        }
    }
    out
}

/// The mesh of a part, placed on the board: x and y in board coordinates, z = 0 at the bottom face of the board
/// (`BOARD_THICKNESS` at the top).
#[derive(Clone, Debug, PartialEq)]
pub struct PartMesh {
    pub refdes: String,
    /// Index into `Board::parts`.
    pub part: usize,
    pub bottom: bool,
    pub mesh: crate::mesh::Mesh3d,
}

/// The parts that have a mesh (`Board::models3d`), moved to where the part stands: turned with it, mirrored when it
/// is on the bottom (mirrored left to right, hanging below the board).
pub fn part_meshes(board: &Board) -> Vec<PartMesh> {
    let mut out = vec![];
    for (i, part) in board.parts.iter().enumerate() {
        let Some(m) = board.models3d.get(&part.footprint) else { continue };
        let bottom = part.side == Side::Bottom;
        let mut mesh = m.clone();
        for v in &mut mesh.vertices {
            let local = if bottom { Point::new(-f64::from(v[0]), f64::from(v[1])) } else { Point::new(f64::from(v[0]), f64::from(v[1])) };
            let r = crate::parts::rotate(local, part.rot);
            let z = f64::from(v[2]);
            *v = [(part.pos.x + r.x) as f32, (part.pos.y + r.y) as f32, if bottom { -z } else { BOARD_THICKNESS + z } as f32];
        }
        out.push(PartMesh { refdes: part.refdes.clone(), part: i, bottom, mesh });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A part with a mesh is drawn from the mesh: turned with the part, mirrored on the bottom, and no guessed box.
    #[test]
    fn a_part_with_a_mesh_is_placed_with_the_part() {
        use crate::mesh::Mesh3d;
        let mut b = Board::new("t", 30.0, 30.0);
        let mut fp = crate::footprints::get("R_0805").unwrap();
        fp.name = "WITH_MESH".into();
        b.add_footprint(fp);
        // one triangle: 2 mm along x, 1 mm along y, 3 mm up
        b.models3d.insert("WITH_MESH".into(), Mesh3d { vertices: vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 1.0, 3.0]], triangles: vec![[0, 1, 2]], palette: vec![[9, 9, 9]], tri_color: vec![0] });
        b.set_part(Part { refdes: "U1".into(), value: "x".into(), footprint: "WITH_MESH".into(), pos: Point::new(10.0, 10.0), rot: 1, side: Side::Top }).unwrap();
        assert!(part_boxes(&b).is_empty(), "no guessed box for a part with a mesh");
        let m = &part_meshes(&b)[0];
        // a quarter turn counter-clockwise: (2, 0) -> (0, 2), (0, 1) -> (-1, 0)
        assert_eq!(m.mesh.vertices[1], [10.0, 12.0, BOARD_THICKNESS as f32 + 0.0]);
        assert_eq!(m.mesh.vertices[2], [9.0, 10.0, BOARD_THICKNESS as f32 + 3.0]);
        // on the bottom the part hangs below the board and is mirrored before it is turned
        b.set_part(Part { refdes: "U1".into(), value: "x".into(), footprint: "WITH_MESH".into(), pos: Point::new(10.0, 10.0), rot: 0, side: Side::Bottom }).unwrap();
        let m = &part_meshes(&b)[0];
        assert!(m.bottom);
        assert_eq!(m.mesh.vertices[1], [8.0, 10.0, 0.0]);
        assert_eq!(m.mesh.vertices[2], [10.0, 11.0, -3.0]);
    }

    /// A part whose footprint carries the data sheet body is drawn as that body: its size, its height,
    /// turned with the part, with the pin 1 dot on top.
    #[test]
    fn a_data_sheet_body_sets_the_size_and_height_of_the_3d_part() {
        use crate::body::{BodyMark, BodySpec};
        let mut b = Board::new("t", 30.0, 30.0);
        let mut fp = crate::footprints::get("R_0805").unwrap();
        fp.name = "BODY".into();
        fp.height = Some(2.0);
        fp.body = Some(BodySpec { shape: "rect".into(), w: 4.0, h: 3.0, x: 0.0, y: 0.0, mark: BodyMark::Dot, mark_pin: "1".into(), color_code: None });
        b.add_footprint(fp);
        b.set_part(Part { refdes: "U1".into(), value: "x".into(), footprint: "BODY".into(), pos: Point::new(10.0, 10.0), rot: 0, side: Side::Top }).unwrap();
        let boxes = part_boxes(&b);
        let body = &boxes[0];
        assert!((body.hi[0] - body.lo[0] - 4.0).abs() < 1e-9 && (body.hi[1] - body.lo[1] - 3.0).abs() < 1e-9);
        assert!((body.hi[2] - body.lo[2] - 2.0).abs() < 1e-9, "the height of the data sheet");
        assert_eq!(boxes.len(), 2, "the body and the pin 1 dot");
        // turned a quarter: width and length swap
        b.set_part(Part { refdes: "U1".into(), value: "x".into(), footprint: "BODY".into(), pos: Point::new(10.0, 10.0), rot: 1, side: Side::Top }).unwrap();
        let turned = &part_boxes(&b)[0];
        assert!((turned.hi[0] - turned.lo[0] - 3.0).abs() < 1e-9 && (turned.hi[1] - turned.lo[1] - 4.0).abs() < 1e-9);
        // a circle is a cylinder along z
        let mut fp = crate::footprints::get("R_0805").unwrap();
        fp.name = "ROUND".into();
        fp.body = Some(BodySpec { shape: "circle".into(), w: 5.0, h: 0.0, x: 0.0, y: 0.0, mark: BodyMark::None, mark_pin: String::new(), color_code: None });
        b.add_footprint(fp);
        b.set_part(Part { refdes: "U2".into(), value: "x".into(), footprint: "ROUND".into(), pos: Point::new(20.0, 20.0), rot: 0, side: Side::Top }).unwrap();
        let round = part_boxes(&b).into_iter().find(|p| p.refdes == "U2").unwrap();
        assert_eq!(round.shape, BodyShape::Cylinder(2));
        assert!((round.hi[0] - round.lo[0] - 5.0).abs() < 1e-9);
    }

    fn pad(refdes: &str, x: f64, y: f64, w: f64, h: f64, drill: Option<f64>) -> Pad {
        let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w, h }, drill, Side::Top);
        p.refdes = refdes.into();
        p
    }

    fn part(refdes: &str, fp: &str, side: Side) -> Part {
        Part { refdes: refdes.into(), value: "v".into(), footprint: fp.into(), pos: Point::default(), rot: 0, side }
    }

    fn pin_pad(refdes: &str, pin: &str, x: f64, y: f64, w: f64, h: f64, drill: Option<f64>) -> Pad {
        let mut p = pad(refdes, x, y, w, h, drill);
        p.pin = pin.into();
        p
    }

    fn one(b: &Board, refdes: &str) -> Vec<PartBox> {
        part_boxes(b).into_iter().filter(|x| x.refdes == refdes).collect()
    }

    #[test]
    fn an_ic_has_a_dot_next_to_pin_1() {
        let mut b = Board::new("t", 40.0, 25.0);
        // SOIC-like: pin 1 top left (smaller x, bigger y), pin 8 bottom right
        for k in 0..4 {
            b.pads.push(pin_pad("U1", &format!("{}", k + 1), 10.0 - 2.7, 10.0 + 1.9 - 1.27 * k as f64, 1.5, 0.6, None));
            b.pads.push(pin_pad("U1", &format!("{}", 8 - k), 10.0 + 2.7, 10.0 + 1.9 - 1.27 * k as f64, 1.5, 0.6, None));
        }
        b.parts.push(part("U1", "SOIC-8_P1.27", Side::Top));
        let boxes = one(&b, "U1");
        let dot = boxes.iter().find(|x| x.color == MARK).expect("a pin 1 dot");
        assert_eq!(dot.shape, BodyShape::Cylinder(2));
        let body = boxes.iter().filter(|x| x.color != SILVER && x.color != MARK).max_by(|a, c| a.hi[2].total_cmp(&c.hi[2])).unwrap();
        assert!(dot.lo[2] >= body.hi[2] - 1e-9, "on top of the body");
        let (dx, dy) = ((dot.lo[0] + dot.hi[0]) / 2.0, (dot.lo[1] + dot.hi[1]) / 2.0);
        assert!(dx < 10.0 && dy > 10.0, "top left, next to pad 1: {dx} {dy}");
        // a DIP has one too, and a part without a pin called 1 (a transistor) has none
        let mut d = Board::new("t", 40.0, 25.0);
        for k in 0..2 {
            d.pads.push(pin_pad("U2", &format!("{}", k + 1), 5.0, 5.0 - 2.54 * k as f64, 1.6, 1.6, Some(0.8)));
            d.pads.push(pin_pad("U2", &format!("{}", 8 - k), 12.6, 5.0 - 2.54 * k as f64, 1.6, 1.6, Some(0.8)));
        }
        d.parts.push(part("U2", "DIP-8_W7.62", Side::Top));
        assert!(one(&d, "U2").iter().any(|x| x.color == MARK));
        let mut q = Board::new("t", 40.0, 25.0);
        for (pin, x) in [("B", 0.0), ("C", 1.0), ("E", 2.0)] {
            q.pads.push(pin_pad("Q1", pin, 10.0 + x, 10.0, 0.9, 0.8, None));
        }
        q.parts.push(part("Q1", "SOT-23", Side::Top));
        assert!(!one(&q, "Q1").iter().any(|x| x.color == MARK));
    }

    #[test]
    fn a_diode_shows_its_cathode_end_and_an_led_its_flat() {
        let mut b = Board::new("t", 60.0, 25.0);
        // SMD diode: K on the left
        b.pads.push(pin_pad("D1", "K", 10.0 - 1.65, 10.0, 0.9, 1.2, None));
        b.pads.push(pin_pad("D1", "A", 10.0 + 1.65, 10.0, 0.9, 1.2, None));
        b.parts.push(part("D1", "D_SOD-123", Side::Top));
        let cap = |r: &str| one(&b, r).into_iter().filter(|x| x.color == MARK).collect::<Vec<_>>();
        let c = cap("D1");
        assert_eq!(c.len(), 1);
        assert!((c[0].lo[0] + c[0].hi[0]) / 2.0 < 10.0, "the white end is the K side");
        // the same diode with A on the left: the mark follows the cathode
        let mut b2 = Board::new("t", 60.0, 25.0);
        b2.pads.push(pin_pad("D1", "A", 10.0 - 1.65, 10.0, 0.9, 1.2, None));
        b2.pads.push(pin_pad("D1", "K", 10.0 + 1.65, 10.0, 0.9, 1.2, None));
        b2.parts.push(part("D1", "D_SOD-123", Side::Top));
        let m = one(&b2, "D1").into_iter().find(|x| x.color == MARK).unwrap();
        assert!((m.lo[0] + m.hi[0]) / 2.0 > 10.0);
        // axial diode: a band on the K side, not on a resistor
        let mut a = Board::new("t", 60.0, 25.0);
        a.pads.push(pin_pad("D2", "K", 20.0, 10.0, 1.8, 1.8, Some(0.9)));
        a.pads.push(pin_pad("D2", "A", 27.62, 10.0, 1.8, 1.8, Some(0.9)));
        a.pads.push(pin_pad("R1", "1", 20.0, 18.0, 1.8, 1.8, Some(0.9)));
        a.pads.push(pin_pad("R1", "2", 30.16, 18.0, 1.8, 1.8, Some(0.9)));
        a.parts.push(part("D2", "D_DO-35_7.62", Side::Top));
        a.parts.push(part("R1", "R_Axial_10.16", Side::Top));
        let band = one(&a, "D2").into_iter().find(|x| x.color == MARK).expect("band");
        assert_eq!(band.shape, BodyShape::Cylinder(0));
        assert!((band.lo[0] + band.hi[0]) / 2.0 < 23.81, "cathode side");
        assert!(!one(&a, "R1").iter().any(|x| x.color == MARK));
        // LED 5 mm: a pale patch on the rim at the K side (below here, K is at smaller y)
        let mut l = Board::new("t", 60.0, 25.0);
        l.pads.push(pin_pad("D3", "K", 30.0, 10.0 - 1.27, 1.8, 1.8, Some(0.9)));
        l.pads.push(pin_pad("D3", "A", 30.0, 10.0 + 1.27, 1.8, 1.8, Some(0.9)));
        l.parts.push(part("D3", "LED_5mm", Side::Top));
        let flat = one(&l, "D3").into_iter().find(|x| x.color == MARK).expect("flat");
        assert!((flat.lo[1] + flat.hi[1]) / 2.0 < 10.0 && flat.shape == BodyShape::Box);
        // an SMD LED has a green cathode end
        let mut s = Board::new("t", 60.0, 25.0);
        s.pads.push(pin_pad("D4", "K", 8.0, 10.0, 1.0, 1.4, None));
        s.pads.push(pin_pad("D4", "A", 10.0, 10.0, 1.0, 1.4, None));
        s.parts.push(part("D4", "LED_0805", Side::Top));
        assert!(one(&s, "D4").iter().any(|x| x.color == CATHODE_LED));
    }

    #[test]
    fn an_electrolytic_has_a_stripe_on_the_minus_side_and_a_ceramic_disc_has_none() {
        let mut b = Board::new("t", 60.0, 25.0);
        b.pads.push(pin_pad("C1", "1", 10.0 - 1.0, 10.0, 1.6, 1.6, Some(0.8)));
        b.pads.push(pin_pad("C1", "2", 10.0 + 1.0, 10.0, 1.6, 1.6, Some(0.8)));
        b.pads.push(pin_pad("C2", "1", 30.0 - 2.54, 10.0, 1.6, 1.6, Some(0.8)));
        b.pads.push(pin_pad("C2", "2", 30.0 + 2.54, 10.0, 1.6, 1.6, Some(0.8)));
        b.parts.push(part("C1", "CP_Radial_D5_P2.0", Side::Top));
        b.parts.push(part("C2", "C_Disc_5.08", Side::Top));
        let stripe = one(&b, "C1").into_iter().find(|x| x.color == MARK).expect("stripe");
        assert!((stripe.lo[0] + stripe.hi[0]) / 2.0 > 10.0, "pin 2 (the minus lead) is on the right");
        assert!(!one(&b, "C2").iter().any(|x| x.color == MARK));
    }

    #[test]
    fn the_height_given_for_a_footprint_replaces_the_guess() {
        let mut b = Board::new("t", 40.0, 25.0);
        let mut fp = crate::footprints::get("C_Disc_5.08").unwrap();
        fp.name = "C_Big".into();
        fp.height = Some(11.0);
        b.add_footprint(fp);
        for p in crate::footprints::get("C_Disc_5.08").unwrap().pads {
            b.pads.push(pin_pad("C1", &p.pin, 10.0 + p.pos.x, 10.0 + p.pos.y, 1.6, 1.6, Some(0.8)));
            b.pads.push(pin_pad("C2", &p.pin, 25.0 + p.pos.x, 10.0 + p.pos.y, 1.6, 1.6, Some(0.8)));
        }
        b.parts.push(part("C1", "C_Big", Side::Top));
        b.parts.push(part("C2", "C_Disc_5.08", Side::Top));
        assert_eq!(part_height_on(&b, &b.parts[0]), 11.0);
        assert_eq!(part_height_on(&b, &b.parts[1]), part_total_height("C2", "C_Disc_5.08"));
        let top = |r: &str| one(&b, r).iter().map(|x| x.hi[2]).fold(0.0, f64::max);
        assert!((top("C1") - (BOARD_THICKNESS + 11.0)).abs() < 1e-9 && top("C2") < BOARD_THICKNESS + 6.0);
        // the height is saved with the footprint, and an old footprint without it still reads
        let json = serde_json::to_string(&b.footprints[0]).unwrap();
        assert!(json.contains("\"height\":11"));
        let old: crate::footprints::Footprint = serde_json::from_str(r#"{"name":"X","pads":[]}"#).unwrap();
        assert_eq!(old.height, None);
    }

    #[test]
    fn heights_follow_the_kind_of_part() {
        let h = |r: &str, f: &str| part_style(r, f).0;
        assert!(h("R1", "R_0805") < 1.0 && h("R1", "R_Axial_10.16") > 2.0);
        assert!(h("U1", "DIP-14_W7.62") > h("U2", "SOIC-8_P1.27"));
        assert!(h("D1", "LED_5mm") > 8.0 && h("D2", "LED_0805") < 1.0);
        assert!(part_total_height("J1", "PinHeader_1x4") > 8.0, "the pins of a header stick out of the plastic");
        assert_eq!(part_style("Z9", "").0, 2.0, "an unknown kind still gets a body");
    }

    #[test]
    fn kinds_are_told_apart() {
        assert_eq!(part_kind("R1", "R_0805"), Kind::Chip);
        assert_eq!(part_kind("R1", "R_Axial_10.16"), Kind::Axial);
        assert_eq!(part_kind("C1", "C_Disc_5.08"), Kind::Can);
        assert_eq!(part_kind("D1", "LED_5mm"), Kind::Led);
        assert_eq!(part_kind("D2", "D_DO-35_7.62"), Kind::Axial);
        assert_eq!(part_kind("J1", "PinHeader_1x4"), Kind::Header);
        assert_eq!(part_kind("V1", "PinHeader_1x2_PM"), Kind::Header, "a header used as a source symbol is still a header");
        assert_eq!(part_style("V1", "PinHeader_1x2_PM").0, 2.5);
        assert_eq!(part_kind("U1", "SOIC-8_P1.27"), Kind::SmdIc);
        assert_eq!(part_kind("U2", "DIP-14_W7.62"), Kind::Dip);
        assert_eq!(part_kind("T1", "Transformer_EI30"), Kind::Transformer);
    }

    #[test]
    fn a_chip_has_a_body_and_two_silver_caps_and_bottom_parts_hang_below() {
        let mut b = Board::new("t", 40.0, 25.0);
        for (x, r) in [(10.0, "R1"), (10.0, "R2")] {
            let y = if r == "R1" { 10.0 } else { 20.0 };
            b.pads.push(pad(r, x - 0.9, y, 1.0, 1.2, None));
            b.pads.push(pad(r, x + 0.9, y, 1.0, 1.2, None));
        }
        b.parts.push(part("R1", "R_0805", Side::Top));
        b.parts.push(part("R2", "R_0805", Side::Bottom));
        b.parts.push(part("X9", "", Side::Top));
        let boxes = part_boxes(&b);
        assert_eq!(boxes.len(), 6, "body + 2 caps for each chip; a part without pads has nothing");
        let top: Vec<&PartBox> = boxes.iter().filter(|x| !x.bottom).collect();
        assert!(top.iter().all(|x| x.lo[2] == BOARD_THICKNESS && x.hi[2] > BOARD_THICKNESS && x.part == 0));
        assert_eq!(top.iter().filter(|x| x.color == SILVER).count(), 2);
        assert!(top.iter().map(|x| x.lo[0]).fold(f64::MAX, f64::min) < 9.2 && top.iter().map(|x| x.hi[0]).fold(f64::MIN, f64::max) > 10.8);
        assert!(boxes.iter().filter(|x| x.bottom).all(|x| x.hi[2] == 0.0 && x.lo[2] < 0.0 && x.part == 1));
    }

    #[test]
    fn a_chip_leaves_the_outer_part_of_its_pads_uncovered() {
        // R_0603: pad centres 1.65 mm apart, pads 0.8 mm long: the pads reach 1.225 mm from the middle
        let mut b = Board::new("t", 20.0, 20.0);
        let fp = crate::footprints::get("R_0603").unwrap();
        for p in &fp.pads {
            b.pads.push(pad("R1", 10.0 + p.pos.x, 10.0 + p.pos.y, 0.8, 0.95, None));
        }
        b.parts.push(part("R1", "R_0603", Side::Top));
        let boxes = part_boxes(&b);
        let reach = boxes.iter().map(|x| (x.hi[0] - 10.0).max(10.0 - x.lo[0])).fold(0.0, f64::max);
        assert!(reach < 0.95 && reach > 0.8, "the whole chip is {reach} mm from the middle at most, about the real 1.6 mm long");
    }

    #[test]
    fn headers_get_pins_ics_get_legs_and_axial_parts_a_round_body() {
        let mut b = Board::new("t", 40.0, 25.0);
        for k in 0..4 {
            b.pads.push(pad("J1", 5.0 + 2.54 * k as f64, 5.0, 1.7, 1.7, Some(1.0)));
        }
        for k in 0..4 {
            b.pads.push(pad("U1", 20.0 - 2.7, 10.0 + 1.27 * k as f64, 1.55, 0.6, None));
            b.pads.push(pad("U1", 20.0 + 2.7, 10.0 + 1.27 * k as f64, 1.55, 0.6, None));
        }
        b.pads.push(pad("R5", 30.0, 20.0, 1.8, 1.8, Some(0.9)));
        b.pads.push(pad("R5", 40.16 - 2.0, 20.0, 1.8, 1.8, Some(0.9)));
        b.parts.push(part("J1", "PinHeader_1x4", Side::Top));
        b.parts.push(part("U1", "SOIC-8_P1.27", Side::Top));
        b.parts.push(part("R5", "R_Axial_10.16", Side::Top));
        let boxes = part_boxes(&b);
        let of = |p: usize| boxes.iter().filter(|x| x.part == p).collect::<Vec<_>>();
        assert_eq!(of(0).len(), 5, "plastic + 4 pins");
        assert!(of(0).iter().any(|x| x.color == GOLD && x.hi[2] - BOARD_THICKNESS > 8.0));
        assert_eq!(of(1).len(), 9, "8 legs + body");
        assert!(of(1).iter().any(|x| x.color != SILVER && x.lo[2] > BOARD_THICKNESS), "the body sits on its legs");
        assert!(of(2).iter().any(|x| matches!(x.shape, BodyShape::Cylinder(0))), "a resistor lying along x");
    }

    #[test]
    fn a_to_220_stands_with_its_tab_and_a_terminal_block_is_tall() {
        let mut b = Board::new("x", 30.0, 20.0);
        for (i, x) in [10.0, 12.54, 15.08].iter().enumerate() {
            let mut p = Pad::new(Point::new(*x, 10.0), PadShape::Oval { w: 1.8, h: 1.8 }, Some(1.0), Side::Top);
            p.refdes = "Q1".into();
            p.pin = format!("{}", i + 1);
            b.pads.push(p);
        }
        b.parts.push(Part { refdes: "Q1".into(), value: String::new(), footprint: "TO-220-3".into(), pos: Point::new(12.54, 10.0), rot: 0, side: Side::Top });
        assert_eq!(part_kind("Q1", "TO-220-3"), Kind::To220);
        let total = part_height_on(&b, &b.parts[0]);
        assert!((total - 15.8).abs() < 1e-9, "body 9.3 + tab 6.5: {total}");
        let boxes = part_boxes(&b);
        let top = boxes.iter().filter(|x| x.refdes == "Q1").map(|x| x.hi[2]).fold(0.0, f64::max);
        assert!((top - (BOARD_THICKNESS + total)).abs() < 1e-9, "the tab reaches the total height");
        assert_eq!(boxes.len(), 2 + 3, "body, tab and three legs");
        // the case check sees it: 12 mm of room is not enough
        let r = crate::case::check(&b, &crate::case::CaseSpec { top: 12.0, ..Default::default() });
        assert_eq!(r.too_tall.len(), 1);
        // a terminal block is 10 mm tall, a footprint's own height still wins
        assert_eq!(part_style("J1", "TerminalBlock_4P_5.08").0, 10.0);
    }

    #[test]
    fn a_to_92_is_a_body_on_three_legs_and_a_smd_crystal_is_low() {
        let mut b = Board::new("x", 30.0, 20.0);
        for (i, x) in [10.0, 12.54, 15.08].iter().enumerate() {
            let mut p = Pad::new(Point::new(*x, 10.0), PadShape::Oval { w: 1.4, h: 1.4 }, Some(0.8), Side::Top);
            p.refdes = "Q1".into();
            p.pin = format!("{}", i + 1);
            b.pads.push(p);
        }
        b.parts.push(Part { refdes: "Q1".into(), value: String::new(), footprint: "TO-92_inline".into(), pos: Point::new(12.54, 10.0), rot: 0, side: Side::Top });
        assert_eq!(part_kind("Q1", "TO-92_inline"), Kind::To92);
        let boxes = part_boxes(&b);
        assert_eq!(boxes.len(), 1 + 3, "the body and three legs");
        let body = boxes.iter().max_by(|a, c| (a.hi[0] - a.lo[0]).total_cmp(&(c.hi[0] - c.lo[0]))).unwrap();
        assert!(((body.hi[0] - body.lo[0]) - 4.8).abs() < 1e-9 && ((body.hi[1] - body.lo[1]) - 3.8).abs() < 1e-9, "the package's size, not the pads'");
        assert_eq!(part_style("Y1", "XTAL_3225").0, 0.9);
    }

    fn board_with(refdes: &str, fp: &str, pads: &[(f64, f64)]) -> Board {
        let mut b = Board::new("x", 60.0, 40.0);
        for (i, (x, y)) in pads.iter().enumerate() {
            let mut p = Pad::new(Point::new(*x, *y), PadShape::Circle { d: 1.7 }, Some(1.0), Side::Top);
            p.refdes = refdes.into();
            p.pin = format!("{}", i + 1);
            b.pads.push(p);
        }
        b.parts.push(Part { refdes: refdes.into(), value: String::new(), footprint: fp.into(), pos: Point::new(20.0, 20.0), rot: 0, side: Side::Top });
        b
    }

    #[test]
    fn a_usb_connector_is_a_metal_shell_on_a_base_a_terminal_block_has_screws() {
        // the shapes of the shipped sample: the signal pins in a row, the shell legs to one side
        assert_eq!(part_kind("J1", "USB_DEMO_TH"), Kind::Shell);
        assert_eq!(part_kind("J1", "PinHeader_1x4"), Kind::Header, "a header is still a header");
        assert_eq!(part_kind("U1", "USBLC6-2SC6"), Kind::SmdIc, "a USB protection chip is not a connector");
        let b = board_with("J1", "USB_DEMO_TH", &[(16.5, 20.0), (19.0, 20.0), (21.0, 20.0), (23.5, 20.0), (13.5, 16.0), (26.5, 16.0)]);
        let boxes = part_boxes(&b);
        assert_eq!(boxes.len(), 3, "the plastic base, the shell and its opening (the pins show as pads)");
        let shell = boxes.iter().find(|x| x.color == [190, 190, 200]).expect("a silver shell");
        assert!((shell.hi[2] - (BOARD_THICKNESS + 7.0)).abs() < 1e-9, "a USB-A is about 7 mm tall");
        // the shell is nearer the bottom edge (y = 0) than the others: the slot is on the low-y face
        let mouth = boxes.iter().find(|x| x.color == [8, 8, 10]).expect("the opening");
        assert_eq!(mouth.shape, BodyShape::Box, "a USB opening is a slot");
        assert!(mouth.lo[1] < shell.lo[1] && mouth.hi[1] > shell.lo[1] && mouth.hi[1] - mouth.lo[1] < 0.5, "on the face towards the edge: {mouth:?}");
        assert_eq!(part_style("J1", "USB_C_Receptacle").0, 3.3);
        assert_eq!(part_style("J1", "USB_Micro-B").0, 2.8);

        // four terminals, one screw (a cylinder and its slot) on each
        let t = board_with("V1", "TerminalBlock_4P_5.08", &[(10.0, 20.0), (15.08, 20.0), (20.16, 20.0), (25.24, 20.0)]);
        assert_eq!(part_kind("V1", "TerminalBlock_4P_5.08"), Kind::Terminal);
        let boxes = part_boxes(&t);
        assert_eq!(boxes.iter().filter(|x| x.shape == BodyShape::Cylinder(2)).count(), 4, "a screw head per terminal");
        assert_eq!(boxes.len(), 1 + 4 * 2, "the block, and a screw with its slot on each terminal");
    }

    /// The opening of a DC jack is a round hole, on the face towards whichever board edge is nearest.
    #[test]
    fn a_jack_opens_towards_the_nearest_board_edge() {
        for (x, face_x) in [(55.0, true), (5.0, false)] {
            let b = board_with("J1", "BarrelJack_Horizontal", &[(x - 3.0, 20.0), (x + 3.0, 20.0), (x, 24.7)]);
            let boxes = part_boxes(&b);
            let shell = boxes.iter().max_by(|a, c| a.hi[2].total_cmp(&c.hi[2])).unwrap();
            let mouth = boxes.iter().find(|x| x.color == [8, 8, 10]).expect("the opening");
            assert_eq!(mouth.shape, BodyShape::Cylinder(0), "a round hole along x");
            let face = if face_x { shell.hi[0] } else { shell.lo[0] };
            assert!(mouth.lo[0] < face && face < mouth.hi[0], "the hole is on the face towards the edge at x = {}: {mouth:?}", if face_x { 60 } else { 0 });
        }
    }

    /// Holders: the cell lies in its tray with the nub at the + end; the coin sits in a round base under its clip; the
    /// glass fuse lies in two clips at the height of their tops.
    #[test]
    fn holders_show_the_cell_coin_or_fuse_in_them() {
        assert_eq!(part_kind("BT1", "BatteryHolder_AA"), Kind::BatteryTray);
        assert_eq!(part_kind("BT1", "BatteryHolder_Keystone_3034_1x20mm"), Kind::CoinHolder);
        assert_eq!(part_kind("F1", "Fuse_5x20_P22.5"), Kind::FuseHolder);
        assert_eq!(part_style("BT1", "BatteryHolder_Keystone_3034_1x20mm").0, 5.0, "a coin holder is low whatever it is called");
        assert_eq!(part_style("BT1", "BatteryHolder_AAA").0, 12.5, "AAA is not AA");

        // AA: pads 50 mm apart, pin 1 (+) on the right
        let b = board_with("BT1", "BatteryHolder_AA", &[(55.0, 20.0), (5.0, 20.0)]);
        let boxes = part_boxes(&b);
        let cell = boxes.iter().find(|x| x.color == CELL).expect("the cell");
        assert_eq!(cell.shape, BodyShape::Cylinder(0), "lying along the pads");
        assert!(((cell.hi[2] - cell.lo[2]) - 14.5).abs() < 1e-9, "an AA is 14.5 mm across");
        assert!(boxes.iter().all(|x| x.hi[2] <= BOARD_THICKNESS + 16.0 + 1e-9), "nothing above the holder's height");
        let nub = boxes.iter().filter(|x| x.color == SILVER).min_by(|a, c| (a.hi[1] - a.lo[1]).total_cmp(&(c.hi[1] - c.lo[1]))).unwrap();
        assert!(nub.lo[0] > 30.0, "the nub is at the + end (pin 1, right): {nub:?}");

        let c = board_with("BT2", "BatteryHolder_CR2032", &[(10.0, 20.0), (30.0, 20.0)]);
        let boxes = part_boxes(&c);
        let coin = boxes.iter().find(|x| x.color == SILVER && x.shape == BodyShape::Cylinder(2)).expect("the coin");
        assert!(((coin.hi[0] - coin.lo[0]) - 20.0).abs() < 1e-9 && ((coin.hi[2] - coin.lo[2]) - 3.2).abs() < 1e-9, "20 mm across, 3.2 mm thick");
        assert_eq!(boxes.len(), 3, "base, coin and clip");

        let f = board_with("F1", "Fuse_5x20_P22.5", &[(8.75, 20.0), (31.25, 20.0)]);
        let boxes = part_boxes(&f);
        let glass = boxes.iter().find(|x| x.color == GLASS).expect("the glass");
        assert!(((glass.hi[0] - glass.lo[0]) - 10.0).abs() < 1e-9, "the glass between the 5 mm caps of a 20 mm fuse");
        let clip_top = boxes.iter().filter(|x| x.shape == BodyShape::Box).map(|x| x.hi[2]).fold(0.0, f64::max);
        assert!((clip_top - (glass.lo[2] + glass.hi[2]) / 2.0).abs() < 1e-9, "the clips reach the middle of the fuse");
    }

    /// A crystal is the size of its can, not of its two legs; a relay is a box with a label; a tactile switch has its
    /// button (as high as the name says); a slide switch its knob; a DIP switch a lever for every switch.
    #[test]
    fn crystals_relays_and_switches_have_their_shapes() {
        assert_eq!(part_kind("Y1", "Crystal_HC49-4H_Vertical"), Kind::Crystal);
        assert_eq!(part_style("Y1", "Crystal_HC49-4H_Vertical").0, 3.5);
        assert_eq!(part_style("Y1", "Crystal_HC49-U_Vertical").0, 13.5);
        let y = board_with("Y1", "Crystal_HC49-U_Vertical", &[(17.56, 20.0), (22.44, 20.0)]);
        let boxes = part_boxes(&y);
        let x0 = boxes.iter().map(|b| b.lo[0]).fold(f64::MAX, f64::min);
        let x1 = boxes.iter().map(|b| b.hi[0]).fold(f64::MIN, f64::max);
        assert!(((x1 - x0) - 11.0).abs() < 1e-9, "the can is 11 mm long");
        assert_eq!(boxes.iter().filter(|b| b.shape == BodyShape::Cylinder(2)).count(), 2, "round ends");

        assert_eq!(part_kind("K1", "Relay_SPDT_SANYOU_SRD_Series_Form_C"), Kind::Relay);
        assert_eq!(part_style("K1", "Relay_SPDT_SANYOU_SRD_Series_Form_C"), (15.3, [40, 80, 170]), "the blue SRD");
        let k = board_with("K1", "Relay_SPDT_SANYOU_SRD_Series_Form_C", &[(10.0, 15.0), (22.0, 15.0), (10.0, 27.0), (22.0, 27.0), (12.0, 21.0)]);
        let boxes = part_boxes(&k);
        assert_eq!(boxes.len(), 2, "the box and its label");
        assert!(boxes.iter().any(|b| b.color == MARK && b.lo[2] >= BOARD_THICKNESS + 15.3 - 1e-9), "the label on top");

        assert_eq!(part_kind("SW1", "SW_PUSH_6mm_H5mm"), Kind::Tact);
        assert_eq!(part_style("SW1", "SW_PUSH_6mm_H5mm").0, 5.0, "the height in the name");
        assert_eq!(part_style("SW1", "SW_PUSH_6mm").0, 4.3);
        let t = board_with("SW1", "SW_PUSH_6mm", &[(16.75, 17.75), (23.25, 17.75), (16.75, 22.25), (23.25, 22.25)]);
        let boxes = part_boxes(&t);
        let body = boxes.iter().find(|b| b.color == [30, 30, 34] && b.shape == BodyShape::Box).unwrap();
        assert!(((body.hi[0] - body.lo[0]) - 6.0).abs() < 1e-9, "6 x 6 mm");
        assert!(boxes.iter().any(|b| b.shape == BodyShape::Cylinder(2) && (b.hi[2] - (BOARD_THICKNESS + 4.3)).abs() < 1e-9), "the button on top");

        let s = board_with("SW2", "SW_Slide_1P2T_CK_OS102011MS2Q", &[(18.0, 20.0), (20.0, 20.0), (22.0, 20.0)]);
        assert_eq!(part_kind("SW2", "SW_Slide_1P2T_CK_OS102011MS2Q"), Kind::Slide);
        assert_eq!(part_boxes(&s).len(), 2, "the body and the knob");

        // a 4-way DIP switch: two rows of four pins 7.62 mm apart, 2.54 mm pitch (its name also says "Slide")
        let fp = "SW_DIP_SPSTx04_Slide_9.78x12.34mm_W7.62mm_P2.54mm";
        assert_eq!(part_kind("SW3", fp), Kind::DipSwitch);
        let pins: Vec<(f64, f64)> = (0..4).flat_map(|i| [(16.19, 16.19 + 2.54 * i as f64), (23.81, 16.19 + 2.54 * i as f64)]).collect();
        let d = board_with("SW3", fp, &pins);
        let boxes = part_boxes(&d);
        assert_eq!(boxes.iter().filter(|b| b.color == MARK).count(), 4, "a lever for each switch");
    }

    #[test]
    fn a_transformer_shows_core_and_coil_and_a_buzzer_is_a_can_with_a_hole() {
        let t = board_with("T1", "Transformer_EI30", &[(10.0, 10.0), (10.0, 30.0), (30.0, 10.0), (30.0, 30.0)]);
        assert_eq!(part_kind("T1", "Transformer_EI30"), Kind::Transformer);
        let boxes = part_boxes(&t);
        let body: Vec<&PartBox> = boxes.iter().filter(|x| x.hi[2] - BOARD_THICKNESS > 5.0).collect();
        assert_eq!(body.len(), 2, "core and coil");
        let heights: Vec<f64> = body.iter().map(|x| x.hi[2] - BOARD_THICKNESS).collect();
        assert!(heights.iter().any(|h| (h - 25.0).abs() < 1e-9) && heights.iter().any(|h| (h - 17.5).abs() < 1e-9), "{heights:?}");

        let z = board_with("BZ1", "Buzzer_D12_P7.6", &[(16.2, 20.0), (23.8, 20.0)]);
        assert_eq!(part_kind("BZ1", "Buzzer_D12_P7.6"), Kind::Buzzer);
        let boxes = part_boxes(&z);
        let can = boxes.iter().find(|x| x.hi[2] - BOARD_THICKNESS > 8.0).expect("the can");
        assert_eq!(can.shape, BodyShape::Cylinder(2));
        assert!(((can.hi[0] - can.lo[0]) - 12.0).abs() < 1e-9, "the 12 mm the name says");
        assert_eq!(boxes.iter().filter(|x| x.color == [8, 8, 10]).count(), 1, "the sound hole");
        // the height counts in the case check (9 mm)
        assert_eq!(part_total_height("BZ1", "Buzzer_D12_P7.6"), 9.0);
    }

    #[test]
    fn a_banded_resistor_shows_its_value_from_pin_1() {
        use crate::body::{BodySpec, ColorCode};
        let mut b = Board::new("t", 30.0, 20.0);
        let mut fp = crate::footprints::get("R_0805").unwrap();
        fp.name = "R_Axial_Test".into();
        fp.height = Some(2.6);
        fp.pads = [("1", -5.08), ("2", 5.08)]
            .into_iter()
            .map(|(pin, x)| crate::footprints::FpPad { pin: pin.into(), pos: Point::new(x, 0.0), shape: PadShape::Circle { d: 1.7 }, drill: Some(0.9), slot: 0.0, slot_v: false })
            .collect();
        fp.body = Some(BodySpec { shape: "rect".into(), w: 6.8, h: 2.6, x: 0.0, y: 0.0, mark: crate::body::BodyMark::None, mark_pin: String::new(), color_code: Some(ColorCode { bands: 4, tolerance_pct: 5.0, body: Some([215, 185, 140]) }) });
        b.add_footprint(fp);
        b.set_part(Part { refdes: "R1".into(), value: "220".into(), footprint: "R_Axial_Test".into(), pos: Point::new(15.0, 10.0), rot: 0, side: Side::Top }).unwrap();
        let solids = solids(&b, &b.parts[0]);
        // the wire, the body and four bands: red red brown gold, the first nearest pin 1 (on the left)
        assert_eq!(solids.len(), 6);
        assert_eq!(solids[1].color, [215, 185, 140]);
        let bands: Vec<([u8; 3], f64)> = solids[2..].iter().map(|s| (s.color, (s.lo[0] + s.hi[0]) / 2.0)).collect();
        assert_eq!(bands.iter().map(|b| b.0).collect::<Vec<_>>(), vec![[210, 35, 35], [210, 35, 35], [125, 70, 30], [200, 160, 50]]);
        assert!(bands.windows(2).all(|w| w[0].1 < w[1].1), "read from the left: {bands:?}");
        assert!(matches!(solids[1].shape, BodyShape::Cylinder(0)));
    }

    #[test]
    fn a_round_can_marked_plus_shows_its_minus_stripe() {
        use crate::body::BodySpec;
        let mut b = Board::new("t", 30.0, 20.0);
        let mut fp = crate::footprints::get("R_0805").unwrap();
        fp.name = "CP_Test".into();
        fp.height = Some(11.0);
        fp.pads = [("1", -1.25), ("2", 1.25)]
            .into_iter()
            .map(|(pin, x)| crate::footprints::FpPad { pin: pin.into(), pos: Point::new(x, 0.0), shape: PadShape::Circle { d: 1.6 }, drill: Some(0.8), slot: 0.0, slot_v: false })
            .collect();
        fp.body = Some(BodySpec { shape: "circle".into(), w: 6.3, h: 0.0, x: 0.0, y: 0.0, mark: crate::body::BodyMark::Plus, mark_pin: "1".into(), color_code: None });
        b.add_footprint(fp);
        b.set_part(Part { refdes: "C1".into(), value: "100u".into(), footprint: "CP_Test".into(), pos: Point::new(15.0, 10.0), rot: 0, side: Side::Top }).unwrap();
        let s = solids(&b, &b.parts[0]);
        // the can and the stripe; pin 1 (+) is on the left, so the stripe is on the right edge of the can
        assert_eq!(s.len(), 2);
        assert_eq!(s[1].color, MARK);
        assert!(s[1].lo[0] > 15.0 + 3.15 - 0.5 && s[1].hi[0] > 15.0 + 3.15, "{:?}", (s[1].lo, s[1].hi));
    }
}
