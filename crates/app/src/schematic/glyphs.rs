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
    const KNOWN: [&str; 15] = [
        "diode", "zener", "schottky", "npn", "pnp", "nmos", "pmos", "opamp", "transformer", "battery", "ldr", "photodiode", "shunt_reg", "led", "tvs",
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
                L(-0.4, -1.0 * k, -0.4, -0.6 * k),
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
