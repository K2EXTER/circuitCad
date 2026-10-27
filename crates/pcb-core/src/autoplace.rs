//! First-guess placement: pack the parts into rows, strongly connected parts next to each other,
//! and fit a rectangular outline around them. The order of the parts is first made greedily (the part most connected
//! to those already placed comes next), then improved by a short, repeatable annealing run that shortens the nets
//! (half-perimeter of each net's box). A starting point to refine by hand.

use crate::*;

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
}

impl Tuning {
    /// Without a pour every net needs wires (see `GAP`). With one, the poured nets drop out of the cost and the
    /// parts can sit closer.
    pub fn for_board(board: &Board) -> Self {
        if board.zones.iter().any(|z| !z.net.is_empty()) {
            Self { gap: POURED_GAP, label: POURED_LABEL, skip_poured: true, align: true, bypass: true, sheets: false, arrays: true, channels: false }
        } else {
            Self { gap: GAP, label: LABEL, skip_poured: false, align: true, bypass: true, sheets: false, arrays: true, channels: false }
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
            for ic in board.parts.iter().filter(|p| p.refdes.starts_with('U')) {
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

/// A repeatable stream of numbers (xorshift): the same board always gives the same placement.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Shelf-pack the parts in `order`: each part's cell corner, and the used width and height.
fn pack(items: &[(String, Point, f64, f64)], order: &[usize], row_w: f64) -> (Vec<(usize, f64, f64)>, f64, f64) {
    let (mut x, mut y, mut row_h, mut used_w) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let mut spots: Vec<(usize, f64, f64)> = Vec::with_capacity(order.len());
    for &i in order {
        let (_, _, w, h) = &items[i];
        if x > 0.0 && x + w > row_w {
            y += row_h;
            x = 0.0;
            row_h = 0.0;
        }
        spots.push((i, x, y));
        x += w;
        used_w = used_w.max(x);
        row_h = row_h.max(*h);
    }
    (spots, used_w, y + row_h)
}

/// A net as the parts' pads on it: (item, pad offset from the cell corner).
type NetPins = Vec<(usize, f64, f64)>;

/// The sum of the half-perimeters of the boxes around each net's pads for the packing of `order`, each divided by the
/// number of connections the net needs.
fn net_length(items: &[(String, Point, f64, f64)], nets: &[NetPins], order: &[usize], row_w: f64) -> f64 {
    let (spots, ..) = pack(items, order, row_w);
    spots_cost(nets, &spots, items.len())
}

/// [`net_length`] for given cell corners (`spots`: item, x, y).
fn spots_cost(nets: &[NetPins], spots: &[(usize, f64, f64)], n: usize) -> f64 {
    let mut at = vec![(0.0, 0.0); n];
    for &(i, x, y) in spots {
        at[i] = (x, y);
    }
    nets.iter().map(|pins| net_hpwl(pins, |i| Some(at[i]))).sum()
}

/// The half-perimeter of the box around a net's pads (the parts that `at` knows), divided by the number of
/// connections the net needs.
fn net_hpwl(pins: &NetPins, at: impl Fn(usize) -> Option<(f64, f64)>) -> f64 {
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    let mut any = false;
    for &(i, ox, oy) in pins {
        if let Some((x, y)) = at(i) {
            x0 = x0.min(x + ox);
            x1 = x1.max(x + ox);
            y0 = y0.min(y + oy);
            y1 = y1.max(y + oy);
            any = true;
        }
    }
    if !any {
        return 0.0;
    }
    ((x1 - x0) + (y1 - y0)) / (pins.len() as f64 - 1.0).max(1.0)
}

/// Free placement, part by part in `order` (the most connected part first, then always the one most tied to those
/// already placed): each part goes as near as it can to where its pins would sit on the pads they connect to, on the
/// nearest free spot of a 0.5 mm grid, picking from the first few free spots the one with the shortest nets. Parts
/// gather round the parts they belong to instead of lining up in rows. Returns the cell corners, moved to the origin.
fn attach_layout(items: &[(String, Point, f64, f64)], nets: &[NetPins], order: &[usize]) -> Vec<(usize, f64, f64)> {
    const STEP: f64 = 0.5;
    const WANT: usize = 12;
    // how much a part's net length may grow to stay closer to the middle of what is placed (mm per mm); measured on a
    // 73-part LED board: 0.1 left 1 connection open, 0.3 left 1, 1.0 left 3, 3.0 left 4
    const PULL: f64 = 0.1;
    let n = items.len();
    let mut mine: Vec<Vec<(usize, f64, f64)>> = vec![vec![]; n];
    for (k, pins) in nets.iter().enumerate() {
        for &(i, ox, oy) in pins {
            mine[i].push((k, ox, oy));
        }
    }
    let mut at: Vec<Option<(f64, f64)>> = vec![None; n];
    let mut placed: Vec<usize> = vec![];
    let free = |at: &[Option<(f64, f64)>], placed: &[usize], i: usize, x: f64, y: f64| {
        placed.iter().all(|&j| {
            let (px, py) = at[j].unwrap();
            !(x < px + items[j].2 - 1e-9 && px < x + items[i].2 - 1e-9 && y < py + items[j].3 - 1e-9 && py < y + items[i].3 - 1e-9)
        })
    };
    for &i in order {
        // where the part would like to be: its pads on the pads of its nets
        let (mut sx, mut sy, mut sw) = (0.0, 0.0, 0.0);
        for &(k, ox, oy) in &mine[i] {
            let w = 1.0 / (nets[k].len() as f64 - 1.0).max(1.0);
            for &(j, px, py) in &nets[k] {
                if let Some((x, y)) = at[j].filter(|_| j != i) {
                    sx += w * (x + px - ox);
                    sy += w * (y + py - oy);
                    sw += w;
                }
            }
        }
        let (bx0, bx1, by0, by1) = placed.iter().fold((f64::MAX, f64::MIN, f64::MAX, f64::MIN), |b, &j| {
            let (x, y) = at[j].unwrap();
            (b.0.min(x), b.1.max(x + items[j].2), b.2.min(y), b.3.max(y + items[j].3))
        });
        let ideal = if sw > 0.0 { (sx / sw, sy / sw) } else if placed.is_empty() { (0.0, 0.0) } else { (bx1, by0) };
        let centre = ((bx0 + bx1) / 2.0, (by0 + by1) / 2.0);
        let (cx, cy) = ((ideal.0 / STEP).round() * STEP, (ideal.1 / STEP).round() * STEP);
        // the nearest free spots, ring by ring
        let mut found: Vec<(f64, f64)> = vec![];
        let rmax = if placed.is_empty() { 0 } else { 400 };
        for r in 0..=rmax as i64 {
            let mut ring: Vec<(f64, f64)> = vec![];
            if r == 0 {
                ring.push((cx, cy));
            } else {
                for d in -r..=r {
                    ring.extend([(cx + d as f64 * STEP, cy - r as f64 * STEP), (cx + d as f64 * STEP, cy + r as f64 * STEP)]);
                    if d.abs() != r {
                        ring.extend([(cx - r as f64 * STEP, cy + d as f64 * STEP), (cx + r as f64 * STEP, cy + d as f64 * STEP)]);
                    }
                }
            }
            for (x, y) in ring {
                if free(&at, &placed, i, x, y) {
                    found.push((x, y));
                }
            }
            if found.len() >= WANT {
                break;
            }
        }
        if found.is_empty() {
            found.push((bx1.max(0.0), by0.min(0.0)));
        }
        let cost = |(x, y): (f64, f64)| -> f64 {
            let mut c = 0.0;
            for &(k, ..) in &mine[i] {
                let known = |j: usize| if j == i { Some((x, y)) } else { at[j] };
                c += net_hpwl(&nets[k], known);
            }
            // stay compact: a small pull to the middle of what is placed
            if placed.is_empty() { c } else { c + PULL * ((x + items[i].2 / 2.0 - centre.0).hypot(y + items[i].3 / 2.0 - centre.1)) }
        };
        let best = found.into_iter().min_by(|a, b| cost(*a).total_cmp(&cost(*b))).unwrap();
        at[i] = Some(best);
        placed.push(i);
    }
    let (x0, y0) = placed.iter().fold((f64::MAX, f64::MAX), |b, &j| (b.0.min(at[j].unwrap().0), b.1.min(at[j].unwrap().1)));
    order.iter().map(|&i| (i, at[i].unwrap().0 - x0, at[i].unwrap().1 - y0)).collect()
}

/// Improve the order by swapping and moving parts, keeping what shortens the nets (and, early on, some of what does
/// not, to get out of dead ends).
fn improve_order(items: &[(String, Point, f64, f64)], nets: &[NetPins], mut order: Vec<usize>, row_w: f64) -> Vec<usize> {
    let n = order.len();
    if n < 4 || nets.is_empty() {
        return order;
    }
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut cost = net_length(items, nets, &order, row_w);
    let (mut best, mut best_cost) = (order.clone(), cost);
    let steps = (600 * n).clamp(4000, 60_000);
    let (t0, t1) = (3.0f64, 0.03f64);
    for k in 0..steps {
        let temp = t0 * (t1 / t0).powf(k as f64 / steps as f64);
        let (i, j) = (rng.below(n), rng.below(n));
        if i == j {
            continue;
        }
        let mut next = order.clone();
        if rng.below(2) == 0 {
            next.swap(i, j);
        } else {
            let v = next.remove(i);
            next.insert(j, v);
        }
        let c = net_length(items, nets, &next, row_w);
        if c <= cost || rng.unit() < ((cost - c) / temp).exp() {
            order = next;
            cost = c;
            if cost < best_cost - 1e-9 {
                best = order.clone();
                best_cost = cost;
            }
        }
    }
    best
}

/// How closely two parts belong together: the nets they share count less the more pins the net has (a ground that
/// touches everything says little about who belongs together). Made once, for every pair.
fn ties(nets: &[NetPins], n: usize) -> Vec<Vec<f64>> {
    let mut tie = vec![vec![0.0f64; n]; n];
    for pins in nets {
        let w = 1.0 / (pins.len() as f64 - 1.0).max(1.0);
        let mut parts: Vec<usize> = pins.iter().map(|p| p.0).collect();
        parts.sort_unstable();
        parts.dedup();
        for (x, &a) in parts.iter().enumerate() {
            for &b in &parts[x + 1..] {
                tie[a][b] += w;
                tie[b][a] += w;
            }
        }
    }
    tie
}

/// Lay the `items` out: the order (begin with the part that has the most pads, then always the one most tied to those placed),
/// improved by swaps, shelf-packed into a roughly 3:2 block, or placed freely round the most connected ones, whichever has the
/// shorter nets (`compact`: only the shelf packing, which leaves a rectangular block; `fit`: the widths of the shelves and the orders are searched for the
/// least waste, for blocks of different shapes). Returns the cell corners and the used width and height.
fn layout(items: &[(String, Point, f64, f64)], nets: &[NetPins], degree: &[usize], tie: &[Vec<f64>], improve: bool, compact: bool, fit: bool) -> (Vec<(usize, f64, f64)>, f64, f64) {
    let mut order: Vec<usize> = vec![];
    let mut left: Vec<usize> = (0..items.len()).collect();
    left.sort_by(|&a, &b| degree[b].cmp(&degree[a]).then(items[a].0.cmp(&items[b].0)));
    let mut score = vec![0.0f64; items.len()];
    let mut best = 0;
    while !left.is_empty() {
        let i = left.remove(best);
        order.push(i);
        if left.is_empty() {
            break;
        }
        for &m in &left {
            score[m] += tie[m][i];
        }
        best = 0;
        for k in 1..left.len() {
            if score[left[k]] > score[left[best]] + 1e-9 {
                best = k;
            }
        }
    }
    // shelf packing into a roughly 3:2 board
    let area: f64 = items.iter().map(|i| i.2 * i.3).sum();
    let widest = items.iter().map(|i| i.2).fold(0.0, f64::max);
    let row_w = (area * 1.5).sqrt().max(widest);
    let greedy = order.clone();
    // a block of parts is packed into several widths and the one that wastes the least room (and has the shorter nets) is kept: the
    // blocks of a sheet, and of a copy, are of very different shapes, and a width of 3:2 for the sum of their areas often leaves holes
    let widths: Vec<f64> = if fit && improve && items.len() >= 3 {
        let mut w: Vec<f64> = [0.5, 0.6, 0.7, 0.85, 1.0, 1.2, 1.5].iter().map(|f| (row_w * f).max(widest)).collect();
        w.dedup_by(|a, b| (*a - *b).abs() < 0.5);
        w
    } else {
        vec![row_w]
    };
    let mut best_pack: Option<(f64, Vec<usize>, f64)> = None;
    for &rw in &widths {
        // a few blocks: every order is tried; more: the order improved by swaps
        let orders: Vec<Vec<usize>> = if widths.len() > 1 && items.len() <= EVERY_ORDER {
            let mut all = vec![];
            permutations(&mut (0..items.len()).collect::<Vec<_>>(), 0, &mut all);
            all
        } else if improve {
            vec![improve_order(items, nets, order.clone(), rw)]
        } else {
            vec![order.clone()]
        };
        for o in orders {
            let (_, uw, uh) = pack(items, &o, rw);
            // (a long thin strip has the least area for some blocks: the aspect counts against it)
            let aspect = (uw / uh).max(uh / uw);
            let score = if widths.len() > 1 { uw * uh * (1.0 + 0.5 * (aspect - 1.8).max(0.0)) + BLOCK_NET_WEIGHT * net_length(items, nets, &o, rw) } else { 0.0 };
            if best_pack.as_ref().is_none_or(|b| score < b.0 - 1e-9) {
                best_pack = Some((score, o, rw));
            }
        }
    }
    let (_, best_order, row_w) = best_pack.unwrap();
    order = best_order;
    let (mut spots, mut used_w, mut used_h) = pack(items, &order, row_w);
    // the other way to lay the parts out: free placement round the most connected ones; the one with the shorter nets wins
    if improve && !compact && items.len() >= 4 {
        let cand = attach_layout(items, nets, &greedy);
        if spots_cost(nets, &cand, items.len()) < spots_cost(nets, &spots, items.len()) {
            (used_w, used_h) = cand.iter().fold((0.0f64, 0.0f64), |b, &(i, x, y)| (b.0.max(x + items[i].2), b.1.max(y + items[i].3)));
            spots = cand;
        }
    }
    (spots, used_w, used_h)
}

/// How much a mm of net length counts against a mm² of block area when the widths a block is packed to are compared.
const BLOCK_NET_WEIGHT: f64 = 0.4;
/// A block of at most this many items is packed in every order (the blocks of a sheet, the parts of a small copy).
const EVERY_ORDER: usize = 7;

fn permutations(v: &mut Vec<usize>, k: usize, out: &mut Vec<Vec<usize>>) {
    if k == v.len() {
        out.push(v.clone());
        return;
    }
    for i in k..v.len() {
        v.swap(k, i);
        permutations(v, k + 1, out);
        v.swap(k, i);
    }
}

/// Room kept round a sheet's block (mm), so that the blocks can be told apart.
const BLOCK_GAP: f64 = 2.0;

/// The items of each schematic sheet (`sheet`: the sheet of every item), when there is more than one sheet; items of no sheet make a
/// group of their own. None for a one-sheet circuit.
fn sheet_groups(sheet: &[String]) -> Option<Vec<Vec<usize>>> {
    let mut by: std::collections::BTreeMap<&str, Vec<usize>> = Default::default();
    for (i, s) in sheet.iter().enumerate() {
        by.entry(s.as_str()).or_default().push(i);
    }
    (by.len() >= 2).then(|| by.into_values().collect())
}

/// A block for every sheet (laid out as a circuit of its own, with the nets it has inside), then the blocks laid out the same way
/// with the nets that run between sheets (one pin per block, at the middle of the sheet's pins on that net). The sheets are
/// kept together instead of mixing with the parts they only share a net with.
/// The squares (x, y, w, h) a block of parts covers, in the block's own coordinates.
type Cells = Vec<(f64, f64, f64, f64)>;

/// The raster of the nesting: the size of a pixel (mm).
const NEST_PIXEL: f64 = 1.0;
/// The places tried are this many pixels apart.
const NEST_STEP: usize = 3;
/// How many orders of the blocks are tried by `nest_blocks` besides the biggest first.
const NEST_ORDERS: usize = 12;

/// The blocks packed as the shapes they are, not as boxes: each is rasterised (its squares, grown by half the gap round them), and one after
/// another, in an order, each goes to the place that makes the whole box smallest (a long thin box counted against), among the places where it
/// does not cover what is there: a block can lie in the bay of another. Many orders are tried (the biggest first, then shuffled ones) and
/// the best whole is kept: the box and the nets between the blocks (the pins of a block at the places its parts have). Returns the corner of each
/// block, and the size of the whole.
fn nest_blocks(shapes: &[Cells], nets: &[NetPins]) -> (Vec<(usize, f64, f64)>, f64, f64) {
    let n = shapes.len();
    // the pixels each block covers, and its size (the block's parts stand half the gap in from its corner: the squares are grown by the gap)
    let masks: Vec<(Vec<(usize, usize)>, usize, usize)> = shapes
        .iter()
        .map(|cells| {
            let mut px: std::collections::BTreeSet<(usize, usize)> = Default::default();
            let (mut mw, mut mh) = (0usize, 0usize);
            for &(x, y, w, h) in cells {
                let (x0, y0, x1, y1) = (x, y, x + w + BLOCK_GAP, y + h + BLOCK_GAP);
                let (i0, i1) = ((x0 / NEST_PIXEL).floor().max(0.0) as usize, (x1 / NEST_PIXEL).ceil() as usize);
                let (j0, j1) = ((y0 / NEST_PIXEL).floor().max(0.0) as usize, (y1 / NEST_PIXEL).ceil() as usize);
                for j in j0..j1 {
                    for i in i0..i1 {
                        px.insert((i, j));
                    }
                }
                mw = mw.max(i1);
                mh = mh.max(j1);
            }
            (px.into_iter().collect(), mw, mh)
        })
        .collect();
    let area: usize = masks.iter().map(|m| m.1 * m.2).sum();
    let side = ((area as f64) * 3.0).sqrt().ceil() as usize + masks.iter().map(|m| m.1.max(m.2)).max().unwrap_or(0);
    let hpwl = |at: &[Option<(f64, f64)>]| -> f64 { nets.iter().map(|pins| net_hpwl(pins, |i| at[i])).sum() };
    let score_of = |w: usize, h: usize, at: &[Option<(f64, f64)>]| -> f64 {
        let (w, h) = (w as f64, h as f64);
        let aspect = (w / h).max(h / w);
        w * h * (1.0 + 0.5 * (aspect - 1.8).max(0.0)) + BLOCK_NET_WEIGHT * hpwl(at)
    };
    let pack = |order: &[usize]| -> (f64, Vec<(usize, usize)>, usize, usize) {
        let mut occupied = vec![false; side * side];
        let mut at: Vec<Option<(f64, f64)>> = vec![None; n];
        let mut put: Vec<(usize, usize)> = vec![(0, 0); n];
        let (mut bw, mut bh) = (0usize, 0usize);
        for &b in order {
            let (px, mw, mh) = (&masks[b].0, masks[b].1, masks[b].2);
            let mut best: Option<(f64, usize, usize)> = None;
            let mut y = 0;
            while y + mh <= side {
                let mut x = 0;
                while x + mw <= side {
                    // a place that would stretch the box much further than the best so far is not worth looking at
                    let (nw, nh) = (bw.max(x + mw), bh.max(y + mh));
                    if best.is_none_or(|bb| (nw * nh) as f64 <= bb.0 * 1.5 + 1.0) && px.iter().all(|&(i, j)| !occupied[(y + j) * side + x + i]) {
                        at[b] = Some((x as f64 * NEST_PIXEL, y as f64 * NEST_PIXEL));
                        let sc = score_of(nw, nh, &at);
                        if best.is_none_or(|bb| sc < bb.0) {
                            best = Some((sc, x, y));
                        }
                        at[b] = None;
                    }
                    x += NEST_STEP;
                }
                y += NEST_STEP;
            }
            let Some((_, x, y)) = best else { return (f64::MAX, put, side, side) };
            for &(i, j) in px {
                occupied[(y + j) * side + x + i] = true;
            }
            at[b] = Some((x as f64 * NEST_PIXEL, y as f64 * NEST_PIXEL));
            put[b] = (x, y);
            bw = bw.max(x + mw);
            bh = bh.max(y + mh);
        }
        (score_of(bw, bh, &at), put, bw, bh)
    };
    let mut first: Vec<usize> = (0..n).collect();
    first.sort_by_key(|&b| std::cmp::Reverse(masks[b].0.len()));
    let mut best = pack(&first);
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    for _ in 0..NEST_ORDERS.min(if n <= 3 { 6 } else { usize::MAX }) {
        let mut order = first.clone();
        for k in (1..n).rev() {
            order.swap(k, rng.below(k + 1));
        }
        let r = pack(&order);
        if r.0 < best.0 {
            best = r;
        }
    }
    let (_, put, bw, bh) = best;
    (put.into_iter().enumerate().map(|(b, (x, y))| (b, x as f64 * NEST_PIXEL, y as f64 * NEST_PIXEL)).collect(), bw as f64 * NEST_PIXEL, bh as f64 * NEST_PIXEL)
}

fn sheet_blocks(items: &[(String, Point, f64, f64)], nets: &[NetPins], degree: &[usize], groups: &[Vec<usize>], improve: bool) -> (Vec<(usize, f64, f64)>, f64, f64) {
    // each sheet on its own, in its own coordinates
    let mut local: Vec<(Vec<(usize, f64, f64)>, f64, f64)> = vec![];
    let mut group_of = vec![0usize; items.len()];
    for (k, members) in groups.iter().enumerate() {
        for &i in members {
            group_of[i] = k;
        }
    }
    for members in groups {
        let at = |i: usize| members.iter().position(|&m| m == i);
        let sub_items: Vec<_> = members.iter().map(|&i| items[i].clone()).collect();
        let sub_degree: Vec<usize> = members.iter().map(|&i| degree[i]).collect();
        let sub_nets: Vec<NetPins> = nets
            .iter()
            .map(|pins| pins.iter().filter_map(|&(i, x, y)| at(i).map(|l| (l, x, y))).collect::<NetPins>())
            .filter(|pins| pins.iter().any(|p| p.0 != pins[0].0))
            .collect();
        let tie = ties(&sub_nets, sub_items.len());
        // (a sheet grows round its biggest part, or is packed in rows, whichever has the shorter nets: the blocks are nested as the shapes they are)
        let (spots, w, h) = layout(&sub_items, &sub_nets, &sub_degree, &tie, improve, false, false);
        // back to the numbers of `items`
        local.push((spots.into_iter().map(|(l, x, y)| (members[l], x, y)).collect(), w, h));
    }
    // the blocks: one item per sheet
    let block_items: Vec<(String, Point, f64, f64)> = local.iter().enumerate().map(|(k, (_, w, h))| (format!("sheet{k}"), Point::new(0.0, 0.0), w + BLOCK_GAP, h + BLOCK_GAP)).collect();
    let mut block_degree = vec![0usize; groups.len()];
    for (k, members) in groups.iter().enumerate() {
        block_degree[k] = members.iter().map(|&i| degree[i]).sum();
    }
    let spot_of: std::collections::HashMap<usize, (f64, f64)> = local.iter().flat_map(|(s, ..)| s.iter().map(|&(i, x, y)| (i, (x, y)))).collect();
    let mut block_nets: Vec<NetPins> = vec![];
    for pins in nets {
        let mut per: std::collections::BTreeMap<usize, (f64, f64, f64)> = Default::default();
        for &(i, ox, oy) in pins {
            let (x, y) = spot_of[&i];
            let e = per.entry(group_of[i]).or_default();
            *e = (e.0 + x + ox, e.1 + y + oy, e.2 + 1.0);
        }
        if per.len() >= 2 {
            block_nets.push(per.into_iter().map(|(k, (sx, sy, n))| (k, sx / n + BLOCK_GAP / 2.0, sy / n + BLOCK_GAP / 2.0)).collect());
        }
    }
    let _ = (&block_items, &block_degree);
    // the shapes of the blocks: the squares of their parts' cells
    let shapes: Vec<Cells> = local.iter().map(|(spots, ..)| spots.iter().map(|&(i, x, y)| (x, y, items[i].2, items[i].3)).collect()).collect();
    // (the pins of a block for the nets between blocks are at the places the parts have, from the block's corner, plus the half gap)
    let (bspots, used_w, used_h) = if improve { nest_blocks(&shapes, &block_nets) } else {
        let tie = ties(&block_nets, groups.len());
        layout(&block_items, &block_nets, &block_degree, &tie, improve, true, true)
    };
    let mut out = vec![];
    for (k, bx, by) in bspots {
        for &(i, x, y) in &local[k].0 {
            out.push((i, bx + BLOCK_GAP / 2.0 + x, by + BLOCK_GAP / 2.0 + y));
        }
    }
    (out, used_w, used_h)
}

/// A grid of copies of one structure, as one item: where each member stands (item, offset from the array's corner), and the size.
struct Array {
    members: Vec<(usize, f64, f64)>,
    w: f64,
    h: f64,
}

/// For every group of copies (see `replicas`): lay the first copy out as a small circuit of its own, and put every copy in the same way,
/// one after another in the order of the designators' numbers, in rows of about the shape of a 3:2 board.
fn build_arrays(items: &[(String, Point, f64, f64)], nets: &[NetPins], degree: &[usize], groups: &[Vec<Vec<usize>>], improve: bool) -> Vec<Array> {
    let mut out = vec![];
    for copies in groups {
        let template = &copies[0];
        let at = |i: usize| template.iter().position(|&m| m == i);
        // every copy has the same slot for a role: as big as the biggest part that plays it (the values and footprints of copies may differ)
        let sub_items: Vec<_> = template
            .iter()
            .enumerate()
            .map(|(j, &i)| {
                let mut it = items[i].clone();
                it.2 = copies.iter().map(|c| items[c[j]].2).fold(0.0, f64::max);
                it.3 = copies.iter().map(|c| items[c[j]].3).fold(0.0, f64::max);
                it
            })
            .collect();
        let sub_degree: Vec<usize> = template.iter().map(|&i| degree[i]).collect();
        let sub_nets: Vec<NetPins> = nets
            .iter()
            .map(|pins| pins.iter().filter_map(|&(i, x, y)| at(i).map(|l| (l, x, y))).collect::<NetPins>())
            .filter(|pins| pins.iter().any(|p| p.0 != pins[0].0))
            .collect();
        let tie = ties(&sub_nets, sub_items.len());
        let (spots, w, h) = layout(&sub_items, &sub_nets, &sub_degree, &tie, improve, true, false);
        // the cell is a whole number of grid steps, so that every copy lands on the grid the same way
        let (w, h) = ((w / SNAP).ceil() * SNAP, (h / SNAP).ceil() * SNAP);
        let mut rel = vec![(0.0, 0.0); template.len()];
        for (l, x, y) in spots {
            rel[l] = (x, y);
        }
        let mut order: Vec<&Vec<usize>> = copies.iter().collect();
        order.sort_by_key(|c| crate::replicas::natural(&items[c[0]].0));
        let k = order.len();
        let cols = ((1.5 * k as f64 * h / w).sqrt().round() as usize).clamp(1, k);
        let rows = k.div_ceil(cols);
        let mut members = vec![];
        for (r, c) in order.iter().enumerate() {
            let (col, row) = ((r % cols) as f64, (r / cols) as f64);
            for (j, &i) in c.iter().enumerate() {
                members.push((i, rel[j].0 + col * w, rel[j].1 + row * h));
            }
        }
        out.push(Array { members, w: cols as f64 * w, h: rows as f64 * h });
    }
    out
}

/// Where an item of the collapsed problem comes from.
#[derive(Clone, Copy)]
enum Origin {
    Single(usize),
    Array(usize),
}

type Collapsed = (Vec<(String, Point, f64, f64)>, Vec<NetPins>, Vec<usize>, Vec<String>, Vec<Origin>);

/// The problem with every array as one item: the parts not in an array, then the arrays. The pins of the members sit where the array's
/// cell puts them; a net inside one array is no net any more.
fn collapse(items: &[(String, Point, f64, f64)], nets: &[NetPins], degree: &[usize], arrays: &[Array], sheet: &dyn Fn(usize) -> String) -> Collapsed {
    let mut member_of: std::collections::HashMap<usize, (usize, f64, f64)> = Default::default();
    for (k, a) in arrays.iter().enumerate() {
        for &(i, x, y) in &a.members {
            member_of.insert(i, (k, x, y));
        }
    }
    let (mut citems, mut cdegree, mut csheet, mut origin) = (vec![], vec![], vec![], vec![]);
    let mut single = vec![usize::MAX; items.len()];
    for (i, it) in items.iter().enumerate() {
        if !member_of.contains_key(&i) {
            single[i] = citems.len();
            citems.push(it.clone());
            cdegree.push(degree[i]);
            csheet.push(sheet(i));
            origin.push(Origin::Single(i));
        }
    }
    let mut sup = vec![0usize; arrays.len()];
    for (k, a) in arrays.iter().enumerate() {
        sup[k] = citems.len();
        citems.push((format!("array{k}"), Point::new(0.0, 0.0), a.w, a.h));
        cdegree.push(a.members.iter().map(|m| degree[m.0]).sum());
        csheet.push(sheet(a.members[0].0));
        origin.push(Origin::Array(k));
    }
    let cnets: Vec<NetPins> = nets
        .iter()
        .map(|pins| {
            pins.iter()
                .map(|&(i, ox, oy)| match member_of.get(&i) {
                    Some(&(k, ax, ay)) => (sup[k], ox + ax, oy + ay),
                    None => (single[i], ox, oy),
                })
                .collect::<NetPins>()
        })
        .filter(|pins| pins.iter().any(|p| p.0 != pins[0].0))
        .collect();
    (citems, cnets, cdegree, csheet, origin)
}

/// How good a way of standing is for the small circuit that a group of copies is made of, by wiring it: the parts (`members`, turned as `rots`) are
/// laid out alone on a board of their own, with only the nets between them, and routed. The cost is the wire and the vias that took, a big
/// penalty for a connection that did not go through, and a little for the room it needs.
fn template_cost(board: &Board, members: &[String], rots: &[u8], gap: f64, label: f64) -> f64 {
    let mut b = Board::new("copy", 200.0, 200.0);
    let wanted: Vec<&str> = members.iter().filter_map(|r| board.parts.iter().find(|p| &p.refdes == r)).map(|p| p.footprint.as_str()).collect();
    b.footprints = board.footprints.iter().filter(|f| wanted.contains(&f.name.as_str())).cloned().collect();
    b.rules = board.rules.clone();
    for (r, &rot) in members.iter().zip(rots) {
        let Some(part) = board.parts.iter().find(|p| &p.refdes == r) else { return f64::MAX };
        if b.set_part(Part { pos: Point::new(0.0, 0.0), rot, ..part.clone() }).is_err() {
            return f64::MAX;
        }
    }
    // the nets between the parts of the copy; the other nets of their pads do not matter here
    let mut count: std::collections::HashMap<&str, std::collections::HashSet<&str>> = Default::default();
    for p in board.pads.iter().filter(|p| members.contains(&p.refdes) && !p.net.is_empty()) {
        count.entry(p.net.as_str()).or_default().insert(p.refdes.as_str());
    }
    for pad in b.pads.iter_mut() {
        let net = board.pads.iter().find(|q| q.refdes == pad.refdes && q.pin == pad.pin).map_or("", |q| q.net.as_str());
        pad.net = if count.get(net).is_some_and(|parts| parts.len() >= 2) { net.to_string() } else { String::new() };
    }
    // laid out like the parts of the board are: the cells, the nets in the cells, the shelf packing
    let mut items: Vec<(String, Point, f64, f64)> = vec![];
    for r in members {
        let Some((lo, hi)) = extent(&b, r) else { return f64::MAX };
        items.push((r.clone(), lo, hi.x - lo.x + gap, hi.y - lo.y + gap + label));
    }
    let mut nets: std::collections::BTreeMap<&str, NetPins> = Default::default();
    let mut degree = vec![0usize; items.len()];
    for pad in b.pads.iter().filter(|p| !p.net.is_empty()) {
        if let Some(i) = items.iter().position(|it| it.0 == pad.refdes) {
            nets.entry(pad.net.as_str()).or_default().push((i, pad.pos.x - items[i].1.x + gap / 2.0, pad.pos.y - items[i].1.y + gap / 2.0 + label));
            degree[i] += 1;
        }
    }
    let nets: Vec<NetPins> = nets.into_values().filter(|pins| pins.iter().any(|p| p.0 != pins[0].0)).collect();
    let tie = ties(&nets, items.len());
    let (spots, w, h) = layout(&items, &nets, &degree, &tie, false, true, false);
    for (i, sx, sy) in &spots {
        let (refdes, lo, ..) = &items[*i];
        let rot = b.parts.iter().find(|p| &p.refdes == refdes).map_or(0, |p| p.rot);
        b.place_part(refdes, Point::new(snap(1.0 + gap / 2.0 + sx - lo.x), snap(1.0 + gap / 2.0 + label + sy - lo.y)), rot);
    }
    b.set_rect_outline(0.0, 0.0, (w + 2.0).ceil(), (h + 2.0).ceil());
    let report = crate::autoroute::autoroute(&mut b, &crate::autoroute::RouteOptions { outer_only: true, ..Default::default() }, None);
    let length: f64 = b.tracks.iter().map(|t| t.a.dist(t.b)).sum();
    length + 10.0 * b.vias.len() as f64 + 200.0 * report.failed.len() as f64 + 0.05 * w * h
}

/// Turn the parts of the copies the way that wires best (see `template_cost`): the first copy of each group is tried in the turns of its
/// parts one at a time (twice over), and the turns that win are given to every copy (each part by its role). Parts are not moved.
fn turn_copies(board: &mut Board, groups: &[Vec<Vec<usize>>], names: &[String], gap: f64, label: f64) {
    for copies in groups {
        let members: Vec<String> = copies[0].iter().map(|&i| names[i].clone()).collect();
        let mut rots: Vec<u8> = members.iter().map(|r| board.parts.iter().find(|p| &p.refdes == r).map_or(0, |p| p.rot)).collect();
        let mut best = template_cost(board, &members, &rots, gap, label);
        if best == f64::MAX {
            continue;
        }
        for _ in 0..2 {
            let mut changed = false;
            for j in 0..members.len() {
                let current = rots[j];
                for r in (0..4u8).filter(|r| *r != current) {
                    let mut trial = rots.clone();
                    trial[j] = r;
                    let c = template_cost(board, &members, &trial, gap, label);
                    if c < best - 0.5 {
                        best = c;
                        rots = trial;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        for copy in copies {
            for (j, &i) in copy.iter().enumerate() {
                if let Some(part) = board.parts.iter().find(|p| p.refdes == names[i]).cloned() {
                    board.place_part(&part.refdes, part.pos, rots[j]);
                }
            }
        }
    }
}

/// ICs that share nets want to sit close: a bus between two ICs (the outputs of one that go to the other) is many wires that all have to run
/// the distance between them. For every pair of ICs (8 pads or more) that have pads on one net, a two-pin net of one pad of each is added
/// twice over (the placement adds up the lengths of nets, so the pair counts more the more nets they share).
fn ic_pair_nets(board: &Board, items: &[(String, Point, f64, f64)], gap: f64, label: f64, poured: &[String]) -> Vec<NetPins> {
    let index = |r: &str| items.iter().position(|it| it.0 == r);
    let offset = |pad: &Pad, i: usize| (i, pad.pos.x - items[i].1.x + gap / 2.0, pad.pos.y - items[i].1.y + gap / 2.0 + label);
    let count = |r: &str| board.pads.iter().filter(|p| p.refdes == r).count();
    let ics: Vec<&Part> = board.parts.iter().filter(|p| count(&p.refdes) >= 8 && p.refdes.starts_with('U')).collect();
    let mut out: Vec<NetPins> = vec![];
    for (a, ia) in ics.iter().enumerate() {
        for ib in &ics[a + 1..] {
            let (Some(i), Some(j)) = (index(&ia.refdes), index(&ib.refdes)) else { continue };
            let nets_a: std::collections::BTreeSet<&str> = board.pads.iter().filter(|p| p.refdes == ia.refdes && !p.net.is_empty() && !poured.contains(&p.net)).map(|p| p.net.as_str()).collect();
            for net in board.pads.iter().filter(|p| p.refdes == ib.refdes && nets_a.contains(p.net.as_str())).map(|p| p.net.as_str()).collect::<std::collections::BTreeSet<_>>() {
                // a net on many ICs (a supply, a bus to all of them) says little: only nets of a few pads
                if board.pads.iter().filter(|p| p.net == net).count() > 8 {
                    continue;
                }
                let (Some(pa), Some(pb)) = (board.pads.iter().find(|p| p.refdes == ia.refdes && p.net == net), board.pads.iter().find(|p| p.refdes == ib.refdes && p.net == net)) else { continue };
                for _ in 0..2 {
                    out.push(vec![offset(pa, i), offset(pb, j)]);
                }
            }
        }
    }
    out
}

/// An IC with the bypass capacitors that belong to it, settled once and kept: how each capacitor is turned, and where its part origin stands from the IC's.
struct IcBlock {
    ic: String,
    caps: Vec<(String, u8, Point)>,
}

/// For every IC (8 pads or more) the bypass capacitors that serve it (a supply pad on a net of the IC, the other pad on another rail), each
/// put at the best free place round the IC's pin (as `attach_bypass_caps` does, on a board of the IC and its capacitors alone), turned so that
/// the supply pad faces the pin. The IC and its capacitors then move as one piece. A capacitor goes to the IC that has both of its nets (a
/// capacitor between VCC and VNEG, to the IC that runs between them), else to the IC with fewer capacitors; `skip` are parts that are in a copy.
fn ic_blocks(board: &Board, skip: &std::collections::BTreeSet<String>) -> Vec<IcBlock> {
    use crate::drc::{is_ground_net, is_negative_supply_net, is_supply_net};
    let rail = |n: &str| is_ground_net(n) || is_supply_net(n) || is_negative_supply_net(n);
    let supply = |n: &str| is_supply_net(n) || is_negative_supply_net(n);
    let count = |r: &str| board.pads.iter().filter(|p| p.refdes == r).count();
    let ics: Vec<&Part> = board.parts.iter().filter(|p| p.refdes.starts_with('U') && count(&p.refdes) >= 8 && !skip.contains(&p.refdes)).collect();
    let mut assigned: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for cap in board.parts.iter().filter(|p| p.refdes.starts_with('C') && !skip.contains(&p.refdes)) {
        let cp: Vec<&Pad> = board.pads.iter().filter(|p| p.refdes == cap.refdes).collect();
        let Some(sp) = cp.iter().find(|p| supply(&p.net) && cp.iter().any(|q| q.net != p.net && rail(&q.net))) else { continue };
        let other = cp.iter().find(|q| q.net != sp.net && rail(&q.net)).map(|q| q.net.as_str());
        let has = |ic: &Part, net: &str| board.pads.iter().any(|p| p.refdes == ic.refdes && p.net == net);
        let mut fit: Vec<&&Part> = ics.iter().filter(|ic| has(ic, &sp.net)).collect();
        if fit.iter().any(|ic| other.is_some_and(|o| has(ic, o))) {
            fit.retain(|ic| other.is_some_and(|o| has(ic, o)));
        }
        fit.sort_by_key(|ic| assigned.get(&ic.refdes).map_or(0, Vec::len));
        if let Some(ic) = fit.first() {
            assigned.entry(ic.refdes.clone()).or_default().push(cap.refdes.clone());
        }
    }
    // the small parts that hang on one IC and nothing else of size (the resistors round an op amp, a filter on one input): on the nets they
    // share with it, which are not hubs, they meet that one IC and no other IC or header
    let mut sat_net: std::collections::BTreeMap<String, String> = Default::default();
    let taken: std::collections::BTreeSet<String> = assigned.values().flatten().cloned().collect();
    for part in board.parts.iter().filter(|p| !skip.contains(&p.refdes) && !p.refdes.starts_with('U') && count(&p.refdes) <= SATELLITE_PADS && !taken.contains(&p.refdes)) {
        let mut touched: std::collections::BTreeMap<&str, &str> = Default::default();
        let mut other_anchor = false;
        for pad in board.pads.iter().filter(|p| p.refdes == part.refdes && !p.net.is_empty()) {
            let on: std::collections::BTreeSet<&str> = board.pads.iter().filter(|q| q.net == pad.net).map(|q| q.refdes.as_str()).collect();
            if on.len() > 6 {
                continue;
            }
            for r in on.iter().filter(|r| count(r) >= 8) {
                if r.starts_with('U') {
                    touched.insert(r, pad.net.as_str());
                } else {
                    other_anchor = true;
                }
            }
        }
        if touched.len() == 1 && !other_anchor {
            let (ic, net) = touched.into_iter().next().unwrap();
            if ics.iter().any(|i| i.refdes == ic) {
                sat_net.insert(part.refdes.clone(), net.to_string());
                assigned.entry(ic.to_string()).or_default().push(part.refdes.clone());
            }
        }
    }
    let mut out = vec![];
    for (ic_name, caps) in assigned {
        let Some(ic) = board.parts.iter().find(|p| p.refdes == ic_name) else { continue };
        let mut scratch = Board::new("block", 300.0, 300.0);
        let mut wanted: Vec<&str> = vec![ic.footprint.as_str()];
        wanted.extend(caps.iter().filter_map(|c| board.parts.iter().find(|p| &p.refdes == c)).map(|p| p.footprint.as_str()));
        scratch.footprints = board.footprints.iter().filter(|f| wanted.contains(&f.name.as_str())).cloned().collect();
        scratch.rules = board.rules.clone();
        let origin = Point::new(100.0, 100.0);
        if scratch.set_part(Part { pos: origin, ..ic.clone() }).is_err() {
            continue;
        }
        for c in &caps {
            if let Some(part) = board.parts.iter().find(|p| &p.refdes == c)
                && scratch.set_part(Part { pos: Point::new(0.0, 0.0), ..part.clone() }).is_err()
            {
                continue;
            }
        }
        for pad in scratch.pads.iter_mut() {
            pad.net = board.pads.iter().find(|q| q.refdes == pad.refdes && q.pin == pad.pin).map_or(String::new(), |q| q.net.clone());
        }
        let mut placed: Vec<IcBlock> = vec![];
        let mut block = IcBlock { ic: ic_name.clone(), caps: vec![] };
        for c in &caps {
            let cap_pads: Vec<Pad> = scratch.pads.iter().filter(|p| &p.refdes == c).cloned().collect();
            let sp = match sat_net.get(c) {
                Some(net) => cap_pads.iter().find(|p| &p.net == net).cloned(),
                None => cap_pads.iter().find(|p| supply(&p.net) && cap_pads.iter().any(|q| q.net != p.net && rail(&q.net))).cloned(),
            };
            let Some(sp) = sp else { continue };
            let other_net = if sat_net.contains_key(c) { None } else { cap_pads.iter().find(|q| q.net != sp.net && rail(&q.net)).map(|q| q.net.clone()) };
            let pins: Vec<Pad> = scratch.pads.iter().filter(|p| p.refdes == ic_name && p.net == sp.net).cloned().collect();
            let other_pins: Vec<Pad> = other_net.map_or(vec![], |n| scratch.pads.iter().filter(|p| p.refdes == ic_name && p.net == n).cloned().collect());
            // the boxes in the way: the IC and the capacitors already put
            let mut boxes: Vec<(Point, Point)> = extent(&scratch, &ic_name).into_iter().collect();
            boxes.extend(block.caps.iter().filter_map(|(r, ..)| extent(&scratch, r)));
            let cost_of = |pads: &[Pad]| -> f64 {
                let Some(s) = pads.iter().find(|q| q.pin == sp.pin) else { return f64::MAX };
                let o = pads.iter().find(|q| q.pin != sp.pin && q.net != sp.net);
                pins.iter()
                    .map(|pin| {
                        let near = other_pins.iter().map(|op| o.map_or(0.0, |o| o.pos.dist(op.pos)) + 0.0 * pin.pos.dist(op.pos)).fold(f64::MAX, f64::min);
                        s.pos.dist(pin.pos) + 0.5 * if near == f64::MAX { 0.0 } else { near }
                    })
                    .fold(f64::MAX, f64::min)
            };
            let mut best: Option<(f64, Point, u8)> = None;
            for pin in &pins {
                let steps = (BYPASS_REACH / BYPASS_STEP) as i32;
                for ix in -steps..=steps {
                    for iy in -steps..=steps {
                        let target = Point::new(pin.pos.x + ix as f64 * BYPASS_STEP, pin.pos.y + iy as f64 * BYPASS_STEP);
                        if target.dist(pin.pos) > BYPASS_REACH {
                            continue;
                        }
                        for rot in 0..4u8 {
                            scratch.place_part(c, Point::new(0.0, 0.0), rot);
                            let Some((lo, hi)) = extent(&scratch, c) else { continue };
                            let at = Point::new(snap(target.x - (lo.x + hi.x) / 2.0), snap(target.y - (lo.y + hi.y) / 2.0));
                            scratch.place_part(c, at, rot);
                            let Some((lo, hi)) = extent(&scratch, c) else { continue };
                            if boxes.iter().any(|(l, h)| (l.x - hi.x).max(lo.x - h.x).max((l.y - hi.y).max(lo.y - h.y)) < ALIGN_SPACE) {
                                continue;
                            }
                            let pads: Vec<Pad> = scratch.pads.iter().filter(|q| &q.refdes == c).cloned().collect();
                            let cst = cost_of(&pads);
                            if best.as_ref().is_none_or(|b| cst < b.0 - 1e-9) {
                                best = Some((cst, at, rot));
                            }
                        }
                    }
                }
            }
            if let Some((_, at, rot)) = best {
                scratch.place_part(c, at, rot);
                block.caps.push((c.clone(), rot, Point::new(at.x - origin.x, at.y - origin.y)));
            }
        }
        let _ = &mut placed;
        if !block.caps.is_empty() {
            out.push(block);
        }
    }
    out
}

fn place(board: &mut Board, turn: bool, improve: bool, tuning: &Tuning) -> Result<PlaceReport, String> {
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
            degree[i] += 1;
        }
    }
    let nets: Vec<NetPins> = nets.into_values().filter(|pins| pins.iter().any(|p| p.0 != pins[0].0)).collect();
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
    let (citems, cnets, cdegree, csheet, origin) = collapse(&items, &nets, &degree, &arrays, &sheet_name);
    let by_sheet = if tuning.sheets { sheet_groups(&csheet) } else { None };
    let (cspots, used_w, used_h) = match by_sheet {
        Some(groups) => sheet_blocks(&citems, &cnets, &cdegree, &groups, improve),
        None => {
            if citems.is_empty() {
                return Err("パッドを持つ部品がありません".into());
            }
            let tie = ties(&cnets, citems.len());
            layout(&citems, &cnets, &cdegree, &tie, improve, false, false)
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
    Ok(PlaceReport { placed: spots.len(), width: w.ceil(), height: h.ceil(), poured, repoured, crossings })
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

/// Try turning each part by half a turn (same footprint box, so nothing starts to overlap) and
/// keep the turn when the airwires get shorter. Two passes let neighbours settle.
/// Parts whose centres lie within this distance of each other on one axis are lined up on that axis.
const ALIGN_TOL: f64 = 2.0;
/// Least distance kept between the pad boxes of two parts when one is moved to line up (mm).
const ALIGN_SPACE: f64 = 0.9;

/// Line up the parts: those in the same row (centres within `ALIGN_TOL` in y) get one centre line, those in the same
/// column likewise. A part moves by whole `SNAP` steps and only when it keeps `ALIGN_SPACE` to every other part.
/// Everything is undone when the airwires get longer than 1 % or the DRC finds more than before.
fn align_parts(board: &mut Board, keep: &std::collections::BTreeSet<String>) {
    let before = board.clone();
    let (wire, drc) = (airwire_length(board), crate::drc::check(board).len());
    for axis in [1usize, 0] {
        align_axis(board, axis, keep);
    }
    if airwire_length(board) > wire * 1.01 + 0.5 || crate::drc::check(board).len() > drc {
        *board = before;
    }
}

fn align_axis(board: &mut Board, axis: usize, keep: &std::collections::BTreeSet<String>) {
    let coord = |p: Point| if axis == 0 { p.x } else { p.y };
    let mut centers: Vec<(String, f64)> = board.parts.iter().filter(|p| !keep.contains(&p.refdes)).filter_map(|p| extent(board, &p.refdes).map(|(lo, hi)| (p.refdes.clone(), coord(Point::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0))))).collect();
    centers.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    let mut i = 0;
    while i < centers.len() {
        let mut j = i + 1;
        while j < centers.len() && centers[j].1 - centers[i].1 <= ALIGN_TOL {
            j += 1;
        }
        // the member in the middle keeps its place; the others come to its line
        if j - i >= 2 {
            let target = centers[(i + j - 1) / 2].1;
            for (refdes, c) in &centers[i..j] {
                let step = snap(target - c);
                if step.abs() < 1e-9 || (target - c - step).abs() > 0.3 {
                    continue;
                }
                let Some(part) = board.parts.iter().find(|p| &p.refdes == refdes).cloned() else { continue };
                let moved = if axis == 0 { Point::new(part.pos.x + step, part.pos.y) } else { Point::new(part.pos.x, part.pos.y + step) };
                board.place_part(refdes, moved, part.rot);
                if crowded(board, refdes) {
                    board.place_part(refdes, part.pos, part.rot);
                }
            }
        }
        i = j;
    }
}

/// `refdes` is closer than `ALIGN_SPACE` to another part (pad box to pad box).
pub(crate) fn crowded(board: &Board, refdes: &str) -> bool {
    let Some((lo, hi)) = extent(board, refdes) else { return false };
    board.parts.iter().filter(|p| p.refdes != refdes).filter_map(|p| extent(board, &p.refdes)).any(|(l, h)| {
        let gap_x = (l.x - hi.x).max(lo.x - h.x);
        let gap_y = (l.y - hi.y).max(lo.y - h.y);
        gap_x.max(gap_y) < ALIGN_SPACE
    })
}

/// Turn a part about the middle of its pads (the pads' box keeps its place, to a grid step).
fn turn_about_centre(board: &mut Board, refdes: &str, turns: u8) {
    let center = |board: &Board, r: &str| extent(board, r).map(|(lo, hi)| Point::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0));
    let (Some(part), Some(c0)) = (board.parts.iter().find(|p| p.refdes == refdes).cloned(), center(board, refdes)) else { return };
    board.place_part(refdes, part.pos, part.rot + turns);
    if let Some(c1) = center(board, refdes) {
        board.place_part(refdes, Point::new(snap(part.pos.x + c0.x - c1.x), snap(part.pos.y + c0.y - c1.y)), part.rot + turns);
    }
}

/// The copies of a structure stand in their grid; what each copy is joined to outside it (the ports) lies where the rest of the board put
/// it. Here the role of every part is turned, in all the copies at once, to the way that makes the airwires of the whole board shortest:
/// a resistor whose pin goes to the IC turns that pin to the IC. A turn is dropped when a part of a copy would stand too close to another.
fn turn_copies_toward_ports(board: &mut Board, groups: &[Vec<Vec<String>>]) {
    for copies in groups {
        for role in 0..copies[0].len() {
            let before = airwire_length(board);
            let mut best: Option<(f64, Board)> = None;
            for turns in 1..4u8 {
                let mut trial = board.clone();
                for copy in copies {
                    turn_about_centre(&mut trial, &copy[role], turns);
                }
                if copies.iter().any(|c| crowded(&trial, &c[role])) {
                    continue;
                }
                let len = airwire_length(&trial);
                if len < before - 0.5 && best.as_ref().is_none_or(|b| len < b.0) {
                    best = Some((len, trial));
                }
            }
            if let Some((_, b)) = best {
                *board = b;
            }
        }
    }
}

/// A bypass capacitor farther than this (mm) from the pin it serves is moved to the pin.
const BYPASS_NEAR: f64 = 2.5;
/// A part with this many pads or fewer can be a satellite of an IC (see `ic_blocks`).
const SATELLITE_PADS: usize = 3;
/// How far (mm) from the pin a place is looked for, and the distance between the places tried.
const BYPASS_REACH: f64 = 6.0;
const BYPASS_STEP: f64 = 1.0;

/// The bypass capacitors that the placement left far from their IC are put next to the supply pin, with the pad on that supply towards the
/// pin (and the other pad towards the IC's pin on the other rail, when it has one): each one goes to the best free place on the grid within
/// `BYPASS_REACH` of the pin, on the board (`inside`), not too close to another part. Parts in `keep` (copies in a grid) are not moved.
fn attach_bypass_caps(board: &mut Board, inside: (Point, Point), keep: &std::collections::BTreeSet<String>) {
    use crate::drc::{is_ground_net, is_negative_supply_net, is_supply_net};
    let rail = |n: &str| is_ground_net(n) || is_supply_net(n) || is_negative_supply_net(n);
    let supply = |n: &str| is_supply_net(n) || is_negative_supply_net(n);
    let caps: Vec<String> = board.parts.iter().filter(|p| p.refdes.starts_with('C') && !keep.contains(&p.refdes)).map(|p| p.refdes.clone()).collect();
    for cap in caps {
        let cap_pads: Vec<Pad> = board.pads.iter().filter(|p| p.refdes == cap).cloned().collect();
        // the supply pad of the capacitor and the pin of an IC that it serves: the nearest pair
        let mut best: Option<(f64, Pad, Pad)> = None;
        for cp in cap_pads.iter().filter(|p| supply(&p.net) && cap_pads.iter().any(|q| q.net != p.net && rail(&q.net))) {
            for ic in board.parts.iter().filter(|p| p.refdes.starts_with('U')) {
                let ic_pads: Vec<&Pad> = board.pads.iter().filter(|p| p.refdes == ic.refdes).collect();
                if ic_pads.len() < 5 {
                    continue;
                }
                for ip in ic_pads.iter().filter(|p| p.net == cp.net) {
                    let d = cp.pos.dist(ip.pos);
                    if best.as_ref().is_none_or(|b| d < b.0) {
                        best = Some((d, cp.clone(), (*ip).clone()));
                    }
                }
            }
        }
        let Some((now, cp, pin)) = best else { continue };
        if now <= BYPASS_NEAR {
            continue;
        }
        // the IC's pin on the other net of the capacitor (a negative rail: its 0 V pin), if there is one: the other pad faces it
        let other_net = cap_pads.iter().find(|q| q.net != cp.net && rail(&q.net)).map(|q| q.net.clone());
        let other_pin: Option<Pad> = other_net.and_then(|n| board.pads.iter().filter(|p| p.refdes == pin.refdes && p.net == n).min_by(|a, b| a.pos.dist(pin.pos).total_cmp(&b.pos.dist(pin.pos))).cloned());
        let Some(part) = board.parts.iter().find(|p| p.refdes == cap).cloned() else { continue };
        let boxes: Vec<(Point, Point)> = board.parts.iter().filter(|p| p.refdes != cap).filter_map(|p| extent(board, &p.refdes)).collect();
        let cost_of = |pads: &[Pad]| -> f64 {
            let a = pads.iter().find(|q| q.pin == cp.pin).map_or(f64::MAX, |q| q.pos.dist(pin.pos));
            let b = other_pin.as_ref().map_or(0.0, |op| pads.iter().find(|q| q.pin != cp.pin && q.net != cp.net).map_or(0.0, |q| q.pos.dist(op.pos)) * 0.5);
            a + b
        };
        let mut scratch = board.clone();
        let mut best_at: Option<(f64, Point, u8)> = None;
        // (places 1 mm apart: finer ones cost much and gain little)
        let steps = (BYPASS_REACH / BYPASS_STEP) as i32;
        for ix in -steps..=steps {
            for iy in -steps..=steps {
                let target = Point::new(pin.pos.x + ix as f64 * BYPASS_STEP, pin.pos.y + iy as f64 * BYPASS_STEP);
                if target.dist(pin.pos) > BYPASS_REACH || target.x < inside.0.x || target.y < inside.0.y || target.x > inside.1.x || target.y > inside.1.y {
                    continue;
                }
                for rot in 0..4u8 {
                    // the capacitor turned, its pads' box centred on the target
                    scratch.place_part(&cap, Point::new(0.0, 0.0), rot);
                    let Some((lo, hi)) = extent(&scratch, &cap) else { continue };
                    let at = Point::new(snap(target.x - (lo.x + hi.x) / 2.0), snap(target.y - (lo.y + hi.y) / 2.0));
                    scratch.place_part(&cap, at, rot);
                    let Some((lo, hi)) = extent(&scratch, &cap) else { continue };
                    if lo.x < inside.0.x || lo.y < inside.0.y || hi.x > inside.1.x || hi.y > inside.1.y {
                        continue;
                    }
                    if boxes.iter().any(|(l, h)| (l.x - hi.x).max(lo.x - h.x).max((l.y - hi.y).max(lo.y - h.y)) < ALIGN_SPACE) {
                        continue;
                    }
                    let pads: Vec<Pad> = scratch.pads.iter().filter(|q| q.refdes == cap).cloned().collect();
                    let c = cost_of(&pads);
                    if best_at.as_ref().is_none_or(|b| c < b.0 - 1e-9) {
                        best_at = Some((c, at, rot));
                    }
                }
            }
        }
        if let Some((c, at, rot)) = best_at
            && c < cost_of(&cap_pads) - 1.0
        {
            board.place_part(&cap, at, rot);
        } else {
            let _ = part;
        }
    }
}

fn turn_to_shorten(board: &mut Board, keep: &std::collections::BTreeSet<String>) {
    let center = |board: &Board, r: &str| extent(board, r).map(|(lo, hi)| Point::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0));
    for _ in 0..2 {
        let mut better = false;
        for part in board.parts.clone().into_iter().filter(|p| !keep.contains(&p.refdes)) {
            let before = airwire_length(board);
            let Some(c0) = center(board, &part.refdes) else { continue };
            board.place_part(&part.refdes, part.pos, part.rot + 2);
            if let Some(c1) = center(board, &part.refdes) {
                board.place_part(&part.refdes, Point::new(snap(part.pos.x + c0.x - c1.x), snap(part.pos.y + c0.y - c1.y)), part.rot + 2);
            }
            if airwire_length(board) < before - 1e-6 {
                better = true;
            } else {
                board.place_part(&part.refdes, part.pos, part.rot);
            }
        }
        if !better {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::netlist::*;

    fn divider() -> Board {
        let part = |r: &str| NetlistPart { refdes: r.into(), value: "1k".into(), footprint: "R_0805".into() };
        let net = |n: &str, pins: &[(&str, &str)]| NetlistNet { name: n.into(), pins: pins.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect() };
        let mut b = Board::new("t", 10.0, 10.0);
        import(
            &mut b,
            &(1..=6).map(|i| part(&format!("R{i}"))).collect::<Vec<_>>(),
            &[net("A", &[("R1", "1"), ("R2", "1")]), net("B", &[("R2", "2"), ("R3", "1")]), net("C", &[("R3", "2"), ("R4", "1"), ("R5", "1")]), net("GND", &[("R4", "2"), ("R5", "2"), ("R6", "1"), ("R6", "2"), ("R1", "2")])],
        );
        b
    }

    /// Two sheets of eight resistors each, a chain on each sheet; one net runs between the sheets.
    fn two_sheets() -> Board {
        let part = |r: &str| NetlistPart { refdes: r.into(), value: "1k".into(), footprint: "R_0805".into() };
        let net = |n: &str, pins: &[(&str, &str)]| NetlistNet { name: n.into(), pins: pins.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect() };
        let mut b = Board::new("t", 10.0, 10.0);
        let mut nets = vec![];
        for (sheet, base) in [("電源", 0), ("制御", 8)] {
            for k in 1..8 {
                nets.push(net(&format!("{sheet}{k}"), &[(&format!("R{}", base + k), "2"), (&format!("R{}", base + k + 1), "1")]));
            }
            for k in 1..=8 {
                b.part_sheets.insert(format!("R{}", base + k), sheet.to_string());
            }
        }
        // every part of a sheet has a partner on the other sheet (shared net on pin 1 and pin 2): without the sheets the parts mix
        for k in 1..=8 {
            nets.push(net(&format!("PAIR{k}"), &[(&format!("R{k}"), "1"), (&format!("R{}", 8 + k), "2")]));
        }
        import(&mut b, &(1..=16).map(|i| part(&format!("R{i}"))).collect::<Vec<_>>(), &nets);
        for (r, s) in b.part_sheets.clone() {
            b.part_sheets.insert(r, s);
        }
        b
    }

    fn sheet_box(b: &Board, sheet: &str) -> (Point, Point) {
        let (mut lo, mut hi) = (Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN));
        for (r, _) in b.part_sheets.iter().filter(|(_, s)| s.as_str() == sheet) {
            let (a, c) = extent(b, r).unwrap();
            lo = Point::new(lo.x.min(a.x), lo.y.min(a.y));
            hi = Point::new(hi.x.max(c.x), hi.y.max(c.y));
        }
        (lo, hi)
    }

    /// Eight copies of "a resistor and a capacitor in series", with a ground that touches all of them and an input and an output each.
    fn eight_stages() -> Board {
        let part = |r: &str, f: &str| NetlistPart { refdes: r.into(), value: "1k".into(), footprint: f.into() };
        let net = |n: &str, pins: &[(&str, &str)]| NetlistNet { name: n.into(), pins: pins.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect() };
        let mut b = Board::new("t", 10.0, 10.0);
        let (mut parts, mut nets, mut gnd) = (vec![], vec![], vec![]);
        for k in 1..=8 {
            parts.push(part(&format!("R{k}"), "R_0805"));
            parts.push(part(&format!("C{k}"), "C_0805"));
            nets.push(net(&format!("MID{k}"), &[(&format!("R{k}"), "2"), (&format!("C{k}"), "1")]));
            gnd.push((format!("C{k}"), "2".to_string()));
        }
        nets.push(NetlistNet { name: "GND".into(), pins: gnd });
        import(&mut b, &parts, &nets);
        b
    }

    #[test]
    fn copies_of_a_structure_are_laid_out_alike_in_a_regular_grid() {
        let mut b = eight_stages();
        autoplace(&mut b).unwrap();
        let pos = |r: &str| b.parts.iter().find(|p| p.refdes == r).unwrap().pos;
        // the capacitor of each stage stands the same way from its resistor
        let off = |k: usize| (pos(&format!("C{k}")).x - pos(&format!("R{k}")).x, pos(&format!("C{k}")).y - pos(&format!("R{k}")).y);
        for k in 2..=8 {
            assert!((off(k).0 - off(1).0).abs() < 1e-6 && (off(k).1 - off(1).1).abs() < 1e-6, "stage {k}: {:?} against {:?}", off(k), off(1));
        }
        // the resistors stand on a grid: few different columns and rows, even steps
        let mut xs: Vec<f64> = (1..=8).map(|k| pos(&format!("R{k}")).x).collect();
        let mut ys: Vec<f64> = (1..=8).map(|k| pos(&format!("R{k}")).y).collect();
        xs.sort_by(f64::total_cmp);
        xs.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        ys.sort_by(f64::total_cmp);
        ys.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        assert!(xs.len() * ys.len() >= 8 && xs.len() <= 5 && ys.len() <= 3, "{xs:?} {ys:?}");
        let even = |v: &[f64]| v.windows(2).all(|w| ((w[1] - w[0]) - (v[1] - v[0])).abs() < 1e-6);
        assert!(even(&xs) && even(&ys), "{xs:?} {ys:?}");
        // they come in the order of their numbers, row by row
        let order: Vec<(f64, f64)> = (1..=8).map(|k| (pos(&format!("R{k}")).y, pos(&format!("R{k}")).x)).collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
        // without the grid the stages are not alike
        let mut plain = eight_stages();
        let t = Tuning::for_board(&plain);
        autoplace_with(&mut plain, &Tuning { arrays: false, ..t }).unwrap();
        let pp = |r: &str| plain.parts.iter().find(|p| p.refdes == r).unwrap().pos;
        let alike = (2..=8).all(|k| ((pp(&format!("C{k}")).x - pp(&format!("R{k}")).x) - (pp("C1").x - pp("R1").x)).abs() < 1e-6 && ((pp(&format!("C{k}")).y - pp(&format!("R{k}")).y) - (pp("C1").y - pp("R1").y)).abs() < 1e-6);
        assert!(!alike || plain.parts.len() < 16, "the test means something only if the plain layout is not already a grid");
    }

    #[test]
    fn a_bypass_capacitor_left_far_from_its_ic_is_moved_to_the_supply_pin() {
        let mut b = ics_with_caps();
        b.set_rect_outline(0.0, 0.0, 100.0, 60.0);
        // U1 at the middle, its capacitor C1 far away
        b.place_part("U1", Point::new(30.0, 20.0), 0);
        b.place_part("U2", Point::new(60.0, 20.0), 0);
        b.place_part("C1", Point::new(5.0, 5.0), 0);
        b.place_part("C2", Point::new(80.0, 5.0), 0);
        for k in 0..6 {
            b.place_part(&format!("R{k}"), Point::new(30.0 + 5.0 * k as f64, 45.0), 0);
        }
        let near = |b: &Board, cap: &str, ic: &str| gap_to_pin(b, cap, ic, "VCC");
        assert!(near(&b, "C1", "U1") > 20.0 && near(&b, "C2", "U2") > 15.0);
        attach_bypass_caps(&mut b, (Point::new(0.0, 0.0), Point::new(100.0, 60.0)), &Default::default());
        assert!(near(&b, "C1", "U1") <= BYPASS_NEAR + 1.5, "{}", near(&b, "C1", "U1"));
        assert!(near(&b, "C2", "U2") <= BYPASS_NEAR + 1.5, "{}", near(&b, "C2", "U2"));
        // not on top of anything
        assert!(!crowded(&b, "C1") && !crowded(&b, "C2"));
        assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
        // a capacitor already near its pin stays
        let again = b.clone();
        attach_bypass_caps(&mut b, (Point::new(0.0, 0.0), Point::new(100.0, 60.0)), &Default::default());
        assert!(b.parts.iter().zip(&again.parts).all(|(a, c)| a.pos == c.pos && a.rot == c.rot));
    }

    #[test]
    fn blocks_are_nested_as_the_shapes_they_are() {
        // two L-shaped blocks (an arm along the bottom, an arm up the side) and a small square that fits in the bay of an L
        let l = |len: f64, arm: f64| -> Cells { vec![(0.0, 0.0, len, arm), (0.0, 0.0, arm, len)] };
        let shapes = vec![l(24.0, 4.0), l(24.0, 4.0), vec![(0.0, 0.0, 6.0, 6.0)]];
        let (put, w, h) = nest_blocks(&shapes, &[]);
        assert_eq!(put.len(), 3);
        let boxes: f64 = shapes.iter().map(|c| c.iter().map(|q| q.0 + q.2).fold(0.0, f64::max) * c.iter().map(|q| q.1 + q.3).fold(0.0, f64::max)).sum();
        // the box of the whole is smaller than the boxes of the blocks added up (they lie in each other's bays)
        assert!(w * h < boxes * 0.9, "{w} x {h} against {boxes}");
        // and nothing covers anything: the squares of two blocks do not overlap
        for (a, pa) in put.iter().enumerate() {
            for (b, pb) in put.iter().enumerate().skip(a + 1) {
                for ca in &shapes[a] {
                    for cb in &shapes[b] {
                        let (ax, ay) = (pa.1 + ca.0, pa.2 + ca.1);
                        let (bx, by) = (pb.1 + cb.0, pb.2 + cb.1);
                        assert!(!(ax < bx + cb.2 && bx < ax + ca.2 && ay < by + cb.3 && by < ay + ca.3), "blocks {a} and {b} overlap");
                    }
                }
            }
        }
    }

    #[test]
    fn the_role_of_a_part_in_every_copy_is_turned_to_face_what_it_is_joined_to() {
        // a header on the left with a pin for each stage; three stages (R, C) to the right: each R's pin 1 is joined to its header pin,
        // and the resistors stand turned the wrong way (their pin 1 faces away from the header)
        let part = |r: &str, f: &str| NetlistPart { refdes: r.into(), value: "1k".into(), footprint: f.into() };
        let net = |n: &str, pins: &[(&str, &str)]| NetlistNet { name: n.into(), pins: pins.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect() };
        let mut b = Board::new("t", 60.0, 40.0);
        b.add_footprint(package::PackageSpec::Dual { pins: 6, pitch: 2.54, row_spacing: 5.08, pad_len: 1.6, pad_width: 1.6, drill: Some(0.8) }.build("DIP-6").unwrap());
        let mut parts = vec![part("J1", "DIP-6")];
        let mut nets = vec![];
        for k in 1..=3 {
            parts.push(part(&format!("R{k}"), "R_0805"));
            parts.push(part(&format!("C{k}"), "C_0805"));
            nets.push(net(&format!("PORT{k}"), &[("J1", &k.to_string()), (&format!("R{k}"), "1")]));
            nets.push(net(&format!("MID{k}"), &[(&format!("R{k}"), "2"), (&format!("C{k}"), "1")]));
        }
        import(&mut b, &parts, &nets);
        b.place_part("J1", Point::new(5.0, 20.0), 0);
        for k in 1..=3 {
            b.place_part(&format!("R{k}"), Point::new(30.0, 8.0 + 4.0 * k as f64), 2);
            b.place_part(&format!("C{k}"), Point::new(40.0, 8.0 + 4.0 * k as f64), 0);
        }
        let before = airwire_length(&b);
        let groups: Vec<Vec<Vec<String>>> = vec![(1..=3).map(|k| vec![format!("R{k}"), format!("C{k}")]).collect()];
        turn_copies_toward_ports(&mut b, &groups);
        assert!(airwire_length(&b) < before - 1.0, "{before} -> {}", airwire_length(&b));
        let rots: Vec<u8> = (1..=3).map(|k| b.parts.iter().find(|p| p.refdes == format!("R{k}")).unwrap().rot).collect();
        assert!(rots.iter().all(|r| *r == rots[0]) && rots[0] != 2, "every copy turned alike: {rots:?}");
    }

    #[test]
    fn the_parts_of_a_sheet_stay_together_in_a_block_of_their_own() {
        let mut b = two_sheets();
        let t = Tuning::for_board(&b);
        autoplace_with(&mut b, &Tuning { sheets: true, ..t }).unwrap();
        let (a, c) = (sheet_box(&b, "電源"), sheet_box(&b, "制御"));
        let apart = a.1.x <= c.0.x || c.1.x <= a.0.x || a.1.y <= c.0.y || c.1.y <= a.0.y;
        assert!(apart, "the two sheets' boxes overlap: {a:?} {c:?}");
        // (and without the sheets the same circuit does mix: the test means something)
        let mut mixed = two_sheets();
        let t = Tuning::for_board(&mixed);
        autoplace_with(&mut mixed, &Tuning { sheets: false, ..t }).unwrap();
        let (a, c) = (sheet_box(&mixed, "電源"), sheet_box(&mixed, "制御"));
        assert!(!(a.1.x <= c.0.x || c.1.x <= a.0.x || a.1.y <= c.0.y || c.1.y <= a.0.y), "{a:?} {c:?}");
        // a one-sheet circuit is laid out as it always was
        let mut one = two_sheets();
        one.part_sheets.clear();
        let mut plain = one.clone();
        let t = Tuning::for_board(&one);
        autoplace_with(&mut one, &Tuning { sheets: true, ..t }).unwrap();
        autoplace_with(&mut plain, &Tuning { sheets: false, ..t }).unwrap();
        assert_eq!(one.parts.iter().map(|p| (p.pos, p.rot)).collect::<Vec<_>>(), plain.parts.iter().map(|p| (p.pos, p.rot)).collect::<Vec<_>>());
    }

    #[test]
    fn parts_land_inside_a_fitted_outline_without_rule_violations() {
        let mut b = divider();
        let rep = autoplace(&mut b).unwrap();
        assert_eq!(rep.placed, 6);
        let (lo, hi) = b.bounds().unwrap();
        assert!(b.pads.iter().all(|p| p.pos.x > lo.x && p.pos.x < hi.x && p.pos.y > lo.y && p.pos.y < hi.y));
        let v = drc::check(&b);
        assert!(v.is_empty(), "{v:?}");
    }

    #[test]
    fn the_placement_can_be_routed_completely() {
        let mut b = divider();
        autoplace(&mut b).unwrap();
        let r = autoroute::autoroute(&mut b, &autoroute::RouteOptions::default(), None);
        assert!(r.failed.is_empty(), "{:?}", r.failed);
        assert!(drc::check(&b).is_empty());
    }

    #[test]
    fn connected_parts_sit_closer_than_unrelated_ones() {
        let mut b = divider();
        autoplace(&mut b).unwrap();
        let at = |r: &str| b.parts.iter().find(|p| p.refdes == r).unwrap().pos;
        // R4 and R5 share the net C
        let near = at("R4").dist(at("R5"));
        let far = at("R1").dist(at("R6")).max(at("R2").dist(at("R6")));
        assert!(near <= far, "{near} {far}");
    }

    #[test]
    fn turning_parts_shortens_the_airwires_and_keeps_the_board_clean() {
        let (mut plain, mut turned) = (divider(), divider());
        place(&mut plain, false, true, &Tuning { gap: GAP, label: LABEL, skip_poured: false, align: false, bypass: true, sheets: true, arrays: true, channels: true }).unwrap();
        place(&mut turned, true, true, &Tuning { gap: GAP, label: LABEL, skip_poured: false, align: false, bypass: true, sheets: true, arrays: true, channels: true }).unwrap();
        let (l0, l1) = (airwire_length(&plain), airwire_length(&turned));
        assert!(l1 < l0, "{l1} vs {l0}");
        assert!(drc::check(&turned).is_empty());
        assert!(turned.parts.iter().any(|p| p.rot == 2));
    }

    /// A header whose pins each drive a resistor and an LED in series (the LED to ground): the greedy order puts all the
    /// resistors, then all the LEDs, in rows.
    fn fan(pairs: usize) -> Board {
        let part = |r: String, f: &str| NetlistPart { refdes: r, value: "1k".into(), footprint: f.into() };
        let mut b = Board::new("t", 10.0, 10.0);
        let mut parts = vec![part("J1".into(), "PinHeader_1x8")];
        let mut nets: Vec<NetlistNet> = vec![];
        let mut gnd = vec![];
        for k in 0..pairs {
            let (r, d) = (format!("R{k:02}"), format!("D{k:02}"));
            parts.push(part(r.clone(), "R_0805"));
            parts.push(part(d.clone(), "LED_0805"));
            nets.push(NetlistNet { name: format!("Q{k}"), pins: vec![("J1".into(), (k + 1).to_string()), (r.clone(), "1".into())] });
            nets.push(NetlistNet { name: format!("N{k}"), pins: vec![(r, "2".into()), (d.clone(), "A".into())] });
            gnd.push((d, "K".to_string()));
        }
        nets.push(NetlistNet { name: "GND".into(), pins: gnd });
        import(&mut b, &parts, &nets);
        b
    }

    #[test]
    fn improving_the_order_puts_the_parts_of_a_net_together() {
        let (mut plain, mut improved) = (fan(8), fan(8));
        place(&mut plain, false, false, &Tuning { gap: GAP, label: LABEL, skip_poured: false, align: false, bypass: true, sheets: true, arrays: false, channels: true }).unwrap();
        place(&mut improved, false, true, &Tuning { gap: GAP, label: LABEL, skip_poured: false, align: false, bypass: true, sheets: true, arrays: false, channels: true }).unwrap();
        // every resistor with its LED
        let pair_len = |b: &Board| -> f64 { (0..8).map(|k| { let at = |r: String| b.parts.iter().find(|p| p.refdes == r).unwrap().pos; at(format!("R{k:02}")).dist(at(format!("D{k:02}"))) }).sum() };
        let (l0, l1) = (pair_len(&plain), pair_len(&improved));
        assert!(l1 < l0 * 0.8, "{l1} vs {l0}");
        assert!(drc::check(&improved).is_empty());
        // and the same board always gives the same placement
        let mut again = fan(8);
        place(&mut again, false, true, &Tuning { gap: GAP, label: LABEL, skip_poured: false, align: false, bypass: true, sheets: true, arrays: false, channels: true }).unwrap();
        assert!(again.parts.iter().zip(&improved.parts).all(|(a, b)| a.pos == b.pos));
    }

    #[test]
    fn a_hub_with_many_parts_gets_a_compact_board_with_its_parts_round_it() {
        let mut b = fan(8);
        let rep = autoplace(&mut b).unwrap();
        assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
        // not a sprawl: the outline is not much bigger than the parts need
        let cells: f64 = b.parts.iter().filter_map(|p| extent(&b, &p.refdes)).map(|(lo, hi)| (hi.x - lo.x + GAP) * (hi.y - lo.y + GAP + LABEL)).sum();
        assert!(rep.width * rep.height <= 3.0 * cells, "{}x{} for {cells}", rep.width, rep.height);
        // each resistor is next to its LED
        let at = |r: &str| b.parts.iter().find(|p| p.refdes == r).unwrap().pos;
        for k in 0..8 {
            let d = at(&format!("R{k:02}")).dist(at(&format!("D{k:02}")));
            assert!(d < 10.0, "R{k:02} and D{k:02} are {d} mm apart");
        }
    }

    #[test]
    fn routed_boards_are_left_alone() {
        let mut b = divider();
        autoplace(&mut b).unwrap();
        autoroute::autoroute(&mut b, &autoroute::RouteOptions::default(), None);
        assert!(autoplace(&mut b).is_err());
    }

    fn with_pour(b: &mut Board, inset: f64) {
        let (lo, hi) = b.bounds().unwrap();
        let (x0, y0, x1, y1) = (lo.x + inset, lo.y + inset, hi.x - inset, hi.y - inset);
        b.zones.push(Zone::new("GND", Side::Top, vec![Point::new(x0, y0), Point::new(x0, y1), Point::new(x1, y1), Point::new(x1, y0)]));
    }

    fn area(b: &Board) -> f64 {
        let (lo, hi) = b.bounds().unwrap();
        (hi.x - lo.x) * (hi.y - lo.y)
    }

    #[test]
    fn a_poured_ground_gives_a_smaller_board_that_still_routes() {
        let mut plain = divider();
        autoplace(&mut plain).unwrap();
        let mut poured = divider();
        with_pour(&mut poured, 0.5);
        autoplace(&mut poured).unwrap();
        assert!(area(&poured) < area(&plain), "{} {}", area(&poured), area(&plain));
        let r = autoroute::autoroute(&mut poured, &autoroute::RouteOptions::default(), None);
        assert!(r.failed.is_empty(), "{:?}", r.failed);
        assert!(ratsnest::ratsnest(&poured).is_empty());
        let v = drc::check(&poured);
        assert!(v.is_empty(), "{v:?}");
    }

    #[test]
    fn the_tuning_follows_the_pour() {
        let plain = Tuning::for_board(&divider());
        assert_eq!((plain.gap, plain.label, plain.skip_poured), (GAP, LABEL, false));
        let mut b = divider();
        with_pour(&mut b, 0.5);
        let t = Tuning::for_board(&b);
        assert!(t.skip_poured && t.gap < GAP && t.label < LABEL, "{t:?}");
    }

    #[test]
    fn a_full_board_pour_follows_the_new_outline_and_other_pours_stay() {
        let mut b = divider();
        with_pour(&mut b, 0.5);
        let odd = vec![Point::new(1.0, 1.0), Point::new(3.0, 1.0), Point::new(2.0, 3.0)];
        b.zones.push(Zone::new("GND", Side::Bottom, odd.clone()));
        autoplace(&mut b).unwrap();
        let (lo, hi) = b.bounds().unwrap();
        let xs: Vec<f64> = b.zones[0].outline().iter().map(|p| p.x).collect();
        let ys: Vec<f64> = b.zones[0].outline().iter().map(|p| p.y).collect();
        let (zx0, zx1) = (xs.iter().cloned().fold(f64::MAX, f64::min), xs.iter().cloned().fold(f64::MIN, f64::max));
        let (zy0, zy1) = (ys.iter().cloned().fold(f64::MAX, f64::min), ys.iter().cloned().fold(f64::MIN, f64::max));
        assert_eq!((zx0, zy0, zx1, zy1), (lo.x + 0.5, lo.y + 0.5, hi.x - 0.5, hi.y - 0.5));
        assert_eq!(b.zones[1].outline(), odd, "a pour that is not the whole board is left alone");
    }

    fn centre(b: &Board, r: &str) -> Point {
        let (lo, hi) = extent(b, r).unwrap();
        Point::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0)
    }

    #[test]
    fn parts_of_a_row_get_one_centre_line() {
        let mut b = divider();
        b.set_rect_outline(0.0, 0.0, 60.0, 40.0);
        // a row that wanders by a millimetre, and a second row far away
        for (r, x, y) in [("R1", 10.0, 10.0), ("R2", 20.0, 10.5), ("R3", 30.0, 11.0), ("R4", 10.0, 30.0), ("R5", 20.0, 30.0), ("R6", 30.0, 30.0)] {
            b.place_part(r, Point::new(x, y), 0);
        }
        align_parts(&mut b, &Default::default());
        let ys: Vec<f64> = ["R1", "R2", "R3"].iter().map(|r| centre(&b, r).y).collect();
        assert!(ys.iter().all(|y| (y - ys[0]).abs() < 1e-9), "{ys:?}");
        assert!((centre(&b, "R4").y - 30.0).abs() < 1e-9, "the other row stays");
        assert!(drc::check(&b).is_empty());
    }

    #[test]
    fn a_part_does_not_move_next_to_another_one() {
        let mut b = divider();
        b.set_rect_outline(0.0, 0.0, 60.0, 40.0);
        for (r, x, y) in [("R1", 10.0, 10.0), ("R2", 20.0, 10.5), ("R3", 40.0, 30.0), ("R4", 40.0, 20.0), ("R5", 50.0, 20.0), ("R6", 50.0, 30.0)] {
            b.place_part(r, Point::new(x, y), 0);
        }
        assert!(!crowded(&b, "R1"));
        b.place_part("R3", Point::new(10.0, 10.5), 0);
        assert!(crowded(&b, "R1"), "R3 lies on R1's pads");
    }

    #[test]
    fn alignment_does_not_add_centre_lines() {
        let rows = |align: bool| {
            let mut b = divider();
            autoplace_with(&mut b, &Tuning { gap: GAP, label: LABEL, skip_poured: false, align, bypass: true, sheets: true, arrays: true, channels: true }).unwrap();
            let mut ys: Vec<i64> = b.parts.iter().map(|p| (centre(&b, &p.refdes).y * 100.0).round() as i64).collect();
            ys.sort_unstable();
            ys.dedup();
            ys.len()
        };
        assert!(rows(true) <= rows(false), "{} {}", rows(true), rows(false));
    }

    #[test]
    fn a_board_with_only_the_pour_wiring_can_be_placed_again() {
        let mut b = divider();
        autoplace(&mut b).unwrap();
        let r = groundpour::ground_pour(&mut b, &groundpour::PourOptions::default()).unwrap();
        assert!(r.zone_added && r.vias > 0, "{r:?}");
        let vias = b.vias.len();
        // not placed with the plain tuning: the pour is there now, so the poured net drops out
        let rep = autoplace(&mut b).unwrap();
        assert_eq!(rep.poured, vec!["GND".to_string()]);
        assert_eq!(rep.repoured, vias, "the pour is joined through the board again");
        assert!(!b.vias.is_empty());
        assert!(b.zones.iter().any(|z| z.side == Side::Top), "the top pour comes back at the new size");
        assert!(ratsnest::ratsnest(&b).iter().all(|w| w.net != "GND"), "ground is joined");
        let v = drc::check(&b);
        assert!(v.is_empty(), "{v:?}");
    }

    #[test]
    fn other_wiring_still_stops_the_placement_and_changes_nothing() {
        let mut b = divider();
        autoplace(&mut b).unwrap();
        autoroute::autoroute(&mut b, &autoroute::RouteOptions::default(), None);
        let before = (b.tracks.len(), b.vias.len(), b.parts.iter().map(|p| p.pos).collect::<Vec<_>>());
        assert!(autoplace(&mut b).is_err());
        assert_eq!(before, (b.tracks.len(), b.vias.len(), b.parts.iter().map(|p| p.pos).collect::<Vec<_>>()));
    }

    /// Two ICs with their own bypass capacitors, and some other parts for the capacitors to drift off to.
    fn ics_with_caps() -> Board {
        let part = |r: &str, f: &str| NetlistPart { refdes: r.into(), value: "1k".into(), footprint: f.into() };
        let net = |n: &str, pins: &[(&str, &str)]| NetlistNet { name: n.into(), pins: pins.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect() };
        let mut b = Board::new("t", 10.0, 10.0);
        b.add_footprint(package::PackageSpec::Dual { pins: 14, pitch: 2.54, row_spacing: 7.62, pad_len: 1.6, pad_width: 1.6, drill: Some(0.8) }.build("DIP-14_W7.62").unwrap());
        let mut parts = vec![part("U1", "DIP-14_W7.62"), part("U2", "DIP-14_W7.62"), part("C1", "C_0805"), part("C2", "C_0805")];
        let mut nets = vec![net("VCC", &[("U1", "14"), ("U2", "14"), ("C1", "1"), ("C2", "1")]), net("GND", &[("U1", "7"), ("U2", "7"), ("C1", "2"), ("C2", "2")])];
        for k in 0..6 {
            parts.push(part(&format!("R{k}"), "R_0805"));
            nets.push(net(&format!("S{k}"), &[("U1", &(k + 1).to_string()), (&format!("R{k}"), "1")]));
            nets.push(net(&format!("T{k}"), &[("U2", &(k + 1).to_string()), (&format!("R{k}"), "2")]));
        }
        import(&mut b, &parts, &nets);
        b
    }

    /// The least distance from any pad of `cap` on `net` to a pad of `ic` on that net.
    fn gap_to_pin(b: &Board, cap: &str, ic: &str, net: &str) -> f64 {
        let (cp, ip): (Vec<&Pad>, Vec<&Pad>) = (b.pads.iter().filter(|p| p.refdes == cap && p.net == net).collect(), b.pads.iter().filter(|p| p.refdes == ic && p.net == net).collect());
        cp.iter().flat_map(|c| ip.iter().map(move |i| c.pos.dist(i.pos))).fold(f64::MAX, f64::min)
    }

    #[test]
    fn a_capacitor_between_two_supplies_is_pulled_to_the_ic_that_runs_between_them() {
        // the ICs' 0 V is VNEG (a single-supply logic IC on a negative rail): the capacitors sit between VCC and VNEG
        let place_with = |bypass: bool| {
            let mut b = ics_with_caps();
            for p in b.pads.iter_mut().filter(|p| p.net == "GND") {
                p.net = "VNEG".into();
            }
            autoplace_with(&mut b, &Tuning { gap: GAP, label: LABEL, skip_poured: false, align: false, bypass, sheets: true, arrays: true, channels: true }).unwrap();
            b
        };
        let (with, without) = (place_with(true), place_with(false));
        let d = |b: &Board| (gap_to_pin(b, "C1", "U1", "VCC") + gap_to_pin(b, "C2", "U2", "VCC") + gap_to_pin(b, "C1", "U1", "VNEG") + gap_to_pin(b, "C2", "U2", "VNEG"))
            .min(gap_to_pin(b, "C1", "U2", "VCC") + gap_to_pin(b, "C2", "U1", "VCC") + gap_to_pin(b, "C1", "U2", "VNEG") + gap_to_pin(b, "C2", "U1", "VNEG"));
        // (the two pins of a DIP-14 are 15 mm apart, so a capacitor cannot be at both: it lands between them)
        assert!(d(&with) < d(&without) * 0.75, "closer than without the pull: {} {}", d(&with), d(&without));
    }

    #[test]
    fn bypass_capacitors_sit_by_their_ic_supply_pin() {
        let place_with = |bypass: bool| {
            let mut b = ics_with_caps();
            autoplace_with(&mut b, &Tuning { gap: GAP, label: LABEL, skip_poured: false, align: false, bypass, sheets: true, arrays: true, channels: true }).unwrap();
            b
        };
        let (with, without) = (place_with(true), place_with(false));
        let d = |b: &Board| (gap_to_pin(b, "C1", "U1", "VCC") + gap_to_pin(b, "C2", "U2", "VCC")).min(gap_to_pin(b, "C1", "U2", "VCC") + gap_to_pin(b, "C2", "U1", "VCC"));
        assert!(d(&with) <= d(&without) + 1e-9, "{} {}", d(&with), d(&without));
        assert!(d(&with) < 2.0 * crate::drc::MAX_DECOUPLING_DISTANCE, "each capacitor within the limit of the check: {}", d(&with));
        assert!(d(&with) < d(&without) / 2.0, "much closer than without the pull: {} {}", d(&with), d(&without));
        assert!(drc::decoupling_checks(&with).is_empty(), "{:?}", drc::decoupling_checks(&with));
    }
}
