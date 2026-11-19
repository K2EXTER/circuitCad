//! Design review (N1): the slips that pass the connection checks but fail on the bench. A part the data sheet asks for is
//! missing or too small (a regulator's input and output capacitors, a reset pull-up: the part's `companions`), a crystal's two
//! capacitors do not make its load capacitance (`PartRatings::load_pf`), an LED hangs straight off an IC's output pin, and a
//! power inlet has no reverse-polarity protection (information only). What a part is comes from its data (companions, glyph,
//! ratings, pin kinds), never from its name. The findings join the ERC's (`erc_all`).

use crate::netlist::{Net, Problem, Severity};
use crate::{Component, Kind, PinKind, Pt, Schematic};
use std::collections::HashMap;

/// The stray capacitance (pF) the oscillator's pins and the board add to a crystal's load: the usual figure when the IC's data
/// sheet gives none.
pub const STRAY_PF: f64 = 5.0;

/// How far (as a share of CL) the load the two capacitors make may stray from the crystal's CL before it is pointed out.
const LOAD_TOLERANCE: f64 = 0.25;

/// A capacitor below this share of the recommended value is "too small" (a smaller standard value next to it is fine).
const TOO_SMALL: f64 = 0.5;

/// What a two-terminal part is, by its kind or its library glyph: 'R', 'C', 'L' or 'D' (a diode of any sort).
fn role(c: &Component) -> Option<char> {
    match c.kind {
        Kind::Resistor => Some('R'),
        Kind::Capacitor => Some('C'),
        Kind::Inductor => Some('L'),
        Kind::Diode => Some('D'),
        Kind::Custom => {
            let sym = c.custom.as_ref()?;
            let glyph = if sym.glyph.is_empty() { sym.inferred_glyph() } else { sym.glyph.as_str() };
            match glyph {
                "resistor" => Some('R'),
                "capacitor" | "electrolytic" => Some('C'),
                "inductor" => Some('L'),
                "diode" | "zener" | "schottky" | "tvs" => Some('D'),
                _ if sym.pins.len() == 2 && sym.sim.as_ref().is_some_and(|m| m.element.eq_ignore_ascii_case("D")) => Some('D'),
                _ => None,
            }
        }
        _ => None,
    }
}

fn glyph_of(c: &Component) -> &str {
    c.custom.as_ref().map_or("", |s| if s.glyph.is_empty() { s.inferred_glyph() } else { s.glyph.as_str() })
}

/// A number with an SI prefix ("10u", "22µF", "100n", "4.7k", "1Meg"); None when it does not start with one.
pub(crate) fn si_value(text: &str) -> Option<f64> {
    let t = text.trim();
    let end = t.find(|ch: char| !(ch.is_ascii_digit() || ch == '.' || ch == '-' || ch == '+')).unwrap_or(t.len());
    let num: f64 = t[..end].parse().ok()?;
    let rest = t[end..].trim();
    let lower = rest.to_ascii_lowercase();
    let mult = if lower.starts_with("meg") {
        1e6
    } else {
        match rest.chars().next() {
            Some('p') => 1e-12,
            Some('n') => 1e-9,
            Some('u') | Some('µ') | Some('μ') => 1e-6,
            Some('m') => 1e-3,
            Some('k') | Some('K') => 1e3,
            Some('M') => 1e6,
            Some('G') => 1e9,
            _ => 1.0,
        }
    };
    Some(num * mult)
}

/// A capacitance for people: "22pF", "100nF", "10µF".
fn farads(x: f64) -> String {
    let (v, unit) = if x >= 1e-6 * 0.999 { (x * 1e6, "µF") } else if x >= 1e-9 * 0.999 { (x * 1e9, "nF") } else { (x * 1e12, "pF") };
    let r = (v * 100.0).round() / 100.0;
    format!("{r}{unit}")
}

/// The nearest E12 value to `pf` (pF).
fn e12(pf: f64) -> f64 {
    const E12: [f64; 12] = [1.0, 1.2, 1.5, 1.8, 2.2, 2.7, 3.3, 3.9, 4.7, 5.6, 6.8, 8.2];
    if pf <= 0.0 {
        return 0.0;
    }
    let decade = 10f64.powf(pf.log10().floor());
    let mut best = E12[0] * decade;
    for d in [decade, decade * 10.0] {
        for e in E12 {
            if ((e * d).ln() - pf.ln()).abs() < (best.ln() - pf.ln()).abs() {
                best = e * d;
            }
        }
    }
    (best * 10.0).round() / 10.0
}

struct Ctx<'a> {
    s: &'a Schematic,
    nets: &'a [Net],
    comp: HashMap<&'a str, &'a Component>,
    net_of: HashMap<(&'a str, &'a str), usize>,
    gnd: Option<usize>,
}

impl<'a> Ctx<'a> {
    fn new(s: &'a Schematic, nets: &'a [Net]) -> Self {
        let mut comp = HashMap::new();
        for c in &s.components {
            comp.entry(c.refdes.as_str()).or_insert(c);
        }
        let mut net_of = HashMap::new();
        for (i, n) in nets.iter().enumerate() {
            for p in &n.pins {
                net_of.entry((p.refdes.as_str(), p.pin.as_str())).or_insert(i);
            }
        }
        let gnd = nets.iter().position(|n| n.name == "GND");
        Self { s, nets, comp, net_of, gnd }
    }

    fn net(&self, refdes: &str, pin: &str) -> Option<usize> {
        self.net_of.get(&(refdes, pin)).copied()
    }

    /// The nets of a two-terminal part's ends.
    fn ends(&self, c: &Component) -> Option<(usize, usize)> {
        let pins = c.pins();
        if pins.len() != 2 {
            return None;
        }
        Some((self.net(&c.refdes, &pins[0].0)?, self.net(&c.refdes, &pins[1].0)?))
    }

    /// The parts of `role` that sit between the nets `a` and `b`.
    fn between(&self, kind: char, a: usize, b: usize) -> Vec<&'a Component> {
        self.s
            .components
            .iter()
            .filter(|c| role(c) == Some(kind))
            .filter(|c| self.ends(c).is_some_and(|(x, y)| (x == a && y == b) || (x == b && y == a)))
            .collect()
    }

    /// The library pin `pin` of the part on the net: its name and kind.
    fn lib_pin(&self, refdes: &str, pin: &str) -> Option<(&'a str, Option<PinKind>)> {
        let sym = self.comp.get(refdes)?.custom.as_ref()?;
        sym.pins.iter().find(|q| q.number == pin).map(|q| (q.name.as_str(), q.kind))
    }

    /// Something other than `me` gives the net its voltage: a source, a regulator's output, a power symbol's net that a source feeds.
    fn sourced(&self, n: usize, me: &str) -> bool {
        self.nets[n].pins.iter().any(|p| {
            p.refdes != me
                && (self.comp.get(p.refdes.as_str()).is_some_and(|c| c.kind == Kind::VSource)
                    || self.lib_pin(&p.refdes, &p.pin).is_some_and(|(name, kind)| {
                        let n = name.to_ascii_uppercase();
                        kind == Some(PinKind::PowerOut) || n == "OUT" || n == "VOUT" || n == "VO"
                    }))
        })
    }

    /// A supply net: named by a power symbol, or fed to an IC's supply input.
    fn supply(&self, n: usize) -> bool {
        let name = &self.nets[n].name;
        if name == "GND" {
            return false;
        }
        self.s.components.iter().any(|c| c.kind == Kind::Power && c.value == *name)
            || self.nets[n].pins.iter().any(|p| {
                self.lib_pin(&p.refdes, &p.pin).is_some_and(|(pin, kind)| {
                    let u = pin.to_ascii_uppercase();
                    (kind == Some(PinKind::PowerIn) && !u.contains("GND")) || ["VCC", "VDD", "VIN", "IN"].contains(&u.as_str())
                })
            })
    }

    fn pin_at(c: &Component, pin: &str) -> Option<Pt> {
        c.pins().into_iter().find(|(n, _)| n == pin).map(|(_, at)| at)
    }
}

pub(crate) fn review_problems(s: &Schematic, nets: &[Net]) -> Vec<Problem> {
    let x = Ctx::new(s, nets);
    let mut out = companions(&x);
    out.extend(crystals(&x));
    out.extend(leds_on_outputs(&x));
    out.extend(power_inlets(&x));
    out
}

/// A companion's pin, what it goes to, and its kind.
type Key<'a> = (&'a str, &'a str, &'a str);

/// The parts the data sheet asks for beside a library part (`companions`): there, and a capacitor not far below its value.
fn companions(x: &Ctx) -> Vec<Problem> {
    let mut out = vec![];
    for c in x.s.components.iter().filter(|c| !c.sim_only) {
        let Some(sym) = c.custom.as_ref().filter(|s| !s.companions.is_empty()) else { continue };
        // the companions that go between the same two points as one (an output's 100 nF and 10 µF)
        let mut groups: Vec<(Key, Vec<&crate::Companion>)> = vec![];
        for comp in &sym.companions {
            let key = (comp.pin.as_str(), comp.to.as_str(), comp.kind.as_str());
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some((_, v)) => v.push(comp),
                None => groups.push((key, vec![comp])),
            }
        }
        for ((pin, to, kind), list) in groups {
            let Some(k) = kind.chars().next().map(|ch| ch.to_ascii_uppercase()).filter(|ch| matches!(ch, 'R' | 'C' | 'L')) else { continue };
            let Some(p) = x.net(&c.refdes, pin) else { continue };
            if x.nets[p].pins.len() < 2 {
                continue; // the pin is not wired yet: the ERC says so already
            }
            let t = if to.eq_ignore_ascii_case("GND") { x.gnd } else { x.net(&c.refdes, to) };
            if t == Some(p) {
                continue; // tied straight to its target (a reset pin on VCC): nothing is needed
            }
            let present = t.map_or(vec![], |t| x.between(k, p, t));
            let pin_name = x.lib_pin(&c.refdes, pin).map_or(String::new(), |(n, _)| format!(" ({n})"));
            let to_name = if to.eq_ignore_ascii_case("GND") { "GND".to_string() } else { x.lib_pin(&c.refdes, to).map_or(format!("ピン{to}"), |(n, _)| n.to_string()) };
            let names = list.iter().map(|q| q.name.as_str()).collect::<Vec<_>>().join("・");
            let values = list.iter().map(|q| q.value.as_str()).collect::<Vec<_>>().join(" + ");
            let word = match k {
                'C' => "コンデンサ",
                'R' => "抵抗",
                _ => "コイル",
            };
            let notes: Vec<&str> = list.iter().map(|q| q.note.as_str()).filter(|n| !n.is_empty()).collect();
            let note = if notes.is_empty() { String::new() } else { format!(" (データシートの注: {})", notes.join(" / ")) };
            let at = Ctx::pin_at(c, pin).or(Some(c.pos));
            let why = match k {
                'C' => "無いか小さすぎると、レギュレータの出力が発振したり、負荷が変わったときに電圧がふらついたりします。シミュレーションでは理想に近く動いても、実物で困る所です",
                'R' => "無いと、ピンの電圧が決まらず、雑音で勝手にリセットがかかったり、動作が止まったりします",
                _ => "無いと、データシートのとおりに動きません",
            };
            if present.is_empty() {
                out.push(Problem::new(
                    Severity::Warning,
                    format!("{} の推奨の{word}がありません: {names} ({values}、ピン{pin}{pin_name} と {to_name} の間)", c.refdes),
                    at,
                    &format!(
                        "データシートが、{} のこのピンのそばに付けるように言っている{word}です{note}。{why}。部品を選んで、プロパティの「推奨の周辺部品を置く」で置けます。基板では、ピンのすぐ近くに置きましょう",
                        c.refdes
                    ),
                ));
            } else if k == 'C' {
                let want: f64 = list.iter().filter_map(|q| si_value(&q.value)).sum();
                let have: f64 = present.iter().filter_map(|q| si_value(&q.value)).sum();
                if want > 0.0 && have > 0.0 && have < want * TOO_SMALL {
                    out.push(Problem::new(
                        Severity::Warning,
                        format!("{} のピン{pin}{pin_name} のコンデンサが小さすぎます: {} (推奨 {values})", c.refdes, farads(have)),
                        at,
                        &format!("データシートは、このピンと {to_name} の間に {values} を推奨しています{note}。{why}。値を推奨の値以上にしましょう (電解コンデンサやセラミックの容量は、温度や電圧で減ることも見込みます)"),
                    ));
                }
            }
        }
    }
    out
}

/// A crystal beside an oscillator IC: a capacitor from each end to GND, and the two (in series, plus the stray) near its CL.
fn crystals(x: &Ctx) -> Vec<Problem> {
    let mut out = vec![];
    for c in x.s.components.iter().filter(|c| !c.sim_only && glyph_of(c) == "crystal") {
        let Some(sym) = &c.custom else { continue };
        let Some(cl) = sym.ratings.as_ref().and_then(|r| r.load_pf) else { continue };
        let ends: Vec<&str> = sym.pins.iter().filter(|p| !p.name.to_ascii_uppercase().contains("GND")).map(|p| p.number.as_str()).collect();
        let [a, b] = ends[..] else { continue };
        let (Some(na), Some(nb)) = (x.net(&c.refdes, a), x.net(&c.refdes, b)) else { continue };
        // only a crystal that an IC drives (a crystal on its own is an AC-analysis exercise)
        let ic_on = |n: usize| x.nets[n].pins.iter().any(|p| p.refdes != c.refdes && x.comp.get(p.refdes.as_str()).is_some_and(|o| o.kind == Kind::Custom && role(o).is_none() && glyph_of(o) != "crystal"));
        if !(ic_on(na) || ic_on(nb)) {
            continue;
        }
        let cap = |n: usize| -> f64 { x.gnd.map_or(0.0, |g| x.between('C', n, g).iter().filter_map(|q| si_value(&q.value)).sum::<f64>() * 1e12) };
        let (c1, c2) = (cap(na), cap(nb));
        let rec = e12(if cl > STRAY_PF { 2.0 * (cl - STRAY_PF) } else { cl });
        let hint = format!(
            "水晶は、両端から GND へのコンデンサ 2 個と、配線・ピンの浮遊容量 (約 {STRAY_PF}pF) を合わせた負荷容量 CL で、データシートの周波数になります。CL = C1×C2÷(C1+C2) + 浮遊容量。合わないと周波数がずれ、ひどいと発振しません。この水晶 (CL = {cl}pF) には、{rec}pF を両端に 1 個ずつが目安です (C = 2×(CL − 浮遊容量))。IC のデータシートが値を言っていれば、そちらに従います"
        );
        let at = Some(c.pos);
        if c1 <= 0.0 && c2 <= 0.0 {
            out.push(Problem::new(Severity::Warning, format!("{} (水晶) の両端に負荷コンデンサがありません (目安 {rec}pF × 2)", c.refdes), at, &hint));
        } else if c1 <= 0.0 || c2 <= 0.0 {
            let pin = if c1 <= 0.0 { a } else { b };
            out.push(Problem::new(Severity::Warning, format!("{} (水晶) のピン{pin} 側に、GND への負荷コンデンサがありません", c.refdes), at, &hint));
        } else {
            let made = c1 * c2 / (c1 + c2) + STRAY_PF;
            if (made - cl).abs() > cl * LOAD_TOLERANCE {
                out.push(Problem::new(
                    Severity::Warning,
                    format!("{} (水晶) の負荷容量が合いません: {}pF と {}pF で約 {:.1}pF、水晶は {cl}pF (目安 {rec}pF × 2)", c.refdes, c1.round(), c2.round(), made),
                    at,
                    &hint,
                ));
            }
        }
    }
    out
}

/// An LED wired straight from an IC's output pin to ground or a supply: nothing sets its current.
fn leds_on_outputs(x: &Ctx) -> Vec<Problem> {
    let mut out = vec![];
    for c in x.s.components.iter().filter(|c| c.kind == Kind::Led || glyph_of(c) == "led") {
        let pins = c.pins();
        if pins.len() != 2 {
            continue;
        }
        let (Some(a), Some(k)) = (x.net(&c.refdes, &pins[0].0), x.net(&c.refdes, &pins[1].0)) else { continue };
        let output = |n: usize| {
            x.nets[n].pins.iter().find(|p| p.refdes != c.refdes && x.lib_pin(&p.refdes, &p.pin).is_some_and(|(_, kind)| matches!(kind, Some(PinKind::Output | PinKind::Bidir))))
        };
        let ret = |n: usize| x.nets[n].name == "GND" || x.supply(n) || x.sourced(n, &c.refdes);
        let hit = if ret(k) { output(a) } else { None }.or_else(|| if ret(a) { output(k) } else { None });
        if let Some(p) = hit {
            let name = x.lib_pin(&p.refdes, &p.pin).map_or(String::new(), |(n, _)| format!(" ({n})"));
            out.push(Problem::new(
                Severity::Warning,
                format!("{} が IC の出力ピン {}.{}{name} に直接つながっていて、電流を制限する抵抗がありません", c.refdes, p.refdes, p.pin),
                Some(c.pos),
                "LED は電圧がわずかに上がるだけで電流が急に増えます。IC の出力に直接つなぐと、出力の許す最大の電流が流れ、LED か IC の出力が傷みます (マイコンのピンは 1 本 20mA くらいまで)。出力と LED の間に抵抗 (3.3V・5V なら 330Ω〜1kΩ くらい) を入れましょう",
            ));
        }
    }
    out
}

/// A connector that brings the supply in (nothing else feeds the net) with no diode or MOSFET on it: information only.
fn power_inlets(x: &Ctx) -> Vec<Problem> {
    let mut out = vec![];
    let connector = |c: &Component| match c.kind {
        Kind::Header(_) => true,
        Kind::Custom => role(c).is_none() && (c.refdes.starts_with('J') || c.refdes.starts_with('P') || c.refdes.starts_with("CN")),
        _ => false,
    };
    let guard = |c: &Component| role(c) == Some('D') || c.custom.as_ref().is_some_and(|s| s.sim.as_ref().is_some_and(|m| m.element.eq_ignore_ascii_case("M")));
    for c in x.s.components.iter().filter(|c| !c.sim_only && connector(c)) {
        for (pin, at) in c.pins() {
            let Some(n) = x.net(&c.refdes, &pin) else { continue };
            if x.nets[n].pins.len() < 2 || !x.supply(n) || x.sourced(n, &c.refdes) {
                continue;
            }
            let guarded = x.nets[n].pins.iter().any(|p| p.refdes != c.refdes && x.comp.get(p.refdes.as_str()).is_some_and(|o| guard(o)));
            if !guarded {
                out.push(Problem::new(
                    Severity::Info,
                    format!("{} は電源の入口 ({}) ですが、逆接続・静電気の保護がありません (情報)", c.refdes, x.nets[n].name),
                    Some(at),
                    "電池や AC アダプタを逆に挿すと、IC に逆の電圧がかかって壊れます。電源の入口に、直列のダイオード (0.3〜0.7V 下がる)、P チャネル MOSFET (ほとんど下がらない)、または GND との間に TVS ダイオード (静電気・サージ) を入れると守れます。逆に挿せないコネクタや、付け外ししない電源なら、そのままで構いません",
                ));
                break; // once per connector
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    fn pin(n: &str, name: &str, side: PinSide, kind: Option<PinKind>) -> SymPin {
        SymPin { number: n.into(), name: name.into(), side, sim_only: false, gap: 0, kind }
    }

    fn part(name: &str, prefix: &str, pins: Vec<SymPin>, companions: Vec<Companion>) -> CustomSymbol {
        CustomSymbol { name: name.into(), prefix: prefix.into(), pins, sim: None, ratings: None, glyph: String::new(), shops: Default::default(), programming: vec![], companions }
    }

    fn cap(name: &str, value: &str, pin: &str, to: &str) -> Companion {
        Companion { name: name.into(), kind: "C".into(), value: value.into(), pin: pin.into(), to: to.into(), note: String::new() }
    }

    fn regulator() -> CustomSymbol {
        part(
            "REG",
            "U",
            vec![pin("1", "IN", PinSide::Left, None), pin("2", "GND", PinSide::Bottom, None), pin("3", "OUT", PinSide::Right, None)],
            vec![cap("CIN", "330n", "1", "GND"), cap("COUT", "100n", "3", "GND")],
        )
    }

    fn wire_pins(s: &mut Schematic, a: (usize, usize), b: (usize, usize)) {
        let pa = s.components[a.0].pins()[a.1].1;
        let pb = s.components[b.0].pins()[b.1].1;
        s.wires.push(Wire { a: pa, b: (pb.0, pa.1) });
        s.wires.push(Wire { a: (pb.0, pa.1), b: pb });
    }

    fn msgs(s: &Schematic) -> Vec<String> {
        erc(s).into_iter().map(|p| p.message).collect()
    }

    #[test]
    fn a_regulator_without_its_capacitors_is_pointed_out_and_placing_them_clears_it() {
        let mut s = Schematic::default();
        let u = s.add_custom(regulator(), "REG", "SOT-223", (20, 20), 0);
        let v = s.add(Kind::VSource, (0, 20), 0);
        let r = s.add(Kind::Resistor, (40, 20), 1);
        let g = s.add(Kind::Ground, (20, 40), 0);
        wire_pins(&mut s, (v, 0), (u, 0));
        wire_pins(&mut s, (u, 2), (r, 0));
        for (c, p) in [(v, 1), (u, 1), (r, 1)] {
            wire_pins(&mut s, (c, p), (g, 0));
        }
        let before = msgs(&s);
        assert!(before.iter().any(|m| m.contains("U1 の推奨のコンデンサがありません: CIN")), "{before:?}");
        assert!(before.iter().any(|m| m.contains("COUT")), "{before:?}");
        s.place_companions(u).unwrap();
        let after = msgs(&s);
        assert!(!after.iter().any(|m| m.contains("推奨の")), "{after:?}");
        // a capacitor far below the recommended value
        let c1 = s.components.iter().position(|c| c.refdes == "C2").unwrap();
        s.components[c1].value = "10n".into();
        assert!(msgs(&s).iter().any(|m| m.contains("U1 のピン3 (OUT) のコンデンサが小さすぎます: 10nF")));
    }

    #[test]
    fn a_reset_pin_needs_its_pull_up_unless_tied_to_the_supply() {
        let pull = Companion { name: "RRST".into(), kind: "R".into(), value: "10k".into(), pin: "1".into(), to: "2".into(), note: String::new() };
        let mcu = part("MCU", "U", vec![pin("1", "RESET", PinSide::Left, Some(PinKind::Input)), pin("2", "VCC", PinSide::Top, Some(PinKind::PowerIn)), pin("3", "GND", PinSide::Bottom, None)], vec![pull]);
        let mut s = Schematic::default();
        let u = s.add_custom(mcu, "MCU", "DIP", (20, 20), 0);
        let sw = s.add(Kind::Capacitor, (0, 20), 1);
        wire_pins(&mut s, (u, 0), (sw, 0));
        assert!(msgs(&s).iter().any(|m| m.contains("推奨の抵抗がありません: RRST")));
        let placed = s.place_companions(u).unwrap();
        assert_eq!(placed.len(), 1);
        assert!(!msgs(&s).iter().any(|m| m.contains("RRST")), "{:?}", msgs(&s));
    }

    #[test]
    fn crystal_capacitors_are_checked_against_the_load_capacitance() {
        let mut xtal = part("XTAL_16MHz", "Y", vec![pin("1", "X1", PinSide::Left, None), pin("2", "X2", PinSide::Right, None)], vec![]);
        xtal.glyph = "crystal".into();
        xtal.ratings = Some(PartRatings { load_pf: Some(18.0), ..Default::default() });
        let osc = part("OSC", "U", vec![pin("1", "XI", PinSide::Left, Some(PinKind::Input)), pin("2", "XO", PinSide::Right, Some(PinKind::Output))], vec![]);
        let mut s = Schematic::default();
        let y = s.add_custom(xtal, "XTAL_16MHz", "3225", (20, 40), 0);
        let u = s.add_custom(osc, "OSC", "DIP", (20, 10), 0);
        wire_pins(&mut s, (u, 0), (y, 0));
        wire_pins(&mut s, (u, 1), (y, 1));
        assert!(msgs(&s).iter().any(|m| m.contains("両端に負荷コンデンサがありません (目安 27pF × 2)")), "{:?}", msgs(&s));
        let g = s.add(Kind::Ground, (20, 70), 0);
        let c1 = s.add(Kind::Capacitor, (10, 55), 1);
        let c2 = s.add(Kind::Capacitor, (30, 55), 1);
        for c in [c1, c2] {
            wire_pins(&mut s, (c, 1), (g, 0));
        }
        wire_pins(&mut s, (y, 0), (c1, 0));
        wire_pins(&mut s, (y, 1), (c2, 0));
        s.components[c1].value = "100p".into();
        s.components[c2].value = "100p".into();
        assert!(msgs(&s).iter().any(|m| m.contains("負荷容量が合いません")), "{:?}", msgs(&s));
        s.components[c1].value = "22p".into();
        s.components[c2].value = "27p".into();
        assert!(!msgs(&s).iter().any(|m| m.contains("水晶")), "{:?}", msgs(&s));
    }

    #[test]
    fn an_led_straight_off_an_output_pin_and_an_unguarded_power_inlet() {
        let mcu = part("MCU", "U", vec![pin("1", "PB0", PinSide::Right, Some(PinKind::Output)), pin("2", "VCC", PinSide::Top, Some(PinKind::PowerIn)), pin("3", "GND", PinSide::Bottom, None)], vec![]);
        let mut s = Schematic::default();
        let u = s.add_custom(mcu, "MCU", "DIP", (20, 20), 0);
        let d = s.add(Kind::Led, (40, 20), 1);
        let g = s.add(Kind::Ground, (40, 40), 0);
        let j = s.add(Kind::Header(2), (0, 0), 0);
        let vin = s.add(Kind::Power, (10, -10), 0);
        s.components[vin].value = "VIN".into();
        wire_pins(&mut s, (u, 0), (d, 0));
        wire_pins(&mut s, (d, 1), (g, 0));
        wire_pins(&mut s, (u, 2), (g, 0));
        wire_pins(&mut s, (j, 1), (g, 0));
        wire_pins(&mut s, (j, 0), (vin, 0));
        wire_pins(&mut s, (u, 1), (vin, 0));
        let all = erc(&s);
        assert!(all.iter().any(|p| p.message.contains("D1 が IC の出力ピン U1.1 (PB0) に直接")), "{all:?}");
        let inlet = all.iter().find(|p| p.message.contains("J1 は電源の入口 (VIN)")).expect("the inlet");
        assert_eq!(inlet.severity, Severity::Info);
        // a diode across the inlet (a TVS) answers it
        let tvs = s.add(Kind::Diode, (0, 30), 1);
        wire_pins(&mut s, (tvs, 0), (g, 0));
        wire_pins(&mut s, (tvs, 1), (vin, 0));
        assert!(!msgs(&s).iter().any(|m| m.contains("電源の入口")), "{:?}", msgs(&s));
    }

    #[test]
    fn small_helpers() {
        let near = |t: &str, v: f64| (si_value(t).unwrap() / v - 1.0).abs() < 1e-12;
        assert!(near("22µF", 22e-6) && near("4.7u", 4.7e-6) && near("100n", 100e-9) && near("33p", 33e-12));
        assert_eq!(si_value("1Meg"), Some(1e6));
        assert_eq!(e12(26.0), 27.0);
        assert_eq!(e12(9.5), 10.0);
        assert_eq!(farads(100e-9), "100nF");
    }
}
