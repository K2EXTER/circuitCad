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

/// Everything within the rows and columns from `a` to `b` (both corners in) turned half a turn about the box's middle: the parts
/// whose leads are all inside, the link ends (a link with one end outside stretches), the wires from off the board, the cuts, and
/// the bare wires (one with an end outside stays where it was unless it stays straight). Half a turn keeps the strips along the rows,
/// so what was joined along a strip stays joined; a quarter turn would stand a chip's pins across them. On a perfboard (no strips)
/// a quarter turn clockwise (`turned_box` is where the box then is).
pub fn area_turned(l: &StripLayout, a: At, b: At) -> StripLayout {
    let (r0, r1, c0, c1) = (a.0.min(b.0), a.0.max(b.0), a.1.min(b.1), a.1.max(b.1));
    let inside = |at: At| at.0 >= r0 && at.0 <= r1 && at.1 >= c0 && at.1 <= c1;
    // a perfboard turns a quarter turn clockwise about the box's top-left corner (the box then is as tall as it was wide)
    let turn = |at: At| match (inside(at), l.perf) {
        (false, _) => at,
        (true, false) => (r0 + r1 - at.0, c0 + c1 - at.1),
        (true, true) => (r0 + (at.1 - c0), c0 + (r1 - at.0)),
    };
    let mut out = l.clone();
    for p in &mut out.parts {
        if p.leads.iter().all(|(_, at)| inside(*at)) {
            for (_, at) in &mut p.leads {
                *at = turn(*at);
            }
        }
    }
    for k in &mut out.links {
        k.a = turn(k.a);
        k.b = turn(k.b);
    }
    for (_, pins) in &mut out.off {
        for (_, _, at) in pins {
            *at = turn(*at);
        }
    }
    out.cuts = l.cuts.iter().map(|c| turn(*c)).collect();
    for w in &mut out.bare {
        let ends = (turn(w.0), turn(w.1));
        if ends.0 .0 == ends.1 .0 || ends.0 .1 == ends.1 .1 {
            *w = ends;
        }
    }
    out.names = l.names.iter().map(|(at, n)| (turn(*at), n.clone())).collect();
    grown(out)
}

/// Where a box is after `area_turned`: the same on a stripboard, as tall as it was wide on a perfboard.
pub fn turned_box(l: &StripLayout, a: At, b: At) -> (At, At) {
    let (r0, r1, c0, c1) = (a.0.min(b.0), a.0.max(b.0), a.1.min(b.1), a.1.max(b.1));
    if l.perf {
        ((r0, c0), (r0 + (c1 - c0), c0 + (r1 - r0)))
    } else {
        (a, b)
    }
}

/// Part `refdes` turned: a two-lead part a quarter turn about its first lead (standing ↔ lying), a chip or a row of leads half a
/// turn about its middle (pin 1 to the other end), or on a perfboard (no strips) a quarter turn clockwise. None when it is not there
/// or would leave the board.
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
            // a perfboard has no strips to keep along: a quarter turn clockwise about the top-left corner
            *at = if l.perf { (r0 + (at.1 - c0), c0 + (r1 - at.0)) } else { (r0 + r1 - at.0, c0 + c1 - at.1) };
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
    let auto = if saved.perf { crate::perfboard::perf_layout_with(s, opt) } else { strip_layout_with(s, opt) };
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
    // a perfboard has no strips to cut: the new parts' lines are wired round what the person made (`wire_added`)
    if out.perf {
        let mut out = grown(out);
        let nets = pin_nets(s);
        let mut hole_nets: BTreeMap<At, String> = BTreeMap::new();
        for p in &out.parts {
            for (pin, a) in &p.leads {
                if let Some(n) = nets.get(&(p.refdes.clone(), pin.clone())) {
                    hole_nets.insert(*a, n.clone());
                }
            }
        }
        for (_, v) in &out.off {
            for (_, n, a) in v {
                hole_nets.insert(*a, n.clone());
            }
        }
        let pairs: Vec<(String, At, At)> = crate::stripboard::strip_unjoined(s, &out).into_iter().filter(|(_, a, b)| new_holes.contains(a) || new_holes.contains(b)).collect();
        crate::perfboard::wire_added(&mut out, &hole_nets, &pairs);
        return grown(out);
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
    // on their pieces of strip (`free_hole`)
    let lines: Vec<(String, At, At)> = crate::stripboard::strip_unjoined(s, &out).into_iter().filter(|(_, a, b)| new_holes.contains(a) || new_holes.contains(b)).collect();
    join_lines(s, &mut out, lines);
    out
}

/// Joins each of `lines` (net, one end, the other: lines of the ratsnest) on a stripboard by a wire between the free holes nearest its
/// two ends on their pieces of strip (`free_hole`; an end with no room: another hole of the net, the nearest first; a wire from off the
/// board with no room at its end goes on to the other end's piece instead). A line with no room at either end is left (the check says).
pub(crate) fn join_lines(s: &Schematic, out: &mut StripLayout, lines: Vec<(String, At, At)>) {
    let mut taken: std::collections::BTreeSet<At> = out.used().into_iter().map(|(a, _)| a).collect();
    let nets = pin_nets(s);
    let holes_of = |out: &StripLayout, net: &str| -> Vec<At> {
        let leads = out.parts.iter().flat_map(|p| p.leads.iter().map(move |(pin, a)| ((p.refdes.clone(), pin.clone()), *a)));
        let offs = out.off.iter().flat_map(|(r, v)| v.iter().map(move |(pin, _, a)| ((r.clone(), pin.clone()), *a)));
        leads.chain(offs).filter(|(k, _)| nets.get(k).is_some_and(|n| n == net)).map(|(_, a)| a).collect()
    };
    for (net, a, b) in lines {
        // an end with no room on its piece: another hole of the net instead, the nearest first (not one on the other end's piece)
        let end_near = |out: &mut StripLayout, at: At, other: At, taken: &std::collections::BTreeSet<At>| -> Option<At> {
            if let Some(h) = free_hole(out, at, taken) {
                return Some(h);
            }
            let pieces = out.pieces();
            let piece_of = |h: At| pieces.iter().position(|(r, x, y)| *r == h.0 && h.1 >= *x && h.1 <= *y);
            let mut alt: Vec<At> = holes_of(out, &net).into_iter().filter(|h| *h != at && piece_of(*h) != piece_of(other)).collect();
            alt.sort_by_key(|h| (h.0 as i32 - at.0 as i32).abs() + (h.1 as i32 - at.1 as i32).abs());
            if let Some(h) = alt.into_iter().find_map(|h| free_hole(out, h, taken)) {
                return Some(h);
            }
            // last: any free hole already joined to the end (through a wire to a strip with room), the nearest
            let groups = out.hole_groups();
            let g = groups.get(&at)?;
            let og = groups.get(&other);
            groups
                .iter()
                .filter(|(h, x)| *x == g && Some(*x) != og && !taken.contains(*h) && !out.cuts.contains(*h))
                .map(|(h, _)| *h)
                .min_by_key(|h| (h.0 as i32 - at.0 as i32).abs() + (h.1 as i32 - at.1 as i32).abs())
        };
        // a wire from off the board with no room at its end: the wire goes on to the other end's piece instead
        let off_end = |out: &StripLayout, at: At| out.off.iter().enumerate().find_map(|(i, (_, v))| v.iter().position(|w| w.2 == at).map(|k| (i, k)));
        let Some(ha) = end_near(out, a, b, &taken) else {
            if let Some((i, k)) = off_end(out, a)
                && let Some(hb) = end_near(out, b, a, &taken)
            {
                out.off[i].1[k].2 = hb;
                taken.insert(hb);
            }
            continue;
        };
        taken.insert(ha);
        let Some(hb) = end_near(out, b, a, &taken) else {
            taken.remove(&ha);
            if let Some((i, k)) = off_end(out, b) {
                out.off[i].1[k].2 = ha;
                taken.insert(ha);
            }
            continue;
        };
        taken.insert(hb);
        out.links.push(Link { a: ha, b: hb, net });
    }
}

/// The free hole nearest `at` on its piece of strip, for a wire's end. A piece with none is made longer (nothing more is joined): a
/// cut at its end moves a hole out when that hole is free, or goes when the piece beyond has nothing on it, or the board grows a
/// column at its right edge.
fn free_hole(out: &mut StripLayout, at: At, taken: &std::collections::BTreeSet<At>) -> Option<At> {
    for _ in 0..6 {
        let pieces = out.pieces();
        let piece_of = |h: At| pieces.iter().copied().find(|(r, x, y)| *r == h.0 && h.1 >= *x && h.1 <= *y);
        let (r, x, y) = piece_of(at)?;
        let mut cols: Vec<u16> = (x..=y).collect();
        cols.sort_by_key(|c| c.abs_diff(at.1));
        if let Some(h) = cols.into_iter().map(|c| (r, c)).find(|h| !taken.contains(h)) {
            return Some(h);
        }
        let empty = |p: Option<(u16, u16, u16)>| p.is_some_and(|(r, x, y)| (x..=y).all(|c| !taken.contains(&(r, c))));
        // a cut moved one hole out, onto a free hole of the next piece (a row shared by nets: each stretch as short as its ends)
        if out.cuts.contains(&(r, y + 1)) && y + 2 < out.cols && !taken.contains(&(r, y + 2)) && !out.cuts.contains(&(r, y + 2)) {
            out.cuts.remove(&(r, y + 1));
            out.cuts.insert((r, y + 2));
            continue;
        }
        if x >= 2 && out.cuts.contains(&(r, x - 1)) && !taken.contains(&(r, x - 2)) && !out.cuts.contains(&(r, x - 2)) {
            out.cuts.remove(&(r, x - 1));
            out.cuts.insert((r, x - 2));
            continue;
        }
        if y + 1 >= out.cols {
            out.cols += 1;
        } else if out.cuts.contains(&(r, y + 1)) && (y + 2 >= out.cols || empty(piece_of((r, y + 2)))) {
            out.cuts.remove(&(r, y + 1));
        } else if x > 0 && out.cuts.contains(&(r, x - 1)) && (x < 2 || empty(piece_of((r, x - 2)))) {
            out.cuts.remove(&(r, x - 1));
        } else {
            return None;
        }
    }
    None
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

    /// A box turned half a turn: the whole board of every sample stays joined as drawn (the strips stay along the rows), turned twice
    /// it is as it was, and an empty box changes nothing. A box that cuts through a part leaves it where it was.
    #[test]
    fn a_box_turns_half_a_turn() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        for name in ["ne555_blinker", "seg7_counter", "traffic_light", "current_sensors"] {
            let s: Schematic = serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{name}.schjson"))).unwrap()).unwrap();
            let l = strip_layout(&s);
            let (a, b) = ((0, 0), (l.rows - 1, l.cols - 1));
            let once = area_turned(&l, a, b);
            assert!(strip_check(&s, &once).is_empty(), "{name}: {:?}", strip_check(&s, &once));
            assert_ne!(once.parts, l.parts, "{name}: turned");
            assert_eq!(area_turned(&once, a, b), l, "{name}: twice is as it was");
        }
        let s = blinker();
        let l = strip_layout(&s);
        assert_eq!(area_turned(&l, (l.rows + 5, 0), (l.rows + 6, 3)), l, "nothing in the box");
        let chip = l.parts.iter().find(|p| p.leads.len() == 8).unwrap();
        let top = chip.leads.iter().map(|x| x.1 .0).min().unwrap();
        let half = area_turned(&l, (0, 0), (top, l.cols - 1));
        assert_eq!(half.parts.iter().find(|p| p.refdes == chip.refdes), Some(chip), "a part half in the box stays");
    }

    /// On a perfboard (no strips) a chip and a box turn a quarter turn: the chip's two rows of pins go from columns to rows, the box
    /// is as tall as it was wide, and four turns bring each back as it was.
    #[test]
    fn a_perfboard_turns_a_quarter_turn() {
        let s = blinker();
        let l = crate::perfboard::perf_layout_with(&s, StripOptions::default());
        let chip = l.parts.iter().find(|p| p.leads.len() == 8).unwrap().refdes.clone();
        let cols = |l: &StripLayout| l.parts.iter().find(|p| p.refdes == chip).unwrap().leads.iter().map(|x| x.1 .1).collect::<std::collections::BTreeSet<u16>>().len();
        // upright two columns of pins, turned four (the layout may have it either way)
        let once = part_turned(&l, &chip).unwrap();
        let mut both = [cols(&l), cols(&once)];
        both.sort();
        assert_eq!(both, [2, 4], "a quarter turn lays the two rows of pins along the rows or the columns");
        let mut back = once;
        for _ in 0..3 {
            back = part_turned(&back, &chip).unwrap();
        }
        assert_eq!(back.parts, l.parts);
        let (a, b) = ((0, 0), (l.rows - 1, l.cols - 1));
        assert_eq!(turned_box(&l, a, b), ((0, 0), (l.cols - 1, l.rows - 1)));
        let mut m = l.clone();
        let mut bx = (a, b);
        for _ in 0..4 {
            let next = turned_box(&m, bx.0, bx.1);
            m = area_turned(&m, bx.0, bx.1);
            bx = next;
            assert!(strip_check(&s, &m).is_empty(), "{:?}", strip_check(&s, &m));
        }
        assert_eq!((m.parts, m.bare, m.links), (l.parts.clone(), l.bare.clone(), l.links.clone()), "four turns: as it was");
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
        // each part of a sample added to a board laid out without it: cut off from the board's strips, and wired to its nets (the
        // active filter's input source moves its wire on when its piece has no room; the counter's rows are shared by nets, so the
        // cuts between them move out a hole)
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        for name in ["ne555_blinker", "active_filter", "seg7_counter"] {
            let s: Schematic = serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{name}.schjson"))).unwrap()).unwrap();
            for c in &s.components {
                let mut without = s.clone();
                without.components.retain(|x| x.refdes != c.refdes);
                let before = strip_layout(&without);
                if before.parts.len() == strip_layout(&s).parts.len() {
                    continue;
                }
                let l = synced(&s, &before, StripOptions::default());
                assert!(strip_check(&s, &l).is_empty(), "{name}: {} added: {:?}", c.refdes, strip_check(&s, &l));
                // on every row a cut between the board's last hole in use and the new parts, unless the new part's piece was grown
                // onto the board's strip of its own net (a piece with no room for a wire's end: the blinker's D1 onto R3's LEDA strip,
                // on a board with the 555 turned half a turn)
                let used = before.used();
                let cut = |r: u16| {
                    let last = used.iter().filter(|(a, _)| a.0 == r).map(|(a, _)| a.1).max();
                    let same = last.is_some_and(|x| {
                        let n = crate::stripboard::net_at(&s, &l, (r, x));
                        n.is_some() && n == crate::stripboard::net_at(&s, &l, (r, before.cols))
                    });
                    same || l.cuts.iter().any(|c| c.0 == r && last.is_none_or(|x| c.1 > x) && c.1 <= before.cols)
                };
                assert!((0..before.rows).all(cut), "{name}: a column cut between ({} added)", c.refdes);
            }
        }
    }

    #[test]
    fn a_saved_perfboard_wires_the_parts_added() {
        // a part of a sample (the chip and three two-lead parts: each layout takes a while) added to a perfboard laid out without
        // it: put right of the board and wired to its nets round the copper already there (bare wires, a link where they cannot
        // pass); the rest stays as it was
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let opt = StripOptions::default();
        let mut n_left = 0;
        for name in ["ne555_blinker"] {
            let s: Schematic = serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{name}.schjson"))).unwrap()).unwrap();
            for c in s.components.iter().filter(|c| ["U1", "R2", "C1", "D1"].contains(&c.refdes.as_str())) {
                let mut without = s.clone();
                without.components.retain(|x| x.refdes != c.refdes);
                let before = crate::perfboard::perf_layout_with(&without, opt);
                let l = synced(&s, &before, opt);
                assert!(before.bare.iter().all(|w| l.bare.contains(w)) && before.links.iter().all(|k| l.links.contains(k)), "{name}: the wires there stay");
                // a line may be left only at a lead the wires there close in on all sides (the 555's OUT when its resistor is added:
                // alone on its net, the automatic layout wired other nets round it): the check names it
                let mut closed: std::collections::BTreeSet<At> = before.used().into_iter().map(|(a, _)| a).collect();
                for (a, b) in &before.bare {
                    for r in a.0.min(b.0)..=a.0.max(b.0) {
                        for cc in a.1.min(b.1)..=a.1.max(b.1) {
                            closed.insert((r, cc));
                        }
                    }
                }
                let shut = |h: At| [(0i32, 1i32), (1, 0), (0, -1), (-1, 0)].iter().all(|(dr, dc)| {
                    let (y, x) = (h.0 as i32 + dr, h.1 as i32 + dc);
                    y < 0 || x < 0 || closed.contains(&(y as u16, x as u16))
                });
                let left = crate::stripboard::strip_unjoined(&s, &l);
                assert!(left.iter().all(|(_, a, b)| shut(*a) || shut(*b)), "{name}: {} added: {:?}", c.refdes, strip_check(&s, &l));
                if left.is_empty() {
                    assert!(strip_check(&s, &l).is_empty(), "{name}: {} added: {:?}", c.refdes, strip_check(&s, &l));
                    assert!(l.bare.len() > before.bare.len(), "{name}: {} wired", c.refdes);
                }
                n_left += left.len();
            }
        }
        assert!(n_left <= 1, "{n_left} lines left");
    }
}
