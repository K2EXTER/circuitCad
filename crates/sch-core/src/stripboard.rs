//! A stripboard (and the same layout on a plain perfboard) for building the schematic by hand (B5). The copper strips run along the
//! rows; a strip is broken by cutting it at a hole.
//!
//! The layout: every net gets a strip of its own at the top ("home" strips: the supplies first, ground last). A two-lead part stands
//! across the board between the home strips of its two nets, a column apart from the next. A chip or a part with its leads in a row goes below the home strips:
//! each of its pins sits on a piece of strip of its own (cut off on both sides), and a wire link goes from that piece up to the pin's
//! net's home strip, each in a column of its own so no two links overlap. The supplies off the board come in by wires at the left.
//! On a perfboard (no strips) the same layout holds with a bare wire along each piece of strip from its first used hole to its last.

use crate::breadboard::{is_ground_name, read, Look};
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
}

impl StripLayout {
    /// The pieces of strip: (row, first column, last column), each between two cuts (or the board's ends).
    pub fn pieces(&self) -> Vec<(u16, u16, u16)> {
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

/// Lays the schematic out on a stripboard.
pub fn strip_layout(s: &Schematic) -> StripLayout {
    let c = read(s);
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
    let other_net = |k: usize, n: &str| c.on[k].2.iter().map(|(_, m)| m.clone()).find(|m| m != n && !m.is_empty());
    // how each net is carried: a home strip unless it can do without
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
    // a part can be the wire of one net only; the net at its other end keeps (or gets) a home strip
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
    // the home strips: the supplies first, ground last
    let mut homes: Vec<String> = c.supplies.iter().filter(|n| carry.get(*n) == Some(&Carry::Home)).cloned().collect();
    for n in &order {
        if carry[n] == Carry::Home && !homes.contains(n) && !is_ground_name(n) {
            homes.push(n.clone());
        }
    }
    if let Some(g) = order.iter().find(|n| is_ground_name(n)) {
        homes.push(g.clone());
    }
    let row_of = |n: &str| homes.iter().position(|h| h == n).map(|r| r as u16);
    let mut l = StripLayout { homes: homes.clone(), ..Default::default() };
    let mut taken: BTreeSet<At> = BTreeSet::new();
    // the supplies off the board: a wire into the home strip of each pin, one column each, from the left
    let mut col: u16 = 0;
    for (refdes, pins) in &c.off {
        let mut ends = vec![];
        for (pin, net) in pins {
            if let Some(r) = row_of(net) {
                ends.push((pin.clone(), net.clone(), (r, col)));
                taken.insert((r, col));
                col += 1;
            }
        }
        l.off.push((refdes.clone(), ends));
    }
    col += 1;
    // chips and row parts below the home strips, one block each; every pin on a piece of strip of its own
    let top = homes.len() as u16 + 1;
    let mut bottom = top;
    let mut piece_of_pin: BTreeMap<(String, String), Piece> = BTreeMap::new();
    let mut placed: BTreeMap<usize, Vec<(String, At)>> = BTreeMap::new();
    for (refdes, look, pins) in chip_order(&c.on, &carry).into_iter().map(|k| &c.on[k]) {
        let mut leads = vec![];
        let (half, two_cols) = if *look == Look::Dip { (pins.len() / 2, true) } else { (pins.len(), false) };
        // two spare holes at each piece's outer end when a pin is wired to other chip pins (two wire ends may share a piece)
        let spare = if pins.iter().any(|(_, n)| carry.get(n) == Some(&Carry::Pins)) { 2 } else { 0 };
        // each pin's row and side: a chip's first half down the left column, the rest up the right one (pin 1 top left)
        let side_row = |k: usize| if k < half { (top + k as u16, true) } else { (top + (pins.len() - 1 - k) as u16, false) };
        // a column for the wire (or the standing part) only for the pins that go up to a home strip, in the order of their rows
        let goes_up = |n: &str| matches!(carry.get(n), Some(Carry::Home | Carry::Via(_)));
        let up_rank = |left: bool, row: u16| -> u16 { pins.iter().enumerate().filter(|(k, (_, n))| side_row(*k).1 == left && side_row(*k).0 < row && goes_up(n)).count() as u16 };
        let ups = |left: bool| pins.iter().enumerate().filter(|(k, (_, n))| side_row(*k).1 == left && goes_up(n)).count() as u16;
        let x = col + spare;
        let left_pin = x + ups(true);
        let right_pin = left_pin + 3;
        let end = if two_cols { right_pin + ups(false) + 1 + spare } else { left_pin + 1 };
        for (k, (pin, net)) in pins.iter().enumerate() {
            let (row, left) = side_row(k);
            let (at, wire, first, last) = if left { ((row, left_pin), x + up_rank(true, row), col, left_pin) } else { ((row, right_pin), right_pin + 1 + up_rank(false, row), right_pin, end - 1) };
            leads.push((pin.clone(), at));
            taken.insert(at);
            piece_of_pin.insert((refdes.clone(), pin.clone()), (row, first, last, wire));
            match carry.get(net) {
                Some(Carry::Home) => {
                    let r = row_of(net).unwrap();
                    l.links.push(Link { a: (row, wire), b: (r, wire), net: net.clone() });
                    taken.insert((row, wire));
                    taken.insert((r, wire));
                }
                Some(Carry::Via(k)) => {
                    // the two-lead part stands in the wire's place: from this piece up to the home strip of its other net
                    let other = other_net(*k, net).unwrap();
                    let r = row_of(&other).unwrap();
                    let part_leads: Vec<(String, At)> = c.on[*k].2.iter().map(|(p, n)| (p.clone(), if n == net { (row, wire) } else { (r, wire) })).collect();
                    for (_, a) in &part_leads {
                        taken.insert(*a);
                    }
                    placed.insert(*k, part_leads);
                }
                _ => {}
            }
        }
        for k in 0..half as u16 {
            let r = top + k;
            if col > 0 {
                l.cuts.insert((r, col - 1));
            }
            if two_cols {
                l.cuts.insert((r, left_pin + 1));
            }
            l.cuts.insert((r, end));
        }
        bottom = bottom.max(top + half as u16);
        l.parts.push(StripPart { refdes: refdes.clone(), look: *look, leads });
        // the next block starts right after this one's last cut, which also cuts the next block's rows off from this one
        col = end + 1;
    }
    // nets of chip pins only: a wire from each pin's piece to the one before, at free holes of the pieces
    let free_in = |piece: Piece, taken: &BTreeSet<At>, cuts: &BTreeSet<At>| -> Option<At> {
        let (r, a, b, w) = piece;
        let mut cols: Vec<u16> = (a..=b).collect();
        cols.sort_by_key(|c| c.abs_diff(w));
        cols.into_iter().map(|c| (r, c)).find(|at| !taken.contains(at) && !cuts.contains(at))
    };
    for n in &order {
        if carry[n] != Carry::Pins {
            continue;
        }
        // the shortest set of wires that joins the pins (each next pin wired to the nearest one already joined)
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
            if let (Some(a), Some(b)) = (free_in(pieces[from], &taken, &l.cuts), free_in(pieces[to], &taken, &l.cuts)) {
                taken.insert(a);
                taken.insert(b);
                l.links.push(Link { a, b, net: n.clone() });
            }
        }
    }
    // the other two-lead parts: standing between the home strips of their nets, a column apart
    let mut spare = bottom + 1;
    for (k, (refdes, look, pins)) in c.on.iter().enumerate().filter(|p| p.1 .1 == Look::TwoLead) {
        if let Some(leads) = placed.remove(&k) {
            l.parts.push(StripPart { refdes: refdes.clone(), look: *look, leads });
            continue;
        }
        let mut leads = vec![];
        for (pin, net) in pins {
            let r = row_of(net).unwrap_or_else(|| {
                // a lead on nothing: a strip of its own below everything
                spare += 1;
                spare - 1
            });
            leads.push((pin.clone(), (r, col)));
        }
        // both leads on one net: lying along its strip, two holes apart
        if leads.len() == 2 && leads[0].1 == leads[1].1 {
            leads[1].1 .1 += 2;
            col += 2;
        }
        l.parts.push(StripPart { refdes: refdes.clone(), look: *look, leads });
        // a column between two standing parts: a resistor's body is about a hole wide
        col += 2;
    }
    l.rows = spare.max(bottom) + 1;
    l.cols = col + 1;
    l
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
    for link in &l.links {
        if let (Some(a), Some(b)) = (piece_of(link.a), piece_of(link.b)) {
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
            let l = strip_layout(&s);
            n += 1;
            let p = strip_check(&s, &l);
            if !p.is_empty() {
                bad.push(format!("{name}: {}", p.iter().take(3).cloned().collect::<Vec<_>>().join(" | ")));
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
        open.links.remove(0);
        assert!(strip_check(&s, &open).iter().any(|p| p.contains("分かれています")), "{:?}", strip_check(&s, &open));
        let mut short = l.clone();
        short.links.push(Link { a: (0, l.cols - 1), b: (1, l.cols - 1), net: String::new() });
        assert!(strip_check(&s, &short).iter().any(|p| p.starts_with("ショート")), "{:?}", strip_check(&s, &short));
        assert_eq!(hole_name((2, 11)), "C12");
        assert_eq!(hole_name((27, 0)), "AB1");
    }
}

#[cfg(test)]
mod size_tests {
    /// The parts' leads stand in for wires (a decoder's resistor from its pin's piece to the display's strip), nets of chip pins only
    /// or of one lead get no strip, and only the pins going up to a strip take a column: the 7-segment counter took 100 x 37 holes with
    /// a strip and a column for everything, and now 82 x 21.
    #[test]
    fn leads_as_wires_keep_the_board_small() {
        let size = |f: &str| {
            let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../docs/examples/{f}.schjson"));
            let s: crate::Schematic = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
            let l = super::strip_layout(&s);
            (l.cols, l.rows, l.homes.len())
        };
        let (cols, rows, homes) = size("seg7_counter");
        assert!(rows <= 22 && homes <= 10 && cols <= 85, "{cols} x {rows}, {homes} home strips");
        let (cols, rows, _) = size("traffic_light");
        assert!((cols as u32) * (rows as u32) < 58 * 30 / 2, "{cols} x {rows}");
    }
}
