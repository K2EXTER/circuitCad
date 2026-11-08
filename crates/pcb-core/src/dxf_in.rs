//! Read the outline and the holes of a board from a DXF (the case's drawing), the other way of [`crate::dxf`].
//!
//! ASCII DXF: LINE, ARC, CIRCLE, LWPOLYLINE and POLYLINE (with bulges) are read; anything else is named in the notes. Lines, arcs
//! and polylines that meet end to end make loops; the loop with the largest area is the outline, circles inside it are holes.
//! The drawing's unit comes from `$INSUNITS` (inch, foot, cm, m; none or mm: millimetres).

use crate::model::*;

/// What a DXF of a case drawing gives a board.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DxfShape {
    pub outline: Vec<Point>,
    /// (centre, diameter) in mm.
    pub holes: Vec<(Point, f64)>,
    /// What was found but left out, in Japanese.
    pub notes: Vec<String>,
}

#[derive(Debug, Default)]
struct Ent {
    kind: String,
    layer: String,
    codes: Vec<(i32, String)>,
}

impl Ent {
    fn f(&self, code: i32) -> Option<f64> {
        self.codes.iter().find(|(c, _)| *c == code).and_then(|(_, v)| v.trim().parse().ok())
    }
}

fn parse_pairs(text: &str) -> Result<Vec<(i32, String)>, String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = vec![];
    let mut i = 0;
    while i + 1 < lines.len() {
        let code: i32 = lines[i].trim().parse().map_err(|_| format!("{} 行目: グループコードが数字ではありません。DXF ではないか、バイナリ形式の DXF です (ASCII 形式で保存し直してください)", i + 1))?;
        out.push((code, lines[i + 1].trim().to_string()));
        i += 2;
    }
    Ok(out)
}

/// The turn of one straight piece of an arc of radius `r` (radians): within the outline module's tolerance.
fn step(r: f64) -> f64 {
    let by_tolerance = if r > crate::outline::ARC_TOLERANCE { 2.0 * (1.0 - crate::outline::ARC_TOLERANCE / r).clamp(-1.0, 1.0).acos() } else { 0.5 };
    by_tolerance.min(crate::outline::ARC_STEP_DEG.to_radians()).max(0.001)
}

/// The points of an arc from `a0` to `a1` degrees (counter-clockwise).
fn arc_points(c: Point, r: f64, a0: f64, a1: f64) -> Vec<Point> {
    let mut sweep = (a1 - a0).to_radians();
    while sweep <= 0.0 {
        sweep += std::f64::consts::TAU;
    }
    let n = (sweep / step(r)).ceil().max(1.0) as usize;
    (0..=n)
        .map(|k| {
            let a = a0.to_radians() + sweep * k as f64 / n as f64;
            Point::new(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect()
}

/// A polyline segment from `p` to `q` with a DXF bulge (the tangent of a quarter of the turn; positive = counter-clockwise).
fn bulge_points(p: Point, q: Point, bulge: f64) -> Vec<Point> {
    let chord = (q.x - p.x).hypot(q.y - p.y);
    if bulge.abs() < 1e-9 || chord < 1e-12 {
        return vec![p, q];
    }
    let theta = 4.0 * bulge.atan();
    let r = chord / (2.0 * (theta / 2.0).sin().abs());
    let mid = Point::new((p.x + q.x) / 2.0, (p.y + q.y) / 2.0);
    let d = (r * r - chord * chord / 4.0).max(0.0).sqrt();
    // the centre is to the left of the chord for a counter-clockwise turn below a half circle, to the right above it
    let (ux, uy) = ((q.x - p.x) / chord, (q.y - p.y) / chord);
    let left = (bulge > 0.0) == (theta.abs() < std::f64::consts::PI);
    let sgn = if left { 1.0 } else { -1.0 };
    let c = Point::new(mid.x - uy * d * sgn, mid.y + ux * d * sgn);
    let a0 = (p.y - c.y).atan2(p.x - c.x);
    let n = (theta.abs() / step(r)).ceil().max(1.0) as usize;
    (0..=n)
        .map(|k| {
            let a = a0 + theta * k as f64 / n as f64;
            Point::new(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect()
}

fn area(poly: &[Point]) -> f64 {
    let n = poly.len();
    (0..n).map(|i| poly[i].x * poly[(i + 1) % n].y - poly[(i + 1) % n].x * poly[i].y).sum::<f64>() / 2.0
}

fn poly_to_chain(verts: &[(f64, f64, f64)], closed: bool, scale: f64, chains: &mut Vec<Vec<Point>>, loops: &mut Vec<Vec<Point>>) {
    let n = verts.len();
    if n < 2 {
        return;
    }
    let p = |i: usize| Point::new(verts[i].0 * scale, verts[i].1 * scale);
    let mut pts: Vec<Point> = vec![];
    let segs = if closed { n } else { n - 1 };
    for i in 0..segs {
        let part = bulge_points(p(i), p((i + 1) % n), verts[i].2);
        let skip = usize::from(!pts.is_empty());
        pts.extend(part.into_iter().skip(skip));
    }
    if closed {
        if pts.len() > 1 && (pts[0].x - pts[pts.len() - 1].x).hypot(pts[0].y - pts[pts.len() - 1].y) < 1e-9 {
            pts.pop();
        }
        loops.push(pts);
    } else {
        chains.push(pts);
    }
}

/// Join open pieces that meet end to end into loops. Pieces that do not close are reported in `notes`.
fn join(mut chains: Vec<Vec<Point>>, loops: &mut Vec<Vec<Point>>, notes: &mut Vec<String>) {
    let near = |a: Point, b: Point| (a.x - b.x).hypot(a.y - b.y) <= 0.01;
    while let Some(mut c) = chains.pop() {
        loop {
            if c.len() >= 3 && near(c[0], c[c.len() - 1]) {
                c.pop();
                loops.push(c);
                break;
            }
            let end = c[c.len() - 1];
            // a piece that starts where this one ends (turned round when it ends there), else one that meets the start
            let j = chains.iter().position(|o| near(o[0], end) || near(o[o.len() - 1], end));
            if let Some(j) = j {
                let mut o = chains.swap_remove(j);
                if !near(o[0], end) {
                    o.reverse();
                }
                c.pop();
                c.extend(o);
                continue;
            }
            let start = c[0];
            let j = chains.iter().position(|o| near(o[0], start) || near(o[o.len() - 1], start));
            if let Some(j) = j {
                // put the piece in front: it must end where this one starts
                let mut o = chains.swap_remove(j);
                if !near(o[o.len() - 1], start) {
                    o.reverse();
                }
                o.pop();
                o.extend(c);
                c = o;
                continue;
            }
            notes.push(format!("つながっていない線の連なり ({:.1}, {:.1}) から ({:.1}, {:.1}) は、閉じた輪になっていないので、外形にしていません。線の端がずれていないか確かめてください。", start.x, start.y, end.x, end.y));
            break;
        }
    }
}

/// Read the outline and the holes from a DXF. `layer`: only the entities of that layer (None: all of them).
pub fn read(text: &str, layer: Option<&str>) -> Result<DxfShape, String> {
    let pairs = parse_pairs(text)?;
    if !pairs.iter().any(|(c, v)| *c == 0 && v == "SECTION") {
        return Err("DXF の形式ではありません (SECTION がありません)。AutoCAD の ASCII 形式の DXF を選んでください".into());
    }
    let mut scale = 1.0;
    for w in pairs.windows(2) {
        if w[0] == (9, "$INSUNITS".to_string()) && w[1].0 == 70 {
            scale = match w[1].1.as_str() {
                "1" => 25.4,
                "2" => 304.8,
                "5" => 10.0,
                "6" => 1000.0,
                _ => 1.0,
            };
        }
    }
    let mut ents: Vec<Ent> = vec![];
    let mut in_entities = false;
    let mut after_section = false;
    for (code, v) in &pairs {
        if *code == 0 {
            after_section = v == "SECTION";
            match v.as_str() {
                "ENDSEC" => in_entities = false,
                "EOF" => break,
                "SECTION" => {}
                _ if in_entities => ents.push(Ent { kind: v.clone(), ..Default::default() }),
                _ => {}
            }
            continue;
        }
        if after_section && *code == 2 {
            in_entities = v == "ENTITIES";
            after_section = false;
            continue;
        }
        if let (true, Some(e)) = (in_entities, ents.last_mut()) {
            if *code == 8 {
                e.layer = v.clone();
            } else {
                e.codes.push((*code, v.clone()));
            }
        }
    }
    let mut layers: Vec<String> = ents.iter().map(|e| e.layer.clone()).collect();
    layers.sort();
    layers.dedup();
    let pt = |x: f64, y: f64| Point::new(x * scale, y * scale);

    let mut chains: Vec<Vec<Point>> = vec![];
    let mut loops: Vec<Vec<Point>> = vec![];
    let mut out = DxfShape::default();
    let mut skipped: std::collections::BTreeMap<String, usize> = Default::default();
    let mut k = 0;
    while k < ents.len() {
        let e = &ents[k];
        k += 1;
        if layer.is_some_and(|l| !e.layer.eq_ignore_ascii_case(l)) {
            continue;
        }
        match e.kind.as_str() {
            "LINE" => {
                if let (Some(x0), Some(y0), Some(x1), Some(y1)) = (e.f(10), e.f(20), e.f(11), e.f(21)) {
                    chains.push(vec![pt(x0, y0), pt(x1, y1)]);
                }
            }
            "ARC" => {
                if let (Some(x), Some(y), Some(r), Some(a0), Some(a1)) = (e.f(10), e.f(20), e.f(40), e.f(50), e.f(51)) {
                    chains.push(arc_points(pt(x, y), r * scale, a0, a1));
                }
            }
            "CIRCLE" => {
                if let (Some(x), Some(y), Some(r)) = (e.f(10), e.f(20), e.f(40)) {
                    out.holes.push((pt(x, y), 2.0 * r * scale));
                }
            }
            "LWPOLYLINE" => {
                let mut verts: Vec<(f64, f64, f64)> = vec![];
                for (c, v) in &e.codes {
                    let x: f64 = v.parse().unwrap_or(0.0);
                    match (c, verts.last_mut()) {
                        (10, _) => verts.push((x, 0.0, 0.0)),
                        (20, Some(l)) => l.1 = x,
                        (42, Some(l)) => l.2 = x,
                        _ => {}
                    }
                }
                let closed = e.f(70).is_some_and(|f| f as i64 & 1 == 1);
                poly_to_chain(&verts, closed, scale, &mut chains, &mut loops);
            }
            "POLYLINE" => {
                let closed = e.f(70).is_some_and(|f| f as i64 & 1 == 1);
                let mut verts: Vec<(f64, f64, f64)> = vec![];
                while k < ents.len() && ents[k].kind != "SEQEND" {
                    if ents[k].kind == "VERTEX" {
                        verts.push((ents[k].f(10).unwrap_or(0.0), ents[k].f(20).unwrap_or(0.0), ents[k].f(42).unwrap_or(0.0)));
                    }
                    k += 1;
                }
                k += 1;
                poly_to_chain(&verts, closed, scale, &mut chains, &mut loops);
            }
            "VERTEX" | "SEQEND" => {}
            other => *skipped.entry(other.to_string()).or_default() += 1,
        }
    }
    join(chains, &mut loops, &mut out.notes);
    loops.retain(|l| l.len() >= 3);
    loops.sort_by(|a, b| area(b).abs().total_cmp(&area(a).abs()));
    let Some(big) = loops.first() else {
        let hint = if layers.len() > 1 { format!(" 層の名前: {}。層を指定すると見つかるかもしれません (layer=名前)。", layers.join(", ")) } else { String::new() };
        return Err(format!("外形になる閉じた輪が見つかりませんでした。線・円弧・ポリラインが端でつながって閉じている必要があります。{hint}"));
    };
    out.outline = big.clone();
    if area(&out.outline) < 0.0 {
        out.outline.reverse();
    }
    if loops.len() > 1 {
        out.notes.push(format!("ほかに閉じた輪が {} 個あります (切り欠きや長穴など)。外形には、いちばん大きい輪だけを使いました。", loops.len() - 1));
    }
    let before = out.holes.len();
    let outline = out.outline.clone();
    out.holes.retain(|(c, _)| crate::geom::inside_polygon(&outline, *c));
    if out.holes.len() < before {
        out.notes.push(format!("外形の外にある円 {} 個は、穴にしていません。", before - out.holes.len()));
    }
    for (kind, n) in skipped {
        out.notes.push(format!("{kind} が {n} 個ありましたが、読んでいません (線・円弧・円・ポリラインだけを読みます)。"));
    }
    if scale != 1.0 {
        out.notes.push(format!("図面の単位 (INSUNITS) から、{scale} 倍して mm にしました。"));
    }
    Ok(out)
}

/// Put what [`read`] found on a board: the outline replaced, the circles added as holes (the loose holes of before are
/// replaced; a part's own holes stay), and, with `to_origin`, everything moved so that the outline's lower left corner is (0, 0).
pub fn apply(board: &mut Board, shape: &DxfShape, to_origin: bool) {
    let (mut dx, mut dy) = (0.0, 0.0);
    if to_origin {
        dx = -shape.outline.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
        dy = -shape.outline.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
    }
    board.outline = shape.outline.iter().map(|p| Point::new(p.x + dx, p.y + dy)).collect();
    board.holes.retain(|h| !h.refdes.is_empty());
    for (c, d) in &shape.holes {
        board.holes.push(Hole::new(Point::new(c.x + dx, c.y + dy), *d));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dxf::mechanical;

    #[test]
    fn a_dxf_we_wrote_reads_back_as_the_same_outline_and_holes() {
        let mut b = Board::new("demo", 30.0, 20.0);
        b.holes.push(Hole::new(Point::new(3.0, 3.0), 3.2));
        b.holes.push(Hole::new(Point::new(27.0, 17.0), 2.0));
        let text = mechanical(&b);
        let shape = read(&text, Some("BOARD_OUTLINE")).unwrap();
        assert!((area(&shape.outline) - 600.0).abs() < 1e-6, "{}", area(&shape.outline));
        // the circles are on the HOLES layer, so the outline's layer alone has none
        assert!(shape.holes.is_empty());
        let all = read(&text, None).unwrap();
        assert_eq!(all.holes.len(), 2);
        assert!(all.holes.iter().any(|(c, d)| (c.x - 3.0).abs() < 1e-3 && (d - 3.2).abs() < 1e-3));
    }

    #[test]
    fn lines_and_an_arc_that_meet_make_a_rounded_outline() {
        // a 20 x 10 box whose right side is a half circle: three lines and one arc, given in no particular order
        let dxf = "0\nSECTION\n2\nENTITIES\n\
            0\nLINE\n8\n0\n10\n0\n20\n0\n11\n20\n21\n0\n\
            0\nLINE\n8\n0\n10\n0\n20\n10\n11\n0\n21\n0\n\
            0\nARC\n8\n0\n10\n20\n20\n5\n40\n5\n50\n270\n51\n90\n\
            0\nLINE\n8\n0\n10\n20\n20\n10\n11\n0\n21\n10\n\
            0\nENDSEC\n0\nEOF\n";
        let s = read(dxf, None).unwrap();
        let expected = 20.0 * 10.0 + std::f64::consts::PI * 25.0 / 2.0;
        assert!((area(&s.outline) - expected).abs() < 0.05, "{} vs {expected}", area(&s.outline));
    }

    #[test]
    fn a_closed_polyline_in_inches_and_a_bulge_are_read() {
        let dxf = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n1\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n\
            0\nLWPOLYLINE\n8\n0\n90\n4\n70\n1\n10\n0\n20\n0\n10\n2\n20\n0\n10\n2\n20\n1\n10\n0\n20\n1\n\
            0\nENDSEC\n0\nEOF\n";
        let s = read(dxf, None).unwrap();
        assert!((area(&s.outline) - 50.8 * 25.4).abs() < 1e-6);
        assert!(s.notes.iter().any(|n| n.contains("INSUNITS")));
        // a bulge of 1 over a chord of 2 is a half circle (counter-clockwise: it bulges to the right of the chord's direction)
        let p = bulge_points(Point::new(0.0, 0.0), Point::new(2.0, 0.0), 1.0);
        assert!(p.iter().all(|q| q.y <= 1e-9), "below the chord for a counter-clockwise turn from 0 to 2 on x");
        assert!(p.iter().any(|q| (q.y + 1.0).abs() < 1e-3), "its lowest point is 1 below");
        // the arc and its chord enclose half a circle of radius 1
        assert!((area(&p).abs() - std::f64::consts::PI / 2.0).abs() < 0.01, "{}", area(&p));
    }

    #[test]
    fn open_shapes_and_other_files_are_refused_with_the_reason() {
        let open = "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n5\n21\n0\n0\nENDSEC\n0\nEOF\n";
        assert!(read(open, None).unwrap_err().contains("閉じた輪"));
        assert!(read("not a dxf\nat all\n", None).unwrap_err().contains("DXF"));
    }

    #[test]
    fn applying_moves_the_outline_to_the_origin_and_keeps_the_holes_of_parts() {
        let mut b = Board::new("x", 10.0, 10.0);
        let mut mine = Hole::new(Point::new(1.0, 1.0), 1.0);
        mine.refdes = "J1".into();
        b.holes.push(mine);
        b.holes.push(Hole::new(Point::new(2.0, 2.0), 1.0));
        let shape = DxfShape { outline: vec![Point::new(100.0, 50.0), Point::new(130.0, 50.0), Point::new(130.0, 70.0), Point::new(100.0, 70.0)], holes: vec![(Point::new(103.0, 53.0), 3.2)], notes: vec![] };
        apply(&mut b, &shape, true);
        assert_eq!(b.outline[0], Point::new(0.0, 0.0));
        assert_eq!(b.holes.len(), 2, "the part's hole stays, the loose one is replaced by the drawing's");
        assert!(b.holes.iter().any(|h| (h.pos.x - 3.0).abs() < 1e-9 && (h.pos.y - 3.0).abs() < 1e-9 && (h.drill - 3.2).abs() < 1e-9));
    }
}
