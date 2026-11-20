//! A stripboard / perfboard layout drawn to print at its real size (B5): the parts' side (the parts, the wire links, the holes'
//! names) and the copper side seen from below, left and right swapped (the strips and where to cut them, or the bare wires of a
//! perfboard, and the holes to solder). SVG in millimetres: printed at 100 %, a hole of the drawing lies on a hole of the board.

use crate::stripboard::{hole_name, At, StripLayout};
use std::fmt::Write;

/// The hole pitch (mm).
const PITCH: f64 = 2.54;
/// Room around the board for the rows' letters, the strips' names and the columns' numbers (mm).
const LEFT: f64 = 22.0;
const RIGHT: f64 = 22.0;
const TOP: f64 = 18.0;
const BOTTOM: f64 = 16.0;

/// Which face of the board the drawing shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    /// From above: where the parts and the wire links go.
    Parts,
    /// From below, left and right swapped: the copper, the cuts, the holes to solder.
    Copper,
}

/// Where hole `at` is drawn (mm from the drawing's top left): on the copper side, column 1 is at the right.
pub fn hole_xy(l: &StripLayout, face: Face, at: At) -> (f64, f64) {
    let col = match face {
        Face::Parts => at.1 as f64,
        Face::Copper => (l.cols.saturating_sub(1) - at.1.min(l.cols.saturating_sub(1))) as f64,
    };
    (LEFT + col * PITCH, TOP + at.0 as f64 * PITCH)
}

/// The drawing's size (mm).
pub fn page_size(l: &StripLayout) -> (f64, f64) {
    (LEFT + RIGHT + (l.cols.max(1) - 1) as f64 * PITCH, TOP + BOTTOM + (l.rows.max(1) - 1) as f64 * PITCH)
}

/// A printable colour for a net's wire links (the supplies red, ground black).
fn net_colour(net: &str) -> &'static str {
    let u = net.to_ascii_uppercase();
    if matches!(u.as_str(), "GND" | "0" | "VSS" | "AGND" | "DGND") {
        return "#222222";
    }
    if u.starts_with("VCC") || u.starts_with("VDD") || u.starts_with('+') || u.starts_with("VIN") || (u.contains('V') && u.starts_with(|c: char| c.is_ascii_digit())) {
        return "#d02020";
    }
    const PALETTE: [&str; 5] = ["#2266cc", "#1e9046", "#d68000", "#8a3cbe", "#139a9e"];
    PALETTE[net.bytes().fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize)) % PALETTE.len()]
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The bare wires of a perfboard: along each piece of strip, from its first used hole to its last, and those drawn by hand.
pub fn perf_wires(l: &StripLayout) -> Vec<(At, At)> {
    l.perf_spans().into_iter().chain(l.bare.iter().copied()).collect()
}

/// The drawing of one face as an SVG text; `perf` for a perfboard (bare wires instead of strips), `title` on top.
pub fn strip_svg(l: &StripLayout, face: Face, perf: bool, title: &str) -> String {
    let (w, h) = page_size(l);
    let mut s = String::new();
    let _ = writeln!(s, r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.2}mm" height="{h:.2}mm" viewBox="0 0 {w:.2} {h:.2}" font-family="sans-serif">"#);
    let _ = writeln!(s, r#"<rect x="0" y="0" width="{w:.2}" height="{h:.2}" fill="white"/>"#);
    let xy = |a: At| hole_xy(l, face, a);
    // the board's edge, half a pitch out from the outer holes
    let (x0, y0) = xy((0, if face == Face::Parts { 0 } else { l.cols.saturating_sub(1) }));
    let bw = (l.cols.max(1) - 1) as f64 * PITCH + PITCH;
    let bh = (l.rows.max(1) - 1) as f64 * PITCH + PITCH;
    let _ = writeln!(s, r##"<rect x="{:.2}" y="{:.2}" width="{bw:.2}" height="{bh:.2}" fill="#fbf4e4" stroke="#888" stroke-width="0.2"/>"##, x0 - PITCH / 2.0, y0 - PITCH / 2.0);
    let face_name = match face {
        Face::Parts => "表 (部品の面)",
        Face::Copper => "裏 (銅の面・左右が逆)",
    };
    let _ = writeln!(s, r#"<text x="2" y="5" font-size="3.2">{} — {}</text>"#, esc(title), face_name);
    let _ = writeln!(s, r##"<text x="2" y="9" font-size="2.2" fill="#555">{} 列 × {} 行 (穴の間隔 2.54 mm)。100 % (実寸) で印刷すると、右下の線が 10 mm になります</text>"##, l.cols, l.rows);
    // the scale bar
    let _ = writeln!(s, r#"<line x1="{:.2}" y1="{:.2}" x2="{:.2}" y2="{:.2}" stroke="black" stroke-width="0.4"/><text x="{:.2}" y="{:.2}" font-size="2.2" text-anchor="end">10 mm</text>"#, w - 12.0, h - 4.0, w - 2.0, h - 4.0, w - 2.0, h - 5.2);
    // the copper: the strips cut at the cuts, or the bare wires of a perfboard
    if face == Face::Copper {
        if perf {
            for (a, b) in perf_wires(l) {
                let ((ax, ay), (bx, by)) = (xy(a), xy(b));
                let _ = writeln!(s, r##"<line x1="{ax:.2}" y1="{ay:.2}" x2="{bx:.2}" y2="{by:.2}" stroke="#9aa0a8" stroke-width="0.7" stroke-linecap="round"/>"##);
            }
        } else {
            for (r, a, b) in l.pieces() {
                let ((ax, ay), (bx, _)) = (xy((r, a)), xy((r, b)));
                let (lo, hi) = (ax.min(bx), ax.max(bx));
                let _ = writeln!(s, r##"<rect x="{:.2}" y="{:.2}" width="{:.2}" height="1.7" rx="0.3" fill="#e2b48a"/>"##, lo - 1.1, ay - 0.85, hi - lo + 2.2);
            }
        }
    }
    // the holes
    for r in 0..l.rows {
        for c in 0..l.cols {
            let (x, y) = xy((r, c));
            let _ = writeln!(s, r##"<circle cx="{x:.2}" cy="{y:.2}" r="0.45" fill="#777"/>"##);
        }
    }
    // the rows' letters and the strips' names (the parts' side: at the left; the copper side: at the right, where the board's left
    // edge now is), the columns' numbers every five
    for r in 0..l.rows {
        let (x, y) = xy((r, 0));
        let letters = hole_name((r, 0)).trim_end_matches(|c: char| c.is_ascii_digit()).to_string();
        let (lx, anchor) = if face == Face::Parts { (x - 2.4, "end") } else { (x + 2.4, "start") };
        let name = l.homes.get(r as usize).filter(|n| !n.is_empty()).map(|n| format!(" {n}")).unwrap_or_default();
        let label = if face == Face::Parts { format!("{}{letters}", if name.is_empty() { String::new() } else { format!("{} ", name.trim()) }) } else { format!("{letters}{name}") };
        let _ = writeln!(s, r#"<text x="{lx:.2}" y="{:.2}" font-size="1.8" text-anchor="{anchor}">{}</text>"#, y + 0.6, esc(&label));
    }
    for c in (0..l.cols).filter(|c| c % 5 == 0 || *c + 1 == l.cols) {
        let (x, y) = xy((0, c));
        let _ = writeln!(s, r#"<text x="{x:.2}" y="{:.2}" font-size="1.8" text-anchor="middle">{}</text>"#, y - 2.4, c + 1);
    }
    // the sections: a frame and the name above it (both faces), the strips' names at each section's left end (the parts' side)
    for (name, a, b) in &l.sections {
        let ((ax, ay), (bx, by)) = (xy(*a), xy(*b));
        let (lo, hi) = (ax.min(bx), ax.max(bx));
        let _ = writeln!(s, r##"<rect x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" fill="none" stroke="#4455bb" stroke-width="0.35" stroke-dasharray="1.5 0.8"/>"##, lo - 1.4, ay - 1.4, hi - lo + 2.8, by - ay + 2.8);
        let _ = writeln!(s, r##"<text x="{:.2}" y="{:.2}" font-size="2" fill="#4455bb">{}</text>"##, lo - 1.2, ay - 1.8, esc(name));
    }
    if face == Face::Parts {
        for (a, n) in &l.names {
            let (x, y) = xy(*a);
            let _ = writeln!(s, r#"<text x="{:.2}" y="{:.2}" font-size="1.3" fill="{}">{}</text>"#, x - 0.9, y - 0.7, net_colour(n), esc(n));
        }
    }
    match face {
        Face::Copper => {
            if !perf {
                for cut in &l.cuts {
                    let (x, y) = xy(*cut);
                    let d = 0.9;
                    let _ = writeln!(s, r##"<path d="M{:.2} {:.2}L{:.2} {:.2}M{:.2} {:.2}L{:.2} {:.2}" stroke="#d01010" stroke-width="0.45"/>"##, x - d, y - d, x + d, y + d, x - d, y + d, x + d, y - d);
                }
            }
            // every hole with a lead or a wire's end in it: a ring to solder
            let mut used: Vec<At> = l.used().into_iter().map(|(a, _)| a).collect();
            used.sort();
            used.dedup();
            for a in used {
                let (x, y) = xy(a);
                let _ = writeln!(s, r##"<circle cx="{x:.2}" cy="{y:.2}" r="0.85" fill="none" stroke="#333" stroke-width="0.3"/>"##);
            }
        }
        Face::Parts => {
            for p in &l.parts {
                let pts: Vec<(f64, f64)> = p.leads.iter().map(|(_, a)| xy(*a)).collect();
                if pts.is_empty() {
                    continue;
                }
                let (lo_x, hi_x) = pts.iter().fold((f64::MAX, f64::MIN), |m, q| (m.0.min(q.0), m.1.max(q.0)));
                let (lo_y, hi_y) = pts.iter().fold((f64::MAX, f64::MIN), |m, q| (m.0.min(q.1), m.1.max(q.1)));
                if p.leads.len() > 2 {
                    // a chip: its outline (the adapter board's or the socket's), pin 1 marked, the name in the middle
                    let fill = match p.mount {
                        crate::mount::Mount::Adapter { .. } => "#d8eedd",
                        _ => "#eeeeee",
                    };
                    let _ = writeln!(s, r##"<rect x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" rx="0.4" fill="{fill}" stroke="#222" stroke-width="0.3"/>"##, lo_x - 1.2, lo_y - 1.4, hi_x - lo_x + 2.4, hi_y - lo_y + 2.8);
                    if let Some((_, a)) = p.leads.iter().find(|(pin, _)| pin == "1").or(p.leads.first()) {
                        let (x, y) = xy(*a);
                        let _ = writeln!(s, r##"<circle cx="{:.2}" cy="{:.2}" r="0.45" fill="#222"/>"##, x, y - 1.0);
                    }
                    let _ = writeln!(s, r#"<text x="{:.2}" y="{:.2}" font-size="2.2" text-anchor="middle">{}</text>"#, (lo_x + hi_x) / 2.0, (lo_y + hi_y) / 2.0 + 0.8, esc(&p.refdes));
                } else {
                    // a two-lead part: the leads, the body between them, the name beside it
                    if let [(ax, ay), (bx, by)] = pts.as_slice() {
                        let _ = writeln!(s, r##"<line x1="{ax:.2}" y1="{ay:.2}" x2="{bx:.2}" y2="{by:.2}" stroke="#555" stroke-width="0.3"/>"##);
                        let (mx, my) = ((ax + bx) / 2.0, (ay + by) / 2.0);
                        let vertical = (by - ay).abs() >= (bx - ax).abs();
                        let len = ((bx - ax).hypot(by - ay) * 0.5).clamp(2.0, 7.0);
                        let (rw, rh) = if vertical { (1.6, len) } else { (len, 1.6) };
                        let _ = writeln!(s, r##"<rect x="{:.2}" y="{:.2}" width="{rw:.2}" height="{rh:.2}" rx="0.5" fill="#f3dfb5" stroke="#333" stroke-width="0.25"/>"##, mx - rw / 2.0, my - rh / 2.0);
                        let _ = writeln!(s, r#"<text x="{:.2}" y="{:.2}" font-size="1.7">{}</text>"#, mx + rw / 2.0 + 0.4, my + 0.6, esc(&p.refdes));
                    }
                }
                for (x, y) in &pts {
                    let _ = writeln!(s, r##"<circle cx="{x:.2}" cy="{y:.2}" r="0.6" fill="#333"/>"##);
                }
            }
            for k in &l.links {
                let ((ax, ay), (bx, by)) = (xy(k.a), xy(k.b));
                let c = net_colour(&k.net);
                let _ = writeln!(s, r#"<line x1="{ax:.2}" y1="{ay:.2}" x2="{bx:.2}" y2="{by:.2}" stroke="{c}" stroke-width="0.55" stroke-linecap="round"/>"#);
                for (x, y) in [(ax, ay), (bx, by)] {
                    let _ = writeln!(s, r#"<circle cx="{x:.2}" cy="{y:.2}" r="0.55" fill="{c}"/>"#);
                }
            }
            // the supplies off the board: their pin's name at the hole
            for (r, pins) in &l.off {
                for (pin, net, a) in pins {
                    let (x, y) = xy(*a);
                    let _ = writeln!(s, r#"<circle cx="{x:.2}" cy="{y:.2}" r="0.6" fill="{}"/><text x="{:.2}" y="{:.2}" font-size="1.5" fill="{}">{} {}</text>"#, net_colour(net), x + 0.8, y - 0.8, net_colour(net), esc(r), esc(pin));
                }
            }
        }
    }
    s.push_str("</svg>\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stripboard::strip_layout;

    /// The copper side is the parts' side swapped left and right, a pitch is 2.54 mm, and every cut is drawn there.
    #[test]
    fn the_copper_side_is_the_mirror_and_to_scale() {
        let s: crate::Schematic = serde_json::from_str(include_str!("../../../docs/examples/ne555_blinker.schjson")).unwrap();
        let l = strip_layout(&s);
        let (fx, fy) = hole_xy(&l, Face::Parts, (2, 0));
        let (bx, by) = hole_xy(&l, Face::Copper, (2, l.cols - 1));
        assert_eq!((fx, fy), (bx, by), "column 1 on the parts' side is where the last column is from below");
        let (x1, _) = hole_xy(&l, Face::Parts, (0, 1));
        assert!((x1 - fx - 2.54).abs() < 1e-9);
        let back = strip_svg(&l, Face::Copper, false, "ne555");
        assert!(back.starts_with("<svg") && back.contains("mm\" height="));
        assert_eq!(back.matches("<path d=\"M").count(), l.cuts.len(), "a cross for every cut");
        let front = strip_svg(&l, Face::Parts, false, "ne555");
        assert!(l.parts.iter().all(|p| front.contains(&format!(">{}<", p.refdes))), "every part named");
        assert_eq!(front.matches("<path d=\"M").count(), 0, "no cuts on the parts' side");
        // a perfboard's copper side: the bare wires, no cuts
        let perf = strip_svg(&l, Face::Copper, true, "ne555");
        assert_eq!(perf.matches("<path d=\"M").count(), 0);
        assert_eq!(perf.matches("stroke=\"#9aa0a8\"").count(), perf_wires(&l).len());
    }
}
