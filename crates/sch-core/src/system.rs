//! Several boards in one project (E5, multi-board): which connector of one board's schematic goes to which connector of another's, the
//! check that the two sides carry the same signals pin by pin, and the parts list of the whole system. GUI-free: the app's project and
//! `ccad project` use it.
//!
//! A link pairs the pins of two connectors: by default pin 1 with pin 1, 2 with 2 ... (a cable or a board-to-board header), or as listed
//! (a cable that crosses, a socket numbered the other way). The check compares the nets on the two sides by name: the same name is the same
//! signal. A net without a name (`N$...`) cannot be compared and is only noted: naming the nets on both boards is what makes the check work.

use crate::netlist::{extract, Severity};
use crate::Schematic;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// One end of a link: a connector (by designator) in a board's schematic (by the board's name in the project).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkEnd {
    pub design: String,
    pub refdes: String,
}

/// Two connectors joined (a cable, or one board plugged into the other).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardLink {
    pub a: LinkEnd,
    pub b: LinkEnd,
    /// Pin of `a` to pin of `b`; empty: the same numbers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pairs: Vec<(String, String)>,
}

/// What the check found on a link.
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub severity: Severity,
    /// Index into the links.
    pub link: usize,
    /// The pins of the two connectors it is about, if one pair.
    pub pins: Option<(String, String)>,
    pub message: String,
    /// Why it matters and what to do.
    pub hint: String,
}

/// What a net carries, as far as its name tells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Ground,
    Power,
    Signal,
    /// Not named by the person (`N$1` ...): it cannot be compared with the other board.
    Unnamed,
}

/// The role of a net by its name: GND / 0 / VSS / AGND ... ground; VCC / VDD / VIN / VBAT / +5V / 3V3 / 12V ... power.
pub fn role(net: &str) -> Role {
    let n = net.trim().to_ascii_uppercase();
    if n.starts_with("N$") || n.is_empty() {
        return Role::Unnamed;
    }
    if n == "0" || n.contains("GND") || n.starts_with("VSS") || n == "V-" || n == "VEE" {
        return Role::Ground;
    }
    let volts = |s: &str| {
        // 5V, 3V3, 12V, +5V, -12V, 3.3V
        let s = s.trim_start_matches(['+', '-']);
        let d = s.find('V').map(|i| (&s[..i], &s[i + 1..]));
        d.is_some_and(|(a, b)| !a.is_empty() && a.chars().all(|c| c.is_ascii_digit() || c == '.') && b.chars().all(|c| c.is_ascii_digit()))
    };
    if ["VCC", "VDD", "VIN", "VBAT", "VBUS", "VSYS", "VOUT", "VPP", "V+", "VREF"].iter().any(|p| n.starts_with(p)) || volts(&n) {
        return Role::Power;
    }
    Role::Signal
}

/// The net of every (designator, pin) of a schematic. A pin left open (alone in a net without a name) has none.
pub fn pin_nets(sch: &Schematic) -> HashMap<(String, String), String> {
    let mut out = HashMap::new();
    for net in extract(sch) {
        if net.pins.len() < 2 && role(&net.name) == Role::Unnamed {
            continue;
        }
        for p in net.pins {
            out.insert((p.refdes, p.pin), net.name.clone());
        }
    }
    out
}

/// The pin numbers of a part of a schematic, in order (`None`: no such part).
pub fn pins_of(sch: &Schematic, refdes: &str) -> Option<Vec<String>> {
    let flat = sch.flat();
    let c = flat.components.iter().find(|c| c.refdes == refdes)?;
    Some(c.pins().into_iter().map(|p| p.0).collect())
}

/// The pin pairs of a link: as listed, else the same numbers on both sides (as far as the shorter connector goes).
pub fn pin_pairs(link: &BoardLink, a_pins: &[String], b_pins: &[String]) -> Vec<(String, String)> {
    if !link.pairs.is_empty() {
        return link.pairs.clone();
    }
    a_pins.iter().filter(|p| b_pins.contains(p)).map(|p| (p.clone(), p.clone())).collect()
}

fn same(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// Checks every link: that both connectors exist and have as many pins, and pin by pin that the two sides carry the same net (by name),
/// that power does not meet ground, that two crossed pins are not swapped, and that a signal does not end in a pin left open on the other
/// side. `designs` are the boards' names and schematics.
pub fn check(designs: &[(&str, &Schematic)], links: &[BoardLink]) -> Vec<Finding> {
    let mut out = vec![];
    let mut nets: HashMap<&str, HashMap<(String, String), String>> = HashMap::new();
    for (name, sch) in designs {
        nets.insert(name, pin_nets(sch));
    }
    let find = |n: &str| designs.iter().find(|(d, _)| *d == n).map(|(_, s)| *s);
    for (k, link) in links.iter().enumerate() {
        let f = |severity, pins: Option<(String, String)>, message: String, hint: &str| Finding { severity, link: k, pins, message, hint: hint.to_string() };
        let (Some(sa), Some(sb)) = (find(&link.a.design), find(&link.b.design)) else {
            out.push(f(Severity::Error, None, format!("基板「{}」か「{}」がプロジェクトにありません", link.a.design, link.b.design), "基板の名前を変えたか消したときは、つなぎを作り直してください"));
            continue;
        };
        let (Some(pa), Some(pb)) = (pins_of(sa, &link.a.refdes), pins_of(sb, &link.b.refdes)) else {
            out.push(f(Severity::Error, None, format!("{}.{} か {}.{} が回路図にありません", link.a.design, link.a.refdes, link.b.design, link.b.refdes), "部品番号を変えたときは、つなぎの部品番号も合わせてください"));
            continue;
        };
        if link.pairs.is_empty() && pa.len() != pb.len() {
            out.push(f(
                Severity::Warning,
                None,
                format!("{}.{} は {} ピン、{}.{} は {} ピンです", link.a.design, link.a.refdes, pa.len(), link.b.design, link.b.refdes, pb.len()),
                "ピンの数が違うコネクタどうしは、同じ番号のピンだけをつないで調べます。数が違うのが正しい (一部だけ使う) なら、ピンの組を書いてください",
            ));
        }
        let (na, nb) = (&nets[link.a.design.as_str()], &nets[link.b.design.as_str()]);
        let pairs = pin_pairs(link, &pa, &pb);
        let net_a = |p: &str| na.get(&(link.a.refdes.clone(), p.to_string())).cloned();
        let net_b = |p: &str| nb.get(&(link.b.refdes.clone(), p.to_string())).cloned();
        let mut unnamed = 0;
        for (p, q) in &pairs {
            let (x, y) = (net_a(p), net_b(q));
            let pins = Some((p.clone(), q.clone()));
            let label = format!("{}.{}-{} ↔ {}.{}-{}", link.a.design, link.a.refdes, p, link.b.design, link.b.refdes, q);
            match (&x, &y) {
                (None, None) => {}
                (Some(n), None) | (None, Some(n)) => {
                    if role(n) != Role::Unnamed {
                        out.push(f(Severity::Warning, pins, format!("{label}: 片側だけ {n} につながり、もう片側はどこにもつながっていません"), "相手の基板で、そのピンにも同じ信号をつなぐか、使わないピンなら両側とも空けます"));
                    }
                }
                (Some(m), Some(n)) => {
                    let (rm, rn) = (role(m), role(n));
                    if rm == Role::Unnamed || rn == Role::Unnamed {
                        unnamed += 1;
                        continue;
                    }
                    if same(m, n) {
                        continue;
                    }
                    let swapped = matches!((rm, rn), (Role::Ground, Role::Power) | (Role::Power, Role::Ground));
                    if swapped {
                        out.push(f(Severity::Error, pins, format!("{label}: 電源 ({}) と GND ({}) がつながります", if rm == Role::Power { m } else { n }, if rm == Role::Ground { m } else { n }), "このままつなぐと電源がショートします。ピンの並びか、どちらかの基板の配線を直してください"));
                        continue;
                    }
                    // the other side's net on another pin of this link: two pins crossed
                    let crossed = pairs.iter().find(|(p2, q2)| (p2, q2) != (p, q) && net_b(q2).is_some_and(|z| same(&z, m)));
                    let hint = "コネクタの向き (1 番ピンの位置) や、ケーブルが入れ替わっていないか確かめます。名前の付け方が違うだけ (例: SDA と I2C_SDA) なら、どちらかの名前を合わせます";
                    match crossed {
                        Some((_, q2)) => out.push(f(Severity::Error, pins, format!("{label}: {m} と {n} が違います ({m} は相手の {q2} 番にあります。入れ替わっていませんか)"), hint)),
                        None => out.push(f(Severity::Warning, pins, format!("{label}: {m} と {n} の名前が違います"), hint)),
                    }
                }
            }
        }
        if unnamed > 0 {
            out.push(f(
                Severity::Warning,
                None,
                format!("{}.{} ↔ {}.{}: 名前の無いネット (N$…) のピンが {unnamed} 本あり、照合できません", link.a.design, link.a.refdes, link.b.design, link.b.refdes),
                "両方の基板で、コネクタのピンにラベル (同じ名前) を付けると、信号が合っているかを調べられます",
            ));
        }
    }
    out
}

/// A line of the whole system's parts list: one kind of part (same part name, value and footprint), how many on each board and in all.
#[derive(Clone, Debug, PartialEq)]
pub struct BomRow {
    pub part: String,
    pub value: String,
    pub footprint: String,
    /// Shop part numbers (shop -> number), from the library part.
    pub shops: BTreeMap<String, String>,
    /// (board, designators) in the project's order.
    pub per_board: Vec<(String, Vec<String>)>,
    pub total: usize,
}

/// The parts of every board counted together: the real parts (not power symbols, not the simulation-only sources and probes), by part
/// name, value and footprint. `copies` is how many of each board are built (1 if not given).
pub fn system_bom(designs: &[(&str, &Schematic)], copies: &[usize]) -> Vec<BomRow> {
    let mut rows: BTreeMap<(String, String, String), BomRow> = BTreeMap::new();
    for (k, (name, sch)) in designs.iter().enumerate() {
        let n = copies.get(k).copied().unwrap_or(1).max(1);
        let flat = sch.flat();
        for c in flat.components.iter().filter(|c| !c.kind.is_power_symbol() && !c.sim_only) {
            let part = c.custom.as_ref().map_or_else(|| c.kind.label(), |s| s.name.clone());
            let key = (part.clone(), c.value.clone(), c.footprint.clone());
            let row = rows.entry(key).or_insert_with(|| BomRow { part, value: c.value.clone(), footprint: c.footprint.clone(), shops: c.custom.as_ref().map(|s| s.shops.clone()).unwrap_or_default(), per_board: vec![], total: 0 });
            match row.per_board.iter_mut().find(|(b, _)| b == name) {
                Some((_, r)) => r.push(c.refdes.clone()),
                None => row.per_board.push((name.to_string(), vec![c.refdes.clone()])),
            }
            row.total += n;
        }
    }
    let mut v: Vec<BomRow> = rows.into_values().collect();
    for r in &mut v {
        for (_, refs) in &mut r.per_board {
            refs.sort();
        }
    }
    v
}

/// The parts list as CSV (UTF-8 with a BOM, so that a spreadsheet opens the Japanese right): one column per board with its designators.
pub fn bom_csv(rows: &[BomRow], boards: &[&str]) -> String {
    let q = |s: &str| if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() };
    let mut out = String::from("\u{feff}");
    let mut head = vec!["部品".to_string(), "値".into(), "フットプリント".into(), "合計".into()];
    head.extend(boards.iter().map(|b| q(b)));
    head.push("店の型番".into());
    out.push_str(&head.join(","));
    out.push('\n');
    for r in rows {
        let mut cols = vec![q(&r.part), q(&r.value), q(&r.footprint), r.total.to_string()];
        for b in boards {
            cols.push(q(&r.per_board.iter().find(|(n, _)| n == b).map(|(_, refs)| refs.join(" ")).unwrap_or_default()));
        }
        cols.push(q(&r.shops.iter().map(|(s, n)| format!("{s}:{n}")).collect::<Vec<_>>().join(" ")));
        out.push_str(&cols.join(","));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, Kind, Label, Schematic, Wire};

    /// A schematic with a 4-pin header J1 whose pins are wired to labels `names` (empty: left open).
    fn board(header: &str, names: &[&str]) -> Schematic {
        let mut s = Schematic::default();
        let c = Component { refdes: header.into(), kind: Kind::Header(names.len() as u8), value: String::new(), pos: (10, 10), rot: 0, footprint: String::new(), custom: None, sim_only: false, pin_names: vec![], theta: 0.0, tc_ppm: 0.0, cth: 0.0, t_max: 0.0, zth: vec![], sub: None, shops: Default::default() };
        let pins = c.pins();
        s.components.push(c);
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

    fn link(pairs: &[(&str, &str)]) -> BoardLink {
        BoardLink { a: LinkEnd { design: "main".into(), refdes: "J1".into() }, b: LinkEnd { design: "sensor".into(), refdes: "J1".into() }, pairs: pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect() }
    }

    #[test]
    fn nets_are_told_by_their_names() {
        assert_eq!(role("GND"), Role::Ground);
        assert_eq!(role("AGND"), Role::Ground);
        assert_eq!(role("+5V"), Role::Power);
        assert_eq!(role("3V3"), Role::Power);
        assert_eq!(role("VBAT"), Role::Power);
        assert_eq!(role("SDA"), Role::Signal);
        assert_eq!(role("N$3"), Role::Unnamed);
    }

    /// The same names on both sides: nothing to say. Swapped power and ground: an error; two signals crossed: an error naming where the
    /// other one is; a different name alone: a warning; a pin used on one side only: a warning.
    #[test]
    fn a_link_is_checked_pin_by_pin() {
        let main = board("J1", &["+5V", "GND", "SDA", "SCL"]);
        let good = board("J1", &["+5V", "GND", "SDA", "SCL"]);
        assert!(check(&[("main", &main), ("sensor", &good)], &[link(&[])]).is_empty());
        let swapped = board("J1", &["GND", "+5V", "SDA", "SCL"]);
        let f = check(&[("main", &main), ("sensor", &swapped)], &[link(&[])]);
        assert!(f.iter().filter(|x| x.severity == Severity::Error).count() == 2 && f[0].message.contains("電源"), "{f:?}");
        let crossed = board("J1", &["+5V", "GND", "SCL", "SDA"]);
        let f = check(&[("main", &main), ("sensor", &crossed)], &[link(&[])]);
        assert!(f.iter().any(|x| x.severity == Severity::Error && x.message.contains("入れ替わって")), "{f:?}");
        // the same cable crossed back by the pin pairs: fine
        assert!(check(&[("main", &main), ("sensor", &crossed)], &[link(&[("1", "1"), ("2", "2"), ("3", "4"), ("4", "3")])]).is_empty());
        let renamed = board("J1", &["+5V", "GND", "I2C_SDA", "SCL"]);
        let f = check(&[("main", &main), ("sensor", &renamed)], &[link(&[])]);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, Severity::Warning);
        let open = board("J1", &["+5V", "GND", "SDA", ""]);
        let f = check(&[("main", &main), ("sensor", &open)], &[link(&[])]);
        assert!(f.len() == 1 && f[0].message.contains("片側だけ"), "{f:?}");
    }

    /// A connector or a board that is not there, and connectors of different sizes, are told.
    #[test]
    fn missing_parts_and_different_sizes_are_told() {
        let main = board("J1", &["+5V", "GND", "SDA", "SCL"]);
        let small = board("J1", &["+5V", "GND"]);
        let f = check(&[("main", &main), ("sensor", &small)], &[link(&[])]);
        assert!(f.iter().any(|x| x.message.contains("4 ピン")), "{f:?}");
        let f = check(&[("main", &main)], &[link(&[])]);
        assert!(f[0].message.contains("sensor"));
        let mut gone = link(&[]);
        gone.b.refdes = "J9".into();
        assert!(check(&[("main", &main), ("sensor", &small)], &[gone])[0].message.contains("J9"));
    }

    /// The parts of two boards counted together, with the boards' designators and the copies built.
    #[test]
    fn the_parts_of_all_boards_are_counted_together() {
        let main = board("J1", &["+5V", "GND", "SDA", "SCL"]);
        let sensor = board("J1", &["+5V", "GND", "SDA", "SCL"]);
        let rows = system_bom(&[("main", &main), ("sensor", &sensor)], &[1, 3]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].total, 4, "one on the main board and one on each of three sensor boards");
        assert_eq!(rows[0].per_board.len(), 2);
        let csv = bom_csv(&rows, &["main", "sensor"]);
        assert!(csv.starts_with('\u{feff}') && csv.lines().count() == 2 && csv.contains(",4,J1,J1,"), "{csv}");
    }
}
