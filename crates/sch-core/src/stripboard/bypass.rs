//! The bypass capacitors of the requirements table on a stripboard: a capacitor too far from its IC moved to the holes of its nets'
//! copper nearest the IC's pins (L17).

use super::*;

/// Moves each bypass capacitor past its distance (`bypass_spans`) to where its leads are nearest, along the copper, the IC's pins of
/// their nets: each lead on a hole of copper joined to such a pin (a lead whose net the IC has no pin on stays on its piece of strip),
/// the two in one column, on free holes nothing lies over, nothing ending under its body. The copper is not changed, so the board
/// stays joined as drawn. A capacitor in a Kelvin fork of the table is left where the fork put it.
pub(super) fn keep_bypass(s: &Schematic, l: &mut StripLayout) {
    if l.perf || s.requirements.bypass.is_empty() {
        return;
    }
    let spans = bypass_spans(s, l);
    if spans.iter().all(|sp| sp.judged() <= sp.limit + 1e-9) {
        return;
    }
    let in_fork = |cap: &str| s.requirements.measures.iter().any(|m| m.what == "fork" && m.pads.iter().any(|p| p.split_once('.').is_some_and(|(r, _)| r == cap)));
    let nets = extract(&s.flat());
    let net_of = |r: &str, pin: &str| nets.iter().find(|n| n.pins.iter().any(|p| p.refdes == r && p.pin == pin)).map(|n| n.name.clone()).unwrap_or_default();
    for sp in spans.iter().filter(|sp| sp.judged() > sp.limit + 1e-9 && !in_fork(&sp.cap)) {
        let Some(p) = l.parts.iter().position(|x| x.refdes == sp.cap && x.leads.len() == 2) else { continue };
        let Some(ic) = l.parts.iter().find(|x| x.refdes == sp.ic).cloned() else { continue };
        let old: Vec<At> = l.parts[p].leads.iter().map(|x| x.1).collect();
        let pieces = l.pieces();
        let piece_of = |at: At| pieces.iter().copied().find(|(r, x, y)| *r == at.0 && at.1 >= *x && at.1 <= *y);
        // each lead's holes and how far each is (µm) from the IC's nearest pin of its net along the copper; a lead with no such pin:
        // its own piece of strip, counting nothing
        let mut reach: Vec<BTreeMap<At, u64>> = vec![];
        for (pin, at) in &l.parts[p].leads {
            let net = net_of(&sp.cap, pin);
            let mut m: BTreeMap<At, u64> = BTreeMap::new();
            for (_, b) in ic.leads.iter().filter(|(ip, _)| !net.is_empty() && net_of(&sp.ic, ip) == net) {
                for (h, d) in copper_ways(l, *b).0 {
                    let e = m.entry(h).or_insert(d);
                    *e = (*e).min(d);
                }
            }
            if m.is_empty()
                && let Some((r, x0, x1)) = piece_of(*at)
            {
                m.extend((x0..=x1).map(|c| ((r, c), 0)));
            }
            // only holes on a strip (a wire's far end is on one too)
            m.retain(|h, _| piece_of(*h).is_some() && !l.cuts.contains(h));
            reach.push(m);
        }
        // what else is on the board: the other ends, and the holes their parts and wires lie over
        let mine: BTreeSet<End> = (0..2).map(|k| End::Lead(p, k)).collect();
        let others: Vec<End> = all_ends(l).into_iter().filter(|e| !mine.contains(e)).collect();
        let used: BTreeSet<At> = others.iter().map(|e| end_at(l, *e)).collect();
        let over: BTreeSet<At> = others.iter().filter_map(|e| other_end(l, *e).filter(|o| e < o).map(|o| between(end_at(l, *e), end_at(l, o)))).flatten().collect();
        let span = old[0].0.abs_diff(old[1].0).max(4);
        let value = |a: At, b: At| reach[0].get(&a).copied().unwrap_or(u64::MAX).max(reach[1].get(&b).copied().unwrap_or(u64::MAX));
        let now = value(old[0], old[1]);
        let mut best: Option<(u64, u16, [At; 2])> = None;
        for (&a, _) in reach[0].iter().filter(|(h, _)| !used.contains(h) && !over.contains(h)) {
            for (&b, _) in reach[1].iter().filter(|(h, _)| h.1 == a.1 && h.0 != a.0 && h.0.abs_diff(a.0) <= span && !used.contains(h) && !over.contains(h)) {
                if between(a, b).iter().any(|h| used.contains(h)) {
                    continue;
                }
                let key = (value(a, b), a.1.abs_diff(old[0].1));
                if best.as_ref().is_none_or(|x| key < (x.0, x.1)) {
                    best = Some((key.0, key.1, [a, b]));
                }
            }
        }
        if let Some((_, _, [a, b])) = best.filter(|x| x.0 < now) {
            l.parts[p].leads[0].1 = a;
            l.parts[p].leads[1].1 = b;
        }
    }
}
