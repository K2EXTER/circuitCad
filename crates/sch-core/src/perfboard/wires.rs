//! The wires after the layout: each wire's side and kind (`wire_sides`), the nets left apart wired with the wires kept ("自動配線",
//! `perf_route_left`), the wires on top along a chip's pins (`links_along_pins`), and the lines a hand-edited board still lacks
//! (`wire_added`).

use super::*;

/// The wires on top that run along a chip's (or a row of leads') pins: over two of its pins one after the other, lying on the chip's
/// or the socket's row of pins (the automatic layout may lay one so where nothing else gets through; the person may move its ends).
/// Each wire's index and the part. A wire across a row of pins, under a chip (between its rows) or over a two-lead part is not one,
/// nor a wire on the copper side (`Link::back`).
pub fn links_along_pins(l: &StripLayout) -> Vec<(usize, String)> {
    let pin_at: BTreeMap<At, &str> = l.parts.iter().filter(|p| p.look != Look::TwoLead).flat_map(|p| p.leads.iter().map(move |x| (x.1, p.refdes.as_str()))).collect();
    l.links
        .iter()
        .enumerate()
        .filter(|(_, w)| !w.back)
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

/// The layout with the nets still left apart wired, every wire already on it kept (hand-drawn or automatic): each net in two or more
/// pieces of copper is wired from piece to piece as the automatic wiring goes (bare wires on the copper side, along a row, a column
/// or at 45 degrees, and a wire on top over what is in the way), round the copper already there. The new wires' sides are chosen as
/// the automatic layout's are (`wire_sides`); the old ones are left as they are. Four orders are tried (the supplies first or last,
/// with or without a way out kept by each lead), each again with the nets left apart first and then with the nets that needed wires
/// on top first (as the automatic layout's rounds, up to `STEPS` of work): the one with the fewest nets left apart, then the fewest
/// wires on top. Then each net wired here is wired again alone round all the others, the shorter way kept (`reroute`).
pub fn perf_route_left(s: &Schematic, l: &StripLayout) -> StripLayout {
    let c = read(s);
    let net_of_lead = |r: &str, p: &str| lead_net(&c, r, p);
    let mut best: Option<((usize, usize), StripLayout)> = None;
    let work = std::cell::Cell::new(0u64);
    for (rails_first, escapes) in [(true, false), (false, false), (true, true), (false, true)] {
        // each order again with the nets left apart first, and once all are joined with the nets that needed wires on top first (as
        // the automatic layout's rounds)
        let mut first: Vec<String> = vec![];
        let mut extra = 0;
        for _ in 0..ROUNDS + MORE_ROUNDS {
            if best.is_some() && work.get() > STEPS {
                break;
            }
            let mut m = l.clone();
            route(&c, &mut m, &net_of_lead, Order { rails_first, escapes, first: &first, forks: &[], along: ALONG_COST, bypass: &[], keep: true, work: &work });
            let apart = crate::stripboard::strip_unjoined(s, &m);
            let key = (apart.len(), m.links.len());
            let on_top: Vec<String> = m.links[l.links.len()..].iter().map(|k| k.net.clone()).collect();
            if best.as_ref().is_none_or(|b| key < b.0) {
                best = Some((key, m));
            }
            if apart.is_empty() {
                if extra >= MORE_ROUNDS || on_top.is_empty() {
                    break;
                }
                extra += 1;
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
    let mut m = best.map(|b| b.1).unwrap_or_else(|| l.clone());
    // each net wired here wired again alone round the others, the shorter kept (the wires already on the board left as they are)
    reroute(s, &c, &mut m, &net_of_lead, (l.bare.len(), l.links.len()), &BTreeSet::new(), &work);
    let old = l.links.len();
    let mut new = StripLayout { links: m.links.split_off(old), ..m.clone() };
    wire_sides(&mut new);
    m.links.extend(new.links);
    m
}

/// The layout with its parts where they are and no wires (`StripLayout::links`, `bare`): what "自動配置" leaves, the nets shown as
/// lines to join until "自動配線" (`perf_route_left`) wires them.
pub fn without_wires(l: &StripLayout) -> StripLayout {
    StripLayout { links: vec![], bare: vec![], ..l.clone() }
}

/// Chooses each automatic wire's side and kind once a layout is done (`Link::back`, `Link::tinned`): a wire on top that would lie
/// along a chip's row of pins (`links_along_pins`: on the socket) goes on the copper side, insulated (it may cross the bare wires
/// there); a wire on top along a row or a column that passes over no hole with something coming up through it (a lead, a wire's
/// end), and meets no tinned wire of another net, is a tinned wire (no insulation to strip). Changes no join: the holes a tinned
/// wire touches are its own two.
pub fn wire_sides(l: &mut StripLayout) {
    for (k, _) in links_along_pins(l) {
        l.links[k].back = true;
        l.links[k].tinned = false;
    }
    let mut up: BTreeSet<At> = l.parts.iter().filter(|p| !matches!(p.mount, crate::mount::Mount::Back { .. })).flat_map(|p| p.leads.iter().map(|x| x.1)).collect();
    up.extend(l.links.iter().filter(|k| !k.back).flat_map(|k| [k.a, k.b]));
    up.extend(l.off.iter().flat_map(|(_, v)| v.iter().map(|w| w.2)));
    // the holes each tinned wire on top runs over, by net (those drawn by hand first)
    let mut tinned_over: BTreeMap<At, BTreeSet<String>> = BTreeMap::new();
    for k in l.links.iter().filter(|k| k.tinned && !k.back) {
        for h in crate::stripboard::bare_holes(k.a, k.b) {
            tinned_over.entry(h).or_default().insert(k.net.clone());
        }
    }
    for k in l.links.iter_mut().filter(|k| !k.back && !k.tinned) {
        if k.a.0 != k.b.0 && k.a.1 != k.b.1 {
            continue;
        }
        let holes = crate::stripboard::bare_holes(k.a, k.b);
        let inner = &holes[1..holes.len() - 1];
        let clear = inner.iter().all(|h| !up.contains(h)) && holes.iter().all(|h| tinned_over.get(h).is_none_or(|n| n.iter().all(|x| *x == k.net)));
        if clear {
            k.tinned = true;
            for h in holes {
                tinned_over.entry(h).or_default().insert(k.net.clone());
            }
        }
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
pub(super) fn wire_try(l: &mut StripLayout, hole_nets: &BTreeMap<At, String>, pairs: &[(String, At, At)]) -> Vec<(String, At, At)> {
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
    let along = crate::stripboard::bare_holes;
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
                l.links.push(Link::new(from, to, net.clone()));
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
