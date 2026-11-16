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
    fn the_glow_of_a_net_includes_the_polygons_of_the_finish() {
        // two tracks leave one small pad at a sharp angle: with the finish on, the notch between them is a patch
        let mut b = pcb_core::Board::new("t", 40.0, 30.0);
        let mut p = Pad::new(Point::new(5.0, 15.0), PadShape::Rect { w: 0.3, h: 0.3 }, None, Side::Top);
        p.net = "A".into();
        b.pads.push(p);
        let a = 15f64.to_radians();
        for s in [1.0, -1.0] {
            b.tracks.push(pcb_core::Track { side: Side::Top, a: Point::new(5.0, 15.0), b: Point::new(5.0 + 10.0 * a.cos(), 15.0 + s * 10.0 * a.sin()), width: 0.25 });
        }
        b.finish = pcb_core::smooth::Finish { corners: false, tapers: false, ..Default::default() };
        let f = pcb_core::smooth::finished(&b).unwrap();
        assert_eq!(f.patches.len(), 1);
        let conn = pcb_core::drc::connectivity(&f.board);
        let plain = net_glow(&f.board, &conn, "A", &[]).len();
        let with = net_glow(&f.board, &conn, "A", &f.patches).len();
        assert!(with > plain, "{with} > {plain}");
    }

    #[test]
    fn a_picked_net_lights_its_copper_and_nothing_else() {
        let b = two_resistors();
        let mut s = Scene::build(&b, &egui::Context::default(), false, false).unwrap();
        assert_eq!(s.nets, vec!["A".to_string(), "B".to_string()]);
        // net B: the pads at x=9 and x=31 and the track leaving the first
        s.glow = net_glow(&b, &pcb_core::drc::connectivity(&b), "B", &[]);
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
    fn a_picked_net_lights_its_vias_on_both_faces() {
        let mut b = two_resistors();
        b.vias.push(pcb_core::Via { pos: Point::new(20.0, 20.0), diameter: 1.2, drill: 0.6, net: "B".into() });
        b.vias.push(pcb_core::Via { pos: Point::new(25.0, 20.0), diameter: 1.2, drill: 0.6, net: "A".into() });
        let glow = net_glow(&b, &pcb_core::drc::connectivity(&b), "B", &[]);
        let near = |bottom: bool, x: f64| glow.iter().any(|(f, t)| *f == bottom && t.iter().all(|p| (p.x - x).abs() < 0.7 && (p.y - 20.0).abs() < 0.7));
        assert!(near(false, 20.0) && near(true, 20.0), "the via of net B shows on the top and the bottom");
        assert!(!near(false, 25.0) && !near(true, 25.0), "the via of net A does not");
        // and it is drawn: lit at the via when seen from above
        let mut s = Scene::build(&b, &egui::Context::default(), false, false).unwrap();
        s.glow = glow;
        let (rect, cam) = top_view(&s);
        let (img, ..) = snapshot(&s, rect, &cam, false, Quality::Fine(1), None, None);
        let (p, _) = cam.project([20.0, 20.0, BOARD_THICKNESS]);
        let c = img[p.y.round() as usize * 800 + p.x.round() as usize];
        assert!(c[0] > 200 && c[1] < 100 && c[2] > 120, "{c:?}");
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

    /// A cube 4 x 4 x 6 mm (corners at +-2) as a mesh, outward-facing triangles, one colour.
    fn cube_mesh(color: [u8; 3]) -> pcb_core::mesh::Mesh3d {
        let v = |x: f32, y: f32, z: f32| [x, y, z];
        let vertices = vec![v(-2.0, -2.0, 0.0), v(2.0, -2.0, 0.0), v(2.0, 2.0, 0.0), v(-2.0, 2.0, 0.0), v(-2.0, -2.0, 6.0), v(2.0, -2.0, 6.0), v(2.0, 2.0, 6.0), v(-2.0, 2.0, 6.0)];
        let triangles = vec![[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [1, 2, 6], [1, 6, 5], [2, 3, 7], [2, 7, 6], [3, 0, 4], [3, 4, 7]];
        pcb_core::mesh::Mesh3d { tri_color: vec![0; triangles.len()], vertices, triangles, palette: vec![color] }
    }

    #[test]
    fn a_part_with_a_mesh_is_drawn_from_the_mesh_and_counts_in_the_heights() {
        let mut b = two_resistors();
        let mut pad = Pad::new(Point::new(20.0, 20.0), PadShape::Rect { w: 1.2, h: 1.6 }, None, Side::Top);
        pad.refdes = "U1".into();
        b.pads.push(pad);
        b.models3d.insert("CUBE".into(), cube_mesh([40, 200, 60]));
        b.parts.push(pcb_core::Part { refdes: "U1".into(), value: "x".into(), footprint: "CUBE".into(), pos: Point::new(20.0, 20.0), rot: 0, side: Side::Top });
        let mut s = Scene::build(&b, &egui::Context::default(), false, false).unwrap();
        assert!(s.boxes.iter().all(|x| x.refdes != "U1"), "no guessed box for the part");
        assert_eq!(s.meshes.len(), 1);
        assert_eq!(s.tallest[0].as_ref().map(|t| (t.0.as_str(), t.1.round())), Some(("U1", 6.0)));
        // seen from above it is green; over the case height it is red
        let (rect, cam) = top_view(&s);
        let (img, ..) = snapshot(&s, rect, &cam, true, Quality::Fine(1), None, None);
        let (p, _) = cam.project([20.0, 20.0, BOARD_THICKNESS + 6.0]);
        let c = img[p.y.round() as usize * 800 + p.x.round() as usize];
        assert!(c[1] > 120 && c[0] < 100, "green cube: {c:?}");
        s.set_limits(true, 3.0, 0.0);
        let (img, ..) = snapshot(&s, rect, &cam, true, Quality::Fine(1), None, None);
        let c = img[p.y.round() as usize * 800 + p.x.round() as usize];
        assert!(c[0] > 150 && c[1] < 90, "red cube: {c:?}");
        assert_eq!(s.over_list()[0].0, "U1");
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
    fn the_case_box_is_a_wire_frame_round_the_board_and_turns_when_the_board_only_fits_turned() {
        let b = tall_and_short();
        let mut s = Scene::build(&b, &egui::Context::default(), false, false).unwrap();
        let (bw, bd) = (s.hi.x - s.lo.x, s.hi.y - s.lo.y);
        let spec = |w, d| pcb_core::case::CaseSpec { inner_w: w, inner_d: d, top: 10.0, bottom: 2.0, clearance: 0.5 };
        assert!(s.set_case(Some(spec(bw + 10.0, bd + 10.0)), &b));
        let w = s.case.clone().unwrap();
        assert!(((w.hi.x - w.lo.x) - (bw + 10.0)).abs() < 1e-9, "the box has the inside size given");
        assert_eq!((w.z0, w.z1), (-2.0, BOARD_THICKNESS + 10.0));
        assert!(!s.set_case(Some(spec(bw + 10.0, bd + 10.0)), &b), "same settings: nothing to redraw");
        // given the other way round and only big enough that way: the box is drawn turned, the board stays
        if (bw - bd).abs() > 3.0 {
            assert!(s.set_case(Some(spec(bd + 2.0, bw + 2.0)), &b));
            let w = s.case.clone().unwrap();
            assert!(((w.hi.x - w.lo.x) - (bw + 2.0)).abs() < 1e-9, "turned: the box is wide enough for the board as it lies");
        }
        assert!(s.set_case(None, &b) && s.case.is_none());
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

    /// The GPU path finds the part under the pointer on the CPU (the ray against the parts' triangles), as the software picture's part image
    /// does: the middle of each resistor from above and at a slant, nothing over bare board, and nothing when the parts are hidden.
    #[test]
    fn the_part_under_the_pointer_is_found_by_its_ray() {
        let b = two_resistors();
        let s = Scene::build(&b, &egui::Context::default(), false, false).unwrap();
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 500.0));
        for pitch in [std::f32::consts::FRAC_PI_2, 0.9] {
            let cam = camera(&s, rect, 0.3, pitch, 1.0, Vec2::ZERO);
            for (i, x) in [(0usize, 8.0), (1, 30.0)] {
                let top = s.boxes.iter().filter(|bx| bx.part == i).map(|bx| bx.hi[2]).fold(0.0, f64::max);
                let (p, _) = cam.project([x, 12.0, top]);
                assert_eq!(gpu::pick(&s, &cam, p, true), Some(i), "part {i} at pitch {pitch}");
                assert_eq!(gpu::pick(&s, &cam, p, false), None, "parts hidden");
            }
            let (p, _) = cam.project([20.0, 5.0, BOARD_THICKNESS]);
            assert_eq!(gpu::pick(&s, &cam, p, true), None, "bare board");
        }
    }

    /// A guessed cylinder shades round: the light at a corner of a side comes from straight out of the axis, and only the rims
    /// (and every edge of a box) are drawn as dark lines, not the seams between the 32 sides.
    #[test]
    fn a_guessed_cylinder_has_round_normals_and_no_seams() {
        let cyl = PartBox { shape: BodyShape::Cylinder(2), lo: [0.0, 0.0, 1.6], hi: [4.0, 4.0, 6.0], ..solid_box(BodyShape::Box) };
        let n = solid_shade(&cyl, [4.0, 2.0, 3.0], [0.98, 0.1, 0.0]);
        assert!((n[0] - 1.0).abs() < 1e-6 && n[1].abs() < 1e-6 && n[2] == 0.0, "{n:?}");
        assert_eq!(solid_shade(&cyl, [4.0, 2.0, 6.0], [0.0, 0.0, 1.0]), [0.0, 0.0, 1.0], "the cap keeps its own");
        assert_eq!(solid_shade(&solid_box(BodyShape::Box), [4.0, 2.0, 3.0], [1.0, 0.0, 0.0]), [1.0, 0.0, 0.0]);
        // a side's edge along the axis is soft, its rim edge hard; a box's edges are all hard
        assert!(!hard_edge(&cyl, [1.0, 0.0, 0.0], [4.0, 2.0, 1.6], [4.0, 2.0, 6.0]));
        assert!(hard_edge(&cyl, [1.0, 0.0, 0.0], [4.0, 2.0, 1.6], [3.9, 2.8, 1.6]));
        assert!(hard_edge(&solid_box(BodyShape::Box), [1.0, 0.0, 0.0], [4.0, 0.0, 1.6], [4.0, 0.0, 4.0]));
    }

    /// A cylinder of radius 4 and height 6 cut into 16 flat sides, each its own quad (as a STEP gives them), with caps.
    fn faceted_cylinder() -> pcb_core::mesh::Mesh3d {
        let seg = 16;
        let mut m = pcb_core::mesh::Mesh3d { palette: vec![[200, 200, 200]], ..Default::default() };
        let at = |k: usize, z: f32| {
            let t = std::f32::consts::TAU * k as f32 / seg as f32;
            [4.0 * t.cos(), 4.0 * t.sin(), z]
        };
        for k in 0..seg {
            let b = m.vertices.len() as u32;
            m.vertices.extend([at(k, 0.0), at(k + 1, 0.0), at(k + 1, 6.0), at(k, 6.0)]);
            m.triangles.extend([[b, b + 1, b + 2], [b, b + 2, b + 3]]);
        }
        for (z, up) in [(6.0, true), (0.0, false)] {
            let c = m.vertices.len() as u32;
            m.vertices.push([0.0, 0.0, z]);
            m.vertices.extend((0..seg).map(|k| at(k, z)));
            for k in 0..seg as u32 {
                let (a, b) = (c + 1 + k, c + 1 + (k + 1) % seg as u32);
                m.triangles.push(if up { [c, a, b] } else { [c, b, a] });
            }
        }
        m.tri_color = vec![0; m.triangles.len()];
        m
    }

    /// Seen from the side, a cylinder made of flat sides shades without steps between the sides (smooth normals); with every face
    /// kept flat the steps are plain.
    #[test]
    fn a_faceted_mesh_shades_without_steps() {
        let mut b = two_resistors();
        let mut pad = Pad::new(Point::new(20.0, 20.0), PadShape::Rect { w: 1.2, h: 1.6 }, None, Side::Top);
        pad.refdes = "U1".into();
        b.pads.push(pad);
        b.models3d.insert("CYL".into(), faceted_cylinder());
        b.parts.push(pcb_core::Part { refdes: "U1".into(), value: "x".into(), footprint: "CYL".into(), pos: Point::new(20.0, 20.0), rot: 0, side: Side::Top });
        let mut s = Scene::build(&b, &egui::Context::default(), false, false).unwrap();
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 500.0));
        let cam = camera(&s, rect, 0.3, 0.1, 6.0, Vec2::ZERO);
        // the largest step between neighbouring pixels across the middle of the cylinder's side (away from its outline)
        let steps = |s: &Scene| {
            let (img, ..) = snapshot(s, rect, &cam, true, Quality::Fine(1), None, None);
            let z = BOARD_THICKNESS + 3.0;
            let (l, _) = cam.project([20.0 - 4.0 * 0.3f64.cos(), 20.0 + 4.0 * 0.3f64.sin(), z]);
            let (r, _) = cam.project([20.0 + 4.0 * 0.3f64.cos(), 20.0 - 4.0 * 0.3f64.sin(), z]);
            let (x0, x1) = (l.x.min(r.x), l.x.max(r.x));
            let (a, e) = ((x0 + 0.15 * (x1 - x0)) as usize, (x1 - 0.15 * (x1 - x0)) as usize);
            assert!(e > a + 40, "the cylinder is big enough to see: {a}..{e}");
            let y = l.y.round() as usize;
            let px = |x: usize| img[y * 800 + x];
            (a..e).map(|x| (0..3).map(|k| (px(x + 1)[k] as i32 - px(x)[k] as i32).abs()).max().unwrap()).max().unwrap()
        };
        let smooth = steps(&s);
        s.mesh_normals = s.meshes.iter().map(|m| m.mesh.corner_normals(1.0)).collect();
        let flat = steps(&s);
        assert!(smooth <= 6 && flat >= 12, "smooth {smooth}, flat {flat}");
    }

    /// The heat colours: the palette runs from dark blue (cold) to pale yellow (hot) and gets brighter all the way; the scale
    /// runs from the ambient to the hottest part, but at least `MIN_SPAN` K.
    #[test]
    fn the_heat_palette_runs_cold_to_hot_and_the_scale_has_a_floor() {
        let bright = |c: [u8; 3]| c.iter().map(|&v| v as u32).sum::<u32>();
        let mut last = 0;
        for k in 0..=20 {
            let b = bright(heat::heat_color(k as f64 / 20.0));
            assert!(b >= last, "step {k}: {b} < {last}");
            last = b;
        }
        let cold = heat::heat_color(0.0);
        assert!(cold[2] > cold[0] && cold[2] > cold[1], "cold is blue: {cold:?}");
        assert_eq!(heat::heat_color(-1.0), cold);
        assert_eq!(heat::heat_color(f64::NAN), cold);
        assert_eq!(heat::heat_color(2.0), heat::heat_color(1.0));
        let t = |hot: f64| heat::PartTemps { ambient: 25.0, parts: vec![("R1".into(), hot, 0.1)] };
        assert_eq!(heat::scale(&t(80.0)), (25.0, 80.0));
        assert_eq!(heat::scale(&t(27.0)), (25.0, 25.0 + heat::MIN_SPAN));
    }

    /// With the temperatures on, a warmed part is painted hot and the others at the cold end (the ambient), in both the software and
    /// the GPU picture (kept in the white look); taking them away gives the parts their own colours back.
    #[test]
    fn parts_are_painted_by_their_temperature() {
        let b = tall_and_short();
        let mut s = Scene::build(&b, &egui::Context::default(), false, false).unwrap();
        let c1 = s.parts.iter().position(|p| p.0 == "C1").unwrap();
        let r1 = s.parts.iter().position(|p| p.0 == "R1").unwrap();
        let temps = heat::PartTemps { ambient: 25.0, parts: vec![("C1".into(), 85.0, 0.5), ("NOT_ON_BOARD".into(), 40.0, 0.1)] };
        assert!(s.set_heat(Some(&temps)));
        assert!(!s.set_heat(Some(&temps)), "same temperatures: nothing to redraw");
        assert_eq!(s.heat[c1], Some(heat::heat_color(1.0)));
        assert_eq!(s.heat[r1], Some(heat::heat_color(0.0)), "a part the run did not warm is at the ambient");
        assert_eq!(s.temps[c1], Some((85.0, 0.5)));
        assert_eq!(s.temps[r1], None);
        assert!(s.part_marked(c1) && s.part_marked(r1));
        // the hot can seen from above is pale yellow, the cold resistor blue
        let (rect, cam) = top_view(&s);
        let (img, ..) = snapshot(&s, rect, &cam, true, Quality::Fine(1), None, None);
        let at = |p: [f64; 3]| {
            let (q, _) = cam.project(p);
            img[q.y.round() as usize * 800 + q.x.round() as usize]
        };
        let can = at([20.0, 20.0, BOARD_THICKNESS + 4.5]);
        assert!(can[0] > 200 && can[1] > 150, "hot can: {can:?}");
        let rb = s.boxes.iter().find(|bx| bx.part == r1).unwrap();
        let res = at([(rb.lo[0] + rb.hi[0]) / 2.0, (rb.lo[1] + rb.hi[1]) / 2.0, rb.hi[2]]);
        assert!(res[2] > res[0] && res[2] > res[1], "cold resistor: {res:?}");
        // the GPU's data marks every part to keep its colour in the white look
        let g = gpu::Geometry::build(&s, 1);
        assert!(g.part_texes().iter().all(|t| *t == 4), "{:?}", g.part_texes());
        // off: the own colours again
        assert!(s.set_heat(None));
        assert!(s.heat.iter().all(Option::is_none));
        let g = gpu::Geometry::build(&s, 2);
        assert!(g.part_texes().iter().all(|t| *t == 0));
    }

    /// The GPU's vertex data: the board's faces and edge first (drawn alone when the parts are hidden), then the parts; rebuilt for a new
    /// generation.
    #[test]
    fn the_gpu_vertex_data_holds_the_board_then_the_parts() {
        let b = two_resistors();
        let s = Scene::build(&b, &egui::Context::default(), false, false).unwrap();
        let g = gpu::Geometry::build(&s, 7);
        assert_eq!(g.key(), (s.serial, 7));
        let (board, all) = g.counts();
        // the outline is a rectangle: 2 triangles on each face and 2 on each of the 4 sides
        assert_eq!(board, 3 * (2 * 2 + 4 * 2));
        let part_tris: usize = s.boxes.iter().map(|bx| solid_faces(bx).iter().map(|(c, _)| c.len() - 2).sum::<usize>()).sum();
        assert_eq!(all - board, 3 * part_tris as u32);
        let other = Scene::build(&b, &egui::Context::default(), false, false).unwrap();
        assert_ne!(other.serial, s.serial, "another scene is uploaded again");
    }

    /// How long a picture takes with many fine part meshes (as SolidCraft cuts them): sets the triangles a part may keep (候補 E8 ③).
    /// `cargo test -p app view3d_mesh_cost -- --ignored --nocapture` (add `--release` for the times of the shipped program).
    #[test]
    #[ignore]
    fn view3d_mesh_cost() {
        use pcb_core::mesh::Mesh3d;
        // a cylinder of `seg` sides and `rows` bands: 2 seg (rows + 1) triangles
        fn cylinder(seg: usize, rows: usize) -> Mesh3d {
            let mut m = Mesh3d { palette: vec![[40, 70, 160]], ..Default::default() };
            for r in 0..=rows {
                for k in 0..seg {
                    let a = std::f32::consts::TAU * k as f32 / seg as f32;
                    m.vertices.push([2.5 * a.cos(), 2.5 * a.sin(), 11.0 * r as f32 / rows as f32]);
                }
            }
            for r in 0..rows {
                for k in 0..seg {
                    let (i, j) = ((r * seg + k) as u32, (r * seg + (k + 1) % seg) as u32);
                    let s = seg as u32;
                    m.triangles.push([i, j, j + s]);
                    m.triangles.push([i, j + s, i + s]);
                }
            }
            let (bot, top) = (m.vertices.len() as u32, m.vertices.len() as u32 + 1);
            m.vertices.push([0.0, 0.0, 0.0]);
            m.vertices.push([0.0, 0.0, 11.0]);
            let s = seg as u32;
            for k in 0..s {
                m.triangles.push([bot, (k + 1) % s, k]);
                m.triangles.push([top, rows as u32 * s + k, rows as u32 * s + (k + 1) % s]);
            }
            m.tri_color = vec![0; m.triangles.len()];
            m
        }
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 680.0));
        println!("parts  tris/part  total     build ms  draft ms  fine(1) ms  fine(2) ms");
        for (parts, seg, rows) in [(10, 32, 4), (50, 32, 4), (10, 64, 40), (50, 64, 40), (50, 64, 10), (100, 64, 10), (50, 128, 40), (100, 128, 40)] {
            let mut b = Board::new("t", 100.0, 80.0);
            let mut fp = pcb_core::footprints::get("R_0805").unwrap();
            fp.name = "MESHED".into();
            b.add_footprint(fp);
            let mesh = cylinder(seg, rows);
            let per = mesh.triangles.len();
            b.models3d.insert("MESHED".into(), mesh);
            for i in 0..parts {
                let (x, y) = (8.0 + (i % 10) as f64 * 9.0, 8.0 + (i / 10) as f64 * 7.0);
                b.set_part(pcb_core::model::Part { refdes: format!("C{i}"), value: "x".into(), footprint: "MESHED".into(), pos: Point::new(x, y), rot: 0, side: pcb_core::model::Side::Top }).unwrap();
            }
            let t = std::time::Instant::now();
            let s = Scene::build(&b, &egui::Context::default(), false, false).unwrap();
            let build = t.elapsed().as_secs_f64() * 1000.0;
            let cam = camera(&s, rect, 0.6, 0.7, 1.0, Vec2::ZERO);
            let time = |q: Quality| {
                let t = std::time::Instant::now();
                for _ in 0..3 {
                    let _ = snapshot(&s, rect, &cam, true, q, None, None);
                }
                t.elapsed().as_secs_f64() * 1000.0 / 3.0
            };
            let (d, f1, f2) = (time(Quality::Draft), time(Quality::Fine(1)), time(Quality::Fine(2)));
            println!("{parts:5}  {per:9}  {:7}  {build:8.1}  {d:8.1}  {f1:10.1}  {f2:10.1}", parts * per);
        }
    }
}

