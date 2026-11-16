use super::*;

/// Overlaps smaller than this (mm, each way) are let pass: bodies drawn edge to edge, rounding of the data.
const BODY_SLACK: f64 = 0.05;

/// Parts whose bodies overlap on the same side, so they cannot both be mounted. Only a part whose data gives its body (a 3D
/// mesh or the data sheet's body, `model3d::body_extent`) takes part, against the room of every other part on its side (its
/// body, else its pads): two parts known only by their pads are left to the copper checks, since their pads alone say little
/// about where the plastic is. A through-hole part stands on the top.
pub fn body_overlap_checks(board: &Board) -> Vec<Violation> {
    let face = |p: &Part| if p.side == Side::Bottom { Side::Bottom } else { Side::Top };
    let rooms: Vec<(&Part, (Point, Point), bool)> = board
        .parts
        .iter()
        .filter_map(|p| {
            let body = crate::model3d::body_extent(board, p);
            let room = crate::autoplace::extent(board, &p.refdes)?;
            Some((p, room, body.is_some()))
        })
        .collect();
    let mut out = vec![];
    for (i, (a, ra, ba)) in rooms.iter().enumerate() {
        for (b, rb, bb) in &rooms[i + 1..] {
            if !(*ba || *bb) || face(a) != face(b) {
                continue;
            }
            let (x0, x1) = (ra.0.x.max(rb.0.x), ra.1.x.min(rb.1.x));
            let (y0, y1) = (ra.0.y.max(rb.0.y), ra.1.y.min(rb.1.y));
            if x1 - x0 > BODY_SLACK && y1 - y0 > BODY_SLACK {
                out.push(v(
                    format!("{} と {} の本体が重なっています ({:.1} × {:.1} mm)。部品の本体はパッドより大きいことがあります (3D の形・データシートの本体の大きさ)。どちらかを動かして、3D で確かめましょう", a.refdes, b.refdes, x1 - x0, y1 - y0),
                    Point::new((x0 + x1) / 2.0, (y0 + y1) / 2.0),
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::Mesh3d;

    /// A box mesh `w` × `h` (footprint frame, centred on `c`) for a footprint named `fp`.
    fn with_box(b: &mut Board, fp: &str, c: [f32; 2], w: f32, h: f32) {
        let (x0, x1, y0, y1) = (c[0] - w / 2.0, c[0] + w / 2.0, c[1] - h / 2.0, c[1] + h / 2.0);
        let v = vec![[x0, y0, 0.0], [x1, y0, 0.0], [x1, y1, 0.0], [x0, y1, 0.0], [x0, y0, 5.0], [x1, y0, 5.0], [x1, y1, 5.0], [x0, y1, 5.0]];
        b.models3d.insert(fp.into(), Mesh3d { vertices: v, triangles: vec![[0, 1, 2], [0, 2, 3], [4, 6, 5], [4, 7, 6]], palette: vec![[30, 30, 30]], tri_color: vec![0; 4] });
    }

    fn part(b: &mut Board, refdes: &str, fp: &str, at: (f64, f64), rot: u8) {
        let mut p = Pad::new(Point::new(at.0, at.1), PadShape::Circle { d: 1.6 }, Some(0.8), Side::Top);
        p.refdes = refdes.into();
        p.pin = "1".into();
        b.pads.push(p);
        b.parts.push(Part { refdes: refdes.into(), value: String::new(), footprint: fp.into(), pos: Point::new(at.0, at.1), rot, side: Side::Top });
    }

    /// A jack whose housing reaches 14 mm to the −x of its pin overlaps a part standing there, though their pads are far
    /// apart; turned the other way it does not. Parts known only by their pads are not compared.
    #[test]
    fn a_body_bigger_than_its_pads_is_seen() {
        let mut b = Board::new("t", 60.0, 30.0);
        with_box(&mut b, "JACK", [-7.0, 0.0], 14.0, 9.0);
        part(&mut b, "J1", "JACK", (30.0, 15.0), 0);
        part(&mut b, "R1", "R", (20.0, 15.0), 0);
        let found = body_overlap_checks(&b);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].message.contains("J1") && found[0].message.contains("R1"));
        // the room autoplace leaves is the body's too
        let (lo, _) = crate::autoplace::extent(&b, "J1").unwrap();
        assert!((lo.x - 16.0).abs() < 1e-6, "{lo:?}");
        // turned half round, the housing points away from R1
        b.parts[0].rot = 2;
        assert!(body_overlap_checks(&b).is_empty());
        // two parts without a body, pads overlapping: left to the copper checks
        let mut c = Board::new("t", 30.0, 30.0);
        part(&mut c, "R1", "R", (10.0, 10.0), 0);
        part(&mut c, "R2", "R", (10.5, 10.0), 0);
        assert!(body_overlap_checks(&c).is_empty());
    }

    /// The body the data sheet gives counts as well, and parts on opposite faces do not collide.
    #[test]
    fn a_data_sheet_body_counts_and_the_other_face_does_not() {
        let mut b = Board::new("t", 40.0, 30.0);
        let mut fp = crate::footprints::get("R_0805").unwrap();
        fp.name = "BIG".into();
        fp.body = Some(serde_json::from_str(r#"{"w": 10.0, "h": 10.0}"#).unwrap());
        b.add_footprint(fp);
        part(&mut b, "U1", "BIG", (20.0, 15.0), 0);
        part(&mut b, "C1", "R", (24.0, 15.0), 0);
        assert_eq!(body_overlap_checks(&b).len(), 1);
        b.parts[1].side = Side::Bottom;
        assert!(body_overlap_checks(&b).is_empty(), "C1 on the bottom");
    }
}
