//! The assembly sheet (candidate B3): the parts in the order they are soldered on by hand (the lowest body first, so the board
//! lies flat on the bench while each one is soldered), the mark that shows which way a part goes on (pin 1 of an IC or a transistor,
//! the cathode of a diode or an LED, + of an electrolytic capacitor), and a page to print (SVG, A4 wide): the placement drawing
//! with the marks, and the list with a box to tick for each part. The bench tab of the app (`app/src/bench_tab.rs`) shows the same
//! order and marks on screen.

use crate::model::*;
use std::fmt::Write;

/// Which way a part goes on, as the sheet marks it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// Pin 1 of an IC, a transistor or a connector with a direction.
    Pin1,
    /// The cathode (K) of a diode or an LED: the band on the body.
    Cathode,
    /// The + lead of an electrolytic or tantalum capacitor (the - side has the stripe on the body).
    Plus,
}

impl Mark {
    /// The letter written by the mark on the drawing.
    pub fn letter(self) -> &'static str {
        match self {
            Mark::Pin1 => "1",
            Mark::Cathode => "K",
            Mark::Plus => "+",
        }
    }

    /// What it means, for the list.
    pub fn label(self) -> &'static str {
        match self {
            Mark::Pin1 => "1 番ピン",
            Mark::Cathode => "カソード (K)",
            Mark::Plus => "+ 側",
        }
    }
}

/// One part in the order of fitting.
#[derive(Clone, Debug)]
pub struct FitItem {
    pub refdes: String,
    pub value: String,
    pub footprint: String,
    /// What kind of part it is, in words ("抵抗", "IC" ...).
    pub kind: &'static str,
    /// Height of the body above the board (mm), as far as the 3D data tells (0 when it does not).
    pub height: f64,
    /// The way it goes on, and the pad that carries the mark.
    pub mark: Option<(Mark, Point)>,
    pub bottom: bool,
}

fn letters(refdes: &str) -> String {
    refdes.chars().take_while(|c| c.is_ascii_alphabetic()).collect::<String>().to_ascii_uppercase()
}

/// The kind of a part from its designator and footprint.
pub fn kind_of(refdes: &str, footprint: &str) -> &'static str {
    let fp = footprint.to_ascii_lowercase();
    match letters(refdes).as_str() {
        "R" | "RN" | "VR" => "抵抗",
        "C" => "コンデンサ",
        "D" if fp.contains("led") => "LED",
        "LED" => "LED",
        "D" | "ZD" => "ダイオード",
        "Q" | "TR" => "トランジスタ",
        "U" | "IC" => "IC",
        "L" | "FB" => "コイル",
        "Y" | "XTAL" => "水晶",
        "J" | "CN" | "P" => "コネクタ",
        "SW" | "S" => "スイッチ",
        "F" => "ヒューズ",
        "RT" | "TH" | "NTC" => "サーミスタ",
        "BT" => "電池",
        _ => "その他",
    }
}

/// The mark of a part and the pad it is on: the pad `K` of a diode or an LED, pin 1 (= +) of an electrolytic capacitor, pin 1 of
/// an IC or a transistor. None for the parts that go on either way (resistors, ceramic capacitors ...).
pub fn mark_of(board: &Board, part: &Part) -> Option<(Mark, Point)> {
    let pads: Vec<&Pad> = board.pads.iter().filter(|p| p.refdes == part.refdes).collect();
    let pad = |name: &str| pads.iter().find(|p| p.pin.eq_ignore_ascii_case(name)).map(|p| p.pos);
    let fp = part.footprint.to_ascii_lowercase();
    if let Some(k) = pad("K") {
        return Some((Mark::Cathode, k));
    }
    let polar_cap = fp.starts_with("cp_") || fp.contains("elec") || fp.contains("tantal") || fp.contains("polar");
    if polar_cap {
        return pad("+").or_else(|| pad("1")).map(|p| (Mark::Plus, p));
    }
    match letters(&part.refdes).as_str() {
        "U" | "IC" | "Q" | "TR" | "D" | "LED" | "ZD" | "BT" => pad("1").map(|p| (Mark::Pin1, p)),
        _ => None,
    }
}

/// Height of a part's body above the board (mm): the maker's 3D shape, else the footprint's own height, else the guessed solids.
fn height_of(board: &Board, part: &Part, boxes: &[crate::model3d::PartBox]) -> f64 {
    let from_mesh = board.models3d.get(&part.footprint).and_then(|m| m.bounds()).map(|(lo, hi)| hi[2] - lo[2]).unwrap_or(0.0);
    if from_mesh > 0.0 {
        return from_mesh;
    }
    let from_fp = board.footprint(&part.footprint).and_then(|f| f.height).unwrap_or(0.0);
    if from_fp > 0.0 {
        return from_fp;
    }
    boxes.iter().filter(|b| b.refdes == part.refdes).map(|b| b.hi[2] - b.lo[2]).fold(0.0_f64, f64::max)
}

/// The parts of the board, lowest body first; the same height in designator order (R2 before R10). Bare-copper test points are
/// left out (nothing is soldered there).
pub fn fitting_order(board: &Board) -> Vec<FitItem> {
    let boxes = crate::model3d::part_boxes(board);
    let mut out: Vec<FitItem> = board
        .parts
        .iter()
        .filter(|p| !crate::assembly::is_bare_copper(&p.footprint))
        .map(|p| FitItem {
            refdes: p.refdes.clone(),
            value: p.value.clone(),
            footprint: p.footprint.clone(),
            kind: kind_of(&p.refdes, &p.footprint),
            height: height_of(board, p, &boxes),
            mark: mark_of(board, p),
            bottom: p.side == Side::Bottom,
        })
        .collect();
    let natural = |s: &str| {
        let head: String = s.chars().take_while(|c| !c.is_ascii_digit()).collect();
        let num: String = s.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
        (head, num.parse::<u64>().unwrap_or(0), s.to_string())
    };
    out.sort_by(|a, b| a.height.total_cmp(&b.height).then_with(|| natural(&a.refdes).cmp(&natural(&b.refdes))));
    out
}

fn n(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

const PAGE_W: f64 = 210.0;
const MARGIN: f64 = 12.0;
const RED: &str = "#d62828";
const ROW: f64 = 6.0;

/// The page to print: the drawing of the top (bottom parts dashed) with the designators and the marks, then the list in the order
/// of fitting with a box to tick. `fitted`: the parts already ticked on screen (their boxes come ticked). A4 wide; as long as
/// the list needs (a browser prints it on as many pages).
pub fn assembly_svg(board: &Board, fitted: &[String]) -> Result<String, String> {
    let order = fitting_order(board);
    if order.is_empty() {
        return Err("基板に部品がないので、組み立て表を作れません。先に回路図から部品を取り込んでください".into());
    }
    let (lo, hi) = board
        .bounds()
        .or_else(|| {
            let mut it = board.pads.iter().map(|p| p.pos);
            let first = it.next()?;
            Some(it.fold((first, first), |(a, b), p| (Point::new(a.x.min(p.x), a.y.min(p.y)), Point::new(b.x.max(p.x), b.y.max(p.y)))))
        })
        .ok_or("基板に外形も部品もないので、図にできません")?;
    let mut s = String::new();
    // the heading
    let title = if board.name.is_empty() { "組み立て表".to_string() } else { format!("組み立て表: {}", board.name) };
    let mut y = MARGIN + 6.0;
    let mut body = String::new();
    let _ = write!(body, "<text x=\"{}\" y=\"{}\" font-size=\"6\" font-weight=\"bold\">{}</text>", n(MARGIN), n(y), esc(&title));
    y += 6.0;
    for line in [
        "背の低い部品から順に付けます (基板が平らに置けて、はんだ付けしやすい順)。付けたら □ に印をつけます。",
        "赤い印は向きのある部品の目印です: 1 = 1 番ピン、K = カソード (ダイオード・LED の帯の側)、+ = 電解コンデンサの + 側。",
        "破線の部品は裏面に付けます。付けたあと、電源を入れる前に、ブリッジ・付け忘れ・向きを拡大鏡で見直します。",
    ] {
        let _ = write!(body, "<text x=\"{}\" y=\"{}\" font-size=\"3\">{}</text>", n(MARGIN), n(y), esc(line));
        y += 4.4;
    }
    y += 2.0;
    // the drawing, scaled to the page
    let (bw, bh) = ((hi.x - lo.x).max(1.0), (hi.y - lo.y).max(1.0));
    let avail_w = PAGE_W - 2.0 * MARGIN;
    let scale = (avail_w / bw).min(110.0 / bh).min(8.0);
    let x0 = MARGIN + (avail_w - bw * scale) / 2.0;
    let y0 = y;
    let to = |p: Point| (x0 + (p.x - lo.x) * scale, y0 + (hi.y - p.y) * scale);
    if board.outline.len() >= 3 {
        let pts: Vec<String> = board.outline.iter().map(|p| { let (a, b) = to(*p); format!("{},{}", n(a), n(b)) }).collect();
        let _ = write!(body, "<polygon points=\"{}\" fill=\"#eef4ef\" stroke=\"#333\" stroke-width=\"0.3\"/>", pts.join(" "));
    }
    for pad in &board.pads {
        let (w, h) = pad.shape.size();
        let (cx, cy) = to(pad.pos);
        let (w, h) = ((w * scale).max(0.4), (h * scale).max(0.4));
        match pad.shape {
            PadShape::Circle { .. } => {
                let _ = write!(body, "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"#b5b5b5\"/>", n(cx), n(cy), n(w / 2.0));
            }
            _ => {
                let rx = pad.shape.corner_radius() * scale;
                let _ = write!(body, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" fill=\"#b5b5b5\"/>", n(cx - w / 2.0), n(cy - h / 2.0), n(w), n(h), n(rx));
            }
        }
    }
    let font = (scale * 1.1).clamp(1.6, 3.6);
    for part in board.parts.iter().filter(|p| order.iter().any(|f| f.refdes == p.refdes)) {
        let (mut plo, mut phi) = (Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN));
        for pad in board.pads.iter().filter(|p| p.refdes == part.refdes) {
            let (w, h) = pad.shape.size();
            (plo.x, plo.y, phi.x, phi.y) = (plo.x.min(pad.pos.x - w / 2.0), plo.y.min(pad.pos.y - h / 2.0), phi.x.max(pad.pos.x + w / 2.0), phi.y.max(pad.pos.y + h / 2.0));
        }
        if let Some((a, b)) = crate::model3d::body_extent(board, part) {
            (plo.x, plo.y, phi.x, phi.y) = (plo.x.min(a.x), plo.y.min(a.y), phi.x.max(b.x), phi.y.max(b.y));
        }
        if plo.x > phi.x {
            continue;
        }
        let (ax, ay) = to(Point::new(plo.x - 0.3, phi.y + 0.3));
        let (bx, by) = to(Point::new(phi.x + 0.3, plo.y - 0.3));
        let dash = if part.side == Side::Bottom { " stroke-dasharray=\"1,0.7\"" } else { "" };
        let _ = write!(body, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"#444\" stroke-width=\"0.25\"{dash}/>", n(ax), n(ay), n(bx - ax), n(by - ay));
        let _ = write!(body, "<text x=\"{}\" y=\"{}\" font-size=\"{}\" text-anchor=\"middle\" dominant-baseline=\"central\">{}</text>", n((ax + bx) / 2.0), n((ay + by) / 2.0), n(font), esc(&part.refdes));
    }
    for f in &order {
        if let Some((m, at)) = f.mark {
            let (cx, cy) = to(at);
            let _ = write!(body, "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"none\" stroke=\"{RED}\" stroke-width=\"0.4\"/>", n(cx), n(cy), n(0.9_f64.max(0.5 * scale)));
            let _ = write!(body, "<text x=\"{}\" y=\"{}\" font-size=\"{}\" fill=\"{RED}\" font-weight=\"bold\">{}</text>", n(cx + 0.9_f64.max(0.5 * scale) + 0.3), n(cy - 0.6), n(font * 0.9), m.letter());
        }
    }
    y = y0 + bh * scale + 8.0;
    // the list
    let cols = [(MARGIN, "済"), (MARGIN + 9.0, "順"), (MARGIN + 18.0, "部品番号"), (MARGIN + 38.0, "値"), (MARGIN + 66.0, "種類"), (MARGIN + 88.0, "向き"), (MARGIN + 114.0, "高さ"), (MARGIN + 130.0, "フットプリント")];
    for (x, h) in cols {
        let _ = write!(body, "<text x=\"{}\" y=\"{}\" font-size=\"3.2\" font-weight=\"bold\">{h}</text>", n(x), n(y));
    }
    let _ = write!(body, "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#333\" stroke-width=\"0.3\"/>", n(MARGIN), n(y + 1.6), n(PAGE_W - MARGIN), n(y + 1.6));
    y += 1.6;
    for (i, f) in order.iter().enumerate() {
        let top = y;
        y += ROW;
        let base = y - 1.8;
        if i % 2 == 1 {
            let _ = write!(body, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#f2f2f2\"/>", n(MARGIN), n(top), n(PAGE_W - 2.0 * MARGIN), n(ROW));
        }
        let _ = write!(body, "<rect x=\"{}\" y=\"{}\" width=\"3.6\" height=\"3.6\" fill=\"#fff\" stroke=\"#333\" stroke-width=\"0.3\"/>", n(MARGIN + 0.5), n(top + 1.2));
        if fitted.contains(&f.refdes) {
            let _ = write!(body, "<path d=\"M{} {} l1.1 1.3 l2.2 -2.8\" fill=\"none\" stroke=\"#1a7a35\" stroke-width=\"0.6\"/>", n(MARGIN + 0.9), n(top + 3.0));
        }
        let side = if f.bottom { " (裏)" } else { "" };
        let mark = f.mark.map_or(String::new(), |(m, _)| m.label().to_string());
        let height = if f.height > 0.0 { format!("{:.1} mm", f.height) } else { "—".into() };
        let cells = [
            (cols[1].0, (i + 1).to_string(), false),
            (cols[2].0, format!("{}{side}", f.refdes), false),
            (cols[3].0, f.value.clone(), false),
            (cols[4].0, f.kind.to_string(), false),
            (cols[5].0, mark, true),
            (cols[6].0, height, false),
            (cols[7].0, f.footprint.clone(), false),
        ];
        for (x, text, red) in cells {
            let colour = if red { format!(" fill=\"{RED}\"") } else { String::new() };
            let _ = write!(body, "<text x=\"{}\" y=\"{}\" font-size=\"3\"{colour}>{}</text>", n(x), n(base), esc(&text));
        }
    }
    y += MARGIN;
    let _ = writeln!(
        s,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}mm\" height=\"{h}mm\" viewBox=\"0 0 {w} {h}\" font-family=\"'Yu Gothic', 'Meiryo', 'Noto Sans CJK JP', sans-serif\"><title>{}</title><rect width=\"{w}\" height=\"{h}\" fill=\"#fff\"/>{body}</svg>",
        esc(&title),
        w = n(PAGE_W),
        h = n(y),
    );
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(b: &mut Board, refdes: &str, fp: &str, pins: &[(&str, f64, f64)], drill: Option<f64>) {
        b.parts.push(Part { refdes: refdes.into(), value: "v".into(), footprint: fp.into(), pos: Point::new(pins[0].1, pins[0].2), rot: 0, side: Side::Top });
        for (pin, x, y) in pins {
            let mut p = Pad::new(Point::new(*x, *y), PadShape::Rect { w: 1.0, h: 1.2 }, drill, Side::Top);
            p.refdes = refdes.into();
            p.pin = (*pin).into();
            b.pads.push(p);
        }
    }

    fn sample() -> Board {
        let mut b = Board::new("demo", 40.0, 30.0);
        part(&mut b, "U1", "DIP-8_W7.62", &[("1", 20.0, 10.0), ("2", 20.0, 12.54), ("8", 27.62, 10.0)], Some(0.8));
        part(&mut b, "R10", "R_0805", &[("1", 5.0, 5.0), ("2", 7.0, 5.0)], None);
        part(&mut b, "R2", "R_0805", &[("1", 5.0, 8.0), ("2", 7.0, 8.0)], None);
        part(&mut b, "D1", "D_DO-41_P10.16", &[("A", 5.0, 20.0), ("K", 15.16, 20.0)], Some(1.0));
        part(&mut b, "C1", "CP_Radial_D5_P2.0", &[("1", 30.0, 25.0), ("2", 32.0, 25.0)], Some(0.8));
        part(&mut b, "C2", "C_0805", &[("1", 10.0, 25.0), ("2", 12.0, 25.0)], None);
        part(&mut b, "TP1", "TestPoint_Pad_D1.5mm", &[("1", 35.0, 5.0)], None);
        b
    }

    #[test]
    fn marks_go_on_the_cathode_the_plus_and_pin_one() {
        let b = sample();
        let order = fitting_order(&b);
        let mark = |r: &str| order.iter().find(|f| f.refdes == r).unwrap().mark;
        assert_eq!(mark("D1"), Some((Mark::Cathode, Point::new(15.16, 20.0))));
        assert_eq!(mark("C1"), Some((Mark::Plus, Point::new(30.0, 25.0))));
        assert_eq!(mark("U1"), Some((Mark::Pin1, Point::new(20.0, 10.0))));
        assert_eq!(mark("C2"), None, "a ceramic capacitor goes on either way");
        assert_eq!(mark("R2"), None);
    }

    #[test]
    fn low_parts_come_first_and_test_points_are_left_out() {
        let order = fitting_order(&sample());
        assert!(!order.iter().any(|f| f.refdes == "TP1"));
        assert!(order.windows(2).all(|w| w[0].height <= w[1].height), "{:?}", order.iter().map(|f| (&f.refdes, f.height)).collect::<Vec<_>>());
        let at = |r: &str| order.iter().position(|f| f.refdes == r).unwrap();
        assert!(at("R2") < at("R10"), "the same height in designator order");
        assert!(at("R2") < at("U1") && at("R2") < at("C1"), "chip resistors before the DIP and the electrolytic");
        assert_eq!(order[at("U1")].kind, "IC");
        assert_eq!(order[at("D1")].kind, "ダイオード");
    }

    #[test]
    fn the_sheet_has_every_part_a_box_and_the_marks() {
        let s = assembly_svg(&sample(), &["R2".to_string()]).unwrap();
        assert!(s.starts_with("<svg ") && s.trim_end().ends_with("</svg>"));
        assert!(s.contains("width=\"210mm\""));
        for r in ["U1", "R2", "R10", "D1", "C1", "C2"] {
            assert!(s.contains(&format!(">{r}<")), "{r} on the drawing");
        }
        assert!(!s.contains(">TP1<"));
        assert!(s.contains("カソード (K)") && s.contains("+ 側") && s.contains("1 番ピン"));
        assert_eq!(s.matches("stroke=\"#1a7a35\"").count(), 1, "the part ticked on screen comes ticked");
        assert!(assembly_svg(&Board::new("empty", 10.0, 10.0), &[]).is_err());
    }
}
