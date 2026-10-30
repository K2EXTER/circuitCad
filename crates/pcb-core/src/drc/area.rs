//! The area of the copper that a pad is joined to: what a part's heat can spread into.

use super::*;
use crate::zonefill::{fill_area, zone_fill, FillShape};
use std::collections::HashSet;

fn pad_area(shape: PadShape) -> f64 {
    match shape {
        PadShape::Circle { d } => std::f64::consts::PI * d * d / 4.0,
        PadShape::Rect { w, h } => w * h,
        PadShape::Oval { w, h } => {
            let (short, long) = (w.min(h), w.max(h));
            std::f64::consts::PI * short * short / 4.0 + (long - short) * short
        }
    }
}

fn in_piece(piece: &FillShape, p: Point) -> bool {
    inside_polygon(&piece.outer, p) && !piece.holes.iter().any(|h| inside_polygon(h, p))
}

/// The area (mm²) of the copper joined to the pad(s) `r` (every pad of that part and pin: an exposed pad and a pin of one net count
/// together), and the middle of the first of them. The copper is the pads, tracks and vias of the group the pad is in, and the pieces of
/// the pours of its net that hold something of the group. What lies in a pour piece is not counted a second time. A track is its length
/// times its width (the round ends are left out). None when the board has no such pad.
pub fn pad_copper_area(board: &Board, r: &PadRef) -> Option<(f64, Point)> {
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
    let covered = |side: Option<Side>, p: Point| pieces.iter().any(|(s, piece)| side.is_none_or(|x| x == *s) && in_piece(piece, p));

    let mut area: f64 = pieces.iter().map(|(_, p)| fill_area(std::slice::from_ref(p))).sum();
    for (i, p) in board.pads.iter().enumerate().filter(|(i, _)| pad_in(*i)) {
        let side = if p.drill.is_some() { None } else { Some(p.side) };
        if !covered(side, p.pos) {
            area += pad_area(p.shape);
        }
    }
    for (i, t) in board.tracks.iter().enumerate().filter(|(i, _)| track_in(*i)) {
        if !(covered(Some(t.side), t.a) && covered(Some(t.side), t.b)) {
            area += t.a.dist(t.b) * t.width;
        }
    }
    for (i, v) in board.vias.iter().enumerate().filter(|(i, _)| via_in(*i)) {
        if !covered(None, v.pos) {
            area += std::f64::consts::PI * v.diameter * v.diameter / 4.0;
        }
    }
    Some((area, board.pads[first].pos))
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
        assert!((pad_copper_area(&b, &r).unwrap().0 - 4.0).abs() < 1e-9);
        // a track of 10 mm by 1 mm adds 10 mm²
        b.tracks.push(Track { side: Side::Top, a: Point::new(10.0, 10.0), b: Point::new(20.0, 10.0), width: 1.0 });
        assert!((pad_copper_area(&b, &r).unwrap().0 - 14.0).abs() < 1e-9);
        // a 10 x 10 mm pour of the net on the same side takes over what lies in it: the pad and the track are not counted again
        b.zones.push(Zone::new("DRN", Side::Top, vec![Point::new(5.0, 5.0), Point::new(15.0, 5.0), Point::new(15.0, 15.0), Point::new(5.0, 15.0)]));
        let with_pour = pad_copper_area(&b, &r).unwrap().0;
        assert!(with_pour > 99.0 && with_pour < 112.0, "{with_pour}");
        // a pour of another net does not count
        b.zones.clear();
        b.zones.push(Zone::new("GND", Side::Top, vec![Point::new(5.0, 5.0), Point::new(15.0, 5.0), Point::new(15.0, 15.0), Point::new(5.0, 15.0)]));
        assert!((pad_copper_area(&b, &r).unwrap().0 - 14.0).abs() < 1e-9);
        // a pad that is not there
        assert!(pad_copper_area(&b, &PadRef { refdes: "Q9".into(), pin: "1".into() }).is_none());
    }
}
