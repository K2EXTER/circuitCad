//! Sheet hierarchy: a sheet symbol (a part with `sub`) stands for another sheet, which can be used many times
//! (the same channel repeated). Design: `docs/hierarchy-design.md`.
//!
//! Everything downstream (netlist, ERC, simulation, board import) works on `flat()`, so the expansion is done there:
//! a sheet symbol is replaced by a copy of its child sheet, one copy per symbol.
//! - Reference designators of the copy: `R1_CH1` (the first letter stays, SPICE decides the kind of the element by it).
//! - Net labels of the copy: `CH1.NAME`, a net of its own per copy. Power symbols (VCC, GND) stay global.
//! - Ports of the child sheet join the pins of the symbol, by a pair of labels with the same made-up name.
//! - A sheet that no symbol refers to stays an ordinary sheet, as before.

use crate::*;

/// How many symbols deep a child sheet may be used (a sheet using itself is cut earlier).
pub const MAX_DEPTH: usize = 8;

/// The first character of a net name made by the expansion. Such a name never wins over a name the user wrote when a net
/// has both (the net of a port is called by the name of the parent's wire); `extract` takes the character off again.
pub const GENERATED: char = '\u{1}';

/// Which way a port passes a signal. It only decides the side of the sheet symbol the pin is drawn on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortDir {
    In,
    Out,
    Io,
}

/// A port of a child sheet: a name at a place on the sheet. It is the pin of the same name on the sheet symbols that use the sheet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetPort {
    pub pos: Pt,
    pub name: String,
    pub dir: PortDir,
}

/// What a sheet symbol stands for: the number of the child sheet, and the values this copy gives to some of its parts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubRef {
    pub sheet: usize,
    /// Part of the child sheet (its reference designator there) -> the value this copy uses instead of the sheet's own.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub values: std::collections::BTreeMap<String, String>,
}

/// One copy of a sheet in the flat sheet: its place (slot, `slot * SHEET_STRIDE` to the right), the sheet it is a copy of, and
/// the instance names from the top (empty for an ordinary sheet).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instance {
    pub slot: usize,
    pub sheet: usize,
    pub path: Vec<String>,
    /// The values this copy gives to parts of the sheet (from its symbol).
    pub values: std::collections::BTreeMap<String, String>,
}

struct Parts<'a> {
    comps: &'a [Component],
    wires: &'a [Wire],
    labels: &'a [Label],
    buses: &'a [Wire],
    skip: &'a [(Pt, Pt)],
    ports: &'a [SheetPort],
}

/// The name a net of the copy at `path` gets: the same text on both sides of a port makes them one net.
fn generated(path: &[String], name: &str) -> String {
    format!("{GENERATED}{}.{name}", path.join("."))
}

impl Schematic {
    fn parts(&self, k: usize) -> Parts<'_> {
        if k == self.active_sheet || self.sheets.is_empty() {
            Parts { comps: &self.components, wires: &self.wires, labels: &self.labels, buses: &self.buses, skip: &self.erc_skip, ports: &self.ports }
        } else {
            let s = &self.sheets[k];
            Parts { comps: &s.components, wires: &s.wires, labels: &s.labels, buses: &s.buses, skip: &s.erc_skip, ports: &s.ports }
        }
    }

    /// The sheets some other sheet's symbol refers to (the child sheets).
    pub fn child_sheets(&self) -> Vec<usize> {
        let n = self.sheet_count();
        let mut out: Vec<usize> = vec![];
        for k in 0..n {
            for c in self.parts(k).comps {
                if let Some(sub) = &c.sub
                    && sub.sheet < n
                    && sub.sheet != k
                    && !out.contains(&sub.sheet)
                {
                    out.push(sub.sheet);
                }
            }
        }
        out.sort_unstable();
        out
    }

    /// The sheets that hold a symbol of another sheet (the parent sheets), with how many symbols each holds.
    pub fn parent_sheets(&self) -> Vec<(usize, usize)> {
        let n = self.sheet_count();
        let mut out = vec![];
        for k in 0..n {
            let c = self.parts(k).comps.iter().filter(|c| c.sub.as_ref().is_some_and(|s| s.sheet < n && s.sheet != k)).count();
            if c > 0 {
                out.push((k, c));
            }
        }
        out
    }

    /// Every copy of a sheet in the flat sheet, in the order they are laid out. The sheets nothing refers to come first
    /// in their own places (as before); the copies made for the symbols follow, from slot `sheet_count()` on.
    pub fn instances(&self) -> Vec<Instance> {
        let n = self.sheet_count();
        let children = self.child_sheets();
        let mut tops: Vec<usize> = (0..n).filter(|k| !children.contains(k)).collect();
        if tops.is_empty() {
            // sheets that use each other in a ring: the first one stands for the whole
            tops.push(0);
        }
        let mut out = vec![];
        let mut next = n;
        for k in tops {
            out.push(Instance { slot: k, sheet: k, path: vec![], values: Default::default() });
            self.collect_copies(k, &[], &[k], &mut next, &mut out);
        }
        out
    }

    fn collect_copies(&self, k: usize, path: &[String], chain: &[usize], next: &mut usize, out: &mut Vec<Instance>) {
        if path.len() >= MAX_DEPTH {
            return;
        }
        for c in self.parts(k).comps {
            let Some(sub) = &c.sub else { continue };
            // a sheet inside itself (directly or through others) cannot be expanded
            if sub.sheet >= self.sheet_count() || chain.contains(&sub.sheet) {
                continue;
            }
            let mut p = path.to_vec();
            p.push(c.refdes.clone());
            out.push(Instance { slot: *next, sheet: sub.sheet, path: p.clone(), values: sub.values.clone() });
            *next += 1;
            let mut ch = chain.to_vec();
            ch.push(sub.sheet);
            self.collect_copies(sub.sheet, &p, &ch, next, out);
        }
    }

    /// The sheet (number) of a place in the flat sheet: for a copy of a child sheet, the child sheet itself.
    pub fn slot_sheet(&self, slot: usize) -> usize {
        self.instances().iter().find(|i| i.slot == slot).map_or(slot, |i| i.sheet)
    }

    /// The place in the flat sheet where sheet `k` is drawn (its first copy, for a child sheet; nothing is drawn for a child
    /// sheet that no symbol uses yet, then its own number).
    pub fn first_slot_of(&self, k: usize) -> usize {
        self.instances().iter().find(|i| i.sheet == k).map_or(k, |i| i.slot)
    }

    /// The flat sheet: the sheets and the copies of the child sheets side by side. Only called when there are several sheets.
    pub(crate) fn expand(&self) -> Schematic {
        let mut out = Schematic { components: vec![], wires: vec![], labels: vec![], buses: vec![], ports: vec![], erc_skip: vec![], sheets: vec![], active_sheet: 0, ..self.clone() };
        let insts = self.instances();
        for inst in &insts {
            let dx = inst.slot as i32 * SHEET_STRIDE;
            let mv = |p: Pt| (p.0 + dx, p.1);
            let p = self.parts(inst.sheet);
            let suffix = inst.path.join("_");
            for c in p.comps {
                if let Some(sub) = &c.sub {
                    // a symbol: its copy is laid out elsewhere; the pins are tied to the ports of the child by name
                    let mut child_path = inst.path.clone();
                    child_path.push(c.refdes.clone());
                    if insts.iter().any(|i| i.path == child_path && i.sheet == sub.sheet) {
                        let pins = c.pins();
                        for port in self.parts(sub.sheet).ports {
                            if let Some((_, at)) = pins.iter().find(|(n, _)| *n == port.name) {
                                out.labels.push(Label { pos: mv(*at), text: generated(&child_path, &port.name) });
                            }
                        }
                    }
                    continue;
                }
                let mut c2 = Component { pos: mv(c.pos), ..c.clone() };
                if let Some(v) = inst.values.get(&c.refdes) {
                    c2.value = v.clone();
                }
                if !inst.path.is_empty() && !c.kind.is_power_symbol() {
                    c2.refdes = format!("{}_{suffix}", c.refdes);
                }
                out.components.push(c2);
            }
            out.wires.extend(p.wires.iter().map(|w| Wire { a: mv(w.a), b: mv(w.b) }));
            out.buses.extend(p.buses.iter().map(|w| Wire { a: mv(w.a), b: mv(w.b) }));
            out.erc_skip.extend(p.skip.iter().map(|&(a, b)| (mv(a), mv(b))));
            for l in p.labels {
                // a copy's labels are its own nets (a bus label keeps its text: the bus check reads it)
                let text = if inst.path.is_empty() || crate::bus::is_bus_label(&l.text) { l.text.clone() } else { generated(&inst.path, &l.text) };
                out.labels.push(Label { pos: mv(l.pos), text });
            }
            if !inst.path.is_empty() {
                for port in p.ports {
                    out.labels.push(Label { pos: mv(port.pos), text: generated(&inst.path, &port.name) });
                }
            }
        }
        out
    }

    /// The symbol that stands for sheet `k`: a box with one pin per port, the inputs on the left and the rest on the right.
    pub fn sheet_symbol(&self, k: usize) -> CustomSymbol {
        // in the order the ports were put: a port added later is a pin added at the end, the pins that are wired stay in their order
        let ports: Vec<&SheetPort> = self.parts(k).ports.iter().collect();
        let mut pins: Vec<SymPin> = vec![];
        for p in ports {
            if p.name.is_empty() || pins.iter().any(|q| q.number == p.name) {
                continue;
            }
            let side = if p.dir == PortDir::Out { PinSide::Right } else { PinSide::Left };
            pins.push(SymPin { number: p.name.clone(), name: p.name.clone(), side, sim_only: false });
        }
        CustomSymbol { name: self.sheet_name(k), prefix: "CH".into(), pins, sim: None, ratings: None, glyph: String::new(), shops: Default::default(), programming: vec![] }
    }

    /// Puts a sheet symbol for sheet `child` on the sheet being edited. `None` when there is no such sheet, or it is the sheet
    /// being edited (a sheet cannot hold itself). The symbol is called CH1, CH2, ... (the instance name; rename it in the part's number).
    pub fn add_sheet_symbol(&mut self, child: usize, pos: Pt) -> Option<usize> {
        if child >= self.sheet_count() || child == self.active_sheet || self.sheets.is_empty() {
            return None;
        }
        let sym = self.sheet_symbol(child);
        let i = self.add_custom(sym, &self.sheet_name(child), "", pos, 0);
        self.components[i].sub = Some(SubRef { sheet: child, ..Default::default() });
        self.components[i].sim_only = true;
        Some(i)
    }

    /// Makes the pins of every sheet symbol follow the ports of its child sheet (after a port was added, renamed or removed).
    /// The pins are centred on the box, so a port added or removed moves the others: what is wired to a moved pin stays joined to it
    /// (a short wire from the new place of the pin to the old end of the wire; a net label on the pin moves with it).
    pub fn refresh_sheet_symbols(&mut self) {
        let syms: Vec<CustomSymbol> = (0..self.sheet_count()).map(|k| self.sheet_symbol(k)).collect();
        follow_pins(&mut self.components, &mut self.wires, &mut self.labels, &syms);
        for s in &mut self.sheets {
            follow_pins(&mut s.components, &mut s.wires, &mut s.labels, &syms);
        }
    }

    /// A sheet is about to be deleted: its symbols go too, and the numbers of the later sheets move up by one.
    pub(crate) fn forget_sheet_refs(&mut self, gone: usize) {
        let fix = |comps: &mut Vec<Component>| {
            comps.retain(|c| c.sub.as_ref().is_none_or(|s| s.sheet != gone));
            for c in comps.iter_mut() {
                if let Some(s) = &mut c.sub
                    && s.sheet > gone
                {
                    s.sheet -= 1;
                }
            }
        };
        fix(&mut self.components);
        for s in &mut self.sheets {
            fix(&mut s.components);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sheet 0 is the top; sheet 1 is a child: R1 and C1 in series, ports IN and OUT, a label MID between them.
    fn channel() -> Schematic {
        let mut s = Schematic::default();
        s.add_sheet(); // sheet 1 is being edited
        let r = s.add(Kind::Resistor, (10, 10), 0);
        let c = s.add(Kind::Capacitor, (30, 10), 0);
        let rp = s.components[r].pins();
        let cp = s.components[c].pins();
        s.wires.push(Wire { a: rp[1].1, b: cp[0].1 });
        s.labels.push(Label { pos: rp[1].1, text: "MID".into() });
        s.ports.push(SheetPort { pos: rp[0].1, name: "IN".into(), dir: PortDir::In });
        s.ports.push(SheetPort { pos: cp[1].1, name: "OUT".into(), dir: PortDir::Out });
        s.switch_sheet(0);
        s
    }

    fn pin_of(s: &Schematic, i: usize, name: &str) -> Pt {
        s.components[i].pins().into_iter().find(|(n, _)| n == name).unwrap().1
    }

    /// The top sheet uses the channel twice: CH1 gets SIG on its input, CH2 gets the output of CH1.
    fn two_channels() -> Schematic {
        let mut s = channel();
        let a = s.add_sheet_symbol(1, (10, 10)).unwrap();
        let b = s.add_sheet_symbol(1, (40, 10)).unwrap();
        assert_eq!((s.components[a].refdes.as_str(), s.components[b].refdes.as_str()), ("CH1", "CH2"));
        let (a_in, a_out, b_in) = (pin_of(&s, a, "IN"), pin_of(&s, a, "OUT"), pin_of(&s, b, "IN"));
        s.labels.push(Label { pos: a_in, text: "SIG".into() });
        s.wires.push(Wire { a: a_out, b: b_in });
        s.labels.push(Label { pos: a_out, text: "BETWEEN".into() });
        s
    }

    #[test]
    fn a_child_sheet_is_expanded_once_per_symbol() {
        let s = two_channels();
        let flat = s.flat();
        let mut refs: Vec<&str> = flat.components.iter().map(|c| c.refdes.as_str()).collect();
        refs.sort();
        assert_eq!(refs, ["C1_CH1", "C1_CH2", "R1_CH1", "R1_CH2"], "the symbols themselves are gone, the child's parts are copied");
        assert_eq!(s.instances().len(), 3, "the top sheet and two copies");
    }

    #[test]
    fn ports_join_the_symbol_pins_and_nets_keep_the_name_the_user_gave() {
        let s = two_channels();
        let nets = extract(&s);
        let has = |name: &str, pin: (&str, &str)| nets.iter().any(|n| n.name == name && n.pins.iter().any(|p| p.refdes == pin.0 && p.pin == pin.1));
        assert!(has("SIG", ("R1_CH1", "1")), "the parent's label names the net: {nets:?}");
        assert!(has("BETWEEN", ("C1_CH1", "2")) && has("BETWEEN", ("R1_CH2", "1")), "the output of CH1 is the input of CH2: {nets:?}");
        assert!(nets.iter().all(|n| !n.name.contains(GENERATED)), "the mark is taken off: {nets:?}");
    }

    #[test]
    fn labels_of_a_copy_are_nets_of_its_own() {
        let s = two_channels();
        let nets = extract(&s);
        let mid1 = nets.iter().find(|n| n.name == "CH1.MID").expect("CH1.MID");
        let mid2 = nets.iter().find(|n| n.name == "CH2.MID").expect("CH2.MID");
        assert!(mid1.pins.iter().all(|p| p.refdes.ends_with("_CH1")) && mid1.pins.len() == 2);
        assert!(mid2.pins.iter().all(|p| p.refdes.ends_with("_CH2")) && mid2.pins.len() == 2);
    }

    #[test]
    fn power_symbols_stay_global_in_a_copy() {
        let mut s = channel();
        s.switch_sheet(1);
        let g = s.add(Kind::Ground, (50, 30), 0);
        let c = s.components.iter().position(|c| c.refdes == "C1").unwrap();
        let (cp, gp) = (s.components[c].pins()[1].1, s.components[g].pins()[0].1);
        s.wires.push(Wire { a: cp, b: gp });
        s.switch_sheet(0);
        s.add_sheet_symbol(1, (10, 10)).unwrap();
        let g0 = s.add(Kind::Ground, (60, 60), 0);
        let top_gnd = s.components[g0].pins()[0].1;
        s.wires.push(Wire { a: top_gnd, b: (top_gnd.0, top_gnd.1 + 2) });
        let nets = extract(&s);
        let gnd = nets.iter().find(|n| n.name == "GND").expect("GND");
        assert!(gnd.pins.iter().any(|p| p.refdes == "C1_CH1" && p.pin == "2"), "{nets:?}");
    }

    #[test]
    fn a_problem_in_a_copy_is_shown_on_the_child_sheet() {
        let mut s = channel();
        // a resistor left alone in the child sheet
        s.switch_sheet(1);
        s.add(Kind::Resistor, (60, 30), 0);
        s.switch_sheet(0);
        s.add_sheet_symbol(1, (10, 10)).unwrap();
        let ps = erc(&s);
        let p = ps.iter().find(|p| p.message.contains("R2_CH1")).expect("R2_CH1 is not connected");
        assert_eq!(p.sheet, 1, "the child sheet, where the part is drawn");
        assert_eq!(s.sheet_name(2), "シート2 (CH1)");
    }

    #[test]
    fn a_sheet_inside_itself_is_not_expanded_forever() {
        let mut s = channel();
        // sheet 1 gets a symbol of sheet 0, and sheet 0 a symbol of sheet 1: a ring
        s.add_sheet_symbol(1, (10, 10)).unwrap();
        s.switch_sheet(1);
        s.add_sheet_symbol(0, (10, 40)).unwrap();
        let flat = s.flat();
        assert!(flat.components.len() < 20, "cut at the ring, not {}", flat.components.len());
        assert!(s.add_sheet_symbol(1, (0, 0)).is_none(), "a sheet cannot hold itself");
    }

    #[test]
    fn a_child_inside_a_child_gets_both_names() {
        let mut s = Schematic::default();
        s.add_sheet();
        s.add_sheet(); // 0 = top, 1 = middle, 2 = leaf being edited
        let r = s.add(Kind::Resistor, (10, 10), 0);
        let rp = s.components[r].pins();
        s.ports.push(SheetPort { pos: rp[0].1, name: "A".into(), dir: PortDir::Io });
        s.switch_sheet(1);
        let leaf = s.add_sheet_symbol(2, (10, 10)).unwrap();
        s.components[leaf].refdes = "L".into();
        s.switch_sheet(0);
        let mid = s.add_sheet_symbol(1, (10, 10)).unwrap();
        s.components[mid].refdes = "M".into();
        let flat = s.flat();
        assert!(flat.components.iter().any(|c| c.refdes == "R1_M_L"), "{:?}", flat.components.iter().map(|c| &c.refdes).collect::<Vec<_>>());
    }

    #[test]
    fn deleting_a_child_sheet_takes_its_symbols_and_renumbers() {
        let mut s = two_channels();
        s.add_sheet(); // a third sheet, sheet 2, being edited
        s.switch_sheet(0);
        assert!(s.remove_sheet(1));
        assert!(s.components.iter().all(|c| c.sub.is_none()), "the symbols of the deleted sheet are gone");
        assert_eq!(s.sheet_count(), 2);
    }

    #[test]
    fn symbols_follow_the_ports_of_their_sheet() {
        let mut s = two_channels();
        s.switch_sheet(1);
        s.ports.push(SheetPort { pos: (0, 0), name: "EN".into(), dir: PortDir::In });
        s.switch_sheet(0);
        s.refresh_sheet_symbols();
        let ch = s.components.iter().find(|c| c.sub.is_some()).unwrap();
        let pins: Vec<String> = ch.custom.as_ref().unwrap().pins.iter().map(|p| p.name.clone()).collect();
        assert_eq!(pins.len(), 3);
        assert!(pins.contains(&"EN".to_string()));
    }

    #[test]
    fn a_hierarchy_round_trips_through_json_and_plain_files_stay_plain() {
        let s = two_channels();
        let back: Schematic = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        let plain = serde_json::to_string(&Schematic::default()).unwrap();
        assert!(!plain.contains("ports") && !plain.contains("sub"), "{plain}");
    }
}

/// A port seen from both ends, in the flat sheet: the pin of a sheet symbol, and the port of the child sheet it is tied to.
struct Tie {
    symbol: String,
    port: String,
    dir: PortDir,
    parent_pt: Pt,
    child_pt: Pt,
    /// The first copy of this port of this child sheet (the others are the same by construction, they are checked once).
    first: bool,
    /// The parts of this copy of the child sheet (and of the copies inside it), by their names in the flat sheet.
    inside: Vec<String>,
}

/// Whether something can drive the wiring of input tie `i`: an output (or two-way) port tied to it, or a source, a supply or
/// ground symbol or a library part outside the child copy itself; the search goes through resistors and coils.
fn net_is_fed(flat: &Schematic, ties: &[Tie], group: &[Option<usize>], nets: &[(usize, crate::netlist::Net)], i: usize) -> bool {
    let Some(start) = group[i] else { return true };
    let me = &ties[i];
    let kind_of = |refdes: &str| flat.components.iter().find(|c| c.refdes == refdes).map(|c| c.kind);
    // the group of every pin that has a net
    let group_of_pin = |refdes: &str, pin: &str| nets.iter().find(|(_, n)| n.pins.iter().any(|p| p.refdes == refdes && p.pin == pin)).map(|(g, _)| *g);
    let mut seen = vec![start];
    let mut todo = vec![start];
    while let Some(g) = todo.pop() {
        if ties.iter().enumerate().any(|(k, t)| k != i && t.dir != PortDir::In && group[k] == Some(g)) {
            return true;
        }
        let Some((_, net)) = nets.iter().find(|(r, _)| *r == g) else { continue };
        for p in &net.pins {
            if me.inside.contains(&p.refdes) {
                continue;
            }
            match kind_of(&p.refdes) {
                Some(Kind::VSource | Kind::Header(_) | Kind::Power | Kind::Ground | Kind::Custom) => return true,
                Some(Kind::Resistor | Kind::Inductor) => {
                    let other = if p.pin == "1" { "2" } else { "1" };
                    if let Some(h) = group_of_pin(&p.refdes, other)
                        && !seen.contains(&h)
                    {
                        seen.push(h);
                        todo.push(h);
                    }
                }
                _ => {}
            }
        }
    }
    false
}

impl Schematic {
    fn port_ties(&self) -> Vec<Tie> {
        let insts = self.instances();
        let mut out: Vec<Tie> = vec![];
        let mut seen: Vec<(usize, String)> = vec![];
        for inst in &insts {
            let dx = inst.slot as i32 * SHEET_STRIDE;
            for c in self.parts(inst.sheet).comps {
                let Some(sub) = &c.sub else { continue };
                let mut child_path = inst.path.clone();
                child_path.push(c.refdes.clone());
                let Some(child) = insts.iter().find(|i| i.path == child_path && i.sheet == sub.sheet) else { continue };
                let cdx = child.slot as i32 * SHEET_STRIDE;
                let pins = c.pins();
                for port in self.parts(sub.sheet).ports {
                    let Some((_, at)) = pins.iter().find(|(n, _)| *n == port.name) else { continue };
                    let key = (sub.sheet, port.name.clone());
                    let first = !seen.contains(&key);
                    seen.push(key);
                    out.push(Tie { symbol: format!("{}{}", c.refdes, if inst.path.is_empty() { String::new() } else { format!(" ({})", inst.path.join(".")) }), port: port.name.clone(), dir: port.dir, parent_pt: (at.0 + dx, at.1), child_pt: (port.pos.0 + cdx, port.pos.1), first, inside: self.copy_parts(&insts, &child_path) });
                }
            }
        }
        out
    }

    /// The names, in the flat sheet, of the parts of the copy at `path` and of the copies inside it.
    fn copy_parts(&self, insts: &[Instance], path: &[String]) -> Vec<String> {
        let mut out = vec![];
        for i in insts.iter().filter(|i| i.path.starts_with(path)) {
            let suffix = i.path.join("_");
            out.extend(self.parts(i.sheet).comps.iter().filter(|c| c.sub.is_none() && !c.kind.is_power_symbol()).map(|c| format!("{}_{suffix}", c.refdes)));
        }
        out
    }

    /// Whether sheet `from` can reach sheet `to` through sheet symbols (a ring when a symbol of `to` sits on `from` and `from` can reach `to`).
    fn reaches(&self, from: usize, to: usize, seen: &mut Vec<usize>) -> bool {
        if from == to {
            return true;
        }
        if seen.contains(&from) || from >= self.sheet_count() {
            return false;
        }
        seen.push(from);
        self.parts(from).comps.iter().filter_map(|c| c.sub.as_ref()).any(|s| self.reaches(s.sheet, to, seen))
    }

    /// The findings about the hierarchy, in the places of the flat sheet `flat` (`erc` puts them back on their sheets).
    pub(crate) fn hierarchy_problems(&self, flat: &Schematic) -> Vec<Problem> {
        use crate::netlist::Severity::{Error, Warning};
        let mut out = vec![];
        let n = self.sheet_count();
        let place = |k: usize, p: Pt| (p.0 + self.first_slot_of(k) as i32 * SHEET_STRIDE, p.1);
        for k in 0..n {
            let parts = self.parts(k);
            // a symbol that cannot be expanded: the sheet inside itself, or a ring
            for c in parts.comps {
                let Some(sub) = &c.sub else { continue };
                if sub.sheet >= n || self.reaches(sub.sheet, k, &mut vec![]) {
                    out.push(Problem::new(
                        Error,
                        format!("シートの記号 {} が、自分自身を含むシートを指しています", c.refdes),
                        Some(place(k, c.pos)),
                        "シートの記号は、そのシートの中 (孫のシートも含めて) にある自分を指せません。記号が堂々巡りになるので、このシートは展開されず、回路に入りません。この記号を消すか、別のシートを指す記号に置き換えましょう",
                    ));
                } else if self.parts(sub.sheet).ports.is_empty() {
                    out.push(Problem::new(
                        Warning,
                        format!("シートの記号 {} が指すシート「{}」に、ポートがありません", c.refdes, self.sheet_name(sub.sheet)),
                        Some(place(k, c.pos)),
                        &format!("ポートは、シートの出入口です。ポートがないと、記号にピンがなく、親のシートとつなげません。「{}」を開き、ラベルツールの「ポートにする」で、信号の出入りする所にポートを置きましょう", self.sheet_name(sub.sheet)),
                    ));
                }
            }
            // two ports of one name on a sheet are one pin of the symbol
            let ports = parts.ports;
            for (i, p) in ports.iter().enumerate() {
                if ports[..i].iter().any(|q| q.name == p.name) {
                    out.push(Problem::new(
                        Error,
                        format!("シート「{}」に、ポート {} が2つあります", self.sheet_name(k), p.name),
                        Some(place(k, p.pos)),
                        "同じ名前のポートは、記号の1つのピンにまとまってしまいます。片方を選んで、名前を変えるか消しましょう",
                    ));
                }
            }
        }
        // a port nothing is wired to, on the parent side (the pin of the symbol) or inside the child sheet
        let ties = self.port_ties();
        let points: Vec<Pt> = ties.iter().flat_map(|t| [t.parent_pt, t.child_pt]).collect();
        let lonely = crate::netlist::lonely_points(flat, &points);
        let parent_pts: Vec<Pt> = ties.iter().map(|t| t.parent_pt).collect();
        let (group, nets) = crate::netlist::groups_at(flat, &parent_pts);
        for (i, t) in ties.iter().enumerate() {
            // two outputs tied together: each tries to hold the net at its own level
            if t.dir == PortDir::Out
                && let Some(g) = group[i]
                && let Some(j) = (0..i).find(|&j| ties[j].dir == PortDir::Out && group[j] == Some(g))
            {
                let other = &ties[j];
                out.push(Problem::new(
                    Error,
                    format!("出力のポート {} {} と {} {} が、同じネットにつながっています", t.symbol, t.port, other.symbol, other.port),
                    Some(t.parent_pt),
                    "出力どうしをつなぐと、2つが別々の電圧を出そうとして、ぶつかります (実際の回路なら壊れることもあります)。どちらかを入力のポートにするか、つなぎ方を見直しましょう。スイッチで切り替えたいときは、間にスイッチやゲートを入れます",
                ));
            }
            // an input nothing feeds: the wire leads to no source, no output and no part that could drive it
            if t.dir == PortDir::In && !lonely.contains(&t.parent_pt) && !net_is_fed(flat, &ties, &group, &nets, i) {
                out.push(Problem::new(
                    Warning,
                    format!("{} の入力のポート {} に、信号を出すものがつながっていません", t.symbol, t.port),
                    Some(t.parent_pt),
                    "入力のポートは、外から信号や電圧をもらう口です。このピンにつながる線の先に、電源・信号源・ICの出力・別の記号の出力のポートが見つかりません (抵抗やコイルを通った先も調べました)。入力が決まらないと、子シートの中が動きません。信号を出す所までつなぎましょう。意図して浮かせるなら、このままで構いません",
                ));
            }
            // an output left alone is harmless; an input or a two-way port left alone is a floating pin
            if t.dir != PortDir::Out && lonely.contains(&t.parent_pt) {
                out.push(Problem::new(
                    Warning,
                    format!("{} のポート {} が、つながっていません", t.symbol, t.port),
                    Some(t.parent_pt),
                    "シートの記号のピンに、線もラベルもついていません。入力のピンが浮いていると電圧が決まらず、動作が不安定になります。線かラベルでつなぎましょう。使わないポートなら、そのままで構いません",
                ));
            }
            if t.first && lonely.contains(&t.child_pt) {
                out.push(Problem::new(
                    Warning,
                    format!("ポート {} が、シートの中でどこにもつながっていません", t.port),
                    Some(t.child_pt),
                    "ポートの場所に、部品のピンも線もラベルもありません。ポートは、シートの中の線や部品のピンの上に置きます。ずれていたら、消して置き直しましょう",
                ));
            }
        }
        out
    }
}

#[cfg(test)]
mod problem_tests {
    use super::*;

    /// Child sheet 1: R1 with ports IN (input, on pin 1) and OUT (output, on pin 2). Sheet 0 is the top.
    fn child() -> Schematic {
        let mut s = Schematic::default();
        s.add_sheet();
        let r = s.add(Kind::Resistor, (10, 10), 0);
        let p = s.components[r].pins();
        s.ports.push(SheetPort { pos: p[0].1, name: "IN".into(), dir: PortDir::In });
        s.ports.push(SheetPort { pos: p[1].1, name: "OUT".into(), dir: PortDir::Out });
        s.switch_sheet(0);
        s
    }

    fn messages(s: &Schematic) -> Vec<String> {
        erc(s).into_iter().map(|p| p.message).collect()
    }

    #[test]
    fn an_input_port_left_alone_on_the_parent_is_a_warning_and_an_output_is_not() {
        let mut s = child();
        s.add_sheet_symbol(1, (10, 10)).unwrap();
        let m = messages(&s);
        assert!(m.iter().any(|m| m.contains("CH1 のポート IN")), "{m:?}");
        assert!(!m.iter().any(|m| m.contains("ポート OUT")), "an output left open is fine: {m:?}");
        // wire the input: the warning goes
        let pin = s.components[0].pins().into_iter().find(|(n, _)| n == "IN").unwrap().1;
        s.labels.push(Label { pos: pin, text: "SIG".into() });
        assert!(!messages(&s).iter().any(|m| m.contains("CH1 のポート IN")));
    }

    #[test]
    fn a_port_with_nothing_under_it_is_reported_on_the_child_sheet() {
        let mut s = child();
        s.switch_sheet(1);
        s.ports.push(SheetPort { pos: (90, 90), name: "LOST".into(), dir: PortDir::Io });
        s.switch_sheet(0);
        s.add_sheet_symbol(1, (10, 10)).unwrap();
        let p = erc(&s).into_iter().find(|p| p.message.contains("ポート LOST が、シートの中")).expect("LOST");
        assert_eq!((p.sheet, p.at), (1, Some((90, 90))));
    }

    #[test]
    fn two_ports_of_one_name_and_a_sheet_without_ports_are_reported() {
        let mut s = child();
        s.switch_sheet(1);
        s.ports.push(SheetPort { pos: (12, 12), name: "IN".into(), dir: PortDir::In });
        s.switch_sheet(0);
        assert!(messages(&s).iter().any(|m| m.contains("ポート IN が2つ")));
        // a third sheet without ports, used by a symbol
        s.add_sheet();
        s.switch_sheet(0);
        s.add_sheet_symbol(2, (30, 10)).unwrap();
        assert!(messages(&s).iter().any(|m| m.contains("に、ポートがありません")));
    }

    #[test]
    fn two_outputs_on_one_net_are_an_error_but_an_output_into_an_input_is_not() {
        let mut s = child();
        let a = s.add_sheet_symbol(1, (10, 10)).unwrap();
        let b = s.add_sheet_symbol(1, (10, 40)).unwrap();
        let pin = |s: &Schematic, i: usize, n: &str| s.components[i].pins().into_iter().find(|(p, _)| p == n).unwrap().1;
        // CH1.OUT feeds CH2.IN: fine
        s.labels.push(Label { pos: pin(&s, a, "OUT"), text: "MID".into() });
        s.labels.push(Label { pos: pin(&s, b, "IN"), text: "MID".into() });
        assert!(!messages(&s).iter().any(|m| m.contains("出力のポート")), "{:?}", messages(&s));
        // CH2.OUT onto the same net as CH1.OUT: two outputs
        s.labels.push(Label { pos: pin(&s, b, "OUT"), text: "MID".into() });
        let p = erc(&s).into_iter().find(|p| p.message.contains("出力のポート")).expect("two outputs");
        assert_eq!(p.severity, crate::netlist::Severity::Error);
    }

    #[test]
    fn an_input_nothing_feeds_is_a_warning_until_a_source_or_an_output_is_wired() {
        let mut s = child();
        let a = s.add_sheet_symbol(1, (10, 10)).unwrap();
        let pin = |s: &Schematic, i: usize, n: &str| s.components[i].pins().into_iter().find(|(p, _)| p == n).unwrap().1;
        let fed = |s: &Schematic| !messages(s).iter().any(|m| m.contains("入力のポート IN に"));
        // a name that nothing else uses: the pin is wired but leads nowhere
        s.labels.push(Label { pos: pin(&s, a, "IN"), text: "SIG".into() });
        assert!(!fed(&s), "{:?}", messages(&s));
        // a voltage source on that name feeds it
        let v = s.add(Kind::VSource, (60, 60), 0);
        let vp = s.components[v].pins()[0].1;
        s.labels.push(Label { pos: vp, text: "SIG".into() });
        assert!(fed(&s), "{:?}", messages(&s));
        // through a resistor from a source also counts
        s.labels.retain(|l| l.text != "SIG" || l.pos != vp);
        assert!(!fed(&s));
        let r = s.add(Kind::Resistor, (60, 90), 0);
        let rp = s.components[r].pins();
        s.labels.push(Label { pos: rp[0].1, text: "SIG".into() });
        s.labels.push(Label { pos: rp[1].1, text: "SRC".into() });
        s.labels.push(Label { pos: vp, text: "SRC".into() });
        assert!(fed(&s), "{:?}", messages(&s));
    }

    #[test]
    fn an_output_of_another_symbol_feeds_an_input() {
        let mut s = child();
        let a = s.add_sheet_symbol(1, (10, 10)).unwrap();
        let b = s.add_sheet_symbol(1, (10, 40)).unwrap();
        let pin = |s: &Schematic, i: usize, n: &str| s.components[i].pins().into_iter().find(|(p, _)| p == n).unwrap().1;
        s.labels.push(Label { pos: pin(&s, a, "OUT"), text: "MID".into() });
        s.labels.push(Label { pos: pin(&s, b, "IN"), text: "MID".into() });
        let m = messages(&s);
        assert!(!m.iter().any(|m| m.contains("CH2 の入力のポート IN に")), "{m:?}");
        assert!(m.iter().any(|m| m.contains("CH1 の入力のポート IN")) || m.iter().any(|m| m.contains("CH1 のポート IN")), "CH1 IN is left alone: {m:?}");
    }

    #[test]
    fn a_ring_of_symbols_is_an_error() {
        let mut s = child();
        s.add_sheet_symbol(1, (10, 10)).unwrap();
        s.switch_sheet(1);
        s.add_sheet_symbol(0, (40, 40)).unwrap();
        s.switch_sheet(0);
        assert!(erc(&s).iter().any(|p| p.message.contains("自分自身を含む") && p.severity == crate::netlist::Severity::Error));
    }

    #[test]
    fn every_hierarchy_finding_explains_itself() {
        let mut s = child();
        s.add_sheet_symbol(1, (10, 10)).unwrap();
        s.switch_sheet(1);
        s.ports.push(SheetPort { pos: (90, 90), name: "IN".into(), dir: PortDir::In });
        s.switch_sheet(0);
        for p in erc(&s) {
            assert!(p.hint.len() > 20 && !p.hint.contains("{}"), "{}: {}", p.message, p.hint);
        }
    }
}

#[cfg(test)]
mod backannotate_tests {
    use super::*;

    #[test]
    fn a_trade_on_a_copy_of_a_child_sheet_is_not_put_into_the_drawing() {
        let mut s = Schematic::default();
        s.add_sheet();
        s.add(Kind::Resistor, (10, 10), 0);
        s.switch_sheet(0);
        s.add_sheet_symbol(1, (10, 10)).unwrap();
        let trades = std::collections::BTreeMap::from([("R1_CH1".to_string(), std::collections::BTreeMap::from([("1".to_string(), "2".to_string()), ("2".to_string(), "1".to_string())]))]);
        let before = s.clone();
        let rep = s.apply_pin_trades(&trades).unwrap();
        assert!(rep.parts.is_empty());
        assert_eq!(rep.copies, ["R1_CH1"]);
        assert_eq!(s, before, "nothing in the drawing changed");
    }
}

/// Gives the sheet symbols among `comps` the symbols `syms` (by their sheet), and keeps what is joined to a pin that moved joined.
fn follow_pins(comps: &mut [Component], wires: &mut Vec<Wire>, labels: &mut [Label], syms: &[CustomSymbol]) {
    // what to do, found before anything moves (a pin may move to the place another pin leaves)
    let mut stubs: Vec<Wire> = vec![];
    let mut label_moves: Vec<(usize, Pt)> = vec![];
    for c in comps.iter_mut() {
        let Some(sheet) = c.sub.as_ref().map(|s| s.sheet) else { continue };
        let Some(sym) = syms.get(sheet) else { continue };
        let old = c.pins();
        c.custom = Some(sym.clone());
        for (number, from) in old {
            let Some((_, to)) = c.pins().into_iter().find(|(n, _)| *n == number) else { continue };
            if to == from {
                continue;
            }
            if wires.iter().any(|w| w.a == from || w.b == from) {
                // keep the wire where it is; a short wire (or two, with a corner) joins it to the pin
                if to.0 == from.0 || to.1 == from.1 {
                    stubs.push(Wire { a: to, b: from });
                } else {
                    let corner = (to.0, from.1);
                    stubs.push(Wire { a: to, b: corner });
                    stubs.push(Wire { a: corner, b: from });
                }
            } else {
                for (i, l) in labels.iter().enumerate() {
                    if l.pos == from {
                        label_moves.push((i, to));
                    }
                }
            }
        }
    }
    for (i, to) in label_moves {
        labels[i].pos = to;
    }
    wires.extend(stubs);
}

#[cfg(test)]
mod pin_order_tests {
    use super::*;

    #[test]
    fn a_port_added_later_keeps_the_wired_pins_joined_and_in_order() {
        let mut s = Schematic::default();
        s.add_sheet();
        let r = s.add(Kind::Resistor, (10, 10), 0);
        let p = s.components[r].pins();
        s.ports.push(SheetPort { pos: p[0].1, name: "IN".into(), dir: PortDir::In });
        s.ports.push(SheetPort { pos: p[1].1, name: "OUT".into(), dir: PortDir::Out });
        s.switch_sheet(0);
        let ch = s.add_sheet_symbol(1, (20, 20)).unwrap();
        let pin = |s: &Schematic, n: &str| s.components[ch].pins().into_iter().find(|(x, _)| x == n).unwrap().1;
        // IN is wired to a far point and OUT is named by a label
        let far = (pin(&s, "IN").0 - 10, pin(&s, "IN").1);
        s.wires.push(Wire { a: far, b: pin(&s, "IN") });
        s.labels.push(Label { pos: far, text: "SIG".into() });
        s.labels.push(Label { pos: pin(&s, "OUT"), text: "RES".into() });
        let nets_before = extract(&s);
        let (in_before, out_before) = (pin(&s, "IN"), pin(&s, "OUT"));
        // a third port (output): the pins on the right are centred again
        s.switch_sheet(1);
        s.ports.push(SheetPort { pos: (60, 60), name: "EN".into(), dir: PortDir::Out });
        s.switch_sheet(0);
        s.refresh_sheet_symbols();
        assert!(pin(&s, "OUT") != out_before || pin(&s, "IN") != in_before, "the test needs a pin that moves");
        let pins: Vec<String> = s.components[ch].pins().into_iter().map(|(n, _)| n).collect();
        assert_eq!(pins, ["IN", "OUT", "EN"], "in the order the ports were put");
        let nets = extract(&s);
        let on = |nets: &[Net], name: &str, refdes: &str| nets.iter().any(|n| n.name == name && n.pins.iter().any(|p| p.refdes.starts_with(refdes)));
        for (name, refdes) in [("SIG", "R1_CH1"), ("RES", "R1_CH1")] {
            assert!(on(&nets_before, name, refdes) && on(&nets, name, refdes), "{name} still reaches {refdes}: {nets:?}");
        }
    }
}

impl Schematic {
    /// The parts of sheet `k` that a symbol can give another value in its copy: (reference designator, the value the sheet has).
    pub fn value_rows(&self, k: usize) -> Vec<(String, String)> {
        if k >= self.sheet_count() {
            return vec![];
        }
        self.parts(k).comps.iter().filter(|c| c.sub.is_none() && !c.kind.is_power_symbol() && c.kind != Kind::Ground).map(|c| (c.refdes.clone(), c.value.clone())).collect()
    }
}

#[cfg(test)]
mod value_tests {
    use super::*;

    #[test]
    fn a_copy_can_give_a_part_its_own_value_and_the_others_keep_the_sheets() {
        let mut s = Schematic::default();
        s.add_sheet();
        let r = s.add(Kind::Resistor, (10, 10), 0);
        s.components[r].value = "10k".into();
        s.switch_sheet(0);
        let a = s.add_sheet_symbol(1, (10, 10)).unwrap();
        let b = s.add_sheet_symbol(1, (40, 10)).unwrap();
        s.components[b].sub.as_mut().unwrap().values.insert("R1".into(), "4.7k".into());
        let value_of = |s: &Schematic, r: &str| s.flat().components.iter().find(|c| c.refdes == r).map(|c| c.value.clone());
        assert_eq!(value_of(&s, "R1_CH1").as_deref(), Some("10k"));
        assert_eq!(value_of(&s, "R1_CH2").as_deref(), Some("4.7k"));
        assert!(s.components[a].sub.as_ref().unwrap().values.is_empty());
        assert_eq!(s.value_rows(1), [("R1".to_string(), "10k".to_string())]);
        // the sheet itself did not change, and the override is saved
        s.switch_sheet(1);
        assert_eq!(s.components[0].value, "10k");
        s.switch_sheet(0);
        let back: Schematic = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(value_of(&back, "R1_CH2").as_deref(), Some("4.7k"));
    }

    #[test]
    fn a_value_for_a_part_the_sheet_does_not_have_does_nothing() {
        let mut s = Schematic::default();
        s.add_sheet();
        s.add(Kind::Resistor, (10, 10), 0);
        s.switch_sheet(0);
        let a = s.add_sheet_symbol(1, (10, 10)).unwrap();
        s.components[a].sub.as_mut().unwrap().values.insert("R9".into(), "1".into());
        let own = s.value_rows(1)[0].1.clone();
        assert_eq!(s.flat().components.iter().map(|c| c.value.clone()).collect::<Vec<_>>(), [own]);
    }
}
