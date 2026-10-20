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

fn snap(v: f64) -> f64 {
    (v / SNAP).round() * SNAP
}

#[derive(Debug, Default)]
pub struct PlaceReport {
    pub placed: usize,
    pub width: f64,
    pub height: f64,
    /// Nets left out of the placement because a pour carries them (see `Tuning::skip_poured`).
    pub poured: Vec<String>,
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
}

impl Tuning {
    /// Without a pour every net needs wires (see `GAP`). With one, the poured nets drop out of the cost and the
    /// parts can sit closer.
    pub fn for_board(board: &Board) -> Self {
        if board.zones.iter().any(|z| !z.net.is_empty()) {
            Self { gap: POURED_GAP, label: POURED_LABEL, skip_poured: true, align: true }
        } else {
            Self { gap: GAP, label: LABEL, skip_poured: false, align: true }
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
        let (zlo, zhi) = z.polygon.iter().fold((Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN)), |(a, b), p| (Point::new(a.x.min(p.x), a.y.min(p.y)), Point::new(b.x.max(p.x), b.y.max(p.y))));
        let rect = z.polygon.len() == 4 && z.polygon.iter().all(|p| (p.x == zlo.x || p.x == zhi.x) && (p.y == zlo.y || p.y == zhi.y));
        let inset = [zlo.x - lo.x, zlo.y - lo.y, hi.x - zhi.x, hi.y - zhi.y];
        if rect && inset.iter().all(|d| (0.0..=2.0).contains(d)) {
            z.polygon = vec![Point::new(nlo.x + inset[0], nlo.y + inset[1]), Point::new(nlo.x + inset[0], nhi.y - inset[3]), Point::new(nhi.x - inset[2], nhi.y - inset[3]), Point::new(nhi.x - inset[2], nlo.y + inset[1])];
        }
    }
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

fn place(board: &mut Board, turn: bool, improve: bool, tuning: &Tuning) -> Result<PlaceReport, String> {
    let (gap, label) = (tuning.gap, tuning.label);
    if !board.tracks.is_empty() || !board.vias.is_empty() {
        return Err("配線済みの基板では自動配置できません。先に配線を削除してください (配線は部品の位置に合わせて引かれているため)".into());
    }
    if board.parts.is_empty() {
        return Err("部品がありません。先に「回路図から取り込み」をしてください".into());
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
    // how closely two parts belong together: the nets they share count less the more pins the net has (a ground that
    // touches everything says little about who belongs together). Made once, for every pair.
    let mut tie = vec![vec![0.0f64; items.len()]; items.len()];
    for pins in &nets {
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
    // order: begin with the part that has the most pads, then always the one most tied to those placed
    let mut order: Vec<usize> = vec![];
    let mut left: Vec<usize> = (0..items.len()).collect();
    left.sort_by(|&a, &b| degree[b].cmp(&degree[a]).then(items[a].0.cmp(&items[b].0)));
    if left.is_empty() {
        return Err("パッドを持つ部品がありません".into());
    }
    let mut score = vec![0.0f64; items.len()];
    let mut best = 0;
    loop {
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
    if improve {
        order = improve_order(&items, &nets, order, row_w);
    }
    let (mut spots, mut used_w, mut used_h) = pack(&items, &order, row_w);
    // the other way to lay the parts out: free placement round the most connected ones; the one with the shorter nets wins
    if improve && items.len() >= 4 {
        let cand = attach_layout(&items, &nets, &greedy);
        if spots_cost(&nets, &cand, items.len()) < spots_cost(&nets, &spots, items.len()) {
            (used_w, used_h) = cand.iter().fold((0.0f64, 0.0f64), |b, &(i, x, y)| (b.0.max(x + items[i].2), b.1.max(y + items[i].3)));
            spots = cand;
        }
    }
    for (i, sx, sy) in &spots {
        let (refdes, lo, ..) = &items[*i];
        let rot = board.parts.iter().find(|p| &p.refdes == refdes).map_or(0, |p| p.rot);
        // the pad box starts gap/2 inside its cell
        board.place_part(refdes, Point::new(snap(MARGIN + gap / 2.0 + sx - lo.x), snap(MARGIN + gap / 2.0 + label + sy - lo.y)), rot);
    }
    if turn {
        turn_to_shorten(board);
    }
    if tuning.align {
        align_parts(board);
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
    Ok(PlaceReport { placed: spots.len(), width: w.ceil(), height: h.ceil(), poured })
}

fn airwire_length(board: &Board) -> f64 {
    crate::ratsnest::ratsnest(board).iter().map(|w| w.a.dist(w.b)).sum()
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
fn align_parts(board: &mut Board) {
    let before = board.clone();
    let (wire, drc) = (airwire_length(board), crate::drc::check(board).len());
    for axis in [1usize, 0] {
        align_axis(board, axis);
    }
    if airwire_length(board) > wire * 1.01 + 0.5 || crate::drc::check(board).len() > drc {
        *board = before;
    }
}

fn align_axis(board: &mut Board, axis: usize) {
    let coord = |p: Point| if axis == 0 { p.x } else { p.y };
    let mut centers: Vec<(String, f64)> = board.parts.iter().filter_map(|p| extent(board, &p.refdes).map(|(lo, hi)| (p.refdes.clone(), coord(Point::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0))))).collect();
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
fn crowded(board: &Board, refdes: &str) -> bool {
    let Some((lo, hi)) = extent(board, refdes) else { return false };
    board.parts.iter().filter(|p| p.refdes != refdes).filter_map(|p| extent(board, &p.refdes)).any(|(l, h)| {
        let gap_x = (l.x - hi.x).max(lo.x - h.x);
        let gap_y = (l.y - hi.y).max(lo.y - h.y);
        gap_x.max(gap_y) < ALIGN_SPACE
    })
}

fn turn_to_shorten(board: &mut Board) {
    let center = |board: &Board, r: &str| extent(board, r).map(|(lo, hi)| Point::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0));
    for _ in 0..2 {
        let mut better = false;
        for part in board.parts.clone() {
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
        place(&mut plain, false, true, &Tuning { gap: GAP, label: LABEL, skip_poured: false, align: false }).unwrap();
        place(&mut turned, true, true, &Tuning { gap: GAP, label: LABEL, skip_poured: false, align: false }).unwrap();
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
        place(&mut plain, false, false, &Tuning { gap: GAP, label: LABEL, skip_poured: false, align: false }).unwrap();
        place(&mut improved, false, true, &Tuning { gap: GAP, label: LABEL, skip_poured: false, align: false }).unwrap();
        // every resistor with its LED
        let pair_len = |b: &Board| -> f64 { (0..8).map(|k| { let at = |r: String| b.parts.iter().find(|p| p.refdes == r).unwrap().pos; at(format!("R{k:02}")).dist(at(format!("D{k:02}"))) }).sum() };
        let (l0, l1) = (pair_len(&plain), pair_len(&improved));
        assert!(l1 < l0 * 0.8, "{l1} vs {l0}");
        assert!(drc::check(&improved).is_empty());
        // and the same board always gives the same placement
        let mut again = fan(8);
        place(&mut again, false, true, &Tuning { gap: GAP, label: LABEL, skip_poured: false, align: false }).unwrap();
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
        let xs: Vec<f64> = b.zones[0].polygon.iter().map(|p| p.x).collect();
        let ys: Vec<f64> = b.zones[0].polygon.iter().map(|p| p.y).collect();
        let (zx0, zx1) = (xs.iter().cloned().fold(f64::MAX, f64::min), xs.iter().cloned().fold(f64::MIN, f64::max));
        let (zy0, zy1) = (ys.iter().cloned().fold(f64::MAX, f64::min), ys.iter().cloned().fold(f64::MIN, f64::max));
        assert_eq!((zx0, zy0, zx1, zy1), (lo.x + 0.5, lo.y + 0.5, hi.x - 0.5, hi.y - 0.5));
        assert_eq!(b.zones[1].polygon, odd, "a pour that is not the whole board is left alone");
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
        align_parts(&mut b);
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
            autoplace_with(&mut b, &Tuning { gap: GAP, label: LABEL, skip_poured: false, align }).unwrap();
            let mut ys: Vec<i64> = b.parts.iter().map(|p| (centre(&b, &p.refdes).y * 100.0).round() as i64).collect();
            ys.sort_unstable();
            ys.dedup();
            ys.len()
        };
        assert!(rows(true) <= rows(false), "{} {}", rows(true), rows(false));
    }
}
