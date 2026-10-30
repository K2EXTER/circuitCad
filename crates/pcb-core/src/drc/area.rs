//! The area of the copper that a pad is joined to: what a part's heat can spread into.

use super::*;
use crate::zonefill::{copper_area, pad_piece, track_piece, via_piece, zone_fill, FillShape};
use std::collections::HashSet;

fn in_piece(piece: &FillShape, p: Point) -> bool {
    inside_polygon(&piece.outer, p) && !piece.holes.iter().any(|h| inside_polygon(h, p))
}

/// The area (mm²) of the copper joined to the pad(s) `r` (every pad of that part and pin: an exposed pad and a pin of one net count
/// together), and the middle of the first of them. The copper is the pads, tracks and vias of the group the pad is in, and the pieces of
/// the pours of its net that hold something of the group. The copper of each layer is joined into one shape, so what lies in two items
/// is counted once; the layers are added (a through-hole pad and a via are on every layer the group has copper on). With `radius` above
/// zero only the copper within that distance of the first pad's middle counts (the heat of a part spreads into the copper near it). None
/// when the board has no such pad.
pub fn pad_copper_area(board: &Board, r: &PadRef, radius: f64) -> Option<(f64, Point)> {
    let conn = connectivity(board);
    let mine: Vec<usize> = board.pads.iter().enumerate().filter(|(_, p)| p.refdes == r.refdes && p.pin == r.pin).map(|(i, _)| i).collect();
    let first = *mine.first()?;
    let groups: HashSet<usize> = mine.iter().map(|&i| conn.pad_group(i)).collect();
    let pad_in = |i: usize| groups.contains(&conn.pad_group(i));
    let track_in = |i: usize| groups.contains(&conn.track_group(i));
    let via_in = |i: usize| groups.contains(&conn.via_group(i));

    // the pieces of the pours of the net that hold a pad, via or track end of the group
    let net = board.pads[first].net.as_str();
    let mut pieces: Vec<(Side, FillShape)> = vec![];
    if !net.is_empty() {
        for z in board.zones.iter().filter(|z| z.net == net) {
            for piece in zone_fill(board, z, &conn) {
                let holds = board.pads.iter().enumerate().any(|(i, p)| pad_in(i) && (p.drill.is_some() || p.side == z.side) && in_piece(&piece, p.pos))
                    || board.vias.iter().enumerate().any(|(i, v)| via_in(i) && in_piece(&piece, v.pos))
                    || board.tracks.iter().enumerate().any(|(i, t)| track_in(i) && t.side == z.side && (in_piece(&piece, t.a) || in_piece(&piece, t.b)));
                if holds {
                    pieces.push((z.side, piece));
                }
            }
        }
    }

    // the layers the group has copper on of its own (a track, a pad of one side, a pour piece)
    let mut layers: Vec<Side> = vec![];
    let mut note = |s: Side| {
        if !layers.contains(&s) {
            layers.push(s);
        }
    };
    for (s, _) in &pieces {
        note(*s);
    }
    for (i, t) in board.tracks.iter().enumerate() {
        if track_in(i) {
            note(t.side);
        }
    }
    for (i, p) in board.pads.iter().enumerate() {
        if pad_in(i) && p.drill.is_none() {
            note(p.side);
        }
    }
    if layers.is_empty() {
        layers.push(Side::Top);
    }

    let centre = board.pads[first].pos;
    let within = (radius > 0.0).then_some((centre, radius));
    let mut area = 0.0;
    for &layer in &layers {
        let mut shapes: Vec<FillShape> = pieces.iter().filter(|(s, _)| *s == layer).map(|(_, p)| p.clone()).collect();
        for (i, p) in board.pads.iter().enumerate() {
            if pad_in(i) && (p.drill.is_some() || p.side == layer) {
                shapes.push(pad_piece(p.pos, p.shape));
            }
        }
        for (i, t) in board.tracks.iter().enumerate() {
            if track_in(i) && t.side == layer {
                shapes.push(track_piece(t.a, t.b, t.width));
            }
        }
        for (i, v) in board.vias.iter().enumerate() {
            if via_in(i) {
                shapes.push(via_piece(v.pos, v.diameter));
            }
        }
        area += copper_area(&shapes, within);
    }
    Some((area, centre))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> Board {
        let mut b = Board::new("t", 60.0, 40.0);
        let mut pad = Pad::new(Point::new(10.0, 10.0), PadShape::Rect { w: 2.0, h: 2.0 }, None, Side::Top);
        pad.refdes = "Q1".into();
        pad.pin = "2".into();
        pad.net = "DRN".into();
        b.pads.push(pad);
        b
    }

    #[test]
    fn a_lone_pad_has_its_own_area_and_tracks_and_pours_add_to_it() {
        let mut b = board();
        let r = PadRef { refdes: "Q1".into(), pin: "2".into() };
        assert!((pad_copper_area(&b, &r, 0.0).unwrap().0 - 4.0).abs() < 1e-6);
        // a track of 10 mm by 1 mm from the pad's middle: what lies in the pad is not counted twice (a round end adds a little)
        b.tracks.push(Track { side: Side::Top, a: Point::new(10.0, 10.0), b: Point::new(20.0, 10.0), width: 1.0 });
        let with_track = pad_copper_area(&b, &r, 0.0).unwrap().0;
        assert!(with_track > 13.0 && with_track < 14.0, "{with_track}");
        // a 10 x 10 mm pour of the net on the same side takes over what lies in it
        b.zones.push(Zone::new("DRN", Side::Top, vec![Point::new(5.0, 5.0), Point::new(15.0, 5.0), Point::new(15.0, 15.0), Point::new(5.0, 15.0)]));
        let with_pour = pad_copper_area(&b, &r, 0.0).unwrap().0;
        assert!(with_pour > 104.0 && with_pour < 107.0, "{with_pour}");
        // only the copper within 1 mm of the pad's middle: a disc of 3.14 mm² (the pour and the pad cover all of it)
        let near = pad_copper_area(&b, &r, 1.0).unwrap().0;
        assert!(near > 3.0 && near < 3.3, "{near}");
        // a pour of another net does not count
        b.zones.clear();
        b.zones.push(Zone::new("GND", Side::Top, vec![Point::new(5.0, 5.0), Point::new(15.0, 5.0), Point::new(15.0, 15.0), Point::new(5.0, 15.0)]));
        let other = pad_copper_area(&b, &r, 0.0).unwrap().0;
        assert!((other - with_track).abs() < 1e-6, "{other}");
        // a pad that is not there
        assert!(pad_copper_area(&b, &PadRef { refdes: "Q9".into(), pin: "1".into() }, 0.0).is_none());
    }
}
