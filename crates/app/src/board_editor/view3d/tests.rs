use super::*;

#[cfg(test)]
mod view3d_tests {
    use super::*;

    fn cam(yaw: f32, pitch: f32) -> Cam {
        Cam { yaw, pitch, scale: 10.0, center: [0.0, 0.0, 0.0], dist: 300.0, origin: Pos2::new(100.0, 100.0) }
    }

    fn solid_box(shape: BodyShape) -> PartBox {
        PartBox { refdes: "U1".into(), part: 0, lo: [0.0, 0.0, 1.6], hi: [4.0, 4.0, 4.0], color: [30, 30, 30], bottom: false, shape }
    }

    #[test]
    fn straight_down_shows_the_board_as_a_map_and_higher_points_are_nearer() {
        let c = cam(0.0, std::f32::consts::FRAC_PI_2);
        let (p, w0) = c.project([0.0, 0.0, 0.0]);
        let (q, w1) = c.project([5.0, 3.0, 0.0]);
        // +x goes right, +y goes up the screen (smaller y)
        assert!(q.x > p.x && q.y < p.y);
        let (_, wz) = c.project([0.0, 0.0, 10.0]);
        assert!(wz > w0 && (w1 - w0).abs() < 1e-4);
        // the top face (normal +z) faces the viewer from above and not from below
        assert!(c.dir([0.0, 0.0, 1.0])[2] > 0.99);
        assert!(cam(0.0, -std::f32::consts::FRAC_PI_2).dir([0.0, 0.0, 1.0])[2] < -0.99);
    }

    #[test]
    fn from_the_side_the_height_is_the_vertical_axis() {
        let c = cam(0.0, 0.0);
        let (p, _) = c.project([0.0, 0.0, 0.0]);
        let (q, _) = c.project([0.0, 0.0, 4.0]);
        assert!(q.y < p.y && (q.x - p.x).abs() < 1e-4);
        // the viewer stands on the -y side: the -y wall faces them, the +y wall does not
        assert!(c.dir([0.0, -1.0, 0.0])[2] > 0.99 && c.dir([0.0, 1.0, 0.0])[2] < -0.99);
    }

    #[test]
    fn a_box_shows_three_faces_and_a_cylinder_its_visible_half() {
        let c = cam(0.5, 0.9);
        let faces = |b: &PartBox| solid_faces(b).iter().filter(|(_, n)| c.dir(*n)[2] > 1e-4).count();
        assert_eq!(faces(&solid_box(BodyShape::Box)), 3);
        // 32 side facets, about half face the viewer, plus the top cap
        let cyl = faces(&solid_box(BodyShape::Cylinder(2)));
        assert!((16..=18).contains(&cyl), "{cyl}");
        // a cylinder lying along x has a cap at each end and a ring of sides
        assert_eq!(solid_faces(&solid_box(BodyShape::Cylinder(0))).len(), 34);
        // brighter when facing the light than when turned away
        assert!(light([-0.3, 0.4, 0.85]) > light([0.9, -0.3, 0.1]));
    }

    /// The board picture lies where the parts' pads are: looking straight down, the copper of a pad is under the
    /// pad's position in the 3D scene (the picture has a margin around the outline, which the mapping must account for).
    #[test]
    fn the_board_picture_and_the_part_positions_agree() {
        let ctx = egui::Context::default();
        let mut b = Board::new("t", 40.0, 25.0);
        for (x, name) in [(8.0, "R1"), (30.0, "R2")] {
            for dx in [-1.0, 1.0] {
                let mut pad = Pad::new(Point::new(x + dx, 12.0), PadShape::Rect { w: 1.2, h: 1.6 }, None, Side::Top);
                pad.refdes = name.into();
                b.pads.push(pad);
            }
            b.parts.push(pcb_core::Part { refdes: name.into(), value: "1k".into(), footprint: "R_0805".into(), pos: Point::new(x, 12.0), rot: 0, side: Side::Top });
        }
        let s = Scene::build(&b, &ctx, false, false).unwrap();
        let (w, h) = (800usize, 500usize);
        let cam = Cam { yaw: 0.0, pitch: std::f32::consts::FRAC_PI_2, scale: 16.0, center: [20.0, 12.5, 0.8], dist: 300.0, origin: Pos2::new(w as f32 / 2.0, h as f32 / 2.0) };
        let (img, _, w, _) = s.render(&cam, w, h, false, None).resolve(1);
        let at = |x: f64, y: f64| {
            let (p, _) = cam.project([x, y, BOARD_THICKNESS]);
            img[p.y.round() as usize * w + p.x.round() as usize]
        };
        let gold = |c: [u8; 4]| c[0] > 180 && c[1] > 140 && c[2] < 120;
        for pad in &b.pads {
            assert!(gold(at(pad.pos.x, pad.pos.y)), "no copper under the pad at {:?}: {:?}", pad.pos, at(pad.pos.x, pad.pos.y));
        }
        // between the two pads of a part and away from them it is bare board
        assert!(!gold(at(8.0, 12.0)) && !gold(at(20.0, 20.0)));
        // the copper of a pad ends where the pad ends (0.6 mm from its centre, but not 1 mm)
        assert!(gold(at(7.0 + 0.5, 12.0)) && !gold(at(7.0 - 1.0, 12.0)));
    }

    fn two_resistors() -> Board {
        let mut b = Board::new("t", 40.0, 25.0);
        for (x, name) in [(8.0, "R1"), (30.0, "R2")] {
            for (dx, net) in [(-1.0, "A"), (1.0, "B")] {
                let mut pad = Pad::new(Point::new(x + dx, 12.0), PadShape::Rect { w: 1.2, h: 1.6 }, None, Side::Top);
                pad.refdes = name.into();
                pad.net = net.into();
                b.pads.push(pad);
            }
            b.parts.push(pcb_core::Part { refdes: name.into(), value: "1k".into(), footprint: "R_0805".into(), pos: Point::new(x, 12.0), rot: 0, side: Side::Top });
        }
        b.tracks.push(pcb_core::Track { side: Side::Top, a: Point::new(9.0, 12.0), b: Point::new(15.0, 12.0), width: 0.5 });
        b
    }

    fn top_view(s: &Scene) -> (Rect, Cam) {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 500.0));
        (rect, camera(s, rect, 0.0, std::f32::consts::FRAC_PI_2, 1.0, Vec2::ZERO))
    }

    #[test]
    fn a_picked_net_lights_its_copper_and_nothing_else() {
        let b = two_resistors();
        let mut s = Scene::build(&b, &egui::Context::default(), false, false).unwrap();
        assert_eq!(s.nets, vec!["A".to_string(), "B".to_string()]);
        // net B: the pads at x=9 and x=31 and the track leaving the first
        s.glow = net_glow(&b, &pcb_core::drc::connectivity(&b), "B");
        assert!(!s.glow.is_empty() && s.glow.iter().all(|(bottom, _)| !bottom));
        let (rect, cam) = top_view(&s);
        let (img, ..) = snapshot(&s, rect, &cam, false, Quality::Fine(1), None, None);
        let at = |x: f64, y: f64| {
            let (p, _) = cam.project([x, y, BOARD_THICKNESS]);
            img[p.y.round() as usize * 800 + p.x.round() as usize]
        };
        let lit = |c: [u8; 4]| c[0] > 200 && c[1] < 100 && c[2] > 120;
        assert!(lit(at(9.0, 12.0)) && lit(at(12.0, 12.0)) && lit(at(31.0, 12.0)), "pad, track, pad");
        // the other net's pads (x=7, x=29) and bare board are not lit
        assert!(!lit(at(7.0, 12.0)) && !lit(at(29.0, 12.0)) && !lit(at(20.0, 12.0)));
        s.glow.clear();
        let (img, ..) = snapshot(&s, rect, &cam, false, Quality::Fine(1), None, None);
        let (p, _) = cam.project([9.0, 12.0, BOARD_THICKNESS]);
        assert!(!lit(img[p.y.round() as usize * 800 + p.x.round() as usize]));
    }

    #[test]
    fn focusing_a_part_centres_it_and_fills_more_of_the_view() {
        let b = two_resistors();
        let s = Scene::build(&b, &egui::Context::default(), false, false).unwrap();
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 500.0));
        let (pitch, zoom, pan) = focus_on(&s, 1, rect, 0.0, 0.9).unwrap();
        assert!(zoom > 2.0, "a small part is magnified: {zoom}");
        let cam = camera(&s, rect, 0.0, pitch, zoom, pan);
        let (lo, hi) = (s.boxes.iter().filter(|b| b.part == 1).map(|b| b.lo[2]).fold(f64::MAX, f64::min), s.boxes.iter().filter(|b| b.part == 1).map(|b| b.hi[2]).fold(0.0, f64::max));
        let (p, _) = cam.project([30.0, 12.0, (lo + hi) / 2.0]);
        assert!((p.x - 400.0).abs() < 20.0 && (p.y - 250.0).abs() < 20.0, "{p:?}");
        assert!(focus_on(&s, 9, rect, 0.0, 0.9).is_none());
        // a part on the bottom is looked at from below
        let mut b2 = two_resistors();
        b2.parts[1].side = Side::Bottom;
        let s2 = Scene::build(&b2, &egui::Context::default(), false, false).unwrap();
        assert!(focus_on(&s2, 1, rect, 0.0, 0.9).unwrap().0 < 0.0);
    }

    #[test]
    fn a_snapshot_is_a_clean_picture_of_the_window_size() {
        let b = two_resistors();
        let s = Scene::build(&b, &egui::Context::default(), true, true).unwrap();
        let (rect, cam) = top_view(&s);
        let (px, ids, w, h) = snapshot(&s, rect, &cam, true, Quality::Fine(2), Some(0), Some(1));
        assert_eq!((w, h, px.len(), ids.len()), (800, 500, 400_000, 400_000));
        // hover / selected outlines are colours that only outlines use
        assert!(px.iter().any(|c| c[..3] == [255, 230, 90]));
        let (clean, ..) = snapshot(&s, rect, &cam, true, Quality::Fine(2), None, None);
        assert!(!clean.iter().any(|c| c[..3] == [255, 230, 90] || c[..3] == [90, 220, 255]));
    }

    fn tall_and_short() -> Board {
        let mut b = two_resistors();
        for (dx, pin) in [(-1.0, "1"), (1.0, "2")] {
            let mut pad = Pad::new(Point::new(20.0 + dx, 20.0), PadShape::Circle { d: 1.6 }, Some(0.8), Side::Top);
            pad.refdes = "C1".into();
            pad.pin = pin.into();
            b.pads.push(pad);
        }
        b.parts.push(pcb_core::Part { refdes: "C1".into(), value: "100u".into(), footprint: "CP_Radial_D5_P2.0".into(), pos: Point::new(20.0, 20.0), rot: 0, side: Side::Top });
        b
    }

    #[test]
    fn parts_over_the_case_height_turn_red_and_are_listed() {
        let b = tall_and_short();
        let mut s = Scene::build(&b, &egui::Context::default(), false, false).unwrap();
        // not checked: nothing is over
        assert!(s.set_limits(false, 3.0, 0.0) && s.over_list().is_empty());
        assert!(!s.set_limits(false, 3.0, 0.0), "same settings: nothing to redraw");
        // 3 mm of room above the top face: the 4.5 mm can is 1.5 mm over, the chips are not
        assert!(s.set_limits(true, 3.0, 5.0));
        let list = s.over_list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0, "C1");
        assert!((list[0].1 - 1.5).abs() < 0.01, "{:?}", list);
        assert_eq!(s.rings[0], Some(3.0));
        // a limit of 0 on a face means that face is not checked
        s.set_limits(true, 0.0, 5.0);
        assert!(s.over_list().is_empty() && s.rings[0].is_none());
        // seen from above, the can is red; with the check off it is its own colour
        s.set_limits(true, 3.0, 5.0);
        let (rect, cam) = top_view(&s);
        let (img, ..) = snapshot(&s, rect, &cam, true, Quality::Fine(1), None, None);
        let (p, _) = cam.project([20.0, 20.0, BOARD_THICKNESS + 4.5]);
        let c = img[p.y.round() as usize * 800 + p.x.round() as usize];
        assert!(c[0] > 150 && c[1] < 90, "red can: {c:?}");
        s.set_limits(false, 3.0, 5.0);
        let (img, ..) = snapshot(&s, rect, &cam, true, Quality::Fine(1), None, None);
        let c = img[p.y.round() as usize * 800 + p.x.round() as usize];
        assert!(c[1] > 90, "orange can: {c:?}");
    }

    #[test]
    fn a_draft_is_half_size_and_hover_still_finds_the_part() {
        let b = tall_and_short();
        let mut s = Scene::build(&b, &egui::Context::default(), false, false).unwrap();
        s.set_limits(false, 0.0, 0.0);
        let (rect, cam) = top_view(&s);
        let (px, ids, w, h) = snapshot(&s, rect, &cam, true, Quality::Draft, None, None);
        assert_eq!((w, h, px.len(), ids.len()), (400, 250, 100_000, 100_000));
        // the pointer over the can in the window finds the can in the half-size picture
        let (p, _) = cam.project([20.0, 20.0, BOARD_THICKNESS + 4.5]);
        let k = sample_at(rect, [w, h], p).unwrap();
        assert_eq!(ids[k], 2, "C1 is the third part");
        assert!(sample_at(rect, [w, h], Pos2::new(-5.0, 10.0)).is_none() && sample_at(rect, [w, h], Pos2::new(800.0, 10.0)).is_none());
        // the draft looks like the fine picture at the same place (the can's colour)
        let (fine, ..) = snapshot(&s, rect, &cam, true, Quality::Fine(1), None, None);
        let d = px[(p.y / 2.0) as usize * 400 + (p.x / 2.0) as usize];
        let f = fine[p.y.round() as usize * 800 + p.x.round() as usize];
        assert!((d[0] as i32 - f[0] as i32).abs() < 12 && (d[1] as i32 - f[1] as i32).abs() < 12);
    }

    #[test]
    fn the_window_builds_a_scene_with_part_info_and_the_tallest_part() {
        let ctx = egui::Context::default();
        let mut b = Board::new("round", 40.0, 25.0);
        b.outline = pcb_core::outline::rounded_rect(0.0, 0.0, 40.0, 25.0, 5.0);
        for k in 0..2 {
            let mut pad = Pad::new(Point::new(20.0 + 2.54 * k as f64, 12.0), PadShape::Circle { d: 1.6 }, Some(0.8), Side::Top);
            pad.refdes = "J1".into();
            b.pads.push(pad);
        }
        b.parts.push(pcb_core::Part { refdes: "J1".into(), value: "conn".into(), footprint: "PinHeader_1x2".into(), pos: Point::new(20.0, 12.0), rot: 0, side: Side::Top });
        let s = Scene::build(&b, &ctx, true, true).unwrap();
        assert!(s.boxes.len() >= 3, "plastic and pins");
        assert!(s.tris.len() > 10, "{}", s.tris.len());
        assert_eq!(s.parts[0].0, "J1");
        let (r, t) = s.tallest[0].clone().unwrap();
        assert_eq!(r, "J1");
        assert!(t > 8.0, "the pins make the header the tall part: {t}");
        assert!(s.tallest[1].is_none());
        // a board without an outline is explained
        b.outline.clear();
        assert!(Scene::build(&b, &ctx, true, true).is_err());
    }
}
