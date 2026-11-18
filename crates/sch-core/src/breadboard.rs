//! The breadboard view of a schematic (candidate B6 ①): which hole each lead of each part goes in, the jumper wires, and the
//! supplies and sources off the board with their wires. A solderless breadboard has, per column, two strips of five holes that are
//! joined inside (rows a–e above the groove, f–j below) and, along the long edges, the supply rails (+ and − at the top and at the
//! bottom) that run the whole length. The layout is another view of the same circuit, kept with the sheet (not a file of its own):
//! `auto_layout` places the parts the way people usually do (ICs across the groove, the supply and ground on the rails, a part's leads
//! in the strips of its nets, short jumpers where a strip is full), and `check` compares what the holes join with the schematic's nets,
//! so a part moved by hand into the wrong strip shows up as a short or an open.

use crate::{extract, Component, Kind, Schematic};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Rows of the strips: 0..5 are a–e (above the groove), 5..10 are f–j (below).
pub const ROWS: u8 = 10;
/// Holes in one strip.
pub const STRIP: usize = 5;
/// The columns of a half-size board; the layout grows the board when it needs more.
pub const MIN_COLS: u16 = 30;

/// One of the four rails along the long edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Rail {
    TopPlus,
    TopMinus,
    BottomPlus,
    BottomMinus,
}

/// A hole of the board.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Hole {
    /// Column (0-based) and row (0..10, a–j).
    Strip { col: u16, row: u8 },
    Rail { rail: Rail, col: u16 },
}

impl Hole {
    /// What the hole is joined to inside the board: its strip (a column's upper or lower half) or its rail.
    pub fn node(self) -> Node {
        match self {
            Hole::Strip { col, row } => Node::Strip { col, upper: row < 5 },
            Hole::Rail { rail, .. } => Node::Rail(rail),
        }
    }

    pub fn col(self) -> u16 {
        match self {
            Hole::Strip { col, .. } | Hole::Rail { col, .. } => col,
        }
    }

    /// The name printed on boards: "a12", "+上 5".
    pub fn name(self) -> String {
        match self {
            Hole::Strip { col, row } => format!("{}{}", (b'a' + row) as char, col + 1),
            Hole::Rail { rail, col } => format!("{}{}", rail_name(rail), col + 1),
        }
    }
}

pub fn rail_name(r: Rail) -> &'static str {
    match r {
        Rail::TopPlus => "上+",
        Rail::TopMinus => "上−",
        Rail::BottomPlus => "下+",
        Rail::BottomMinus => "下−",
    }
}

/// What a hole is joined to inside the board.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Node {
    Strip { col: u16, upper: bool },
    Rail(Rail),
}

/// How a part sits on the board, for drawing it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Look {
    /// Two leads (resistor, capacitor, coil, diode, LED, a two-pin library part).
    #[default]
    TwoLead,
    /// A chip with its leads in two rows across the groove.
    Dip,
    /// Leads in a row (transistor, regulator, header).
    Row,
}

/// A part on the board: the hole of each of its leads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placed {
    pub refdes: String,
    #[serde(default)]
    pub look: Look,
    /// (pin, hole), in the part's pin order.
    pub leads: Vec<(String, Hole)>,
}

/// A wire between two holes (a jumper), or from a part off the board (`from` = the supply's pin) to a hole.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wire {
    pub a: Hole,
    pub b: Hole,
    /// The net it carries, for its colour (red for the supply, black for ground).
    #[serde(default)]
    pub net: String,
}

/// A supply or a source that stays off the board (a bench supply, a battery, a signal generator): its pins go to holes by wires.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OffBoard {
    pub refdes: String,
    /// (pin, net, the hole its wire goes to).
    pub pins: Vec<(String, String, Hole)>,
}

/// The breadboard view of a schematic.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Layout {
    pub cols: u16,
    pub parts: Vec<Placed>,
    pub wires: Vec<Wire>,
    pub off_board: Vec<OffBoard>,
    /// What each rail carries (net names), for the labels.
    #[serde(default)]
    pub rails: BTreeMap<String, String>,
}

/// A difference between the board and the schematic.
#[derive(Clone, Debug, PartialEq)]
pub enum Problem {
    /// Two nets of the schematic meet in the holes (the parts' leads named).
    Short { nets: Vec<String>, at: Vec<String> },
    /// The pins of one net are in holes that are not joined.
    Open { net: String, apart: Vec<String> },
    /// A part of the schematic is not on the board, or a lead is missing.
    Missing { refdes: String },
    /// Two leads in one hole.
    SameHole { hole: String, leads: Vec<String> },
}

impl Problem {
    /// The message, with what to do about it.
    pub fn message(&self) -> String {
        match self {
            Problem::Short { nets, at } => format!("ショート: {} がブレッドボードの中でつながっています ({})。どちらかの足を別の列に差し替えます", nets.join(" と "), at.join(", ")),
            Problem::Open { net, apart } => format!("つながっていません: {net} のピン {} が、別々の列にあります。同じ列に差すか、ジャンパー線でつなぎます", apart.join(" / ")),
            Problem::Missing { refdes } => format!("{refdes} がブレッドボードにありません。「自動で並べる」で並べ直せます"),
            Problem::SameHole { hole, leads } => format!("{hole} の穴に {} が重なっています。1 つの穴には足を 1 本だけ差します", leads.join(" と ")),
        }
    }
}

/// The supplies and sources that stay off the board.
fn is_off_board(c: &Component) -> bool {
    matches!(c.kind, Kind::VSource | Kind::ISource) || c.sim_only || c.custom.as_ref().is_some_and(|s| s.prefix.eq_ignore_ascii_case("BT") || s.prefix.eq_ignore_ascii_case("V"))
}

/// The pins of a part that have a lead (library pins only for the simulation left out), in their order.
fn leads_of(c: &Component) -> Vec<String> {
    match &c.custom {
        Some(sym) => {
            let mut pins: Vec<&crate::SymPin> = sym.pins.iter().filter(|p| !p.sim_only).collect();
            pins.sort_by_key(|p| (p.number.parse::<u32>().unwrap_or(u32::MAX), p.number.clone()));
            pins.iter().map(|p| p.number.clone()).collect()
        }
        None => c.kind.pins().into_iter().map(|(n, _)| n).collect(),
    }
}

/// A net whose name says it is a supply (VCC, VDD, VIN, VBAT, +5V, 3V3, 12V): a source that ramps up (PWL) still feeds the rails.
fn looks_like_supply(n: &str) -> bool {
    let u = n.to_ascii_uppercase();
    ["VCC", "VDD", "VIN", "VBAT", "VS", "V+"].iter().any(|p| u.starts_with(p)) || u.starts_with('+') || (u.starts_with(|c: char| c.is_ascii_digit()) && u.contains('V'))
}

fn is_ground_name(n: &str) -> bool {
    matches!(n.to_ascii_uppercase().as_str(), "GND" | "0" | "VSS" | "AGND" | "DGND")
}

/// A part's pins and their nets.
type Pins = Vec<(String, String)>;

/// The parts that go on the board and their pins' nets, and the off-board ones.
struct Circuit {
    on: Vec<(String, Look, Pins)>,
    off: Vec<(String, Pins)>,
    /// Supply nets, the main one first (the + of the sources and the power symbols), ground apart.
    supplies: Vec<String>,
    ground: Option<String>,
}

fn read(s: &Schematic) -> Circuit {
    let flat = s.flat();
    let nets = extract(&flat);
    let mut net_of: BTreeMap<(String, String), String> = BTreeMap::new();
    for n in &nets {
        for p in &n.pins {
            net_of.insert((p.refdes.clone(), p.pin.clone()), n.name.clone());
        }
    }
    let (mut on, mut off) = (vec![], vec![]);
    let mut supplies: Vec<String> = vec![];
    let mut ground = None;
    let add_supply = |n: &str, supplies: &mut Vec<String>| {
        if !n.is_empty() && !is_ground_name(n) && !supplies.iter().any(|x| x == n) {
            supplies.push(n.to_string());
        }
    };
    for c in &flat.components {
        if matches!(c.kind, Kind::Ground | Kind::Power) {
            if c.kind == Kind::Power {
                add_supply(&c.value, &mut supplies);
            }
            continue;
        }
        let pins: Vec<(String, String)> = leads_of(c).into_iter().map(|p| { let n = net_of.get(&(c.refdes.clone(), p.clone())).cloned().unwrap_or_default(); (p, n) }).collect();
        if pins.is_empty() {
            continue;
        }
        if is_off_board(c) {
            // a DC source between a net and ground feeds a rail; the first pin is +
            if let (Some((_, plus)), Some((_, minus))) = (pins.first(), pins.get(1))
                && is_ground_name(minus)
                && c.kind != Kind::ISource
                && (!c.value.contains('(') || looks_like_supply(plus))
            {
                add_supply(plus, &mut supplies);
            }
            off.push((c.refdes.clone(), pins));
            continue;
        }
        let look = match (c.kind, pins.len()) {
            (Kind::Header(_), _) => Look::Row,
            (_, 0..=2) => Look::TwoLead,
            (_, 3) => Look::Row,
            _ if pins.len().is_multiple_of(2) => Look::Dip,
            _ => Look::Row,
        };
        on.push((c.refdes.clone(), look, pins));
    }
    for n in &nets {
        if is_ground_name(&n.name) {
            ground = Some(n.name.clone());
        }
    }
    Circuit { on, off, supplies, ground }
}

/// Places the parts of `s` on a breadboard.
pub fn auto_layout(s: &Schematic) -> Layout {
    let c = read(s);
    // the rails: ground on both − rails, the supply on both + rails (joined at the left end); a second supply takes the bottom +
    let mut rail_of: RailMap = BTreeMap::new();
    let mut rails = BTreeMap::new();
    if let Some(g) = &c.ground {
        rail_of.insert(g.clone(), (Rail::TopMinus, Some(Rail::BottomMinus)));
        rails.insert("TopMinus".to_string(), g.clone());
        rails.insert("BottomMinus".to_string(), g.clone());
    }
    match (c.supplies.first(), c.supplies.get(1)) {
        (Some(v), None) => {
            rail_of.insert(v.clone(), (Rail::TopPlus, Some(Rail::BottomPlus)));
            rails.insert("TopPlus".to_string(), v.clone());
            rails.insert("BottomPlus".to_string(), v.clone());
        }
        (Some(v), Some(w)) => {
            rail_of.insert(v.clone(), (Rail::TopPlus, None));
            rails.insert("TopPlus".to_string(), v.clone());
            rail_of.insert(w.clone(), (Rail::BottomPlus, None));
            rails.insert("BottomPlus".to_string(), w.clone());
        }
        _ => {}
    }
    let mut b = Builder { next_col: 2, strips: BTreeMap::new(), home: BTreeMap::new(), wires: vec![], rail_used: BTreeMap::new() };
    let mut parts = vec![];
    // chips first, across the groove, each with a free column on both sides
    for (refdes, look, pins) in c.on.iter().filter(|p| p.1 == Look::Dip) {
        let half = pins.len() / 2;
        let col0 = b.next_col;
        b.next_col += half as u16 + 1;
        let mut leads = vec![];
        for (k, (pin, net)) in pins.iter().enumerate() {
            // pin 1 at the lower left, counting on to the right, then back along the upper row
            let (col, row) = if k < half { (col0 + k as u16, 5) } else { (col0 + (pins.len() - 1 - k) as u16, 4) };
            let hole = Hole::Strip { col, row };
            b.claim(hole, net, &rail_of);
            leads.push((pin.clone(), hole));
        }
        parts.push(Placed { refdes: refdes.clone(), look: *look, leads });
    }
    // parts with leads in a row (transistors, regulators, headers): in row e next to the groove (the body over the groove), side by side
    for (refdes, look, pins) in c.on.iter().filter(|p| p.1 == Look::Row) {
        let col0 = b.next_col;
        b.next_col += pins.len() as u16 + 1;
        let mut leads = vec![];
        for (k, (pin, net)) in pins.iter().enumerate() {
            let hole = Hole::Strip { col: col0 + k as u16, row: 4 };
            b.claim(hole, net, &rail_of);
            leads.push((pin.clone(), hole));
        }
        parts.push(Placed { refdes: refdes.clone(), look: *look, leads });
    }
    // two-lead parts: lying along a row between strips of their nets, or standing from a strip to the rail beside it
    for (refdes, look, pins) in c.on.iter().filter(|p| p.1 == Look::TwoLead) {
        let holes: Vec<Hole> = match pins.as_slice() {
            [(_, a), (_, z)] => b.place_two(a, z, &rail_of).to_vec(),
            _ => pins.iter().map(|(_, n)| b.lead_hole(n, &rail_of, None)).collect(),
        };
        let leads = pins.iter().zip(holes).map(|((pin, _), h)| (pin.clone(), h)).collect();
        parts.push(Placed { refdes: refdes.clone(), look: *look, leads });
    }
    // the parts off the board: a wire from each pin to its rail or a strip of its net
    let mut off_board = vec![];
    for (refdes, pins) in &c.off {
        let mut out = vec![];
        for (pin, net) in pins {
            let hole = b.lead_hole(net, &rail_of, Some(0));
            out.push((pin.clone(), net.clone(), hole));
        }
        off_board.push(OffBoard { refdes: refdes.clone(), pins: out });
    }
    // a net on both rails: the bottom one is joined to the top one by a wire at the left end, when it is used
    for (net, (top, bottom)) in &rail_of {
        if let Some(bottom) = bottom
            && b.rail_used.contains_key(bottom)
        {
            let (a, z) = (b.rail_hole(*top, 0), b.rail_hole(*bottom, 0));
            b.wires.push(Wire { a, b: z, net: net.clone() });
        }
    }
    let cols = (b.next_col + 2).max(MIN_COLS);
    Layout { cols, parts, wires: b.wires, off_board, rails }
}

/// The rails a net is on: the top one, and the bottom one when it is on both.
type RailMap = BTreeMap<String, (Rail, Option<Rail>)>;

/// The rail of a net beside a strip in the upper or the lower half.
fn rail_beside(pair: (Rail, Option<Rail>), upper: bool) -> Rail {
    if upper { pair.0 } else { pair.1.unwrap_or(pair.0) }
}

struct Builder {
    next_col: u16,
    /// What net each strip carries and how many of its holes are taken.
    strips: BTreeMap<Node, (String, BTreeSet<u8>)>,
    /// The strips of each net, the first one is its home.
    home: BTreeMap<String, Vec<Node>>,
    wires: Vec<Wire>,
    /// Columns taken on each rail.
    rail_used: BTreeMap<Rail, BTreeSet<u16>>,
}

impl Builder {
    /// A hole that a chip or a row part takes: its strip becomes one of its net's (joined to the net's other strips by a jumper).
    fn claim(&mut self, hole: Hole, net: &str, rail_of: &RailMap) {
        let Hole::Strip { col, row } = hole else { return };
        let node = hole.node();
        let entry = self.strips.entry(node).or_insert_with(|| (net.to_string(), BTreeSet::new()));
        entry.1.insert(row);
        if net.is_empty() {
            return;
        }
        let strips = self.home.entry(net.to_string()).or_default();
        if !strips.contains(&node) {
            strips.push(node);
            // tie the strip to the rail, or to the net's first strip
            if let Some(&pair) = rail_of.get(net) {
                let from = self.free_row(node).map(|row| Hole::Strip { col, row }).unwrap_or(hole);
                self.mark(from);
                let to = self.rail_hole(rail_beside(pair, row < 5), col);
                self.wires.push(Wire { a: from, b: to, net: net.to_string() });
            } else if strips.len() > 1 {
                let from = self.free_row(node).map(|row| Hole::Strip { col, row }).unwrap_or(hole);
                self.mark(from);
                self.join(net, node, from);
            }
        }
    }

    fn mark(&mut self, h: Hole) {
        if let Hole::Strip { row, .. } = h
            && let Some(e) = self.strips.get_mut(&h.node())
        {
            e.1.insert(row);
        }
    }

    fn rows(node: Node) -> std::ops::Range<u8> {
        match node {
            Node::Strip { upper: true, .. } => 0..5,
            _ => 5..10,
        }
    }

    /// A free row of a strip, the one farthest from the groove first (the chip's leads are next to it).
    fn free_row(&self, node: Node) -> Option<u8> {
        let taken = self.strips.get(&node).map(|e| e.1.clone()).unwrap_or_default();
        let mut rows: Vec<u8> = Self::rows(node).collect();
        if matches!(node, Node::Strip { upper: false, .. }) {
            rows.reverse();
        }
        rows.into_iter().find(|r| !taken.contains(r))
    }

    fn free_hole(&self, node: Node) -> Option<Hole> {
        let Node::Strip { col, .. } = node else { return None };
        self.free_row(node).map(|row| Hole::Strip { col, row })
    }

    /// A hole on rail `r` at `col` or the nearest free column.
    fn rail_hole(&mut self, r: Rail, col: u16) -> Hole {
        let used = self.rail_used.entry(r).or_default();
        let mut c = col;
        for d in 0..200u16 {
            for cand in [col.saturating_add(d), col.saturating_sub(d)] {
                if !used.contains(&cand) {
                    c = cand;
                    break;
                }
            }
            if !used.contains(&c) {
                break;
            }
        }
        used.insert(c);
        Hole::Rail { rail: r, col: c }
    }

    /// A hole for a lead on `net`: its rail, else a free hole of one of its strips, else a new strip (joined to the others).
    fn lead_hole(&mut self, net: &str, rail_of: &RailMap, near: Option<u16>) -> Hole {
        if let Some(&(r, _)) = rail_of.get(net) {
            let col = near.unwrap_or(self.next_col);
            return self.rail_hole(r, col);
        }
        if net.is_empty() {
            // a lead on no net: a strip of its own
            let col = self.next_col;
            self.next_col += 1;
            let hole = Hole::Strip { col, row: 0 };
            self.strips.insert(hole.node(), (String::new(), BTreeSet::from([0])));
            return hole;
        }
        let strips = self.home.get(net).cloned().unwrap_or_default();
        // the last free hole of the net is kept for the jumper to a new strip
        if self.free_count(net) > 1 {
            for node in &strips {
                if let Some(h) = self.free_hole(*node) {
                    self.mark(h);
                    return h;
                }
            }
        }
        // a new strip in the upper half
        let col = self.next_col;
        self.next_col += 1;
        let hole = Hole::Strip { col, row: 0 };
        self.strips.insert(hole.node(), (net.to_string(), BTreeSet::from([0])));
        let list = self.home.entry(net.to_string()).or_default();
        list.push(hole.node());
        if list.len() > 1 {
            let from = Hole::Strip { col, row: 1 };
            self.mark(from);
            self.join(net, hole.node(), from);
        }
        hole
    }

    /// The strips of `net` a lead may go in now (each net keeps its last free hole for a jumper to a new strip).
    fn usable(&self, net: &str) -> Vec<Node> {
        if net.is_empty() || self.free_count(net) <= 1 {
            return vec![];
        }
        self.home.get(net).cloned().unwrap_or_default().into_iter().filter(|n| self.free_row(*n).is_some()).collect()
    }

    /// A new strip for `net` in the upper or lower half at the next free column, joined to the net's other strips (the jumper takes
    /// the hole next to the groove, leaving the outer ones for parts).
    fn new_strip(&mut self, net: &str, upper: bool) -> Node {
        let col = self.next_col;
        self.next_col += 1;
        let node = Node::Strip { col, upper };
        self.strips.insert(node, (net.to_string(), BTreeSet::new()));
        if net.is_empty() {
            return node;
        }
        let list = self.home.entry(net.to_string()).or_default();
        list.push(node);
        if list.len() > 1 {
            let from = Hole::Strip { col, row: if upper { 4 } else { 5 } };
            self.mark(from);
            self.join(net, node, from);
        }
        node
    }

    /// The holes of a two-lead part between nets `a` and `z`.
    fn place_two(&mut self, a: &str, z: &str, rail_of: &RailMap) -> [Hole; 2] {
        match (rail_of.get(a).copied(), rail_of.get(z).copied()) {
            // across the rails (a capacitor from the supply to ground)
            (Some(pa), Some(pz)) => {
                let col = self.next_col;
                [self.rail_hole(pa.0, col), self.rail_hole(pz.0, col)]
            }
            // standing: from the outer hole of a strip of the other net to the rail beside it
            (Some(pa), None) => {
                let [hz, ha] = self.stand_on_rail(z, pa);
                [ha, hz]
            }
            (None, Some(pz)) => self.stand_on_rail(a, pz),
            // lying along a row between a strip of each net: the nearest pair in one half with a row free in both
            (None, None) => {
                let (sa, sz) = (self.usable(a), self.usable(z));
                let mut best: Option<(u16, Node, Node, u8)> = None;
                for na in &sa {
                    for nz in &sz {
                        let (Node::Strip { col: ca, upper: ua }, Node::Strip { col: cz, upper: uz }) = (*na, *nz) else { continue };
                        if ua != uz || ca == cz {
                            continue;
                        }
                        if let Some(r) = self.common_row(*na, *nz) {
                            let d = ca.abs_diff(cz);
                            if best.is_none_or(|b| d < b.0) {
                                best = Some((d, *na, *nz, r));
                            }
                        }
                    }
                }
                let (na, nz, row) = match best {
                    Some((_, na, nz, r)) => (na, nz, r),
                    None => {
                        // a new strip for one net beside a strip of the other, in its half
                        let (na, nz) = match (sa.first(), sz.first()) {
                            (Some(&na), _) => {
                                let upper = matches!(na, Node::Strip { upper: true, .. });
                                (na, self.new_strip(z, upper))
                            }
                            (None, Some(&nz)) => {
                                let upper = matches!(nz, Node::Strip { upper: true, .. });
                                (self.new_strip(a, upper), nz)
                            }
                            (None, None) => (self.new_strip(a, true), self.new_strip(z, true)),
                        };
                        let row = self.common_row(na, nz).or_else(|| self.free_row(na)).unwrap_or(0);
                        (na, nz, row)
                    }
                };
                let ha = self.hole_in(na, row);
                let hz = self.hole_in(nz, row);
                [ha, hz]
            }
        }
    }

    /// A hole of strip `n` in `row` if it is free, else its first free one; taken.
    fn hole_in(&mut self, n: Node, row: u8) -> Hole {
        let Node::Strip { col, .. } = n else { return Hole::Strip { col: 0, row } };
        let h = if self.row_free(n, row) { Hole::Strip { col, row } } else { self.free_hole(n).unwrap_or(Hole::Strip { col, row }) };
        self.mark(h);
        h
    }

    /// A lead in the outer free hole of a strip of `net` and the other on the rail `pair` beside it.
    fn stand_on_rail(&mut self, net: &str, pair: (Rail, Option<Rail>)) -> [Hole; 2] {
        let node = match self.usable(net).first() {
            Some(&n) => n,
            None => self.new_strip(net, true),
        };
        let h = self.hole_in(node, if matches!(node, Node::Strip { upper: true, .. }) { 0 } else { 9 });
        let upper = matches!(node, Node::Strip { upper: true, .. });
        let r = self.rail_hole(rail_beside(pair, upper), h.col());
        [h, r]
    }

    fn row_free(&self, n: Node, row: u8) -> bool {
        Self::rows(n).contains(&row) && !self.strips.get(&n).is_some_and(|e| e.1.contains(&row))
    }

    /// A row free in both strips (of one half), the one farthest from the groove first.
    fn common_row(&self, a: Node, z: Node) -> Option<u8> {
        let mut rows: Vec<u8> = Self::rows(a).collect();
        if matches!(a, Node::Strip { upper: false, .. }) {
            rows.reverse();
        }
        rows.into_iter().find(|r| self.row_free(a, *r) && self.row_free(z, *r))
    }

    /// A jumper from `from` (in strip `node`) to the latest other strip of `net` that has a free hole (a chain, so no strip fills up
    /// with jumpers; each strip keeps a hole for the next one, see `lead_hole`).
    fn join(&mut self, net: &str, node: Node, from: Hole) {
        let strips = self.home.get(net).cloned().unwrap_or_default();
        if let Some(to) = strips.iter().rev().filter(|n| **n != node).find_map(|n| self.free_hole(*n)) {
            self.mark(to);
            self.wires.push(Wire { a: from, b: to, net: net.to_string() });
        }
    }

    /// Free holes left in the strips of `net`.
    fn free_count(&self, net: &str) -> usize {
        let strips = self.home.get(net).cloned().unwrap_or_default();
        strips.iter().map(|n| Self::rows(*n).len() - self.strips.get(n).map_or(0, |e| e.1.len())).sum()
    }
}

/// Which holes are joined, through the board, the wires and the off-board parts' own connections (none: each pin is its own net).
fn groups(l: &Layout) -> BTreeMap<Node, Node> {
    let mut parent: BTreeMap<Node, Node> = BTreeMap::new();
    fn find(p: &mut BTreeMap<Node, Node>, n: Node) -> Node {
        let up = *p.entry(n).or_insert(n);
        if up == n {
            return n;
        }
        let root = find(p, up);
        p.insert(n, root);
        root
    }
    for w in &l.wires {
        let (a, b) = (find(&mut parent, w.a.node()), find(&mut parent, w.b.node()));
        if a != b {
            parent.insert(a, b);
        }
    }
    let nodes: Vec<Node> = parent.keys().copied().collect();
    let mut out = BTreeMap::new();
    for n in nodes {
        let r = find(&mut parent, n);
        out.insert(n, r);
    }
    out
}

/// Compares what the holes join with the schematic's nets.
pub fn check(s: &Schematic, l: &Layout) -> Vec<Problem> {
    let c = read(s);
    let g = groups(l);
    let root = |h: Hole| g.get(&h.node()).copied().unwrap_or(h.node());
    let mut problems = vec![];
    // every lead of the schematic, with its hole
    let mut leads: Vec<(String, String, Hole)> = vec![];
    for (refdes, _, pins) in &c.on {
        let Some(p) = l.parts.iter().find(|p| p.refdes == *refdes) else {
            problems.push(Problem::Missing { refdes: refdes.clone() });
            continue;
        };
        for (pin, net) in pins {
            match p.leads.iter().find(|(n, _)| n == pin) {
                Some((_, h)) => leads.push((format!("{refdes}.{pin}"), net.clone(), *h)),
                None => problems.push(Problem::Missing { refdes: format!("{refdes}.{pin}") }),
            }
        }
    }
    for (refdes, pins) in &c.off {
        let Some(o) = l.off_board.iter().find(|o| o.refdes == *refdes) else {
            problems.push(Problem::Missing { refdes: refdes.clone() });
            continue;
        };
        for (pin, net) in pins {
            if let Some((_, _, h)) = o.pins.iter().find(|(n, _, _)| n == pin) {
                leads.push((format!("{refdes}.{pin}"), net.clone(), *h));
            }
        }
    }
    // two leads in one hole
    let mut by_hole: BTreeMap<Hole, Vec<String>> = BTreeMap::new();
    for (who, _, h) in &leads {
        by_hole.entry(*h).or_default().push(who.clone());
    }
    for (h, who) in &by_hole {
        if who.len() > 1 {
            problems.push(Problem::SameHole { hole: h.name(), leads: who.clone() });
        }
    }
    // a group with two nets: a short
    let mut nets_in: BTreeMap<Node, BTreeMap<String, Vec<String>>> = BTreeMap::new();
    for (who, net, h) in &leads {
        if !net.is_empty() {
            nets_in.entry(root(*h)).or_default().entry(net.clone()).or_default().push(who.clone());
        }
    }
    for nets in nets_in.values() {
        if nets.len() > 1 {
            problems.push(Problem::Short { nets: nets.keys().cloned().collect(), at: nets.values().filter_map(|v| v.first().cloned()).collect() });
        }
    }
    // a net in two groups: an open
    let mut groups_of: BTreeMap<String, BTreeMap<Node, Vec<String>>> = BTreeMap::new();
    for (who, net, h) in &leads {
        if !net.is_empty() {
            groups_of.entry(net.clone()).or_default().entry(root(*h)).or_default().push(who.clone());
        }
    }
    for (net, gs) in groups_of {
        if gs.len() > 1 {
            problems.push(Problem::Open { net, apart: gs.values().map(|v| v.join(",")).collect() });
        }
    }
    problems
}

/// Moves part `refdes` by whole holes (`dcol` columns, `drow` rows); a chip keeps to its rows across the groove, and a lead on a rail
/// moves along it. None when a lead would leave the board.
pub fn moved(l: &Layout, refdes: &str, dcol: i32, drow: i32) -> Option<Layout> {
    let mut out = l.clone();
    let p = out.parts.iter_mut().find(|p| p.refdes == refdes)?;
    let drow = if p.look == Look::Dip { 0 } else { drow };
    for (_, h) in &mut p.leads {
        *h = match *h {
            Hole::Strip { col, row } => {
                let (c, r) = (col as i32 + dcol, row as i32 + drow);
                if c < 0 || c >= l.cols as i32 || !(0..ROWS as i32).contains(&r) {
                    return None;
                }
                Hole::Strip { col: c as u16, row: r as u8 }
            }
            Hole::Rail { rail, col } => {
                let c = col as i32 + dcol;
                if c < 0 || c >= l.cols as i32 {
                    return None;
                }
                Hole::Rail { rail, col: c as u16 }
            }
        };
    }
    Some(out)
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

    /// The layout of every sample joins exactly what its schematic joins.
    #[test]
    fn every_sample_lays_out_without_a_short_or_an_open() {
        let mut bad = vec![];
        let mut n = 0;
        for (name, s) in examples() {
            let l = auto_layout(&s);
            n += 1;
            let p = check(&s, &l);
            if !p.is_empty() {
                bad.push(format!("{name}: {}", p.iter().take(3).map(|x| x.message()).collect::<Vec<_>>().join(" | ")));
            }
        }
        assert!(n > 50, "{n} samples");
        assert!(bad.is_empty(), "{} of {n}:\n{}", bad.len(), bad.join("\n"));
    }

    #[test]
    fn a_chip_straddles_the_groove_and_the_supply_is_on_the_rails() {
        let s: Schematic = serde_json::from_str(include_str!("../../../docs/examples/ne555_blinker.schjson")).unwrap();
        let l = auto_layout(&s);
        let chip = l.parts.iter().find(|p| p.look == Look::Dip).expect("the 555");
        let rows: BTreeSet<u8> = chip.leads.iter().filter_map(|(_, h)| match h { Hole::Strip { row, .. } => Some(*row), _ => None }).collect();
        assert_eq!(rows, BTreeSet::from([4, 5]), "rows e and f");
        assert!(l.rails.contains_key("TopPlus") && l.rails.contains_key("TopMinus"), "{:?}", l.rails);
        assert!(l.parts.iter().flat_map(|p| &p.leads).any(|(_, h)| matches!(h, Hole::Rail { .. })) || l.wires.iter().any(|w| matches!(w.b, Hole::Rail { .. })));
    }

    #[test]
    fn a_lead_moved_into_another_strip_is_a_short_and_moving_back_clears_it() {
        let s: Schematic = serde_json::from_str(include_str!("../../../docs/examples/ne555_blinker.schjson")).unwrap();
        let l = auto_layout(&s);
        assert!(check(&s, &l).is_empty());
        // move a two-lead part sideways until the check finds something wrong
        let r = l.parts.iter().find(|p| p.look == Look::TwoLead).unwrap().refdes.clone();
        let m = (1..6).find_map(|d| moved(&l, &r, d, 0).filter(|m| !check(&s, m).is_empty())).expect("a move that breaks it");
        let p = check(&s, &m);
        assert!(p.iter().any(|x| matches!(x, Problem::Short { .. } | Problem::Open { .. } | Problem::SameHole { .. })), "{p:?}");
        assert!(!p[0].message().is_empty());
        assert!(moved(&l, &r, -1000, 0).is_none(), "off the board");
    }

    #[test]
    fn holes_are_named_as_on_the_board() {
        assert_eq!(Hole::Strip { col: 11, row: 0 }.name(), "a12");
        assert_eq!(Hole::Strip { col: 0, row: 9 }.name(), "j1");
        assert_eq!(Hole::Strip { col: 3, row: 4 }.node(), Node::Strip { col: 3, upper: true });
        assert_eq!(Hole::Strip { col: 3, row: 5 }.node(), Node::Strip { col: 3, upper: false });
        let l = Layout { cols: 30, ..Default::default() };
        let json = serde_json::to_string(&Layout { parts: vec![Placed { refdes: "R1".into(), look: Look::TwoLead, leads: vec![("1".into(), Hole::Strip { col: 1, row: 2 }), ("2".into(), Hole::Rail { rail: Rail::TopPlus, col: 4 })] }], ..l }).unwrap();
        let back: Layout = serde_json::from_str(&json).unwrap();
        assert_eq!(back.parts[0].leads[1].1, Hole::Rail { rail: Rail::TopPlus, col: 4 });
    }
}
