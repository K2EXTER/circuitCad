//! The full-size print to try the parts on (candidate X4): one side of the board at 1:1 on white paper (the outline, the pads
//! and holes of that side, the references), with a 50 mm scale ruled in millimetres to check that the printer did not shrink the
//! page. The real parts are laid on the paper (or their leads pushed through it into a piece of foam) to see that the legs land
//! on the pads before the board is ordered: the beginner's most common board mistake is a footprint that does not fit the part.
//! The SVG is sized in millimetres, so a print at 100 % (no "fit to page") is full size. The bottom is drawn as seen from below.

use crate::model::*;
use std::fmt::Write;

/// The scale's length (mm).
pub const RULER_MM: f64 = 50.0;
const MARGIN: f64 = 10.0;
/// The room below the drawing for the scale; the steps' font size and line pitch.
const FOOT: f64 = 16.0;
const STEP_FONT: f64 = 2.6;
const STEP_PITCH: f64 = 3.8;
/// The page is at least this wide, so the steps fit.
const MIN_W: f64 = 150.0;

fn n(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The steps printed on the page.
pub const STEPS: [&str; 3] = [
    "1. 印刷の設定を「実際のサイズ」(100 %、ページに合わせない) にして印刷し、下の目盛りの 50 mm を定規で測ります (ずれていたら設定を見直す)。",
    "2. 実物の部品を紙の上に置き、足が黒いパッドに乗るか見ます。足のある部品は、紙を発泡スチロールなどに載せ、穴の位置に足を差します。",
    "3. 足の間隔・ピンの並び・向き (1 番ピン) が合わない部品は、フットプリントを直してから発注します。",
];

/// `text` cut into lines no wider than `width` mm at font size `font` (a full-width character one font size wide, others
/// about half), between characters (Japanese has no spaces to break at).
fn wrap(text: &str, width: f64, font: f64) -> Vec<String> {
    let mut out = vec![];
    let (mut line, mut used) = (String::new(), 0.0);
    for c in text.chars() {
        let cw = if c.is_ascii() { 0.55 } else { 1.0 } * font;
        if used + cw > width && !line.is_empty() {
            out.push(std::mem::take(&mut line));
            used = 0.0;
        }
        line.push(c);
        used += cw;
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

/// The print of side `side` (Top or Bottom; the through holes are on both).
pub fn print_check_svg(board: &Board, side: Side) -> Result<String, String> {
    let (lo, hi) = board.bounds().ok_or("基板に外形がないので、原寸の図を作れません。先に外形を決めてください")?;
    if board.pads.is_empty() {
        return Err("基板に部品がないので、当てて確かめるものがありません。先に回路図から部品を取り込んでください".into());
    }
    let mirror = side == Side::Bottom;
    let (bw, bh) = (hi.x - lo.x, hi.y - lo.y);
    let w = (bw + 2.0 * MARGIN).max(MIN_W).max(RULER_MM + 2.0 * MARGIN);
    let lines: Vec<String> = STEPS.iter().flat_map(|t| wrap(t, w - 2.0 * MARGIN, STEP_FONT)).collect();
    let head = 12.0 + lines.len() as f64 * STEP_PITCH;
    let h = bh + head + FOOT + 2.0 * MARGIN;
    let x0 = (w - bw) / 2.0;
    let y0 = MARGIN + head;
    // board (x, y up) to page (x, y down); the bottom is mirrored left to right
    let to = |p: Point| (if mirror { x0 + (hi.x - p.x) } else { x0 + (p.x - lo.x) }, y0 + (hi.y - p.y));
    let mut s = String::new();
    let _ = write!(s, "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}mm\" height=\"{}mm\" viewBox=\"0 0 {} {}\" font-family=\"sans-serif\">", n(w), n(h), n(w), n(h));
    let what = if mirror { "裏面 (下から見た図)" } else { "表面" };
    let _ = write!(s, "<title>{} 原寸の確かめ ({what})</title>", esc(&board.name));
    let _ = write!(s, "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#fff\"/>", n(w), n(h));
    let title = format!("原寸の確かめ: {}{what}", if board.name.is_empty() { String::new() } else { format!("{} の", board.name) });
    let _ = write!(s, "<text x=\"{}\" y=\"{}\" font-size=\"5\" font-weight=\"bold\">{}</text>", n(MARGIN), n(MARGIN + 4.0), esc(&title));
    for (k, line) in lines.iter().enumerate() {
        let _ = write!(s, "<text x=\"{}\" y=\"{}\" font-size=\"{STEP_FONT}\">{}</text>", n(MARGIN), n(MARGIN + 10.0 + k as f64 * STEP_PITCH), esc(line));
    }
    if board.outline.len() >= 3 {
        let pts: Vec<String> = board.outline.iter().map(|p| { let (a, b) = to(*p); format!("{},{}", n(a), n(b)) }).collect();
        let _ = write!(s, "<polygon points=\"{}\" fill=\"none\" stroke=\"#888\" stroke-width=\"0.2\"/>", pts.join(" "));
    }
    for pad in board.pads.iter().filter(|p| p.drill.is_some() || p.side == side) {
        let (cx, cy) = to(pad.pos);
        match pad.shape {
            PadShape::Circle { d } => {
                let _ = write!(s, "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"#000\"/>", n(cx), n(cy), n(d / 2.0));
            }
            PadShape::Rect { w: pw, h: ph } => {
                let _ = write!(s, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#000\"/>", n(cx - pw / 2.0), n(cy - ph / 2.0), n(pw), n(ph));
            }
            PadShape::Oval { w: pw, h: ph } | PadShape::RoundRect { w: pw, h: ph, .. } => {
                let r = pad.shape.corner_radius();
                let _ = write!(s, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" fill=\"#000\"/>", n(cx - pw / 2.0), n(cy - ph / 2.0), n(pw), n(ph), n(r));
            }
        }
        if let Some(d) = pad.drill {
            let (sw, sh) = if pad.slot > 0.0 { if pad.slot_v { (d, pad.slot) } else { (pad.slot, d) } } else { (d, d) };
            let _ = write!(s, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" fill=\"#fff\"/>", n(cx - sw / 2.0), n(cy - sh / 2.0), n(sw), n(sh), n(d / 2.0));
        }
    }
    for h in &board.holes {
        let (cx, cy) = to(h.pos);
        let _ = write!(s, "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"none\" stroke=\"#000\" stroke-width=\"0.15\"/>", n(cx), n(cy), n(h.drill / 2.0));
    }
    // the references, beside each part's pads (above them), so they do not cover a pad
    let mut refs: Vec<&str> = board.pads.iter().filter(|p| !p.refdes.is_empty() && (p.drill.is_some() || p.side == side)).map(|p| p.refdes.as_str()).collect();
    refs.sort_unstable();
    refs.dedup();
    for r in refs {
        let pads: Vec<&Pad> = board.pads.iter().filter(|q| q.refdes == r).collect();
        let Some(top) = pads.iter().map(|q| q.pos.y + q.shape.size().1 / 2.0).reduce(f64::max) else { continue };
        let cx = pads.iter().map(|q| q.pos.x).sum::<f64>() / pads.len() as f64;
        let (x, y) = to(Point::new(cx, top));
        let _ = write!(s, "<text x=\"{}\" y=\"{}\" font-size=\"1.4\" fill=\"#c03030\" text-anchor=\"middle\">{}</text>", n(x), n(y - 0.4), esc(r));
    }
    // the scale: 50 mm, a tick each millimetre, longer each 5 and 10, numbered each 10
    let (rx, ry) = ((w - RULER_MM) / 2.0, h - MARGIN - 6.0);
    let _ = write!(s, "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#000\" stroke-width=\"0.15\"/>", n(rx), n(ry), n(rx + RULER_MM), n(ry));
    for k in 0..=RULER_MM as usize {
        let len = if k % 10 == 0 { 3.0 } else if k % 5 == 0 { 2.0 } else { 1.0 };
        let x = rx + k as f64;
        let _ = write!(s, "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#000\" stroke-width=\"0.1\"/>", n(x), n(ry), n(x), n(ry - len));
        if k % 10 == 0 {
            let _ = write!(s, "<text x=\"{}\" y=\"{}\" font-size=\"2\" text-anchor=\"middle\">{k}</text>", n(x), n(ry + 3.0));
        }
    }
    let _ = write!(s, "<text x=\"{}\" y=\"{}\" font-size=\"2.4\" text-anchor=\"middle\">{}</text>", n(w / 2.0), n(ry + 6.0), esc("この目盛りが定規で 50 mm なら原寸です"));
    s.push_str("</svg>\n");
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> Board {
        let mut b = Board::new("", 30.0, 20.0);
        let mut thru = Pad::new(Point::new(5.0, 10.0), PadShape::Circle { d: 1.8 }, Some(1.0), Side::Top);
        thru.refdes = "J1".into();
        let mut smd = Pad::new(Point::new(20.0, 10.0), PadShape::Rect { w: 1.2, h: 1.4 }, None, Side::Top);
        smd.refdes = "R1".into();
        let mut under = Pad::new(Point::new(25.0, 5.0), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Bottom);
        under.refdes = "C1".into();
        b.pads = vec![thru, smd, under];
        b
    }

    /// The page is in millimetres with its view box in the same units (a print at 100 % is full size); a pad is drawn at its own
    /// size; the top shows the top pads and the through holes, the bottom the bottom pads and the holes, mirrored.
    #[test]
    fn the_print_is_full_size_with_the_pads_of_its_side() {
        let b = board();
        let top = print_check_svg(&b, Side::Top).unwrap();
        let w: f64 = top.split("width=\"").nth(1).unwrap().split("mm").next().unwrap().parse().unwrap();
        assert!(top.contains(&format!("viewBox=\"0 0 {} ", n(w))), "the view box is the page in mm");
        assert_eq!(w, MIN_W);
        // R1's 1.2 x 1.4 pad, and the hole of J1; C1 (bottom) is not on the top's print
        assert!(top.contains("width=\"1.2\" height=\"1.4\""), "{top}");
        assert!(top.contains(">J1<") && top.contains(">R1<") && !top.contains(">C1<"));
        // the scale: 51 ticks and its numbers
        assert_eq!(top.matches("stroke-width=\"0.1\"").count(), RULER_MM as usize + 1);
        assert!(top.contains(">50<"));
        let bottom = print_check_svg(&b, Side::Bottom).unwrap();
        assert!(bottom.contains(">C1<") && bottom.contains(">J1<") && !bottom.contains(">R1<"));
        // mirrored: J1 (x = 5, near the left) is near the right from below
        let x0 = (MIN_W - 30.0) / 2.0;
        assert!(bottom.contains(&format!("cx=\"{}\"", n(x0 + 25.0))), "{bottom}");
        assert!(print_check_svg(&Board::new("", 30.0, 20.0), Side::Top).is_err());
        // the steps are wrapped to the page
        assert!(wrap(STEPS[0], MIN_W - 2.0 * MARGIN, STEP_FONT).len() >= 2);
        for t in STEPS {
            let lines = wrap(t, MIN_W - 2.0 * MARGIN, STEP_FONT);
            let width = |l: &str| l.chars().map(|c| if c.is_ascii() { 0.55 } else { 1.0 } * STEP_FONT).sum::<f64>();
            assert!(lines.iter().all(|l| width(l) <= MIN_W - 2.0 * MARGIN), "{lines:?}");
            assert_eq!(lines.concat(), t);
        }
    }

    /// The check before ordering says to print it, once the board has parts.
    #[test]
    fn the_check_before_ordering_asks_for_the_print() {
        let has = |b: &Board| crate::fab::preflight(b, None).iter().any(|c| c.title == crate::fab::PRINT_CHECK_TITLE);
        let mut b = board();
        assert!(!has(&b), "no parts yet");
        b.parts.push(Part { refdes: "R1".into(), value: "1k".into(), footprint: "R_0805".into(), pos: Point::new(20.0, 10.0), rot: 0, side: Side::Top });
        assert!(has(&b));
    }
}
