//! A stripboard (and the same layout on a plain perfboard) for building the schematic by hand (B5). The copper strips run along the
//! rows; a strip is broken by cutting it at a hole.
//!
//! The layout: every net gets a strip of its own at the top ("home" strips: the supplies first, ground last). A two-lead part stands
//! across the board between the home strips of its two nets, a column apart from the next. A chip or a part with its leads in a row goes below the home strips:
//! each of its pins sits on a piece of strip of its own (cut off on both sides), and a wire link goes from that piece up to the pin's
//! net's home strip, each in a column of its own so no two links overlap. The supplies off the board come in by wires at the left.
//! On a perfboard (no strips) the same layout holds with a bare wire along each piece of strip from its first used hole to its last.

use crate::breadboard::{is_ground_name, read, Look};
use crate::mount::Mount;
use crate::{extract, Schematic};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A hole: (row, column), both from 0. Its name is the row's letter and the column's number from 1 (`C12`).
pub type At = (u16, u16);

/// The name of a hole: row A, B, … Z, AA, AB, …; column from 1.
pub fn hole_name(at: At) -> String {
    let mut r = at.0 as usize;
    let mut letters = String::new();
    loop {
        letters.insert(0, (b'A' + (r % 26) as u8) as char);
        if r < 26 {
            break;
        }
        r = r / 26 - 1;
    }
    format!("{letters}{}", at.1 + 1)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StripPart {
    pub refdes: String,
    pub look: Look,
    /// Each lead's pin and hole.
    pub leads: Vec<(String, At)>,
    /// How it goes on: in a socket, on an adapter board (a SOP on a DIP board), or straight in.
    #[serde(default)]
    pub mount: Mount,
}

/// A wire link between two holes (on the parts' side), carrying `net`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub a: At,
    pub b: At,
    pub net: String,
}

/// An off-board part's pin: its pin, net and the hole its wire goes to.
pub type OffWire = (String, String, At);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StripLayout {
    pub rows: u16,
    pub cols: u16,
    /// The net of each home strip (rows 0..), top to bottom.
    pub homes: Vec<String>,
    pub parts: Vec<StripPart>,
    /// The holes where a strip is cut.
    pub cuts: BTreeSet<At>,
    pub links: Vec<Link>,
    /// The parts off the board (supplies, sources): each pin, its net and the hole its wire goes to.
    pub off: Vec<(String, Vec<OffWire>)>,
    /// The sections (the requirements table's groups marked as one): each name and its corners (top left, bottom right).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<(String, At, At)>,
    /// The strips' names written where a section's strip starts (with sections, `homes` is empty: rows side by side differ).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<(At, String)>,
    /// Bare wires drawn by hand on the copper side of a perfboard, each straight along a row or a column (`StripLayout::bare_joins`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bare: Vec<(At, At)>,
    /// Laid out for a perfboard (`perfboard::perf_layout_with`): no strips, every hole apart, joined only by the bare wires and the
    /// links.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub perf: bool,
    /// The parts the person fixed where they are (perfboard): "固定した部品のまわりを並べ直す" (`perfboard::perf_layout_around`)
    /// lays the others out round them.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub locked: BTreeSet<String>,
}

impl StripLayout {
    /// The automatic bare wires of a perfboard: along each piece of strip from its first hole in use to its last (a lead, a link's or
    /// an off-board wire's end, or the end of a bare wire drawn by hand), where there are two.
    pub fn perf_spans(&self) -> Vec<(At, At)> {
        let used: Vec<At> = self.used().into_iter().map(|(a, _)| a).chain(self.bare.iter().flat_map(|(a, b)| [*a, *b])).collect();
        self.pieces()
            .into_iter()
            .filter_map(|(r, a, b)| {
                let cols: Vec<u16> = used.iter().filter(|u| u.0 == r && u.1 >= a && u.1 <= b).map(|u| u.1).collect();
                let (lo, hi) = (cols.iter().min()?, cols.iter().max()?);
                (lo < hi).then_some(((r, *lo), (r, *hi)))
            })
            .collect()
    }

    /// What the bare wires drawn by hand join: each wire's first hole to every hole along it that it touches (its two ends, a hole
    /// in use, a hole an automatic bare wire runs through, or a hole on another bare wire: two bare wires that cross on the copper
    /// side touch).
    pub fn bare_joins(&self) -> Vec<(At, At)> {
        let spans = self.perf_spans();
        let used: BTreeSet<At> = self.used().into_iter().map(|(a, _)| a).collect();
        let on_span = |h: At| spans.iter().any(|(a, b)| a.0 == h.0 && h.1 >= a.1 && h.1 <= b.1);
        let along = |a: At, b: At| -> Vec<At> { if a.0 == b.0 { (a.1.min(b.1)..=a.1.max(b.1)).map(|c| (a.0, c)).collect() } else { (a.0.min(b.0)..=a.0.max(b.0)).map(|r| (r, a.1)).collect() } };
        // how many bare wires run over each hole
        let mut wires: BTreeMap<At, usize> = BTreeMap::new();
        for (a, b) in &self.bare {
            for h in along(*a, *b) {
                *wires.entry(h).or_default() += 1;
            }
        }
        let mut out = vec![];
        for (a, b) in &self.bare {
            for h in along(*a, *b) {
                if h != *a && (h == *b || used.contains(&h) || on_span(h) || wires[&h] > 1) {
                    out.push((*a, h));
                }
            }
        }
        out
    }

    /// Everything that joins two holes besides the strips: the links, and the bare wires drawn by hand.
    pub fn joins(&self) -> Vec<(At, At)> {
        self.links.iter().map(|k| (k.a, k.b)).chain(self.bare_joins()).collect()
    }

    /// The pieces of strip: (row, first column, last column), each between two cuts (or the board's ends).
    pub fn pieces(&self) -> Vec<(u16, u16, u16)> {
        if self.perf {
            return (0..self.rows).flat_map(|r| (0..self.cols).map(move |c| (r, c, c))).collect();
        }
        let mut out = vec![];
        for r in 0..self.rows {
            let mut start = 0u16;
            for c in 0..=self.cols {
                if c == self.cols || self.cuts.contains(&(r, c)) {
                    if c > start {
                        out.push((r, start, c - 1));
                    }
                    start = c + 1;
                }
            }
        }
        out
    }

    /// Every hole used (a lead, a link's end, an off-board wire's end) and by what.
    pub fn used(&self) -> Vec<(At, String)> {
        let mut v: Vec<(At, String)> = self.parts.iter().flat_map(|p| p.leads.iter().map(move |(pin, at)| (*at, format!("{}.{pin}", p.refdes)))).collect();
        for (k, l) in self.links.iter().enumerate() {
            v.push((l.a, format!("link{k}")));
            v.push((l.b, format!("link{k}")));
        }
        for (r, pins) in &self.off {
            for (pin, _, at) in pins {
                v.push((*at, format!("{r}.{pin}")));
            }
        }
        v
    }
}

/// How a net is carried on the board.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Carry {
    /// A home strip of its own across the top (the supplies, ground, the nets off-board parts or several parts share).
    Home,
    /// One chip pin and one two-lead part: the part itself stands from the pin's piece of strip to the home strip of its other net
    /// (the part's lead is the wire).
    Via(usize),
    /// Chip pins only: wires from piece to piece.
    Pins,
    /// One lead alone: nothing to join.
    Alone,
}

/// Who is on a net: the chip / row parts' pins, the two-lead parts (by index into the parts on the board), off-board parts.
#[derive(Default)]
struct Members {
    pins: Vec<(String, String)>,
    twos: Vec<usize>,
    off: bool,
}

/// A chip pin's piece of strip: its row, first and last column, and the column of its wire.
type Piece = (u16, u16, u16, u16);

/// The chips and row parts in the order they go along the board: each next to the one it shares the most pin-to-pin nets with
/// (the wires between them stay short), else in the schematic's order.
fn chip_order(on: &[(String, Look, crate::breadboard::Pins)], carry: &BTreeMap<String, Carry>) -> Vec<usize> {
    let chips: Vec<usize> = (0..on.len()).filter(|k| on[*k].1 != Look::TwoLead).collect();
    let shared = |a: usize, b: usize| -> usize {
        let nets_b: BTreeSet<&String> = on[b].2.iter().map(|(_, n)| n).filter(|n| carry.get(*n) == Some(&Carry::Pins)).collect();
        on[a].2.iter().map(|(_, n)| n).filter(|n| nets_b.contains(n)).count()
    };
    let mut out: Vec<usize> = vec![];
    let mut left: Vec<usize> = chips;
    while !left.is_empty() {
        let next = match out.last() {
            None => 0,
            Some(&last) => (0..left.len()).max_by_key(|i| (shared(last, left[*i]), std::cmp::Reverse(*i))).unwrap_or(0),
        };
        out.push(left.remove(next));
    }
    out
}

/// How to lay the board out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StripOptions {
    /// The board's width in holes: the chips wrap to a new row of chips past it. None: one row of chips, as wide as it takes.
    pub cols: Option<u16>,
}

/// Lays the schematic out on a stripboard, one row of chips.
pub fn strip_layout(s: &Schematic) -> StripLayout {
    strip_layout_with(s, StripOptions::default())
}

/// Which way a chip pin's wire (or the part standing in its place) goes: to the band of strips above its row of chips or below it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dir {
    Up,
    Down,
}

/// A chip pin's place within its block: its row from the block's top, its side, and (when it goes to a strip) the way and the
/// column of its wire, counted from the block's first column.
struct PinPlace {
    row: u16,
    left: bool,
    wire: Option<(Dir, u16)>,
}

/// A chip or a row part laid out by itself, its columns counted from the block's first: its pins' places, the columns of its two
/// rows of leads, and the column of the cut that ends it.
struct Block {
    k: usize,
    look: Look,
    mount: Mount,
    half: u16,
    pins: Vec<PinPlace>,
    left_pin: u16,
    right_pin: u16,
    end: u16,
}

/// Lays the schematic out on a stripboard (and the same on a perfboard).
///
/// The board is bands of strips and rows of chips in turn: a band at the top, a row of chips, a band, a row of chips … a band at the
/// bottom. A band has a strip for each net its neighbours need. A chip pin in the upper half of its chip has its wire go up to the
/// band above, a pin in the lower half down to the band below, so one column carries a wire up and another down. The chips wrap to a
/// new row past the board's width (`StripOptions::cols`). A two-lead part stands across two strips of a band, in a column free there.
/// The strips of one net in different bands are joined by wires at the left, beside the wires of the supplies off the board.
pub fn strip_layout_with(s: &Schematic, opt: StripOptions) -> StripLayout {
    let c = read(s);
    let secs = sections(s, &c);
    let mut l = if secs.len() < 2 { lay(&c, opt, &BTreeSet::new()) } else { lay_sections(&c, opt, &secs) };
    keep_forks(s, &mut l);
    l
}

/// The sections of the board: the requirements table's groups marked as sections, in the table's order, with the parts on the board
/// that are in them (a part in two goes with the first); the parts in none make one more section at the end ("その他").
pub(crate) fn sections(s: &Schematic, c: &crate::breadboard::Circuit) -> Vec<(String, BTreeSet<String>)> {
    let on: BTreeSet<&String> = c.on.iter().map(|p| &p.0).collect();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out: Vec<(String, BTreeSet<String>)> = vec![];
    for g in s.requirements.groups.iter().filter(|g| g.section) {
        let parts: BTreeSet<String> = g.parts.iter().filter(|p| on.contains(p) && !seen.contains(*p)).cloned().collect();
        if parts.is_empty() {
            continue;
        }
        seen.extend(parts.iter().cloned());
        out.push((g.name.clone(), parts));
    }
    if out.is_empty() {
        return out;
    }
    let rest: BTreeSet<String> = c.on.iter().map(|p| p.0.clone()).filter(|r| !seen.contains(r)).collect();
    if !rest.is_empty() {
        out.push(("その他".into(), rest));
    }
    out
}

/// Each section laid out by itself, the sections side by side (wrapped past the board's width), every strip cut between two
/// sections. A net in more than one section has a strip in each, joined by a wire from section to section; ground instead goes
/// from each section's strip by a wire of its own to the ground strip of the first section that has one (the one ground point: the
/// return current of one section does not flow through another's ground).
fn lay_sections(c: &crate::breadboard::Circuit, opt: StripOptions, secs: &[(String, BTreeSet<String>)]) -> StripLayout {
    let nets_of = |parts: &BTreeSet<String>| -> BTreeSet<String> { c.on.iter().filter(|p| parts.contains(&p.0)).flat_map(|p| p.2.iter().map(|x| x.1.clone())).filter(|n| !n.is_empty()).collect() };
    let sec_nets: Vec<BTreeSet<String>> = secs.iter().map(|(_, p)| nets_of(p)).collect();
    // an off-board part goes with the section that has the most of its nets
    let off_sec: Vec<usize> = c
        .off
        .iter()
        .map(|(_, pins)| (0..secs.len()).max_by_key(|i| (pins.iter().filter(|(_, n)| sec_nets[*i].contains(n)).count(), std::cmp::Reverse(*i))).unwrap_or(0))
        .collect();
    let mut nets_in: Vec<BTreeSet<String>> = sec_nets.clone();
    for (k, (_, pins)) in c.off.iter().enumerate() {
        nets_in[off_sec[k]].extend(pins.iter().map(|(_, n)| n.clone()).filter(|n| !n.is_empty()));
    }
    let shared = |n: &String, i: usize| (0..secs.len()).any(|j| j != i && nets_in[j].contains(n));
    let mut subs: Vec<StripLayout> = vec![];
    for (i, (_, parts)) in secs.iter().enumerate() {
        let sub = crate::breadboard::Circuit {
            on: c.on.iter().filter(|p| parts.contains(&p.0)).cloned().collect(),
            off: c.off.iter().enumerate().filter(|(k, _)| off_sec[*k] == i).map(|(_, o)| o.clone()).collect(),
            supplies: c.supplies.clone(),
            ground: c.ground.clone(),
            mounts: c.mounts.clone(),
        };
        let external: BTreeSet<String> = nets_in[i].iter().filter(|n| shared(n, i)).cloned().collect();
        subs.push(lay(&sub, opt, &external));
    }
    // the sections side by side, a column between (cut on every row), a new shelf past the board's width
    let mut out = StripLayout::default();
    let (mut x, mut y, mut shelf_h) = (0u16, 0u16, 0u16);
    let mut shelf_gaps: Vec<u16> = vec![];
    let mut origin: Vec<(u16, u16)> = vec![];
    let close_shelf = |out: &mut StripLayout, y: u16, h: u16, gaps: &mut Vec<u16>| {
        for g in gaps.drain(..) {
            for r in y..y + h {
                out.cuts.insert((r, g));
            }
        }
    };
    for (i, l) in subs.iter().enumerate() {
        if let Some(w) = opt.cols
            && x > 0
            && x + l.cols > w
        {
            close_shelf(&mut out, y, shelf_h, &mut shelf_gaps);
            shelf_gaps.clear();
            y += shelf_h + 1;
            x = 0;
            shelf_h = 0;
        }
        let at = |a: At| (a.0 + y, a.1 + x);
        for p in &l.parts {
            let mut p = p.clone();
            for (_, a) in &mut p.leads {
                *a = at(*a);
            }
            out.parts.push(p);
        }
        for k in &l.links {
            out.links.push(Link { a: at(k.a), b: at(k.b), net: k.net.clone() });
        }
        out.cuts.extend(l.cuts.iter().map(|c| at(*c)));
        for (r, pins) in &l.off {
            out.off.push((r.clone(), pins.iter().map(|(p, n, a)| (p.clone(), n.clone(), at(*a))).collect()));
        }
        for (r, n) in l.homes.iter().enumerate().filter(|(_, n)| !n.is_empty()) {
            out.names.push(((r as u16 + y, x), n.clone()));
        }
        out.names.extend(l.names.iter().map(|(a, n)| (at(*a), n.clone())));
        out.sections.push((secs[i].0.clone(), (y, x), (y + l.rows - 1, x + l.cols - 1)));
        origin.push((y, x));
        shelf_h = shelf_h.max(l.rows);
        x += l.cols;
        shelf_gaps.push(x);
        x += 1;
    }
    shelf_gaps.pop();
    close_shelf(&mut out, y, shelf_h, &mut shelf_gaps);
    // the nets shared by sections: a wire between their strips (ground: each to the first section's)
    // a net's strip in a section: its row and the stretch of it the net has (a row may carry more than one net, cut between)
    let strip_row = |i: usize, n: &str| strip_of(&subs[i], n).map(|(r, a, b)| (r + origin[i].0, a + origin[i].1, b + origin[i].1));
    let mut taken: BTreeSet<At> = out.used().into_iter().map(|(a, _)| a).collect();
    let free_on = |(row, x0, x1): (u16, u16, u16), toward: u16, taken: &mut BTreeSet<At>, cuts: &BTreeSet<At>| -> Option<At> {
        let mut cols: Vec<u16> = (x0..=x1).collect();
        cols.sort_by_key(|c| c.abs_diff(toward));
        let a = cols.into_iter().map(|c| (row, c)).find(|a| !taken.contains(a) && !cuts.contains(a))?;
        taken.insert(a);
        Some(a)
    };
    let mut nets: Vec<&String> = nets_in.iter().flatten().collect();
    nets.sort();
    nets.dedup();
    for n in nets {
        let have: Vec<usize> = (0..secs.len()).filter(|i| strip_row(*i, n).is_some()).collect();
        if have.len() < 2 {
            continue;
        }
        let pairs: Vec<(usize, usize)> = if is_ground_name(n) { have[1..].iter().map(|j| (have[0], *j)).collect() } else { have.windows(2).map(|w| (w[0], w[1])).collect() };
        for (i, j) in pairs {
            let (si, sj) = (strip_row(i, n).unwrap(), strip_row(j, n).unwrap());
            let (ci, cj) = (origin[i].1, origin[j].1);
            if let (Some(a), Some(b)) = (free_on(si, cj, &mut taken, &out.cuts), free_on(sj, ci, &mut taken, &out.cuts)) {
                out.links.push(Link { a, b, net: n.clone() });
            }
        }
    }
    let far = out.used().into_iter().map(|(a, _)| a).chain(out.cuts.iter().copied()).fold((0u16, 0u16), |m, a| (m.0.max(a.0), m.1.max(a.1)));
    out.rows = far.0.max(y + shelf_h.max(1) - 1) + 1;
    out.cols = far.1 + 1;
    out
}

/// A net's strip in a layout of one section: its row and the stretch of the row it has (first and last column; a row shared by nets
/// is cut between them, its first net named in `homes`, the others in `names` where theirs starts).
fn strip_of(l: &StripLayout, n: &str) -> Option<(u16, u16, u16)> {
    let pieces = l.pieces();
    let piece = |at: At| pieces.iter().find(|(r, a, b)| *r == at.0 && at.1 >= *a && at.1 <= *b).copied();
    let first = l.homes.iter().position(|h| h == n).and_then(|r| piece((r as u16, 0)));
    let other = l.names.iter().filter(|(_, m)| m == n).filter_map(|(a, _)| piece(*a)).min();
    [first, other].into_iter().flatten().min()
}

/// Lays out one section (or the whole board): `external` are the nets that also go to other sections (they get a strip in the top
/// band, as the supplies do).
fn lay(c: &crate::breadboard::Circuit, opt: StripOptions, external: &BTreeSet<String>) -> StripLayout {
    let mut order: Vec<String> = vec![];
    let mut members: BTreeMap<String, Members> = BTreeMap::new();
    let seen = |n: &str, order: &mut Vec<String>| {
        if !n.is_empty() && !order.iter().any(|x| x == n) {
            order.push(n.to_string());
        }
    };
    for (k, (refdes, look, pins)) in c.on.iter().enumerate() {
        for (pin, n) in pins.iter().filter(|(_, n)| !n.is_empty()) {
            seen(n, &mut order);
            let m = members.entry(n.clone()).or_default();
            if *look == Look::TwoLead {
                if !m.twos.contains(&k) {
                    m.twos.push(k);
                }
            } else {
                m.pins.push((refdes.clone(), pin.clone()));
            }
        }
    }
    for (_, pins) in &c.off {
        for (_, n) in pins.iter().filter(|(_, n)| !n.is_empty()) {
            seen(n, &mut order);
            members.entry(n.clone()).or_default().off = true;
        }
    }
    for n in external {
        if let Some(m) = members.get_mut(n) {
            m.off = true;
        }
    }
    let other_net = |k: usize, n: &str| c.on[k].2.iter().map(|(_, m)| m.clone()).find(|m| m != n && !m.is_empty());
    // how each net is carried: a strip unless it can do without
    let mut carry: BTreeMap<String, Carry> = BTreeMap::new();
    for n in &order {
        let m = &members[n];
        let rail = is_ground_name(n) || c.supplies.contains(n);
        let kind = if rail || m.off {
            Carry::Home
        } else if m.pins.len() + m.twos.len() <= 1 {
            Carry::Alone
        } else if m.twos.is_empty() {
            Carry::Pins
        } else if m.pins.len() == 1 && m.twos.len() == 1 && other_net(m.twos[0], n).is_some() {
            Carry::Via(m.twos[0])
        } else {
            Carry::Home
        };
        carry.insert(n.clone(), kind);
    }
    // a part can be the wire of one net only; the net at its other end keeps (or gets) a strip
    let mut used_parts: BTreeSet<usize> = BTreeSet::new();
    for n in &order {
        let Carry::Via(k) = carry[n] else { continue };
        let other = other_net(k, n).unwrap();
        let ok = !used_parts.contains(&k)
            && match carry.get(&other) {
                Some(Carry::Home) => true,
                Some(Carry::Via(_)) => {
                    carry.insert(other.clone(), Carry::Home);
                    true
                }
                _ => false,
            };
        if ok {
            used_parts.insert(k);
        } else {
            carry.insert(n.clone(), Carry::Home);
        }
    }
    // a socket or an adapter board decides the shape: two rows its width apart, or one row (a SIP)
    let mount_of = |refdes: &str| c.mounts.get(refdes).cloned().unwrap_or_default();
    let look_on = |k: usize| -> Look {
        let (refdes, look, _) = &c.on[k];
        let m = mount_of(refdes);
        match *look {
            Look::TwoLead => Look::TwoLead,
            _ if m.width().is_some() => Look::Dip,
            _ if m.one_row() => Look::Row,
            other => other,
        }
    };
    // each chip by itself: its pins' rows and sides, the way of each wire and its column
    let to_strip = |n: &str| matches!(carry.get(n), Some(Carry::Home | Carry::Via(_)));
    let mut blocks: Vec<Block> = vec![];
    for k in chip_order(&c.on, &carry) {
        let (refdes, _, pins) = &c.on[k];
        let look = look_on(k);
        let mount = mount_of(refdes);
        // each lead's place on the socket or the adapter, by its pin number (a pin the symbol leaves out leaves its place empty)
        let (places, slot) = mount.slots(pins);
        let (half, two_cols) = if look == Look::Dip { (places.div_ceil(2), true) } else { (places, false) };
        // two spare holes at each piece's outer end when a pin is wired to other chip pins (two wire ends may share a piece)
        let spare = if pins.iter().any(|(_, n)| carry.get(n) == Some(&Carry::Pins)) { 2 } else { 0 };
        let mut placed: Vec<PinPlace> = pins
            .iter()
            .enumerate()
            .map(|(i, (_, n))| {
                let p = slot[i];
                // a chip's first half down the left column, the rest up the right one (pin 1 top left)
                let (row, left) = if p < half { (p as u16, true) } else { ((places - 1 - p) as u16, false) };
                // the upper half of the rows up, the lower half down (a single row: up)
                let dir = if half <= 1 || (row as usize) * 2 < half { Dir::Up } else { Dir::Down };
                PinPlace { row, left, wire: to_strip(n).then_some((dir, 0)) }
            })
            .collect();
        // a side's wires going up take columns from the outside in by their rows, those going down the same columns again (a wire up
        // ends above every wire down, so the two never meet)
        let mut count = [0u16; 2];
        for left in [true, false] {
            for dir in [Dir::Up, Dir::Down] {
                let mut idx: Vec<usize> = (0..placed.len()).filter(|i| placed[*i].left == left && placed[*i].wire.is_some_and(|w| w.0 == dir)).collect();
                idx.sort_by_key(|i| placed[*i].row);
                for (n, i) in idx.iter().enumerate() {
                    placed[*i].wire = Some((dir, n as u16));
                }
                let side = if left { 0 } else { 1 };
                count[side] = count[side].max(idx.len() as u16);
            }
        }
        let x = spare;
        let left_pin = x + count[0];
        let right_pin = left_pin + mount.width().unwrap_or(3);
        let end = if two_cols { right_pin + count[1] + 1 + spare } else { left_pin + 1 };
        for p in &mut placed {
            if let Some((d, n)) = p.wire {
                p.wire = Some((d, if p.left { x + n } else { right_pin + 1 + n }));
            }
        }
        blocks.push(Block { k, look, mount, half: half as u16, pins: placed, left_pin, right_pin, end });
    }
    // the off-board parts' wires: one column each at the far left, into the top band
    let off_pins: usize = c.off.iter().map(|(_, p)| p.iter().filter(|(_, n)| carry.get(n) == Some(&Carry::Home)).count()).sum();
    let two_nets = |k: usize| -> Vec<String> {
        let mut v: Vec<String> = c.on[k].2.iter().map(|(_, n)| n.clone()).filter(|n| !n.is_empty()).collect();
        v.dedup();
        v
    };
    // the strips' order in a band: the supplies first, ground last, the rest as the nets first come
    let rank = |n: &str| -> (u8, usize) {
        if let Some(i) = c.supplies.iter().position(|x| x == n) {
            (0, i)
        } else if is_ground_name(n) {
            (2, 0)
        } else {
            (1, order.iter().position(|x| x == n).unwrap_or(usize::MAX))
        }
    };
    // The rows of chips depend on the margin at the left, and the margin on the wires that join the bands: lay out, count them,
    // and lay out again with that margin (it settles in a step or two).
    let margin = off_pins as u16 + 1;
    #[allow(clippy::never_loop)]
    loop {
        // the rows of chips: each block from the margin on, a new row past the board's width
        let mut tiers: Vec<Vec<(usize, u16)>> = vec![vec![]];
        let mut col = margin;
        // with a width to keep to, the tall chips go together (a short chip beside a tall one leaves the rows under it empty), the
        // order among equal heights kept (the chips that share wires stay side by side)
        let mut seq: Vec<usize> = (0..blocks.len()).collect();
        if opt.cols.is_some() {
            seq.sort_by_key(|b| std::cmp::Reverse(blocks[*b].half));
        }
        for b in seq {
            let blk = &blocks[b];
            if let Some(w) = opt.cols
                && !tiers.last().unwrap().is_empty()
                && col + blk.end >= w
            {
                tiers.push(vec![]);
                col = margin;
            }
            tiers.last_mut().unwrap().push((b, col));
            col += blk.end + 1;
        }
        let tier_of: BTreeMap<usize, usize> = tiers.iter().enumerate().flat_map(|(t, v)| v.iter().map(move |(b, _)| (*b, t))).collect();
        // the nets each band needs: the chip pins' wires, the parts standing in their place, the off-board wires
        let bands = tiers.len() + 1;
        let mut band_nets: Vec<BTreeSet<String>> = vec![BTreeSet::new(); bands];
        for (b, blk) in blocks.iter().enumerate() {
            let t = tier_of[&b];
            let pins = &c.on[blk.k].2;
            for (p, (_, n)) in blk.pins.iter().zip(pins) {
                let Some((dir, _)) = p.wire else { continue };
                let band = if dir == Dir::Up { t } else { t + 1 };
                match carry.get(n) {
                    Some(Carry::Via(k)) => {
                        band_nets[band].insert(other_net(*k, n).unwrap());
                    }
                    _ => {
                        band_nets[band].insert(n.clone());
                    }
                }
            }
        }
        for (_, pins) in &c.off {
            for (_, n) in pins.iter().filter(|(_, n)| carry.get(n) == Some(&Carry::Home)) {
                band_nets[0].insert(n.clone());
            }
        }
        // a net that goes on to another section: a strip at the top, where the wire from the other section comes in
        for n in external.iter().filter(|n| carry.get(*n) == Some(&Carry::Home)) {
            band_nets[0].insert(n.clone());
        }
        // the two-lead parts not standing in a pin's place: into a band that has both their nets (the one with the fewest parts),
        // else one of them, else the top band
        let via_parts: BTreeSet<usize> = carry.values().filter_map(|v| if let Carry::Via(k) = v { Some(*k) } else { None }).collect();
        let mut part_band: BTreeMap<usize, usize> = BTreeMap::new();
        let mut per_band = vec![0usize; bands];
        for k in (0..c.on.len()).filter(|k| c.on[*k].1 == Look::TwoLead && !via_parts.contains(k)) {
            let nets = two_nets(k);
            if nets.is_empty() {
                continue;
            }
            let has = |b: usize| nets.iter().filter(|n| band_nets[b].contains(*n)).count();
            let band = (0..bands).max_by_key(|b| (has(*b), std::cmp::Reverse(per_band[*b]), std::cmp::Reverse(*b))).unwrap_or(0);
            for n in &nets {
                band_nets[band].insert(n.clone());
            }
            per_band[band] += 1;
            part_band.insert(k, band);
        }
        // the rows: a band's strips, then a row of chips, in turn
        let mut strip_row: Vec<BTreeMap<String, u16>> = vec![BTreeMap::new(); bands];
        let mut tier_top: Vec<u16> = vec![];
        let mut homes: Vec<String> = vec![];
        let mut r: u16 = 0;
        for b in 0..bands {
            let mut nets: Vec<&String> = band_nets[b].iter().collect();
            nets.sort_by_key(|n| rank(n));
            for n in nets {
                strip_row[b].insert(n.clone(), r);
                homes.resize(r as usize + 1, String::new());
                homes[r as usize] = n.clone();
                r += 1;
            }
            if b < tiers.len() {
                tier_top.push(r);
                r += tiers[b].iter().map(|(blk, _)| blocks[*blk].half).max().unwrap_or(0);
            }
        }
        let chips_end = r;
        // the wires joining a net's strips from band to band (placed once the chips are: in the first column free all the way down)
        let mut bus: Vec<(String, u16, u16)> = vec![];
        for n in &order {
            let rows: Vec<u16> = (0..bands).filter_map(|b| strip_row[b].get(n).copied()).collect();
            for w in rows.windows(2) {
                bus.push((n.clone(), w[0], w[1]));
            }
        }
        // lay it all out
        let mut l = StripLayout::default();
        let mut taken: BTreeSet<At> = BTreeSet::new();
        // holes a wire or a standing part passes over (a wire's end there would lie under it)
        let mut over: BTreeSet<At> = BTreeSet::new();
        let span = |a: u16, b: u16, col: u16, over: &mut BTreeSet<At>| {
            for r in a.min(b) + 1..a.max(b) {
                over.insert((r, col));
            }
        };
        let mut col = 0u16;
        for (refdes, pins) in &c.off {
            let mut ends = vec![];
            for (pin, net) in pins {
                if let Some(&r) = strip_row[0].get(net).filter(|_| carry.get(net) == Some(&Carry::Home)) {
                    ends.push((pin.clone(), net.clone(), (r, col)));
                    taken.insert((r, col));
                    col += 1;
                }
            }
            l.off.push((refdes.clone(), ends));
        }
        // the chips: each pin on a piece of strip of its own, cut off on both sides; its wire to its net's strip in the band
        let mut piece_of_pin: BTreeMap<(String, String), Piece> = BTreeMap::new();
        let mut placed: BTreeMap<usize, Vec<(String, At)>> = BTreeMap::new();
        for (t, tier) in tiers.iter().enumerate() {
            let top = tier_top[t];
            for &(b, start) in tier {
                let blk = &blocks[b];
                let (refdes, _, pins) = &c.on[blk.k];
                let mut leads = vec![];
                for (p, (pin, net)) in blk.pins.iter().zip(pins) {
                    let row = top + p.row;
                    let at = if p.left { (row, start + blk.left_pin) } else { (row, start + blk.right_pin) };
                    let (first, last) = if p.left { (start, start + blk.left_pin) } else { (start + blk.right_pin, start + blk.end - 1) };
                    let wire_col = p.wire.map(|(_, w)| start + w).unwrap_or(at.1);
                    leads.push((pin.clone(), at));
                    taken.insert(at);
                    piece_of_pin.insert((refdes.clone(), pin.clone()), (row, first, last, wire_col));
                    let Some((dir, _)) = p.wire else { continue };
                    let band = if dir == Dir::Up { t } else { t + 1 };
                    // the end on the strip: straight up or down, or the nearest free hole along the strip when a wire from the other
                    // row of chips already ends there (a band between two rows of chips takes wires from both)
                    let strip_end = |r: u16, taken: &BTreeSet<At>| -> At {
                        (0..400u16).flat_map(|d| [wire_col.checked_sub(d), wire_col.checked_add(d)]).flatten().map(|cc| (r, cc)).find(|a| !taken.contains(a)).unwrap_or((r, wire_col))
                    };
                    match carry.get(net) {
                        Some(Carry::Via(k)) => {
                            // the two-lead part stands in the wire's place: from this piece to the strip of its other net
                            let other = other_net(*k, net).unwrap();
                            let e = strip_end(strip_row[band][&other], &taken);
                            let part_leads: Vec<(String, At)> = c.on[*k].2.iter().map(|(pp, n)| (pp.clone(), if n == net { (row, wire_col) } else { e })).collect();
                            for (_, a) in &part_leads {
                                taken.insert(*a);
                            }
                            if e.1 == wire_col {
                                span(row, e.0, wire_col, &mut over);
                            }
                            placed.insert(*k, part_leads);
                        }
                        _ => {
                            let e = strip_end(strip_row[band][net], &taken);
                            l.links.push(Link { a: (row, wire_col), b: e, net: net.clone() });
                            taken.insert((row, wire_col));
                            taken.insert(e);
                            if e.1 == wire_col {
                                span(row, e.0, wire_col, &mut over);
                            }
                        }
                    }
                }
                for k in 0..blk.half {
                    let r = top + k;
                    if start > 0 {
                        l.cuts.insert((r, start - 1));
                    }
                    if blk.look == Look::Dip {
                        l.cuts.insert((r, start + blk.left_pin + 1));
                    }
                    l.cuts.insert((r, start + blk.end));
                }
                l.parts.push(StripPart { refdes: refdes.clone(), look: blk.look, leads, mount: blk.mount.clone() });
            }
        }
        // nets of chip pins only: wires between free holes of the pins' pieces (the shortest set that joins them)
        let free_in = |piece: Piece, taken: &BTreeSet<At>, over: &BTreeSet<At>, cuts: &BTreeSet<At>| -> Option<At> {
            let (r, a, b, w) = piece;
            let mut cols: Vec<u16> = (a..=b).collect();
            cols.sort_by_key(|c| c.abs_diff(w));
            cols.into_iter().map(|c| (r, c)).find(|at| !taken.contains(at) && !over.contains(at) && !cuts.contains(at))
        };
        for n in &order {
            if carry[n] != Carry::Pins {
                continue;
            }
            let pieces: Vec<Piece> = members[n].pins.iter().filter_map(|p| piece_of_pin.get(p).copied()).collect();
            let dist = |a: Piece, b: Piece| (a.0 as f64 - b.0 as f64).hypot(a.3 as f64 - b.3 as f64);
            let mut joined = vec![0usize];
            while joined.len() < pieces.len() {
                let Some((from, to)) = joined
                    .iter()
                    .flat_map(|&j| (0..pieces.len()).filter(|k| !joined.contains(k)).map(move |k| (j, k)))
                    .min_by(|x, y| dist(pieces[x.0], pieces[x.1]).total_cmp(&dist(pieces[y.0], pieces[y.1])))
                else {
                    break;
                };
                joined.push(to);
                if let (Some(a), Some(b)) = (free_in(pieces[from], &taken, &over, &l.cuts), free_in(pieces[to], &taken, &over, &l.cuts)) {
                    taken.insert(a);
                    taken.insert(b);
                    l.links.push(Link { a, b, net: n.clone() });
                }
            }
        }
        // the wires between bands: down a column with nothing in it on the rows between (right of a row of chips that is not full,
        // else past the widest), the short ones first
        bus.sort_by_key(|(_, a, b)| b - a);
        for (n, a, b) in &bus {
            // nothing ends or passes on the rows between, and neither end on a cut (passing over a cut is fine)
            let free = |cc: u16| (*a..=*b).all(|r| !taken.contains(&(r, cc)) && !over.contains(&(r, cc))) && !l.cuts.contains(&(*a, cc)) && !l.cuts.contains(&(*b, cc));
            let in_chips = |cc: u16| l.parts.iter().any(|p| {
                let (lo, hi) = (p.leads.iter().map(|x| x.1 .1).min().unwrap_or(0), p.leads.iter().map(|x| x.1 .1).max().unwrap_or(0));
                let (top, bot) = (p.leads.iter().map(|x| x.1 .0).min().unwrap_or(0), p.leads.iter().map(|x| x.1 .0).max().unwrap_or(0));
                cc >= lo && cc <= hi && top <= *b && bot >= *a
            });
            let cc = (off_pins as u16..).find(|cc| free(*cc) && !in_chips(*cc)).unwrap();
            l.links.push(Link { a: (*a, cc), b: (*b, cc), net: n.clone() });
            taken.insert((*a, cc));
            taken.insert((*b, cc));
            span(*a, *b, cc, &mut over);
        }
        // the two-lead parts: standing across their band's two strips, in the first column free on every row between (a part's
        // body is about a hole wide: none in the columns either side over the same rows); past the board's width when the band is full
        let mut bodies: BTreeSet<At> = BTreeSet::new();
        let mut spare_row = chips_end;
        let mut loose: Vec<(usize, Vec<(String, At)>)> = vec![];
        for (k, (refdes, look, pins)) in c.on.iter().enumerate().filter(|p| p.1 .1 == Look::TwoLead) {
            let mount = mount_of(refdes);
            if let Some(leads) = placed.remove(&k) {
                l.parts.push(StripPart { refdes: refdes.clone(), look: *look, leads, mount });
                continue;
            }
            let Some(&band) = part_band.get(&k) else {
                loose.push((k, vec![]));
                continue;
            };
            let rows: Vec<Option<u16>> = pins.iter().map(|(_, n)| strip_row[band].get(n).copied()).collect();
            let fits = |cols: &[u16], r0: u16, r1: u16| -> bool {
                cols.iter().all(|&cc| {
                    (r0.min(r1)..=r0.max(r1)).all(|r| {
                        let at = (r, cc);
                        !taken.contains(&at) && !over.contains(&at) && !bodies.contains(&at) && !bodies.contains(&(r, cc + 1)) && (cc == 0 || !bodies.contains(&(r, cc - 1)))
                    })
                })
            };
            // near the wires already on its nets' strips (not the supplies' or ground's: they run the whole board), so the strips
            // stay short and can share a row (`share_rows`); else from the left
            let near: Vec<u16> = pins
                .iter()
                .filter(|(_, n)| !is_ground_name(n) && !c.supplies.contains(n))
                .filter_map(|(_, n)| strip_row[band].get(n))
                .flat_map(|r| taken.iter().filter(move |a| a.0 == *r).map(|a| a.1))
                .collect();
            let from = |want: &dyn Fn(u16) -> bool| -> u16 {
                if near.is_empty() {
                    return (margin..).find(|cc| want(*cc)).unwrap();
                }
                let mid = (near.iter().map(|x| *x as u32).sum::<u32>() / near.len() as u32) as u16;
                (0..400u16).flat_map(|d| [mid.checked_sub(d), mid.checked_add(d)]).flatten().filter(|cc| *cc >= margin).find(|cc| want(*cc)).unwrap_or_else(|| (margin..).find(|cc| want(*cc)).unwrap())
            };
            let leads: Vec<(String, At)> = match rows.as_slice() {
                [Some(a), Some(b)] if a != b => {
                    let cc = from(&|cc| fits(&[cc], *a, *b));
                    for r in *a.min(b)..=*a.max(b) {
                        bodies.insert((r, cc));
                    }
                    span(*a, *b, cc, &mut over);
                    vec![(pins[0].0.clone(), (*a, cc)), (pins[1].0.clone(), (*b, cc))]
                }
                // both leads on one net: lying along its strip, two holes apart
                [Some(a), Some(_)] => {
                    let cc = from(&|cc| fits(&[cc, cc + 1, cc + 2], *a, *a));
                    for x in cc..=cc + 2 {
                        bodies.insert((*a, x));
                    }
                    vec![(pins[0].0.clone(), (*a, cc)), (pins[1].0.clone(), (*a, cc + 2))]
                }
                _ => {
                    loose.push((k, vec![]));
                    continue;
                }
            };
            for (_, a) in &leads {
                taken.insert(*a);
            }
            l.parts.push(StripPart { refdes: refdes.clone(), look: *look, leads, mount });
        }
        // a part with a lead on nothing: below everything, a strip of its own for that lead
        let loose_col = l.used().iter().map(|(a, _)| a.1).max().unwrap_or(0) + 2;
        for (i, (k, _)) in loose.into_iter().enumerate() {
            let (refdes, look, pins) = &c.on[k];
            let cc = loose_col + 2 * i as u16;
            let mut leads = vec![];
            for (pin, net) in pins {
                let r = (0..bands).find_map(|b| strip_row[b].get(net).copied()).unwrap_or_else(|| {
                    spare_row += 1;
                    spare_row
                });
                leads.push((pin.clone(), (r, cc)));
            }
            if leads.len() == 2 && leads[0].1 == leads[1].1 {
                leads[1].1 .1 += 1;
            }
            l.parts.push(StripPart { refdes: refdes.clone(), look: *look, leads, mount: mount_of(refdes) });
        }
        // the parts in their schematic's order (the list on the left reads as the schematic does)
        let pos = |r: &str| c.on.iter().position(|x| x.0 == r).unwrap_or(usize::MAX);
        let mut parts = std::mem::take(&mut l.parts);
        parts.sort_by_key(|p| {
            let chip = p.look != Look::TwoLead;
            (!chip, if chip { blocks.iter().position(|b| c.on[b.k].0 == p.refdes).unwrap_or(0) } else { pos(&p.refdes) })
        });
        l.parts = parts;
        l.homes = homes;
        let gone = share_rows(&mut l);
        let used = l.used().into_iter().map(|(a, _)| a).chain(l.cuts.iter().copied()).fold((0u16, 0u16), |m, a| (m.0.max(a.0), m.1.max(a.1)));
        l.rows = used.0.max(spare_row.saturating_sub(gone)) + 1;
        l.cols = used.1 + 1;
        return l;
    }
}

/// An end of something on the board: a part's lead, a link's end (`true`: its `a`), an off-board wire's end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum End {
    Lead(usize, usize),
    Link(usize, bool),
    Off(usize, usize),
}

fn end_at(l: &StripLayout, e: End) -> At {
    match e {
        End::Lead(p, k) => l.parts[p].leads[k].1,
        End::Link(k, true) => l.links[k].a,
        End::Link(k, false) => l.links[k].b,
        End::Off(o, k) => l.off[o].1[k].2,
    }
}

fn set_end(l: &mut StripLayout, e: End, at: At) {
    match e {
        End::Lead(p, k) => l.parts[p].leads[k].1 = at,
        End::Link(k, true) => l.links[k].a = at,
        End::Link(k, false) => l.links[k].b = at,
        End::Off(o, k) => l.off[o].1[k].2 = at,
    }
}

/// Every end on the board.
fn all_ends(l: &StripLayout) -> Vec<End> {
    let mut v = vec![];
    for (p, part) in l.parts.iter().enumerate() {
        v.extend((0..part.leads.len()).map(|k| End::Lead(p, k)));
    }
    for k in 0..l.links.len() {
        v.push(End::Link(k, true));
        v.push(End::Link(k, false));
    }
    for (o, (_, pins)) in l.off.iter().enumerate() {
        v.extend((0..pins.len()).map(|k| End::Off(o, k)));
    }
    v
}

/// The other end of a two-lead part or a link (what lies between the two).
fn other_end(l: &StripLayout, e: End) -> Option<End> {
    match e {
        End::Lead(p, k) if l.parts[p].leads.len() == 2 => Some(End::Lead(p, 1 - k)),
        End::Link(k, a) => Some(End::Link(k, !a)),
        _ => None,
    }
}

/// The holes strictly between two ends in one row or one column (a part's body or a wire lies over them).
fn between(a: At, b: At) -> Vec<At> {
    if a.1 == b.1 {
        (a.0.min(b.0) + 1..a.0.max(b.0)).map(|r| (r, a.1)).collect()
    } else if a.0 == b.0 {
        (a.1.min(b.1) + 1..a.1.max(b.1)).map(|c| (a.0, c)).collect()
    } else {
        vec![]
    }
}

/// The bands of strips: (first row, row past the last).
fn bands_of(homes: &[String]) -> Vec<(u16, u16)> {
    let rows = homes.len() as u16;
    let mut bands = vec![];
    let mut r = 0u16;
    while r < rows {
        if homes[r as usize].is_empty() {
            r += 1;
            continue;
        }
        let st = r;
        while r < rows && !homes[r as usize].is_empty() {
            r += 1;
        }
        bands.push((st, r));
    }
    bands
}

/// Packs each band's strips: a strip whose ends all lie apart (a hole between, to cut) from the ends on a strip above it in the same
/// band moves onto that strip's row, the row cut between the two, when every end can go straight up or down (the hole there free and
/// nothing lying over it, and nothing in the way of a wire or part made longer). The rows left empty go. A band carries the supplies
/// and ground for every chip along the board, but most other nets are used along a short stretch of it: the DMM's B stage had 44 rows
/// of strips in 87. Returns how many rows went.
fn share_rows(l: &mut StripLayout) -> u16 {
    // each shared row's stretches of strip: (first used column, last used column, net)
    let mut segs: BTreeMap<u16, Vec<(u16, u16, String)>> = BTreeMap::new();
    let mut empty: Vec<u16> = vec![];
    let cols_on = |l: &StripLayout, r: u16| -> Vec<(End, u16)> { all_ends(l).into_iter().map(|e| (e, end_at(l, e))).filter(|(_, a)| a.0 == r).map(|(e, a)| (e, a.1)).collect() };
    for (st, en) in bands_of(&l.homes) {
        for m in st + 1..en {
            let ends = cols_on(l, m);
            if ends.is_empty() {
                continue;
            }
            let (lo, hi) = ends.iter().fold((u16::MAX, 0), |a, (_, c)| (a.0.min(*c), a.1.max(*c)));
            for n in (st..m).filter(|n| !empty.contains(n)) {
                let here: Vec<(u16, u16, String)> = segs.get(&n).cloned().unwrap_or_else(|| {
                    let cols: Vec<u16> = cols_on(l, n).into_iter().map(|x| x.1).collect();
                    match (cols.iter().min(), cols.iter().max()) {
                        (Some(a), Some(b)) => vec![(*a, *b, l.homes[n as usize].clone())],
                        _ => vec![],
                    }
                });
                if here.is_empty() || !here.iter().all(|(a, b, _)| *b + 1 < lo || hi + 1 < *a) {
                    continue;
                }
                let ends: Vec<End> = ends.iter().map(|x| x.0).collect();
                if !can_move(l, &ends, n) {
                    continue;
                }
                for e in &ends {
                    let at = end_at(l, *e);
                    set_end(l, *e, (n, at.1));
                }
                let mut v = here;
                v.push((lo, hi, l.homes[m as usize].clone()));
                v.sort();
                segs.insert(n, v);
                empty.push(m);
                break;
            }
        }
    }
    // the cuts between the stretches, and each stretch's name where it starts (the leftmost one's at the board's edge)
    for (n, v) in &segs {
        l.homes[*n as usize] = v[0].2.clone();
        for w in v.windows(2) {
            let cut = (w[0].1 + w[1].0) / 2;
            l.cuts.insert((*n, cut));
            l.names.push(((*n, cut + 1), w[1].2.clone()));
        }
    }
    empty.sort();
    for &m in empty.iter().rev() {
        drop_row(l, m);
    }
    empty.len() as u16
}

/// Whether the ends (all on one row) can move straight up or down to row `n`.
fn can_move(l: &StripLayout, ends: &[End], n: u16) -> bool {
    let all = all_ends(l);
    let used: BTreeSet<At> = all.iter().map(|e| end_at(l, *e)).collect();
    // what lies over each hole: the holes between a part's or a link's two ends, by the end that names it
    let key = |e: End| -> End {
        match other_end(l, e) {
            Some(o) if o < e => o,
            _ => e,
        }
    };
    let mut over: BTreeMap<At, Vec<End>> = BTreeMap::new();
    for e in &all {
        if let Some(o) = other_end(l, *e).filter(|o| e < o) {
            for h in between(end_at(l, *e), end_at(l, o)) {
                over.entry(h).or_default().push(*e);
            }
        }
    }
    for e in ends {
        // a chip's or a row part's lead stays
        if matches!(e, End::Lead(p, _) if l.parts[*p].leads.len() != 2) {
            return false;
        }
        let at = end_at(l, *e);
        let to = (n, at.1);
        if used.contains(&to) || over.get(&to).is_some_and(|v| v.iter().any(|x| *x != key(*e))) {
            return false;
        }
        // a part or a wire made longer: nothing ends under it
        if let Some(o) = other_end(l, *e) {
            let far = if ends.contains(&o) { (n, end_at(l, o).1) } else { end_at(l, o) };
            if far == to || between(to, far).iter().any(|h| used.contains(h) && *h != at) {
                return false;
            }
        }
    }
    true
}

/// Takes the (empty) row `m` off the board, the rows below it up by one.
fn drop_row(l: &mut StripLayout, m: u16) {
    let up = |a: &mut At| {
        if a.0 > m {
            a.0 -= 1;
        }
    };
    for p in &mut l.parts {
        for (_, a) in &mut p.leads {
            up(a);
        }
    }
    for k in &mut l.links {
        up(&mut k.a);
        up(&mut k.b);
    }
    for (_, pins) in &mut l.off {
        for (_, _, a) in pins {
            up(a);
        }
    }
    l.cuts = l.cuts.iter().filter(|a| a.0 != m).map(|a| if a.0 > m { (a.0 - 1, a.1) } else { *a }).collect();
    for (a, _) in &mut l.names {
        up(a);
    }
    if (m as usize) < l.homes.len() {
        l.homes.remove(m as usize);
    }
}

/// The shortest set of joins between groups of points (each next group joined to the nearest one already joined, by its nearest
/// pair of points): the lines of a ratsnest.
pub(crate) fn nearest_joins<P: Copy>(groups: &[Vec<P>], dist: impl Fn(P, P) -> f64) -> Vec<(P, P)> {
    let groups: Vec<&Vec<P>> = groups.iter().filter(|g| !g.is_empty()).collect();
    let mut out = vec![];
    let mut joined = vec![0usize];
    while joined.len() < groups.len() {
        let mut best: Option<(f64, usize, P, P)> = None;
        for &j in &joined {
            for k in (0..groups.len()).filter(|k| !joined.contains(k)) {
                for &a in groups[j].iter() {
                    for &b in groups[k].iter() {
                        let d = dist(a, b);
                        if best.as_ref().is_none_or(|x| d < x.0) {
                            best = Some((d, k, a, b));
                        }
                    }
                }
            }
        }
        let Some((_, k, a, b)) = best else { break };
        joined.push(k);
        out.push((a, b));
    }
    out
}

/// The hole of a pad named "PART.PIN" (a part's lead, or an off-board part's wire).
pub(crate) fn pad_at(l: &StripLayout, pad: &str) -> Option<At> {
    let (r, pin) = pad.rsplit_once('.')?;
    l.parts.iter().filter(|p| p.refdes == r).flat_map(|p| p.leads.iter()).find(|(p, _)| p == pin).map(|(_, a)| *a).or_else(|| {
        l.off.iter().filter(|(x, _)| x == r).flat_map(|(_, v)| v.iter()).find(|(p, _, _)| p == pin).map(|(_, _, a)| *a)
    })
}

/// How far (mm) the ways along the copper from hole `s` to holes `a` and `b` run together before they part: the shortest ways along the
/// strips (2.54 mm a hole) and the wires. `None` when `a` or `b` is not joined to `s`. A strip's current flows only between where it
/// comes in and where it goes out, so a sense wire taken off the strip on the far side of the lead from the current's wire sees the
/// lead's own voltage (0 mm together), and one taken off between them sees the drop along the strip as well.
pub fn strip_fork(l: &StripLayout, s: At, a: At, b: At) -> Option<f64> {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let pieces = l.pieces();
    let piece_of = |at: At| pieces.iter().position(|(r, x, y)| *r == at.0 && at.1 >= *x && at.1 <= *y);
    let mut wires: BTreeMap<At, Vec<(At, u64)>> = BTreeMap::new();
    for (ka, kb) in l.joins() {
        let d = ((ka.0 as f64 - kb.0 as f64).hypot(ka.1 as f64 - kb.1 as f64) * 2540.0) as u64;
        wires.entry(ka).or_default().push((kb, d));
        wires.entry(kb).or_default().push((ka, d));
    }
    // distances in µm, and each hole's hole before it on the way from `s`
    let mut dist: BTreeMap<At, u64> = BTreeMap::new();
    let mut prev: BTreeMap<At, At> = BTreeMap::new();
    let mut heap = BinaryHeap::new();
    dist.insert(s, 0);
    heap.push(Reverse((0u64, s)));
    while let Some(Reverse((d, at))) = heap.pop() {
        if dist.get(&at).is_some_and(|x| *x < d) {
            continue;
        }
        let mut next: Vec<(At, u64)> = wires.get(&at).cloned().unwrap_or_default();
        if let Some(p) = piece_of(at) {
            let (_, x, y) = pieces[p];
            if at.1 > x {
                next.push(((at.0, at.1 - 1), 2540));
            }
            if at.1 < y {
                next.push(((at.0, at.1 + 1), 2540));
            }
        }
        for (to, w) in next {
            if dist.get(&to).is_none_or(|x| d + w < *x) {
                dist.insert(to, d + w);
                prev.insert(to, at);
                heap.push(Reverse((d + w, to)));
            }
        }
    }
    if !dist.contains_key(&a) || !dist.contains_key(&b) {
        return None;
    }
    // where the two ways part: the first hole on the way back from `b` that is on the way back from `a`
    let mut on_a: BTreeSet<At> = BTreeSet::new();
    let mut h = a;
    loop {
        on_a.insert(h);
        match prev.get(&h) {
            Some(p) => h = *p,
            None => break,
        }
    }
    let mut h = b;
    while !on_a.contains(&h) {
        h = prev[&h];
    }
    Some(dist[&h] as f64 / 1000.0)
}

/// The requirements table's forks (Kelvin connections) on the board: (the measure, its three holes, how far the ways run together).
fn forks(s: &Schematic, l: &StripLayout) -> Vec<(design_spec::Measure, [At; 3], Option<f64>)> {
    let mut out = vec![];
    for m in s.requirements.measures.iter().filter(|m| m.what == "fork" && m.pads.len() == 3) {
        let at: Vec<Option<At>> = m.pads.iter().map(|p| pad_at(l, p)).collect();
        let (Some(a), Some(b), Some(c)) = (at[0], at[1], at[2]) else { continue };
        out.push((m.clone(), [a, b, c], strip_fork(l, a, b, c)));
    }
    out
}

/// Keeps the table's forks: when the ways from a fork's first pad run together along a strip (a current and a sense wire both taken
/// off on one side of a resistor's lead), the part with that lead moves along its strips to where they part at the lead (the lead
/// between the two), each lead staying on its piece of strip, on a free hole that nothing lies over, nothing ending under its body.
fn keep_forks(s: &Schematic, l: &mut StripLayout) {
    for (m, [st, _, _], now) in forks(s, l) {
        let Some(now) = now else { continue };
        if m.op == "min" || now <= m.limit + 1e-9 {
            continue;
        }
        let Some(p) = l.parts.iter().position(|p| p.leads.len() == 2 && p.leads.iter().any(|(_, a)| *a == st)) else { continue };
        let old: Vec<At> = l.parts[p].leads.iter().map(|x| x.1).collect();
        let pieces = l.pieces();
        let piece_of = |at: At| pieces.iter().position(|(r, x, y)| *r == at.0 && at.1 >= *x && at.1 <= *y);
        // what else is on the board: the other ends, and the holes their parts and wires lie over
        let mine: BTreeSet<End> = (0..2).map(|k| End::Lead(p, k)).collect();
        let others: Vec<End> = all_ends(l).into_iter().filter(|e| !mine.contains(e)).collect();
        let used: BTreeSet<At> = others.iter().map(|e| end_at(l, *e)).collect();
        let over: BTreeSet<At> = others.iter().filter_map(|e| other_end(l, *e).filter(|o| e < o).map(|o| between(end_at(l, *e), end_at(l, o)))).flatten().collect();
        let mut best: Option<(f64, u16, Vec<At>)> = None;
        for c in 0..l.cols {
            let new: Vec<At> = old.iter().map(|a| (a.0, c)).collect();
            if new == old || new.iter().zip(&old).any(|(n, o)| piece_of(*n).is_none() || piece_of(*n) != piece_of(*o)) {
                continue;
            }
            let body = between(new[0], new[1]);
            if new.iter().any(|a| used.contains(a) || over.contains(a)) || body.iter().any(|a| used.contains(a)) {
                continue;
            }
            let mut trial = l.clone();
            for (k, a) in new.iter().enumerate() {
                trial.parts[p].leads[k].1 = *a;
            }
            let st2 = new[old.iter().position(|a| *a == st).unwrap()];
            let [_, fa, fb] = forks(s, &trial).into_iter().find(|f| f.0 == m).map(|f| f.1).unwrap_or([st2; 3]);
            let Some(v) = strip_fork(&trial, st2, fa, fb) else { continue };
            let key = (v, c.abs_diff(old[0].1));
            if best.as_ref().is_none_or(|b| key < (b.0, b.1)) {
                best = Some((v, key.1, new));
            }
        }
        if let Some((_, _, new)) = best.filter(|b| b.0 < now) {
            for (k, a) in new.into_iter().enumerate() {
                l.parts[p].leads[k].1 = a;
            }
        }
        move_wire_ends(s, l, &m);
    }
}

/// The second way to keep a fork: a wire's end on the strip of the fork's first pad moves along that piece of strip to a free hole
/// (one at a time, the best first), so the wires of the two ways come in on either side of the lead.
fn move_wire_ends(s: &Schematic, l: &mut StripLayout, m: &design_spec::Measure) {
    let value = |l: &StripLayout| forks(s, l).into_iter().find(|f| f.0 == *m).and_then(|f| f.2);
    for _ in 0..2 {
        let Some(now) = value(l) else { return };
        if now <= m.limit + 1e-9 {
            return;
        }
        let Some(st) = pad_at(l, &m.pads[0]) else { return };
        let pieces = l.pieces();
        let Some(&(row, x0, x1)) = pieces.iter().find(|(r, x, y)| *r == st.0 && st.1 >= *x && st.1 <= *y) else { return };
        let all = all_ends(l);
        let used: BTreeSet<At> = all.iter().map(|e| end_at(l, *e)).collect();
        let over: BTreeSet<At> = all.iter().filter_map(|e| other_end(l, *e).filter(|o| e < o).map(|o| between(end_at(l, *e), end_at(l, o)))).flatten().collect();
        let mut best: Option<(f64, u16, End, At)> = None;
        for e in all.iter().filter(|e| matches!(e, End::Link(..))) {
            let at = end_at(l, *e);
            if at.0 != row || at.1 < x0 || at.1 > x1 {
                continue;
            }
            for c in x0..=x1 {
                let to = (row, c);
                if used.contains(&to) || over.contains(&to) {
                    continue;
                }
                let far = end_at(l, other_end(l, *e).unwrap());
                if between(to, far).iter().any(|h| used.contains(h)) {
                    continue;
                }
                let mut trial = l.clone();
                set_end(&mut trial, *e, to);
                let Some(v) = value(&trial) else { continue };
                let key = (v, c.abs_diff(at.1));
                if best.as_ref().is_none_or(|b| key < (b.0, b.1)) {
                    best = Some((v, key.1, *e, to));
                }
            }
        }
        match best.filter(|b| b.0 < now) {
            Some((_, _, e, to)) => set_end(l, e, to),
            None => return,
        }
    }
}

/// Where the layout leaves a net of the schematic apart: for each net on more than one group of joined holes (pieces of strip and
/// the wires between them), the lines that would join its groups, nearest leads first, as on a board's ratsnest.
pub fn strip_unjoined(s: &Schematic, l: &StripLayout) -> Vec<(String, At, At)> {
    let pieces = l.pieces();
    let piece_of = |at: At| pieces.iter().position(|(r, a, b)| *r == at.0 && at.1 >= *a && at.1 <= *b);
    let mut parent: Vec<usize> = (0..pieces.len()).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut i = i;
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for (ja, jb) in l.joins() {
        if let (Some(a), Some(b)) = (piece_of(ja), piece_of(jb)) {
            let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
            parent[ra] = rb;
        }
    }
    let nets = extract(&s.flat());
    let net_of = |r: &str, pin: &str| nets.iter().find(|n| n.pins.iter().any(|p| p.refdes == r && p.pin == pin)).map(|n| n.name.clone()).unwrap_or_default();
    let mut groups_of: BTreeMap<String, BTreeMap<usize, Vec<At>>> = BTreeMap::new();
    let leads = l.parts.iter().flat_map(|p| p.leads.iter().map(move |(pin, at)| (p.refdes.clone(), pin.clone(), *at))).chain(l.off.iter().flat_map(|(r, v)| v.iter().map(move |(pin, _, at)| (r.clone(), pin.clone(), *at))));
    for (r, pin, at) in leads {
        let net = net_of(&r, &pin);
        if net.is_empty() {
            continue;
        }
        let Some(p) = piece_of(at) else { continue };
        let g = find(&mut parent, p);
        groups_of.entry(net).or_default().entry(g).or_default().push(at);
    }
    let mut out = vec![];
    for (net, gs) in groups_of.into_iter().filter(|(_, gs)| gs.len() > 1) {
        let gs: Vec<Vec<At>> = gs.into_values().collect();
        for (a, b) in nearest_joins(&gs, |a: At, b: At| (a.0 as f64 - b.0 as f64).hypot(a.1 as f64 - b.1 as f64)) {
            out.push((net.clone(), a, b));
        }
    }
    out
}

/// What the layout joins that the schematic does not, or keeps apart that it joins: one sentence each (empty when it matches).
pub fn strip_check(s: &Schematic, l: &StripLayout) -> Vec<String> {
    let mut problems = vec![];
    // two things in one hole, or a lead in a cut
    let mut seen: BTreeMap<At, String> = BTreeMap::new();
    for (at, who) in l.used() {
        if l.cuts.contains(&at) {
            problems.push(format!("{who} が切った穴 {} にあります", hole_name(at)));
        }
        if let Some(other) = seen.insert(at, who.clone())
            && !(other.starts_with("link") && who.starts_with("link") && other == who)
        {
            problems.push(format!("{} に {other} と {who} の 2 つが入っています", hole_name(at)));
        }
    }
    // the groups of joined holes: pieces of strip, links, the off-board wires of one net
    let pieces = l.pieces();
    let piece_of = |at: At| pieces.iter().position(|(r, a, b)| *r == at.0 && at.1 >= *a && at.1 <= *b);
    let mut parent: Vec<usize> = (0..pieces.len()).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut i = i;
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for (ja, jb) in l.joins() {
        if let (Some(a), Some(b)) = (piece_of(ja), piece_of(jb)) {
            let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
            parent[ra] = rb;
        }
    }
    let nets = extract(&s.flat());
    let net_of = |r: &str, pin: &str| nets.iter().find(|n| n.pins.iter().any(|p| p.refdes == r && p.pin == pin)).map(|n| n.name.clone()).unwrap_or_default();
    let mut group_nets: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
    let mut net_groups: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    let leads = l.parts.iter().flat_map(|p| p.leads.iter().map(move |(pin, at)| (p.refdes.clone(), pin.clone(), *at))).chain(l.off.iter().flat_map(|(r, v)| v.iter().map(move |(pin, _, at)| (r.clone(), pin.clone(), *at))));
    for (r, pin, at) in leads {
        let net = net_of(&r, &pin);
        if net.is_empty() {
            continue;
        }
        let Some(p) = piece_of(at) else {
            problems.push(format!("{r}.{pin} の穴 {} が帯の上にありません", hole_name(at)));
            continue;
        };
        let g = find(&mut parent, p);
        group_nets.entry(g).or_default().insert(net.clone());
        net_groups.entry(net).or_default().insert(g);
    }
    for nets in group_nets.values().filter(|n| n.len() > 1) {
        problems.push(format!("ショート: {} がつながっています", nets.iter().cloned().collect::<Vec<_>>().join(" と ")));
    }
    for (net, groups) in net_groups.iter().filter(|(_, g)| g.len() > 1) {
        problems.push(format!("{net} が {} か所に分かれています (つなぎ忘れ)", groups.len()));
    }
    problems
}

/// How a rule of the requirements table fares on the board: its line, and whether it holds.
#[derive(Clone, Debug, PartialEq)]
pub struct RuleCheck {
    pub ok: bool,
    pub message: String,
}

/// The requirements table's rules that a board built by hand can keep: the distances to keep (`KeepApart`: the closest two holes of
/// the two sets, 2.54 mm a hole) and, with sections, the one ground point (every section's ground goes back to the first section's
/// ground strip by a wire of its own).
pub fn strip_rules(s: &Schematic, l: &StripLayout) -> Vec<RuleCheck> {
    let mut out = vec![];
    let nets = extract(&s.flat());
    let net_of = |r: &str, pin: &str| nets.iter().find(|n| n.pins.iter().any(|p| p.refdes == r && p.pin == pin)).map(|n| n.name.clone()).unwrap_or_default();
    // the holes of an item: a part's leads, or everything on a net (leads, the ends of its wires)
    let holes_of = |items: &[String]| -> Vec<At> {
        let mut v = vec![];
        for item in items {
            if let Some(p) = l.parts.iter().find(|p| &p.refdes == item) {
                v.extend(p.leads.iter().map(|(_, a)| *a));
                continue;
            }
            for p in &l.parts {
                v.extend(p.leads.iter().filter(|(pin, _)| design_spec::matches_name(item, &net_of(&p.refdes, pin))).map(|(_, a)| *a));
            }
            for k in l.links.iter().filter(|k| design_spec::matches_name(item, &k.net)) {
                v.push(k.a);
                v.push(k.b);
            }
        }
        v
    };
    for rule in &s.requirements.apart {
        let (a, b) = (holes_of(&rule.a), holes_of(&rule.b));
        let closest = a.iter().flat_map(|x| b.iter().map(move |y| (x.0 as f64 - y.0 as f64).hypot(x.1 as f64 - y.1 as f64) * 2.54)).filter(|d| *d > 0.0).fold(f64::INFINITY, f64::min);
        if !closest.is_finite() {
            continue;
        }
        let ok = closest >= rule.min_dist;
        let name = if rule.name.is_empty() { format!("{} と {}", rule.a.join(","), rule.b.join(",")) } else { rule.name.clone() };
        out.push(RuleCheck {
            ok,
            message: if ok {
                format!("離す「{name}」: いちばん近い所で {closest:.1} mm ({:.1} mm 以上)", rule.min_dist)
            } else {
                format!("離す「{name}」: いちばん近い所が {closest:.1} mm で、{:.1} mm に足りません。部品か線を、四角で囲んで離してください", rule.min_dist)
            },
        });
    }
    for (m, _, v) in forks(s, l) {
        let name = if m.name.is_empty() { m.pads.join(",") } else { m.name.clone() };
        let (start, a, b) = (&m.pads[0], &m.pads[1], &m.pads[2]);
        let Some(v) = v else {
            out.push(RuleCheck { ok: false, message: format!("ケルビン「{name}」: {start} から {a} か {b} へ、帯と線でつながっていません") });
            continue;
        };
        let ok = if m.op == "min" { v >= m.limit } else { v <= m.limit + 1e-9 };
        out.push(RuleCheck {
            ok,
            message: if ok {
                format!("ケルビン「{name}」: {a} と {b} への道は {start} の足から {v:.1} mm で分かれます ({:.1} mm 以下)", m.limit)
            } else {
                format!("ケルビン「{name}」: {a} と {b} への道が {start} の足から {v:.1} mm 一緒に走り、{:.1} mm を超えます。{start} の足を、帯の上で 2 本の線の間に置いてください (片方の電流が流れる帯から、もう片方の線を取らない)", m.limit)
            },
        });
    }
    if l.sections.len() > 1 {
        let inside = |k: usize, a: At| {
            let (_, p, q) = &l.sections[k];
            a.0 >= p.0 && a.0 <= q.0 && a.1 >= p.1 && a.1 <= q.1
        };
        // a section's ground: its ground strip (named where it starts), or on a perfboard (no strips) a lead on ground
        let ground_leads: Vec<At> = l.parts.iter().flat_map(|p| p.leads.iter().filter(|(pin, _)| is_ground_name(&net_of(&p.refdes, pin))).map(|(_, a)| *a)).chain(l.off.iter().flat_map(|(_, v)| v.iter().filter(|w| is_ground_name(&w.1)).map(|w| w.2))).collect();
        let ground_in = |k: usize| l.names.iter().any(|(a, n)| is_ground_name(n) && inside(k, *a)) || (l.perf && ground_leads.iter().any(|a| inside(k, *a)));
        let hub = (0..l.sections.len()).find(|k| ground_in(*k));
        if let Some(h) = hub {
            for k in (0..l.sections.len()).filter(|k| *k != h) {
                let has_ground = ground_in(k);
                if !has_ground {
                    continue;
                }
                let back = l.links.iter().filter(|w| is_ground_name(&w.net) && ((inside(h, w.a) && inside(k, w.b)) || (inside(h, w.b) && inside(k, w.a)))).count();
                let other = l.links.iter().filter(|w| is_ground_name(&w.net) && (inside(k, w.a) != inside(k, w.b)) && !(inside(h, w.a) || inside(h, w.b))).count();
                let ok = back == 1 && other == 0;
                out.push(RuleCheck {
                    ok,
                    message: if ok {
                        format!("GND: 「{}」の GND は、1 本の線で「{}」の GND の帯 (1 点) へ戻ります", l.sections[k].0, l.sections[h].0)
                    } else {
                        format!("GND: 「{}」の GND から「{}」の GND への線が {back} 本、ほかの区画への線が {other} 本あります。1 点へ 1 本で戻すと、ほかの区画の電流が GND を通りません", l.sections[k].0, l.sections[h].0)
                    },
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn examples() -> Vec<(String, Schematic)> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let mut v: Vec<(String, Schematic)> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "schjson"))
            .filter_map(|e| Some((e.file_name().to_string_lossy().into_owned(), serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok()?)))
            .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }

    /// Every sample laid out on a stripboard joins what its schematic joins and nothing more, with no two things in one hole.
    #[test]
    fn every_sample_lays_out_on_a_stripboard_as_drawn() {
        let mut bad = vec![];
        let mut n = 0;
        for (name, s) in examples() {
            n += 1;
            // one row of chips, and wrapped to a board 40 holes wide (about 10 cm)
            for cols in [None, Some(40)] {
                let l = strip_layout_with(&s, StripOptions { cols });
                let p = strip_check(&s, &l);
                if !p.is_empty() {
                    bad.push(format!("{name} ({cols:?}): {}", p.iter().take(3).cloned().collect::<Vec<_>>().join(" | ")));
                }
            }
        }
        assert!(n > 50, "{n}");
        assert!(bad.is_empty(), "{} of {n}:\n{}", bad.len(), bad.join("\n"));
    }

    #[test]
    fn a_chip_sits_on_cut_pieces_joined_to_its_nets() {
        let s: Schematic = serde_json::from_str(include_str!("../../../docs/examples/ne555_blinker.schjson")).unwrap();
        let l = strip_layout(&s);
        let chip = l.parts.iter().find(|p| p.look == Look::Dip).expect("the 555");
        assert_eq!(chip.leads.len(), 8);
        // pin 1 top left, pin 8 top right, three columns apart
        let at = |pin: &str| chip.leads.iter().find(|(p, _)| p == pin).unwrap().1;
        assert_eq!(at("1").0, at("8").0);
        assert_eq!(at("8").1 - at("1").1, 3);
        assert!(l.cuts.contains(&(at("1").0, at("1").1 + 1)), "cut between the chip's columns");
        assert!(strip_check(&s, &l).is_empty());
        // the check finds a link left out (a pin cut off from its net) and a link between two home strips (a short)
        let mut open = l.clone();
        let gone = open.links.remove(0);
        assert!(strip_check(&s, &open).iter().any(|p| p.contains("分かれています")), "{:?}", strip_check(&s, &open));
        // the ratsnest: one line for that net, none on the board as laid out
        assert!(strip_unjoined(&s, &l).is_empty());
        let rats = strip_unjoined(&s, &open);
        assert_eq!(rats.len(), 1, "{rats:?}");
        assert_eq!(rats[0].0, gone.net);
        let mut short = l.clone();
        short.links.push(Link { a: (0, l.cols - 1), b: (1, l.cols - 1), net: String::new() });
        assert!(strip_check(&s, &short).iter().any(|p| p.starts_with("ショート")), "{:?}", strip_check(&s, &short));
        assert_eq!(hole_name((2, 11)), "C12");
        assert_eq!(hole_name((27, 0)), "AB1");
    }
}

#[cfg(test)]
mod mount_tests {
    use super::*;

    fn sample(f: &str) -> Schematic {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../docs/examples/{f}.schjson"));
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    /// A surface-mount chip sits on an adapter board: its pins where the adapter's pins are (by number, also when the symbol leaves
    /// some out), the rows as far apart as the adapter's.
    #[test]
    fn chips_on_adapters_sit_by_pin_number() {
        let at = |l: &StripLayout, r: &str, pin: &str| l.parts.iter().find(|p| p.refdes == r).unwrap().leads.iter().find(|(p, _)| p == pin).unwrap().1;
        // QFN-16 with 12 pins drawn (no 3, 4, 5, 16): a DIP-16 adapter, pin 15 across from pin 2, pin 13 a row below the last pin drawn on the left (2)
        let s = sample("tp5100_charger");
        let l = strip_layout(&s);
        let u1 = l.parts.iter().find(|p| p.refdes == "U1").unwrap();
        assert_eq!(u1.mount, Mount::Adapter { package: "QFN-16".into(), pins: 16, width: 3 });
        assert_eq!(u1.look, Look::Dip);
        assert_eq!(at(&l, "U1", "15").0, at(&l, "U1", "2").0);
        assert_eq!(at(&l, "U1", "13").0, at(&l, "U1", "2").0 + 2);
        assert_eq!(at(&l, "U1", "15").1 - at(&l, "U1", "2").1, 3);
        assert!(strip_check(&s, &l).is_empty(), "{:?}", strip_check(&s, &l));
        // TQFP-32 on a 0.6 inch adapter
        let l = strip_layout(&sample("arduino_minimal"));
        assert_eq!((at(&l, "U1", "1").0, at(&l, "U1", "32").1 - at(&l, "U1", "1").1), (at(&l, "U1", "32").0, 6));
        // a SOT-223 regulator on a one-row adapter
        let l = strip_layout(&sample("ldo_compare"));
        let u1 = l.parts.iter().find(|p| p.refdes == "U1").unwrap();
        assert_eq!(u1.look, Look::Row);
        assert!(matches!(u1.mount, Mount::Adapter { width: 0, .. }));
    }
}

#[cfg(test)]
mod size_tests {
    /// The parts' leads stand in for wires (a decoder's resistor from its pin's piece to the display's strip), nets of chip pins only
    /// or of one lead get no strip, and only the pins going up to a strip take a column: the 7-segment counter took 100 x 37 holes with
    /// a strip and a column for everything, then 78 x 21. With bands of strips above and below the chips (a pin's wire up or down,
    /// a wire up and a wire down sharing a column), the two-lead parts standing in the bands' free columns and the wires between bands
    /// in free columns: 60 x 24; the traffic light 36 x 19 → 19 x 20, the 555 blinker 25 x 12 → 13 x 13; wrapped to 40 holes 35 x 34
    /// (2026-11-19). The two-lead parts standing near their nets' wires and the strips sharing rows (`share_rows`): 60 x 22, 19 x 19,
    /// 13 x 11, wrapped to 40 holes 35 x 30; the DMM's B stage at 59 holes 60 x 87 → 60 x 78 (2026-11-20).
    #[test]
    fn leads_as_wires_keep_the_board_small() {
        let size = |f: &str, cols: Option<u16>| {
            let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../docs/examples/{f}.schjson"));
            let s: crate::Schematic = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
            let l = super::strip_layout_with(&s, super::StripOptions { cols });
            (l.cols, l.rows)
        };
        let (cols, rows) = size("seg7_counter", None);
        assert!(rows <= 22 && cols <= 60, "{cols} x {rows}");
        let (cols, rows) = size("traffic_light", None);
        assert!(cols <= 19 && rows <= 19, "{cols} x {rows}");
        let (cols, rows) = size("ne555_blinker", None);
        assert!(cols <= 13 && rows <= 11, "{cols} x {rows}");
        // wrapped to 40 holes: the chips go in rows, the board as wide as asked
        let (cols, rows) = size("seg7_counter", Some(40));
        assert!(cols <= 40 && rows <= 30, "{cols} x {rows}");
    }
}


#[cfg(test)]
mod section_tests {
    use super::*;

    /// Two sections from the requirements table: each laid out by itself side by side, every strip cut between them, joined as the
    /// schematic joins them; ground goes back from the second section to the first's ground strip by a wire of its own.
    #[test]
    fn sections_are_cut_apart_and_ground_meets_at_one_point() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/seg7_counter.schjson");
        let mut s: Schematic = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        let group = |name: &str, parts: &[&str]| design_spec::PlaceGroup { name: name.into(), parts: parts.iter().map(|p| p.to_string()).collect(), section: true, ..Default::default() };
        s.requirements.groups = vec![group("カウンタ", &["U1", "U3"]), group("表示", &["U2", "DS1", "R1", "R2", "R3", "R4", "R5", "R6", "R7"])];
        for cols in [None, Some(40)] {
            let l = strip_layout_with(&s, StripOptions { cols });
            assert!(strip_check(&s, &l).is_empty(), "{cols:?}: {:?}", strip_check(&s, &l));
            assert_eq!(l.sections.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["カウンタ", "表示"]);
            let inside = |k: usize, a: At| {
                let (_, p, q) = &l.sections[k];
                a.0 >= p.0 && a.0 <= q.0 && a.1 >= p.1 && a.1 <= q.1
            };
            // every part inside its own section
            for p in &l.parts {
                let k = if ["U1", "U3"].contains(&p.refdes.as_str()) { 0 } else { 1 };
                assert!(p.leads.iter().all(|(_, a)| inside(k, *a)), "{} outside {}", p.refdes, l.sections[k].0);
            }
            // the ground wires between sections all end on the first section's ground strip
            let gnd: Vec<&Link> = l.links.iter().filter(|k| is_ground_name(&k.net) && (inside(0, k.a) != inside(0, k.b))).collect();
            assert_eq!(gnd.len(), 1, "{cols:?}: one ground wire back to the first section");
            // side by side: every row cut between them
            let (_, _, q0) = l.sections[0];
            let (_, p1, _) = l.sections[1];
            if p1.0 == l.sections[0].1 .0 {
                assert!((p1.0..=q0.0).all(|r| l.cuts.contains(&(r, q0.1 + 1))), "the strips cut between the sections");
            }
        }
        // the one ground point holds; a distance to keep between the two sections' chips is measured
        s.requirements.apart = vec![design_spec::KeepApart { name: "離す".into(), a: vec!["U1".into()], b: vec!["DS1".into()], min_dist: 500.0, ..Default::default() }];
        let l = strip_layout(&s);
        let rules = strip_rules(&s, &l);
        assert!(rules.iter().any(|r| r.ok && r.message.starts_with("GND")), "{rules:?}");
        assert!(rules.iter().any(|r| !r.ok && r.message.contains("離す")), "500 mm is not kept: {rules:?}");
        // without sections marked the board is laid out as one
        s.requirements.groups.iter_mut().for_each(|g| g.section = false);
        assert!(strip_layout(&s).sections.is_empty());
    }
}

#[cfg(test)]
mod share_tests {
    use super::*;

    /// Two nets used along different stretches of a band share one row, cut between them and named where the second starts; the
    /// sections find a net on its stretch of a shared row.
    #[test]
    fn nets_apart_along_a_band_share_a_row() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/seg7_counter.schjson");
        let s: Schematic = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        let l = strip_layout(&s);
        assert!(strip_check(&s, &l).is_empty(), "{:?}", strip_check(&s, &l));
        let ((row, col), name) = l.names.first().cloned().expect("a shared row");
        assert!(l.cuts.contains(&(row, col - 1)), "cut where the second net starts");
        assert_ne!(l.homes[row as usize], name);
        let (r, a, _) = strip_of(&l, &name).unwrap();
        assert_eq!((r, a), (row, col));
        let (r, a, _) = strip_of(&l, &l.homes[row as usize]).unwrap();
        assert_eq!((r, a), (row, 0));
        // every end on that row lies on the stretch of its own net
        let nets = extract(&s.flat());
        let net_of = |r: &str, pin: &str| nets.iter().find(|n| n.pins.iter().any(|p| p.refdes == r && p.pin == pin)).map(|n| n.name.clone()).unwrap_or_default();
        for p in &l.parts {
            for (pin, at) in p.leads.iter().filter(|x| x.1 .0 == row) {
                assert_eq!(at.1 >= col, net_of(&p.refdes, pin) == name, "{}.{pin}", p.refdes);
            }
        }
    }
}


#[cfg(test)]
mod fork_tests {
    use super::*;

    fn load(name: &str) -> Schematic {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// A Kelvin connection from the requirements table (a fork: the pad, and the two pads its ways go to): on the active filter's
    /// strips, the ways from R3.2 to R4.1 and to C3.1 share 15 mm of strip; told to keep them apart, the layout moves the part or a
    /// wire's end so the two part at the lead, still joined as the schematic joins them, and the rules say so. One it cannot keep
    /// (R4.1's, both ways going off the same side) is told with how far they run together.
    #[test]
    fn a_fork_in_the_table_parts_at_the_lead() {
        let mut s = load("active_filter.schjson");
        let pads = ["R3.2", "R4.1", "C3.1"];
        let at = |l: &StripLayout| pads.map(|p| pad_at(l, p).unwrap());
        let l = strip_layout(&s);
        let [a, b, c] = at(&l);
        assert!(strip_fork(&l, a, b, c).unwrap() > 10.0, "without the rule the ways share the strip");
        s.requirements.measures.push(design_spec::Measure { name: "検出".into(), what: "fork".into(), pads: pads.map(String::from).to_vec(), op: "max".into(), limit: 0.5, ..Default::default() });
        let l = strip_layout(&s);
        let [a, b, c] = at(&l);
        assert_eq!(strip_fork(&l, a, b, c), Some(0.0));
        assert!(strip_check(&s, &l).is_empty(), "{:?}", strip_check(&s, &l));
        let rules = strip_rules(&s, &l);
        assert!(rules.iter().any(|r| r.ok && r.message.starts_with("ケルビン「検出」")), "{rules:?}");
        s.requirements.measures[0].pads = ["R4.1", "R3.2", "C3.1"].map(String::from).to_vec();
        let l = strip_layout(&s);
        let rules = strip_rules(&s, &l);
        assert!(rules.iter().any(|r| !r.ok && r.message.contains("一緒に走り")), "{rules:?}");
        assert!(strip_check(&s, &l).is_empty());
    }

    /// The current sensors' sample keeps both of its shunt's Kelvin connections on a stripboard and on a perfboard.
    #[test]
    fn the_shunt_sample_keeps_its_kelvin_connections() {
        let s = load("current_sensors.schjson");
        for l in [strip_layout(&s), strip_layout_with(&s, StripOptions { cols: Some(20), ..Default::default() })] {
            let kelvin: Vec<RuleCheck> = strip_rules(&s, &l).into_iter().filter(|r| r.message.starts_with("ケルビン")).collect();
            assert_eq!(kelvin.len(), 2);
            assert!(kelvin.iter().all(|r| r.ok), "{kelvin:?}");
        }
    }
}
