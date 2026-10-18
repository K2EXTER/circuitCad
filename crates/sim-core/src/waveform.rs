//! Independent-source waveforms: DC, SIN, PULSE, PWL (SPICE conventions).

use crate::value::parse_value;

#[derive(Clone, Debug, PartialEq)]
pub enum Waveform {
    Dc(f64),
    /// offset + amplitude * exp(-(t-td)*theta) * sin(2*pi*freq*(t-td) + phase), phase in degrees
    Sin { vo: f64, va: f64, freq: f64, td: f64, theta: f64, phase: f64 },
    /// `pw` and `per` are infinite when omitted.
    Pulse { v1: f64, v2: f64, td: f64, tr: f64, tf: f64, pw: f64, per: f64 },
    /// EXP(v1 v2 td1 tau1 td2 tau2): rises towards v2 from td1, falls back towards v1 from td2.
    Exp { v1: f64, v2: f64, td1: f64, tau1: f64, td2: f64, tau2: f64 },
    Pwl(Vec<(f64, f64)>),
    /// Band-limited pseudo-random Gaussian noise: independent samples every `1/(2*fmax)`,
    /// linearly interpolated, scaled so that the RMS value is `vrms`. Deterministic for a
    /// given `seed`, so runs are reproducible.
    Noise { vrms: f64, fmax: f64, seed: u64, offset: f64 },
    /// Sum of waveforms, e.g. a DC level plus ripple plus noise.
    Sum(Vec<Waveform>),
}

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Standard normal value number `k` of stream `seed` (stateless, so any time can be evaluated).
fn gauss(seed: u64, k: i64) -> f64 {
    let h = splitmix64(seed ^ (k as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93));
    let h2 = splitmix64(h);
    // (0, 1] and [0, 1)
    let u1 = ((h >> 11) as f64 + 1.0) / (1u64 << 53) as f64;
    let u2 = (h2 >> 11) as f64 / (1u64 << 53) as f64;
    (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
}

impl Waveform {
    /// Value used for the DC operating point.
    pub fn dc_value(&self) -> f64 {
        match self {
            Waveform::Dc(v) => *v,
            Waveform::Sin { vo, .. } => *vo,
            Waveform::Pulse { v1, .. } | Waveform::Exp { v1, .. } => *v1,
            Waveform::Pwl(p) => p.first().map_or(0.0, |x| x.1),
            Waveform::Noise { offset, .. } => *offset,
            Waveform::Sum(parts) => parts.iter().map(|w| w.dc_value()).sum(),
        }
    }

    /// Spacing of the independent noise samples, if this waveform contains noise.
    pub fn noise_period(&self) -> Option<f64> {
        match self {
            Waveform::Noise { fmax, .. } => Some(0.5 / fmax),
            Waveform::Sum(parts) => parts.iter().filter_map(|w| w.noise_period()).reduce(f64::min),
            _ => None,
        }
    }

    pub fn eval(&self, t: f64) -> f64 {
        match self {
            Waveform::Dc(v) => *v,
            Waveform::Sin { vo, va, freq, td, theta, phase } => {
                let ph = phase.to_radians();
                if t < *td {
                    vo + va * ph.sin()
                } else {
                    let dt = t - td;
                    vo + va * (-dt * theta).exp() * (2.0 * std::f64::consts::PI * freq * dt + ph).sin()
                }
            }
            Waveform::Pulse { v1, v2, td, tr, tf, pw, per } => {
                if t < *td {
                    return *v1;
                }
                let mut tt = t - td;
                if per.is_finite() && *per > 0.0 {
                    tt %= per;
                }
                if tt < *tr {
                    v1 + (v2 - v1) * tt / tr
                } else if tt < tr + pw {
                    *v2
                } else if tt < tr + pw + tf {
                    v2 + (v1 - v2) * (tt - tr - pw) / tf
                } else {
                    *v1
                }
            }
            Waveform::Exp { v1, v2, td1, tau1, td2, tau2 } => {
                let mut v = *v1;
                if t > *td1 {
                    v += (v2 - v1) * (1.0 - (-(t - td1) / tau1).exp());
                }
                if t > *td2 {
                    v += (v1 - v2) * (1.0 - (-(t - td2) / tau2).exp());
                }
                v
            }
            Waveform::Pwl(p) => {
                let (first, last) = match (p.first(), p.last()) {
                    (Some(f), Some(l)) => (f, l),
                    _ => return 0.0,
                };
                if t <= first.0 {
                    return first.1;
                }
                if t >= last.0 {
                    return last.1;
                }
                let i = p.partition_point(|x| x.0 <= t);
                let ((t0, v0), (t1, v1)) = (p[i - 1], p[i]);
                v0 + (v1 - v0) * (t - t0) / (t1 - t0)
            }
            Waveform::Noise { vrms, fmax, seed, offset } => {
                let x = t * 2.0 * fmax;
                let k = x.floor();
                let f = x - k;
                let (n0, n1) = (gauss(*seed, k as i64), gauss(*seed, k as i64 + 1));
                // linear interpolation of unit white samples has mean-square 2/3: rescale to vrms
                offset + vrms * 1.5f64.sqrt() * (n0 + (n1 - n0) * f)
            }
            Waveform::Sum(parts) => parts.iter().map(|w| w.eval(t)).sum(),
        }
    }

    /// Times at which the waveform has a corner; the integrator lands exactly on them.
    pub fn breakpoints(&self, tstop: f64) -> Vec<f64> {
        let mut v = vec![];
        match self {
            Waveform::Dc(_) => {}
            Waveform::Sin { td, .. } => {
                if *td > 0.0 {
                    v.push(*td);
                }
            }
            Waveform::Exp { td1, td2, .. } => v.extend([*td1, *td2].into_iter().filter(|t| *t > 0.0 && *t <= tstop)),
            Waveform::Pulse { td, tr, tf, pw, per, .. } => {
                let periodic = per.is_finite() && *per > 0.0;
                let mut k = 0usize;
                loop {
                    let base = td + k as f64 * if periodic { *per } else { 0.0 };
                    if base > tstop || k > 200_000 {
                        break;
                    }
                    for dt in [0.0, *tr, tr + pw, tr + pw + tf] {
                        let t = base + dt;
                        if t.is_finite() && t <= tstop {
                            v.push(t);
                        }
                    }
                    if !periodic {
                        break;
                    }
                    k += 1;
                }
            }
            Waveform::Pwl(p) => v.extend(p.iter().map(|x| x.0).filter(|t| *t <= tstop)),
            Waveform::Noise { .. } => {}
            Waveform::Sum(parts) => {
                for w in parts {
                    v.extend(w.breakpoints(tstop));
                }
            }
        }
        v
    }
}

/// A source value as typed: the time-domain waveform plus an optional AC small-signal
/// phasor (magnitude, phase in degrees) for AC analysis.
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub wave: Waveform,
    pub ac: Option<(f64, f64)>,
}

/// Position of a standalone `ac` word outside parentheses.
fn find_ac(t: &str) -> Option<usize> {
    let b = t.as_bytes();
    let mut depth = 0;
    for i in 0..b.len() {
        match b[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            c if depth == 0 && (c == b'a' || c == b'A') && i + 1 < b.len() && (b[i + 1] == b'c' || b[i + 1] == b'C') => {
                let before_ok = i == 0 || b[i - 1].is_ascii_whitespace() || b[i - 1] == b')';
                let after_ok = i + 2 == b.len() || b[i + 2].is_ascii_whitespace();
                if before_ok && after_ok {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Parse `5`, `5 AC 1`, `AC 1 90`, `SIN(0 1 1k) AC 1`, ... (AC magnitude defaults to 1).
pub fn parse_source_full(s: &str) -> Result<Source, String> {
    let t = s.trim();
    match find_ac(t) {
        None => Ok(Source { wave: parse_source(t)?, ac: None }),
        Some(i) => {
            let left = t[..i].trim();
            let wave = if left.is_empty() { Waveform::Dc(0.0) } else { parse_source(left)? };
            let nums: Result<Vec<f64>, String> = t[i + 2..]
                .split_whitespace()
                .map(|w| parse_value(w).ok_or_else(|| format!("AC の値「{w}」を数値にできません")))
                .collect();
            let nums = nums?;
            let ac = match nums.as_slice() {
                [] => (1.0, 0.0),
                [m] => (*m, 0.0),
                [m, p] => (*m, *p),
                _ => return Err("AC は「AC 大きさ [位相(度)]」の形式で指定してください".into()),
            };
            Ok(Source { wave, ac: Some(ac) })
        }
    }
}

/// Parse a source value: `5`, `DC 5`, `SIN(0 1 1k [td theta phase])`,
/// `PULSE(v1 v2 [td tr tf pw per])`, `PWL(t1 v1 t2 v2 ...)`.
pub fn parse_source(s: &str) -> Result<Waveform, String> {
    let t = s.trim();
    let parts = split_sum(t);
    if parts.len() > 1 {
        return Ok(Waveform::Sum(parts.iter().map(|p| parse_single(p)).collect::<Result<_, _>>()?));
    }
    parse_single(t)
}

/// Split `a + b + c` at top-level `+` that follows whitespace or `)` (so `1e+3` and `+5` stay whole).
fn split_sum(t: &str) -> Vec<&str> {
    let b = t.as_bytes();
    let (mut depth, mut start) = (0i32, 0usize);
    let mut out = vec![];
    for i in 0..b.len() {
        match b[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b'+' if depth == 0 && i > 0 && (b[i - 1].is_ascii_whitespace() || b[i - 1] == b')') && !t[start..i].trim().is_empty() => {
                out.push(t[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(t[start..].trim());
    out
}

fn parse_single(s: &str) -> Result<Waveform, String> {
    let t = s.trim();
    let lower = t.to_ascii_lowercase();
    // `DC=1.21` is `DC 1.21`
    if let Some(rest) = lower.strip_prefix("dc=") {
        return parse_value(rest.trim()).map(Waveform::Dc).ok_or_else(|| format!("値「{t}」を数値にできません"));
    }
    // `DC 0 PULSE(...)`: the DC value is only the level before the waveform starts; the waveform is what runs
    if lower.starts_with("dc ")
        && let Some((first, after)) = t[3..].trim_start().split_once(char::is_whitespace)
        && parse_value(first).is_some()
        && after.trim_start().starts_with(|c: char| c.is_ascii_alphabetic())
    {
        return parse_single(after);
    }
    let (kind, rest) = match lower.find(|c: char| c == '(' || c.is_whitespace()) {
        Some(i) if !lower[..i].chars().next().is_some_and(|c| c.is_ascii_digit() || c == '-' || c == '+' || c == '.') => {
            (lower[..i].to_string(), &t[i..])
        }
        _ => (String::new(), t),
    };
    let nums = |rest: &str| -> Result<Vec<f64>, String> {
        rest.split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',')
            .filter(|w| !w.is_empty())
            .map(|w| parse_value(w).ok_or_else(|| format!("「{w}」を数値にできません")))
            .collect()
    };
    match kind.as_str() {
        "" | "dc" => {
            let n = nums(rest)?;
            match n.as_slice() {
                [v] => Ok(Waveform::Dc(*v)),
                _ => Err(format!("値「{t}」を数値にできません")),
            }
        }
        "sin" => {
            let n = nums(rest)?;
            if n.len() < 3 || n.len() > 6 {
                return Err("SIN(オフセット 振幅 周波数 [遅延 減衰 位相]) の形式で指定してください".into());
            }
            let g = |i: usize| n.get(i).copied().unwrap_or(0.0);
            if n[2] <= 0.0 {
                return Err("SIN の周波数は正の値にしてください".into());
            }
            Ok(Waveform::Sin { vo: n[0], va: n[1], freq: n[2], td: g(3), theta: g(4), phase: g(5) })
        }
        "pulse" => {
            let n = nums(rest)?;
            if n.len() < 2 || n.len() > 7 {
                return Err("PULSE(初期値 パルス値 [遅延 立上り 立下り 幅 周期]) の形式で指定してください".into());
            }
            let tr = n.get(3).copied().unwrap_or(1e-9).max(1e-12);
            let tf = n.get(4).copied().unwrap_or(1e-9).max(1e-12);
            Ok(Waveform::Pulse {
                v1: n[0],
                v2: n[1],
                td: n.get(2).copied().unwrap_or(0.0),
                tr,
                tf,
                pw: n.get(5).copied().unwrap_or(f64::INFINITY),
                per: n.get(6).copied().unwrap_or(f64::INFINITY),
            })
        }
        "noise" => {
            let n = nums(rest)?;
            if n.len() < 2 || n.len() > 4 {
                return Err("NOISE(実効値 最高周波数 [シード] [オフセット]) の形式で指定してください".into());
            }
            if n[0] < 0.0 || n[1] <= 0.0 {
                return Err("NOISE の実効値は0以上、最高周波数は正の値にしてください".into());
            }
            Ok(Waveform::Noise { vrms: n[0], fmax: n[1], seed: n.get(2).map_or(1, |s| *s as u64), offset: n.get(3).copied().unwrap_or(0.0) })
        }
        "exp" => {
            let n = nums(rest)?;
            // LTspice accepts a seventh number (ignored here)
            if n.len() < 2 || n.len() > 7 {
                return Err("EXP(初期値 最終値 [上昇の開始 時定数 下降の開始 時定数]) の形式で指定してください".into());
            }
            let g = |i: usize, d: f64| n.get(i).copied().unwrap_or(d);
            let (td1, tau1) = (g(2, 0.0), g(3, 1e-6));
            let (td2, tau2) = (g(4, td1 + 1e-6), g(5, 1e-6));
            if tau1 <= 0.0 || tau2 <= 0.0 {
                return Err("EXP の時定数は正の値にしてください".into());
            }
            Ok(Waveform::Exp { v1: n[0], v2: n[1], td1, tau1, td2, tau2 })
        }
        "pwl" => {
            let n = nums(rest)?;
            if n.len() < 2 || n.len() % 2 != 0 {
                return Err("PWL(時間 値 時間 値 ...) は時間と値の組で指定してください".into());
            }
            let mut pts: Vec<(f64, f64)> = n.chunks(2).map(|c| (c[0], c[1])).collect();
            if pts.windows(2).any(|w| w[1].0 < w[0].0) {
                return Err("PWL の時間は増加させてください".into());
            }
            // two points at the same time are a vertical step (`pwl(0 0 0 1 ...)`): make the edge 1 ps long
            for i in 1..pts.len() {
                if pts[i].0 <= pts[i - 1].0 {
                    pts[i].0 = pts[i - 1].0 + 1e-12;
                }
            }
            Ok(Waveform::Pwl(pts))
        }
        other => Err(format!("未対応の電源波形 {other} (SIN / PULSE / PWL / NOISE / DC)")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_variants() {
        assert_eq!(parse_source("5").unwrap(), Waveform::Dc(5.0));
        assert_eq!(parse_source("DC 3.3").unwrap(), Waveform::Dc(3.3));
        assert_eq!(parse_source("12m").unwrap(), Waveform::Dc(0.012));
        assert_eq!(
            parse_source("SIN(0 1 1k)").unwrap(),
            Waveform::Sin { vo: 0.0, va: 1.0, freq: 1000.0, td: 0.0, theta: 0.0, phase: 0.0 }
        );
        match parse_source("pulse(0 5 1m 1u 1u 2m 5m)").unwrap() {
            Waveform::Pulse { v2, td, tr, pw, per, .. } => assert_eq!((v2, td, tr, pw, per), (5.0, 1e-3, 1e-6, 2e-3, 5e-3)),
            w => panic!("{w:?}"),
        }
        assert_eq!(parse_source("PWL(0 0, 1m 5, 2m 0)").unwrap(), Waveform::Pwl(vec![(0.0, 0.0), (1e-3, 5.0), (2e-3, 0.0)]));
    }

    #[test]
    fn parse_errors() {
        assert!(parse_source("SIN(0 1)").is_err());
        assert!(parse_source("SIN(0 1 0)").is_err());
        assert!(parse_source("PWL(0 0 1m)").is_err());
        assert!(parse_source("PWL(1m 0 0 5)").is_err());
        assert!(parse_source("EXP(1)").is_err());
        assert!(parse_source("EXP(1 2 0 0)").is_err());
        assert!(parse_source("abc").is_err());
        assert!(parse_source("5 6").is_err());
    }

    #[test]
    fn evaluation() {
        let p = parse_source("PULSE(0 5 1m 1m 1m 2m 10m)").unwrap();
        let close = |a: f64, b: f64| assert!((a - b).abs() < 1e-9, "{a} vs {b}");
        close(p.eval(0.0), 0.0);
        close(p.eval(1.5e-3), 2.5); // mid rise
        close(p.eval(3e-3), 5.0);
        close(p.eval(4.5e-3), 2.5); // mid fall
        close(p.eval(6e-3), 0.0);
        close(p.eval(11.5e-3), 2.5); // second period
        close(p.dc_value(), 0.0);
        let s = parse_source("SIN(1 2 1k)").unwrap();
        close(s.eval(0.0), 1.0);
        close(s.eval(0.25e-3), 3.0);
        let w = parse_source("PWL(0 0 1 10)").unwrap();
        close(w.eval(0.5), 5.0);
        close(w.eval(5.0), 10.0);
    }

    #[test]
    fn breakpoints_cover_every_corner() {
        let p = parse_source("PULSE(0 5 1m 1u 1u 2m 5m)").unwrap();
        let b = p.breakpoints(8e-3);
        for t in [1e-3, 1.001e-3, 3.001e-3, 3.002e-3, 6e-3] {
            assert!(b.iter().any(|x| (x - t).abs() < 1e-12), "{t} missing in {b:?}");
        }
        assert!(b.iter().all(|&t| t <= 8e-3));
        assert!(Waveform::Dc(1.0).breakpoints(1.0).is_empty());
    }

    #[test]
    fn ac_specification() {
        let f = |t: &str| parse_source_full(t).unwrap();
        assert_eq!(f("5"), Source { wave: Waveform::Dc(5.0), ac: None });
        assert_eq!(f("5 AC 1"), Source { wave: Waveform::Dc(5.0), ac: Some((1.0, 0.0)) });
        assert_eq!(f("ac 2 90"), Source { wave: Waveform::Dc(0.0), ac: Some((2.0, 90.0)) });
        assert_eq!(f("AC"), Source { wave: Waveform::Dc(0.0), ac: Some((1.0, 0.0)) });
        let s = f("SIN(0 1 1k) AC 0.5");
        assert!(matches!(s.wave, Waveform::Sin { .. }) && s.ac == Some((0.5, 0.0)));
        assert!(parse_source_full("5 AC x").is_err());
        assert!(parse_source_full("5 AC 1 2 3").is_err());
        // a bare number or a name that merely contains "ac" is not an AC spec
        assert!(parse_source_full("pulse(0 5) ").is_ok());
    }

    #[test]
    fn noise_is_reproducible_gaussian_with_the_requested_rms() {
        let w = parse_source("NOISE(0.1 1k 7)").unwrap();
        assert_eq!(w.eval(0.0123), w.eval(0.0123));
        let other = parse_source("NOISE(0.1 1k 8)").unwrap();
        assert_ne!(w.eval(0.0123), other.eval(0.0123));
        // statistics over many independent sample times
        let n = 20000;
        let vals: Vec<f64> = (0..n).map(|k| w.eval(k as f64 * 0.5e-3 + 0.2e-3)).collect();
        let mean = vals.iter().sum::<f64>() / n as f64;
        let rms = (vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n as f64).sqrt();
        assert!(mean.abs() < 0.005, "mean {mean}");
        // sampling at a fixed phase inside the interval gives variance (1-x)^2+x^2 times 1.5
        let x = 0.4f64;
        let want = 0.1 * (1.5 * ((1.0 - x).powi(2) + x * x)).sqrt();
        assert!((rms - want).abs() / want < 0.04, "rms {rms} vs {want}");
        // continuous: neighbouring evaluations differ little
        assert!((w.eval(1e-3) - w.eval(1.000001e-3)).abs() < 1e-3);
        assert_eq!(w.dc_value(), 0.0);
        assert_eq!(w.noise_period(), Some(0.5e-3));
    }

    #[test]
    fn sums_combine_levels() {
        let w = parse_source("5 + SIN(0 1 1k) + NOISE(0.01 1Meg)").unwrap();
        assert!(matches!(&w, Waveform::Sum(p) if p.len() == 3));
        assert_eq!(w.dc_value(), 5.0);
        assert!((w.eval(0.25e-3) - 6.0).abs() < 0.1);
        // exponents and a leading sign are not sums
        assert_eq!(parse_source("1e+3").unwrap(), Waveform::Dc(1000.0));
        assert_eq!(parse_source("+5").unwrap(), Waveform::Dc(5.0));
        assert!(parse_source("NOISE(1)").is_err());
        assert!(parse_source("NOISE(-1 1k)").is_err());
        let w = parse_source_full("5 + NOISE(0.1 1Meg) AC 1").unwrap();
        assert!(w.ac == Some((1.0, 0.0)) && matches!(w.wave, Waveform::Sum(_)));
    }
}
