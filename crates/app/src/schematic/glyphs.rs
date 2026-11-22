//! The standard bodies of library parts now live in `sch_core::glyphs` (the browser version draws them too); the tests that need the
//! bundled parts (`part_lib`, which `sch-core` cannot use) stay here.

pub(crate) use sch_core::glyphs::*;
#[cfg(test)]
use sch_core::{symbol_shapes::Sh::{C, L}, CustomSymbol};

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

    /// The buzzer is a half circle on its flat side, both leads coming up from the pins below to the flat side.
    #[test]
    fn the_buzzer_is_a_dome_with_its_leads_below() {
        let text = part_lib::EXAMPLES.iter().find(|(n, _)| *n == "BUZZER_5V").unwrap().1;
        let d: part_lib::PartDef = serde_json::from_str(text).unwrap();
        let sym = d.symbol();
        assert_eq!(body_glyph(&sym), Some("buzzer"));
        let l = sym.layout();
        let lines: Vec<(f32, f32, f32, f32)> = body("buzzer", &l).unwrap().iter().filter_map(|s| if let L(a, b, c, d) = s { Some((*a, *b, *c, *d)) } else { None }).collect();
        for p in &l.pins {
            let t = (p.tip.0 as f32, p.tip.1 as f32);
            assert!(t.1 > 0.4, "pin {} is below the flat side", p.number);
            assert!(lines.iter().any(|s| (s.0, s.1) == t && s.0 == s.2 && s.3 == 0.4), "a straight lead up from pin {}", p.number);
        }
        // the dome rises above the flat side
        assert!(lines.iter().any(|s| s.1.min(s.3) < -0.6), "the dome");
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

#[cfg(test)]
mod gate_tests {
    use super::*;

    fn sym(name: &str) -> CustomSymbol {
        let text = part_lib::EXAMPLES.iter().find(|(n, _)| n.starts_with(name)).unwrap().1;
        serde_json::from_str::<part_lib::PartDef>(text).unwrap().symbol()
    }

    /// A glyph chosen by hand wins over the guess; "box" forces the plain box; a glyph the pins do not suit is reported.
    #[test]
    fn a_chosen_glyph_wins_over_the_guess() {
        let mut d: part_lib::PartDef = serde_json::from_str(part_lib::EXAMPLES.iter().find(|(n, _)| n.starts_with("LM358")).unwrap().1).unwrap();
        assert_eq!(body_glyph(&d.symbol()), Some("opamp"));
        d.glyph = "box".into();
        assert!(!d.has_errors());
        assert_eq!((body_glyph(&d.symbol()), overlay_glyph(&d.symbol())), (None, None));
        d.glyph = "comparator".into();
        assert_eq!(body_glyph(&d.symbol()), Some("comparator"));
        assert!(glyph_fits("opamp", &d.symbol()) && glyph_fits("gate_and", &d.symbol()));
        assert!(!glyph_fits("npn", &d.symbol()) && !glyph_fits("resistor", &d.symbol()));
    }

    /// The gate ICs, the three-terminal regulators and the comparator get their marks from the name and pins alone.
    #[test]
    fn gates_regulators_and_comparators_are_recognised() {
        for (part, g) in [("74HC00", "gate_nand"), ("74HC02", "gate_nor"), ("74HC04", "gate_not"), ("74HC08", "gate_and"), ("74HC14_DIP", "gate_schmitt"), ("74HC32_DIP", "gate_or"), ("74HC86", "gate_xor"), ("74HC74", "ff_d"), ("74HC4017_DIP", "counter"), ("74HC595", "shift_reg"), ("LM7805", "regulator"), ("AMS1117-3.3", "regulator"), ("L78L05", "regulator"), ("MCP1700", "regulator")] {
            assert_eq!(overlay_glyph(&sym(part)), Some(g), "{part}");
        }
        assert_eq!(body_glyph(&sym("LM393")), Some("comparator"));
        assert_eq!(body_glyph(&sym("LM358")), Some("opamp"));
        // a mark never sticks out of the box
        for g in ["regulator", "gate_and", "gate_or", "gate_not", "gate_nand", "gate_nor", "gate_xor", "gate_schmitt", "gate_buffer", "ff_d", "counter", "shift_reg", "seg7"] {
            let l = sym("74HC00").layout();
            for s in overlay(g, &l) {
                let pts = match s {
                    L(a, b, c, d) => vec![(a, b), (c, d)],
                    C(x, y, r) => vec![(x - r, y), (x + r, y)],
                };
                assert!(pts.iter().all(|p| p.0.abs() <= l.hw as f32 && p.1.abs() <= l.hh as f32), "{g} sticks out");
            }
        }
    }
}
