//! The copper and the silkscreen of the board as thin solids for its STEP (E3b): what the 3D view paints on the board's faces, for a
//! case's CAD to show (a STEP has no pictures). Each face's copper is split where the solder mask opens: the copper under the mask in a
//! light green, the exposed pads in gold; the silkscreen in white over it. The shapes are read back from the Gerber the board writes
//! (`gerber::copper`, `mask`, `silk`), so they are what the factory makes, pours and their clearances included.

use crate::gerber_view::{parse_gerber, Aperture, Prim};
use crate::model::*;
use crate::model3d::BOARD_THICKNESS;
use crate::step_out::{prism, Axes, Solid};
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;

type Path = Vec<[f64; 2]>;
/// Shapes as the overlay engine gives them: each an outer contour and its holes.
type Shapes = Vec<Vec<Path>>;

/// How thick the copper is drawn (35 µm, 1 oz) and the silkscreen on it (mm).
pub const COPPER: f64 = 0.035;
pub const SILK: f64 = 0.015;
const UNDER_MASK: [u8; 3] = [70, 150, 90];
const PAD: [u8; 3] = [212, 175, 55];
const SILK_COLOR: [u8; 3] = [240, 240, 240];
/// Points round a circle.
const ROUND: usize = 16;

fn circle(c: Point, r: f64, n: usize) -> Path {
    (0..n).map(|k| { let a = std::f64::consts::TAU * k as f64 / n as f64; [c.x + r * a.cos(), c.y + r * a.sin()] }).collect()
}

/// The stadium round the segment `a`..`b`, `w` wide (a stroke with a round pen), counter-clockwise.
fn stadium(a: Point, b: Point, w: f64) -> Path {
    let r = w / 2.0;
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-9 {
        return circle(a, r, ROUND);
    }
    let base = dy.atan2(dx) - std::f64::consts::FRAC_PI_2;
    let half = ROUND / 2;
    let mut out = vec![];
    // round the end at b, then the end at a
    for (c, from) in [(b, base), (a, base + std::f64::consts::PI)] {
        for k in 0..=half {
            let t = from + std::f64::consts::PI * k as f64 / half as f64;
            out.push([c.x + r * t.cos(), c.y + r * t.sin()]);
        }
    }
    out
}

/// A rectangle `w` x `h` round `c` with corners rounded by `r`, counter-clockwise.
fn rounded(c: Point, w: f64, h: f64, r: f64) -> Path {
    let r = r.clamp(0.0, w.min(h) / 2.0);
    if r < 1e-9 {
        return vec![[c.x - w / 2.0, c.y - h / 2.0], [c.x + w / 2.0, c.y - h / 2.0], [c.x + w / 2.0, c.y + h / 2.0], [c.x - w / 2.0, c.y + h / 2.0]];
    }
    let (ix, iy) = (w / 2.0 - r, h / 2.0 - r);
    let mut out = vec![];
    for (q, (sx, sy)) in [(1.0, -1.0), (1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0)].iter().enumerate() {
        let start = -std::f64::consts::FRAC_PI_2 + q as f64 * std::f64::consts::FRAC_PI_2;
        for k in 0..=4 {
            let t = start + std::f64::consts::FRAC_PI_2 * k as f64 / 4.0;
            out.push([c.x + sx * ix + r * t.cos(), c.y + sy * iy + r * t.sin()]);
        }
    }
    out
}

/// A Gerber primitive as a polygon.
fn polygon(p: &Prim) -> Path {
    match p {
        Prim::Flash { ap, at, .. } => match *ap {
            Aperture::Circle(d) => circle(*at, d / 2.0, ROUND),
            Aperture::Rect(w, h) => rounded(*at, w, h, 0.0),
            Aperture::RoundRect(w, h, r) => rounded(*at, w, h, r),
            Aperture::Oval(w, h) => {
                let d = w.min(h);
                let half = (w.max(h) - d) / 2.0;
                let (ax, ay) = if w >= h { (half, 0.0) } else { (0.0, half) };
                stadium(Point::new(at.x - ax, at.y - ay), Point::new(at.x + ax, at.y + ay), d)
            }
        },
        Prim::Line { a, b, width, .. } => stadium(*a, *b, *width),
        Prim::Region { pts, .. } => pts.iter().map(|p| [p.x, p.y]).collect(),
    }
}

/// Counter-clockwise (the overlay engine unions one winding).
fn ccw(mut p: Path) -> Path {
    let area: f64 = (0..p.len()).map(|i| { let (a, b) = (p[i], p[(i + 1) % p.len()]); a[0] * b[1] - b[0] * a[1] }).sum();
    if area < 0.0 {
        p.reverse();
    }
    p
}

/// The paths of shapes: outer contours counter-clockwise, holes clockwise.
fn paths(shapes: &Shapes) -> Vec<Path> {
    let mut out = vec![];
    for s in shapes {
        for (k, c) in s.iter().enumerate() {
            let mut c = ccw(c.clone());
            if k > 0 {
                c.reverse();
            }
            out.push(c);
        }
    }
    out
}

/// What a layer's primitives cover, in drawing order: dark ones added, clear ones taken away (runs of one polarity at a time).
fn layer_shapes(prims: &[Prim]) -> Shapes {
    let mut have: Shapes = vec![];
    let mut run: Vec<Path> = vec![];
    let mut clear_run = false;
    let flush = |have: &mut Shapes, run: &mut Vec<Path>, clear: bool| {
        if run.is_empty() {
            return;
        }
        let batch = std::mem::take(run);
        *have = if clear {
            paths(have).overlay(&batch, OverlayRule::Difference, FillRule::NonZero)
        } else {
            let mut all = paths(have);
            all.extend(batch);
            let none: Vec<Path> = vec![];
            all.overlay(&none, OverlayRule::Union, FillRule::NonZero)
        };
    };
    for p in prims {
        let clear = match p {
            Prim::Flash { clear, .. } | Prim::Line { clear, .. } | Prim::Region { clear, .. } => *clear,
        };
        if clear != clear_run {
            flush(&mut have, &mut run, clear_run);
            clear_run = clear;
        }
        let poly = polygon(p);
        if poly.len() >= 3 {
            run.push(ccw(poly));
        }
    }
    flush(&mut have, &mut run, clear_run);
    have
}

fn op(a: &Shapes, b: &Shapes, rule: OverlayRule) -> Shapes {
    if a.is_empty() {
        return vec![];
    }
    paths(a).overlay(&paths(b), rule, FillRule::NonZero)
}

/// Points closer than this (mm) are one: the overlay engine leaves pairs 0.1 µm apart, and a CAD takes an edge that short for none.
const MERGE: f64 = 1e-4;

/// The contour without its points that come within `MERGE` of the one kept before them.
fn clean(c: &Path) -> Vec<Point> {
    let mut out: Vec<Point> = vec![];
    for p in c {
        let q = Point::new(p[0], p[1]);
        if out.last().is_none_or(|l| l.dist(q) > MERGE) {
            out.push(q);
        }
    }
    while out.len() > 1 && out[0].dist(out[out.len() - 1]) <= MERGE {
        out.pop();
    }
    out
}

/// Thin solids over shapes, between `z0` and `z1`.
fn slabs(name: &str, color: [u8; 3], shapes: &Shapes, z0: f64, z1: f64) -> Vec<Solid> {
    shapes
        .iter()
        .filter_map(|s| {
            let outer = clean(s.first()?);
            if outer.len() < 3 {
                return None;
            }
            let holes: Vec<Vec<Point>> = s[1..].iter().map(clean).filter(|h| h.len() >= 3).collect();
            Some(prism(name, color, &outer, &holes, z0, z1))
        })
        .collect()
}

/// The copper and silkscreen products of the board's STEP: per face, the copper under the mask, the exposed pads and the silkscreen,
/// each a product (`<board>_COPPER_TOP` ...) of thin solids in the board's frame. Faces with nothing on them give nothing.
pub fn layers(board: &Board) -> Vec<(String, Vec<Solid>, Axes)> {
    const IDENTITY: Axes = [[0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]];
    let name = if board.name.is_empty() { "board".to_string() } else { board.name.clone() };
    let read = |text: String| parse_gerber(&text).map(|p| layer_shapes(&p)).unwrap_or_default();
    let mut out = vec![];
    for (side, tag) in [(Side::Top, "TOP"), (Side::Bottom, "BOTTOM")] {
        let (copper, mask, silk) = (read(crate::gerber::copper(board, side)), read(crate::gerber::mask(board, side)), read(crate::gerber::silk(board, side)));
        // up from the top face, down from the bottom one
        let (zc, zs) = if side == Side::Top { ((BOARD_THICKNESS, BOARD_THICKNESS + COPPER), (BOARD_THICKNESS + COPPER, BOARD_THICKNESS + COPPER + SILK)) } else { ((-COPPER, 0.0), (-COPPER - SILK, -COPPER)) };
        let under = op(&copper, &mask, OverlayRule::Difference);
        let open = op(&copper, &mask, OverlayRule::Intersect);
        let mut solids = slabs(&format!("{name}_COPPER_{tag}"), UNDER_MASK, &under, zc.0, zc.1);
        solids.extend(slabs(&format!("{name}_PADS_{tag}"), PAD, &open, zc.0, zc.1));
        if !solids.is_empty() {
            out.push((format!("{name}_COPPER_{tag}"), solids, IDENTITY));
        }
        // the silkscreen is not printed over the openings of the mask
        let silk = op(&silk, &mask, OverlayRule::Difference);
        let s = slabs(&format!("{name}_SILK_{tag}"), SILK_COLOR, &silk, zs.0, zs.1);
        if !s.is_empty() {
            out.push((format!("{name}_SILK_{tag}"), s, IDENTITY));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A dark square with a clear hole through it, a line beside it, and a clear spot that misses: the square with its hole, and the line.
    #[test]
    fn dark_and_clear_primitives_make_shapes_with_holes() {
        let prims = vec![
            Prim::Flash { ap: Aperture::Rect(4.0, 4.0), at: Point::new(0.0, 0.0), clear: false },
            Prim::Flash { ap: Aperture::Circle(1.0), at: Point::new(0.0, 0.0), clear: true },
            Prim::Line { a: Point::new(5.0, 0.0), b: Point::new(8.0, 0.0), width: 0.5, clear: false },
        ];
        let s = layer_shapes(&prims);
        assert_eq!(s.len(), 2, "{s:?}");
        assert!(s.iter().any(|sh| sh.len() == 2), "the square keeps its hole");
        let area = |p: &Path| (0..p.len()).map(|i| { let (a, b) = (p[i], p[(i + 1) % p.len()]); a[0] * b[1] - b[0] * a[1] }).sum::<f64>().abs() / 2.0;
        let total: f64 = s.iter().map(|sh| area(&sh[0]) - sh[1..].iter().map(area).sum::<f64>()).sum();
        let want = 16.0 - std::f64::consts::PI * 0.25 + (3.0 * 0.5 + std::f64::consts::PI * 0.0625);
        assert!((total - want).abs() < 0.05, "{total} vs {want}");
        // points 0.1 µm apart are one
        assert_eq!(clean(&vec![[0.0, 0.0], [1.0, 0.0], [1.0 + 1e-7, 1e-7], [1.0, 1.0], [1e-8, 0.0]]).len(), 3);
    }
}

