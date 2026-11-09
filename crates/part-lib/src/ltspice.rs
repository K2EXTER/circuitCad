//! Reading an LTspice schematic (`.asc`): which parts it has, where their pins are, and which pins are joined.
//!
//! Like the KiCad reader, only the connections are taken: the parts are placed afresh by the builder (`ltspice_import.rs`) and
//! tied together by net labels. The nets follow LTspice's drawing rules:
//!   * a wire joins its two ends; a wire end, pin or flag that sits on another wire (at its end or inside it) is on that wire;
//!   * wires that only cross are not joined;
//!   * a flag (`FLAG x y name`, also the `IOPIN` ports) names the net it sits on; flags of the same name are one net, and the
//!     flag `0` is ground.
//!
//! The pins of a symbol come from its `.asy` file (looked up by the caller: next to the `.asc`, or in LTspice's `lib/sym`).
//! The basic symbols (resistor, capacitor, coil, diode, sources, transistors) are built in, so a circuit of those reads even
//! without LTspice on the computer.

use std::collections::{BTreeMap, HashMap};

type P = (i64, i64);

/// The text of an LTspice file: UTF-16 (newer versions, with or without a byte order mark), UTF-8, or Latin-1 (the `µ` of older
/// versions).
pub fn decode(bytes: &[u8]) -> String {
    let utf16 = |b: &[u8]| String::from_utf16_lossy(&b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>());
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest);
    }
    // UTF-16 without a mark: every other byte of ASCII text is zero
    if bytes.len() >= 4 && bytes[1] == 0 && bytes[3] == 0 {
        return utf16(bytes);
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

/// The folders LTspice keeps its symbols in on this computer (the newer per-user place, then the older ones). Only the ones that exist.
pub fn ltspice_symbol_dirs() -> Vec<std::path::PathBuf> {
    let mut v = vec![];
    if let Ok(base) = std::env::var("LOCALAPPDATA") {
        v.push(std::path::Path::new(&base).join("LTspice").join("lib").join("sym"));
    }
    if let Ok(home) = std::env::var("USERPROFILE") {
        v.push(std::path::Path::new(&home).join("Documents").join("LTspiceXVII").join("lib").join("sym"));
        v.push(std::path::Path::new(&home).join("Documents").join("LTspice").join("lib").join("sym"));
    }
    for pf in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Ok(p) = std::env::var(pf) {
            v.push(std::path::Path::new(&p).join("LTC").join("LTspiceXVII").join("lib").join("sym"));
        }
    }
    v.into_iter().filter(|d| d.is_dir()).collect()
}

/// Every `.asy` under the folders, by its path from the folder (`opamps/lt1001`) and by its bare name (`lt1001`), in lower case.
/// The earlier folders win. LTspice finds a symbol named without its folder the same way.
pub fn symbol_index(dirs: &[std::path::PathBuf]) -> HashMap<String, std::path::PathBuf> {
    fn walk(root: &std::path::Path, dir: &std::path::Path, depth: usize, out: &mut HashMap<String, std::path::PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if depth < 6 {
                    walk(root, &p, depth + 1, out);
                }
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("asy")) {
                let rel = p.strip_prefix(root).unwrap_or(&p).with_extension("").to_string_lossy().replace('\\', "/").to_lowercase();
                let bare = rel.rsplit('/').next().unwrap_or(&rel).to_string();
                out.entry(rel).or_insert_with(|| p.clone());
                out.entry(bare).or_insert(p);
            }
        }
    }
    let mut out = HashMap::new();
    for d in dirs {
        walk(d, d, 0, &mut out);
    }
    out
}

/// Looks a symbol up in an index from [`symbol_index`]: by its path (`OpAmps\LT1001`), then by its bare name.
pub fn find_symbol(index: &HashMap<String, std::path::PathBuf>, name: &str) -> Option<String> {
    let rel = name.replace('\\', "/").to_lowercase();
    let bare = rel.rsplit('/').next().unwrap_or(&rel).to_string();
    let path = index.get(&rel).or_else(|| index.get(&bare))?;
    std::fs::read(path).ok().map(|b| decode(&b))
}

/// A pin of a symbol, relative to the symbol's origin before it is turned.
#[derive(Clone, Debug, PartialEq)]
pub struct SymPin {
    pub name: String,
    /// The pin's place in the SPICE line (1 = first node).
    pub order: usize,
    pub at: P,
}

/// What a symbol file (`.asy`) says about the symbol.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Symbol {
    pub prefix: String,
    pub pins: Vec<SymPin>,
    /// `SYMATTR Value` / `SpiceModel` / `Description` of the symbol (the defaults of a part placed from it).
    pub value: String,
    pub model: String,
    pub description: String,
}

pub fn parse_asy(text: &str) -> Symbol {
    let mut s = Symbol::default();
    for line in text.lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        match w.as_slice() {
            ["PIN", x, y, ..] => {
                if let (Ok(x), Ok(y)) = (x.parse(), y.parse()) {
                    let n = s.pins.len() + 1;
                    s.pins.push(SymPin { name: String::new(), order: n, at: (x, y) });
                }
            }
            ["PINATTR", "PinName", rest @ ..] => {
                if let Some(p) = s.pins.last_mut() {
                    p.name = rest.join(" ");
                }
            }
            ["PINATTR", "SpiceOrder", n] => {
                if let (Some(p), Ok(n)) = (s.pins.last_mut(), n.parse()) {
                    p.order = n;
                }
            }
            ["SYMATTR", key, rest @ ..] => {
                let v = rest.join(" ");
                match *key {
                    "Prefix" => s.prefix = v,
                    "Value" => s.value = v,
                    "SpiceModel" => s.model = v,
                    "Description" => s.description = v,
                    _ => {}
                }
            }
            _ => {}
        }
    }
    s
}

/// The basic symbols, so that a circuit of plain parts reads without LTspice's library (the same pins as its `.asy` files).
pub fn builtin_symbol(name: &str) -> Option<Symbol> {
    let pins = |list: &[(&str, i64, i64)]| list.iter().enumerate().map(|(i, &(n, x, y))| SymPin { name: n.into(), order: i + 1, at: (x, y) }).collect();
    let (prefix, p): (&str, Vec<SymPin>) = match name.to_ascii_lowercase().as_str() {
        "res" | "res2" => ("R", pins(&[("A", 16, 16), ("B", 16, 96)])),
        "cap" | "polcap" => ("C", pins(&[("A", 16, 0), ("B", 16, 64)])),
        "ind" | "ind2" => ("L", pins(&[("A", 16, 16), ("B", 16, 96)])),
        "diode" | "zener" | "schottky" | "led" | "varactor" => ("D", pins(&[("+", 16, 0), ("-", 16, 64)])),
        "voltage" => ("V", pins(&[("+", 0, 16), ("-", 0, 96)])),
        "current" => ("I", pins(&[("+", 0, 0), ("-", 0, 80)])),
        "npn" | "pnp" => ("Q", pins(&[("C", 64, 0), ("B", 0, 48), ("E", 64, 96)])),
        "nmos" | "pmos" => ("M", pins(&[("D", 48, 0), ("G", 0, 80), ("S", 48, 96)])),
        _ => return None,
    };
    Some(Symbol { prefix: prefix.into(), pins: p, ..Default::default() })
}

/// Where a pin at `p` (relative to the symbol) lands for a symbol at `at` turned by `rot` (`R0` `R90` `R180` `R270`, or
/// mirrored `M0` `M90` `M180` `M270`). LTspice's y axis points down; R90 turns clockwise on the screen.
pub fn place(at: P, rot: &str, p: P) -> P {
    let (x, y) = p;
    let (x, y) = match rot {
        "R90" => (-y, x),
        "R180" => (-x, -y),
        "R270" => (y, -x),
        "M0" => (-x, y),
        // a mirrored turn is the turn, then mirrored left to right
        "M90" => (y, x),
        "M180" => (x, -y),
        "M270" => (-y, -x),
        _ => (x, y),
    };
    (at.0 + x, at.1 + y)
}

/// A part of the schematic.
#[derive(Clone, Debug, PartialEq)]
pub struct AscPart {
    /// `InstName` (R1, V1, U1 ...).
    pub inst: String,
    /// The symbol as the file names it (`res`, `voltage`, `OpAmps\LT1001`).
    pub symbol: String,
    pub at: P,
    pub rot: String,
    /// `SYMATTR` lines: `Value`, `Value2`, `SpiceLine`, `SpiceLine2`, `SpiceModel` ...
    pub attrs: BTreeMap<String, String>,
}

impl AscPart {
    /// The last part of the symbol's name, without its folder (`OpAmps\LT1001` → `LT1001`).
    pub fn base_symbol(&self) -> &str {
        self.symbol.rsplit(['\\', '/']).next().unwrap_or(&self.symbol)
    }
    pub fn attr(&self, k: &str) -> &str {
        self.attrs.get(k).map(String::as_str).unwrap_or("")
    }
}

/// A read schematic: the parts with their pins placed, and the nets.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AscSch {
    pub parts: Vec<AscPart>,
    /// The symbol of each part, by the part's `inst` (missing when no `.asy` was found).
    pub symbols: HashMap<String, Symbol>,
    /// Nets with at least one pin: (name, named by a flag, [(inst, pin name)]). Unnamed nets are `N001`, `N002` ...
    pub nets: Vec<(String, bool, Vec<(String, String)>)>,
    /// SPICE directives from the sheet's text (`.tran 10m`, `.param R=1k`, `.lib ...`), one per line.
    pub directives: Vec<String>,
    /// What could not be read, in Japanese.
    pub notes: Vec<String>,
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

/// Is `p` on the segment a–b (wires are drawn at any angle, mostly level or upright)?
fn on_segment(p: P, a: P, b: P) -> bool {
    let cross = (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
    cross == 0 && p.0 >= a.0.min(b.0) && p.0 <= a.0.max(b.0) && p.1 >= a.1.min(b.1) && p.1 <= a.1.max(b.1)
}

/// Reads an `.asc`. `symbol_file` gives the text of a symbol's `.asy` by the name the file uses (None when it is not found);
/// the basic symbols are built in and used when it gives nothing.
pub fn parse_asc(text: &str, symbol_file: &dyn Fn(&str) -> Option<String>) -> Result<AscSch, String> {
    if !text.trim_start().starts_with("Version") {
        return Err("LTspice の回路図 (.asc) ではないようです (先頭に「Version」の行がありません)".into());
    }
    let mut out = AscSch::default();
    let mut wires: Vec<(P, P)> = vec![];
    let mut flags: Vec<(P, String)> = vec![];
    let mut skipped: BTreeMap<String, usize> = BTreeMap::new();
    let num = |s: &str| s.parse::<i64>().ok();
    for line in text.lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        match w.as_slice() {
            ["WIRE", a, b, c, d, ..] => {
                if let (Some(a), Some(b), Some(c), Some(d)) = (num(a), num(b), num(c), num(d)) {
                    wires.push(((a, b), (c, d)));
                }
            }
            ["FLAG", x, y, name @ ..] | ["IOPIN", x, y, name @ ..] if !name.is_empty() => {
                // IOPIN's last word is its direction (In / Out / BiDir), not part of the name
                let name = if w[0] == "IOPIN" { name[0].to_string() } else { name.join(" ") };
                if let (Some(x), Some(y)) = (num(x), num(y)) {
                    flags.push(((x, y), name));
                }
            }
            ["SYMBOL", sym, x, y, rot, ..] => {
                if let (Some(x), Some(y)) = (num(x), num(y)) {
                    out.parts.push(AscPart { inst: String::new(), symbol: sym.to_string(), at: (x, y), rot: rot.to_string(), attrs: BTreeMap::new() });
                }
            }
            ["SYMATTR", key, rest @ ..] => {
                if let Some(p) = out.parts.last_mut() {
                    let v = rest.join(" ");
                    if *key == "InstName" {
                        p.inst = v;
                    } else {
                        p.attrs.insert(key.to_string(), v);
                    }
                }
            }
            ["TEXT", _, _, _, _, rest @ ..] => {
                // the text after the size: "!" makes it a SPICE directive, ";" a comment; "\n" separates its lines
                let t = rest.join(" ");
                if let Some(d) = t.strip_prefix('!') {
                    out.directives.extend(d.split("\\n").map(|l| l.trim().to_string()).filter(|l| !l.is_empty() && !l.starts_with(';')));
                }
            }
            ["BUSTAP", ..] => *skipped.entry(w[0].to_string()).or_default() += 1,
            _ => {}
        }
    }

    // every point that matters: wire ends, flags, pins; joined by wires
    let mut points: Vec<P> = vec![];
    let mut index: HashMap<P, usize> = HashMap::new();
    let mut id = |p: P, points: &mut Vec<P>| -> usize {
        *index.entry(p).or_insert_with(|| {
            points.push(p);
            points.len() - 1
        })
    };
    for &(a, b) in &wires {
        id(a, &mut points);
        id(b, &mut points);
    }
    for (p, _) in &flags {
        id(*p, &mut points);
    }
    // the pins of each part
    let mut pin_points: Vec<(usize, String, usize)> = vec![]; // (part index, pin name, point id)
    let mut shorts: Vec<(usize, usize)> = vec![];
    let mut unnamed = 0;
    for (k, part) in out.parts.iter_mut().enumerate() {
        if part.inst.is_empty() {
            unnamed += 1;
            part.inst = format!("X{unnamed}");
        }
        let sym = symbol_file(&part.symbol).map(|t| parse_asy(&t)).filter(|s| !s.pins.is_empty()).or_else(|| builtin_symbol(part.base_symbol()));
        let Some(sym) = sym else {
            out.notes.push(format!("{} ({}): 記号のファイル (.asy) が見つからないので、ピンの位置が分からず、取り込めませんでした。.asc と同じフォルダに .asy を置くか、LTspice を入れてから取り込んでください。", part.inst, part.symbol));
            continue;
        };
        // a jumper is a piece of wire between its pins, not a part
        if part.base_symbol().eq_ignore_ascii_case("jumper") {
            let ends: Vec<usize> = sym.pins.iter().map(|pin| id(place(part.at, &part.rot, pin.at), &mut points)).collect();
            shorts.extend(ends.windows(2).map(|w| (w[0], w[1])));
            continue;
        }
        for pin in &sym.pins {
            let p = place(part.at, &part.rot, pin.at);
            pin_points.push((k, pin.name.clone(), id(p, &mut points)));
        }
        out.symbols.insert(part.inst.clone(), sym);
    }
    let mut parent: Vec<usize> = (0..points.len()).collect();
    let join = |a: usize, b: usize, parent: &mut Vec<usize>| {
        let (ra, rb) = (find(parent, a), find(parent, b));
        if ra != rb {
            parent[ra] = rb;
        }
    };
    for &(a, b) in &wires {
        let (ia, ib) = (index[&a], index[&b]);
        join(ia, ib, &mut parent);
        // anything sitting inside this wire is on it (a T, a pin or flag dropped on the wire)
        for (i, &p) in points.iter().enumerate() {
            if p != a && p != b && on_segment(p, a, b) {
                join(i, ia, &mut parent);
            }
        }
    }
    for &(a, b) in &shorts {
        join(a, b, &mut parent);
    }
    // flags of one name are one net
    let mut by_name: HashMap<String, usize> = HashMap::new();
    for (p, name) in &flags {
        let key = if name == "0" { "0".to_string() } else { name.to_ascii_uppercase() };
        let i = index[p];
        match by_name.get(&key) {
            Some(&j) => join(i, j, &mut parent),
            None => {
                by_name.insert(key, i);
            }
        }
    }
    let mut name_of: HashMap<usize, String> = HashMap::new();
    for (p, name) in &flags {
        let r = find(&mut parent, index[p]);
        name_of.entry(r).or_insert_with(|| if name == "0" { "GND".to_string() } else { name.clone() });
    }
    // nets in the order of the parts' pins
    let mut nets: Vec<(usize, Vec<(String, String)>)> = vec![];
    for (k, pin, pt) in &pin_points {
        let r = find(&mut parent, *pt);
        let entry = (out.parts[*k].inst.clone(), pin.clone());
        match nets.iter_mut().find(|(root, _)| *root == r) {
            Some((_, list)) => list.push(entry),
            None => nets.push((r, vec![entry])),
        }
    }
    let mut n = 0;
    for (r, pins) in nets {
        let (name, named) = match name_of.get(&r) {
            Some(s) => (s.clone(), true),
            None => {
                n += 1;
                (format!("N{n:03}"), false)
            }
        };
        out.nets.push((name, named, pins));
    }
    for (what, count) in skipped {
        out.notes.push(format!("{what} ({count} 個) は読みません (バスの扱いは、このソフトにありません)。"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A divider drawn the way LTspice writes it: V1 upright, R1 turned on its side (R90), R2 upright.
    pub(crate) const DIVIDER: &str = "Version 4
SHEET 1 880 680
WIRE 160 96 64 96
WIRE 256 96 240 96
WIRE 256 144 256 96
WIRE 64 128 64 96
WIRE 256 256 256 224
WIRE 64 256 64 208
FLAG 64 256 0
FLAG 256 256 0
FLAG 256 96 out
SYMBOL voltage 64 112 R0
SYMATTR InstName V1
SYMATTR Value 9
SYMBOL res 256 80 R90
WINDOW 0 0 56 VBottom 2
SYMATTR InstName R1
SYMATTR Value 4k7
SYMBOL res 240 128 R0
SYMATTR InstName R2
SYMATTR Value 4.7k
TEXT 64 296 Left 2 !.op\\n.param X=1
";

    #[test]
    fn a_divider_reads_with_its_nets() {
        let s = parse_asc(DIVIDER, &|_| None).unwrap();
        assert_eq!(s.parts.len(), 3);
        let net = |name: &str| {
            let mut v: Vec<String> = s.nets.iter().find(|n| n.0 == name).unwrap_or_else(|| panic!("no {name}: {:?}", s.nets)).2.iter().map(|(a, b)| format!("{a}.{b}")).collect();
            v.sort();
            v
        };
        assert_eq!(net("out"), ["R1.A", "R2.A"]);
        assert_eq!(net("GND"), ["R2.B", "V1.-"]);
        // V1+ and R1's other end: an unnamed net
        let first = s.nets.iter().find(|n| !n.1).unwrap();
        let mut v: Vec<String> = first.2.iter().map(|(a, b)| format!("{a}.{b}")).collect();
        v.sort();
        assert_eq!(v, ["R1.B", "V1.+"]);
        assert_eq!(s.directives, [".op", ".param X=1"]);
        assert!(s.notes.is_empty(), "{:?}", s.notes);
    }

    #[test]
    fn turns_and_mirrors_put_pins_where_ltspice_draws_them() {
        assert_eq!(place((100, 100), "R0", (16, 96)), (116, 196));
        assert_eq!(place((100, 100), "R90", (16, 96)), (4, 116));
        assert_eq!(place((100, 100), "R180", (16, 96)), (84, 4));
        assert_eq!(place((100, 100), "R270", (16, 96)), (196, 84));
        assert_eq!(place((100, 100), "M0", (16, 96)), (84, 196));
        // checked against LTspice's Fc.asc (a resistor at M90 whose ends meet its wires)
        assert_eq!(place((2352, -624), "M90", (16, 96)), (2448, -608));
    }

    #[test]
    fn the_text_is_read_in_any_of_ltspices_encodings() {
        let utf16: Vec<u8> = "Version 4\n".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(decode(&utf16), "Version 4\n");
        let mut bom = vec![0xFF, 0xFE];
        bom.extend(&utf16);
        assert_eq!(decode(&bom), "Version 4\n");
        assert_eq!(decode(b"10\xB5"), "10µ", "Latin-1 micro sign");
        assert!(parse_asc("hello", &|_| None).is_err());
    }

    #[test]
    fn a_symbol_file_gives_the_pins() {
        let s = parse_asy("Version 4\nSymbolType BLOCK\nPIN -32 16 NONE 0\nPINATTR PinName In+\nPINATTR SpiceOrder 1\nPIN 32 0 NONE 0\nPINATTR PinName OUT\nPINATTR SpiceOrder 5\nSYMATTR Prefix X\nSYMATTR Value LT1001\n");
        assert_eq!(s.prefix, "X");
        assert_eq!(s.pins, vec![SymPin { name: "In+".into(), order: 1, at: (-32, 16) }, SymPin { name: "OUT".into(), order: 5, at: (32, 0) }]);
        assert_eq!(s.value, "LT1001");
    }

    /// LTspice's examples, when LTspice is on this computer: with the symbols from its library, nearly every pin should sit on
    /// a wire, a flag or another pin (a pin left alone means the turn of the symbol was read wrong).
    #[test]
    fn ltspices_own_examples_have_their_pins_on_wires() {
        let Ok(base) = std::env::var("LOCALAPPDATA") else { return };
        let ex = std::path::Path::new(&base).join("LTspice/examples/Educational");
        let Ok(dir) = std::fs::read_dir(&ex) else { return };
        let index = symbol_index(&ltspice_symbol_dirs());
        let lookup = |name: &str| find_symbol(&index, name);
        let (mut pins, mut alone, mut files) = (0, 0, 0);
        for e in dir.flatten() {
            let path = e.path();
            if path.extension().is_none_or(|x| x != "asc") {
                continue;
            }
            let s = parse_asc(&decode(&std::fs::read(&path).unwrap()), &lookup).unwrap();
            files += 1;
            for (name, named, list) in &s.nets {
                pins += list.iter().filter(|(i, _)| s.parts.iter().any(|p| &p.inst == i && builtin_symbol(p.base_symbol()).is_some())).count();
                // only the basic parts: a digital gate's unused input or common pin is left open on purpose
                let basic = s.parts.iter().find(|p| p.inst == list[0].0).is_some_and(|p| builtin_symbol(p.base_symbol()).is_some());
                if !named && list.len() == 1 && basic {
                    alone += 1;
                    if alone <= 40 {
                        eprintln!("{}: {name} {:?}", path.file_name().unwrap().to_string_lossy(), list);
                    }
                }
            }
        }
        eprintln!("{files} files, {pins} pins, {alone} alone");
        assert!(files == 0 || (alone as f64) < 0.02 * pins as f64, "{alone} of {pins} pins on nothing");
    }
}
