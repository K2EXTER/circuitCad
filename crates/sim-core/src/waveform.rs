//! Independent-source waveforms: DC, SIN, PULSE, PWL (SPICE conventions).

use crate::value::parse_value;

#[derive(Clone, Debug, PartialEq)]
pub enum Waveform {
    Dc(f64),
    /// offset + amplitude * exp(-(t-td)*theta) * sin(2*pi*freq*(t-td) + phase), phase in degrees
    Sin { vo: f64, va: f64, freq: f64, td: f64, theta: f64, phase: f64 },
    /// `pw` and `per` are infinite when omitted.
    Pulse { v1: f64, v2: f64, td: f64, tr: f64, tf: f64, pw: f64, per: f64 },
    Pwl(Vec<(f64, f64)>),
}

impl Waveform {
    /// Value used for the DC operating point.
    pub fn dc_value(&self) -> f64 {
        match self {
            Waveform::Dc(v) => *v,
            Waveform::Sin { vo, .. } => *vo,
            Waveform::Pulse { v1, .. } => *v1,
            Waveform::Pwl(p) => p.first().map_or(0.0, |x| x.1),
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
        }
        v
    }
}

/// Parse a source value: `5`, `DC 5`, `SIN(0 1 1k [td theta phase])`,
/// `PULSE(v1 v2 [td tr tf pw per])`, `PWL(t1 v1 t2 v2 ...)`.
pub fn parse_source(s: &str) -> Result<Waveform, String> {
    let t = s.trim();
    let lower = t.to_ascii_lowercase();
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
        "pwl" => {
            let n = nums(rest)?;
            if n.len() < 2 || n.len() % 2 != 0 {
                return Err("PWL(時間 値 時間 値 ...) は時間と値の組で指定してください".into());
            }
            let pts: Vec<(f64, f64)> = n.chunks(2).map(|c| (c[0], c[1])).collect();
            if pts.windows(2).any(|w| w[1].0 <= w[0].0) {
                return Err("PWL の時間は増加させてください".into());
            }
            Ok(Waveform::Pwl(pts))
        }
        other => Err(format!("未対応の電源波形 {other} (SIN / PULSE / PWL / DC)")),
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
        assert!(parse_source("EXP(1 2)").is_err());
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
}
