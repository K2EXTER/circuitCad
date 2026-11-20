//! Changes made by hand to a stripboard / perfboard layout (B5): a part or a block of the board moved by whole holes, a part turned,
//! wire links added, moved and taken away, strips cut and joined again; and a saved layout kept in step with a schematic that changed
//! (the parts taken out go, the new ones are added at the right, the rest stays where the person put it).
//!
//! Every change gives a new layout; whether it still joins what the schematic joins is `strip_check`'s to say, run again after each.

use crate::stripboard::{strip_layout_with, At, Link, StripLayout, StripOptions};
use crate::{extract, Schematic};
use std::collections::BTreeMap;

/// The hole `d` rows and `c` columns from `at`; None off the top or the left. The board grows to the right and down.
fn shift(at: At, dr: i32, dc: i32) -> Option<At> {
    let (r, c) = (at.0 as i32 + dr, at.1 as i32 + dc);
    (r >= 0 && c >= 0 && r < u16::MAX as i32 && c < u16::MAX as i32).then_some((r as u16, c as u16))
}

/// The board made big enough for every hole in use (and the cuts), never smaller than it was.
fn grown(mut l: StripLayout) -> StripLayout {
    let mut far = l.used().into_iter().map(|(a, _)| a).chain(l.cuts.iter().copied()).chain(l.bare.iter().flat_map(|(a, b)| [*a, *b])).fold((0u16, 0u16), |m, a| (m.0.max(a.0), m.1.max(a.1)));
    far = (far.0 + 1, far.1 + 1);
    l.rows = l.rows.max(far.0);
    l.cols = l.cols.max(far.1);
    l
}

/// Part `refdes` moved `dr` rows and `dc` columns; None when it is not there or would leave the board at the top or the left.
pub fn part_moved(l: &StripLayout, refdes: &str, dr: i32, dc: i32) -> Option<StripLayout> {
    let mut out = l.clone();
    let p = out.parts.iter_mut().find(|p| p.refdes == refdes)?;
    for (_, at) in &mut p.leads {
        *at = shift(*at, dr, dc)?;
    }
    Some(grown(out))
}

/// Everything within the rows and columns from `a` to `b` (both corners in): the parts whose leads are all inside, the link ends,
/// the cuts and the wires from off the board, moved `dr` rows and `dc` columns. A link with one end outside stretches. None when
/// something would leave the board at the top or the left.
pub fn area_moved(l: &StripLayout, a: At, b: At, dr: i32, dc: i32) -> Option<StripLayout> {
    let inside = |at: At| at.0 >= a.0.min(b.0) && at.0 <= a.0.max(b.0) && at.1 >= a.1.min(b.1) && at.1 <= a.1.max(b.1);
    let mut out = l.clone();
    for p in &mut out.parts {
        if p.leads.iter().all(|(_, at)| inside(*at)) {
            for (_, at) in &mut p.leads {
                *at = shift(*at, dr, dc)?;
            }
        }
    }
    for k in &mut out.links {
        for end in [&mut k.a, &mut k.b] {
            if inside(*end) {
                *end = shift(*end, dr, dc)?;
            }
        }
    }
    for (_, pins) in &mut out.off {
        for (_, _, at) in pins {
            if inside(*at) {
                *at = shift(*at, dr, dc)?;
            }
        }
    }
    for w in &mut out.bare {
        let ends = (if inside(w.0) { shift(w.0, dr, dc)? } else { w.0 }, if inside(w.1) { shift(w.1, dr, dc)? } else { w.1 });
        // a bare wire stays straight: with one end inside and the move off its row or column, it stays where it was
        if ends.0 .0 == ends.1 .0 || ends.0 .1 == ends.1 .1 {
            *w = ends;
        }
    }
    let mut cuts = std::collections::BTreeSet::new();
    for c in &l.cuts {
        cuts.insert(if inside(*c) { shift(*c, dr, dc)? } else { *c });
    }
    out.cuts = cuts;
    Some(grown(out))
}

/// Part `refdes` turned: a two-lead part a quarter turn about its first lead (standing ↔ lying), a chip or a row of leads half a
/// turn about its middle (pin 1 to the other end). None when it is not there or would leave the board.
pub fn part_turned(l: &StripLayout, refdes: &str) -> Option<StripLayout> {
    let mut out = l.clone();
    let p = out.parts.iter_mut().find(|p| p.refdes == refdes)?;
    if p.leads.len() == 2 {
        let (a, b) = (p.leads[0].1, p.leads[1].1);
        let (dr, dc) = (b.0 as i32 - a.0 as i32, b.1 as i32 - a.1 as i32);
        // a quarter turn: (dr, dc) -> (dc, -dr)
        p.leads[1].1 = shift(a, dc, -dr)?;
    } else {
        let (r0, r1) = (p.leads.iter().map(|(_, a)| a.0).min()?, p.leads.iter().map(|(_, a)| a.0).max()?);
        let (c0, c1) = (p.leads.iter().map(|(_, a)| a.1).min()?, p.leads.iter().map(|(_, a)| a.1).max()?);
        for (_, at) in &mut p.leads {
            *at = (r0 + r1 - at.0, c0 + c1 - at.1);
        }
    }
    Some(grown(out))
}

/// The schematic net of each part's pin: (refdes, pin) → net.
fn pin_nets(s: &Schematic) -> BTreeMap<(String, String), String> {
    let mut out = BTreeMap::new();
    for n in extract(&s.flat()) {
        for p in n.pins {
            out.insert((p.refdes, p.pin), n.name.clone());
        }
    }
    out
}

/// The net a hole is on: the schematic net of a lead or an off-board wire on the same piece of strip (the colour of a link drawn
/// there). Empty when nothing on the piece has a net.
pub fn net_at(s: &Schematic, l: &StripLayout, at: At) -> String {
    let pieces = l.pieces();
    let Some(&(r, c0, c1)) = pieces.iter().find(|(r, a, b)| *r == at.0 && at.1 >= *a && at.1 <= *b) else { return String::new() };
    let on = |h: At| h.0 == r && h.1 >= c0 && h.1 <= c1;
    let nets = pin_nets(s);
    for p in &l.parts {
        for (pin, h) in &p.leads {
            if on(*h)
                && let Some(n) = nets.get(&(p.refdes.clone(), pin.clone()))
            {
                return n.clone();
            }
        }
    }
    l.off.iter().flat_map(|(_, v)| v.iter()).find(|(_, n, h)| on(*h) && !n.is_empty()).map(|(_, n, _)| n.clone()).unwrap_or_default()
}

/// The layout with a link from `a` to `b` (coloured by the net at either end).
pub fn with_link(s: &Schematic, l: &StripLayout, a: At, b: At) -> StripLayout {
    let mut out = l.clone();
    let net = Some(net_at(s, l, a)).filter(|n| !n.is_empty()).unwrap_or_else(|| net_at(s, l, b));
    out.links.push(Link { a, b, net });
    grown(out)
}

/// The layout with a bare wire drawn by hand on the copper side from `a` to `b` (a perfboard); None unless the two are apart on
/// one row or one column.
pub fn with_bare(l: &StripLayout, a: At, b: At) -> Option<StripLayout> {
    if a == b || (a.0 != b.0 && a.1 != b.1) {
        return None;
    }
    let mut out = l.clone();
    out.bare.push((a, b));
    Some(grown(out))
}

/// The layout without the bare wires drawn by hand that run over hole `at` (ends included); None when there is none.
pub fn without_bare_at(l: &StripLayout, at: At) -> Option<StripLayout> {
    let on = |(a, b): &(At, At)| (a.0 == b.0 && at.0 == a.0 && at.1 >= a.1.min(b.1) && at.1 <= a.1.max(b.1)) || (a.1 == b.1 && at.1 == a.1 && at.0 >= a.0.min(b.0) && at.0 <= a.0.max(b.0));
    let mut out = l.clone();
    out.bare.retain(|w| !on(w));
    (out.bare.len() < l.bare.len()).then_some(out)
}

/// The layout without link `k`.
pub fn without_link(l: &StripLayout, k: usize) -> StripLayout {
    let mut out = l.clone();
    if k < out.links.len() {
        out.links.remove(k);
    }
    out
}

/// End `end` (0 = a, 1 = b) of link `k` moved to `to`, its colour from where it now ends.
pub fn link_end_moved(s: &Schematic, l: &StripLayout, k: usize, end: usize, to: At) -> StripLayout {
    let mut out = l.clone();
    let Some(link) = out.links.get_mut(k) else { return out };
    if end == 0 {
        link.a = to;
    } else {
        link.b = to;
    }
    let (a, b) = (link.a, link.b);
    out.links[k].net = Some(net_at(s, l, a)).filter(|n| !n.is_empty()).unwrap_or_else(|| net_at(s, l, b));
    grown(out)
}

/// The strip cut at `at`, or joined again when it was cut.
pub fn cut_toggled(l: &StripLayout, at: At) -> StripLayout {
    let mut out = l.clone();
    if !out.cuts.remove(&at) {
        out.cuts.insert(at);
    }
    grown(out)
}

/// A saved layout in step with the schematic: the parts no longer in it (or whose pins changed) go, the parts new to it are laid out
/// as the automatic layout would and put to the right of the board (a column between, cut on every row, and three spare holes
/// either side), with the automatic layout's cuts among them, and every other part, link and cut stays where it was. The wires the
/// new parts need are drawn: each line of the ratsnest that ends at a new part, between the free holes nearest its ends on their
/// pieces of strip; a piece with no free hole leaves its net apart, which the check names.
pub fn synced(s: &Schematic, saved: &StripLayout, opt: StripOptions) -> StripLayout {
    let auto = strip_layout_with(s, opt);
    let mut out = saved.clone();
    let pins = |leads: &[(String, At)]| {
        let mut v: Vec<&String> = leads.iter().map(|(p, _)| p).collect();
        v.sort();
        v.into_iter().cloned().collect::<Vec<_>>()
    };
    out.parts.retain(|p| auto.parts.iter().any(|a| a.refdes == p.refdes && pins(&a.leads) == pins(&p.leads)));
    for p in &mut out.parts {
        // a footprint changed (a SOP for a DIP): the socket or the adapter follows the schematic
        if let Some(a) = auto.parts.iter().find(|a| a.refdes == p.refdes) {
            p.mount = a.mount.clone();
        }
    }
    out.off.retain(|(r, v)| auto.off.iter().any(|(a, w)| a == r && w.len() == v.len()));
    let new_parts: Vec<_> = auto.parts.iter().filter(|a| !out.parts.iter().any(|p| p.refdes == a.refdes)).cloned().collect();
    let new_off: Vec<_> = auto.off.iter().filter(|(a, _)| !out.off.iter().any(|(r, _)| r == a)).cloned().collect();
    if new_parts.is_empty() && new_off.is_empty() {
        return out;
    }
    // the new ones keep their places among themselves, moved right of everything on the board
    let left = new_parts.iter().flat_map(|p| p.leads.iter().map(|(_, a)| a.1)).chain(new_off.iter().flat_map(|(_, v)| v.iter().map(|(_, _, a)| a.1))).min().unwrap_or(0);
    // a column between the board and the new parts (cut on every row), then three spare holes: the pieces of the new parts' strips
    // need free holes for the ends of their wires
    const SPARE: i32 = 3;
    let dc = out.cols as i32 + 1 + SPARE - left as i32;
    let gap = out.cols;
    let span = new_parts.iter().flat_map(|p| p.leads.iter().map(|(_, a)| a.1)).fold((u16::MAX, 0u16), |m, c| (m.0.min(c), m.1.max(c)));
    let mut new_holes: Vec<At> = vec![];
    for mut p in new_parts {
        for (_, at) in &mut p.leads {
            at.1 = (at.1 as i32 + dc) as u16;
            new_holes.push(*at);
        }
        out.parts.push(p);
    }
    for (r, mut v) in new_off {
        for (_, _, at) in &mut v {
            at.1 = (at.1 as i32 + dc) as u16;
            new_holes.push(*at);
        }
        out.off.push((r, v));
    }
    // the new parts' strips cut off from the board's (a column between), and cut as the automatic layout cuts them (a chip's pins
    // each on a piece of their own)
    let rows = out.rows.max(auto.rows);
    for r in 0..rows {
        out.cuts.insert((r, gap));
    }
    if span.0 <= span.1 {
        for c in auto.cuts.iter().filter(|c| c.1 as i32 + dc > gap as i32 && c.1 <= span.1 + 1) {
            if let Some(at) = shift(*c, 0, dc) {
                out.cuts.insert(at);
            }
        }
        out.cols = out.cols.max((span.1 as i32 + dc + 1 + SPARE) as u16);
    }
    let mut out = grown(out);
    // the wires the new parts need: each line of the ratsnest that ends at a new part, between the free holes nearest its two ends
    // on their pieces of strip
    let mut taken: std::collections::BTreeSet<At> = out.used().into_iter().map(|(a, _)| a).collect();
    let pieces = out.pieces();
    for (net, a, b) in crate::stripboard::strip_unjoined(s, &out) {
        if !new_holes.contains(&a) && !new_holes.contains(&b) {
            continue;
        }
        let free_near = |at: At, taken: &std::collections::BTreeSet<At>| -> Option<At> {
            let (r, x, y) = *pieces.iter().find(|(r, x, y)| *r == at.0 && at.1 >= *x && at.1 <= *y)?;
            let mut cols: Vec<u16> = (x..=y).collect();
            cols.sort_by_key(|c| c.abs_diff(at.1));
            cols.into_iter().map(|c| (r, c)).find(|h| !taken.contains(h))
        };
        let Some(ha) = free_near(a, &taken) else { continue };
        taken.insert(ha);
        let Some(hb) = free_near(b, &taken) else {
            taken.remove(&ha);
            continue;
        };
        taken.insert(hb);
        out.links.push(Link { a: ha, b: hb, net });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stripboard::{strip_check, strip_layout};

    fn blinker() -> Schematic {
        serde_json::from_str(include_str!("../../../docs/examples/ne555_blinker.schjson")).unwrap()
    }

    /// Moving the whole board by an area keeps it joined as drawn; moving one chip off its cut pieces does not, and the check says so.
    #[test]
    fn moves_keep_or_break_the_joins_and_the_check_follows() {
        let s = blinker();
        let l = strip_layout(&s);
        let whole = area_moved(&l, (0, 0), (l.rows - 1, l.cols - 1), 2, 3).unwrap();
        assert!(strip_check(&s, &whole).is_empty(), "{:?}", strip_check(&s, &whole));
        assert!(whole.used().iter().all(|(a, _)| a.0 < whole.rows && a.1 < whole.cols), "the board grows with what is on it");
        assert!(whole.rows >= l.rows && whole.cols >= l.cols);
        let chip = l.parts.iter().find(|p| p.leads.len() == 8).unwrap().refdes.clone();
        let off = part_moved(&l, &chip, 0, 1).unwrap();
        assert!(!strip_check(&s, &off).is_empty(), "a chip moved off its pieces is caught");
        assert!(part_moved(&l, &chip, -100, 0).is_none(), "not off the top");
        // turning a chip twice puts it back
        let back = part_turned(&part_turned(&l, &chip).unwrap(), &chip).unwrap();
        assert_eq!(back.parts, l.parts);
    }

    #[test]
    fn links_and_cuts_by_hand() {
        let s = blinker();
        let l = strip_layout(&s);
        // a link taken away leaves a net apart; drawn again between the same holes, it is whole again with its net's colour
        let k = l.links[0].clone();
        let open = without_link(&l, 0);
        assert!(!strip_check(&s, &open).is_empty());
        let again = with_link(&s, &open, k.a, k.b);
        assert!(strip_check(&s, &again).is_empty(), "{:?}", strip_check(&s, &again));
        assert_eq!(again.links.last().unwrap().net, k.net);
        // a cut joined again shorts two pins' pieces; cutting it again puts it back
        let c = *l.cuts.iter().next().unwrap();
        let joined = cut_toggled(&l, c);
        assert_eq!(cut_toggled(&joined, c).cuts, l.cuts);
        // an end moved to another hole and back
        let moved = link_end_moved(&s, &l, 0, 1, (k.b.0, k.b.1 + 1));
        assert_eq!(link_end_moved(&s, &moved, 0, 1, k.b).links, l.links);
    }

    /// Bare wires drawn by hand on a perfboard's copper side: one in place of a link joins the net again; one whose way runs over
    /// another net's wire touches it (a short, as on the board). A wire is straight along a row or a column, a right click on it
    /// takes it away, it is kept with the layout, and it is drawn among the perfboard's wires.
    #[test]
    fn bare_wires_by_hand() {
        let s = blinker();
        let l = strip_layout(&s);
        let straight: Vec<usize> = (0..l.links.len()).filter(|k| l.links[*k].a.1 == l.links[*k].b.1).collect();
        let bare_for = |k: usize| with_bare(&without_link(&l, k), l.links[k].a, l.links[k].b).unwrap();
        let clean = straight.iter().copied().find(|k| strip_check(&s, &bare_for(*k)).is_empty()).expect("a link a bare wire can take the place of");
        assert!(!strip_check(&s, &without_link(&l, clean)).is_empty(), "the link alone joined the net");
        let crossing = straight.iter().copied().find(|k| strip_check(&s, &bare_for(*k)).iter().any(|p| p.starts_with("ショート"))).expect("a bare wire over another net's wire");
        let spans = l.perf_spans();
        let k = &l.links[crossing];
        assert!((k.a.0.min(k.b.0) + 1..k.a.0.max(k.b.0)).any(|r| spans.iter().any(|(p, q)| p.0 == r && k.a.1 >= p.1 && k.a.1 <= q.1)), "it runs over a wire");
        // straight only; taken away by a hole on it; saved; drawn
        assert!(with_bare(&l, (0, 0), (1, 1)).is_none() && with_bare(&l, (0, 0), (0, 0)).is_none());
        let m = bare_for(clean);
        let mid = l.links[clean].a;
        assert_eq!(without_bare_at(&m, mid).unwrap().bare, vec![]);
        assert!(without_bare_at(&l, mid).is_none());
        let back: StripLayout = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(back, m);
        assert!(!serde_json::to_string(&l).unwrap().contains("bare"), "nothing written without them");
        assert!(crate::strip_print::perf_wires(&m).contains(&m.bare[0]));
        // a block moved with both ends inside moves the wire; one end inside and a sideways move leaves it straight where it was
        let (a, b) = m.bare[0];
        let all = area_moved(&m, (0, 0), (m.rows, m.cols), 0, 1).unwrap();
        assert_eq!(all.bare[0], ((a.0, a.1 + 1), (b.0, b.1 + 1)));
        let one = area_moved(&m, a, a, 0, 1).unwrap();
        assert_eq!(one.bare[0], (a, b));
    }

    /// A part added to the schematic lands right of the saved board (its strips cut off from the board's, and the wires its nets need
    /// drawn), the moved parts stay where they were, a part taken out goes.
    #[test]
    fn a_saved_layout_follows_the_schematic() {
        let s = blinker();
        let auto = strip_layout(&s);
        let first = auto.parts.iter().find(|p| p.leads.len() == 2).unwrap().refdes.clone();
        let saved = part_moved(&auto, &first, 0, 1).unwrap();
        assert_eq!(synced(&s, &saved, StripOptions::default()), saved, "nothing changed: nothing to do");
        let mut more = s.clone();
        let k = more.add(crate::Kind::Resistor, (100, 100), 0);
        let added = more.components[k].refdes.clone();
        let l = synced(&more, &saved, StripOptions::default());
        let r = l.parts.iter().find(|p| p.refdes == added).expect("the new resistor");
        assert!(r.leads.iter().all(|(_, a)| a.1 > saved.cols), "{:?} right of {} columns", r.leads, saved.cols);
        assert_eq!(l.parts.iter().find(|p| p.refdes == first), saved.parts.iter().find(|p| p.refdes == first), "a moved part stays");
        let mut fewer = s.clone();
        fewer.components.retain(|c| c.refdes != first);
        assert!(synced(&fewer, &saved, StripOptions::default()).parts.iter().all(|p| p.refdes != first));
        // each part of the sample added to a board laid out without it: cut off from the board's strips, and wired to its nets
        for c in &s.components {
            let mut without = s.clone();
            without.components.retain(|x| x.refdes != c.refdes);
            let before = strip_layout(&without);
            let l = synced(&s, &before, StripOptions::default());
            assert!(strip_check(&s, &l).is_empty(), "{} added: {:?}", c.refdes, strip_check(&s, &l));
            assert!(l.links.len() > before.links.len(), "{} added: wires drawn", c.refdes);
            assert!((0..l.rows).all(|r| l.cuts.contains(&(r, before.cols))), "a column cut between");
        }
    }
}
