use super::*;

#[cfg(test)]
mod drc_tests {
    use super::*;

    fn track(side: Side, a: (f64, f64), b: (f64, f64), w: f64) -> Track {
        Track { side, a: Point::new(a.0, a.1), b: Point::new(b.0, b.1), width: w }
    }

    fn board() -> Board {
        Board::new("t", 40.0, 40.0)
    }

    #[test]
    fn parallel_tracks_too_close() {
        let mut b = board();
        // gap = 0.35 - 0.25 = 0.10 < 0.15
        b.tracks.push(track(Side::Top, (10.0, 10.0), (20.0, 10.0), 0.25));
        b.tracks.push(track(Side::Top, (10.0, 10.35), (20.0, 10.35), 0.25));
        assert_eq!(clearance(&b).len(), 1);
    }

    #[test]
    fn parallel_tracks_far_enough() {
        let mut b = board();
        b.tracks.push(track(Side::Top, (10.0, 10.0), (20.0, 10.0), 0.25));
        b.tracks.push(track(Side::Top, (10.0, 10.5), (20.0, 10.5), 0.25)); // gap 0.25
        assert!(clearance(&b).is_empty());
    }

    #[test]
    fn different_layers_do_not_interact() {
        let mut b = board();
        b.tracks.push(track(Side::Top, (10.0, 10.0), (20.0, 10.0), 0.25));
        b.tracks.push(track(Side::Bottom, (10.0, 10.1), (20.0, 10.1), 0.25));
        assert!(clearance(&b).is_empty());
    }

    #[test]
    fn touching_items_are_one_net() {
        let mut b = board();
        b.tracks.push(track(Side::Top, (10.0, 10.0), (20.0, 10.0), 0.25));
        b.tracks.push(track(Side::Top, (20.0, 10.0), (20.0, 20.0), 0.25));
        assert!(clearance(&b).is_empty());
    }

    #[test]
    fn via_joins_layers_so_crossing_other_net_is_flagged() {
        let mut b = board();
        // top track A -> via -> bottom track A'; bottom track B passes 0.05 from the via ring
        b.tracks.push(track(Side::Top, (10.0, 10.0), (15.0, 10.0), 0.25));
        b.vias.push(Via { pos: Point::new(15.0, 10.0), diameter: 0.6, drill: 0.3 });
        b.tracks.push(track(Side::Bottom, (15.0, 10.0), (15.0, 20.0), 0.25));
        b.tracks.push(track(Side::Bottom, (15.45, 5.0), (15.45, 12.0), 0.2));
        // gap between via edge (x=15.3) and B edge (x=15.35) is 0.05
        assert_eq!(clearance(&b).len(), 1);
    }

    #[test]
    fn rect_pad_to_track() {
        let mut b = board();
        b.pads.push(Pad::new(Point::new(10.0, 10.0), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top));
        // pad edge x=10.5, track edge x=10.6 -> 0.1
        b.tracks.push(track(Side::Top, (10.725, 5.0), (10.725, 15.0), 0.25));
        assert_eq!(clearance(&b).len(), 1);
        b.tracks[0] = track(Side::Top, (10.85, 5.0), (10.85, 15.0), 0.25); // gap 0.225
        assert!(clearance(&b).is_empty());
    }

    #[test]
    fn copper_near_board_edge() {
        let mut b = board();
        b.tracks.push(track(Side::Top, (0.3, 10.0), (0.3, 20.0), 0.25)); // edge gap ~0.175 < 0.3
        assert_eq!(clearance(&b).len(), 1);
    }

    #[test]
    fn silk_over_a_pad_thin_silk_and_tiny_text_are_flagged() {
        let mut b = board();
        b.pads.push(Pad::new(Point::new(10.0, 10.0), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top));
        b.silk.push(SilkLine { side: Side::Top, a: Point::new(9.0, 10.0), b: Point::new(11.0, 10.0), width: 0.15, refdes: String::new() });
        // far away and on the other side: fine
        b.silk.push(SilkLine { side: Side::Top, a: Point::new(20.0, 10.0), b: Point::new(22.0, 10.0), width: 0.15, refdes: String::new() });
        b.silk.push(SilkLine { side: Side::Bottom, a: Point::new(9.0, 10.0), b: Point::new(11.0, 10.0), width: 0.15, refdes: String::new() });
        assert_eq!(silk_checks(&b).len(), 1, "{:?}", silk_checks(&b));
        b.silk.push(SilkLine { side: Side::Top, a: Point::new(30.0, 10.0), b: Point::new(32.0, 10.0), width: 0.08, refdes: String::new() });
        b.texts.push(SilkText { side: Side::Top, pos: Point::new(5.0, 5.0), height: 0.5, text: "R1".into(), refdes: "R1".into() });
        let v = silk_checks(&b);
        assert_eq!(v.len(), 3, "{v:?}");
        assert!(v.iter().any(|x| x.message.contains("パッドに重なって")) && v.iter().any(|x| x.message.contains("細すぎ")) && v.iter().any(|x| x.message.contains("小さすぎ")));
    }

    #[test]
    fn narrow_mask_dams_between_different_nets_are_flagged() {
        let pad = |x: f64, net: &str| {
            let mut p = Pad::new(Point::new(x, 10.0), PadShape::Rect { w: 0.6, h: 1.0 }, None, Side::Top);
            p.net = net.into();
            p
        };
        let mut b = board();
        // gap 0.2 mm, mask opens 0.05 on each side: a 0.1 mm dam is just enough
        b.pads = vec![pad(10.0, "A"), pad(10.8, "B")];
        assert!(mask_dam_checks(&b).is_empty());
        // gap 0.15 mm: the dam is 0.05 mm
        b.pads = vec![pad(10.0, "A"), pad(10.75, "B")];
        let v = mask_dam_checks(&b);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("0.05mm") && v[0].message.contains("剥がれ"));
        // gap 0.08 mm: the openings merge
        b.pads = vec![pad(10.0, "A"), pad(10.68, "B")];
        assert!(mask_dam_checks(&b)[0].message.contains("つながり"));
        // the same net does not need a dam; pads on opposite sides do not share a mask
        b.pads = vec![pad(10.0, "A"), pad(10.75, "A")];
        assert!(mask_dam_checks(&b).is_empty());
        let mut other_side = pad(10.75, "B");
        other_side.side = Side::Bottom;
        b.pads = vec![pad(10.0, "A"), other_side];
        assert!(mask_dam_checks(&b).is_empty());
    }

    /// The footprints that come with the program keep the mask strip between their own pads (a false alarm on
    /// every board that uses them would teach the wrong lesson).
    #[test]
    fn built_in_footprints_have_mask_dams() {
        let mut bad = vec![];
        for name in crate::footprints::names() {
            let Some(fp) = crate::footprints::get(&name) else { continue };
            let mut b = board();
            for (i, p) in fp.pads.iter().enumerate() {
                let mut pad = Pad::new(Point::new(20.0 + p.pos.x, 20.0 + p.pos.y), p.shape, p.drill, Side::Top);
                pad.refdes = "U1".into();
                pad.pin = p.pin.clone();
                pad.net = format!("N{i}");
                b.pads.push(pad);
            }
            if let Some(first) = mask_dam_checks(&b).first() {
                bad.push(format!("{name}: {}", first.message));
            }
        }
        assert!(bad.is_empty(), "{bad:#?}");
    }

    #[test]
    fn pour_slivers_and_islands_are_found() {
        let mut b = board();
        let z = |x0: f64, y0: f64, x1: f64, y1: f64| Zone::new("GND", Side::Top, vec![Point::new(x0, y0), Point::new(x1, y0), Point::new(x1, y1), Point::new(x0, y1)]);
        // a plain pour with a GND pad in it: nothing to report
        let mut gnd = Pad::new(Point::new(10.0, 10.0), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
        gnd.net = "GND".into();
        b.pads.push(gnd);
        b.zones.push(z(5.0, 5.0, 25.0, 15.0));
        assert!(zone_checks(&b).is_empty(), "{:?}", zone_checks(&b));
        // a track of another net cuts the pour in two: the far piece touches no GND pad, it is an island
        b.tracks.push(Track { side: Side::Top, a: Point::new(18.0, 4.0), b: Point::new(18.0, 16.0), width: 0.25 });
        let v = zone_checks(&b);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("島"), "{v:?}");
        // with islands removed the far piece is not drawn, and nothing is reported
        b.zones[0].remove_islands = true;
        let conn = connectivity(&b);
        assert_eq!(crate::zonefill::zone_fill(&b, &b.zones[0], &conn).len(), 1);
        assert!(zone_checks(&b).is_empty(), "{:?}", zone_checks(&b));
        // a pour that is only a thin strip is a sliver
        let mut b = board();
        b.zones.push(z(5.0, 5.0, 25.0, 5.12));
        let v = zone_checks(&b);
        assert!(v.iter().any(|x| x.message.contains("スリバー")), "{v:?}");
    }

    #[test]
    fn thermal_vias_are_placed_in_the_heat_pad() {
        let mut b = board();
        for (x, y, w, h, pin) in [(10.0, 10.0, 2.0, 1.5, "1"), (10.0, 12.3, 2.0, 1.5, "2"), (16.0, 11.0, 2.0, 3.8, "3")] {
            let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w, h }, None, Side::Top);
            p.refdes = "U1".into();
            p.pin = pin.into();
            b.pads.push(p);
        }
        assert_eq!(thermal_via_checks(&b).len(), 1);
        let before = check(&b).len();
        let n = add_thermal_vias(&mut b);
        assert!(n >= 4, "added {n}");
        assert!(thermal_via_checks(&b).is_empty(), "{:?}", thermal_via_checks(&b));
        assert!(check(&b).len() <= before, "the new vias must not add DRC errors: {:?}", check(&b));
        // nothing more to add the second time
        assert_eq!(add_thermal_vias(&mut b), 0);
    }

    #[test]
    fn a_big_pour_halves_the_thermal_vias_but_a_small_one_does_not() {
        let tab_board = || {
            let mut b = board();
            for (x, y, w, h, pin, net) in [(10.0, 10.0, 2.0, 1.5, "1", "VIN"), (10.0, 12.3, 2.0, 1.5, "2", "VOUT"), (16.0, 11.0, 2.0, 3.8, "3", "GND")] {
                let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w, h }, None, Side::Top);
                p.refdes = "U1".into();
                p.pin = pin.into();
                p.net = net.into();
                b.pads.push(p);
            }
            b
        };
        let rect = |x0: f64, y0: f64, x1: f64, y1: f64| vec![Point::new(x0, y0), Point::new(x1, y0), Point::new(x1, y1), Point::new(x0, y1)];
        // no pour: 7.6 mm2 asks for 4
        let b = tab_board();
        assert!(thermal_via_checks(&b)[0].message.contains("4 個以上") && thermal_via_checks(&b)[0].message.contains("接しているベタ: なし"));
        // a 20 x 20 mm GND pour around the tab: half, so 2 vias are enough
        let mut big = tab_board();
        big.zones.push(Zone::new("GND", Side::Top, rect(14.0, 5.0, 34.0, 25.0)));
        let msg = thermal_via_checks(&big)[0].message.clone();
        assert!(msg.contains("2 個以上") && msg.contains("半分"), "{msg}");
        for dy in [-0.9, 0.9] {
            big.vias.push(Via { pos: Point::new(16.0, 11.0 + dy), diameter: 0.6, drill: 0.3 });
        }
        assert!(thermal_via_checks(&big).is_empty(), "{:?}", thermal_via_checks(&big));
        // a 6 x 6 mm pour (an island around the tab) is too small to spread heat: the full number stays
        let mut small = tab_board();
        small.zones.push(Zone::new("GND", Side::Top, rect(14.0, 8.0, 20.0, 14.0)));
        let msg = thermal_via_checks(&small)[0].message.clone();
        assert!(msg.contains("4 個以上") && msg.contains("足りず"), "{msg}");
        // a GND pour on another net's tab does not count
        let mut other = tab_board();
        other.zones.push(Zone::new("VIN", Side::Top, rect(14.0, 5.0, 34.0, 25.0)));
        assert!(thermal_via_checks(&other)[0].message.contains("接しているベタ: なし"));
        // the auto-placer follows the same count
        assert_eq!(add_thermal_vias(&mut big), 0, "already enough with the pour");
    }

    #[test]
    fn a_heat_pad_needs_thermal_vias() {
        let mut b = board();
        // a small 3-pin part with a big tab (like a SOT-223)
        for (x, y, w, h, pin) in [(10.0, 10.0, 2.0, 1.5, "1"), (10.0, 12.3, 2.0, 1.5, "2"), (16.0, 11.0, 2.0, 3.8, "3")] {
            let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w, h }, None, Side::Top);
            p.refdes = "U1".into();
            p.pin = pin.into();
            b.pads.push(p);
        }
        assert_eq!(heat_pads(&b).len(), 1, "only the tab is a heat pad");
        let v = thermal_via_checks(&b);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("U1.3") && v[0].message.contains("0 個"), "{v:?}");
        // vias in the tab: 7.6 mm2 asks for 4
        for dy in [-1.2, -0.4, 0.4] {
            b.vias.push(Via { pos: Point::new(16.0, 11.0 + dy), diameter: 0.6, drill: 0.3 });
        }
        assert_eq!(thermal_via_checks(&b).len(), 1, "three are not enough");
        b.vias.push(Via { pos: Point::new(16.0, 12.8), diameter: 0.6, drill: 0.3 });
        assert!(thermal_via_checks(&b).is_empty());
        // a plain two-pad resistor with big pads is not a heat pad
        let mut c = board();
        for (x, pin) in [(10.0, "1"), (14.0, "2")] {
            let mut p = Pad::new(Point::new(x, 10.0), PadShape::Rect { w: 1.5, h: 3.0 }, None, Side::Top);
            p.refdes = "R1".into();
            p.pin = pin.into();
            c.pads.push(p);
        }
        assert!(heat_pads(&c).is_empty());
    }

    #[test]
    fn a_sharp_corner_under_a_pad_or_via_is_not_an_acid_trap() {
        let mut b = board();
        let corner = Point::new(10.0, 10.0);
        b.tracks.push(track(Side::Top, (10.0, 10.0), (15.0, 10.0), 0.25));
        b.tracks.push(track(Side::Top, (10.0, 10.0), (14.0, 12.0), 0.25));
        assert_eq!(acute_angle_checks(&b).len(), 1, "in the open it is reported");
        b.pads.push(Pad::new(corner, PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top));
        assert!(acute_angle_checks(&b).is_empty(), "under a pad it is filled with copper");
        // a pad on the other side does not cover it ...
        b.pads[0].side = Side::Bottom;
        assert_eq!(acute_angle_checks(&b).len(), 1);
        // ... unless it goes through the board; a via covers it too
        b.pads[0].drill = Some(0.6);
        assert!(acute_angle_checks(&b).is_empty());
        b.pads.clear();
        b.vias.push(Via { pos: corner, diameter: 0.6, drill: 0.3 });
        assert!(acute_angle_checks(&b).is_empty());
    }

    fn ic_board(cap_x: Option<f64>) -> Board {
        let mut b = Board::new("t", 40.0, 40.0);
        b.parts.push(Part { refdes: "U1".into(), value: "74HC00".into(), footprint: "x".into(), pos: Point::new(10.0, 10.0), rot: 0, side: Side::Top });
        for i in 0..8 {
            let mut p = Pad::new(Point::new(10.0 + i as f64 * 0.5, 10.0), PadShape::Circle { d: 1.0 }, Some(0.6), Side::Top);
            p.refdes = "U1".into();
            p.pin = (i + 1).to_string();
            p.net = match i {
                0 => "VCC".into(),
                1 => "GND".into(),
                _ => String::new(),
            };
            b.pads.push(p);
        }
        if let Some(x) = cap_x {
            b.parts.push(Part { refdes: "C1".into(), value: "100n".into(), footprint: "C_0805".into(), pos: Point::new(x, 10.0), rot: 0, side: Side::Top });
            for (pin, net, dx) in [("1", "VCC", 0.0), ("2", "GND", 2.0)] {
                let mut p = Pad::new(Point::new(x + dx, 10.0), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
                p.refdes = "C1".into();
                p.pin = pin.into();
                p.net = net.into();
                b.pads.push(p);
            }
        }
        b
    }

    #[test]
    fn supply_pin_needs_a_bypass_capacitor_nearby() {
        // no capacitor at all
        let none = decoupling_checks(&ic_board(None));
        assert_eq!(none.len(), 1, "{none:?}");
        assert!(none[0].message.contains("U1.1") && none[0].message.contains("見当たりません"));
        // a capacitor 2 mm away is fine
        assert!(decoupling_checks(&ic_board(Some(12.0))).is_empty());
        // 20 mm away is too far
        let far = decoupling_checks(&ic_board(Some(30.0)));
        assert_eq!(far.len(), 1, "{far:?}");
        assert!(far[0].message.contains("mm あります"));
    }

    #[test]
    fn acute_corners_are_flagged_but_45_degree_bends_are_not() {
        let mut b = board();
        // 135 degree interior angle (a 45 degree bend)
        b.tracks.push(track(Side::Top, (10.0, 10.0), (20.0, 10.0), 0.25));
        b.tracks.push(track(Side::Top, (20.0, 10.0), (25.0, 15.0), 0.25));
        // a plain 90 degree corner
        b.tracks.push(track(Side::Top, (30.0, 10.0), (35.0, 10.0), 0.25));
        b.tracks.push(track(Side::Top, (35.0, 10.0), (35.0, 15.0), 0.25));
        assert!(acute_angle_checks(&b).is_empty(), "{:?}", acute_angle_checks(&b));
        // 45 degree interior angle: doubling back
        b.tracks.push(track(Side::Top, (10.0, 30.0), (20.0, 30.0), 0.25));
        b.tracks.push(track(Side::Top, (20.0, 30.0), (10.0, 35.0), 0.25));
        let v = acute_angle_checks(&b);
        assert_eq!(v.len(), 1, "{v:?}");
        // the same shape on different layers does not touch
        let mut c = board();
        c.tracks.push(track(Side::Top, (10.0, 30.0), (20.0, 30.0), 0.25));
        c.tracks.push(track(Side::Bottom, (20.0, 30.0), (10.0, 35.0), 0.25));
        assert!(acute_angle_checks(&c).is_empty());
    }

    #[test]
    fn a_ground_fill_is_suggested_only_when_there_is_none() {
        let mut b = ic_board(Some(12.0));
        assert!(missing_ground_fill(&b));
        b.zones.push(Zone::new("GND", Side::Bottom, vec![Point::new(1.0, 1.0), Point::new(30.0, 1.0), Point::new(30.0, 30.0)]));
        assert!(!missing_ground_fill(&b));
    }
}

#[cfg(test)]
mod silk_outline_tests {
    use super::*;
    use crate::model::Part;

    /// A radial capacitor, a box capacitor, an axial resistor, a diode and an LED are drawn with their body
    /// (and polarity marks), in every orientation and on both sides, without silk over a pad.
    #[test]
    fn through_hole_parts_get_a_body_outline_clear_of_the_pads() {
        for name in ["CP_Radial_D5_P2.0", "C_Disc_5.08", "R_Axial_10.16", "D_DO-35_7.62", "LED_5mm"] {
            for rot in 0..4u8 {
                for side in [Side::Top, Side::Bottom] {
                    let mut b = Board::new("t", 40.0, 40.0);
                    b.set_part(Part { refdes: "X1".into(), value: "v".into(), footprint: name.into(), pos: Point::new(20.0, 20.0), rot, side }).unwrap();
                    assert!(b.silk.len() >= 4 && b.silk.iter().all(|l| l.refdes == "X1"), "{name}");
                    assert!(silk_checks(&b).is_empty(), "{name} rot {rot} {side:?}: {:?}", silk_checks(&b));
                }
            }
        }
        // the can of the electrolytic is round: many short lines, and marks for + and -
        let mut b = Board::new("t", 40.0, 40.0);
        b.set_part(Part { refdes: "C1".into(), value: "100u".into(), footprint: "CP_Radial_D5_P2.0".into(), pos: Point::new(20.0, 20.0), rot: 0, side: Side::Top }).unwrap();
        assert!(b.silk.len() > 30, "{}", b.silk.len());
        let far = b.silk.iter().map(|l| l.a.dist(Point::new(20.0, 20.0))).fold(0.0f64, f64::max);
        assert!((far - 2.7).abs() < 0.2, "the can has a radius of about 2.7 mm, got {far}");
        // a footprint saved without the outline (a library part, an old board) still gets the built-in one
        let mut b = Board::new("t", 40.0, 40.0);
        let mut fp = crate::footprints::get("CP_Radial_D5_P2.0").unwrap();
        fp.silk.clear();
        b.add_footprint(fp);
        b.set_part(Part { refdes: "C1".into(), value: "100u".into(), footprint: "CP_Radial_D5_P2.0".into(), pos: Point::new(20.0, 20.0), rot: 0, side: Side::Top }).unwrap();
        assert!(b.silk.len() > 30);
        // a chip part still gets the box around its pads
        let mut b = Board::new("t", 40.0, 40.0);
        b.set_part(Part { refdes: "R1".into(), value: "1k".into(), footprint: "R_0805".into(), pos: Point::new(20.0, 20.0), rot: 0, side: Side::Top }).unwrap();
        assert_eq!(b.silk.len(), 4);
    }
}
