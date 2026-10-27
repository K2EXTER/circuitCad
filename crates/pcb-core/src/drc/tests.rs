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

    /// A hand-drawn pour of another net than GND joins the pads it only overlaps (their centres can lie outside its outline).
    #[test]
    fn a_pour_joins_pads_that_stick_out_of_its_outline() {
        let mut b = board();
        for (x, y) in [(10.0, 9.9), (14.0, 10.1)] {
            let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
            p.net = "BAT".into();
            b.pads.push(p);
        }
        let poly = [(9.0, 10.0), (15.0, 10.0), (15.0, 12.0), (9.0, 12.0)].map(|(x, y)| Point::new(x, y));
        b.zones.push(Zone::new("BAT", Side::Top, poly.to_vec()));
        let c = connectivity(&b);
        assert_eq!(c.pad_group(0), c.pad_group(1), "one piece of copper through the pour");
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
        b.vias.push(Via { pos: Point::new(15.0, 10.0), diameter: 0.6, drill: 0.3, net: String::new() });
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
        b.texts.push(SilkText { side: Side::Top, pos: Point::new(5.0, 5.0), height: 0.5, text: "R1".into(), refdes: "R1".into(), rot: 0 });
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
        // a track of another net cuts the pour in two: the far piece touches no GND pad, it is an island, which is always left out: the
        // far piece is not drawn, and nothing is reported
        b.tracks.push(Track { side: Side::Top, a: Point::new(18.0, 4.0), b: Point::new(18.0, 16.0), width: 0.25 });
        let conn = connectivity(&b);
        assert_eq!(crate::zonefill::zone_fill(&b, &b.zones[0], &conn).len(), 1);
        assert!(zone_checks(&b).is_empty(), "{:?}", zone_checks(&b));
        // a pour that is only a thin strip is a sliver
        let mut b = board();
        let mut strip_pad = Pad::new(Point::new(10.0, 5.06), PadShape::Rect { w: 0.08, h: 0.08 }, None, Side::Top);
        strip_pad.net = "GND".into();
        b.pads.push(strip_pad); // a pad of the net keeps the strip (a pour that touches nothing of its net is left out)
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
            big.vias.push(Via { pos: Point::new(16.0, 11.0 + dy), diameter: 0.6, drill: 0.3, net: String::new() });
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
            b.vias.push(Via { pos: Point::new(16.0, 11.0 + dy), diameter: 0.6, drill: 0.3, net: String::new() });
        }
        assert_eq!(thermal_via_checks(&b).len(), 1, "three are not enough");
        b.vias.push(Via { pos: Point::new(16.0, 12.8), diameter: 0.6, drill: 0.3, net: String::new() });
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
        b.vias.push(Via { pos: corner, diameter: 0.6, drill: 0.3, net: String::new() });
        assert!(acute_angle_checks(&b).is_empty());
    }

    #[test]
    fn supply_and_ground_net_names_are_recognised() {
        for n in ["VCC", "VDD", "VBUS", "+5V", "V+", "5V", "3V3", "V5", "V12", "V3V3", "vcc"] {
            assert!(is_supply_net(n), "{n}");
        }
        for n in ["GND", "VIN", "VOUT", "CLK", "N$1", ""] {
            assert!(!is_supply_net(n), "{n}");
        }
        for n in ["GND", "AGND", "VSS", "0"] {
            assert!(is_ground_net(n), "{n}");
        }
    }

    #[test]
    fn an_input_tied_to_the_supply_is_not_a_supply_pin() {
        // U1's pin 1 is VCC (the cap is next to it); pin 3 (a preset input) is tied to VCC as well, far from the cap
        let mut b = ic_board(Some(12.0));
        b.pads[2].net = "VCC".into();
        b.pads[2].pos = Point::new(30.0, 30.0);
        assert!(decoupling_checks(&b).is_empty(), "{:?}", decoupling_checks(&b));
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
    fn a_negative_supply_pin_needs_a_bypass_capacitor_too() {
        let mut b = ic_board(Some(12.0));
        // U1 pin 5 is on VNEG; the only capacitor is on VCC–GND, so the negative rail has none
        b.pads[4].net = "VNEG".into();
        let v = decoupling_checks(&b);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("U1.5") && v[0].message.contains("VNEG"), "{}", v[0].message);
        // a VNEG–GND capacitor next to it settles it
        b.parts.push(Part { refdes: "C2".into(), value: "100n".into(), footprint: "C_0805".into(), pos: Point::new(12.0, 10.0), rot: 0, side: Side::Top });
        for (pin, net, dx) in [("1", "VNEG", 0.0), ("2", "GND", 2.0)] {
            let mut p = Pad::new(Point::new(12.0 + dx, 10.0), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
            p.refdes = "C2".into();
            p.pin = pin.into();
            p.net = net.into();
            b.pads.push(p);
        }
        assert!(decoupling_checks(&b).is_empty(), "{:?}", decoupling_checks(&b));
        for n in ["VNEG", "VEE", "V-", "-5V", "-12V"] {
            assert!(is_negative_supply_net(n), "{n}");
        }
        for n in ["VCC", "GND", "VSS", "VIN"] {
            assert!(!is_negative_supply_net(n), "{n}");
        }
    }

    #[test]
    fn a_capacitor_between_two_supplies_bypasses_an_ic_that_runs_between_them() {
        // a single-supply IC whose 0 V is the negative rail: U1 pin 1 is VCC and pin 5 is VNEG; the capacitor sits between VCC and VNEG
        let mut b = ic_board(Some(12.0));
        b.pads[4].net = "VNEG".into();
        for p in b.pads.iter_mut().filter(|p| p.refdes == "C1" && p.pin == "2") {
            p.net = "VNEG".into();
        }
        let v = decoupling_checks(&b);
        assert!(v.is_empty(), "{v:?}");
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

/// `violations_of_added` finds the same trouble as the whole-board `check` does for a route put on a clean board.
#[cfg(test)]
mod added_tests {
    use super::*;

    fn pad(b: &mut Board, x: f64, y: f64, net: &str, th: bool) {
        let shape = if th { PadShape::Circle { d: 1.6 } } else { PadShape::Rect { w: 1.0, h: 1.0 } };
        let mut p = Pad::new(Point::new(x, y), shape, th.then_some(0.8), Side::Top);
        p.net = net.into();
        b.pads.push(p);
    }

    fn base() -> Board {
        let mut b = Board::new("t", 40.0, 30.0);
        pad(&mut b, 5.0, 10.0, "A", false);
        pad(&mut b, 25.0, 10.0, "A", false);
        pad(&mut b, 15.0, 11.0, "B", false); // another net between them, a little off the straight way
        pad(&mut b, 15.0, 20.0, "C", true);
        b.tracks.push(Track { side: Side::Top, a: Point::new(30.0, 25.0), b: Point::new(35.0, 25.0), width: 0.25 });
        b
    }

    /// Add `tracks` and `vias` to the board and ask both ways.
    fn both(tracks: Vec<Track>, vias: Vec<Via>) -> (bool, bool) {
        let b = base();
        assert!(check(&b).is_empty(), "{:?}", check(&b));
        let mut t = b.clone();
        let (nt, nv) = (t.tracks.len(), t.vias.len());
        t.tracks.extend(tracks);
        t.vias.extend(vias);
        let conn = connectivity(&t);
        (violations_of_added(&t, nt, nv, &conn) > 0, !check(&t).is_empty())
    }

    fn tr(a: (f64, f64), z: (f64, f64), w: f64) -> Track {
        Track { side: Side::Top, a: Point::new(a.0, a.1), b: Point::new(z.0, z.1), width: w }
    }

    #[test]
    fn the_quick_check_agrees_with_the_full_one() {
        let via = |x: f64, y: f64, d: f64, drill: f64| Via { pos: Point::new(x, y), diameter: d, drill, net: String::new() };
        let cases: Vec<(&str, Vec<Track>, Vec<Via>)> = vec![
            ("a clear route", vec![tr((5.0, 10.0), (5.0, 5.0), 0.25), tr((5.0, 5.0), (25.0, 5.0), 0.25), tr((25.0, 5.0), (25.0, 10.0), 0.25)], vec![]),
            ("through another net's pad", vec![tr((5.0, 10.0), (25.0, 10.0), 0.25), tr((15.0, 10.0), (15.0, 11.0), 0.25)], vec![]),
            ("too close to another net's pad", vec![tr((10.0, 10.3), (20.0, 10.3), 0.25)], vec![]),
            ("too close to another net's track", vec![tr((29.0, 25.35), (36.0, 25.35), 0.25)], vec![]),
            ("joining two nets", vec![tr((5.0, 10.0), (15.0, 11.0), 0.25)], vec![]),
            ("too narrow", vec![tr((5.0, 5.0), (10.0, 5.0), 0.05)], vec![]),
            ("end outside the board", vec![tr((5.0, 5.0), (-1.0, 5.0), 0.25)], vec![]),
            ("too close to the edge", vec![tr((5.0, 0.1), (10.0, 0.1), 0.25)], vec![]),
            ("a via touching a pad (it joins it)", vec![], vec![via(15.0, 20.0, 0.6, 0.3)]),
            ("a via too small a drill", vec![], vec![via(20.0, 20.0, 0.6, 0.1)]),
            ("a via with too thin a ring", vec![], vec![via(20.0, 20.0, 0.4, 0.3)]),
            ("a via outside the board", vec![], vec![via(41.0, 20.0, 0.6, 0.3)]),
            ("a clear via and track", vec![tr((20.0, 20.0), (20.0, 25.0), 0.25)], vec![via(20.0, 20.0, 0.6, 0.3)]),
        ];
        for (what, tracks, vias) in cases {
            let (quick, full) = both(tracks, vias);
            // the cases that must be found are found
            if what.starts_with("joining") || what.starts_with("through") || what.starts_with("too") || what.starts_with("end") || what.starts_with("a via too") || what.starts_with("a via with") || what.starts_with("a via outside") {
                assert!(full, "{what}: the full check should find something");
            }
            assert_eq!(quick, full, "{what}: the quick check says {quick}, the full one {full}");
        }
    }
}

/// The sweep that skips far-apart items gives the groups and the clearance findings of trying every pair.
#[cfg(test)]
mod sweep_tests {
    use super::*;

    /// A board of random copper (the same every time).
    fn scattered() -> Board {
        let mut seed = 0x1234_5678_9ABC_DEF0u64;
        let mut next = move |lo: f64, hi: f64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            lo + (seed >> 11) as f64 / (1u64 << 53) as f64 * (hi - lo)
        };
        let mut b = Board::new("t", 30.0, 30.0);
        for k in 0..90 {
            let shape = if k % 3 == 0 { PadShape::Circle { d: 1.2 } } else { PadShape::Rect { w: 1.0, h: 0.8 } };
            let mut p = Pad::new(Point::new(next(1.0, 29.0), next(1.0, 29.0)), shape, (k % 3 == 0).then_some(0.6), Side::Top);
            p.net = format!("N{}", k % 17);
            b.pads.push(p);
        }
        for k in 0..140 {
            let a = Point::new(next(1.0, 29.0), next(1.0, 29.0));
            let z = Point::new((a.x + next(-4.0, 4.0)).clamp(0.5, 29.5), (a.y + next(-4.0, 4.0)).clamp(0.5, 29.5));
            b.tracks.push(Track { side: if k % 4 == 0 { Side::Bottom } else { Side::Top }, a, b: z, width: 0.25 });
        }
        for _ in 0..25 {
            b.vias.push(Via { pos: Point::new(next(1.0, 29.0), next(1.0, 29.0)), diameter: 0.6, drill: 0.3, net: String::new() });
        }
        b
    }

    #[test]
    fn the_groups_are_those_of_trying_every_pair() {
        let b = scattered();
        let items = copper_items(&b);
        // every pair
        let mut parent: Vec<usize> = (0..items.len()).collect();
        for i in 0..items.len() {
            for j in (i + 1)..items.len() {
                if items[i].layers.iter().any(|&s| items[j].on(s)) && dist(&items[i].geo, &items[j].geo) <= 1e-9 {
                    let (x, y) = (find(&mut parent, i), find(&mut parent, j));
                    parent[x] = y;
                }
            }
        }
        let slow: Vec<usize> = (0..items.len()).map(|i| find(&mut parent, i)).collect();
        let fast = groups(&items);
        // the same partition (the numbers of the groups may differ)
        for i in 0..items.len() {
            for j in (i + 1)..items.len() {
                assert_eq!(slow[i] == slow[j], fast[i] == fast[j], "items {i} and {j}");
            }
        }
        assert!(slow.iter().collect::<std::collections::HashSet<_>>().len() < items.len(), "some copper must touch for this to say anything");
    }

    #[test]
    fn the_clearance_findings_are_those_of_trying_every_pair() {
        for gap in [0.15, 0.5, 1.2] {
            let mut b = scattered();
            b.rules.min_clearance = gap;
            let items = copper_items(&b);
            let grp = connectivity(&b).items;
            let mut slow = 0;
            for side in b.copper_layers() {
                let on: Vec<usize> = (0..items.len()).filter(|&i| items[i].on(side)).collect();
                for (k, &i) in on.iter().enumerate() {
                    for &j in &on[k + 1..] {
                        if grp[i] != grp[j] && dist(&items[i].geo, &items[j].geo) + 1e-9 < gap {
                            slow += 1;
                        }
                    }
                }
            }
            // clearance() also reports the copper near the edge: count those separately
            let edge = clearance_edge_findings(&b);
            assert_eq!(clearance(&b).len(), slow + edge, "clearance {gap}");
            assert!(slow > 0 || gap < 0.2, "clearance {gap}: the board must have findings to compare");
        }
    }

    #[test]
    fn the_patch_check_is_that_of_trying_all_the_copper() {
        let b = scattered();
        let conn = connectivity(&b);
        let items = copper_items(&b);
        let check = PatchCheck::new(&b);
        let (mut clear, mut blocked) = (0, 0);
        // little squares of copper laid on the board, on both sides
        for k in 0..400 {
            let (x, y) = (1.0 + (k % 20) as f64 * 1.4, 1.0 + (k / 20) as f64 * 1.4);
            let outline = [Point::new(x, y), Point::new(x + 0.6, y), Point::new(x + 0.6, y + 0.6), Point::new(x, y + 0.6)];
            let side = if k % 3 == 0 { Side::Bottom } else { Side::Top };
            let track = k % b.tracks.len();
            // every item
            let own = conn.items[track];
            let slow = items.iter().enumerate().all(|(i, it)| {
                if conn.items[i] == own || !it.on(side) {
                    return true;
                }
                if inside_polygon(&outline, center(&it.geo)) {
                    return false;
                }
                (0..4).all(|e| dist(&Geo::Cap { a: outline[e], b: outline[(e + 1) % 4], r: 0.0 }, &it.geo) + 1e-9 >= b.rules.min_clearance)
            });
            assert_eq!(check.clear(&b, &conn, side, &outline, track), slow, "square {k}");
            assert_eq!(patch_clear(&b, &conn, side, &outline, track), slow, "square {k}");
            if slow { clear += 1 } else { blocked += 1 }
        }
        assert!(clear > 20 && blocked > 20, "{clear} clear, {blocked} blocked: the squares must tell both apart");
    }

    /// The findings about the copper near the board's edge, as `clearance` counts them.
    fn clearance_edge_findings(b: &Board) -> usize {
        let items = copper_items(b);
        let n = b.outline.len();
        items
            .iter()
            .filter(|it| (0..n).any(|e| dist(&it.geo, &Geo::Cap { a: b.outline[e], b: b.outline[(e + 1) % n], r: 0.0 }) + 1e-9 < b.rules.edge_clearance))
            .count()
    }
}

/// Narrow gaps inside one net: found where a fab's check would find them, and not where the copper is one piece.
#[cfg(test)]
mod slit_tests {
    use super::*;

    fn tr(a: (f64, f64), z: (f64, f64), w: f64) -> Track {
        Track { side: Side::Top, a: Point::new(a.0, a.1), b: Point::new(z.0, z.1), width: w }
    }

    /// A wide track (0.5) with a thin one (0.25) beside it, joined by a short piece at the right end (so one net), the
    /// thin one `centres` mm from the wide one's centre line.
    fn branch(centres: f64) -> Board {
        let mut b = Board::new("t", 40.0, 30.0);
        b.tracks.push(tr((5.0, 10.0), (25.0, 10.0), 0.5));
        b.tracks.push(tr((5.0, 10.0 + centres), (20.0, 10.0 + centres), 0.25));
        b.tracks.push(tr((20.0, 10.0 + centres), (22.0, 10.25), 0.25));
        b
    }

    #[test]
    fn a_thin_track_beside_a_wide_one_of_its_own_net_too_close_is_a_slit_and_check_does_not_see_it() {
        // 0.5 mm from centre to centre: the gap between the edges is 0.5 - 0.25 - 0.125 = 0.125 < 0.15
        let b = branch(0.5);
        assert!(check(&b).is_empty(), "one group: check has nothing to say");
        let slits = slit_checks(&b);
        assert!(!slits.is_empty());
        assert!(slits[0].message.contains("0.125"), "{}", slits[0].message);
        // the gap's place is between the two tracks
        assert!(slits.iter().all(|s| s.at.y > 10.25 && s.at.y < 10.5), "{:?}", slits.iter().map(|s| s.at).collect::<Vec<_>>());
        // far enough: no slit
        assert!(slit_checks(&branch(0.6)).is_empty());
        // touching: no gap at all
        assert!(slit_checks(&branch(0.375)).is_empty());
    }

    #[test]
    fn a_piece_in_the_middle_that_fills_the_gap_makes_no_slit() {
        // three pieces of one straight run, the middle one short: the first and the last are 0.1 mm from each other
        // (their round ends), but the middle one fills the way between
        let mut b = Board::new("t", 40.0, 30.0);
        b.tracks.push(tr((5.0, 10.0), (10.0, 10.0), 0.25));
        b.tracks.push(tr((10.0, 10.0), (10.35, 10.0), 0.25));
        b.tracks.push(tr((10.35, 10.0), (15.0, 10.0), 0.25));
        assert!(slit_checks(&b).is_empty(), "{:?}", slit_checks(&b));
    }

    #[test]
    fn the_two_legs_of_a_hairpin_too_close_are_a_slit_and_other_nets_are_left_to_check() {
        let mut b = Board::new("t", 40.0, 30.0);
        b.tracks.push(tr((5.0, 10.0), (15.0, 10.0), 0.25));
        b.tracks.push(tr((15.0, 10.0), (15.0, 10.35), 0.25));
        b.tracks.push(tr((15.0, 10.35), (5.0, 10.35), 0.25)); // 0.35 - 0.25 = 0.10 between the legs
        assert!(!slit_checks(&b).is_empty());
        // a track of another net (not touching) that close is for `check`, not a slit
        let mut c = Board::new("t", 40.0, 30.0);
        c.tracks.push(tr((5.0, 10.0), (15.0, 10.0), 0.25));
        c.tracks.push(tr((5.0, 10.35), (15.0, 10.35), 0.25));
        assert!(slit_checks(&c).is_empty());
        assert_eq!(clearance(&c).len(), 1);
    }
}
