//! The river (`river`): a long header's two-lead nets to the row of a chip's pins under it, wired first as nested Ls that do not cross.

use super::*;

/// The holes from `a` to `b` along a row or a column, both ends in.
pub(super) fn along(a: At, b: At) -> Vec<At> {
    if a.0 == b.0 {
        (a.1.min(b.1)..=a.1.max(b.1)).map(|c| (a.0, c)).collect()
    } else {
        (a.0.min(b.0)..=a.0.max(b.0)).map(|r| (r, a.1)).collect()
    }
}

/// The river: the nets of two leads each, one on a long header lying along a row (6 leads and more) and one on the top row of a
/// chip's pins below it, wired as Ls (down from the header, along a row of their own, down to the chip) that do not cross: in the
/// order along the header (a net out of order with the others is left to the router), each Ls's turn on its own row, the net
/// whose way along crosses over the start of the next one turning below it. Only through free holes (not a lead's, not one kept
/// as another net's way out). Returns each net and its straight wires.
pub(super) fn river(l: &StripLayout, leads: &[(At, u32)], owner: &[Option<u32>], escape: &BTreeMap<At, u32>) -> Vec<(u32, Vec<(At, At)>)> {
    let cols = l.cols as usize;
    let idx = |a: At| a.0 as usize * cols + a.1 as usize;
    let mut by_net: BTreeMap<u32, Vec<At>> = BTreeMap::new();
    for (a, n) in leads {
        by_net.entry(*n).or_default().push(*a);
    }
    let part_of = |a: At| l.parts.iter().position(|p| p.leads.iter().any(|(_, h)| *h == a));
    let mut out = vec![];
    let mut used: BTreeSet<At> = BTreeSet::new();
    for (hi, h) in l.parts.iter().enumerate() {
        let rows: BTreeSet<u16> = h.leads.iter().map(|(_, a)| a.0).collect();
        if h.look != Look::Row || h.leads.len() < 6 || rows.len() != 1 {
            continue;
        }
        let yh = *rows.iter().next().unwrap();
        // the header's two-lead nets, grouped by the chip at their other end (on that chip's top row, below the header)
        let mut to_chip: BTreeMap<usize, Vec<(u32, u16, u16)>> = BTreeMap::new();
        for (net, holes) in &by_net {
            let [a, b] = holes.as_slice() else { continue };
            let (top, other) = if a.0 == yh && part_of(*a) == Some(hi) { (*a, *b) } else if b.0 == yh && part_of(*b) == Some(hi) { (*b, *a) } else { continue };
            let Some(ci) = part_of(other) else { continue };
            let chip = &l.parts[ci];
            if chip.look != Look::Dip || chip.leads.iter().map(|(_, x)| x.0).min() != Some(other.0) || other.0 <= yh + 1 {
                continue;
            }
            to_chip.entry(ci).or_default().push((*net, top.1, other.1));
        }
        for (ci, mut v) in to_chip {
            let yc = l.parts[ci].leads.iter().map(|(_, x)| x.0).min().unwrap();
            // in the order along the header, keeping those whose chip pins come in the same order
            v.sort_by_key(|x| x.1);
            let mut kept: Vec<(u32, u16, u16)> = vec![];
            for x in v {
                if kept.last().is_none_or(|k| k.2 < x.2) {
                    kept.push(x);
                }
            }
            let tracks: Vec<u16> = (yh + 1..yc).rev().collect();
            // the nets going right turn deepest from the left, the nets going left deepest from the right
            let right: Vec<&(u32, u16, u16)> = kept.iter().filter(|x| x.2 > x.1).collect();
            let left: Vec<&(u32, u16, u16)> = kept.iter().filter(|x| x.2 < x.1).rev().collect();
            let straight: Vec<&(u32, u16, u16)> = kept.iter().filter(|x| x.2 == x.1).collect();
            let mut plan: Vec<(u32, Vec<(At, At)>)> = vec![];
            for group in [right, left] {
                for (k, (net, xh, xc)) in group.into_iter().enumerate() {
                    let Some(&t) = tracks.get(k) else { break };
                    plan.push((*net, vec![((yh, *xh), (t, *xh)), ((t, *xh), (t, *xc)), ((t, *xc), (yc, *xc))]));
                }
            }
            for (net, _, x) in straight {
                plan.push((*net, vec![((yh, *x), (yc, *x))]));
            }
            for (net, segs) in plan {
                let holes: Vec<At> = segs.iter().flat_map(|(a, b)| along(*a, *b)).collect();
                let (start, end) = (segs[0].0, segs.last().unwrap().1);
                let free = holes.iter().all(|x| *x == start || *x == end || (owner[idx(*x)].is_none() && escape.get(x).is_none_or(|e| *e == net) && !used.contains(x)));
                if !free {
                    continue;
                }
                used.extend(holes.iter().copied());
                out.push((net, segs.into_iter().filter(|(a, b)| a != b).collect()));
            }
        }
    }
    out
}
