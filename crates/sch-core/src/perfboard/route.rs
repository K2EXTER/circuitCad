//! The perfboard's wiring (`route`): every net wired from the holes already joined to the nearest lead not yet, by the cheapest way
//! along the free holes. A step to the next free hole is a bare wire on the copper side (2 along a row or a column, 3 at 45 degrees,
//! a turn 3 more, so the wires run straight); a jump of 2 to `JUMP` holes along a row or a column, from and to a free hole, is an
//! insulated wire on top over whatever lies between (`JUMP_COST` more): where the copper is shut in, a way still goes on.
//!
//! The parts of it: `Board::read` (each hole's net, the leads, the chips' pins, a way out kept by each lead), `Board::nets` (the nets
//! in the order they are wired, ground split by section, the river wired first), `Board::keep_wires` (the wires already on the board
//! kept, `Order::keep`), `Ways::new` (where a net's wiring starts: its first lead, a Kelvin fork's lead, a bypass capacitor's IC pin,
//! or the first piece of copper kept), `Board::search` (the cheapest way), `Board::lay` (the way as wires) and `Board::join_grounds`
//! (each section's ground back to one point).

use super::*;

/// A net of two leads this many holes apart or nearer (along the rows and columns) is wired before the other nets.
const SHORT: usize = 10;
/// The longest insulated wire on top that a way may jump with (holes).
pub(super) const JUMP: i32 = 16;
/// What a jump costs on top of its length: a way along the copper is taken unless it is much longer.
pub(super) const JUMP_COST: u32 = 30;
/// What a jump costs more when it runs along a chip's (or a socket's, a header's) row of pins, over two of its pins one after the
/// other (`links_along_pins`): such a wire on top lies on the socket and is hard to fit (L9). A way round by bare wires on the copper
/// side, or a jump across the row, is taken first when it is not much longer.
/// (60 kept as many such wires on the DMM's board and made 12 more wires on top elsewhere; 30 takes off most of them for about 5.)
pub(super) const ALONG_COST: u32 = 30;

/// The steps along a row or a column.
const DIRS: [(i32, i32); 4] = [(0, 1), (1, 0), (0, -1), (-1, 0)];
/// The steps at 45 degrees (each the opposite of the one two along).
const SLANTS: [(i32, i32); 4] = [(1, 1), (1, -1), (-1, -1), (-1, 1)];
/// A state of the search is a hole and how the way came in: 0..4 a step that way along a row or a column (`DIRS`), 4..8 a step at
/// 45 degrees (`SLANTS`), `START`, or landed from a jump (`JUMPED`).
const WAYS: usize = 10;
const START: usize = 8;
const JUMPED: usize = 9;

/// How `route` goes about the nets.
pub(super) struct Order<'a> {
    /// The supplies and ground before the other nets (else the small nets first).
    pub(super) rails_first: bool,
    /// A free hole beside each lead kept for its own net (a crowded board; on a small one the kept holes are in the way).
    pub(super) escapes: bool,
    /// Nets wired before all the others, in this order (the ones a try before left apart).
    pub(super) first: &'a [String],
    /// The requirements table's Kelvin forks (its lead, and the two leads its ways go to): the two ways part at the lead.
    pub(super) forks: &'a [[At; 3]],
    /// What a jump along a chip's row of pins costs more (`ALONG_COST`; 0 in a test to compare).
    pub(super) along: u32,
    /// The bypass capacitors' leads and the IC's pin each serves (the pin, the lead): each wired from the pin first (L17).
    pub(super) bypass: &'a [(At, At)],
    /// The wires already on the board kept (`perf_route_left`): their holes are their nets', and only the nets still in two or more
    /// pieces are wired, each from one piece to the next.
    pub(super) keep: bool,
    /// The steps of the search counted here (each hole taken from the queue): the work done, the same on every computer.
    pub(super) work: &'a std::cell::Cell<u64>,
}

/// A net to wire: its id, its terms (the leads; with the wires kept, every hole of its pieces of copper) and, for ground split by
/// section, the section.
type NetToWire = (u32, Vec<At>, Option<usize>);

pub(super) fn route(c: &Circuit, l: &mut StripLayout, net_of_lead: &dyn Fn(&str, &str) -> String, order: Order) {
    let mut b = Board::read(l, net_of_lead, order.escapes);
    let (mut nets, grounds) = b.nets(c, l, &order);
    let pieces = if order.keep { b.keep_wires(l, &mut nets) } else { Pieces::default() };
    // the middles crossed by the bare wires already on the board (no net: none of another net may cross them)
    for (p, q) in &l.bare {
        for (m, sl) in crate::stripboard::bare_crossings(*p, *q) {
            b.middles.insert(m, (u32::MAX, sl));
        }
    }
    let mut ground_trees: BTreeMap<usize, Vec<At>> = BTreeMap::new();
    for n_at in 0..nets.len() {
        let tree = b.wire_net(l, &nets, n_at, &order, &pieces);
        if let Some(k) = nets[n_at].2 {
            ground_trees.insert(k, tree);
        }
    }
    b.join_grounds(l, ground_trees, &grounds);
}

/// The wires kept (`Order::keep`): each piece of copper (the holes joined by the wires, with the leads on them) of a net, by number,
/// and the piece each of its holes is in.
#[derive(Default)]
struct Pieces {
    of: BTreeMap<At, usize>,
    holes: BTreeMap<usize, Vec<At>>,
}

/// The board as the wiring sees it, hole by hole (`idx`): each hole's net, the leads, the chips' pins, the holes kept as ways out, the
/// wires' ends on top and the middles the bare wires at 45 degrees cross.
struct Board {
    rows: usize,
    cols: usize,
    /// The nets' names by id (a lead on no net has one of its own, "\0" and a number).
    names: Vec<String>,
    /// Each net's id by its name.
    ids: std::collections::HashMap<String, u32>,
    /// Each hole's net (copper or a lead).
    owner: Vec<Option<u32>>,
    /// Each lead's hole and net (the parts' leads, then the off-board wires).
    leads: Vec<(At, u32)>,
    lead_holes: BTreeSet<At>,
    /// Each hole's chip (or row of leads) when it is one of its pins: a jump over two pins of one part in a row runs along its pins.
    chip_of: Vec<u32>,
    /// One free hole beside each lead kept for its own net (`Order::escapes`).
    escape: BTreeMap<At, u32>,
    /// The holes where a wire on top ends (no other wire's end, no jump from there).
    link_ends: BTreeSet<At>,
    /// The middles of the squares of four holes a bare wire at 45 degrees crosses (doubled coordinates), its net and its slant: a
    /// wire of another net slanting the other way there would touch it between the holes (those already on the board: no net).
    middles: BTreeMap<(i32, i32), (u32, bool)>,
    /// The sections' boxes (with ground split by section, each section's ground is wired inside its box).
    sections: Vec<(String, At, At)>,
}

impl Board {
    fn idx(&self, a: At) -> usize {
        a.0 as usize * self.cols + a.1 as usize
    }

    fn hole(&self, h: usize) -> At {
        ((h / self.cols) as u16, (h % self.cols) as u16)
    }

    fn on_board(&self, r: i32, c: i32) -> bool {
        r >= 0 && c >= 0 && (r as usize) < self.rows && (c as usize) < self.cols
    }

    /// The holes beside `a` along a row or a column, on the board.
    fn beside(&self, a: At) -> Vec<At> {
        DIRS.iter().map(|(dr, dc)| (a.0 as i32 + dr, a.1 as i32 + dc)).filter(|(r, c)| self.on_board(*r, *c)).map(|(r, c)| (r as u16, c as u16)).collect()
    }

    /// Whether hole `a` is in section `k`'s box.
    fn inside(&self, k: usize, a: At) -> bool {
        let (_, p, q) = &self.sections[k];
        a.0 >= p.0 && a.0 <= q.0 && a.1 >= p.1 && a.1 <= q.1
    }

    /// A net's id by its name, added when new.
    fn id(&mut self, n: &str) -> u32 {
        if let Some(i) = self.ids.get(n) {
            return *i;
        }
        self.names.push(n.to_string());
        let i = (self.names.len() - 1) as u32;
        self.ids.insert(n.to_string(), i);
        i
    }

    /// The board read from the layout: each lead's hole taken by its net (a lead on no net keeps its hole to itself), the holes kept
    /// free for the mounting holes taken by no net that is wired, the chips' pins, and with `escapes` a way out kept by each lead.
    fn read(l: &StripLayout, net_of_lead: &dyn Fn(&str, &str) -> String, escapes: bool) -> Board {
        let (rows, cols) = (l.rows as usize, l.cols as usize);
        let mut b = Board {
            rows,
            cols,
            names: vec![],
            ids: Default::default(),
            owner: vec![None; rows * cols],
            leads: vec![],
            lead_holes: BTreeSet::new(),
            chip_of: vec![u32::MAX; rows * cols],
            escape: BTreeMap::new(),
            link_ends: BTreeSet::new(),
            middles: BTreeMap::new(),
            sections: l.sections.clone(),
        };
        let mut lone = 0;
        for p in &l.parts {
            for (pin, a) in &p.leads {
                let n = net_of_lead(&p.refdes, pin);
                let n = if n.is_empty() {
                    lone += 1;
                    format!("\u{0}{lone}")
                } else {
                    n
                };
                let i = b.id(&n);
                let k = b.idx(*a);
                b.owner[k] = Some(i);
                b.leads.push((*a, i));
            }
        }
        for (_, v) in &l.off {
            for (_, n, a) in v {
                let i = b.id(n);
                let k = b.idx(*a);
                b.owner[k] = Some(i);
                b.leads.push((*a, i));
            }
        }
        let keep_free = b.id("\u{0}keepout");
        for a in l.keepout.iter().filter(|a| (a.0 as usize) < rows && (a.1 as usize) < cols) {
            let k = b.idx(*a);
            if b.owner[k].is_none() {
                b.owner[k] = Some(keep_free);
            }
        }
        b.lead_holes = b.leads.iter().map(|x| x.0).collect();
        for (k, p) in l.parts.iter().enumerate().filter(|(_, p)| p.look != Look::TwoLead) {
            for (_, a) in &p.leads {
                if (a.0 as usize) < rows && (a.1 as usize) < cols {
                    let h = b.idx(*a);
                    b.chip_of[h] = k as u32;
                }
            }
        }
        if escapes {
            b.escapes();
        }
        b
    }

    /// One free hole beside each lead kept for its own net: a way out from the lead (a jump starts from a hole, not a lead), however
    /// the wires of the other nets crowd round it; the hole with the most free holes round it, the leads with the fewest ways out first.
    fn escapes(&mut self) {
        let mut by_room: Vec<(At, u32)> = self.leads.clone();
        by_room.sort_by_key(|(a, _)| self.beside(*a).iter().filter(|h| !self.lead_holes.contains(h)).count());
        for (a, net) in by_room {
            let free = |b: &Board, h: &At| !b.lead_holes.contains(h) && !b.escape.contains_key(h);
            let room = |h: At| self.beside(h).iter().filter(|x| free(self, x)).count();
            if let Some(h) = self.beside(a).into_iter().filter(|h| free(self, h)).max_by_key(|h| room(*h)) {
                self.escape.insert(h, net);
            }
        }
    }

    /// The nets to wire, in the order they are wired, and the pieces of ground of the sections (ground split by section). With sections
    /// (not with the wires kept), ground is wired within each section, and each section's ground goes back to the first's by a wire of
    /// its own (`join_grounds`: one section's return current does not flow through another's ground). The river (a long header's
    /// two-lead nets to the row of a chip's pins under it, not with the wires kept) is wired here, first. Then the nets a try before
    /// left apart (`Order::first`), a net of two leads near each other (a short bare wire, wired before the others cut across it: a
    /// header's pin to the chip under it would else be jumped by a wire on top), the rails first or last, the fewer leads and the
    /// smaller first.
    fn nets(&mut self, c: &Circuit, l: &mut StripLayout, order: &Order) -> (Vec<NetToWire>, Vec<NetToWire>) {
        let mut nets: Vec<NetToWire> = vec![];
        for (a, i) in &self.leads {
            match nets.iter_mut().find(|x| x.0 == *i) {
                Some(x) => x.1.push(*a),
                None => nets.push((*i, vec![*a], None)),
            }
        }
        if !self.sections.is_empty() && !order.keep {
            let mut split = vec![];
            for (i, v, _) in std::mem::take(&mut nets) {
                if is_ground_name(&self.names[i as usize]) {
                    for k in 0..self.sections.len() {
                        let part: Vec<At> = v.iter().copied().filter(|a| self.inside(k, *a)).collect();
                        if !part.is_empty() {
                            split.push((i, part, Some(k)));
                        }
                    }
                } else {
                    split.push((i, v, None));
                }
            }
            nets = split;
        }
        let grounds: Vec<NetToWire> = nets.iter().filter(|x| x.2.is_some()).cloned().collect();
        nets.retain(|(_, v, _)| v.len() > 1);
        let river_nets = if order.keep { vec![] } else { river(l, &self.leads, &self.owner, &self.escape) };
        for (net, segs) in river_nets {
            for (a, b) in &segs {
                for h in along(*a, *b) {
                    let k = self.idx(h);
                    self.owner[k] = Some(net);
                }
                l.bare.push((*a, *b));
            }
            nets.retain(|x| x.0 != net);
        }
        let rail = |i: u32| is_ground_name(&self.names[i as usize]) || c.supplies.contains(&self.names[i as usize]);
        let short = |v: &[At]| v.len() == 2 && (v[0].0.abs_diff(v[1].0) + v[0].1.abs_diff(v[1].1)) as usize <= SHORT;
        nets.sort_by_key(|(i, v, _)| {
            let (r0, r1) = (v.iter().map(|a| a.0).min().unwrap(), v.iter().map(|a| a.0).max().unwrap());
            let (c0, c1) = (v.iter().map(|a| a.1).min().unwrap(), v.iter().map(|a| a.1).max().unwrap());
            let ahead = order.first.iter().position(|n| *n == self.names[*i as usize]).unwrap_or(usize::MAX);
            (ahead, !short(v), rail(*i) != order.rails_first, v.len(), (r1 - r0) + (c1 - c0))
        });
        (nets, grounds)
    }

    /// The wires kept (`Order::keep`): each piece of copper (the holes joined by the wires, with the leads on them) belongs to the net
    /// of a lead on it, its holes taken by that net (a piece with no lead on it is taken by no net that is wired); a net already in one
    /// piece is left alone, one in two or more is wired from piece to piece (its terms: every hole of its pieces).
    fn keep_wires(&mut self, l: &StripLayout, nets: &mut Vec<NetToWire>) -> Pieces {
        let groups = l.hole_groups();
        let mut net_of_group: BTreeMap<usize, u32> = BTreeMap::new();
        for (a, i) in &self.leads {
            if let Some(g) = groups.get(a) {
                net_of_group.entry(*g).or_insert(*i);
            }
        }
        let wired = self.id("\u{0}wired");
        let mut copper: Vec<(At, usize)> = l.bare.iter().flat_map(|(a, b)| crate::stripboard::bare_holes(*a, *b).into_iter().map(|h| (h, groups.get(a).copied().unwrap_or(usize::MAX)))).collect();
        for k in &l.links {
            for e in [k.a, k.b] {
                copper.push((e, groups.get(&e).copied().unwrap_or(usize::MAX)));
                self.link_ends.insert(e);
            }
        }
        let mut pieces = Pieces::default();
        for (h, g) in copper.into_iter().filter(|(h, _)| (h.0 as usize) < self.rows && (h.1 as usize) < self.cols) {
            let k = self.idx(h);
            self.owner[k] = Some(net_of_group.get(&g).copied().unwrap_or(wired));
            if net_of_group.contains_key(&g) {
                pieces.holes.entry(g).or_default().push(h);
            }
        }
        for (a, _) in &self.leads {
            if let Some(g) = groups.get(a) {
                pieces.holes.entry(*g).or_default().push(*a);
            }
        }
        for (g, hs) in &mut pieces.holes {
            hs.sort();
            hs.dedup();
            for h in hs.iter() {
                pieces.of.insert(*h, *g);
            }
        }
        for (_, terms, _) in nets.iter_mut() {
            let mut gs: Vec<usize> = terms.iter().filter_map(|a| groups.get(a).copied()).collect();
            gs.sort();
            gs.dedup();
            *terms = if gs.len() < 2 { vec![] } else { gs.iter().flat_map(|g| pieces.holes[g].iter().copied()).collect() };
        }
        nets.retain(|(_, v, _)| !v.is_empty());
        pieces
    }

    /// The free holes beside the leads of the nets still to wire (`later`), when a lead has two or fewer: kept for that net (another
    /// net's wire there would shut the lead in).
    fn kept_for_later(&self, later: &[NetToWire]) -> BTreeMap<At, u32> {
        let mut kept: BTreeMap<At, u32> = BTreeMap::new();
        for (other, ts, _) in later {
            for t in ts {
                let free: Vec<At> = self.beside(*t).into_iter().filter(|h| self.owner[self.idx(*h)].is_none() && !self.link_ends.contains(h)).collect();
                if free.len() <= 2 {
                    for h in free {
                        kept.entry(h).or_insert(*other);
                    }
                }
            }
        }
        kept
    }

    /// Wires net `nets[n_at]` (the nets after it keep the holes their leads need, `kept_for_later`): from where it starts
    /// (`Ways::new`), the cheapest way (`search`) to a term not joined yet, laid as wires (`lay`), until every term is joined or no way
    /// is left (the check then names the net left apart). Returns the holes joined (its tree).
    fn wire_net(&mut self, l: &mut StripLayout, nets: &[NetToWire], n_at: usize, order: &Order, pieces: &Pieces) -> Vec<At> {
        let (net, ref terms, bx) = nets[n_at];
        let kept = self.kept_for_later(&nets[n_at + 1..]);
        let mut ways = Ways::new(terms, order, pieces);
        // holes this net's ways keep off: where a way would end two wires on top in one hole
        let mut avoid: BTreeSet<At> = BTreeSet::new();
        let mut tries = 0;
        while !ways.left.is_empty() {
            ways.drop_joined();
            let target = ways.target();
            let starts = ways.starts(target);
            // where this net may go (the fork's second way keeps off the first's copper)
            let open: Vec<bool> = (0..self.rows * self.cols)
                .map(|h| {
                    let a = self.hole(h);
                    self.owner[h].is_none_or(|o| o == net)
                        && !self.link_ends.contains(&a)
                        && !avoid.contains(&a)
                        && kept.get(&a).is_none_or(|o| *o == net)
                        && self.escape.get(&a).is_none_or(|o| *o == net)
                        && bx.is_none_or(|k| self.inside(k, a))
                        && !(target.is_some() && ways.first_way.contains(&a))
                })
                .collect();
            let ends: Vec<At> = match target {
                Some(t) => vec![t],
                None => ways.left.iter().copied().collect(),
            };
            let Some(mut path) = self.search(net, &starts, &ends, &open, order) else {
                // a fork's way not found: the net is wired as any other (the rules then say how far its ways run together)
                if target.is_some() {
                    ways.forced.clear();
                    ways.forced_from.clear();
                    ways.first_way.clear();
                    continue;
                }
                // shut in on every side: the check names the net left apart
                break;
            };
            // two wires on top ending in one hole (a way that comes back to where it jumped): that hole kept off, and the way found again
            let mut link_ends: Vec<At> = path.windows(2).filter(|w| jump(w[0], w[1])).flat_map(|w| [w[0], w[1]]).collect();
            link_ends.sort();
            if let Some(w) = link_ends.windows(2).find(|w| w[0] == w[1]) {
                tries += 1;
                if tries > 8 {
                    break;
                }
                avoid.insert(w[0]);
                continue;
            }
            // the jumps of one way made one wire on top, from where the first starts to where the last lands (a wire on top goes
            // straight, slanting if need be; the holes between are left free)
            let jumps: Vec<usize> = (1..path.len()).filter(|i| jump(path[i - 1], path[*i])).collect();
            if jumps.len() > 1 {
                let (i0, i1) = (jumps[0], *jumps.last().unwrap());
                path.drain(i0..i1);
            }
            self.lay(l, net, &path);
            ways.joined(&path, target, order.keep, pieces);
        }
        ways.tree
    }

    /// The cheapest way for `net` from any of `starts` to any of `ends` through the `open` holes: the holes one after the other (a
    /// step to the next hole, or a jump over what is in the way). None when there is none.
    fn search(&self, net: u32, starts: &[At], ends: &[At], open: &[bool], order: &Order) -> Option<Vec<At>> {
        let n = self.rows * self.cols;
        let mut dist: Vec<u32> = vec![u32::MAX; n * WAYS];
        let mut prev: Vec<u32> = vec![u32::MAX; n * WAYS];
        let mut heap = BinaryHeap::new();
        for t in starts {
            let s = self.idx(*t) * WAYS + START;
            dist[s] = 0;
            heap.push(Reverse((0u32, s as u32)));
        }
        let mut is_lead = vec![false; n];
        let mut is_end = vec![false; n];
        let mut is_left = vec![false; n];
        for a in &self.lead_holes {
            is_lead[self.idx(*a)] = true;
        }
        for a in &self.link_ends {
            is_end[self.idx(*a)] = true;
        }
        for a in ends {
            is_left[self.idx(*a)] = true;
        }
        let mut found = None;
        while let Some(Reverse((d, s))) = heap.pop() {
            order.work.set(order.work.get() + 1);
            let s = s as usize;
            if d > dist[s] {
                continue;
            }
            let (h, way) = (s / WAYS, s % WAYS);
            let at = self.hole(h);
            if is_left[h] {
                found = Some(s);
                break;
            }
            // a lead is an end, not a hole to pass through on the way to another (its wire would run under the part)
            if way != START && is_lead[h] {
                continue;
            }
            let mut push = |to: At, w: usize, nd: u32, heap: &mut BinaryHeap<Reverse<(u32, u32)>>| {
                let ns = self.idx(to) * WAYS + w;
                if nd < dist[ns] {
                    dist[ns] = nd;
                    prev[ns] = s as u32;
                    heap.push(Reverse((nd, ns as u32)));
                }
            };
            // the jumps: from a free hole (not a lead, not a wire's end), along the row or the column, to a free hole
            let can_jump = way != JUMPED && !is_lead[h] && !is_end[h];
            // the steps at 45 degrees (B12): 3 a step (about 1.4 times a step along a row), a turn 3 more as along a row; not
            // across a wire of another net slanting the other way between the same four holes (they would touch)
            for (k, (dr, dc)) in SLANTS.iter().enumerate() {
                let w = 4 + k;
                let (r, cc) = (at.0 as i32 + dr, at.1 as i32 + dc);
                if !self.on_board(r, cc) {
                    continue;
                }
                let to = (r as u16, cc as u16);
                let mid = (at.0 as i32 + r, at.1 as i32 + cc);
                let slant = dr.signum() == dc.signum();
                let touches = self.middles.get(&mid).is_some_and(|(n, sl)| *n != net && *sl != slant);
                let back = (4..START).contains(&way) && 4 + (way - 4 + 2) % 4 == w;
                if open[self.idx(to)] && !touches && !back {
                    let turn = way < START && way != w;
                    push(to, w, d + 3 + if turn { 3 } else { 0 }, &mut heap);
                }
            }
            for (w, (dr, dc)) in DIRS.iter().enumerate() {
                let (r, cc) = (at.0 as i32 + dr, at.1 as i32 + dc);
                if self.on_board(r, cc) {
                    let to = (r as u16, cc as u16);
                    // no going straight back (a way back to where it jumped from could jump again from the same hole)
                    if open[self.idx(to)] && !(way < 4 && (way + 2) % 4 == w) {
                        let turn = way < START && way != w;
                        push(to, w, d + 2 + if turn { 3 } else { 0 }, &mut heap);
                    }
                }
                if !can_jump {
                    continue;
                }
                // over something in the way, to the first free hole past it (a jump over free holes is no better than steps, and
                // past the first free hole the steps cost what a longer jump would)
                let mut over = false;
                // whether the jump has gone over two pins of one chip one after the other (along its row of pins)
                let (mut last_chip, mut along) = (u32::MAX, false);
                for k in 1..=JUMP {
                    let (r, cc) = (at.0 as i32 + dr * k, at.1 as i32 + dc * k);
                    if !self.on_board(r, cc) {
                        break;
                    }
                    let to = self.idx((r as u16, cc as u16));
                    let free = open[to] && !is_lead[to];
                    if k >= 2 && over && free {
                        push((r as u16, cc as u16), JUMPED, d + JUMP_COST + 2 * k as u32 + if along { order.along } else { 0 }, &mut heap);
                        break;
                    }
                    along |= self.chip_of[to] != u32::MAX && self.chip_of[to] == last_chip;
                    last_chip = self.chip_of[to];
                    over |= !free;
                }
            }
        }
        let mut s = found?;
        let mut path: Vec<At> = vec![];
        loop {
            path.push(self.hole(s / WAYS));
            if prev[s] == u32::MAX {
                break;
            }
            s = prev[s] as usize;
        }
        path.reverse();
        Some(path)
    }

    /// A way of `net` laid on the board: its holes taken, each straight run of steps (along a row, a column or at 45 degrees) a bare
    /// wire (the middles it crosses at 45 degrees noted), each jump an insulated wire on top (its ends noted).
    fn lay(&mut self, l: &mut StripLayout, net: u32, path: &[At]) {
        for a in path {
            let k = self.idx(*a);
            self.owner[k] = Some(net);
        }
        let step = |a: At, b: At| (b.0 as i32 - a.0 as i32, b.1 as i32 - a.1 as i32);
        let mut start = 0;
        for i in 1..path.len() {
            let (a, b) = (path[i - 1], path[i]);
            if jump(a, b) {
                if start < i - 1 {
                    l.bare.push((path[start], a));
                }
                l.links.push(Link::new(a, b, self.names[net as usize].clone()));
                self.link_ends.insert(a);
                self.link_ends.insert(b);
                start = i;
                continue;
            }
            let next_turns = i + 1 < path.len() && {
                let n = path[i + 1];
                jump(b, n) || step(b, n) != step(a, b)
            };
            if next_turns || i + 1 == path.len() {
                l.bare.push((path[start], b));
                for (m, sl) in crate::stripboard::bare_crossings(path[start], b) {
                    self.middles.insert(m, (net, sl));
                }
                start = i;
            }
        }
    }

    /// Each section's ground back to the first section's: one wire on top between the nearest two holes of their copper (a hole
    /// beside a lead when a section's ground is one lead, joined to it by a bare wire).
    fn join_grounds(&mut self, l: &mut StripLayout, mut ground_trees: BTreeMap<usize, Vec<At>>, grounds: &[NetToWire]) {
        for (_, v, k) in grounds {
            if let Some(k) = k {
                ground_trees.entry(*k).or_insert_with(|| vec![v[0]]);
            }
        }
        let Some((&hub, _)) = ground_trees.iter().next() else { return };
        let gnd = grounds.first().map(|g| g.0);
        let hub_tree = ground_trees[&hub].clone();
        for (&k, tree) in ground_trees.iter().filter(|(k, _)| **k != hub) {
            let toward = hub_tree[0];
            let Some(a) = self.ground_end(l, tree, k, gnd, toward) else { continue };
            let Some(b) = self.ground_end(l, &hub_tree, hub, gnd, a) else { continue };
            self.link_ends.insert(a);
            self.link_ends.insert(b);
            l.links.push(Link::new(a, b, gnd.map(|g| self.names[g as usize].clone()).unwrap_or_default()));
        }
    }

    /// Where a section's ground wire ends: the hole of its copper (`tree`, in section `k`) nearest `toward` that is no lead and no
    /// wire's end; with a lead only, a free hole beside it, joined by a bare wire.
    fn ground_end(&mut self, l: &mut StripLayout, tree: &[At], k: usize, gnd: Option<u32>, toward: At) -> Option<At> {
        let ok = |b: &Board, h: &At| !b.lead_holes.contains(h) && !b.link_ends.contains(h) && b.inside(k, *h);
        let dist = |h: &At| h.0.abs_diff(toward.0) as u32 + h.1.abs_diff(toward.1) as u32;
        if let Some(h) = tree.iter().filter(|h| ok(self, h)).min_by_key(|h| dist(h)) {
            return Some(*h);
        }
        let (lead, h) = tree.iter().flat_map(|a| self.beside(*a).into_iter().map(move |h| (*a, h))).filter(|(_, h)| ok(self, h) && self.owner[self.idx(*h)].is_none()).min_by_key(|(_, h)| dist(h))?;
        let i = self.idx(h);
        self.owner[i] = gnd;
        l.bare.push((lead, h));
        Some(h)
    }
}

/// Whether two holes one after the other on a way are a jump (a step is one hole along a row, a column or at 45 degrees).
fn jump(a: At, b: At) -> bool {
    a.0.abs_diff(b.0).max(a.1.abs_diff(b.1)) > 1
}

/// One net's wiring as it goes: the holes joined (`tree`), the terms not yet (`left`), and the ways to draw first (`forced`, each
/// from `forced_from` when given): a Kelvin fork's two ways, each from the fork's lead itself, the second keeping off the first's
/// copper (`first_way`: so they part at the lead, the current's way and the sense's sharing no copper); a bypass capacitor's leads,
/// each from its IC's pin (the pin joined first), so the capacitor's current goes to the pin and not round the rest of the net.
struct Ways {
    tree: Vec<At>,
    left: BTreeSet<At>,
    fork: Option<[At; 3]>,
    forced: Vec<At>,
    forced_from: Vec<Option<At>>,
    first_way: BTreeSet<At>,
}

impl Ways {
    /// Where the net starts: with the wires kept, its first piece of copper (to any hole of the others); else a Kelvin fork's lead,
    /// or a bypass capacitor's IC pin, or its first term.
    fn new(terms: &[At], order: &Order, pieces: &Pieces) -> Ways {
        let mut w = Ways { tree: vec![terms[0]], left: terms[1..].iter().copied().collect(), fork: None, forced: vec![], forced_from: vec![], first_way: BTreeSet::new() };
        w.fork = order.forks.iter().find(|f| f.iter().all(|a| terms.contains(a))).copied();
        if order.keep {
            let g0 = pieces.of[&terms[0]];
            w.tree = pieces.holes[&g0].clone();
            w.left = terms.iter().copied().filter(|t| pieces.of.get(t) != Some(&g0)).collect();
        } else if let Some([st, a, b]) = w.fork {
            w.tree = vec![st];
            w.left = terms.iter().copied().filter(|t| *t != st).collect();
            w.forced = vec![a, b];
        } else {
            let pairs: Vec<(At, At)> = order.bypass.iter().copied().filter(|(p, q)| terms.contains(p) && terms.contains(q)).collect();
            if let Some((p0, _)) = pairs.first() {
                w.tree = vec![*p0];
                w.left = terms.iter().copied().filter(|t| t != p0).collect();
                for (p, q) in pairs {
                    if !w.tree.contains(&p) && !w.forced.contains(&p) {
                        w.forced.push(p);
                        w.forced_from.push(None);
                    }
                    w.forced.push(q);
                    w.forced_from.push(Some(p));
                }
            }
        }
        w
    }

    /// A bypass way whose end is joined already: dropped.
    fn drop_joined(&mut self) {
        while self.fork.is_none() && self.forced.first().is_some_and(|t| !self.left.contains(t)) {
            self.forced.remove(0);
            self.forced_from.remove(0);
        }
    }

    /// The term the next way must reach (the first forced way's), or None for any term left.
    fn target(&self) -> Option<At> {
        self.forced.first().copied().filter(|t| self.left.contains(t))
    }

    /// Where the next way starts: a fork's lead for its ways, a bypass capacitor's IC pin when joined already, else the whole tree.
    fn starts(&self, target: Option<At>) -> Vec<At> {
        match (target, self.fork) {
            (Some(_), Some([st, _, _])) => vec![st],
            (Some(_), None) => self.forced_from.first().copied().flatten().filter(|a| self.tree.contains(a)).map_or_else(|| self.tree.clone(), |a| vec![a]),
            _ => self.tree.clone(),
        }
    }

    /// A way laid: its end joined (with the wires kept, the whole piece of copper it reached), the forced way done (a fork's first
    /// way's copper, but the lead, kept off by the second), its holes added to the tree.
    fn joined(&mut self, path: &[At], target: Option<At>, keep: bool, pieces: &Pieces) {
        let end = *path.last().unwrap();
        self.left.remove(&end);
        if let Some(g) = pieces.of.get(&end).copied().filter(|_| keep) {
            self.left.retain(|h| pieces.of.get(h) != Some(&g));
            self.tree.extend(pieces.holes[&g].iter().copied());
        }
        if target.is_some() {
            if self.forced.len() == 2 && self.fork.is_some() {
                self.first_way = path.iter().copied().filter(|a| Some(*a) != self.fork.map(|f| f[0])).collect();
            }
            self.forced.remove(0);
            if !self.forced_from.is_empty() {
                self.forced_from.remove(0);
            }
        }
        self.tree.extend(path.iter().copied());
    }
}
