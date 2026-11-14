//! A quadtree mesh for the field solver: square cells split only where the field needs it (next to the corners of the copper,
//! less along its sides), so the cells far from the copper stay big. On the rows-and-columns mesh a line made fine for a corner
//! runs across the whole section; here it does not, so the same accuracy takes a fraction of the nodes.
//!
//! Neighbouring cells differ by at most one level (2:1). A node in the middle of a side of a bigger cell ("hanging") is held at
//! the mean of that side's two ends, which keeps the potential continuous. Each cell is a bilinear element; its permittivity is
//! averaged from samples inside it, along x and along y as the layers of the board are crossed (in series across the layers, side
//! by side along them), which is exact for flat layers, so the edges of the board and the mask need no cells of their own.

use super::{Rect, Shapes};
use std::collections::{HashMap, HashSet};

/// How fine the mesh is: the cell next to a corner of the copper and next to its sides (fractions of the smallest feature), and
/// how fast the cells grow with the distance from them.
#[derive(Clone, Copy, Debug)]
pub(super) struct Params {
    pub corner_div: f64,
    pub edge_div: f64,
    pub grow: f64,
}

/// The deepest level a cell is split to.
const MAX_LEVEL: u32 = 26;

/// A cell: level, column, row (the roots are level 0).
type Cell = (u32, u64, u64);

pub(super) struct Mesh {
    /// Side of a root cell (mm) and how many roots across and up.
    root: f64,
    nrx: u64,
    nry: u64,
    /// Node positions (mm), the regular nodes first (`regular` of them); the others hang and follow `follow`.
    pos: Vec<(f64, f64)>,
    regular: usize,
    /// Each node as a sum of regular nodes with weights.
    expr: Vec<Vec<(usize, f64)>>,
    /// The cells: their corners (bottom left, bottom right, top right, top left), size (mm) and permittivity across x / y with the
    /// materials.
    cells: Vec<([usize; 4], f64, f64, f64)>,
    copper: Vec<(Rect, f64)>,
    /// The cell at each (level, column, row), to find the one a point is in.
    lookup: HashMap<Cell, usize>,
    /// Number of nodes the potential is solved at (the hanging ones follow them).
    pub nodes: usize,
}

/// The roots for a section: square, `root` mm on a side; for a microstrip the top of the board falls on a line of every level (a
/// thin track on it is exactly on nodes), for a stripline the middle between the planes.
pub(super) fn domain(xmax: f64, ymax: f64, height: Option<f64>) -> (f64, u64, u64) {
    match height {
        Some(h) => {
            let mut l = h;
            while l < xmax.max(ymax) {
                l *= 2.0;
            }
            (l, 1, 1)
        }
        None => (ymax, (xmax / ymax).ceil().max(1.0) as u64, 1),
    }
}

fn seg_dist(p: (f64, f64), a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((p.0 - a[0]) * dx + (p.1 - a[1]) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    ((p.0 - a[0] - t * dx).powi(2) + (p.1 - a[1] - t * dy).powi(2)).sqrt()
}

impl Mesh {
    pub(super) fn new(sh: &Shapes, (root, nrx, nry): (f64, u64, u64), prm: Params) -> Self {
        let (xdom, ydom) = (root * nrx as f64, root * nry as f64);
        let on_box = |x: f64, y: f64| x <= 1e-12 || x >= xdom - 1e-9 || y <= 1e-12 || y >= ydom - 1e-9;
        // the corners of the copper (not where it meets the box or the middle) and its sides
        let mut corners = vec![];
        let mut sides = vec![];
        for (r, _) in &sh.copper {
            let c = r.corners();
            for k in 0..4 {
                let (a, b) = (c[k], c[(k + 1) % 4]);
                if !on_box(a[0], a[1]) && !(a[0] <= 1e-12) {
                    corners.push((a[0], a[1]));
                }
                // a side along the box edge or the middle line is not a side the field crowds at
                let along_box = (a[0] <= 1e-12 && b[0] <= 1e-12) || (a[0] >= xdom - 1e-9 && b[0] >= xdom - 1e-9);
                if !along_box && (a[0] - b[0]).hypot(a[1] - b[1]) > 0.0 {
                    sides.push((a, b));
                }
            }
        }
        let (hc, he, g) = (sh.fine / prm.corner_div, sh.fine / prm.edge_div, prm.grow);
        let hmax = root / 4.0;
        let wanted = |cx: f64, cy: f64, r: f64| -> f64 {
            let dc = corners.iter().map(|&(x, y)| (cx - x).hypot(cy - y)).fold(f64::INFINITY, f64::min);
            let mut want = hmax.min(hc + g * (dc - r).max(0.0));
            for &(a, b) in &sides {
                let d = seg_dist((cx, cy), a, b);
                // along a long side (a ground beside the track runs to the box) the cells grow with the distance from the corners
                want = want.min(he.max(0.15 * (dc - r).max(0.0)) + g * (d - r).max(0.0));
            }
            want
        };
        let size_of = |l: u32| root / (1u64 << l) as f64;
        // split the roots until every cell is as small as it should be
        let mut leaves: HashSet<Cell> = HashSet::new();
        let mut queue: Vec<Cell> = (0..nrx).flat_map(|i| (0..nry).map(move |j| (0, i, j))).collect();
        while let Some((l, i, j)) = queue.pop() {
            let s = size_of(l);
            let (cx, cy) = ((i as f64 + 0.5) * s, (j as f64 + 0.5) * s);
            if l < MAX_LEVEL && s > wanted(cx, cy, s * std::f64::consts::FRAC_1_SQRT_2) {
                queue.extend([(l + 1, 2 * i, 2 * j), (l + 1, 2 * i + 1, 2 * j), (l + 1, 2 * i, 2 * j + 1), (l + 1, 2 * i + 1, 2 * j + 1)]);
            } else {
                leaves.insert((l, i, j));
            }
        }
        balance(&mut leaves, nrx, nry);
        // nodes at the corners of the cells, on the finest grid (level MAX_LEVEL) as integers
        let key = |l: u32, i: u64, j: u64| (i << (MAX_LEVEL - l), j << (MAX_LEVEL - l));
        let mut sorted: Vec<Cell> = leaves.into_iter().collect();
        sorted.sort_unstable_by_key(|&(l, i, j)| {
            let (x, y) = key(l, i, j);
            (y, x, l)
        });
        let mut keys: Vec<(u64, u64)> = sorted.iter().flat_map(|&(l, i, j)| [key(l, i, j), key(l, i + 1, j), key(l, i + 1, j + 1), key(l, i, j + 1)]).collect();
        keys.sort_unstable_by_key(|&(x, y)| (y, x));
        keys.dedup();
        let index: HashMap<(u64, u64), usize> = keys.iter().enumerate().map(|(n, k)| (*k, n)).collect();
        // hanging nodes: in the middle of a side of a cell
        let mut hangs: HashMap<usize, (usize, usize)> = HashMap::new();
        for &(l, i, j) in &sorted {
            let half = 1u64 << (MAX_LEVEL - l - 1).min(63);
            if l == MAX_LEVEL {
                continue;
            }
            let (x0, y0) = key(l, i, j);
            let (x1, y1) = key(l, i + 1, j + 1);
            for (mid, a, b) in [((x0 + half, y0), (x0, y0), (x1, y0)), ((x1, y0 + half), (x1, y0), (x1, y1)), ((x0 + half, y1), (x0, y1), (x1, y1)), ((x0, y0 + half), (x0, y0), (x0, y1))] {
                if let Some(&m) = index.get(&mid) {
                    hangs.insert(m, (index[&a], index[&b]));
                }
            }
        }
        // number the regular nodes first, in the order of the rows (good for the preconditioner)
        let mut number = vec![usize::MAX; keys.len()];
        let mut regular = 0;
        for (n, num) in number.iter_mut().enumerate() {
            if !hangs.contains_key(&n) {
                *num = regular;
                regular += 1;
            }
        }
        let unit = root / (1u64 << MAX_LEVEL) as f64;
        let mut expr: Vec<Option<Vec<(usize, f64)>>> = vec![None; keys.len()];
        fn resolve(n: usize, hangs: &HashMap<usize, (usize, usize)>, number: &[usize], expr: &mut Vec<Option<Vec<(usize, f64)>>>) -> Vec<(usize, f64)> {
            if let Some(e) = &expr[n] {
                return e.clone();
            }
            let e = match hangs.get(&n) {
                None => vec![(number[n], 1.0)],
                Some(&(a, b)) => {
                    let mut e: Vec<(usize, f64)> = vec![];
                    for (q, w) in resolve(a, hangs, number, expr).into_iter().chain(resolve(b, hangs, number, expr)) {
                        match e.iter_mut().find(|x| x.0 == q) {
                            Some(x) => x.1 += 0.5 * w,
                            None => e.push((q, 0.5 * w)),
                        }
                    }
                    e
                }
            };
            expr[n] = Some(e.clone());
            e
        }
        let exprs: Vec<Vec<(usize, f64)>> = (0..keys.len()).map(|n| resolve(n, &hangs, &number, &mut expr)).collect();
        let mut pos = vec![(0.0, 0.0); keys.len()];
        for (n, &(x, y)) in keys.iter().enumerate() {
            pos[n] = (x as f64 * unit, y as f64 * unit);
        }
        // the permittivity of each cell from 8 x 8 samples: across x the samples of a row are in series and the rows side by side,
        // across y the other way round
        const S: usize = 8;
        let cells = sorted
            .iter()
            .map(|&(l, i, j)| {
                let s = size_of(l);
                let (x0, y0) = (i as f64 * s, j as f64 * s);
                let e = |a: usize, b: usize| {
                    let (x, y) = (x0 + (a as f64 + 0.5) * s / S as f64, y0 + (b as f64 + 0.5) * s / S as f64);
                    sh.eps.iter().rev().find(|(r, _)| r.holds(x, y)).map_or(1.0, |(_, e)| *e)
                };
                let grid: Vec<f64> = (0..S * S).map(|k| e(k % S, k / S)).collect();
                let ex = (0..S).map(|b| S as f64 / (0..S).map(|a| 1.0 / grid[b * S + a]).sum::<f64>()).sum::<f64>() / S as f64;
                let ey = (0..S).map(|a| S as f64 / (0..S).map(|b| 1.0 / grid[b * S + a]).sum::<f64>()).sum::<f64>() / S as f64;
                let c = [key(l, i, j), key(l, i + 1, j), key(l, i + 1, j + 1), key(l, i, j + 1)].map(|k| index[&k]);
                (c, s, ex, ey)
            })
            .collect();
        let lookup = sorted.iter().enumerate().map(|(n, c)| (*c, n)).collect();
        Mesh { root, nrx, nry, nodes: regular, pos, regular, expr: exprs, cells, copper: sh.copper.clone(), lookup }
    }

    /// The potential at (x, y) from the potentials of the regular nodes (bilinear in the cell it is in; 0 outside the box).
    pub(super) fn potential(&self, v: &[f64], x: f64, y: f64) -> f64 {
        for l in 0..=MAX_LEVEL {
            let size = self.root / (1u64 << l) as f64;
            let (i, j) = ((x / size).floor(), (y / size).floor());
            if i < 0.0 || j < 0.0 {
                return 0.0;
            }
            if let Some(&c) = self.lookup.get(&(l, i as u64, j as u64)) {
                let (corners, s, _, _) = self.cells[c];
                let (fx, fy) = ((x / s - i).clamp(0.0, 1.0), (y / s - j).clamp(0.0, 1.0));
                let val = |n: usize| self.expr[n].iter().map(|&(q, w)| w * v[q]).sum::<f64>();
                let [a, b, cc, d] = corners.map(val);
                return (a * (1.0 - fx) + b * fx) * (1.0 - fy) + (d * (1.0 - fx) + cc * fx) * fy;
            }
        }
        0.0
    }

    /// `k` (the capacitance per length of one track over ε0) with the materials or in air, the middle at 0 V for the odd mode, and
    /// the potentials of the regular nodes.
    pub(super) fn solve(&self, odd: bool, air: bool) -> (f64, Vec<f64>) {
        let (xdom, ydom) = (self.root * self.nrx as f64, self.root * self.nry as f64);
        let n = self.regular;
        // fixed regular nodes: the box (the middle too for the odd mode) and the copper
        let mut fixed = vec![false; n];
        let mut v = vec![0.0; n];
        for (node, e) in self.expr.iter().enumerate() {
            if e.len() != 1 || e[0].1 != 1.0 {
                continue;
            }
            let q = e[0].0;
            let (x, y) = self.pos[node];
            if y <= 1e-12 || y >= ydom - 1e-9 || x >= xdom - 1e-9 || (odd && x <= 1e-12) {
                fixed[q] = true;
            } else if let Some((_, p)) = self.copper.iter().find(|(r, _)| r.holds(x, y)) {
                fixed[q] = true;
                v[q] = *p;
            }
        }
        // the stiffness of a bilinear square cell (corners bottom left, bottom right, top right, top left), across x and across y
        const KX: [[f64; 4]; 4] = [[2.0, -2.0, -1.0, 1.0], [-2.0, 2.0, 1.0, -1.0], [-1.0, 1.0, 2.0, -2.0], [1.0, -1.0, -2.0, 2.0]];
        const KY: [[f64; 4]; 4] = [[2.0, 1.0, -1.0, -2.0], [1.0, 2.0, -2.0, -1.0], [-1.0, -2.0, 2.0, 1.0], [-2.0, -1.0, 1.0, 2.0]];
        let mut trip: Vec<(usize, usize, f64)> = Vec::with_capacity(self.cells.len() * 20);
        for &(c, _, ex, ey) in &self.cells {
            let (ex, ey) = if air { (1.0, 1.0) } else { (ex, ey) };
            for a in 0..4 {
                for b in 0..4 {
                    let kab = (ex * KX[a][b] + ey * KY[a][b]) / 6.0;
                    for &(p, wp) in &self.expr[c[a]] {
                        for &(q, wq) in &self.expr[c[b]] {
                            trip.push((p, q, kab * wp * wq));
                        }
                    }
                }
            }
        }
        trip.sort_unstable_by_key(|t| (t.0, t.1));
        // compressed rows
        let mut start = vec![0usize; n + 1];
        let mut cols: Vec<usize> = Vec::with_capacity(trip.len() / 3);
        let mut vals: Vec<f64> = Vec::with_capacity(trip.len() / 3);
        let mut last = (usize::MAX, usize::MAX);
        for (p, q, x) in trip {
            if (p, q) == last {
                *vals.last_mut().unwrap() += x;
            } else {
                cols.push(q);
                vals.push(x);
                start[p + 1] = cols.len();
                last = (p, q);
            }
        }
        for r in 1..=n {
            start[r] = start[r].max(start[r - 1]);
        }
        let row = |p: usize| start[p]..start[p + 1];
        let diag: Vec<f64> = (0..n).map(|p| row(p).find(|&k| cols[k] == p).map_or(1.0, |k| vals[k])).collect();
        // the free nodes: b = -K_fixed · v
        let mut b = vec![0.0; n];
        for p in (0..n).filter(|&p| !fixed[p]) {
            b[p] = -row(p).filter(|&k| fixed[cols[k]]).map(|k| vals[k] * v[cols[k]]).sum::<f64>();
        }
        let apply = |x: &[f64], y: &mut [f64]| {
            for p in 0..n {
                y[p] = if fixed[p] { 0.0 } else { row(p).filter(|&k| !fixed[cols[k]]).map(|k| vals[k] * x[cols[k]]).sum() };
            }
        };
        // symmetric Gauss-Seidel: (D + L) D⁻¹ (D + U)
        let precond = |r: &[f64], z: &mut [f64]| {
            for p in 0..n {
                if fixed[p] {
                    z[p] = 0.0;
                    continue;
                }
                let s: f64 = row(p).filter(|&k| cols[k] < p && !fixed[cols[k]]).map(|k| vals[k] * z[cols[k]]).sum();
                z[p] = (r[p] - s) / diag[p];
            }
            for p in (0..n).rev() {
                if fixed[p] {
                    continue;
                }
                let s: f64 = row(p).filter(|&k| cols[k] > p && !fixed[cols[k]]).map(|k| vals[k] * z[cols[k]]).sum();
                z[p] -= s / diag[p];
            }
        };
        let mut x = vec![0.0; n];
        let mut r = b.clone();
        let bn = b.iter().map(|x| x * x).sum::<f64>().sqrt().max(1e-300);
        let mut z = vec![0.0; n];
        precond(&r, &mut z);
        let mut d = z.clone();
        let mut rz: f64 = r.iter().zip(&z).map(|(a, b)| a * b).sum();
        let mut q = vec![0.0; n];
        for _ in 0..20_000 {
            if r.iter().map(|x| x * x).sum::<f64>().sqrt() <= 1e-9 * bn {
                break;
            }
            apply(&d, &mut q);
            let dq: f64 = d.iter().zip(&q).map(|(a, b)| a * b).sum();
            if dq <= 0.0 {
                break;
            }
            let alpha = rz / dq;
            for p in 0..n {
                x[p] += alpha * d[p];
                r[p] -= alpha * q[p];
            }
            precond(&r, &mut z);
            let rz2: f64 = r.iter().zip(&z).map(|(a, b)| a * b).sum();
            let beta = rz2 / rz;
            rz = rz2;
            for p in 0..n {
                d[p] = z[p] + beta * d[p];
            }
        }
        for p in 0..n {
            if !fixed[p] {
                v[p] = x[p];
            }
        }
        // the energy of the half: ½ vᵀ K v
        let mut w = 0.0;
        for p in 0..n {
            w += v[p] * row(p).map(|k| vals[k] * v[cols[k]]).sum::<f64>();
        }
        w *= 0.5;
        let lines = if self.copper.iter().any(|(r, p)| *p > 0.0 && r.x0 > 0.0) { 2.0 } else { 1.0 };
        (2.0 * (2.0 * w) / lines, v)
    }
}

/// Splits cells until neighbours across a side differ by at most one level.
fn balance(leaves: &mut HashSet<Cell>, nrx: u64, nry: u64) {
    loop {
        let mut split: HashSet<Cell> = HashSet::new();
        for &(l, i, j) in leaves.iter() {
            let (w, h) = (nrx << l, nry << l);
            let around = [(i.wrapping_add(1), j), (i.wrapping_sub(1), j), (i, j.wrapping_add(1)), (i, j.wrapping_sub(1))];
            for (ni, nj) in around {
                if ni >= w || nj >= h {
                    continue;
                }
                // the leaf over the neighbour of the same size: itself, or a bigger one
                for m in (0..=l).rev() {
                    let c = (m, ni >> (l - m), nj >> (l - m));
                    if leaves.contains(&c) {
                        if m + 1 < l {
                            split.insert(c);
                        }
                        break;
                    }
                }
            }
        }
        if split.is_empty() {
            return;
        }
        for (l, i, j) in split {
            leaves.remove(&(l, i, j));
            leaves.extend([(l + 1, 2 * i, 2 * j), (l + 1, 2 * i + 1, 2 * j), (l + 1, 2 * i, 2 * j + 1), (l + 1, 2 * i + 1, 2 * j + 1)]);
        }
    }
}
