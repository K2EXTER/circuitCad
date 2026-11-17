//! `nyquist` (the marks of a loop gain's Nyquist and Nichols charts) and `locus` (the root locus: the poles of a transfer
//! function as one part's value is stepped).

use crate::session::{obj, opt, options, Session};
use crate::Res;
use serde_json::json;

fn freq(s: &str) -> Result<f64, String> {
    sim_core::parse_value(s).filter(|v| *v > 0.0).ok_or_else(|| format!("'{s}' is not a positive frequency (write 10, 1k, 100k ...)"))
}

impl Session {
    /// `nyquist PART F0 F1 [ppd=10] [from=NET]`: the loop gain as the Nyquist chart sees it: the crossovers as points of T, the
    /// point nearest to -1 (the modulus margin |1+T|) and the sensitivity peak.
    pub fn nyquist(&mut self, args: &[String]) -> Res {
        let (pos, kv) = options(args);
        if pos.len() < 3 {
            return Err("nyquist PART FSTART FSTOP [ppd=10] [from=NET] [points=0]   (the loop gain of 'loopgain' as a Nyquist chart: where the curve crosses the unit circle and the negative real axis, and how near it comes to -1; points=N also lists N points of T for drawing)".into());
        }
        let (f0, f1) = (freq(&pos[1])?, freq(&pos[2])?);
        let ppd: usize = opt(&kv, "ppd").and_then(|s| s.parse().ok()).unwrap_or(10);
        let gate = self.erc_gate()?;
        let c = self.circuit()?;
        let dir = sim_core::BreakDirection::from_net(&c, &pos[0], opt(&kv, "from").unwrap_or(""))?;
        let r = sim_core::loop_gain(&c, &pos[0], dir, &sim_core::AcSettings { fstart: f0, fstop: f1, sweep: sim_core::AcSweep::Decade(ppd) }).map_err(|e| e.to_string())?;
        let m = r.marks();
        let pt = |f: f64, t: sim_core::C64| json!({"hz": f, "re": t.re, "im": t.im, "db": 20.0 * t.abs().max(1e-15).log10(), "phase_deg": t.arg_deg()});
        let modulus = m.nearest.map(|n| n.2);
        let verdict = match modulus {
            Some(d) if d >= 0.5 => "the curve keeps well clear of -1 (|1+T| 0.5 or more: a disturbance is amplified at most 2 times)",
            Some(d) if d >= 0.3 => "the curve comes near -1 (|1+T| 0.3 to 0.5: some peaking in the closed loop)",
            Some(_) => "the curve passes close to -1 (|1+T| under 0.3: the closed loop rings or oscillates)",
            None => "no points",
        };
        let n: usize = opt(&kv, "points").and_then(|s| s.parse().ok()).unwrap_or(0);
        let points: Vec<_> = if n == 0 { vec![] } else { (0..n).map(|i| i * (r.freq.len() - 1) / (n - 1).max(1)).map(|k| pt(r.freq[k], r.t[k])).collect() };
        Ok(obj(json!({
            "circuit_ok": gate.is_none(), "cut_part": r.element, "signal_goes_from": r.from_net, "signal_goes_to": r.to_net,
            "gain_crossover": m.gain_cross.map(|(f, t)| pt(f, t)), "phase_crossover": m.phase_cross.map(|(f, t)| pt(f, t)),
            "nearest_to_minus_one": m.nearest.map(|(f, t, d)| json!({"hz": f, "re": t.re, "im": t.im, "distance": d})),
            "modulus_margin": modulus, "sensitivity_peak_db": m.sensitivity_peak_db(), "verdict": verdict, "points": points,
            "warnings": r.warnings.iter().cloned().chain(gate).collect::<Vec<_>>()
        })))
    }

    /// `locus PART FROM TO OUT F0 F1 [in=NET] [n=15] [lin] [ppd=20] [temp=27]`: the root locus of V(OUT)/V(in) as PART goes from
    /// FROM to TO.
    pub fn locus(&mut self, args: &[String]) -> Res {
        let (pos, kv) = options(args);
        if pos.len() < 6 {
            return Err("locus PART FROM TO OUT FSTART FSTOP [in=NET] [n=15] [lin] [ppd=20] [temp=27]   (root locus: PART (a resistor, capacitor or coil) is stepped from FROM to TO (by ratio, or evenly with 'lin'), and at each value the poles of V(OUT)/V(in) are fitted as 'pz' does; says the first value where a pole reaches the right half plane)".into());
        }
        let num = |s: &str| sim_core::parse_value(s).filter(|v| *v > 0.0).ok_or_else(|| format!("'{s}' is not a positive value"));
        let (a, b) = (num(&pos[1])?, num(&pos[2])?);
        let (f0, f1) = (freq(&pos[4])?, freq(&pos[5])?);
        let n: usize = opt(&kv, "n").and_then(|s| s.parse().ok()).unwrap_or(15);
        let ppd: usize = opt(&kv, "ppd").and_then(|s| s.parse().ok()).unwrap_or(20);
        let temp: f64 = opt(&kv, "temp").and_then(|s| s.parse().ok()).unwrap_or(27.0);
        let lin = pos.iter().skip(6).any(|w| w == "lin");
        let gate = self.erc_gate()?;
        let s = sim_core::LocusSettings {
            refdes: pos[0].clone(),
            values: sim_core::sweep_values(a, b, n, !lin),
            output: pos[3].clone(),
            input: opt(&kv, "in").map(str::to_string),
            ac: sim_core::AcSettings { fstart: f0, fstop: f1, sweep: sim_core::AcSweep::Decade(ppd) },
            temp_c: temp,
            max_poles: 6,
            tolerance: 0.02,
        };
        let r = sim_core::root_locus(&self.sch, &s).map_err(|e| e.to_string())?;
        let hz = |p: &sim_core::C64| json!({"real_hz": p.re / std::f64::consts::TAU, "imag_hz": p.im / std::f64::consts::TAU});
        let steps: Vec<_> = r
            .steps
            .iter()
            .map(|st| json!({"value": st.value, "poles": st.pole_points().iter().map(hz).collect::<Vec<_>>(), "min_damping": st.min_damping(), "unstable": st.unstable(), "fit_error_percent": st.error * 100.0}))
            .collect();
        let first_unstable = r.first_unstable();
        let verdict = match first_unstable {
            Some(_) => "a pole crosses into the right half plane inside the range (from that value on the circuit oscillates)",
            None if r.steps[0].unstable() => "a pole is in the right half plane already at the first value",
            None => "every pole stays in the left half plane over the range (stable)",
        };
        Ok(obj(json!({
            "circuit_ok": gate.is_none(), "part": r.refdes, "steps": steps, "paths": r.branches().len(), "first_unstable_value": first_unstable,
            "failed": r.failed.iter().map(|(v, why)| json!({"value": v, "why": why})).collect::<Vec<_>>(), "verdict": verdict,
            "warnings": r.warnings.iter().cloned().chain(gate).collect::<Vec<_>>()
        })))
    }
}
