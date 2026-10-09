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
    e.board.holes.push(Hole { pos: p(30.0, 30.0), drill: 3.2 });
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
    assert!(matches!(e.hit_test(on_pad), Some(Item::Part(0))));
    e.vis.bottom = false;
    assert!(e.hit_test(on_pad).is_none());
}
