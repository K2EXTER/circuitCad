//! Several boards in one project (E5, multi-board): which connector of one board's schematic goes to which connector of another's, the
//! check that the two sides carry the same signals pin by pin, and the parts list of the whole system. GUI-free: the app's project and
//! `ccad project` use it.
//!
//! A link pairs the pins of two connectors: by default pin 1 with pin 1, 2 with 2 ... (a cable or a board-to-board header), or as listed
//! (a cable that crosses, a socket numbered the other way). The check compares the nets on the two sides by name: the same name is the same
//! signal, and so are the names a link lists as one (`aliases`: `SDA` on one board is `I2C_DATA` on the other). A net without a name (`N$...`)
//! cannot be compared and is only noted: naming the nets on both boards is what makes the check work.

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
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BoardLink {
    pub a: LinkEnd,
    pub b: LinkEnd,
    /// Pin of `a` to pin of `b`; empty: the same numbers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pairs: Vec<(String, String)>,
    /// Net names of `a` and `b` that are the same signal although named differently (`SDA` and `I2C_DATA`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<(String, String)>,
    /// The two boards joined by a cable of this length (mm), not plugged into each other (`None`: plugged, or not said).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cable: Option<f64>,
    /// The cable's wire colours chosen by hand (pin of `a`, colour key of `harness::COLORS`); the others by what they carry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub colors: Vec<(String, String)>,
}

/// What the person can do about a finding with one click.
#[derive(Clone, Debug, PartialEq)]
pub enum Fix {
    /// Register the two names (side A, side B) as one signal on the link.
    Alias(String, String),
    /// Use these pin pairs for the link (the numbers mirrored, say).
    Pairs(Vec<(String, String)>),
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
    /// The change that settles it, when there is one to offer.
    pub fix: Option<Fix>,
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

/// The pins of two connectors paired the other way round: the first of `a` with the last of `b` ... (a header and a socket facing each
/// other, or a ribbon cable folded over). The shorter connector sets how many.
pub fn mirrored_pairs(a_pins: &[String], b_pins: &[String]) -> Vec<(String, String)> {
    let n = a_pins.len().min(b_pins.len());
    mirror_pairs(Mirror::Reversed, &a_pins[..n], &b_pins[..n]).unwrap_or_default()
}

/// The ways two connectors of the same size can be paired other than number to number, each with its pairs: the numbers the other way
/// round (`Reversed`: a single row facing, or a 2-row connector turned 180°), and for 2-row connectors numbered zigzag (1 and 2 side by
/// side, 3 and 4 next ...) the two flips: across the rows (`RowsSwapped`: 1 ↔ 2, 3 ↔ 4 ...) and end to end (`EndToEnd`: 1 ↔ 2n−1,
/// 2 ↔ 2n ...).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mirror {
    Reversed,
    RowsSwapped,
    EndToEnd,
}

/// The pairs of `a_pins` with `b_pins` (same length) for one way of mirroring; `None` when it does not apply (the 2-row ways on an odd
/// count).
pub fn mirror_pairs(m: Mirror, a_pins: &[String], b_pins: &[String]) -> Option<Vec<(String, String)>> {
    let n = a_pins.len().min(b_pins.len());
    let to = |i: usize| -> usize {
        match m {
            Mirror::Reversed => n - 1 - i,
            Mirror::RowsSwapped => i ^ 1,
            Mirror::EndToEnd => (n - 2 - (i & !1)) | (i & 1),
        }
    };
    if m != Mirror::Reversed && (n < 4 || !n.is_multiple_of(2)) {
        return None;
    }
    Some((0..n).map(|i| (a_pins[i].clone(), b_pins[to(i)].clone())).collect())
}

/// The next way round for a link (`a`, `b`: its two connectors' pins; `rows`: both 2-row): number to number, then each mirror that fits
/// in turn (`Reversed`, and on 2-row connectors `RowsSwapped`, `EndToEnd`), then number to number again. Returns the way (`None`: number
/// to number) and the pairs to store (empty for number to number).
pub fn next_mirror(current: &[(String, String)], a: &[String], b: &[String], rows: bool) -> (Option<Mirror>, Vec<(String, String)>) {
    let ways: &[Mirror] = if rows { &[Mirror::Reversed, Mirror::RowsSwapped, Mirror::EndToEnd] } else { &[Mirror::Reversed] };
    let now = ways.iter().position(|&m| mirror_pairs(m, a, b).is_some_and(|p| p == current));
    let from = now.map_or(0, |i| i + 1);
    ways[from.min(ways.len())..].iter().find_map(|&m| Some((Some(m), mirror_pairs(m, a, b)?))).unwrap_or((None, vec![]))
}

/// Whether a part is a 2-row connector numbered zigzag (a `…2x…` footprint: `PinHeader_2x5`, `AVR_ISP_2x3`).
pub fn two_rows(sch: &Schematic, refdes: &str) -> bool {
    let flat = sch.flat();
    flat.components.iter().find(|c| c.refdes == refdes).is_some_and(|c| c.footprint.to_ascii_lowercase().contains("2x"))
}

fn same(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// Whether net `m` of side A and net `n` of side B are the same signal on `link`: the same name, or two names the link lists as one.
pub fn same_signal(link: &BoardLink, m: &str, n: &str) -> bool {
    same(m, n) || link.aliases.iter().any(|(x, y)| same(x, m) && same(y, n))
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
        let f = |severity, pins: Option<(String, String)>, message: String, hint: &str| Finding { severity, link: k, pins, message, hint: hint.to_string(), fix: None };
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
        // how many pairs have a name on both sides, and how many of those agree
        let score = |pairs: &[(String, String)]| {
            pairs.iter().fold((0, 0), |(ok, named), (p, q)| match (net_a(p), net_b(q)) {
                (Some(m), Some(n)) if role(&m) != Role::Unnamed && role(&n) != Role::Unnamed => (ok + usize::from(same_signal(link, &m, &n)), named + 1),
                _ => (ok, named),
            })
        };
        // the numbers mirrored (a socket facing the header, a folded cable, a 2-row connector turned or flipped): nothing agrees as it is,
        // everything does one of the other ways round
        if link.pairs.is_empty() && pa.len() == pb.len() {
            let (ok, named) = score(&pairs);
            let rows = two_rows(sa, &link.a.refdes) && two_rows(sb, &link.b.refdes);
            let ways: &[Mirror] = if rows { &[Mirror::Reversed, Mirror::RowsSwapped, Mirror::EndToEnd] } else { &[Mirror::Reversed] };
            let found = ok < named
                && ways.iter().find_map(|&m| {
                    let pm = mirror_pairs(m, &pa, &pb)?;
                    let (ok_m, named_m) = score(&pm);
                    (named_m >= 2 && ok_m == named_m && named_m >= named).then_some((m, pm))
                }).map(|found| {
                    let n = pa.len();
                    let (what, hint) = match (found.0, rows) {
                        (Mirror::Reversed, false) => (format!("ピンの番号が逆に並んでいませんか (1 番 ↔ {n} 番にすると、すべての信号が合います)"), "ソケットをピンヘッダに向かい合わせに差すときや、フラットケーブルを折り返すときは、番号が逆になります。そのつなぎ方なら「直す」でピンの組を逆順にします。同じ番号どうしで差すつもりなら、どちらかの基板のピンの並びを直してください"),
                        (Mirror::Reversed, true) => (format!("コネクタが 180° 回っていませんか (1 番 ↔ {n} 番、2 番 ↔ {} 番 … にすると、すべての信号が合います)", n - 1), "2 列のコネクタは、向きの印 (切り欠き・1 番の印) を逆にして差すと 1 番が最後の番号に来ます。そのつなぎ方なら「直す」でピンの組を合わせます。向きを合わせて差すつもりなら、どちらかの基板のピンの並びを直してください"),
                        (Mirror::RowsSwapped, _) => ("2 列のコネクタの列が入れ替わっていませんか (1 番 ↔ 2 番、3 番 ↔ 4 番 … にすると、すべての信号が合います)".to_string(), "2 列のソケットを向かい合わせに差すと、列が入れ替わって奇数と偶数の番号が逆になります。そのつなぎ方なら「直す」でピンの組を合わせます。同じ番号どうしで差すつもりなら、どちらかの基板のピンの並びを直してください"),
                        (Mirror::EndToEnd, _) => (format!("2 列のコネクタの並びが前後逆になっていませんか (1 番 ↔ {} 番、2 番 ↔ {n} 番 … にすると、すべての信号が合います)", n - 1), "2 列のコネクタを、列の向きに裏返して差すと (向かい合わせに差す、ケーブルを縦に折り返す)、並びが前後逆になります。そのつなぎ方なら「直す」でピンの組を合わせます。同じ番号どうしで差すつもりなら、どちらかの基板のピンの並びを直してください"),
                    };
                    out.push(Finding { fix: Some(Fix::Pairs(found.1)), ..f(Severity::Error, None, format!("{}.{} ↔ {}.{}: {what}", link.a.design, link.a.refdes, link.b.design, link.b.refdes), hint) });
                }).is_some();
            if found {
                continue;
            }
        }
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
                    if same_signal(link, m, n) {
                        continue;
                    }
                    let swapped = matches!((rm, rn), (Role::Ground, Role::Power) | (Role::Power, Role::Ground));
                    if swapped {
                        out.push(f(Severity::Error, pins, format!("{label}: 電源 ({}) と GND ({}) がつながります", if rm == Role::Power { m } else { n }, if rm == Role::Ground { m } else { n }), "このままつなぐと電源がショートします。ピンの並びか、どちらかの基板の配線を直してください"));
                        continue;
                    }
                    // the other side's net on another pin of this link: two pins crossed
                    let crossed = pairs.iter().find(|(p2, q2)| (p2, q2) != (p, q) && net_b(q2).is_some_and(|z| same_signal(link, m, &z)));
                    let hint = "コネクタの向き (1 番ピンの位置) や、ケーブルが入れ替わっていないか確かめます。名前の付け方が違うだけ (例: SDA と I2C_SDA) なら、どちらかの名前を合わせるか、「同じ信号にする」で名前の対応をつなぎに登録します";
                    match crossed {
                        Some((_, q2)) => out.push(f(Severity::Error, pins, format!("{label}: {m} と {n} が違います ({m} は相手の {q2} 番にあります。入れ替わっていませんか)"), hint)),
                        None => out.push(Finding { fix: Some(Fix::Alias(m.clone(), n.clone())), ..f(Severity::Warning, pins, format!("{label}: {m} と {n} の名前が違います"), hint) }),
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

/// A connector pin of the merged circuit (`merge`): the link and its pins, the 0 V source that joins the two sides (its current is the
/// current from side A to side B through the pin), and the nets of the two sides in the merged circuit.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkProbe {
    pub link: usize,
    pub pins: (String, String),
    /// The 0 V source's designator; empty when both sides are ground (they are one net already).
    pub source: String,
    pub net_a: String,
    pub net_b: String,
    /// The names of the two sides in the simulated circuit (the labels put on the connector pins; `GND` on ground).
    pub node_a: String,
    pub node_b: String,
    /// How many pins share the source (pins of the two connectors on the same two nets, such as two +5V pins: one source for them all,
    /// since two 0 V sources side by side cannot be solved; its current is their total).
    pub shared: usize,
}

/// A name usable in a designator: the board's name with anything but ASCII letters and digits left out (`B2` when nothing is left).
fn tag(name: &str, k: usize) -> String {
    let t: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if t.is_empty() { format!("B{}", k + 1) } else { t }
}

/// A net of board `board` in the merged circuit: ground stays the one ground of every board; every other name gets the board's name in
/// front (`main.SDA`), so that two boards' nets of the same name stay apart unless a link joins them.
fn merged_net(board: &str, net: &str) -> String {
    if role(net) == Role::Ground { if net == "0" { "0".into() } else { "GND".into() } } else { format!("{board}.{net}") }
}

/// The whole system as one circuit, to simulate (E5, stage 3): every board's schematic (flattened) side by side, the designators with the
/// board's name behind them (`R1_main`), the nets with it in front (`main.SDA`, ground shared), and for every pin of every link a 0 V
/// source for the simulation only between the two sides (labels on the two connector pins): the sides are joined, and the source's current
/// is what flows through the pin. A pin pair with both sides on ground needs no source; a pin with nothing on one side is left out.
pub fn merge(designs: &[(&str, &Schematic)], links: &[BoardLink]) -> Result<(Schematic, Vec<LinkProbe>), String> {
    use crate::{Component, Kind, Label, Wire};
    let mut out = Schematic::default();
    let mut x0 = 0i32;
    // where each board's connector pins landed, by (board, designator, pin)
    let mut pin_at: HashMap<(String, String, String), crate::Pt> = HashMap::new();
    for (k, (name, sch)) in designs.iter().enumerate() {
        let t = tag(name, k);
        let flat = sch.flat();
        let xs = flat.components.iter().map(|c| c.pos.0).chain(flat.wires.iter().flat_map(|w| [w.a.0, w.b.0])).chain(flat.labels.iter().map(|l| l.pos.0));
        let (lo, hi) = xs.fold((i32::MAX, i32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
        let (lo, hi) = if lo > hi { (0, 0) } else { (lo, hi) };
        let dx = x0 - lo + 20;
        let shift = |p: crate::Pt| (p.0 + dx, p.1);
        for c in &flat.components {
            let mut c: Component = c.clone();
            c.pos = shift(c.pos);
            if c.kind == Kind::Power {
                c.value = merged_net(name, &c.value);
            }
            if !c.kind.is_power_symbol() {
                for (pin, at) in c.pins() {
                    pin_at.insert((name.to_string(), c.refdes.clone(), pin), at);
                }
                c.refdes = format!("{}_{t}", c.refdes);
            }
            out.components.push(c);
        }
        out.wires.extend(flat.wires.iter().map(|w| Wire { a: shift(w.a), b: shift(w.b) }));
        out.labels.extend(flat.labels.iter().map(|l| Label { pos: shift(l.pos), text: merged_net(name, &l.text) }));
        out.thermal_links.extend(flat.thermal_links.iter().map(|(a, b, g)| (format!("{a}_{t}"), format!("{b}_{t}"), *g)));
        x0 += hi - lo + 60;
    }
    // the 0 V sources, in a column to the right of every board
    let mut probes = vec![];
    let nets: HashMap<&str, HashMap<(String, String), String>> = designs.iter().map(|(n, s)| (*n, pin_nets(s))).collect();
    let mut y = 0;
    for (k, link) in links.iter().enumerate() {
        let find = |n: &str| designs.iter().find(|(d, _)| *d == n).map(|(_, s)| *s);
        let (Some(sa), Some(sb)) = (find(&link.a.design), find(&link.b.design)) else { return Err(format!("基板「{}」か「{}」がありません", link.a.design, link.b.design)) };
        let (Some(pa), Some(pb)) = (pins_of(sa, &link.a.refdes), pins_of(sb, &link.b.refdes)) else { return Err(format!("{}.{} か {}.{} が回路図にありません", link.a.design, link.a.refdes, link.b.design, link.b.refdes)) };
        for (p, q) in pin_pairs(link, &pa, &pb) {
            let na = nets[link.a.design.as_str()].get(&(link.a.refdes.clone(), p.clone())).cloned();
            let nb = nets[link.b.design.as_str()].get(&(link.b.refdes.clone(), q.clone())).cloned();
            let (Some(na), Some(nb)) = (na, nb) else { continue };
            let (ma, mb) = (merged_net(&link.a.design, &na), merged_net(&link.b.design, &nb));
            let mut probe = LinkProbe { link: k, pins: (p.clone(), q.clone()), source: String::new(), net_a: ma.clone(), net_b: mb.clone(), node_a: ma.clone(), node_b: mb.clone(), shared: 1 };
            if role(&na) == Role::Ground && role(&nb) == Role::Ground {
                probes.push(probe);
                continue;
            }
            // the source's two ends named after the two sides' nets; a net without a name gets one on the connector pin
            // (`main.J1-3`), readable in the merged schematic
            let mut side = |board: &str, refdes: &str, pin: &str, net: &str, merged: &str| -> Result<String, String> {
                if role(net) != Role::Unnamed {
                    return Ok(merged.to_string());
                }
                let name = format!("{board}.{refdes}-{pin}");
                let at = pin_at.get(&(board.to_string(), refdes.to_string(), pin.to_string())).copied().ok_or("コネクタのピンの位置がわかりません")?;
                out.labels.push(Label { pos: at, text: name.clone() });
                Ok(name)
            };
            let la = side(&link.a.design, &link.a.refdes, &p, &na, &ma)?;
            let lb = side(&link.b.design, &link.b.refdes, &q, &nb, &mb)?;
            // the same two nets already joined (another pin of the same supply): the same source carries this pin too
            if role(&na) != Role::Unnamed && role(&nb) != Role::Unnamed
                && let Some(prev) = probes.iter().position(|x: &LinkProbe| !x.source.is_empty() && x.net_a == ma && x.net_b == mb)
            {
                probe.source = probes[prev].source.clone();
                (probe.node_a, probe.node_b) = (la, lb);
                let n = probes.iter().filter(|x| x.source == probe.source).count() + 1;
                for x in probes.iter_mut().filter(|x| x.source == probe.source) {
                    x.shared = n;
                }
                probe.shared = n;
                probes.push(probe);
                continue;
            }
            let refdes = format!("VLINK{}_{}", k + 1, p);
            let i = out.add(Kind::VSource, (x0 + 20, y), 0);
            let v = &mut out.components[i];
            v.refdes = refdes.clone();
            v.value = "0".into();
            v.sim_only = true;
            let pins = v.pins();
            for (pin, at) in &pins {
                out.labels.push(Label { pos: *at, text: if pin == "+" { la.clone() } else { lb.clone() } });
            }
            // the next source clear of this one's pins (a pin of one landing on a pin of the next would join two links)
            let (top, bottom) = pins.iter().fold((i32::MAX, i32::MIN), |(t, b), (_, at)| (t.min(at.1), b.max(at.1)));
            y += (bottom - top).max(0) + 6;
            probe.source = refdes;
            (probe.node_a, probe.node_b) = (la, lb);
            probes.push(probe);
        }
    }
    Ok((out, probes))
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
    /// A remark for the buyer (the cables' wire: the length to cut).
    pub note: String,
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
            let row = rows.entry(key).or_insert_with(|| BomRow { part, value: c.value.clone(), footprint: c.footprint.clone(), shops: c.custom.as_ref().map(|s| s.shops.clone()).unwrap_or_default(), per_board: vec![], total: 0, note: String::new() });
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
    head.push("メモ".into());
    out.push_str(&head.join(","));
    out.push('\n');
    for r in rows {
        let mut cols = vec![q(&r.part), q(&r.value), q(&r.footprint), r.total.to_string()];
        for b in boards {
            cols.push(q(&r.per_board.iter().find(|(n, _)| n == b).map(|(_, refs)| refs.join(" ")).unwrap_or_default()));
        }
        cols.push(q(&r.shops.iter().map(|(s, n)| format!("{s}:{n}")).collect::<Vec<_>>().join(" ")));
        cols.push(q(&r.note));
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
        let c = Component { refdes: header.into(), kind: Kind::Header(names.len() as u8), value: String::new(), pos: (10, 10), rot: 0, footprint: String::new(), custom: None, sim_only: false, pin_names: vec![], theta: 0.0, tc_ppm: 0.0, cth: 0.0, t_max: 0.0, zth: vec![], theta_jc: 0.0, heatsink: 0.0, sub: None, shops: Default::default() };
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
        BoardLink { a: LinkEnd { design: "main".into(), refdes: "J1".into() }, b: LinkEnd { design: "sensor".into(), refdes: "J1".into() }, pairs: pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(), aliases: vec![], cable: None, colors: vec![] }
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
        // the offered fix: the two names as one signal on the link, and then nothing to say
        assert_eq!(f[0].fix, Some(Fix::Alias("SDA".into(), "I2C_SDA".into())));
        let mut named = link(&[]);
        named.aliases.push(("sda".into(), "i2c_sda".into()));
        assert!(check(&[("main", &main), ("sensor", &renamed)], std::slice::from_ref(&named)).is_empty());
        // the alias also counts when telling crossed pins: I2C_SDA on pin 4 is the SDA of pin 3
        let renamed_crossed = board("J1", &["+5V", "GND", "SCL", "I2C_SDA"]);
        let f = check(&[("main", &main), ("sensor", &renamed_crossed)], &[named]);
        assert!(f.iter().any(|x| x.message.contains("入れ替わって") && x.message.contains("4 番")), "{f:?}");
        let open = board("J1", &["+5V", "GND", "SDA", ""]);
        let f = check(&[("main", &main), ("sensor", &open)], &[link(&[])]);
        assert!(f.len() == 1 && f[0].message.contains("片側だけ"), "{f:?}");
    }

    /// A socket facing the header numbers its pins the other way round: one error offering the mirrored pairs instead of four, and with
    /// those pairs nothing to say. A mix-up that the mirror does not settle stays pin by pin.
    #[test]
    fn mirrored_numbers_are_told_as_one() {
        let main = board("J1", &["+5V", "GND", "SDA", "SCL"]);
        let facing = board("J1", &["SCL", "SDA", "GND", "+5V"]);
        let f = check(&[("main", &main), ("sensor", &facing)], &[link(&[])]);
        assert_eq!(f.len(), 1, "{f:?}");
        assert!(f[0].severity == Severity::Error && f[0].message.contains("逆に"), "{f:?}");
        let Some(Fix::Pairs(pairs)) = &f[0].fix else { panic!("{f:?}") };
        assert_eq!(pairs[0], ("1".to_string(), "4".to_string()));
        let fixed = BoardLink { pairs: pairs.clone(), ..link(&[]) };
        assert!(check(&[("main", &main), ("sensor", &facing)], &[fixed]).is_empty());
        let jumbled = board("J1", &["SDA", "GND", "+5V", "SCL"]);
        let f = check(&[("main", &main), ("sensor", &jumbled)], &[link(&[])]);
        assert!(f.iter().all(|x| !x.message.contains("逆に")) && f.len() > 1, "{f:?}");
    }

    /// 2-row connectors (zigzag numbering, a `2x` footprint): flipped across the rows, flipped end to end and turned 180° are each told
    /// with their own pairs; the same wrong order on single-row headers is not called a mirror (it is not one there).
    #[test]
    fn two_row_connectors_are_told_how_they_are_flipped() {
        let names = ["+5V", "GND", "SDA", "SCL", "MOSI", "MISO"];
        let rows = |order: [usize; 6], fp: &str| {
            let mut s = board("J1", &order.map(|i| names[i]));
            s.components[0].footprint = fp.into();
            s
        };
        let main = rows([0, 1, 2, 3, 4, 5], "PinHeader_2x3");
        for (order, want, first) in [
            ([1, 0, 3, 2, 5, 4], "列が入れ替わって", ("1", "2")),
            ([4, 5, 2, 3, 0, 1], "前後逆", ("1", "5")),
            ([5, 4, 3, 2, 1, 0], "180°", ("1", "6")),
        ] {
            let other = rows(order, "PinHeader_2x3");
            let f = check(&[("main", &main), ("sensor", &other)], &[link(&[])]);
            assert!(f.len() == 1 && f[0].message.contains(want), "{order:?}: {f:?}");
            let Some(Fix::Pairs(pairs)) = &f[0].fix else { panic!("{f:?}") };
            assert_eq!((pairs[0].0.as_str(), pairs[0].1.as_str()), first);
            assert!(check(&[("main", &main), ("sensor", &other)], &[BoardLink { pairs: pairs.clone(), ..link(&[]) }]).is_empty());
        }
        // the button's turn: number to number → reversed → rows swapped → end to end → number to number
        let pins: Vec<String> = (1..=6).map(|i| i.to_string()).collect();
        let mut pairs = vec![];
        let mut seen = vec![];
        for _ in 0..4 {
            let (m, p) = next_mirror(&pairs, &pins, &pins, true);
            seen.push(m);
            pairs = p;
        }
        assert_eq!(seen, vec![Some(Mirror::Reversed), Some(Mirror::RowsSwapped), Some(Mirror::EndToEnd), None]);
        assert_eq!(next_mirror(&mirrored_pairs(&pins, &pins), &pins, &pins, false), (None, vec![]));
        // single-row: swapping neighbours is not a mirror
        let (main1, other1) = (rows([0, 1, 2, 3, 4, 5], "PinHeader_1x6"), rows([1, 0, 3, 2, 5, 4], "PinHeader_1x6"));
        let f = check(&[("main", &main1), ("sensor", &other1)], &[link(&[])]);
        assert!(f.iter().all(|x| x.fix.as_ref().is_none_or(|fx| !matches!(fx, Fix::Pairs(_)))), "{f:?}");
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
