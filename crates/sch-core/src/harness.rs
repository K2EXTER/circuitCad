//! Cables between boards (E11): every link of a multi-board project joined by a cable (`BoardLink::cable`) as a harness: the wires pin
//! to pin with what each carries, a colour per wire (ground black, power red, the signals in turn, or as chosen by hand), the length, the
//! mating housing and contacts at each end, and the drawing (SVG) and the wire list (CSV) to build it from. The parts list gets the
//! housings, the contacts and the wire.
//!
//! The mating parts are told from the board's connector: a 2.54 mm pin header takes a Dupont housing and female contacts, a 2-row header
//! (`2x`) an IDC socket on a flat cable (no contacts, the colours of a ribbon: wire 1 red, the rest grey), a JST XH or PH header its own
//! housing and contacts. Anything else is named as "the mating housing" for the person to pick.

use crate::system::{pin_nets, pin_pairs, pins_of, role, two_rows, BoardLink, BomRow, Role};
use crate::Schematic;
use std::collections::BTreeMap;

/// The wire colours: key (stored in the file), name, colour drawn.
pub const COLORS: &[(&str, &str, &str)] = &[
    ("black", "黒", "#202020"),
    ("red", "赤", "#d62828"),
    ("orange", "橙", "#f28c28"),
    ("yellow", "黄", "#f2d22e"),
    ("green", "緑", "#2a9d3f"),
    ("blue", "青", "#2a6fdb"),
    ("violet", "紫", "#8e44ad"),
    ("grey", "灰", "#8a8a8a"),
    ("white", "白", "#f4f4f4"),
    ("brown", "茶", "#8b5a2b"),
];

/// The signals' colours in turn (after black for ground and red, then orange, for power).
const SIGNAL_COLORS: &[&str] = &["yellow", "green", "blue", "white", "violet", "grey", "brown"];

/// A colour's name (the key itself when unknown).
pub fn color_name(key: &str) -> &str {
    COLORS.iter().find(|c| c.0 == key).map_or(key, |c| c.1)
}

/// A colour as drawn.
pub fn color_hex(key: &str) -> &'static str {
    COLORS.iter().find(|c| c.0 == key).map_or("#606060", |c| c.2)
}

/// What a cable's end plugs onto the board's connector.
#[derive(Clone, Debug, PartialEq)]
pub struct Mating {
    /// The family, as named to the person (`デュポン 2.54 mm`).
    pub family: String,
    pub housing: String,
    /// The crimp contact, one per wire (`None`: an IDC socket pressed onto the flat cable).
    pub contact: Option<String>,
    /// The wire size the contacts take.
    pub awg: u8,
    /// Whether the mating part was told from the footprint (`false`: the person picks it).
    pub known: bool,
}

/// The mating part of a connector with `n` pins and footprint `fp` (and part name `part`).
pub fn mating(fp: &str, part: &str, n: usize) -> Mating {
    let f = format!("{} {}", fp.to_ascii_lowercase(), part.to_ascii_lowercase());
    let m = |family: &str, housing: String, contact: Option<&str>, awg: u8| Mating { family: family.into(), housing, contact: contact.map(str::to_string), awg, known: true };
    if f.contains("jst_xh") || f.contains("xh-") || f.contains("b-xh") || f.contains("_xh_") {
        return m("JST XH 2.5 mm", format!("XHP-{n}"), Some("SXH-001T-P0.6"), 24);
    }
    if f.contains("jst_ph") || f.contains("b-ph") || f.contains("_ph_") || f.contains("ph-k") {
        return m("JST PH 2.0 mm", format!("PHR-{n}"), Some("SPH-002T-P0.5S"), 28);
    }
    if f.contains("2x") && (f.contains("header") || f.contains("idc") || f.contains("box")) {
        return m("IDC 2.54 mm (フラットケーブル)", format!("IDC ソケット 2×{} 極", n.div_ceil(2)), None, 28);
    }
    if f.contains("pinsocket") || f.contains("socket") {
        return m("デュポン 2.54 mm", format!("ハウジング 1×{n} 極 (2.54 mm)"), Some("オスのコンタクト (2.54 mm デュポン)"), 26);
    }
    if f.contains("header") || f.trim().is_empty() {
        return m("デュポン 2.54 mm", format!("ハウジング 1×{n} 極 (2.54 mm)"), Some("メスのコンタクト (2.54 mm デュポン)"), 26);
    }
    Mating { family: format!("{fp} の相手"), housing: format!("{fp} の相手のハウジング ({n} 極)"), contact: Some(format!("{fp} の相手のコンタクト")), awg: 24, known: false }
}

/// One wire of a cable.
#[derive(Clone, Debug, PartialEq)]
pub struct HarnessWire {
    /// The wire's number in the cable, from 1.
    pub n: usize,
    pub pin_a: String,
    pub pin_b: String,
    /// The nets on the two boards (`None`: the pin is open on that side).
    pub net_a: Option<String>,
    pub net_b: Option<String>,
    /// Colour key (`COLORS`).
    pub color: String,
    /// Chosen by hand (`BoardLink::colors`).
    pub chosen: bool,
}

impl HarnessWire {
    /// What the wire carries, as one name (`SDA`, or `SDA / I2C_SDA` when the two boards name it differently).
    pub fn signal(&self) -> String {
        match (&self.net_a, &self.net_b) {
            (Some(a), Some(b)) if a.eq_ignore_ascii_case(b) => a.clone(),
            (Some(a), Some(b)) => format!("{a} / {b}"),
            (Some(a), None) | (None, Some(a)) => a.clone(),
            (None, None) => String::new(),
        }
    }
}

/// A cable between two boards.
#[derive(Clone, Debug, PartialEq)]
pub struct Harness {
    /// Index into the links.
    pub link: usize,
    /// `W1`, `W2` ... in the order of the links.
    pub name: String,
    /// The two ends: `main.J1`.
    pub end_a: String,
    pub end_b: String,
    pub length_mm: f64,
    pub mate_a: Mating,
    pub mate_b: Mating,
    pub wires: Vec<HarnessWire>,
    /// A flat cable (both ends IDC): every pin has its wire, used or not.
    pub ribbon: bool,
    /// How many of this cable are made (the more of the two boards' copies).
    pub copies: usize,
    /// The pin counts of the two connectors.
    pub pins: (usize, usize),
}

impl Harness {
    /// The wire size: the thinner of what the two ends take.
    pub fn awg(&self) -> u8 {
        self.mate_a.awg.max(self.mate_b.awg)
    }
}

/// The default colours of a cable's wires, by what they carry: ground black, the first power net red and the next orange, the signals in
/// turn; on a flat cable wire 1 red and the rest grey.
fn default_colors(nets: &[Option<String>], ribbon: bool) -> Vec<String> {
    if ribbon {
        return (0..nets.len()).map(|i| if i == 0 { "red" } else { "grey" }.to_string()).collect();
    }
    let mut power: Vec<String> = vec![];
    let mut signals: Vec<String> = vec![];
    nets.iter()
        .map(|n| {
            let Some(n) = n else { return "grey".to_string() };
            match role(n) {
                Role::Ground => "black".to_string(),
                Role::Power => {
                    let k = power.iter().position(|p| p.eq_ignore_ascii_case(n)).unwrap_or_else(|| {
                        power.push(n.clone());
                        power.len() - 1
                    });
                    ["red", "orange", "brown"][k.min(2)].to_string()
                }
                _ => {
                    let k = signals.iter().position(|p| p.eq_ignore_ascii_case(n)).unwrap_or_else(|| {
                        signals.push(n.clone());
                        signals.len() - 1
                    });
                    SIGNAL_COLORS[k % SIGNAL_COLORS.len()].to_string()
                }
            }
        })
        .collect()
}

/// The cables of a project: every link with a cable length, its wires (the pin pairs with a net on at least one side; every pair on a
/// flat cable), colours and mating parts. `copies` is how many of each board are built, in the order of `designs`.
pub fn harnesses(designs: &[(&str, &Schematic)], links: &[BoardLink], copies: &[usize]) -> Vec<Harness> {
    let find = |n: &str| designs.iter().position(|(d, _)| *d == n);
    let mut out = vec![];
    for (k, link) in links.iter().enumerate() {
        let Some(length_mm) = link.cable else { continue };
        let (Some(ia), Some(ib)) = (find(&link.a.design), find(&link.b.design)) else { continue };
        let (sa, sb) = (designs[ia].1, designs[ib].1);
        let (Some(pa), Some(pb)) = (pins_of(sa, &link.a.refdes), pins_of(sb, &link.b.refdes)) else { continue };
        let part = |s: &Schematic, r: &str| {
            let flat = s.flat();
            flat.components.iter().find(|c| c.refdes == r).map(|c| (c.footprint.clone(), c.custom.as_ref().map(|x| x.name.clone()).unwrap_or_default())).unwrap_or_default()
        };
        let ((fa, na), (fb, nb)) = (part(sa, &link.a.refdes), part(sb, &link.b.refdes));
        let (mate_a, mate_b) = (mating(&fa, &na, pa.len()), mating(&fb, &nb, pb.len()));
        let ribbon = mate_a.contact.is_none() && mate_b.contact.is_none() && two_rows(sa, &link.a.refdes) && two_rows(sb, &link.b.refdes);
        let (nets_a, nets_b) = (pin_nets(sa), pin_nets(sb));
        let mut rows = vec![];
        for (p, q) in pin_pairs(link, &pa, &pb) {
            let x = nets_a.get(&(link.a.refdes.clone(), p.clone())).cloned();
            let y = nets_b.get(&(link.b.refdes.clone(), q.clone())).cloned();
            if x.is_none() && y.is_none() && !ribbon {
                continue;
            }
            rows.push((p, q, x, y));
        }
        let nets: Vec<Option<String>> = rows.iter().map(|r| r.2.clone().or(r.3.clone())).collect();
        let colors = default_colors(&nets, ribbon);
        let wires = rows
            .into_iter()
            .zip(colors)
            .enumerate()
            .map(|(i, ((pin_a, pin_b, net_a, net_b), c))| {
                let hand = link.colors.iter().find(|(p, _)| *p == pin_a).map(|(_, c)| c.clone());
                HarnessWire { n: i + 1, chosen: hand.is_some(), color: hand.unwrap_or(c), pin_a, pin_b, net_a, net_b }
            })
            .collect();
        let copies = copies.get(ia).copied().unwrap_or(1).max(copies.get(ib).copied().unwrap_or(1)).max(1);
        out.push(Harness {
            link: k,
            name: format!("W{}", out.len() + 1),
            end_a: format!("{}.{}", link.a.design, link.a.refdes),
            end_b: format!("{}.{}", link.b.design, link.b.refdes),
            length_mm,
            mate_a,
            mate_b,
            wires,
            ribbon,
            copies,
            pins: (pa.len(), pb.len()),
        });
    }
    out
}

fn csv_cell(s: &str) -> String {
    if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

/// The wire list as CSV (UTF-8 with a BOM): one line per wire of every cable, to cut, crimp and check them by.
pub fn harness_csv(hs: &[Harness]) -> String {
    let mut out = String::from("\u{feff}ケーブル,本数,線,色,AWG,長さ (mm),A 側,A のピン,A の信号,B 側,B のピン,B の信号\n");
    for h in hs {
        for w in &h.wires {
            let cols = [
                h.name.clone(),
                h.copies.to_string(),
                w.n.to_string(),
                color_name(&w.color).to_string(),
                h.awg().to_string(),
                format!("{:.0}", h.length_mm),
                h.end_a.clone(),
                w.pin_a.clone(),
                w.net_a.clone().unwrap_or_else(|| "(空き)".into()),
                h.end_b.clone(),
                w.pin_b.clone(),
                w.net_b.clone().unwrap_or_else(|| "(空き)".into()),
            ];
            out.push_str(&cols.iter().map(|c| csv_cell(c)).collect::<Vec<_>>().join(","));
            out.push('\n');
        }
    }
    out
}

/// The parts of the cables for the parts list: the housings of both ends, the contacts (one per wire and end), and the wire by colour
/// (or the flat cable) with the length to buy in the note. Equal lines of several cables are added up.
pub fn harness_bom(hs: &[Harness]) -> Vec<BomRow> {
    // (part, value) -> (count, mm)
    let mut rows: BTreeMap<(String, String), (usize, f64)> = BTreeMap::new();
    let mut add = |part: &str, value: String, n: usize, mm: f64| {
        let e = rows.entry((part.to_string(), value)).or_insert((0, 0.0));
        e.0 += n;
        e.1 += mm;
    };
    for h in hs {
        let c = h.copies;
        for m in [&h.mate_a, &h.mate_b] {
            add("ハウジング", m.housing.clone(), c, 0.0);
            if let Some(x) = &m.contact {
                add("コンタクト", x.clone(), c * h.wires.len(), 0.0);
            }
        }
        if h.ribbon {
            add("フラットケーブル", format!("{} 芯 1.27 mm ピッチ AWG{}", h.wires.len(), h.awg()), c, h.length_mm * c as f64);
        } else {
            for w in &h.wires {
                add("電線", format!("AWG{} {}", h.awg(), color_name(&w.color)), c, h.length_mm * c as f64);
            }
        }
    }
    rows.into_iter()
        .map(|((part, value), (total, mm))| BomRow {
            part,
            value,
            footprint: String::new(),
            shops: BTreeMap::new(),
            per_board: vec![],
            total,
            note: if mm > 0.0 { format!("合計 {mm:.0} mm (切る長さ。むく分と余裕を足して買う)") } else { String::new() },
        })
        .collect()
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The drawing of every cable (SVG): for each, the two connectors as boxes with their pins and signals, the wires drawn between them in
/// their colours (crossings show where the cable crosses), the length, the mating parts, and the wire list beside it.
pub fn harness_svg(hs: &[Harness]) -> String {
    const ROW: f64 = 22.0;
    const W: f64 = 1000.0;
    let mut body = String::new();
    let mut y0 = 20.0;
    for h in hs {
        let n = h.wires.len().max(1) as f64;
        let top = y0 + 66.0;
        let height = n * ROW + 16.0;
        let title = format!("{}: {} ↔ {}   ケーブル {:.0} mm   × {} 本", h.name, h.end_a, h.end_b, h.length_mm, h.copies);
        body.push_str(&format!("<text x=\"20\" y=\"{:.1}\" font-size=\"16\" font-weight=\"bold\">{}</text>\n", y0 + 16.0, esc(&title)));
        let kind = if h.ribbon { format!("フラットケーブル {} 芯 AWG{}", h.wires.len(), h.awg()) } else { format!("電線 AWG{}", h.awg()) };
        body.push_str(&format!("<text x=\"20\" y=\"{:.1}\" font-size=\"12\" fill=\"#555\">{}</text>\n", y0 + 34.0, esc(&kind)));
        // the two connectors: A's pins in the order of the wires, B's in its own pin order (a crossed cable crosses between them)
        let mut order_b: Vec<usize> = (0..h.wires.len()).collect();
        order_b.sort_by_key(|&i| (h.wires[i].pin_b.parse::<u32>().unwrap_or(u32::MAX), h.wires[i].pin_b.clone()));
        let row_of_b = |i: usize| order_b.iter().position(|&k| k == i).unwrap_or(i);
        let (ax, bx, bw) = (20.0, 420.0, 160.0);
        for (x, end, mate, left) in [(ax, &h.end_a, &h.mate_a, true), (bx, &h.end_b, &h.mate_b, false)] {
            body.push_str(&format!("<rect x=\"{x:.1}\" y=\"{top:.1}\" width=\"{bw:.1}\" height=\"{height:.1}\" rx=\"4\" fill=\"#f7f7f2\" stroke=\"#333\" stroke-width=\"1.5\"/>\n"));
            body.push_str(&format!("<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"13\" font-weight=\"bold\" text-anchor=\"middle\">{}</text>\n", x + bw / 2.0, top - 4.0, esc(end)));
            let under = top + height + 16.0;
            body.push_str(&format!("<text x=\"{x:.1}\" y=\"{under:.1}\" font-size=\"11\">{}</text>\n", esc(&mate.housing)));
            if let Some(c) = &mate.contact {
                body.push_str(&format!("<text x=\"{x:.1}\" y=\"{:.1}\" font-size=\"11\">{} × {}</text>\n", under + 14.0, esc(c), h.wires.len()));
            }
            if !mate.known {
                body.push_str(&format!("<text x=\"{x:.1}\" y=\"{:.1}\" font-size=\"11\" fill=\"#c0392b\">型番はデータシートで選んでください</text>\n", under + 28.0));
            }
            for (i, w) in h.wires.iter().enumerate() {
                let row = if left { i } else { row_of_b(i) };
                let y = top + 8.0 + (row as f64 + 0.5) * ROW;
                let (pin, net) = if left { (&w.pin_a, &w.net_a) } else { (&w.pin_b, &w.net_b) };
                let net = net.clone().unwrap_or_else(|| "(空き)".into());
                let (px, nx, anchor) = if left { (x + bw - 10.0, x + 10.0, "start") } else { (x + 10.0, x + bw - 10.0, "end") };
                let pin_anchor = if left { "end" } else { "start" };
                body.push_str(&format!("<text x=\"{px:.1}\" y=\"{:.1}\" font-size=\"12\" text-anchor=\"{pin_anchor}\">{}</text>\n", y + 4.0, esc(pin)));
                body.push_str(&format!("<text x=\"{nx:.1}\" y=\"{:.1}\" font-size=\"12\" text-anchor=\"{anchor}\" fill=\"#333\">{}</text>\n", y + 4.0, esc(&net)));
            }
        }
        // the wires, a dark edge under each so that white and yellow show
        let (x1, x2) = (ax + bw, bx);
        for (i, w) in h.wires.iter().enumerate() {
            let ya = top + 8.0 + (i as f64 + 0.5) * ROW;
            let j = row_of_b(i);
            let yb = top + 8.0 + (j as f64 + 0.5) * ROW;
            let d = format!("M{x1:.1},{ya:.1} C{:.1},{ya:.1} {:.1},{yb:.1} {x2:.1},{yb:.1}", x1 + 80.0, x2 - 80.0);
            body.push_str(&format!("<path d=\"{d}\" fill=\"none\" stroke=\"#202020\" stroke-width=\"6\"/>\n"));
            body.push_str(&format!("<path d=\"{d}\" fill=\"none\" stroke=\"{}\" stroke-width=\"4\"/>\n", color_hex(&w.color)));
        }
        // the length between the boxes
        let ly = top - 14.0;
        body.push_str(&format!("<line x1=\"{x1:.1}\" y1=\"{ly:.1}\" x2=\"{x2:.1}\" y2=\"{ly:.1}\" stroke=\"#555\" marker-start=\"url(#arr)\" marker-end=\"url(#arr)\"/>\n"));
        body.push_str(&format!("<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"12\" text-anchor=\"middle\" fill=\"#555\">{:.0} mm</text>\n", (x1 + x2) / 2.0, ly - 4.0, h.length_mm));
        // the wire list
        let tx = 620.0;
        let cols = [("線", 0.0), ("色", 30.0), ("A", 90.0), ("B", 140.0), ("信号", 190.0)];
        for (t, dx) in cols {
            body.push_str(&format!("<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"12\" font-weight=\"bold\">{t}</text>\n", tx + dx, top + 4.0));
        }
        for (i, w) in h.wires.iter().enumerate() {
            let y = top + 4.0 + (i as f64 + 1.0) * ROW * 0.85;
            body.push_str(&format!("<rect x=\"{:.1}\" y=\"{:.1}\" width=\"14\" height=\"10\" fill=\"{}\" stroke=\"#202020\"/>\n", tx + 30.0, y - 9.0, color_hex(&w.color)));
            let cells = [(w.n.to_string(), 0.0), (color_name(&w.color).to_string(), 48.0), (w.pin_a.clone(), 90.0), (w.pin_b.clone(), 140.0), (w.signal(), 190.0)];
            for (t, dx) in cells {
                body.push_str(&format!("<text x=\"{:.1}\" y=\"{y:.1}\" font-size=\"12\">{}</text>\n", tx + dx, esc(&t)));
            }
        }
        let table = 4.0 + (n + 1.0) * ROW * 0.85;
        y0 = top + height.max(table) + 64.0;
    }
    if hs.is_empty() {
        body.push_str("<text x=\"20\" y=\"40\" font-size=\"14\">ケーブルでつなぐつなぎがありません</text>\n");
        y0 = 60.0;
    }
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{W}\" height=\"{y0:.0}\" viewBox=\"0 0 {W} {y0:.0}\" font-family=\"sans-serif\">\n<defs><marker id=\"arr\" viewBox=\"0 0 10 10\" refX=\"5\" refY=\"5\" markerWidth=\"6\" markerHeight=\"6\" orient=\"auto-start-reverse\"><path d=\"M0,0 L10,5 L0,10 z\" fill=\"#555\"/></marker></defs>\n<rect width=\"100%\" height=\"100%\" fill=\"white\"/>\n{body}</svg>\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::LinkEnd;
    use crate::{Kind, Label, Wire};

    /// A schematic with header J1 (footprint `fp`) whose pins are wired to labels `names` (empty: left open).
    fn board(fp: &str, names: &[&str]) -> Schematic {
        let mut s = Schematic::default();
        let i = s.add(Kind::Header(names.len() as u8), (10, 10), 0);
        s.components[i].refdes = "J1".into();
        s.components[i].footprint = fp.into();
        let pins = s.components[i].pins();
        for ((_, at), n) in pins.iter().zip(names) {
            if n.is_empty() {
                continue;
            }
            let end = (at.0 - 3, at.1);
            s.wires.push(Wire { a: *at, b: end });
            s.labels.push(Label { pos: end, text: n.to_string() });
        }
        s
    }

    fn cable(mm: Option<f64>) -> BoardLink {
        BoardLink { a: LinkEnd { design: "main".into(), refdes: "J1".into() }, b: LinkEnd { design: "sensor".into(), refdes: "J1".into() }, cable: mm, ..Default::default() }
    }

    /// A 4-wire cable of pin headers: Dupont housings and female contacts at both ends, ground black, power red, the signals yellow and
    /// green; an open pin has no wire; a link without a cable is not a harness; a colour chosen by hand wins.
    #[test]
    fn a_cable_has_its_wires_colours_and_mating_parts() {
        let main = board("PinHeader_1x5", &["+5V", "GND", "SDA", "SCL", ""]);
        let sensor = board("PinHeader_1x5", &["+5V", "GND", "SDA", "SCL", ""]);
        let designs = [("main", &main), ("sensor", &sensor)];
        assert!(harnesses(&designs, &[cable(None)], &[1, 1]).is_empty());
        let hs = harnesses(&designs, &[cable(Some(120.0))], &[1, 3]);
        assert_eq!(hs.len(), 1);
        let h = &hs[0];
        assert_eq!((h.name.as_str(), h.copies, h.wires.len()), ("W1", 3, 4), "{h:?}");
        let colors: Vec<&str> = h.wires.iter().map(|w| w.color.as_str()).collect();
        assert_eq!(colors, ["red", "black", "yellow", "green"]);
        assert!(h.mate_a.family.contains("デュポン") && h.mate_a.contact.as_deref().is_some_and(|c| c.contains("メス")) && h.mate_a.housing.contains("1×5"), "{:?}", h.mate_a);
        let mut l = cable(Some(120.0));
        l.colors.push(("3".into(), "blue".into()));
        let h = &harnesses(&designs, &[l], &[1, 1])[0];
        assert!(h.wires[2].color == "blue" && h.wires[2].chosen);
        // the parts list: two housings, 4 contacts at each end of each of 3 cables, 4 wires of 120 mm three times
        let bom = harness_bom(&hs);
        let find = |p: &str, v: &str| bom.iter().find(|r| r.part == p && r.value.contains(v)).unwrap_or_else(|| panic!("{p} {v}: {bom:?}"));
        assert_eq!(find("ハウジング", "1×5").total, 6);
        assert_eq!(find("コンタクト", "メス").total, 24);
        let red = find("電線", "赤");
        assert!(red.total == 3 && red.note.contains("360 mm"), "{red:?}");
        // the wire list and the drawing name every wire
        let csv = harness_csv(&hs);
        assert!(csv.starts_with('\u{feff}') && csv.lines().count() == 5 && csv.contains("W1,3,1,赤,26,120,main.J1,1,+5V,sensor.J1,1,+5V"), "{csv}");
        let svg = harness_svg(&hs);
        assert!(svg.starts_with("<svg") && svg.contains("SCL") && svg.contains(color_hex("green")) && svg.contains("120 mm"), "{svg}");
    }

    /// A crossed cable keeps its pairs (wire 3 goes from pin 3 to pin 4), a 2-row header pair is a flat cable with a wire on every pin
    /// (red first, then grey) and no contacts, and JST XH is told from its footprint.
    #[test]
    fn crossed_flat_and_jst_cables() {
        let main = board("PinHeader_1x4", &["+5V", "GND", "SDA", "SCL"]);
        let crossed = board("PinHeader_1x4", &["+5V", "GND", "SCL", "SDA"]);
        let mut l = cable(Some(100.0));
        l.pairs = vec![("1".into(), "1".into()), ("2".into(), "2".into()), ("3".into(), "4".into()), ("4".into(), "3".into())];
        let h = &harnesses(&[("main", &main), ("sensor", &crossed)], &[l], &[1, 1])[0];
        assert_eq!((h.wires[2].pin_a.as_str(), h.wires[2].pin_b.as_str(), h.wires[2].signal()), ("3", "4", "SDA".to_string()));
        let a = board("PinHeader_2x3", &["+5V", "GND", "MOSI", "", "SCK", ""]);
        let b = board("PinHeader_2x3", &["+5V", "GND", "MOSI", "", "SCK", ""]);
        let h = &harnesses(&[("main", &a), ("sensor", &b)], &[cable(Some(150.0))], &[1, 1])[0];
        assert!(h.ribbon && h.wires.len() == 6 && h.mate_a.contact.is_none(), "{h:?}");
        assert_eq!(h.wires.iter().map(|w| w.color.as_str()).collect::<Vec<_>>(), ["red", "grey", "grey", "grey", "grey", "grey"]);
        let bom = harness_bom(std::slice::from_ref(h));
        assert!(bom.iter().any(|r| r.part == "フラットケーブル") && bom.iter().all(|r| r.part != "コンタクト"), "{bom:?}");
        let xh = mating("JST_XH_B4B-XH-A_1x04_P2.50mm_Vertical", "", 4);
        assert_eq!((xh.housing.as_str(), xh.contact.as_deref()), ("XHP-4", Some("SXH-001T-P0.6")));
        assert!(!mating("Molex_KK_254", "", 3).known);
    }
}
