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
}

/// Factor a sparse matrix given as rows of (column, value), sorted by column with no duplicates.
/// Gaussian elimination column by column; the pivot is the sparsest row among those within a
/// tenth of the largest entry (threshold pivoting), which keeps fill-in small.
pub(super) fn factor(n: usize, mut rows: Vec<Vec<(usize, f64)>>) -> Result<Factor, SimError> {
    const THRESHOLD: f64 = 0.1;
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
    Ok(Factor { n, pivot_row, ops, u: rows })
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
        b.copy_from_slice(&x);
    }
}
