//! The sections of the requirements table laid out each by itself and put side by side, joined by wires.

use super::*;

/// Each section laid out by itself, the sections side by side (wrapped past the board's width), every strip cut between two
/// sections. A net in more than one section has a strip in each, joined by a wire from section to section; ground instead goes
/// from each section's strip by a wire of its own to the ground strip of the first section that has one (the one ground point: the
/// return current of one section does not flow through another's ground).
pub(super) fn lay_sections(c: &crate::breadboard::Circuit, opt: StripOptions, secs: &[(String, BTreeSet<String>)]) -> StripLayout {
    let nets_of = |parts: &BTreeSet<String>| -> BTreeSet<String> { c.on.iter().filter(|p| parts.contains(&p.0)).flat_map(|p| p.2.iter().map(|x| x.1.clone())).filter(|n| !n.is_empty()).collect() };
    let sec_nets: Vec<BTreeSet<String>> = secs.iter().map(|(_, p)| nets_of(p)).collect();
    // an off-board part goes with the section that has the most of its nets
    let off_sec: Vec<usize> = c
        .off
        .iter()
        .map(|(_, pins)| (0..secs.len()).max_by_key(|i| (pins.iter().filter(|(_, n)| sec_nets[*i].contains(n)).count(), std::cmp::Reverse(*i))).unwrap_or(0))
        .collect();
    let mut nets_in: Vec<BTreeSet<String>> = sec_nets.clone();
    for (k, (_, pins)) in c.off.iter().enumerate() {
        nets_in[off_sec[k]].extend(pins.iter().map(|(_, n)| n.clone()).filter(|n| !n.is_empty()));
    }
    let shared = |n: &String, i: usize| (0..secs.len()).any(|j| j != i && nets_in[j].contains(n));
    let mut subs: Vec<StripLayout> = vec![];
    for (i, (_, parts)) in secs.iter().enumerate() {
        let sub = crate::breadboard::Circuit {
            on: c.on.iter().filter(|p| parts.contains(&p.0)).cloned().collect(),
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
        let external: BTreeSet<String> = nets_in[i].iter().filter(|n| shared(n, i)).cloned().collect();
        subs.push(lay_within(&sub, opt, &external));
    }
    // the sections side by side, a column between (cut on every row), a new shelf past the board's width
    let mut out = StripLayout::default();
    let (mut x, mut y, mut shelf_h) = (0u16, 0u16, 0u16);
    let mut shelf_gaps: Vec<u16> = vec![];
    let mut origin: Vec<(u16, u16)> = vec![];
    let close_shelf = |out: &mut StripLayout, y: u16, h: u16, gaps: &mut Vec<u16>| {
        for g in gaps.drain(..) {
            for r in y..y + h {
                out.cuts.insert((r, g));
            }
        }
    };
    for (i, l) in subs.iter().enumerate() {
        if let Some(w) = opt.cols
            && x > 0
            && x + l.cols > w
        {
            close_shelf(&mut out, y, shelf_h, &mut shelf_gaps);
            shelf_gaps.clear();
            y += shelf_h + 1;
            x = 0;
            shelf_h = 0;
        }
        let at = |a: At| (a.0 + y, a.1 + x);
        for p in &l.parts {
            let mut p = p.clone();
            for (_, a) in &mut p.leads {
                *a = at(*a);
            }
            out.parts.push(p);
        }
        for k in &l.links {
            out.links.push(Link { a: at(k.a), b: at(k.b), ..k.clone() });
        }
        out.cuts.extend(l.cuts.iter().map(|c| at(*c)));
        for (r, pins) in &l.off {
            out.off.push((r.clone(), pins.iter().map(|(p, n, a)| (p.clone(), n.clone(), at(*a))).collect()));
        }
        for (r, n) in l.homes.iter().enumerate().filter(|(_, n)| !n.is_empty()) {
            out.names.push(((r as u16 + y, x), n.clone()));
        }
        out.names.extend(l.names.iter().map(|(a, n)| (at(*a), n.clone())));
        out.sections.push((secs[i].0.clone(), (y, x), (y + l.rows - 1, x + l.cols - 1)));
        origin.push((y, x));
        shelf_h = shelf_h.max(l.rows);
        x += l.cols;
        shelf_gaps.push(x);
        x += 1;
    }
    shelf_gaps.pop();
    close_shelf(&mut out, y, shelf_h, &mut shelf_gaps);
    // the nets shared by sections: a wire between their strips (ground: each to the first section's)
    // a net's strip in a section: its row and the stretch of it the net has (a row may carry more than one net, cut between)
    let strip_row = |i: usize, n: &str| strip_of(&subs[i], n).map(|(r, a, b)| (r + origin[i].0, a + origin[i].1, b + origin[i].1));
    let mut taken: BTreeSet<At> = out.used().into_iter().map(|(a, _)| a).collect();
    let free_on = |(row, x0, x1): (u16, u16, u16), toward: u16, taken: &mut BTreeSet<At>, cuts: &BTreeSet<At>| -> Option<At> {
        let mut cols: Vec<u16> = (x0..=x1).collect();
        cols.sort_by_key(|c| c.abs_diff(toward));
        let a = cols.into_iter().map(|c| (row, c)).find(|a| !taken.contains(a) && !cuts.contains(a))?;
        taken.insert(a);
        Some(a)
    };
    let mut nets: Vec<&String> = nets_in.iter().flatten().collect();
    nets.sort();
    nets.dedup();
    for n in nets {
        let have: Vec<usize> = (0..secs.len()).filter(|i| strip_row(*i, n).is_some()).collect();
        if have.len() < 2 {
            continue;
        }
        let pairs: Vec<(usize, usize)> = if is_ground_name(n) { have[1..].iter().map(|j| (have[0], *j)).collect() } else { have.windows(2).map(|w| (w[0], w[1])).collect() };
        for (i, j) in pairs {
            let (si, sj) = (strip_row(i, n).unwrap(), strip_row(j, n).unwrap());
            let (ci, cj) = (origin[i].1, origin[j].1);
            if let (Some(a), Some(b)) = (free_on(si, cj, &mut taken, &out.cuts), free_on(sj, ci, &mut taken, &out.cuts)) {
                out.links.push(Link::new(a, b, n.clone()));
            }
        }
    }
    let far = out.used().into_iter().map(|(a, _)| a).chain(out.cuts.iter().copied()).fold((0u16, 0u16), |m, a| (m.0.max(a.0), m.1.max(a.1)));
    out.rows = far.0.max(y + shelf_h.max(1) - 1) + 1;
    out.cols = far.1 + 1;
    out
}

/// One section laid out within the board's width: the rows of chips wrap at the width, but the wires at the left and the parts beside
/// the chips can take a section past it, so a section that comes out wider is laid out again with the width less what it went over.
pub(super) fn lay_within(c: &crate::breadboard::Circuit, opt: StripOptions, external: &BTreeSet<String>) -> StripLayout {
    let first = lay(c, opt, external);
    let Some(w) = opt.cols else { return first };
    let mut best = first;
    let mut try_w = w;
    for _ in 0..6 {
        if best.cols <= w {
            break;
        }
        try_w = try_w.saturating_sub(best.cols - w).max(4);
        let l = lay(c, StripOptions { cols: Some(try_w), ..opt }, external);
        if l.cols < best.cols {
            best = l;
        }
        if try_w == 4 {
            break;
        }
    }
    best
}

/// A net's strip in a layout of one section: its row and the stretch of the row it has (first and last column; a row shared by nets
/// is cut between them, its first net named in `homes`, the others in `names` where theirs starts).
pub(super) fn strip_of(l: &StripLayout, n: &str) -> Option<(u16, u16, u16)> {
    let pieces = l.pieces();
    let piece = |at: At| pieces.iter().find(|(r, a, b)| *r == at.0 && at.1 >= *a && at.1 <= *b).copied();
    let first = l.homes.iter().position(|h| h == n).and_then(|r| piece((r as u16, 0)));
    let other = l.names.iter().filter(|(_, m)| m == n).filter_map(|(a, _)| piece(*a)).min();
    [first, other].into_iter().flatten().min()
}
