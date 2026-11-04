//! Buses: a label such as `D[0..7]` names the eight nets `D0` ... `D7` as a group. A bus line (`Schematic::buses`) is a
//! picture only: it connects nothing, and a bus label does not name a net. The pins are joined by ordinary labels `D0` ... `D7`
//! (the editor numbers them one after another), so the netlist needs no expansion; what the bus adds is the checks below.

use crate::netlist::{Net, Problem, Severity};
use crate::{Pt, Schematic};
use std::collections::{BTreeMap, BTreeSet};

/// The widest bus.
pub const MAX_BUS: u32 = 256;

/// A bus label: its base name and first / last number, as in `D[0..7]`, `D[7:0]` or `D[0-7]`.
pub fn parse_bus(text: &str) -> Option<(String, u32, u32)> {
    let t = text.trim();
    let inner = t.strip_suffix(']')?;
    let (base, range) = inner.rsplit_once('[')?;
    if base.is_empty() || !base.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    let (a, b) = range.split_once("..").or_else(|| range.split_once(':')).or_else(|| range.split_once('-'))?;
    let (a, b): (u32, u32) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
    (a.abs_diff(b) < MAX_BUS).then(|| (base.to_string(), a, b))
}

pub fn is_bus_label(text: &str) -> bool {
    parse_bus(text).is_some()
}

/// The nets a bus label stands for, in the order of the label (`D[7:0]` gives D7 first).
pub fn bus_members(text: &str) -> Option<Vec<String>> {
    let (base, a, b) = parse_bus(text)?;
    let nums: Vec<u32> = if a <= b { (a..=b).collect() } else { (b..=a).rev().collect() };
    Some(nums.into_iter().map(|n| format!("{base}{n}")).collect())
}

/// The name of member `k` (0 = the first) of the bus `text`; `None` past the end.
pub fn bus_member(text: &str, k: usize) -> Option<String> {
    bus_members(text)?.into_iter().nth(k)
}

fn on_segment(p: Pt, a: Pt, b: Pt) -> bool {
    let cross = (b.0 - a.0) as i64 * (p.1 - a.1) as i64 - (b.1 - a.1) as i64 * (p.0 - a.0) as i64;
    cross == 0 && p.0 >= a.0.min(b.0) && p.0 <= a.0.max(b.0) && p.1 >= a.1.min(b.1) && p.1 <= a.1.max(b.1)
}

/// The checks of the buses of the (flat) sheet `s`; `nets` are its nets.
pub(crate) fn bus_problems(s: &Schematic, nets: &[Net]) -> Vec<Problem> {
    use Severity::Warning;
    let mut out = vec![];
    let pins_of: BTreeMap<&str, usize> = nets.iter().map(|n| (n.name.as_str(), n.pins.len())).collect();
    let mut declared: BTreeMap<String, Vec<(u32, u32, Pt)>> = BTreeMap::new();
    let mut seen: BTreeSet<(String, u32, u32, usize)> = BTreeSet::new();
    for l in &s.labels {
        let Some((base, a, b)) = parse_bus(&l.text) else { continue };
        let sheet = crate::sheets::sheet_of(l.pos).0;
        if !s.buses.iter().any(|w| on_segment(l.pos, w.a, w.b)) {
            out.push(Problem::new(
                Warning,
                format!("バスのラベル {} が、バスの線の上にありません", l.text),
                Some(l.pos),
                "バスのラベルは、バスの線の上に置きます (「バス」ツールで線を引けます)。線から離れていると、どの線の名前か分かりません",
            ));
        }
        if !seen.insert((base.clone(), a, b, sheet)) {
            continue;
        }
        declared.entry(base.clone()).or_default().push((a.min(b), a.max(b), l.pos));
        for m in bus_members(&l.text).unwrap_or_default() {
            match pins_of.get(m.as_str()) {
                None => out.push(Problem::new(
                    Warning,
                    format!("バス {} の {m} が、どこにもつながっていません", l.text),
                    Some(l.pos),
                    &format!("バスの範囲に入っているのに、{m} というラベルのついた配線がありません。番号を数え間違えたか、{m} をつなぎ忘れています。使わない番号なら、バスの範囲を狭めましょう"),
                )),
                Some(1) => out.push(Problem::new(
                    Warning,
                    format!("バス {} の {m} が、部品のピン1つにしかつながっていません", l.text),
                    Some(l.pos),
                    &format!("{m} は、もう片方の端に相手がありません。バスは両端をつなぐものです。相手のピンに {m} のラベルを付けましょう"),
                )),
                _ => {}
            }
        }
    }
    for (base, list) in &declared {
        let first = list[0];
        if let Some(other) = list.iter().find(|r| (r.0, r.1) != (first.0, first.1)) {
            out.push(Problem::new(
                Warning,
                format!("バス {base} の幅が食い違っています: [{}..{}] と [{}..{}]", first.0, first.1, other.0, other.1),
                Some(other.2),
                "同じ名前のバスなのに、番号の範囲が違います。片方の端が足りないか余っています。幅をそろえましょう (別のバスなら、名前を変えます)",
            ));
        }
    }
    // a numbered net next to a bus of that name, outside its range
    for n in nets {
        let digits = n.name.chars().rev().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 || digits == n.name.len() {
            continue;
        }
        let (base, num) = n.name.split_at(n.name.len() - digits);
        let (Some(list), Ok(num)) = (declared.get(base), num.parse::<u32>()) else { continue };
        if !list.iter().any(|r| num >= r.0 && num <= r.1) {
            out.push(Problem::new(
                Warning,
                format!("{} は、バス {base}[{}..{}] の範囲の外です", n.name, list[0].0, list[0].1),
                Some(list[0].2),
                "同じ名前で始まる番号つきの配線が、バスの範囲に入っていません。バスの範囲を広げるか、名前を変えましょう (関係のない配線なら、このままで構いません)",
            ));
        }
    }
    out
}
