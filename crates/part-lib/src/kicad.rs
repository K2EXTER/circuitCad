//! Import of KiCad library files: symbols (`.kicad_sym`) and footprints (`.kicad_mod`) become a [`PartDef`].
//! Only the pins and pads are taken (not the drawing); the shape of the symbol is the usual box.

use crate::{PartDef, PinDef};
use pcb_core::package::{HoleSpec, PackageSpec, PadSpec};
use sch_core::PinSide;

// ---------------------------------------------------------------- S-expressions

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Atom(String),
    List(Vec<Node>),
}

impl Node {
    fn text(&self) -> Option<&str> {
        match self {
            Node::Atom(s) => Some(s),
            Node::List(_) => None,
        }
    }

    fn items(&self) -> &[Node] {
        match self {
            Node::List(v) => v,
            Node::Atom(_) => &[],
        }
    }

    /// The first element when it is an atom: `(pad ...)` -> "pad".
    fn head(&self) -> Option<&str> {
        self.items().first().and_then(|n| n.text())
    }

    fn find(&self, name: &str) -> Option<&Node> {
        self.items().iter().skip(1).find(|n| n.head() == Some(name))
    }

    fn find_all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> {
        self.items().iter().skip(1).filter(move |n| n.head() == Some(name))
    }

    /// The `i`-th element after the head, as text.
    fn arg(&self, i: usize) -> Option<&str> {
        self.items().get(i + 1).and_then(|n| n.text())
    }

    fn num(&self, i: usize) -> Option<f64> {
        self.arg(i).and_then(|s| s.parse().ok())
    }
}

/// Parse one S-expression. Strings in double quotes become atoms without the quotes.
pub fn parse_sexpr(src: &str) -> Result<Node, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut stack: Vec<Vec<Node>> = vec![];
    let mut root: Option<Node> = None;
    while i < chars.len() {
        let c = chars[i];
        match c {
            c if c.is_whitespace() => i += 1,
            '(' => {
                stack.push(vec![]);
                i += 1;
            }
            ')' => {
                let items = stack.pop().ok_or("カッコ ) が多すぎます。KiCad のファイルではないか、途中で壊れています")?;
                let node = Node::List(items);
                match stack.last_mut() {
                    Some(parent) => parent.push(node),
                    None => {
                        root = Some(node);
                        break;
                    }
                }
                i += 1;
            }
            _ => {
                let mut s = String::new();
                if c == '"' {
                    i += 1;
                    loop {
                        let Some(&ch) = chars.get(i) else { return Err("文字列の \" が閉じていません".into()) };
                        i += 1;
                        match ch {
                            '"' => break,
                            '\\' => {
                                let Some(&n) = chars.get(i) else { return Err("文字列の \" が閉じていません".into()) };
                                i += 1;
                                s.push(if n == 'n' { '\n' } else { n });
                            }
                            _ => s.push(ch),
                        }
                    }
                } else {
                    while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '(' && chars[i] != ')' {
                        s.push(chars[i]);
                        i += 1;
                    }
                }
                stack.last_mut().ok_or("ファイルの先頭がカッコで始まっていません。KiCad のファイルではありません")?.push(Node::Atom(s));
            }
        }
    }
    if !stack.is_empty() {
        return Err("カッコ ( が閉じていません。ファイルが途中で切れている可能性があります".into());
    }
    root.ok_or_else(|| "中身が空です".into())
}

// ---------------------------------------------------------------- symbols

#[derive(Clone, Debug, PartialEq)]
pub struct KicadSymbol {
    pub name: String,
    /// Reference prefix without digits ("R", "U", ...).
    pub prefix: String,
    pub value: String,
    pub description: String,
    pub datasheet: String,
    /// Footprint the symbol asks for (`Library:Name`), if any.
    pub footprint: String,
    pub pins: Vec<PinDef>,
    /// Number of units (gates) the KiCad symbol has; they are merged into one symbol.
    pub units: usize,
}

fn side_of(angle: f64) -> PinSide {
    // The angle is the direction from the connection point towards the body.
    match ((angle.rem_euclid(360.0) / 90.0).round() as i64) % 4 {
        0 => PinSide::Left,
        1 => PinSide::Bottom,
        2 => PinSide::Right,
        _ => PinSide::Top,
    }
}

fn collect_pins(sym: &Node, out: &mut Vec<(PinSide, f64, f64, String, String)>) {
    for n in sym.items().iter().skip(1) {
        match n.head() {
            Some("pin") => {
                let (Some(at), Some(num)) = (n.find("at"), n.find("number").and_then(|x| x.arg(0))) else { continue };
                let num = num.trim();
                if num.is_empty() || num == "~" {
                    continue;
                }
                let name = n.find("name").and_then(|x| x.arg(0)).unwrap_or("~");
                let name = if name == "~" || name.is_empty() { num } else { name };
                out.push((side_of(at.num(2).unwrap_or(0.0)), at.num(0).unwrap_or(0.0), at.num(1).unwrap_or(0.0), num.to_string(), name.to_string()));
            }
            Some("symbol") => collect_pins(n, out),
            _ => {}
        }
    }
}

/// Pins as (side, x, y, number, name): top to bottom on the left/right, left to right on the top/bottom, as the part file lists them.
/// A pin number that came twice (another unit of the same part) is kept once.
fn ordered_pins(raw: Vec<(PinSide, f64, f64, String, String)>) -> Vec<PinDef> {
    let mut seen = std::collections::HashSet::new();
    let mut v: Vec<_> = raw.into_iter().filter(|r| seen.insert(r.3.clone())).collect();
    v.sort_by(|a, b| {
        let rank = |s: PinSide| match s {
            PinSide::Left => 0,
            PinSide::Right => 1,
            PinSide::Top => 2,
            PinSide::Bottom => 3,
        };
        rank(a.0).cmp(&rank(b.0)).then_with(|| match a.0 {
            PinSide::Left | PinSide::Right => b.2.total_cmp(&a.2),
            _ => a.1.total_cmp(&b.1),
        })
    });
    v.into_iter().map(|(side, _, _, number, name)| PinDef { number, name, side: Some(side), sim_only: false }).collect()
}

fn prop<'a>(sym: &'a Node, key: &str) -> &'a str {
    sym.find_all("property").find(|p| p.arg(0) == Some(key)).and_then(|p| p.arg(1)).unwrap_or("")
}

fn symbol_of(all: &[&Node], sym: &Node, depth: usize) -> Option<KicadSymbol> {
    let name = sym.arg(0)?.to_string();
    // `(extends "Parent")`: the pins live in the parent, the properties here.
    let parent = sym.find("extends").and_then(|e| e.arg(0)).filter(|_| depth < 8);
    let mut raw = vec![];
    collect_pins(sym, &mut raw);
    let mut parent_sym = None;
    if raw.is_empty()
        && let Some(p) = parent
        && let Some(pn) = all.iter().find(|n| n.arg(0) == Some(p))
    {
        parent_sym = symbol_of(all, pn, depth + 1);
    }
    let prefix: String = prop(sym, "Reference").chars().take_while(|c| c.is_ascii_alphabetic() || *c == '#').collect();
    if prefix.starts_with('#') {
        return None; // power flags and the like are not parts
    }
    let units = sym.items().iter().skip(1).filter(|n| n.head() == Some("symbol")).filter_map(|n| n.arg(0)).filter_map(|s| s.rsplit('_').nth(1)).filter(|u| *u != "0").collect::<std::collections::HashSet<_>>().len().max(1);
    let pins = if let Some(p) = &parent_sym {
        p.pins.clone()
    } else {
        ordered_pins(raw)
    };
    let nonempty = |s: &str, fallback: &str| if s.is_empty() { fallback.to_string() } else { s.to_string() };
    Some(KicadSymbol {
        value: nonempty(prop(sym, "Value"), &name),
        prefix: nonempty(&prefix, "U"),
        description: nonempty(prop(sym, "Description"), prop(sym, "ki_description")),
        datasheet: prop(sym, "Datasheet").trim_matches('~').to_string(),
        footprint: prop(sym, "Footprint").to_string(),
        name,
        pins,
        units: parent_sym.map_or(units, |p| p.units),
    })
}

/// All parts of a `.kicad_sym` file (power symbols and parts without pins are skipped).
pub fn parse_symbols(text: &str) -> Result<Vec<KicadSymbol>, String> {
    let root = parse_sexpr(text)?;
    if root.head() != Some("kicad_symbol_lib") {
        return Err("KiCad のシンボルライブラリ (.kicad_sym) ではありません".into());
    }
    let all: Vec<&Node> = root.find_all("symbol").collect();
    let v: Vec<KicadSymbol> = all.iter().filter_map(|s| symbol_of(&all, s, 0)).filter(|s| !s.pins.is_empty()).collect();
    if v.is_empty() {
        return Err("取り込める部品 (ピンのあるシンボル) がありません".into());
    }
    Ok(v)
}

// ---------------------------------------------------------------- legacy `.lib` symbols

/// The words of a line of a legacy library; a double-quoted word may hold spaces.
fn words(line: &str) -> Vec<String> {
    let (mut out, mut cur, mut quoted, mut any) = (vec![], String::new(), false, false);
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// A legacy (KiCad 4/5, `EESchema-LIBRARY`) `.lib` file, which Ultra Librarian and SnapEDA write for KiCad: the pins (`X` lines) and the
/// fields. The drawing is not taken; the units are merged as for `.kicad_sym`.
pub fn parse_legacy_lib(text: &str) -> Result<Vec<KicadSymbol>, String> {
    if !text.trim_start().starts_with("EESchema-LIBRARY") {
        return Err("KiCad の旧形式のシンボルライブラリ (.lib) ではありません".into());
    }
    let mut out = vec![];
    // the part being read, and its pins
    let mut cur: Option<(KicadSymbol, Vec<(PinSide, f64, f64, String, String)>)> = None;
    let mut in_draw = false;
    for line in text.lines() {
        let w = words(line);
        let Some(head) = w.first().map(String::as_str) else { continue };
        match head {
            "DEF" if w.len() >= 8 => {
                let prefix: String = w[2].chars().take_while(|c| c.is_ascii_alphabetic() || *c == '#').collect();
                // power symbols (#PWR ...) are not parts
                cur = (!prefix.starts_with('#')).then(|| {
                    let name = w[1].trim_start_matches('~').to_string();
                    let sym = KicadSymbol {
                        value: name.clone(),
                        name,
                        prefix: if prefix.is_empty() { "U".into() } else { prefix },
                        description: String::new(),
                        datasheet: String::new(),
                        footprint: String::new(),
                        pins: vec![],
                        units: w[7].parse().unwrap_or(1),
                    };
                    (sym, vec![])
                });
                in_draw = false;
            }
            "DRAW" => in_draw = true,
            "ENDDRAW" => in_draw = false,
            "ENDDEF" => {
                if let Some((mut sym, raw)) = cur.take() {
                    sym.pins = ordered_pins(raw);
                    if !sym.pins.is_empty() {
                        out.push(sym);
                    }
                }
                in_draw = false;
            }
            "X" if in_draw && w.len() >= 7 => {
                let Some((_, pins)) = cur.as_mut() else { continue };
                // X name number x y length orientation ...: the orientation is the way the pin points from its connection point
                let (Ok(x), Ok(y)) = (w[3].parse::<f64>(), w[4].parse::<f64>()) else { continue };
                let number = w[2].trim().to_string();
                if number.is_empty() || number == "~" {
                    continue;
                }
                let name = w[1].trim_start_matches('~');
                let name = if name.is_empty() { number.clone() } else { name.to_string() };
                let side = match w[6].as_str() {
                    "R" => PinSide::Left,
                    "L" => PinSide::Right,
                    "U" => PinSide::Bottom,
                    _ => PinSide::Top,
                };
                pins.push((side, x, y, number, name));
            }
            h if !in_draw && h.len() == 2 && h.starts_with('F') && w.len() >= 2 => {
                let Some((sym, _)) = cur.as_mut() else { continue };
                let value = w[1].trim_matches('~').to_string();
                match &h[1..] {
                    "1" if !value.is_empty() => sym.value = value,
                    "2" => sym.footprint = value,
                    "3" => sym.datasheet = value,
                    _ => {}
                }
            }
            _ => {}
        }
    }
    if out.is_empty() {
        return Err("取り込める部品 (ピンのあるシンボル) がありません".into());
    }
    Ok(out)
}

/// The symbols of a `.kicad_sym` or a legacy `.lib` file, whichever it is.
pub fn parse_symbol_file(text: &str) -> Result<Vec<KicadSymbol>, String> {
    if text.trim_start().starts_with("EESchema-LIBRARY") { parse_legacy_lib(text) } else { parse_symbols(text) }
}

// ---------------------------------------------------------------- footprints

#[derive(Clone, Debug, PartialEq)]
pub struct KicadFootprint {
    pub name: String,
    pub description: String,
    pub pads: Vec<PadSpec>,
    pub holes: Vec<HoleSpec>,
    /// Things that could not be carried over faithfully.
    pub warnings: Vec<String>,
}

/// A `.kicad_mod` file (KiCad 6 and later `footprint`, older `module`).
pub fn parse_footprint(text: &str) -> Result<KicadFootprint, String> {
    let root = parse_sexpr(text)?;
    if !matches!(root.head(), Some("footprint") | Some("module")) {
        return Err("KiCad のフットプリント (.kicad_mod) ではありません".into());
    }
    let mut fp = KicadFootprint {
        name: root.arg(0).unwrap_or("footprint").to_string(),
        description: root.find("descr").and_then(|d| d.arg(0)).unwrap_or("").to_string(),
        pads: vec![],
        holes: vec![],
        warnings: vec![],
    };
    let mut odd_shape = false;
    let mut odd_angle = false;
    for pad in root.find_all("pad") {
        let number = pad.arg(0).unwrap_or("").to_string();
        let kind = pad.arg(1).unwrap_or("smd");
        let shape = pad.arg(2).unwrap_or("rect");
        let (Some(at), Some(size)) = (pad.find("at"), pad.find("size")) else { continue };
        let (x, y) = (at.num(0).unwrap_or(0.0), -at.num(1).unwrap_or(0.0));
        let angle = at.num(2).unwrap_or(0.0).rem_euclid(360.0);
        let quarter = (angle - 90.0).abs() < 1e-6 || (angle - 270.0).abs() < 1e-6;
        if !quarter && angle.abs() > 1e-6 && (angle - 180.0).abs() > 1e-6 {
            odd_angle = true;
        }
        let (mut w, mut h) = (size.num(0).unwrap_or(0.0), size.num(1).unwrap_or(0.0));
        if quarter {
            std::mem::swap(&mut w, &mut h);
        }
        // `(drill D)`, `(drill oval W H)`, optionally followed by `(offset ..)`.
        let (mut drill, mut slot, mut slot_v) = (None, 0.0, false);
        if let Some(d) = pad.find("drill") {
            let (a, b) = if d.arg(0) == Some("oval") { (d.num(1), d.num(2)) } else { (d.num(0), None) };
            if let Some(a) = a.filter(|a| *a > 0.0) {
                match b.filter(|b| *b > 0.0 && (*b - a).abs() > 1e-6) {
                    Some(b) if b > a => (drill, slot, slot_v) = (Some(a), b, true),
                    Some(b) => (drill, slot) = (Some(b), a),
                    None => drill = Some(a),
                }
                if quarter && slot > 0.0 {
                    slot_v = !slot_v;
                }
            }
        }
        if kind == "np_thru_hole" {
            if let Some(d) = drill {
                fp.holes.push(HoleSpec { x, y, drill: d, slot, slot_v });
            }
            continue;
        }
        if number.is_empty() || w <= 0.0 {
            continue; // paste / mask apertures and the like
        }
        let (shape_name, ww, hh) = match shape {
            "rect" | "roundrect" | "trapezoid" => ("rect", w, h),
            "oval" => ("oval", w, h),
            "circle" => ("circle", w, w),
            _ => {
                odd_shape = true;
                ("rect", w, h)
            }
        };
        if matches!(shape, "trapezoid" | "custom") {
            odd_shape = true;
        }
        fp.pads.push(PadSpec { pin: number, x, y, shape: shape_name.into(), w: ww, h: hh, drill, slot, slot_v });
    }
    if fp.pads.is_empty() {
        return Err("パッドが 1 つもありません。ファイルを確かめてください".into());
    }
    if odd_shape {
        fp.warnings.push("台形・自由形状のパッドは四角に置き換えました。ランドの形をデータシートで確かめてください".into());
    }
    if odd_angle {
        fp.warnings.push("斜めに回したパッドの角度は無視しました".into());
    }
    Ok(fp)
}

// ---------------------------------------------------------------- to a part

/// Make a part from a KiCad symbol and (optionally) footprint.
/// Without a footprint, a single row of through-hole pads (2.54mm) stands in; replace it before ordering.
pub fn to_part(sym: &KicadSymbol, fp: Option<&KicadFootprint>) -> Result<PartDef, String> {
    let mut pins = sym.pins.clone();
    let mut notes = vec![];
    if sym.units > 1 {
        notes.push(format!("{} ユニットの部品を 1 つの記号にまとめています。", sym.units));
    }
    // A two-pin R / C / L is drawn with the usual symbol.
    let glyph = if pins.len() == 2 {
        match sym.prefix.as_str() {
            "R" => "resistor",
            "L" => "inductor",
            "C" if sym.name.starts_with("CP") => "electrolytic",
            "C" => "capacitor",
            _ => "",
        }
    } else {
        ""
    };
    if !glyph.is_empty() {
        pins[0].side = Some(PinSide::Left);
        pins[1].side = Some(PinSide::Right);
    }
    let (package, footprint_name) = match fp {
        Some(f) => {
            for p in &pins {
                if !f.pads.iter().any(|pad| pad.pin == p.number) {
                    return Err(format!(
                        "シンボルのピン {} ({}) に対応するパッドが、フットプリント {} にありません。組み合わせが違う可能性があります",
                        p.number, p.name, f.name
                    ));
                }
            }
            notes.extend(f.warnings.iter().cloned());
            (PackageSpec::Explicit { pads: f.pads.clone(), holes: f.holes.clone() }, Some(f.name.clone()))
        }
        None => {
            notes.push("フットプリントは仮のもの (2.54mm の 1 列) です。基板に使う前に、本物のフットプリントに替えてください。".into());
            let n = pins.len() as f64;
            let pads = pins
                .iter()
                .enumerate()
                .map(|(i, p)| PadSpec {
                    pin: p.number.clone(),
                    x: (i as f64 - (n - 1.0) / 2.0) * 2.54,
                    y: 0.0,
                    shape: if i == 0 { "rect" } else { "circle" }.into(),
                    w: 1.7,
                    h: 1.7,
                    drill: Some(1.0),
                    slot: 0.0,
                    slot_v: false,
                })
                .collect();
            (PackageSpec::Explicit { pads, holes: vec![] }, None)
        }
    };
    let mut description = sym.description.clone();
    if !notes.is_empty() {
        if !description.is_empty() {
            description.push(' ');
        }
        description.push_str(&format!("(KiCad から取り込み。{} シミュレーションモデルはありません。)", notes.join(" ")));
    } else {
        description.push_str(" (KiCad から取り込み。シミュレーションモデルはありません。)");
    }
    Ok(PartDef {
        height: None,
        name: sym.name.clone(),
        category: "KiCad".into(),
        description,
        datasheet: sym.datasheet.clone(),
        prefix: sym.prefix.clone(),
        value: Some(sym.value.clone()),
        pins,
        package,
        footprint_name,
        sim: None,
        ratings: None,
        glyph: glyph.into(),
        body: None,
        shops: Default::default(),
        land_unverified: false,
        model3d: None,
        swap: None,
    })
}

// ---------------------------------------------------------------- a maker's zip

/// What a library-maker's zip (Ultra Librarian, SnapEDA ... with "KiCad" chosen) holds.
#[derive(Clone, Debug, Default)]
pub struct KicadPackage {
    pub symbols: Vec<KicadSymbol>,
    pub footprints: Vec<KicadFootprint>,
    /// The 3D model (file name, bytes), when the zip has one (the makers add it only when it was asked for at the download).
    pub step: Option<(String, Vec<u8>)>,
    /// Files of the zip that could not be read.
    pub warnings: Vec<String>,
}

/// The largest file read from a zip (a STEP file is the big one).
const MAX_ENTRY: u64 = 64 << 20;

/// Reads the symbols (`.kicad_sym` or legacy `.lib`), footprints (`.kicad_mod`) and the STEP model of a zip.
pub fn read_package(path: &std::path::Path) -> Result<KicadPackage, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| format!("zip を開けませんでした: {e}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("zip として読めませんでした: {e}。ダウンロードした zip をそのまま選んでください"))?;
    let mut pkg = KicadPackage::default();
    for i in 0..zip.len() {
        let Ok(f) = zip.by_index(i) else { continue };
        let name = f.name().to_string();
        let base = name.rsplit('/').next().unwrap_or(&name).to_string();
        if f.is_dir() || name.starts_with("__MACOSX") || base.starts_with("._") {
            continue;
        }
        let ext = base.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        if !matches!(ext.as_str(), "kicad_sym" | "lib" | "kicad_mod" | "step" | "stp") {
            continue;
        }
        if f.size() > MAX_ENTRY {
            pkg.warnings.push(format!("{base} は大きすぎるので読みませんでした"));
            continue;
        }
        let mut bytes = vec![];
        if f.take(MAX_ENTRY).read_to_end(&mut bytes).is_err() {
            pkg.warnings.push(format!("{base} を読めませんでした"));
            continue;
        }
        match ext.as_str() {
            "step" | "stp" => {
                if pkg.step.is_none() {
                    pkg.step = Some((base, bytes));
                }
            }
            "kicad_mod" => match parse_footprint(&String::from_utf8_lossy(&bytes)) {
                Ok(fp) => pkg.footprints.push(fp),
                Err(e) => pkg.warnings.push(format!("{base}: {e}")),
            },
            // a `.lib` that is not a symbol library (a SPICE library, say) is passed over
            _ => match parse_symbol_file(&String::from_utf8_lossy(&bytes)) {
                Ok(s) => pkg.symbols.extend(s),
                Err(e) if ext == "kicad_sym" => pkg.warnings.push(format!("{base}: {e}")),
                Err(_) => {}
            },
        }
    }
    if pkg.symbols.is_empty() {
        return Err("この zip には KiCad のシンボル (.kicad_sym か、旧形式の .lib) が見つかりませんでした。ダウンロードのとき、形式に「KiCad」を選んだ zip を選んでください".into());
    }
    Ok(pkg)
}

/// The footprint of the package that goes with a symbol: the only one there is, or the one named as the symbol asks for it (or as the
/// symbol is called).
pub fn pick_footprint(sym: &KicadSymbol, fps: &[KicadFootprint]) -> Option<usize> {
    if fps.len() == 1 {
        return Some(0);
    }
    let asked = sym.footprint.rsplit(':').next().unwrap_or("");
    fps.iter().position(|f| !asked.is_empty() && f.name.eq_ignore_ascii_case(asked)).or_else(|| fps.iter().position(|f| f.name.eq_ignore_ascii_case(&sym.name)))
}
