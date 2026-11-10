//! Build a circuitCad schematic from a read LTspice schematic ([`crate::ltspice::AscSch`]).
//!
//! As with KiCad, the drawing is not carried over: the parts are laid out again in rows and joined by net labels. Resistors,
//! capacitors, coils and voltage sources become circuitCad's own parts, with LTspice's way of writing values turned into ours
//! (`10µ` → `10u`, `SINE(...)` → `SIN(...)`, `Rser=` of a capacitor → `ESR=`). Diodes, transistors and other parts whose value
//! (or symbol) names a part of the library become that part, so they simulate with its model; a plain diode or NPN without one
//! becomes the basic part. Anything else gets a symbol made from LTspice's pins, without a model, and a note says so.

use crate::kicad::{to_part, KicadSymbol};
use crate::kicad_import::{label_nets, normalise_value, place_in_row, Imported};
use crate::ltspice::{AscPart, AscSch};
use crate::{PartDef, PinDef, PinSide};
use sch_core::{Kind, Schematic};
use std::collections::HashMap;

type Params = HashMap<String, f64>;

/// A value as circuitCad writes it: the micro sign as `u`, an empty `""` as nothing, and every `{...}` worked out from the
/// `.param` lines (`{R1/2}` → `500`). A `{...}` that cannot be worked out is left as it is (the simulation then says so).
fn plain_value(v: &str, params: &Params) -> String {
    let v = v.trim().replace(['µ', 'μ'], "u");
    if v == "\"\"" {
        return String::new();
    }
    let mut out = String::new();
    let mut rest = v.as_str();
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}').map(|c| open + c) else { break };
        out.push_str(&rest[..open]);
        match crate::ltspice_expr::eval(&rest[open..=close], params) {
            Some(x) => out.push_str(&crate::ltspice_expr::format_spice(x)),
            None => out.push_str(&rest[open..=close]),
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

/// A voltage source's value: LTspice's `SINE(...)` is our `SIN(...)`; a value that is only `AC 1` has a DC of 0; `Value2` (where
/// LTspice puts the `AC 1` of a source with a DC or wave value) follows the value.
fn source_value(p: &AscPart, params: &Params) -> String {
    let mut v = plain_value(p.attr("Value"), params);
    if v.to_ascii_uppercase().starts_with("SINE(") {
        v = format!("SIN({}", &v[5..]);
        // LTspice's 7th number is a count of cycles, which our SIN does not take
        if let Some(inner) = v.strip_prefix("SIN(").and_then(|r| r.strip_suffix(')')) {
            let nums: Vec<&str> = inner.split_whitespace().collect();
            if nums.len() > 6 {
                v = format!("SIN({})", nums[..6].join(" "));
            }
        }
    }
    if v.to_ascii_uppercase().starts_with("AC ") {
        v = format!("0 {v}");
    }
    let v2 = plain_value(p.attr("Value2"), params);
    if !v2.is_empty() {
        v = if v.is_empty() { format!("0 {v2}") } else { format!("{v} {v2}") };
    }
    if v.is_empty() { "0".into() } else { v }
}

/// A capacitor's or coil's value with LTspice's extra settings (`Rser=10m Lser=1n` in `SpiceLine`) in our words (`ESR=10m
/// ESL=1n` for a capacitor, `DCR=` for a coil). The settings we have no word for are returned for a note.
fn passive_value(p: &AscPart, kind: Kind, params: &Params) -> (String, Vec<String>) {
    let mut v = normalise_value(&plain_value(p.attr("Value"), params));
    let mut dropped = vec![];
    let line = format!("{} {}", p.attr("SpiceLine"), p.attr("Value2"));
    for w in line.split_whitespace() {
        let Some((k, x)) = w.split_once('=') else { continue };
        let x = plain_value(x, params);
        let ours = match (kind, k.to_ascii_lowercase().as_str()) {
            (Kind::Capacitor, "rser") => Some("ESR"),
            (Kind::Capacitor, "lser") => Some("ESL"),
            (Kind::Capacitor, "ic") => Some("IC"),
            (Kind::Inductor, "rser") => Some("DCR"),
            // ratings and the manufacturer's name do not change the simulation
            (_, "v" | "irms" | "ipk" | "mfg" | "type" | "pwr" | "tol") => None,
            _ => {
                dropped.push(w.to_string());
                None
            }
        };
        if let Some(o) = ours {
            v = format!("{v} {o}={x}");
        }
    }
    // LTspice gives a coil 1 mΩ in series unless told otherwise (so a coil across a source still solves): do the same
    if kind == Kind::Inductor && !v.contains("DCR=") && !v.is_empty() {
        v = format!("{v} DCR=1m");
    }
    (v, dropped)
}

/// The kind a symbol becomes when it is one of the plain ones (before the library is asked).
fn basic_kind(p: &AscPart) -> Option<Kind> {
    match p.base_symbol().to_ascii_lowercase().as_str() {
        "res" | "res2" => Some(Kind::Resistor),
        "cap" | "polcap" => Some(Kind::Capacitor),
        "ind" | "ind2" => Some(Kind::Inductor),
        "voltage" | "battery" => Some(Kind::VSource),
        "current" => Some(Kind::ISource),
        _ => None,
    }
}

/// Our pin for an LTspice pin of a plain part.
fn basic_pin(kind: Kind, pin: &str) -> Option<&'static str> {
    match (kind, pin) {
        (Kind::Resistor | Kind::Capacitor | Kind::Inductor, "A") => Some("1"),
        (Kind::Resistor | Kind::Capacitor | Kind::Inductor, "B") => Some("2"),
        (Kind::VSource, "+") => Some("+"),
        (Kind::VSource, "-") => Some("-"),
        // SPICE's current runs into + and out of - (LTspice's arrow points to -); ours flows out of +
        (Kind::ISource, "+") => Some("-"),
        (Kind::ISource, "-") => Some("+"),
        (Kind::Diode | Kind::Led, "+") => Some("A"),
        (Kind::Diode | Kind::Led, "-") => Some("K"),
        (Kind::Npn, "C" | "B" | "E") => Some(match pin {
            "C" => "C",
            "B" => "B",
            _ => "E",
        }),
        _ => None,
    }
}

/// The library pin for an LTspice pin: diodes are `+`/`-` in LTspice and A/K here; otherwise the names, ignoring case and spaces.
fn library_pin(def: &PartDef, pin: &str) -> Option<String> {
    let norm = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_uppercase();
    let want = match pin {
        "+" if def.pins.iter().any(|p| p.name.eq_ignore_ascii_case("A")) => "A".to_string(),
        "-" if def.pins.iter().any(|p| p.name.eq_ignore_ascii_case("K")) => "K".to_string(),
        _ => norm(pin),
    };
    def.pins.iter().find(|p| norm(&p.name) == want).map(|p| p.number.clone())
}

/// A library part named by the part's value (`2N3904`, `1N4148`) or its symbol (`LM358`), ignoring case.
fn library_match<'a>(p: &AscPart, lib: &'a [PartDef]) -> Option<&'a PartDef> {
    let names = [p.attr("Value").trim(), p.base_symbol()];
    names.iter().filter(|n| !n.is_empty()).find_map(|n| lib.iter().find(|d| d.name.eq_ignore_ascii_case(n)))
}

/// A symbol of our own from LTspice's pins (no model): pins in SPICE order, each on the side of the body it sticks out of.
fn drawn_symbol(p: &AscPart, sym: &crate::ltspice::Symbol) -> KicadSymbol {
    let n = sym.pins.len().max(1) as i64;
    let (cx, cy) = (sym.pins.iter().map(|q| q.at.0).sum::<i64>() / n, sym.pins.iter().map(|q| q.at.1).sum::<i64>() / n);
    let mut pins: Vec<&crate::ltspice::SymPin> = sym.pins.iter().collect();
    pins.sort_by_key(|q| q.order);
    let pins = pins
        .iter()
        .map(|q| {
            let (dx, dy) = (q.at.0 - cx, q.at.1 - cy);
            let side = if dx.abs() >= dy.abs() {
                if dx < 0 { PinSide::Left } else { PinSide::Right }
            } else if dy < 0 {
                PinSide::Top
            } else {
                PinSide::Bottom
            };
            PinDef { number: q.order.to_string(), name: if q.name.is_empty() { q.order.to_string() } else { q.name.clone() }, side: Some(side), sim_only: false, gap: 0 }
        })
        .collect();
    let value = if p.attr("Value").is_empty() { sym.value.clone() } else { p.attr("Value").to_string() };
    let name = if value.is_empty() { p.base_symbol().to_string() } else { value.clone() };
    KicadSymbol { name, prefix: if sym.prefix.is_empty() { "U".into() } else { sym.prefix.clone() }, value, description: sym.description.clone(), datasheet: String::new(), footprint: String::new(), pins, units: 1 }
}

/// Lay the parts out in rows and connect them by net labels. `lib` is the part library (for diodes, transistors and ICs named
/// by their value).
pub fn import(a: &AscSch, lib: &[PartDef]) -> Imported {
    let mut sch = Schematic::default();
    let mut notes = a.notes.clone();
    let (mut basic, mut custom) = (0, 0);
    let mut cursor: (i32, i32, i32) = (8, 8, 0);
    let prm = crate::ltspice_expr::params(&a.directives);
    // LTspice (inst, pin name) → (our reference, our pin)
    let mut pin_of: HashMap<(String, String), (String, String)> = HashMap::new();
    let mut skipped: Vec<String> = vec![];
    // notes that many parts share ("model NP is not in the library"): gathered and written once with all the parts
    let mut per_part: Vec<(String, String)> = vec![];

    for p in &a.parts {
        let Some(sym) = a.symbols.get(&p.inst) else { continue };
        let base = p.base_symbol().to_ascii_lowercase();
        // what to place: a plain part, a library part, a plain diode / NPN, or a symbol drawn from the pins
        enum Pick<'a> {
            Basic(Kind),
            Lib(&'a PartDef),
            Drawn,
        }
        let pick = match basic_kind(p) {
            Some(k) => Pick::Basic(k),
            None => match library_match(p, lib) {
                Some(d) => Pick::Lib(d),
                None => match base.as_str() {
                    "diode" | "schottky" | "zener" | "varactor" => Pick::Basic(Kind::Diode),
                    "led" => Pick::Basic(Kind::Led),
                    "npn" => Pick::Basic(Kind::Npn),
                    // no basic PNP or MOSFET: a common library part stands in, and a note says so
                    "pnp" | "nmos" | "pmos" => {
                        let stand_in = match base.as_str() {
                            "pnp" => "2N3906",
                            "nmos" => "2N7000",
                            _ => "AO3401A",
                        };
                        match lib.iter().find(|d| d.name.eq_ignore_ascii_case(stand_in)) {
                            Some(d) => {
                                let v = p.attr("Value").trim();
                                per_part.push((format!("型番「{v}」はライブラリにないので、代わりに {stand_in} にしました (特性は型番と違います)"), p.inst.clone()));
                                Pick::Lib(d)
                            }
                            None => Pick::Drawn,
                        }
                    }
                    "e" | "e2" | "f" | "g" | "g2" | "h" | "bv" | "bi" | "bi2" => {
                        skipped.push(format!("{} (式や制御で動く電源 {})", p.inst, p.base_symbol()));
                        continue;
                    }
                    _ => Pick::Drawn,
                },
            },
        };
        let idx = match &pick {
            Pick::Basic(k) => sch.add(*k, (0, 0), 0),
            Pick::Lib(d) => sch.add_custom(d.symbol(), &d.default_value(), &d.footprint_name(), (0, 0), 0),
            Pick::Drawn => match to_part(&drawn_symbol(p, sym), None) {
                Ok(mut d) => {
                    d.category = "LTspice".into();
                    d.description = d.description.replace("KiCad から取り込み", "LTspice から取り込み");
                    sch.add_custom(d.symbol(), &d.default_value(), &d.footprint_name(), (0, 0), 0)
                }
                Err(e) => {
                    notes.push(format!("{}: 記号を作れませんでした ({e})。", p.inst));
                    continue;
                }
            },
        };
        let mut refdes = p.inst.clone();
        if refdes.is_empty() || sch.components.iter().enumerate().any(|(i, c)| i != idx && c.refdes.eq_ignore_ascii_case(&refdes)) {
            notes.push(format!("{} は名前が重なっているので、{} にしました。", p.inst, sch.components[idx].refdes));
            refdes = sch.components[idx].refdes.clone();
        }
        place_in_row(&mut sch, idx, &mut cursor);
        let c = &mut sch.components[idx];
        c.refdes = refdes.clone();
        match &pick {
            Pick::Basic(k @ (Kind::Capacitor | Kind::Inductor)) => {
                let (v, dropped) = passive_value(p, *k, &prm);
                c.value = v;
                if !dropped.is_empty() {
                    notes.push(format!("{refdes}: LTspice の設定 {} は、このソフトにない項目なので入れていません。", dropped.join(" ")));
                }
            }
            Pick::Basic(Kind::Resistor) => c.value = normalise_value(&plain_value(p.attr("Value"), &prm)),
            Pick::Basic(Kind::ISource) => c.value = source_value(p, &prm),
            Pick::Basic(Kind::VSource) => {
                c.value = source_value(p, &prm);
                if p.attr("SpiceLine").to_ascii_lowercase().contains("rser") {
                    notes.push(format!("{refdes}: 電源の内部抵抗 (Rser) は入れていません。要るなら、抵抗を直列に足してください。"));
                }
            }
            Pick::Basic(Kind::Diode | Kind::Led | Kind::Npn) => {
                let v = p.attr("Value").trim();
                if !v.is_empty() && !matches!(v.to_ascii_uppercase().as_str(), "D" | "NPN" | "LED") {
                    per_part.push((format!("型番「{v}」はライブラリにないので、標準の値の部品にしました (特性は型番と違います)"), refdes.clone()));
                }
            }
            Pick::Lib(_) => {}
            Pick::Drawn => per_part.push((format!("「{}」はこのソフトのライブラリにない部品です。記号だけ作りました (シミュレーションのモデルはありません)", p.base_symbol()), refdes.clone())),
            Pick::Basic(_) => {}
        }
        let have: Vec<String> = c.pins().into_iter().map(|(n, _)| n).collect();
        let lib_def = if let Pick::Lib(d) = &pick { Some(*d) } else { None };
        for pin in &sym.pins {
            let ours = match &pick {
                Pick::Basic(k) => basic_pin(*k, &pin.name).map(str::to_string),
                Pick::Lib(_) => lib_def.and_then(|d| library_pin(d, &pin.name)),
                Pick::Drawn => Some(pin.order.to_string()),
            };
            match ours {
                Some(o) if have.contains(&o) => {
                    pin_of.insert((p.inst.clone(), pin.name.clone()), (refdes.clone(), o));
                }
                _ => notes.push(format!("{refdes}: ピン {} は対応するピンがなく、つなぎませんでした。", pin.name)),
            }
        }
        if matches!(pick, Pick::Drawn) { custom += 1 } else { basic += 1 }
        if p.attrs.values().any(|v| v.to_ascii_lowercase().contains("mc(")) {
            per_part.push(("ばらつき (mc) は公称値で取り込みました。「温度とばらつき」で調べられます".into(), refdes.clone()));
        }
    }

    let nets: Vec<(String, Vec<(String, String)>)> = a
        .nets
        .iter()
        .map(|(name, _, pins)| (if name.eq_ignore_ascii_case("gnd") { "GND".into() } else { name.clone() }, pins.iter().filter_map(|k| pin_of.get(k).cloned()).collect()))
        .collect();
    label_nets(&mut sch, &nets);
    if !skipped.is_empty() {
        notes.push(format!("取り込めない部品がありました: {}。このソフトの部品で置き換えてください。", skipped.join("、")));
    }
    if !nets.iter().any(|(n, _)| n == "GND") {
        notes.push("グラウンド (LTspice の「0」の印) がありません。シミュレーションするには、0 V の基準にするネットを GND にしてください。".into());
    }
    // the shared notes, once each, with their parts
    let mut seen: Vec<(String, Vec<String>)> = vec![];
    for (msg, r) in per_part {
        match seen.iter_mut().find(|(m, _)| *m == msg) {
            Some((_, rs)) => rs.push(r),
            None => seen.push((msg, vec![r])),
        }
    }
    for (msg, rs) in seen {
        notes.push(format!("{}: {msg}。", rs.join(", ")));
    }
    let stepped: Vec<&String> = a.directives.iter().filter(|d| d.to_ascii_lowercase().starts_with(".step")).collect();
    if !stepped.is_empty() {
        notes.push(format!("{} は、掃引する値の最初の 1 つで取り込みました。値を振って比べるには「パラメータスイープ」を使います。", stepped.iter().map(|s| format!("「{s}」")).collect::<Vec<_>>().join(" ")));
    }
    let used = |d: &str| [".tran", ".ac", ".param", ".op", ".step"].iter().any(|k| d.to_ascii_lowercase().starts_with(k));
    let others: Vec<&String> = a.directives.iter().filter(|d| !used(d)).collect();
    if !others.is_empty() {
        notes.push(format!("SPICE の指示 {} は取り込みません (モデルの定義・パラメータの掃引・結合などは、このソフトの部品と解析の欄で設定し直してください)。", others.iter().map(|s| format!("「{s}」")).collect::<Vec<_>>().join(" ")));
    }
    notes.push("配置は取り込み時に並べ直しています (LTspice の図の位置・形は引き継ぎません)。つながりは、ピンについたネットの名前で表しています。".into());
    Imported { sch, notes, basic, custom, directives: a.directives.clone() }
}

/// Reads an `.asc` file and builds the schematic. The symbols are looked for next to the file first, then in LTspice's library on
/// this computer (the basic ones are built in, so a circuit of plain parts reads without LTspice).
pub fn import_file(path: &std::path::Path, lib: &[PartDef]) -> Result<Imported, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = crate::ltspice::decode(&bytes);
    let mut index = crate::ltspice::symbol_index(&crate::ltspice::ltspice_symbol_dirs());
    // the folder of the sheet (not its subfolders: it may be a big one like Downloads) wins over the library
    if let Some(dir) = path.parent()
        && let Ok(rd) = std::fs::read_dir(dir)
    {
        for e in rd.flatten() {
            let f = e.path();
            if f.extension().is_some_and(|x| x.eq_ignore_ascii_case("asy"))
                && let Some(stem) = f.file_stem()
            {
                index.insert(stem.to_string_lossy().to_lowercase(), f);
            }
        }
    }
    let a = crate::ltspice::parse_asc(&text, &|name| crate::ltspice::find_symbol(&index, name))?;
    Ok(import(&a, lib))
}

/// The number a value starts with (`10m` → 10, `0` → 0): enough to tell a zero step from a real one.
fn lead_number(x: &str) -> Option<f64> {
    let end = x.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(x.len());
    x[..end].parse().ok()
}

/// The analyses the sheet asks for, in the words of the app's boxes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Analyses {
    /// `.tran`: the stop time, the largest step (empty = automatic) and `uic` (start from zero instead of the operating point).
    pub tran: Option<(String, String, bool)>,
    /// `.ac`: start and stop frequency and points per decade.
    pub ac: Option<(String, String, String)>,
}

/// Reads `.tran` and `.ac` from the directives. LTspice writes `.tran 10m` or `.tran Tstep Tstop [Tstart [dTmax]] [uic]`, and
/// `.ac dec|oct|lin N fstart fstop` (octaves and linear counts are turned into points per decade, near enough).
pub fn analyses(directives: &[String]) -> Analyses {
    let mut out = Analyses::default();
    for d in directives {
        let w: Vec<&str> = d.split_whitespace().collect();
        let Some(first) = w.first() else { continue };
        let nums: Vec<&str> = w[1..].iter().copied().filter(|x| x.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '.')).collect();
        match first.to_ascii_lowercase().as_str() {
            ".tran" => {
                let uic = w.iter().any(|x| x.eq_ignore_ascii_case("uic"));
                let clean = |x: &str| x.replace(['µ', 'μ'], "u");
                let positive = |x: &str| lead_number(x).is_some_and(|v| v > 0.0);
                out.tran = match nums.as_slice() {
                    [stop] => Some((clean(stop), String::new(), uic)),
                    [step, stop, rest @ ..] => {
                        // the largest step: dTmax when given, else the printing step (0 = automatic)
                        let max = [rest.get(1).copied(), Some(*step)].into_iter().flatten().find(|x| positive(x)).map(clean).unwrap_or_default();
                        Some((clean(stop), max, uic))
                    }
                    _ => None,
                };
            }
            ".ac" if w.len() >= 5 => {
                let n: f64 = lead_number(w[2]).unwrap_or(20.0);
                let ppd = match w[1].to_ascii_lowercase().as_str() {
                    "dec" => n,
                    "oct" => n * 3.32,
                    // a linear sweep has no points per decade: a fine log sweep shows the same range
                    _ => 50.0,
                };
                out.ac = Some((w[3].replace(['µ', 'μ'], "u"), w[4].replace(['µ', 'μ'], "u"), format!("{}", ppd.round().max(1.0))));
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ltspice::parse_asc;

    const RC: &str = "Version 4
SHEET 1 880 680
WIRE 144 96 64 96
WIRE 256 96 224 96
WIRE 256 128 256 96
WIRE 64 128 64 96
WIRE 64 240 64 208
WIRE 256 240 256 192
FLAG 64 240 0
FLAG 256 240 0
FLAG 256 96 out
SYMBOL voltage 64 112 R0
SYMATTR InstName V1
SYMATTR Value SINE(0 1 1k 0 0 0 10)
SYMATTR Value2 AC 1
SYMBOL res 240 80 R90
SYMATTR InstName R1
SYMATTR Value 1k5
SYMBOL cap 240 128 R0
SYMATTR InstName C1
SYMATTR Value {C}
SYMATTR SpiceLine V=16 Rser=10m Cpar=1p
TEXT 64 280 Left 2 !.tran 5m\\n.param C=100\u{b5}
";

    #[test]
    fn an_rc_filter_comes_in_with_our_values_and_nets() {
        let a = parse_asc(RC, &|_| None).unwrap();
        let imp = import(&a, &[]);
        assert_eq!((imp.basic, imp.custom), (3, 0), "{:?}", imp.notes);
        let c = |r: &str| imp.sch.components.iter().find(|c| c.refdes == r).unwrap();
        assert_eq!(c("V1").value, "SIN(0 1 1k 0 0 0) AC 1");
        assert_eq!(c("R1").value, "1.5k");
        assert_eq!(c("C1").value, "100u ESR=10m", "the .param and the micro sign are read, Rser becomes ESR");
        assert!(imp.notes.iter().any(|n| n.contains("Cpar=1p")), "{:?}", imp.notes);
        assert_eq!(imp.directives, [".tran 5m", ".param C=100µ"]);
        let nets = sch_core::extract(&imp.sch);
        let on = |name: &str| -> Vec<String> {
            let mut v: Vec<String> = nets.iter().find(|n| n.name == name).unwrap_or_else(|| panic!("no net {name}")).pins.iter().map(|p| format!("{}.{}", p.refdes, p.pin)).collect();
            v.sort();
            v
        };
        assert_eq!(on("out"), ["C1.1", "R1.1"]);
        assert_eq!(on("GND"), ["C1.2", "V1.-"]);
        assert!(imp.sch.components.iter().any(|c| c.kind == Kind::Ground));
    }

    #[test]
    fn a_transistor_named_in_the_library_becomes_that_part() {
        let lib = crate::load_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples").as_path()).0;
        let t = RC.replace("SYMBOL cap 240 128 R0\nSYMATTR InstName C1\nSYMATTR Value {C}\nSYMATTR SpiceLine V=16 Rser=10m Cpar=1p\n", "SYMBOL npn 192 128 R0\nSYMATTR InstName Q1\nSYMATTR Value 2N3904\nSYMBOL npn 400 128 R0\nSYMATTR InstName Q2\nSYMATTR Value MYPART\nSYMBOL current 500 100 R0\nSYMATTR InstName I1\n");
        let imp = import(&parse_asc(&t, &|_| None).unwrap(), &lib);
        let c = |r: &str| imp.sch.components.iter().find(|c| c.refdes == r).unwrap();
        assert_eq!(c("Q1").kind, Kind::Custom, "2N3904 from the library");
        assert_eq!(c("Q2").kind, Kind::Npn, "an unknown model: the basic NPN, with a note");
        assert!(imp.notes.iter().any(|n| n.contains("MYPART")), "{:?}", imp.notes);
        assert_eq!(c("I1").kind, Kind::ISource, "a current source comes in as one");
        // every pin of Q1 found its library pin
        assert!(!imp.notes.iter().any(|n| n.starts_with("Q1: ピン")), "{:?}", imp.notes);
    }

    #[test]
    fn an_ltspice_current_source_pushes_the_same_way_here() {
        // LTspice's arrow points to its - pin: the current leaves there, into "out" (1 mA into 1k: +1 V in LTspice)
        let t = "Version 4
SHEET 1 880 680
WIRE 64 192 64 224
WIRE 160 224 64 224
FLAG 64 112 0
FLAG 160 304 0
FLAG 64 224 out
SYMBOL current 64 112 R0
SYMATTR InstName I1
SYMATTR Value 1m
SYMBOL res 144 208 R0
SYMATTR InstName R1
SYMATTR Value 1k
";
        let imp = import(&parse_asc(t, &|_| None).unwrap(), &[]);
        let i1 = imp.sch.components.iter().find(|c| c.refdes == "I1").unwrap();
        assert_eq!((i1.kind, i1.value.as_str(), i1.sim_only), (Kind::ISource, "1m", true));
        let nets = sch_core::extract(&imp.sch);
        let out: Vec<String> = nets.iter().find(|n| n.name == "out").unwrap().pins.iter().map(|p| format!("{}.{}", p.refdes, p.pin)).collect();
        assert!(out.contains(&"I1.+".to_string()), "ours pushes out of + into out: {out:?}");
    }

    #[test]
    fn the_sheets_analyses_are_read() {
        let d = |v: &[&str]| analyses(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(d(&[".tran 10m"]).tran, Some(("10m".into(), String::new(), false)));
        assert_eq!(d(&[".tran 0 5m 0 1u uic"]).tran, Some(("5m".into(), "1u".into(), true)));
        assert_eq!(d(&[".tran 10u 2m"]).tran, Some(("2m".into(), "10u".into(), false)));
        assert_eq!(d(&[".ac dec 20 10 1Meg"]).ac, Some(("10".into(), "1Meg".into(), "20".into())));
        assert_eq!(d(&[".ac oct 10 1 100k"]).ac.unwrap().2, "33");
        assert_eq!(d(&[".op"]), Analyses::default());
    }
}

/// Every `.asc` LTspice ships (several thousand) read and built without a panic. Slow; run by hand:
/// `cargo test -p part-lib --lib every_ltspice_example -- --ignored --nocapture`.
#[cfg(test)]
#[test]
#[ignore]
fn every_ltspice_example_reads_without_a_panic() {
    let Ok(base) = std::env::var("LOCALAPPDATA") else { return };
    let root = std::path::Path::new(&base).join("LTspice/examples");
    let lib = crate::load_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples").as_path()).0;
    let index = crate::ltspice::symbol_index(&crate::ltspice::ltspice_symbol_dirs());
    let mut stack = vec![root];
    let (mut files, mut refused) = (0, 0);
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("asc")) {
                files += 1;
                let text = crate::ltspice::decode(&std::fs::read(&p).unwrap());
                match crate::ltspice::parse_asc(&text, &|n| crate::ltspice::find_symbol(&index, n)) {
                    Ok(a) => {
                        let _ = import(&a, &lib);
                    }
                    Err(_) => refused += 1,
                }
            }
        }
    }
    eprintln!("{files} files, {refused} refused");
    assert_eq!(refused, 0);
}
