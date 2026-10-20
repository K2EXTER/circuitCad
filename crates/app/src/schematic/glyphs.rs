//! Standard bodies (JIS C 0617 / IEC 60617) for library parts that are not plain chip parts:
//! diodes, transistors, op-amps, transformers, batteries and so on.
//!
//! The pin tips stay where the box layout (`CustomSymbol::layout`) puts them, so circuits saved
//! with the box symbol keep their wiring; only the body inside the box and the leads change.

use super::symbols::Sh::{self, C, L};
use sch_core::{CustomSymbol, PinGeom, SymLayout};

type P = (f32, f32);

/// Which body a part gets: the `glyph` of the part when it names one, else a guess from the part
/// name and pin names (so parts installed before the glyphs existed look right too).
pub(super) fn body_glyph(sym: &CustomSymbol) -> Option<&'static str> {
    const KNOWN: [&str; 17] = [
        "diode", "zener", "schottky", "npn", "pnp", "nmos", "pmos", "opamp", "transformer", "battery", "ldr", "photodiode", "shunt_reg", "led", "tvs", "ac3p", "crystal",
    ];
    if let Some(g) = KNOWN.iter().find(|g| **g == sym.glyph) {
        return Some(g);
    }
    if !sym.glyph.is_empty() {
        return None;
    }
    // saved before the glyphs existed: guess from the name and pins (the box layout stays)
    let g = sym.inferred_glyph();
    KNOWN.iter().find(|k| **k == g).copied()
}

/// Does a chip resistor from the library stand for a thermistor / RTD (a slash is added to the resistor)?
pub(super) fn is_thermistor(sym: &CustomSymbol) -> bool {
    sym.glyph == "resistor" && (sym.name.starts_with("NTC") || sym.name.starts_with("PTC") || sym.name.starts_with("PT100") || sym.name.starts_with("PT1000"))
}

/// The slash drawn over the resistor of a thermistor (the foot marks "changes with temperature").
pub(super) fn thermistor_mark() -> Vec<Sh> {
    vec![L(-1.2, 0.9, -0.6, 0.9), L(-0.6, 0.9, 0.7, -0.9)]
}

fn pt(p: &PinGeom) -> P {
    (p.tip.0 as f32, p.tip.1 as f32)
}

fn find<'a>(l: &'a SymLayout, names: &[&str]) -> Option<&'a PinGeom> {
    l.pins.iter().find(|p| names.iter().any(|n| p.name.eq_ignore_ascii_case(n)))
}

/// Two barbs of an arrow head whose tip is at `t` and which points along `d` (unit).
fn arrow_head(t: P, d: P) -> Vec<Sh> {
    let n = (-d.1, d.0);
    let (b, w) = (0.4, 0.22);
    let base = (t.0 - d.0 * b, t.1 - d.1 * b);
    vec![L(t.0, t.1, base.0 + n.0 * w, base.1 + n.1 * w), L(t.0, t.1, base.0 - n.0 * w, base.1 - n.1 * w)]
}

/// An arrow from `a` to `b` (shaft and head).
fn arrow(a: P, b: P) -> Vec<Sh> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt().max(1e-6);
    let mut v = vec![L(a.0, a.1, b.0, b.1)];
    v.extend(arrow_head(b, (dx / len, dy / len)));
    v
}

/// The two light arrows pointing at a body (a photo sensor), from the upper left.
fn light_in() -> Vec<Sh> {
    let mut v = arrow((-1.5, -2.0), (-0.7, -1.0));
    v.extend(arrow((-0.6, -2.2), (0.2, -1.2)));
    v
}

/// The diode triangle and bar between the anode lead and the cathode lead, mirrored by `s` (+1 = cathode on the right).
fn diode_shapes(a: P, k: P, s: f32, hooks: &str) -> Vec<Sh> {
    let mut v = vec![
        L(a.0, a.1, -0.6 * s, 0.0),
        L(0.6 * s, 0.0, k.0, k.1),
        L(-0.6 * s, -0.6, -0.6 * s, 0.6),
        L(-0.6 * s, -0.6, 0.6 * s, 0.0),
        L(-0.6 * s, 0.6, 0.6 * s, 0.0),
        L(0.6 * s, -0.6, 0.6 * s, 0.6),
    ];
    match hooks {
        // zener: both ends of the bar bend away, like a "Z"
        "zener" => v.extend([L(0.6 * s, -0.6, 0.25 * s, -0.85), L(0.6 * s, 0.6, 0.95 * s, 0.85)]),
        // schottky: both ends bend at right angles, like an "S"
        "schottky" => v.extend([
            L(0.6 * s, -0.6, 0.25 * s, -0.6),
            L(0.25 * s, -0.6, 0.25 * s, -0.85),
            L(0.6 * s, 0.6, 0.95 * s, 0.6),
            L(0.95 * s, 0.6, 0.95 * s, 0.85),
        ]),
        _ => {}
    }
    v
}

/// The shapes of the standard symbol, leads included. None when the pins do not fit the symbol (the box is used then).
pub(super) fn body(glyph: &str, l: &SymLayout) -> Option<Vec<Sh>> {
    // compact transistors (tips 2 grid steps from the centre): draw the full-size symbol for tips
    // scaled up by 1/F, then scale everything down, so the leads still end on the real tips
    const F: f32 = 0.62;
    if l.hh == 0 && l.hw == 0 && matches!(glyph, "npn" | "pnp" | "nmos" | "pmos") {
        let big = SymLayout {
            hw: 0,
            hh: 0,
            pins: l
                .pins
                .iter()
                .map(|p| PinGeom { number: p.number.clone(), name: p.name.clone(), side: p.side, tip: ((p.tip.0 as f32 / F).round() as i32, (p.tip.1 as f32 / F).round() as i32), edge: p.edge })
                .collect(),
        };
        return body_full(glyph, &big).map(|v| {
            v.into_iter()
                .map(|s| match s {
                    L(a, b, c, d) => L(a * F, b * F, c * F, d * F),
                    C(x, y, r) => C(x * F, y * F, r * F),
                })
                .collect()
        });
    }
    body_full(glyph, l)
}

fn body_full(glyph: &str, l: &SymLayout) -> Option<Vec<Sh>> {
    match glyph {
        "diode" | "zener" | "schottky" | "tvs" => {
            let (a, k) = (find(l, &["A", "1"])?, find(l, &["K", "2"])?);
            let (a, k) = (pt(a), pt(k));
            let s = if k.0 >= a.0 { 1.0 } else { -1.0 };
            Some(diode_shapes(a, k, s, glyph))
        }
        "photodiode" => {
            let (a, k) = (pt(find(l, &["A"])?), pt(find(l, &["K"])?));
            let s = if k.0 >= a.0 { 1.0 } else { -1.0 };
            let mut v = diode_shapes(a, k, s, "diode");
            v.extend(light_in());
            if let Some(p) = find(l, &["L", "LIGHT", "IRRAD"]) {
                let t = pt(p);
                v.push(L(t.0, t.1, 0.0, 1.0));
            }
            Some(v)
        }
        "npn" | "pnp" => {
            let (b, c, e) = (pt(find(l, &["B"])?), pt(find(l, &["C"])?), pt(find(l, &["E"])?));
            // collector side of the symbol is where the C pin is (up or down)
            let k = if c.1 < e.1 { 1.0 } else { -1.0 };
            let mut v = vec![
                C(-0.1, 0.0, 1.55),
                L(b.0, b.1, -0.6, 0.0),
                L(-0.6, -1.0, -0.6, 1.0),
                L(-0.6, -0.4 * k, 0.0, -1.2 * k),
                L(0.0, -1.2 * k, c.0, c.1),
                L(-0.6, 0.4 * k, 0.0, 1.2 * k),
                L(0.0, 1.2 * k, e.0, e.1),
            ];
            // the emitter arrow: out of the base for NPN, into the base for PNP
            let (p0, p1) = ((-0.6, 0.4 * k), (0.0, 1.2 * k));
            let d = (0.6f32 / 1.0, 0.8 * k);
            let len = (d.0 * d.0 + d.1 * d.1).sqrt();
            let u = (d.0 / len, d.1 / len);
            if glyph == "npn" {
                let t = (p0.0 + (p1.0 - p0.0) * 0.8, p0.1 + (p1.1 - p0.1) * 0.8);
                v.extend(arrow_head(t, u));
            } else {
                let t = (p0.0 + (p1.0 - p0.0) * 0.3, p0.1 + (p1.1 - p0.1) * 0.3);
                v.extend(arrow_head(t, (-u.0, -u.1)));
            }
            Some(v)
        }
        "nmos" | "pmos" => {
            let (g, d, s) = (pt(find(l, &["G"])?), pt(find(l, &["D"])?), pt(find(l, &["S"])?));
            // template has the drain above; flip when the D pin is below
            let k = if d.1 < s.1 { 1.0 } else { -1.0 };
            let mut v = vec![
                L(g.0, g.1, -0.9, 0.0),
                L(-0.9, -1.0, -0.9, 1.0),
                // channel, drawn in three pieces
                L(-0.4, -k, -0.4, -0.6 * k),
                L(-0.4, -0.2, -0.4, 0.2),
                L(-0.4, 0.6 * k, -0.4, 1.0 * k),
                L(-0.4, -0.8 * k, 0.0, -0.8 * k),
                L(0.0, -0.8 * k, d.0, d.1),
                L(-0.4, 0.8 * k, 0.0, 0.8 * k),
                L(0.0, 0.8 * k, s.0, s.1),
                // body tied to the source
                L(-0.4, 0.0, 0.0, 0.0),
                L(0.0, 0.0, 0.0, 0.8 * k),
            ];
            // body arrow: towards the channel for N, away from it for P
            if glyph == "nmos" {
                v.extend(arrow_head((-0.4, 0.0), (-1.0, 0.0)));
            } else {
                v.extend(arrow_head((-0.05, 0.0), (1.0, 0.0)));
            }
            Some(v)
        }
        "opamp" => {
            let out = pt(find(l, &["OUT"])?);
            let plus = find(l, &["IN+"])?;
            let minus = find(l, &["IN-"])?;
            let (pp, mp) = (pt(plus), pt(minus));
            let mut v = vec![
                L(-2.0, -2.0, -2.0, 2.0),
                L(-2.0, -2.0, 2.0, 0.0),
                L(-2.0, 2.0, 2.0, 0.0),
                L(2.0, 0.0, out.0, out.1),
                L(pp.0, pp.1, -2.0, pp.1),
                L(mp.0, mp.1, -2.0, mp.1),
                // + and - marks inside, next to the inputs
                L(-1.6, pp.1, -1.0, pp.1),
                L(-1.3, pp.1 - 0.3, -1.3, pp.1 + 0.3),
                L(-1.6, mp.1, -1.0, mp.1),
            ];
            for p in &l.pins {
                // power pins come in from the top and bottom edges of the triangle
                let t = pt(p);
                if t.0.abs() < 0.5 && t.1.abs() > 2.5 {
                    v.push(L(t.0, t.1, 0.0, t.1.signum() * 1.0));
                }
            }
            Some(v)
        }
        "shunt_reg" => {
            // TL431: a zener-like diode with the reference lead on the bar
            let (c, r, a) = (pt(find(l, &["CATHODE", "K"])?), pt(find(l, &["REF", "R"])?), pt(find(l, &["ANODE", "A"])?));
            Some(vec![
                L(c.0, c.1, 0.0, -0.6),
                L(a.0, a.1, 0.0, 0.6),
                L(-0.6, 0.6, 0.6, 0.6),
                L(-0.6, 0.6, 0.0, -0.6),
                L(0.6, 0.6, 0.0, -0.6),
                L(-0.6, -0.6, 0.6, -0.6),
                L(-0.6, -0.6, -0.9, -0.85),
                L(0.6, -0.6, 0.9, -0.35),
                L(r.0, r.1, -1.2, r.1),
                L(-1.2, r.1, -1.2, -0.6),
                L(-1.2, -0.6, -0.6, -0.6),
            ])
        }
        "transformer" => {
            let (p1, p2, s1, s2) = (pt(find(l, &["P1"])?), pt(find(l, &["P2"])?), pt(find(l, &["S1"])?), pt(find(l, &["S2"])?));
            let mut v = vec![];
            // each winding: three half-circle bumps between its two leads
            for (top, bottom, x, bulge) in [(p1, p2, -2.0f32, -1.0f32), (s1, s2, 2.0, 1.0)] {
                v.push(L(top.0, top.1, x, top.1));
                v.push(L(bottom.0, bottom.1, x, bottom.1));
                let (y0, y1) = (top.1.min(bottom.1), top.1.max(bottom.1));
                let n = 3;
                let r = (y1 - y0) / (2.0 * n as f32);
                for i in 0..n {
                    let cy = y0 + r * (2 * i + 1) as f32;
                    let m = 10;
                    for j in 0..m {
                        let (a0, a1) = (std::f32::consts::PI * (j as f32 / m as f32 - 0.5), std::f32::consts::PI * ((j + 1) as f32 / m as f32 - 0.5));
                        v.push(L(x + bulge * r * a0.cos(), cy + r * a0.sin(), x + bulge * r * a1.cos(), cy + r * a1.sin()));
                    }
                }
            }
            // iron core: two lines between the windings
            v.push(L(-0.15, -1.3, -0.15, 1.3));
            v.push(L(0.15, -1.3, 0.15, 1.3));
            Some(v)
        }
        "crystal" => {
            // two parallel plates with a long rectangle between them (JIS C 0617 / IEC 60617); the leads are X1/X2 (or 1/2 of a
            // two-pin crystal); the case pins of a four-pin part (named GND) hang below on a bar
            let two = l.pins.len() == 2;
            let (a, b) = if two { (pt(find(l, &["1"])?), pt(find(l, &["2"])?)) } else { (pt(find(l, &["X1"])?), pt(find(l, &["X2"])?)) };
            let s = if b.0 >= a.0 { 1.0 } else { -1.0 };
            let mut v = vec![
                L(a.0, a.1, -0.5 * s, 0.0),
                L(b.0, b.1, 0.5 * s, 0.0),
                // the plates
                L(-0.5 * s, -0.9, -0.5 * s, 0.9),
                L(0.5 * s, -0.9, 0.5 * s, 0.9),
                // the crystal between them
                L(-0.25, -0.7, 0.25, -0.7),
                L(0.25, -0.7, 0.25, 0.7),
                L(0.25, 0.7, -0.25, 0.7),
                L(-0.25, 0.7, -0.25, -0.7),
            ];
            let case: Vec<P> = l.pins.iter().filter(|p| p.name.eq_ignore_ascii_case("GND")).map(pt).collect();
            if !two && !case.is_empty() {
                let bar = 1.5f32;
                for c in &case {
                    v.push(L(c.0, c.1, c.0, bar));
                }
                let (x0, x1) = case.iter().fold((f32::MAX, f32::MIN), |m, c| (m.0.min(c.0), m.1.max(c.0)));
                v.push(L(x0, bar, x1, bar));
            }
            Some(v)
        }
        "battery" => {
            let (plus, minus) = (pt(find(l, &["+"])?), pt(find(l, &["-"])?));
            let s = if plus.0 >= minus.0 { 1.0 } else { -1.0 };
            Some(vec![
                L(plus.0, plus.1, 0.3 * s, 0.0),
                L(minus.0, minus.1, -0.3 * s, 0.0),
                // long plate = plus, short plate = minus
                L(0.3 * s, -0.9, 0.3 * s, 0.9),
                L(-0.3 * s, -0.45, -0.3 * s, 0.45),
                // a small + beside the plus lead
                L(0.9 * s, -0.8, 1.5 * s, -0.8),
                L(1.2 * s, -1.1, 1.2 * s, -0.5),
            ])
        }
        "ac3p" => {
            // three AC sources (circles with a sine wave, 120 degrees apart) joined at the neutral N: one per phase pin
            let (a, b, c, n) = (pt(find(l, &["A"])?), pt(find(l, &["B"])?), pt(find(l, &["C"])?), pt(find(l, &["N"])?));
            let (cx, r, bus) = (1.0f32, 0.9f32, -1.5f32);
            let mut v = vec![];
            for (tip, phase) in [(a, 0.0f32), (b, -120.0), (c, -240.0)] {
                let y = tip.1;
                v.push(C(cx, y, r));
                v.push(L(tip.0, y, cx + r, y));
                v.push(L(cx - r, y, bus, y));
                let m = 12;
                let at = |j: usize| {
                    let t = j as f32 / m as f32;
                    (cx - 0.6 + 1.2 * t, y - 0.35 * (std::f32::consts::TAU * t + phase.to_radians()).sin())
                };
                for j in 0..m {
                    let (p, q) = (at(j), at(j + 1));
                    v.push(L(p.0, p.1, q.0, q.1));
                }
            }
            // neutral: a bus joining the three sources, and the N lead down from its lower end
            v.push(L(bus, a.1, bus, c.1 + 1.0));
            v.push(L(n.0, n.1, n.0, c.1 + 1.0));
            v.push(L(n.0, c.1 + 1.0, bus, c.1 + 1.0));
            Some(v)
        }
        "ldr" => {
            let (a, b) = (pt(find(l, &["A"])?), pt(find(l, &["B"])?));
            let mut v = vec![
                L(a.0, a.1, -1.2, 0.0),
                L(1.2, 0.0, b.0, b.1),
                L(-1.2, -0.5, 1.2, -0.5),
                L(-1.2, 0.5, 1.2, 0.5),
                L(-1.2, -0.5, -1.2, 0.5),
                L(1.2, -0.5, 1.2, 0.5),
            ];
            v.extend(light_in());
            if let Some(p) = find(l, &["L", "LIGHT"]) {
                let t = pt(p);
                v.push(L(t.0, t.1, 0.0, 0.5));
            }
            Some(v)
        }
        _ => None,
    }
}

/// The symbol of a part as line segments in grid units (leads included), for the preview in the part form:
/// the standard symbol when the part has one, the basic capacitor / resistor / inductor for a chip part,
/// else the box with its pins.
pub(crate) fn symbol_lines(sym: &CustomSymbol) -> Vec<([f32; 2], [f32; 2])> {
    use super::symbols::symbol;
    let l = sym.layout();
    let mut out: Vec<([f32; 2], [f32; 2])> = vec![];
    let mut add = |shapes: Vec<Sh>| {
        for s in shapes {
            match s {
                L(a, b, c, d) => out.push(([a, b], [c, d])),
                C(x, y, r) => {
                    let n = 24;
                    for i in 0..n {
                        let (t0, t1) = (std::f32::consts::TAU * i as f32 / n as f32, std::f32::consts::TAU * (i + 1) as f32 / n as f32);
                        out.push(([x + r * t0.cos(), y + r * t0.sin()], [x + r * t1.cos(), y + r * t1.sin()]));
                    }
                }
            }
        }
    };
    if let Some(shapes) = body_glyph(sym).and_then(|g| body(g, &l)) {
        add(shapes);
        return out;
    }
    if sym.is_two_lead_glyph() {
        let kind = match sym.glyph.as_str() {
            "capacitor" | "electrolytic" => sch_core::Kind::Capacitor,
            "resistor" => sch_core::Kind::Resistor,
            _ => sch_core::Kind::Inductor,
        };
        add(symbol(kind));
        return out;
    }
    let (hw, hh) = (l.hw as f32, l.hh as f32);
    add(vec![L(-hw, -hh, hw, -hh), L(hw, -hh, hw, hh), L(hw, hh, -hw, hh), L(-hw, hh, -hw, -hh)]);
    add(l.pins.iter().map(|p| L(p.tip.0 as f32, p.tip.1 as f32, p.edge.0 as f32, p.edge.1 as f32)).collect());
    out
}

/// The pin names of a part's symbol with the point each one is drawn at (for the preview).
pub(crate) fn symbol_pin_labels(sym: &CustomSymbol) -> Vec<(String, [f32; 2])> {
    sym.layout().pins.iter().map(|p| (p.name.clone(), [p.tip.0 as f32, p.tip.1 as f32 - 0.2])).collect()
}

/// Extra marks for the basic LED: arrow heads on the two light rays.
pub(super) fn led_arrow_heads() -> Vec<Sh> {
    let mut v = arrow_head((0.6, -1.5), (0.707, -0.707));
    v.extend(arrow_head((1.2, -1.5), (0.707, -0.707)));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every bundled part that gets a standard symbol has the pins that symbol needs
    /// (otherwise it would silently fall back to the box).
    #[test]
    fn bundled_parts_fit_their_symbol() {
        let mut n = 0;
        for (_, text) in part_lib::EXAMPLES {
            let d: part_lib::PartDef = serde_json::from_str(text).unwrap();
            let sym = d.symbol();
            if let Some(g) = body_glyph(&sym) {
                assert!(body(g, &sym.layout()).is_some(), "{}: pins do not fit the {g} symbol", d.name);
                n += 1;
            }
        }
        assert!(n >= 45, "only {n} parts got a standard symbol");
    }

    /// The crystal is drawn as two plates with a rectangle between them (also for a circuit saved before the glyph existed),
    /// with its leads on the real pin tips and the case pins on a bar below.
    #[test]
    fn the_crystal_gets_plates_a_rectangle_and_a_case_bar() {
        let text = part_lib::EXAMPLES.iter().find(|(n, _)| n.starts_with("XTAL_16MHz")).unwrap().1;
        let d: part_lib::PartDef = serde_json::from_str(text).unwrap();
        let mut sym = d.symbol();
        assert_eq!(body_glyph(&sym), Some("crystal"));
        let l = sym.layout();
        // a sheet saved without the glyph keeps its old box layout (so its wires stay), but is still drawn as a crystal
        sym.glyph = String::new();
        assert_eq!(body_glyph(&sym), Some("crystal"), "recognised by the name");
        let shapes = body("crystal", &l).unwrap();
        let lines: Vec<(f32, f32, f32, f32)> = shapes.iter().filter_map(|s| if let L(a, b, c, d) = s { Some((*a, *b, *c, *d)) } else { None }).collect();
        // every pin tip is the end of a lead
        for p in &l.pins {
            let t = (p.tip.0 as f32, p.tip.1 as f32);
            assert!(lines.iter().any(|s| (s.0, s.1) == t || (s.2, s.3) == t), "no lead ends on pin {}", p.number);
        }
        // two tall plates and the four sides of the rectangle
        assert_eq!(lines.iter().filter(|s| s.0 == s.2 && s.1.abs() < 1.0 && (s.3 - s.1).abs() > 1.7).count(), 2);
        // compact: the leads end 2 steps from the centre and the case pins 3 steps below it
        assert_eq!(l.pins.iter().map(|p| p.tip).max_by_key(|t| t.0).unwrap().0, 2);
        assert_eq!(l.pins.iter().map(|p| p.tip.1).max().unwrap(), 3);
        assert_eq!(lines.iter().filter(|s| (s.0 - s.2).abs() == 0.5 || (s.1 - s.3).abs() == 1.4).count(), 4);
    }
}

#[cfg(test)]
mod size_tests {
    use super::super::symbols::sym_extents;

    /// New parts with a small symbol also take a small place on the sheet (overlap check, label position).
    #[test]
    fn compact_parts_have_small_extents() {
        let get = |n: &str| {
            let (_, t) = part_lib::EXAMPLES.iter().find(|(f, _)| f.starts_with(n)).unwrap();
            serde_json::from_str::<part_lib::PartDef>(t).unwrap().symbol()
        };
        assert_eq!(sym_extents(&get("1N4007")), (-2., 2., -1., 1.));
        assert_eq!(sym_extents(&get("CR2032")), (-2., 2., -1., 1.));
        assert_eq!(sym_extents(&get("BC547")), (-2., 2., -2., 2.));
    }
}
