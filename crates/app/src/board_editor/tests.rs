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
    e.auto_route = true; // the shove belongs to the automatic route
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

/// The legs of a route from `from` through `pts`: (length, is axis-aligned or 45 degrees).
fn legs(from: Point, pts: &[Point]) -> Vec<(f64, bool)> {
    let mut out = vec![];
    let mut prev = from;
    for q in pts {
        let (dx, dy) = ((q.x - prev.x).abs(), (q.y - prev.y).abs());
        out.push((prev.dist(*q), dx < 1e-9 || dy < 1e-9 || (dx - dy).abs() < 1e-6));
        prev = *q;
    }
    out
}

#[test]
fn a_bend_whose_straight_part_would_be_a_hair_is_laid_as_straight_diagonal_straight() {
    let mut e = pad_editor();
    e.route_mode = RouteMode::Angle45;
    let a = Point::new(0.0, 0.0);
    // the two legs differ by 0.03 mm: a straight piece of 0.03 mm would be a bump, so three pieces, each long enough
    for (p, flip) in [(Point::new(10.0, 9.97), false), (Point::new(10.0, 9.97), true), (Point::new(-9.97, 10.0), false), (Point::new(-9.97, 10.0), true)] {
        let pts = e.route_points_plain(a, p, flip);
        assert_eq!(pts.len(), 3, "{pts:?}");
        assert_eq!(*pts.last().unwrap(), p);
        for (len, ok) in legs(a, &pts) {
            assert!(ok && len >= 0.3 - 1e-9, "{pts:?}: a leg of {len} mm (ok {ok})");
        }
    }
    // a clear difference, a grid point and a point too near for three pieces keep the old two-piece way
    assert_eq!(e.route_points_plain(a, Point::new(10.0, 4.0), false).len(), 2);
    assert_eq!(e.route_points_plain(a, Point::new(10.0, 10.0), false).len(), 1);
    assert_eq!(e.route_points_plain(a, Point::new(0.5, 0.47), false).len(), 2);
}

#[test]
fn a_route_from_a_corner_point_off_the_grid_has_no_tiny_piece() {
    let mut e = pad_editor();
    e.grid = 0.5;
    e.route_mode = RouteMode::Angle45;
    // the corners are off the 0.5 mm grid (11.375, 10.175): routes to grid points
    let corner = Point::new(11.375, 10.175);
    assert!(e.pad_end_points().iter().any(|q| q.dist(corner) < 1e-9));
    let mut checked = 0;
    for ix in 0..24 {
        for iy in 0..24 {
            let target = Point::new(16.0 + ix as f64 * 0.5, 14.0 + iy as f64 * 0.5);
            if target.dist(corner) < 4.0 {
                continue;
            }
            let pts = e.route_points(corner, target);
            for (len, ok) in legs(corner, &pts) {
                assert!(ok && len > 0.28, "to {target:?}: {pts:?} has a leg of {len} mm (axis or 45: {ok})");
            }
            checked += 1;
        }
    }
    assert!(checked > 400);
}

#[test]
fn a_big_pad_snaps_at_its_four_corners_flush_with_the_track_and_a_small_one_only_at_its_centre() {
    let e = pad_editor();
    let ends = e.pad_end_points();
    // 3 x 0.6 mm pad, 0.25 mm track: the corner points are where the track's edges are flush with the pad's edges
    // (half the track's width in from each edge); the 0.5 mm pad has no corner that differs from its centre
    assert_eq!(ends.len(), 4, "{ends:?}");
    let want = [(8.625, 9.825), (8.625, 10.175), (11.375, 9.825), (11.375, 10.175)];
    for (x, y) in want {
        assert!(ends.iter().any(|q| (q.x - x).abs() < 1e-9 && (q.y - y).abs() < 1e-9), "{ends:?}");
    }
    let near_corner = Point::new(ends[0].x + 0.1, ends[0].y + 0.1);
    assert_eq!(e.snap_object(near_corner), Some(ends[0]));
    // with another tool there is no such snap
    let mut v = pad_editor();
    v.tool = Tool::Via;
    assert_ne!(v.snap_object(near_corner), Some(ends[0]));
    // a thinner track moves the corners out (the edges stay flush) ...
    let mut thin = pad_editor();
    thin.track_width = 0.2;
    assert!(thin.pad_end_points().iter().any(|q| (q.x - 11.4).abs() < 1e-9 && (q.y - 10.2).abs() < 1e-9), "{:?}", thin.pad_end_points());
    // ... a thicker one moves them in, and where the pad is then too thin to tell top and bottom apart (0.4 mm track on
    // a 0.6 mm pad) the two corners become one point on the middle line
    let mut thick = pad_editor();
    thick.track_width = 0.4;
    let t = thick.pad_end_points();
    assert_eq!(t.len(), 2, "{t:?}");
    assert!(t.iter().all(|q| (q.y - 10.0).abs() < 1e-9 && ((q.x - 10.0).abs() - 1.3).abs() < 1e-9), "{t:?}");
}

/// Fine-pitch pads like the pins of a QFN: three 0.85 x 0.3 mm pads 0.65 mm apart, of three nets, and a far pad of the
/// middle net.
fn fine_pitch_editor() -> BoardEditor {
    let mut e = pad_editor();
    e.board.pads.clear();
    for (x, y, w, h, net) in [(20.0, 10.65, 0.85, 0.3, "RTRICK"), (20.0, 10.0, 0.85, 0.3, "GND"), (20.0, 9.35, 0.85, 0.3, "VREG"), (30.0, 24.0, 0.5, 0.5, "GND")] {
        let mut pad = Pad::new(Point::new(x, y), PadShape::Rect { w, h }, None, Side::Top);
        pad.net = net.into();
        e.board.pads.push(pad);
    }
    e
}

#[test]
fn the_warning_says_a_short_when_the_route_would_join_two_nets_and_a_gap_only_when_it_is_one() {
    // pads A1 and A2 of net A, a pad G of net GND to the right, and a track from A1 up to (10, 15)
    let mut e = pad_editor();
    e.board.pads.clear();
    for (x, y, net) in [(10.0, 10.0, "A"), (10.0, 20.0, "A"), (20.0, 15.0, "GND")] {
        let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
        p.net = net.into();
        e.board.pads.push(p);
    }
    e.board.tracks.push(Track { side: Side::Top, a: Point::new(10.0, 10.0), b: Point::new(10.0, 15.0), width: 0.25 });
    let end = Point::new(10.0, 15.0);
    // from the track of net A to the pad of net GND: the two nets would be joined, and the warning names them
    let short = e.route_preview(end, Point::new(20.0, 15.0));
    assert!(short.bad && short.why.contains("ショート") && short.why.contains('A') && short.why.contains("GND"), "{short:?}");
    // to the other pad of its own net: fine, and nothing is said
    let ok = e.route_preview(end, Point::new(10.0, 20.0));
    assert!(!ok.bad && ok.why.is_empty(), "{ok:?}");
    // a pad of another net 0.1 mm from the way (not touching it): it is the gap that is too small
    let mut near = Pad::new(Point::new(10.725, 17.5), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
    near.net = "B".into();
    e.board.pads.push(near);
    let gap = e.route_preview(end, Point::new(10.0, 20.0));
    assert!(gap.bad && gap.why.contains("間隔") && !gap.why.contains("ショート"), "{gap:?}");
}

#[test]
fn between_fine_pitch_pads_the_ends_leave_straight_out_of_the_end_and_never_through_the_long_side() {
    let e = fine_pitch_editor();
    let mid = Point::new(20.0, 10.0);
    // a thin pad has two points, its two ends (the track as wide as the pad is thin: one line)
    let ends = e.pad_end_points();
    let of_mid: Vec<&Point> = ends.iter().filter(|q| (q.y - 10.0).abs() < 1e-9).collect();
    assert_eq!(of_mid.len(), 2, "{ends:?}");
    assert!(of_mid.iter().all(|q| (q.x - 20.0).abs() > 0.25), "{of_mid:?}");
    // the ends of the middle pad would leave through a side that is 0.35 mm from the next pad: they go straight out of
    // the end, whichever way the cursor is
    for q in &of_mid {
        for target in [Point::new(20.0, 20.0), Point::new(20.0, 1.0), Point::new(q.x, 30.0)] {
            let lead = e.pad_lead_point(**q, target).expect("a way out");
            assert!((lead.y - 10.0).abs() < 1e-9 && (lead.x - 20.0).abs() > (q.x - 20.0).abs(), "{lead:?} out of {q:?} towards {target:?}");
        }
    }
    // from the middle of the pad with the cursor straight above or below: not up or down into the neighbours
    for target in [Point::new(20.0, 20.0), Point::new(20.0, 1.0), Point::new(20.1, 20.0)] {
        if let Some(lead) = e.pad_lead_point(mid, target) {
            assert!((lead.y - mid.y).abs() < 1e-9, "a lead {lead:?} out of the long side towards {target:?}");
        }
        for q in e.route_points(mid, target) {
            assert!(q.dist(Point::new(20.0, 10.65)) > 0.35 + 0.125 || q.y < 10.65 - 0.3, "{q:?} runs into the pad above");
        }
    }
    // the way out is still there when the cursor is beside the pad
    let lead = e.pad_lead_point(mid, Point::new(30.0, 10.0)).unwrap();
    assert!(lead.x > 20.425 + 0.4 && (lead.y - 10.0).abs() < 1e-9, "{lead:?}");
}

#[test]
fn a_corner_leaves_through_the_side_the_cursor_is_nearer_to_and_a_blocked_side_gives_way_to_the_free_one() {
    let mut e = pad_editor(); // the 3 x 0.6 mm pad at (10, 10) and a small one far away, all of net A
    let corner = Point::new(11.375, 10.175); // the top right corner: the track flush with the top and right edges
    assert!(e.pad_end_points().iter().any(|q| q.dist(corner) < 1e-9));
    // the cursor above: out through the top; the cursor to the right: out through the end
    let up = e.pad_lead_point(corner, Point::new(corner.x, corner.y + 10.0)).unwrap();
    assert!((up.x - corner.x).abs() < 1e-9 && (up.y - (corner.y + 0.125 + 0.5)).abs() < 1e-9, "{up:?}");
    let right = e.pad_lead_point(corner, Point::new(corner.x + 10.0, corner.y)).unwrap();
    assert!((right.y - corner.y).abs() < 1e-9 && (right.x - (corner.x + 0.125 + 0.5)).abs() < 1e-9, "{right:?}");
    // the cursor above, but a pad of another net just above the corner: the lead goes out through the end instead
    let mut other = Pad::new(Point::new(corner.x, corner.y + 1.0), PadShape::Rect { w: 1.0, h: 0.6 }, None, Side::Top);
    other.net = "B".into();
    e.board.pads.push(other);
    let way = e.pad_lead_point(corner, Point::new(corner.x, corner.y + 10.0)).unwrap();
    assert!((way.y - corner.y).abs() < 1e-9 && way.x > corner.x, "{way:?}");
}

#[test]
fn a_route_leaves_and_enters_a_pad_at_a_right_angle_to_its_edge() {
    let e = pad_editor();
    // the top right corner of the wide pad, the track flush with its edges: out through the end, towards the target
    let end = Point::new(11.375, 10.175);
    assert!(e.pad_end_points().iter().any(|q| q.dist(end) < 1e-9));
    let pts = e.route_points(end, Point::new(30.0, 24.0));
    // first the lead straight out of the pad's end (the track's half width plus 0.5 mm), in the direction of the target
    assert!((pts[0].y - end.y).abs() < 1e-9 && (pts[0].x - (end.x + 0.125 + 0.5)).abs() < 1e-9, "{pts:?}");
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

/// Two pads of net A with a wall of another net between them that has a gap at the top.
fn wall_editor() -> BoardEditor {
    let mut e = BoardEditor::default();
    e.board = Board::new("t", 40.0, 30.0);
    let mut add = |x: f64, y: f64, net: &str| {
        let mut pad = Pad::new(p(x, y), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
        pad.net = net.into();
        e.board.pads.push(pad);
    };
    add(5.0, 15.0, "A");
    add(35.0, 15.0, "A");
    for k in 0..13 {
        add(20.0, 3.0 + k as f64, "W");
    }
    e.tool = Tool::Track;
    e.track_width = 0.25;
    e.grid = 0.5;
    e.zoom = 20.0;
    e.pad_lead = false;
    e.route_mode = RouteMode::Angle45;
    e
}

#[test]
fn the_automatic_route_goes_round_the_wall_and_the_manual_one_does_not_look() {
    let mut e = wall_editor();
    let (a, z) = (p(5.0, 15.0), p(35.0, 15.0));
    // manual: the plain line, straight through the wall (shown red, no search)
    let manual = e.route_preview(a, z);
    assert!(!manual.found && manual.bad, "{manual:?}");
    // automatic: a route round the wall
    e.auto_route = true;
    let auto = e.route_preview(a, z);
    assert!(auto.found && !auto.bad, "{auto:?}");
    assert_eq!(auto.tracks.first().unwrap().a, a);
    assert_eq!(auto.tracks.last().unwrap().b, z);
    assert_eq!(e.board.tracks.len(), 0, "looking does not lay anything");
    // clicking lays it, and the board stays clean and connected; one undo step
    e.pending = Some(a);
    e.click(z);
    assert!(drc::check(&e.board).is_empty(), "{:?}", drc::check(&e.board));
    assert!(ratsnest(&e.board).is_empty(), "the two pads are connected");
    assert!(e.status.contains("自動経路"), "{}", e.status);
    e.undo();
    assert!(e.board.tracks.is_empty());
}

#[test]
fn the_automatic_route_is_searched_only_towards_something_to_connect_to() {
    let mut e = wall_editor();
    e.auto_route = true;
    let a = p(5.0, 15.0);
    // the cursor is on an empty spot: the plain line (and, with the shove on, no search)
    let free = e.route_preview(a, p(30.0, 10.0));
    assert!(!free.found);
    assert!(e.on_object(p(35.0, 15.0)) && !e.on_object(p(30.0, 10.0)));
    // the router is built once for the board as it is, and again when the copper changes
    let r1 = e.router().unwrap();
    assert!(std::rc::Rc::ptr_eq(&r1, &e.router().unwrap()));
    e.board.tracks.push(Track { side: Side::Top, a: p(1.0, 1.0), b: p(2.0, 1.0), width: 0.25 });
    assert!(!std::rc::Rc::ptr_eq(&r1, &e.router().unwrap()));
}

#[test]
fn without_the_automatic_route_the_shove_is_not_used_in_routing() {
    let mut e = two_net_editor();
    e.auto_route = false;
    let preview = e.route_preview(p(12.0, 11.65), p(28.0, 11.65));
    assert_eq!(preview.pushed, 0, "manual routing leaves the other nets where they are");
    assert!(preview.bad, "and says the clearance is broken");
}

#[test]
fn the_automatic_route_goes_round_the_wall_with_the_pad_leads_on_too() {
    let mut e = wall_editor();
    e.pad_lead = true;
    e.auto_route = true;
    let (a, z) = (p(5.0, 15.0), p(35.0, 15.0));
    let auto = e.route_preview(a, z);
    println!("{:?}", auto.tracks);
    assert!(auto.found && !auto.bad, "{auto:?}");
    assert!(auto.tracks.iter().any(|t| t.a.y > 15.7 || t.b.y > 15.7), "{:?}", auto.tracks);
}

#[test]
fn lengths_are_matched_with_one_undo_step() {
    let mut e = BoardEditor::default();
    e.board = Board::new("t", 60.0, 40.0);
    for (net, y, x1) in [("A", 10.0, 50.0), ("B", 20.0, 35.0)] {
        for x in [5.0, x1] {
            let mut pad = Pad::new(p(x, y), PadShape::Rect { w: 0.8, h: 0.8 }, None, Side::Top);
            pad.net = net.into();
            e.board.pads.push(pad);
        }
        e.board.tracks.push(Track { side: Side::Top, a: p(5.0, y), b: p(x1, y), width: 0.25 });
    }
    let nets = vec!["A".to_string(), "B".to_string()];
    e.match_lengths(&nets, None);
    let (la, lb) = (pcb_core::meander::length_of(&e.board, "A"), pcb_core::meander::length_of(&e.board, "B"));
    assert!((la - lb).abs() < 0.2, "{la} {lb}: {}", e.status);
    assert!(e.status.contains("長さ合わせ"), "{}", e.status);
    assert!(drc::check(&e.board).is_empty());
    e.undo();
    assert_eq!(e.board.tracks.len(), 2, "one undo step brings back the straight lines");
    // nothing to do: no undo step is left behind
    let steps = e.undo.len();
    e.match_lengths(&["A".to_string()], None);
    assert_eq!(e.undo.len(), steps);
}

#[test]
fn the_autorouter_leaves_and_enters_pads_at_a_right_angle_when_the_pad_lead_is_on() {
    let build = |lead: bool| {
        let mut e = BoardEditor::default();
        e.board = Board::new("t", 40.0, 30.0);
        for (x, y) in [(5.0, 5.0), (30.0, 20.0)] {
            let mut pad = Pad::new(p(x, y), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
            pad.net = "A".into();
            e.board.pads.push(pad);
        }
        e.track_width = 0.25;
        e.grid = 0.5;
        e.pad_lead = lead;
        e.autoroute_now(None);
        e
    };
    let axis_aligned = |t: &Track| (t.a.x - t.b.x).abs() < 1e-9 || (t.a.y - t.b.y).abs() < 1e-9;
    let on = build(true);
    assert!(pcb_core::ratsnest::ratsnest(&on.board).is_empty(), "{}", on.status);
    assert!(drc::check(&on.board).is_empty(), "{:?}", drc::check(&on.board));
    // the track at each pad runs straight out of its edge for at least the half pad plus the lead
    for pad in [p(5.0, 5.0), p(30.0, 20.0)] {
        let t = on.board.tracks.iter().find(|t| t.a == pad || t.b == pad).unwrap();
        assert!(axis_aligned(t) && t.a.dist(t.b) >= 1.0 - 1e-6, "{t:?}");
    }
    // with the setting off, the router's own shape is left (it may end at any angle): no pad is aligned afterwards either
    let off = build(false);
    assert!(!off.status.contains("垂直"), "{}", off.status);
}

/// Frame time of the board editor with a big imported board: `cargo test --release -p app board_frame_time -- --ignored --nocapture`
#[test]
#[ignore]
fn board_frame_time() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut sch = crate::schematic::SchEditor::default();
    sch.load_path(root.join("mixed_signal.schjson"));
    let mut e = BoardEditor::default();
    e.import_from_schematic(&sch);
    println!("parts {} pads {} outline {}", e.board.parts.len(), e.board.pads.len(), e.board.outline.len());
    let ctx = egui::Context::default();
    let frame = |e: &mut BoardEditor, label: &str| {
        let mut worst = 0.0f64;
        let mut total = 0.0;
        for _ in 0..20 {
            let t = std::time::Instant::now();
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() }, |ui| {
                e.canvas(ui);
            });
            out.textures_delta.clear();
            let n_shapes = out.shapes.len();
            let prims = ctx.tessellate(out.shapes, out.pixels_per_point);
            let tris: usize = prims.iter().map(|p| match &p.primitive { egui::epaint::Primitive::Mesh(m) => m.indices.len() / 3, _ => 0 }).sum();
            if worst == 0.0 {
                println!("  shapes {n_shapes}, triangles {tris}");
            }
            let dt = t.elapsed().as_secs_f64() * 1000.0;
            worst = worst.max(dt);
            total += dt;
        }
        println!("{label}: avg {:.1} ms, worst {:.1} ms", total / 20.0, worst);
    };
    frame(&mut e, "imported");
    let mut panel_ms = 0.0;
    for _ in 0..20 {
        let t = std::time::Instant::now();
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() }, |ui| {
            e.side_panel(ui, &sch);
        });
        out.textures_delta.clear();
        panel_ms += t.elapsed().as_secs_f64() * 1000.0;
    }
    println!("side panel avg {:.1} ms", panel_ms / 20.0);
    e.fit_view(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0)));
    frame(&mut e, "fitted");
    e.vis.nets = true;
    frame(&mut e, "with net names");
    let t = std::time::Instant::now();
    let r = pcb_core::ratsnest::ratsnest(&e.board);
    println!("ratsnest {} lines {:.1} ms", r.len(), t.elapsed().as_secs_f64() * 1000.0);
    let t = std::time::Instant::now();
    let c = pcb_core::drc::connectivity(&e.board);
    let _ = c;
    println!("connectivity {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
    let t = std::time::Instant::now();
    let v = pcb_core::drc::check(&e.board);
    println!("drc {} violations {:.1} ms", v.len(), t.elapsed().as_secs_f64() * 1000.0);
}

/// Frame time while the pointer moves, drags a part and routes: `cargo test -p app board_interaction_time -- --ignored --nocapture`
#[test]
#[ignore]
fn board_interaction_time() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut sch = crate::schematic::SchEditor::default();
    sch.load_path(root.join("mixed_signal.schjson"));
    let mut e = BoardEditor::default();
    e.import_from_schematic(&sch);
    e.board.set_rect_outline(0.0, 0.0, 80.0, 60.0);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0));
    e.fit_view(screen);
    let ctx = egui::Context::default();
    let frame = |e: &mut BoardEditor, events: Vec<egui::Event>| -> f64 {
        let t = std::time::Instant::now();
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| {
            e.canvas(ui);
        });
        out.textures_delta.clear();
        let _ = ctx.tessellate(out.shapes, out.pixels_per_point);
        t.elapsed().as_secs_f64() * 1000.0
    };
    frame(&mut e, vec![]);
    let t0 = std::time::Instant::now();
    for _ in 0..10 {
        e.rev += 1;
        let _ = e.schematic_diff(&sch).nets;
    }
    println!("schematic_diff when the board changed: {:.1} ms each", t0.elapsed().as_secs_f64() * 100.0);
    let at = |e: &BoardEditor, p: Point| e.to_screen(screen, p);
    let u1 = e.board.pads.iter().find(|p| p.refdes == "U1").map(|p| p.pos).unwrap();
    let u3 = e.board.pads.iter().find(|p| p.refdes == "U3").map(|p| p.pos).unwrap();
    let report = |label: &str, times: Vec<f64>| {
        let avg = times.iter().sum::<f64>() / times.len() as f64;
        let worst = times.iter().cloned().fold(0.0, f64::max);
        println!("{label}: avg {avg:.1} ms, worst {worst:.1} ms");
    };
    // 1: hovering over the board
    let mut t = vec![];
    for i in 0..30 {
        let p = at(&e, Point::new(u1.x + i as f64 * 0.3, u1.y + i as f64 * 0.2));
        t.push(frame(&mut e, vec![egui::Event::PointerMoved(p)]));
    }
    report("hover (select tool)", t);
    // 2: dragging a part
    let start = at(&e, u1);
    let mut t = vec![];
    t.push(frame(&mut e, vec![egui::Event::PointerMoved(start), egui::Event::PointerButton { pos: start, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }]));
    for i in 1..30 {
        let p = start + egui::vec2(i as f32 * 3.0, i as f32 * 2.0);
        t.push(frame(&mut e, vec![egui::Event::PointerMoved(p)]));
    }
    report("drag (select tool)", t);
    let end = start + egui::vec2(90.0, 60.0);
    frame(&mut e, vec![egui::Event::PointerButton { pos: end, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }]);
    // 3: routing with the auto path on
    e.tool = Tool::Track;
    e.auto_route = true;
    e.shove = true;
    let a = at(&e, u1);
    let b = at(&e, u3);
    let click = |p: egui::Pos2, pressed| vec![egui::Event::PointerMoved(p), egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() }];
    frame(&mut e, click(a, true));
    frame(&mut e, click(a, false));
    let mut t = vec![];
    for i in 1..=30 {
        let p = a + (b - a) * (i as f32 / 30.0);
        t.push(frame(&mut e, vec![egui::Event::PointerMoved(p)]));
    }
    report("routing preview (auto path + shove)", t);
}

#[test]
fn the_first_route_asks_for_the_currents_and_routes_a_heavy_net_wide() {
    let mut e = BoardEditor::default();
    e.board = Board::new("t", 60.0, 40.0);
    for (x, y) in [(10.0, 10.0), (45.0, 25.0)] {
        let mut pad = Pad::new(p(x, y), PadShape::Rect { w: 3.0, h: 3.0 }, None, Side::Top);
        pad.net = "A".into();
        e.board.pads.push(pad);
    }
    e.track_width = 0.25;
    e.grid = 0.5;
    e.autoroute(None);
    // nothing is drawn yet: the editor asked `App` for the schematic's DC currents and will route when they are in
    assert_eq!(e.currents_request, Some(CurrentSource::Dc));
    assert_eq!(e.route_after_currents, Some(None));
    assert!(e.board.tracks.is_empty());
    // App answers: 5 A on net A
    e.currents_request = None;
    e.use_simulated_currents(Ok(vec![("A".into(), 5.0)]), vec![]);
    let net = e.route_after_currents.take().unwrap();
    e.autoroute_now(net.as_deref());
    assert!(pcb_core::ratsnest::ratsnest(&e.board).is_empty(), "{}", e.status);
    assert!(e.board.tracks.iter().all(|t| t.width >= 2.0), "5 A needs a wide track: {:?}", e.board.tracks.iter().map(|t| t.width).collect::<Vec<_>>());
    assert!(e.status.contains("必要な太さ"), "{}", e.status);
    // once currents are declared (or tried), the next route does not ask again
    e.board.tracks.clear();
    e.autoroute(None);
    assert!(e.currents_request.is_none() && !e.board.tracks.is_empty());
}

#[test]
fn a_route_without_a_circuit_goes_ahead_after_one_try() {
    let mut e = BoardEditor::default();
    e.board = Board::new("t", 40.0, 30.0);
    for (x, y) in [(5.0, 5.0), (30.0, 20.0)] {
        let mut pad = Pad::new(p(x, y), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
        pad.net = "A".into();
        e.board.pads.push(pad);
    }
    e.autoroute(None);
    assert_eq!(e.currents_request, Some(CurrentSource::Dc));
    // App could not get currents (no solvable circuit): the route is made anyway, and the next press goes straight to it
    e.currents_request = None;
    e.use_simulated_currents(Err("no circuit".into()), vec![]);
    let net = e.route_after_currents.take().unwrap();
    e.autoroute_now(net.as_deref());
    assert!(!e.board.tracks.is_empty());
    e.board.tracks.clear();
    e.autoroute(None);
    assert!(e.currents_request.is_none() && !e.board.tracks.is_empty());
}

/// Drags part `refdes` so that each of its pads is between 2 mm and `max` mm from the pad it should be near (`ends`: own pin, target
/// part, target pin): tries the valid spots (inside the
/// outline, no rule violation) one after another, spread over the ring, and keeps the first one where the fine-grid auto routing of the
/// whole board still leaves nothing open (a learner tries and undoes the same way). Panics if there is none.
fn drag_next_to(e: &mut BoardEditor, refdes: &str, ends: &[(&str, (&str, &str))], max: f64) {
    let pad = |b: &Board, r: &str, p: &str| b.pads.iter().find(|x| x.refdes == r && x.pin == p).map(|x| x.pos).unwrap();
    let dist = |a: Point, c: Point| ((a.x - c.x).powi(2) + (a.y - c.y).powi(2)).sqrt();
    let goal = pad(&e.board, (ends[0].1).0, (ends[0].1).1);
    let rot = e.board.parts.iter().find(|p| p.refdes == refdes).unwrap().rot;
    let mut valid = vec![];
    for dx in -9..=9 {
        for dy in -9..=9 {
            let at = Point::new(goal.x + dx as f64, goal.y + dy as f64);
            let mut t = e.board.clone();
            t.place_part(refdes, at, rot);
            let ds: Vec<f64> = ends.iter().map(|(own, tgt)| dist(pad(&t, refdes, own), pad(&t, tgt.0, tgt.1))).collect();
            let fits = ds.iter().all(|d| *d >= 2.0 && *d <= max);
            let inside = t.pads.iter().filter(|p| p.refdes == refdes).all(|p| inside_polygon(&t.outline, p.pos));
            let clean = pcb_core::drc::check(&t).iter().all(|v| !v.message.contains(refdes));
            if fits && inside && clean {
                valid.push(at);
            }
        }
    }
    assert!(!valid.is_empty(), "no free spot for {refdes} near {ends:?}");
    for at in valid.iter().step_by((valid.len() / 8).max(1)) {
        let mut t = e.board.clone();
        t.place_part(refdes, *at, rot);
        let mut probe = BoardEditor::default();
        probe.board = t.clone();
        probe.grid = 0.1;
        probe.autoroute_now(None);
        if ratsnest(&probe.board).is_empty() {
            e.checkpoint();
            e.board = t;
            return;
        }
    }
    panic!("every spot for {refdes} near {ends:?} makes the board unroutable");
}

/// The placement lesson of the Arduino-compatible board: the auto placement leaves the crystal and the bypass capacitors far from
/// the microcontroller (14 - 16 mm), so two steps are the learner's own; the default grid leaves connections open, a finer one routes them.
#[test]
fn the_arduino_board_lesson_walks_from_import_to_the_order_window() {
    let mut sch = crate::schematic::SchEditor::default();
    assert!(sch.load_sample(BoardEditor::arduino_lesson_circuit()));
    let mut e = BoardEditor::default();
    e.start_arduino_lesson();
    assert_eq!(e.lesson_progress(), 0);
    e.import_from_schematic(&sch);
    assert_eq!(e.lesson_progress(), 1);
    e.autoplace();
    assert_eq!(e.lesson_progress(), 2);
    // the auto placement is only a first draft: the crystal and the bypass capacitors are not near their pins yet
    let far = |e: &BoardEditor, a: (&str, &str), b: (&str, &str)| {
        let at = |r: &str, p: &str| e.board.pads.iter().find(|x| x.refdes == r && x.pin == p).map(|x| x.pos).unwrap();
        let (p, q) = (at(a.0, a.1), at(b.0, b.1));
        ((p.x - q.x).powi(2) + (p.y - q.y).powi(2)).sqrt()
    };
    assert!(far(&e, ("Y1", "1"), ("U1", "7")) > 9.0, "the crystal starts {} mm from XTAL1", far(&e, ("Y1", "1"), ("U1", "7")));
    drag_next_to(&mut e, "Y1", &[("1", ("U1", "7")), ("3", ("U1", "8"))], 8.0);
    assert!(far(&e, ("Y1", "3"), ("U1", "8")) <= 9.0, "the other end follows: {}", far(&e, ("Y1", "3"), ("U1", "8")));
    assert_eq!(e.lesson_progress(), 3, "the crystal is near");
    drag_next_to(&mut e, "C3", &[("1", ("U1", "4"))], 5.0);
    assert_eq!(e.lesson_progress(), 3, "only the VCC capacitor is near: the AVCC pin has none yet");
    drag_next_to(&mut e, "C4", &[("1", ("U1", "18"))], 5.0);
    assert_eq!(e.lesson_progress(), 4, "the bypass capacitors are near");
    // the default grid (0.5 mm) may leave connections of this dense board open (on the plain auto placement it leaves two); the
    // tracks it did lay then get in the way of a second try, so the learner takes them back (Ctrl+Z) and routes with a finer grid
    e.autoroute_now(None);
    if !ratsnest(&e.board).is_empty() {
        assert_eq!(e.lesson_progress(), 4, "{}", e.status);
        e.undo();
        assert!(e.board.tracks.is_empty(), "Ctrl+Z took the coarse routing back");
        e.grid = 0.1;
        e.autoroute_now(None);
    }
    assert!(ratsnest(&e.board).is_empty(), "{}", e.status);
    assert_eq!(e.lesson_progress(), 5, "{}", e.status);
    e.run_drc();
    assert_eq!(e.lesson_progress(), 6, "{}", e.status);
    e.open_order();
    assert_eq!(e.lesson_progress(), 7);
    assert_eq!(e.finished_board_lesson(), Some("board_arduino"));
}

/// The USB lesson: the two pairs are found by name, routed side by side (D+ first, then D- next to it), the rest is auto-routed.
#[test]
fn the_usb_board_lesson_walks_from_import_to_the_order_window() {
    let mut sch = crate::schematic::SchEditor::default();
    assert!(sch.load_sample(BoardEditor::usb_lesson_circuit()));
    let mut e = BoardEditor::default();
    e.start_usb_lesson();
    assert_eq!(e.lesson_progress(), 0);
    e.import_from_schematic(&sch);
    assert_eq!(e.lesson_progress(), 1);
    e.autoplace();
    assert_eq!(e.lesson_progress(), 2);
    // "名前から探す"
    let found = pcb_core::diffpair::suggest_pairs(&e.board_nets());
    assert_eq!(found.iter().map(|d| d.p.as_str()).collect::<Vec<_>>().len(), 2, "USB_DP/USB_DM and MCU_DP/MCU_DM: {found:?}");
    e.board.diff_pairs.extend(found);
    assert_eq!(e.lesson_progress(), 3);
    // the automatic placement puts the resistors beside each other: the two lines of a pair do not run the same way, so the step
    // that asks for a placement fit for a pair is not done yet (this is what the learner fixes by dragging)
    assert_eq!(e.lesson_progress(), 3, "the auto placement does not fit the pairs");
    e.board.place_part("J1", Point::new(14.0, 6.0), 0);
    e.board.place_part("R2", Point::new(13.0, 11.0), 1);
    e.board.place_part("R1", Point::new(15.0, 11.0), 1);
    e.board.place_part("J2", Point::new(9.9, 19.0), 1);
    e.board.place_part("C1", Point::new(22.0, 12.0), 0);
    assert_eq!(e.lesson_progress(), 4, "D+ and D- can now run side by side");
    for i in 0..e.board.diff_pairs.len() {
        e.route_pair_at(i);
    }
    assert_eq!(e.lesson_progress(), 5, "{}", e.status);
    e.autoroute_now(None);
    assert_eq!(e.lesson_progress(), 6, "{}", e.status);
    // the DRC checks the pairs too: the connector's pads are 2 mm apart, so the lines keep about 1.75 mm between them, far from the
    // 0.2 mm the pair is asked for; the learner either routes them closer by hand or sets the gap the pair really has
    e.run_drc();
    assert!(e.violations.iter().any(|v| v.message.contains("間隔")), "{:?}", e.violations.iter().map(|v| v.message.clone()).collect::<Vec<_>>());
    assert_eq!(e.lesson_progress(), 6, "the DRC is not clean yet");
    for i in 0..e.board.diff_pairs.len() {
        let r = pcb_core::diffpair::pair_report(&e.board, &e.board.diff_pairs[i].clone());
        e.board.diff_pairs[i].gap = r.gap_min.unwrap();
    }
    e.run_drc();
    assert_eq!(e.lesson_progress(), 7, "{:?}", e.violations.iter().map(|v| v.message.clone()).collect::<Vec<_>>());
    e.open_order();
    assert_eq!(e.lesson_progress(), 8);
    assert_eq!(e.finished_board_lesson(), Some("board_usb"));
}

/// Writes the Mega board as far as the autorouter gets (`docs/examples/work_in_progress/`): the parts as the autoplacer puts them, the
/// GND pour, then the auto routing with the 0.1 mm grid. The open nets are left for the learner to route by hand in the board editor.
/// `cargo test -p app write_mega_open_board -- --ignored --nocapture`
#[test]
#[ignore]
fn write_mega_open_board() {
    let text = include_str!("../../../../docs/examples/arduino_mega_usb.schjson");
    let mut sch = crate::schematic::SchEditor::default();
    assert!(sch.load_sample(text));
    let mut e = BoardEditor::default();
    e.import_from_schematic(&sch);
    e.board.name = "arduino_mega_usb".into();
    e.ground_pour();
    e.autoplace();
    e.grid = 0.1;
    e.autoroute_now(None);
    e.run_drc();
    let open = ratsnest(&e.board);
    let mut nets: Vec<String> = open.iter().map(|w| w.net.clone()).collect();
    nets.sort();
    nets.dedup();
    println!("unrouted {} connections in {} nets: {nets:?}; vias {}; drc {}", open.len(), nets.len(), e.board.vias.len(), e.violations.len());
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/work_in_progress");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("arduino_mega_usb_open.pcbjson"), serde_json::to_string_pretty(&e.board).unwrap()).unwrap();
}
