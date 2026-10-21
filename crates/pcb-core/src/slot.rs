//! Narrow slots of one net's copper, filled as polygons for the finish: the cut between two tracks that run side by side
//! to the same place (a pair of tracks from two pads that join into one trunk, a hairpin), where the fork patch of
//! `smooth` only reaches the sharp tip. The slot is found by closing the copper of a piece (grow it by `d`, shrink it by
//! `d`: whatever is narrower than `2 d` fills in), and what the closing added is the patch. Nothing about the tracks
//! changes; a patch is used only where the other copper keeps its distance (`drc::PatchCheck`, in `smooth::finished`).

use crate::drc::Connectivity;
use crate::geom::signed_area;
use crate::model::*;
use crate::zonefill::{capsule, circle, pad_path};
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use i_overlay::mesh::float::outline::offset::OutlineOffset;
use i_overlay::mesh::float::style::OutlineStyle;

type Path = Vec<[f64; 2]>;

/// The closing reaches this fraction of the track width (or of the finish's corner radius, if that is smaller), so
/// slots narrower than twice that fill in. Less than 1: the round pockets that `smooth` leaves at the tip of a notch
/// (radius = the track width, or the corner radius) are wider than the closing can fill, and stay.
const REACH_OF_WIDTH: f64 = 0.8;
/// The reach of the closing stays in this range (mm).
const REACH: (f64, f64) = (0.12, 0.8);
/// How far (mm) the patches reach into the copper they join.
const SINK: f64 = 0.02;
/// A piece smaller than this many `d * d` is only a rounded inside corner, which is not a slot.
const MIN_PIECE: f64 = 0.6;

/// A polygon of copper to be added on `side` to the group of track `track`.
#[derive(Clone, Debug)]
pub struct SlotPatch {
    pub side: Side,
    pub outline: Vec<Point>,
    pub track: usize,
}

/// The slots of every piece of copper (group) on every copper layer. `patches` are the polygons that the finish already
/// puts there (they count as copper, so what they leave open is not filled). The reach follows the corner `radius`.
pub fn slot_patches(board: &Board, conn: &Connectivity, patches: &[crate::smooth::Patch], radius: f64) -> Vec<SlotPatch> {
    let mut out = vec![];
    for side in board.copper_layers() {
        // the shapes of each group on this side, with the widest track among them
        // per group: its shapes, a disc (centre, radius) round each of them, the widest track, a track of the group
        type Group = (Vec<Path>, Vec<(Point, f64)>, f64, usize);
        let mut groups: std::collections::BTreeMap<usize, Group> = Default::default();
        for (i, t) in board.tracks.iter().enumerate().filter(|(_, t)| t.side == side) {
            let g = groups.entry(conn.items[i]).or_insert_with(|| (vec![], vec![], 0.0, i));
            g.0.push(capsule(t.a, t.b, t.width / 2.0));
            g.1.push((Point::new((t.a.x + t.b.x) / 2.0, (t.a.y + t.b.y) / 2.0), t.a.dist(t.b) / 2.0 + t.width / 2.0));
            g.2 = g.2.max(t.width);
        }
        if groups.is_empty() {
            continue;
        }
        for patch in patches.iter().filter(|p| p.side == side) {
            if let Some(g) = groups.get_mut(&conn.items[patch.track]) {
                let mut path: Path = patch.outline.iter().map(|q| [q.x, q.y]).collect();
                if signed_area(&patch.outline) < 0.0 {
                    path.reverse();
                }
                g.0.push(path);
                let c = Point::new(patch.outline.iter().map(|q| q.x).sum::<f64>() / patch.outline.len() as f64, patch.outline.iter().map(|q| q.y).sum::<f64>() / patch.outline.len() as f64);
                g.1.push((c, patch.outline.iter().map(|q| q.dist(c)).fold(0.0, f64::max)));
            }
        }
        for (pi, p) in board.pads.iter().enumerate() {
            if !(p.side == side || p.drill.is_some()) {
                continue;
            }
            if let Some(g) = groups.get_mut(&conn.pad_group(pi)) {
                g.0.push(pad_path(p.pos, p.shape, 0.0));
                let (w, h) = p.shape.size();
                g.1.push((p.pos, w.hypot(h) / 2.0));
            }
        }
        for (vi, v) in board.vias.iter().enumerate() {
            if let Some(g) = groups.get_mut(&conn.via_group(vi)) {
                g.0.push(circle(v.pos, v.diameter / 2.0));
                g.1.push((v.pos, v.diameter / 2.0));
            }
        }
        for (paths, discs, width, track) in groups.into_values() {
            let d = (width.min(radius) * REACH_OF_WIDTH).clamp(REACH.0, REACH.1);
            if !has_close_pair(&discs, 2.0 * d) {
                continue;
            }
            for outline in closed_pieces(&paths, d) {
                out.push(SlotPatch { side, outline, track });
            }
        }
    }
    out
}

/// Are two of the discs (centre, radius) closer than `gap` to each other? (A cheap look before the polygon work.)
fn has_close_pair(discs: &[(Point, f64)], gap: f64) -> bool {
    discs.iter().enumerate().any(|(i, a)| discs[i + 1..].iter().any(|b| a.0.dist(b.0) - a.1 - b.1 < gap))
}

/// What closing the union of `paths` by `d` adds: the polygons of the new copper (outer contours; pieces with a hole
/// and corner-sized pieces are left out).
fn closed_pieces(paths: &[Path], d: f64) -> Vec<Vec<Point>> {
    let none: Vec<Path> = vec![];
    let union = paths.to_vec().overlay(&none, OverlayRule::Union, FillRule::NonZero);
    let grown = union.outline(&OutlineStyle::new(d));
    let closed = grown.outline(&OutlineStyle::new(-d));
    // the closing has to contain the copper it started from (the offsets are rounded to a grid), so take it together
    // with the copper before looking at what was added
    let closed_paths: Vec<Path> = closed.iter().flat_map(|s| s.iter().cloned()).collect();
    let union_paths: Vec<Path> = union.iter().flat_map(|s| s.iter().cloned()).collect();
    let added = closed_paths.overlay(&union_paths, OverlayRule::Difference, FillRule::NonZero);
    // only real slots: a plain piece (no hole) that is more than the rounded inside corner of a bend
    let slots: Vec<Vec<Path>> = added
        .into_iter()
        .filter(|shape| {
            let pts: Vec<Point> = shape[0].iter().map(|p| Point::new(p[0], p[1])).collect();
            shape.len() == 1 && pts.len() >= 3 && signed_area(&pts).abs() >= MIN_PIECE * d * d
        })
        .collect();
    // grown a hair into the copper around them, so that no hairline is left between a patch and the tracks
    let mut out = vec![];
    for shape in slots.outline(&OutlineStyle::new(SINK)) {
        let Some(outer) = shape.first() else { continue };
        let mut pts: Vec<Point> = outer.iter().map(|p| Point::new(p[0], p[1])).collect();
        if pts.len() < 3 {
            continue;
        }
        if signed_area(&pts) < 0.0 {
            pts.reverse();
        }
        out.push(pts);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(net_pad: bool, a: (f64, f64), b: (f64, f64)) -> Track {
        let _ = net_pad;
        Track { side: Side::Top, a: Point::new(a.0, a.1), b: Point::new(b.0, b.1), width: 0.5 }
    }

    /// Two tracks side by side, `gap` mm between their edges, joined at the bottom by a third one (a U).
    fn u_shape(gap: f64) -> Board {
        let mut b = Board::new("t", 40.0, 40.0);
        let x2 = 10.0 + 0.5 + gap;
        b.tracks = vec![track(false, (10.0, 10.0), (10.0, 20.0)), track(false, (x2, 10.0), (x2, 20.0)), track(false, (10.0, 10.0), (x2, 10.0))];
        b
    }

    fn area(p: &SlotPatch) -> f64 {
        signed_area(&p.outline).abs()
    }

    #[test]
    fn the_slot_of_a_u_is_filled() {
        let b = u_shape(0.7);
        let patches = slot_patches(&b, &crate::drc::connectivity(&b), &[], 0.5);
        assert!(!patches.is_empty());
        let total: f64 = patches.iter().map(area).sum();
        // the slot is 0.7 wide and about 9.5 long (less the rounded end): most of it is filled
        assert!(total > 4.0 && total < 8.0, "{total}");
    }

    #[test]
    fn a_wide_gap_or_a_lone_track_gets_nothing() {
        let b = u_shape(3.0);
        assert!(slot_patches(&b, &crate::drc::connectivity(&b), &[], 0.5).is_empty());
        let mut one = Board::new("t", 40.0, 40.0);
        one.tracks = vec![track(false, (10.0, 10.0), (10.0, 20.0))];
        assert!(slot_patches(&one, &crate::drc::connectivity(&one), &[], 0.5).is_empty());
    }

    #[test]
    fn the_finish_leaves_the_slot_open_when_another_net_is_in_it() {
        let mut b = u_shape(0.7);
        b.finish.on = true;
        b.finish.forks = true;
        let with = crate::smooth::finished(&b).unwrap().patches.len();
        assert!(with > 0, "the finished board has the slot patch");
        // a pad of another net in the middle of the slot
        let mut p = Pad::new(Point::new(10.6, 15.0), PadShape::Rect { w: 0.2, h: 0.2 }, None, Side::Top);
        p.net = "X".into();
        b.pads.push(p);
        let without = crate::smooth::finished(&b).unwrap().patches.len();
        assert!(without < with, "{without} < {with}");
    }
}
