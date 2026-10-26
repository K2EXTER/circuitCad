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
use i_overlay::mesh::float::style::{LineJoin, OutlineStyle};

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
/// Where a slot ends between two pads of its net, a notch is left open: this many slot widths deep from the pads' edge,
/// with a rounded tip (a solid wall of copper up to the pads would read as one wide pad).
const NOTCH_DEPTH: f64 = 1.4;
/// The tip of the notch is rounded by opening it with this fraction of the slot width.
const NOTCH_ROUND: f64 = 0.45;
/// A pad counts as touching a slot when it comes this close (mm) to it.
const TOUCH: f64 = 0.03;

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
        type Group = (Vec<Path>, Vec<(Point, f64)>, f64, usize, Vec<Path>);
        let mut groups: std::collections::BTreeMap<usize, Group> = Default::default();
        for (i, t) in board.tracks.iter().enumerate().filter(|(_, t)| t.side == side) {
            let g = groups.entry(conn.items[i]).or_insert_with(|| (vec![], vec![], 0.0, i, vec![]));
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
                g.4.push(pad_path(p.pos, p.shape, 0.0));
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
        let groups: Vec<Group> = groups.into_values().collect();
        // (the groups are independent: one thread for each)
        let found: Vec<Vec<SlotPatch>> = crate::par::par_map(&groups, |(paths, discs, width, track, pads)| {
            let mut mine = vec![];
            // the copper of a net that has a pour is a plane's stubs and vias: the pour carries it, and these groups are the biggest
            // of the board (a second of the finish on a board with three planes)
            if conn.track_net(*track).is_some_and(|n| board.zones.iter().any(|z| z.net == n)) {
                return mine;
            }
            let d = (width.min(radius) * REACH_OF_WIDTH).clamp(REACH.0, REACH.1);
            if !has_close_pair(discs, 2.0 * d) {
                return mine;
            }
            // the pads are left out of the closing: the space between a pad and a track beside it is not a slot (closing it makes a
            // funnel from the track to the pad, wider than the neck it belongs to); the slot between two tracks is
            let tracks_only: Vec<Path> = paths.iter().filter(|p| !pads.contains(p)).cloned().collect();
            for slot in closed_pieces(&tracks_only, d) {
                for outline in notched(&slot, pads, 2.0 * d) {
                    mine.push(SlotPatch { side, outline, track: *track });
                }
            }
            mine
        });
        out.extend(found.into_iter().flatten());
    }
    out
}

/// The copper of `paths` grown by `r` (shrunk when `r` is negative), as paths. Corners are round (the closing above cuts them
/// off, which is fine for a patch that is mostly covered; the notch is seen, so its tip has to be a real arc).
fn grown(paths: &[Path], r: f64) -> Vec<Path> {
    let none: Vec<Path> = vec![];
    let union = paths.to_vec().overlay(&none, OverlayRule::Union, FillRule::NonZero);
    union.outline(&OutlineStyle::new(r).line_join(LineJoin::Round(0.15))).into_iter().flatten().collect()
}

fn area_of(paths: &[Path]) -> f64 {
    paths.iter().map(|p| signed_area(&p.iter().map(|q| Point::new(q[0], q[1])).collect::<Vec<_>>()).abs()).sum()
}

/// A slot that ends between two or more pads of its net keeps a notch open toward them: the part of the slot within
/// `NOTCH_DEPTH` slot widths (`gap`) of the pads is left out, with a rounded tip. Any other slot comes back whole.
fn notched(slot: &[Point], pads: &[Path], gap: f64) -> Vec<Vec<Point>> {
    let slot_path: Vec<Path> = vec![slot.iter().map(|p| [p.x, p.y]).collect()];
    let touching: Vec<Path> = pads.iter().filter(|p| area_of(&slot_path.clone().overlay(&grown(std::slice::from_ref(*p), TOUCH), OverlayRule::Intersect, FillRule::NonZero).into_iter().flatten().collect::<Vec<_>>()) > 1e-6).cloned().collect();
    if touching.len() < 2 {
        return vec![slot.to_vec()];
    }
    let near = grown(&touching, NOTCH_DEPTH * gap);
    let cut: Vec<Path> = slot_path.clone().overlay(&near, OverlayRule::Intersect, FillRule::NonZero).into_iter().flatten().collect();
    // a slot that lies wholly within the notch depth (a short one: the necks of a trunk) is all notch: nothing is filled, instead of the
    // whole slot coming back as one block between the pads
    if area_of(&cut) >= 0.98 * area_of(&slot_path) {
        return vec![];
    }
    // opening: what is thinner than the rounding goes, and the tip comes back round
    let opened = grown(&grown(&cut, -NOTCH_ROUND * gap), NOTCH_ROUND * gap);
    if opened.is_empty() {
        return vec![slot.to_vec()];
    }
    let kept = slot_path.overlay(&opened, OverlayRule::Difference, FillRule::NonZero);
    let mut out = vec![];
    for shape in kept {
        let Some(outer) = shape.first() else { continue };
        let mut pts: Vec<Point> = outer.iter().map(|p| Point::new(p[0], p[1])).collect();
        if pts.len() < 3 || signed_area(&pts).abs() < MIN_PIECE * gap * gap / 4.0 {
            continue;
        }
        if signed_area(&pts) < 0.0 {
            pts.reverse();
        }
        out.push(pts);
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
    use std::hash::{Hash, Hasher};
    // the result depends on these paths and `d` alone, and it costs milliseconds per piece of copper: a piece that is the same as
    // in the last run (most of them, after one edit) comes from the memory
    thread_local! {
        static MEMORY: std::cell::RefCell<std::collections::HashMap<u64, Vec<Vec<Point>>>> = Default::default();
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    d.to_bits().hash(&mut h);
    for p in paths {
        p.len().hash(&mut h);
        for q in p {
            q[0].to_bits().hash(&mut h);
            q[1].to_bits().hash(&mut h);
        }
    }
    let key = h.finish();
    if let Some(hit) = MEMORY.with(|m| m.borrow().get(&key).cloned()) {
        return hit;
    }
    let out = close_pieces(paths, d);
    MEMORY.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() >= 1024 {
            m.clear();
        }
        m.insert(key, out.clone());
    });
    out
}

fn close_pieces(paths: &[Path], d: f64) -> Vec<Vec<Point>> {
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

    /// Two pads of one net (0.65 mm apart, like the two LX pins of a QFN) with a track from each running left to a common trunk:
    /// the slot between the tracks is filled, but it stays open for a rounded notch in front of the pads.
    fn lx_pair() -> Board {
        let mut b = Board::new("t", 40.0, 40.0);
        for y in [18.825, 18.175] {
            let mut p = Pad::new(Point::new(15.05, y), PadShape::Rect { w: 0.85, h: 0.3 }, None, Side::Top);
            p.net = "LX".into();
            b.pads.push(p);
            b.tracks.push(Track { side: Side::Top, a: Point::new(15.05, y), b: Point::new(12.5, y), width: 0.25 });
        }
        b.tracks.push(Track { side: Side::Top, a: Point::new(12.5, 18.175), b: Point::new(12.5, 18.825), width: 0.25 });
        b
    }

    fn covered(patches: &[SlotPatch], x: f64, y: f64) -> bool {
        patches.iter().any(|p| crate::geom::inside_polygon(&p.outline, Point::new(x, y)))
    }

    #[test]
    fn the_slot_between_two_pads_of_a_net_keeps_a_rounded_notch_in_front_of_them() {
        let b = lx_pair();
        let patches = slot_patches(&b, &crate::drc::connectivity(&b), &[], 0.5);
        assert!(!patches.is_empty());
        let edge = 15.05 - 0.425; // the pads' left edge
        // the slot is 0.4 wide, the notch 1.4 x 0.4 deep: nothing within 0.4 of the pads, copper well beyond the tip
        assert!(!covered(&patches, edge - 0.2, 18.5), "open just in front of the pads");
        assert!(!covered(&patches, edge + 0.3, 18.5), "and between them");
        assert!(covered(&patches, edge - 0.9, 18.5), "filled beyond the notch");
        assert!(covered(&patches, edge - 1.5, 18.5));
        // the tip is round: on the centre line the notch is deepest, near the slot's edges it is shallower
        let depth_at = |y: f64| (0..200).map(|k| edge - k as f64 * 0.01).find(|x| covered(&patches, *x, y)).map(|x| edge - x);
        let (mid, edge_row) = (depth_at(18.5).unwrap(), depth_at(18.3 + 0.02).unwrap());
        assert!(mid > edge_row + 0.04, "a round tip: {mid} on the centre line, {edge_row} near the wall");
        assert!((0.4..0.7).contains(&mid), "about 1.4 slot widths deep: {mid}");
    }

    /// A slot shorter than the notch (the necks of a trunk are 0.4 mm long in front of the pads) is all notch: nothing is filled, instead of the
    /// whole slot coming back as one block between the pads.
    #[test]
    fn a_slot_shorter_than_the_notch_is_left_open() {
        let mut b = lx_pair();
        // the tracks end 0.4 mm in front of the pads' left edge (x = 14.625) and are joined by a wide one there
        for t in b.tracks.iter_mut().take(2) {
            t.b.x = 14.225;
        }
        b.tracks.truncate(2);
        b.tracks.push(Track { side: Side::Top, a: Point::new(14.225, 18.5), b: Point::new(12.0, 18.5), width: 0.9 });
        let patches = slot_patches(&b, &crate::drc::connectivity(&b), &[], 0.5);
        assert!(!covered(&patches, 15.05, 18.5), "nothing between the pads");
        assert!(!covered(&patches, 14.4, 18.5), "nor in front of them");
    }

    #[test]
    fn a_slot_not_between_two_pads_is_filled_to_its_end() {
        // one pad only: the slot between two tracks that end at it is filled right up to the pad (no notch)
        let mut b = lx_pair();
        b.pads.truncate(1);
        let patches = slot_patches(&b, &crate::drc::connectivity(&b), &[], 0.5);
        let edge = 15.05 - 0.425;
        assert!(covered(&patches, edge - 0.2, 18.5), "{patches:?}");
    }

    #[test]
    fn the_finish_leaves_the_slot_open_when_another_net_is_in_it() {
        let mut b = u_shape(0.7);
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
