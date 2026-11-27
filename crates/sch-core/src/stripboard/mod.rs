//! A stripboard (and the same layout on a plain perfboard) for building the schematic by hand (B5). The copper strips run along the
//! rows; a strip is broken by cutting it at a hole.
//!
//! The layout: every net gets a strip of its own at the top ("home" strips: the supplies first, ground last). A two-lead part stands
//! across the board between the home strips of its two nets, a column apart from the next. A chip or a part with its leads in a row goes below the home strips:
//! each of its pins sits on a piece of strip of its own (cut off on both sides), and a wire link goes from that piece up to the pin's
//! net's home strip, each in a column of its own so no two links overlap. The supplies off the board come in by wires at the left.
//! On a perfboard (no strips) the same layout holds with a bare wire along each piece of strip from its first used hole to its last.

//!
//! The files: this one has the board's model (`StripLayout`), the nets it joins and the sections; `search.rs` tries layouts and keeps
//! the best (`strip_layout_with`), `sections.rs` lays out the sections side by side, `lay.rs` lays out one section (bands of strips and
//! rows of chips), `ends.rs` has the ends on the board and packs the strips into fewer rows, `forks.rs` keeps the Kelvin connections and
//! `check.rs` checks the board against the schematic and the requirements table.

use crate::breadboard::{is_ground_name, read, Look};
use crate::mount::Mount;
use crate::{extract, Schematic};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

mod bypass;
mod check;
mod ends;
mod forks;
mod lay;
mod mate;
mod search;
mod sections;
#[cfg(test)]
mod tests;

pub use check::{bypass_spans, group_spreads, strip_check, strip_rules, strip_unjoined, BypassSpan, RuleCheck};
pub use forks::strip_fork;
pub use mate::{mate_check, mate_parts, Mate, MateLink};
pub use search::strip_layout_with;
use ends::{all_ends, between, end_at, other_end, set_end, share_rows, End};
use bypass::keep_bypass;
use forks::{copper_ways, forks, keep_forks};
use lay::lay;
use sections::{lay_sections, lay_within};


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

/// A wire link between two holes, carrying `net`: an insulated wire on the parts' side (the usual), an insulated wire on the copper
/// side (`back`: it may cross the bare wires there), or a tinned (bare) wire on the parts' side (`tinned`: along a row, a column or at
/// 45 degrees; it touches every lead it passes over and the tinned wires it crosses, as a bare wire on the copper side does). A tinned
/// wire on the copper side is one of `StripLayout::bare`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub a: At,
    pub b: At,
    pub net: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub back: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub tinned: bool,
}

impl Link {
    /// An insulated wire on the parts' side.
    pub fn new(a: At, b: At, net: impl Into<String>) -> Link {
        Link { a, b, net: net.into(), ..Default::default() }
    }

    /// Whether it lies on the parts' side.
    pub fn on_top(&self) -> bool {
        !self.back
    }

    /// Its name for the person: "表の線" (insulated, parts' side), "表のすずめっき線", "裏の線" (insulated, copper side).
    pub fn kind_name(&self) -> &'static str {
        match (self.back, self.tinned) {
            (false, false) => "表の線",
            (false, true) => "表のすずめっき線",
            (true, _) => "裏の線",
        }
    }
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
    /// Bare wires drawn by hand on the copper side of a perfboard, each straight along a row or a column, or at 45 degrees
    /// (`bare_holes`, `StripLayout::bare_joins`).
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
    /// The sections whose box the person put where it is (perfboard, the boxes packed across the board): laid out again
    /// (`perfboard::perf_layout_around`), each box keeps its top-left hole and its width, its parts placed afresh inside, and the other
    /// boxes go round it.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub pinned: BTreeSet<String>,
    /// The holes kept free round the mounting holes (`StripOptions::keepout`): the automatic layout puts nothing there.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub keepout: BTreeSet<At>,
}

/// The holes a bare wire from `a` to `b` runs over, from `a`: along a row or a column, or at 45 degrees (a step of a row and a
/// column at a time: through the middle of each hole on the way). Only its ends for any other slant (no wire is drawn so).
pub fn bare_holes(a: At, b: At) -> Vec<At> {
    let (dr, dc) = (b.0 as i32 - a.0 as i32, b.1 as i32 - a.1 as i32);
    if dr != 0 && dc != 0 && dr.abs() != dc.abs() {
        return vec![a, b];
    }
    let n = dr.abs().max(dc.abs());
    (0..=n).map(|k| ((a.0 as i32 + dr.signum() * k) as u16, (a.1 as i32 + dc.signum() * k) as u16)).collect()
}

/// Whether a bare wire may run from `a` to `b`: along a row or a column, or at 45 degrees.
pub fn bare_way(a: At, b: At) -> bool {
    let (dr, dc) = (b.0 as i32 - a.0 as i32, b.1 as i32 - a.1 as i32);
    dr == 0 || dc == 0 || dr.abs() == dc.abs()
}

/// The middles of the squares of four holes a 45-degree bare wire crosses, each with the way it slants (doubled coordinates, so
/// that a middle is whole): two wires slanting the other way through the same middle touch there, between the holes.
pub fn bare_crossings(a: At, b: At) -> Vec<((i32, i32), bool)> {
    let (dr, dc) = (b.0 as i32 - a.0 as i32, b.1 as i32 - a.1 as i32);
    if dr == 0 || dc == 0 || dr.abs() != dc.abs() {
        return vec![];
    }
    let holes = bare_holes(a, b);
    // one slant: down-right (or up-left); the other: down-left (or up-right)
    let slant = dr.signum() == dc.signum();
    holes.windows(2).map(|w| ((w[0].0 as i32 + w[1].0 as i32, w[0].1 as i32 + w[1].1 as i32), slant)).collect()
}

impl StripLayout {
    /// The automatic bare wires of a perfboard: along each piece of strip from its first hole in use to its last (a lead, a link's or
    /// an off-board wire's end, or the end of a bare wire drawn by hand), where there are two. None on a perfboard laid out for one
    /// (`perf`: every hole a piece of its own, so no piece has two).
    pub fn perf_spans(&self) -> Vec<(At, At)> {
        if self.perf {
            return vec![];
        }
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
        let along = bare_holes;
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

    /// What the tinned wires on the parts' side (`Link::tinned`) join besides their ends: each wire's first hole to every hole along
    /// it with something coming up through it (a lead of a part on top, the end of a wire on top, an off-board wire), and to the
    /// tinned wires on top it crosses (at a hole, or between holes at 45 degrees). A lead on the copper side, a bare wire there and a
    /// wire there are below the board and do not touch it.
    pub fn tinned_joins(&self) -> Vec<(At, At)> {
        let tinned: Vec<&Link> = self.links.iter().filter(|k| k.tinned && !k.back).collect();
        if tinned.is_empty() {
            return vec![];
        }
        let mut up: BTreeSet<At> = self.parts.iter().filter(|p| !matches!(p.mount, crate::mount::Mount::Back { .. })).flat_map(|p| p.leads.iter().map(|x| x.1)).collect();
        up.extend(self.links.iter().filter(|k| !k.back).flat_map(|k| [k.a, k.b]));
        up.extend(self.off.iter().flat_map(|(_, v)| v.iter().map(|w| w.2)));
        let mut over: BTreeMap<At, Vec<usize>> = BTreeMap::new();
        for (i, k) in tinned.iter().enumerate() {
            for h in bare_holes(k.a, k.b) {
                over.entry(h).or_default().push(i);
            }
        }
        let mut out = vec![];
        for (i, k) in tinned.iter().enumerate() {
            for h in bare_holes(k.a, k.b).into_iter().filter(|h| *h != k.a && *h != k.b) {
                if up.contains(&h) || over[&h].len() > 1 {
                    out.push((k.a, h));
                }
            }
            let mine = bare_crossings(k.a, k.b);
            for o in tinned.iter().skip(i + 1) {
                if bare_crossings(o.a, o.b).iter().any(|(m, sl)| mine.iter().any(|(n, t)| n == m && t != sl)) {
                    out.push((k.a, o.a));
                }
            }
        }
        out
    }

    /// Everything that joins two holes besides the strips: the links (whichever side), the bare wires drawn by hand, and what the
    /// tinned wires on top touch.
    pub fn joins(&self) -> Vec<(At, At)> {
        self.links.iter().map(|k| (k.a, k.b)).chain(self.bare_joins()).chain(self.tinned_joins()).collect()
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

    /// What sits on a hole kept free for a mounting hole (`keepout`): a lead, a wire's end, a bare wire, a cut.
    pub fn in_keepout(&self) -> Vec<(At, String)> {
        if self.keepout.is_empty() {
            return vec![];
        }
        let mut v: Vec<(At, String)> = self.used().into_iter().filter(|(a, _)| self.keepout.contains(a)).collect();
        for (a, b) in self.bare.iter().chain(self.perf_spans().iter()) {
            for h in bare_holes(*a, *b) {
                if self.keepout.contains(&h) && !v.iter().any(|x| x.0 == h) {
                    v.push((h, "bare".into()));
                }
            }
        }
        v
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

    /// Each hole on the board and the group of holes it is joined to as the board is built: along its piece of strip, by the bare
    /// wires and by the links (what a meter's probes would find joined). The group is a number, the same for joined holes.
    pub fn hole_groups(&self) -> BTreeMap<At, usize> {
        let (rows, cols) = (self.rows as usize, self.cols as usize);
        let inside = |a: At| (a.0 as usize) < rows && (a.1 as usize) < cols;
        let idx = |a: At| a.0 as usize * cols + a.1 as usize;
        let mut parent: Vec<usize> = (0..rows * cols).collect();
        fn find(p: &mut [usize], mut i: usize) -> usize {
            while p[i] != i {
                p[i] = p[p[i]];
                i = p[i];
            }
            i
        }
        let mut join = |a: At, b: At| {
            if inside(a) && inside(b) {
                let (ra, rb) = (find(&mut parent, idx(a)), find(&mut parent, idx(b)));
                parent[ra] = rb;
            }
        };
        for (r, a, b) in self.pieces() {
            for c in a..b {
                join((r, c), (r, c + 1));
            }
        }
        for (a, b) in self.joins() {
            join(a, b);
        }
        (0..rows * cols).map(|i| (((i / cols) as u16, (i % cols) as u16), find(&mut parent, i))).collect()
    }

    /// The holes joined to `at` (`hole_groups`). A hole off the board is joined to itself alone.
    pub fn joined_to(&self, at: At) -> BTreeSet<At> {
        let groups = self.hole_groups();
        let Some(g) = groups.get(&at) else { return BTreeSet::from([at]) };
        groups.iter().filter(|(_, x)| *x == g).map(|(a, _)| *a).collect()
    }
}

/// The circuit as the stripboard or perfboard joins it (its strips, bare wires and links), for running it powered: each lead's
/// net, named as `breadboard::board_nets` names a breadboard's: a group of joined holes takes ground if a lead of it is on ground,
/// else the first schematic net among its leads; a net split over several groups (an open) keeps its name in the group with the
/// most leads and the others are `NET~2`...; a group with no schematic net is `SB.<hole>`. A short joins two nets into one group
/// (one name), as on the real board.
pub fn strip_board_nets(s: &Schematic, l: &StripLayout) -> crate::breadboard::BoardNets {
    let flat = s.flat();
    let nets = extract(&flat);
    let net_of: BTreeMap<(&str, &str), &str> = nets.iter().flat_map(|n| n.pins.iter().map(move |p| ((p.refdes.as_str(), p.pin.as_str()), n.name.as_str()))).collect();
    let groups = l.hole_groups();
    let group = |a: At| groups.get(&a).copied().unwrap_or(usize::MAX - (a.0 as usize * 4096 + a.1 as usize));
    // (part, pin, schematic net, group, hole, off the board)
    let mut leads: Vec<(String, String, String, usize, At, bool)> = vec![];
    for p in &l.parts {
        for (pin, a) in &p.leads {
            leads.push((p.refdes.clone(), pin.clone(), net_of.get(&(p.refdes.as_str(), pin.as_str())).map(|n| n.to_string()).unwrap_or_default(), group(*a), *a, false));
        }
    }
    for (r, pins) in &l.off {
        for (pin, _, a) in pins {
            leads.push((r.clone(), pin.clone(), net_of.get(&(r.as_str(), pin.as_str())).map(|n| n.to_string()).unwrap_or_default(), group(*a), *a, true));
        }
    }
    let mut info: BTreeMap<usize, (BTreeSet<String>, usize, At)> = BTreeMap::new();
    for (_, _, net, g, a, _) in &leads {
        let e = info.entry(*g).or_insert_with(|| (BTreeSet::new(), 0, *a));
        if !net.is_empty() {
            e.0.insert(net.clone());
        }
        e.1 += 1;
        e.2 = e.2.min(*a);
    }
    let mut order: Vec<(usize, String, usize)> = info
        .iter()
        .map(|(g, (ns, n, a))| (*g, ns.iter().find(|x| crate::breadboard::is_ground_name(x)).or_else(|| ns.iter().next()).cloned().unwrap_or_else(|| format!("SB.{}", hole_name(*a))), *n))
        .collect();
    order.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    let mut used: BTreeMap<String, usize> = BTreeMap::new();
    let mut names: BTreeMap<usize, String> = BTreeMap::new();
    for (g, name, _) in order {
        let k = used.entry(name.to_ascii_uppercase()).or_insert(0);
        *k += 1;
        names.insert(g, if *k == 1 { name } else { format!("{name}~{k}") });
    }
    let mut pins = BTreeMap::new();
    let mut off_pins = vec![];
    for (r, pin, _, g, _, off) in leads {
        if off {
            off_pins.push((r.clone(), pin.clone()));
        }
        pins.insert((r, pin), names[&g].clone());
    }
    // the simulation-only pins (a light sensor's light) and the parts that stand in for what the board is joined to are not leads:
    // they keep the schematic's net, as the board names it
    let board_name: BTreeMap<&str, &str> = names.iter().filter_map(|(g, n)| info.get(g).map(|(ns, _, _)| (ns, n))).flat_map(|(ns, n)| ns.iter().map(move |x| (x.as_str(), n.as_str()))).filter(|(x, n)| n.split('~').next() == Some(*x)).collect();
    for key in crate::breadboard::net_only_pins(&flat) {
        if pins.contains_key(&key) {
            continue;
        }
        if let Some(n) = net_of.get(&(key.0.as_str(), key.1.as_str())) {
            let name = board_name.get(n).map(|x| x.to_string()).unwrap_or_else(|| n.to_string());
            pins.insert(key, name);
        }
    }
    crate::breadboard::BoardNets::from_pins(pins, off_pins)
}

/// The net (as `strip_board_nets` names it: the circuit that runs powered) of every hole joined to a lead: where a meter's probe
/// reads something. A hole joined to no lead is not in it.
pub fn strip_hole_nets(s: &Schematic, l: &StripLayout) -> BTreeMap<At, String> {
    let b = strip_board_nets(s, l);
    let groups = l.hole_groups();
    let mut of_group: BTreeMap<usize, String> = BTreeMap::new();
    let leads = l.parts.iter().flat_map(|p| p.leads.iter().map(move |(pin, a)| (p.refdes.clone(), pin.clone(), *a))).chain(l.off.iter().flat_map(|(r, v)| v.iter().map(move |(pin, _, a)| (r.clone(), pin.clone(), *a))));
    for (r, pin, a) in leads {
        if let (Some(g), Some(n)) = (groups.get(&a), b.pins.get(&(r, pin))) {
            of_group.entry(*g).or_insert_with(|| n.clone());
        }
    }
    groups.into_iter().filter_map(|(a, g)| of_group.get(&g).map(|n| (a, n.clone()))).collect()
}

/// The net of the holes joined to `at`: the schematic's net of a lead among them (None: no lead there).
pub fn net_at(s: &Schematic, l: &StripLayout, at: At) -> Option<String> {
    let joined = l.joined_to(at);
    let nets = extract(&s.flat());
    let leads = l.parts.iter().flat_map(|p| p.leads.iter().map(move |(pin, a)| (p.refdes.as_str(), pin.as_str(), *a))).chain(l.off.iter().flat_map(|(r, v)| v.iter().map(move |(pin, _, a)| (r.as_str(), pin.as_str(), *a))));
    leads.filter(|x| joined.contains(&x.2)).find_map(|(r, pin, _)| nets.iter().find(|n| n.pins.iter().any(|p| p.refdes == r && p.pin == pin)).map(|n| n.name.clone()))
}

/// How to lay the board out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StripOptions {
    /// The board's width in holes: the chips wrap to a new row of chips past it. None: one row of chips, as wide as it takes.
    pub cols: Option<u16>,
    /// A perfboard's 1206 and 0805 chip resistors and capacitors go on the copper side across two holes (`Mount::Back`), next to
    /// what they join, instead of the leaded part on top. For those used to it: off by default.
    #[serde(default)]
    pub chips_back: bool,
    /// The board's height in holes (with `cols`, the board's size): where its bottom corners are.
    #[serde(default)]
    pub rows: Option<u16>,
    /// Mounting holes (M3 with a spacer) in the four corners: the holes round them are kept free (`keepout`). The bottom corners
    /// need `rows`, the right ones `cols`.
    #[serde(default)]
    pub mount_holes: bool,
}

/// How many holes from each corner the mounting hole and its spacer take (an M3 hole on the second hole in from both edges, about
/// 3.8 mm, and a spacer of 6 mm round it): a square of 3 × 3 holes.
pub const MOUNT_HOLE_HOLES: u16 = 3;

impl StripOptions {
    /// The holes kept free for the mounting holes in the corners (none without `mount_holes`).
    pub fn keepout(&self) -> BTreeSet<At> {
        let mut out = BTreeSet::new();
        if !self.mount_holes {
            return out;
        }
        let k = MOUNT_HOLE_HOLES;
        let mut corners = vec![(0u16, 0u16)];
        if let Some(c) = self.cols.filter(|c| *c > 2 * k) {
            corners.push((0, c - k));
        }
        if let Some(r) = self.rows.filter(|r| *r > 2 * k) {
            corners.push((r - k, 0));
            if let Some(c) = self.cols.filter(|c| *c > 2 * k) {
                corners.push((r - k, c - k));
            }
        }
        for (r0, c0) in corners {
            for r in r0..r0 + k {
                for c in c0..c0 + k {
                    out.insert((r, c));
                }
            }
        }
        out
    }
}

/// Lays the schematic out on a stripboard, one row of chips.
pub fn strip_layout(s: &Schematic) -> StripLayout {
    strip_layout_with(s, StripOptions::default())
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
    // the section chosen as the one ground point goes first: the layouts take the first section's ground as the point
    if let Some(k) = ground_point(s).and_then(|g| out.iter().position(|(n, _)| *n == g)) {
        let sec = out.remove(k);
        out.insert(0, sec);
    }
    let rest: BTreeSet<String> = c.on.iter().map(|p| p.0.clone()).filter(|r| !seen.contains(r)).collect();
    if !rest.is_empty() {
        out.push(("その他".into(), rest));
    }
    out
}

/// The section the requirements table chose as the one ground point (`PlaceGroup::ground_point`), by name.
pub fn ground_point(s: &Schematic) -> Option<String> {
    s.requirements.groups.iter().find(|g| g.section && g.ground_point).map(|g| g.name.clone())
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
