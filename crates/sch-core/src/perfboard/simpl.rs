//! Where the parts should go on a perfboard, worked out the way SimPL does it (B5a, 2026-11-21): the nets as springs (each net a
//! clique, each pin at its place on its part), the sum of the springs' squared lengths made least by solving a linear system (one for
//! the columns, one for the rows), then spread: the parts cut in two by area, again and again, inside a rectangle as wide as the board
//! (the shelves' placement is not used; the springs alone pile the parts up in the middle). Each part is then pulled toward its spread
//! place by a spring of its own, made stronger round by round, and solved again. The spread places are the targets `place` puts the
//! chips nearest to (the headers and the wires from off the board stay where the shelves put them, and pull as fixed pins).

use super::*;

/// How many rounds of solving and spreading.
const ROUNDS: usize = 30;
/// The anchor springs' strength added each round (the nets' springs are about 1).
const ANCHOR_STEP: f64 = 0.05;
/// A spring to the middle of the board, so a part joined to nothing fixed still has one place to go.
const HOLD: f64 = 1e-3;

/// A part that moves: its name, the room it takes (holes), and each pin's net and place from its centre (rows, columns).
struct Mover {
    refdes: String,
    room: f64,
    pins: Vec<(String, f64, f64)>,
}

/// Each part's target centre (row, column) other than the headers', or None when the table asks for sections (each a band of
/// rows of its own; not done here).
pub(super) fn targets(c: &Circuit, opt: StripOptions, gap: u16, turned: bool, wants: &Wants) -> Option<BTreeMap<String, (f64, f64)>> {
    if !wants.sections.is_empty() {
        return None;
    }
    // the headers and the wires from off the board, where the shelves put them
    let pre = place(c, opt, gap, turned, false, wants, None);
    let shapes = shapes_of(c, turned);
    let n_twos = (0..c.on.len()).filter(|k| look_on(c, *k) == Look::TwoLead).count();
    let margin: u16 = if pre.off.iter().any(|(_, v)| !v.is_empty()) { 2 } else { 1 };
    let width = board_width(&shapes, n_twos, opt, gap, margin).max(wants.kept_width());
    let heads: BTreeSet<String> = shapes.iter().filter(|s| is_head(s) && !wants.kept(&c.on[s.k].0)).map(|s| c.on[s.k].0.clone()).collect();
    let mut fixed: BTreeMap<String, Vec<(f64, f64)>> = BTreeMap::new();
    let mut y0 = 1u16;
    for p in pre.parts.iter().filter(|p| heads.contains(&p.refdes)) {
        for (pin, a) in &p.leads {
            fixed.entry(lead_net(c, &p.refdes, pin)).or_default().push((a.0 as f64, a.1 as f64));
            y0 = y0.max(a.0 + 1 + gap);
        }
    }
    // the locked parts pull as fixed pins too
    for p in &wants.keep {
        for (pin, a) in &p.leads {
            fixed.entry(lead_net(c, &p.refdes, pin)).or_default().push((a.0 as f64, a.1 as f64));
        }
    }
    for (_, v) in &pre.off {
        for (_, n, a) in v {
            fixed.entry(n.clone()).or_default().push((a.0 as f64, a.1 as f64));
        }
    }
    // the parts that move: the chips (pins from the centre of their outline) and the two-lead parts (both pins at the centre: which
    // way they lie is chosen later)
    let mut movers: Vec<Mover> = shapes
        .iter()
        .filter(|s| !is_head(s) && !wants.kept(&c.on[s.k].0))
        .map(|s| {
            let (cy, cx) = ((s.h as f64 - 1.0) / 2.0, (s.w as f64 - 1.0) / 2.0);
            Mover { refdes: c.on[s.k].0.clone(), room: room_of(s, gap) as f64, pins: s.leads.iter().map(|(_, n, a)| (n.clone(), a.0 as f64 - cy, a.1 as f64 - cx)).collect() }
        })
        .collect();
    for k in (0..c.on.len()).filter(|k| look_on(c, *k) == Look::TwoLead && !wants.kept(&c.on[*k].0)) {
        let (refdes, _, pins) = &c.on[k];
        movers.push(Mover { refdes: refdes.clone(), room: TWO_ROOM as f64, pins: pins.iter().map(|(_, n)| (n.clone(), 0.0, 0.0)).collect() });
    }
    if movers.is_empty() {
        return None;
    }
    let n = movers.len();
    // each net's pins: (part, row offset, column offset) and the fixed ones
    let mut nets: BTreeMap<String, (Vec<(usize, f64, f64)>, Vec<(f64, f64)>)> = BTreeMap::new();
    for (i, m) in movers.iter().enumerate() {
        for (net, dy, dx) in m.pins.iter().filter(|p| !p.0.is_empty()) {
            nets.entry(net.clone()).or_default().0.push((i, *dy, *dx));
        }
    }
    for (net, v) in fixed {
        if let Some(e) = nets.get_mut(&net) {
            e.1.extend(v);
        }
    }
    // the springs: (part, its pin's offset, the other end) with the other end a part's pin or a fixed hole, and a strength
    enum End {
        Part(usize, f64, f64),
        Hole(f64, f64),
    }
    let mut springs: Vec<(usize, f64, f64, End, f64)> = vec![];
    for (net, (pins, holes)) in &nets {
        let k = pins.len() + holes.len();
        if k < 2 {
            continue;
        }
        let w = if is_ground_name(net) || c.supplies.contains(net) { 0.2 } else { 1.0 } / (k - 1) as f64;
        for (a, (i, dy, dx)) in pins.iter().enumerate() {
            for (j, ey, ex) in &pins[a + 1..] {
                if i != j {
                    springs.push((*i, *dy, *dx, End::Part(*j, *ey, *ex), w));
                }
            }
            for (hy, hx) in holes {
                springs.push((*i, *dy, *dx, End::Hole(*hy, *hx), w));
            }
        }
    }
    // the rectangle the parts spread over: the board's width (inside the margins), as tall as their room needs, below the headers
    let room: f64 = movers.iter().map(|m| m.room).sum();
    let (x0, x1) = (margin as f64, (width - margin) as f64);
    let (ya, yb) = (y0 as f64, y0 as f64 + (room / (x1 - x0)).ceil().max(1.0));
    let mid = ((ya + yb) / 2.0, (x0 + x1) / 2.0);
    let mut pos: Vec<(f64, f64)> = vec![mid; n];
    let mut spread_at: Vec<(f64, f64)> = vec![mid; n];
    for round in 0..ROUNDS {
        let anchor = ANCHOR_STEP * round as f64;
        for axis in 0..2 {
            // the system A x = b for this axis (0: rows, 1: columns)
            let mut a = vec![0.0; n * n];
            let mut b = vec![0.0; n];
            let pick = |p: (f64, f64)| if axis == 0 { p.0 } else { p.1 };
            for (i, dy, dx, end, w) in &springs {
                let oi = pick((*dy, *dx));
                match end {
                    End::Part(j, ey, ex) => {
                        let oj = pick((*ey, *ex));
                        a[i * n + i] += w;
                        a[j * n + j] += w;
                        a[i * n + j] -= w;
                        a[j * n + i] -= w;
                        b[*i] += w * (oj - oi);
                        b[*j] += w * (oi - oj);
                    }
                    End::Hole(hy, hx) => {
                        a[i * n + i] += w;
                        b[*i] += w * (pick((*hy, *hx)) - oi);
                    }
                }
            }
            for i in 0..n {
                a[i * n + i] += HOLD + anchor;
                b[i] += HOLD * pick(mid) + anchor * pick(spread_at[i]);
            }
            let mut x: Vec<f64> = pos.iter().map(|p| pick(*p)).collect();
            solve(&a, &b, &mut x);
            for (p, v) in pos.iter_mut().zip(x) {
                if axis == 0 {
                    p.0 = v;
                } else {
                    p.1 = v;
                }
            }
        }
        let mut items: Vec<(usize, f64, f64, f64)> = (0..n).map(|i| (i, movers[i].room, pos[i].0, pos[i].1)).collect();
        spread(&mut items, (ya, yb, x0, x1), &mut spread_at);
    }
    Some(movers.into_iter().zip(spread_at).map(|(m, p)| (m.refdes, p)).collect())
}

/// Solves A x = b (A symmetric and positive definite, n × n) by conjugate gradients, starting from `x`.
fn solve(a: &[f64], b: &[f64], x: &mut [f64]) {
    let n = b.len();
    let mul = |v: &[f64]| -> Vec<f64> { (0..n).map(|i| (0..n).map(|j| a[i * n + j] * v[j]).sum()).collect() };
    let ax = mul(x);
    let mut r: Vec<f64> = (0..n).map(|i| b[i] - ax[i]).collect();
    let mut p = r.clone();
    let mut rr: f64 = r.iter().map(|v| v * v).sum();
    for _ in 0..2 * n + 10 {
        if rr < 1e-10 {
            break;
        }
        let ap = mul(&p);
        let step = rr / p.iter().zip(&ap).map(|(u, v)| u * v).sum::<f64>();
        for i in 0..n {
            x[i] += step * p[i];
            r[i] -= step * ap[i];
        }
        let next: f64 = r.iter().map(|v| v * v).sum();
        for i in 0..n {
            p[i] = r[i] + next / rr * p[i];
        }
        rr = next;
    }
}

/// Spreads the parts (index, room, row, column) over the rectangle (top, bottom, left, right): cut across its longer side where
/// the parts' rooms divide in two halves (in order along that side), each half into its share of the rectangle, again and again; a
/// part alone keeps its place, moved inside its piece. The places go to `out`.
fn spread(items: &mut [(usize, f64, f64, f64)], r: (f64, f64, f64, f64), out: &mut [(f64, f64)]) {
    let (y0, y1, x0, x1) = r;
    match items.len() {
        0 => {}
        1 => {
            let (i, _, y, x) = items[0];
            out[i] = (y.clamp(y0, y1), x.clamp(x0, x1));
        }
        len => {
            let across = x1 - x0 >= y1 - y0;
            items.sort_by(|p, q| if across { p.3.total_cmp(&q.3) } else { p.2.total_cmp(&q.2) });
            let total: f64 = items.iter().map(|p| p.1).sum();
            let (mut k, mut sum) = (0, 0.0);
            while k < len - 1 && (k == 0 || sum + items[k].1 / 2.0 <= total / 2.0) {
                sum += items[k].1;
                k += 1;
            }
            let f = sum / total;
            let (a, b) = items.split_at_mut(k);
            if across {
                let cut = x0 + (x1 - x0) * f;
                spread(a, (y0, y1, x0, cut), out);
                spread(b, (y0, y1, cut, x1), out);
            } else {
                let cut = y0 + (y1 - y0) * f;
                spread(a, (y0, cut, x0, x1), out);
                spread(b, (cut, y1, x0, x1), out);
            }
        }
    }
}
