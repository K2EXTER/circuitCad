//! Dimensions kept on the board: a measured length drawn the way a mechanical drawing does it (two extension lines from the
//! measured points, a dimension line with an arrow at each end, the length written beside it in mm). They are a drawing layer
//! of their own: shown in the editor and written to the DXF (layer DIMENSIONS) and the SVG picture, never to the Gerber files,
//! so nothing of them is made on the board.

use crate::model::*;
use serde::{Deserialize, Serialize};

/// Which length is measured between the two points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DimDir {
    /// The straight distance, along the line between them.
    #[default]
    Aligned,
    /// Only the x part (a horizontal dimension line).
    Horizontal,
    /// Only the y part (a vertical one).
    Vertical,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dimension {
    pub a: Point,
    pub b: Point,
    /// How far (mm) the dimension line stands off point `a`, to the left of the measuring direction (negative: to the right).
    pub offset: f64,
    #[serde(default)]
    pub dir: DimDir,
    /// Height of the written length (mm).
    #[serde(default = "default_text_height")]
    pub text_height: f64,
    /// Made by [`add_outline_dimensions`] (pressing it again replaces these).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub outline: bool,
}

fn default_text_height() -> f64 {
    1.2
}

/// Gap between a measured point and the start of its extension line, and how far the extension line runs past the dimension line.
const GAP: f64 = 0.5;
const PAST: f64 = 1.0;
const ARROW: f64 = 1.0;

/// What a dimension is drawn of: the straight lines (extension lines, dimension line, arrow heads) and the text.
#[derive(Clone, Debug, PartialEq)]
pub struct DimDrawing {
    pub lines: Vec<[Point; 2]>,
    pub text: String,
    /// Centre of the text and its angle (degrees, counter-clockwise from +x; always between -90 and 90 so it reads upright).
    pub text_at: Point,
    pub angle: f64,
}

impl Dimension {
    pub fn new(a: Point, b: Point, offset: f64, dir: DimDir) -> Self {
        Self { a, b, offset, dir, text_height: default_text_height(), outline: false }
    }

    /// The unit vector the length is measured along.
    fn along(&self) -> (f64, f64) {
        match self.dir {
            DimDir::Horizontal => (1.0, 0.0),
            DimDir::Vertical => (0.0, 1.0),
            DimDir::Aligned => {
                let (dx, dy) = (self.b.x - self.a.x, self.b.y - self.a.y);
                let l = dx.hypot(dy);
                if l < 1e-9 { (1.0, 0.0) } else { (dx / l, dy / l) }
            }
        }
    }

    /// The measured length (mm).
    pub fn length(&self) -> f64 {
        let (ux, uy) = self.along();
        ((self.b.x - self.a.x) * ux + (self.b.y - self.a.y) * uy).abs()
    }

    pub fn drawing(&self) -> DimDrawing {
        let (ux, uy) = self.along();
        let (nx, ny) = (-uy, ux);
        // the dimension line: through a + n * offset, along u
        let base = Point::new(self.a.x + nx * self.offset, self.a.y + ny * self.offset);
        let t = (self.b.x - self.a.x) * ux + (self.b.y - self.a.y) * uy;
        let (pa, pb) = (base, Point::new(base.x + ux * t, base.y + uy * t));
        let mut lines = vec![[pa, pb]];
        // extension lines from each measured point to just past the dimension line
        for (from, to) in [(self.a, pa), (self.b, pb)] {
            let (dx, dy) = (to.x - from.x, to.y - from.y);
            let l = dx.hypot(dy);
            if l > GAP {
                let (ex, ey) = (dx / l, dy / l);
                lines.push([Point::new(from.x + ex * GAP, from.y + ey * GAP), Point::new(to.x + ex * PAST, to.y + ey * PAST)]);
            }
        }
        // arrow heads pointing out to the extension lines (inwards when the length is too short to hold them)
        let sign = if t >= 0.0 { 1.0 } else { -1.0 };
        let inside = t.abs() >= 2.5 * ARROW;
        for (tip, out) in [(pa, -sign), (pb, sign)] {
            let back = if inside { -out } else { out };
            for turn in [0.35f64, -0.35] {
                let (c, s) = (turn.cos(), turn.sin());
                let (bx, by) = (ux * back, uy * back);
                let (rx, ry) = (bx * c - by * s, bx * s + by * c);
                lines.push([tip, Point::new(tip.x + rx * ARROW, tip.y + ry * ARROW)]);
            }
        }
        // the text sits beside the middle of the dimension line, on the side away from the measured points, upright
        let mut angle = uy.atan2(ux).to_degrees();
        if angle > 90.0 + 1e-6 {
            angle -= 180.0;
        } else if angle <= -90.0 + 1e-6 {
            angle += 180.0;
        }
        let away = if self.offset >= 0.0 { 1.0 } else { -1.0 };
        let d = 0.4 + self.text_height / 2.0;
        let mid = Point::new((pa.x + pb.x) / 2.0 + nx * away * d, (pa.y + pb.y) / 2.0 + ny * away * d);
        DimDrawing { lines, text: format!("{:.2}", self.length()), text_at: mid, angle }
    }

    /// Everything as straight strokes, the text drawn with the stroke font (for the SVG and anything without a font).
    pub fn strokes(&self) -> Vec<[Point; 2]> {
        let d = self.drawing();
        let (c, s) = (d.angle.to_radians().cos(), d.angle.to_radians().sin());
        let place = |p: Point| Point::new(d.text_at.x + p.x * c - p.y * s, d.text_at.y + p.x * s + p.y * c);
        let mut out = d.lines.clone();
        out.extend(crate::strokefont::text_strokes(&d.text, self.text_height, false).into_iter().map(|[a, b]| [place(a), place(b)]));
        out
    }

    /// The box round everything drawn.
    pub fn bounds(&self) -> (Point, Point) {
        let mut lo = Point::new(f64::MAX, f64::MAX);
        let mut hi = Point::new(f64::MIN, f64::MIN);
        for [a, b] in self.strokes() {
            for p in [a, b] {
                lo = Point::new(lo.x.min(p.x), lo.y.min(p.y));
                hi = Point::new(hi.x.max(p.x), hi.y.max(p.y));
            }
        }
        (lo, hi)
    }
}

/// The width and the height of the board's outline (its bounding box), below and to the left of it, `gap` mm off.
pub fn outline_dimensions(board: &Board, gap: f64) -> Option<[Dimension; 2]> {
    let (lo, hi) = board.bounds().filter(|_| board.outline.len() >= 3)?;
    // the bottom left corner to the bottom right one, measured to the right: left of that is up, so the line goes below with -gap
    let width = Dimension::new(Point::new(lo.x, lo.y), Point::new(hi.x, lo.y), -gap, DimDir::Horizontal);
    // the bottom left corner up to the top left one, measured upwards: left of that is -x, so +gap puts it to the left
    let height = Dimension::new(Point::new(lo.x, lo.y), Point::new(lo.x, hi.y), gap, DimDir::Vertical);
    Some([width, height])
}

/// Put the width and height of the outline on the board, taking off dimensions that measured the outline before (so it can be
/// pressed again after the outline changed). Returns the two lengths.
pub fn add_outline_dimensions(board: &mut Board) -> Result<(f64, f64), String> {
    let [mut w, mut h] = outline_dimensions(board, 4.0).ok_or("先に基板の外形を決めてください")?;
    (w.outline, h.outline) = (true, true);
    board.dimensions.retain(|d| !d.outline);
    let out = (w.length(), h.length());
    board.dimensions.push(w);
    board.dimensions.push(h);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_horizontal_dimension_measures_the_x_part_and_reads_upright() {
        let d = Dimension::new(Point::new(0.0, 0.0), Point::new(30.0, 5.0), -4.0, DimDir::Horizontal);
        assert!((d.length() - 30.0).abs() < 1e-9);
        let g = d.drawing();
        assert_eq!(g.text, "30.00");
        assert!(g.angle.abs() < 1e-9);
        // the dimension line runs at y = -4 from x 0 to 30
        assert_eq!(g.lines[0], [Point::new(0.0, -4.0), Point::new(30.0, -4.0)]);
        // the text is below the line (away from the measured points)
        assert!(g.text_at.y < -4.0 && (g.text_at.x - 15.0).abs() < 1e-9, "{:?}", g.text_at);
        // two extension lines and four arrow strokes
        assert_eq!(g.lines.len(), 1 + 2 + 4);
        // the extension line of b goes from near b down past the line
        let ext = g.lines[2];
        assert!((ext[0].x - 30.0).abs() < 1e-9 && ext[0].y < 5.0 && ext[1].y < -4.0, "{ext:?}");
    }

    #[test]
    fn an_aligned_dimension_measures_the_straight_distance_and_turns_its_text() {
        let d = Dimension::new(Point::new(0.0, 0.0), Point::new(-3.0, -4.0), 2.0, DimDir::Aligned);
        assert!((d.length() - 5.0).abs() < 1e-9);
        let g = d.drawing();
        assert!(g.angle > -90.0 && g.angle <= 90.0, "{}", g.angle);
        assert!(!d.strokes().is_empty());
        let (lo, hi) = d.bounds();
        // measured towards (-0.6, -0.8): left of that is (0.8, -0.6), so the line stands 1.6 mm to the right of a
        assert!((g.lines[0][0].x - 1.6).abs() < 1e-9 && (g.lines[0][0].y + 1.2).abs() < 1e-9, "{:?}", g.lines[0]);
        assert!(lo.x < -1.0 && hi.x > 1.6, "{lo:?} {hi:?}");
    }

    #[test]
    fn the_outline_gets_its_width_and_height_once() {
        let mut b = Board::new("t", 40.0, 30.0);
        assert_eq!(add_outline_dimensions(&mut b), Ok((40.0, 30.0)));
        assert_eq!(add_outline_dimensions(&mut b), Ok((40.0, 30.0)));
        assert_eq!(b.dimensions.len(), 2, "pressing again replaces them");
        let (lo, hi) = b.bounds().unwrap();
        // the width below the board, the height left of it
        let [w, h] = [&b.dimensions[0], &b.dimensions[1]];
        assert!(w.drawing().lines[0][0].y < lo.y && h.drawing().lines[0][0].x < lo.x);
        assert!(w.bounds().1.y < hi.y);
        // a dimension of its own stays
        b.dimensions.push(Dimension::new(Point::new(1.0, 1.0), Point::new(5.0, 1.0), 2.0, DimDir::Aligned));
        add_outline_dimensions(&mut b).unwrap();
        assert_eq!(b.dimensions.len(), 3);
    }
}
