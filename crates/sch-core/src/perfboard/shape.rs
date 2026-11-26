//! The parts as the placing sees them (`Shape`): each one's holes, size and pins, upright or turned a quarter turn (`Turn`), and
//! the board's width they need.

use super::*;

/// A chip or a row of leads by itself: each lead's pin, net and place from its top-left corner, and its height and width in holes.
pub(super) struct Shape {
    pub(super) k: usize,
    pub(super) look: Look,
    pub(super) leads: Vec<(String, String, At)>,
    pub(super) h: u16,
    pub(super) w: u16,
}

/// How a part goes on the board: a socket or an adapter decides (two rows its width apart, or one row), else its own look.
pub(super) fn look_on(c: &Circuit, k: usize) -> Look {
    let (refdes, look, _) = &c.on[k];
    let m = c.mounts.get(refdes).cloned().unwrap_or_default();
    match *look {
        Look::TwoLead => Look::TwoLead,
        _ if m.width().is_some() => Look::Dip,
        _ if m.one_row() => Look::Row,
        other => other,
    }
}

pub(super) fn shape(c: &Circuit, k: usize) -> Shape {
    let (refdes, _, pins) = &c.on[k];
    let look = look_on(c, k);
    let mount = c.mounts.get(refdes).cloned().unwrap_or_default();
    let (places, slot) = mount.slots(pins);
    let places = places.max(pins.len()).max(slot.iter().map(|p| p + 1).max().unwrap_or(0));
    if look == Look::Dip {
        // pin 1 top left, down the left column and up the right one
        let half = places.div_ceil(2);
        let width = mount.width().unwrap_or(3);
        let leads = pins
            .iter()
            .zip(&slot)
            .map(|((pin, net), &p)| (pin.clone(), net.clone(), if p < half { (p as u16, 0) } else { ((places - 1 - p) as u16, width) }))
            .collect();
        Shape { k, look, leads, h: half as u16, w: width + 1 }
    } else {
        // a row of leads lies along a row of the board (a header along the edge, a transistor's three legs)
        let leads = pins.iter().zip(&slot).map(|((pin, net), &p)| (pin.clone(), net.clone(), (0, p as u16))).collect();
        Shape { k, look, leads, h: 1, w: places as u16 }
    }
}

/// A chip turned a quarter turn clockwise (its two rows of pins along the board's rows); a row of leads stays as it is (a header
/// lies along the edge already).
pub(super) fn turn(s: Shape) -> Shape {
    if s.look != Look::Dip {
        return s;
    }
    let leads = s.leads.into_iter().map(|(p, n, a)| (p, n, (a.1, s.h - 1 - a.0))).collect();
    Shape { leads, h: s.w, w: s.h, ..s }
}

/// Which chips a placing turns a quarter turn: none, all, or those facing a long header (`Circuit::facing`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Turn {
    Up,
    All,
    Facing,
}

/// The chips and rows of leads (not the two-lead parts), each turned a quarter turn as `turned` says.
pub(super) fn shapes_of(c: &Circuit, turned: Turn) -> Vec<Shape> {
    (0..c.on.len())
        .filter(|k| look_on(c, *k) != Look::TwoLead)
        .map(|k| (k, shape(c, k)))
        .map(|(k, s)| if turned == Turn::All || (turned == Turn::Facing && c.facing.contains(&c.on[k].0)) { turn(s) } else { s })
        .collect()
}

/// The chips with several nets (3 and more, not the supplies or ground) to a long header (a row of 6 leads and more, laid along
/// the top edge): turned a quarter turn, their rows of pins lie along the header's, and the wires from one row to the other need
/// not cross (an upright chip's column of pins under a header's row is crossed by every wire to it).
pub(crate) fn facing_headers(c: &Circuit) -> BTreeSet<String> {
    let plain = |n: &str| !n.is_empty() && !crate::breadboard::is_ground_name(n) && !crate::section_plan::supply_name(n) && !c.supplies.iter().any(|x| x == n);
    let headers: Vec<BTreeSet<&str>> = (0..c.on.len()).filter(|k| look_on(c, *k) == Look::Row && c.on[*k].2.len() >= 6).map(|k| c.on[k].2.iter().map(|(_, n)| n.as_str()).filter(|n| plain(n)).collect()).collect();
    (0..c.on.len())
        .filter(|k| look_on(c, *k) == Look::Dip)
        .filter(|k| {
            let nets: BTreeSet<&str> = c.on[*k].2.iter().map(|(_, n)| n.as_str()).filter(|n| plain(n)).collect();
            headers.iter().any(|h| h.intersection(&nets).count() >= 3)
        })
        .map(|k| c.on[k].0.clone())
        .collect()
}

/// What a chip takes on the board with the free holes round it for the wires (`gap` holes between chips).
pub(super) fn room_of(s: &Shape, gap: u16) -> u32 {
    (s.h + gap) as u32 * (s.w + gap) as u32
}

/// What a two-lead part takes on the board with room round it, in holes.
pub(super) const TWO_ROOM: u32 = 12;

/// The board's width: as asked, else about square for what goes on it (and never narrower than the widest part).
pub(super) fn board_width(shapes: &[Shape], twos: usize, opt: StripOptions, gap: u16, margin: u16) -> u16 {
    let area: u32 = shapes.iter().map(|s| room_of(s, gap)).sum::<u32>() + twos as u32 * TWO_ROOM;
    let widest = shapes.iter().map(|s| s.w).max().unwrap_or(0) + 2 * margin;
    opt.cols.unwrap_or_else(|| ((area as f64 * 1.3).sqrt().ceil() as u16).max(widest)).max(widest).max(8)
}

/// Whether a part is a header (a long row of leads: along the top edge).
pub(super) fn is_head(s: &Shape) -> bool {
    s.look != Look::Dip && s.leads.len() >= LONG_ROW
}

/// The holes a part takes on the board: a chip its outline, a two-lead part its leads and the hole between.
pub(super) fn outline(p: &StripPart) -> Vec<At> {
    let (r0, r1) = (p.leads.iter().map(|x| x.1 .0).min().unwrap_or(0), p.leads.iter().map(|x| x.1 .0).max().unwrap_or(0));
    let (c0, c1) = (p.leads.iter().map(|x| x.1 .1).min().unwrap_or(0), p.leads.iter().map(|x| x.1 .1).max().unwrap_or(0));
    (r0..=r1).flat_map(|r| (c0..=c1).map(move |cc| (r, cc))).collect()
}
