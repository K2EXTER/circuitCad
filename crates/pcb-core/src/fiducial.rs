//! Fiducials: small round marks of bare copper with a clear ring of no solder mask round them, which the camera of a
//! pick-and-place machine finds to line the board up. Three go near three corners of the board (not four: with three in an
//! uneven triangle the machine also sees a board that is turned round). They are loose pads (no part, no net) with a mask
//! opening of their own (`Pad::mask_open`); pours keep out of that ring, and the DRC reports copper inside it.
//!
//! Only needed when a shop mounts the parts by machine; a hand-soldered board does not need them.

use crate::drc::{check, geo::point_seg, Violation};
use crate::model::*;

/// The pin name of a fiducial pad (the pad has no part).
pub const PIN: &str = "FID";

#[derive(Clone, Debug)]
pub struct FiducialOptions {
    /// Diameter of the copper dot (mm).
    pub diameter: f64,
    /// Diameter of the round opening in the solder mask round the dot (mm).
    pub opening: f64,
    pub side: Side,
}

impl Default for FiducialOptions {
    fn default() -> Self {
        Self { diameter: 1.0, opening: 3.0, side: Side::Top }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FiducialReport {
    /// Fiducials put on the board.
    pub put: usize,
    /// The corners where no place was found (左上・右上・左下).
    pub missed: Vec<&'static str>,
}

pub fn is_fiducial(p: &Pad) -> bool {
    p.refdes.is_empty() && p.pin == PIN && p.mask_open > 0.0
}

/// How far (mm) from its corner a fiducial is looked for, at most.
const REACH: f64 = 15.0;
const STEP: f64 = 0.5;

/// Put three fiducials near three corners of the board's bounding box, each where nothing comes inside its clear ring and the
/// DRC finds nothing new. Changes nothing when it fails; refuses when the side has fiducials already.
pub fn place_fiducials(board: &mut Board, opt: &FiducialOptions) -> Result<FiducialReport, String> {
    if !(opt.diameter > 0.0 && opt.opening > opt.diameter) {
        return Err("レジストの穴の直径は、印の直径より大きくしてください (目安は印の 2〜3 倍)".into());
    }
    if board.pads.iter().any(|p| is_fiducial(p) && p.side == opt.side) {
        return Err(format!("{}には、もうフィデューシャルがあります。置き直すときは、先に取り除いてください", opt.side.label()));
    }
    let Some((lo, hi)) = board.bounds().filter(|_| board.outline.len() >= 3) else {
        return Err("先に基板の外形を決めてください".into());
    };
    let mut work = board.clone();
    let mut put = 0;
    let mut missed = vec![];
    // three corners: top-left, top-right, bottom-left (y grows upwards)
    for (name, corner, sx, sy) in [("左上", Point::new(lo.x, hi.y), 1.0, -1.0), ("右上", Point::new(hi.x, hi.y), -1.0, -1.0), ("左下", Point::new(lo.x, lo.y), 1.0, 1.0)] {
        let before = check(&work).len();
        let inset = work.rules.edge_clearance + opt.opening / 2.0 + 0.3;
        let n = (REACH / STEP) as usize;
        let mut spots: Vec<Point> = (0..n).flat_map(|i| (0..n).map(move |j| (i, j))).map(|(i, j)| Point::new(corner.x + sx * (inset + i as f64 * STEP), corner.y + sy * (inset + j as f64 * STEP))).collect();
        spots.sort_by(|a, b| a.dist(corner).total_cmp(&b.dist(corner)));
        let mut done = false;
        for at in spots {
            let mut pad = Pad::new(at, PadShape::Circle { d: opt.diameter }, None, opt.side);
            pad.pin = PIN.into();
            pad.mask_open = opt.opening;
            if !clear_round(&work, &pad) {
                continue;
            }
            work.pads.push(pad);
            if check(&work).len() <= before {
                done = true;
                break;
            }
            work.pads.pop();
        }
        if done {
            put += 1;
        } else {
            missed.push(name);
        }
    }
    if put == 0 {
        return Err("フィデューシャルを置ける所が、基板の角の近くにありませんでした (部品・配線・穴でふさがっています)。角を少し空けてから、もう一度押してください".into());
    }
    *board = work;
    Ok(FiducialReport { put, missed })
}

/// Take the fiducials of `side` off the board; returns how many.
pub fn remove_fiducials(board: &mut Board, side: Side) -> usize {
    let n = board.pads.len();
    board.pads.retain(|p| !(is_fiducial(p) && p.side == side));
    n - board.pads.len()
}

/// Nothing of the board comes inside the clear ring of `fid`, and the ring keeps off the board's edge.
fn clear_round(board: &Board, fid: &Pad) -> bool {
    let ring = fid.keep_clear();
    let p = fid.pos;
    let r = &board.rules;
    let n = board.outline.len();
    if !crate::geom::inside_polygon(&board.outline, p) || (0..n).any(|i| point_seg(p, board.outline[i], board.outline[(i + 1) % n]) < ring + r.edge_clearance) {
        return false;
    }
    let gap = r.min_clearance;
    let tracks = board.tracks.iter().filter(|t| t.side == fid.side).all(|t| point_seg(p, t.a, t.b) >= ring + t.width / 2.0 + gap);
    let pads = board.pads.iter().filter(|q| q.drill.is_some() || q.side == fid.side).all(|q| {
        let (w, h) = q.shape.size();
        p.dist(q.pos) >= ring + w.max(h) / 2.0 + gap + q.keep_clear()
    });
    let vias = board.vias.iter().all(|v| p.dist(v.pos) >= ring + v.diameter / 2.0 + gap);
    let holes = board.holes.iter().all(|h| p.dist(h.pos) >= ring + h.drill.max(h.slot) / 2.0 + gap);
    // silk over the opening would be cut away by the maker, and the camera wants a plain ring
    let silk = board.silk.iter().filter(|s| s.side == fid.side).all(|s| point_seg(p, s.a, s.b) >= ring + s.width / 2.0 + 0.2);
    let texts = board.texts.iter().filter(|t| t.side == fid.side).all(|t| {
        let (w, h) = t.size();
        p.dist(t.pos) >= ring + w.max(h) / 2.0 + 0.2
    });
    tracks && pads && vias && holes && silk && texts
}

/// Copper inside the clear ring of a fiducial (it would be bare and could confuse the camera).
pub fn fiducial_checks(board: &Board) -> Vec<Violation> {
    let mut out = vec![];
    for f in board.pads.iter().filter(|p| is_fiducial(p)) {
        let ring = f.mask_open / 2.0;
        let p = f.pos;
        let hit = board.tracks.iter().any(|t| t.side == f.side && point_seg(p, t.a, t.b) < ring + t.width / 2.0)
            || board.vias.iter().any(|v| p.dist(v.pos) < ring + v.diameter / 2.0)
            || board.pads.iter().any(|q| !std::ptr::eq(q, f) && (q.drill.is_some() || q.side == f.side) && p.dist(q.pos) < ring + q.shape.size().0.max(q.shape.size().1) / 2.0);
        if hit {
            out.push(Violation {
                message: "フィデューシャル (位置合わせの印) のまわりの、レジストのない円の中に銅があります。実装機のカメラが印を見誤るので、配線・ビア・パッドを円の外へ動かしましょう".into(),
                at: p,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drc;

    #[test]
    fn three_fiducials_go_near_three_corners_and_the_pour_keeps_out_of_their_rings() {
        let mut b = Board::new("t", 40.0, 30.0);
        let mut pad = Pad::new(Point::new(20.0, 15.0), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
        pad.net = "GND".into();
        pad.refdes = "R1".into();
        pad.pin = "1".into();
        b.pads.push(pad);
        crate::groundpour::ground_pour(&mut b, &Default::default()).unwrap();
        assert_eq!(place_fiducials(&mut b, &FiducialOptions::default()).map(|r| r.put), Ok(3));
        let fids: Vec<&Pad> = b.pads.iter().filter(|p| is_fiducial(p)).collect();
        assert_eq!(fids.len(), 3);
        for f in &fids {
            let (lo, hi) = b.bounds().unwrap();
            let near = (f.pos.x - lo.x).min(hi.x - f.pos.x) < 5.0 && (f.pos.y - lo.y).min(hi.y - f.pos.y) < 5.0;
            assert!(near, "{:?}", f.pos);
        }
        assert!(drc::check_final(&b).is_empty(), "{:?}", drc::check_final(&b));
        // the top pour has a hole round each mark
        let conn = drc::connectivity(&b);
        let top: Vec<_> = crate::zonefill::merged_fills(&b, &conn).into_iter().filter(|(s, n, _)| *s == Side::Top && n == "GND").flat_map(|(_, _, f)| f).collect();
        let near_copper = crate::zonefill::copper_area(&top, Some((fids[0].pos, 1.5)));
        assert!(near_copper < 0.05, "{near_copper} mm² of pour inside the ring");
        // the mask has its own big opening
        let mask = crate::gerber::mask(&b, Side::Top);
        assert!(mask.contains("C,3.0") || mask.contains("C,3."), "{mask}");
        // a second time is refused; removing takes all three away
        assert!(place_fiducials(&mut b, &FiducialOptions::default()).is_err());
        assert_eq!(remove_fiducials(&mut b, Side::Top), 3);
    }

    #[test]
    fn copper_in_the_ring_is_reported() {
        let mut b = Board::new("t", 40.0, 30.0);
        place_fiducials(&mut b, &FiducialOptions::default()).unwrap();
        let f = b.pads.iter().find(|p| is_fiducial(p)).unwrap().pos;
        assert!(fiducial_checks(&b).is_empty());
        b.tracks.push(Track { side: Side::Top, a: Point::new(f.x + 1.0, f.y - 5.0), b: Point::new(f.x + 1.0, f.y + 5.0), width: 0.25 });
        assert_eq!(fiducial_checks(&b).len(), 1);
        assert!(drc::check(&b).iter().any(|v| v.message.contains("フィデューシャル")));
    }

    #[test]
    fn a_crowded_corner_moves_the_mark_inwards() {
        let mut b = Board::new("t", 40.0, 30.0);
        let (lo, hi) = b.bounds().unwrap();
        // a big pad right in the top-left corner
        let mut pad = Pad::new(Point::new(lo.x + 3.0, hi.y - 3.0), PadShape::Rect { w: 4.0, h: 4.0 }, None, Side::Top);
        pad.refdes = "J1".into();
        pad.pin = "1".into();
        b.pads.push(pad);
        assert_eq!(place_fiducials(&mut b, &FiducialOptions::default()).map(|r| r.put), Ok(3));
        let f = b.pads.iter().find(|p| is_fiducial(p)).unwrap();
        assert!(f.pos.dist(Point::new(lo.x + 3.0, hi.y - 3.0)) > 3.5, "{:?}", f.pos);
        assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
    }
}
