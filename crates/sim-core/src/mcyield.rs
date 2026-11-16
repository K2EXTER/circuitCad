//! Yield and process capability of a Monte Carlo run: how many of the random boards meet the limits of a requirement, how far
//! the spread sits from those limits (Cpk), and which parts the failing boards had pushed to one side.
//!
//! - Yield counts every run: one that did not solve is a fail (a board that does not work is not a pass).
//! - The 95 % interval of the yield is Wilson's: 300 runs that all pass only show a yield above about 98.7 %.
//! - Cp = (USL − LSL) / 6σ, Cpk = the distance from the mean to the nearer limit / 3σ (σ of the runs, n − 1). With one limit,
//!   Cpk is that side alone and Cp is not given. 1.33 is the usual target for a product, 1.0 means the limit is at 3σ.
//! - The expected share outside the limits assumes a normal distribution with that mean and σ, so it can be read below the
//!   1 / runs a count can show (the parts are drawn evenly in their ranges, so the tails of a sum of a few parts are thinner than
//!   normal: this is on the cautious side).
//! - The parts behind the failures, for each side (above the upper limit, below the lower) on its own, as the two would cancel:
//!   for each varied part, the mean of where it was drawn (−1 low end … +1 high end) on the
//!   failing runs minus the same on the passing runs. A part that does not matter is near 0; one that pushes the result out is
//!   near ±1. Sorted by the size times the side's share of the failures, the biggest first like the sensitivity list.

use crate::McResult;

#[derive(Clone, Debug, PartialEq)]
pub struct Culprit {
    /// `R1`, or a model figure `U1:offset`.
    pub name: String,
    /// Mean draw on the failing runs of one side minus the mean on the passing runs (−2 … +2; + = the failing boards had it high).
    pub shift: f64,
    /// The failures above the upper limit (else below the lower).
    pub above: bool,
    /// That side's share of the failures (0 … 1); the list is sorted by |shift| × share.
    pub share: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct YieldReport {
    pub lo: Option<f64>,
    pub hi: Option<f64>,
    /// All runs, including the ones that did not solve.
    pub runs: usize,
    pub passed: usize,
    /// Solved runs below the lower / above the upper limit.
    pub below: usize,
    pub above: usize,
    /// Runs that did not solve (counted as fails).
    pub unsolved: usize,
    /// passed / runs.
    pub yield_frac: f64,
    /// 95 % interval of the yield (Wilson).
    pub interval: (f64, f64),
    pub mean: f64,
    /// σ of the solved runs (n − 1).
    pub std: f64,
    /// Both limits only.
    pub cp: Option<f64>,
    /// `None` when the runs do not spread (σ = 0) or fewer than 2 solved.
    pub cpk: Option<f64>,
    /// Expected share outside the limits from the normal distribution (0 … 1); `None` with `cpk`.
    pub expected_out: Option<f64>,
    /// Empty when no solved run failed or none passed.
    pub culprits: Vec<Culprit>,
}

/// Complementary error function (Numerical Recipes' Chebyshev fit, relative error below 1.2e-7).
fn erfc(x: f64) -> f64 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.5 * z);
    let r = t * (-z * z - 1.265_512_23
        + t * (1.000_023_68
            + t * (0.374_091_96 + t * (0.096_784_18 + t * (-0.186_288_06 + t * (0.278_868_07 + t * (-1.135_203_98 + t * (1.488_515_87 + t * (-0.822_152_23 + t * 0.170_872_77)))))))))
        .exp();
    if x >= 0.0 { r } else { 2.0 - r }
}

/// P(Z > z) of a standard normal.
fn upper_tail(z: f64) -> f64 {
    0.5 * erfc(z / std::f64::consts::SQRT_2)
}

/// Wilson's 95 % interval of a share `k / n`.
fn wilson(k: usize, n: usize) -> (f64, f64) {
    if n == 0 {
        return (0.0, 1.0);
    }
    let (n, p, z) = (n as f64, k as f64 / n as f64, 1.959_964);
    let d = 1.0 + z * z / n;
    let c = (p + z * z / (2.0 * n)) / d;
    let h = z * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt() / d;
    ((c - h).max(0.0), (c + h).min(1.0))
}

impl McResult {
    /// The runs judged against `[lo, hi]` (either may be absent). `None` when both are absent or the lower is above the upper.
    pub fn yield_report(&self, lo: Option<f64>, hi: Option<f64>) -> Option<YieldReport> {
        if lo.is_none() && hi.is_none() || lo.zip(hi).is_some_and(|(l, h)| l > h) {
            return None;
        }
        let inside = |v: f64| lo.is_none_or(|l| v >= l) && hi.is_none_or(|h| v <= h);
        let below = self.values.iter().filter(|v| lo.is_some_and(|l| **v < l)).count();
        let above = self.values.iter().filter(|v| hi.is_some_and(|h| **v > h)).count();
        let passed = self.values.len() - below - above;
        let runs = self.values.len() + self.failed;
        let n = self.values.len();
        let mean = if n > 0 { self.values.iter().sum::<f64>() / n as f64 } else { f64::NAN };
        let std = if n > 1 { (self.values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1) as f64).sqrt() } else { 0.0 };
        let spread = n > 1 && std > 1e-12 * mean.abs().max(1e-30);
        let cpu = hi.filter(|_| spread).map(|h| (h - mean) / (3.0 * std));
        let cpl = lo.filter(|_| spread).map(|l| (mean - l) / (3.0 * std));
        let cpk = match (cpl, cpu) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let cp = lo.zip(hi).filter(|_| spread).map(|(l, h)| (h - l) / (6.0 * std));
        let expected_out = cpk.map(|_| cpl.map_or(0.0, |c| upper_tail(3.0 * c)) + cpu.map_or(0.0, |c| upper_tail(3.0 * c)));

        // each side on its own: boards above the upper limit had a part high, boards below had it low, and together they cancel
        let nf = below + above;
        let mut culprits = vec![];
        if nf > 0 && passed > 0 && self.draws.len() == n {
            let mean_of = |pick: &dyn Fn(f64) -> bool, k: usize| {
                let (s, c) = self.values.iter().zip(&self.draws).filter(|(v, _)| pick(**v)).fold((0.0, 0), |(s, c), (_, x)| (s + x[k], c + 1));
                s / c.max(1) as f64
            };
            for (above_side, count) in [(true, above), (false, below)] {
                if count == 0 {
                    continue;
                }
                let failing = |v: f64| if above_side { hi.is_some_and(|h| v > h) } else { lo.is_some_and(|l| v < l) };
                for (k, name) in self.varied.iter().enumerate() {
                    let shift = mean_of(&failing, k) - mean_of(&inside, k);
                    culprits.push(Culprit { name: name.clone(), shift, above: above_side, share: count as f64 / nf as f64 });
                }
            }
            culprits.sort_by(|a, b| (b.shift.abs() * b.share).total_cmp(&(a.shift.abs() * a.share)));
        }
        Some(YieldReport {
            lo,
            hi,
            runs,
            passed,
            below,
            above,
            unsolved: self.failed,
            yield_frac: if runs > 0 { passed as f64 / runs as f64 } else { 0.0 },
            interval: wilson(passed, runs),
            mean,
            std,
            cp,
            cpk,
            expected_out,
            culprits,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(values: Vec<f64>, draws: Vec<Vec<f64>>, failed: usize) -> McResult {
        McResult { output: "out".into(), unit: "V", nominal: 1.0, values, failed, sensitivity: vec![], varied: vec!["R1".into(), "R2".into()], draws }
    }

    #[test]
    fn normal_tail_and_wilson_interval() {
        assert!((upper_tail(0.0) - 0.5).abs() < 1e-7);
        assert!((upper_tail(3.0) - 1.349_9e-3).abs() < 1e-6);
        assert!((upper_tail(-1.0) - 0.841_345).abs() < 1e-6);
        // 300 of 300: the yield is only known to be above ~98.7 %
        let (l, h) = wilson(300, 300);
        assert!((l - 0.9874).abs() < 1e-3 && h == 1.0, "{l} {h}");
    }

    #[test]
    fn counts_cpk_and_the_part_behind_the_failures() {
        // R1 drives the result (value = 1 + 0.1 x1), R2 does nothing; 1 run did not solve
        let xs: Vec<f64> = (0..21).map(|i| -1.0 + i as f64 * 0.1).collect();
        let draws: Vec<Vec<f64>> = xs.iter().map(|x| vec![*x, if (x * 10.0).round() as i64 % 2 == 0 { 0.5 } else { -0.5 }]).collect();
        let values: Vec<f64> = xs.iter().map(|x| 1.0 + 0.1 * x).collect();
        let r = result(values, draws, 1);
        let y = r.yield_report(None, Some(1.05)).unwrap();
        assert_eq!((y.runs, y.above, y.below, y.unsolved), (22, 5, 0, 1));
        assert_eq!(y.passed, 16);
        assert!((y.yield_frac - 16.0 / 22.0).abs() < 1e-12);
        assert!(y.cp.is_none(), "one limit has no Cp");
        let cpk = (1.05 - y.mean) / (3.0 * y.std);
        assert!((y.cpk.unwrap() - cpk).abs() < 1e-12);
        assert_eq!(y.culprits[0].name, "R1");
        assert!(y.culprits[0].shift > 1.0, "{:?}", y.culprits);
        assert!(y.culprits[1].shift.abs() < 0.3, "{:?}", y.culprits);
        assert!(y.culprits.iter().all(|c| c.above && c.share == 1.0));
        // both limits: Cp and Cpk, the nearer side sets Cpk
        let y = r.yield_report(Some(0.0), Some(1.05)).unwrap();
        assert!(y.cp.unwrap() > y.cpk.unwrap());
        // failures on both sides do not cancel: R1 high above the upper limit, low below the lower (5 above, 3 below)
        let y = r.yield_report(Some(0.925), Some(1.05)).unwrap();
        assert_eq!((y.above, y.below), (5, 3));
        let first = &y.culprits[0];
        assert!(first.name == "R1" && first.above && first.shift > 0.6, "{:?}", y.culprits);
        let low = y.culprits.iter().find(|c| c.name == "R1" && !c.above).unwrap();
        assert!(low.shift < -0.6 && (low.share - 3.0 / 8.0).abs() < 1e-12, "{low:?}");
    }

    #[test]
    fn no_limits_no_spread_and_all_passing() {
        let r = result(vec![1.0, 1.0, 1.0], vec![vec![0.1, 0.2]; 3], 0);
        assert!(r.yield_report(None, None).is_none());
        assert!(r.yield_report(Some(2.0), Some(1.0)).is_none());
        let y = r.yield_report(Some(0.9), Some(1.1)).unwrap();
        assert_eq!(y.passed, 3);
        assert!(y.cpk.is_none() && y.expected_out.is_none(), "no spread");
        assert!(y.culprits.is_empty());
    }
}
