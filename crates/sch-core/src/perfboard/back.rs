//! The chips on the copper side (`back_chips`, `with_back`).

use super::*;

/// The chips that go on the copper side, by reference, with their size: those the person put there (`MountChoice::Lands`), and with
/// `all` (`StripOptions::chips_back`) the 1206 and 0805 resistors and capacitors built with the leaded part otherwise whose way on
/// the person did not choose.
pub(crate) fn back_chips(s: &Schematic, c: &Circuit, all: bool) -> BTreeMap<String, String> {
    let flat = s.flat();
    let comp = |r: &str| flat.components.iter().find(|x| x.refdes == r);
    c.on.iter()
        .filter(|(_, _, pins)| pins.len() == 2)
        .filter_map(|(r, _, _)| match c.mounts.get(r) {
            Some(crate::mount::Mount::Back { size }) => Some((r.clone(), size.clone())),
            Some(crate::mount::Mount::Leaded) if all && comp(r).is_some_and(|x| x.mount.is_none()) => Some((r.clone(), crate::mount::back_size(comp(r)?)?.to_string())),
            _ => None,
        })
        .collect()
}

/// The chips put on the copper side of a layout made without them: each across two holes side by side, an end on a hole that
/// already carries its net (a chip's pin: soldered on that pad) where it can, else on a free hole as near as can be to its net. A
/// hole is a back chip's alone (two chips' ends never share one); a lead of another net is never under an end. The wiring after
/// joins the free ends.
pub(super) fn with_back(c: &Circuit, mut l: StripLayout, backs: &BTreeMap<String, String>) -> StripLayout {
    let mut nets: BTreeMap<At, String> = BTreeMap::new();
    for p in &l.parts {
        for (pin, a) in &p.leads {
            nets.insert(*a, lead_net(c, &p.refdes, pin));
        }
    }
    for (_, pins) in &l.off {
        for (_, n, a) in pins {
            nets.insert(*a, n.clone());
        }
    }
    let mut taken: BTreeSet<At> = BTreeSet::new();
    let dist = |a: At, b: At| a.0.abs_diff(b.0) as u32 + a.1.abs_diff(b.1) as u32;
    let far = (l.rows + l.cols) as u32;
    for (r, size) in backs {
        let Some((_, look, pins)) = c.on.iter().find(|x| &x.0 == r) else { continue };
        let ((p1, n1), (p2, n2)) = (&pins[0], &pins[1]);
        // how far an end on hole `h` is from its net: 0 on a hole of the net, else to the nearest one (none yet: as far as the board)
        let cost = |h: At, n: &str, nets: &BTreeMap<At, String>| -> Option<u32> {
            match nets.get(&h) {
                Some(x) if x == n && !n.is_empty() => Some(0),
                Some(_) => None,
                None => Some(nets.iter().filter(|(_, x)| *x == n).map(|(a, _)| dist(h, *a)).min().unwrap_or(far)),
            }
        };
        let mut best: Option<(u32, At, At)> = None;
        for row in 0..l.rows {
            for col in 0..l.cols {
                let a = (row, col);
                for b in [(row, col + 1), (row + 1, col)] {
                    if b.0 >= l.rows || b.1 >= l.cols || taken.contains(&a) || taken.contains(&b) {
                        continue;
                    }
                    for (x, y) in [(a, b), (b, a)] {
                        let (Some(cx), Some(cy)) = (cost(x, n1, &nets), cost(y, n2, &nets)) else { continue };
                        // a free end next to a lead of another net is harder to solder: a little worse
                        let crowd = [x, y]
                            .iter()
                            .filter(|h| !nets.contains_key(h))
                            .flat_map(|h| [(h.0.wrapping_sub(1), h.1), (h.0 + 1, h.1), (h.0, h.1.wrapping_sub(1)), (h.0, h.1 + 1)])
                            .filter(|q| nets.get(q).is_some_and(|m| m != n1 && m != n2))
                            .count() as u32;
                        let k = cx + cy + crowd;
                        if best.is_none_or(|b| k < b.0) {
                            best = Some((k, x, y));
                        }
                    }
                }
            }
        }
        let Some((_, x, y)) = best else { continue };
        taken.extend([x, y]);
        nets.entry(x).or_insert_with(|| n1.clone());
        nets.entry(y).or_insert_with(|| n2.clone());
        l.parts.push(StripPart { refdes: r.clone(), look: *look, leads: vec![(p1.clone(), x), (p2.clone(), y)], mount: crate::mount::Mount::Back { size: size.clone() } });
    }
    l
}
