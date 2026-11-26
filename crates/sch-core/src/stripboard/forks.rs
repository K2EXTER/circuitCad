//! The Kelvin connections (forks) of the requirements table: how far two ways run together, and moving parts and wire ends to keep them.

use super::*;

/// The shortest ways along the copper from hole `s`: the strips (2.54 mm a hole) and the wires. Each joined hole's distance (µm) and
/// the hole before it on its way.
pub(super) fn copper_ways(l: &StripLayout, s: At) -> (BTreeMap<At, u64>, BTreeMap<At, At>) {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let pieces = l.pieces();
    let piece_of = |at: At| pieces.iter().position(|(r, x, y)| *r == at.0 && at.1 >= *x && at.1 <= *y);
    let mut wires: BTreeMap<At, Vec<(At, u64)>> = BTreeMap::new();
    for (ka, kb) in l.joins() {
        let d = ((ka.0 as f64 - kb.0 as f64).hypot(ka.1 as f64 - kb.1 as f64) * 2540.0) as u64;
        wires.entry(ka).or_default().push((kb, d));
        wires.entry(kb).or_default().push((ka, d));
    }
    // distances in µm, and each hole's hole before it on the way from `s`
    let mut dist: BTreeMap<At, u64> = BTreeMap::new();
    let mut prev: BTreeMap<At, At> = BTreeMap::new();
    let mut heap = BinaryHeap::new();
    dist.insert(s, 0);
    heap.push(Reverse((0u64, s)));
    while let Some(Reverse((d, at))) = heap.pop() {
        if dist.get(&at).is_some_and(|x| *x < d) {
            continue;
        }
        let mut next: Vec<(At, u64)> = wires.get(&at).cloned().unwrap_or_default();
        if let Some(p) = piece_of(at) {
            let (_, x, y) = pieces[p];
            if at.1 > x {
                next.push(((at.0, at.1 - 1), 2540));
            }
            if at.1 < y {
                next.push(((at.0, at.1 + 1), 2540));
            }
        }
        for (to, w) in next {
            if dist.get(&to).is_none_or(|x| d + w < *x) {
                dist.insert(to, d + w);
                prev.insert(to, at);
                heap.push(Reverse((d + w, to)));
            }
        }
    }
    (dist, prev)
}

/// How far (mm) the ways along the copper from hole `s` to holes `a` and `b` run together before they part: the shortest ways along the
/// strips (2.54 mm a hole) and the wires. `None` when `a` or `b` is not joined to `s`. A strip's current flows only between where it
/// comes in and where it goes out, so a sense wire taken off the strip on the far side of the lead from the current's wire sees the
/// lead's own voltage (0 mm together), and one taken off between them sees the drop along the strip as well.
pub fn strip_fork(l: &StripLayout, s: At, a: At, b: At) -> Option<f64> {
    let (dist, prev) = copper_ways(l, s);
    if !dist.contains_key(&a) || !dist.contains_key(&b) {
        return None;
    }
    // where the two ways part: the first hole on the way back from `b` that is on the way back from `a`
    let mut on_a: BTreeSet<At> = BTreeSet::new();
    let mut h = a;
    loop {
        on_a.insert(h);
        match prev.get(&h) {
            Some(p) => h = *p,
            None => break,
        }
    }
    let mut h = b;
    while !on_a.contains(&h) {
        h = prev[&h];
    }
    Some(dist[&h] as f64 / 1000.0)
}

/// The requirements table's forks (Kelvin connections) on the board: (the measure, its three holes, how far the ways run together).
pub(super) fn forks(s: &Schematic, l: &StripLayout) -> Vec<(design_spec::Measure, [At; 3], Option<f64>)> {
    let mut out = vec![];
    for m in s.requirements.measures.iter().filter(|m| m.what == "fork" && m.pads.len() == 3) {
        let at: Vec<Option<At>> = m.pads.iter().map(|p| pad_at(l, p)).collect();
        let (Some(a), Some(b), Some(c)) = (at[0], at[1], at[2]) else { continue };
        out.push((m.clone(), [a, b, c], strip_fork(l, a, b, c)));
    }
    out
}

/// Keeps the table's forks: when the ways from a fork's first pad run together along a strip (a current and a sense wire both taken
/// off on one side of a resistor's lead), the part with that lead moves along its strips to where they part at the lead (the lead
/// between the two), each lead staying on its piece of strip, on a free hole that nothing lies over, nothing ending under its body.
pub(super) fn keep_forks(s: &Schematic, l: &mut StripLayout) {
    for (m, [st, _, _], now) in forks(s, l) {
        let Some(now) = now else { continue };
        if m.op == "min" || now <= m.limit + 1e-9 {
            continue;
        }
        let Some(p) = l.parts.iter().position(|p| p.leads.len() == 2 && p.leads.iter().any(|(_, a)| *a == st)) else { continue };
        let old: Vec<At> = l.parts[p].leads.iter().map(|x| x.1).collect();
        let pieces = l.pieces();
        let piece_of = |at: At| pieces.iter().position(|(r, x, y)| *r == at.0 && at.1 >= *x && at.1 <= *y);
        // what else is on the board: the other ends, and the holes their parts and wires lie over
        let mine: BTreeSet<End> = (0..2).map(|k| End::Lead(p, k)).collect();
        let others: Vec<End> = all_ends(l).into_iter().filter(|e| !mine.contains(e)).collect();
        let used: BTreeSet<At> = others.iter().map(|e| end_at(l, *e)).collect();
        let over: BTreeSet<At> = others.iter().filter_map(|e| other_end(l, *e).filter(|o| e < o).map(|o| between(end_at(l, *e), end_at(l, o)))).flatten().collect();
        let mut best: Option<(f64, u16, Vec<At>)> = None;
        for c in 0..l.cols {
            let new: Vec<At> = old.iter().map(|a| (a.0, c)).collect();
            if new == old || new.iter().zip(&old).any(|(n, o)| piece_of(*n).is_none() || piece_of(*n) != piece_of(*o)) {
                continue;
            }
            let body = between(new[0], new[1]);
            if new.iter().any(|a| used.contains(a) || over.contains(a)) || body.iter().any(|a| used.contains(a)) {
                continue;
            }
            let mut trial = l.clone();
            for (k, a) in new.iter().enumerate() {
                trial.parts[p].leads[k].1 = *a;
            }
            let st2 = new[old.iter().position(|a| *a == st).unwrap()];
            let [_, fa, fb] = forks(s, &trial).into_iter().find(|f| f.0 == m).map(|f| f.1).unwrap_or([st2; 3]);
            let Some(v) = strip_fork(&trial, st2, fa, fb) else { continue };
            let key = (v, c.abs_diff(old[0].1));
            if best.as_ref().is_none_or(|b| key < (b.0, b.1)) {
                best = Some((v, key.1, new));
            }
        }
        if let Some((_, _, new)) = best.filter(|b| b.0 < now) {
            for (k, a) in new.into_iter().enumerate() {
                l.parts[p].leads[k].1 = a;
            }
        }
        move_wire_ends(s, l, &m);
    }
}

/// The second way to keep a fork: a wire's end on the strip of the fork's first pad moves along that piece of strip to a free hole
/// (one at a time, the best first), so the wires of the two ways come in on either side of the lead.
fn move_wire_ends(s: &Schematic, l: &mut StripLayout, m: &design_spec::Measure) {
    let value = |l: &StripLayout| forks(s, l).into_iter().find(|f| f.0 == *m).and_then(|f| f.2);
    for _ in 0..2 {
        let Some(now) = value(l) else { return };
        if now <= m.limit + 1e-9 {
            return;
        }
        let Some(st) = pad_at(l, &m.pads[0]) else { return };
        let pieces = l.pieces();
        let Some(&(row, x0, x1)) = pieces.iter().find(|(r, x, y)| *r == st.0 && st.1 >= *x && st.1 <= *y) else { return };
        let all = all_ends(l);
        let used: BTreeSet<At> = all.iter().map(|e| end_at(l, *e)).collect();
        let over: BTreeSet<At> = all.iter().filter_map(|e| other_end(l, *e).filter(|o| e < o).map(|o| between(end_at(l, *e), end_at(l, o)))).flatten().collect();
        let mut best: Option<(f64, u16, End, At)> = None;
        for e in all.iter().filter(|e| matches!(e, End::Link(..))) {
            let at = end_at(l, *e);
            if at.0 != row || at.1 < x0 || at.1 > x1 {
                continue;
            }
            for c in x0..=x1 {
                let to = (row, c);
                if used.contains(&to) || over.contains(&to) {
                    continue;
                }
                let far = end_at(l, other_end(l, *e).unwrap());
                if between(to, far).iter().any(|h| used.contains(h)) {
                    continue;
                }
                let mut trial = l.clone();
                set_end(&mut trial, *e, to);
                let Some(v) = value(&trial) else { continue };
                let key = (v, c.abs_diff(at.1));
                if best.as_ref().is_none_or(|b| key < (b.0, b.1)) {
                    best = Some((v, key.1, *e, to));
                }
            }
        }
        match best.filter(|b| b.0 < now) {
            Some((_, _, e, to)) => set_end(l, e, to),
            None => return,
        }
    }
}
