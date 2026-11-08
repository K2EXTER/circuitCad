//! First-guess placement: pack the parts into rows, strongly connected parts next to each other,
//! and fit a rectangular outline around them. The order of the parts is first made greedily (the part most connected
//! to those already placed comes next), then improved by a short, repeatable annealing run that shortens the nets
//! (half-perimeter of each net's box). A starting point to refine by hand.

use crate::*;

mod blocks;
mod finish;
mod ics;
mod order;
#[cfg(test)]
mod tests;

use blocks::*;
use finish::*;
pub(crate) use finish::crowded;
use ics::*;
use order::*;

/// Clear space kept around every part's pads (mm), on top of the DRC clearance.
/// (2.4: with the shipped examples autorouted, 1.6 left 8 connections open, 2.0 left 6, 2.4 left 2 and 3.0 left 3.)
const GAP: f64 = 2.4;
/// Extra room above every part for its reference text (silkscreen), so labels do not run into the neighbours.
const LABEL: f64 = 2.4;
/// The same two, for a board whose ground is poured (see `Tuning`).
const POURED_GAP: f64 = 1.2;
const POURED_LABEL: f64 = 1.6;
/// Space between the parts and the board edge (mm).
const MARGIN: f64 = 3.0;
/// Part origins land on this grid (mm), the editor's default snap, so hand-moved parts line up with placed ones.
const SNAP: f64 = 0.5;

pub(crate) fn snap(v: f64) -> f64 {
    (v / SNAP).round() * SNAP
}

#[derive(Debug, Default)]
pub struct PlaceReport {
    pub placed: usize,
    pub width: f64,
    pub height: f64,
    /// Nets left out of the placement because a pour carries them (see `Tuning::skip_poured`).
    pub poured: Vec<String>,
    /// Vias of the pour that were put again (see `pour_wiring`).
    pub repoured: usize,
    /// Pairs of airwires of different nets that cross (see `ratsnest::crossings`), after the placement.
    pub crossings: usize,
    /// What the groups of the requirements could not get: parts that are not in the block of their group (they are in a copy or an IC's block).
    pub group_notes: Vec<String>,
}

pub(crate) fn extent(board: &Board, refdes: &str) -> Option<(Point, Point)> {
    let mut lo = Point::new(f64::MAX, f64::MAX);
    let mut hi = Point::new(f64::MIN, f64::MIN);
    let mut any = false;
    for p in board.pads.iter().filter(|p| p.refdes == refdes) {
        let (w, h) = p.shape.size();
        lo.x = lo.x.min(p.pos.x - w / 2.0);
        lo.y = lo.y.min(p.pos.y - h / 2.0);
        hi.x = hi.x.max(p.pos.x + w / 2.0);
        hi.y = hi.y.max(p.pos.y + h / 2.0);
        any = true;
    }
    any.then_some((lo, hi))
}

/// Lay every part out in rows starting at the origin and replace the outline by a rectangle
/// around them. Refuses when tracks or vias exist, since moving parts would break them.
pub fn autoplace(board: &mut Board) -> Result<PlaceReport, String> {
    place(board, true, true, &Tuning::for_board(board))
}

/// The numbers that shape a placement.
#[derive(Clone, Copy, Debug)]
pub struct Tuning {
    /// Clear space round every part's pads (mm).
    pub gap: f64,
    /// Extra room above every part for its reference text (mm).
    pub label: f64,
    /// Leave the nets that a pour carries (usually GND) out of the placement: they need no wire, so they must not
    /// pull parts together.
    pub skip_poured: bool,
    /// Line the parts up afterwards (see `align_parts`).
    pub align: bool,
    /// Pull each bypass capacitor to the supply pin it serves (see `bypass_nets`).
    pub bypass: bool,
    /// When the circuit has several schematic sheets, lay each sheet's parts out as a block of its own and then the blocks
    /// (see `sheet_blocks`). Off by default: the blocks are easy to tell apart, but the board is bigger and the wiring longer.
    pub sheets: bool,
    /// Lay out copies of one small structure (eight range switches, ten LEDs with their resistors) alike, in a regular grid (see `build_arrays`).
    pub arrays: bool,
    /// Copies that go together (a range stage and the LED of that range) are one bigger copy (see `replicas::merge_channels`). Off by default:
    /// it shortens the wiring where the LEDs go with the stages, but breaks up a row of LEDs of which only some hang on a stage.
    pub channels: bool,
    /// The parts of each group of the board's requirements (`Board.place_groups`) are laid out as one block that moves as one piece.
    pub groups: bool,
    /// The distances the board's requirements say to keep (`Board.keep_apart`) count in the layout: parts of the two sides are put further apart.
    pub apart: bool,
}

impl Tuning {
    /// Without a pour every net needs wires (see `GAP`). With one, the poured nets drop out of the cost and the
    /// parts can sit closer.
    pub fn for_board(board: &Board) -> Self {
        if board.zones.iter().any(|z| !z.net.is_empty()) {
            Self { gap: POURED_GAP, label: POURED_LABEL, skip_poured: true, align: true, bypass: true, sheets: false, arrays: true, channels: false, groups: true, apart: true }
        } else {
            Self { gap: GAP, label: LABEL, skip_poured: false, align: true, bypass: true, sheets: false, arrays: true, channels: false, groups: true, apart: true }
        }
    }
}

/// `autoplace` with explicit numbers (for measuring them).
pub fn autoplace_with(board: &mut Board, tuning: &Tuning) -> Result<PlaceReport, String> {
    place(board, true, true, tuning)
}

/// The pours that filled the old board (a rectangle a little inside its edge) follow the new outline, with the same
/// margin. Any other pour is left as it is.
fn refit_pours(board: &mut Board, lo: Point, hi: Point) {
    let (nlo, nhi) = match board.bounds() {
        Some(b) => b,
        None => return,
    };
    for z in board.zones.iter_mut() {
        let Some((zlo, zhi)) = z.bounds() else { continue };
        // a pour of one rectangle (four straight corners at the box of the shape)
        let rect = z.shapes.len() == 1
            && z.shapes[0].shape.verts.len() == 4
            && z.shapes[0].shape.verts.iter().all(|v| v.arc_mid.is_none() && (v.p.x == zlo.x || v.p.x == zhi.x) && (v.p.y == zlo.y || v.p.y == zhi.y));
        let inset = [zlo.x - lo.x, zlo.y - lo.y, hi.x - zhi.x, hi.y - zhi.y];
        if rect && inset.iter().all(|d| (0.0..=2.0).contains(d)) {
            z.shapes[0].shape = crate::shape::Shape::from_points(&[Point::new(nlo.x + inset[0], nlo.y + inset[1]), Point::new(nlo.x + inset[0], nhi.y - inset[3]), Point::new(nhi.x - inset[2], nhi.y - inset[3]), Point::new(nhi.x - inset[2], nlo.y + inset[1])]);
        }
    }
}

/// The IC that the requirements table says a capacitor serves (None: the placement works it out).
fn bypass_ic<'a>(board: &'a Board, cap: &str) -> Option<&'a str> {
    board.bypass.iter().find(|b| b.caps.iter().any(|c| c == cap)).map(|b| b.ic.as_str())
}

/// A bypass capacitor belongs right next to the supply pin it serves: a `C` part with one pad on a supply net and
/// another on ground, and an IC (`U`, 5+ pads) with a pad on that net. Each such pair is a two-pad net of its own, three
/// times over (the placement adds up the lengths of nets, so it counts three times as much as an ordinary connection),
/// pulling the capacitor to the pin and not just somewhere in the supply's box.
fn bypass_nets(board: &Board, items: &[(String, Point, f64, f64)], gap: f64, label: f64) -> Vec<NetPins> {
    use crate::drc::{is_ground_net, is_negative_supply_net, is_supply_net};
    let index = |r: &str| items.iter().position(|it| it.0 == r);
    let offset = |pad: &Pad, i: usize| (i, pad.pos.x - items[i].1.x + gap / 2.0, pad.pos.y - items[i].1.y + gap / 2.0 + label);
    // a rail: the nets a capacitor bypasses between (ground, a supply, a negative supply)
    let rail = |n: &str| is_ground_net(n) || is_supply_net(n) || is_negative_supply_net(n);
    let mut out: Vec<NetPins> = vec![];
    for cap in board.parts.iter().filter(|p| p.refdes.starts_with('C')) {
        let cap_pads: Vec<&Pad> = board.pads.iter().filter(|p| p.refdes == cap.refdes).collect();
        let Some(ci) = index(&cap.refdes) else { continue };
        // the pads on a supply (positive or negative) whose other side is another rail: a ground, or the other supply the IC runs between
        for cp in cap_pads.iter().filter(|p| (is_supply_net(&p.net) || is_negative_supply_net(&p.net)) && cap_pads.iter().any(|q| q.net != p.net && rail(&q.net))) {
            for ic in board.parts.iter().filter(|p| p.refdes.starts_with('U') && bypass_ic(board, &cap.refdes).is_none_or(|b| b == p.refdes)) {
                let ic_pads: Vec<&Pad> = board.pads.iter().filter(|p| p.refdes == ic.refdes).collect();
                if ic_pads.len() < 5 {
                    continue;
                }
                let Some(ii) = index(&ic.refdes) else { continue };
                for sp in ic_pads.iter().filter(|p| p.net == cp.net) {
                    for _ in 0..3 {
                        out.push(vec![offset(cp, ci), offset(sp, ii)]);
                    }
                }
            }
        }
    }
    out
}

/// When every track and via of `board` belongs to a net that a pour carries, they are what `groundpour` made: the
/// options to make them again, one per net, with the width and via size they had. `None` when anything else is wired.
fn pour_wiring(board: &Board) -> Option<Vec<(String, groundpour::PourOptions)>> {
    let conn = crate::drc::connectivity(board);
    let poured: Vec<&str> = board.zones.iter().map(|z| z.net.as_str()).filter(|n| !n.is_empty()).collect();
    let mut out: Vec<(String, groundpour::PourOptions)> = vec![];
    let mut note = |net: &str, width: Option<f64>, via: Option<(f64, f64)>| {
        let i = out.iter().position(|(n, _)| n == net).unwrap_or_else(|| {
            out.push((net.to_string(), groundpour::PourOptions { net: Some(net.to_string()), top: false, ..Default::default() }));
            out.len() - 1
        });
        if let Some(w) = width {
            out[i].1.track_width = w;
        }
        if let Some((d, h)) = via {
            out[i].1.via_diameter = d;
            out[i].1.via_drill = h;
        }
    };
    for (i, t) in board.tracks.iter().enumerate() {
        let net = conn.track_net(i).filter(|n| poured.contains(n))?;
        note(net, Some(t.width), None);
    }
    for (i, v) in board.vias.iter().enumerate() {
        let net = conn.via_net(i).filter(|n| poured.contains(n))?;
        note(net, None, Some((v.diameter, v.drill)));
    }
    Some(out)
}

fn place(board: &mut Board, turn: bool, improve: bool, tuning: &Tuning) -> Result<PlaceReport, String> {
    // the placement draws a new outline round the parts: areas drawn on the old board would stand somewhere else
    if !board.keepouts.is_empty() {
        return Err(format!("禁止領域が {} 個ある基板は、自動で並べられません (自動配置は外形を作り直すので、領域の場所がずれます)。先に部品を並べてから禁止領域を描くか、禁止領域を消してから並べてください", board.keepouts.len()));
    }
    // a refusal must leave the board as it was (the pour wiring is taken out on the way)
    let backup = (!board.tracks.is_empty() || !board.vias.is_empty()).then(|| board.clone());
    let r = place_inner(board, turn, improve, tuning);
    if let (Err(_), Some(b)) = (&r, backup) {
        *board = b;
    }
    r
}

fn place_inner(board: &mut Board, turn: bool, improve: bool, tuning: &Tuning) -> Result<PlaceReport, String> {
    let (gap, label) = (tuning.gap, tuning.label);
    // The only wiring that may be there is what the GND pour put in (short tracks and vias that join pads to a poured
    // net): it is taken out now and made again once the parts have their new places.
    let mut repour: Vec<(String, groundpour::PourOptions)> = vec![];
    if !board.tracks.is_empty() || !board.vias.is_empty() {
        let Some(found) = pour_wiring(board) else {
            return Err("配線済みの基板では自動配置できません。先に配線を削除してください (配線は部品の位置に合わせて引かれているため)".into());
        };
        repour = found;
        board.tracks.clear();
        board.vias.clear();
    }
    if board.parts.is_empty() {
        return Err("部品がありません。先に「回路図から取り込み」をしてください".into());
    }
    // the copies of a small structure are turned the way that wires best, the first one tried by routing it alone (a person tries the small
    // circuit first, and copies what works)
    let mut in_copies: std::collections::BTreeSet<String> = Default::default();
    if improve && tuning.arrays {
        let names: Vec<String> = board.parts.iter().map(|p| p.refdes.clone()).collect();
        let dummy: Vec<(String, Point, f64, f64)> = names.iter().map(|r| (r.clone(), Point::default(), 0.0, 0.0)).collect();
        let skip: Vec<String> = if tuning.skip_poured { board.zones.iter().map(|z| z.net.clone()).filter(|n| !n.is_empty()).collect() } else { vec![] };
        let sheet_by_name = |i: usize| board.part_sheets.get(&names[i]).cloned().unwrap_or_default();
        let groups = replicas::find(board, &dummy, &skip, &sheet_by_name);
        let groups = if tuning.channels { replicas::merge_channels(board, &dummy, groups, &skip) } else { groups };
        turn_copies(board, &groups, &names, gap, label);
        in_copies = groups.iter().flat_map(|g| g.iter().flat_map(|c| c.iter().map(|&i| names[i].clone()))).collect();
    }
    // an IC and its bypass capacitors are settled once and move as one piece (the capacitors are turned now, before the parts are measured)
    let blocks: Vec<IcBlock> = if improve && tuning.bypass && tuning.arrays { ic_blocks(board, &in_copies) } else { vec![] };
    for b in &blocks {
        for (cap, rot, _) in &b.caps {
            if let Some(part) = board.parts.iter().find(|p| &p.refdes == cap).cloned() {
                board.place_part(cap, part.pos, *rot);
            }
        }
    }
    // measure each part at the origin, keeping its rotation
    let mut items: Vec<(String, Point, f64, f64)> = vec![]; // refdes, offset of the pad box, w, h
    for part in board.parts.clone() {
        board.place_part(&part.refdes, Point::new(0.0, 0.0), part.rot);
        let Some((lo, hi)) = extent(board, &part.refdes) else { continue };
        items.push((part.refdes, lo, hi.x - lo.x + gap, hi.y - lo.y + gap + label));
    }
    // the nets as pad offsets from each part's cell corner (the parts are all at the origin now)
    let mut nets: std::collections::BTreeMap<&str, NetPins> = Default::default();
    let mut degree = vec![0usize; items.len()];
    let poured: Vec<String> = if tuning.skip_poured { board.zones.iter().map(|z| z.net.clone()).filter(|n| !n.is_empty()).collect() } else { vec![] };
    for pad in board.pads.iter().filter(|p| !p.net.is_empty() && !p.refdes.is_empty() && !poured.contains(&p.net)) {
        if let Some(i) = items.iter().position(|it| it.0 == pad.refdes) {
            nets.entry(pad.net.as_str()).or_default().push((i, pad.pos.x - items[i].1.x + gap / 2.0, pad.pos.y - items[i].1.y + gap / 2.0 + label));
        }
    }
    let nets: Vec<NetPins> = nets.into_values().filter(|pins| pins.iter().any(|p| p.0 != pins[0].0)).collect();
    // how many nets a part is on: each net once, however many pins of the part it has, and only a net that reaches another part
    for pins in &nets {
        let mut on: Vec<usize> = pins.iter().map(|p| p.0).collect();
        on.sort_unstable();
        on.dedup();
        for i in on {
            degree[i] += 1;
        }
    }
    let mut nets = nets;
    if tuning.bypass {
        nets.extend(bypass_nets(board, &items, gap, label));
    }
    if tuning.arrays {
        nets.extend(ic_pair_nets(board, &items, gap, label, &poured));
    }
    // copies of one small structure become one item each (laid out alike in a grid); the placement works on those
    let sheet_name = |i: usize| board.part_sheets.get(&items[i].0).cloned().unwrap_or_default();
    let copies = if tuning.arrays { replicas::find(board, &items, &poured, &sheet_name) } else { vec![] };
    let copies = if tuning.arrays && tuning.channels { replicas::merge_channels(board, &items, copies, &poured) } else { copies };
    let mut arrays = build_arrays(&items, &nets, &degree, &copies, improve);
    // an IC with its capacitors is one item as well: the corners of the cells of its parts, from where the capacitors stand from the IC
    for b in &blocks {
        let Some(ii) = items.iter().position(|it| it.0 == b.ic) else { continue };
        let corner = |i: usize, off: Point| (off.x + items[i].1.x - gap / 2.0, off.y + items[i].1.y - gap / 2.0 - label);
        let mut cells: Vec<(usize, f64, f64)> = vec![(ii, corner(ii, Point::new(0.0, 0.0)).0, corner(ii, Point::new(0.0, 0.0)).1)];
        for (cap, _, off) in &b.caps {
            if let Some(ci) = items.iter().position(|it| &it.0 == cap) {
                let c = corner(ci, *off);
                cells.push((ci, c.0, c.1));
            }
        }
        if cells.len() < 2 {
            continue;
        }
        let (x0, y0) = cells.iter().fold((f64::MAX, f64::MAX), |a, c| (a.0.min(c.1), a.1.min(c.2)));
        let w = cells.iter().map(|c| c.1 - x0 + items[c.0].2).fold(0.0, f64::max);
        let h = cells.iter().map(|c| c.2 - y0 + items[c.0].3).fold(0.0, f64::max);
        arrays.push(Array { members: cells.into_iter().map(|(i, x, y)| (i, x - x0, y - y0)).collect(), w, h });
    }
    // the parts that the design's requirements say belong together are one block each (the parts already in a copy or an IC block stay there)
    let mut group_notes: Vec<String> = vec![];
    if improve && tuning.groups {
        let mut taken: std::collections::BTreeSet<usize> = arrays.iter().flat_map(|a| a.members.iter().map(|m| m.0)).collect();
        for (g, names) in crate::requirements::groups_on_board(board) {
            let members: Vec<usize> = names.iter().filter_map(|r| items.iter().position(|it| &it.0 == r)).filter(|i| !taken.contains(i)).collect();
            let left_out: Vec<&str> = names.iter().filter(|r| items.iter().position(|it| &it.0 == *r).is_some_and(|i| !members.contains(&i))).map(|r| r.as_str()).collect();
            if !left_out.is_empty() {
                let result = if members.len() >= 2 { "残りだけを塊にしました" } else { "塊にしていません" };
                group_notes.push(format!("組「{}」: {} は、コピーまたは IC のブロックにまとまっているため、組の塊に入れられません ({result}。組の上限は距離のコストで守ります)", g.name, left_out.join("・")));
            }
            if members.len() < 2 {
                continue;
            }
            let at = |i: usize| members.iter().position(|&m| m == i);
            let sub_items: Vec<_> = members.iter().map(|&i| items[i].clone()).collect();
            let sub_degree: Vec<usize> = members.iter().map(|&i| degree[i]).collect();
            let sub_nets: Vec<NetPins> = nets
                .iter()
                .map(|pins| pins.iter().filter_map(|&(i, x, y)| at(i).map(|l| (l, x, y))).collect::<NetPins>())
                .filter(|pins| pins.iter().any(|p| p.0 != pins[0].0))
                .collect();
            let tie = ties(&sub_nets, sub_items.len());
            let (spots, w, h) = layout(&sub_items, &sub_nets, &[], &sub_degree, &tie, improve, true, false);
            taken.extend(members.iter().copied());
            arrays.push(Array { members: spots.into_iter().map(|(l, x, y)| (members[l], x, y)).collect(), w, h });
        }
    }
    let (citems, cnets, cdegree, csheet, origin) = collapse(&items, &nets, &degree, &arrays, &sheet_name);
    // the distances of the requirements, for the items of the layout (a part in an array stands for the array): those to keep, and the
    // limits of the groups (a group that is one item already, as a block, needs none)
    let item_of = |r: &String| {
        origin.iter().position(|o| match o {
            Origin::Single(i) => &items[*i].0 == r,
            Origin::Array(k) => arrays[*k].members.iter().any(|m| &items[m.0].0 == r),
        })
    };
    let to_items = |names: &[String]| {
        let mut v: Vec<usize> = names.iter().filter_map(&item_of).collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let mut apart_items: Vec<Apart> = vec![];
    if improve && tuning.apart {
        apart_items.extend(
            crate::requirements::apart_on_board(board)
                .into_iter()
                .map(|(a, b, limit)| Apart { a: to_items(&a), b: to_items(&b), limit, near: false })
                .filter(|r| !r.a.is_empty() && !r.b.is_empty()),
        );
    }
    if improve && tuning.groups {
        for (g, names) in crate::requirements::groups_on_board(board) {
            let v = to_items(&names);
            if g.max_dist > 0.0 && v.len() >= 2 {
                apart_items.push(Apart { a: v.clone(), b: v, limit: g.max_dist, near: true });
            }
        }
    }
    let by_sheet = if tuning.sheets { sheet_groups(&csheet) } else { None };
    let (cspots, used_w, used_h) = match by_sheet {
        Some(groups) => sheet_blocks(&citems, &cnets, &cdegree, &groups, improve),
        None => {
            if citems.is_empty() {
                return Err("パッドを持つ部品がありません".into());
            }
            let tie = ties(&cnets, citems.len());
            layout(&citems, &cnets, &apart_items, &cdegree, &tie, improve, false, false)
        }
    };
    // back to the parts: the members of an array stand where the array's cell puts them
    let mut spots: Vec<(usize, f64, f64)> = vec![];
    for (ci, x, y) in cspots {
        match origin[ci] {
            Origin::Single(i) => spots.push((i, x, y)),
            Origin::Array(k) => spots.extend(arrays[k].members.iter().map(|&(i, ax, ay)| (i, x + ax, y + ay))),
        }
    }
    let in_array: std::collections::BTreeSet<String> = arrays.iter().flat_map(|a| a.members.iter().map(|m| items[m.0].0.clone())).collect();
    for (i, sx, sy) in &spots {
        let (refdes, lo, ..) = &items[*i];
        let rot = board.parts.iter().find(|p| &p.refdes == refdes).map_or(0, |p| p.rot);
        // the pad box starts gap/2 inside its cell
        board.place_part(refdes, Point::new(snap(MARGIN + gap / 2.0 + sx - lo.x), snap(MARGIN + gap / 2.0 + label + sy - lo.y)), rot);
    }
    if turn {
        turn_to_shorten(board, &in_array);
        if improve && tuning.arrays {
            let groups: Vec<Vec<Vec<String>>> = copies.iter().map(|g| g.iter().map(|c| c.iter().map(|&i| items[i].0.clone()).collect()).collect()).collect();
            turn_copies_toward_ports(board, &groups);
        }
        crate::connectors::orient_connectors(board, (Point::new(MARGIN, MARGIN), Point::new(MARGIN + used_w, MARGIN + used_h)));
    }
    if tuning.align {
        align_parts(board, &in_array);
    }
    if tuning.bypass && improve {
        attach_bypass_caps(board, (Point::new(MARGIN, MARGIN), Point::new(MARGIN + used_w, MARGIN + used_h)), &in_array);
    }
    let (w, h) = (used_w + 2.0 * MARGIN, used_h + 2.0 * MARGIN);
    let old = board.bounds().filter(|_| board.outline.len() >= 3);
    board.set_rect_outline(0.0, 0.0, w.ceil(), h.ceil());
    if let Some((lo, hi)) = old {
        refit_pours(board, lo, hi);
    }
    let mut poured = poured;
    poured.sort();
    poured.dedup();
    let repoured = repour.iter().map(|(_, o)| groundpour::ground_pour(board, o).map_or(0, |r| r.vias)).sum();
    let crossings = crate::ratsnest::crossings(board);
    Ok(PlaceReport { placed: spots.len(), width: w.ceil(), height: h.ceil(), poured, repoured, crossings, group_notes })
}

pub(crate) fn airwire_length(board: &Board) -> f64 {
    if !board.tracks.is_empty() || !board.vias.is_empty() {
        return crate::ratsnest::ratsnest(board).iter().map(|w| w.a.dist(w.b)).sum();
    }
    // no copper yet: the airwires of a net are the shortest tree over its pads (a pour's nets have none). This is the ratsnest without
    // working out which pads the copper joins, which takes much longer on a board with pours
    let poured: std::collections::BTreeSet<&str> = board.zones.iter().map(|z| z.net.as_str()).filter(|n| !n.is_empty()).collect();
    let mut by_net: std::collections::BTreeMap<&str, Vec<Point>> = Default::default();
    for p in board.pads.iter().filter(|p| !p.net.is_empty() && !poured.contains(p.net.as_str())) {
        by_net.entry(p.net.as_str()).or_default().push(p.pos);
    }
    let mut total = 0.0;
    for pts in by_net.values().filter(|p| p.len() >= 2) {
        let mut near: Vec<f64> = pts.iter().map(|q| q.dist(pts[0])).collect();
        let mut inside = vec![false; pts.len()];
        inside[0] = true;
        for _ in 1..pts.len() {
            let Some((k, d)) = near.iter().enumerate().filter(|(i, _)| !inside[*i]).min_by(|a, b| a.1.total_cmp(b.1)).map(|(i, d)| (i, *d)) else { break };
            inside[k] = true;
            total += d;
            for (i, n) in near.iter_mut().enumerate() {
                if !inside[i] {
                    *n = n.min(pts[i].dist(pts[k]));
                }
            }
        }
    }
    total
}
