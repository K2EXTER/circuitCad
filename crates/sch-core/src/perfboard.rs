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

mod simpl;
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

/// A chip turned a quarter turn clockwise (its two rows of pins along the board's rows); a row of leads stays as it is (a header
/// lies along the edge already).
fn turn(s: Shape) -> Shape {
    if s.look != Look::Dip {
        return s;
    }
    let leads = s.leads.into_iter().map(|(p, n, a)| (p, n, (a.1, s.h - 1 - a.0))).collect();
    Shape { leads, h: s.w, w: s.h, ..s }
}

/// Lays the schematic out on a perfboard: the headers along the top edge, the chips in rows within the board's width (`opt.cols`,
/// else about square) with room between for the wires, each two-lead part where its leads are nearest the leads of their nets, then
/// every net wired.
pub fn perf_layout_with(s: &Schematic, opt: StripOptions) -> StripLayout {
    perf_layout_keeping(s, opt, vec![])
}

/// Lays the schematic out on a perfboard round the parts `from` has locked (`StripLayout::locked`): those stay in their holes as they
/// are, the others are placed round them (the shelves and the springs leave the locked parts' holes and room round them free) and
/// every net is wired again. A locked part no longer on the schematic, or with other pins now, is placed afresh.
pub fn perf_layout_around(s: &Schematic, opt: StripOptions, from: &StripLayout) -> StripLayout {
    let c = read(s);
    let keep: Vec<StripPart> = from
        .parts
        .iter()
        .filter(|p| from.locked.contains(&p.refdes))
        .filter(|p| {
            c.on.iter().find(|x| x.0 == p.refdes).is_some_and(|x| {
                let mut a: Vec<&str> = x.2.iter().map(|y| y.0.as_str()).collect();
                let mut b: Vec<&str> = p.leads.iter().map(|y| y.0.as_str()).collect();
                a.sort_unstable();
                b.sort_unstable();
                a == b
            })
        })
        .cloned()
        .collect();
    let mut l = perf_layout_keeping(s, opt, keep.clone());
    l.locked = keep.into_iter().map(|p| p.refdes).collect();
    l
}

fn perf_layout_keeping(s: &Schematic, opt: StripOptions, keep: Vec<StripPart>) -> StripLayout {
    let c = read(s);
    let mut wants = Wants::read(s, &c);
    wants.keep = keep;
    let net_of_lead = |refdes: &str, pin: &str| lead_net(&c, refdes, pin);
    // each placement (the gaps between chips, the chips upright and turned a quarter turn, without and with annealing) wired the rails first and the small nets
    // first, without and then with a way out kept by each lead, every placement with one order before the next order (so each has its
    // turn within `BUDGET`); each again with the nets left apart wired first (a few rounds), and once all are joined with the nets
    // that needed wires on top first: the one with the fewest nets left apart, then the fewest wires on top, then the smallest board
    let mut best: Option<(usize, usize, StripLayout)> = None;
    let start = std::time::Instant::now();
    let done = |_: &Option<(usize, usize, StripLayout)>| start.elapsed() > BUDGET;
    // the shelves as placed, then the same made better by annealing (it helps small boards; on a crowded one the estimate it goes by
    // packs the parts too tight for the wires, and the shelves as placed win)
    // and each again with the chips where the springs and the spreading put them (`simpl`)
    let mut placings: Vec<(u16, bool, bool, bool)> =
        [false, true].iter().flat_map(|q| [false, true].iter().flat_map(move |s| GAPS.iter().flat_map(move |g| [(*g, false, *s, *q), (*g, true, *s, *q)]))).collect();
    // round locked parts the springs go first: they pull the other parts toward the locked parts' leads (the shelves fill the rows
    // after the locked parts in order, far from what they join)
    if !wants.keep.is_empty() {
        placings.sort_by_key(|p| !p.3);
    }
    let mut placed: Vec<Option<Option<StripLayout>>> = vec![None; placings.len()];
    for (rails_first, escapes) in [(true, false), (false, false), (true, true), (false, true)] {
        for (pi, (gap, turned, sa, springs)) in placings.iter().copied().enumerate() {
            if done(&best) {
                break;
            }
            let l = placed[pi]
                .get_or_insert_with(|| {
                    if !springs {
                        return Some(place(&c, opt, gap, turned, sa, &wants, None));
                    }
                    simpl::targets(&c, opt, gap, turned, &wants).map(|t| place(&c, opt, gap, turned, sa, &wants, Some(&t)))
                })
                .clone();
            let Some(l) = l else { continue };
            let mut first: Vec<String> = vec![];
            let mut extra = 0;
            for round in 0..ROUNDS + MORE_ROUNDS {
                if best.is_some() && start.elapsed() > BUDGET {
                    break;
                }
                let mut m = l.clone();
                route(&c, &mut m, &net_of_lead, Order { rails_first, escapes, first: &first });
                let apart = crate::stripboard::strip_unjoined(s, &m);
                let key = (apart.len(), m.links.len(), m.rows as usize * m.cols as usize);
                // a placing far behind the best on its first wiring is not wired again (the rounds after gain less than that)
                if round == 0 && best.as_ref().is_some_and(|b| key.0 > b.0 + 1 || (key.0 >= b.0 && key.1 > b.1 + b.1 / 2 + 2)) {
                    break;
                }
                let on_top: Vec<String> = m.links.iter().map(|k| k.net.clone()).collect();
                if best.as_ref().is_none_or(|b| key < (b.0, b.1, b.2.rows as usize * b.2.cols as usize)) {
                    best = Some((key.0, key.1, m));
                }
                if apart.is_empty() {
                    if extra >= MORE_ROUNDS {
                        break;
                    }
                    extra += 1;
                    // the nets that needed wires on top, the most first, wired before the others
                    let mut count: BTreeMap<String, usize> = BTreeMap::new();
                    for n in on_top {
                        *count.entry(n).or_default() += 1;
                    }
                    let mut v: Vec<(String, usize)> = count.into_iter().collect();
                    v.sort_by_key(|x| Reverse(x.1));
                    for (n, _) in v.into_iter().rev() {
                        first.retain(|x| *x != n);
                        first.insert(0, n);
                    }
                    continue;
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

/// What the requirements table asks of the placement: the parts kept apart (each rule's two sets of parts and the distance in
/// holes), the groups each part is in (placed together), and the sections (each placed in a band of rows of its own).
#[derive(Default)]
struct Wants {
    apart: Vec<(BTreeSet<String>, BTreeSet<String>, f64)>,
    groups: BTreeMap<String, Vec<usize>>,
    sections: Vec<(String, BTreeSet<String>)>,
    /// The parts locked where they are (`perf_layout_around`): placed first as they are, never moved.
    keep: Vec<StripPart>,
}

impl Wants {
    fn read(s: &Schematic, c: &Circuit) -> Wants {
        // a rule's item: a part by its name, else every part with a lead on that net
        let parts_of = |items: &[String]| -> BTreeSet<String> {
            c.on.iter()
                .filter(|(r, _, pins)| items.iter().any(|it| design_spec::matches_name(it, r) || pins.iter().any(|(_, n)| !n.is_empty() && design_spec::matches_name(it, n))))
                .map(|(r, _, _)| r.clone())
                .collect()
        };
        let apart = s.requirements.apart.iter().map(|k| (parts_of(&k.a), parts_of(&k.b), k.min_dist / 2.54)).collect();
        let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, g) in s.requirements.groups.iter().enumerate() {
            for p in &g.parts {
                groups.entry(p.clone()).or_default().push(i);
            }
        }
        Wants { apart, groups, sections: crate::stripboard::sections(s, c), keep: vec![] }
    }

    /// Whether a part is locked where it is.
    fn kept(&self, refdes: &str) -> bool {
        self.keep.iter().any(|p| p.refdes == refdes)
    }

    /// The holes the locked parts take (a chip its outline, a two-lead part its leads and the hole between).
    fn kept_holes(&self) -> BTreeSet<At> {
        self.keep.iter().flat_map(outline).collect()
    }

    /// The narrowest board the locked parts fit on (their rightmost hole and a free column).
    fn kept_width(&self) -> u16 {
        self.keep.iter().flat_map(|p| p.leads.iter().map(|x| x.1 .1 + 2)).max().unwrap_or(0)
    }

    /// Whether a part with these leads keeps its distance from the parts already placed.
    fn far_enough(&self, refdes: &str, leads: &[At], placed: &[StripPart]) -> bool {
        if !self.apart.iter().any(|(a, b, _)| a.contains(refdes) || b.contains(refdes)) {
            return true;
        }
        let gap = |p: &StripPart| leads.iter().flat_map(|a| p.leads.iter().map(move |(_, b)| (a.0 as f64 - b.0 as f64).hypot(a.1 as f64 - b.1 as f64))).fold(f64::INFINITY, f64::min);
        self.apart.iter().all(|(a, b, d)| {
            placed.iter().filter(|p| p.refdes != refdes).all(|p| {
                let pair = (a.contains(refdes) && b.contains(&p.refdes)) || (b.contains(refdes) && a.contains(&p.refdes));
                !pair || gap(p) >= *d
            })
        })
    }

    /// Whether two parts are in a group together.
    fn together(&self, x: &str, y: &str) -> bool {
        self.groups.get(x).is_some_and(|gx| self.groups.get(y).is_some_and(|gy| gx.iter().any(|g| gy.contains(g))))
    }
}

/// How many moves the annealing tries.
const ANNEAL_MOVES: usize = 30_000;

/// The placement made better by annealing (simulated annealing on an estimate, not on the wires themselves: a wiring takes too long
/// to try each move): a two-lead part moved to a free place near it (lying or standing, either way round), two two-lead parts
/// swapped, or two chips of one size swapped; a move that makes the nets' spread (each net's half the perimeter round its leads, the
/// supplies and ground counting less) smaller is kept, a worse one now and then while it is still warm. Every part stays within its
/// section's rows (or the board as placed), each lead keeps two free holes beside it, and the table's distances are kept.
fn anneal(c: &Circuit, wants: &Wants, parts: &mut [StripPart], off: &[(String, Vec<crate::stripboard::OffWire>)], width: u16, margin: u16, bands: &[(u16, u16)]) {
    if parts.is_empty() {
        return;
    }
    let net_of = |p: &StripPart, pin: &str| lead_net(c, &p.refdes, pin);
    // each net's leads (part, lead), the fixed holes of the off-board wires
    let mut net_ids: Vec<String> = vec![];
    let mut leads_of: Vec<Vec<(usize, usize)>> = vec![];
    let mut fixed_of: Vec<Vec<At>> = vec![];
    let id = |n: &str, net_ids: &mut Vec<String>, leads_of: &mut Vec<Vec<(usize, usize)>>, fixed_of: &mut Vec<Vec<At>>| -> usize {
        match net_ids.iter().position(|x| x == n) {
            Some(i) => i,
            None => {
                net_ids.push(n.to_string());
                leads_of.push(vec![]);
                fixed_of.push(vec![]);
                net_ids.len() - 1
            }
        }
    };
    let mut part_nets: Vec<Vec<usize>> = vec![vec![]; parts.len()];
    for (pi, p) in parts.iter().enumerate() {
        for (li, (pin, _)) in p.leads.iter().enumerate() {
            let n = net_of(p, pin);
            if n.is_empty() {
                continue;
            }
            let i = id(&n, &mut net_ids, &mut leads_of, &mut fixed_of);
            leads_of[i].push((pi, li));
            if !part_nets[pi].contains(&i) {
                part_nets[pi].push(i);
            }
        }
    }
    for (_, v) in off {
        for (_, n, a) in v {
            let i = id(n, &mut net_ids, &mut leads_of, &mut fixed_of);
            fixed_of[i].push(*a);
        }
    }
    let weight: Vec<f64> = net_ids.iter().map(|n| if is_ground_name(n) || c.supplies.contains(n) { 0.2 } else { 1.0 }).collect();
    let spread = |i: usize, parts: &[StripPart]| -> f64 {
        let pts = leads_of[i].iter().map(|(p, l)| parts[*p].leads[*l].1).chain(fixed_of[i].iter().copied());
        let (mut r0, mut r1, mut c0, mut c1, mut n) = (u16::MAX, 0u16, u16::MAX, 0u16, 0);
        for a in pts {
            r0 = r0.min(a.0);
            r1 = r1.max(a.0);
            c0 = c0.min(a.1);
            c1 = c1.max(a.1);
            n += 1;
        }
        if n < 2 {
            0.0
        } else {
            weight[i] * ((r1 - r0) + (c1 - c0)) as f64
        }
    };
    let nets_cost = |nets: &[usize], parts: &[StripPart]| -> f64 { nets.iter().map(|i| spread(*i, parts)).sum() };
    // the holes each part takes (a chip its whole outline, a two-lead part its leads and the hole between), and who takes each
    let holes_of = |p: &StripPart| -> Vec<At> {
        let (r0, r1) = (p.leads.iter().map(|x| x.1 .0).min().unwrap_or(0), p.leads.iter().map(|x| x.1 .0).max().unwrap_or(0));
        let (c0, c1) = (p.leads.iter().map(|x| x.1 .1).min().unwrap_or(0), p.leads.iter().map(|x| x.1 .1).max().unwrap_or(0));
        (r0..=r1).flat_map(|r| (c0..=c1).map(move |cc| (r, cc))).collect()
    };
    let mut taken: BTreeMap<At, usize> = BTreeMap::new();
    for (pi, p) in parts.iter().enumerate() {
        for h in holes_of(p) {
            taken.insert(h, pi);
        }
    }
    for (_, v) in off {
        for (_, _, a) in v {
            taken.insert(*a, usize::MAX);
        }
    }
    let band_of = |a: At| bands.iter().copied().find(|(t, b)| a.0 >= *t && a.0 <= *b).unwrap_or((1, u16::MAX));
    let twos: Vec<usize> = (0..parts.len()).filter(|i| parts[*i].look == Look::TwoLead && parts[*i].leads.len() == 2 && !wants.kept(&parts[*i].refdes)).collect();
    let chips: Vec<usize> = (0..parts.len()).filter(|i| parts[*i].look != Look::TwoLead && !wants.kept(&parts[*i].refdes)).collect();
    let size = |p: &StripPart| {
        let (r0, r1) = (p.leads.iter().map(|x| x.1 .0).min().unwrap_or(0), p.leads.iter().map(|x| x.1 .0).max().unwrap_or(0));
        let (c0, c1) = (p.leads.iter().map(|x| x.1 .1).min().unwrap_or(0), p.leads.iter().map(|x| x.1 .1).max().unwrap_or(0));
        (r1 - r0, c1 - c0, p.leads.len(), r0, c0)
    };
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    // whether the leads of part `pi` (its holes taken out of `taken` first) each keep two free holes beside them
    let room = |pi: usize, parts: &[StripPart], taken: &BTreeMap<At, usize>| -> bool {
        let own: Vec<At> = holes_of(&parts[pi]);
        parts[pi].leads.iter().all(|(_, a)| {
            [(0i32, 1i32), (1, 0), (0, -1), (-1, 0)]
                .iter()
                .filter(|(dr, dc)| {
                    let (y, x) = (a.0 as i32 + dr, a.1 as i32 + dc);
                    y >= 0 && x >= 0 && x < width as i32 && !own.contains(&(y as u16, x as u16)) && !taken.contains_key(&(y as u16, x as u16))
                })
                .count()
                >= 2
        })
    };
    let (t0, t1) = (3.0f64, 0.05f64);
    for step in 0..ANNEAL_MOVES {
        let temp = t0 * (t1 / t0).powf(step as f64 / ANNEAL_MOVES as f64);
        let accept = |delta: f64, r: u64| delta <= 0.0 || ((r % 1_000_000) as f64 / 1_000_000.0) < (-delta / temp).exp();
        let kind = rnd() % 10;
        if kind < 7 && !twos.is_empty() {
            // a two-lead part to a free place near it
            let pi = twos[(rnd() % twos.len() as u64) as usize];
            let old = parts[pi].leads.clone();
            let (band_top, band_bottom) = band_of(old[0].1);
            let (ar, ac) = old[0].1;
            let (dr, dc) = ((rnd() % 13) as i32 - 6, (rnd() % 13) as i32 - 6);
            let (r, cc) = (ar as i32 + dr, ac as i32 + dc);
            let standing = rnd() % 2 == 0;
            let flip = rnd() % 2 == 0;
            let (end_r, end_c) = if standing { (r + SPAN as i32, cc) } else { (r, cc + SPAN as i32) };
            // (the last column is left free, as the chips leave it: else the board is a hole wider than asked)
            if r < band_top as i32 || end_r > band_bottom as i32 || cc < margin as i32 || end_c + 1 >= width as i32 {
                continue;
            }
            let (a, b) = ((r as u16, cc as u16), (end_r as u16, end_c as u16));
            let (p, q) = if flip { (b, a) } else { (a, b) };
            let new_holes: Vec<At> = if standing { (a.0..=b.0).map(|y| (y, a.1)).collect() } else { (a.1..=b.1).map(|x| (a.0, x)).collect() };
            if new_holes.iter().any(|h| taken.get(h).is_some_and(|o| *o != pi)) {
                continue;
            }
            let before = nets_cost(&part_nets[pi], parts);
            for h in holes_of(&parts[pi]) {
                taken.remove(&h);
            }
            parts[pi].leads[0].1 = p;
            parts[pi].leads[1].1 = q;
            for h in &new_holes {
                taken.insert(*h, pi);
            }
            let ok = room(pi, parts, &taken) && wants.far_enough(&parts[pi].refdes, &[p, q], parts);
            let delta = nets_cost(&part_nets[pi], parts) - before;
            if !(ok && accept(delta, rnd())) {
                for h in &new_holes {
                    taken.remove(h);
                }
                parts[pi].leads = old;
                for h in holes_of(&parts[pi]) {
                    taken.insert(h, pi);
                }
            }
        } else if kind < 9 && twos.len() > 1 {
            // two two-lead parts swapped (each takes the other's holes)
            let (i, j) = (twos[(rnd() % twos.len() as u64) as usize], twos[(rnd() % twos.len() as u64) as usize]);
            if i == j || band_of(parts[i].leads[0].1) != band_of(parts[j].leads[0].1) {
                continue;
            }
            let mut nets = part_nets[i].clone();
            nets.extend(part_nets[j].iter().copied());
            let before = nets_cost(&nets, parts);
            let (li, lj) = (parts[i].leads.clone(), parts[j].leads.clone());
            for k in 0..2 {
                parts[i].leads[k].1 = lj[k].1;
                parts[j].leads[k].1 = li[k].1;
            }
            let ok = wants.far_enough(&parts[i].refdes, &[lj[0].1, lj[1].1], parts) && wants.far_enough(&parts[j].refdes, &[li[0].1, li[1].1], parts);
            let delta = nets_cost(&nets, parts) - before;
            if ok && accept(delta, rnd()) {
                for h in holes_of(&parts[i]) {
                    taken.insert(h, i);
                }
                for h in holes_of(&parts[j]) {
                    taken.insert(h, j);
                }
            } else {
                parts[i].leads = li;
                parts[j].leads = lj;
            }
        } else if chips.len() > 1 {
            // two chips of one size and shape swapped
            let (i, j) = (chips[(rnd() % chips.len() as u64) as usize], chips[(rnd() % chips.len() as u64) as usize]);
            let (si, sj) = (size(&parts[i]), size(&parts[j]));
            if i == j || (si.0, si.1, si.2) != (sj.0, sj.1, sj.2) || parts[i].look != parts[j].look || band_of((si.3, si.4)) != band_of((sj.3, sj.4)) {
                continue;
            }
            let mut nets = part_nets[i].clone();
            nets.extend(part_nets[j].iter().copied());
            let before = nets_cost(&nets, parts);
            let (li, lj) = (parts[i].leads.clone(), parts[j].leads.clone());
            let shift = |l: &[(String, At)], from: (u16, u16), to: (u16, u16)| -> Vec<(String, At)> { l.iter().map(|(p, a)| (p.clone(), (a.0 - from.0 + to.0, a.1 - from.1 + to.1))).collect() };
            parts[i].leads = shift(&li, (si.3, si.4), (sj.3, sj.4));
            parts[j].leads = shift(&lj, (sj.3, sj.4), (si.3, si.4));
            let ok = wants.far_enough(&parts[i].refdes, &parts[i].leads.iter().map(|x| x.1).collect::<Vec<_>>(), parts) && wants.far_enough(&parts[j].refdes, &parts[j].leads.iter().map(|x| x.1).collect::<Vec<_>>(), parts);
            let delta = nets_cost(&nets, parts) - before;
            if ok && accept(delta, rnd()) {
                for h in holes_of(&parts[i]) {
                    taken.insert(h, i);
                }
                for h in holes_of(&parts[j]) {
                    taken.insert(h, j);
                }
            } else {
                parts[i].leads = li;
                parts[j].leads = lj;
            }
        }
    }
}

/// The chips and rows of leads (not the two-lead parts), each turned a quarter turn when `turned`.
fn shapes_of(c: &Circuit, turned: bool) -> Vec<Shape> {
    (0..c.on.len()).filter(|k| look_on(c, *k) != Look::TwoLead).map(|k| shape(c, k)).map(|s| if turned { turn(s) } else { s }).collect()
}

/// What a chip takes on the board with the free holes round it for the wires (`gap` holes between chips).
fn room_of(s: &Shape, gap: u16) -> u32 {
    (s.h + gap) as u32 * (s.w + gap) as u32
}

/// What a two-lead part takes on the board with room round it, in holes.
const TWO_ROOM: u32 = 12;

/// The board's width: as asked, else about square for what goes on it (and never narrower than the widest part).
fn board_width(shapes: &[Shape], twos: usize, opt: StripOptions, gap: u16, margin: u16) -> u16 {
    let area: u32 = shapes.iter().map(|s| room_of(s, gap)).sum::<u32>() + twos as u32 * TWO_ROOM;
    let widest = shapes.iter().map(|s| s.w).max().unwrap_or(0) + 2 * margin;
    opt.cols.unwrap_or_else(|| ((area as f64 * 1.3).sqrt().ceil() as u16).max(widest)).max(widest).max(8)
}

/// Whether a part is a header (a long row of leads: along the top edge).
fn is_head(s: &Shape) -> bool {
    s.look != Look::Dip && s.leads.len() >= LONG_ROW
}

/// The free place (top-left corner) nearest a chip's target centre `t` (row, column): on or below row `y0`, within the board's
/// width, its outline and `gap` holes round it clear of what is placed (`blocked`), and `ok` there (the table's distances).
fn target_place(s: &Shape, t: (f64, f64), y0: u16, gap: u16, margin: u16, width: u16, blocked: &BTreeSet<At>, ok: impl Fn(u16, u16) -> bool) -> Option<At> {
    let want = ((t.0 - (s.h as f64 - 1.0) / 2.0).round() as i32, (t.1 - (s.w as f64 - 1.0) / 2.0).round() as i32);
    let fits = |yy: u16, xx: u16| -> bool {
        xx >= margin
            && xx + s.w < width
            && yy >= y0
            && (yy.saturating_sub(gap)..yy + s.h + gap).all(|r| (xx.saturating_sub(gap)..xx + s.w + gap).all(|cc| !blocked.contains(&(r, cc))))
            && ok(yy, xx)
    };
    for d in 0..400i32 {
        for dy in -d..=d {
            let dx = d - dy.abs();
            let xs = if dx == 0 { vec![want.1] } else { vec![want.1 - dx, want.1 + dx] };
            for xx in xs {
                let yy = want.0 + dy;
                if yy >= 0 && xx >= 0 && fits(yy as u16, xx as u16) {
                    return Some((yy as u16, xx as u16));
                }
            }
        }
    }
    None
}

/// The holes a part takes on the board: a chip its outline, a two-lead part its leads and the hole between.
fn outline(p: &StripPart) -> Vec<At> {
    let (r0, r1) = (p.leads.iter().map(|x| x.1 .0).min().unwrap_or(0), p.leads.iter().map(|x| x.1 .0).max().unwrap_or(0));
    let (c0, c1) = (p.leads.iter().map(|x| x.1 .1).min().unwrap_or(0), p.leads.iter().map(|x| x.1 .1).max().unwrap_or(0));
    (r0..=r1).flat_map(|r| (c0..=c1).map(move |cc| (r, cc))).collect()
}

/// The wires on top that run along a chip's (or a row of leads') pins: over two of its pins one after the other, lying on the chip's
/// or the socket's row of pins (the automatic layout may lay one so where nothing else gets through; the person may move its ends).
/// Each wire's index and the part. A wire across a row of pins, under a chip (between its rows) or over a two-lead part is not one.
pub fn links_along_pins(l: &StripLayout) -> Vec<(usize, String)> {
    let pin_at: BTreeMap<At, &str> = l.parts.iter().filter(|p| p.look != Look::TwoLead).flat_map(|p| p.leads.iter().map(move |x| (x.1, p.refdes.as_str()))).collect();
    l.links
        .iter()
        .enumerate()
        .filter_map(|(k, w)| {
            let n = w.a.0.abs_diff(w.b.0).max(w.a.1.abs_diff(w.b.1)) as usize;
            let at = |i: usize| {
                let f = i as f64 / n.max(1) as f64;
                ((w.a.0 as f64 + (w.b.0 as f64 - w.a.0 as f64) * f).round() as u16, (w.a.1 as f64 + (w.b.1 as f64 - w.a.1 as f64) * f).round() as u16)
            };
            (1..n.saturating_sub(1)).find_map(|i| match (pin_at.get(&at(i)), pin_at.get(&at(i + 1))) {
                (Some(x), Some(y)) if x == y => Some((k, x.to_string())),
                _ => None,
            })
        })
        .collect()
}

/// The holes beside `h` (up, down, left, right) on a board `width` holes wide.
fn beside4(h: At, width: u16) -> impl Iterator<Item = At> {
    [(0i32, 1i32), (1, 0), (0, -1), (-1, 0)].into_iter().filter_map(move |(dr, dc)| {
        let (y, x) = (h.0 as i32 + dr, h.1 as i32 + dc);
        (y >= 0 && x >= 0 && x < width as i32).then_some((y as u16, x as u16))
    })
}

/// The parts placed (`gap` free holes between chips, the chips `turned` a quarter turn, made better by annealing when `sa`), not yet
/// wired: section by section (one band of rows each, a gap between), in
/// each the headers along its top, then the chips in rows (shelves) left to right, then the two-lead parts. With `targets` (each
/// part's centre, row and column, from `simpl::targets`), a chip other than a header goes to the free place nearest its target
/// instead of the next on the shelf, and a two-lead part weighs its distance from its target too.
fn place(c: &Circuit, opt: StripOptions, gap: u16, turned: bool, sa: bool, wants: &Wants, targets: Option<&BTreeMap<String, (f64, f64)>>) -> StripLayout {
    let off_pins: Vec<(usize, usize)> = c.off.iter().enumerate().flat_map(|(i, (_, p))| p.iter().enumerate().filter(|(_, (_, n))| !n.is_empty()).map(move |(j, _)| (i, j))).collect();
    let margin: u16 = if off_pins.is_empty() { 1 } else { 2 };
    let all_shapes = shapes_of(c, turned);
    let all_twos: Vec<usize> = (0..c.on.len()).filter(|k| look_on(c, *k) == Look::TwoLead).collect();
    let width = board_width(&all_shapes, all_twos.len(), opt, gap, margin).max(wants.kept_width());
    let net_of_lead = |refdes: &str, pin: &str| lead_net(c, refdes, pin);
    // the sections, or the whole board as one; an off-board part goes with the section that has the most of its nets
    let slices: Vec<(String, BTreeSet<String>)> = if wants.sections.is_empty() { vec![(String::new(), c.on.iter().map(|p| p.0.clone()).collect())] } else { wants.sections.clone() };
    let nets_in = |parts: &BTreeSet<String>| -> BTreeSet<String> { c.on.iter().filter(|p| parts.contains(&p.0)).flat_map(|p| p.2.iter().map(|x| x.1.clone())).collect() };
    let off_slice = |i: usize| -> usize { (0..slices.len()).max_by_key(|k| (c.off[i].1.iter().filter(|(_, n)| nets_in(&slices[*k].1).contains(n)).count(), Reverse(*k))).unwrap_or(0) };
    let mut parts: Vec<StripPart> = vec![];
    let mut blocked: BTreeSet<At> = BTreeSet::new();
    let mut at_net: BTreeMap<String, Vec<At>> = BTreeMap::new();
    // each lead's net by its hole (a lead next to another net's crowds the wires)
    let mut lead_net_at: BTreeMap<At, String> = BTreeMap::new();
    let mut off: Vec<(String, Vec<crate::stripboard::OffWire>)> = c.off.iter().map(|(r, _)| (r.clone(), vec![])).collect();
    let mut boxes: Vec<(String, At, At)> = vec![];
    let mut top = 1u16;
    // the locked parts first, where they are; the others keep `gap` holes off them
    let kept_holes = wants.kept_holes();
    for p in &wants.keep {
        blocked.extend(outline(p));
        for (pin, a) in &p.leads {
            at_net.entry(net_of_lead(&p.refdes, pin)).or_default().push(*a);
            lead_net_at.insert(*a, net_of_lead(&p.refdes, pin));
        }
        parts.push(p.clone());
    }
    let clear = |y: u16, x: u16, h: u16, w: u16| (y.saturating_sub(gap)..y + h + gap).all(|r| (x.saturating_sub(gap)..x + w + gap).all(|cc| !kept_holes.contains(&(r, cc))));
    for (si, (name, members)) in slices.iter().enumerate() {
        let mut bottom = top;
        // the wires from off the board come in at the left edge, every other row (each has a free hole of its own beside it)
        let mut off_row = top;
        for (i, j) in off_pins.iter().copied().filter(|(i, _)| off_slice(*i) == si) {
            let (pin, net) = &c.off[i].1[j];
            while blocked.contains(&(off_row, 0)) || blocked.contains(&(off_row, 1)) {
                off_row += 1;
            }
            let a = (off_row, 0);
            off[i].1.push((pin.clone(), net.clone(), a));
            at_net.entry(net.clone()).or_default().push(a);
            lead_net_at.insert(a, net.clone());
            blocked.insert(a);
            bottom = bottom.max(off_row + 1);
            off_row += 2;
        }
        // the order: the headers first (the top edge), then each chip next to the ones it shares a group or the most nets with
        let nets_of = |s: &Shape| -> BTreeSet<String> { s.leads.iter().map(|x| x.1.clone()).filter(|n| !n.is_empty() && !is_ground_name(n) && !c.supplies.contains(n)).collect() };
        let shapes: Vec<&Shape> = all_shapes.iter().filter(|s| members.contains(&c.on[s.k].0) && !wants.kept(&c.on[s.k].0)).collect();
        let (mut heads, mut rest): (Vec<&Shape>, Vec<&Shape>) = shapes.into_iter().partition(|s| is_head(s));
        heads.sort_by_key(|s| Reverse(s.w));
        let mut order: Vec<&Shape> = heads;
        let mut seen: BTreeSet<String> = order.iter().flat_map(|s| nets_of(s)).collect();
        rest.sort_by_key(|s| Reverse(s.leads.len()));
        while !rest.is_empty() {
            let last = order.last().map(|s| c.on[s.k].0.clone());
            let i = (0..rest.len())
                .max_by_key(|i| {
                    let mate = last.as_ref().is_some_and(|l| wants.together(l, &c.on[rest[*i].k].0));
                    (mate, nets_of(rest[*i]).intersection(&seen).count(), rest[*i].leads.len(), Reverse(*i))
                })
                .unwrap();
            let s = rest.remove(i);
            seen.extend(nets_of(s));
            order.push(s);
        }
        // the chips in rows (shelves), left to right; one kept apart from a part placed moves on along the row (or to the next)
        let (mut x, mut y, mut shelf) = (margin, top, 0u16);
        // with targets: the first row below the headers, where the other chips may go
        let mut below: Option<u16> = None;
        for s in order {
            let refdes = &c.on[s.k].0;
            let leads_at = |y: u16, x: u16| -> Vec<At> { s.leads.iter().map(|(_, _, a)| (y + a.0, x + a.1)).collect() };
            if let Some(t) = targets.and_then(|t| t.get(refdes)).filter(|_| !is_head(s)) {
                let y0 = *below.get_or_insert(if shelf > 0 { y + shelf + gap } else { top });
                let (ty, tx) = target_place(s, *t, y0, gap, margin, width, &blocked, |yy, xx| wants.far_enough(refdes, &leads_at(yy, xx), &parts)).unwrap_or((bottom + gap, margin));
                for r in ty..ty + s.h {
                    for cc in tx..tx + s.w {
                        blocked.insert((r, cc));
                    }
                }
                let leads: Vec<(String, At)> = s.leads.iter().map(|(p, _, a)| (p.clone(), (ty + a.0, tx + a.1))).collect();
                for (p, a) in &leads {
                    at_net.entry(net_of_lead(refdes, p)).or_default().push(*a);
                    lead_net_at.insert(*a, net_of_lead(refdes, p));
                }
                parts.push(StripPart { refdes: refdes.clone(), look: s.look, leads, mount: c.mounts.get(refdes).cloned().unwrap_or_default() });
                bottom = bottom.max(ty + s.h);
                continue;
            }
            for _ in 0..4000 {
                if x > margin && x + s.w > width.saturating_sub(1) {
                    y += shelf + gap;
                    x = margin;
                    shelf = 0;
                }
                if clear(y, x, s.h, s.w) && wants.far_enough(refdes, &leads_at(y, x), &parts) {
                    break;
                }
                x += 1;
            }
            for r in y..y + s.h {
                for cc in x..x + s.w {
                    blocked.insert((r, cc));
                }
            }
            let leads: Vec<(String, At)> = s.leads.iter().map(|(p, _, a)| (p.clone(), (y + a.0, x + a.1))).collect();
            for (p, a) in &leads {
                at_net.entry(net_of_lead(refdes, p)).or_default().push(*a);
                lead_net_at.insert(*a, net_of_lead(refdes, p));
            }
            parts.push(StripPart { refdes: refdes.clone(), look: s.look, leads, mount: c.mounts.get(refdes).cloned().unwrap_or_default() });
            x += s.w + gap;
            shelf = shelf.max(s.h);
            bottom = bottom.max(y + shelf);
        }
        // the two-lead parts: the most joined to what is placed first, each where its leads are nearest their nets' leads (and its
        // group's parts), kept apart as the table says, in the section's rows
        let mut twos: Vec<usize> = all_twos.iter().copied().filter(|k| members.contains(&c.on[*k].0) && !wants.kept(&c.on[*k].0)).collect();
        twos.sort_by_key(|k| Reverse(c.on[*k].2.iter().filter(|(_, n)| at_net.contains_key(n)).count()));
        for k in twos {
            let (refdes, look, pins) = &c.on[k];
            if pins.len() != 2 {
                continue;
            }
            let near = |n: &str, a: At, at_net: &BTreeMap<String, Vec<At>>| -> u32 { at_net.get(n).map(|v| v.iter().map(|b| a.0.abs_diff(b.0) as u32 + a.1.abs_diff(b.1) as u32).min().unwrap_or(0)).unwrap_or(0) };
            let mates: Vec<At> = parts.iter().filter(|p| wants.together(refdes, &p.refdes)).flat_map(|p| p.leads.iter().map(|x| x.1)).collect();
            let mut best: Option<(u32, At, At)> = None;
            let reach = bottom.max(wants.keep.iter().flat_map(|p| p.leads.iter().map(|x| x.1 .0 + 1)).max().unwrap_or(0));
            for r in top..reach + 4 {
                for cc in 0..width {
                    for (dr, dc) in [(0, SPAN), (SPAN, 0)] {
                        let (a, b) = ((r, cc), (r + dr, cc + dc));
                        // the last column left free, as the chips leave it (else the board is a hole wider than asked)
                        if b.1 + 1 >= width {
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
                                    y >= top as i32 - 1 && x >= 0 && x < width as i32 && !blocked.contains(&(y as u16, x as u16)) && !holes.contains(&(y as u16, x as u16))
                                })
                                .count()
                        };
                        // and no lead already placed beside it is walled in (left without a free hole beside it)
                        let walls = holes.iter().flat_map(|h| beside4(*h, width)).any(|n| {
                            !holes.contains(&n) && lead_net_at.contains_key(&n) && beside4(n, width).all(|m| blocked.contains(&m) || holes.contains(&m))
                        });
                        if walls || room(a) < 2 || room(b) < 2 || !wants.far_enough(refdes, &[a, b], &parts) {
                            continue;
                        }
                        let group = mates.iter().map(|m| a.0.abs_diff(m.0) as u32 + a.1.abs_diff(m.1) as u32).min().unwrap_or(0);
                        for (p, q) in [(a, b), (b, a)] {
                            // a lead next to another lead of another net crowds the wires
                            let crowd = |h: At, n: &str| -> u32 {
                                [(0i32, 1i32), (1, 0), (0, -1), (-1, 0)]
                                    .iter()
                                    .filter(|(dr, dc)| {
                                        let (y, x) = (h.0 as i32 + dr, h.1 as i32 + dc);
                                        y >= 0 && x >= 0 && lead_net_at.get(&(y as u16, x as u16)).is_some_and(|m| m != n)
                                    })
                                    .count() as u32
                            };
                            let aim = targets.and_then(|t| t.get(refdes)).map_or(0, |t| (3.0 * (((p.0 + q.0) as f64 / 2.0 - t.0).abs() + ((p.1 + q.1) as f64 / 2.0 - t.1).abs())) as u32);
                            let cost = 4 * (near(&pins[0].1, p, &at_net) + near(&pins[1].1, q, &at_net)) + group + crowd(p, &pins[0].1) + crowd(q, &pins[1].1) + aim;
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
            lead_net_at.insert(p, pins[0].1.clone());
            lead_net_at.insert(q, pins[1].1.clone());
            parts.push(StripPart { refdes: refdes.clone(), look: *look, leads: vec![(pins[0].0.clone(), p), (pins[1].0.clone(), q)], mount: c.mounts.get(refdes).cloned().unwrap_or_default() });
        }
        if !wants.sections.is_empty() {
            boxes.push((name.clone(), (top, 0), (bottom, width - 1)));
        }
        top = bottom + gap;
    }
    // then made better by annealing, each part within its section's rows (or the rows used)
    if sa {
        let last = parts.iter().flat_map(|p| p.leads.iter().map(|x| x.1 .0)).max().unwrap_or(1);
        let bands: Vec<(u16, u16)> = if boxes.is_empty() { vec![(1, last)] } else { boxes.iter().map(|b| (b.1 .0, b.2 .0)).collect() };
        anneal(c, wants, &mut parts, &off, width, margin, &bands);
    }
    // the parts in their schematic's order
    let pos = |r: &str| c.on.iter().position(|x| x.0 == r).unwrap_or(usize::MAX);
    parts.sort_by_key(|p| (p.look == Look::TwoLead, pos(&p.refdes)));
    let mut l = StripLayout { perf: true, parts, off, sections: boxes, ..Default::default() };
    let far = l.used().iter().map(|(a, _)| *a).fold((0u16, 0u16), |m, a| (m.0.max(a.0), m.1.max(a.1)));
    l.rows = far.0 + 2;
    l.cols = (far.1 + 2).max(width.min(far.1 + 2));
    // the sections' bands reach the board's edges and each other: from the top (or the band above) to the band below (or the bottom)
    let (rows, cols) = (l.rows, l.cols);
    let tops: Vec<u16> = l.sections.iter().map(|s| s.1 .0).collect();
    for (i, s) in l.sections.iter_mut().enumerate() {
        s.1 = (if i == 0 { 0 } else { tops[i] }, 0);
        s.2 = (tops.get(i + 1).map(|t| t - 1).unwrap_or(rows - 1), cols - 1);
    }
    l
}

/// How long the tries go on (the best so far is kept): a big board does not hold the screen up long.
const BUDGET: std::time::Duration = std::time::Duration::from_millis(1500);
/// Once every net is joined, how many times more the nets are wired with the ones that needed wires on top first (fewer of them).
const MORE_ROUNDS: usize = 3;

/// How many times the nets are wired again with the ones left apart first.
const ROUNDS: usize = 4;

/// A net of two leads this many holes apart or nearer (along the rows and columns) is wired before the other nets.
const SHORT: usize = 10;

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
    let mut nets: Vec<(u32, Vec<At>, Option<usize>)> = vec![];
    for (a, i) in &leads {
        match nets.iter_mut().find(|x| x.0 == *i) {
            Some(x) => x.1.push(*a),
            None => nets.push((*i, vec![*a], None)),
        }
    }
    // with sections, ground is wired within each section, and each section's ground goes back to the first's by a wire of its own
    // (the one ground point: one section's return current does not flow through another's ground)
    let sections = l.sections.clone();
    let inside = |k: usize, a: At| {
        let (_, p, q) = &sections[k];
        a.0 >= p.0 && a.0 <= q.0 && a.1 >= p.1 && a.1 <= q.1
    };
    if !l.sections.is_empty() {
        let mut split = vec![];
        for (i, v, _) in std::mem::take(&mut nets) {
            if is_ground_name(&names[i as usize]) {
                for k in 0..l.sections.len() {
                    let part: Vec<At> = v.iter().copied().filter(|a| inside(k, *a)).collect();
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
    let grounds: Vec<(u32, Vec<At>, Option<usize>)> = nets.iter().filter(|x| x.2.is_some()).cloned().collect();
    nets.retain(|(_, v, _)| v.len() > 1);
    let rail = |i: u32| is_ground_name(&names[i as usize]) || c.supplies.contains(&names[i as usize]);
    // a net of two leads near each other (a short bare wire), wired before the others cut across it (a header's pin to the chip
    // under it would else be jumped by a wire on top)
    let short = |v: &[At]| v.len() == 2 && (v[0].0.abs_diff(v[1].0) + v[0].1.abs_diff(v[1].1)) as usize <= SHORT;
    nets.sort_by_key(|(i, v, _)| {
        let (r0, r1) = (v.iter().map(|a| a.0).min().unwrap(), v.iter().map(|a| a.0).max().unwrap());
        let (c0, c1) = (v.iter().map(|a| a.1).min().unwrap(), v.iter().map(|a| a.1).max().unwrap());
        let ahead = first.iter().position(|n| *n == names[*i as usize]).unwrap_or(usize::MAX);
        (ahead, !short(v), rail(*i) != rails_first, v.len(), (r1 - r0) + (c1 - c0))
    });
    let mut link_ends: BTreeSet<At> = BTreeSet::new();
    // a state is a hole and how the way came in: 0..4 a step that way, 4 the start, 5 landed from a jump
    const WAYS: usize = 6;
    let all_nets: Vec<(u32, Vec<At>, Option<usize>)> = nets.clone();
    let mut ground_trees: BTreeMap<usize, Vec<At>> = BTreeMap::new();
    for (n_at, (net, terms, bx)) in nets.into_iter().enumerate() {
        // the free holes beside the leads of the nets still to wire, when a lead has two or fewer: kept for that net (another net's
        // wire there would shut the lead in)
        let mut kept: BTreeMap<At, u32> = BTreeMap::new();
        for (other, ts, _) in all_nets.iter().skip(n_at + 1) {
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
            // where this net may go, the leads, the wires' ends and the leads left, hole by hole (looked up for every way tried)
            let mut open = vec![false; rows * cols];
            let mut is_lead = vec![false; rows * cols];
            let mut is_end = vec![false; rows * cols];
            let mut is_left = vec![false; rows * cols];
            for (h, o) in open.iter_mut().enumerate() {
                let a = hole(h);
                *o = owner[h].is_none_or(|o| o == net) && !link_ends.contains(&a) && !avoid.contains(&a) && kept.get(&a).is_none_or(|o| *o == net) && escape.get(&a).is_none_or(|o| *o == net) && bx.is_none_or(|k| inside(k, a));
            }
            for a in &lead_holes {
                is_lead[idx(*a)] = true;
            }
            for a in &link_ends {
                is_end[idx(*a)] = true;
            }
            for a in &left {
                is_left[idx(*a)] = true;
            }
            let mut found = None;
            while let Some(Reverse((d, s))) = heap.pop() {
                let s = s as usize;
                if d > dist[s] {
                    continue;
                }
                let (h, way) = (s / WAYS, s % WAYS);
                let at = hole(h);
                if is_left[h] {
                    found = Some(s);
                    break;
                }
                // a lead is an end, not a hole to pass through on the way to another (its wire would run under the part)
                if way != 4 && is_lead[h] {
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
                let can_jump = way != 5 && !is_lead[h] && !is_end[h];
                for (w, (dr, dc)) in DIRS.iter().enumerate() {
                    let (r, cc) = (at.0 as i32 + dr, at.1 as i32 + dc);
                    if r >= 0 && cc >= 0 && r < rows as i32 && cc < cols as i32 {
                        let to = (r as u16, cc as u16);
                        // no going straight back (a way back to where it jumped from could jump again from the same hole)
                        if open[idx(to)] && !(way < 4 && (way + 2) % 4 == w) {
                            let turn = way < 4 && way != w;
                            push(to, w, d + 2 + if turn { 3 } else { 0 }, &mut heap);
                        }
                    }
                    if !can_jump {
                        continue;
                    }
                    // over something in the way, to the first free hole past it (a jump over free holes is no better than steps, and
                    // past the first free hole the steps cost what a longer jump would)
                    let mut over = false;
                    for k in 1..=JUMP {
                        let (r, cc) = (at.0 as i32 + dr * k, at.1 as i32 + dc * k);
                        if r < 0 || cc < 0 || r >= rows as i32 || cc >= cols as i32 {
                            break;
                        }
                        let to = idx((r as u16, cc as u16));
                        let free = open[to] && !is_lead[to];
                        if k >= 2 && over && free {
                            push((r as u16, cc as u16), 5, d + JUMP_COST + 2 * k as u32, &mut heap);
                            break;
                        }
                        over |= !free;
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
            // the jumps of one way made one wire on top, from where the first starts to where the last lands (a wire on top goes
            // straight, slanting if need be; the holes between are left free)
            let jumps: Vec<usize> = (1..path.len()).filter(|i| path[i - 1].0.abs_diff(path[*i].0) + path[i - 1].1.abs_diff(path[*i].1) > 1).collect();
            if jumps.len() > 1 {
                let (i0, i1) = (jumps[0], *jumps.last().unwrap());
                path.drain(i0..i1);
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
        if let Some(k) = bx {
            ground_trees.insert(k, tree);
        }
    }
    // each section's ground to the first section's: one wire on top between the nearest two holes of their copper (a hole beside a
    // lead when a section's ground is one lead)
    for (_, v, k) in &grounds {
        if let Some(k) = k {
            ground_trees.entry(*k).or_insert_with(|| vec![v[0]]);
        }
    }
    let Some((&hub, _)) = ground_trees.iter().next() else { return };
    let gnd = grounds.first().map(|g| g.0);
    let end_on = |tree: &[At], k: usize, owner: &mut Vec<Option<u32>>, link_ends: &BTreeSet<At>, l: &mut StripLayout, toward: At| -> Option<At> {
        let ok = |h: &At| !lead_holes.contains(h) && !link_ends.contains(h) && inside(k, *h);
        let dist = |h: &At| h.0.abs_diff(toward.0) as u32 + h.1.abs_diff(toward.1) as u32;
        if let Some(h) = tree.iter().filter(|h| ok(h)).min_by_key(|h| dist(h)) {
            return Some(*h);
        }
        // a lead only: a free hole beside it, joined by a bare wire
        let (lead, h) = tree.iter().flat_map(|a| beside(*a).into_iter().map(move |h| (*a, h))).filter(|(_, h)| ok(h) && owner[idx(*h)].is_none()).min_by_key(|(_, h)| dist(h))?;
        owner[idx(h)] = gnd;
        l.bare.push((lead, h));
        Some(h)
    };
    let hub_tree = ground_trees[&hub].clone();
    for (&k, tree) in ground_trees.iter().filter(|(k, _)| **k != hub) {
        let toward = hub_tree[0];
        let Some(a) = end_on(tree, k, &mut owner, &link_ends, l, toward) else { continue };
        let Some(b) = end_on(&hub_tree, hub, &mut owner, &link_ends, l, a) else { continue };
        link_ends.insert(a);
        link_ends.insert(b);
        l.links.push(Link { a, b, net: gnd.map(|g| names[g as usize].clone()).unwrap_or_default() });
    }
}

/// The lines a hand-edited perfboard still lacks, wired (B5a: the parts added to the schematic after the board was edited by hand):
/// each pair (net, a, b) of `pairs` (the ratsnest's lines that end at a new part) by the cheapest way from the copper `a` is on to the
/// copper `b` is on, over free holes and the net's own copper, as the automatic layout wires a net (`route`): a step to the next
/// hole, more for a turn, and a jump over what is in the way to the first free hole beyond (a link on top). The straight runs of
/// steps are bare wires, each jump a link. The rest of the board is left as the person made it; two rows and columns round it may be
/// used (the board grows). `hole_nets` is each lead's and off-board wire's net. A pair with no way is left (the check names it).
pub(crate) fn wire_added(l: &mut StripLayout, hole_nets: &BTreeMap<At, String>, pairs: &[(String, At, At)]) {
    // the pairs that found no way go first the next time (a few times), the try that leaves the fewest apart (then the fewest
    // links) kept
    let mut order: Vec<(String, At, At)> = pairs.to_vec();
    let mut best: Option<(usize, usize, StripLayout)> = None;
    for _ in 0..4 {
        let mut t = l.clone();
        let failed = wire_try(&mut t, hole_nets, &order);
        let key = (failed.len(), t.links.len());
        if best.as_ref().is_none_or(|b| key < (b.0, b.1)) {
            best = Some((key.0, key.1, t));
        }
        if failed.is_empty() {
            break;
        }
        let (mut first, rest): (Vec<_>, Vec<_>) = order.into_iter().partition(|p| failed.contains(p));
        first.extend(rest);
        order = first;
    }
    if let Some((_, _, t)) = best {
        *l = t;
    }
}

/// One try of `wire_added` in the order given; the pairs left without a way.
fn wire_try(l: &mut StripLayout, hole_nets: &BTreeMap<At, String>, pairs: &[(String, At, At)]) -> Vec<(String, At, At)> {
    let mut failed = vec![];
    let (rows, cols) = (l.rows as usize + 2, l.cols as usize + 2);
    let idx = |a: At| a.0 as usize * cols + a.1 as usize;
    let hole = |h: usize| ((h / cols) as u16, (h % cols) as u16);
    let fits = |a: At| (a.0 as usize) < rows && (a.1 as usize) < cols;
    let mut parent: Vec<usize> = (0..rows * cols).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut i = i;
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let mut taken = vec![false; rows * cols];
    let mut lead = vec![false; rows * cols];
    let mut link_end = vec![false; rows * cols];
    for a in l.used().into_iter().map(|(a, _)| a).filter(|a| fits(*a)) {
        taken[idx(a)] = true;
    }
    for p in &l.parts {
        for (_, a) in p.leads.iter().filter(|(_, a)| fits(*a)) {
            lead[idx(*a)] = true;
        }
    }
    for (_, v) in &l.off {
        for (_, _, a) in v.iter().filter(|w| fits(w.2)) {
            lead[idx(*a)] = true;
        }
    }
    for k in l.links.iter().filter(|k| fits(k.a) && fits(k.b)) {
        link_end[idx(k.a)] = true;
        link_end[idx(k.b)] = true;
        let (x, y) = (find(&mut parent, idx(k.a)), find(&mut parent, idx(k.b)));
        parent[x] = y;
    }
    let along = |a: At, b: At| -> Vec<At> { if a.0 == b.0 { (a.1.min(b.1)..=a.1.max(b.1)).map(|c| (a.0, c)).collect() } else { (a.0.min(b.0)..=a.0.max(b.0)).map(|r| (r, a.1)).collect() } };
    for (a, b) in l.bare.clone() {
        let hs: Vec<At> = along(a, b).into_iter().filter(|h| fits(*h)).collect();
        for w in hs.windows(2) {
            let (x, y) = (find(&mut parent, idx(w[0])), find(&mut parent, idx(w[1])));
            parent[x] = y;
        }
        for h in hs {
            taken[idx(h)] = true;
        }
    }
    // each piece of copper's net (from a lead on it)
    let mut net_of: BTreeMap<usize, String> = BTreeMap::new();
    for (a, n) in hole_nets.iter().filter(|(a, _)| fits(**a)) {
        let r = find(&mut parent, idx(*a));
        net_of.entry(r).or_insert_with(|| n.clone());
    }
    const DIRS: [(i32, i32); 4] = [(0, 1), (1, 0), (0, -1), (-1, 0)];
    let step = |h: usize, d: usize, k: i32| -> Option<usize> {
        let (r, c) = (h / cols, h % cols);
        let (y, x) = (r as i32 + DIRS[d].0 * k, c as i32 + DIRS[d].1 * k);
        (y >= 0 && x >= 0 && (y as usize) < rows && (x as usize) < cols).then(|| y as usize * cols + x as usize)
    };
    for (n_at, (net, a, b)) in pairs.iter().enumerate() {
        if !fits(*a) || !fits(*b) {
            failed.push((net.clone(), *a, *b));
            continue;
        }
        let (ra, rb) = (find(&mut parent, idx(*a)), find(&mut parent, idx(*b)));
        if ra == rb {
            continue;
        }
        let roots: Vec<usize> = (0..rows * cols).map(|h| find(&mut parent, h)).collect();
        let own = |h: usize| taken[h] && net_of.get(&roots[h]) == Some(net);
        // the free holes beside the copper a lead of another net's pair still to come is on (the lead and what is joined to it), when
        // it has two or fewer: kept for it (a lead with no free hole of its own but copper going on elsewhere keeps nothing)
        let mut kept = vec![false; rows * cols];
        for (m, x, y) in &pairs[n_at + 1..] {
            if m == net {
                continue;
            }
            for e in [*x, *y].into_iter().filter(|e| fits(*e) && lead[idx(*e)]) {
                let piece = roots[idx(e)];
                let mut free: Vec<usize> = (0..rows * cols).filter(|h| roots[*h] == piece).flat_map(|h| (0..4).filter_map(move |d| step(h, d, 1))).filter(|h| !taken[*h]).collect();
                free.sort_unstable();
                free.dedup();
                if free.len() <= 2 {
                    for h in free {
                        kept[h] = true;
                    }
                }
            }
        }
        let open = |h: usize| (!taken[h] && !kept[h]) || own(h);
        // states: hole × the way the last step went (4: none yet, or just after a jump)
        let mut cost = vec![u32::MAX; rows * cols * 5];
        let mut prev: Vec<Option<(usize, bool)>> = vec![None; rows * cols * 5];
        let mut heap = BinaryHeap::new();
        for h in (0..rows * cols).filter(|h| roots[*h] == ra) {
            cost[h * 5 + 4] = 0;
            heap.push(Reverse((0u32, h * 5 + 4)));
        }
        let mut end = None;
        while let Some(Reverse((c0, s))) = heap.pop() {
            if c0 > cost[s] {
                continue;
            }
            let (h, way) = (s / 5, s % 5);
            if roots[h] == rb {
                end = Some(s);
                break;
            }
            for d in 0..4 {
                // a step (no going straight back)
                if way < 4 && (d + 2) % 4 == way {
                    continue;
                }
                if let Some(n) = step(h, d, 1)
                    && open(n)
                {
                    let c1 = c0 + 2 + if way < 4 && way != d { 3 } else { 0 };
                    let t = n * 5 + d;
                    if c1 < cost[t] {
                        cost[t] = c1;
                        prev[t] = Some((s, false));
                        heap.push(Reverse((c1, t)));
                    }
                }
                // a jump: from a hole that is not a lead or a link's end, over what is in the way, to the first hole beyond it that
                // a link may end on (free, or the net's own copper that is not a lead or a link's end; the check allows one thing in a
                // hole); not again straight after a jump (way 4 off the start's copper)
                if lead[h] || link_end[h] || (way == 4 && roots[h] != ra) {
                    continue;
                }
                let mut over = false;
                for k in 1..=JUMP {
                    let Some(n) = step(h, d, k) else { break };
                    if lead[n] || link_end[n] || !open(n) {
                        over = true;
                        continue;
                    }
                    // a hole right beside is a step, not a jump
                    if over {
                        let c1 = c0 + JUMP_COST + 2 * k as u32;
                        let t = n * 5 + 4;
                        if c1 < cost[t] {
                            cost[t] = c1;
                            prev[t] = Some((s, true));
                            heap.push(Reverse((c1, t)));
                        }
                    }
                    break;
                }
            }
        }
        let Some(mut s) = end else {
            failed.push((net.clone(), *a, *b));
            continue;
        };
        // the way back to the start: each move (from, to, a jump?)
        let mut moves: Vec<(At, At, bool)> = vec![];
        while let Some((p, jump)) = prev[s] {
            moves.push((hole(p / 5), hole(s / 5), jump));
            s = p;
        }
        moves.reverse();
        let mut run: Option<(At, At)> = None;
        let mut done: Vec<At> = vec![];
        for (from, to, jump) in moves {
            done.push(from);
            done.push(to);
            if jump {
                if let Some(r) = run.take() {
                    l.bare.push(r);
                }
                l.links.push(Link { a: from, b: to, net: net.clone() });
                link_end[idx(from)] = true;
                link_end[idx(to)] = true;
                continue;
            }
            run = match run {
                Some((p, q)) if (p.0 == q.0 && q.0 == to.0) || (p.1 == q.1 && q.1 == to.1) => Some((p, to)),
                Some(r) => {
                    l.bare.push(r);
                    Some((from, to))
                }
                None => Some((from, to)),
            };
        }
        if let Some(r) = run {
            l.bare.push(r);
        }
        // the new copper is the net's: later pairs go over it, the other nets round it
        for h in done {
            let i = idx(h);
            taken[i] = true;
            let (x, y) = (find(&mut parent, i), find(&mut parent, ra));
            parent[x] = y;
        }
        let (x, y) = (find(&mut parent, rb), find(&mut parent, ra));
        parent[x] = y;
        let r = find(&mut parent, ra);
        net_of.insert(r, net.clone());
    }
    failed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stripboard::{strip_check, strip_layout, strip_layout_with};

    fn load(name: &str) -> Schematic {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// A sample laid out on a perfboard (`opt`): it joins what its schematic joins and nothing more; every bare wire runs straight
    /// along a row or a column; no lead or wire's end shares a hole; all on the board. With the layout, the stripboard's holes.
    fn laid_out(name: &str, opt: StripOptions) -> (StripLayout, u32) {
        let s = load(name);
        let l = perf_layout_with(&s, opt);
        assert!(l.perf);
        assert!(strip_check(&s, &l).is_empty(), "{name}: {:?}", strip_check(&s, &l));
        assert!(l.bare.iter().all(|(a, b)| a.0 == b.0 || a.1 == b.1), "{name}: straight wires");
        assert!(l.used().iter().all(|(a, _)| a.0 < l.rows && a.1 < l.cols), "{name}: on the board");
        let st = strip_layout(&s);
        (l, st.rows as u32 * st.cols as u32)
    }

    /// Samples laid out on a perfboard as drawn (`laid_out`), on boards far smaller than the stripboard's, with few wires on top.
    #[test]
    fn samples_lay_out_on_a_perfboard_as_drawn_and_small() {
        let (mut perf, mut strip, mut on_top) = (0u32, 0u32, 0usize);
        for name in ["ne555_blinker.schjson", "active_filter.schjson", "current_sensors.schjson"] {
            let (l, st) = laid_out(name, StripOptions::default());
            on_top += l.links.len();
            perf += l.rows as u32 * l.cols as u32;
            strip += st;
        }
        // the holes: 369 with the shelves alone, 442 since `simpl` (its boards are bigger, about ten holes for each wire on top fewer)
        assert!(perf * 3 < strip * 2 && perf <= 460, "{perf} holes on perfboards, {strip} on stripboards");
        // the wires on top (these three with the seven-segment counter): 41 before a way's jumps were one wire and the nets that needed
        // them were wired first (2026-11-20), 32 since, 30 with the chips also tried turned a quarter turn, 27 with the placement also
        // made better by annealing, 19 with the chips also placed by the springs and the spreading (`simpl`, 2026-11-21): these three
        // 8 → 2
        assert!(on_top <= 3, "{on_top} wires on top");
    }

    /// The seven-segment counter (on its own: it takes the most time) as drawn, on a board far smaller than the stripboard's (60 × 22
    /// holes on strips, 21 × 18 here), with few wires on top (19 → 17 with `simpl`); and a width to keep to kept.
    #[test]
    fn the_seven_segment_counter_on_a_perfboard() {
        let (l, strip) = laid_out("seg7_counter.schjson", StripOptions::default());
        assert!(l.rows as u32 * l.cols as u32 * 2 < strip, "{} × {} holes", l.cols, l.rows);
        assert!(l.links.len() <= 18, "{} wires on top", l.links.len());
        let s = load("seg7_counter.schjson");
        assert!(!strip_layout_with(&s, StripOptions::default()).perf, "the stripboard keeps its strips");
    }

    /// The seven-segment counter on a board 16 holes wide: kept to.
    #[test]
    fn a_width_to_keep_to_is_kept() {
        let (l, _) = laid_out("seg7_counter.schjson", StripOptions { cols: Some(16) });
        assert!(l.cols <= 16, "{} wide", l.cols);
    }

    /// A locked part stays where it is and the others are laid out round it: the 555 moved down and right of where the layout put it
    /// and locked; laid out again, it is in the same holes, no other part takes a hole of its outline, the board joins what the
    /// schematic joins, and the lock is kept on the layout.
    #[test]
    fn a_locked_part_stays_and_the_rest_goes_round_it() {
        let s = load("ne555_blinker.schjson");
        let mut l = perf_layout_with(&s, StripOptions::default());
        let u = l.parts.iter_mut().find(|p| p.refdes == "U1").unwrap();
        for (_, a) in u.leads.iter_mut() {
            *a = (a.0 + 6, a.1 + 8);
        }
        let held = u.leads.clone();
        l.locked.insert("U1".into());
        let m = perf_layout_around(&s, StripOptions::default(), &l);
        assert_eq!(m.parts.iter().find(|p| p.refdes == "U1").unwrap().leads, held, "U1 stays");
        let taken: BTreeSet<At> = outline(m.parts.iter().find(|p| p.refdes == "U1").unwrap()).into_iter().collect();
        for p in m.parts.iter().filter(|p| p.refdes != "U1") {
            assert!(outline(p).iter().all(|h| !taken.contains(h)), "{} on U1", p.refdes);
        }
        assert!(strip_check(&s, &m).is_empty(), "{:?}", strip_check(&s, &m));
        assert!(m.locked.contains("U1"));
    }

    /// A wire on top along a chip's row of pins is found (and named by the chip); one across the row, or under the chip, is not.
    #[test]
    fn a_wire_along_a_chips_pins_is_found() {
        let s = load("ne555_blinker.schjson");
        let mut l = perf_layout_with(&s, StripOptions::default());
        let u = l.parts.iter().find(|p| p.refdes == "U1").unwrap().clone();
        let (r0, r1) = (u.leads.iter().map(|x| x.1 .0).min().unwrap(), u.leads.iter().map(|x| x.1 .0).max().unwrap());
        let (c0, c1) = (u.leads.iter().map(|x| x.1 .1).min().unwrap(), u.leads.iter().map(|x| x.1 .1).max().unwrap());
        // its pins in two columns (upright) or two rows (turned): a wire past both ends of one of them runs along four pins; one
        // across the two, over a pin of each, does not
        let upright = u.leads.iter().map(|x| x.1 .1).collect::<BTreeSet<u16>>().len() == 2;
        let before = links_along_pins(&l).len();
        let k = l.links.len();
        let (along, across) = if upright {
            (Link { a: (r0.saturating_sub(1), c0), b: (r1 + 1, c0), net: "X".into() }, Link { a: (r0, c0.saturating_sub(1)), b: (r0, c1 + 1), net: "Y".into() })
        } else {
            (Link { a: (r0, c0.saturating_sub(1)), b: (r0, c1 + 1), net: "X".into() }, Link { a: (r0.saturating_sub(1), c0), b: (r1 + 1, c0), net: "Y".into() })
        };
        l.links.push(along);
        l.links.push(across);
        let found = links_along_pins(&l);
        assert_eq!(found.len(), before + 1, "{found:?}");
        assert!(found.contains(&(k, "U1".to_string())));
    }

    /// The shelves as placed wall no lead in: every lead keeps a free hole beside it for its wire (arduino_mega_usb's USB connector,
    /// a row of five pins, had two-lead parts put right above and right below its middle pin, and its net could not be wired). The
    /// annealing may still do it (keeping it from that made the samples' wires on top 265 → 276); a placing that leaves a net apart
    /// loses to the others.
    #[test]
    fn no_lead_is_walled_in() {
        let s = load("arduino_mega_usb.schjson");
        let c = read(&s);
        let wants = Wants::read(&s, &c);
        for turned in [false, true] {
            let l = place(&c, StripOptions::default(), GAPS[0], turned, false, &wants, None);
            let mut taken: BTreeSet<At> = l.off.iter().flat_map(|(_, v)| v.iter().map(|w| w.2)).collect();
            for p in &l.parts {
                let (r0, r1) = (p.leads.iter().map(|x| x.1 .0).min().unwrap(), p.leads.iter().map(|x| x.1 .0).max().unwrap());
                let (c0, c1) = (p.leads.iter().map(|x| x.1 .1).min().unwrap(), p.leads.iter().map(|x| x.1 .1).max().unwrap());
                taken.extend((r0..=r1).flat_map(|r| (c0..=c1).map(move |cc| (r, cc))));
            }
            for p in &l.parts {
                for (pin, a) in &p.leads {
                    assert!(beside4(*a, l.cols).any(|n| !taken.contains(&n)), "turned {turned}: {}.{pin} at {a:?} walled in", p.refdes);
                }
            }
        }
    }

    /// The requirements table on a perfboard: two sections, each in a band of rows of its own with all its parts in it, ground
    /// wired within each and back to the first section's by one wire on top; and a pair of chips kept apart by a distance placed
    /// that far apart. The check and the rules both pass.
    #[test]
    fn sections_and_distances_from_the_table() {
        let mut s = load("seg7_counter.schjson");
        let group = |name: &str, parts: &[&str]| design_spec::PlaceGroup { name: name.into(), parts: parts.iter().map(|p| p.to_string()).collect(), section: true, ..Default::default() };
        s.requirements.groups = vec![group("カウンタ", &["U1", "U3"]), group("表示", &["U2", "DS1", "R1", "R2", "R3", "R4", "R5", "R6", "R7"])];
        s.requirements.apart = vec![design_spec::KeepApart { name: "離す".into(), a: vec!["U1".into()], b: vec!["U2".into()], min_dist: 25.0, ..Default::default() }];
        let l = perf_layout_with(&s, StripOptions::default());
        assert!(strip_check(&s, &l).is_empty(), "{:?}", strip_check(&s, &l));
        assert_eq!(l.sections.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["カウンタ", "表示"]);
        for (k, (name, p, q)) in l.sections.iter().enumerate() {
            let members = &s.requirements.groups[k].parts;
            for part in l.parts.iter().filter(|x| members.contains(&x.refdes)) {
                assert!(part.leads.iter().all(|(_, a)| a.0 >= p.0 && a.0 <= q.0), "{} in {name}", part.refdes);
            }
        }
        let rules = crate::stripboard::strip_rules(&s, &l);
        assert!(!rules.is_empty() && rules.iter().all(|r| r.ok), "{rules:?}");
        assert!(rules.iter().any(|r| r.message.starts_with("GND:")), "the one ground point is checked: {rules:?}");
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
