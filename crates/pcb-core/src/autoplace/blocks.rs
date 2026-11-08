//! Placing by blocks: one block per schematic sheet, nested together, and the repeated copies (arrays) of one sub-circuit.

use super::*;

/// Room kept round a sheet's block (mm), so that the blocks can be told apart.
pub(super) const BLOCK_GAP: f64 = 2.0;

/// The items of each schematic sheet (`sheet`: the sheet of every item), when there is more than one sheet; items of no sheet make a
/// group of their own. None for a one-sheet circuit.
pub(super) fn sheet_groups(sheet: &[String]) -> Option<Vec<Vec<usize>>> {
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
pub(super) type Cells = Vec<(f64, f64, f64, f64)>;

/// The raster of the nesting: the size of a pixel (mm).
pub(super) const NEST_PIXEL: f64 = 1.0;
/// The places tried are this many pixels apart.
pub(super) const NEST_STEP: usize = 3;
/// How many orders of the blocks are tried by `nest_blocks` besides the biggest first.
pub(super) const NEST_ORDERS: usize = 12;
/// How many mirrorings of a block are tried (1: as it is, 2: also left to right, 4: also top to bottom). Mirroring was tried and left
/// at 1: the nets between the sheets got longer (crossings 403 → 638 on the microohm meter) for a smaller box.
pub(super) const NEST_MIRRORS: u8 = 1;

/// The blocks packed as the shapes they are, not as boxes: each is rasterised (its squares, grown by half the gap round them), and one after
/// another, in an order, each goes to the place that makes the whole box smallest (a long thin box counted against), among the places where it
/// does not cover what is there: a block can lie in the bay of another. A block may also stand mirrored (left to right, top to bottom: the
/// parts keep their own turn, only their places are mirrored), which gives the bays more ways to meet. Many orders are tried (the biggest
/// first, then shuffled ones) and the best whole is kept: the box and the nets between the blocks (the pins of a block at the places its
/// parts have). At the end every block is slid left and up as far as it goes. Returns the corner and the mirroring (bit 0: left to right,
/// bit 1: top to bottom) of each block, and the size of the whole.
pub(super) fn nest_blocks(shapes: &[Cells], nets: &[NetPins]) -> (Vec<(usize, f64, f64, u8)>, f64, f64) {
    let n = shapes.len();
    let half = BLOCK_GAP / 2.0;
    // the size of each block in the cells' own coordinates
    let dims: Vec<(f64, f64)> = shapes.iter().map(|c| (c.iter().map(|k| k.0 + k.2).fold(0.0, f64::max), c.iter().map(|k| k.1 + k.3).fold(0.0, f64::max))).collect();
    // the pixels each block covers in each of its 4 mirrorings, and its size (the block's parts stand half the gap in from its corner: the squares are grown by the gap)
    let masks: Vec<[(Vec<(usize, usize)>, usize, usize); 4]> = shapes
        .iter()
        .enumerate()
        .map(|(b, cells)| {
            std::array::from_fn(|v| {
                let mut px: std::collections::BTreeSet<(usize, usize)> = Default::default();
                let (mut mw, mut mh) = (0usize, 0usize);
                for &(x, y, w, h) in cells {
                    let x = if v & 1 != 0 { dims[b].0 - x - w } else { x };
                    let y = if v & 2 != 0 { dims[b].1 - y - h } else { y };
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
        })
        .collect();
    let area: usize = masks.iter().map(|m| m[0].1 * m[0].2).sum();
    let side = ((area as f64) * 3.0).sqrt().ceil() as usize + masks.iter().map(|m| m[0].1.max(m[0].2)).max().unwrap_or(0);
    // the nets between blocks, the pins of a block at its corner, mirrored as the block is
    let hpwl = |at: &[Option<(f64, f64, u8)>]| -> f64 {
        nets.iter()
            .map(|pins| {
                let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
                let mut any = false;
                for &(i, ox, oy) in pins {
                    if let Some((x, y, v)) = at[i] {
                        let (lx, ly) = (ox - half, oy - half);
                        let lx = if v & 1 != 0 { dims[i].0 - lx } else { lx };
                        let ly = if v & 2 != 0 { dims[i].1 - ly } else { ly };
                        x0 = x0.min(x + lx + half);
                        x1 = x1.max(x + lx + half);
                        y0 = y0.min(y + ly + half);
                        y1 = y1.max(y + ly + half);
                        any = true;
                    }
                }
                if any { ((x1 - x0) + (y1 - y0)) / (pins.len() as f64 - 1.0).max(1.0) } else { 0.0 }
            })
            .sum()
    };
    let score_of = |w: usize, h: usize, at: &[Option<(f64, f64, u8)>]| -> f64 {
        let (w, h) = (w as f64, h as f64);
        let aspect = (w / h).max(h / w);
        w * h * (1.0 + 0.5 * (aspect - 1.8).max(0.0)) + BLOCK_NET_WEIGHT * hpwl(at)
    };
    type Packed = (f64, Vec<(usize, usize, u8)>, usize, usize);
    let pack = |order: &[usize]| -> Packed {
        let mut occupied = vec![false; side * side];
        let mut at: Vec<Option<(f64, f64, u8)>> = vec![None; n];
        let mut put: Vec<(usize, usize, u8)> = vec![(0, 0, 0); n];
        let (mut bw, mut bh) = (0usize, 0usize);
        for &b in order {
            let mut best: Option<(f64, usize, usize, u8)> = None;
            for v in 0..NEST_MIRRORS {
                let (px, mw, mh) = (&masks[b][v as usize].0, masks[b][v as usize].1, masks[b][v as usize].2);
                let mut y = 0;
                while y + mh <= side {
                    let mut x = 0;
                    while x + mw <= side {
                        // a place that would stretch the box much further than the best so far is not worth looking at
                        let (nw, nh) = (bw.max(x + mw), bh.max(y + mh));
                        if best.is_none_or(|bb| (nw * nh) as f64 <= bb.0 * 1.5 + 1.0) && px.iter().all(|&(i, j)| !occupied[(y + j) * side + x + i]) {
                            at[b] = Some((x as f64 * NEST_PIXEL, y as f64 * NEST_PIXEL, v));
                            let sc = score_of(nw, nh, &at);
                            if best.is_none_or(|bb| sc < bb.0) {
                                best = Some((sc, x, y, v));
                            }
                            at[b] = None;
                        }
                        x += NEST_STEP;
                    }
                    y += NEST_STEP;
                }
            }
            let Some((_, x, y, v)) = best else { return (f64::MAX, put, side, side) };
            let (px, mw, mh) = (&masks[b][v as usize].0, masks[b][v as usize].1, masks[b][v as usize].2);
            for &(i, j) in px {
                occupied[(y + j) * side + x + i] = true;
            }
            at[b] = Some((x as f64 * NEST_PIXEL, y as f64 * NEST_PIXEL, v));
            put[b] = (x, y, v);
            bw = bw.max(x + mw);
            bh = bh.max(y + mh);
        }
        // every block slid left and up as far as it goes (the places tried are `NEST_STEP` apart, which leaves slack), the ones at the
        // left and the top first, until nothing moves
        let mut by_corner: Vec<usize> = order.to_vec();
        by_corner.sort_by_key(|&b| put[b].0 + put[b].1);
        loop {
            let mut moved = false;
            for &b in &by_corner {
                let (mut x, mut y, v) = put[b];
                let px = &masks[b][v as usize].0;
                for &(i, j) in px {
                    occupied[(y + j) * side + x + i] = false;
                }
                let free = |x: usize, y: usize| px.iter().all(|&(i, j)| !occupied[(y + j) * side + x + i]);
                while x > 0 && free(x - 1, y) {
                    x -= 1;
                }
                while y > 0 && free(x, y - 1) {
                    y -= 1;
                }
                for &(i, j) in px {
                    occupied[(y + j) * side + x + i] = true;
                }
                if (x, y) != (put[b].0, put[b].1) {
                    put[b].0 = x;
                    put[b].1 = y;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        let (mut bw, mut bh) = (0usize, 0usize);
        for &b in order {
            let (x, y, v) = put[b];
            bw = bw.max(x + masks[b][v as usize].1);
            bh = bh.max(y + masks[b][v as usize].2);
            at[b] = Some((x as f64 * NEST_PIXEL, y as f64 * NEST_PIXEL, v));
        }
        (score_of(bw, bh, &at), put, bw, bh)
    };
    let mut first: Vec<usize> = (0..n).collect();
    first.sort_by_key(|&b| std::cmp::Reverse(masks[b][0].0.len()));
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
    (put.into_iter().enumerate().map(|(b, (x, y, v))| (b, x as f64 * NEST_PIXEL, y as f64 * NEST_PIXEL, v)).collect(), bw as f64 * NEST_PIXEL, bh as f64 * NEST_PIXEL)
}

pub(super) fn sheet_blocks(items: &[(String, Point, f64, f64)], nets: &[NetPins], degree: &[usize], groups: &[Vec<usize>], improve: bool) -> (Vec<(usize, f64, f64)>, f64, f64) {
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
        let (spots, w, h) = layout(&sub_items, &sub_nets, &[], &sub_degree, &tie, improve, false, false);
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
    let (bspots, used_w, used_h) = if improve {
        nest_blocks(&shapes, &block_nets)
    } else {
        let tie = ties(&block_nets, groups.len());
        let (spots, w, h) = layout(&block_items, &block_nets, &[], &block_degree, &tie, improve, true, true);
        (spots.into_iter().map(|(k, x, y)| (k, x, y, 0u8)).collect(), w, h)
    };
    let mut out = vec![];
    for (k, bx, by, v) in bspots {
        // the size of the block's squares, for the mirroring
        let (bw, bh) = (shapes[k].iter().map(|c| c.0 + c.2).fold(0.0, f64::max), shapes[k].iter().map(|c| c.1 + c.3).fold(0.0, f64::max));
        for &(i, x, y) in &local[k].0 {
            let x = if v & 1 != 0 { bw - x - items[i].2 } else { x };
            let y = if v & 2 != 0 { bh - y - items[i].3 } else { y };
            out.push((i, bx + BLOCK_GAP / 2.0 + x, by + BLOCK_GAP / 2.0 + y));
        }
    }
    (out, used_w, used_h)
}

/// A grid of copies of one structure, as one item: where each member stands (item, offset from the array's corner), and the size.
pub(super) struct Array {
    pub(super) members: Vec<(usize, f64, f64)>,
    pub(super) w: f64,
    pub(super) h: f64,
}

/// For every group of copies (see `replicas`): lay the first copy out as a small circuit of its own, and put every copy in the same way,
/// one after another in the order of the designators' numbers, in rows of about the shape of a 3:2 board.
pub(super) fn build_arrays(items: &[(String, Point, f64, f64)], nets: &[NetPins], degree: &[usize], groups: &[Vec<Vec<usize>>], improve: bool) -> Vec<Array> {
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
        let (spots, w, h) = layout(&sub_items, &sub_nets, &[], &sub_degree, &tie, improve, true, false);
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
pub(super) enum Origin {
    Single(usize),
    Array(usize),
}

pub(super) type Collapsed = (Vec<(String, Point, f64, f64)>, Vec<NetPins>, Vec<usize>, Vec<String>, Vec<Origin>);

/// The problem with every array as one item: the parts not in an array, then the arrays. The pins of the members sit where the array's
/// cell puts them; a net inside one array is no net any more.
pub(super) fn collapse(items: &[(String, Point, f64, f64)], nets: &[NetPins], degree: &[usize], arrays: &[Array], sheet: &dyn Fn(usize) -> String) -> Collapsed {
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
pub(super) fn template_cost(board: &Board, members: &[String], rots: &[u8], gap: f64, label: f64) -> f64 {
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
    let (spots, w, h) = layout(&items, &nets, &[], &degree, &tie, false, true, false);
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
pub(super) fn turn_copies(board: &mut Board, groups: &[Vec<Vec<usize>>], names: &[String], gap: f64, label: f64) {
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
