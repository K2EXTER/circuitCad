//! Reading a KiCad schematic (`.kicad_sch`, file formats 6 to 9): which parts it has and which of their pins are joined.
//!
//! Only the connections are taken, not the drawing: KiCad's symbols and ours do not share a shape, so the parts are
//! placed afresh and tied together by net labels (see the builder that turns this into a `Schematic`). The nets are
//! worked out from the geometry the way KiCad does:
//!   * a wire joins its two ends; anything that sits on a wire end, or on the inside of a wire, is on that wire
//!     (a pin, a label, a junction dot, the end of another wire: a T);
//!   * two wires that merely cross are NOT joined, unless a junction dot sits on the crossing;
//!   * a label names the wire it sits on, and labels (local, global, hierarchical) and power symbols of the same name
//!     are one net.
//!
//! What is not read is listed in [`KicadSch::notes`] (child sheets, buses ...), so nothing is dropped silently.

use crate::kicad::{parse_sexpr, symbol_of, KicadSymbol, Node};
use std::collections::{BTreeMap, HashMap};

/// A point in micrometres (KiCad writes millimetres with a few decimals).
type P = (i64, i64);

fn um(v: f64) -> i64 {
    (v * 1000.0).round() as i64
}

/// One pin of a placed part.
#[derive(Clone, Debug, PartialEq)]
pub struct SchPin {
    pub number: String,
    pub name: String,
    /// Where the pin connects, in micrometres on KiCad's sheet.
    pub at: (i64, i64),
}

/// A part of the schematic. The units (gates) of one KiCad part are merged: their pins are all here.
#[derive(Clone, Debug, PartialEq)]
pub struct SchPart {
    pub reference: String,
    pub value: String,
    /// `Library:Name`, as placed.
    pub lib_id: String,
    /// The key of this part's symbol in [`KicadSch::symbols`].
    pub symbol: String,
    pub footprint: String,
    /// Not fitted ("do not populate").
    pub dnp: bool,
    /// All the other fields (`Spice_Model`, `Spice_Primitive`, `Datasheet` ...), empty ones left out.
    pub fields: BTreeMap<String, String>,
    pub pins: Vec<SchPin>,
}

/// A net with at least one part pin on it.
#[derive(Clone, Debug, PartialEq)]
pub struct SchNet {
    /// The name from a label or a power symbol, or `N001`, `N002` ... when the net has none.
    pub name: String,
    pub named: bool,
    /// (reference, pin number), in the order of the parts.
    pub pins: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct KicadSch {
    pub title: String,
    pub parts: Vec<SchPart>,
    pub nets: Vec<SchNet>,
    /// The embedded library symbols of the placed parts, by the name used in [`SchPart::symbol`].
    pub symbols: BTreeMap<String, KicadSymbol>,
    /// Pins marked "no connect" (the X), as (reference, pin number).
    pub no_connects: Vec<(String, String)>,
    /// What was found but not read, in Japanese, for the person who opens the file.
    pub notes: Vec<String>,
}

// ---------------------------------------------------------------- library symbols

struct LibPin {
    number: String,
    name: String,
    x: f64,
    y: f64,
    unit: u32,
    style: u32,
}

struct LibSym {
    pins: Vec<LibPin>,
    power: bool,
}

/// `NAME_<unit>_<style>` -> (unit, style); anything else -> (0, 0).
fn unit_style(name: &str) -> (u32, u32) {
    let mut it = name.rsplit('_');
    let (style, unit) = (it.next().and_then(|s| s.parse().ok()), it.next().and_then(|s| s.parse().ok()));
    match (unit, style) {
        (Some(u), Some(s)) => (u, s),
        _ => (0, 0),
    }
}

fn collect_lib_pins(sym: &Node, unit: u32, style: u32, out: &mut Vec<LibPin>) {
    for n in sym.items().iter().skip(1) {
        match n.head() {
            Some("pin") => {
                let (Some(at), Some(num)) = (n.find("at"), n.find("number").and_then(|x| x.arg(0))) else { continue };
                let name = n.find("name").and_then(|x| x.arg(0)).unwrap_or("~");
                let name = if name == "~" || name.is_empty() { num } else { name };
                out.push(LibPin { number: num.trim().to_string(), name: name.to_string(), x: at.num(0).unwrap_or(0.0), y: at.num(1).unwrap_or(0.0), unit, style });
            }
            Some("symbol") => {
                let (u, s) = unit_style(n.arg(0).unwrap_or(""));
                collect_lib_pins(n, u, s, out);
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------- geometry

/// Where a pin of a library symbol (millimetres, y up) lands on the sheet (y down) for a symbol placed at `at`, turned
/// `rot` degrees counter-clockwise and mirrored. KiCad mirrors first and turns after.
fn place(px: f64, py: f64, at: (f64, f64), rot: f64, mirror: &str) -> P {
    let (mut x, mut y) = (px, py);
    match mirror {
        "x" => y = -y,
        "y" => x = -x,
        _ => {}
    }
    let (rx, ry) = match ((rot.rem_euclid(360.0) / 90.0).round() as i64) % 4 {
        0 => (x, y),
        1 => (-y, x),
        2 => (-x, -y),
        _ => (y, -x),
    };
    (um(at.0 + rx), um(at.1 - ry))
}

/// `p` lies on the segment `a`-`b`, not at an end.
fn inside(p: P, a: P, b: P) -> bool {
    if p == a || p == b {
        return false;
    }
    let cross = (b.0 - a.0) as i128 * (p.1 - a.1) as i128 - (b.1 - a.1) as i128 * (p.0 - a.0) as i128;
    cross == 0 && p.0 >= a.0.min(b.0) && p.0 <= a.0.max(b.0) && p.1 >= a.1.min(b.1) && p.1 <= a.1.max(b.1)
}

struct Dsu(Vec<usize>);

impl Dsu {
    fn new() -> Self {
        Dsu(vec![])
    }
    fn add(&mut self) -> usize {
        self.0.push(self.0.len());
        self.0.len() - 1
    }
    fn find(&mut self, i: usize) -> usize {
        let mut r = i;
        while self.0[r] != r {
            r = self.0[r];
        }
        let mut c = i;
        while self.0[c] != r {
            let n = self.0[c];
            self.0[c] = r;
            c = n;
        }
        r
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[rb] = ra;
        }
    }
}

fn xy(n: &Node) -> Option<(f64, f64)> {
    let at = n.find("at")?;
    Some((at.num(0)?, at.num(1)?))
}

fn property<'a>(n: &'a Node, key: &str) -> Option<&'a str> {
    n.find_all("property").find(|p| p.arg(0) == Some(key)).and_then(|p| p.arg(1))
}

// ---------------------------------------------------------------- the schematic

pub fn parse_schematic(text: &str) -> Result<KicadSch, String> {
    let root = parse_sexpr(text)?;
    if root.head() != Some("kicad_sch") {
        return Err("KiCad の回路図 (.kicad_sch) ではありません。KiCad 6 以降で保存した .kicad_sch を選んでください".into());
    }
    let mut out = KicadSch::default();
    if let Some(t) = root.find("title_block").and_then(|t| t.find("title")).and_then(|t| t.arg(0)) {
        out.title = t.to_string();
    }

    // the symbols the file carries with it
    let mut libs: HashMap<String, LibSym> = HashMap::new();
    let mut lib_nodes: Vec<&Node> = vec![];
    if let Some(l) = root.find("lib_symbols") {
        lib_nodes = l.find_all("symbol").collect();
        for s in &lib_nodes {
            let Some(name) = s.arg(0) else { continue };
            let mut pins = vec![];
            collect_lib_pins(s, 0, 0, &mut pins);
            libs.insert(name.to_string(), LibSym { pins, power: s.find("power").is_some() });
        }
    }

    // points of the sheet: each distinct place gets a number, and the wires and names join them
    let mut dsu = Dsu::new();
    let mut points: HashMap<P, usize> = HashMap::new();
    let mut point = |dsu: &mut Dsu, p: P| *points.entry(p).or_insert_with(|| dsu.add());
    let mut names: HashMap<String, usize> = HashMap::new();
    // (net name, point, rank): power symbols name a net before a global label does, and a global label before a local one
    let mut named_at: Vec<(String, usize, u8)> = vec![];

    let mut wires: Vec<(P, P)> = vec![];
    for w in root.find_all("wire") {
        let pts: Vec<(f64, f64)> = w.find("pts").map(|p| p.find_all("xy").filter_map(|q| Some((q.num(0)?, q.num(1)?))).collect()).unwrap_or_default();
        if pts.len() >= 2 {
            let (a, b) = ((um(pts[0].0), um(pts[0].1)), (um(pts[1].0), um(pts[1].1)));
            let (ia, ib) = (point(&mut dsu, a), point(&mut dsu, b));
            dsu.union(ia, ib);
            wires.push((a, b));
        }
    }
    for j in root.find_all("junction") {
        if let Some((x, y)) = xy(j) {
            point(&mut dsu, (um(x), um(y)));
        }
    }

    // parts
    let mut parts: Vec<SchPart> = vec![];
    let mut pin_points: Vec<(usize, String, String)> = vec![]; // (point, reference, number)
    for s in root.find_all("symbol") {
        let Some(lib_id) = s.find("lib_id").and_then(|l| l.arg(0)) else { continue };
        let embedded = s.find("lib_name").and_then(|l| l.arg(0)).unwrap_or(lib_id);
        let Some(lib) = libs.get(embedded).or_else(|| libs.get(lib_id)).or_else(|| libs.get(lib_id.rsplit(':').next().unwrap_or(lib_id))) else {
            out.notes.push(format!("記号 {lib_id} の定義がファイルの中に見つからないので、読み飛ばしました"));
            continue;
        };
        let Some(at) = xy(s) else { continue };
        let rot = s.find("at").and_then(|a| a.num(2)).unwrap_or(0.0);
        let mirror = s.find("mirror").and_then(|m| m.arg(0)).unwrap_or("");
        let unit: u32 = s.find("unit").and_then(|u| u.arg(0)).and_then(|u| u.parse().ok()).unwrap_or(1);
        let reference = property(s, "Reference").unwrap_or("").to_string();
        let value = property(s, "Value").unwrap_or("").to_string();
        let pins: Vec<SchPin> = lib
            .pins
            .iter()
            .filter(|p| (p.unit == 0 || p.unit == unit) && p.style <= 1)
            .map(|p| SchPin { number: p.number.clone(), name: p.name.clone(), at: place(p.x, p.y, at, rot, mirror) })
            .collect();
        if lib.power || reference.starts_with('#') {
            // a power symbol is a name for the net its pin sits on
            if !value.is_empty() && !reference.starts_with("#FLG") {
                for p in &pins {
                    let id = point(&mut dsu, p.at);
                    named_at.push((value.clone(), id, 0));
                }
            }
            continue;
        }
        for p in &pins {
            let id = point(&mut dsu, p.at);
            pin_points.push((id, reference.clone(), p.number.clone()));
        }
        // the units of one part are one part here
        if let Some(existing) = parts.iter_mut().find(|q| q.reference == reference && !reference.is_empty()) {
            for p in pins {
                if !existing.pins.iter().any(|q| q.number == p.number) {
                    existing.pins.push(p);
                }
            }
            continue;
        }
        let mut fields = BTreeMap::new();
        for p in s.find_all("property") {
            if let (Some(k), Some(v)) = (p.arg(0), p.arg(1))
                && !v.is_empty()
                && !matches!(k, "Reference" | "Value" | "Footprint")
            {
                fields.insert(k.to_string(), v.to_string());
            }
        }
        parts.push(SchPart {
            reference,
            value,
            lib_id: lib_id.to_string(),
            symbol: embedded.to_string(),
            footprint: property(s, "Footprint").unwrap_or("").to_string(),
            dnp: s.find("dnp").and_then(|d| d.arg(0)) == Some("yes"),
            fields,
            pins,
        });
    }

    // labels
    for (kind, rank) in [("global_label", 1u8), ("label", 2), ("hierarchical_label", 2)] {
        for l in root.find_all(kind) {
            if let (Some(name), Some((x, y))) = (l.arg(0), xy(l)) {
                let id = point(&mut dsu, (um(x), um(y)));
                named_at.push((name.to_string(), id, rank));
            }
        }
    }

    // what sits on the inside of a wire is on the wire
    let all: Vec<(P, usize)> = points.iter().map(|(p, i)| (*p, *i)).collect();
    for (a, b) in &wires {
        let wa = points[a];
        for (p, i) in &all {
            if inside(*p, *a, *b) {
                dsu.union(wa, *i);
            }
        }
    }
    // labels and power symbols of one name are one net
    for (name, id, _) in &named_at {
        let n = *names.entry(name.clone()).or_insert_with(|| dsu.add());
        dsu.union(n, *id);
    }

    // group the pins by net
    let mut groups: Vec<(usize, Vec<(String, String)>)> = vec![];
    for (id, reference, number) in &pin_points {
        let root_id = dsu.find(*id);
        match groups.iter_mut().find(|g| g.0 == root_id) {
            Some(g) => g.1.push((reference.clone(), number.clone())),
            None => groups.push((root_id, vec![(reference.clone(), number.clone())])),
        }
    }
    let mut auto = 0;
    for (root_id, pins) in groups {
        // the name: a power symbol first, then a global label, then a local one (the first of that kind in the file)
        let mut candidates: Vec<(u8, usize, &String)> = vec![];
        for (k, (name, id, rank)) in named_at.iter().enumerate() {
            if dsu.find(*id) == root_id {
                candidates.push((*rank, k, name));
            }
        }
        candidates.sort();
        let named = !candidates.is_empty();
        let name = match candidates.first() {
            Some((_, _, n)) => (*n).clone(),
            None => {
                auto += 1;
                format!("N{auto:03}")
            }
        };
        let mut distinct: Vec<&String> = candidates.iter().map(|c| c.2).collect();
        distinct.dedup();
        let distinct: std::collections::BTreeSet<&String> = distinct.into_iter().collect();
        if distinct.len() > 1 {
            out.notes.push(format!("名前の違うラベル ({}) が同じ配線についています。{name} として読みました", distinct.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("・")));
        }
        out.nets.push(SchNet { name, named, pins });
    }

    // the "no connect" crosses
    for n in root.find_all("no_connect") {
        if let Some((x, y)) = xy(n) {
            let p = (um(x), um(y));
            for part in &parts {
                for pin in part.pins.iter().filter(|q| q.at == p) {
                    out.no_connects.push((part.reference.clone(), pin.number.clone()));
                }
            }
        }
    }

    // the embedded symbols of the parts that are placed
    let all_syms: Vec<&Node> = lib_nodes;
    for part in &parts {
        if let Some(node) = all_syms.iter().find(|n| n.arg(0) == Some(part.symbol.as_str()))
            && let Some(sym) = symbol_of(&all_syms, node, 0)
        {
            out.symbols.insert(part.symbol.clone(), sym);
        }
    }

    // what is not read
    let count = |name: &str| root.find_all(name).count();
    for sheet in root.find_all("sheet") {
        let name = property(sheet, "Sheetname").or_else(|| property(sheet, "Sheet name")).unwrap_or("(名前なし)");
        out.notes.push(format!("子シート「{name}」は読み込めません (階層のシートは未対応です)。その中の部品は入っていません"));
    }
    if count("bus") + count("bus_entry") + count("bus_alias") > 0 {
        out.notes.push("バス (太い線・バスの引き出し) は読み込めません。バスでつながるはずの信号は、つながっていません".into());
    }
    out.parts = parts;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small sheet that uses each rule of the connection: a turned part, a mirrored one, a T, a plain crossing, a
    /// power symbol, a label of the same name, a part with two units, and a no-connect cross.
    const SHEET: &str = r##"(kicad_sch (version 20231120) (generator "eeschema")
  (title_block (title "Test sheet"))
  (lib_symbols
    (symbol "Device:R" (pin_numbers hide) (in_bom yes) (on_board yes)
      (property "Reference" "R" (at 2 0 90))
      (property "Value" "R" (at 0 0 90))
      (symbol "R_1_1"
        (pin passive line (at 0 3.81 270) (length 1.27) (name "~" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
        (pin passive line (at 0 -3.81 90) (length 1.27) (name "~" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))))
    (symbol "power:GND" (power) (pin_names (offset 0) hide)
      (property "Reference" "#PWR" (at 0 -6 0))
      (property "Value" "GND" (at 0 -3.8 0))
      (symbol "GND_1_1"
        (pin power_in line (at 0 0 270) (length 0) hide (name "GND" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))))
    (symbol "74xx:Quad" (in_bom yes) (on_board yes)
      (property "Reference" "U" (at 0 0 0))
      (property "Value" "Quad" (at 0 0 0))
      (symbol "Quad_1_1"
        (pin input line (at -5 0 0) (length 2) (name "A" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
        (pin output line (at 5 0 180) (length 2) (name "Y" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27))))))
      (symbol "Quad_2_1"
        (pin input line (at -5 0 0) (length 2) (name "A" (effects (font (size 1.27 1.27)))) (number "4" (effects (font (size 1.27 1.27)))))
        (pin output line (at 5 0 180) (length 2) (name "Y" (effects (font (size 1.27 1.27)))) (number "5" (effects (font (size 1.27 1.27)))))))
  )
  (symbol (lib_id "Device:R") (at 100 100 0) (unit 1)
    (property "Reference" "R1" (at 0 0 0)) (property "Value" "10k" (at 0 0 0)) (property "Footprint" "Resistor_SMD:R_0603_1608Metric" (at 0 0 0)))
  (symbol (lib_id "Device:R") (at 120 100 90) (unit 1)
    (property "Reference" "R2" (at 0 0 0)) (property "Value" "1k" (at 0 0 0)))
  (symbol (lib_id "Device:R") (at 108 86.19 0) (unit 1)
    (property "Reference" "R3" (at 0 0 0)) (property "Value" "2k" (at 0 0 0)))
  (symbol (lib_id "Device:R") (at 112 111.81 0) (unit 1)
    (property "Reference" "R5" (at 0 0 0)) (property "Value" "5k" (at 0 0 0)))
  (symbol (lib_id "Device:R") (at 140 100 0) (mirror x) (unit 1)
    (property "Reference" "R6" (at 0 0 0)) (property "Value" "6k" (at 0 0 0)))
  (symbol (lib_id "power:GND") (at 123.81 100 0) (unit 1)
    (property "Reference" "#PWR01" (at 0 0 0)) (property "Value" "GND" (at 0 0 0)))
  (symbol (lib_id "74xx:Quad") (at 200 100 0) (unit 1)
    (property "Reference" "U1" (at 0 0 0)) (property "Value" "Quad" (at 0 0 0)))
  (symbol (lib_id "74xx:Quad") (at 200 120 0) (unit 2)
    (property "Reference" "U1" (at 0 0 0)) (property "Value" "Quad" (at 0 0 0)))
  (wire (pts (xy 100 103.81) (xy 116.19 103.81)))
  (wire (pts (xy 116.19 103.81) (xy 116.19 100)))
  (wire (pts (xy 108 90) (xy 108 103.81)))
  (wire (pts (xy 112 100) (xy 112 108)))
  (label "GND" (at 100 96.19 0))
  (no_connect (at 140 103.81))
  (sheet (at 10 10) (size 20 20) (property "Sheetname" "Power" (at 0 0 0)))
)"##;

    fn net_of<'a>(s: &'a KicadSch, r: &str, n: &str) -> &'a SchNet {
        s.nets.iter().find(|x| x.pins.iter().any(|p| p.0 == r && p.1 == n)).unwrap_or_else(|| panic!("{r}.{n} is on no net"))
    }

    #[test]
    fn parts_and_pins_are_read_with_their_fields() {
        let s = parse_schematic(SHEET).unwrap();
        assert_eq!(s.title, "Test sheet");
        let r1 = s.parts.iter().find(|p| p.reference == "R1").unwrap();
        assert_eq!((r1.value.as_str(), r1.lib_id.as_str(), r1.footprint.as_str()), ("10k", "Device:R", "Resistor_SMD:R_0603_1608Metric"));
        assert_eq!(r1.pins.iter().map(|p| p.at).collect::<Vec<_>>(), vec![(100_000, 96_190), (100_000, 103_810)], "pin 1 above, pin 2 below an unturned part");
        assert!(!s.parts.iter().any(|p| p.reference.starts_with('#')), "a power symbol is not a part");
        assert!(s.symbols.contains_key("Device:R"));
    }

    #[test]
    fn a_part_turned_a_quarter_counter_clockwise_has_pin_1_on_the_left() {
        let s = parse_schematic(SHEET).unwrap();
        let r2 = s.parts.iter().find(|p| p.reference == "R2").unwrap();
        assert_eq!(r2.pins[0].at, (116_190, 100_000));
        assert_eq!(r2.pins[1].at, (123_810, 100_000));
    }

    #[test]
    fn a_mirrored_part_has_its_pins_swapped_top_to_bottom() {
        let s = parse_schematic(SHEET).unwrap();
        let r6 = s.parts.iter().find(|p| p.reference == "R6").unwrap();
        assert_eq!(r6.pins[0].at, (140_000, 103_810), "pin 1 is below");
        assert_eq!(r6.pins[1].at, (140_000, 96_190));
        assert_eq!(s.no_connects, vec![("R6".to_string(), "1".to_string())], "the cross sits on R6 pin 1");
    }

    #[test]
    fn wires_join_at_their_ends_and_a_wire_end_on_a_wire_is_a_t() {
        let s = parse_schematic(SHEET).unwrap();
        let n = net_of(&s, "R1", "2");
        assert!(n.pins.contains(&("R2".into(), "1".into())), "R1.2 - wire - corner - R2.1");
        assert!(n.pins.contains(&("R3".into(), "2".into())), "the wire from R3 ends on the middle of the first wire: a T");
        assert!(!n.named);
        assert_eq!(n.name, "N001");
    }

    #[test]
    fn wires_that_only_cross_are_not_joined() {
        let s = parse_schematic(SHEET).unwrap();
        let alone = net_of(&s, "R5", "1");
        assert_eq!(alone.pins.len(), 1, "the wire from R5 crosses the first wire without a junction: {alone:?}");
    }

    #[test]
    fn a_power_symbol_and_a_label_of_the_same_name_are_one_net() {
        let s = parse_schematic(SHEET).unwrap();
        let g = net_of(&s, "R2", "2");
        assert_eq!(g.name, "GND");
        assert!(g.named);
        assert!(g.pins.contains(&("R1".into(), "1".into())), "the label GND on R1 pin 1 joins the GND symbol's net");
    }

    #[test]
    fn the_units_of_one_part_are_one_part() {
        let s = parse_schematic(SHEET).unwrap();
        let u: Vec<&SchPart> = s.parts.iter().filter(|p| p.reference == "U1").collect();
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].pins.iter().map(|p| p.number.as_str()).collect::<Vec<_>>(), vec!["1", "2", "4", "5"]);
    }

    #[test]
    fn what_is_not_read_is_said() {
        let s = parse_schematic(SHEET).unwrap();
        assert!(s.notes.iter().any(|n| n.contains("Power") && n.contains("子シート")), "{:?}", s.notes);
    }

    #[test]
    fn a_file_that_is_not_a_schematic_is_refused_with_the_reason() {
        let e = parse_schematic("(kicad_symbol_lib (version 1))").unwrap_err();
        assert!(e.contains(".kicad_sch"), "{e}");
        assert!(parse_schematic("(kicad_sch (version 1)").unwrap_err().contains("閉じていません"));
    }

    /// Against a real sheet: the ngspice examples carry `ECL-OR.kicad_sch` and the netlist KiCad wrote for it
    /// (`ECL-OR.cir`). The parts on each net must be the same. (Skipped when the examples are not on this machine.)
    #[test]
    fn a_real_kicad_sheet_gives_the_nets_kicad_wrote() {
        let dir = std::path::Path::new(r"F:\Tools\ngspice\Spice64\examples\osdi\hicuml0");
        let (Ok(sch), Ok(cir)) = (std::fs::read_to_string(dir.join("KiCad").join("ECL-OR.kicad_sch")), std::fs::read_to_string(dir.join("ECL-OR.cir"))) else {
            return;
        };
        let s = parse_schematic(&sch).unwrap();
        // from KiCad's netlist: net -> the elements on it (R and V have two nodes, the X transistors have five: C B E S DT)
        let norm = |n: &str| if n == "0" { "GND".to_string() } else { n.to_ascii_uppercase() };
        let mut want: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for line in cir.lines() {
            let w: Vec<&str> = line.split_whitespace().collect();
            let Some(first) = w.first() else { continue };
            let n_nodes = match first.chars().next() {
                Some('R') | Some('V') => 2,
                Some('X') => 5,
                _ => continue,
            };
            let reference = first.trim_start_matches('X').to_string();
            for node in w.iter().skip(1).take(n_nodes) {
                want.entry(norm(node)).or_default().push(reference.clone());
            }
        }
        let mut got: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for n in &s.nets {
            let key = if n.named { norm(&n.name) } else { n.name.clone() };
            got.entry(key).or_default().extend(n.pins.iter().map(|p| p.0.clone()));
        }
        // compare as partitions: nets that KiCad named by a pin ("Net-_Q1-Pad1_") have another name here
        let canon = |m: &BTreeMap<String, Vec<String>>| -> Vec<Vec<String>> {
            let mut v: Vec<Vec<String>> = m.values().map(|r| { let mut r = r.clone(); r.sort(); r }).collect();
            v.sort();
            v
        };
        let (w, g) = (canon(&want), canon(&got));
        eprintln!("real sheet: {} parts, {} nets (KiCad: {} nets)", s.parts.len(), s.nets.len(), w.len());
        // the .cir lists the substrate and thermal nodes too; ours has every pin: compare the nets that have a part pin on both sides
        assert_eq!(w.len(), g.len(), "the same number of nets\nKiCad: {w:?}\nours:  {g:?}");
        assert_eq!(w, g, "the same parts on each net");
    }
}
