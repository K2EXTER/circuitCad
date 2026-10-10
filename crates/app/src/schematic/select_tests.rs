use super::*;

/// R1 at (0,0) and R2 at (10,0), a wire between them and a label near R2.
fn editor() -> SchEditor {
    let mut e = SchEditor::default();
    e.sch = Schematic::default();
    e.sch.add(Kind::Resistor, (0, 0), 0);
    e.sch.add(Kind::Resistor, (10, 0), 0);
    e.sch.wires.push(Wire { a: (2, 0), b: (8, 0) });
    e.sch.labels.push(Label { pos: (10, -4), text: "X".into() });
    e
}

#[test]
fn undo_and_redo_step_through_committed_edits() {
    let mut e = editor();
    e.reset_history();
    e.sch.wires.push(Wire { a: (0, 5), b: (4, 5) });
    e.commit_history();
    e.sch.labels.clear();
    e.commit_history();
    assert_eq!((e.sch.wires.len(), e.sch.labels.len()), (2, 0));
    e.undo();
    assert_eq!((e.sch.wires.len(), e.sch.labels.len()), (2, 1));
    e.undo();
    assert_eq!(e.sch.wires.len(), 1);
    e.undo(); // nothing left
    assert_eq!(e.sch.wires.len(), 1);
    e.redo();
    e.redo();
    assert_eq!((e.sch.wires.len(), e.sch.labels.len()), (2, 0));
    // a new edit discards the redo history
    e.undo();
    e.sch.wires.clear();
    e.commit_history();
    e.redo();
    assert_eq!(e.sch.wires.len(), 0);
}

#[test]
fn uncommitted_edit_is_undone_in_one_step() {
    let mut e = editor();
    e.reset_history();
    e.sch.components[0].value = "47k".into();
    e.undo(); // commits the pending edit first, then reverts it
    assert_ne!(e.sch.components[0].value, "47k");
    e.redo();
    assert_eq!(e.sch.components[0].value, "47k");
}

#[test]
fn live_update_reruns_the_open_analysis_after_an_edit() {
    let mut e = editor();
    e.run_dc();
    e.live = true;
    e.live_update(); // first call syncs the snapshot
    let before = e.dc.as_ref().unwrap().signature.clone();
    e.sch.components[0].value = "2.2k".into();
    assert_ne!(e.dc.as_ref().unwrap().signature, e.signature(), "stale before the live update");
    e.live_update();
    assert_eq!(e.dc.as_ref().unwrap().signature, e.signature());
    assert_ne!(e.dc.as_ref().unwrap().signature, before);
    e.live = false;
    e.sch.components[0].value = "3.3k".into();
    e.live_update();
    assert_ne!(e.dc.as_ref().unwrap().signature, e.signature(), "off: nothing is recomputed");
}

#[test]
fn dc_result_colours_every_wire_and_reports_part_currents() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/led.schjson");
    let mut e = editor();
    e.sch = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    e.run_dc();
    let dc = e.dc.as_ref().unwrap();
    let res = dc.outcome.as_ref().expect("the LED example converges");
    assert_eq!(dc.wire_v.len(), e.sch.wires.len());
    assert!(dc.wire_v.iter().all(|v| v.is_some()), "every wire belongs to a net with a voltage");
    assert!(dc.v_range.1 > dc.v_range.0);
    assert!(e.sch.components.iter().filter(|c| !c.kind.is_power_symbol()).all(|c| res.element(&c.refdes).is_some()));
    // blue at the bottom of the range, red at the top
    let (lo, hi) = (volt_color(0.0, (0.0, 5.0)), volt_color(5.0, (0.0, 5.0)));
    assert!(lo.b() > lo.r() && hi.r() > hi.b());
}

#[test]
fn parts_may_touch_but_not_overlap_and_ground_symbols_are_exempt() {
    let e = editor(); // R1 at (0,0), R2 at (10,0); a resistor is 4 wide and 2 tall
    let r = |pos, rot| e.would_overlap(Kind::Resistor, extents(Kind::Resistor), pos, rot, &[]);
    assert!(r((0, 0), 0), "exactly on top of R1");
    assert!(r((3, 0), 0), "half on R1");
    assert!(r((0, 1), 0), "shifted one row");
    assert!(!r((4, 0), 0), "pins meeting end to end is the normal way to join parts");
    assert!(!r((0, 2), 0), "stacked with the edges touching");
    assert!(r((4, 0), 1) == false && r((3, 0), 1) == false, "a vertical resistor beside R1 fits");
    assert!(r((1, 0), 1), "a vertical resistor across R1 does not");
    // moving a part does not collide with itself
    assert!(!e.would_overlap(Kind::Resistor, extents(Kind::Resistor), (0, 0), 0, &[0]));
    // a ground symbol sits right on R1's pin
    assert!(!e.would_overlap(Kind::Ground, extents(Kind::Ground), (2, 0), 0, &[]));
    assert!(!e.would_overlap(Kind::Ground, extents(Kind::Ground), (0, 0), 0, &[]));
}

#[test]
fn rotating_into_a_neighbour_is_refused() {
    let mut e = editor(); // R1 at (0,0) fills x -2..2, y -1..1
    e.sch.add(Kind::Resistor, (0, 2), 0); // R3 right above it, edges touching
    e.rotate_comp(2); // upright it would span y 0..4 and cut into R1
    assert_eq!(e.sch.components[2].rot, 0);
    e.sch.components[2].pos = (0, 5); // clear of everything
    e.rotate_comp(2);
    assert_eq!(e.sch.components[2].rot, 1);
}

#[test]
fn window_box_needs_full_enclosure_and_crossing_box_accepts_a_touch() {
    let e = editor();
    // covers R1 completely and only the left part of R2 and of the wire's far end
    let (a, b) = ((-4.0, -3.0), (9.0, 3.0));
    let window = e.objects_in_box(a, b, false);
    assert!(window.contains(&Sel::Comp(0)));
    assert!(!window.contains(&Sel::Comp(1)), "R2 is only partly inside");
    assert!(window.contains(&Sel::Wire(0)), "the wire (2..8) is fully inside");
    assert!(!window.contains(&Sel::Label(0)));
    let crossing = e.objects_in_box(a, b, true);
    assert!(crossing.contains(&Sel::Comp(0)) && crossing.contains(&Sel::Comp(1)));
    // a box that only grazes the wire in the middle
    let graze = e.objects_in_box((4.0, -1.0), (5.0, 1.0), true);
    assert_eq!(graze, vec![Sel::Wire(0)]);
    assert!(e.objects_in_box((4.0, -1.0), (5.0, 1.0), false).is_empty());
}

#[test]
fn selecting_many_then_deleting_removes_exactly_those() {
    let mut e = editor();
    let items = e.objects_in_box((-4.0, -3.0), (9.0, 3.0), true);
    e.set_selection(items);
    assert!(e.selected.is_none() && e.multi.len() >= 3);
    e.delete_selected();
    assert!(e.sch.components.is_empty() && e.sch.wires.is_empty());
    assert_eq!(e.sch.labels.len(), 1, "the label was outside the box");
    // a single hit goes to the normal selection
    let mut e = editor();
    e.set_selection(vec![Sel::Label(0)]);
    assert_eq!(e.selected, Some(Sel::Label(0)));
    assert!(e.multi.is_empty());
}
