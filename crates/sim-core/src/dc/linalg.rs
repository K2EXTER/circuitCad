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
        Symbolic { n, pattern, perm: vec![], pos: vec![], pivot_row, row_ops, u_cols }
    }
}

/// Factor a sparse matrix given as rows of (column, value), sorted by column with no duplicates.
/// Gaussian elimination column by column; the pivot is the sparsest row among those within a
/// tenth of the largest entry (threshold pivoting), which keeps fill-in small.
pub(super) fn factor(n: usize, rows: Vec<Vec<(usize, f64)>>) -> Result<Factor, SimError> {
    factor_sym(n, rows).map(|(f, _)| f)
}

/// `factor`, and the plan for `refactor`.
pub(super) fn factor_sym(n: usize, mut rows: Vec<Vec<(usize, f64)>>) -> Result<(Factor, Symbolic), SimError> {
    const THRESHOLD: f64 = 0.1;
    let orig_pattern: Vec<Vec<usize>> = rows.iter().map(|r| r.iter().map(|e| e.0).collect()).collect();
    // A big system is eliminated in a minimum-degree order of its unknowns: in the natural order a circuit of a few hundred nodes
    // fills in to fifteen times its entries, and the factorisation is most of the time of a DC solve. A small one keeps the natural
    // order (a latch there is sensitive to the pivots chosen).
    let (perm, pos) = if n >= ORDER_MIN_SIZE {
        let perm = min_degree_order(n, &orig_pattern);
        let mut pos = vec![0usize; n];
        for (k, &c) in perm.iter().enumerate() {
            pos[c] = k;
        }
        rows = permute_columns(&rows, &pos);
        (perm, pos)
    } else {
        (vec![], vec![])
    };
    let pattern: Vec<Vec<usize>> = rows.iter().map(|r| r.iter().map(|e| e.0).collect()).collect();
    // rows that have (or once had) an entry in each column
    let mut col_rows: Vec<Vec<usize>> = vec![vec![]; n];
    for (r, row) in rows.iter().enumerate() {
        for &(c, _) in row {
            col_rows[c].push(r);
        }
    }
    let mut done = vec![false; n];
    let mut pivot_row = vec![0usize; n];
    let mut ops = vec![];
    let mut merged: Vec<(usize, f64)> = vec![];
    let mut cand: Vec<(usize, f64)> = vec![];
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
            rows[r].clear();
            rows[r].extend_from_slice(&merged);
            ops.push((r, p, f));
        }
        rows[p] = prow;
    }
    let mut sym = Symbolic::plan(n, pattern, pivot_row.clone());
    sym.pattern = orig_pattern;
    sym.perm = perm.clone();
    sym.pos = pos;
    Ok((Factor { n, pivot_row, ops, u: rows, perm }, sym))
}

/// The numeric factorisation of a matrix with the pattern of `sym`, in its pivot order: row by row, each row combined with
/// the earlier pivot rows. `None` when the pattern differs or a pivot has become small compared with its row (the caller then
/// factors from scratch and makes a new plan).
pub(super) fn refactor(sym: &Symbolic, rows: &[Vec<(usize, f64)>]) -> Option<Factor> {
    let n = sym.n;
    let permuted;
    let mut rows = rows;
    if rows.len() != n || rows.iter().zip(&sym.pattern).any(|(r, p)| r.len() != p.len() || r.iter().zip(p).any(|(e, c)| e.0 != *c)) {
        return None;
    }
    if !sym.perm.is_empty() {
        permuted = permute_columns(rows, &sym.pos);
        rows = &permuted;
    }
    let mut w = vec![0.0f64; n];
    let mut u: Vec<Vec<(usize, f64)>> = vec![vec![]; n];
    let mut ops: Vec<(usize, usize, f64)> = Vec::with_capacity(sym.row_ops.iter().map(|o| o.len()).sum());
    let mut touched: Vec<usize> = vec![];
    for k in 0..n {
        let p = sym.pivot_row[k];
        touched.clear();
        for &(c, v) in &rows[p] {
            w[c] = v;
            touched.push(c);
        }
        for &(kk, q) in &sym.row_ops[p] {
            let f = w[kk] / u[q][0].1;
            w[kk] = 0.0;
            if f != 0.0 {
                for &(c, v) in &u[q][1..] {
                    w[c] -= f * v;
                    touched.push(c);
                }
                ops.push((p, q, f));
            }
        }
        // The plan came from a factorisation that skipped entries that were exactly zero then. An entry in an already
        // eliminated column that is not zero now was never combined away: the plan does not hold for these values.
        let cols = &sym.u_cols[p];
        let first_col = cols.first().copied().unwrap_or(k);
        if touched.iter().any(|&c| c < first_col && w[c] != 0.0) {
                return None;
        }
        let cols = &sym.u_cols[p];
        let mut row = Vec::with_capacity(cols.len());
        for &c in cols {
            row.push((c, w[c]));
            w[c] = 0.0;
        }
        let diag = row.first().map_or(0.0, |e| e.1);
        // (the accuracy of the answer is checked by the caller: see `residual_ok`)
        if !diag.is_finite() || diag.abs() < 1e-30 {
                return None;
        }
        u[p] = row;
    }
    Some(Factor { n, pivot_row: sym.pivot_row.clone(), ops, u, perm: sym.perm.clone() })
}

impl Factor {
    /// Solve A x = b; `b` holds the solution on return.
    pub(super) fn solve(&self, b: &mut [f64]) {
        for &(r, p, f) in &self.ops {
            b[r] -= f * b[p];
        }
        let mut x = vec![0.0; self.n];
        for k in (0..self.n).rev() {
            let p = self.pivot_row[k];
            let mut sum = b[p];
            let mut diag = 0.0;
            for &(c, v) in &self.u[p] {
                if c == k {
                    diag = v;
                } else if c > k {
                    sum -= v * x[c];
                }
            }
            x[k] = sum / diag;
        }
        if self.perm.is_empty() {
            b.copy_from_slice(&x);
        } else {
            for (k, v) in x.into_iter().enumerate() {
                b[self.perm[k]] = v;
            }
        }
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
        let x1 = solve_with(&f1, &b);
        let f2 = refactor(&sym, &rows_of(&a)).expect("the same matrix must refactor");
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
        let g2 = refactor(&sym, &rows_of(&a2)).expect("new values refactor");
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
        let g = refactor(&sym, &rows2).expect("the pattern is the same");
        let x2 = solve_with(&g, &b);
        assert!(residual_ok(&rows2, &x2, &b));
    }
}
