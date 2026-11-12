//! A sparse LU factorisation with threshold pivoting.

use super::*;

/// A sparse LU factorisation, kept so that systems with the same matrix and a new right-hand
/// side (a linear transient with a fixed step) cost only a replay of the row operations.
pub(crate) struct Factor {
    n: usize,
    /// `pivot_row[k]`: the row that eliminates column k.
    pivot_row: Vec<usize>,
    /// Row operations in elimination order: `b[r] -= f * b[p]`.
    ops: Vec<(usize, usize, f64)>,
    /// The eliminated rows; `u[pivot_row[k]]` holds only columns >= k.
    u: Vec<Vec<(usize, f64)>>,
    /// The unknown eliminated k-th (empty: the natural order). The columns of `u` are positions in this order.
    perm: Vec<usize>,
    /// A refactorisation (`refactor`) keeps its eliminated rows in one array instead of `u` (which is then empty): the values of
    /// `Flat::u_col` of its plan, which also has the pivot rows and the order (`pivot_row` and `perm` are then empty too).
    flat: Option<(std::rc::Rc<Symbolic>, Vec<f64>)>,
}

/// What a factorisation decided, kept for the next matrix with the same sparsity pattern (every Newton iteration of a
/// circuit has one): the pivot rows, which earlier pivot rows each row is combined with, and the final pattern of the
/// eliminated rows. `refactor` then needs no searching and no allocation of rows.
pub(crate) struct Symbolic {
    n: usize,
    /// The columns of every input row (in the original numbering): the pattern this plan is valid for.
    pattern: Vec<Vec<usize>>,
    /// The unknown eliminated k-th (empty: the natural order) and the position of every unknown in that order.
    perm: Vec<usize>,
    pos: Vec<usize>,
    pivot_row: Vec<usize>,
    /// For every row `r`: the `(k, pivot row)` pairs it is combined with, in increasing `k`.
    row_ops: Vec<Vec<(usize, usize)>>,
    /// For every pivot row: the columns of its eliminated row (starting with its own pivot column).
    u_cols: Vec<Vec<usize>>,
    /// The same plan in flat arrays, for `refactor` (made once the order is known).
    flat: Flat,
}

/// A `Symbolic` plan laid out in flat arrays, so that a refactorisation walks a few contiguous lists instead of following a
/// separate `Vec` per row for the input, its row operations and its eliminated row (that was 50 ns a row, half of a Newton
/// iteration of a 436-unknown post-layout circuit). Everything is indexed by the input row `r` through `*_start[r]..*_start[r + 1]`.
#[derive(Default)]
struct Flat {
    /// The columns of the input row (original numbering, to check the pattern) and where each value goes (its position in the order).
    pat_start: Vec<usize>,
    pat_col: Vec<usize>,
    dst: Vec<usize>,
    /// The row operations of the row: the column they clear and the pivot row of that column.
    op_start: Vec<usize>,
    op_kk: Vec<usize>,
    op_q: Vec<usize>,
    /// The columns of the eliminated row (its pivot column first); the factor's values follow the same layout.
    u_start: Vec<usize>,
    u_col: Vec<usize>,
    /// The columns before the pivot column that the row's values reach and no row operation clears: they must come out zero,
    /// or the plan (made when such an entry was exactly zero) does not hold for these values.
    chk_start: Vec<usize>,
    chk_col: Vec<usize>,
}

impl Flat {
    fn new(sym: &Symbolic) -> Flat {
        let n = sym.n;
        let map = |c: usize| if sym.perm.is_empty() { c } else { sym.pos[c] };
        let mut step = vec![0usize; n];
        for (k, &p) in sym.pivot_row.iter().enumerate() {
            step[p] = k;
        }
        let mut f = Flat::default();
        let mut seen = vec![false; n];
        for (r, pattern) in sym.pattern.iter().enumerate() {
            f.pat_start.push(f.pat_col.len());
            f.op_start.push(f.op_kk.len());
            f.u_start.push(f.u_col.len());
            f.chk_start.push(f.chk_col.len());
            for &c in pattern {
                f.pat_col.push(c);
                f.dst.push(map(c));
            }
            for &(kk, q) in &sym.row_ops[r] {
                f.op_kk.push(kk);
                f.op_q.push(q);
            }
            f.u_col.extend_from_slice(&sym.u_cols[r]);
            // the row's pivot column is the first of its eliminated row (the step that eliminates it when it has none)
            let first_col = sym.u_cols[r].first().copied().unwrap_or(step[r]);
            let cleared: Vec<usize> = sym.row_ops[r].iter().map(|e| e.0).collect();
            let reached = pattern.iter().map(|&c| map(c)).chain(sym.row_ops[r].iter().flat_map(|&(_, q)| sym.u_cols[q].iter().skip(1).copied()));
            for c in reached {
                if c < first_col && !cleared.contains(&c) && !seen[c] {
                    seen[c] = true;
                    f.chk_col.push(c);
                }
            }
            for &c in &f.chk_col[f.chk_start[r]..] {
                seen[c] = false;
            }
        }
        f.pat_start.push(f.pat_col.len());
        f.op_start.push(f.op_kk.len());
        f.u_start.push(f.u_col.len());
        f.chk_start.push(f.chk_col.len());
        f
    }
}

impl Symbolic {
    /// The plan for a given pivot order, worked out from the pattern alone: an entry that happens to be zero is still an entry,
    /// so the plan holds for any values (the numeric factorisation skips what it does not need).
    fn plan(n: usize, pattern: Vec<Vec<usize>>, pivot_row: Vec<usize>) -> Symbolic {
        let mut cols = pattern.clone();
        let mut col_rows: Vec<Vec<usize>> = vec![vec![]; n];
        for (r, row) in cols.iter().enumerate() {
            for &c in row {
                col_rows[c].push(r);
            }
        }
        let mut done = vec![false; n];
        let mut row_ops: Vec<Vec<(usize, usize)>> = vec![vec![]; n];
        let mut u_cols: Vec<Vec<usize>> = vec![vec![]; n];
        for (k, &p) in pivot_row.iter().enumerate() {
            done[p] = true;
            let prow: Vec<usize> = cols[p].iter().copied().filter(|c| *c >= k).collect();
            let mut targets = std::mem::take(&mut col_rows[k]);
            targets.sort_unstable();
            targets.dedup();
            for r in targets {
                if done[r] || cols[r].binary_search(&k).is_err() {
                    continue;
                }
                row_ops[r].push((k, p));
                let mut merged: Vec<usize> = Vec::with_capacity(cols[r].len() + prow.len());
                let (mut i, mut j) = (0, 0);
                let row = &cols[r];
                while i < row.len() || j < prow.len() {
                    let ci = row.get(i).copied().unwrap_or(usize::MAX);
                    let cj = prow.get(j).copied().unwrap_or(usize::MAX);
                    if ci == k {
                        i += 1;
                    } else if ci < cj {
                        merged.push(ci);
                        i += 1;
                    } else if cj < ci {
                        if cj != k {
                            merged.push(cj);
                            col_rows[cj].push(r);
                        }
                        j += 1;
                    } else {
                        merged.push(ci);
                        i += 1;
                        j += 1;
                    }
                }
                cols[r] = merged;
            }
            u_cols[p] = prow;
        }
        Symbolic { n, pattern, perm: vec![], pos: vec![], pivot_row, row_ops, u_cols, flat: Flat::default() }
    }
}

/// The work lists of the last `factor_ordered` on this thread, kept for their capacity: a small system is factored from scratch
/// at every Newton iteration, and allocating these was a fifth of the time of a small circuit's transient.
/// `pivot_row` and `ops` go into the `Factor` and come back when it is done with (`Factor::into_rows`); `solve_x` is the
/// work vector of `Factor::solve`.
#[derive(Default)]
struct Scratch {
    col_rows: Vec<Vec<usize>>,
    merged: Vec<(usize, f64)>,
    cand: Vec<(usize, f64)>,
    done: Vec<bool>,
    pivot_row: Vec<usize>,
    ops: Vec<(usize, usize, f64)>,
    solve_x: Vec<f64>,
}

/// The `pivot_row` and `ops` buffers kept on this thread, emptied, for a new factor of `n` unknowns.
fn take_factor_buffers(n: usize) -> (Vec<usize>, Vec<(usize, usize, f64)>) {
    SCRATCH.with(|c| {
        let mut s = c.borrow_mut();
        let (mut p, mut o) = (std::mem::take(&mut s.pivot_row), std::mem::take(&mut s.ops));
        p.clear();
        p.resize(n, 0);
        o.clear();
        (p, o)
    })
}

thread_local! {
    static SCRATCH: std::cell::RefCell<Scratch> = std::cell::RefCell::new(Scratch::default());
}

/// Factor a sparse matrix given as rows of (column, value), sorted by column with no duplicates.
/// Gaussian elimination column by column; the pivot is the sparsest row among those within a
/// tenth of the largest entry (threshold pivoting), which keeps fill-in small.
pub(super) fn factor(n: usize, rows: Vec<Vec<(usize, f64)>>) -> Result<Factor, SimError> {
    factor_planned(n, rows, false).map(|(f, _)| f)
}

/// `factor`, and the plan for `refactor`.
#[cfg(test)]
pub(super) fn factor_sym(n: usize, rows: Vec<Vec<(usize, f64)>>) -> Result<(Factor, Symbolic), SimError> {
    factor_planned(n, rows, true).map(|(f, sym)| (f, sym.expect("the plan was asked for")))
}

/// `factor_sym`, with the plan only when `want_plan`: the plan repeats the whole elimination on the pattern, which is wasted when
/// the caller will never refactor (a small system is always factored from scratch).
pub(super) fn factor_planned(n: usize, rows: Vec<Vec<(usize, f64)>>, want_plan: bool) -> Result<(Factor, Option<Symbolic>), SimError> {
    factor_ordered(n, rows, want_plan, None)
}

/// `factor_planned`; `earlier`: the plan of an earlier factorisation, whose elimination order is taken when the pattern is the
/// same (the order depends on the pattern alone, so it comes out the same, without the search for it).
pub(super) fn factor_ordered(n: usize, mut rows: Vec<Vec<(usize, f64)>>, want_plan: bool, earlier: Option<&Symbolic>) -> Result<(Factor, Option<Symbolic>), SimError> {
    const THRESHOLD: f64 = 0.1;
    let orig_pattern: Vec<Vec<usize>> = if want_plan || n >= ORDER_MIN_SIZE { rows.iter().map(|r| r.iter().map(|e| e.0).collect()).collect() } else { vec![] };
    // A big system is eliminated in a minimum-degree order of its unknowns: in the natural order a circuit of a few hundred nodes
    // fills in to fifteen times its entries, and the factorisation is most of the time of a DC solve. A small one keeps the natural
    // order (a latch there is sensitive to the pivots chosen).
    let (perm, pos) = if n >= ORDER_MIN_SIZE {
        let reuse = earlier.filter(|s| s.n == n && !s.perm.is_empty() && s.pattern == orig_pattern);
        let perm = match reuse {
            Some(s) => s.perm.clone(),
            None => min_degree_order(n, &orig_pattern),
        };
        let mut pos = vec![0usize; n];
        for (k, &c) in perm.iter().enumerate() {
            pos[c] = k;
        }
        rows = permute_columns(&rows, &pos);
        (perm, pos)
    } else {
        (vec![], vec![])
    };
    let pattern: Vec<Vec<usize>> = if want_plan { rows.iter().map(|r| r.iter().map(|e| e.0).collect()).collect() } else { vec![] };
    // rows that have (or once had) an entry in each column; the lists of the last factorisation are filled again (a small system
    // is factored from scratch at every Newton iteration, and these were most of its allocations)
    let (mut pivot_row, mut ops) = take_factor_buffers(n);
    let (mut col_rows, mut merged, mut cand, mut done) = SCRATCH.with(|c| {
        let mut s = c.borrow_mut();
        (std::mem::take(&mut s.col_rows), std::mem::take(&mut s.merged), std::mem::take(&mut s.cand), std::mem::take(&mut s.done))
    });
    col_rows.iter_mut().for_each(Vec::clear);
    col_rows.resize_with(n, Vec::new);
    for (r, row) in rows.iter().enumerate() {
        for &(c, _) in row {
            col_rows[c].push(r);
        }
    }
    done.clear();
    done.resize(n, false);
    merged.clear();
    cand.clear();
    for k in 0..n {
        let mut best = 0.0f64;
        cand.clear();
        for &r in &col_rows[k] {
            if done[r] {
                continue;
            }
            if let Ok(i) = rows[r].binary_search_by_key(&k, |e| e.0) {
                let v = rows[r][i].1;
                if v != 0.0 && !cand.iter().any(|c| c.0 == r) {
                    best = best.max(v.abs());
                    cand.push((r, v));
                }
            }
        }
        if best < 1e-20 || !best.is_finite() {
            return Err(SimError::Singular);
        }
        let &(p, pv) = cand
            .iter()
            .filter(|(_, v)| v.abs() >= THRESHOLD * best)
            .min_by(|x, y| rows[x.0].len().cmp(&rows[y.0].len()).then(y.1.abs().total_cmp(&x.1.abs())))
            .expect("the largest entry passes the threshold");
        done[p] = true;
        pivot_row[k] = p;
        let prow = std::mem::take(&mut rows[p]);
        let jstart = prow.partition_point(|e| e.0 <= k);
        for &(r, v) in &cand {
            if r == p {
                continue;
            }
            let f = v / pv;
            // row r -= f * pivot row (columns after k); the entry in column k disappears
            merged.clear();
            let row = &rows[r];
            let (mut i, mut j) = (0, jstart);
            while i < row.len() || j < prow.len() {
                let ci = row.get(i).map_or(usize::MAX, |e| e.0);
                let cj = prow.get(j).map_or(usize::MAX, |e| e.0);
                if ci == k {
                    i += 1;
                } else if ci < cj {
                    merged.push(row[i]);
                    i += 1;
                } else if cj < ci {
                    merged.push((cj, -f * prow[j].1));
                    col_rows[cj].push(r);
                    j += 1;
                } else {
                    merged.push((ci, row[i].1 - f * prow[j].1));
                    i += 1;
                    j += 1;
                }
            }
            // the merged row takes the place of the old one, whose buffer becomes the next `merged`
            std::mem::swap(&mut rows[r], &mut merged);
            ops.push((r, p, f));
        }
        rows[p] = prow;
    }
    SCRATCH.with(|c| {
        let mut s = c.borrow_mut();
        (s.col_rows, s.merged, s.cand, s.done) = (col_rows, merged, cand, done);
    });
    let sym = want_plan.then(|| {
        let mut sym = Symbolic::plan(n, pattern, pivot_row.clone());
        sym.pattern = orig_pattern;
        sym.perm = perm.clone();
        sym.pos = pos;
        sym.flat = Flat::new(&sym);
        sym
    });
    Ok((Factor { n, pivot_row, ops, u: rows, perm, flat: None }, sym))
}

/// The numeric factorisation of a matrix with the pattern of `sym`, in its pivot order: row by row, each row combined with
/// the earlier pivot rows. `None` when the pattern differs or a pivot has become small compared with its row (the caller then
/// factors from scratch and makes a new plan).
/// `spare`: the values of an earlier factor (`Factor::into_values`), filled again instead of allocating new ones.
pub(super) fn refactor(sym: &std::rc::Rc<Symbolic>, rows: &[Vec<(usize, f64)>], spare: Vec<f64>) -> Option<Factor> {
    let n = sym.n;
    let fl = &sym.flat;
    if rows.len() != n
        || rows.iter().enumerate().any(|(r, row)| row.len() != fl.pat_start[r + 1] - fl.pat_start[r] || row.iter().zip(&fl.pat_col[fl.pat_start[r]..]).any(|(e, c)| e.0 != *c))
    {
        return None;
    }
    // The columns are scattered into `w` below, in whatever order the row lists them, so the permuted numbering is applied on
    // the way in instead of building a permuted, re-sorted copy of every row.
    let mut w = vec![0.0f64; n];
    let mut vals = spare;
    vals.clear();
    vals.resize(fl.u_col.len(), 0.0);
    let (_, mut ops) = take_factor_buffers(0);
    for &p in &sym.pivot_row {
        for (&(_, v), &c) in rows[p].iter().zip(&fl.dst[fl.pat_start[p]..]) {
            w[c] = v;
        }
        for o in fl.op_start[p]..fl.op_start[p + 1] {
            let (kk, q) = (fl.op_kk[o], fl.op_q[o]);
            let (q0, q1) = (fl.u_start[q], fl.u_start[q + 1]);
            let f = w[kk] / vals[q0];
            w[kk] = 0.0;
            if f != 0.0 {
                for (&c, &v) in fl.u_col[q0 + 1..q1].iter().zip(&vals[q0 + 1..q1]) {
                    w[c] -= f * v;
                }
                ops.push((p, q, f));
            }
        }
        // The plan came from a factorisation that skipped entries that were exactly zero then. An entry in an already
        // eliminated column that is not zero now was never combined away: the plan does not hold for these values.
        if fl.chk_col[fl.chk_start[p]..fl.chk_start[p + 1]].iter().any(|&c| w[c] != 0.0) {
            return None;
        }
        let (p0, p1) = (fl.u_start[p], fl.u_start[p + 1]);
        for (&c, v) in fl.u_col[p0..p1].iter().zip(&mut vals[p0..p1]) {
            *v = w[c];
            w[c] = 0.0;
        }
        let diag = if p1 > p0 { vals[p0] } else { 0.0 };
        // (the accuracy of the answer is checked by the caller: see `residual_ok`)
        if !diag.is_finite() || diag.abs() < 1e-30 {
            return None;
        }
    }
    Some(Factor { n, pivot_row: vec![], ops, u: vec![], perm: vec![], flat: Some((sym.clone(), vals)) })
}

impl Factor {
    /// The row buffers of the eliminated matrix (row `r` of the input is `u[r]`), for the next assembly to fill. The factor's
    /// other buffers go back to this thread's `Scratch` for the next factorisation.
    pub(super) fn into_rows(self) -> Vec<Vec<(usize, f64)>> {
        SCRATCH.with(|c| {
            let mut s = c.borrow_mut();
            s.pivot_row = self.pivot_row;
            s.ops = self.ops;
        });
        self.u
    }

    /// The values of a refactorisation (empty for a full factorisation), for the next `refactor` to fill. The row operations go
    /// back to this thread's `Scratch`.
    pub(super) fn into_values(self) -> Vec<f64> {
        SCRATCH.with(|c| c.borrow_mut().ops = self.ops);
        self.flat.map(|f| f.1).unwrap_or_default()
    }

    /// Solve A x = b; `b` holds the solution on return.
    pub(super) fn solve(&self, b: &mut [f64]) {
        for &(r, p, f) in &self.ops {
            b[r] -= f * b[p];
        }
        // every x[k] is written below before it is read (only x[c] with c > k is read), so the old contents do not matter
        let mut x = SCRATCH.with(|c| std::mem::take(&mut c.borrow_mut().solve_x));
        x.resize(self.n, 0.0);
        // x[k] from the eliminated row of step k, its columns and values in order
        fn back(k: usize, mut sum: f64, row: impl Iterator<Item = (usize, f64)>, x: &[f64]) -> f64 {
            let mut diag = 0.0;
            for (c, v) in row {
                if c == k {
                    diag = v;
                } else if c > k {
                    sum -= v * x[c];
                }
            }
            sum / diag
        }
        let perm = match &self.flat {
            Some((sym, vals)) => {
                let fl = &sym.flat;
                for k in (0..self.n).rev() {
                    let p = sym.pivot_row[k];
                    let (p0, p1) = (fl.u_start[p], fl.u_start[p + 1]);
                    x[k] = back(k, b[p], fl.u_col[p0..p1].iter().copied().zip(vals[p0..p1].iter().copied()), &x);
                }
                &sym.perm
            }
            None => {
                for k in (0..self.n).rev() {
                    let p = self.pivot_row[k];
                    x[k] = back(k, b[p], self.u[p].iter().copied(), &x);
                }
                &self.perm
            }
        };
        if perm.is_empty() {
            b.copy_from_slice(&x[..self.n]);
        } else {
            for (k, &v) in x[..self.n].iter().enumerate() {
                b[perm[k]] = v;
            }
        }
        SCRATCH.with(|c| c.borrow_mut().solve_x = x);
    }
}

/// Systems with at least this many unknowns are eliminated in a minimum-degree order.
const ORDER_MIN_SIZE: usize = 60;

/// `rows` with every column index `c` replaced by `pos[c]`, each row sorted again.
fn permute_columns(rows: &[Vec<(usize, f64)>], pos: &[usize]) -> Vec<Vec<(usize, f64)>> {
    rows.iter()
        .map(|row| {
            let mut r: Vec<(usize, f64)> = row.iter().map(|&(c, v)| (pos[c], v)).collect();
            r.sort_unstable_by_key(|e| e.0);
            r
        })
        .collect()
}

/// A minimum-degree elimination order of the unknowns, from the pattern of A + Aᵀ: the unknown with the fewest neighbours goes first
/// (its neighbours then become neighbours of each other, which is the fill-in the order tries to keep small). Ties go to the lower
/// index, so the order does not depend on anything but the pattern.
fn min_degree_order(n: usize, pattern: &[Vec<usize>]) -> Vec<usize> {
    use std::collections::HashSet;
    let mut adj: Vec<HashSet<usize>> = vec![HashSet::new(); n];
    for (r, row) in pattern.iter().enumerate() {
        for &c in row {
            if c != r {
                adj[r].insert(c);
                adj[c].insert(r);
            }
        }
    }
    let mut alive = vec![true; n];
    let mut order = Vec::with_capacity(n);
    for _ in 0..n {
        let v = (0..n).filter(|&i| alive[i]).min_by_key(|&i| (adj[i].len(), i)).expect("one unknown is left");
        alive[v] = false;
        order.push(v);
        let mut nb: Vec<usize> = adj[v].drain().collect();
        nb.sort_unstable();
        for &a in &nb {
            adj[a].remove(&v);
        }
        for i in 0..nb.len() {
            for j in i + 1..nb.len() {
                if adj[nb[i]].insert(nb[j]) {
                    adj[nb[j]].insert(nb[i]);
                }
            }
        }
    }
    order
}

/// `b - rows * x`.
pub(super) fn residual(rows: &[Vec<(usize, f64)>], x: &[f64], b: &[f64]) -> Vec<f64> {
    rows.iter().zip(b).map(|(row, b)| b - row.iter().map(|&(c, v)| v * x[c]).sum::<f64>()).collect()
}

/// Is `x` a solution of `rows * x = b` to rounding level? (A refactorisation in an old pivot order can lose digits when
/// a pivot has become small; this tells.)
pub(super) fn residual_ok(rows: &[Vec<(usize, f64)>], x: &[f64], b: &[f64]) -> bool {
    rows.iter().zip(b).all(|(row, b)| {
        let (mut sum, mut scale) = (0.0f64, b.abs());
        for &(c, v) in row {
            let t = v * x[c];
            sum += t;
            scale += t.abs();
        }
        (sum - b).abs() <= 1e-8 * scale + 1e-30
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows_of(m: &[Vec<f64>]) -> Vec<Vec<(usize, f64)>> {
        m.iter().map(|r| r.iter().enumerate().filter(|(_, v)| **v != 0.0).map(|(c, v)| (c, *v)).collect()).collect()
    }

    fn solve_with(f: &Factor, b: &[f64]) -> Vec<f64> {
        let mut x = b.to_vec();
        f.solve(&mut x);
        x
    }

    #[test]
    fn refactor_gives_the_same_answer_as_a_full_factorisation() {
        // a small matrix with fill-in and a voltage-source style row (zero diagonal)
        let a = vec![
            vec![2.0, -1.0, 0.0, 0.0, 1.0],
            vec![-1.0, 3.0, -1.0, 0.0, 0.0],
            vec![0.0, -1.0, 2.0, -1.0, 0.0],
            vec![0.0, 0.0, -1.0, 4.0, 1.0],
            vec![1.0, 0.0, 0.0, 1.0, 0.0],
        ];
        let b = [1.0, 2.0, 3.0, 4.0, 5.0];
        let (f1, sym) = factor_sym(5, rows_of(&a)).unwrap();
        let sym = std::rc::Rc::new(sym);
        let x1 = solve_with(&f1, &b);
        let f2 = refactor(&sym, &rows_of(&a), vec![]).expect("the same matrix must refactor");
        let x2 = solve_with(&f2, &b);
        for (p, q) in x1.iter().zip(&x2) {
            assert!((p - q).abs() < 1e-12, "{x1:?} vs {x2:?}");
        }
        // other values, same pattern
        let mut a2 = a.clone();
        a2[0][0] = 3.5;
        a2[3][3] = 1.5;
        a2[1][2] = -0.5;
        let (g1, _) = factor_sym(5, rows_of(&a2)).unwrap();
        // into the rows of the earlier factor (as the Newton iteration does): bit for bit what fresh rows give
        let g2 = refactor(&sym, &rows_of(&a2), f2.into_values()).expect("new values refactor");
        let g3 = refactor(&sym, &rows_of(&a2), vec![]).expect("new values refactor");
        assert_eq!(solve_with(&g2, &b), solve_with(&g3, &b));
        for (p, q) in solve_with(&g1, &b).iter().zip(&solve_with(&g2, &b)) {
            assert!((p - q).abs() < 1e-9);
        }
    }

    /// A system big enough to be eliminated in the minimum-degree order: the answer solves the equations, a refactorisation with
    /// other values of the same pattern does too, and the order keeps the fill-in far below that of the natural order.
    #[test]
    fn a_big_system_is_solved_in_minimum_degree_order_with_little_fill() {
        let n = 200;
        let mut seed = 12345u64;
        let mut rnd = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f64) / (1u64 << 31) as f64
        };
        // a ring of nodes plus a few long links and one hub (ground-like) node connected to every tenth node
        let mut m = vec![vec![0.0; n]; n];
        for i in 0..n {
            m[i][i] = 5.0 + rnd();
            let j = (i + 1) % n;
            let g = -1.0 - rnd();
            m[i][j] = g;
            m[j][i] = g;
            if i % 10 == 0 {
                let h = -0.5 - rnd();
                m[i][n - 1] += h;
                m[n - 1][i] += h;
                m[n - 1][n - 1] += 3.0;
            }
        }
        for k in 0..20 {
            let (a, b) = ((k * 37) % n, (k * 91 + 13) % n);
            if a != b {
                m[a][b] -= 0.3;
                m[b][a] -= 0.3;
            }
        }
        let b: Vec<f64> = (0..n).map(|i| (i % 7) as f64 - 3.0).collect();
        let rows = rows_of(&m);
        let (f, sym) = factor_sym(n, rows.clone()).unwrap();
        assert!(!sym.perm.is_empty(), "a system of {n} unknowns is ordered");
        let x = solve_with(&f, &b);
        assert!(residual_ok(&rows, &x, &b), "the answer solves the equations");
        let nnz: usize = rows.iter().map(|r| r.len()).sum();
        let fill: usize = f.u.iter().map(|r| r.len()).sum();
        assert!(fill < 8 * nnz, "fill-in {fill} for {nnz} entries");
        // the same pattern with other values
        let mut m2 = m.clone();
        for i in 0..n {
            m2[i][i] += 1.0;
        }
        let rows2 = rows_of(&m2);
        let sym = std::rc::Rc::new(sym);
        let g = refactor(&sym, &rows2, vec![]).expect("the pattern is the same");
        let x2 = solve_with(&g, &b);
        assert!(residual_ok(&rows2, &x2, &b));
    }
}
