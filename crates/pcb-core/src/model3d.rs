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
    /// A surface-mount IC or transistor with legs.
    SmdIc,
    /// A through-hole IC (DIP).
    Dip,
    /// Anything else: one box.
    Other,
}

fn is_through_hole(fp: &str) -> bool {
    ["axial", "disc", "dip", "to-92", "to-220", "pinheader", "5mm", "do-", "transformer", "radial"].iter().any(|k| fp.contains(k))
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

/// Body height (mm above the board) and colour guessed from the reference designator and footprint.
pub fn part_style(refdes: &str, footprint: &str) -> (f64, [u8; 3]) {
    let fp = footprint.to_ascii_lowercase();
    let th = is_through_hole(&fp);
    if fp.contains("pinheader") {
        return (2.5, [24, 24, 26]);
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
        'Q' | 'U' if fp.contains("to-220") => (4.5, [40, 40, 44]),
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
            // the + sign is drawn on the silkscreen; the 3D body stays plain
            BodyMark::Plus | BodyMark::None => {}
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
        Kind::Other => vec![solid([lo.x - 0.1, lo.y - 0.1, 0.0], [hi.x + 0.1, hi.y + 0.1, h], color, b)],
    }
}

/// The solids of every part that has pads. Bottom-side parts hang below the board.
pub fn part_boxes(board: &Board) -> Vec<PartBox> {
    let mut out = vec![];
    for (i, part) in board.parts.iter().enumerate() {
        let bottom = part.side == Side::Bottom;
        for s in solids(board, part) {
            let (z0, z1) = if bottom { (-s.hi[2], -s.lo[2]) } else { (BOARD_THICKNESS + s.lo[2], BOARD_THICKNESS + s.hi[2]) };
            out.push(PartBox { refdes: part.refdes.clone(), part: i, lo: [s.lo[0], s.lo[1], z0], hi: [s.hi[0], s.hi[1], z1], color: s.color, bottom, shape: s.shape });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A part whose footprint carries the data sheet body is drawn as that body: its size, its height,
    /// turned with the part, with the pin 1 dot on top.
    #[test]
    fn a_data_sheet_body_sets_the_size_and_height_of_the_3d_part() {
        use crate::body::{BodyMark, BodySpec};
        let mut b = Board::new("t", 30.0, 30.0);
        let mut fp = crate::footprints::get("R_0805").unwrap();
        fp.name = "BODY".into();
        fp.height = Some(2.0);
        fp.body = Some(BodySpec { shape: "rect".into(), w: 4.0, h: 3.0, x: 0.0, y: 0.0, mark: BodyMark::Dot, mark_pin: "1".into() });
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
        fp.body = Some(BodySpec { shape: "circle".into(), w: 5.0, h: 0.0, x: 0.0, y: 0.0, mark: BodyMark::None, mark_pin: String::new() });
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
        assert_eq!(part_kind("T1", "Transformer_EI30"), Kind::Other);
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
}
