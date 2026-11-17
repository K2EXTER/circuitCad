//! Active filter design (candidate V8): the poles of a Butterworth / Bessel / Chebyshev prototype, split into first- and
//! second-order sections, and the parts of each section as an RC + follower, Sallen-Key or multiple-feedback (MFB) stage, with
//! the capacitors from E12 and the resistors rounded to E24 or E96. `designcalc::filter_design` shows it as a calculator; the app
//! places `Stage::netlist` on the schematic and draws `FilterDesign::response` over the AC result as the target.

use crate::ac::C64;
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FilterFamily {
    Butterworth,
    Bessel,
    /// Pass-band ripple in dB.
    Chebyshev(f64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterShape {
    LowPass,
    HighPass,
    BandPass,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterTopology {
    SallenKey,
    Mfb,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FilterSpec {
    pub family: FilterFamily,
    pub shape: FilterShape,
    /// The prototype's order (1..=8; a band-pass of order n has 2n poles, 1..=4).
    pub order: usize,
    /// Cut-off (low / high-pass: −3 dB, Chebyshev: the edge of the ripple band) or centre (band-pass), Hz.
    pub freq: f64,
    /// Band-pass only: the width of the pass band, Hz (−3 dB, or the ripple band for Chebyshev).
    pub bandwidth: f64,
    pub topology: FilterTopology,
    /// Resistors from E96 (1 %) instead of E24.
    pub e96: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageKind {
    RcLowPass,
    RcHighPass,
    SkLowPass,
    SkHighPass,
    MfbLowPass,
    MfbHighPass,
    MfbBandPass,
}

impl StageKind {
    pub fn name(self) -> &'static str {
        match self {
            StageKind::RcLowPass => "RC + ボルテージフォロワ (ローパス 1 次)",
            StageKind::RcHighPass => "RC + ボルテージフォロワ (ハイパス 1 次)",
            StageKind::SkLowPass => "サレンキー ローパス",
            StageKind::SkHighPass => "サレンキー ハイパス",
            StageKind::MfbLowPass => "多重帰還 ローパス",
            StageKind::MfbHighPass => "多重帰還 ハイパス",
            StageKind::MfbBandPass => "多重帰還 バンドパス",
        }
    }

    /// The stage turns the signal upside down (the MFB stages drive the op-amp's − input).
    pub fn inverts(self) -> bool {
        matches!(self, StageKind::MfbLowPass | StageKind::MfbHighPass | StageKind::MfbBandPass)
    }
}

/// One op-amp stage.
#[derive(Clone, Debug, PartialEq)]
pub struct Stage {
    pub kind: StageKind,
    /// Natural (or centre) frequency, Hz.
    pub f0: f64,
    /// Quality factor; 0 for a first-order stage.
    pub q: f64,
    /// Band-pass: the gain at its own centre (the other stages pass 1).
    pub gain: f64,
    /// Resistors R1, R2, ... as calculated (the parts give exactly `f0` and `q`).
    pub r: Vec<f64>,
    /// Resistors rounded to the E series.
    pub r_e: Vec<f64>,
    /// Capacitors C1, C2, ..., chosen from E12 first (the resistors are calculated from them).
    pub c: Vec<f64>,
}

/// One part of a stage and the two nets it joins.
#[derive(Clone, Debug, PartialEq)]
pub struct StagePart {
    /// `R1`, `C2`, ... (inside the stage).
    pub name: String,
    pub value: f64,
    pub a: String,
    pub b: String,
}

impl StagePart {
    pub fn is_cap(&self) -> bool {
        self.name.starts_with('C')
    }
}

/// The op-amp's nets in a stage: + input, − input, output.
#[derive(Clone, Debug, PartialEq)]
pub struct OpampNets {
    pub plus: String,
    pub minus: String,
    pub out: String,
}

impl Stage {
    /// The response of this stage at `s` (rad/s) from its parts (`rounded`: the E-series resistors).
    pub fn h(&self, s: C64, rounded: bool) -> C64 {
        let r = if rounded { &self.r_e } else { &self.r };
        let c = &self.c;
        let one = C64::new(1.0, 0.0);
        let k = |v: f64| C64::new(v, 0.0);
        match self.kind {
            StageKind::RcLowPass => one / (one + s * k(r[0] * c[0])),
            StageKind::RcHighPass => s * k(r[0] * c[0]) / (one + s * k(r[0] * c[0])),
            StageKind::SkLowPass => one / (one + s * k(c[1] * (r[0] + r[1])) + s * s * k(r[0] * r[1] * c[0] * c[1])),
            StageKind::SkHighPass => {
                let p = k(r[0] * r[1] * c[0] * c[1]);
                s * s * p / (s * s * p + s * k(r[0] * (c[0] + c[1])) + one)
            }
            StageKind::MfbLowPass => {
                let g = 1.0 / r[0] + 1.0 / r[1] + 1.0 / r[2];
                -(k(1.0 / (r[0] * r[2] * c[0] * c[1])) / (s * s + s * k(g / c[0]) + k(1.0 / (r[1] * r[2] * c[0] * c[1]))))
            }
            StageKind::MfbHighPass => {
                let (c1, c2, c3) = (c[0], c[1], c[2]);
                -(s * s * k(c1 / c3) / (s * s + s * k((c1 + c2 + c3) / (r[1] * c2 * c3)) + k(1.0 / (r[0] * r[1] * c2 * c3))))
            }
            StageKind::MfbBandPass => {
                let (c1, c2) = (c[0], c[1]);
                -(s * k(1.0 / (r[0] * c1)) / (s * s + s * k((c1 + c2) / (r[2] * c1 * c2)) + k((1.0 / r[0] + 1.0 / r[1]) / (r[2] * c1 * c2))))
            }
        }
    }

    /// The parts and the op-amp of this stage between nets `input` and `output`; `inner` names its own nets (`A`, `B`).
    pub fn netlist(&self, input: &str, output: &str, inner: &dyn Fn(&str) -> String, rounded: bool) -> (Vec<StagePart>, OpampNets) {
        let r = if rounded { &self.r_e } else { &self.r };
        let (a, b) = (inner("A"), inner("B"));
        let gnd = "GND".to_string();
        let part = |name: &str, value: f64, x: &str, y: &str| StagePart { name: name.into(), value, a: x.into(), b: y.into() };
        let follower = OpampNets { plus: a.clone(), minus: output.into(), out: output.into() };
        let inverting = OpampNets { plus: gnd.clone(), minus: b.clone(), out: output.into() };
        match self.kind {
            StageKind::RcLowPass => (vec![part("R1", r[0], input, &a), part("C1", self.c[0], &a, &gnd)], follower),
            StageKind::RcHighPass => (vec![part("C1", self.c[0], input, &a), part("R1", r[0], &a, &gnd)], follower),
            StageKind::SkLowPass => (
                vec![part("R1", r[0], input, &a), part("R2", r[1], &a, &b), part("C1", self.c[0], &a, output), part("C2", self.c[1], &b, &gnd)],
                OpampNets { plus: b.clone(), minus: output.into(), out: output.into() },
            ),
            StageKind::SkHighPass => (
                vec![part("C1", self.c[0], input, &a), part("C2", self.c[1], &a, &b), part("R1", r[0], &a, output), part("R2", r[1], &b, &gnd)],
                OpampNets { plus: b.clone(), minus: output.into(), out: output.into() },
            ),
            StageKind::MfbLowPass => (
                vec![part("R1", r[0], input, &a), part("R2", r[1], &a, output), part("R3", r[2], &a, &b), part("C1", self.c[0], &a, &gnd), part("C2", self.c[1], &b, output)],
                inverting,
            ),
            StageKind::MfbHighPass => (
                vec![part("C1", self.c[0], input, &a), part("C2", self.c[1], &a, &b), part("C3", self.c[2], &a, output), part("R1", r[0], &a, &gnd), part("R2", r[1], &b, output)],
                inverting,
            ),
            StageKind::MfbBandPass => (
                vec![part("R1", r[0], input, &a), part("R2", r[1], &a, &gnd), part("C1", self.c[0], &a, output), part("C2", self.c[1], &a, &b), part("R3", r[2], &b, output)],
                inverting,
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FilterDesign {
    pub spec: FilterSpec,
    /// In the order they are joined: the lowest Q first (the high-Q stages peak, so they come last and see a smaller signal).
    pub stages: Vec<Stage>,
}

impl FilterDesign {
    /// The whole filter's response at `f` Hz (`rounded`: with the E-series resistors; otherwise the target).
    pub fn response(&self, f: f64, rounded: bool) -> C64 {
        let s = C64::new(0.0, 2.0 * PI * f);
        self.stages.iter().fold(C64::new(1.0, 0.0), |acc, st| acc * st.h(s, rounded))
    }

    /// Gain in dB at `f`.
    pub fn db(&self, f: f64, rounded: bool) -> f64 {
        20.0 * self.response(f, rounded).abs().log10()
    }

    /// The frequency range worth plotting: two decades either side.
    pub fn plot_range(&self) -> (f64, f64) {
        (self.spec.freq / 100.0, self.spec.freq * 100.0)
    }
}

/// The left-half-plane poles of the prototype (low-pass, −3 dB at 1 rad/s; Chebyshev: the ripple band ends at 1 rad/s).
pub fn prototype_poles(family: FilterFamily, n: usize) -> Vec<C64> {
    match family {
        FilterFamily::Butterworth => (1..=n).map(|k| C64::from_polar(1.0, 180.0 * (2 * k + n - 1) as f64 / (2 * n) as f64)).collect(),
        FilterFamily::Chebyshev(ripple) => {
            let eps = (10f64.powf(ripple / 10.0) - 1.0).sqrt();
            let a = (1.0 / eps).asinh() / n as f64;
            (1..=n)
                .map(|k| {
                    let th = PI * (2 * k - 1) as f64 / (2 * n) as f64;
                    C64::new(-a.sinh() * th.sin(), a.cosh() * th.cos())
                })
                .collect()
        }
        FilterFamily::Bessel => {
            // reverse Bessel polynomial: a_k = (2n-k)! / (2^(n-k) k! (n-k)!), monic (a_n = 1)
            let fact = |m: usize| (1..=m).map(|x| x as f64).product::<f64>();
            let coef: Vec<f64> = (0..=n).map(|k| fact(2 * n - k) / (2f64.powi((n - k) as i32) * fact(k) * fact(n - k))).collect();
            let poles = poly_roots(&coef);
            // scale so the gain is −3 dB at 1 rad/s
            let mag2 = |w: f64| poles.iter().map(|p| p.abs().powi(2) / (C64::new(0.0, w) - *p).abs().powi(2)).product::<f64>();
            let (mut lo, mut hi) = (1e-3f64, 1e3f64);
            for _ in 0..200 {
                let mid = (lo * hi).sqrt();
                if mag2(mid) > 0.5 { lo = mid } else { hi = mid }
            }
            let w3 = (lo * hi).sqrt();
            poles.into_iter().map(|p| C64::new(p.re / w3, p.im / w3)).collect()
        }
    }
}

/// Roots of the monic polynomial `coef[0] + coef[1] x + ... + x^n` (Durand–Kerner).
fn poly_roots(coef: &[f64]) -> Vec<C64> {
    let n = coef.len() - 1;
    let eval = |x: C64| coef.iter().rev().fold(C64::ZERO, |acc, c| acc * x + C64::new(*c, 0.0));
    let scale = coef[0].abs().powf(1.0 / n as f64).max(1.0);
    let seed = C64::new(0.4, 0.9);
    let mut z: Vec<C64> = (0..n).map(|k| (0..k).fold(C64::new(scale, 0.0), |acc, _| acc * seed)).collect();
    for _ in 0..2000 {
        let mut moved = 0.0f64;
        for i in 0..n {
            let mut den = C64::new(1.0, 0.0);
            for j in 0..n {
                if j != i {
                    den = den * (z[i] - z[j]);
                }
            }
            let step = eval(z[i]) / den;
            z[i] = z[i] - step;
            moved = moved.max(step.abs());
        }
        if moved < 1e-14 * scale {
            break;
        }
    }
    z
}

/// The prototype's sections: (pole magnitude, Q), Q = 0 for a real pole, the lowest Q first.
pub fn prototype_sections(family: FilterFamily, n: usize) -> Vec<(f64, f64)> {
    let mut v = sections_of(&prototype_poles(family, n));
    v.sort_by(|a, b| a.1.total_cmp(&b.1));
    v
}

/// Real poles alone (Q = 0), complex ones by their upper half; two real poles of a band-pass are joined into one biquad.
fn sections_of(poles: &[C64]) -> Vec<(f64, f64)> {
    let tiny = |p: &C64| p.im.abs() < 1e-9 * p.abs().max(1e-30);
    let mut out: Vec<(f64, f64)> = poles.iter().filter(|p| !tiny(p) && p.im > 0.0).map(|p| (p.abs(), p.abs() / (-2.0 * p.re))).collect();
    out.extend(poles.iter().filter(|p| tiny(p)).map(|p| (-p.re, 0.0)));
    out
}

fn csqrt(z: C64) -> C64 {
    let m = z.abs();
    let re = ((m + z.re) / 2.0).max(0.0).sqrt();
    let im = ((m - z.re) / 2.0).max(0.0).sqrt();
    C64::new(re, if z.im < 0.0 { -im } else { im })
}

/// The band-pass biquads (ω / ω0, Q) from the low-pass prototype, `b` = bandwidth / centre.
fn bandpass_sections(family: FilterFamily, n: usize, b: f64) -> Vec<(f64, f64)> {
    let mut complex = vec![];
    let mut real = vec![];
    for p in prototype_poles(family, n) {
        // s_lp = (s² + 1) / (b s)  →  s² − p b s + 1 = 0
        let pb = p * C64::new(b, 0.0);
        let d = csqrt(pb * pb - C64::new(4.0, 0.0));
        for s in [(pb + d) * C64::new(0.5, 0.0), (pb - d) * C64::new(0.5, 0.0)] {
            if s.im.abs() < 1e-9 * s.abs() {
                real.push(s.re);
            } else if s.im > 0.0 {
                complex.push(s);
            }
        }
    }
    let mut out: Vec<(f64, f64)> = complex.iter().map(|s| (s.abs(), s.abs() / (-2.0 * s.re))).collect();
    real.sort_by(f64::total_cmp);
    for pair in real.chunks(2) {
        if let [r1, r2] = pair {
            let w = (r1 * r2).sqrt();
            out.push((w, w / (-(r1 + r2))));
        }
    }
    out.sort_by(|a, b| a.1.total_cmp(&b.1));
    out
}

const E12: [f64; 12] = [1.0, 1.2, 1.5, 1.8, 2.2, 2.7, 3.3, 3.9, 4.7, 5.6, 6.8, 8.2];
const E24: [f64; 24] = [1.0, 1.1, 1.2, 1.3, 1.5, 1.6, 1.8, 2.0, 2.2, 2.4, 2.7, 3.0, 3.3, 3.6, 3.9, 4.3, 4.7, 5.1, 5.6, 6.2, 6.8, 7.5, 8.2, 9.1];

/// E96 mantissas: 10^(i/96) to three figures (the series is defined that way, without the E24 exceptions).
fn e96() -> Vec<f64> {
    (0..96).map(|i| (100.0 * 10f64.powf(i as f64 / 96.0)).round() / 100.0).collect()
}

/// The value of the series closest to `v` (`up`: the smallest not below it).
pub fn e_round(v: f64, mantissas: &[f64], up: bool) -> f64 {
    let dec = v.log10().floor();
    let mut best = f64::NAN;
    let mut err = f64::MAX;
    for d in [dec - 1.0, dec, dec + 1.0] {
        for m in mantissas.iter().chain(std::iter::once(&10.0)) {
            let c = m * 10f64.powf(d);
            if up && c < v * (1.0 - 1e-9) {
                continue;
            }
            let e = (c / v).ln().abs();
            if e < err {
                err = e;
                best = c;
            }
        }
    }
    crate::parse_value(&crate::format_value(best)).unwrap_or(best)
}

/// The resistors' series (E24 or E96).
pub fn r_series(e96_on: bool) -> Vec<f64> {
    if e96_on { e96() } else { E24.to_vec() }
}

/// Designs the filter. Errors are in Japanese, with what to change.
pub fn design_filter(spec: &FilterSpec) -> Result<FilterDesign, String> {
    let max_order = if spec.shape == FilterShape::BandPass { 4 } else { 8 };
    if spec.order < 1 || spec.order > max_order {
        return Err(if spec.shape == FilterShape::BandPass {
            "バンドパスの次数は 1〜4 にしてください (できるフィルタは 2〜8 次になります)".into()
        } else {
            "次数は 1〜8 にしてください".into()
        });
    }
    if !(spec.freq.is_finite() && spec.freq > 0.0) {
        return Err("周波数は 0 より大きい数にしてください".into());
    }
    if let FilterFamily::Chebyshev(r) = spec.family
        && !(0.01..=3.0).contains(&r)
    {
        return Err("チェビシェフのリップルは 0.01〜3 dB にしてください (0.5 か 1 dB がよく使われます)".into());
    }
    let w = 2.0 * PI;
    let bp = spec.shape == FilterShape::BandPass;
    let sections: Vec<(f64, f64)> = if bp {
        if !(spec.bandwidth.is_finite() && spec.bandwidth > 0.0) {
            return Err("バンドパスの帯域幅は 0 より大きい数にしてください".into());
        }
        bandpass_sections(spec.family, spec.order, spec.bandwidth / spec.freq).into_iter().map(|(m, q)| (spec.freq * m, q)).collect()
    } else {
        prototype_sections(spec.family, spec.order)
            .into_iter()
            .map(|(m, q)| (if spec.shape == FilterShape::LowPass { spec.freq * m } else { spec.freq / m }, q))
            .collect()
    };
    let rs = r_series(spec.e96);
    let mut stages = vec![];
    for (f0, q) in sections {
        let w0 = w * f0;
        // the capacitor that puts the resistors near 10 kΩ
        let cb = e_round(1.0 / (w0 * 10e3), &E12, false);
        let (kind, r, c, gain) = if q == 0.0 {
            let kind = if spec.shape == FilterShape::LowPass { StageKind::RcLowPass } else { StageKind::RcHighPass };
            (kind, vec![1.0 / (w0 * cb)], vec![cb], 1.0)
        } else if bp {
            let am = (q * q).min(1.0);
            (StageKind::MfbBandPass, vec![q / (w0 * cb * am), 1.0 / (w0 * cb * (2.0 * q - am / q)), 2.0 * q / (w0 * cb)], vec![cb, cb], am)
        } else {
            match (spec.shape, spec.topology) {
                (FilterShape::LowPass, FilterTopology::SallenKey) => {
                    let c2 = cb;
                    let c1 = e_round(4.0 * q * q * c2, &E12, true);
                    let (sum, prod) = (1.0 / (w0 * q * c2), 1.0 / (w0 * w0 * c1 * c2));
                    let d = (sum * sum - 4.0 * prod).max(0.0).sqrt();
                    (StageKind::SkLowPass, vec![(sum + d) / 2.0, (sum - d) / 2.0], vec![c1, c2], 1.0)
                }
                (FilterShape::HighPass, FilterTopology::SallenKey) => (StageKind::SkHighPass, vec![1.0 / (2.0 * q * w0 * cb), 2.0 * q / (w0 * cb)], vec![cb, cb], 1.0),
                (FilterShape::LowPass, FilterTopology::Mfb) => {
                    let c2 = cb;
                    let c1 = e_round(8.0 * q * q * c2, &E12, true);
                    let k = w0 * c1 / q;
                    let a = (k + (k * k - 8.0 * w0 * w0 * c1 * c2).max(0.0).sqrt()) / 4.0;
                    (StageKind::MfbLowPass, vec![1.0 / a, 1.0 / a, 1.0 / (k - 2.0 * a)], vec![c1, c2], 1.0)
                }
                (FilterShape::HighPass, FilterTopology::Mfb) => (StageKind::MfbHighPass, vec![1.0 / (3.0 * q * w0 * cb), 3.0 * q / (w0 * cb)], vec![cb, cb, cb], 1.0),
                (FilterShape::BandPass, _) => unreachable!("band-pass is handled above"),
            }
        };
        let r_e = r.iter().map(|v| e_round(*v, &rs, false)).collect();
        stages.push(Stage { kind, f0, q, gain, r, r_e, c });
    }
    Ok(FilterDesign { spec: *spec, stages })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ac_analysis, AcSettings, AcSweep, Circuit, Element, Waveform};

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    /// (family, order, expected (pole magnitude, Q) per section, lowest Q first). Butterworth and Bessel are normalised to
    /// −3 dB at 1 rad/s, Chebyshev to the edge of the ripple band; the numbers are the textbook tables (Williams & Taylor,
    /// "Electronic Filter Design Handbook", and the Butterworth Q = 1 / (2 sin(...)) values).
    const TABLE: &[(FilterFamily, usize, &[(f64, f64)])] = &[
        (FilterFamily::Butterworth, 2, &[(1.0, std::f64::consts::FRAC_1_SQRT_2)]),
        (FilterFamily::Butterworth, 3, &[(1.0, 0.0), (1.0, 1.0)]),
        (FilterFamily::Butterworth, 4, &[(1.0, 0.5412), (1.0, 1.3066)]),
        (FilterFamily::Butterworth, 5, &[(1.0, 0.0), (1.0, 0.6180), (1.0, 1.0 + 0.6180)]),
        (FilterFamily::Bessel, 2, &[(1.2720, 0.5774)]),
        (FilterFamily::Bessel, 3, &[(1.3226, 0.0), (1.4476, 0.6910)]),
        (FilterFamily::Bessel, 4, &[(1.4302, 0.5219), (1.6034, 0.8055)]),
        (FilterFamily::Bessel, 5, &[(1.5023, 0.0), (1.5562, 0.5635), (1.7556, 0.9165)]),
        (FilterFamily::Chebyshev(0.5), 2, &[(1.2313, 0.8637)]),
        (FilterFamily::Chebyshev(1.0), 2, &[(1.0500, 0.9565)]),
        (FilterFamily::Chebyshev(1.0), 3, &[(0.4942, 0.0), (0.9971, 2.0177)]),
        (FilterFamily::Chebyshev(1.0), 4, &[(0.5286, 0.7845), (0.9932, 3.5590)]),
    ];

    #[test]
    fn the_sections_match_the_textbook_tables() {
        for (family, n, want) in TABLE {
            let got = prototype_sections(*family, *n);
            assert_eq!(got.len(), want.len(), "{family:?} n={n}");
            for ((m, q), (wm, wq)) in got.iter().zip(want.iter()) {
                assert!(close(*m, *wm, 2e-3) && close(*q, *wq, 2e-3), "{family:?} n={n}: got ({m:.4}, {q:.4}), want ({wm}, {wq})");
            }
        }
    }

    #[test]
    fn the_target_passes_the_cutoff_at_the_right_level() {
        // (family, shape, order, dB at the cut-off / centre)
        let cases = [
            (FilterFamily::Butterworth, FilterShape::LowPass, 4, -3.01),
            (FilterFamily::Bessel, FilterShape::LowPass, 6, -3.01),
            (FilterFamily::Butterworth, FilterShape::HighPass, 3, -3.01),
            (FilterFamily::Chebyshev(1.0), FilterShape::LowPass, 5, -1.0),
            // even orders: every stage passes 1 far from the edge, so the ripple band is 0 .. +0.5 dB and the edge is at 0 dB
            (FilterFamily::Chebyshev(0.5), FilterShape::HighPass, 4, 0.0),
        ];
        for (family, shape, order, want) in cases {
            for topology in [FilterTopology::SallenKey, FilterTopology::Mfb] {
                let d = design_filter(&FilterSpec { family, shape, order, freq: 1e3, bandwidth: 0.0, topology, e96: true }).unwrap();
                let got = d.db(1e3, false);
                assert!(close(got, want, 0.01), "{family:?} {shape:?} {order} {topology:?}: {got:.3} dB");
                // E96 resistors move it little
                assert!(close(d.db(1e3, true), want, 0.3), "rounded: {:.3}", d.db(1e3, true));
            }
        }
        // a Butterworth band-pass: 0 dB at the centre (each stage passes its own gain at its centre), −3 dB at the band edges
        let d = design_filter(&FilterSpec { family: FilterFamily::Butterworth, shape: FilterShape::BandPass, order: 2, freq: 1e3, bandwidth: 200.0, topology: FilterTopology::Mfb, e96: true }).unwrap();
        let centre = d.db(1e3, false);
        let lo = 1e3 * ((0.01f64 + 1.0).sqrt() - 0.1);
        let hi = lo + 200.0;
        for f in [lo, hi] {
            assert!(close(d.db(f, false) - centre, -3.01, 0.02), "edge {f}: {:.3}", d.db(f, false) - centre);
        }
    }

    #[test]
    fn the_parts_wired_as_drawn_give_the_formula() {
        // every stage kind, wired by `netlist` with an ideal op-amp, AC-analysed: the same curve as `Stage::h`
        let specs = [
            (FilterShape::LowPass, FilterTopology::SallenKey, 3),
            (FilterShape::HighPass, FilterTopology::SallenKey, 3),
            (FilterShape::LowPass, FilterTopology::Mfb, 2),
            (FilterShape::HighPass, FilterTopology::Mfb, 2),
            (FilterShape::BandPass, FilterTopology::Mfb, 2),
        ];
        for (shape, topology, order) in specs {
            let d = design_filter(&FilterSpec { family: FilterFamily::Chebyshev(1.0), shape, order, freq: 2e3, bandwidth: 500.0, topology, e96: false }).unwrap();
            let mut c = Circuit::new();
            let inp = c.node("IN");
            c.add(Element::VSource { name: "V1".into(), p: inp, n: 0, wave: Waveform::Dc(0.0), ac: Some((1.0, 0.0)) });
            let mut prev = "IN".to_string();
            for (k, st) in d.stages.iter().enumerate() {
                let out = format!("S{k}");
                let (parts, op) = st.netlist(&prev, &out, &|x| format!("S{k}{x}"), true);
                for p in parts {
                    let (a, b) = (c.node(&p.a), c.node(&p.b));
                    let name = format!("{}_{k}", p.name);
                    c.add(if p.is_cap() { Element::Capacitor { name, a, b, c: p.value } } else { Element::Resistor { name, a, b, r: p.value } });
                }
                let (pl, mi, o) = (c.node(&op.plus), c.node(&op.minus), c.node(&op.out));
                c.add(Element::Vcvs { name: format!("E{k}"), p: o, n: 0, cp: pl, cn: mi, gain: 1e8 });
                prev = out;
            }
            let res = ac_analysis(&c, &AcSettings { fstart: 100.0, fstop: 50e3, sweep: AcSweep::Decade(5) }).unwrap();
            let sig = res.signal(&format!("V({prev})")).unwrap_or_else(|| panic!("no V({prev}): {:?}", res.signals.iter().map(|s| &s.name).collect::<Vec<_>>()));
            for (i, f) in res.freq.iter().enumerate() {
                let sim = C64::new(sig.re[i], sig.im[i]);
                let want = d.response(*f, true);
                assert!((sim - want).abs() <= 1e-4 * want.abs().max(1e-3), "{shape:?} {topology:?} at {f} Hz: sim {sim:?}, formula {want:?}");
            }
        }
    }

    #[test]
    fn rounding_and_limits() {
        assert_eq!(e_round(10.17e3, &r_series(true), false), 10.2e3);
        assert_eq!(e_round(10.17e3, &r_series(false), false), 10e3);
        assert!(close(e_round(1.3e-9, &E12, true), 1.5e-9, 1e-20));
        assert_eq!(e96().len(), 96);
        assert_eq!(e96()[95], 9.76);
        let spec = FilterSpec { family: FilterFamily::Butterworth, shape: FilterShape::BandPass, order: 5, freq: 1e3, bandwidth: 100.0, topology: FilterTopology::Mfb, e96: true };
        assert!(design_filter(&spec).is_err(), "band-pass up to 4");
        assert!(design_filter(&FilterSpec { order: 9, shape: FilterShape::LowPass, ..spec }).is_err());
        assert!(design_filter(&FilterSpec { family: FilterFamily::Chebyshev(5.0), order: 2, ..spec }).is_err());
    }
}
