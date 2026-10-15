//! AC small-signal analysis: linearise around the DC operating point, then solve the complex
//! MNA system at every frequency. Nonlinear devices contribute their small-signal conductances
//! (junction/diffusion capacitances are not modelled).

use crate::circuit::{Circuit, Element};
use crate::dc::{compile, floating_warnings, has_ground, solve_op, Ce, Ctx, SimError};
use std::f64::consts::PI;
use std::ops::{Add, Div, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct C64 {
    pub re: f64,
    pub im: f64,
}

impl C64 {
    pub const ZERO: C64 = C64 { re: 0.0, im: 0.0 };
    pub fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }
    pub fn from_polar(mag: f64, deg: f64) -> Self {
        let r = deg.to_radians();
        Self::new(mag * r.cos(), mag * r.sin())
    }
    pub fn abs(self) -> f64 {
        self.re.hypot(self.im)
    }
    pub fn arg_deg(self) -> f64 {
        self.im.atan2(self.re).to_degrees()
    }
}

impl Add for C64 {
    type Output = C64;
    fn add(self, o: C64) -> C64 {
        C64::new(self.re + o.re, self.im + o.im)
    }
}
impl Sub for C64 {
    type Output = C64;
    fn sub(self, o: C64) -> C64 {
        C64::new(self.re - o.re, self.im - o.im)
    }
}
impl Mul for C64 {
    type Output = C64;
    fn mul(self, o: C64) -> C64 {
        C64::new(self.re * o.re - self.im * o.im, self.re * o.im + self.im * o.re)
    }
}
impl Div for C64 {
    type Output = C64;
    fn div(self, o: C64) -> C64 {
        let d = o.re * o.re + o.im * o.im;
        C64::new((self.re * o.re + self.im * o.im) / d, (self.im * o.re - self.re * o.im) / d)
    }
}
impl Neg for C64 {
    type Output = C64;
    fn neg(self) -> C64 {
        C64::new(-self.re, -self.im)
    }
}

/// Solve A x = b (dense, partial pivoting). `None` when singular.
pub(crate) fn solve_complex(n: usize, a: &mut [C64], b: &mut [C64]) -> Option<()> {
    for col in 0..n {
        let (mut piv, mut best) = (col, a[col * n + col].abs());
        for r in col + 1..n {
            let m = a[r * n + col].abs();
            if m > best {
                best = m;
                piv = r;
            }
        }
        if best < 1e-30 || !best.is_finite() {
            return None;
        }
        if piv != col {
            for c in 0..n {
                a.swap(col * n + c, piv * n + c);
            }
            b.swap(col, piv);
        }
        let d = a[col * n + col];
        for r in col + 1..n {
            let f = a[r * n + col] / d;
            if f != C64::ZERO {
                for c in col..n {
                    let t = f * a[col * n + c];
                    a[r * n + c] = a[r * n + c] - t;
                }
                let t = f * b[col];
                b[r] = b[r] - t;
            }
        }
    }
    for r in (0..n).rev() {
        let mut s = b[r];
        for c in r + 1..n {
            s = s - a[r * n + c] * b[c];
        }
        b[r] = s / a[r * n + r];
    }
    Some(())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AcSweep {
    /// Logarithmic, this many points per decade.
    Decade(usize),
    /// Linear, this many points in total.
    Linear(usize),
}

#[derive(Clone, Debug)]
pub struct AcSettings {
    pub fstart: f64,
    pub fstop: f64,
    pub sweep: AcSweep,
}

#[derive(Clone, Debug)]
pub struct AcSignal {
    /// `V(node)` or `I(element)`.
    pub name: String,
    pub unit: &'static str,
    pub re: Vec<f64>,
    pub im: Vec<f64>,
}

impl AcSignal {
    pub fn mag(&self) -> Vec<f64> {
        self.re.iter().zip(&self.im).map(|(r, i)| r.hypot(*i)).collect()
    }

    /// 20*log10(|x|), floored at -300 dB.
    pub fn db(&self) -> Vec<f64> {
        self.mag().iter().map(|m| 20.0 * m.max(1e-15).log10()).collect()
    }

    /// Phase in degrees, unwrapped so that it is continuous along the sweep.
    pub fn phase_deg(&self) -> Vec<f64> {
        let mut out: Vec<f64> = Vec::with_capacity(self.re.len());
        for (r, i) in self.re.iter().zip(&self.im) {
            let mut p = i.atan2(*r).to_degrees();
            if let Some(&prev) = out.last() {
                while p - prev > 180.0 {
                    p -= 360.0;
                }
                while p - prev < -180.0 {
                    p += 360.0;
                }
            }
            out.push(p);
        }
        out
    }
}

#[derive(Clone, Debug)]
pub struct AcResult {
    pub freq: Vec<f64>,
    pub signals: Vec<AcSignal>,
    pub warnings: Vec<String>,
}

impl AcResult {
    pub fn signal(&self, name: &str) -> Option<&AcSignal> {
        self.signals.iter().find(|s| s.name == name)
    }
}

pub(crate) fn frequencies(s: &AcSettings) -> Vec<f64> {
    match s.sweep {
        AcSweep::Decade(ppd) => {
            let mut v = vec![];
            let step = 1.0 / ppd as f64;
            let mut k = 0.0;
            loop {
                let f = s.fstart * 10f64.powf(k);
                if f > s.fstop * (1.0 + 1e-9) {
                    break;
                }
                v.push(f);
                k += step;
            }
            if v.last().is_none_or(|l| (s.fstop - l).abs() > 1e-9 * s.fstop) {
                v.push(s.fstop);
            }
            v
        }
        AcSweep::Linear(n) => (0..n).map(|i| s.fstart + (s.fstop - s.fstart) * i as f64 / (n - 1) as f64).collect(),
    }
}

pub(crate) fn check_sweep(s: &AcSettings) -> Result<(), SimError> {
    if !(s.fstart.is_finite() && s.fstart > 0.0) {
        return Err(SimError::Invalid("開始周波数は正の値にしてください".into()));
    }
    if !(s.fstop.is_finite() && s.fstop >= s.fstart) {
        return Err(SimError::Invalid("終了周波数は開始周波数以上にしてください".into()));
    }
    match s.sweep {
        AcSweep::Decade(p) if p == 0 || p > 1000 => return Err(SimError::Invalid("1デケードあたりの点数は 1〜1000 にしてください".into())),
        AcSweep::Linear(n) if !(2..=100_000).contains(&n) => return Err(SimError::Invalid("点数は 2〜100000 にしてください".into())),
        _ => {}
    }
    if (s.fstop / s.fstart).log10() * match s.sweep { AcSweep::Decade(p) => p as f64, _ => 1.0 } > 100_000.0 {
        return Err(SimError::Invalid("周波数の点数が多すぎます".into()));
    }
    Ok(())
}

pub fn ac_analysis(circuit: &Circuit, s: &AcSettings) -> Result<AcResult, SimError> {
    check_sweep(s)?;
    let has_source = circuit.elements.iter().any(|e| match e {
        Element::VSource { ac, .. } | Element::ISource { ac, .. } => ac.is_some_and(|(m, _)| m != 0.0),
        _ => false,
    });
    if !has_source {
        return Err(SimError::Invalid("AC解析には AC を持つ電源が必要です (電源の値に「AC 1」のように入力)".into()));
    }
    if !has_ground(circuit) {
        return Err(SimError::NoGround);
    }
    let warnings = floating_warnings(circuit);
    let cm = compile(circuit);

    // small-signal matrix: the DC Jacobian at the operating point (C open, L short)
    let (op, _) = solve_op(&cm, &crate::dc::DC_ORDER)?;
    let mut vj = op.vj.clone();
    let (jac, _) = cm.assemble(&mut vj, &op.x, &Ctx::dc(crate::options::options().gmin, 1.0));
    let n = cm.n_unknowns;

    let freqs = frequencies(s);
    let n_nodes = circuit.names.len();
    let mut signals: Vec<AcSignal> = (1..n_nodes)
        .map(|i| AcSignal { name: format!("V({})", circuit.names[i]), unit: "V", re: vec![], im: vec![] })
        .collect();
    let current_elems: Vec<&Ce> = cm
        .ce
        .iter()
        .filter(|e| matches!(e, Ce::R { .. } | Ce::C { .. } | Ce::L { .. } | Ce::V { .. } | Ce::E { .. } | Ce::G { .. } | Ce::I { .. }))
        .collect();
    let elem_name = |e: &Ce| match e {
        Ce::R { name, .. } | Ce::C { name, .. } | Ce::L { name, .. } | Ce::V { name, .. } | Ce::E { name, .. } | Ce::G { name, .. } | Ce::I { name, .. } => name.clone(),
        _ => String::new(),
    };
    for e in &current_elems {
        signals.push(AcSignal { name: format!("I({})", elem_name(e)), unit: "A", re: vec![], im: vec![] });
    }

    let jw_c = |w: f64| C64::new(0.0, w);
    for &f in &freqs {
        let w = 2.0 * PI * f;
        let mut a: Vec<C64> = jac.dense().iter().map(|&x| C64::new(x, 0.0)).collect();
        let mut b = vec![C64::ZERO; n];
        let add = |a: &mut Vec<C64>, r: usize, c: usize, val: C64| {
            if r != 0 && c != 0 {
                a[(r - 1) * n + (c - 1)] = a[(r - 1) * n + (c - 1)] + val;
            }
        };
        for e in &cm.ce {
            match e {
                Ce::C { a: na, b: nb, c, .. } => {
                    let y = jw_c(w) * C64::new(*c, 0.0);
                    add(&mut a, *na, *na, y);
                    add(&mut a, *nb, *nb, y);
                    add(&mut a, *na, *nb, -y);
                    add(&mut a, *nb, *na, -y);
                }
                Ce::L { l, br, i0, .. } => {
                    // a saturating inductor has less inductance at the operating current
                    let l = if *i0 > 0.0 { l / (1.0 + (op.x[*br] / i0).powi(2)) } else { *l };
                    a[*br * n + *br] = a[*br * n + *br] - jw_c(w) * C64::new(l, 0.0);
                }
                Ce::D { ai, k, nvt, dm, slot, cap: Some(_), .. } => {
                    let y = jw_c(w) * C64::new(crate::dc::diode_q(dm, *nvt, op.vj[*slot]).1, 0.0);
                    add(&mut a, *ai, *ai, y);
                    add(&mut a, *k, *k, y);
                    add(&mut a, *ai, *k, -y);
                    add(&mut a, *k, *ai, -y);
                }
                Ce::Q { bi, ci, ei, m, slot, cap: Some(_), .. } => {
                    // small-signal junction capacitances at the operating point
                    let (cbe, cbc) = (crate::dc::bjt_qbe(m, op.vj[*slot]).1, crate::dc::bjt_qbc(m, op.vj[*slot + 1]).1);
                    for (n1, n2, cc) in [(*bi, *ei, cbe), (*bi, *ci, cbc)] {
                        let y = jw_c(w) * C64::new(cc, 0.0);
                        add(&mut a, n1, n1, y);
                        add(&mut a, n2, n2, y);
                        add(&mut a, n1, n2, -y);
                        add(&mut a, n2, n1, -y);
                    }
                }
                Ce::V { ac: Some((m, ph)), br, .. } => b[*br] = C64::from_polar(*m, *ph),
                Ce::I { from, to, ac: Some((m, ph)), .. } => {
                    let i = C64::from_polar(*m, *ph);
                    if *from != 0 {
                        b[from - 1] = b[from - 1] - i;
                    }
                    if *to != 0 {
                        b[to - 1] = b[to - 1] + i;
                    }
                }
                _ => {}
            }
        }
        let mut x = b;
        if solve_complex(n, &mut a, &mut x).is_none() {
            return Err(SimError::Invalid(format!("周波数 {f:.4e} Hz で方程式が解けません")));
        }
        let vn = |node: usize| if node == 0 { C64::ZERO } else { x[node - 1] };
        for i in 1..n_nodes {
            signals[i - 1].re.push(vn(i).re);
            signals[i - 1].im.push(vn(i).im);
        }
        for (k, e) in current_elems.iter().enumerate() {
            let cur = match e {
                Ce::R { a, b, g, .. } => (vn(*a) - vn(*b)) * C64::new(*g, 0.0),
                Ce::C { a, b, c, .. } => (vn(*a) - vn(*b)) * jw_c(w) * C64::new(*c, 0.0),
                Ce::L { br, .. } | Ce::V { br, .. } | Ce::E { br, .. } => x[*br],
                Ce::G { cp, cn, gm, .. } => (vn(*cp) - vn(*cn)) * C64::new(*gm, 0.0),
                Ce::I { ac, .. } => ac.map_or(C64::ZERO, |(m, p)| C64::from_polar(m, p)),
                _ => C64::ZERO,
            };
            let s = &mut signals[n_nodes - 1 + k];
            s.re.push(cur.re);
            s.im.push(cur.im);
        }
    }
    Ok(AcResult { freq: freqs, signals, warnings })
}
