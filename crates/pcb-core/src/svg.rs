//! A vector picture (SVG) of one side of the board, for an article, a report or a print: the board, the copper of the layer
//! (tracks, pads, vias, pours as the Gerber files have them), the silk and the holes. It is drawn from the Gerber and drill text
//! the fabrication package is made of, so it shows what would be made (the finish, the pours and their gaps included).
//! The bottom is shown as seen from below (mirrored), the way the board is turned over.

use crate::gerber_view::{parse_excellon_slots, parse_gerber, Aperture, Prim};
use crate::model::*;
use crate::{excellon, gerber};
use std::fmt::Write;

pub struct SvgOpts {
    /// The copper layer to show. The bottom is drawn mirrored.
    pub side: Side,
    pub silk: bool,
}

const BOARD: &str = "#1f5c3a";
const SILK: &str = "#f4f4f0";
const HOLE: &str = "#101418";

fn colour(side: Side) -> &'static str {
    match side {
        Side::Top => "#d9a441",
        Side::Bottom => "#5b9bd5",
        Side::Inner(_) => "#b07acb",
    }
}

fn n(v: f64) -> String {
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// One primitive as an element, drawn in `c`.
fn element(p: &Prim, c: &str) -> String {
    match p {
        Prim::Flash { ap, at, .. } => match *ap {
            Aperture::Circle(d) => format!("<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{c}\"/>", n(at.x), n(at.y), n(d / 2.0)),
            Aperture::Rect(w, h) => format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{c}\"/>", n(at.x - w / 2.0), n(at.y - h / 2.0), n(w), n(h)),
            Aperture::Oval(w, h) => format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" fill=\"{c}\"/>", n(at.x - w / 2.0), n(at.y - h / 2.0), n(w), n(h), n(w.min(h) / 2.0)),
        },
        Prim::Line { a, b, width, .. } => format!("<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{c}\" stroke-width=\"{}\" stroke-linecap=\"round\"/>", n(a.x), n(a.y), n(b.x), n(b.y), n(*width)),
        Prim::Region { pts, .. } => {
            let d: Vec<String> = pts.iter().map(|q| format!("{},{}", n(q.x), n(q.y))).collect();
            format!("<polygon points=\"{}\" fill=\"{c}\"/>", d.join(" "))
        }
    }
}

fn is_clear(p: &Prim) -> bool {
    match p {
        Prim::Flash { clear, .. } | Prim::Line { clear, .. } | Prim::Region { clear, .. } => *clear,
    }
}

/// The primitives of one Gerber layer as a group of colour `c`; what the file clears is cut out with masks (in the order drawn).
fn layer(prims: &[Prim], c: &str, id: &str, bounds: (Point, Point), defs: &mut String) -> String {
    let (lo, hi) = bounds;
    let mut body = String::new();
    let mut clears: Vec<String> = vec![];
    let mut k = 0;
    let mut cut = |body: &mut String, clears: &mut Vec<String>, defs: &mut String| {
        if clears.is_empty() {
            return;
        }
        k += 1;
        let mid = format!("{id}{k}");
        let _ = write!(
            defs,
            "<mask id=\"{mid}\" maskUnits=\"userSpaceOnUse\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#fff\"/>{}</mask>",
            n(lo.x), n(lo.y), n(hi.x - lo.x), n(hi.y - lo.y), n(lo.x), n(lo.y), n(hi.x - lo.x), n(hi.y - lo.y),
            clears.join("")
        );
        *body = format!("<g mask=\"url(#{mid})\">{body}</g>");
        clears.clear();
    };
    for p in prims {
        if is_clear(p) {
            clears.push(element(p, "#000"));
        } else {
            cut(&mut body, &mut clears, defs);
            body.push_str(&element(p, c));
        }
    }
    cut(&mut body, &mut clears, defs);
    body
}

/// The SVG text of one side of the board.
pub fn board_svg(board: &Board, opts: &SvgOpts) -> Result<String, String> {
    let (lo, hi) = board.bounds().ok_or("基板に外形も部品もないので、図にできません。先に外形を描いてください")?;
    let margin = 1.0;
    let (lo, hi) = (Point::new(lo.x - margin, lo.y - margin), Point::new(hi.x + margin, hi.y + margin));
    let (w, h) = (hi.x - lo.x, hi.y - lo.y);
    let mirror = opts.side == Side::Bottom;
    let silk_side = if mirror { Side::Bottom } else { Side::Top };

    // y points up on the board and down in SVG: the group flips it (and x, for the bottom). The view box is the flipped box.
    let (vx, vy) = (if mirror { -hi.x } else { lo.x }, -hi.y);
    let sx = if mirror { -1 } else { 1 };
    let mut defs = String::new();
    let mut inner = String::new();
    if board.outline.len() >= 3 {
        inner.push_str(&element(&Prim::Region { pts: board.outline.clone(), clear: false }, BOARD));
    }
    let copper = parse_gerber(&gerber::copper(board, opts.side))?;
    inner.push_str(&layer(&copper, colour(opts.side), "c", (lo, hi), &mut defs));
    if opts.silk && matches!(opts.side, Side::Top | Side::Bottom) {
        let silk = parse_gerber(&gerber::silk(board, silk_side))?;
        inner.push_str(&layer(&silk, SILK, "s", (lo, hi), &mut defs));
    }
    for text in [excellon::plated(board), excellon::non_plated(board)] {
        for (d, a, b) in parse_excellon_slots(&text)? {
            match b {
                None => inner.push_str(&element(&Prim::Flash { ap: Aperture::Circle(d), at: a, clear: false }, HOLE)),
                Some(b) => inner.push_str(&element(&Prim::Line { a, b, width: d, clear: false }, HOLE)),
            }
        }
    }
    let mut out = String::new();
    let _ = write!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}mm\" height=\"{}mm\" viewBox=\"{} {} {} {}\">",
        n(w), n(h), n(vx), n(vy), n(w), n(h)
    );
    let _ = write!(out, "<title>{} ({})</title>", board.name.replace('&', "&amp;").replace('<', "&lt;"), if mirror { "裏面を下から見た図" } else { "表面を上から見た図" });
    if !defs.is_empty() {
        let _ = write!(out, "<defs>{defs}</defs>");
    }
    let _ = write!(out, "<g transform=\"scale({sx},-1)\">{inner}</g></svg>\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Board {
        let mut b = Board::new("demo", 30.0, 20.0);
        b.pads.push(Pad::new(Point::new(5.0, 5.0), PadShape::Circle { d: 1.6 }, Some(0.8), Side::Top));
        b.pads.push(Pad::new(Point::new(15.0, 5.0), PadShape::Rect { w: 1.0, h: 1.5 }, None, Side::Top));
        b.tracks.push(Track { side: Side::Top, a: Point::new(5.0, 5.0), b: Point::new(15.0, 5.0), width: 0.25 });
        b.holes.push(Hole::new(Point::new(2.0, 2.0), 3.2));
        b
    }

    #[test]
    fn the_top_is_drawn_with_its_copper_and_holes() {
        let s = board_svg(&sample(), &SvgOpts { side: Side::Top, silk: true }).unwrap();
        assert!(s.starts_with("<svg ") && s.trim_end().ends_with("</svg>"));
        assert!(s.contains("mm\""), "the size is in millimetres");
        assert!(s.contains("stroke-width=\"0.25\""), "the track");
        assert!(s.contains(&format!("fill=\"{}\"", HOLE)), "the holes");
        assert!(s.contains("scale(1,-1)"), "y is flipped to SVG's");
        assert!(!s.contains("scale(-1,-1)"));
    }

    #[test]
    fn what_a_layer_clears_is_cut_out_with_a_mask() {
        let pour = Prim::Region { pts: vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(10.0, 10.0), Point::new(0.0, 10.0)], clear: false };
        let gap = Prim::Line { a: Point::new(5.0, 0.0), b: Point::new(5.0, 10.0), width: 1.0, clear: true };
        let later = Prim::Flash { ap: Aperture::Circle(2.0), at: Point::new(5.0, 5.0), clear: false };
        let mut defs = String::new();
        let g = layer(&[pour, gap, later], "#abc", "t", (Point::new(0.0, 0.0), Point::new(10.0, 10.0)), &mut defs);
        assert_eq!(defs.matches("<mask").count(), 1);
        assert!(g.starts_with("<g mask=\"url(#t1)\"><polygon"), "{g}");
        assert!(g.ends_with("<circle cx=\"5\" cy=\"5\" r=\"1\" fill=\"#abc\"/>"), "what is drawn after the cut is not cut: {g}");
    }

    #[test]
    fn the_bottom_is_mirrored() {
        let s = board_svg(&sample(), &SvgOpts { side: Side::Bottom, silk: false }).unwrap();
        assert!(s.contains("scale(-1,-1)") && s.contains("裏面"));
    }
}
