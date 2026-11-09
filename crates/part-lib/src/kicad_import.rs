//! Build a circuitCad schematic from a read KiCad schematic ([`crate::kicad_sch::KicadSch`]).
//!
//! The drawing of the KiCad sheet is not carried over (its symbols and ours differ): the parts are laid out again in rows, and
//! the nets are kept as net labels on the pins. Plain resistors, capacitors, inductors, diodes, LEDs, NPN transistors and DC
//! sources become circuitCad's own parts (so they simulate); the others become library-style parts whose symbol is made from
//! the KiCad symbol's pins (they simulate only when they are given a model afterwards).

use crate::kicad::to_part;
use crate::kicad_sch::{KicadSch, SchPart, SchPin};
use sch_core::{Kind, Label, Pt, Schematic};

/// The result of an import: the schematic and what could not be carried over (Japanese, for the person who opens the file).
pub struct Imported {
    pub sch: Schematic,
    pub notes: Vec<String>,
    /// How many parts became plain circuitCad parts / parts with a symbol made from the KiCad one.
    pub basic: usize,
    pub custom: usize,
    /// The SPICE directives of the sheet (`.tran 10m`, `.ac dec 20 10 1Meg`): an LTspice sheet carries its analysis.
    pub directives: Vec<String>,
}

/// `4k7` → `4.7k`, `2R2` → `2.2`, `10R` → `10`, `100nF` stays: the value text as circuitCad reads it.
pub(crate) fn normalise_value(v: &str) -> String {
    let t = v.trim().replace(['\u{2126}', '\u{3a9}'], "");
    let b: Vec<char> = t.chars().collect();
    // digits, one letter, digits: the letter stands for the decimal point
    if let Some(i) = b.iter().position(|c| c.is_ascii_alphabetic())
        && i > 0
        && b[..i].iter().all(|c| c.is_ascii_digit())
        && i + 1 < b.len()
        && b[i + 1..].iter().all(|c| c.is_ascii_digit())
    {
        let unit = match b[i] {
            'R' | 'r' => "",
            'k' | 'K' => "k",
            'M' => "Meg",
            'm' => "m",
            'u' | 'U' => "u",
            'n' | 'N' => "n",
            'p' | 'P' => "p",
            _ => return t,
        };
        let int: String = b[..i].iter().collect();
        let frac: String = b[i + 1..].iter().collect();
        return format!("{int}.{frac}{unit}");
    }
    if let Some(stripped) = t.strip_suffix(['R', 'r'])
        && !stripped.is_empty()
        && stripped.chars().all(|c| c.is_ascii_digit() || c == '.')
    {
        return stripped.to_string();
    }
    t
}

/// The kind a KiCad part becomes, when it is one of the plain ones, and the pin of ours each KiCad pin goes to.
fn basic_kind(p: &SchPart) -> Option<Kind> {
    let name = p.lib_id.rsplit(':').next().unwrap_or(&p.lib_id);
    let two = p.pins.len() == 2;
    match name {
        "R" | "R_Small" | "R_US" | "R_Small_US" if two => Some(Kind::Resistor),
        "C" | "C_Small" | "C_Polarized" | "C_Polarized_Small" | "CP" | "CP_Small" if two => Some(Kind::Capacitor),
        "L" | "L_Small" | "L_US" if two => Some(Kind::Inductor),
        "D" | "D_Small" | "D_Schottky" | "D_Zener" if two => Some(Kind::Diode),
        "LED" | "LED_Small" if two => Some(Kind::Led),
        "VDC" | "VSIN" | "VPULSE" if two && source_value(p).is_some() => Some(Kind::VSource),
        "IDC" | "ISIN" | "IPULSE" if two && source_value(p).is_some() => Some(Kind::ISource),
        n if n.starts_with("Q_NPN") && p.pins.len() == 3 => Some(Kind::Npn),
        _ => None,
    }
}

/// The value of a KiCad 7 source (`Sim.Type` and `Sim.Params`: `dc=5`, `dc=0 ampl=1 f=1k`, `y1=0 y2=5 tw=1m per=2m`) as circuitCad
/// writes it: `5`, `SIN(0 1 1k 0 0 0)`, `PULSE(0 5 0 0 0 1m 2m)`. None when it is not one of these three.
fn source_value(p: &SchPart) -> Option<String> {
    let name = p.lib_id.rsplit(':').next().unwrap_or(&p.lib_id);
    let ty = p.fields.get("Sim.Type").map(|t| t.to_ascii_uppercase()).unwrap_or_else(|| match name {
        "VSIN" | "ISIN" => "SIN".into(),
        "VPULSE" | "IPULSE" => "PULSE".into(),
        _ => "DC".into(),
    });
    let params = p.fields.get("Sim.Params")?;
    let get = |k: &str| params.split_whitespace().find_map(|w| w.strip_prefix(&format!("{k}=")).map(str::to_string));
    let or0 = |k: &str| get(k).unwrap_or_else(|| "0".into());
    match ty.as_str() {
        "DC" => get("dc"),
        "SIN" => Some(format!("SIN({} {} {} {} {} {})", or0("dc"), get("ampl")?, get("f")?, or0("td"), or0("theta"), or0("phase"))),
        "PULSE" => Some(format!("PULSE({} {} {} {} {} {} {})", get("y1")?, get("y2")?, or0("td"), or0("tr"), or0("tf"), get("tw")?, get("per")?)),
        _ => None,
    }
}

/// The name of our pin for a KiCad pin.
fn our_pin(kind: Kind, pin: &SchPin) -> Option<String> {
    let by_name = |names: &[(&str, &str)]| names.iter().find(|(k, _)| pin.name.eq_ignore_ascii_case(k)).map(|(_, v)| v.to_string());
    match kind {
        Kind::Resistor | Kind::Capacitor | Kind::Inductor => Some(pin.number.clone()).filter(|n| n == "1" || n == "2"),
        // KiCad's diode: pin 1 is the cathode, pin 2 the anode (the pins are also named K and A)
        Kind::Diode | Kind::Led => by_name(&[("A", "A"), ("K", "K")]).or(match pin.number.as_str() {
            "1" => Some("K".into()),
            "2" => Some("A".into()),
            _ => None,
        }),
        Kind::VSource => by_name(&[("+", "+"), ("-", "-")]).or(match pin.number.as_str() {
            "1" => Some("+".into()),
            "2" => Some("-".into()),
            _ => None,
        }),
        // SPICE's current runs into pin 1 (+) and out of pin 2; ours flows out of +
        Kind::ISource => by_name(&[("+", "-"), ("-", "+")]).or(match pin.number.as_str() {
            "1" => Some("-".into()),
            "2" => Some("+".into()),
            _ => None,
        }),
        Kind::Npn => by_name(&[("C", "C"), ("B", "B"), ("E", "E")]),
        _ => None,
    }
}

fn net_name(n: &str) -> String {
    if n.eq_ignore_ascii_case("gnd") || n == "0" { "GND".to_string() } else { n.to_string() }
}

/// Puts a newly added part at the next free place of the rows (like the CLI's `add`), and moves the place on.
pub(crate) fn place_in_row(sch: &mut Schematic, idx: usize, cursor: &mut (i32, i32, i32)) {
    // size of the part: its pins (and the body of a drawn symbol)
    let c = &sch.components[idx];
    let mut xs: Vec<i32> = c.pins().iter().map(|(_, q)| q.0).collect();
    let mut ys: Vec<i32> = c.pins().iter().map(|(_, q)| q.1).collect();
    if let Some(sym) = &c.custom {
        let l = sym.layout();
        for (cx, cy) in [(-l.hw, -l.hh), (l.hw, l.hh)] {
            xs.push(cx);
            ys.push(cy);
        }
    }
    let (minx, maxx) = (*xs.iter().min().unwrap_or(&0), *xs.iter().max().unwrap_or(&0));
    let (miny, maxy) = (*ys.iter().min().unwrap_or(&0), *ys.iter().max().unwrap_or(&0));
    let (w, h) = (maxx - minx, maxy - miny);
    let (mut x, mut y, mut rowh) = *cursor;
    if x > 8 && x + w > 110 {
        x = 8;
        y += rowh + 10;
        rowh = 0;
    }
    *cursor = (x + w + 10, y, rowh.max(h));

    sch.components[idx].pos = (x - minx, y - miny);
}

/// Ties the pins of each net together with a label of the net's name (pins given as our reference and pin), and puts one ground
/// symbol on the GND net when the sheet has none, so the sheet looks and simulates like a normal one.
pub(crate) fn label_nets(sch: &mut Schematic, nets: &[(String, Vec<(String, String)>)]) {
    let mut first_gnd: Option<Pt> = None;
    for (name, pins) in nets {
        for (our_ref, our_num) in pins {
            let Some(c) = sch.components.iter().find(|c| &c.refdes == our_ref) else { continue };
            let Some((_, pt)) = c.pins().into_iter().find(|(m, _)| m == our_num) else { continue };
            if !sch.labels.iter().any(|l| l.pos == pt) {
                sch.labels.push(Label { pos: pt, text: name.clone() });
            }
            if name == "GND" {
                first_gnd.get_or_insert(pt);
            }
        }
    }
    let has_gnd = sch.components.iter().any(|c| c.kind == Kind::Ground);
    if let (false, Some(pt)) = (has_gnd, first_gnd) {
        sch.add(Kind::Ground, pt, 0);
    }
}

/// Lay the parts out in rows (like the CLI's `add`) and connect them by net labels.
pub fn import(k: &KicadSch) -> Imported {
    let mut sch = Schematic::default();
    let mut notes = k.notes.clone();
    let (mut basic, mut custom) = (0, 0);
    // (x, y, row height): the next free spot
    let mut cursor: (i32, i32, i32) = (8, 8, 0);
    // KiCad (reference, pin number) → (our reference, our pin)
    let mut pin_of: std::collections::HashMap<(String, String), (String, String)> = std::collections::HashMap::new();

    for p in &k.parts {
        let kind = basic_kind(p);
        let idx = match kind {
            Some(kd) => sch.add(kd, (0, 0), 0),
            None => {
                let Some(sym) = k.symbols.get(&p.symbol) else {
                    notes.push(format!("{}: 記号の定義がファイルになかったので、取り込めませんでした。", p.reference));
                    continue;
                };
                match to_part(sym, None) {
                    Ok(d) => sch.add_custom(d.symbol(), &d.default_value(), &d.footprint_name(), (0, 0), 0),
                    Err(e) => {
                        notes.push(format!("{}: 記号を作れませんでした ({e})。", p.reference));
                        continue;
                    }
                }
            }
        };
        // a reference used twice (unannotated parts are all "R?") gets a fresh one
        let mut refdes = p.reference.clone();
        if refdes.is_empty() || refdes.contains('?') || sch.components.iter().enumerate().any(|(i, c)| i != idx && c.refdes.eq_ignore_ascii_case(&refdes)) {
            notes.push(format!("{} は参照名が決まっていない (または重なっている) ので、{} にしました。", p.reference, sch.components[idx].refdes));
            refdes = sch.components[idx].refdes.clone();
        }
        place_in_row(&mut sch, idx, &mut cursor);
        let c = &mut sch.components[idx];
        c.refdes = refdes.clone();
        if !p.value.is_empty() {
            c.value = match kind {
                Some(Kind::Resistor | Kind::Capacitor | Kind::Inductor) => normalise_value(&p.value),
                Some(Kind::VSource | Kind::ISource) => source_value(p).unwrap_or_else(|| p.value.clone()),
                _ => p.value.clone(),
            };
        }
        // the footprint the KiCad part names, when circuitCad has one of that name
        if let Some(fp) = p.footprint.rsplit(':').next().filter(|f| !f.is_empty()) {
            // KiCad's chip names end in the metric size (`R_0805_2012Metric`): ours do not
            let short = fp.strip_suffix("Metric").and_then(|f| f.rsplit_once('_')).map(|(a, _)| a);
            if let Some(name) = [Some(fp), short].into_iter().flatten().find(|n| pcb_core::footprints::get(n).is_some()) {
                c.footprint = name.to_string();
            } else if !p.footprint.is_empty() {
                notes.push(format!("{refdes}: フットプリント「{fp}」はこのソフトにありません。基板に取り込む前に選び直してください。"));
            }
        }
        if p.dnp {
            c.sim_only = true;
            notes.push(format!("{refdes} は KiCad で「実装しない」になっています。シミュレーション専用の部品として取り込みました (基板と部品表には出ません)。"));
        }
        let have: Vec<String> = c.pins().into_iter().map(|(n, _)| n).collect();
        for pin in &p.pins {
            let ours = match kind {
                Some(kd) => our_pin(kd, pin),
                None => Some(pin.number.clone()),
            };
            match ours {
                Some(o) if have.contains(&o) => {
                    pin_of.insert((p.reference.clone(), pin.number.clone()), (refdes.clone(), o));
                }
                _ => notes.push(format!("{refdes}: ピン {} ({}) は対応する場所がなく、つなぎませんでした。", pin.number, pin.name)),
            }
        }
        if p.lib_id.rsplit(':').next().is_some_and(|n| n.starts_with("CP") || n.starts_with("C_Polarized")) && kind.is_some() {
            notes.push(format!("{refdes} は極性のあるコンデンサです。このソフトのコンデンサは極性を記号に出さないので、ピン1が + 側 (KiCad と同じ) であることを覚えておいてください。ESR などは値の欄に書けます。"));
        }
        if kind.is_some() { basic += 1 } else { custom += 1 }
    }

    let named: Vec<(String, Vec<(String, String)>)> = k
        .nets
        .iter()
        .map(|n| (net_name(&n.name), n.pins.iter().filter_map(|(r, num)| pin_of.get(&(r.clone(), num.clone())).cloned()).collect()))
        .collect();
    label_nets(&mut sch, &named);
    for (r, num) in &k.no_connects {
        if let Some((our_ref, our_num)) = pin_of.get(&(r.clone(), num.clone())) {
            sch.set_no_connect(our_ref, our_num, true);
        }
    }
    if k.nets.iter().all(|n| net_name(&n.name) != "GND") {
        notes.push("「GND」という名前のネットがありません。シミュレーションするには、0 V の基準にするネットの名前を GND にしてください。".into());
    }
    let unmodelled: Vec<&str> = sch.components.iter().filter(|c| c.custom.is_some()).map(|c| c.refdes.as_str()).collect();
    if !unmodelled.is_empty() {
        notes.push(format!(
            "記号から作った部品 ({}) には、シミュレーションのモデルがありません。計算では、回路にないものとして扱います。モデルを付けるには、同じ名前の部品をライブラリで作り直すか、基本の部品に置き換えてください。",
            unmodelled.join(", ")
        ));
    }
    notes.push("配置は取り込み時に並べ直しています (KiCad の図の位置・形は引き継ぎません)。つながりは、ピンについたネットの名前で表しています。".into());
    Imported { sch, notes, basic, custom, directives: vec![] }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kicad_sch::parse_schematic;


    const DIVIDER: &str = r##"(kicad_sch (version 20231120) (generator "eeschema")
  (lib_symbols
    (symbol "Device:R" (in_bom yes) (on_board yes)
      (property "Reference" "R" (at 2 0 90)) (property "Value" "R" (at 0 0 90))
      (symbol "R_1_1"
        (pin passive line (at 0 3.81 270) (length 1.27) (name "~" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
        (pin passive line (at 0 -3.81 90) (length 1.27) (name "~" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))))
    (symbol "Simulation_SPICE:VDC" (in_bom yes) (on_board yes)
      (property "Reference" "V" (at 2 0 90)) (property "Value" "VDC" (at 0 0 90))
      (symbol "VDC_1_1"
        (pin passive line (at 0 3.81 270) (length 1.27) (name "+" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
        (pin passive line (at 0 -3.81 90) (length 1.27) (name "-" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))))
  )
  (symbol (lib_id "Simulation_SPICE:VDC") (at 80 100 0) (unit 1)
    (property "Reference" "V1" (at 0 0 0)) (property "Value" "VDC" (at 0 0 0)) (property "Sim.Params" "dc=9" (at 0 0 0)))
  (symbol (lib_id "Device:R") (at 100 100 0) (unit 1)
    (property "Reference" "R1" (at 0 0 0)) (property "Value" "4k7" (at 0 0 0)) (property "Footprint" "Resistor_SMD:R_0805_2012Metric" (at 0 0 0)))
  (symbol (lib_id "Device:R") (at 100 120 0) (unit 1)
    (property "Reference" "R2" (at 0 0 0)) (property "Value" "4.7k" (at 0 0 0)))
  (wire (pts (xy 80 96.19) (xy 100 96.19)))
  (wire (pts (xy 100 103.81) (xy 100 116.19)))
  (label "OUT" (at 100 110 0))
  (label "GND" (at 80 103.81 0))
  (label "GND" (at 100 123.81 0))
)"##;

    #[test]
    fn a_divider_comes_in_as_plain_parts_with_the_same_nets() {
        let k = parse_schematic(DIVIDER).unwrap();
        let imp = import(&k);
        assert_eq!((imp.basic, imp.custom), (3, 0));
        let c = |r: &str| imp.sch.components.iter().find(|c| c.refdes == r).unwrap();
        assert_eq!(c("V1").kind, Kind::VSource);
        assert_eq!(c("V1").value, "9", "the DC value comes from Sim.Params");
        assert_eq!(c("R1").value, "4.7k", "4k7 is written the way circuitCad reads it");
        assert_eq!(c("R1").footprint, "R_0805", "a footprint circuitCad has is kept");
        assert!(!imp.notes.iter().any(|n| n.contains("フットプリント")), "{:?}", imp.notes);
        let nets = sch_core::extract(&imp.sch);
        let on = |name: &str| -> Vec<String> {
            let mut v: Vec<String> = nets.iter().find(|n| n.name == name).unwrap_or_else(|| panic!("no net {name}")).pins.iter().map(|p| format!("{}.{}", p.refdes, p.pin)).collect();
            v.sort();
            v
        };
        assert_eq!(on("OUT"), ["R1.2", "R2.1"]);
        assert_eq!(on("GND"), ["R2.2", "V1.-"]);
        assert!(imp.sch.components.iter().any(|c| c.kind == Kind::Ground), "one ground symbol is placed");
        let first_net: Vec<String> = nets.iter().find(|n| n.pins.iter().any(|p| p.refdes == "V1" && p.pin == "+")).unwrap().pins.iter().map(|p| format!("{}.{}", p.refdes, p.pin)).collect();
        assert!(first_net.contains(&"R1.1".to_string()), "{first_net:?}");
    }

    #[test]
    fn kicad_sources_become_circuitcad_source_values() {
        let part = |lib: &str, fields: &[(&str, &str)]| SchPart {
            reference: "V1".into(),
            value: "x".into(),
            lib_id: lib.into(),
            symbol: lib.into(),
            footprint: String::new(),
            dnp: false,
            fields: fields.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
            pins: vec![],
        };
        assert_eq!(source_value(&part("Simulation_SPICE:VDC", &[("Sim.Params", "dc=9")])).as_deref(), Some("9"));
        assert_eq!(source_value(&part("Simulation_SPICE:VSIN", &[("Sim.Params", "dc=1 ampl=2 f=1k")])).as_deref(), Some("SIN(1 2 1k 0 0 0)"));
        assert_eq!(source_value(&part("Simulation_SPICE:VPULSE", &[("Sim.Params", "y1=0 y2=5 tr=1u tf=1u tw=1m per=2m")])).as_deref(), Some("PULSE(0 5 0 1u 1u 1m 2m)"));
        assert_eq!(source_value(&part("Simulation_SPICE:VPULSE", &[("Sim.Params", "y1=0")])), None, "missing parameters: not guessed");
    }

    #[test]
    fn a_polarized_capacitor_is_a_capacitor_with_a_note() {
        let t = DIVIDER.replace("Device:R\") (at 100 120", "Device:C_Polarized\") (at 100 120").replace("(symbol \"Device:R\" (in_bom", "(symbol \"Device:C_Polarized\" (in_bom yes) (on_board yes) (property \"Reference\" \"C\" (at 0 0 0)) (property \"Value\" \"C\" (at 0 0 0)) (symbol \"C_Polarized_1_1\" (pin passive line (at 0 3.81 270) (length 1.27) (name \"+\" (effects (font (size 1.27 1.27)))) (number \"1\" (effects (font (size 1.27 1.27))))) (pin passive line (at 0 -3.81 90) (length 1.27) (name \"-\" (effects (font (size 1.27 1.27)))) (number \"2\" (effects (font (size 1.27 1.27)))))))
    (symbol \"Device:R\" (in_bom");
        let imp = import(&parse_schematic(&t).unwrap());
        assert_eq!((imp.basic, imp.custom), (3, 0), "{:?}", imp.notes);
        assert!(imp.notes.iter().any(|n| n.contains("極性")), "{:?}", imp.notes);
    }

    #[test]
    fn values_written_the_kicad_way_are_read() {
        assert_eq!(normalise_value("4k7"), "4.7k");
        assert_eq!(normalise_value("2R2"), "2.2");
        assert_eq!(normalise_value("10R"), "10");
        assert_eq!(normalise_value("100nF"), "100nF");
        assert_eq!(normalise_value("10k"), "10k");
        assert_eq!(normalise_value("1M"), "1M");
    }

    #[test]
    fn the_real_ecl_sheet_becomes_a_schematic_with_the_same_nets() {
        let path = r"F:\Tools\ngspice\Spice64\examples\osdi\hicuml0\KiCad\ECL-OR.kicad_sch";
        let Ok(text) = std::fs::read_to_string(path) else { return };
        let k = parse_schematic(&text).unwrap();
        let imp = import(&k);
        let nets = sch_core::extract(&imp.sch);
        // every KiCad net with two or more pins is a net of ours, with the same parts on it
        for kn in k.nets.iter().filter(|n| n.pins.len() >= 2) {
            let name = net_name(&kn.name);
            let mut want: Vec<&str> = kn.pins.iter().map(|(r, _)| r.as_str()).collect();
            want.sort();
            let mine = nets.iter().find(|n| n.name == name).unwrap_or_else(|| panic!("net {name} lost"));
            let mut got: Vec<&str> = mine.pins.iter().map(|p| p.refdes.as_str()).collect();
            got.sort();
            assert_eq!(got, want, "net {name}");
        }
        assert_eq!(imp.sch.components.iter().filter(|c| !c.kind.is_power_symbol()).count(), k.parts.len());
        eprintln!("imported: basic {} custom {} notes {:?}", imp.basic, imp.custom, imp.notes);
    }
}
