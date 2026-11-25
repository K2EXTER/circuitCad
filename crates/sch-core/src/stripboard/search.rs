//! Tries ways of laying the board out (the chips turned, stacked, swapped, on the copper side) and keeps the best.

use super::*;

/// Lays the schematic out on a stripboard (and the same on a perfboard).
///
/// The board is bands of strips and rows of chips in turn: a band at the top, a row of chips, a band, a row of chips … a band at the
/// bottom. A band has a strip for each net its neighbours need. A chip pin in the upper half of its chip has its wire go up to the
/// band above, a pin in the lower half down to the band below, so one column carries a wire up and another down. The chips wrap to a
/// new row past the board's width (`StripOptions::cols`). A two-lead part stands across two strips of a band, in a column free there.
/// The strips of one net in different bands are joined by wires at the left, beside the wires of the supplies off the board.
pub fn strip_layout_with(s: &Schematic, opt: StripOptions) -> StripLayout {
    let full = read(s);
    // a chip the person put on the copper side (`MountChoice::Lands`) whose two strips are not side by side stands on top as the
    // leaded part
    let on_top_where_not_side_by_side = |mut l: StripLayout| {
        for p in l.parts.iter_mut().filter(|p| matches!(p.mount, crate::mount::Mount::Back { .. })) {
            let side_by_side = p.leads.len() == 2 && p.leads[0].1 .1 == p.leads[1].1 .1 && p.leads[0].1 .0.abs_diff(p.leads[1].1 .0) == 1;
            if !side_by_side {
                p.mount = crate::mount::Mount::Leaded;
            }
        }
        l
    };
    if !opt.chips_back {
        return on_top_where_not_side_by_side(strip_lay(s, &full, opt));
    }
    // the chips on the copper side (`StripOptions::chips_back`), two ways, against the board with every part on top: (a) laid out with
    // the rest, each chip's two nets on strips side by side in its band (`pair_up`) and the chip across them on the copper side (no
    // room on top; a chip whose strips are not side by side stands on top as the leaded part); (b) the board laid out without them,
    // then each across two strips that happen to carry its two nets (`with_back_strips`), one that finds no place back on top and
    // the board laid out again. The best by holes + 10 per wire on top - 5 per chip on the back (one leaded part fewer to fit on top)
    // that joins everything as drawn.
    let on_back = |l: &StripLayout| l.parts.iter().filter(|p| matches!(p.mount, crate::mount::Mount::Back { .. })).count();
    let score = |l: &StripLayout| (l.rows as usize * l.cols as usize + 10 * l.links.len()) as i64 - 5 * on_back(l) as i64;
    let mut found: Vec<StripLayout> = vec![on_top_where_not_side_by_side(strip_lay(s, &full, opt))];
    let backs = crate::perfboard::back_chips(s, &full, true);
    if !backs.is_empty() {
        let mut c = full.clone();
        for (r, size) in &backs {
            c.mounts.insert(r.clone(), crate::mount::Mount::Back { size: size.clone() });
        }
        let mut l = strip_lay(s, &c, opt);
        // (the rows' sharing can move one of a chip's strips away from the other: that chip goes on top)
        for p in l.parts.iter_mut().filter(|p| matches!(p.mount, crate::mount::Mount::Back { .. })) {
            let side_by_side = p.leads.len() == 2 && p.leads[0].1 .1 == p.leads[1].1 .1 && p.leads[0].1 .0.abs_diff(p.leads[1].1 .0) == 1;
            if !side_by_side {
                p.mount = crate::mount::Mount::Leaded;
            }
        }
        found.push(l);
        let mut backs = backs;
        for _ in 0..3 {
            if backs.is_empty() {
                break;
            }
            let mut c = full.clone();
            c.on.retain(|(r, _, _)| !backs.contains_key(r));
            let (l, missed) = with_back_strips(&full, strip_lay(s, &c, opt), &backs);
            if missed.is_empty() {
                found.push(l);
                break;
            }
            backs.retain(|r, _| !missed.contains(r));
        }
    }
    let mut best = found.remove(0);
    for l in found {
        if score(&l) < score(&best) && strip_check(s, &l).is_empty() {
            best = l;
        }
    }
    best
}

/// The chips put on the copper side of a stripboard laid out without them: each across two holes one above the other, on two
/// strips that carry its two nets (a strip is one net all along, so any column of them will do), nearest a lead of its nets and on
/// free holes where it can; never on a cut. Returns the board and the chips that found no place.
fn with_back_strips(c: &crate::breadboard::Circuit, mut l: StripLayout, backs: &BTreeMap<String, String>) -> (StripLayout, Vec<String>) {
    let net_of = |r: &str, pin: &str| c.on.iter().find(|x| x.0 == r).and_then(|x| x.2.iter().find(|(p, _)| p == pin)).map(|x| x.1.clone()).unwrap_or_default();
    let groups = l.hole_groups();
    // each group of joined holes and its net (from a lead in it), and where the leads are
    let mut group_net: BTreeMap<usize, String> = BTreeMap::new();
    let mut lead_holes: Vec<(At, String)> = vec![];
    for p in &l.parts {
        for (pin, a) in &p.leads {
            let n = net_of(&p.refdes, pin);
            if let Some(g) = groups.get(a) {
                group_net.entry(*g).or_insert_with(|| n.clone());
            }
            lead_holes.push((*a, n));
        }
    }
    for (_, pins) in &l.off {
        for (_, n, a) in pins {
            if let Some(g) = groups.get(a) {
                group_net.entry(*g).or_insert_with(|| n.clone());
            }
            lead_holes.push((*a, n.clone()));
        }
    }
    let used: BTreeSet<At> = l.used().into_iter().map(|(a, _)| a).collect();
    let hole_net = |a: At| groups.get(&a).and_then(|g| group_net.get(g)).cloned();
    let mut taken: BTreeSet<At> = BTreeSet::new();
    let mut missed = vec![];
    for (r, size) in backs {
        let Some((_, look, pins)) = c.on.iter().find(|x| &x.0 == r) else { continue };
        let ((p1, n1), (p2, n2)) = (&pins[0], &pins[1]);
        let near = |a: At, n: &str| lead_holes.iter().filter(|(_, m)| m == n).map(|(h, _)| h.0.abs_diff(a.0) as u32 + h.1.abs_diff(a.1) as u32).min().unwrap_or(u32::from(l.cols));
        let mut best: Option<(u32, At, At)> = None;
        for row in 0..l.rows.saturating_sub(1) {
            for col in 0..l.cols {
                let (a, b) = ((row, col), (row + 1, col));
                if l.cuts.contains(&a) || l.cuts.contains(&b) || taken.contains(&a) || taken.contains(&b) {
                    continue;
                }
                let (na, nb) = (hole_net(a), hole_net(b));
                let (x, y) = if na.as_deref() == Some(n1.as_str()) && nb.as_deref() == Some(n2.as_str()) {
                    (a, b)
                } else if na.as_deref() == Some(n2.as_str()) && nb.as_deref() == Some(n1.as_str()) {
                    (b, a)
                } else {
                    continue;
                };
                // on a free hole rather than over a lead's joint, near a lead of its nets
                let k = 10 * (used.contains(&x) as u32 + used.contains(&y) as u32) + near(x, n1).min(near(y, n2));
                if best.is_none_or(|bst| k < bst.0) {
                    best = Some((k, x, y));
                }
            }
        }
        match best {
            Some((_, x, y)) => {
                taken.extend([x, y]);
                l.parts.push(StripPart { refdes: r.clone(), look: *look, leads: vec![(p1.clone(), x), (p2.clone(), y)], mount: Mount::Back { size: size.clone() } });
            }
            None => missed.push(r.clone()),
        }
    }
    (l, missed)
}

/// The board laid out for the circuit `c` (all of the schematic's parts, or those that go on top).
fn strip_lay(s: &Schematic, c: &crate::breadboard::Circuit, opt: StripOptions) -> StripLayout {
    // each chip in two rows (a DIP) turned half a turn, one at a time, kept when the board comes out better: joined as drawn and the
    // checks kept (`strip_rules`: the Kelvin connections), then fewer holes, fewer wires, shorter ones. The pins in a chip's upper
    // half send their wires to the band above, so turning it moves a net's wire to the other band, where the net may already have a
    // strip (one strip and one joining wire fewer).
    let score = |l: &StripLayout| {
        let len: u32 = l.links.iter().map(|k| k.a.0.abs_diff(k.b.0) as u32 + k.a.1.abs_diff(k.b.1) as u32).sum();
        let failed = strip_rules(s, l).iter().filter(|r| !r.ok).count();
        (strip_check(s, l).len(), failed, l.rows as usize * l.cols as usize + 10 * l.links.len(), len)
    };
    // and the same with the short chips stacked in the same columns (`Circuit::stack`: two at most, or any number), the best
    let dips: Vec<String> = c.on.iter().filter(|(r, look, _)| *look == Look::Dip || c.mounts.get(r).is_some_and(|m| m.width().is_some())).map(|p| p.0.clone()).collect();
    let mut found: Vec<(StripLayout, Score, crate::breadboard::Circuit)> = vec![];
    for stack in [1, 2, usize::MAX] {
        let mut c = c.clone();
        c.stack = stack;
        let mut best = strip_lay_fit(s, &c, opt);
        let mut best_score = score(&best);
        for r in &dips {
            c.flipped.insert(r.clone());
            let l = strip_lay_fit(s, &c, opt);
            let sc = score(&l);
            if sc < best_score {
                (best, best_score) = (l, sc);
            } else {
                c.flipped.remove(r);
            }
        }
        found.push((best, best_score, c));
    }
    // two chips the requirements table keeps apart and too near: one of them swapped along the board with another chip as tall,
    // each such chip tried (the closest pair first, twice at most), kept when the board comes out better. Tried on each way of
    // stacking whose board failed a check but came out smaller than the best one that passes
    let chips: BTreeMap<String, u16> = c.on.iter().filter(|p| p.1 != Look::TwoLead).map(|p| (p.0.clone(), chip_rows(c, &p.0))).collect();
    let passing = found.iter().filter(|f| f.1 .1 == 0).map(|f| f.1 .2).min().unwrap_or(usize::MAX);
    for f in found.iter_mut().filter(|f| f.1 .1 > 0 && f.1 .2 < passing) {
        let (best, best_score, c) = (&mut f.0, &mut f.1, &mut f.2);
        swap_apart(s, opt, &chips, &score, best, best_score, c);
    }
    found.into_iter().min_by_key(|f| f.1).unwrap().0
}

type Score = (usize, usize, usize, u32);

/// Swaps chips along the board to part two the requirements table keeps apart (`strip_lay`).
fn swap_apart(s: &Schematic, opt: StripOptions, chips: &BTreeMap<String, u16>, score: &dyn Fn(&StripLayout) -> Score, best: &mut StripLayout, best_score: &mut Score, c: &mut crate::breadboard::Circuit) {
    for _ in 0..2 {
        if best_score.1 == 0 {
            break;
        }
        let leads = |l: &StripLayout, r: &str| l.parts.iter().filter(|p| p.refdes == r).flat_map(|p| p.leads.iter().map(|x| x.1)).collect::<Vec<At>>();
        let near = |l: &StripLayout, a: &str, b: &str| {
            let (x, y) = (leads(l, a), leads(l, b));
            x.iter().flat_map(|p| y.iter().map(move |q| (p.0 as f64 - q.0 as f64).hypot(p.1 as f64 - q.1 as f64) * 2.54)).fold(f64::INFINITY, f64::min)
        };
        let Some((a, b)) = c
            .apart
            .iter()
            .filter(|(a, b, d)| chips.contains_key(a) && chips.contains_key(b) && near(best, a, b) < *d)
            .min_by(|x, y| near(best, &x.0, &x.1).total_cmp(&near(best, &y.0, &y.1)))
            .map(|(a, b, _)| (a.clone(), b.clone()))
        else {
            break;
        };
        let mut better: Option<(StripLayout, Score, crate::breadboard::Circuit)> = None;
        for moved in [&a, &b] {
            for (other, rows) in chips {
                if other == moved || *rows != chips[moved] || other == &a || other == &b {
                    continue;
                }
                let mut t = c.clone();
                t.swaps.push((moved.clone(), other.clone()));
                let l = strip_lay_fit(s, &t, opt);
                let sc = score(&l);
                if sc < better.as_ref().map_or(*best_score, |x| x.1) {
                    better = Some((l, sc, t));
                }
            }
        }
        let Some(x) = better else { break };
        (*best, *best_score, *c) = x;
    }
}

/// How many rows a chip of the circuit takes on a stripboard (a DIP: half its places; a row of leads: all of them).
fn chip_rows(c: &crate::breadboard::Circuit, r: &str) -> u16 {
    let Some((_, look, pins)) = c.on.iter().find(|p| p.0 == r) else { return 0 };
    let m = c.mounts.get(r).cloned().unwrap_or_default();
    let places = m.slots(pins).0;
    if *look == Look::Dip || m.width().is_some() { places.div_ceil(2) as u16 } else { places as u16 }
}

/// The board laid out for `c` within the board's width: the wires joining sections can take it a column or two past, and then it
/// is laid out again that much narrower.
fn strip_lay_fit(s: &Schematic, c: &crate::breadboard::Circuit, opt: StripOptions) -> StripLayout {
    let mut l = strip_lay_once(s, c, opt);
    let Some(w) = opt.cols else { return l };
    let mut try_w = w;
    for _ in 0..3 {
        if l.cols <= w || try_w <= 8 {
            break;
        }
        try_w -= l.cols - w;
        let n = strip_lay_once(s, c, StripOptions { cols: Some(try_w), ..opt });
        if n.cols < l.cols {
            l = n;
        }
    }
    l
}

fn strip_lay_once(s: &Schematic, c: &crate::breadboard::Circuit, opt: StripOptions) -> StripLayout {
    let c = c.clone();
    let secs = sections(s, &c);
    let mut l = if secs.len() < 2 { lay_within(&c, opt, &BTreeSet::new()) } else { lay_sections(&c, opt, &secs) };
    // a net left apart is joined by a wire as a line of the ratsnest is (the parts added to a board edited by hand are joined so):
    // with sections, a net between sections that has no strip in one of them (a chip's pin to another section's chip); and a wire
    // between two chip pins (`Carry::Pins`) that found no free hole on a pin's piece of strip (`join_lines` takes another hole of
    // the net, or grows the piece)
    let lines = strip_unjoined(s, &l);
    if !lines.is_empty() {
        crate::strip_edit::join_lines(s, &mut l, lines);
    }
    keep_forks(s, &mut l);
    l
}
