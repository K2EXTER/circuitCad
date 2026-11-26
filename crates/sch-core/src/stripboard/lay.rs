//! One section (or the whole board) laid out: the nets' strips in bands, the chips in rows between them, the wires and the parts
//! standing across the strips.

use super::*;
use crate::breadboard::Circuit;

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
    /// Each pin's way and its wire's place among its side's wires (from the outside in), before the columns are set (`place`).
    raw: Vec<Option<(Dir, u16)>>,
    /// The wires on each side (left, right), the spare holes at the pieces' outer ends, the columns between the two rows of leads.
    count: [u16; 2],
    spare: u16,
    width: u16,
    two_cols: bool,
}

impl Block {
    /// Sets the block's columns: `spare` holes at the outer ends, `total` columns of wires on each side with this block's wires
    /// from `from` on (a stack's chips share the columns: `stack_columns`), its left row of leads after them, the right row its
    /// width on, and the right side's wires past `widest` (the widest chip in the stack: none passes over a chip's body).
    fn place(&mut self, from: [u16; 2], total: [u16; 2], spare: u16, widest: u16) {
        let x = spare;
        self.left_pin = x + total[0];
        self.right_pin = self.left_pin + self.width;
        let right_wires = self.left_pin + widest + 1;
        self.end = if self.two_cols { right_wires + total[1] + spare } else { self.left_pin + 1 };
        for (p, raw) in self.pins.iter_mut().zip(&self.raw) {
            p.wire = raw.map(|(d, n)| (d, if p.left { x + from[0] + n } else { right_wires + from[1] + n }));
        }
    }
}

/// Lays a stack of chips (top first) out in the same columns: each one's wires all up but the bottom one's, all down. The chips with
/// their wires up each take columns of their own on each side (a chip's wires up pass the chips above it), their rows of leads lined
/// up and the right side's wires past the widest of them (none passes over a chip's body). The bottom one keeps its own columns: its
/// wires go down from it, past nothing, and none passes it.
fn stack_columns(stack: &mut [&mut Block]) {
    let n = stack.len();
    let up = n.saturating_sub(1);
    let mut from = vec![[0u16; 2]; n];
    let mut sum = [0u16; 2];
    for (i, b) in stack.iter().enumerate().take(up) {
        from[i] = sum;
        sum = [sum[0] + b.count[0], sum[1] + b.count[1]];
    }
    let spare = stack.iter().take(up).map(|b| b.spare).max().unwrap_or(0);
    let widest = stack.iter().take(up).map(|b| b.width).max().unwrap_or(3);
    for (i, b) in stack.iter_mut().enumerate() {
        if i < up {
            b.place(from[i], sum, spare, widest);
        } else {
            let (count, spare, width) = (b.count, b.spare, b.width);
            b.place([0, 0], count, spare, width);
        }
    }
}

/// A band's strips in their order (`nets`), changed so that the nets of each pair (a chip on the copper side across two strips) sit
/// next to each other where they can: the pairs make chains (each strip has two neighbours at most, no rings), each chain in the
/// place of its first net, ground kept at the bottom (a chain with ground ends with it and goes last).
pub(super) fn pair_up<'a>(nets: Vec<&'a String>, pairs: &[(String, String)], ground: &dyn Fn(&str) -> bool) -> Vec<&'a String> {
    let pos = |n: &str| nets.iter().position(|x| x.as_str() == n);
    let mut next: Vec<Vec<usize>> = vec![vec![]; nets.len()];
    let mut root: Vec<usize> = (0..nets.len()).collect();
    fn find(r: &mut [usize], mut i: usize) -> usize {
        while r[i] != i {
            r[i] = r[r[i]];
            i = r[i];
        }
        i
    }
    for (a, b) in pairs {
        let (Some(i), Some(j)) = (pos(a), pos(b)) else { continue };
        if next[i].len() >= 2 || next[j].len() >= 2 || next[i].contains(&j) {
            continue;
        }
        let (ri, rj) = (find(&mut root, i), find(&mut root, j));
        if ri == rj {
            continue;
        }
        root[ri] = rj;
        next[i].push(j);
        next[j].push(i);
    }
    if next.iter().all(|v| v.is_empty()) {
        return nets;
    }
    // each chain from an end (ground at the far end), in the order of its first net
    let mut seen = vec![false; nets.len()];
    let mut chains: Vec<Vec<usize>> = vec![];
    for i in 0..nets.len() {
        if seen[i] || next[i].len() > 1 {
            continue;
        }
        let mut chain = vec![i];
        seen[i] = true;
        let mut at = i;
        while let Some(&n) = next[at].iter().find(|n| !seen[**n]) {
            seen[n] = true;
            chain.push(n);
            at = n;
        }
        if ground(nets[chain[0]]) {
            chain.reverse();
        }
        chains.push(chain);
    }
    let has_ground = |c: &Vec<usize>| c.iter().any(|i| ground(nets[*i]));
    chains.sort_by_key(|c| (has_ground(c), *c.iter().min().unwrap()));
    chains.into_iter().flatten().map(|i| nets[i]).collect()
}
/// Lays out one section (or the whole board): `external` are the nets that also go to other sections (they get a strip in the top
/// band, as the supplies do).
///
/// The steps: how each net is carried (`Ctx::new`), each chip as a block by itself (`Ctx::build`), the blocks in rows of chips
/// (`Ctx::rows_of_chips`), the nets each band of strips needs (`Ctx::band_nets`), the rows of the board (`Ctx::rows`), then on the
/// board: the off-board wires, the chips and their wires, the wires between chip pins, the wires between bands, the two-lead parts,
/// the loose parts (`Board`), and the strips packed into fewer rows (`share_rows`).
pub(super) fn lay(c: &Circuit, opt: StripOptions, external: &BTreeSet<String>) -> StripLayout {
    let x = Ctx::new(c, opt, external);
    let mut blocks: Vec<Block> = chip_order(&c.on, &x.carry).into_iter().map(|k| x.build(k, None)).collect();
    // the off-board parts' wires: one column each at the far left, into the top band
    let off_pins: usize = c.off.iter().map(|(_, p)| p.iter().filter(|(_, n)| x.carry.get(n) == Some(&Carry::Home)).count()).sum();
    let margin = off_pins as u16 + 1;
    let (tiers, drop) = x.rows_of_chips(&mut blocks, margin);
    let (band_nets, part_band) = x.band_nets(&blocks, &tiers);
    let rows = x.rows(&band_nets, &tiers, &blocks, &drop);
    let mut b = Board::default();
    b.off_wires(&x, &rows.strip_row[0]);
    let (piece_of_pin, mut placed) = b.chips(&x, &tiers, &blocks, &drop, &rows);
    b.pin_nets(&x, &piece_of_pin);
    b.band_wires(&x, &rows, off_pins as u16);
    let loose = b.two_leads(&x, &mut placed, &part_band, &rows, margin);
    let mut spare_row = rows.chips_end;
    b.loose_parts(&x, loose, &rows, &mut spare_row);
    let mut l = b.l;
    // the parts in their schematic's order (the list on the left reads as the schematic does)
    let pos = |r: &str| c.on.iter().position(|p| p.0 == r).unwrap_or(usize::MAX);
    let mut parts = std::mem::take(&mut l.parts);
    parts.sort_by_key(|p| {
        let chip = p.look != Look::TwoLead;
        (!chip, if chip { blocks.iter().position(|b| c.on[b.k].0 == p.refdes).unwrap_or(0) } else { pos(&p.refdes) })
    });
    l.parts = parts;
    l.homes = rows.homes;
    let gone = share_rows(&mut l, external);
    let used = l.used().into_iter().map(|(a, _)| a).chain(l.cuts.iter().copied()).fold((0u16, 0u16), |m, a| (m.0.max(a.0), m.1.max(a.1)));
    l.rows = used.0.max(spare_row.saturating_sub(gone)) + 1;
    l.cols = used.1 + 1;
    l
}


/// What one section's layout works from: the circuit, the options, who is on each net and how each net is carried.
struct Ctx<'a> {
    c: &'a Circuit,
    opt: StripOptions,
    external: &'a BTreeSet<String>,
    /// The nets in the order they first come.
    order: Vec<String>,
    members: BTreeMap<String, Members>,
    carry: BTreeMap<String, Carry>,
    /// The nets of each chip going on the copper side (`StripOptions::chips_back`): across two strips side by side, so their two
    /// nets' strips are put next to each other in their band.
    back_pairs: Vec<(String, String)>,
}

/// The other net of a two-lead part (index `k` into the parts on the board) than `n`.
fn other_net_of(c: &Circuit, k: usize, n: &str) -> Option<String> {
    c.on[k].2.iter().map(|(_, m)| m.clone()).find(|m| m != n && !m.is_empty())
}

impl<'a> Ctx<'a> {
    /// Who is on each net, and how each net is carried: a strip unless it can do without.
    fn new(c: &'a Circuit, opt: StripOptions, external: &'a BTreeSet<String>) -> Self {
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
        let on_back = |k: usize| matches!(c.mounts.get(&c.on[k].0), Some(crate::mount::Mount::Back { .. }));
        let back_pairs: Vec<(String, String)> = (0..c.on.len())
            .filter(|k| on_back(*k) && c.on[*k].2.len() == 2)
            .map(|k| (c.on[k].2[0].1.clone(), c.on[k].2[1].1.clone()))
            .filter(|(a, b)| !a.is_empty() && !b.is_empty() && a != b)
            .collect();
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
            } else if m.pins.len() == 1 && m.twos.len() == 1 && other_net_of(c, m.twos[0], n).is_some() && !on_back(m.twos[0]) {
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
            let other = other_net_of(c, k, n).unwrap();
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
        Ctx { c, opt, external, order, members, carry, back_pairs }
    }

    fn other_net(&self, k: usize, n: &str) -> Option<String> {
        other_net_of(self.c, k, n)
    }

    /// A chip going on the copper side (`StripOptions::chips_back`).
    fn on_back(&self, k: usize) -> bool {
        matches!(self.c.mounts.get(&self.c.on[k].0), Some(crate::mount::Mount::Back { .. }))
    }

    fn mount_of(&self, refdes: &str) -> Mount {
        self.c.mounts.get(refdes).cloned().unwrap_or_default()
    }

    /// A socket or an adapter board decides the shape: two rows its width apart, or one row (a SIP).
    fn look_on(&self, k: usize) -> Look {
        let (refdes, look, _) = &self.c.on[k];
        let m = self.mount_of(refdes);
        match *look {
            Look::TwoLead => Look::TwoLead,
            _ if m.width().is_some() => Look::Dip,
            _ if m.one_row() => Look::Row,
            other => other,
        }
    }

    fn to_strip(&self, n: &str) -> bool {
        matches!(self.carry.get(n), Some(Carry::Home | Carry::Via(_)))
    }

    /// A two-lead part's nets (one when both leads are on it).
    fn two_nets(&self, k: usize) -> Vec<String> {
        let mut v: Vec<String> = self.c.on[k].2.iter().map(|(_, n)| n.clone()).filter(|n| !n.is_empty()).collect();
        v.dedup();
        v
    }

    /// The strips' order in a band: the supplies first, ground last, the rest as the nets first come.
    fn rank(&self, n: &str) -> (u8, usize) {
        if let Some(i) = self.c.supplies.iter().position(|x| x == n) {
            (0, i)
        } else if is_ground_name(n) {
            (2, 0)
        } else {
            (1, self.order.iter().position(|x| x == n).unwrap_or(usize::MAX))
        }
    }

    /// Each chip by itself: its pins' rows and sides, the way of each wire and its column. (`force`: every wire one way, for a chip
    /// in a stack of two: the upper one's all up, the lower one's all down, so neither's wires pass the other)
    fn build(&self, k: usize, force: Option<Dir>) -> Block {
        let c = self.c;
        let (refdes, _, pins) = &c.on[k];
        let look = self.look_on(k);
        let mount = self.mount_of(refdes);
        // each lead's place on the socket or the adapter, by its pin number (a pin the symbol leaves out leaves its place empty)
        let (places, slot) = mount.slots(pins);
        let (half, two_cols) = if look == Look::Dip { (places.div_ceil(2), true) } else { (places, false) };
        // turned half a turn: each place goes where the place half the chip on was (pin 1 at the bottom right)
        let flip = two_cols && places % 2 == 0 && c.flipped.contains(refdes);
        // two spare holes at each piece's outer end when a pin is wired to other chip pins (two wire ends may share a piece)
        let spare = if pins.iter().any(|(_, n)| self.carry.get(n) == Some(&Carry::Pins)) { 2 } else { 0 };
        let mut placed: Vec<PinPlace> = pins
            .iter()
            .enumerate()
            .map(|(i, (_, n))| {
                let p = if flip { (slot[i] + half) % places } else { slot[i] };
                // a chip's first half down the left column, the rest up the right one (pin 1 top left)
                let (row, left) = if p < half { (p as u16, true) } else { ((places - 1 - p) as u16, false) };
                // the upper half of the rows up, the lower half down (a single row: up)
                let dir = force.unwrap_or(if half <= 1 || (row as usize) * 2 < half { Dir::Up } else { Dir::Down });
                PinPlace { row, left, wire: self.to_strip(n).then_some((dir, 0)) }
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
        let raw: Vec<Option<(Dir, u16)>> = placed.iter().map(|p| p.wire).collect();
        let width = mount.width().unwrap_or(3);
        let mut b = Block { k, look, mount, half: half as u16, pins: placed, left_pin: 0, right_pin: 0, end: 0, raw, count, spare, width, two_cols };
        b.place([0, 0], count, spare, width);
        b
    }

    /// The rows of chips: each block from the margin on, a new row past the board's width. Returns each row's blocks with their
    /// columns, and each block's rows down from the top of its row of chips (a stacked chip sits under the ones above it).
    fn rows_of_chips(&self, blocks: &mut [Block], margin: u16) -> (Vec<Vec<(usize, u16)>>, Vec<u16>) {
        let (c, opt) = (self.c, self.opt);
        let mut tiers: Vec<Vec<(usize, u16)>> = vec![];
        // with a width to keep to, the tall chips go together (a short chip beside a tall one leaves the rows under it empty), the
        // order among equal heights kept (the chips that share wires stay side by side)
        let mut seq: Vec<usize> = (0..blocks.len()).collect();
        if opt.cols.is_some() {
            seq.sort_by_key(|b| std::cmp::Reverse(blocks[*b].half));
        }
        // two chips' places swapped (`Circuit::swaps`: to keep two chips apart)
        for (x, y) in &c.swaps {
            let at = |r: &String| seq.iter().position(|b| &c.on[blocks[*b].k].0 == r);
            if let (Some(i), Some(j)) = (at(x), at(y)) {
                seq.swap(i, j);
            }
        }
        // stacking (`Circuit::stack`): a short chip goes under the chips of a column of the row when they all together are no
        // taller than the row's tallest chip (the rows under a short chip are left empty otherwise): the chips above with their
        // wires all up, the bottom one's all down, in the same columns (`stack_columns`); not two the requirements table keeps
        // apart. Into the tallest stack it fits under.
        let tallest_all = blocks.iter().map(|b| b.half).max().unwrap_or(0);
        let mut slots: Vec<Vec<Vec<usize>>> = vec![vec![]];
        let slot_w = |s: &[usize], blocks: &[Block]| s.iter().map(|b| blocks[*b].end).max().unwrap_or(0) + 1;
        for b in seq {
            if c.stack > 1 {
                let tier = slots.last().unwrap();
                let tallest = if opt.cols.is_some() { tier.first().and_then(|s| s.first()).map(|a| blocks[*a].half).unwrap_or(0) } else { tallest_all };
                let width: u16 = margin + tier.iter().map(|s| slot_w(s, blocks)).sum::<u16>();
                let rb = &c.on[blocks[b].k].0;
                let fits = |i: usize| -> Option<(u16, Vec<Block>)> {
                    let s = &tier[i];
                    let high: u16 = s.iter().map(|a| blocks[*a].half).sum();
                    let kept_apart = s.iter().any(|a| {
                        let ra = &c.on[blocks[*a].k].0;
                        c.apart.iter().any(|(x, y, _)| (x == ra && y == rb) || (x == rb && y == ra))
                    });
                    if s.is_empty() || s.len() >= c.stack || high + blocks[b].half > tallest || kept_apart {
                        return None;
                    }
                    let mut new: Vec<Block> = s.iter().map(|a| self.build(blocks[*a].k, Some(Dir::Up))).collect();
                    new.push(self.build(blocks[b].k, Some(Dir::Down)));
                    stack_columns(&mut new.iter_mut().collect::<Vec<_>>());
                    let grown = width - slot_w(s, blocks) + new.iter().map(|x| x.end).max().unwrap_or(0) + 1;
                    opt.cols.is_none_or(|w| grown <= w).then_some((high, new))
                };
                if let Some((i, (_, new))) = (0..tier.len()).filter_map(|i| fits(i).map(|f| (i, f))).max_by_key(|(i, f)| (f.0, std::cmp::Reverse(*i))) {
                    let ids: Vec<usize> = tier[i].iter().copied().chain([b]).collect();
                    for (id, blk) in ids.iter().zip(new) {
                        blocks[*id] = blk;
                    }
                    slots.last_mut().unwrap()[i].push(b);
                    continue;
                }
            }
            let tier = slots.last().unwrap();
            let width: u16 = margin + tier.iter().map(|s| slot_w(s, blocks)).sum::<u16>();
            if let Some(w) = opt.cols
                && !tier.is_empty()
                && width + blocks[b].end >= w
            {
                slots.push(vec![]);
            }
            slots.last_mut().unwrap().push(vec![b]);
        }
        // each block's column and its rows down from the top of its row of chips
        let mut drop: Vec<u16> = vec![0; blocks.len()];
        for tier in &slots {
            let mut col = margin;
            let mut row: Vec<(usize, u16)> = vec![];
            for s in tier {
                let mut d = 0;
                for b in s {
                    row.push((*b, col));
                    drop[*b] = d;
                    d += blocks[*b].half;
                }
                col += slot_w(s, blocks);
            }
            tiers.push(row);
        }
        tiers.retain(|v| !v.is_empty());
        if tiers.is_empty() {
            tiers.push(vec![]);
        }
        (tiers, drop)
    }

    /// The nets each band needs (the chip pins' wires, the parts standing in their place, the off-board wires, the nets going on to
    /// other sections), and the band of each two-lead part not standing in a pin's place.
    fn band_nets(&self, blocks: &[Block], tiers: &[Vec<(usize, u16)>]) -> (Vec<BTreeSet<String>>, BTreeMap<usize, usize>) {
        let (c, carry) = (self.c, &self.carry);
        let tier_of: BTreeMap<usize, usize> = tiers.iter().enumerate().flat_map(|(t, v)| v.iter().map(move |(b, _)| (*b, t))).collect();
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
                        band_nets[band].insert(self.other_net(*k, n).unwrap());
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
        for n in self.external.iter().filter(|n| carry.get(*n) == Some(&Carry::Home)) {
            band_nets[0].insert(n.clone());
        }
        // the two-lead parts not standing in a pin's place: into a band that has both their nets (the one with the fewest parts),
        // else one of them, else the top band
        let via_parts: BTreeSet<usize> = carry.values().filter_map(|v| if let Carry::Via(k) = v { Some(*k) } else { None }).collect();
        let mut part_band: BTreeMap<usize, usize> = BTreeMap::new();
        let mut per_band = vec![0usize; bands];
        for k in (0..c.on.len()).filter(|k| c.on[*k].1 == Look::TwoLead && !via_parts.contains(k)) {
            let nets = self.two_nets(k);
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
        (band_nets, part_band)
    }

    /// The rows: a band's strips, then a row of chips, in turn.
    fn rows(&self, band_nets: &[BTreeSet<String>], tiers: &[Vec<(usize, u16)>], blocks: &[Block], drop: &[u16]) -> Rows {
        let bands = band_nets.len();
        let mut strip_row: Vec<BTreeMap<String, u16>> = vec![BTreeMap::new(); bands];
        let mut tier_top: Vec<u16> = vec![];
        let mut homes: Vec<String> = vec![];
        let mut r: u16 = 0;
        for b in 0..bands {
            let mut nets: Vec<&String> = band_nets[b].iter().collect();
            nets.sort_by_key(|n| self.rank(n));
            let nets = pair_up(nets, &self.back_pairs, &|n: &str| is_ground_name(n));
            for n in nets {
                strip_row[b].insert(n.clone(), r);
                homes.resize(r as usize + 1, String::new());
                homes[r as usize] = n.clone();
                r += 1;
            }
            if b < tiers.len() {
                tier_top.push(r);
                r += tiers[b].iter().map(|(blk, _)| drop[*blk] + blocks[*blk].half).max().unwrap_or(0);
            }
        }
        Rows { strip_row, tier_top, homes, chips_end: r }
    }
}

/// The rows of a section's board: each band's strips (net → row), the top row of each row of chips, the net of each row (`homes`),
/// and the row past the last row of chips.
struct Rows {
    strip_row: Vec<BTreeMap<String, u16>>,
    tier_top: Vec<u16>,
    homes: Vec<String>,
    chips_end: u16,
}

/// A section's board as it is filled in: the layout, the holes taken, and the holes a wire or a standing part passes over (a wire's
/// end there would lie under it).
#[derive(Default)]
struct Board {
    l: StripLayout,
    taken: BTreeSet<At>,
    over: BTreeSet<At>,
}

/// The holes strictly between rows `a` and `b` of column `col`: what a wire or a part standing there passes over.
fn span(a: u16, b: u16, col: u16, over: &mut BTreeSet<At>) {
    for r in a.min(b) + 1..a.max(b) {
        over.insert((r, col));
    }
}

impl Board {
    /// The off-board parts' wires: one column each at the far left, into the top band.
    fn off_wires(&mut self, x: &Ctx, top: &BTreeMap<String, u16>) {
        let mut col = 0u16;
        for (refdes, pins) in &x.c.off {
            let mut ends = vec![];
            for (pin, net) in pins {
                if let Some(&r) = top.get(net).filter(|_| x.carry.get(net) == Some(&Carry::Home)) {
                    ends.push((pin.clone(), net.clone(), (r, col)));
                    self.taken.insert((r, col));
                    col += 1;
                }
            }
            self.l.off.push((refdes.clone(), ends));
        }
    }

    /// The chips: each pin on a piece of strip of its own, cut off on both sides; its wire to its net's strip in the band. Returns each
    /// chip pin's piece of strip, and the leads of the two-lead parts standing in a pin's wire's place.
    #[allow(clippy::type_complexity)]
    fn chips(&mut self, x: &Ctx, tiers: &[Vec<(usize, u16)>], blocks: &[Block], drop: &[u16], rows: &Rows) -> (BTreeMap<(String, String), Piece>, BTreeMap<usize, Vec<(String, At)>>) {
        let c = x.c;
        let (l, taken, over) = (&mut self.l, &mut self.taken, &mut self.over);
        let mut piece_of_pin: BTreeMap<(String, String), Piece> = BTreeMap::new();
        let mut placed: BTreeMap<usize, Vec<(String, At)>> = BTreeMap::new();
        for (t, tier) in tiers.iter().enumerate() {
            for &(b, start) in tier {
                let top = rows.tier_top[t] + drop[b];
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
                    match x.carry.get(net) {
                        Some(Carry::Via(k)) => {
                            // the two-lead part stands in the wire's place: from this piece to the strip of its other net
                            let other = x.other_net(*k, net).unwrap();
                            let e = strip_end(rows.strip_row[band][&other], taken);
                            let part_leads: Vec<(String, At)> = c.on[*k].2.iter().map(|(pp, n)| (pp.clone(), if n == net { (row, wire_col) } else { e })).collect();
                            for (_, a) in &part_leads {
                                taken.insert(*a);
                            }
                            if e.1 == wire_col {
                                span(row, e.0, wire_col, over);
                            }
                            placed.insert(*k, part_leads);
                        }
                        _ => {
                            let e = strip_end(rows.strip_row[band][net], taken);
                            l.links.push(Link::new((row, wire_col), e, net.clone()));
                            taken.insert((row, wire_col));
                            taken.insert(e);
                            if e.1 == wire_col {
                                span(row, e.0, wire_col, over);
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
        (piece_of_pin, placed)
    }

    /// Nets of chip pins only: wires between free holes of the pins' pieces (the shortest set that joins them).
    fn pin_nets(&mut self, x: &Ctx, piece_of_pin: &BTreeMap<(String, String), Piece>) {
        let (l, taken, over) = (&mut self.l, &mut self.taken, &self.over);
        let free_in = |piece: Piece, taken: &BTreeSet<At>, over: &BTreeSet<At>, cuts: &BTreeSet<At>| -> Option<At> {
            let (r, a, b, w) = piece;
            let mut cols: Vec<u16> = (a..=b).collect();
            cols.sort_by_key(|c| c.abs_diff(w));
            cols.into_iter().map(|c| (r, c)).find(|at| !taken.contains(at) && !over.contains(at) && !cuts.contains(at))
        };
        for n in &x.order {
            if x.carry[n] != Carry::Pins {
                continue;
            }
            let pieces: Vec<Piece> = x.members[n].pins.iter().filter_map(|p| piece_of_pin.get(p).copied()).collect();
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
                if let (Some(a), Some(b)) = (free_in(pieces[from], taken, over, &l.cuts), free_in(pieces[to], taken, over, &l.cuts)) {
                    taken.insert(a);
                    taken.insert(b);
                    l.links.push(Link::new(a, b, n.clone()));
                }
            }
        }
    }

    /// The wires joining a net's strips from band to band: down a column with nothing in it on the rows between (right of a row of
    /// chips that is not full, else past the widest), the short ones first. `first_col`: the columns left of it carry the off-board
    /// wires.
    fn band_wires(&mut self, x: &Ctx, rows: &Rows, first_col: u16) {
        let (l, taken, over) = (&mut self.l, &mut self.taken, &mut self.over);
        let mut bus: Vec<(String, u16, u16)> = vec![];
        for n in &x.order {
            let rs: Vec<u16> = rows.strip_row.iter().filter_map(|band| band.get(n).copied()).collect();
            for w in rs.windows(2) {
                bus.push((n.clone(), w[0], w[1]));
            }
        }
        bus.sort_by_key(|(_, a, b)| b - a);
        for (n, a, b) in &bus {
            // nothing ends or passes on the rows between, and neither end on a cut (passing over a cut is fine)
            let free = |cc: u16| (*a..=*b).all(|r| !taken.contains(&(r, cc)) && !over.contains(&(r, cc))) && !l.cuts.contains(&(*a, cc)) && !l.cuts.contains(&(*b, cc));
            let in_chips = |cc: u16| {
                l.parts.iter().any(|p| {
                    let (lo, hi) = (p.leads.iter().map(|x| x.1 .1).min().unwrap_or(0), p.leads.iter().map(|x| x.1 .1).max().unwrap_or(0));
                    let (top, bot) = (p.leads.iter().map(|x| x.1 .0).min().unwrap_or(0), p.leads.iter().map(|x| x.1 .0).max().unwrap_or(0));
                    cc >= lo && cc <= hi && top <= *b && bot >= *a
                })
            };
            let cc = (first_col..).find(|cc| free(*cc) && !in_chips(*cc)).unwrap();
            l.links.push(Link::new((*a, cc), (*b, cc), n.clone()));
            taken.insert((*a, cc));
            taken.insert((*b, cc));
            span(*a, *b, cc, over);
        }
    }

    /// The two-lead parts: standing across their band's two strips, in the first column free on every row between (a part's body is
    /// about a hole wide: none in the columns either side over the same rows); past the board's width when the band is full. Returns
    /// the parts that found no strip for a lead (`loose_parts`).
    fn two_leads(&mut self, x: &Ctx, placed: &mut BTreeMap<usize, Vec<(String, At)>>, part_band: &BTreeMap<usize, usize>, rows: &Rows, margin: u16) -> Vec<usize> {
        let c = x.c;
        let (l, taken, over) = (&mut self.l, &mut self.taken, &mut self.over);
        let mut bodies: BTreeSet<At> = BTreeSet::new();
        let mut loose: Vec<usize> = vec![];
        for (k, (refdes, look, pins)) in c.on.iter().enumerate().filter(|p| p.1 .1 == Look::TwoLead) {
            let mut mount = x.mount_of(refdes);
            if let Some(leads) = placed.remove(&k) {
                l.parts.push(StripPart { refdes: refdes.clone(), look: *look, leads, mount });
                continue;
            }
            let Some(&band) = part_band.get(&k) else {
                loose.push(k);
                continue;
            };
            let strip_row = &rows.strip_row[band];
            let prow: Vec<Option<u16>> = pins.iter().map(|(_, n)| strip_row.get(n).copied()).collect();
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
                .filter_map(|(_, n)| strip_row.get(n))
                .flat_map(|r| taken.iter().filter(move |a| a.0 == *r).map(|a| a.1))
                .collect();
            // the leads already on the board of the parts the requirements table keeps apart from this one, and how far (in holes)
            let others: Vec<(At, f64)> = c
                .apart
                .iter()
                .filter_map(|(x, y, d)| if x == refdes { Some((y, d)) } else if y == refdes { Some((x, d)) } else { None })
                .flat_map(|(o, d)| l.parts.iter().filter(move |p| &p.refdes == o).flat_map(move |p| p.leads.iter().map(move |(_, a)| (*a, d / 2.54))))
                .collect();
            let far = |holes: &[At]| holes.iter().all(|h| others.iter().all(|(o, d)| (h.0 as f64 - o.0 as f64).hypot(h.1 as f64 - o.1 as f64) >= *d));
            // (`holes`: where the leads go in a column; a column far enough from the parts kept apart first, then any)
            let from = |want: &dyn Fn(u16) -> bool, holes: &dyn Fn(u16) -> Vec<At>| -> u16 {
                let pick = |want: &dyn Fn(u16) -> bool| -> Option<u16> {
                    if near.is_empty() {
                        return (margin..margin + 400).find(|cc| want(*cc));
                    }
                    let mid = (near.iter().map(|x| *x as u32).sum::<u32>() / near.len() as u32) as u16;
                    (0..400u16).flat_map(|d| [mid.checked_sub(d), mid.checked_add(d)]).flatten().filter(|cc| *cc >= margin).find(|cc| want(*cc))
                };
                (!others.is_empty()).then(|| pick(&|cc| want(cc) && far(&holes(cc)))).flatten().or_else(|| pick(want)).unwrap_or_else(|| (margin..).find(|cc| want(*cc)).unwrap())
            };
            let leads: Vec<(String, At)> = match prow.as_slice() {
                // a chip on the copper side across its two strips side by side: two holes, nothing on top
                [Some(a), Some(b)] if x.on_back(k) && a.abs_diff(*b) == 1 => {
                    let free = |cc: u16| [*a, *b].iter().all(|r| !taken.contains(&(*r, cc)) && !over.contains(&(*r, cc)));
                    let cc = from(&|cc| free(cc), &|cc| vec![(*a, cc), (*b, cc)]);
                    vec![(pins[0].0.clone(), (*a, cc)), (pins[1].0.clone(), (*b, cc))]
                }
                [Some(a), Some(b)] if a != b => {
                    // a chip for the copper side whose strips are apart stands on top as the leaded part
                    if x.on_back(k) {
                        mount = crate::mount::Mount::Leaded;
                    }
                    let cc = from(&|cc| fits(&[cc], *a, *b), &|cc| vec![(*a, cc), (*b, cc)]);
                    for r in *a.min(b)..=*a.max(b) {
                        bodies.insert((r, cc));
                    }
                    span(*a, *b, cc, over);
                    vec![(pins[0].0.clone(), (*a, cc)), (pins[1].0.clone(), (*b, cc))]
                }
                // both leads on one net: lying along its strip, two holes apart
                [Some(a), Some(_)] => {
                    if x.on_back(k) {
                        mount = crate::mount::Mount::Leaded;
                    }
                    let cc = from(&|cc| fits(&[cc, cc + 1, cc + 2], *a, *a), &|cc| vec![(*a, cc), (*a, cc + 2)]);
                    for x in cc..=cc + 2 {
                        bodies.insert((*a, x));
                    }
                    vec![(pins[0].0.clone(), (*a, cc)), (pins[1].0.clone(), (*a, cc + 2))]
                }
                _ => {
                    loose.push(k);
                    continue;
                }
            };
            for (_, a) in &leads {
                taken.insert(*a);
            }
            l.parts.push(StripPart { refdes: refdes.clone(), look: *look, leads, mount });
        }
        loose
    }

    /// A part with a lead on nothing: below everything, a strip of its own for that lead (`spare_row`: the last row taken so far).
    fn loose_parts(&mut self, x: &Ctx, loose: Vec<usize>, rows: &Rows, spare_row: &mut u16) {
        let l = &mut self.l;
        let loose_col = l.used().iter().map(|(a, _)| a.1).max().unwrap_or(0) + 2;
        for (i, k) in loose.into_iter().enumerate() {
            let (refdes, look, pins) = &x.c.on[k];
            let cc = loose_col + 2 * i as u16;
            let mut leads = vec![];
            for (pin, net) in pins {
                let r = rows.strip_row.iter().find_map(|band| band.get(net).copied()).unwrap_or_else(|| {
                    *spare_row += 1;
                    *spare_row
                });
                leads.push((pin.clone(), (r, cc)));
            }
            if leads.len() == 2 && leads[0].1 == leads[1].1 {
                leads[1].1 .1 += 1;
            }
            l.parts.push(StripPart { refdes: refdes.clone(), look: *look, leads, mount: x.mount_of(refdes) });
        }
    }
}
