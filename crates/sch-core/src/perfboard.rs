//! A layout made for a perfboard (B5a): no strips to share, so the parts go where they are nearest what they join and each net is
//! wired by bare wires on the copper side, straight along the rows and the columns, never touching another net's (the ones that find
//! no way get an insulated wire on top instead). The stripboard's layout (`stripboard::strip_layout_with`) alternates bands of
//! strips with rows of chips, every chip pin wired up or down to a band: on a perfboard that leaves most of the board empty (the DMM's
//! B stage: 60 × 78 holes, 42 of the rows the chips' own).
//!
//! The result is a `StripLayout` with `perf` set: every hole a piece of its own, joined only by the bare wires and the links, so the
//! check, the ratsnest, the forks and the printed drawing read it as they read any other.

use crate::breadboard::{is_ground_name, read, Circuit, Look};
use crate::stripboard::{At, Link, StripLayout, StripOptions, StripPart};
use crate::Schematic;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

/// Free holes left between two chips (and two rows of chips) for the wires: the fewest first, more when the nets do not all find a
/// way.
const GAPS: [u16; 2] = [2, 3];
/// A two-lead part's leads: this many holes apart (one hole under its body between).
const SPAN: u16 = 2;
/// A row of leads this long or longer (a header) goes along the top edge of the board.
const LONG_ROW: usize = 6;

/// A chip or a row of leads by itself: each lead's pin, net and place from its top-left corner, and its height and width in holes.
struct Shape {
    k: usize,
    look: Look,
    leads: Vec<(String, String, At)>,
    h: u16,
    w: u16,
}

/// How a part goes on the board: a socket or an adapter decides (two rows its width apart, or one row), else its own look.
fn look_on(c: &Circuit, k: usize) -> Look {
    let (refdes, look, _) = &c.on[k];
    let m = c.mounts.get(refdes).cloned().unwrap_or_default();
    match *look {
        Look::TwoLead => Look::TwoLead,
        _ if m.width().is_some() => Look::Dip,
        _ if m.one_row() => Look::Row,
        other => other,
    }
}

fn shape(c: &Circuit, k: usize) -> Shape {
    let (refdes, _, pins) = &c.on[k];
    let look = look_on(c, k);
    let mount = c.mounts.get(refdes).cloned().unwrap_or_default();
    let (places, slot) = mount.slots(pins);
    let places = places.max(pins.len()).max(slot.iter().map(|p| p + 1).max().unwrap_or(0));
    if look == Look::Dip {
        // pin 1 top left, down the left column and up the right one
        let half = places.div_ceil(2);
        let width = mount.width().unwrap_or(3);
        let leads = pins
            .iter()
            .zip(&slot)
            .map(|((pin, net), &p)| (pin.clone(), net.clone(), if p < half { (p as u16, 0) } else { ((places - 1 - p) as u16, width) }))
            .collect();
        Shape { k, look, leads, h: half as u16, w: width + 1 }
    } else {
        // a row of leads lies along a row of the board (a header along the edge, a transistor's three legs)
        let leads = pins.iter().zip(&slot).map(|((pin, net), &p)| (pin.clone(), net.clone(), (0, p as u16))).collect();
        Shape { k, look, leads, h: 1, w: places as u16 }
    }
}

/// Lays the schematic out on a perfboard: the headers along the top edge, the chips in rows within the board's width (`opt.cols`,
/// else about square) with room between for the wires, each two-lead part where its leads are nearest the leads of their nets, then
/// every net wired.
pub fn perf_layout_with(s: &Schematic, opt: StripOptions) -> StripLayout {
    let c = read(s);
    let net_of_lead = |refdes: &str, pin: &str| lead_net(&c, refdes, pin);
    // the rails first and the small nets first, without and then with a way out kept by each lead, each again with the nets left
    // apart wired first (a few rounds), until one joins every net: else the one with the fewest nets left apart, then the fewest
    // wires on top
    let mut best: Option<(usize, usize, StripLayout)> = None;
    let start = std::time::Instant::now();
    let done = |best: &Option<(usize, usize, StripLayout)>| best.as_ref().is_some_and(|b| b.0 == 0) || start.elapsed() > BUDGET;
    for gap in GAPS {
        if done(&best) {
            break;
        }
        let l = place(&c, opt, gap);
        for (rails_first, escapes) in [(true, false), (false, false), (true, true), (false, true)] {
            if done(&best) {
                break;
            }
            let mut first: Vec<String> = vec![];
            for _ in 0..ROUNDS {
                if best.is_some() && start.elapsed() > BUDGET {
                    break;
                }
                let mut m = l.clone();
                route(&c, &mut m, &net_of_lead, Order { rails_first, escapes, first: &first });
                let apart = crate::stripboard::strip_unjoined(s, &m);
                let key = (apart.len(), m.links.len());
                if best.as_ref().is_none_or(|b| key < (b.0, b.1)) {
                    best = Some((key.0, key.1, m));
                }
                if apart.is_empty() {
                    break;
                }
                for (n, _, _) in apart {
                    if !first.contains(&n) {
                        first.insert(0, n);
                    }
                }
            }
        }
    }
    best.map(|b| b.2).unwrap_or_default()
}

/// The net of a part's lead ("" when none).
fn lead_net(c: &Circuit, refdes: &str, pin: &str) -> String {
    c.on.iter().find(|x| x.0 == refdes).and_then(|x| x.2.iter().find(|(p, _)| p == pin)).map(|x| x.1.clone()).unwrap_or_default()
}

/// The parts placed (`gap` free holes between chips), not yet wired.
fn place(c: &Circuit, opt: StripOptions, gap: u16) -> StripLayout {
    let off_pins: Vec<(usize, usize)> = c.off.iter().enumerate().flat_map(|(i, (_, p))| p.iter().enumerate().filter(|(_, (_, n))| !n.is_empty()).map(move |(j, _)| (i, j))).collect();
    let margin: u16 = if off_pins.is_empty() { 1 } else { 2 };
    let mut shapes: Vec<Shape> = (0..c.on.len()).filter(|k| look_on(c, *k) != Look::TwoLead).map(|k| shape(c, k)).collect();
    let twos: Vec<usize> = (0..c.on.len()).filter(|k| look_on(c, *k) == Look::TwoLead).collect();
    // the board's width: as asked, else about square for what goes on it
    let area: u32 = shapes.iter().map(|s| (s.h + gap) as u32 * (s.w + gap) as u32).sum::<u32>() + twos.len() as u32 * 12;
    let widest = shapes.iter().map(|s| s.w).max().unwrap_or(0) + 2 * margin;
    let width = opt.cols.unwrap_or_else(|| ((area as f64 * 1.3).sqrt().ceil() as u16).max(widest)).max(widest).max(8);
    // the order: the headers first (the top edge), then each chip next to the ones it shares the most nets with
    let nets_of = |s: &Shape| -> BTreeSet<String> { s.leads.iter().map(|x| x.1.clone()).filter(|n| !n.is_empty() && !is_ground_name(n) && !c.supplies.contains(n)).collect() };
    let (mut heads, mut rest): (Vec<Shape>, Vec<Shape>) = shapes.drain(..).partition(|s| s.look != Look::Dip && s.leads.len() >= LONG_ROW);
    heads.sort_by_key(|s| Reverse(s.w));
    let mut order: Vec<Shape> = heads;
    let mut seen: BTreeSet<String> = order.iter().flat_map(&nets_of).collect();
    rest.sort_by_key(|s| Reverse(s.leads.len()));
    while !rest.is_empty() {
        let i = (0..rest.len()).max_by_key(|i| (nets_of(&rest[*i]).intersection(&seen).count(), rest[*i].leads.len(), Reverse(*i))).unwrap();
        let s = rest.remove(i);
        seen.extend(nets_of(&s));
        order.push(s);
    }
    // the chips in rows (shelves), left to right
    let mut parts: Vec<StripPart> = vec![];
    let mut blocked: BTreeSet<At> = BTreeSet::new();
    let (mut x, mut y, mut shelf) = (margin, 1u16, 0u16);
    for s in &order {
        if x > margin && x + s.w > width.saturating_sub(1) {
            y += shelf + gap;
            x = margin;
            shelf = 0;
        }
        let (refdes, _, _) = &c.on[s.k];
        for r in y..y + s.h {
            for cc in x..x + s.w {
                blocked.insert((r, cc));
            }
        }
        parts.push(StripPart { refdes: refdes.clone(), look: s.look, leads: s.leads.iter().map(|(p, _, a)| (p.clone(), (y + a.0, x + a.1))).collect(), mount: c.mounts.get(refdes).cloned().unwrap_or_default() });
        x += s.w + gap;
        shelf = shelf.max(s.h);
    }
    let mut bottom = if parts.is_empty() { 1 } else { y + shelf };
    // where each net's leads are so far
    let net_of_lead = |refdes: &str, pin: &str| lead_net(c, refdes, pin);
    let mut at_net: BTreeMap<String, Vec<At>> = BTreeMap::new();
    for p in &parts {
        for (pin, a) in &p.leads {
            at_net.entry(net_of_lead(&p.refdes, pin)).or_default().push(*a);
        }
    }
    // the wires from off the board come in at the left edge
    let mut off: Vec<(String, Vec<crate::stripboard::OffWire>)> = c.off.iter().map(|(r, _)| (r.clone(), vec![])).collect();
    for (row, (i, j)) in off_pins.iter().enumerate() {
        let (pin, net) = &c.off[*i].1[*j];
        // every other row: each has a free hole of its own beside it, for its wire
        let a = (1 + 2 * row as u16, 0);
        off[*i].1.push((pin.clone(), net.clone(), a));
        at_net.entry(net.clone()).or_default().push(a);
        blocked.insert(a);
    }
    // the two-lead parts: the most joined to what is placed first, each where its leads are nearest their nets' leads
    let mut twos = twos;
    twos.sort_by_key(|k| Reverse(c.on[*k].2.iter().filter(|(_, n)| at_net.contains_key(n)).count()));
    for k in twos {
        let (refdes, look, pins) = &c.on[k];
        if pins.len() != 2 {
            continue;
        }
        let near = |n: &str, a: At, at_net: &BTreeMap<String, Vec<At>>| -> u32 { at_net.get(n).map(|v| v.iter().map(|b| a.0.abs_diff(b.0) as u32 + a.1.abs_diff(b.1) as u32).min().unwrap_or(0)).unwrap_or(0) };
        let mut best: Option<(u32, At, At)> = None;
        for r in 0..bottom + 4 {
            for cc in 0..width {
                for (dr, dc) in [(0, SPAN), (SPAN, 0)] {
                    let (a, b) = ((r, cc), (r + dr, cc + dc));
                    if b.1 >= width {
                        continue;
                    }
                    let holes: Vec<At> = (0..=SPAN).map(|i| (r + dr / SPAN * i, cc + dc / SPAN * i)).collect();
                    if cc < margin || holes.iter().any(|h| blocked.contains(h)) {
                        continue;
                    }
                    // each lead keeps two free holes beside it, for its wire
                    let room = |h: At| {
                        [(0i32, 1i32), (1, 0), (0, -1), (-1, 0)]
                            .iter()
                            .filter(|(dr, dc)| {
                                let (y, x) = (h.0 as i32 + dr, h.1 as i32 + dc);
                                y >= 0 && x >= 0 && x < width as i32 && !blocked.contains(&(y as u16, x as u16)) && !holes.contains(&(y as u16, x as u16))
                            })
                            .count()
                    };
                    if room(a) < 2 || room(b) < 2 {
                        continue;
                    }
                    for (p, q) in [(a, b), (b, a)] {
                        // a lead next to another lead of another net crowds the wires
                        let crowd = |h: At, n: &str| -> u32 { at_net.iter().filter(|(m, _)| m.as_str() != n).flat_map(|(_, v)| v.iter()).filter(|x| x.0.abs_diff(h.0) + x.1.abs_diff(h.1) == 1).count() as u32 };
                        let cost = 4 * (near(&pins[0].1, p, &at_net) + near(&pins[1].1, q, &at_net)) + crowd(p, &pins[0].1) + crowd(q, &pins[1].1);
                        if best.is_none_or(|x| cost < x.0) {
                            best = Some((cost, p, q));
                        }
                    }
                }
            }
        }
        let Some((_, p, q)) = best else { continue };
        let holes: Vec<At> = if p.0 == q.0 { (p.1.min(q.1)..=p.1.max(q.1)).map(|cc| (p.0, cc)).collect() } else { (p.0.min(q.0)..=p.0.max(q.0)).map(|r| (r, p.1)).collect() };
        blocked.extend(holes);
        bottom = bottom.max(p.0.max(q.0) + 1);
        at_net.entry(pins[0].1.clone()).or_default().push(p);
        at_net.entry(pins[1].1.clone()).or_default().push(q);
        parts.push(StripPart { refdes: refdes.clone(), look: *look, leads: vec![(pins[0].0.clone(), p), (pins[1].0.clone(), q)], mount: c.mounts.get(refdes).cloned().unwrap_or_default() });
    }
    // the parts in their schematic's order
    let pos = |r: &str| c.on.iter().position(|x| x.0 == r).unwrap_or(usize::MAX);
    parts.sort_by_key(|p| (p.look == Look::TwoLead, pos(&p.refdes)));
    let mut l = StripLayout { perf: true, parts, off, ..Default::default() };
    let far = l.used().iter().map(|(a, _)| *a).fold((0u16, 0u16), |m, a| (m.0.max(a.0), m.1.max(a.1)));
    l.rows = far.0 + 2;
    l.cols = (far.1 + 2).max(width.min(far.1 + 2));
    l
}

/// How long the tries go on when no try joins every net (the best so far is kept): a big board does not hold the screen up.
const BUDGET: std::time::Duration = std::time::Duration::from_secs(3);

/// How many times the nets are wired again with the ones left apart first.
const ROUNDS: usize = 4;

/// The longest insulated wire on top that a way may jump with (holes).
const JUMP: i32 = 16;
/// What a jump costs on top of its length: a way along the copper is taken unless it is much longer.
const JUMP_COST: u32 = 30;

/// Wires every net: from the holes already joined, the cheapest way to the nearest lead not yet. A step to the next free hole is a
/// bare wire on the copper side (2, a turn 3 more, so the wires run straight); a jump of 2 to `JUMP` holes along a row or a column,
/// from and to a free hole, is an insulated wire on top over whatever lies between (`JUMP_COST` more): where the copper is shut in,
/// a way still goes on.
/// How `route` goes about the nets.
struct Order<'a> {
    /// The supplies and ground before the other nets (else the small nets first).
    rails_first: bool,
    /// A free hole beside each lead kept for its own net (a crowded board; on a small one the kept holes are in the way).
    escapes: bool,
    /// Nets wired before all the others, in this order (the ones a try before left apart).
    first: &'a [String],
}

fn route(c: &Circuit, l: &mut StripLayout, net_of_lead: &dyn Fn(&str, &str) -> String, order: Order) {
    let Order { rails_first, escapes, first } = order;
    let (rows, cols) = (l.rows as usize, l.cols as usize);
    let idx = |a: At| a.0 as usize * cols + a.1 as usize;
    let hole = |h: usize| ((h / cols) as u16, (h % cols) as u16);
    // each hole's net (copper or a lead); a lead on no net keeps its hole to itself
    let mut names: Vec<String> = vec![];
    let id = |n: &str, names: &mut Vec<String>| -> u32 {
        match names.iter().position(|x| x == n) {
            Some(i) => i as u32,
            None => {
                names.push(n.to_string());
                (names.len() - 1) as u32
            }
        }
    };
    let mut owner: Vec<Option<u32>> = vec![None; rows * cols];
    let mut leads: Vec<(At, u32)> = vec![];
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
            let i = id(&n, &mut names);
            owner[idx(*a)] = Some(i);
            leads.push((*a, i));
        }
    }
    for (_, v) in &l.off {
        for (_, n, a) in v {
            let i = id(n, &mut names);
            owner[idx(*a)] = Some(i);
            leads.push((*a, i));
        }
    }
    let lead_holes: BTreeSet<At> = leads.iter().map(|x| x.0).collect();
    const DIRS: [(i32, i32); 4] = [(0, 1), (1, 0), (0, -1), (-1, 0)];
    let beside = |a: At| -> Vec<At> {
        DIRS.iter().map(|(dr, dc)| (a.0 as i32 + dr, a.1 as i32 + dc)).filter(|(r, cc)| *r >= 0 && *cc >= 0 && (*r as usize) < rows && (*cc as usize) < cols).map(|(r, cc)| (r as u16, cc as u16)).collect()
    };
    // one free hole beside each lead kept for its own net: a way out from the lead (a jump starts from a hole, not a lead), however
    // the wires of the other nets crowd round it; the hole with the most free holes round it, the leads with the fewest ways out first
    let mut escape: BTreeMap<At, u32> = BTreeMap::new();
    let mut by_room: Vec<(At, u32)> = leads.clone();
    by_room.sort_by_key(|(a, _)| beside(*a).iter().filter(|h| !lead_holes.contains(h)).count());
    for (a, net) in by_room.into_iter().filter(|_| escapes) {
        let room = |h: At| beside(h).iter().filter(|x| !lead_holes.contains(x) && !escape.contains_key(x)).count();
        if let Some(h) = beside(a).into_iter().filter(|h| !lead_holes.contains(h) && !escape.contains_key(h)).max_by_key(|h| room(*h)) {
            escape.insert(h, net);
        }
    }
    let mut nets: Vec<(u32, Vec<At>)> = vec![];
    for (a, i) in &leads {
        match nets.iter_mut().find(|x| x.0 == *i) {
            Some(x) => x.1.push(*a),
            None => nets.push((*i, vec![*a])),
        }
    }
    nets.retain(|(_, v)| v.len() > 1);
    let rail = |i: u32| is_ground_name(&names[i as usize]) || c.supplies.contains(&names[i as usize]);
    nets.sort_by_key(|(i, v)| {
        let (r0, r1) = (v.iter().map(|a| a.0).min().unwrap(), v.iter().map(|a| a.0).max().unwrap());
        let (c0, c1) = (v.iter().map(|a| a.1).min().unwrap(), v.iter().map(|a| a.1).max().unwrap());
        let ahead = first.iter().position(|n| *n == names[*i as usize]).unwrap_or(usize::MAX);
        (ahead, rail(*i) != rails_first, v.len(), (r1 - r0) + (c1 - c0))
    });
    let mut link_ends: BTreeSet<At> = BTreeSet::new();
    // a state is a hole and how the way came in: 0..4 a step that way, 4 the start, 5 landed from a jump
    const WAYS: usize = 6;
    let all_nets: Vec<(u32, Vec<At>)> = nets.clone();
    for (n_at, (net, terms)) in nets.into_iter().enumerate() {
        // the free holes beside the leads of the nets still to wire, when a lead has two or fewer: kept for that net (another net's
        // wire there would shut the lead in)
        let mut kept: BTreeMap<At, u32> = BTreeMap::new();
        for (other, ts) in all_nets.iter().skip(n_at + 1) {
            for t in ts {
                let free: Vec<At> = DIRS
                    .iter()
                    .map(|(dr, dc)| (t.0 as i32 + dr, t.1 as i32 + dc))
                    .filter(|(r, cc)| *r >= 0 && *cc >= 0 && (*r as usize) < rows && (*cc as usize) < cols)
                    .map(|(r, cc)| (r as u16, cc as u16))
                    .filter(|h| owner[idx(*h)].is_none() && !link_ends.contains(h))
                    .collect();
                if free.len() <= 2 {
                    for h in free {
                        kept.entry(h).or_insert(*other);
                    }
                }
            }
        }
        let mut tree: Vec<At> = vec![terms[0]];
        let mut left: BTreeSet<At> = terms[1..].iter().copied().collect();
        // holes this net's ways keep off: where a way would end two wires on top in one hole
        let mut avoid: BTreeSet<At> = BTreeSet::new();
        let mut tries = 0;
        while !left.is_empty() {
            let mut dist: Vec<u32> = vec![u32::MAX; rows * cols * WAYS];
            let mut prev: Vec<u32> = vec![u32::MAX; rows * cols * WAYS];
            let mut heap = BinaryHeap::new();
            for t in &tree {
                let s = idx(*t) * WAYS + 4;
                dist[s] = 0;
                heap.push(Reverse((0u32, s as u32)));
            }
            let free_for = |h: At, owner: &[Option<u32>], link_ends: &BTreeSet<At>| {
                owner[idx(h)].is_none_or(|o| o == net) && !link_ends.contains(&h) && !avoid.contains(&h) && kept.get(&h).is_none_or(|o| *o == net) && escape.get(&h).is_none_or(|o| *o == net)
            };
            let mut found = None;
            while let Some(Reverse((d, s))) = heap.pop() {
                let s = s as usize;
                if d > dist[s] {
                    continue;
                }
                let (h, way) = (s / WAYS, s % WAYS);
                let at = hole(h);
                if left.contains(&at) {
                    found = Some(s);
                    break;
                }
                // a lead is an end, not a hole to pass through on the way to another (its wire would run under the part)
                if way != 4 && lead_holes.contains(&at) {
                    continue;
                }
                let mut push = |to: At, w: usize, nd: u32, heap: &mut BinaryHeap<Reverse<(u32, u32)>>| {
                    let ns = idx(to) * WAYS + w;
                    if nd < dist[ns] {
                        dist[ns] = nd;
                        prev[ns] = s as u32;
                        heap.push(Reverse((nd, ns as u32)));
                    }
                };
                // the jumps: from a free hole (not a lead, not a wire's end), along the row or the column, to a free hole
                let can_jump = way != 5 && !lead_holes.contains(&at) && !link_ends.contains(&at);
                for (w, (dr, dc)) in DIRS.iter().enumerate() {
                    let (r, cc) = (at.0 as i32 + dr, at.1 as i32 + dc);
                    if r >= 0 && cc >= 0 && r < rows as i32 && cc < cols as i32 {
                        let to = (r as u16, cc as u16);
                        // no going straight back (a way back to where it jumped from could jump again from the same hole)
                        if free_for(to, &owner, &link_ends) && !(way < 4 && (way + 2) % 4 == w) {
                            let turn = way < 4 && way != w;
                            push(to, w, d + 2 + if turn { 3 } else { 0 }, &mut heap);
                        }
                    }
                    if !can_jump {
                        continue;
                    }
                    for k in 2..=JUMP {
                        let (r, cc) = (at.0 as i32 + dr * k, at.1 as i32 + dc * k);
                        if r < 0 || cc < 0 || r >= rows as i32 || cc >= cols as i32 {
                            break;
                        }
                        let to = (r as u16, cc as u16);
                        if free_for(to, &owner, &link_ends) && !lead_holes.contains(&to) {
                            push(to, 5, d + JUMP_COST + 2 * k as u32, &mut heap);
                        }
                    }
                }
            }
            let Some(mut s) = found else {
                // shut in on every side: the check names the net left apart
                break;
            };
            let mut path: Vec<At> = vec![];
            loop {
                path.push(hole(s / WAYS));
                if prev[s] == u32::MAX {
                    break;
                }
                s = prev[s] as usize;
            }
            path.reverse();
            // two wires on top ending in one hole (a way that comes back to where it jumped): that hole kept off, and the way found again
            let mut ends: Vec<At> = path.windows(2).filter(|w| w[0].0.abs_diff(w[1].0) + w[0].1.abs_diff(w[1].1) > 1).flat_map(|w| [w[0], w[1]]).collect();
            ends.sort();
            if let Some(w) = ends.windows(2).find(|w| w[0] == w[1]) {
                tries += 1;
                if tries > 8 {
                    break;
                }
                avoid.insert(w[0]);
                continue;
            }
            for a in &path {
                owner[idx(*a)] = Some(net);
            }
            // the straight runs of steps, each a bare wire; each jump an insulated wire on top
            let mut start = 0;
            for i in 1..path.len() {
                let (a, b) = (path[i - 1], path[i]);
                if a.0.abs_diff(b.0) + a.1.abs_diff(b.1) > 1 {
                    if start < i - 1 {
                        l.bare.push((path[start], a));
                    }
                    l.links.push(Link { a, b, net: names[net as usize].clone() });
                    link_ends.insert(a);
                    link_ends.insert(b);
                    start = i;
                    continue;
                }
                let next_turns = i + 1 < path.len() && {
                    let n = path[i + 1];
                    let jump = b.0.abs_diff(n.0) + b.1.abs_diff(n.1) > 1;
                    jump || (n.0 != path[start].0 && n.1 != path[start].1)
                };
                if next_turns || i + 1 == path.len() {
                    l.bare.push((path[start], b));
                    start = i;
                }
            }
            left.remove(path.last().unwrap());
            tree.extend(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stripboard::{strip_check, strip_layout, strip_layout_with};

    fn load(name: &str) -> Schematic {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// Samples laid out on a perfboard join what their schematics join and nothing more; every bare wire runs straight along a row
    /// or a column; no lead or wire's end shares a hole; and the boards are far smaller than the stripboard's (the seven-segment
    /// counter 60 × 22 holes on strips, 21 × 18 here). A width to keep to is kept.
    #[test]
    fn samples_lay_out_on_a_perfboard_as_drawn_and_small() {
        let (mut perf, mut strip) = (0u32, 0u32);
        for name in ["ne555_blinker.schjson", "seg7_counter.schjson", "traffic_light.schjson", "active_filter.schjson", "current_sensors.schjson"] {
            let s = load(name);
            let l = perf_layout_with(&s, StripOptions::default());
            assert!(l.perf);
            assert!(strip_check(&s, &l).is_empty(), "{name}: {:?}", strip_check(&s, &l));
            assert!(l.bare.iter().all(|(a, b)| a.0 == b.0 || a.1 == b.1), "{name}: straight wires");
            assert!(l.used().iter().all(|(a, _)| a.0 < l.rows && a.1 < l.cols), "{name}: on the board");
            let st = strip_layout(&s);
            perf += l.rows as u32 * l.cols as u32;
            strip += st.rows as u32 * st.cols as u32;
        }
        assert!(perf * 2 < strip, "{perf} holes on perfboards, {strip} on stripboards");
        let s = load("seg7_counter.schjson");
        let narrow = perf_layout_with(&s, StripOptions { cols: Some(16) });
        assert!(narrow.cols <= 16 && strip_check(&s, &narrow).is_empty(), "{} wide: {:?}", narrow.cols, strip_check(&s, &narrow));
        assert!(!strip_layout_with(&s, StripOptions::default()).perf, "the stripboard keeps its strips");
    }

    /// With no strips every hole is a piece of its own: two leads side by side are apart until a bare wire joins them; a wire that
    /// meets another in the middle (a T) or crosses it touches it, and one beside it does not.
    #[test]
    fn holes_join_only_by_wires() {
        let part = |r: &str, a: At, b: At| StripPart { refdes: r.into(), look: Look::TwoLead, leads: vec![("1".into(), a), ("2".into(), b)], mount: Default::default() };
        let mut l = StripLayout { perf: true, rows: 6, cols: 6, parts: vec![part("R1", (0, 0), (0, 1)), part("R2", (4, 0), (4, 4))], ..Default::default() };
        assert_eq!(l.pieces().len(), 36);
        assert!(l.joins().is_empty());
        // a wire along row 2, and one down column 2 from R1's row to it (a T): R1.2's hole is not on them, R1.1's neither
        l.bare = vec![((2, 0), (2, 4)), ((0, 2), (2, 2))];
        let touches = |l: &StripLayout, a: At, b: At| l.bare_joins().iter().any(|(x, y)| (*x == a && *y == b) || (*x == b && *y == a));
        assert!(touches(&l, (0, 2), (2, 2)) && touches(&l, (2, 0), (2, 2)), "the T joins the two wires");
        // a wire beside it (row 3) touches nothing
        l.bare.push(((3, 0), (3, 4)));
        assert!(!l.bare_joins().iter().any(|(x, y)| (x.0 == 3) != (y.0 == 3)), "a wire beside another does not touch it");
        // down to R2's lead at (4, 4): the lead's hole joins
        l.bare.push(((2, 4), (4, 4)));
        assert!(touches(&l, (2, 4), (4, 4)));
    }
}
