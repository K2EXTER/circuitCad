//! Root locus by sweeping a part: the poles of V(out)/V(in) found again for each value of one part (a resistor that sets the
//! gain or the damping, a load capacitor ...), so the path of every pole across the complex plane shows how the value moves the
//! circuit towards ringing or oscillation. It is the parameter sweep (`paramsweep.rs`: the same values, the same text with the
//! number replaced) with the pole and zero fit (`polezero.rs`) as the analysis of each run, so it works on any circuit the AC
//! analysis does, with models and all, not only on a loop with one gain K as in the textbook root locus.

use crate::ac::{AcSettings, C64};
use crate::paramsweep::{with_number, MAX_RUNS};
use crate::polezero::{transfer_function, Root};
use crate::{from_schematic, parse_value, SimError};
use sch_core::Schematic;

#[derive(Clone, Debug)]
pub struct LocusSettings {
    /// Part number (R1, C2, ...) whose value is swept.
    pub refdes: String,
    pub values: Vec<f64>,
    /// Output net and (optionally) input net of the transfer function.
    pub output: String,
    pub input: Option<String>,
    pub ac: AcSettings,
    pub temp_c: f64,
    /// Largest number of poles the fit may use, and the fit error it is happy with (as `transfer_function`).
    pub max_poles: usize,
    pub tolerance: f64,
}

/// The poles and zeros at one value of the part.
#[derive(Clone, Debug)]
pub struct LocusStep {
    pub value: f64,
    /// As the fit lists them: a complex pair once (`pair`), the upper one.
    pub poles: Vec<Root>,
    pub zeros: Vec<Root>,
    /// Largest relative error of the fit (0.02 = 2 %).
    pub error: f64,
}

impl LocusStep {
    /// Every pole as a point of the s plane (rad/s), both of a pair.
    pub fn pole_points(&self) -> Vec<C64> {
        points(&self.poles)
    }

    pub fn zero_points(&self) -> Vec<C64> {
        points(&self.zeros)
    }

    pub fn unstable(&self) -> bool {
        self.poles.iter().any(|p| p.right_half())
    }

    /// The smallest damping ratio of the complex pairs, -σ/|p| (`None`: all the poles are real). Below 0 for a pair in the right
    /// half plane: the ringing grows.
    pub fn min_damping(&self) -> Option<f64> {
        self.poles.iter().filter(|p| p.pair).map(|p| -p.re / p.re.hypot(p.im)).reduce(f64::min)
    }
}

fn points(r: &[Root]) -> Vec<C64> {
    let mut out = vec![];
    for r in r {
        out.push(C64::new(r.re, r.im));
        if r.pair {
            out.push(C64::new(r.re, -r.im));
        }
    }
    out
}

#[derive(Clone, Debug)]
pub struct RootLocus {
    pub refdes: String,
    pub steps: Vec<LocusStep>,
    /// Values whose run failed, with the reason.
    pub failed: Vec<(f64, String)>,
    pub warnings: Vec<String>,
}

/// One pole followed from value to value: (index into `steps`, the pole there in rad/s).
pub type Branch = Vec<(usize, C64)>;

impl RootLocus {
    /// The poles joined into paths: each pole of a step goes on with the nearest pole of the next step (nearest relative to
    /// their size, as poles of one circuit are often decades apart). A pole with no partner (the fit found one more or one fewer)
    /// starts or ends a path.
    pub fn branches(&self) -> Vec<Branch> {
        let mut out: Vec<Branch> = vec![];
        let mut open: Vec<usize> = vec![];
        for (k, s) in self.steps.iter().enumerate() {
            let pts = s.pole_points();
            let mut pairs: Vec<(f64, usize, usize)> = vec![];
            for (bi, &b) in open.iter().enumerate() {
                let last = out[b].last().expect("a path has a point").1;
                for (pi, p) in pts.iter().enumerate() {
                    let d = (*p - last).abs() / p.abs().max(last.abs()).max(1e-300);
                    pairs.push((d, bi, pi));
                }
            }
            pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
            let (mut used_b, mut used_p) = (vec![false; open.len()], vec![false; pts.len()]);
            let mut next_open = vec![];
            for (d, bi, pi) in pairs {
                // farther than its own size: not the same pole any more
                if used_b[bi] || used_p[pi] || d > 1.0 {
                    continue;
                }
                used_b[bi] = true;
                used_p[pi] = true;
                out[open[bi]].push((k, pts[pi]));
                next_open.push(open[bi]);
            }
            for (_, p) in pts.iter().enumerate().filter(|(pi, _)| !used_p[*pi]) {
                out.push(vec![(k, *p)]);
                next_open.push(out.len() - 1);
            }
            open = next_open;
        }
        out
    }

    /// The first value where a pole is in the right half plane, when the first value has none: about where the circuit starts
    /// to oscillate by itself.
    pub fn first_unstable(&self) -> Option<f64> {
        let first = self.steps.first()?;
        if first.unstable() {
            return None;
        }
        self.steps.iter().find(|s| s.unstable()).map(|s| s.value)
    }
}

/// Poles and zeros of V(output)/V(input) for every value of the part `s.refdes`.
pub fn root_locus(base: &Schematic, s: &LocusSettings) -> Result<RootLocus, SimError> {
    let base = &*base.flat();
    if s.values.len() < 2 || s.values.len() > MAX_RUNS {
        return Err(SimError::Invalid(format!("値の数は 2〜{MAX_RUNS} にしてください")));
    }
    let Some(idx) = base.components.iter().position(|c| c.refdes.eq_ignore_ascii_case(s.refdes.trim())) else {
        return Err(SimError::Invalid(format!("部品 {} が見つかりません。部品番号 (R1 など) を確かめてください", s.refdes)));
    };
    if parse_value(base.components[idx].value.split_whitespace().next().unwrap_or("")).is_none() {
        return Err(SimError::Invalid(format!("{} の値「{}」は数値ではないので変えられません。抵抗・コンデンサ・コイルなど、値が数字の部品を選んでください", s.refdes, base.components[idx].value)));
    }
    let runs = crate::par_map(&s.values, |&v| {
        let mut sch = base.clone();
        sch.components[idx].value = with_number(&base.components[idx].value, v);
        let c = from_schematic(&sch).map_err(|e| SimError::Invalid(e.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" / ")))?.at_temperature(s.temp_c);
        transfer_function(&c, s.output.trim(), s.input.as_deref().map(str::trim).filter(|i| !i.is_empty()), &s.ac, s.max_poles, s.tolerance)
    });
    let mut out = RootLocus { refdes: base.components[idx].refdes.clone(), steps: vec![], failed: vec![], warnings: vec![] };
    for (&v, r) in s.values.iter().zip(runs) {
        match r {
            Ok(tf) => {
                for w in tf.warnings {
                    if !out.warnings.contains(&w) {
                        out.warnings.push(w);
                    }
                }
                out.steps.push(LocusStep { value: v, poles: tf.pz.poles, zeros: tf.pz.zeros, error: tf.pz.error });
            }
            // a wrong net name fails every run the same way: say it once, as the error
            Err(e @ SimError::Invalid(_)) if out.steps.is_empty() && v == s.values[0] => return Err(e),
            Err(e) => out.failed.push((v, e.to_string())),
        }
    }
    if out.steps.len() < 2 {
        let why = out.failed.first().map(|f| f.1.clone()).unwrap_or_default();
        return Err(SimError::Invalid(format!("2 つ以上の値で計算できませんでした: {why}")));
    }
    let bad = out.steps.iter().filter(|step| step.error >= s.tolerance).count();
    if bad > 0 {
        out.warnings.push(format!("{bad} 個の値で、当てはめの誤差が {:.0}% 以上です (その値の極は確かではありません。周波数の範囲を、極が動く所が全部入るように広げてください)", s.tolerance * 100.0));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ac::AcSweep;
    use crate::paramsweep::sweep_values;

    fn example(name: &str) -> Schematic {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples").join(name);
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    fn rlc(values: Vec<f64>) -> LocusSettings {
        LocusSettings {
            refdes: "R1".into(),
            values,
            output: "OUT".into(),
            input: Some("IN".into()),
            ac: AcSettings { fstart: 100.0, fstop: 1e6, sweep: AcSweep::Decade(20) },
            temp_c: 27.0,
            max_poles: 6,
            tolerance: 0.02,
        }
    }

    /// Series R, L = 1 mH, C = 1 uF: s^2 + (R/L) s + 1/LC = 0, so s = -R/2L +- sqrt((R/2L)^2 - 1/LC). Under R = 2 sqrt(L/C) = 63.2
    /// ohm the pair is on the circle |s| = 1/sqrt(LC) (w0 = 31.6 krad/s, 5.03 kHz) with the real part -R/2L; over it, two real poles.
    #[test]
    fn the_rlc_poles_follow_the_formula_round_the_circle_and_split_on_the_real_axis() {
        let r = root_locus(&example("rlc_poles.schjson"), &rlc(sweep_values(5.0, 200.0, 9, true))).unwrap();
        assert_eq!(r.steps.len(), 9);
        let w0 = 1.0 / (1e-3f64 * 1e-6).sqrt();
        for s in &r.steps {
            let a = s.value / 2e-3;
            let pts = s.pole_points();
            assert_eq!(pts.len(), 2, "R = {}: {:?}", s.value, s.poles);
            if a < w0 {
                let wd = (w0 * w0 - a * a).sqrt();
                assert!(s.poles[0].pair);
                assert!((s.poles[0].re / -a - 1.0).abs() < 0.01 && (s.poles[0].im / wd - 1.0).abs() < 0.01, "R = {}: {:?}", s.value, s.poles);
                assert!((C64::new(s.poles[0].re, s.poles[0].im).abs() / w0 - 1.0).abs() < 0.01);
            } else {
                let d = (a * a - w0 * w0).sqrt();
                let mut re: Vec<f64> = pts.iter().map(|p| p.re).collect();
                re.sort_by(f64::total_cmp);
                assert!((re[0] / (-a - d) - 1.0).abs() < 0.02 && (re[1] / (-a + d) - 1.0).abs() < 0.02, "R = {}: {re:?}", s.value);
            }
        }
        // two paths, each through every value; none in the right half plane
        let b = r.branches();
        assert_eq!(b.len(), 2, "{b:?}");
        assert!(b.iter().all(|b| b.len() == 9));
        assert_eq!(r.first_unstable(), None);
        // the upper pole stays in the upper half (or on the axis) all the way
        let upper = b.iter().find(|b| b[0].1.im > 0.0).unwrap();
        assert!(upper.iter().all(|(_, p)| p.im >= -1e-6 * w0));
    }

    /// Sallen-Key low-pass with equal parts on an LM358 (`sallen_key_locus.schjson`): K = 1 + R3/R4, Q = 1/(3 - K), so the damping
    /// is (3 - K)/2 = (2 - R3/10k)/2 and the pair reaches the imaginary axis at R3 = 20 kohm (K = 3). The op-amp's finite gain moves
    /// it a little.
    #[test]
    fn the_sallen_key_poles_cross_into_the_right_half_plane_where_the_gain_reaches_three() {
        let mut s = rlc(vec![5e3, 10e3, 15e3, 19e3, 21e3, 25e3]);
        s.refdes = "R3".into();
        s.ac = AcSettings { fstart: 10.0, fstop: 1e6, sweep: AcSweep::Decade(20) };
        let r = root_locus(&example("sallen_key_locus.schjson"), &s).unwrap();
        for st in r.steps.iter().filter(|st| st.value <= 15e3) {
            let want = (2.0 - st.value / 10e3) / 2.0;
            assert!((st.min_damping().unwrap() - want).abs() < 0.03, "R3 = {}: {:?} vs {want}", st.value, st.min_damping());
        }
        assert_eq!(r.first_unstable(), Some(21e3));
        assert!(r.steps.last().unwrap().min_damping().unwrap() < 0.0);
    }

    #[test]
    fn branches_follow_the_nearest_pole_and_a_new_pole_starts_its_own_path() {
        let step = |v: f64, poles: Vec<Root>| LocusStep { value: v, poles, zeros: vec![], error: 0.0 };
        let real = |re: f64| Root { re, im: 0.0, pair: false };
        let l = RootLocus {
            refdes: "R1".into(),
            steps: vec![step(1.0, vec![real(-10.0), real(-1000.0)]), step(2.0, vec![real(-1100.0), real(-12.0)]), step(3.0, vec![real(-14.0), real(-1200.0), Root { re: 5.0, im: 3e5, pair: true }])],
            failed: vec![],
            warnings: vec![],
        };
        let b = l.branches();
        assert_eq!(b.len(), 4);
        let slow = b.iter().find(|b| b[0].1.re == -10.0).unwrap();
        assert_eq!(slow.iter().map(|p| p.1.re).collect::<Vec<_>>(), vec![-10.0, -12.0, -14.0]);
        assert!(b.iter().filter(|b| b.len() == 1).count() == 2);
        assert_eq!(l.first_unstable(), Some(3.0));
    }

    #[test]
    fn a_part_without_a_number_and_a_wrong_net_are_reported() {
        let sch = example("rlc_poles.schjson");
        let mut s = rlc(vec![10.0, 20.0]);
        s.refdes = "R9".into();
        assert!(root_locus(&sch, &s).is_err());
        let mut s = rlc(vec![10.0, 20.0]);
        s.output = "NOPE".into();
        assert!(root_locus(&sch, &s).is_err());
        assert!(root_locus(&sch, &rlc(vec![10.0])).is_err());
    }
}
