use super::*;

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

#[test]
fn route_modes_bend_as_documented() {
    let mut e = BoardEditor::default();
    // 45 degree: straight run first, then the diagonal
    assert_eq!(e.route_points(p(0.0, 0.0), p(10.0, 4.0)), vec![p(6.0, 0.0), p(10.0, 4.0)]);
    assert_eq!(e.route_points(p(0.0, 0.0), p(4.0, -10.0)), vec![p(0.0, -6.0), p(4.0, -10.0)]);
    e.bend_flip = true;
    assert_eq!(e.route_points(p(0.0, 0.0), p(10.0, 4.0)), vec![p(4.0, 4.0), p(10.0, 4.0)]);
    // exact diagonal and axis-aligned moves need no bend
    assert_eq!(e.route_points(p(0.0, 0.0), p(5.0, 5.0)), vec![p(5.0, 5.0)]);
    assert_eq!(e.route_points(p(0.0, 0.0), p(5.0, 0.0)), vec![p(5.0, 0.0)]);
    e.route_mode = RouteMode::Orthogonal;
    assert_eq!(e.route_points(p(0.0, 0.0), p(10.0, 4.0)), vec![p(0.0, 4.0), p(10.0, 4.0)]);
    e.bend_flip = false;
    assert_eq!(e.route_points(p(0.0, 0.0), p(10.0, 4.0)), vec![p(10.0, 0.0), p(10.0, 4.0)]);
    e.route_mode = RouteMode::Free;
    assert_eq!(e.route_points(p(0.0, 0.0), p(10.0, 4.0)), vec![p(10.0, 4.0)]);
}

#[test]
fn routing_clicks_create_connected_segments_and_undo_removes_them() {
    let mut e = BoardEditor::default();
    e.tool = Tool::Track;
    e.click(p(5.0, 5.0));
    e.click(p(15.0, 9.0)); // 45 degree route: two segments
    assert_eq!(e.board.tracks.len(), 2);
    let (t0, t1) = (&e.board.tracks[0], &e.board.tracks[1]);
    assert_eq!((t0.a, t1.b), (p(5.0, 5.0), p(15.0, 9.0)));
    assert_eq!(t0.b, t1.a);
    assert_eq!(e.pending, Some(p(15.0, 9.0)), "the route continues from the last point");
    e.click(p(15.0, 9.0)); // zero-length: nothing added
    assert_eq!(e.board.tracks.len(), 2);

    e.undo();
    assert_eq!(e.board.tracks.len(), 2, "the zero-length click made a checkpoint of the same state");
    e.undo();
    assert_eq!(e.board.tracks.len(), 0);
    e.redo();
    assert_eq!(e.board.tracks.len(), 2);
    // the first click of a route only picks the start, it is not an edit
    e.click(p(1.0, 1.0));
    assert!(!e.redo.is_empty());
    // a real edit clears the redo history
    e.click(p(3.0, 1.0));
    assert!(e.redo.is_empty());
}

#[test]
fn via_while_routing_switches_layer() {
    let mut e = BoardEditor::default();
    e.tool = Tool::Track;
    e.route_mode = RouteMode::Free;
    e.pending = Some(p(5.0, 5.0));
    e.route_via(p(5.0, 5.0), p(10.0, 5.0));
    assert_eq!((e.board.tracks.len(), e.board.vias.len()), (1, 1));
    assert_eq!(e.board.tracks[0].side, Side::Top);
    assert_eq!(e.side, Side::Bottom);
    assert_eq!(e.pending, Some(p(10.0, 5.0)));
    e.click(p(10.0, 12.0));
    assert_eq!(e.board.tracks[1].side, Side::Bottom);
    // top and bottom tracks are joined through the via
    let c = connectivity(&e.board);
    assert_eq!(c.track_group(0), c.track_group(1));
    assert_eq!(c.via_group(0), c.track_group(0));
}

#[test]
fn polygon_tools_need_three_points() {
    let mut e = BoardEditor::default();
    e.tool = Tool::OutlinePoly;
    e.click(p(0.0, 0.0));
    e.click(p(10.0, 0.0));
    e.finish_polygon();
    assert_eq!(e.board.outline.len(), 4, "unchanged: two points are not a polygon");
    assert!(e.status.contains("3点"));

    e.tool = Tool::OutlinePoly;
    e.poly = vec![p(0.0, 0.0), p(20.0, 0.0), p(20.0, 10.0), p(10.0, 10.0), p(10.0, 20.0), p(0.0, 20.0)];
    e.finish_polygon();
    assert_eq!(e.board.outline.len(), 6, "an L-shaped board");
    e.undo();
    assert_eq!(e.board.outline.len(), 4);

    // zone: the duplicate last point from a double-click is dropped
    e.tool = Tool::Zone;
    e.zone_net = "GND".into();
    e.poly = vec![p(1.0, 1.0), p(9.0, 1.0), p(9.0, 9.0), p(9.0, 9.0)];
    e.finish_polygon();
    assert_eq!(e.board.zones.len(), 1);
    assert_eq!(e.board.zones[0].polygon.len(), 3);
    assert_eq!((e.board.zones[0].net.as_str(), e.board.zones[0].side), ("GND", Side::Top));
}

#[test]
fn closing_a_polygon_by_clicking_the_first_point() {
    let mut e = BoardEditor::default();
    e.tool = Tool::Zone;
    for q in [p(2.0, 2.0), p(8.0, 2.0), p(8.0, 8.0)] {
        e.click(q);
    }
    assert!(e.board.zones.is_empty());
    e.click(p(2.0, 2.0));
    assert_eq!(e.board.zones.len(), 1);
    assert!(e.poly.is_empty());
}

#[test]
fn window_and_crossing_boxes_select_differently() {
    let mut e = BoardEditor::default();
    e.board.tracks.push(Track { side: Side::Top, a: p(5.0, 5.0), b: p(10.0, 5.0), width: 0.25 }); // inside
    e.board.tracks.push(Track { side: Side::Top, a: p(8.0, 5.0), b: p(25.0, 5.0), width: 0.25 }); // leaves the box
    e.board.vias.push(Via { pos: p(6.0, 8.0), diameter: 0.6, drill: 0.3 });
    e.board.zones.push(Zone::new("GND", Side::Top, vec![p(0.0, 0.0), p(40.0, 0.0), p(40.0, 40.0), p(0.0, 40.0)]));
    let (a, b) = (p(3.0, 3.0), p(12.0, 10.0));
    let window = e.items_in_box(a, b, false);
    assert!(window.contains(&Item::Track(0)) && window.contains(&Item::Via(0)));
    assert!(!window.contains(&Item::Track(1)), "only partly inside");
    let crossing = e.items_in_box(a, b, true);
    assert!(crossing.contains(&Item::Track(1)));
    // a box inside the big pour does not grab it; one crossing its edge does
    assert!(!crossing.contains(&Item::Zone(0)));
    assert!(e.items_in_box(p(-2.0, 10.0), p(2.0, 12.0), true).contains(&Item::Zone(0)));
    // objects on a hidden layer are skipped (vias belong to both layers)
    e.vis.top = false;
    assert_eq!(e.items_in_box(a, b, true), vec![Item::Via(0)]);
}

#[test]
fn group_move_and_delete() {
    let mut e = BoardEditor::default();
    e.board.tracks.push(Track { side: Side::Top, a: p(5.0, 5.0), b: p(10.0, 5.0), width: 0.25 });
    e.board.vias.push(Via { pos: p(6.0, 8.0), diameter: 0.6, drill: 0.3 });
    e.board.holes.push(Hole::new(p(30.0, 30.0), 3.2));
    let items = e.items_in_box(p(3.0, 3.0), p(12.0, 10.0), false);
    assert_eq!(items.len(), 2);
    e.set_selection(items.clone());
    assert!(e.selected.is_none() && e.multi.len() == 2);
    let g = GroupDrag { anchor: p(5.0, 5.0), orig: e.board.clone(), items };
    e.move_group(&g, 2.0, -1.0);
    assert_eq!((e.board.tracks[0].a.x, e.board.tracks[0].a.y), (7.0, 4.0));
    assert_eq!((e.board.vias[0].pos.x, e.board.vias[0].pos.y), (8.0, 7.0));
    assert_eq!(e.board.holes[0].pos.x, 30.0, "unselected objects stay");
    // moving again from the same snapshot does not accumulate
    e.move_group(&g, 1.0, 0.0);
    assert_eq!(e.board.tracks[0].a.x, 6.0);
    e.delete_multi();
    assert!(e.board.tracks.is_empty() && e.board.vias.is_empty() && e.board.holes.len() == 1);
    e.undo();
    assert_eq!(e.board.tracks.len(), 1);
}

#[test]
fn picking_and_deleting() {
    let mut e = BoardEditor::default();
    e.board.tracks.push(Track { side: Side::Top, a: p(5.0, 5.0), b: p(15.0, 5.0), width: 0.25 });
    e.board.zones.push(Zone::new("GND", Side::Top, vec![p(20.0, 20.0), p(30.0, 20.0), p(30.0, 30.0), p(20.0, 30.0)]));
    assert!(matches!(e.hit_test(p(10.0, 5.1)), Some(Item::Track(0))));
    assert!(matches!(e.hit_test(p(25.0, 25.0)), Some(Item::Zone(0))), "inside a zone selects it");
    assert!(e.hit_test(p(40.0, 5.0)).is_none());
    // hidden layers cannot be picked
    e.vis.top = false;
    assert!(e.hit_test(p(10.0, 5.1)).is_none());
    e.vis.top = true;

    e.delete(Item::Track(0));
    e.delete(Item::Zone(0));
    assert!(e.board.tracks.is_empty() && e.board.zones.is_empty());
    e.undo();
    assert_eq!(e.board.zones.len(), 1);
    e.undo();
    assert_eq!(e.board.tracks.len(), 1);
}

#[test]
fn parts_cannot_be_deleted_from_the_board() {
    let mut e = BoardEditor::default();
    e.board.set_part(pcb_core::Part { refdes: "R1".into(), value: "1k".into(), footprint: "R_0805".into(), pos: p(10.0, 10.0), rot: 0, side: Side::Top }).unwrap();
    e.delete(Item::Part(0));
    assert_eq!(e.board.parts.len(), 1);
    assert!(e.status.contains("回路図"));
    assert!(e.undo.is_empty());
}

#[test]
fn fab_files_include_assembly_csv() {
    let mut e = BoardEditor::default();
    e.board.name = "unit".into();
    e.board.set_part(pcb_core::Part { refdes: "R1".into(), value: "1k".into(), footprint: "R_0805".into(), pos: p(10.0, 10.0), rot: 0, side: Side::Top }).unwrap();
    let dir = std::env::temp_dir().join(format!("circuitcad_fab_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let n = e.write_fab_files(&dir).unwrap();
    assert_eq!(n, 11); // 9 Gerber/drill files + BOM + CPL
    assert!(std::fs::read_to_string(dir.join("unit-BOM.csv")).unwrap().contains("R1"));
    assert!(std::fs::read_to_string(dir.join("unit-CPL.csv")).unwrap().contains("R1,10.0000mm"));
    assert!(dir.join("unit_gerber.zip").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn flipping_the_selected_part_is_undoable() {
    let mut e = BoardEditor::default();
    e.board.set_part(pcb_core::Part { refdes: "R1".into(), value: "1k".into(), footprint: "R_0805".into(), pos: p(10.0, 10.0), rot: 0, side: Side::Top }).unwrap();
    e.flip_selected_part(); // nothing selected: no-op
    assert!(e.undo.is_empty());
    e.selected = Some(Item::Part(0));
    e.flip_selected_part();
    assert_eq!(e.board.parts[0].side, Side::Bottom);
    assert!(e.board.pads.iter().all(|q| q.side == Side::Bottom));
    assert!(e.status.contains("裏面"), "{}", e.status);
    e.flip_selected_part();
    assert_eq!(e.board.parts[0].side, Side::Top);
    e.undo();
    assert_eq!(e.board.parts[0].side, Side::Bottom);
    e.undo();
    assert_eq!(e.board.parts[0].side, Side::Top);
}

#[test]
fn pads_on_a_hidden_side_cannot_be_picked() {
    let mut e = BoardEditor::default();
    e.board.set_part(pcb_core::Part { refdes: "R1".into(), value: "1k".into(), footprint: "R_0805".into(), pos: p(10.0, 10.0), rot: 0, side: Side::Bottom }).unwrap();
    let on_pad = e.board.pads[0].pos;
    e.side = Side::Bottom;
    assert!(matches!(e.hit_test(on_pad), Some(Item::Part(0))));
    e.vis.bottom = false;
    assert!(e.hit_test(on_pad).is_none());
}

#[test]
fn only_the_layer_being_worked_on_can_be_picked_unless_that_is_switched_off() {
    let mut e = BoardEditor::default();
    e.board.set_part(pcb_core::Part { refdes: "R1".into(), value: "1k".into(), footprint: "R_0805".into(), pos: p(10.0, 10.0), rot: 0, side: Side::Bottom }).unwrap();
    e.board.tracks.push(Track { side: Side::Bottom, a: p(20.0, 10.0), b: p(30.0, 10.0), width: 0.25 });
    e.board.vias.push(Via { pos: p(25.0, 20.0), diameter: 0.6, drill: 0.3 });
    let (on_pad, on_track, on_via) = (e.board.pads[0].pos, p(25.0, 10.0), p(25.0, 20.0));
    // working on the top: the bottom part and track are out of reach, the via (through the board) is not
    e.side = Side::Top;
    assert!(e.hit_test(on_pad).is_none() && e.hit_test(on_track).is_none());
    assert!(matches!(e.hit_test(on_via), Some(Item::Via(0))));
    let all = e.items_in_box(p(0.0, 0.0), p(40.0, 30.0), true);
    assert_eq!(all, vec![Item::Via(0)], "a box does not take the other layer along");
    // on the bottom they are reachable; so they are with the switch off
    e.side = Side::Bottom;
    assert!(matches!(e.hit_test(on_pad), Some(Item::Part(0))) && matches!(e.hit_test(on_track), Some(Item::Track(0))));
    e.side = Side::Top;
    e.pick.active_side_only = false;
    assert!(matches!(e.hit_test(on_track), Some(Item::Track(0))));
}

#[test]
fn a_fixed_part_is_not_dragged_turned_flipped_or_carried_along() {
    let mut e = BoardEditor::default();
    e.board.set_part(pcb_core::Part { refdes: "R1".into(), value: "1k".into(), footprint: "R_0805".into(), pos: p(10.0, 10.0), rot: 0, side: Side::Top }).unwrap();
    e.board.set_part(pcb_core::Part { refdes: "R2".into(), value: "1k".into(), footprint: "R_0805".into(), pos: p(20.0, 10.0), rot: 0, side: Side::Top }).unwrap();
    e.board.locked_parts.push("R1".into());
    assert!(e.is_locked_item(&Item::Part(0)) && !e.is_locked_item(&Item::Part(1)));
    // it can still be selected (to unlock it), but a flip is refused
    e.selected = Some(Item::Part(0));
    e.flip_selected_part();
    assert_eq!(e.board.parts[0].side, Side::Top);
    assert!(e.status.contains("固定"), "{}", e.status);
    // a group move leaves it where it is and takes the other part along
    let g = GroupDrag { anchor: p(0.0, 0.0), orig: e.board.clone(), items: vec![Item::Part(0), Item::Part(1)] };
    e.move_group(&g, 5.0, 0.0);
    assert_eq!(e.board.parts[0].pos, p(10.0, 10.0));
    assert_eq!(e.board.parts[1].pos, p(25.0, 10.0));
    // it is saved with the board
    let json = serde_json::to_string(&e.board).unwrap();
    let back: pcb_core::Board = serde_json::from_str(&json).unwrap();
    assert!(back.is_locked("R1") && !back.is_locked("R2"));
    // an old file without the field loads with nothing fixed
    let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
    v.as_object_mut().unwrap().remove("locked_parts");
    assert!(serde_json::from_value::<pcb_core::Board>(v).unwrap().locked_parts.is_empty());
}

#[test]
fn parts_switched_off_in_the_picking_filter_stay_out_of_boxes_and_clicks() {
    let mut e = BoardEditor::default();
    e.board.set_part(pcb_core::Part { refdes: "R1".into(), value: "1k".into(), footprint: "R_0805".into(), pos: p(10.0, 10.0), rot: 0, side: Side::Top }).unwrap();
    e.board.vias.push(Via { pos: p(10.0, 10.0), diameter: 0.6, drill: 0.3 });
    e.pick.parts = false;
    let items = e.items_in_box(p(0.0, 0.0), p(40.0, 30.0), false);
    assert!(items.iter().all(|i| !matches!(i, Item::Part(_))) && items.contains(&Item::Via(0)), "{items:?}");
    let on_pad = e.board.pads[0].pos;
    assert!(!matches!(e.hit_test(on_pad), Some(Item::Part(_))));
}

#[test]
fn connections_and_unrouted_lines_are_kept_until_the_board_changes() {
    let mut e = BoardEditor::default();
    for (x, name) in [(10.0, "R1"), (20.0, "R2")] {
        e.board.set_part(pcb_core::Part { refdes: name.into(), value: "1k".into(), footprint: "R_0805".into(), pos: p(x, 10.0), rot: 0, side: Side::Top }).unwrap();
    }
    for pad in e.board.pads.iter_mut() {
        pad.net = "N".into(); // every pad on one net: they must be connected
    }
    let first = e.cached_airwires();
    assert!(!first.is_empty());
    assert!(std::rc::Rc::ptr_eq(&first, &e.cached_airwires()), "nothing changed: the same result is reused");
    let c1 = e.cached_connectivity();
    assert!(std::rc::Rc::ptr_eq(&c1, &e.cached_connectivity()));
    // connect everything with a track: the unrouted lines are worked out again and become fewer
    let pads: Vec<Point> = e.board.pads.iter().map(|q| q.pos).collect();
    for w in pads.windows(2) {
        e.board.tracks.push(Track { side: Side::Top, a: w[0], b: w[1], width: 0.25 });
    }
    let after = e.cached_airwires();
    assert!(after.is_empty(), "{} left", after.len());
    assert!(!std::rc::Rc::ptr_eq(&c1, &e.cached_connectivity()));
}

#[test]
fn routing_and_vias_snap_to_track_ends_and_the_router_grid_fits_the_editor_grid() {
    // the router's pitch divides the editor's grid, so every grid point is a router point too
    for grid in [0.5, 0.25, 1.0, 1.27, 2.54, 0.1] {
        let pitch = actions::route_pitch(grid);
        let k = grid / pitch;
        assert!((k - k.round()).abs() < 1e-9 && pitch <= 0.25 + 1e-9, "grid {grid}: pitch {pitch}");
    }
    assert_eq!(actions::route_pitch(0.0), 0.2);
    // a track end that is off the editor's grid (as the router leaves them) is snapped to, not the grid point beside it
    let mut e = BoardEditor::default();
    e.grid = 0.5;
    e.zoom = 20.0;
    e.board.tracks.push(Track { side: Side::Top, a: p(10.1, 10.0), b: p(10.35, 10.25), width: 0.25 });
    assert_eq!(e.snap_object(p(10.4, 10.2)), Some(p(10.35, 10.25)));
    assert_eq!(e.snap_object(p(14.0, 14.0)), None);
    // tracks of the other layer do not attract while working on this one
    e.side = Side::Bottom;
    assert_eq!(e.snap_object(p(10.4, 10.2)), None);
}

mod order_flow {
    use super::*;
    use pcb_core::fab::{self, Level};

    fn routed_editor() -> BoardEditor {
        let mut e = BoardEditor::default();
        e.board = Board::new("demo", 30.0, 20.0);
        for (x, name) in [(5.0, "A"), (25.0, "A")] {
            let mut p = Pad::new(Point::new(x, 10.0), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
            p.net = name.into();
            e.board.pads.push(p);
        }
        e.board.tracks.push(Track { side: Side::Top, a: Point::new(5.0, 10.0), b: Point::new(25.0, 10.0), width: 0.25 });
        e
    }

    #[test]
    fn opening_the_window_runs_the_checks_and_a_finished_board_is_ready() {
        let mut e = routed_editor();
        e.open_order();
        assert!(e.order.open && e.order.ran);
        assert!(fab::ready(&e.order.checks), "{:?}", e.order.checks.iter().map(|c| &c.title).collect::<Vec<_>>());
    }

    #[test]
    fn choosing_a_fab_and_applying_it_changes_the_rules_and_drawing_defaults() {
        let mut e = routed_editor();
        e.order.fab_id = "oshpark".into();
        let f = e.selected_fab().unwrap();
        e.apply_fab(f);
        assert!((e.board.rules.min_drill - 0.254).abs() < 1e-9);
        assert!((e.board.rules.edge_clearance - 0.381).abs() < 1e-9);
        assert!((e.via_drill - f.via_drill).abs() < 1e-9);
        e.undo();
        assert!((e.board.rules.min_drill - 0.3).abs() < 1e-9, "one undo restores the rules");
        // an unrouted board blocks the order
        e.board.tracks.clear();
        e.run_checks();
        assert!(e.order.checks.iter().any(|c| c.level == Level::Error && c.title.contains("未配線")));
    }

    #[test]
    fn the_preview_is_built_from_the_exported_files() {
        let mut e = routed_editor();
        let ctx = egui::Context::default();
        e.build_preview(&ctx).expect("preview");
        let pv = e.order.preview.as_ref().unwrap();
        assert_eq!(pv.drills.len(), 0, "no holes on this board");
        assert!((pv.size_mm.0 - 30.0).abs() < 1e-6);
        e.order.preview.as_mut().unwrap().bottom = true;
        e.repaint_preview(&ctx);
    }
}

#[test]
fn shipped_example_boards_can_be_previewed() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut n = 0;
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        if entry.path().extension().is_some_and(|x| x == "pcbjson") {
            let board: Board = serde_json::from_str(&std::fs::read_to_string(entry.path()).unwrap()).unwrap();
            let pv = pcb_core::gerber_view::Preview::from_files(&export::fab_files(&board), 800).unwrap_or_else(|e| panic!("{}: {e}", entry.path().display()));
            assert!(pv.size_mm.0 > 5.0, "{}", entry.path().display());
            let checks = pcb_core::fab::preflight(&board, pcb_core::fab::preset("jlcpcb"));
            assert!(!checks.is_empty());
            // the samples are tidy: no sharp corners, bypass capacitors where an IC needs them
            assert!(pcb_core::drc::acute_angle_checks(&board).is_empty(), "{}", entry.path().display());
            assert!(pcb_core::drc::decoupling_checks(&board).is_empty(), "{}", entry.path().display());
            assert!(pcb_core::drc::zone_checks(&board).is_empty(), "{}: {:?}", entry.path().display(), pcb_core::drc::zone_checks(&board));
            n += 1;
        }
    }
    assert!(n >= 1, "no example boards found");
}

#[test]
fn widening_a_net_to_its_current_is_undoable_and_only_touches_that_net() {
    let mut e = BoardEditor::default();
    for (x, net) in [(5.0, "VIN"), (25.0, "VIN"), (5.0, "SIG"), (25.0, "SIG")] {
        let y = if net == "VIN" { 10.0 } else { 20.0 };
        let mut pad = Pad::new(p(x, y), PadShape::Circle { d: 1.6 }, Some(0.8), Side::Top);
        pad.net = net.into();
        e.board.pads.push(pad);
    }
    e.board.tracks.push(Track { side: Side::Top, a: p(5.0, 10.0), b: p(25.0, 10.0), width: 0.25 });
    e.board.tracks.push(Track { side: Side::Top, a: p(5.0, 20.0), b: p(25.0, 20.0), width: 0.25 });
    assert_eq!(e.board_nets(), vec!["VIN".to_string(), "SIG".to_string()]);
    // nothing declared: nothing to widen
    assert_eq!(e.widen_net_to_need("VIN"), 0);
    e.board.net_currents.insert("VIN".into(), 2.0);
    assert_eq!(e.widen_net_to_need("VIN"), 1);
    assert!(e.board.tracks[0].width > 0.6, "{}", e.board.tracks[0].width);
    assert!((e.board.tracks[1].width - 0.25).abs() < 1e-9, "the signal net keeps its width");
    // already wide enough: a second call changes nothing and leaves no extra undo step
    let steps = e.undo.len();
    assert_eq!(e.widen_net_to_need("VIN"), 0);
    assert_eq!(e.undo.len(), steps);
    e.undo();
    assert!((e.board.tracks[0].width - 0.25).abs() < 1e-9);
}

#[test]
fn rounded_outlines_from_the_panel_helpers_are_undoable_and_keep_their_size() {
    let mut e = BoardEditor::default();
    e.size_r = 4.0;
    e.checkpoint();
    e.set_outline_box(0.0, 0.0, 40.0, 25.0);
    assert!(e.board.outline.len() > 8, "{}", e.board.outline.len());
    let (lo, hi) = e.board.bounds().unwrap();
    assert!((hi.x - lo.x - 40.0).abs() < 1e-6 && (hi.y - lo.y - 25.0).abs() < 1e-6);
    // radius 0 gives the plain rectangle again
    e.size_r = 0.0;
    e.set_outline_box(0.0, 0.0, 40.0, 25.0);
    assert_eq!(e.board.outline.len(), 4);
    // rounding the current outline, then undo
    e.size_r = 3.0;
    e.checkpoint();
    e.board.outline = pcb_core::outline::round_corners(&e.board.outline, e.size_r);
    assert!(e.board.outline.len() > 8);
    e.undo();
    assert_eq!(e.board.outline.len(), 4);
}

#[test]
fn right_click_in_the_board_editor_confirms_the_drawing_then_returns_to_select() {
    let mut e = BoardEditor::default();
    e.tool = Tool::Track;
    e.pending = Some(p(5.0, 5.0));
    assert!(e.leave_tool());
    assert!(matches!(e.tool, Tool::Track) && e.pending.is_none(), "the first right click ends the track, the tool stays");
    assert!(e.leave_tool() && matches!(e.tool, Tool::Select), "the second one returns to Select");
    // a polygon of 3+ points is closed by the right click, a shorter one is dropped
    e.tool = Tool::OutlinePoly;
    e.poly = vec![p(0.0, 0.0), p(20.0, 0.0), p(20.0, 10.0)];
    assert!(e.leave_tool() && e.poly.is_empty() && matches!(e.tool, Tool::OutlinePoly));
    assert_eq!(e.board.outline.len(), 3, "the polygon became the outline");
    e.poly.push(p(0.0, 0.0));
    assert!(e.leave_tool() && e.poly.is_empty() && e.board.outline.len() == 3);
    assert!(e.leave_tool() && matches!(e.tool, Tool::Select));
    assert!(!e.leave_tool());
}

#[test]
fn layer_count_changes_and_the_layer_key_walks_through_them() {
    let mut e = BoardEditor::default();
    assert_eq!(e.next_layer(), Side::Bottom, "two layers: top <-> bottom as before");
    e.set_layer_count(4);
    assert_eq!(e.board.copper_layers().len(), 4);
    assert_eq!(e.next_layer(), Side::Inner(1));
    e.side = Side::Inner(2);
    assert_eq!(e.next_layer(), Side::Bottom);
    assert_eq!(e.layer_order().last(), Some(&Side::Inner(2)), "the working layer is drawn last");
    // a part or pad is never put on an inner layer
    assert_eq!(e.side.outer(), Side::Top);
}

#[test]
fn fewer_layers_are_refused_while_copper_is_on_the_removed_ones() {
    let mut e = BoardEditor::default();
    e.set_layer_count(4);
    e.board.tracks.push(Track { side: Side::Inner(2), a: p(1.0, 1.0), b: p(5.0, 1.0), width: 0.25 });
    e.set_layer_count(2);
    assert_eq!(e.board.inner_layers, 2, "refused");
    assert!(e.status.contains("内層"), "{}", e.status);
    e.board.tracks.clear();
    e.side = Side::Inner(1);
    e.set_layer_count(2);
    assert_eq!((e.board.inner_layers, e.side), (0, Side::Top));
    e.undo();
    assert_eq!(e.board.inner_layers, 2, "undo brings the layers back");
}

#[test]
fn a_pair_is_added_routed_and_undone() {
    let mut e = BoardEditor::default();
    e.board = Board::new("t", 60.0, 30.0);
    for (x, net, y) in [(5.0, "USB_DP", 10.0), (5.0, "USB_DM", 11.0), (50.0, "USB_DP", 10.0), (50.0, "USB_DM", 11.0)] {
        let mut pad = Pad::new(p(x, y), PadShape::Rect { w: 0.3, h: 0.3 }, None, Side::Top);
        pad.net = net.into();
        e.board.pads.push(pad);
    }
    e.track_width = 0.2;
    e.grid = 0.0;
    let found = pcb_core::diffpair::suggest_pairs(&e.board_nets());
    assert_eq!(found.len(), 1);
    e.board.diff_pairs = found;
    e.route_pair_at(0);
    assert!(ratsnest(&e.board).is_empty(), "{}", e.status);
    assert!(pcb_core::diffpair::check_pairs(&e.board).iter().all(|v| !v.message.contains("片方だけ")));
    e.undo();
    assert!(e.board.tracks.is_empty(), "one undo removes the whole pair route");
}

fn two_net_editor() -> BoardEditor {
    let mut e = BoardEditor::default();
    e.board = Board::new("t", 40.0, 30.0);
    for (x, y, net) in [(5.0, 12.0, "B"), (35.0, 12.0, "B")] {
        let mut pad = Pad::new(p(x, y), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
        pad.net = net.into();
        e.board.pads.push(pad);
    }
    e.board.tracks.push(Track { side: Side::Top, a: p(5.0, 12.0), b: p(20.0, 12.0), width: 0.25 });
    e.board.tracks.push(Track { side: Side::Top, a: p(20.0, 12.0), b: p(35.0, 12.0), width: 0.25 });
    e.track_width = 0.25;
    e.grid = 0.0;
    e.route_mode = RouteMode::Free;
    e
}

#[test]
fn routing_next_to_another_net_pushes_it_aside_and_undo_restores_it() {
    let mut e = two_net_editor();
    let preview = e.route_preview(p(12.0, 11.65), p(28.0, 11.65));
    assert_eq!(preview.pushed, 2, "the preview says two tracks are pushed");
    // and shows where they would go: an end of each is pushed away from the new route, and the board is untouched
    assert_eq!(preview.moved.len(), 2);
    assert!(preview.moved.iter().all(|(old, new)| (new.a.y - old.a.y).abs().max((new.b.y - old.b.y).abs()) > 0.05), "{:?}", preview.moved);
    assert_eq!(e.board.tracks[0].b.y, 12.0, "looking does not move anything");
    e.pending = Some(p(12.0, 11.65));
    e.tool = Tool::Track;
    e.click(p(28.0, 11.65));
    assert!(drc::check(&e.board).is_empty(), "{:?}", drc::check(&e.board));
    assert!(e.status.contains("押しのけ"), "{}", e.status);
    e.undo();
    assert_eq!(e.board.tracks.len(), 2);
    assert_eq!(e.board.tracks[0].b.y, 12.0, "undo brings back the original position");
}

#[test]
fn shove_can_be_turned_off() {
    let mut e = two_net_editor();
    e.shove = false;
    e.pending = Some(p(12.0, 11.65));
    e.tool = Tool::Track;
    e.click(p(28.0, 11.65));
    assert_eq!(e.board.tracks[0].b.y, 12.0);
    assert!(!drc::check(&e.board).is_empty(), "the rule is broken, as drawn");
}

#[test]
fn a_track_dragged_with_the_mouse_follows_and_undoes_in_one_step() {
    let mut e = two_net_editor();
    e.board.tracks.push(Track { side: Side::Top, a: p(10.0, 8.0), b: p(30.0, 8.0), width: 0.25 });
    let before = e.undo.len();
    e.start_track_drag(2, p(20.0, 8.0));
    e.update_track_drag(p(20.0, 9.0));
    e.update_track_drag(p(20.0, 11.65 - 8.0 + 8.0)); // all the way up beside net B
    e.finish_track_drag();
    assert_eq!(e.undo.len(), before + 1);
    assert!(drc::check(&e.board).is_empty(), "{:?}", drc::check(&e.board));
    e.undo();
    assert_eq!(e.board.tracks[2].a.y, 8.0);
}

#[test]
fn a_drag_that_goes_nowhere_leaves_no_undo_step() {
    let mut e = two_net_editor();
    let before = e.undo.len();
    e.start_track_drag(0, p(10.0, 12.0));
    e.update_track_drag(p(10.0, 12.0));
    e.finish_track_drag();
    assert_eq!(e.undo.len(), before);
}

#[test]
fn ripping_up_a_net_from_the_selected_track() {
    let mut e = two_net_editor();
    e.rip_up_selected_net(0);
    assert!(e.board.tracks.is_empty());
    assert_eq!(ratsnest(&e.board).len(), 1);
    e.undo();
    assert_eq!(e.board.tracks.len(), 2);
}

#[test]
fn smoothing_the_tracks_is_one_undo_step_and_nothing_to_do_adds_none() {
    let mut e = BoardEditor::default();
    e.board = Board::new("t", 40.0, 30.0);
    e.board.tracks.push(Track { side: Side::Top, a: Point::new(5.0, 5.0), b: Point::new(25.0, 5.0), width: 0.25 });
    e.board.tracks.push(Track { side: Side::Top, a: Point::new(25.0, 5.0), b: Point::new(25.0, 20.0), width: 0.25 });
    e.smooth_tracks();
    assert!(e.board.tracks.len() > 2, "{}", e.status);
    let steps = e.undo.len();
    e.smooth_tracks();
    assert_eq!(e.undo.len(), steps, "nothing left to smooth: no undo step");
}

/// A board with one wide pad (3 x 0.6 mm) at (10, 10) and a small one (0.5 mm) far away at (30, 24).
fn pad_editor() -> BoardEditor {
    let mut e = BoardEditor::default();
    e.board = Board::new("t", 50.0, 40.0);
    for (x, y, w, h) in [(10.0, 10.0, 3.0, 0.6), (30.0, 24.0, 0.5, 0.5)] {
        let mut pad = Pad::new(Point::new(x, y), PadShape::Rect { w, h }, None, Side::Top);
        pad.net = "A".into();
        e.board.pads.push(pad);
    }
    e.tool = Tool::Track;
    e.track_width = 0.25;
    e.grid = 0.0;
    e.zoom = 20.0;
    e.shove = false;
    e
}

#[test]
fn a_long_pad_snaps_at_its_two_ends_and_a_small_one_only_at_its_centre() {
    let e = pad_editor();
    let ends = e.pad_end_points();
    // 3 mm wide: ends a track's width (and a little) in from the short edges; 0.6 mm tall and the 0.5 mm pad: no room for two tracks
    assert_eq!(ends.len(), 2, "{ends:?}");
    assert!(ends.iter().all(|q| (q.y - 10.0).abs() < 1e-9 && (q.x - 10.0).abs() > 1.0 && (q.x - 10.0).abs() < 1.5));
    let near_end = Point::new(ends[0].x + 0.1, ends[0].y + 0.1);
    assert_eq!(e.snap_object(near_end), Some(ends[0]));
    // with another tool there is no such snap
    let mut v = pad_editor();
    v.tool = Tool::Via;
    assert_ne!(v.snap_object(near_end), Some(ends[0]));
}

#[test]
fn a_route_leaves_and_enters_a_pad_at_a_right_angle_to_its_edge() {
    let e = pad_editor();
    let end = e.pad_end_points()[0]; // an end of the wide pad: out through the long side (up or down)
    let pts = e.route_points(end, Point::new(30.0, 24.0));
    // first the lead straight out of the pad's side (3 x 0.6 mm: half the height 0.3 plus 0.5 mm), in the direction of the target
    assert!((pts[0].x - end.x).abs() < 1e-9 && (pts[0].y - (end.y + 0.8)).abs() < 1e-9, "{pts:?}");
    // the end: a lead into the small pad, horizontal or vertical, ending on its centre
    let n = pts.len();
    assert_eq!(pts[n - 1], Point::new(30.0, 24.0));
    let (before, last) = (pts[n - 2], pts[n - 1]);
    assert!((before.x - last.x).abs() < 1e-9 || (before.y - last.y).abs() < 1e-9, "the way in is axis-aligned: {pts:?}");
    assert!(before.dist(last) >= 0.25 + 0.5 - 1e-9, "outside the 0.5 mm pad's edge by the lead: {pts:?}");
    // every leg is horizontal, vertical or at 45 degrees
    let mut from = end;
    for q in &pts {
        let (dx, dy) = ((q.x - from.x).abs(), (q.y - from.y).abs());
        assert!(dx < 1e-9 || dy < 1e-9 || (dx - dy).abs() < 1e-6, "{from:?} -> {q:?}");
        from = *q;
    }
}

#[test]
fn the_pad_lead_can_be_switched_off_and_does_nothing_in_the_free_mode_or_between_close_pads() {
    let mut e = pad_editor();
    let a = Point::new(10.0, 10.0);
    let b = Point::new(30.0, 24.0);
    let with = e.route_points(a, b).len();
    e.pad_lead = false;
    let plain = e.route_points(a, b);
    assert_eq!(plain, e.route_points_plain(a, b, e.bend_flip));
    assert!(with > plain.len());
    e.pad_lead = true;
    e.route_mode = RouteMode::Free;
    assert_eq!(e.route_points(a, b), vec![b]);
    e.route_mode = RouteMode::Angle45;
    // a pad 2 mm away: the leads would use it all up
    let mut near = pad_editor();
    near.board.pads[1].pos = Point::new(12.5, 10.0);
    assert_eq!(near.route_points(Point::new(10.0, 10.0), Point::new(12.5, 10.0)), near.route_points_plain(Point::new(10.0, 10.0), Point::new(12.5, 10.0), false));
}

#[test]
fn clicking_from_a_pad_lays_the_lead_and_the_tracks_stay_clear_of_other_pads() {
    let mut e = pad_editor();
    let a = Point::new(10.0, 10.0);
    let b = Point::new(30.0, 24.0);
    e.click(a);
    e.click(b);
    assert!(e.board.tracks.len() >= 3, "lead, bend(s), lead");
    assert_eq!(e.board.tracks[0].a, a);
    assert_eq!(e.board.tracks.last().unwrap().b, b);
    assert!(pcb_core::ratsnest::ratsnest(&e.board).is_empty(), "the two pads are connected");
}
