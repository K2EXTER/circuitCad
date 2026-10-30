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
    set_poly(&mut e, &vec![p(0.0, 0.0), p(20.0, 0.0), p(20.0, 10.0), p(10.0, 10.0), p(10.0, 20.0), p(0.0, 20.0)]);
    e.finish_polygon();
    assert_eq!(e.board.outline.len(), 6, "an L-shaped board");
    e.undo();
    assert_eq!(e.board.outline.len(), 4);

    // zone: the duplicate last point from a double-click is dropped
    e.tool = Tool::Zone;
    e.zone_net = "GND".into();
    set_poly(&mut e, &vec![p(1.0, 1.0), p(9.0, 1.0), p(9.0, 9.0), p(9.0, 9.0)]);
    e.finish_polygon();
    assert_eq!(e.board.zones.len(), 1);
    assert_eq!(e.board.zones[0].outline().len(), 3);
    assert_eq!((e.board.zones[0].net.as_str(), e.board.zones[0].side), ("GND", Side::Top));
}

#[test]
fn a_shape_that_takes_away_is_a_child_of_the_pour_and_goes_with_undo() {
    let mut e = BoardEditor::default();
    e.tool = Tool::Zone;
    e.zone_net = "VNEG".into();
    set_poly(&mut e, &[p(0.0, 0.0), p(20.0, 0.0), p(20.0, 20.0), p(0.0, 20.0)]);
    e.finish_polygon();
    assert_eq!(e.selected, Some(Item::Zone(0)), "the new pour is selected, to go on drawing in it");
    // take away: no net to choose, and no new pour
    e.selected = None;
    e.zone_op = pcb_core::model::ShapeOp::Sub;
    set_poly(&mut e, &[p(5.0, 5.0), p(8.0, 5.0), p(8.0, 8.0), p(5.0, 8.0)]);
    e.finish_polygon();
    assert_eq!(e.board.zones.len(), 1, "a hole is not a new pour");
    assert_eq!(e.board.zones[0].holes().len(), 1);
    e.undo();
    assert!(e.board.zones[0].holes().is_empty(), "the hole goes with undo");
    // one net only: a pour of another net is left alone and the status says so
    e.hole_net = "GND".into();
    set_poly(&mut e, &[p(5.0, 5.0), p(8.0, 5.0), p(8.0, 8.0), p(5.0, 8.0)]);
    e.finish_polygon();
    assert!(e.board.zones[0].holes().is_empty() && e.status.contains("ありません"));
    // a polygon drawn with clicks closes by clicking the first point
    e.hole_net.clear();
    for q in [p(10.0, 10.0), p(12.0, 10.0), p(12.0, 12.0), p(10.0, 10.0)] {
        e.click(q);
    }
    assert_eq!(e.board.zones[0].holes().len(), 1);
}

#[test]
fn a_shape_drawn_with_a_pour_selected_is_added_to_that_pour_and_keeps_its_arcs() {
    let mut e = BoardEditor::default();
    e.tool = Tool::Zone;
    e.zone_net = "GND".into();
    set_poly(&mut e, &[p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(0.0, 10.0)]);
    e.finish_polygon();
    // a circle beside it, drawn while the pour is selected: a second shape of the same pour, two arcs
    e.poly_mode = PolyMode::Circle;
    e.click(p(14.0, 5.0));
    e.click(p(17.0, 5.0));
    assert_eq!(e.board.zones.len(), 1);
    let z = &e.board.zones[0];
    assert_eq!(z.shapes.len(), 2);
    assert_eq!((z.shapes[1].shape.verts.len(), z.shapes[1].op), (2, pcb_core::model::ShapeOp::Add));
    assert!(z.shapes[1].shape.verts.iter().all(|v| v.arc_mid.is_some()), "the circle is kept as arcs");
    assert_eq!(z.region().len(), 2, "the two shapes are apart: two pieces");
    // a shape drawn over the arc mode: the arc is kept as the point it passes through
    e.poly_mode = PolyMode::Lines;
    e.selected = None;
    e.click(p(30.0, 0.0));
    e.click(p(40.0, 0.0));
    e.poly_mode = PolyMode::ArcThrough;
    e.click(p(43.0, 5.0));
    e.click(p(40.0, 10.0));
    e.poly_mode = PolyMode::Lines;
    e.click(p(30.0, 10.0));
    e.click(p(30.0, 0.0));
    assert_eq!(e.board.zones.len(), 2, "no pour was selected: a pour of its own");
    let arcs = e.board.zones[1].shapes[0].shape.verts.iter().filter(|v| v.arc_mid.is_some()).count();
    assert_eq!(arcs, 1);
    // the bake button's work: the shapes become straight corners, the area stays
    let before = e.board.zones[1].region();
    assert!(pcb_core::zonefill::bake_zone(&mut e.board, 1));
    assert!(e.board.zones[1].shapes.iter().all(|zs| zs.shape.verts.iter().all(|v| v.arc_mid.is_none())));
    let after = e.board.zones[1].region();
    assert!((area(&before[0].outer) - area(&after[0].outer)).abs() < 0.5);
}

/// A polygon of straight lines as if its corners had been clicked (for the tests that finish it by hand).
fn set_poly(e: &mut BoardEditor, pts: &[Point]) {
    e.poly = pts.to_vec();
    e.poly_verts = pts.iter().map(|q| pcb_core::shape::Vertex::line(*q)).collect();
}

fn area(poly: &[Point]) -> f64 {
    poly.iter().zip(poly.iter().cycle().skip(1)).map(|(a, b)| a.x * b.y - b.x * a.y).sum::<f64>().abs() / 2.0
}

#[test]
fn a_shape_of_lines_and_an_arc_is_confirmed_as_a_pour() {
    let mut e = BoardEditor::default();
    e.tool = Tool::Zone;
    e.zone_net = "GND".into();
    // (0,0) -> (10,0) by lines, then an arc through (13,5) to (10,10), then a line to (0,10), closed on the first point
    e.click(p(0.0, 0.0));
    e.click(p(10.0, 0.0));
    e.poly_mode = PolyMode::ArcThrough;
    e.click(p(13.0, 5.0));
    assert_eq!(e.arc_mid, Some(p(13.0, 5.0)));
    e.click(p(10.0, 10.0));
    assert!(e.arc_mid.is_none() && e.poly.len() > 4, "the arc is many short pieces: {}", e.poly.len());
    e.poly_mode = PolyMode::Lines;
    e.click(p(0.0, 10.0));
    e.click(p(0.0, 0.0));
    assert_eq!(e.board.zones.len(), 1);
    let z = &e.board.zones[0];
    let outline = z.shapes[0].shape.flatten();
    assert!(area(&outline) > 100.0 + 5.0, "the arc bulges out of the 10 x 10 square: {}", area(&outline));
    assert!(outline.iter().any(|q| q.x > 11.0), "points of the arc");
    assert_eq!(z.shapes[0].shape.verts.iter().filter(|v| v.arc_mid == Some(p(13.0, 5.0))).count(), 1, "the arc keeps its middle point");
}

#[test]
fn a_tangent_arc_follows_the_last_line() {
    let mut e = BoardEditor::default();
    e.tool = Tool::Zone;
    e.zone_net = "GND".into();
    e.poly_mode = PolyMode::ArcTangent;
    e.click(p(0.0, 0.0)); // the start: a tangent arc has no direction yet
    e.poly_mode = PolyMode::Lines;
    e.click(p(10.0, 0.0));
    e.poly_mode = PolyMode::ArcTangent;
    e.click(p(15.0, 5.0)); // leaves to the right, turns up: a quarter circle of radius 5
    assert!(e.poly.len() > 3);
    let first = e.poly[2];
    assert!(first.y.abs() < 0.2 && first.x > 10.0, "leaves along the line: {first:?}");
    assert!(e.poly.last().unwrap().dist(p(15.0, 5.0)) < 1e-9);
}

#[test]
fn a_circle_and_a_rounded_rectangle_are_confirmed_at_once() {
    let mut e = BoardEditor::default();
    e.tool = Tool::Zone;
    e.zone_net = "GND".into();
    e.poly_mode = PolyMode::Circle;
    e.click(p(10.0, 10.0));
    assert!(e.board.zones.is_empty(), "the centre alone is not a shape");
    e.click(p(15.0, 10.0));
    assert_eq!(e.board.zones.len(), 1);
    assert!((area(&e.board.zones[0].outline()) - std::f64::consts::PI * 25.0).abs() < 1.0);
    e.selected = None; // no pour selected: the next shape is a pour of its own
    e.poly_mode = PolyMode::Rect;
    e.shape_round = 2.0;
    e.click(p(30.0, 0.0));
    e.click(p(50.0, 10.0));
    assert_eq!(e.board.zones.len(), 2);
    let r = &e.board.zones[1].outline();
    assert!(r.len() > 4 && area(r) < 200.0 && area(r) > 190.0, "rounded corners: {} points, area {}", r.len(), area(r));
}

#[test]
fn a_shape_that_takes_away_can_be_a_circle_too() {
    let mut e = BoardEditor::default();
    e.tool = Tool::Zone;
    e.zone_net = "VNEG".into();
    set_poly(&mut e, &[p(0.0, 0.0), p(40.0, 0.0), p(40.0, 40.0), p(0.0, 40.0)]);
    e.finish_polygon();
    e.zone_op = pcb_core::model::ShapeOp::Sub;
    e.poly_mode = PolyMode::Circle;
    e.click(p(20.0, 20.0));
    e.click(p(23.0, 20.0));
    assert_eq!(e.board.zones.len(), 1);
    assert_eq!(e.board.zones[0].holes().len(), 1);
    assert!((area(&e.board.zones[0].holes()[0]) - std::f64::consts::PI * 9.0).abs() < 0.5);
    assert!(!e.board.zones[0].contains(p(20.0, 20.0)) && e.board.zones[0].contains(p(5.0, 5.0)));
    // a right click drops the centre that was clicked
    e.click(p(30.0, 30.0));
    assert!(e.pending.is_some());
    e.leave_tool();
    assert!(e.pending.is_none() && e.tool == Tool::Zone);
}

#[test]
fn the_mode_changes_while_drawing_and_the_last_corner_can_be_taken_back() {
    let mut e = BoardEditor::default();
    e.tool = Tool::Zone;
    e.zone_net = "GND".into();
    e.click(p(0.0, 0.0));
    e.click(p(10.0, 0.0));
    e.set_poly_mode(PolyMode::ArcThrough); // what the menu and the key A do, without losing the corners
    e.click(p(13.0, 5.0));
    assert!(e.arc_mid.is_some());
    e.set_poly_mode(PolyMode::Lines);
    assert!(e.arc_mid.is_none() && e.poly.len() == 2, "the half-made arc is dropped, the corners stay");
    e.click(p(10.0, 10.0));
    e.poly_back();
    assert_eq!((e.poly.len(), e.poly_verts.len()), (2, 2));
    e.set_poly_mode(PolyMode::ArcTangent);
    e.click(p(15.0, 5.0));
    assert!(e.poly.len() > 3 && e.poly_verts.len() == 3 && e.poly_verts[1].arc_mid.is_some());
    e.poly_back();
    assert!(e.poly_verts[1].arc_mid.is_none(), "taking the arc's end back makes the corner before it a plain one again");
    assert_eq!(e.poly.len(), 2);
}

#[test]
fn a_shape_that_touches_nothing_of_the_net_is_said_to_be_an_island() {
    let mut e = BoardEditor::default();
    e.tool = Tool::Zone;
    e.zone_net = "VNEG".into();
    let mut pad = Pad::new(p(5.0, 5.0), PadShape::Circle { d: 2.0 }, None, Side::Top);
    pad.net = "VNEG".into();
    e.board.pads.push(pad);
    e.board.outline = vec![p(0.0, 0.0), p(60.0, 0.0), p(60.0, 40.0), p(0.0, 40.0)];
    set_poly(&mut e, &[p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(0.0, 10.0)]);
    e.finish_polygon();
    assert!(!e.status.contains("孤立"), "{}", e.status);
    // a second shape in the selected pour, far from the pad
    set_poly(&mut e, &[p(30.0, 20.0), p(40.0, 20.0), p(40.0, 30.0), p(30.0, 30.0)]);
    e.finish_polygon();
    assert!(e.status.contains("孤立"), "{}", e.status);
    assert_eq!(e.board.zones[0].shapes.len(), 2);
}

#[test]
fn hiding_the_parts_hides_their_pads_and_designators_but_not_loose_copper() {
    let mut e = BoardEditor::default();
    e.board.parts.push(pcb_core::Part { refdes: "R1".into(), value: "1k".into(), footprint: "R_0805".into(), pos: p(10.0, 10.0), rot: 0, side: Side::Top });
    let mut pad = Pad::new(p(10.0, 10.0), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
    pad.refdes = "R1".into();
    pad.pin = "1".into();
    e.board.pads.push(pad);
    e.board.pads.push(Pad::new(p(20.0, 10.0), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top)); // a loose pad
    e.board.texts.push(pcb_core::SilkText { side: Side::Top, pos: p(10.0, 12.0), height: 1.0, text: "R1".into(), refdes: "R1".into(), rot: 0 });
    assert_eq!(e.hit_test(p(10.0, 10.0)), Some(Item::Part(0)));
    assert!(e.item_shown(Item::Part(0)) && e.item_shown(Item::Label(0)));
    e.vis.parts = false;
    assert_ne!(e.hit_test(p(10.0, 10.0)), Some(Item::Part(0)), "the part cannot be picked while it is hidden");
    assert!(!e.item_shown(Item::Part(0)) && !e.item_shown(Item::Pad(0)) && !e.item_shown(Item::Label(0)));
    assert!(e.item_shown(Item::Pad(1)), "a loose pad is not a part");
    assert_eq!(e.hit_test(p(20.0, 10.0)), Some(Item::Pad(1)));
}

#[test]
fn the_corners_of_the_selected_pour_can_be_dragged_added_and_taken_away() {
    let mut e = BoardEditor::default();
    e.tool = Tool::Zone;
    e.zone_net = "GND".into();
    set_poly(&mut e, &[p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(0.0, 10.0)]);
    e.finish_polygon();
    e.tool = Tool::Select;
    assert_eq!(e.selected, Some(Item::Zone(0)));
    // a corner is found under the pointer, and dragging moves it (and one undo puts it back)
    let h = e.zone_handle_at(p(10.05, 0.05)).expect("the corner (10, 0)");
    assert_eq!((h.shape, h.vert, h.mid), (0, 1, false));
    e.start_vertex_drag(h);
    e.drag_vertex(p(12.0, -1.0));
    e.vdrag = None;
    assert_eq!(e.board.zones[0].shapes[0].shape.verts[1].p, p(12.0, -1.0));
    e.undo();
    assert_eq!(e.board.zones[0].shapes[0].shape.verts[1].p, p(10.0, 0.0));
    e.selected = Some(Item::Zone(0)); // undo drops the selection
    // a double click on an edge puts a corner there
    let (zone, shape, vert, at) = e.zone_edge_at(p(5.0, 0.1)).expect("the bottom edge");
    e.insert_zone_vertex(zone, shape, vert, at);
    assert_eq!(e.board.zones[0].shapes[0].shape.verts.len(), 5);
    assert_eq!(e.vsel.map(|v| v.vert), Some(1), "the new corner is the picked one");
    // Delete takes the picked corner away, not the pour
    assert!(e.delete_picked_vertex());
    assert_eq!((e.board.zones.len(), e.board.zones[0].shapes[0].shape.verts.len()), (1, 4));
    // a line made an arc: its middle point is a handle of its own
    e.board.zones[0].shapes[0].shape.make_arc(0);
    let mid = e.board.zones[0].shapes[0].shape.verts[0].arc_mid.unwrap();
    let h = e.zone_handle_at(mid).expect("the middle point of the arc");
    assert!(h.mid);
    e.start_vertex_drag(h);
    e.drag_vertex(p(5.0, -4.0));
    e.vdrag = None;
    assert_eq!(e.board.zones[0].shapes[0].shape.verts[0].arc_mid, Some(p(5.0, -4.0)));
    // nothing of this works when the pour is not the selected one
    e.selected = None;
    assert!(e.zone_handle_at(p(10.0, 0.0)).is_none() && e.zone_edge_at(p(5.0, 0.0)).is_none());
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
    e.board.vias.push(Via { pos: p(6.0, 8.0), diameter: 0.6, drill: 0.3, net: String::new() });
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
    e.board.vias.push(Via { pos: p(6.0, 8.0), diameter: 0.6, drill: 0.3, net: String::new() });
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
    assert_eq!(n, 12); // 9 Gerber/drill files + BOM + CPL + shopping list
    assert!(std::fs::read_to_string(dir.join("unit-BOM.csv")).unwrap().contains("R1"));
    assert!(std::fs::read_to_string(dir.join("unit-CPL.csv")).unwrap().contains("R1,10.0000mm"));
    assert!(std::fs::read_to_string(dir.join("unit-shopping.csv")).unwrap().contains("R1"));
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
    e.board.vias.push(Via { pos: p(25.0, 20.0), diameter: 0.6, drill: 0.3, net: String::new() });
    let (on_pad, on_track, on_via) = (e.board.pads[0].pos, p(25.0, 10.0), p(25.0, 20.0));
    // working on the top: the bottom part and track are out of reach, the via (through the board) is not
    e.side = Side::Top;
    assert!(e.hit_test(on_pad).is_none() && e.hit_test(on_track).is_none());
    assert!(matches!(e.hit_test(on_via), Some(Item::Via(0))));
    // the lock is for clicks: a rubber band takes both sides (unless that is switched off)
    let all = e.items_in_box(p(0.0, 0.0), p(40.0, 30.0), true);
    assert!(all.contains(&Item::Via(0)) && all.contains(&Item::Track(0)) && all.contains(&Item::Part(0)), "a box takes the other layer along: {all:?}");
    e.box_both_sides = false;
    let all = e.items_in_box(p(0.0, 0.0), p(40.0, 30.0), true);
    assert_eq!(all, vec![Item::Via(0)], "with the switch off a box does not take the other layer along");
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
    e.board.vias.push(Via { pos: p(10.0, 10.0), diameter: 0.6, drill: 0.3, net: String::new() });
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
fn a_group_turns_about_its_own_middle_and_four_turns_come_back() {
    let mut e = BoardEditor::default();
    for (x, name) in [(10.0, "R1"), (20.0, "R2")] {
        e.board.set_part(pcb_core::Part { refdes: name.into(), value: "1k".into(), footprint: "R_0805".into(), pos: p(x, 10.0), rot: 0, side: Side::Top }).unwrap();
    }
    e.board.tracks.push(Track { side: Side::Top, a: p(10.0, 10.0), b: p(20.0, 10.0), width: 0.25 });
    let items = vec![Item::Part(0), Item::Part(1), Item::Track(0)];
    let before = e.board.parts.iter().map(|q| q.pos).collect::<Vec<_>>();
    e.rotate_group(&items);
    // the pair was side by side about (15, 10): now one above the other about the same middle
    let after = e.board.parts.iter().map(|q| q.pos).collect::<Vec<_>>();
    assert_eq!((after[0], after[1]), (p(15.0, 5.0), p(15.0, 15.0)));
    assert_eq!(e.board.parts[0].rot, 1);
    assert_eq!((e.board.tracks[0].a, e.board.tracks[0].b), (after[0], after[1]));
    for _ in 0..3 {
        e.rotate_group(&items);
    }
    assert_eq!(e.board.parts.iter().map(|q| q.pos).collect::<Vec<_>>(), before);
}

#[test]
fn a_designator_turns_by_itself_and_keeps_the_turn_when_the_part_moves() {
    let mut e = BoardEditor::default();
    e.board.set_part(pcb_core::Part { refdes: "R1".into(), value: "1k".into(), footprint: "R_0805".into(), pos: p(10.0, 10.0), rot: 0, side: Side::Top }).unwrap();
    let (w, h) = e.board.texts[0].size();
    assert!(e.board.turn_label("R1"));
    assert_eq!(e.board.texts[0].size(), (h, w));
    e.board.place_part("R1", p(20.0, 10.0), 0);
    assert_eq!(e.board.texts[0].rot, 1, "placing the part again keeps the turn");
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
    set_poly(&mut e, &vec![p(0.0, 0.0), p(20.0, 0.0), p(20.0, 10.0)]);
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
    let r1 = e.router_for(None).unwrap();
    assert!(std::rc::Rc::ptr_eq(&r1, &e.router_for(None).unwrap()));
    e.board.tracks.push(Track { side: Side::Top, a: p(1.0, 1.0), b: p(2.0, 1.0), width: 0.25 });
    assert!(!std::rc::Rc::ptr_eq(&r1, &e.router_for(None).unwrap()));
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

/// Frame time with the pours shown and hidden, on a real board (set CCAD_BOARD): `cargo test --release -p app zone_frame_cost -- --ignored --nocapture`
#[test]
#[ignore]
fn zone_frame_cost() {
    let path = std::env::var("CCAD_BOARD").unwrap_or_else(|_| "../../microohm-meter/mmUser_sync.pcbjson".into());
    let board: pcb_core::Board = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut e = BoardEditor::default();
    e.board = board;
    e.fit_view(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0)));
    let ctx = egui::Context::default();
    let frame = |e: &mut BoardEditor, label: &str| {
        let (mut worst, mut total) = (0.0f64, 0.0);
        for i in 0..15 {
            let t = std::time::Instant::now();
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() }, |ui| {
                e.canvas(ui);
            });
            out.textures_delta.clear();
            let n = out.shapes.len();
            let prims = ctx.tessellate(out.shapes, out.pixels_per_point);
            let tris: usize = prims.iter().map(|p| match &p.primitive { egui::epaint::Primitive::Mesh(m) => m.indices.len() / 3, _ => 0 }).sum();
            let dt = t.elapsed().as_secs_f64() * 1000.0;
            if i == 0 {
                println!("  first frame {dt:.1} ms, shapes {n}, triangles {tris}");
            } else {
                worst = worst.max(dt);
                total += dt;
            }
        }
        println!("{label}: avg {:.1} ms, worst {:.1} ms", total / 14.0, worst);
    };
    println!("zones {}", e.board.zones.len());
    e.vis.zones = false;
    frame(&mut e, "zones hidden");
    e.vis.zones = true;
    frame(&mut e, "zones shown");
    // an edit: the first frame after it must not wait for the pours, the new ones come in a moment later
    let key0 = pcb_core::zonefill::geometry_key(&e.board);
    let t0 = e.board.tracks[0].clone();
    e.board.tracks.push(pcb_core::model::Track { a: Point::new(t0.a.x + 0.4, t0.a.y), b: Point::new(t0.a.x + 0.9, t0.a.y), ..t0 });
    assert_ne!(key0, pcb_core::zonefill::geometry_key(&e.board));
    frame(&mut e, "zones shown, after an edit");
    let key = pcb_core::zonefill::geometry_key(&e.board);
    assert_eq!(e.zone_cache.borrow().as_ref().map(|(k, _)| *k), Some(key), "the new pours came in");
    // the same edit with the pours hidden: what is left of the first frame is not the pours
    e.vis.zones = false;
    let t1 = e.board.tracks[1].clone();
    e.board.tracks.push(pcb_core::model::Track { a: Point::new(t1.a.x + 0.4, t1.a.y), b: Point::new(t1.a.x + 0.9, t1.a.y), ..t1 });
    frame(&mut e, "zones hidden, after an edit");
    // the glow of the biggest pour's net under the pointer (drawn every frame while the pointer is on it)
    e.vis.zones = true;
    frame(&mut e, "zones shown, settled");
    let net = e.zone_cache.borrow().as_ref().and_then(|(_, f)| f.iter().max_by_key(|(_, _, sh, _)| sh.iter().map(|s| s.outer.len() + s.holes.iter().map(Vec::len).sum::<usize>()).sum::<usize>()).map(|(_, n, _, _)| n.clone())).unwrap();
    let net = std::env::var("CCAD_GLOW").unwrap_or(net);
    e.hover_net = Some((net.clone(), egui::pos2(700.0, 450.0)));
    println!("glow on {net}");
    let hn = e.hover_net.clone();
    let ctx2 = ctx.clone();
    let (mut worst, mut total) = (0.0f64, 0.0);
    for _ in 0..10 {
        let t = std::time::Instant::now();
        let mut out = ctx2.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() }, |ui| {
            e.hover_net = hn.clone();
            let rect = ui.max_rect();
            let painter = ui.painter().clone();
            e.draw_net_focus(&painter, rect);
        });
        out.textures_delta.clear();
        let n_shapes = out.shapes.len();
        let prims = ctx2.tessellate(out.shapes, out.pixels_per_point);
        let tris: usize = prims.iter().map(|p| match &p.primitive { egui::epaint::Primitive::Mesh(m) => m.indices.len() / 3, _ => 0 }).sum();
        if worst == 0.0 {
            println!("  glow shapes {n_shapes}, triangles {tris}");
        }
        let dt = t.elapsed().as_secs_f64() * 1000.0;
        worst = worst.max(dt);
        total += dt;
    }
    let tn = std::time::Instant::now();
    for i in 0..200 {
        let _ = e.net_under(Point::new(20.0 + i as f64 * 0.3, 40.0));
    }
    println!("net_under x200: {:.1} ms", tn.elapsed().as_secs_f64() * 1e3);
    println!("net glow only: avg {:.1} ms, worst {:.1} ms", total / 10.0, worst);
}

/// Where the triangles and the time of a frame go, by kind of drawing, on a real board (set CCAD_BOARD):
/// `cargo test --release -p app frame_breakdown -- --ignored --nocapture`
#[test]
#[ignore]
fn frame_breakdown() {
    let path = std::env::var("CCAD_BOARD").unwrap_or_else(|_| "../../microohm-meter/mmUser_sync.pcbjson".into());
    let board: pcb_core::Board = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut e = BoardEditor::default();
    e.board = board;
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0));
    e.fit_view(screen);
    let ctx = egui::Context::default();
    println!(
        "tracks {} vias {} pads {} parts {} zones {} silk {} texts {}",
        e.board.tracks.len(),
        e.board.vias.len(),
        e.board.pads.len(),
        e.board.parts.len(),
        e.board.zones.len(),
        e.board.silk.len(),
        e.board.texts.len()
    );
    println!("finish active {}", pcb_core::smooth::finish_active(&e.board));
    if let Some(f) = pcb_core::smooth::finished(&e.board) {
        let pv: usize = f.patches.iter().map(|p| p.outline.len()).sum();
        println!("finished: tracks {} patches {} (patch vertices {pv})", f.board.tracks.len(), f.patches.len());
    }
    // (shapes, triangles, vertices, avg ms over 10 settled frames)
    let measure = |e: &mut BoardEditor| -> (usize, usize, usize, f64) {
        let (mut shapes, mut tris, mut verts, mut total) = (0, 0, 0, 0.0f64);
        for i in 0..14 {
            let t = std::time::Instant::now();
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
                e.canvas(ui);
            });
            out.textures_delta.clear();
            shapes = out.shapes.len();
            let prims = ctx.tessellate(out.shapes, out.pixels_per_point);
            tris = prims.iter().map(|p| match &p.primitive { egui::epaint::Primitive::Mesh(m) => m.indices.len() / 3, _ => 0 }).sum();
            verts = prims.iter().map(|p| match &p.primitive { egui::epaint::Primitive::Mesh(m) => m.vertices.len(), _ => 0 }).sum();
            if i >= 4 {
                total += t.elapsed().as_secs_f64() * 1000.0;
            }
        }
        (shapes, tris, verts, total / 10.0)
    };
    let set = |e: &mut BoardEditor, which: &str| {
        e.vis.top = true;
        e.vis.bottom = true;
        let all = which == "all";
        e.vis.silk = all || which == "silk" || which == "parts+silk";
        e.vis.parts = all || which == "parts" || which == "parts+silk";
        e.vis.airwires = all || which == "airwires";
        e.vis.nets = all || which == "nets";
        e.vis.zones = all || which == "zones";
        e.vis.ruler = false;
        e.vis.tracks = all || which == "tracks";
        e.vis.vias = all || which == "vias";
    };
    // let the pours come in before timing anything
    set(&mut e, "zones");
    for _ in 0..40 {
        measure(&mut e);
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
    // the two halves of a frame: building the shapes (`canvas`), and egui cutting them into triangles
    for which in ["tracks", "zones", "parts+silk", "all"] {
        set(&mut e, which);
        let (mut build, mut cut) = (0.0f64, 0.0f64);
        for i in 0..14 {
            let t = std::time::Instant::now();
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
                e.canvas(ui);
            });
            out.textures_delta.clear();
            let t1 = t.elapsed().as_secs_f64() * 1000.0;
            let t = std::time::Instant::now();
            let _ = ctx.tessellate(out.shapes, out.pixels_per_point);
            if i >= 4 {
                build += t1;
                cut += t.elapsed().as_secs_f64() * 1000.0;
            }
        }
        println!("{which:>9}: building shapes {:.2} ms, egui cutting them up {:.2} ms", build / 10.0, cut / 10.0);
    }
    for which in ["none", "tracks", "vias", "zones", "silk", "parts", "parts+silk", "airwires", "nets", "all"] {
        set(&mut e, which);
        let (s, t, v, ms) = measure(&mut e);
        println!("{which:>9}: shapes {s:>6} triangles {t:>8} vertices {v:>8}  {ms:.1} ms");
    }
    // each of those with only one side shown, to see what the semi-transparent other side costs
    for side in ["top only", "bottom only"] {
        set(&mut e, "tracks");
        e.vis.top = side == "top only";
        e.vis.bottom = side == "bottom only";
        let (s, t, v, ms) = measure(&mut e);
        println!("tracks, {side}: shapes {s} triangles {t} vertices {v}  {ms:.1} ms");
        set(&mut e, "zones");
        e.vis.top = side == "top only";
        e.vis.bottom = side == "bottom only";
        let (s, t, v, ms) = measure(&mut e);
        println!("zones,  {side}: shapes {s} triangles {t} vertices {v}  {ms:.1} ms");
    }
    if let Some(c) = e.silk_cache.borrow().as_ref() {
        let seg = |v: &[ribbon::Chain]| (v.len(), v.iter().map(|c| c.pts.len().saturating_sub(1)).sum::<usize>(), v.iter().filter(|c| c.pts.len() <= 2).count());
        println!("silk (chains, pieces, chains of one piece): plain {:?}, of parts {:?}; silk lines {}, texts {}", seg(&c.plain), seg(&c.of_parts), e.board.silk.len(), e.board.texts.len());
    }
    if let Some(c) = e.copper_cache.borrow().as_ref() {
        let pts: usize = c.chains.iter().map(|ch| ch.pts.len()).sum();
        let one = c.chains.iter().filter(|ch| ch.pts.len() <= 2).count();
        let mut widths: Vec<u64> = c.chains.iter().map(|ch| ch.width.to_bits()).collect();
        widths.sort();
        widths.dedup();
        let lod = c.lod(e.zoom);
        let (lp, lt): (usize, usize) = (lod.chains.iter().map(Vec::len).sum(), lod.patches.iter().map(Vec::len).sum());
        println!("copper: chains {} (with at most 2 points: {one}), points {pts} (at this zoom {lp}), widths {}, patches {} with {lt} triangles at this zoom", c.chains.len(), widths.len(), c.patches.len());
    }
    // the zones' own geometry: how many vertices, and how many belong to holes
    if let Some((_, fills)) = e.zone_cache.borrow().as_ref() {
        let (mut outer, mut holes, mut nholes, mut nshapes, mut tri) = (0, 0, 0, 0, 0);
        for (_, _, shapes, tris) in fills {
            for sh in shapes {
                nshapes += 1;
                outer += sh.outer.len();
                nholes += sh.holes.len();
                holes += sh.holes.iter().map(Vec::len).sum::<usize>();
            }
            tri += tris.iter().map(Vec::len).sum::<usize>();
        }
        println!("zone geometry: shapes {nshapes}, outer vertices {outer}, holes {nholes} with {holes} vertices, fill triangles {tri}");
        let key = pcb_core::zonefill::geometry_key(&e.board);
        let thin = e.pour_outlines(key, fills);
        let kept: usize = thin.iter().flatten().flatten().map(Vec::len).sum();
        let big = thin.iter().flatten().flatten().filter(|c| c.len() > 200).count();
        println!("zoom {:.1} px/mm, level {}, outline vertices after thinning {kept}, contours over 200 vertices {big}", e.zoom, ribbon::lod_level(e.zoom));
        for tol in [0.003, 0.01, 0.03, 0.1, 0.3] {
            let n: usize = fills.iter().flat_map(|(_, _, shapes, _)| shapes.iter()).flat_map(|sh| std::iter::once(&sh.outer).chain(sh.holes.iter())).map(|c| ribbon::simplify_closed(c, tol).len()).sum();
            println!("  tolerance {tol} mm ({:.2} px): {n} vertices", tol * e.zoom as f64);
        }
        let mut sizes: Vec<usize> = fills.iter().flat_map(|(_, _, shapes, _)| shapes.iter()).flat_map(|sh| sh.holes.iter()).map(Vec::len).collect();
        sizes.sort();
        println!("  hole sizes: min {} median {} p90 {} max {}", sizes[0], sizes[sizes.len() / 2], sizes[sizes.len() * 9 / 10], sizes[sizes.len() - 1]);
    }
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
    let base_violations = pcb_core::drc::check(&e.board).len();
    let mut valid = vec![];
    for dx in -9..=9 {
        for dy in -9..=9 {
            let at = Point::new(goal.x + dx as f64, goal.y + dy as f64);
            let mut t = e.board.clone();
            t.place_part(refdes, at, rot);
            let ds: Vec<f64> = ends.iter().map(|(own, tgt)| dist(pad(&t, refdes, own), pad(&t, tgt.0, tgt.1))).collect();
            let fits = ds.iter().all(|d| *d >= 2.0 && *d <= max);
            let inside = t.pads.iter().filter(|p| p.refdes == refdes).all(|p| inside_polygon(&t.outline, p.pos));
            // (the messages of the checks name no part: a spot is clean when the move adds no violation)
            let clean = pcb_core::drc::check(&t).len() <= base_violations;
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
    // the auto placement puts the bypass capacitors by their pins itself (the lesson is to look, and to move one that is still far): the
    // step is done as soon as both are near, however many the learner had to move
    drag_next_to(&mut e, "C3", &[("1", ("U1", "4"))], 5.0);
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

#[test]
fn the_mega_board_lesson_starts_half_routed_and_ticks_when_a_net_is_routed_and_drc_stays_clean() {
    let mut sch = crate::schematic::SchEditor::default();
    assert!(sch.load_sample(BoardEditor::mega_lesson_circuit()));
    let mut e = BoardEditor::default();
    e.adopt(serde_json::from_str(BoardEditor::mega_lesson_board()).unwrap(), None);
    e.start_mega_lesson();
    assert_eq!(e.lesson_progress(), 0);
    // 18 nets are open, but the DRC is clean
    let mut open: Vec<String> = ratsnest(&e.board).iter().map(|w| w.net.clone()).collect();
    open.sort();
    open.dedup();
    assert_eq!(open.len(), 18);
    e.run_drc();
    assert!(e.violations.is_empty());
    assert_eq!(e.lesson_progress(), 1);
    // the hand work cannot be scripted here (a straight line over the board shorts other nets): stand in for "one net is routed" by taking
    // one open net out of the board, which is what the lesson counts
    let all = ratsnest(&e.board);
    let net = all[0].net.clone();
    e.checkpoint();
    for p in e.board.pads.iter_mut().filter(|p| p.net == net) {
        p.net.clear();
    }
    assert_eq!(e.lesson_progress(), 2, "one net less is open");
    e.run_drc();
    let clean = e.violations.is_empty();
    assert_eq!(e.lesson_progress(), if clean { 3 } else { 2 }, "the last step needs a clean DRC after the work ({} violations)", e.violations.len());
    assert_eq!(e.finished_board_lesson().is_some(), clean);
}

/// Currents taken from the simulation open the window that lists them (not the width panel: that would switch the tool to the wire tool
/// and push the step list below the fold); taken only to route ("未配線を全部つなぐ") they leave nothing open.
#[test]
fn the_currents_open_their_window_and_it_can_be_shut_again() {
    let mut e = BoardEditor::default();
    e.use_simulated_currents(Ok(vec![("GND".into(), 0.1)]), vec![]);
    assert!(e.current_win_open, "the currents are shown in their window");
    assert_eq!(e.widths_open, None, "and the width panel is left alone");
    e.close_width_panel();
    assert!(!e.current_win_open);
}

/// New parts land beside the outline, so an import that adds parts asks for the view to be fitted (the parts would otherwise sit
/// half under the panel on the right); an import that adds nothing leaves the view alone.
#[test]
fn an_import_that_adds_parts_fits_the_view_to_the_board() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut sch = crate::schematic::SchEditor::default();
    sch.load_path(root.join("led.schjson"));
    let mut e = BoardEditor::default();
    e.fit_pending = false;
    e.import_from_schematic(&sch);
    assert!(e.fit_pending, "parts were added");
    e.fit_pending = false;
    e.import_from_schematic(&sch);
    assert!(!e.fit_pending, "nothing new: the view stays where the user put it");
}

fn board_with_one_part() -> BoardEditor {
    let mut e = BoardEditor::default();
    e.board.set_part(pcb_core::Part { refdes: "R1".into(), value: "10k".into(), footprint: "R_0805".into(), pos: p(10.0, 10.0), rot: 0, side: pcb_core::Side::Top }).unwrap();
    e
}

/// A part's silkscreen designator can be picked on its own, moved, and put back with Delete (it is not deleted).
#[test]
fn a_designator_is_picked_moved_and_put_back() {
    let mut e = board_with_one_part();
    let at = e.board.texts[0].pos;
    // picking: on the text, but a pad of the part still wins over the text
    assert_eq!(e.hit_test(at), Some(Item::Label(0)));
    assert!(!matches!(e.hit_test(p(10.0, 10.0)), Some(Item::Label(_))), "the part's own pads come first");
    // moving it in a group drag (how the app moves a selection): the text follows by the drag's distance
    let g = GroupDrag { anchor: at, orig: e.board.clone(), items: vec![Item::Label(0)] };
    e.move_group(&g, 3.0, -1.0);
    assert_eq!(e.board.texts[0].pos, p(at.x + 3.0, at.y - 1.0));
    // the same drag again from the same start does not pile up
    e.move_group(&g, 4.0, 0.0);
    assert_eq!(e.board.texts[0].pos, p(at.x + 4.0, at.y));
    assert_eq!(e.board.label_offsets["R1"], p(4.0, 0.0));
    // the part moves: the text keeps its place relative to it
    e.board.place_part("R1", p(20.0, 10.0), 0);
    assert_eq!(e.board.texts[0].pos, p(at.x + 10.0 + 4.0, at.y));
    // Delete puts it back instead of deleting it
    let n = e.board.texts.len();
    e.delete(Item::Label(0));
    assert_eq!(e.board.texts.len(), n);
    assert!(e.board.label_offsets.is_empty() && e.status.contains("元の位置"), "{}", e.status);
}

/// Moving a part and its designator together does not move the text twice.
#[test]
fn a_part_and_its_designator_dragged_together_keep_the_offset() {
    let mut e = board_with_one_part();
    let at = e.board.texts[0].pos;
    e.board.move_label("R1", p(at.x + 2.0, at.y));
    let g = GroupDrag { anchor: p(10.0, 10.0), orig: e.board.clone(), items: vec![Item::Part(0), Item::Label(0)] };
    e.move_group(&g, 5.0, 0.0);
    assert_eq!(e.board.texts[0].pos, p(at.x + 5.0 + 2.0, at.y), "the part carried the text, the offset stayed 2 mm");
    assert_eq!(e.board.label_offsets["R1"], p(2.0, 0.0));
}

fn lx_pair_editor() -> BoardEditor {
    let mut e = BoardEditor::default();
    e.board = pcb_core::Board::new("t", 40.0, 40.0);
    for y in [18.825, 18.175] {
        let mut pd = pcb_core::Pad::new(p(15.05, y), pcb_core::PadShape::Rect { w: 0.85, h: 0.3 }, None, pcb_core::Side::Top);
        (pd.refdes, pd.pin, pd.net) = ("U1".into(), "2".into(), "LX".into());
        e.board.pads.push(pd);
    }
    e.tool = Tool::Track;
    e.trunk_mode = true;
    e
}

/// The trunk tool: two pads, then a place; thin necks and a thick trunk are drawn and the DRC stays clean.
#[test]
fn the_trunk_tool_takes_two_pads_and_a_place_and_draws_necks_and_a_trunk() {
    let mut e = lx_pair_editor();
    e.click(p(15.05, 18.825));
    assert_eq!(e.trunk_pads.len(), 1);
    assert!(e.status.contains("U1"), "{}", e.status);
    e.click(p(15.05, 18.175));
    assert_eq!(e.trunk_pads.len(), 2);
    e.click(p(10.0, 18.5));
    assert!(e.trunk_pads.is_empty(), "{}", e.status);
    assert!(e.board.tracks.len() >= 3, "two necks and the trunk: {}", e.board.tracks.len());
    let widest = e.board.tracks.iter().map(|t| t.width).fold(0.0, f64::max);
    assert!((widest - 0.9).abs() < 1e-9, "the trunk is as wide as the pair: {widest}");
    assert!(pcb_core::drc::check(&e.board).is_empty());
    assert!(e.status.contains("太い幹"), "{}", e.status);
}

/// A bad place or pair says why and changes nothing; a right click drops the pads that were picked.
#[test]
fn the_trunk_tool_refuses_with_a_reason_and_can_be_cancelled() {
    let mut e = lx_pair_editor();
    e.click(p(30.0, 30.0));
    assert!(e.trunk_pads.is_empty() && e.status.contains("パッドをクリック"), "{}", e.status);
    e.click(p(15.05, 18.825));
    e.click(p(15.05, 18.825));
    assert_eq!(e.trunk_pads.len(), 1, "the same pad twice is not a pair");
    e.click(p(15.05, 18.175));
    e.click(p(14.5, 18.5));
    assert!(e.status.contains("近すぎ") && e.board.tracks.is_empty(), "{}", e.status);
    assert_eq!(e.trunk_pads.len(), 2, "still waiting for a place");
    assert!(e.leave_tool());
    assert!(e.trunk_pads.is_empty());
    // another net is not a pair
    let mut pd = pcb_core::Pad::new(p(15.05, 19.475), pcb_core::PadShape::Rect { w: 0.85, h: 0.3 }, None, pcb_core::Side::Top);
    pd.net = "VIN".into();
    e.board.pads.push(pd);
    e.click(p(15.05, 18.825));
    e.click(p(15.05, 19.475));
    assert!(e.status.contains("ネットが違い") && e.trunk_pads.len() == 1, "{}", e.status);
}

/// Trunk mode belongs to the wire tool: it is not a tool of its own, and an ordinary click in the wire tool starts a track.
#[test]
fn trunk_mode_is_part_of_the_wire_tool() {
    let mut e = lx_pair_editor();
    e.trunk_mode = false;
    e.click(p(15.05, 18.825));
    assert!(e.trunk_pads.is_empty() && e.pending.is_some(), "the wire tool starts a track as always");
    assert_eq!((tool_key(Tool::Select), tool_key(Tool::Silk)), (Some('1'), Some('0')));
}

/// The net under the pointer: a pad, a track of the pad's net, and nothing on bare board.
#[test]
fn the_net_under_a_point_is_found_for_pads_and_tracks() {
    let mut e = lx_pair_editor();
    e.tool = Tool::Select;
    e.board.tracks.push(pcb_core::Track { a: p(15.05, 18.825), b: p(10.0, 18.825), width: 0.25, side: pcb_core::Side::Top });
    assert_eq!(e.net_under(p(15.05, 18.175)).as_deref(), Some("LX"), "on a pad");
    assert_eq!(e.net_under(p(12.0, 18.825)).as_deref(), Some("LX"), "on a track that leaves the pad");
    assert_eq!(e.net_under(p(30.0, 30.0)), None, "on bare board");
}

/// What is hidden (a layer, or the tracks) is not marked when selected, and a hidden kind is not shown by the glow either.
#[test]
fn hidden_things_get_no_selection_mark() {
    let mut e = lx_pair_editor();
    e.board.tracks.push(pcb_core::Track { a: p(15.05, 18.825), b: p(10.0, 18.825), width: 0.25, side: pcb_core::Side::Top });
    assert!(e.item_shown(Item::Track(0)));
    e.vis.tracks = false;
    assert!(!e.item_shown(Item::Track(0)));
    e.vis.tracks = true;
    e.vis.top = false;
    assert!(!e.item_shown(Item::Track(0)) && !e.item_shown(Item::Pad(0)));
}

/// While only parts are being moved the pours and the finished tracks are kept (they are slow); a dragged track is not frozen.
#[test]
fn pours_are_kept_while_only_parts_are_dragged() {
    let mut e = lx_pair_editor();
    assert!(!e.moving_parts_only());
    e.drag_part = Some(("U1".into(), p(0.0, 0.0)));
    assert!(e.moving_parts_only());
    e.drag_part = None;
    e.gdrag = Some(GroupDrag { anchor: p(0.0, 0.0), orig: e.board.clone(), items: vec![Item::Part(0)] });
    assert!(e.moving_parts_only());
    e.gdrag = Some(GroupDrag { anchor: p(0.0, 0.0), orig: e.board.clone(), items: vec![Item::Part(0), Item::Track(0)] });
    assert!(!e.moving_parts_only(), "a track in the group would stay where it was");
    assert!(e.moving_group() && e.slow_results_kept(), "the pours are kept while a group with tracks is moved");
    e.gdrag = Some(GroupDrag { anchor: p(0.0, 0.0), orig: e.board.clone(), items: vec![Item::Zone(0)] });
    assert!(!e.slow_results_kept(), "a pour that is moved is worked out again");
}

/// What is far outside the drawing area is left out of the picture; what reaches into it (a wide track) is not.
#[test]
fn near_screen_leaves_out_only_what_is_far_away() {
    let mut e = BoardEditor::default();
    e.zoom = 10.0;
    e.pan = egui::Vec2::new(20.0, 20.0);
    let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 300.0));
    assert!(e.near_screen(rect, &[p(20.0, 20.0)], 0.0), "the middle");
    assert!(!e.near_screen(rect, &[p(200.0, 20.0), p(210.0, 20.0)], 0.5), "far to the right");
    assert!(e.near_screen(rect, &[p(5.0, 20.0), p(60.0, 20.0)], 0.5), "a long track across the view, ends outside");
    assert!(e.near_screen(rect, &[p(40.4, 20.0)], 5.0), "a wide track just outside");
}

/// The "文字を配線からよける" button moves a designator off a track, can be undone, and says so when nothing overlaps.
#[test]
fn the_label_button_moves_a_designator_off_a_track_and_undoes() {
    let mut e = BoardEditor::default();
    e.board = pcb_core::Board::new("t", 40.0, 30.0);
    e.board.set_part(pcb_core::Part { refdes: "R1".into(), value: "1k".into(), footprint: "R_0805".into(), pos: p(20.0, 15.0), rot: 0, side: pcb_core::Side::Top }).unwrap();
    e.avoid_label_overlaps_button();
    assert!(e.status.contains("重なっていません"), "{}", e.status);
    let at = e.board.texts[0].pos;
    e.board.tracks.push(pcb_core::Track { a: p(at.x - 3.0, at.y), b: p(at.x + 3.0, at.y), width: 0.25, side: pcb_core::Side::Top });
    e.avoid_label_overlaps_button();
    assert!(e.status.contains("動かしました"), "{}", e.status);
    assert!(e.board.texts[0].pos != at);
    e.undo();
    assert_eq!(e.board.texts[0].pos, at, "undone");
}

/// A net the schematic did not name is shown with the pins it joins, so it can be found on the sheet.
#[test]
fn an_unnamed_net_is_captioned_with_its_pins() {
    let mut e = lx_pair_editor();
    for pd in e.board.pads.iter_mut() {
        pd.net = "N$1".into();
    }
    let c = e.net_caption("N$1");
    assert!(c.contains("U1-2") && c.contains("回路図で名前のない"), "{c}");
    assert_eq!(e.net_caption("GND"), "GND");
}

/// A route that starts on a net of a class is drawn with the width of the class (and its via); other nets use the tool's width.
#[test]
fn a_route_of_a_classed_net_uses_the_class_width_and_via() {
    let mut e = lx_pair_editor();
    e.tool = Tool::Track;
    e.trunk_mode = false;
    e.track_width = 0.25;
    let a = p(15.05, 18.825);
    let to = p(10.0, 18.825);
    assert!(e.route_segments(a, to).iter().all(|t| (t.width - 0.25).abs() < 1e-9));
    e.board.net_classes.push(pcb_core::NetClass { name: "電源".into(), nets: vec!["LX".into()], track_width: 0.6, clearance: 0.3, via_diameter: 0.9, via_drill: 0.45, impedance: None, finish: Default::default(), role: Default::default(), order: None, no_vias: None, layers: vec![], max_length: 0.0 });
    let segs = e.route_segments(a, to);
    assert!(!segs.is_empty() && segs.iter().all(|t| (t.width - 0.6).abs() < 1e-9), "{segs:?}");
    assert_eq!(e.route_settings(Some("LX")), (0.6, 0.9, 0.45, Some(0.3)));
    assert_eq!(e.route_settings(Some("GND")), (0.25, e.via_dia, e.via_drill, None));
}

/// The window of a net draws without trouble for a net of the board, drops a net that is not there, and a value dragged in the window
/// of a classed net is one undo step.
#[test]
fn the_net_window_draws_and_follows_the_net() {
    let mut e = lx_pair_editor();
    e.board.net_currents.insert("LX".into(), 1.0);
    e.net_win = Some("LX".into());
    let ctx = egui::Context::default();
    let frame = |e: &mut BoardEditor| {
        let mut input = egui::RawInput::default();
        input.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0)));
        let mut out = ctx.run_ui(input, |ui| e.net_window(ui.ctx()));
        out.textures_delta.clear();
    };
    frame(&mut e);
    assert_eq!(e.net_win.as_deref(), Some("LX"));
    e.net_win = Some("NOPE".into());
    frame(&mut e);
    assert!(e.net_win.is_none(), "a net that is not on the board has no window");
    // the classes are put in the board through one undo step while the pointer is down
    let class = pcb_core::NetClass { name: "x".into(), nets: vec!["LX".into()], ..Default::default() };
    let undo = e.undo.len();
    e.commit_classes(vec![class.clone()], true);
    let wider = pcb_core::NetClass { track_width: 0.5, ..class };
    e.commit_classes(vec![wider], true);
    assert_eq!(e.undo.len(), undo + 1, "one step for the whole drag");
    e.commit_classes(e.board.net_classes.clone(), false);
    assert!(!e.drag_edit);
}

/// A value dragged in a panel is one undo step, not one per frame: `checkpoint_once` takes the step at the start and waits for the
/// pointer to go up.
#[test]
fn a_dragged_panel_value_is_one_undo_step() {
    let mut e = lx_pair_editor();
    let before = e.undo.len();
    for _ in 0..30 {
        e.checkpoint_once();
        e.board.finish.radius += 0.01;
    }
    assert_eq!(e.undo.len(), before + 1, "thirty frames of one drag");
    // the pointer went up (the canvas does this every frame): the next drag is a step of its own
    e.drag_edit = false;
    e.checkpoint_once();
    assert_eq!(e.undo.len(), before + 2);
    e.undo();
    e.undo();
    assert!((e.board.finish.radius - pcb_core::Board::new("t", 1.0, 1.0).finish.radius).abs() < 1e-9, "two undos reach the start");
}

/// The net window's width slider does not touch the tracks (or take an undo step) until it is moved, with or without a class.
#[test]
fn the_width_slider_leaves_the_net_alone_until_it_is_moved() {
    let mut e = lx_pair_editor();
    e.board.net_currents.insert("LX".into(), 1.0);
    e.board.tracks.push(pcb_core::Track { a: p(15.05, 18.825), b: p(10.0, 18.825), width: 0.3, side: pcb_core::Side::Top });
    e.net_win = Some("LX".into());
    let ctx = egui::Context::default();
    let undo = e.undo.len();
    for with_class in [false, true] {
        if with_class {
            e.board.net_classes.push(pcb_core::NetClass { name: "x".into(), nets: vec!["LX".into()], track_width: 0.3, ..Default::default() });
        }
        for _ in 0..3 {
            let mut input = egui::RawInput::default();
            input.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0)));
            let mut out = ctx.run_ui(input, |ui| e.net_window(ui.ctx()));
            out.textures_delta.clear();
        }
        assert!((e.board.tracks[0].width - 0.3).abs() < 1e-9, "with_class={with_class}");
    }
    assert_eq!(e.undo.len(), undo, "no step was taken");
}

/// The Via tool puts the via on the net under it (a pad, here), or on the net chosen in its settings, or on none on bare board; a via
/// dropped while routing is of the net being routed.
#[test]
fn a_via_is_placed_for_the_net_under_it_or_the_net_chosen() {
    let mut e = lx_pair_editor();
    e.tool = Tool::Via;
    e.click(p(15.05, 18.825));
    assert_eq!(e.board.vias.last().unwrap().net, "LX", "on a pad of LX, automatically");
    e.click(p(30.0, 30.0));
    assert_eq!(e.board.vias.last().unwrap().net, "", "on bare board there is no net");
    e.via_net = Some("GND".into());
    e.click(p(30.0, 25.0));
    assert_eq!(e.board.vias.last().unwrap().net, "GND", "the net chosen in the settings wins");
    assert!(e.status.contains("GND"), "{}", e.status);
    // while routing
    e.via_net = None;
    e.tool = Tool::Track;
    e.trunk_mode = false;
    e.route_via(p(15.05, 18.825), p(10.0, 18.825));
    assert_eq!(e.board.vias.last().unwrap().net, "LX", "the via of a route is of its net");
}

/// A pad that only the outline of a pour seems to join (the pour is cut apart round it) has an airwire in the editor.
#[test]
fn the_editor_shows_an_airwire_for_a_pad_cut_off_by_its_pour() {
    let mut e = BoardEditor::default();
    e.board = pcb_core::Board::new("t", 40.0, 30.0);
    let pad = |b: &mut pcb_core::Board, x: f64, y: f64, net: &str| {
        let mut p = pcb_core::Pad::new(p(x, y), pcb_core::PadShape::Rect { w: 1.0, h: 1.0 }, None, pcb_core::Side::Top);
        p.net = net.into();
        b.pads.push(p);
    };
    pad(&mut e.board, 30.0, 20.0, "GND");
    pad(&mut e.board, 5.0, 5.0, "GND");
    e.board.zones.push(pcb_core::Zone::new("GND", pcb_core::Side::Top, vec![p(2.0, 2.0), p(38.0, 2.0), p(38.0, 28.0), p(2.0, 28.0)]));
    assert!(e.cached_airwires().is_empty(), "one piece of pour joins both");
    // a closed ring of VIN round the pad at (5, 5)
    let c = [p(2.5, 2.5), p(7.5, 2.5), p(7.5, 7.5), p(2.5, 7.5)];
    for q in c {
        pad(&mut e.board, q.x, q.y, "VIN");
    }
    for i in 0..4 {
        e.board.tracks.push(pcb_core::Track { side: pcb_core::Side::Top, a: c[i], b: c[(i + 1) % 4], width: 0.3 });
    }
    let wires = e.cached_airwires();
    assert!(wires.iter().any(|w| w.net == "GND"), "the cut-off pad has an airwire: {wires:?}");
}

/// A part whose footprint is neither built in nor in the library (a part added to the library folder while the program was running,
/// and the library not read again) is reported with what to do about it, not with the bare "not found".
#[test]
fn a_missing_library_footprint_says_to_reload_the_library() {
    let mut sch = crate::schematic::SchEditor::default();
    let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "LM358").unwrap();
    sch.sch.add_custom(def.symbol(), &def.default_value(), "NO_SUCH_FOOTPRINT_XYZ", (20, 10), 0);
    let mut e = BoardEditor::default();
    e.import_from_schematic(&sch);
    assert!(e.import_notes.iter().any(|n| n.contains("NO_SUCH_FOOTPRINT_XYZ") && n.contains("再読み込み")), "{:?}", e.import_notes);
}

/// Timing of what runs every frame while a group is moved (a real board: set CCAD_BOARD). `cargo test -p app --release group_move_cost -- --ignored --nocapture`
#[test]
#[ignore]
fn group_move_cost() {
    use std::time::Instant;
    let path = std::env::var("CCAD_BOARD").unwrap_or_else(|_| "../../microohm-meter/mmUser.pcbjson".into());
    let text = std::fs::read_to_string(&path).unwrap();
    let board: pcb_core::Board = serde_json::from_str(&text).unwrap();
    let mut e = BoardEditor::default();
    e.board = board;
    let t = |label: &str, f: &mut dyn FnMut()| {
        let s = Instant::now();
        f();
        eprintln!("{label:32} {:>8.1} ms", s.elapsed().as_secs_f64() * 1e3);
    };
    // a group: everything in the lower left quarter
    let (lo, hi) = (p(0.0, 0.0), p(50.0, 38.0));
    let items = e.items_in_box(lo, hi, false);
    eprintln!("group of {} items", items.len());
    let orig = e.board.clone();
    let g = GroupDrag { anchor: p(0.0, 0.0), orig, items };
    let mut dx = 0.0;
    for step in [0.0f64, 3.0, 12.0] {
        e.move_group(&g, step, 0.0);
        let s = Instant::now();
        let n = pcb_core::smooth::finished(&e.board).map(|_| 1).unwrap_or(0);
        eprintln!("finished after a move of {step:>4} mm: {:>8.1} ms ({n} pieces)", s.elapsed().as_secs_f64() * 1e3);
    }
    t("smooth::finished (before)", &mut || {
        let _ = pcb_core::smooth::finished(&e.board);
    });
    t("move_group", &mut || {
        dx += 0.5;
        e.move_group(&g, dx, 0.0);
    });
    t("geometry_key", &mut || {
        let _ = pcb_core::zonefill::geometry_key(&e.board);
    });
    t("connectivity", &mut || {
        let _ = pcb_core::drc::connectivity(&e.board);
    });
    let conn = pcb_core::drc::connectivity(&e.board);
    t("merged_fills (pours)", &mut || {
        let _ = pcb_core::zonefill::merged_fills(&e.board, &conn);
    });
    t("dropped_islands", &mut || {
        let _ = pcb_core::zonefill::dropped_islands(&e.board, &conn);
    });
    t("ratsnest", &mut || {
        let _ = pcb_core::ratsnest::ratsnest(&e.board);
    });
    t("smooth::finished", &mut || {
        let _ = pcb_core::smooth::finished(&e.board);
    });
    t("thermal_via_checks", &mut || {
        let _ = pcb_core::drc::thermal_via_checks(&e.board);
    });
    t("board.clone()", &mut || {
        let _ = e.board.clone();
    });
}


#[test]
fn each_sheet_of_the_circuit_gets_its_own_colour() {
    let mut e = BoardEditor::default();
    assert!(e.sheet_colors().is_empty(), "a one-sheet circuit has nothing to colour");
    e.board.part_sheets.insert("R1".into(), "電源".into());
    e.board.part_sheets.insert("R2".into(), "制御".into());
    e.board.part_sheets.insert("R3".into(), "制御".into());
    let c = e.sheet_colors();
    assert_eq!(c.len(), 2);
    assert_ne!(c[0].1, c[1].1);
}

/// The answer of the schematic to "入れ替えを回路図に反映": the board forgets the trades of the parts that went into the drawing (and gets them
/// back with undo); an error leaves the board as it is and says why.
#[test]
fn applied_pin_trades_leave_the_board_and_come_back_with_undo() {
    let mut e = BoardEditor::default();
    e.board.pin_map.insert("U1".into(), [("1".to_string(), "3".to_string()), ("3".to_string(), "1".to_string())].into());
    e.board.pin_map.insert("U2".into(), [("1".to_string(), "2".to_string()), ("2".to_string(), "1".to_string())].into());
    e.trades_applied(Err("つながりが変わります".into()));
    assert_eq!(e.board.pin_map.len(), 2);
    assert!(e.status.contains("つながりが変わります"));
    e.trades_applied(Ok(sch_core::BackAnnotation { parts: vec!["U1".into()], moved: 4 }));
    assert!(!e.board.pin_map.contains_key("U1") && e.board.pin_map.contains_key("U2"));
    assert!(e.status.contains("U1") && e.status.contains('4'), "{}", e.status);
    e.undo();
    assert!(e.board.pin_map.contains_key("U1"), "undo brings the trade back");
}

/// The requirements check is kept until the copper or the rows of the table change (it walks the copper when the table has measures, and
/// the panel asks every frame).
#[test]
fn the_requirements_answer_is_kept_until_the_board_or_the_table_changes() {
    let mut e = BoardEditor::default();
    for (x, name) in [(10.0, "R1"), (20.0, "R2")] {
        let _ = e.board.set_part(pcb_core::Part { refdes: name.into(), value: "1k".into(), footprint: "R_0805".into(), pos: p(x, 10.0), rot: 0, side: Side::Top });
    }
    e.board.keep_apart.push(design_spec::KeepApart { name: "離す".into(), a: vec!["R1".into()], b: vec!["R2".into()], min_dist: 500.0, ..Default::default() });
    let a = e.cached_requirements();
    assert!(std::rc::Rc::ptr_eq(&a, &e.cached_requirements()), "the same answer while nothing changed");
    assert_eq!(a.0.findings.len(), 1, "the two resistors are nearer than 500 mm");
    // a row of the table changes: the answer is worked out again
    e.board.waivers.push(design_spec::Waiver { kind: "apart".into(), name: "離す".into(), note: "x".into(), ..Default::default() });
    let b = e.cached_requirements();
    assert!(!std::rc::Rc::ptr_eq(&a, &b));
    assert_eq!((b.0.findings.len(), b.0.waived.len()), (0, 1), "the finding is accepted now");
}

/// Ctrl+click and Ctrl+rubber band: an object that is selected leaves the selection, one that is not joins it.
#[test]
fn ctrl_adds_an_object_to_the_selection_and_takes_it_out() {
    let mut e = BoardEditor::default();
    for (x, name) in [(10.0, "R1"), (20.0, "R2"), (30.0, "R3")] {
        let _ = e.board.set_part(pcb_core::Part { refdes: name.into(), value: "1k".into(), footprint: "R_0805".into(), pos: p(x, 10.0), rot: 0, side: Side::Top });
    }
    e.set_selection(vec![Item::Part(0)]);
    e.toggle_in_selection(&[Item::Part(1)]);
    assert_eq!(e.selection_items().len(), 2, "R2 joins R1");
    e.toggle_in_selection(&[Item::Part(0), Item::Part(2)]);
    let now = e.selection_items();
    assert!(now.contains(&Item::Part(1)) && now.contains(&Item::Part(2)) && !now.contains(&Item::Part(0)), "R1 leaves, R3 joins: {now:?}");
    e.toggle_in_selection(&[Item::Part(1)]);
    assert_eq!((e.selected, e.multi.len()), (Some(Item::Part(2)), 0), "one left: it is the selected one");
    e.toggle_in_selection(&[Item::Part(2)]);
    assert!(e.selection_items().is_empty());
    // an object the pick filter does not allow does not join
    e.pick.parts = false;
    e.toggle_in_selection(&[Item::Part(0)]);
    assert!(e.selection_items().is_empty());
}

/// A rubber band takes the wiring on the back too (with the work layer on the front and the other side locked); with `box_both_sides` off
/// the lock keeps it out. Moving the group moves the back wiring with the parts.
#[test]
fn a_rubber_band_takes_the_back_wiring_and_the_group_moves_with_it() {
    let mut e = BoardEditor::default();
    let _ = e.board.set_part(pcb_core::Part { refdes: "R1".into(), value: "1k".into(), footprint: "R_0805".into(), pos: p(10.0, 10.0), rot: 0, side: Side::Top });
    e.board.tracks.push(pcb_core::Track { side: Side::Top, a: p(8.0, 14.0), b: p(14.0, 14.0), width: 0.25 });
    e.board.tracks.push(pcb_core::Track { side: Side::Bottom, a: p(8.0, 16.0), b: p(14.0, 16.0), width: 0.25 });
    assert!(e.pick.active_side_only && e.side == Side::Top, "the defaults: front, the other side locked");
    let (a, b) = (p(0.0, 0.0), p(30.0, 30.0));
    let all = e.items_in_box(a, b, false);
    assert!(all.contains(&Item::Track(0)) && all.contains(&Item::Track(1)) && all.contains(&Item::Part(0)), "{all:?}");
    e.box_both_sides = false;
    let front = e.items_in_box(a, b, false);
    assert!(front.contains(&Item::Track(0)) && !front.contains(&Item::Track(1)), "the lock keeps the back out: {front:?}");
    e.box_both_sides = true;
    // the group moves, the back track with it
    let g = GroupDrag { anchor: p(0.0, 0.0), orig: e.board.clone(), items: all };
    e.move_group(&g, 5.0, 2.0);
    let back = &e.board.tracks[1];
    assert!((back.a.x - 13.0).abs() < 1e-6 && (back.a.y - 18.0).abs() < 1e-6, "{:?}", back.a);
}

/// The marks of the lands that are only roughly right follow the library at each import: a part whose land was checked since (the mark
/// taken off its file) is no longer listed, though an older import had written it into the board.
#[test]
fn an_import_forgets_the_marks_of_lands_that_were_checked_since() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut sch = crate::schematic::SchEditor::default();
    sch.load_path(root.join("arduino_minimal.schjson"));
    let fp = sch.sch.flat().components.iter().find(|c| c.custom.is_some()).map(|c| c.footprint.clone()).expect("a library part");
    let unflagged = !sch.library.iter().any(|d| d.footprint_name() == fp && d.land_unverified);
    assert!(unflagged, "the test needs a part whose land is not marked");
    let mut e = BoardEditor::default();
    e.board.land_unverified.insert(fp.clone(), "STALE".into());
    e.import_from_schematic(&sch);
    assert!(!e.board.land_unverified.contains_key(&fp), "{:?}", e.board.land_unverified);
}
