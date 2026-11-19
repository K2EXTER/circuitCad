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

/// Lays the schematic out on a stripboard.
pub fn strip_layout(s: &Schematic) -> StripLayout {
    let c = read(s);
    // the home strips: the supplies, the other nets in the order the parts use them, ground last
    let mut homes: Vec<String> = c.supplies.clone();
    let push = |n: &str, homes: &mut Vec<String>| {
        if !n.is_empty() && !is_ground_name(n) && !homes.iter().any(|h| h == n) {
            homes.push(n.to_string());
        }
    };
    for (_, _, pins) in &c.on {
        for (_, n) in pins {
            push(n, &mut homes);
        }
    }
    for (_, pins) in &c.off {
        for (_, n) in pins {
            push(n, &mut homes);
        }
    }
    if let Some(g) = &c.ground {
        homes.push(g.clone());
    } else if c.on.iter().flat_map(|p| &p.2).chain(c.off.iter().flat_map(|p| &p.1)).any(|(_, n)| is_ground_name(n)) {
        homes.push("GND".into());
    }
    let row_of = |n: &str| homes.iter().position(|h| h == n || (is_ground_name(h) && is_ground_name(n))).map(|r| r as u16);
    let mut l = StripLayout { homes: homes.clone(), ..Default::default() };
    // the supplies off the board: a wire into the home strip of each pin, one column each, from the left
    let mut col: u16 = 0;
    for (refdes, pins) in &c.off {
        let mut ends = vec![];
        for (pin, net) in pins {
            if let Some(r) = row_of(net) {
                ends.push((pin.clone(), net.clone(), (r, col)));
                col += 1;
            }
        }
        l.off.push((refdes.clone(), ends));
    }
    col += 1;
    // chips and row parts below the home strips, one block each
    let top = homes.len() as u16 + 1;
    let mut bottom = top;
    for (refdes, look, pins) in c.on.iter().filter(|p| p.1 != Look::TwoLead) {
        let mut leads = vec![];
        // the pins' rows and columns: a chip's first half down the left column, the rest up the right one (pin 1 top left); a row
        // part's leads down one column
        let (half, two_cols) = if *look == Look::Dip { (pins.len() / 2, true) } else { (pins.len(), false) };
        let left_pin = col + half as u16; // the left jumpers take the columns before it, one per pin
        let right_pin = left_pin + 3;
        let mut place = |pin: &str, net: &str, at: At, jumper: u16, l: &mut StripLayout| {
            leads.push((pin.to_string(), at));
            if let Some(r) = row_of(net) {
                l.links.push(Link { a: (at.0, jumper), b: (r, jumper), net: net.to_string() });
            }
        };
        for (k, (pin, net)) in pins.iter().enumerate() {
            let (row, left) = if k < half { (top + k as u16, true) } else { (top + (pins.len() - 1 - k) as u16, false) };
            if left {
                place(pin, net, (row, left_pin), col + (row - top), &mut l);
            } else {
                place(pin, net, (row, right_pin), right_pin + 1 + (row - top), &mut l);
            }
        }
        // the cuts: before the block, between the two columns of a chip, after the block
        let end = if two_cols { right_pin + half as u16 + 1 } else { left_pin + 1 };
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
        col = end + 2;
    }
    // the two-lead parts: standing between the home strips of their nets, one column each
    let mut spare = bottom + 1;
    for (refdes, look, pins) in c.on.iter().filter(|p| p.1 == Look::TwoLead) {
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
