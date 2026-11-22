//! The schematic symbols as line segments and circles in grid units (no drawing library): the app and the browser version both paint
//! these. Pin positions come from `Kind::pins` / `CustomSymbol::layout`; this is only how the parts look.

use crate::{Component, CustomSymbol, Kind, Pt};

pub enum Sh {
    L(f32, f32, f32, f32),
    C(f32, f32, f32),
}

pub fn symbol(kind: Kind) -> Vec<Sh> {
    use Sh::*;
    match kind {
        Kind::Resistor => vec![
            L(-2., 0., -1., 0.),
            L(1., 0., 2., 0.),
            L(-1., -0.4, 1., -0.4),
            L(-1., 0.4, 1., 0.4),
            L(-1., -0.4, -1., 0.4),
            L(1., -0.4, 1., 0.4),
        ],
        // a resistor with the wiper (pin 2) coming down onto the track as an arrow
        Kind::Pot => vec![
            L(-2., 0., -1., 0.),
            L(1., 0., 2., 0.),
            L(-1., -0.4, 1., -0.4),
            L(-1., 0.4, 1., 0.4),
            L(-1., -0.4, -1., 0.4),
            L(1., -0.4, 1., 0.4),
            L(0., -2., 0., -0.45),
            L(0., -0.45, -0.3, -0.95),
            L(0., -0.45, 0.3, -0.95),
        ],
        Kind::Capacitor => vec![
            L(-2., 0., -0.25, 0.),
            L(0.25, 0., 2., 0.),
            L(-0.25, -0.8, -0.25, 0.8),
            L(0.25, -0.8, 0.25, 0.8),
        ],
        Kind::Inductor => {
            let mut v = vec![L(-2., 0., -1.2, 0.), L(1.2, 0., 2., 0.)];
            let n = 48;
            let pt = |i: i32| {
                let x = -1.2 + 2.4 * i as f32 / n as f32;
                (x, -0.5 * (std::f32::consts::PI * (x + 1.2) / 0.6).sin().abs())
            };
            for i in 0..n {
                let (a, b) = (pt(i), pt(i + 1));
                v.push(L(a.0, a.1, b.0, b.1));
            }
            v
        }
        Kind::Diode | Kind::Led => {
            let mut v = vec![
                L(-2., 0., -0.6, 0.),
                L(0.6, 0., 2., 0.),
                L(-0.6, -0.6, -0.6, 0.6),
                L(-0.6, -0.6, 0.6, 0.),
                L(-0.6, 0.6, 0.6, 0.),
                L(0.6, -0.6, 0.6, 0.6),
            ];
            if kind == Kind::Led {
                v.extend([L(0., -0.9, 0.6, -1.5), L(0.6, -0.9, 1.2, -1.5)]);
                v.extend(crate::glyphs::led_arrow_heads());
            }
            v
        }
        Kind::VSource => vec![
            L(0., -2., 0., -1.2),
            L(0., 1.2, 0., 2.),
            C(0., 0., 1.2),
            L(-0.25, -0.6, 0.25, -0.6),
            L(0., -0.85, 0., -0.35),
            L(-0.25, 0.6, 0.25, 0.6),
        ],
        // a circle with an arrow pointing to pin +: the way the current leaves the source
        Kind::ISource => vec![
            L(0., -2., 0., -1.2),
            L(0., 1.2, 0., 2.),
            C(0., 0., 1.2),
            L(0., 0.7, 0., -0.7),
            L(0., -0.7, -0.3, -0.3),
            L(0., -0.7, 0.3, -0.3),
        ],
        Kind::Npn => vec![
            C(0.5, 0., 1.7),
            L(-2., 0., -0.5, 0.),
            L(-0.5, -1., -0.5, 1.),
            L(-0.5, -0.4, 2., -2.),
            L(-0.5, 0.4, 2., 2.),
            L(1.2, 1.5, 2., 2.),
            L(1.5, 1.1, 2., 2.),
        ],
        Kind::Header(n) => {
            let mut v = vec![
                L(0., -1., 1.2, -1.),
                L(0., -1., 0., 2. * n as f32 - 1.),
                L(1.2, -1., 1.2, 2. * n as f32 - 1.),
                L(0., 2. * n as f32 - 1., 1.2, 2. * n as f32 - 1.),
            ];
            for i in 0..n as i32 {
                v.push(L(1.2, 2. * i as f32, 2., 2. * i as f32));
                v.push(C(0.6, 2. * i as f32, 0.25));
            }
            v
        }
        Kind::Ground => vec![
            L(0., 0., 0., 0.5),
            L(-0.8, 0.5, 0.8, 0.5),
            L(-0.5, 0.8, 0.5, 0.8),
            L(-0.2, 1.1, 0.2, 1.1),
        ],
        Kind::Power => vec![L(0., 0., 0., -0.6), L(-0.6, -0.6, 0.6, -0.6)],
        Kind::Custom => vec![],
    }
}

/// What a voltage source's value says it produces, so the symbol can show it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceShape {
    Dc,
    Sine,
    Pulse,
    Other,
}

pub fn source_shape(value: &str) -> SourceShape {
    let v = value.trim().to_ascii_uppercase();
    if v.starts_with("SIN") {
        SourceShape::Sine
    } else if v.starts_with("PULSE") {
        SourceShape::Pulse
    } else if v.starts_with("PWL") || v.starts_with("NOISE") {
        SourceShape::Other
    } else {
        SourceShape::Dc
    }
}

/// A voltage source's symbol: a circle with +/- for DC, a sine wave for an AC source, a square
/// wave for a pulse source (a small + near the top still marks pin 1).
pub fn source_symbol(value: &str) -> Vec<Sh> {
    use Sh::*;
    let shape = source_shape(value);
    if shape == SourceShape::Dc {
        return symbol(Kind::VSource);
    }
    let mut v = vec![L(0., -2., 0., -1.2), L(0., 1.2, 0., 2.), C(0., 0., 1.2), L(-0.2, -0.85, 0.2, -0.85), L(0., -1.05, 0., -0.65)];
    match shape {
        SourceShape::Sine => {
            let n = 16;
            let pt = |i: i32| {
                let x = -0.8 + 1.6 * i as f32 / n as f32;
                (x, 0.2 - 0.4 * (2.0 * std::f32::consts::PI * (x + 0.8) / 1.6).sin())
            };
            for i in 0..n {
                let (a, b) = (pt(i), pt(i + 1));
                v.push(L(a.0, a.1, b.0, b.1));
            }
        }
        SourceShape::Pulse => {
            let pts = [(-0.9, 0.6), (-0.45, 0.6), (-0.45, -0.1), (0.1, -0.1), (0.1, 0.6), (0.9, 0.6)];
            for w in pts.windows(2) {
                v.push(L(w[0].0, w[0].1, w[1].0, w[1].1));
            }
        }
        _ => {
            // arbitrary waveform: a zigzag
            let pts = [(-0.9, 0.5), (-0.4, -0.1), (0.0, 0.4), (0.4, 0.0), (0.9, 0.5)];
            for w in pts.windows(2) {
                v.push(L(w[0].0, w[0].1, w[1].0, w[1].1));
            }
        }
    }
    v
}

/// Local bounding box (xmin, xmax, ymin, ymax).
pub fn extents(kind: Kind) -> (f32, f32, f32, f32) {
    match kind {
        Kind::VSource | Kind::ISource => (-1.4, 1.4, -2., 2.),
        Kind::Npn => (-2., 2., -2., 2.),
        Kind::Header(n) => (0., 2., -1., 2. * n as f32 - 1.),
        Kind::Custom => (-2., 2., -2., 2.),
        Kind::Ground => (-1., 1., 0., 1.2),
        Kind::Power => (-1., 1., -1., 0.),
        _ => (-2., 2., -1., 1.),
    }
}

pub fn sym_extents(sym: &CustomSymbol) -> (f32, f32, f32, f32) {
    if sym.is_two_lead_glyph() {
        return (-2., 2., -1., 1.);
    }
    let l = sym.layout();
    // a compact diode / battery: the leads are the whole width, the body is one step each side of the line
    if l.hw == 0 && l.hh == 1 {
        // (a four-pin crystal also has its case pins below the body)
        let bottom = l.pins.iter().map(|p| p.tip.1).max().unwrap_or(0).max(1);
        return (-2., 2., -1., bottom as f32);
    }
    let (w, h) = ((l.hw + 2) as f32, (l.hh + 2) as f32);
    (-w, w, -h, h)
}

pub fn comp_extents(c: &Component) -> (f32, f32, f32, f32) {
    match &c.custom {
        Some(sym) => sym_extents(sym),
        None => extents(c.kind),
    }
}


/// The area a part occupies on the sheet (xmin, xmax, ymin, ymax) after rotation. Ground and
/// power symbols have none: they are meant to sit on a pin or wire next to a part.
pub fn sheet_box(kind: Kind, ext: (f32, f32, f32, f32), pos: Pt, rot: u8) -> Option<(f32, f32, f32, f32)> {
    if kind.is_power_symbol() {
        return None;
    }
    let mut b = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for corner in [(ext.0, ext.2), (ext.1, ext.2), (ext.0, ext.3), (ext.1, ext.3)] {
        let (x, y) = rot_f(corner, rot);
        b = (b.0.min(x), b.1.max(x), b.2.min(y), b.3.max(y));
    }
    Some((b.0 + pos.0 as f32, b.1 + pos.0 as f32, b.2 + pos.1 as f32, b.3 + pos.1 as f32))
}

/// Parts that merely touch (a pin on a neighbour's edge) do not overlap.
pub fn boxes_overlap(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> bool {
    const EPS: f32 = 1e-3;
    a.0 < b.1 - EPS && b.0 < a.1 - EPS && a.2 < b.3 - EPS && b.2 < a.3 - EPS
}

pub fn rot_f(p: (f32, f32), rot: u8) -> (f32, f32) {
    let mut q = p;
    for _ in 0..(rot % 4) {
        q = (-q.1, q.0);
    }
    q
}

/// Every line and circle of a part as it is drawn, in grid units around the part's position before its turn (`rot_f` turns them):
/// the standard body of a library part when it has one, a chip part as the basic resistor / capacitor / inductor with leads to its
/// pins, else the box with its pins (and a mark inside: a gate, a regulator). Text (pin names and numbers, the value) is not here.
pub fn component_shapes(c: &Component) -> Vec<Sh> {
    use crate::glyphs::{body, body_glyph, is_thermistor, overlay, overlay_glyph, thermistor_mark};
    let Some(sym) = &c.custom else {
        return if c.kind == Kind::VSource { source_symbol(&c.value) } else { symbol(c.kind) };
    };
    let l = sym.layout();
    if let Some(shapes) = body_glyph(sym).and_then(|g| body(g, &l)) {
        return shapes;
    }
    let chip = match sym.glyph.as_str() {
        "capacitor" | "electrolytic" => Some(Kind::Capacitor),
        "resistor" => Some(Kind::Resistor),
        "inductor" => Some(Kind::Inductor),
        _ => None,
    };
    if let Some(kind) = chip.filter(|_| sym.is_two_lead_glyph()) {
        let mut v: Vec<Sh> = l.pins.iter().map(|p| Sh::L(p.tip.0 as f32, p.tip.1 as f32, if p.tip.0 < 0 { -2.0 } else { 2.0 }, 0.0)).collect();
        v.extend(symbol(kind));
        if is_thermistor(sym) {
            v.extend(thermistor_mark());
        }
        if sym.glyph == "electrolytic" {
            v.extend([Sh::L(-1.5, -0.9, -0.9, -0.9), Sh::L(-1.2, -1.2, -1.2, -0.6)]);
        }
        return v;
    }
    let (hw, hh) = (l.hw as f32, l.hh as f32);
    let mut v = vec![Sh::L(-hw, -hh, hw, -hh), Sh::L(hw, -hh, hw, hh), Sh::L(hw, hh, -hw, hh), Sh::L(-hw, hh, -hw, -hh)];
    v.extend(l.pins.iter().map(|p| Sh::L(p.tip.0 as f32, p.tip.1 as f32, p.edge.0 as f32, p.edge.1 as f32)));
    if let Some(g) = overlay_glyph(sym) {
        v.extend(overlay(g, &l));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every basic part has a drawing, and its pins end on it: a lead tip of the drawing touches each pin.
    #[test]
    fn every_basic_part_is_drawn_up_to_its_pins() {
        for kind in Kind::PALETTE.iter().copied() {
            let c = Component { refdes: "X1".into(), kind, value: "5".into(), pos: (0, 0), rot: 0, footprint: String::new(), custom: None, sim_only: false, pin_names: vec![], theta: 0.0, tc_ppm: 0.0, cth: 0.0, t_max: 0.0, zth: vec![], theta_jc: 0.0, heatsink: 0.0, sub: None, shops: Default::default() };
            let shapes = component_shapes(&c);
            assert!(!shapes.is_empty(), "{kind:?}");
            for (pin, at) in kind.pins() {
                let touches = shapes.iter().any(|s| match *s {
                    Sh::L(a, b, x, y) => [(a, b), (x, y)].iter().any(|p| (p.0 - at.0 as f32).abs() < 0.01 && (p.1 - at.1 as f32).abs() < 0.01),
                    Sh::C(x, y, r) => ((x - at.0 as f32).hypot(y - at.1 as f32) - r).abs() < 0.01,
                });
                assert!(touches, "{kind:?} pin {pin} at {at:?}");
            }
        }
    }
}
