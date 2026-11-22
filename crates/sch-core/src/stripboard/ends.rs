//! The ends on the board (a lead, a link's end, an off-board wire's end) and the strips packed into fewer rows (`share_rows`).

use super::*;

/// An end of something on the board: a part's lead, a link's end (`true`: its `a`), an off-board wire's end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum End {
    Lead(usize, usize),
    Link(usize, bool),
    Off(usize, usize),
}

pub(super) fn end_at(l: &StripLayout, e: End) -> At {
    match e {
        End::Lead(p, k) => l.parts[p].leads[k].1,
        End::Link(k, true) => l.links[k].a,
        End::Link(k, false) => l.links[k].b,
        End::Off(o, k) => l.off[o].1[k].2,
    }
}

pub(super) fn set_end(l: &mut StripLayout, e: End, at: At) {
    match e {
        End::Lead(p, k) => l.parts[p].leads[k].1 = at,
        End::Link(k, true) => l.links[k].a = at,
        End::Link(k, false) => l.links[k].b = at,
        End::Off(o, k) => l.off[o].1[k].2 = at,
    }
}

/// Every end on the board.
pub(super) fn all_ends(l: &StripLayout) -> Vec<End> {
    let mut v = vec![];
    for (p, part) in l.parts.iter().enumerate() {
        v.extend((0..part.leads.len()).map(|k| End::Lead(p, k)));
    }
    for k in 0..l.links.len() {
        v.push(End::Link(k, true));
        v.push(End::Link(k, false));
    }
    for (o, (_, pins)) in l.off.iter().enumerate() {
        v.extend((0..pins.len()).map(|k| End::Off(o, k)));
    }
    v
}

/// The other end of a two-lead part or a link (what lies between the two).
pub(super) fn other_end(l: &StripLayout, e: End) -> Option<End> {
    match e {
        End::Lead(p, k) if l.parts[p].leads.len() == 2 => Some(End::Lead(p, 1 - k)),
        End::Link(k, a) => Some(End::Link(k, !a)),
        _ => None,
    }
}

/// The holes strictly between two ends in one row or one column (a part's body or a wire lies over them).
pub(super) fn between(a: At, b: At) -> Vec<At> {
    if a.1 == b.1 {
        (a.0.min(b.0) + 1..a.0.max(b.0)).map(|r| (r, a.1)).collect()
    } else if a.0 == b.0 {
        (a.1.min(b.1) + 1..a.1.max(b.1)).map(|c| (a.0, c)).collect()
    } else {
        vec![]
    }
}

/// The bands of strips: (first row, row past the last).
fn bands_of(homes: &[String]) -> Vec<(u16, u16)> {
    let rows = homes.len() as u16;
    let mut bands = vec![];
    let mut r = 0u16;
    while r < rows {
        if homes[r as usize].is_empty() {
            r += 1;
            continue;
        }
        let st = r;
        while r < rows && !homes[r as usize].is_empty() {
            r += 1;
        }
        bands.push((st, r));
    }
    bands
}

/// Packs each band's strips: a strip whose ends all lie apart (a hole between, to cut) from the ends on a strip above it in the same
/// band moves onto that strip's row, the row cut between the two, when every end can go straight up or down (the hole there free and
/// nothing lying over it, and nothing in the way of a wire or part made longer). The rows left empty go. A band carries the supplies
/// and ground for every chip along the board, but most other nets are used along a short stretch of it: the DMM's B stage had 44 rows
/// of strips in 87. Returns how many rows went.
pub(super) fn share_rows(l: &mut StripLayout, keep: &BTreeSet<String>) -> u16 {
    // each shared row's stretches of strip: (first used column, last used column, net)
    let mut segs: BTreeMap<u16, Vec<(u16, u16, String)>> = BTreeMap::new();
    let mut empty: Vec<u16> = vec![];
    let cols_on = |l: &StripLayout, r: u16| -> Vec<(End, u16)> { all_ends(l).into_iter().map(|e| (e, end_at(l, e))).filter(|(_, a)| a.0 == r).map(|(e, a)| (e, a.1)).collect() };
    for (st, en) in bands_of(&l.homes) {
        for m in st + 1..en {
            let ends = cols_on(l, m);
            if ends.is_empty() {
                continue;
            }
            let (lo, hi) = ends.iter().fold((u16::MAX, 0), |a, (_, c)| (a.0.min(*c), a.1.max(*c)));
            for n in (st..m).filter(|n| !empty.contains(n)) {
                let here: Vec<(u16, u16, String)> = segs.get(&n).cloned().unwrap_or_else(|| {
                    let cols: Vec<u16> = cols_on(l, n).into_iter().map(|x| x.1).collect();
                    match (cols.iter().min(), cols.iter().max()) {
                        (Some(a), Some(b)) => vec![(*a, *b, l.homes[n as usize].clone())],
                        _ => vec![],
                    }
                });
                // a net that goes on to another section keeps a free hole beside its stretch, for the wire from there
                let mine = &l.homes[m as usize];
                let gap = |other: &String| 1 + keep.contains(mine) as u16 + keep.contains(other) as u16;
                if here.is_empty() || !here.iter().all(|(a, b, o)| *b + gap(o) < lo || hi + gap(o) < *a) {
                    continue;
                }
                let ends: Vec<End> = ends.iter().map(|x| x.0).collect();
                if !can_move(l, &ends, n) {
                    continue;
                }
                for e in &ends {
                    let at = end_at(l, *e);
                    set_end(l, *e, (n, at.1));
                }
                let mut v = here;
                v.push((lo, hi, l.homes[m as usize].clone()));
                v.sort();
                segs.insert(n, v);
                empty.push(m);
                break;
            }
        }
    }
    // the cuts between the stretches, and each stretch's name where it starts (the leftmost one's at the board's edge)
    for (n, v) in &segs {
        l.homes[*n as usize] = v[0].2.clone();
        for w in v.windows(2) {
            // the free holes between go to a stretch that must keep one
            let cut = match (keep.contains(&w[0].2), keep.contains(&w[1].2)) {
                (true, false) => w[1].0 - 1,
                (false, true) => w[0].1 + 1,
                _ => (w[0].1 + w[1].0) / 2,
            };
            l.cuts.insert((*n, cut));
            l.names.push(((*n, cut + 1), w[1].2.clone()));
        }
    }
    empty.sort();
    for &m in empty.iter().rev() {
        drop_row(l, m);
    }
    empty.len() as u16
}

/// Whether the ends (all on one row) can move straight up or down to row `n`.
fn can_move(l: &StripLayout, ends: &[End], n: u16) -> bool {
    let all = all_ends(l);
    let used: BTreeSet<At> = all.iter().map(|e| end_at(l, *e)).collect();
    // what lies over each hole: the holes between a part's or a link's two ends, by the end that names it
    let key = |e: End| -> End {
        match other_end(l, e) {
            Some(o) if o < e => o,
            _ => e,
        }
    };
    let mut over: BTreeMap<At, Vec<End>> = BTreeMap::new();
    for e in &all {
        if let Some(o) = other_end(l, *e).filter(|o| e < o) {
            for h in between(end_at(l, *e), end_at(l, o)) {
                over.entry(h).or_default().push(*e);
            }
        }
    }
    for e in ends {
        // a chip's or a row part's lead stays
        if matches!(e, End::Lead(p, _) if l.parts[*p].leads.len() != 2) {
            return false;
        }
        let at = end_at(l, *e);
        let to = (n, at.1);
        if used.contains(&to) || over.get(&to).is_some_and(|v| v.iter().any(|x| *x != key(*e))) {
            return false;
        }
        // a part or a wire made longer: nothing ends under it
        if let Some(o) = other_end(l, *e) {
            let far = if ends.contains(&o) { (n, end_at(l, o).1) } else { end_at(l, o) };
            if far == to || between(to, far).iter().any(|h| used.contains(h) && *h != at) {
                return false;
            }
        }
    }
    true
}

/// Takes the (empty) row `m` off the board, the rows below it up by one.
fn drop_row(l: &mut StripLayout, m: u16) {
    let up = |a: &mut At| {
        if a.0 > m {
            a.0 -= 1;
        }
    };
    for p in &mut l.parts {
        for (_, a) in &mut p.leads {
            up(a);
        }
    }
    for k in &mut l.links {
        up(&mut k.a);
        up(&mut k.b);
    }
    for (_, pins) in &mut l.off {
        for (_, _, a) in pins {
            up(a);
        }
    }
    l.cuts = l.cuts.iter().filter(|a| a.0 != m).map(|a| if a.0 > m { (a.0 - 1, a.1) } else { *a }).collect();
    for (a, _) in &mut l.names {
        up(a);
    }
    if (m as usize) < l.homes.len() {
        l.homes.remove(m as usize);
    }
}
