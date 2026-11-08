//! Stitching vias: vias of a pour's net (GND, as a rule) put where the net is poured on two or more layers, so the pours
//! are joined in many places and not only at the pads. Two ways to place them:
//! - a grid over the whole overlap of the pours (`StitchMode::Grid`): the return current of a signal finds a short way to
//!   the other side, and pieces of copper that touch nothing (islands) are joined to the rest;
//! - a fence along the board's edge (`StitchMode::Fence`): a row of vias just inside the outline, which keeps the edges of
//!   the two pours at the same potential (less radiation from the edge).
//!
//! A via goes only where its whole ring lies in the copper of the net on every poured layer, kept off the edge of that
//! copper by `margin`, and away from pads, holes and other vias. After that the DRC is run; vias near a new finding are
//! taken out again, and if the findings still grow nothing is changed.

use crate::drc::{check, connectivity, geo::point_seg};
use crate::model::*;
use crate::zonefill::{dropped_islands, merged_fills, overlap_shapes, FillShape};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StitchMode {
    Grid,
    Fence,
}

#[derive(Clone, Debug)]
pub struct StitchOptions {
    /// The net; `None`: GND.
    pub net: Option<String>,
    /// Distance between neighbouring vias (mm).
    pub pitch: f64,
    pub via_diameter: f64,
    pub via_drill: f64,
    pub mode: StitchMode,
    /// How far the ring of a via keeps off the edge of the poured copper (mm).
    pub margin: f64,
}

impl Default for StitchOptions {
    fn default() -> Self {
        Self { net: None, pitch: 5.0, via_diameter: 0.6, via_drill: 0.3, mode: StitchMode::Grid, margin: 0.25 }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StitchReport {
    /// Vias put on the board.
    pub added: usize,
    /// Places that passed the copper test but were left out because the DRC found something there.
    pub dropped: usize,
    /// The copper layers the vias join.
    pub layers: Vec<Side>,
}

/// Put stitching vias on `board` (see the module). Changes nothing when it fails.
pub fn stitch_vias(board: &mut Board, opt: &StitchOptions) -> Result<StitchReport, String> {
    let net = opt.net.clone().unwrap_or_else(|| "GND".to_string());
    if !(opt.pitch >= 1.0) {
        return Err("ビアの間隔は 1 mm 以上にしてください".into());
    }
    if !(opt.via_drill > 0.0 && opt.via_diameter > opt.via_drill) {
        return Err("ビアの外径は、穴の径より大きくしてください".into());
    }
    let mut layers: Vec<Side> = board.zones.iter().filter(|z| z.net == net).map(|z| z.side).collect();
    layers.sort_by_key(|s| match s {
        Side::Top => 0,
        Side::Inner(n) => *n as u16,
        Side::Bottom => u16::MAX,
    });
    layers.dedup();
    if layers.len() < 2 {
        return Err(format!(
            "{net} のベタが2つ以上の層にありません (いまは {} 層)。ビアの縫い付けは、表と裏など2つの層のベタをつなぐものです。先に「GND ベタ」(表面も埋める) を押してください",
            layers.len()
        ));
    }
    let copper = poured_copper(board, &net, &layers);
    if copper.is_empty() {
        return Err(format!("{net} のベタが重なる所がありません (片方の層が配線でふさがっています)"));
    }
    let r = opt.via_diameter / 2.0;
    let candidates = match opt.mode {
        StitchMode::Grid => grid_points(board, opt.pitch),
        StitchMode::Fence => fence_points(board, &copper, opt, r),
    };
    let mut placed: Vec<Point> = vec![];
    for p in candidates {
        if fits(board, &copper, p, r, opt) && placed.iter().all(|q| q.dist(p) >= opt.pitch * 0.7) {
            placed.push(p);
        }
    }
    if placed.is_empty() {
        return Err("ビアを置ける所が見つかりませんでした (ベタが狭いか、部品・配線でふさがっています)。間隔を狭くするか、ビアを小さくしてみてください".into());
    }
    let before = check(board).len();
    let base = board.vias.len();
    let mut work = board.clone();
    let mut dropped = 0;
    let via = |p: Point| Via { pos: p, diameter: opt.via_diameter, drill: opt.via_drill, net: net.clone() };
    work.vias.extend(placed.iter().map(|&p| via(p)));
    // the DRC sees things the copper test does not (hole to hole, drill to pad, ...): take out the vias near a finding
    for _ in 0..6 {
        let found = check(&work);
        if found.len() <= before {
            break;
        }
        let n = work.vias.len();
        let reach = opt.via_diameter + work.rules.min_clearance + 0.3;
        let keep: Vec<bool> = (0..n).map(|i| i < base || !found.iter().any(|f| f.at.dist(work.vias[i].pos) <= reach)).collect();
        let mut it = keep.iter();
        work.vias.retain(|_| *it.next().unwrap_or(&true));
        let gone = n - work.vias.len();
        if gone == 0 {
            break;
        }
        dropped += gone;
    }
    if check(&work).len() > before {
        return Err("縫い付けのビアを置くとルール違反が増えるので、置きませんでした".into());
    }
    let added = work.vias.len() - base;
    if added == 0 {
        return Err("ビアを置ける所は、どれもルール違反になりました。間隔やビアの大きさを変えてみてください".into());
    }
    *board = work;
    Ok(StitchReport { added, dropped, layers })
}

/// The copper of `net` that lies on every layer of `layers` (islands included: a via may join an island to the rest).
fn poured_copper(board: &Board, net: &str, layers: &[Side]) -> Vec<FillShape> {
    let conn = connectivity(board);
    let mut per_layer: Vec<Vec<FillShape>> = layers.iter().map(|_| vec![]).collect();
    for (side, n, shapes) in merged_fills(board, &conn).into_iter().chain(dropped_islands(board, &conn)) {
        if n == net
            && let Some(i) = layers.iter().position(|l| *l == side) {
                per_layer[i].extend(shapes);
            }
    }
    let mut it = per_layer.into_iter();
    let first = it.next().unwrap_or_default();
    it.fold(first, |acc, b| overlap_shapes(&acc, &b))
}

/// A square grid over the board, its first row and column half a pitch in from the corner.
fn grid_points(board: &Board, pitch: f64) -> Vec<Point> {
    let Some((lo, hi)) = board.bounds() else { return vec![] };
    let mut out = vec![];
    let mut y = lo.y + pitch / 2.0;
    while y < hi.y {
        let mut x = lo.x + pitch / 2.0;
        while x < hi.x {
            out.push(Point::new(x, y));
            x += pitch;
        }
        y += pitch;
    }
    out
}

/// Points along the board outline, `pitch` apart, moved inwards until the via fits (at most 3 mm in).
fn fence_points(board: &Board, copper: &[FillShape], opt: &StitchOptions, r: f64) -> Vec<Point> {
    let poly = &board.outline;
    if poly.len() < 3 {
        return vec![];
    }
    // inward normal: to the left of each edge for a counter-clockwise outline
    let ccw = crate::geom::signed_area(poly) > 0.0;
    let mut out = vec![];
    let mut carry = 0.0;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let len = a.dist(b);
        if len < 1e-9 {
            continue;
        }
        let (ux, uy) = ((b.x - a.x) / len, (b.y - a.y) / len);
        let (nx, ny) = if ccw { (-uy, ux) } else { (uy, -ux) };
        let mut t = carry;
        while t <= len {
            let on = Point::new(a.x + ux * t, a.y + uy * t);
            let mut d = board.rules.edge_clearance + r;
            while d <= 3.0 + r {
                let p = Point::new(on.x + nx * d, on.y + ny * d);
                if fits(board, copper, p, r, opt) {
                    out.push(p);
                    break;
                }
                d += 0.25;
            }
            t += opt.pitch;
        }
        carry = t - len;
    }
    out
}

/// The ring of a via at `p` lies in `copper`, `margin` off its edge, and keeps clear of pads, holes and other vias.
fn fits(board: &Board, copper: &[FillShape], p: Point, r: f64, opt: &StitchOptions) -> bool {
    let need = r + opt.margin;
    let inside = copper.iter().any(|s| crate::geom::inside_polygon(&s.outer, p) && !s.holes.iter().any(|h| crate::geom::inside_polygon(h, p)));
    if !inside {
        return false;
    }
    let edge_ok = |c: &[Point]| (0..c.len()).all(|i| point_seg(p, c[i], c[(i + 1) % c.len()]) >= need);
    if !copper.iter().all(|s| edge_ok(&s.outer) && s.holes.iter().all(|h| edge_ok(h))) {
        return false;
    }
    let gap = board.rules.min_clearance + 0.2;
    let pads_ok = board.pads.iter().all(|pad| {
        let (w, h) = pad.shape.size();
        p.dist(pad.pos) >= r + w.max(h) / 2.0 + gap
    });
    let vias_ok = board.vias.iter().all(|v| p.dist(v.pos) >= r + v.diameter / 2.0 + gap);
    let holes_ok = board.holes.iter().all(|h| p.dist(h.pos) >= r + h.drill.max(h.slot) / 2.0 + gap + 0.3);
    pads_ok && vias_ok && holes_ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drc;
    use crate::groundpour::{ground_pour, PourOptions};

    fn board() -> Board {
        let mut b = Board::new("t", 40.0, 30.0);
        for (x, y, net) in [(8.0, 8.0, "GND"), (30.0, 8.0, "GND"), (15.0, 15.0, "SIG"), (25.0, 15.0, "SIG")] {
            let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w: 1.0, h: 1.4 }, None, Side::Top);
            p.net = net.into();
            b.pads.push(p);
        }
        b
    }

    #[test]
    fn a_grid_of_vias_joins_the_two_pours_and_keeps_the_board_clean() {
        let mut b = board();
        ground_pour(&mut b, &PourOptions::default()).unwrap();
        crate::autoroute::autoroute(&mut b, &Default::default(), None);
        let before = b.vias.len();
        let r = stitch_vias(&mut b, &StitchOptions::default()).unwrap();
        assert!(r.added >= 10, "{r:?}");
        assert_eq!(r.layers, vec![Side::Top, Side::Bottom]);
        assert_eq!(b.vias.len(), before + r.added);
        assert!(b.vias[before..].iter().all(|v| v.net == "GND"));
        assert!(drc::check_final(&b).is_empty(), "{:?}", drc::check_final(&b));
        // no via sits on the signal track or near a pad
        for v in &b.vias[before..] {
            assert!(b.pads.iter().all(|p| p.pos.dist(v.pos) > 1.0), "{v:?}");
        }
    }

    #[test]
    fn a_fence_runs_along_the_edge() {
        let mut b = board();
        ground_pour(&mut b, &PourOptions::default()).unwrap();
        let base = b.vias.len();
        let r = stitch_vias(&mut b, &StitchOptions { mode: StitchMode::Fence, pitch: 3.0, ..Default::default() }).unwrap();
        // the outline is 140 mm round: about 140 / 3 places, less the corners and the pads
        assert!(r.added >= 30, "{r:?}");
        let (lo, hi) = b.bounds().unwrap();
        for v in &b.vias[base..] {
            let to_edge = (v.pos.x - lo.x).min(hi.x - v.pos.x).min(v.pos.y - lo.y).min(hi.y - v.pos.y);
            assert!(to_edge < 4.0, "a fence via far from the edge: {v:?}");
        }
        assert!(drc::check_final(&b).is_empty(), "{:?}", drc::check_final(&b));
    }

    #[test]
    fn one_poured_layer_is_refused_and_nothing_changes() {
        let mut b = board();
        ground_pour(&mut b, &PourOptions { top: false, ..Default::default() }).unwrap();
        let vias = b.vias.len();
        let e = stitch_vias(&mut b, &StitchOptions::default()).unwrap_err();
        assert!(e.contains("2つ以上"), "{e}");
        assert_eq!(b.vias.len(), vias);
        assert!(stitch_vias(&mut b, &StitchOptions { pitch: 0.2, ..Default::default() }).is_err());
    }

    #[test]
    fn stitching_twice_fills_only_the_gaps() {
        let mut b = board();
        ground_pour(&mut b, &PourOptions::default()).unwrap();
        stitch_vias(&mut b, &StitchOptions::default()).unwrap();
        let n = b.vias.len();
        // the same grid again: every place has its via already
        assert!(stitch_vias(&mut b, &StitchOptions::default()).is_err());
        assert_eq!(b.vias.len(), n);
    }
}
