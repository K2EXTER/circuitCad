//! Placing the parts (`place`): the shelves (the chips in rows, the two-lead parts by what they join), the springs' targets, and with
//! sections each section's box packed across the board (`place_packed`, `pack`).

use super::*;

/// The sections each in a box of its own, packed across the board instead of one band of rows each (small sections side by side):
/// each section's parts placed alone (`place`, its off-board wires with it) about square, then the boxes in shelves across the
/// board's width, the tallest first, `gap` holes between. None without two sections, or with locked parts (they keep their holes).
pub(super) fn place_packed(c: &Circuit, opt: StripOptions, gap: u16, turned: Turn, sa: bool, wants: &Wants) -> Option<StripLayout> {
    if wants.sections.len() < 2 || !wants.keep.is_empty() {
        return None;
    }
    let has_off = c.off.iter().any(|(_, p)| p.iter().any(|(_, n)| !n.is_empty()));
    let margin: u16 = if has_off { 2 } else { 1 };
    let n_twos = (0..c.on.len()).filter(|k| look_on(c, *k) == Look::TwoLead).count();
    let board = board_width(&shapes_of(c, turned), n_twos, opt, gap, margin);
    // an off-board part goes with the section that has the most of its nets
    let nets_in = |parts: &BTreeSet<String>| -> BTreeSet<String> { c.on.iter().filter(|p| parts.contains(&p.0)).flat_map(|p| p.2.iter().map(|x| x.1.clone())).collect() };
    let sec_nets: Vec<BTreeSet<String>> = wants.sections.iter().map(|(_, m)| nets_in(m)).collect();
    let off_sec: Vec<usize> = c.off.iter().map(|(_, pins)| (0..sec_nets.len()).max_by_key(|k| (pins.iter().filter(|(_, n)| sec_nets[*k].contains(n)).count(), Reverse(*k))).unwrap_or(0)).collect();
    // each section's box in a few widths (about square, narrower, wider, the board's width)
    let mut choices: Vec<(usize, Vec<StripLayout>)> = vec![];
    for (i, (_, members)) in wants.sections.iter().enumerate() {
        let sub = Circuit {
            on: c.on.iter().filter(|p| members.contains(&p.0)).cloned().collect(),
            off: c.off.iter().enumerate().filter(|(k, _)| off_sec[*k] == i).map(|(_, o)| o.clone()).collect(),
            supplies: c.supplies.clone(),
            ground: c.ground.clone(),
            mounts: c.mounts.clone(),
            facing: c.facing.clone(),
            flipped: c.flipped.clone(),
            stack: c.stack,
            apart: c.apart.clone(),
            swaps: c.swaps.clone(),
        };
        if sub.on.is_empty() {
            continue;
        }
        let shapes = shapes_of(&sub, turned);
        let twos = (0..sub.on.len()).filter(|k| look_on(&sub, *k) == Look::TwoLead).count();
        let area: u32 = shapes.iter().map(|s| room_of(s, gap)).sum::<u32>() + twos as u32 * TWO_ROOM;
        let m = if sub.off.iter().any(|(_, p)| p.iter().any(|(_, n)| !n.is_empty())) { 2 } else { 1 };
        let widest = shapes.iter().map(|s| s.w).max().unwrap_or(0) + 2 * m;
        let square = (area as f64 * 1.5).sqrt();
        let mut widths: Vec<u16> = match wants.boxes.get(&wants.sections[i].0) {
            // a box put by hand keeps its width
            Some((_, w)) => vec![(*w).max(widest)],
            None => [0.7, 1.0, 1.4].iter().map(|f| (square * f).ceil() as u16).chain([board]).map(|w| w.max(widest).max(8).min(board.max(widest))).collect(),
        };
        widths.sort_unstable();
        widths.dedup();
        let alone = Wants { apart: wants.apart.clone(), groups: wants.groups.clone(), group_max: wants.group_max.clone(), last: wants.last.clone(), early: wants.early.clone(), bypass: wants.bypass.clone(), ..Default::default() };
        let mut ls: Vec<StripLayout> = widths.into_iter().map(|w| place(&sub, StripOptions { cols: Some(w), ..opt }, gap, turned, sa, &alone, None)).collect();
        ls.dedup_by_key(|l| (l.cols, l.rows));
        choices.push((i, ls));
    }
    // every mix of the widths packed; the one with the fewest rows (then the smallest board)
    let mut pick: Vec<usize> = vec![0; choices.len()];
    let mut best: Option<((u32, u16, u32), StripLayout)> = None;
    loop {
        let blocks: Vec<(usize, &StripLayout)> = choices.iter().zip(&pick).map(|((i, ls), k)| (*i, &ls[*k])).collect();
        let l = pack(c, wants, &blocks, board, gap);
        // (the groups past their farthest distance in whole holes first, then the fewest rows, the smallest board)
        let key = (wants.group_over(&l.parts).ceil() as u32, l.rows, l.rows as u32 * l.cols as u32);
        if best.as_ref().is_none_or(|b| key < b.0) {
            best = Some((key, l));
        }
        // the next mix
        let mut k = 0;
        while k < pick.len() {
            pick[k] += 1;
            if pick[k] < choices[k].1.len() {
                break;
            }
            pick[k] = 0;
            k += 1;
        }
        if k == pick.len() {
            break;
        }
    }
    best.map(|b| b.1)
}

/// The sections' boxes (`blocks`: the section's index and its parts placed alone) packed across a board `board` holes wide: the boxes
/// put by hand (`Wants::boxes`) first where they are, then the others the tallest first, each at the lowest place it fits (then the
/// leftmost), `gap` holes from every box placed, keeping the table's distances from the boxes placed (further down where none across
/// does).
pub(super) fn pack(c: &Circuit, wants: &Wants, blocks: &[(usize, &StripLayout)], board: u16, gap: u16) -> StripLayout {
    let mut blocks: Vec<(usize, &StripLayout)> = blocks.to_vec();
    let pin_of = |i: usize| wants.boxes.get(&wants.sections[i].0).map(|b| b.0);
    blocks.sort_by_key(|(i, l)| (pin_of(*i).is_none(), Reverse(l.rows), *i));
    let mut out = StripLayout { perf: true, ..Default::default() };
    let span = gap.saturating_sub(1);
    let board = board.max(blocks.iter().map(|b| b.1.cols).max().unwrap_or(0));
    // the boxes placed: top-left and bottom-right (inclusive)
    let mut taken: Vec<(At, At)> = vec![];
    let mut boxes: Vec<(usize, String, At, At)> = vec![];
    for (i, l) in blocks {
        let moved = |dy: u16, dx: u16| -> Vec<StripPart> {
            l.parts.iter().map(|p| StripPart { leads: p.leads.iter().map(|(n, a)| (n.clone(), (a.0 + dy, a.1 + dx))).collect(), ..p.clone() }).collect()
        };
        let fits_far = |dy: u16, dx: u16, placed: &[StripPart]| moved(dy, dx).iter().all(|p| wants.far_enough(&p.refdes, &p.leads.iter().map(|q| q.1).collect::<Vec<_>>(), placed));
        // clear of every box placed by `span` free holes on each side
        let clear = |y: u16, x: u16| {
            taken.iter().all(|(a, b)| y + l.rows + span <= a.0 || b.0 + 1 + span <= y || x + l.cols + span <= a.1 || b.1 + 1 + span <= x)
        };
        let (y, x) = match pin_of(i) {
            Some(at) => at,
            None => {
                // the places to try: the top and the left edge, and just past each box placed (below it, right of it)
                let mut ys: Vec<u16> = std::iter::once(0).chain(taken.iter().map(|(_, b)| b.0 + 1 + span)).collect();
                let mut xs: Vec<u16> = std::iter::once(0).chain(taken.iter().map(|(_, b)| b.1 + 1 + span)).collect();
                ys.sort_unstable();
                ys.dedup();
                xs.sort_unstable();
                xs.dedup();
                let found = ys.iter().find_map(|&y| xs.iter().copied().filter(|&x| x + l.cols <= board).find(|&x| clear(y, x) && fits_far(y, x, &out.parts)).map(|x| (y, x)));
                // none keeps the distances: further down, below everything, until one does
                found.unwrap_or_else(|| {
                    let low = taken.iter().map(|(_, b)| b.0 + 1 + span).max().unwrap_or(0);
                    (low..low + 200).find_map(|y| (0..=board.saturating_sub(l.cols)).find(|x| fits_far(y, *x, &out.parts)).map(|x| (y, x))).unwrap_or((low, 0))
                })
            }
        };
        taken.push(((y, x), (y + l.rows - 1, x + l.cols - 1)));
        let at = |a: At| (a.0 + y, a.1 + x);
        for p in &l.parts {
            let mut p = p.clone();
            for (_, a) in &mut p.leads {
                *a = at(*a);
            }
            out.parts.push(p);
        }
        for (r, pins) in &l.off {
            if let Some(e) = out.off.iter_mut().find(|(x, _)| x == r) {
                e.1.extend(pins.iter().map(|(p, n, a)| (p.clone(), n.clone(), at(*a))));
            } else {
                out.off.push((r.clone(), pins.iter().map(|(p, n, a)| (p.clone(), n.clone(), at(*a))).collect()));
            }
        }
        boxes.push((i, wants.sections[i].0.clone(), (y, x), (y + l.rows - 1, x + l.cols - 1)));
    }
    // every off-board part listed (in the schematic's order), the sections in the table's order
    for (r, _) in &c.off {
        if !out.off.iter().any(|(x, _)| x == r) {
            out.off.push((r.clone(), vec![]));
        }
    }
    out.off.sort_by_key(|(r, _)| c.off.iter().position(|o| &o.0 == r));
    boxes.sort_by_key(|b| b.0);
    out.sections = boxes.into_iter().map(|(_, n, a, b)| (n, a, b)).collect();
    let pos = |r: &str| c.on.iter().position(|x| x.0 == r).unwrap_or(usize::MAX);
    out.parts.sort_by_key(|p| (p.look == Look::TwoLead, pos(&p.refdes)));
    out.rows = out.sections.iter().map(|s| s.2 .0 + 1).max().unwrap_or(1);
    out.cols = out.sections.iter().map(|s| s.2 .1 + 1).max().unwrap_or(1);
    out
}

/// The free place (top-left corner) nearest a chip's target centre `t` (row, column): on or below row `y0`, within the board's
/// width, its outline and `gap` holes round it clear of what is placed (`blocked`), and `ok` there (the table's distances).
pub(super) fn target_place(s: &Shape, t: (f64, f64), y0: u16, gap: u16, margin: u16, width: u16, blocked: &BTreeSet<At>, ok: impl Fn(u16, u16) -> bool) -> Option<At> {
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

/// The holes beside `h` (up, down, left, right) on a board `width` holes wide.
pub(super) fn beside4(h: At, width: u16) -> impl Iterator<Item = At> {
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
pub(super) fn place(c: &Circuit, opt: StripOptions, gap: u16, turned: Turn, sa: bool, wants: &Wants, targets: Option<&BTreeMap<String, (f64, f64)>>) -> StripLayout {
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
    // (the mounting holes' corners too: nothing is placed on them nor `gap` holes round them)
    let kept_holes: BTreeSet<At> = wants.kept_holes().into_iter().chain(wants.keepout.iter().copied()).collect();
    blocked.extend(wants.keepout.iter().copied());
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
        // a group's parts whose other parts are in a section below go last (the bottom of this section), those whose others are above
        // right after the headers (its top): the group then sits across the boundary (L10)
        if !wants.last.is_empty() || !wants.early.is_empty() {
            let name_of = |s: &Shape| c.on[s.k].0.clone();
            let rank = |s: &Shape| if is_head(s) { 0 } else if wants.early.contains(&name_of(s)) { 1 } else if wants.last.contains(&name_of(s)) { 3 } else { 2 };
            order.sort_by_key(|s| rank(s));
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
        // the bypass capacitors first, while there is room by their IC's pins (L17)
        twos.sort_by_key(|k| wants.bypass_of(&c.on[*k].0).is_none());
        for k in twos {
            let (refdes, look, pins) = &c.on[k];
            if pins.len() != 2 {
                continue;
            }
            let near = |n: &str, a: At, at_net: &BTreeMap<String, Vec<At>>| -> u32 { at_net.get(n).map(|v| v.iter().map(|b| a.0.abs_diff(b.0) as u32 + a.1.abs_diff(b.1) as u32).min().unwrap_or(0)).unwrap_or(0) };
            let mates: Vec<At> = parts.iter().filter(|p| wants.together(refdes, &p.refdes)).flat_map(|p| p.leads.iter().map(|x| x.1)).collect();
            let mut best: Option<(u32, At, At)> = None;
            let serves = wants.bypass_of(refdes).and_then(|(ic, limit)| Some((parts.iter().find(|p| p.refdes == ic)?.clone(), limit)));
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
                            // a bypass capacitor: its leads by its IC's pins of their nets, past the distance weighed heavily
                            let by = serves.as_ref().map_or(0, |(ic, limit)| {
                                let gap = bypass_gap(c, refdes, &[(pins[0].0.clone(), p), (pins[1].0.clone(), q)], ic);
                                (8.0 * gap + 40.0 * (gap - limit).max(0.0)) as u32
                            });
                            let cost = 4 * (near(&pins[0].1, p, &at_net) + near(&pins[1].1, q, &at_net)) + group + crowd(p, &pins[0].1) + crowd(q, &pins[1].1) + aim + by;
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
