use crate::model::*;

fn board() -> Board {
    let mut b = Board::new("t", 40.0, 30.0);
    b.set_part(Part { refdes: "R1".into(), value: "10k".into(), footprint: "R_0805".into(), pos: Point::new(10.0, 10.0), rot: 0, side: Side::Top }).unwrap();
    b
}

fn label(b: &Board) -> Point {
    b.texts.iter().find(|t| t.refdes == "R1").unwrap().pos
}

#[test]
fn a_moved_designator_keeps_its_offset_when_the_part_moves_and_turns() {
    let mut b = board();
    let auto = label(&b);
    assert!(b.move_label("R1", Point::new(auto.x + 2.0, auto.y + 3.0)));
    assert_eq!(label(&b), Point::new(auto.x + 2.0, auto.y + 3.0));
    assert_eq!(b.label_offsets["R1"], Point::new(2.0, 3.0));
    // the part moves 5 mm right: the text goes with it, still 2 / 3 mm from the automatic spot
    b.place_part("R1", Point::new(15.0, 10.0), 0);
    assert_eq!(label(&b), Point::new(auto.x + 5.0 + 2.0, auto.y + 3.0));
    // moving it again adds to the offset (the text is dragged in steps)
    b.move_label("R1", Point::new(label(&b).x + 1.0, label(&b).y));
    assert_eq!(b.label_offsets["R1"], Point::new(3.0, 3.0));
}

#[test]
fn a_designator_goes_back_to_the_automatic_spot_and_leaves_with_its_part() {
    let mut b = board();
    let auto = label(&b);
    assert!(!b.reset_label("R1"), "nothing to put back yet");
    b.move_label("R1", Point::new(auto.x - 4.0, auto.y + 1.0));
    assert!(b.reset_label("R1"));
    assert_eq!(label(&b), auto);
    assert!(b.label_offsets.is_empty());
    b.move_label("R1", Point::new(auto.x, auto.y + 2.0));
    b.remove_part("R1");
    assert!(b.label_offsets.is_empty(), "no entry is left for a part that is gone");
    assert!(!b.move_label("R1", Point::new(0.0, 0.0)), "a part with no text cannot be moved");
}

#[test]
fn the_offset_is_saved_with_the_board_and_old_files_still_load() {
    let mut b = board();
    let auto = label(&b);
    b.move_label("R1", Point::new(auto.x + 1.5, auto.y));
    let json = serde_json::to_string(&b).unwrap();
    let back: Board = serde_json::from_str(&json).unwrap();
    assert_eq!(back.label_offsets["R1"], Point::new(1.5, 0.0));
    // a board without the field (every file written before this) is read with none
    let mut v: serde_json::Value = serde_json::from_str(&serde_json::to_string(&board()).unwrap()).unwrap();
    v.as_object_mut().unwrap().remove("label_offsets");
    let old: Board = serde_json::from_value(v).unwrap();
    assert!(old.label_offsets.is_empty());
}
