//! Small-signal noise analysis (thermal + shot noise), SPICE `.noise` style.
//!
//! Every noisy element becomes an uncorrelated current source `S_i` (A^2/Hz) across two nodes.
//! One adjoint solve `A^T y = e_out` per frequency gives the transfer from any such source to the
//! output, so the output noise is `sum |H_i|^2 S_i` without one solve per source.
//!
//! Modelled sources: resistor thermal `4kT/R`; diode shot `2q|Id|` and series-resistance thermal;
//! BJT collector/base shot `2q|Ic|`, `2q|Ib|`; MOSFET channel thermal `4kT max(2/3 gm, gds)`.
//! Capacitors, inductors and controlled sources are noiseless.

use crate::ac::{check_sweep, frequencies, solve_complex, AcSettings, C64};
use crate::circuit::Circuit;
use crate::dc::{bjt_eval, compile, exp_safe, floating_warnings, has_ground, mos_eval, solve_op, v, Ce, Ctx, SimError, Strategy, GMIN};
use std::f64::consts::PI;

const K_B: f64 = 1.380649e-23;
const Q_E: f64 = 1.602176634e-19;

#[derive(Clone, Debug)]
pub struct NoiseSettings {
    pub sweep: AcSettings,
    /// Output node name; with `output_ref` the output is the difference of the two.
    pub output: String,
    pub output_ref: Option<String>,
    /// Name of a V or I source to refer the noise to the input (optional).
    pub input: Option<String>,
    /// Temperature in degrees Celsius.
    pub temp_c: f64,
}

#[derive(Clone, Debug)]
pub struct NoiseContribution {
    pub name: String,
    /// Output-referred power spectral density, V^2/Hz.
    pub psd: Vec<f64>,
    /// RMS over the swept band, V.
    pub rms: f64,
}

#[derive(Clone, Debug)]
pub struct NoiseResult {
    pub freq: Vec<f64>,
    /// Total output noise PSD, V^2/Hz.
    pub out_psd: Vec<f64>,
    /// Per-element contributions, largest total first.
    pub contributions: Vec<NoiseContribution>,
    /// |Vout / source| when an input source was given.
    pub gain: Option<Vec<f64>>,
    /// Input-referred PSD (V^2/Hz for a voltage source, A^2/Hz for a current source).
    pub in_psd: Option<Vec<f64>>,
    pub out_rms: f64,
    pub in_rms: Option<f64>,
    pub warnings: Vec<String>,
}

impl NoiseResult {
    /// Output noise density in V/√Hz.
    pub fn out_density(&self) -> Vec<f64> {
        self.out_psd.iter().map(|p| p.sqrt()).collect()
    }
    pub fn in_density(&self) -> Option<Vec<f64>> {
        self.in_psd.as_ref().map(|v| v.iter().map(|p| p.sqrt()).collect())
    }
    pub fn contribution(&self, name: &str) -> Option<&NoiseContribution> {
        self.contributions.iter().find(|c| c.name == name)
    }
}

/// Integral of a PSD over the sweep, interpolating each segment as a power law in f
/// (exact for flat and 1/f parts, good for roll-offs on a log grid).
pub fn integrate_psd(freq: &[f64], psd: &[f64]) -> f64 {
    let mut total = 0.0;
    for k in 1..freq.len() {
        let (f0, f1, s0, s1) = (freq[k - 1], freq[k], psd[k - 1], psd[k]);
        if f1 <= f0 {
            continue;
        }
        total += if s0 > 0.0 && s1 > 0.0 && f0 > 0.0 {
            let alpha = (s1 / s0).ln() / (f1 / f0).ln();
            if (alpha + 1.0).abs() < 1e-9 {
                s0 * f0 * (f1 / f0).ln()
            } else {
                s0 * f0 / (alpha + 1.0) * ((f1 / f0).powf(alpha + 1.0) - 1.0)
            }
        } else {
            0.5 * (s0 + s1) * (f1 - f0)
        };
    }
    total
}

struct Src {
    name: String,
    /// Noise current flows from `p` to `n`.
    p: usize,
    n: usize,
    /// PSD in A^2/Hz.
    s: f64,
}

fn node_index(c: &Circuit, name: &str) -> Result<usize, SimError> {
    let t = name.trim();
    if t == "0" || t.eq_ignore_ascii_case("gnd") {
        return Ok(0);
    }
    c.names.iter().position(|n| n == t).ok_or_else(|| SimError::Invalid(format!("出力ノード {t} が見つかりません")))
}

pub fn noise_analysis(circuit: &Circuit, st: &NoiseSettings) -> Result<NoiseResult, SimError> {
    check_sweep(&st.sweep)?;
    if !(st.temp_c.is_finite() && st.temp_c > -273.15) {
        return Err(SimError::Invalid("温度は絶対零度より高くしてください".into()));
    }
    if !has_ground(circuit) {
        return Err(SimError::NoGround);
    }
    let op_node = node_index(circuit, &st.output)?;
    let on_node = match &st.output_ref {
        Some(n) => node_index(circuit, n)?,
        None => 0,
    };
    if op_node == on_node {
        return Err(SimError::Invalid("出力ノードが GND と同じです".into()));
    }
    let warnings = floating_warnings(circuit);
    let cm = compile(circuit);
    let (op, _) = solve_op(&cm, &[Strategy::Direct, Strategy::Gmin, Strategy::Source])?;
    let mut vj = op.vj.clone();
    let (jac, _) = cm.assemble(&mut vj, &op.x, &Ctx::dc(GMIN, 1.0));
    let n = cm.n_unknowns;

    // input source: where its unit excitation enters the system
    enum Inp {
        Branch(usize),
        Current(usize, usize),
    }
    let input = match &st.input {
        None => None,
        Some(name) => {
            let found = cm.ce.iter().find_map(|e| match e {
                Ce::V { name: nm, br, .. } if nm == name => Some(Inp::Branch(*br)),
                Ce::I { name: nm, from, to, .. } if nm == name => Some(Inp::Current(*from, *to)),
                _ => None,
            });
            Some(found.ok_or_else(|| SimError::Invalid(format!("入力ソース {name} が見つかりません (V/I 電源の名前を指定)")))?)
        }
    };

    // ---- noise sources
    let kt4 = 4.0 * K_B * (st.temp_c + 273.15);
    let mut srcs: Vec<Src> = vec![];
    for e in &cm.ce {
        match e {
            Ce::R { name, a, b, g } => srcs.push(Src { name: name.clone(), p: *a, n: *b, s: kt4 * g }),
            Ce::D { name, a, ai, k, rs, is, nvt, bv, ibv, slot, .. } => {
                let lim = op.vj[*slot];
                let mut id = is * (exp_safe(lim / nvt) - 1.0);
                if bv.is_finite() {
                    id -= ibv * exp_safe(-(lim + bv) / nvt);
                }
                srcs.push(Src { name: name.clone(), p: *ai, n: *k, s: 2.0 * Q_E * id.abs() });
                if *rs > 0.0 {
                    srcs.push(Src { name: format!("{name}.rs"), p: *a, n: *ai, s: kt4 / rs });
                }
            }
            Ce::Q { name, c, b, bi, e, m, slot } => {
                let q = bjt_eval(m, op.vj[*slot], op.vj[*slot + 1]);
                srcs.push(Src { name: format!("{name}.ic"), p: *c, n: *e, s: 2.0 * Q_E * q.ic.abs() });
                srcs.push(Src { name: format!("{name}.ib"), p: *bi, n: *e, s: 2.0 * Q_E * q.ib.abs() });
                if m.rb > 0.0 {
                    srcs.push(Src { name: format!("{name}.rb"), p: *b, n: *bi, s: kt4 / m.rb });
                }
            }
            Ce::M { name, d, g, s, m } => {
                let pol = if m.pmos { -1.0 } else { 1.0 };
                let (vgs, vds) = (v(&op.x, *g) - v(&op.x, *s), v(&op.x, *d) - v(&op.x, *s));
                let (_, gm, gds) = mos_eval(m, pol * vgs, pol * vds);
                let g_n = (2.0 / 3.0 * gm.abs()).max(gds.abs());
                srcs.push(Src { name: name.clone(), p: *d, n: *s, s: kt4 * g_n });
            }
            Ce::C { .. } | Ce::L { .. } | Ce::V { .. } | Ce::I { .. } | Ce::E { .. } | Ce::G { .. } => {}
        }
    }

    // ---- sweep
    let freqs = frequencies(&st.sweep);
    let mut contributions: Vec<NoiseContribution> = srcs.iter().map(|s| NoiseContribution { name: s.name.clone(), psd: Vec::with_capacity(freqs.len()), rms: 0.0 }).collect();
    let mut out_psd = Vec::with_capacity(freqs.len());
    let mut gain: Vec<f64> = vec![];
    for &f in &freqs {
        let w = 2.0 * PI * f;
        // A^T for the adjoint system
        let mut a = vec![C64::ZERO; n * n];
        for r in 0..n {
            for c in 0..n {
                a[c * n + r] = C64::new(jac.a[r * n + c], 0.0);
            }
        }
        // transposed stamp: A^T[col][row]
        let add = |a: &mut Vec<C64>, r: usize, c: usize, val: C64| {
            if r != 0 && c != 0 {
                a[(c - 1) * n + (r - 1)] = a[(c - 1) * n + (r - 1)] + val;
            }
        };
        for e in &cm.ce {
            match e {
                Ce::C { a: na, b: nb, c, .. } => {
                    let y = C64::new(0.0, w * c);
                    add(&mut a, *na, *na, y);
                    add(&mut a, *nb, *nb, y);
                    add(&mut a, *na, *nb, -y);
                    add(&mut a, *nb, *na, -y);
                }
                Ce::L { l, br, .. } => {
                    a[*br * n + *br] = a[*br * n + *br] - C64::new(0.0, w * l);
                }
                _ => {}
            }
        }
        let mut y = vec![C64::ZERO; n];
        if op_node != 0 {
            y[op_node - 1] = C64::new(1.0, 0.0);
        }
        if on_node != 0 {
            y[on_node - 1] = y[on_node - 1] - C64::new(1.0, 0.0);
        }
        if solve_complex(n, &mut a, &mut y).is_none() {
            return Err(SimError::Invalid(format!("周波数 {f:.4e} Hz で方程式が解けません")));
        }
        let yn = |node: usize| if node == 0 { C64::ZERO } else { y[node - 1] };
        let mut total = 0.0;
        for (s, c) in srcs.iter().zip(contributions.iter_mut()) {
            // current p -> n through the source: b[p] -= i, b[n] += i
            let h = yn(s.n) - yn(s.p);
            let p = h.abs().powi(2) * s.s;
            c.psd.push(p);
            total += p;
        }
        out_psd.push(total);
        if let Some(inp) = &input {
            let h = match inp {
                Inp::Branch(br) => y[*br],
                Inp::Current(from, to) => yn(*to) - yn(*from),
            };
            gain.push(h.abs());
        }
    }

    for c in contributions.iter_mut() {
        c.rms = integrate_psd(&freqs, &c.psd).sqrt();
    }
    contributions.sort_by(|a, b| b.rms.total_cmp(&a.rms));
    let out_rms = integrate_psd(&freqs, &out_psd).sqrt();
    let (gain, in_psd, in_rms) = if input.is_some() {
        let ip: Vec<f64> = out_psd.iter().zip(&gain).map(|(p, g)| if *g > 0.0 { p / (g * g) } else { f64::INFINITY }).collect();
        let r = integrate_psd(&freqs, &ip).sqrt();
        (Some(gain), Some(ip), Some(r))
    } else {
        (None, None, None)
    };
    Ok(NoiseResult { freq: freqs, out_psd, contributions, gain, in_psd, out_rms, in_rms, warnings })
}

/// Name of the element a noise source belongs to ("Q1.ic" -> "Q1").
pub fn source_element(name: &str) -> &str {
    name.split('.').next().unwrap_or(name)
}
