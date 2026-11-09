//! Ordering the parts in rows: the net-length cost, the greedy order, the short annealing run and the row packing.

use super::*;

/// A repeatable stream of numbers (xorshift): the same board always gives the same placement.
pub(super) struct Rng(pub(super) u64);

impl Rng {
    pub(super) fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    pub(super) fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    pub(super) fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Shelf-pack the parts in `order`: each part's cell corner, and the used width and height.
pub(super) fn pack(items: &[(String, Point, f64, f64)], order: &[usize], row_w: f64) -> (Vec<(usize, f64, f64)>, f64, f64) {
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
pub(super) type NetPins = Vec<(usize, f64, f64)>;

/// The sum of the half-perimeters of the boxes around each net's pads for the packing of `order`, each divided by the
/// number of connections the net needs.
pub(super) fn net_length(items: &[(String, Point, f64, f64)], nets: &[NetPins], apart: &[Apart], order: &[usize], row_w: f64) -> f64 {
    let (spots, ..) = pack(items, order, row_w);
    spots_cost(items, nets, apart, &spots)
}

/// A distance of the design's requirements, for the layout: either parts to keep apart (`near` false: the items on the two sides and the
/// least gap between their cells, mm) or parts that belong together (`near`: the items of a group, `a` and `b` the same, and the most
/// distance between the middles of their cells, mm).
pub(super) struct Apart {
    pub(super) a: Vec<usize>,
    pub(super) b: Vec<usize>,
    pub(super) limit: f64,
    pub(super) near: bool,
}

/// What a mm of a distance that falls short counts against a mm of net length (see `apart_cost`). Measured on the microohm meter.
pub(super) const APART_WEIGHT: f64 = 300.0;

/// The gap between the cells of two items (0 when they touch), from their corners.
pub(super) fn cell_gap(items: &[(String, Point, f64, f64)], i: usize, (x, y): (f64, f64), j: usize, (u, v): (f64, f64)) -> f64 {
    let dx = (u - (x + items[i].2)).max(x - (u + items[j].2)).max(0.0);
    let dy = (v - (y + items[i].3)).max(y - (v + items[j].3)).max(0.0);
    dx.hypot(dy)
}

/// The distance between the middles of the cells of two items.
pub(super) fn cell_centres(items: &[(String, Point, f64, f64)], i: usize, (x, y): (f64, f64), j: usize, (u, v): (f64, f64)) -> f64 {
    ((x + items[i].2 / 2.0) - (u + items[j].2 / 2.0)).hypot((y + items[i].3 / 2.0) - (v + items[j].3 / 2.0))
}

/// How far the cells fall short of the distances to keep (or are too far for a group), in mm, times `APART_WEIGHT`. The cell is bigger than
/// the pads in it, so the real distance of the pads is a little more than the gap of the cells: a part may be pushed a little further than
/// the rule says.
pub(super) fn apart_cost(items: &[(String, Point, f64, f64)], apart: &[Apart], at: impl Fn(usize) -> Option<(f64, f64)>) -> f64 {
    let mut sum = 0.0;
    for r in apart {
        for &i in &r.a {
            let Some(p) = at(i) else { continue };
            for &j in r.b.iter().filter(|&&j| j != i && !(r.near && j < i)) {
                let Some(q) = at(j) else { continue };
                sum += if r.near { (cell_centres(items, i, p, j, q) - r.limit).max(0.0) } else { (r.limit - cell_gap(items, i, p, j, q)).max(0.0) };
            }
        }
    }
    APART_WEIGHT * sum
}

/// [`net_length`] for given cell corners (`spots`: item, x, y).
pub(super) fn spots_cost(items: &[(String, Point, f64, f64)], nets: &[NetPins], apart: &[Apart], spots: &[(usize, f64, f64)]) -> f64 {
    let mut at = vec![(0.0, 0.0); items.len()];
    for &(i, x, y) in spots {
        at[i] = (x, y);
    }
    nets.iter().map(|pins| net_hpwl(pins, |i| Some(at[i]))).sum::<f64>() + apart_cost(items, apart, |i| Some(at[i]))
}

/// The half-perimeter of the box around a net's pads (the parts that `at` knows), divided by the number of
/// connections the net needs.
pub(super) fn net_hpwl(pins: &NetPins, at: impl Fn(usize) -> Option<(f64, f64)>) -> f64 {
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
/// `fixed` (empty, or one entry per item): the items that stand already (the parts a person locked), at these cell corners; then the others
/// are placed round them, and nothing is moved to the origin.
pub(super) fn attach_layout(items: &[(String, Point, f64, f64)], nets: &[NetPins], apart: &[Apart], order: &[usize], fixed: &[Option<(f64, f64)>]) -> Vec<(usize, f64, f64)> {
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
    for (i, f) in fixed.iter().enumerate() {
        if let Some(p) = f {
            at[i] = Some(*p);
            placed.push(i);
        }
    }
    let free = |at: &[Option<(f64, f64)>], placed: &[usize], i: usize, x: f64, y: f64| {
        placed.iter().all(|&j| {
            let (px, py) = at[j].unwrap();
            !(x < px + items[j].2 - 1e-9 && px < x + items[i].2 - 1e-9 && y < py + items[j].3 - 1e-9 && py < y + items[i].3 - 1e-9)
        })
    };
    let todo: Vec<usize> = order.iter().copied().filter(|&i| fixed.get(i).is_none_or(|f| f.is_none())).collect();
    for &i in &todo {
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
            // the distances to keep from the parts already placed
            c += apart_cost(items, apart, |j| if j == i { Some((x, y)) } else { at[j] }) - apart_cost(items, apart, |j| if j == i { None } else { at[j] });
            // stay compact: a small pull to the middle of what is placed
            if placed.is_empty() { c } else { c + PULL * ((x + items[i].2 / 2.0 - centre.0).hypot(y + items[i].3 / 2.0 - centre.1)) }
        };
        // a part that has to keep away from parts already placed may find no room far enough among the spots nearest to where it would like to
        // be: the whole board is searched on a coarser grid for the spots where it keeps its distance
        let keeps_away = apart.iter().any(|r| (r.a.contains(&i) && r.b.iter().any(|j| placed.contains(j))) || (r.b.contains(&i) && r.a.iter().any(|j| placed.contains(j))));
        if keeps_away && found.iter().any(|&p| apart_cost(items, apart, |j| if j == i { Some(p) } else { at[j] }) > apart_cost(items, apart, |j| if j == i { None } else { at[j] }) + 1e-9) {
            let reach = apart.iter().map(|r| r.limit).fold(0.0, f64::max);
            let (gx0, gx1, gy0, gy1) = (bx0 - reach, bx1 + reach, by0 - reach, by1 + reach);
            let mut y = gy0;
            while y <= gy1 {
                let mut x = gx0;
                while x <= gx1 {
                    let p = ((x / STEP).round() * STEP, (y / STEP).round() * STEP);
                    if free(&at, &placed, i, p.0, p.1) {
                        found.push(p);
                    }
                    x += 1.0;
                }
                y += 1.0;
            }
        }
        // round parts a person locked, the room left between them may be too small for a big block where it would like to be (the nearest
        // free spots are then all on one side): the whole board and a cell's size round it are searched on the coarser grid as well
        if fixed.iter().any(|f| f.is_some()) && !placed.is_empty() {
            let reach = items[i].2.max(items[i].3);
            let mut y = by0 - reach;
            while y <= by1 + reach {
                let mut x = bx0 - reach;
                while x <= bx1 + reach {
                    let p = ((x / STEP).round() * STEP, (y / STEP).round() * STEP);
                    if free(&at, &placed, i, p.0, p.1) {
                        found.push(p);
                    }
                    x += 1.0;
                }
                y += 1.0;
            }
        }
        let best = found.into_iter().min_by(|a, b| cost(*a).total_cmp(&cost(*b))).unwrap();
        at[i] = Some(best);
        placed.push(i);
    }
    if fixed.iter().any(|f| f.is_some()) {
        return placed.iter().map(|&i| (i, at[i].unwrap().0, at[i].unwrap().1)).collect();
    }
    let (x0, y0) = placed.iter().fold((f64::MAX, f64::MAX), |b, &j| (b.0.min(at[j].unwrap().0), b.1.min(at[j].unwrap().1)));
    order.iter().map(|&i| (i, at[i].unwrap().0 - x0, at[i].unwrap().1 - y0)).collect()
}

/// The order in which to place the items round the `fixed` ones: always the one most tied to those placed (the fixed first), the one with
/// the most nets when none is tied.
pub(super) fn fixed_order(items: &[(String, Point, f64, f64)], tie: &[Vec<f64>], degree: &[usize], fixed: &[Option<(f64, f64)>]) -> Vec<usize> {
    let n = items.len();
    let mut score = vec![0.0f64; n];
    let mut left: Vec<usize> = (0..n).filter(|&i| fixed[i].is_none()).collect();
    left.sort_by(|&a, &b| degree[b].cmp(&degree[a]).then(items[a].0.cmp(&items[b].0)));
    for f in (0..n).filter(|&i| fixed[i].is_some()) {
        for &m in &left {
            score[m] += tie[m][f];
        }
    }
    let mut order = vec![];
    while !left.is_empty() {
        let mut best = 0;
        for k in 1..left.len() {
            if score[left[k]] > score[left[best]] + 1e-9 {
                best = k;
            }
        }
        let i = left.remove(best);
        order.push(i);
        for &m in &left {
            score[m] += tie[m][i];
        }
    }
    order
}

/// Improve the order by swapping and moving parts, keeping what shortens the nets (and, early on, some of what does
/// not, to get out of dead ends).
pub(super) fn improve_order(items: &[(String, Point, f64, f64)], nets: &[NetPins], apart: &[Apart], mut order: Vec<usize>, row_w: f64) -> Vec<usize> {
    let n = order.len();
    if n < 4 || nets.is_empty() {
        return order;
    }
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut cost = net_length(items, nets, apart, &order, row_w);
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
        let c = net_length(items, nets, apart, &next, row_w);
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
pub(super) fn ties(nets: &[NetPins], n: usize) -> Vec<Vec<f64>> {
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
pub(super) fn layout(items: &[(String, Point, f64, f64)], nets: &[NetPins], apart: &[Apart], degree: &[usize], tie: &[Vec<f64>], improve: bool, compact: bool, fit: bool) -> (Vec<(usize, f64, f64)>, f64, f64) {
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
            let mut o = vec![improve_order(items, nets, apart, order.clone(), rw)];
            // shelf by shelf, the places of the parts follow their order: with distances to keep, the search also starts from an order with
            // one side of each at the start and the other at the end, and the cheaper of the two wins
            if let Some(far) = parted_order(&order, apart) {
                o.push(improve_order(items, nets, apart, far, rw));
            }
            o
        } else {
            vec![order.clone()]
        };
        let orders_count = orders.len();
        for o in orders {
            let (_, uw, uh) = pack(items, &o, rw);
            // (a long thin strip has the least area for some blocks: the aspect counts against it)
            let aspect = (uw / uh).max(uh / uw);
            let score = if widths.len() == 1 && orders_count > 1 {
                net_length(items, nets, apart, &o, rw)
            } else if widths.len() > 1 { uw * uh * (1.0 + 0.5 * (aspect - 1.8).max(0.0)) + BLOCK_NET_WEIGHT * net_length(items, nets, apart, &o, rw) } else { 0.0 };
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
        let cand = attach_layout(items, nets, apart, &greedy, &[]);
        if spots_cost(items, nets, apart, &cand) < spots_cost(items, nets, apart, &spots) {
            (used_w, used_h) = cand.iter().fold((0.0f64, 0.0f64), |b, &(i, x, y)| (b.0.max(x + items[i].2), b.1.max(y + items[i].3)));
            spots = cand;
        }
    }
    if improve && !apart.is_empty() {
        repair_apart(items, nets, apart, &mut spots);
        (used_w, used_h) = spots.iter().fold((0.0f64, 0.0f64), |b, &(i, x, y)| (b.0.max(x + items[i].2), b.1.max(y + items[i].3)));
    }
    (spots, used_w, used_h)
}

/// The parts that fall short of a distance to keep are moved one by one to the place on a 1 mm grid, free of the others, where the nets and
/// the distances together cost least (a little pull to the middle keeps the board from growing); until nothing moves.
pub(super) fn repair_apart(items: &[(String, Point, f64, f64)], nets: &[NetPins], apart: &[Apart], spots: &mut [(usize, f64, f64)]) {
    const PULL: f64 = 0.1;
    let n = items.len();
    let mut at: Vec<Option<(f64, f64)>> = vec![None; n];
    for &(i, x, y) in spots.iter() {
        at[i] = Some((x, y));
    }
    let mut mine: Vec<Vec<usize>> = vec![vec![]; n];
    for (k, pins) in nets.iter().enumerate() {
        for &(i, ..) in pins {
            if !mine[i].contains(&k) {
                mine[i].push(k);
            }
        }
    }
    let reach = apart.iter().map(|r| r.limit).fold(0.0, f64::max);
    let involved: Vec<usize> = (0..n).filter(|i| apart.iter().any(|r| r.a.contains(i) || r.b.contains(i))).collect();
    for _ in 0..3 {
        let mut moved = false;
        for &i in &involved {
            let with = |at: &[Option<(f64, f64)>], p: (f64, f64)| {
                let known = |j: usize| if j == i { Some(p) } else { at[j] };
                let c: f64 = mine[i].iter().map(|&k| net_hpwl(&nets[k], known)).sum();
                c + apart_cost(items, apart, known)
            };
            let here = at[i].unwrap();
            if apart_cost(items, apart, |j| at[j]) - apart_cost(items, apart, |j| if j == i { None } else { at[j] }) <= 1e-9 {
                continue;
            }
            let (bx0, bx1, by0, by1) = (0..n).filter(|&j| j != i).fold((f64::MAX, f64::MIN, f64::MAX, f64::MIN), |b, j| {
                let (x, y) = at[j].unwrap();
                (b.0.min(x), b.1.max(x + items[j].2), b.2.min(y), b.3.max(y + items[j].3))
            });
            let centre = ((bx0 + bx1) / 2.0, (by0 + by1) / 2.0);
            let score = |at: &[Option<(f64, f64)>], p: (f64, f64)| with(at, p) + PULL * (p.0 + items[i].2 / 2.0 - centre.0).hypot(p.1 + items[i].3 / 2.0 - centre.1);
            let mut best = (score(&at, here), here);
            let mut y = by0 - reach;
            while y <= by1 + reach {
                let mut x = bx0 - reach;
                while x <= bx1 + reach {
                    let free = (0..n).filter(|&j| j != i).all(|j| {
                        let (px, py) = at[j].unwrap();
                        !(x < px + items[j].2 - 1e-9 && px < x + items[i].2 - 1e-9 && y < py + items[j].3 - 1e-9 && py < y + items[i].3 - 1e-9)
                    });
                    if free {
                        let c = score(&at, (x, y));
                        if c < best.0 - 1e-9 {
                            best = (c, (x, y));
                        }
                    }
                    x += 1.0;
                }
                y += 1.0;
            }
            if best.1 != here {
                at[i] = Some(best.1);
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    // (the board is moved to the origin again)
    let (x0, y0) = at.iter().flatten().fold((f64::MAX, f64::MAX), |b, p| (b.0.min(p.0), b.1.min(p.1)));
    for s in spots.iter_mut() {
        let p = at[s.0].unwrap();
        (s.1, s.2) = (p.0 - x0, p.1 - y0);
    }
}

/// `order` with the items on the first side of every distance to keep moved to the front and those on the second side to the back, the
/// rest in their order between (None when there is nothing to keep apart).
pub(super) fn parted_order(order: &[usize], apart: &[Apart]) -> Option<Vec<usize>> {
    if !apart.iter().any(|r| !r.near) {
        return None;
    }
    let first: std::collections::BTreeSet<usize> = apart.iter().filter(|r| !r.near).flat_map(|r| r.a.iter().copied()).collect();
    let last: std::collections::BTreeSet<usize> = apart.iter().filter(|r| !r.near).flat_map(|r| r.b.iter().copied()).filter(|i| !first.contains(i)).collect();
    let pick = |f: &dyn Fn(usize) -> bool| order.iter().copied().filter(|&i| f(i)).collect::<Vec<_>>();
    let mut out = pick(&|i| first.contains(&i));
    out.extend(pick(&|i| !first.contains(&i) && !last.contains(&i)));
    out.extend(pick(&|i| last.contains(&i)));
    Some(out)
}

/// How much a mm of net length counts against a mm² of block area when the widths a block is packed to are compared.
pub(super) const BLOCK_NET_WEIGHT: f64 = 0.4;
/// A block of at most this many items is packed in every order (the blocks of a sheet, the parts of a small copy).
pub(super) const EVERY_ORDER: usize = 7;

pub(super) fn permutations(v: &mut Vec<usize>, k: usize, out: &mut Vec<Vec<usize>>) {
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
