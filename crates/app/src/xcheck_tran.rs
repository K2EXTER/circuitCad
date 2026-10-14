//! Developer tool: cross-check our transient waveforms against ngspice.
//!
//! `circuitCad --xcheck <file.schjson | folder> --tran [--tstop S] [--tstep S] [--ngspice <exe>] [--out report.txt]`
//! The run length and step come from the source waveforms (a few periods) unless given.

use crate::xcheck::run_ngspice_script;
use sch_core::{extract, spice_node, to_spice_for, Dialect, Kind, Schematic};
use sim_core::{from_schematic, parse_source_full, transient, TranSettings, Waveform};
use std::path::{Path, PathBuf};

/// A node agrees when the RMS difference is below this fraction of the larger of its swing and `MIN_SPAN` volts.
const NRMSE_TOL: f64 = 0.01;
/// ... or, for edge-dominated signals, when every edge is within this fraction of a period.
const EDGE_TOL: f64 = 0.02;
const MIN_SPAN: f64 = 0.1;
/// Most output points per run (keeps both simulators quick).
const MAX_STEPS: f64 = 20_000.0;

#[derive(Debug, PartialEq)]
pub enum TranOutcome {
    Match { nodes: usize, tstop: f64, worst: String, nrmse: f64, ripple: String },
    Differ { nodes: usize, tstop: f64, worst: String, nrmse: f64, peak: f64, at: f64, bad: usize, bad_list: String, ripple: String },
    Skipped(String),
    Failed(String),
}

impl TranOutcome {
    fn label(&self) -> &'static str {
        match self {
            TranOutcome::Match { .. } => "一致",
            TranOutcome::Differ { .. } => "ズレ",
            TranOutcome::Skipped(_) => "対象外",
            TranOutcome::Failed(_) => "失敗",
        }
    }
}

/// (fastest feature, time worth showing) of a source, or why it cannot be compared.
fn timescale(w: &Waveform) -> Result<Option<(f64, f64)>, String> {
    Ok(match w {
        Waveform::Dc(_) => None,
        Waveform::Sin { freq, td, .. } if *freq > 0.0 => Some((1.0 / freq / 40.0, td + 5.0 / freq)),
        Waveform::Pulse { td, tr, tf, pw, per, .. } => {
            let period = if per.is_finite() { *per } else { tr + pw + tf };
            let shows = if per.is_finite() { 5.0 * period } else { 2.0 * period };
            Some((period / 100.0, td + shows))
        }
        Waveform::Pwl(p) if p.len() >= 2 => {
            let end = p.last().map_or(0.0, |x| x.0);
            Some((end / 200.0, end))
        }
        _ => return Err("ノイズや和の波形、周波数0の電源は比べられません".into()),
    })
}

/// Run length and step for the circuit: the longest "worth showing" time, the finest step.
/// `Ok(None)`: no source changes with time (the circuit may still move by itself).
pub fn pick_settings(s: &Schematic) -> Result<Option<(f64, f64)>, String> {
    let (mut step, mut stop): (f64, f64) = (f64::INFINITY, 0.0);
    for c in s.components.iter().filter(|c| c.kind == Kind::VSource) {
        let src = parse_source_full(&c.value).map_err(|m| format!("{} の値を読めません ({}): {m}", c.refdes, c.value))?;
        if let Some((fast, shows)) = timescale(&src.wave).map_err(|m| format!("{}: {m}", c.refdes))? {
            step = step.min(fast);
            stop = stop.max(shows);
        }
    }
    if !step.is_finite() || stop <= 0.0 {
        return Ok(None);
    }
    Ok(Some((stop, step.max(stop / MAX_STEPS))))
}

/// Run length and step for circuits that move by themselves (their own oscillator or start-up);
/// anything else gets a short run that checks the circuit stays at its operating point.
fn own_dynamics(tag: &str) -> (f64, f64) {
    match tag {
        // 150 kHz PWM: the start-up of the output filter
        "buck_converter" => (1e-3, 100e-9),
        // the two output-capacitor variants are about the steady-state ripple: run until the output has settled (about 4 ms)
        "buck_cout_ceramic" | "buck_cout_electrolytic" => (5e-3, 100e-9),
        // astable timers: a few periods of about a second
        "ne555_blinker" => (3.0, 5e-4),
        "led_blinker" => (5.0, 2e-3),
        _ => (1e-3, 10e-6),
    }
}

/// `wrdata` with `wr_vecnames`: a header line of names, then one row per time point whose first
/// column is the time. Returns the time axis and `(name, values)` per vector (lower-cased).
/// Named columns of a `wrdata` file.
pub type Columns = Vec<(String, Vec<f64>)>;

pub fn parse_wrdata(text: &str) -> Option<(Vec<f64>, Columns)> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let names: Vec<String> = lines.next()?.split_whitespace().map(|n| n.to_ascii_lowercase()).collect();
    if names.len() < 2 {
        return None;
    }
    let mut cols: Vec<Vec<f64>> = vec![vec![]; names.len()];
    for l in lines {
        let vals: Vec<f64> = l.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        if vals.len() != names.len() {
            return None;
        }
        for (c, v) in cols.iter_mut().zip(vals) {
            c.push(v);
        }
    }
    let time = cols.remove(0);
    Some((time, names.into_iter().skip(1).zip(cols).collect()))
}

/// Times at which `v` crosses `level` (either direction), by linear interpolation.
pub fn crossings(t: &[f64], v: &[f64], level: f64) -> Vec<f64> {
    let mut out = vec![];
    for k in 1..t.len().min(v.len()) {
        let (a, b) = (v[k - 1] - level, v[k] - level);
        if (a < 0.0) != (b < 0.0) && b != a {
            out.push(t[k - 1] + (t[k] - t[k - 1]) * (-a) / (b - a));
        }
    }
    out
}

/// For a signal that is mostly edges (a PWM node, an oscillator output): how far the edges of
/// `ours` are from those of `theirs`, as a fraction of the period. `Err` (with the reason) when the two do not have
/// the same number of edges or there are too few to speak of a period.
pub fn edge_drift(ours: (&[f64], &[f64]), theirs: (&[f64], &[f64])) -> Result<f64, String> {
    let (lo, hi) = theirs.1.iter().fold((f64::MAX, f64::MIN), |(l, h), &x| (l.min(x), h.max(x)));
    if hi - lo < 0.5 {
        return Err("振れ幅が小さい".into());
    }
    let level = (lo + hi) / 2.0;
    let (a, b) = (crossings(ours.0, ours.1, level), crossings(theirs.0, theirs.1, level));
    if b.len() < 4 || a.len().abs_diff(b.len()) > 1 {
        return Err(format!("エッジの数が違う (自前{} ngspice{})", a.len(), b.len()));
    }
    let n = a.len().min(b.len());
    let mut gaps: Vec<f64> = (0..n - 2).map(|i| b[i + 2] - b[i]).collect();
    gaps.sort_by(|x, y| x.total_cmp(y));
    let period = gaps[gaps.len() / 2];
    // each edge of theirs against the nearest edge of ours (an extra or missing edge at the ends must not shift the pairing)
    let drift = b.iter().map(|&x| a.iter().map(|&y| (x - y).abs()).fold(f64::MAX, f64::min)).fold(0.0, f64::max);
    Ok(drift / period)
}

/// Linear interpolation of `(t, v)` at `x` (clamped at the ends).
pub fn interpolate(t: &[f64], v: &[f64], x: f64) -> f64 {
    match t.partition_point(|&a| a <= x) {
        0 => v[0],
        n if n >= t.len() => v[v.len() - 1],
        n => {
            let (t0, t1) = (t[n - 1], t[n]);
            if t1 > t0 { v[n - 1] + (v[n] - v[n - 1]) * (x - t0) / (t1 - t0) } else { v[n] }
        }
    }
}

/// (normalised RMS error, peak error) of `ours` against `theirs` on the time axis `t`.
/// The RMS is weighted by time: the step control puts many points into a fast event, and counting
/// points would let that one event dominate.
pub fn waveform_error(t: &[f64], ours: &[f64], theirs: &[f64]) -> (f64, f64) {
    let (lo, hi) = theirs.iter().fold((f64::MAX, f64::MIN), |(l, h), &x| (l.min(x), h.max(x)));
    let span = (hi - lo).max(MIN_SPAN);
    let (mut sum, mut total) = (0.0, 0.0);
    for k in 1..t.len().min(ours.len()) {
        let dt = t[k] - t[k - 1];
        let (e0, e1) = (ours[k - 1] - theirs[k - 1], ours[k] - theirs[k]);
        sum += dt * (e0 * e0 + e1 * e1) / 2.0;
        total += dt;
    }
    let rms = if total > 0.0 { (sum / total).sqrt() } else { 0.0 };
    let peak = ours.iter().zip(theirs).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
    (rms / span, peak)
}

/// Run settings given on the command line; `ours_step` / `ng_step` set one engine's step alone
/// (to see which engine is the coarse-step one by comparing it with a fine run of the other).
#[derive(Clone, Copy, Default)]
pub struct Given {
    pub tstop: Option<f64>,
    pub tstep: Option<f64>,
    pub ours_step: Option<f64>,
    pub ng_step: Option<f64>,
    /// Our step-error tolerance (`SimOptions::lte_reltol`); a huge value switches the control off.
    pub lte: Option<f64>,
}

pub fn check_tran(s: &Schematic, exe: &Path, work: &Path, tag: &str, given: Given) -> TranOutcome {
    check_tran_keeping(s, exe, work, tag, given, &mut None)
}

/// Like `check_tran`; when the circuit matches, `keep` gets ngspice's waveform boiled down to per-window
/// statistics (what CI compares against, see `docs/xcheck/ngspice_tran.json`).
pub fn check_tran_keeping(s: &Schematic, exe: &Path, work: &Path, tag: &str, given: Given, keep: &mut Option<TranGolden>) -> TranOutcome {
    if s.components.iter().any(|c| c.kind == Kind::Custom && c.custom.as_ref().is_none_or(|x| x.sim.is_none())) {
        return TranOutcome::Skipped("シミュレーションモデルのない部品があります".into());
    }
    if s.components.iter().any(|c| c.value.to_ascii_uppercase().contains("IC=")) {
        return TranOutcome::Skipped("初期電圧 (IC=) のある部品は比べられません".into());
    }
    let (tstop, tstep) = match (given.tstop.zip(given.tstep), pick_settings(s)) {
        (Some(g), _) => g,
        (None, Ok(Some((x, y)))) => (given.tstop.unwrap_or(x), given.tstep.unwrap_or(y)),
        (None, Ok(None)) => {
            let (x, y) = own_dynamics(tag);
            (given.tstop.unwrap_or(x), given.tstep.unwrap_or(y))
        }
        (None, Err(m)) => return TranOutcome::Skipped(m),
    };
    let (ours_step, ng_step) = (given.ours_step.unwrap_or(tstep), given.ng_step.unwrap_or(tstep));
    let circuit = match from_schematic(s) {
        Ok(c) => c,
        Err(e) => return TranOutcome::Failed(format!("自前エンジン: 回路を作れません ({})", e.first().map(|x| x.to_string()).unwrap_or_default())),
    };
    if let Some(l) = given.lte {
        let mut o = sim_core::options();
        o.lte_reltol = l;
        sim_core::set_options(o);
    }
    let ours = match transient(&circuit, &TranSettings { tstop, tstep: ours_step, uic: false }) {
        Ok(r) => r,
        Err(e) => return TranOutcome::Failed(format!("自前エンジン: {e}")),
    };

    // An oscillator with no operating point starts from all zeros in our engine (SPICE's UIC): do the
    // same in ngspice, and leave out the t = 0 sample, where even the supply reads 0 V.
    let zero_start = ours.warnings.iter().any(|w| w.contains("すべてゼロ"));
    // only nets of the schematic, under the names to_spice gave them
    let nets: Vec<(String, String)> = extract(s).iter().map(|n| (n.name.clone(), spice_node(&n.name))).filter(|(_, sp)| sp != "0").collect();
    let body = to_spice_for(s, tag, Dialect::Ngspice);
    let body = body.strip_suffix(".end\n").unwrap_or(&body);
    let out_file = work.join(format!("{tag}_tran.txt"));
    let _ = std::fs::remove_file(&out_file);
    let vecs: Vec<String> = nets.iter().map(|(_, sp)| format!("v({sp})")).collect();
    let script = format!(
        "{body}.options reltol=1e-6 vntol=1e-8 abstol=1e-12\n.control\nset noaskquit\nset wr_singlescale\nset wr_vecnames\ntran {ng_step:e} {tstop:e} 0 {ng_step:e}{}\nwrdata {} {}\n.endc\n.end\n",
        if zero_start { " uic" } else { "" },
        out_file.to_string_lossy().replace('\\', "/"),
        vecs.join(" ")
    );
    if let Err(m) = run_ngspice_script(exe, work, tag, &script) {
        return TranOutcome::Failed(m);
    }
    let Some((t, cols)) = std::fs::read_to_string(&out_file).ok().and_then(|x| parse_wrdata(&x)) else {
        return TranOutcome::Failed("ngspice: 波形ファイルを読めません".into());
    };
    if t.len() < 2 {
        return TranOutcome::Failed("ngspice: 波形が出ませんでした".into());
    }

    let (mut n, mut bad) = (0, 0);
    let mut worst: Option<(f64, String, f64, f64)> = None;
    let mut ripple: Vec<String> = vec![];
    let mut bad_names: Vec<String> = vec![];
    for (name, sp) in &nets {
        let (Some(a), Some((_, b))) = (ours.signal(&format!("V({name})")), cols.iter().find(|(c, _)| *c == format!("v({})", sp.to_ascii_lowercase()))) else { continue };
        // compare on our time axis, inside the span both reached
        let end = ours.time.last().copied().unwrap_or(0.0).min(*t.last().unwrap());
        let skip = usize::from(zero_start);
        let (mine, theirs): (Vec<f64>, Vec<f64>) =
            ours.time.iter().zip(&a.values).skip(skip).filter(|(x, _)| **x <= end).map(|(x, v)| (*v, interpolate(&t, b, *x))).unzip();
        let (nrmse, peak) = waveform_error(&ours.time[skip..skip + mine.len()], &mine, &theirs);
        let at = mine.iter().zip(&theirs).enumerate().fold((0, 0.0), |m, (k, (a, b))| if (a - b).abs() > m.1 { (k, (a - b).abs()) } else { m }).0;
        let at = ours.time[skip + at];
        // ripple: the peak-to-peak of the last fifth of the run, which a timing shift of an edge does not disturb
        let from = mine.len() - mine.len() / 5;
        let pp = |v: &[f64]| v[from..].iter().cloned().fold(f64::MIN, f64::max) - v[from..].iter().cloned().fold(f64::MAX, f64::min);
        let (pm, pt) = (pp(&mine), pp(&theirs));
        if pt > 1e-3 && (pm / pt - 1.0).abs() > 0.1 {
            ripple.push(format!("{name} 末尾のp-p 自前{pm:.3e} ngspice{pt:.3e}"));
        }
        n += 1;
        // a waveform that is mostly edges differs wherever an edge sits a little off: judge it by the edge times
        let timing = if nrmse > NRMSE_TOL {
            edge_drift((&ours.time[skip..skip + mine.len()], &mine), (&ours.time[skip..skip + mine.len()], &theirs))
        } else {
            Err(String::new())
        };
        match timing {
            Ok(d) if d <= EDGE_TOL => ripple.push(format!("{name} は形の差{:.1}%だがエッジ時刻のずれは周期の{:.1}%", nrmse * 100.0, d * 100.0)),
            _ if nrmse > NRMSE_TOL => {
                bad += 1;
                let edges = match &timing {
                    Ok(d) => format!("エッジのずれ 周期の{:.1}%", d * 100.0),
                    Err(why) => format!("エッジは比べられない: {why}"),
                };
                bad_names.push(format!("{name} {:.1}% ({edges})", nrmse * 100.0));
            }
            _ => {}
        }
        if worst.as_ref().is_none_or(|w| nrmse > w.0) && !matches!(timing, Ok(d) if d <= EDGE_TOL) {
            worst = Some((nrmse, name.clone(), peak, at));
        }
    }
    let ripple = ripple.join(" / ");
    let bad_list = bad_names.join(", ");
    let outcome = match worst {
        None => TranOutcome::Skipped("名前が一致するノードがありません".into()),
        Some((nrmse, name, peak, at)) if bad > 0 => TranOutcome::Differ { nodes: n, tstop, worst: name, nrmse, peak, at, bad, bad_list, ripple },
        Some((nrmse, name, ..)) => TranOutcome::Match { nodes: n, tstop, worst: name, nrmse, ripple },
    };
    if matches!(outcome, TranOutcome::Match { .. }) {
        let nodes = nets
            .iter()
            .filter_map(|(_, sp)| {
                let key = sp.to_ascii_lowercase();
                cols.iter().find(|(c, _)| *c == format!("v({key})")).map(|(_, b)| (key, node_stats(&t, b, tstop, usize::from(zero_start))))
            })
            .collect();
        *keep = Some(TranGolden { tstop, tstep, zero_start, nodes });
    }
    outcome
}

// ------------------------------------------------------------------ stored ngspice answers (for CI)

/// A run is cut into this many equal windows; each keeps the mean, minimum and maximum of a node.
const WINDOWS: usize = 10;
/// A window's mean may differ by this fraction of the node's swing (floor `MIN_SPAN`), its minimum / maximum by `EXTREME_TOL`.
#[cfg(test)]
const MEAN_TOL: f64 = 0.01;
#[cfg(test)]
const EXTREME_TOL: f64 = 0.03;
/// The ripple (see `local_ripple`) may differ by this fraction. It is a median over many short slices, so one odd
/// cycle does not move it; the slow settling of a converter's output (which differs by a few mV between CPUs) does not enter.
#[cfg(test)]
const RIPPLE_TOL: f64 = 0.1;
/// The run is cut into this many slices for `local_ripple`; the last `RIPPLE_SLICES / 5` of them are used.
const RIPPLE_SLICES: usize = 100;

/// What is kept of ngspice's transient run of one circuit.
pub struct TranGolden {
    pub tstop: f64,
    pub tstep: f64,
    /// Both engines started from all zeros (and the first sample was left out).
    pub zero_start: bool,
    /// (lower-case SPICE node name, what is kept of its waveform)
    pub nodes: Vec<(String, NodeStats)>,
}

/// [mean, min, max] per window, and the local ripple.
pub struct NodeStats {
    pub windows: Vec<[f64; 3]>,
    pub ripple: f64,
}

pub fn node_stats(t: &[f64], v: &[f64], tstop: f64, skip: usize) -> NodeStats {
    NodeStats { windows: window_stats(t, v, tstop, skip), ripple: local_ripple(t, v, tstop, skip) }
}

/// Time-weighted mean, minimum and maximum of a piecewise-linear signal between `lo` and `hi`.
fn span_stats(t: &[f64], v: &[f64], lo: f64, hi: f64) -> [f64; 3] {
    let (mut area, mut mn, mut mx) = (0.0, f64::MAX, f64::MIN);
    let mut prev = (lo, interpolate(t, v, lo));
    mn = mn.min(prev.1);
    mx = mx.max(prev.1);
    for (&x, &y) in t.iter().zip(v).filter(|(x, _)| **x > lo && **x < hi) {
        area += (x - prev.0) * (y + prev.1) / 2.0;
        mn = mn.min(y);
        mx = mx.max(y);
        prev = (x, y);
    }
    let end = interpolate(t, v, hi);
    area += (hi - prev.0) * (end + prev.1) / 2.0;
    mn = mn.min(end);
    mx = mx.max(end);
    [area / (hi - lo), mn, mx]
}

/// The ripple of a node: the median, over the last fifth of the run cut into `RIPPLE_SLICES` slices, of each
/// slice's peak-to-peak. A slice is a few switching periods long, so this is the cycle-by-cycle ripple.
pub fn local_ripple(t: &[f64], v: &[f64], tstop: f64, skip: usize) -> f64 {
    let n = t.len().min(v.len());
    let (t, v) = (&t[skip.min(n)..n], &v[skip.min(n)..n]);
    if t.is_empty() {
        return 0.0;
    }
    let mut pp: Vec<f64> = (RIPPLE_SLICES - RIPPLE_SLICES / 5..RIPPLE_SLICES)
        .map(|k| {
            let w = span_stats(t, v, tstop * k as f64 / RIPPLE_SLICES as f64, tstop * (k + 1) as f64 / RIPPLE_SLICES as f64);
            w[2] - w[1]
        })
        .collect();
    pp.sort_by(|a, b| a.total_cmp(b));
    pp[pp.len() / 2]
}

/// Mean (time-weighted), minimum and maximum of a piecewise-linear signal in each of `WINDOWS` equal
/// windows of `0..tstop`. `skip` leaves out the first samples (the t = 0 sample of a zero start).
pub fn window_stats(t: &[f64], v: &[f64], tstop: f64, skip: usize) -> Vec<[f64; 3]> {
    let n = t.len().min(v.len());
    let (t, v) = (&t[skip.min(n)..n], &v[skip.min(n)..n]);
    if t.is_empty() {
        return vec![];
    }
    (0..WINDOWS).map(|k| span_stats(t, v, tstop * k as f64 / WINDOWS as f64, tstop * (k + 1) as f64 / WINDOWS as f64)).collect()
}

/// `Err` (what differs) when our window statistics are not those of the stored ngspice run.
#[cfg(test)]
pub fn compare_stats(name: &str, ours: &NodeStats, theirs: &NodeStats) -> Result<(), String> {
    let (ours_w, theirs_w) = (&ours.windows, &theirs.windows);
    if ours_w.len() != theirs_w.len() || theirs_w.is_empty() {
        return Err(format!("{name}: 窓の数が違う"));
    }
    let lo = theirs_w.iter().map(|w| w[1]).fold(f64::MAX, f64::min);
    let hi = theirs_w.iter().map(|w| w[2]).fold(f64::MIN, f64::max);
    let span = (hi - lo).max(MIN_SPAN);
    for (k, (a, b)) in ours_w.iter().zip(theirs_w).enumerate() {
        let bad = |what: &str, x: f64, y: f64, tol: f64| {
            ((x - y).abs() > tol * span).then(|| format!("{name} 窓{k} の{what}: 自前 {x:.4e} / ngspice {y:.4e} (振れ幅 {span:.3e} の {:.1}%)", (x - y).abs() / span * 100.0))
        };
        // window 0 starts at t = 0, where the two engines may begin from slightly different initial states
        // (an astable timer): its extremes are left out, its mean is not
        let found = bad("平均", a[0], b[0], MEAN_TOL).or_else(|| if k == 0 { None } else { bad("最小", a[1], b[1], EXTREME_TOL) }).or_else(|| if k == 0 { None } else { bad("最大", a[2], b[2], EXTREME_TOL) });
        if let Some(m) = found {
            return Err(m);
        }
    }
    let (rm, rt) = (ours.ripple, theirs.ripple);
    // only for a node that settles around a level, where "ripple" means something: the last two windows are not
    // much wider than the ripple (a pulse train with slow decays between its edges is not compared this way)
    let last2 = theirs_w[theirs_w.len() - 2..].iter().fold((f64::MAX, f64::MIN), |(l, h), w| (l.min(w[1]), h.max(w[2])));
    let steady = last2.1 - last2.0 <= 10.0 * rt;
    if steady && rt > 1e-3 && (rm / rt - 1.0).abs() > RIPPLE_TOL {
        return Err(format!("{name} のリップル (短い区間ごとのp-pの中央値): 自前 {rm:.4e} / ngspice {rt:.4e}"));
    }
    Ok(())
}

/// Run our engine with the stored settings and compare with the stored ngspice statistics (no ngspice needed).
#[cfg(test)]
pub fn check_against_stored(s: &Schematic, g: &TranGolden) -> Result<(), String> {
    let circuit = from_schematic(s).map_err(|e| format!("回路を作れません ({})", e.first().map(|x| x.to_string()).unwrap_or_default()))?;
    let ours = transient(&circuit, &TranSettings { tstop: g.tstop, tstep: g.tstep, uic: false }).map_err(|e| format!("自前エンジン: {e}"))?;
    let zero_start = ours.warnings.iter().any(|w| w.contains("すべてゼロ"));
    if zero_start != g.zero_start {
        return Err(format!("すべてゼロからの開始が変わった (保存={} 今={zero_start})", g.zero_start));
    }
    let nets: Vec<(String, String)> = extract(s).iter().map(|n| (n.name.clone(), spice_node(&n.name).to_ascii_lowercase())).collect();
    let mut errors = vec![];
    for (key, theirs) in &g.nodes {
        let Some(a) = nets.iter().find(|(_, sp)| sp == key).and_then(|(name, _)| ours.signal(&format!("V({name})"))) else {
            errors.push(format!("{key}: 自前の波形がありません"));
            continue;
        };
        let mine = node_stats(&ours.time, &a.values, g.tstop, usize::from(zero_start));
        if let Err(m) = compare_stats(key, &mine, theirs) {
            errors.push(m);
        }
    }
    if errors.is_empty() { Ok(()) } else { Err(errors.join(" / ")) }
}

/// Where the stored answers are kept.
#[cfg(test)]
pub const GOLDEN_TRAN: &str = "docs/xcheck/ngspice_tran.json";

fn round6(x: f64) -> f64 {
    format!("{x:.6e}").parse().unwrap_or(x)
}

/// Run ngspice over a folder of examples and store the statistics of every circuit that matches.
pub fn write_golden(dir: &Path, exe: &Path, out: &Path) -> String {
    let work = std::env::temp_dir().join("circuitcad_xcheck_tran");
    let _ = std::fs::create_dir_all(&work);
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "schjson")).collect();
    files.sort();
    let (mut lines, mut left_out) = (vec![], vec![]);
    for f in &files {
        let tag = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let Ok(s) = std::fs::read_to_string(f).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<Schematic>(&t).map_err(|e| e.to_string())) else { continue };
        let mut keep = None;
        let outcome = check_tran_keeping(&s, exe, &work, &tag, Given::default(), &mut keep);
        match keep {
            Some(g) if matches!(outcome, TranOutcome::Match { .. }) => {
                let nodes: serde_json::Map<String, serde_json::Value> =
                    g.nodes
                        .iter()
                        .map(|(k, n)| (k.clone(), serde_json::json!({ "windows": n.windows.iter().map(|x| x.map(round6)).collect::<Vec<_>>(), "ripple": round6(n.ripple) })))
                        .collect();
                let one = serde_json::json!({ "tstop": g.tstop, "tstep": g.tstep, "zero_start": g.zero_start, "nodes": nodes });
                lines.push(format!("    {}: {}", serde_json::json!(tag), one));
            }
            _ => left_out.push(format!("{tag} ({})", outcome.label())),
        }
    }
    let text = format!(
        "{{\n  \"about\": \"ngspice transient runs of the shipped examples, boiled down to mean/min/max per window ({WINDOWS} windows of 0..tstop); written by `circuitCad --xcheck docs/examples --tran --golden docs/xcheck/ngspice_tran.json`\",\n  \"circuits\": {{\n{}\n  }}\n}}\n",
        lines.join(",\n")
    );
    let written = std::fs::create_dir_all(out.parent().unwrap_or(Path::new("."))).and_then(|_| std::fs::write(out, text));
    format!("{} 回路を {} に保存 ({:?})\n保存しなかった: {}\n", lines.len(), out.display(), written.map_err(|e| e.to_string()), left_out.join(", "))
}

/// Read the stored file: circuit name -> run.
#[cfg(test)]
pub fn read_golden(text: &str) -> Vec<(String, TranGolden)> {
    let json: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
    let num = |v: &serde_json::Value| v.as_f64().unwrap_or(f64::NAN);
    json["circuits"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, c)| {
            let nodes = c["nodes"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(k, n)| (k.clone(), NodeStats { windows: n["windows"].as_array().into_iter().flatten().map(|x| [num(&x[0]), num(&x[1]), num(&x[2])]).collect(), ripple: num(&n["ripple"]) }))
                .collect();
            (name.clone(), TranGolden { tstop: num(&c["tstop"]), tstep: num(&c["tstep"]), zero_start: c["zero_start"].as_bool().unwrap_or(false), nodes })
        })
        .collect()
}

fn note(ripple: &str) -> String {
    if ripple.is_empty() { String::new() } else { format!("  [リップルの差: {ripple}]") }
}

fn describe(o: &TranOutcome) -> String {
    match o {
        TranOutcome::Match { nodes, tstop, worst, nrmse, ripple } => {
            format!("{nodes} ノード ({tstop:.3e} 秒)。最大誤差 {worst} {:.3}%{}", nrmse * 100.0, note(ripple))
        }
        TranOutcome::Differ { nodes, tstop, worst, nrmse, peak, at, bad, bad_list, ripple } => {
            format!("{bad}/{nodes} ノードがズレ [{bad_list}] ({tstop:.3e} 秒)。最大: {worst} 誤差 {:.2}%、ピーク差 {peak:.3e} V (t={at:.4e} 秒){}", nrmse * 100.0, note(ripple))
        }
        TranOutcome::Skipped(m) | TranOutcome::Failed(m) => m.clone(),
    }
}

pub fn run_path(path: &Path, exe: &Path, given: Given) -> String {
    let files: Vec<PathBuf> = if path.is_dir() {
        let mut v: Vec<PathBuf> =
            std::fs::read_dir(path).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "schjson")).collect();
        v.sort();
        v
    } else {
        vec![path.to_path_buf()]
    };
    let work = std::env::temp_dir().join("circuitcad_xcheck_tran");
    let _ = std::fs::create_dir_all(&work);
    let mut report = format!("過渡解析の検算 (自前エンジン vs ngspice)  合格: 規格化RMS誤差 {}% 未満 (幅は{MIN_SPAN}V以上で割る)\n\n", NRMSE_TOL * 100.0);
    let mut counts = [0usize; 4];
    for f in &files {
        let tag = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let outcome = match std::fs::read_to_string(f).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<Schematic>(&t).map_err(|e| e.to_string())) {
            Ok(s) => check_tran(&s, exe, &work, &tag, given),
            Err(e) => TranOutcome::Failed(format!("読み込めません: {e}")),
        };
        counts[match outcome {
            TranOutcome::Match { .. } => 0,
            TranOutcome::Differ { .. } => 1,
            TranOutcome::Skipped(_) => 2,
            TranOutcome::Failed(_) => 3,
        }] += 1;
        report.push_str(&format!("[{}] {tag}: {}\n", outcome.label(), describe(&outcome)));
    }
    report.push_str(&format!("\n一致 {} / ズレ {} / 対象外 {} / 失敗 {}\n", counts[0], counts[1], counts[2], counts[3]));
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The transient run of every stored example must still look like ngspice's (window means, extremes and
    /// ripple), without running ngspice. After a deliberate model change regenerate the file with
    /// `circuitCad --xcheck docs/examples --tran --golden docs/xcheck/ngspice_tran.json` and read the diff.
    #[test]
    fn transient_runs_still_match_the_stored_ngspice_statistics() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let stored = read_golden(&std::fs::read_to_string(root.join(GOLDEN_TRAN)).expect("stored ngspice runs missing: see the doc comment of this test"));
        assert!(stored.len() >= 20, "only {} circuits are stored", stored.len());
        let mut bad = vec![];
        for (name, g) in &stored {
            let s: Schematic = serde_json::from_str(&std::fs::read_to_string(root.join(format!("docs/examples/{name}.schjson"))).unwrap()).unwrap();
            if let Err(m) = check_against_stored(&s, g) {
                bad.push(format!("{name}: {m}"));
            }
        }
        assert!(bad.is_empty(), "differs from ngspice:\n{}", bad.join("\n"));
    }

    #[test]
    fn window_stats_of_a_ramp_and_a_comparison() {
        // a ramp 0..10 V over 10 s: window k has mean k + 0.5, min k, max k + 1
        let t: Vec<f64> = (0..=100).map(|i| i as f64 * 0.1).collect();
        let v: Vec<f64> = t.clone();
        let w = node_stats(&t, &v, 10.0, 0);
        assert_eq!(w.windows.len(), WINDOWS);
        let k3 = w.windows[3];
        assert!((k3[0] - 3.5).abs() < 1e-9 && (k3[1] - 3.0).abs() < 1e-9 && (k3[2] - 4.0).abs() < 1e-9, "{k3:?}");
        assert!(compare_stats("x", &w, &w).is_ok());
        let mut off = NodeStats { windows: w.windows.clone(), ripple: w.ripple };
        off.windows[5][0] += 1.0;
        assert!(compare_stats("x", &off, &w).unwrap_err().contains("窓5"));
        // ripple: a 5 V swing early on, then a 0.08 V ripple against one of 0.12 V: only the ripple check catches that
        let mut windows = vec![[2.5, 0.0, 5.0]; WINDOWS];
        windows[WINDOWS - 1] = [5.0, 4.96, 5.04];
        windows[WINDOWS - 2] = [5.0, 4.96, 5.04];
        let theirs = NodeStats { windows: windows.clone(), ripple: 0.08 };
        let rippled = NodeStats { windows, ripple: 0.12 };
        assert!(compare_stats("y", &rippled, &theirs).unwrap_err().contains("リップル"));
        assert!(compare_stats("y", &theirs, &theirs).is_ok());
    }

    /// The ripple is the median of the slices' peak-to-peak: a slow drift and one odd cycle do not enter.
    #[test]
    fn local_ripple_ignores_drift_and_one_odd_cycle() {
        // 1 s run, 1000 points; a 0.1 V triangle ripple of period 0.01 s on a drift of 1 V/s, with one cycle 0.3 V higher
        let t: Vec<f64> = (0..=10000).map(|i| i as f64 * 1e-4).collect();
        let tri = |x: f64| ((x / 0.01).fract() - 0.5).abs() * 0.2;
        let v: Vec<f64> = t.iter().map(|&x| 4.0 + x + tri(x) + if (0.93..0.94).contains(&x) { 0.3 } else { 0.0 }).collect();
        let r = local_ripple(&t, &v, 1.0, 0);
        // a slice is 0.01 s long: ripple 0.1 plus a part of the drift over the slice (up to 0.01 V)
        assert!((r - 0.105).abs() < 0.006, "{r}");
    }

    #[test]
    fn reads_wrdata_and_interpolates() {
        let (t, cols) = parse_wrdata("time v(a) v(b)\n0 1 10\n1 3 20\n2 5 30\n").unwrap();
        assert_eq!(t, vec![0.0, 1.0, 2.0]);
        assert_eq!(cols[1], ("v(b)".to_string(), vec![10.0, 20.0, 30.0]));
        assert_eq!(interpolate(&t, &cols[0].1, 0.5), 2.0);
        assert_eq!(interpolate(&t, &cols[0].1, 9.0), 5.0);
        assert!(parse_wrdata("time v(a)\n0 1 2\n").is_none());
    }

    #[test]
    fn waveform_error_is_relative_to_the_swing() {
        let theirs: Vec<f64> = (0..100).map(|k| (k as f64 * 0.2).sin() * 5.0).collect();
        let near: Vec<f64> = theirs.iter().map(|x| x + 0.01).collect();
        let far: Vec<f64> = theirs.iter().map(|x| x + 0.5).collect();
        let t: Vec<f64> = (0..100).map(|k| k as f64).collect();
        assert!(waveform_error(&t, &near, &theirs).0 < NRMSE_TOL);
        assert!(waveform_error(&t, &far, &theirs).0 > NRMSE_TOL);
        // a flat node is judged against MIN_SPAN, not against its tiny swing
        let t10: Vec<f64> = (0..10).map(|k| k as f64).collect();
        assert!(waveform_error(&t10, &[0.0005; 10], &[0.0; 10]).0 < NRMSE_TOL);
        // a burst of points inside one event does not count more than the time it lasts
        let tt = [0.0, 10.0, 10.001, 10.002, 10.003, 20.0];
        let r = [0.0, 20.0, 40.0, 60.0, 80.0, 100.0];
        let o: Vec<f64> = r.iter().zip([0.0, 0.0, 5.0, 5.0, 0.0, 0.0]).map(|(a, d)| a + d).collect();
        assert!(waveform_error(&tt, &o, &r).0 < 0.001);
    }

    #[test]
    fn edge_times_tell_a_shifted_square_wave_from_a_wrong_one() {
        // a 1 kHz square wave, sampled every 10 us
        let t: Vec<f64> = (0..2000).map(|k| k as f64 * 1e-5).collect();
        let square = |shift: f64| -> Vec<f64> { t.iter().map(|x| if ((x - shift) / 5e-4).floor() as i64 % 2 == 0 { 5.0 } else { 0.0 }).collect() };
        let theirs = square(0.0);
        // 10 us late = 1 % of the period: edges agree although the waveform differs at every edge
        let d = edge_drift((&t, &square(1e-5)), (&t, &theirs)).unwrap();
        assert!((d - 0.01).abs() < 0.002, "{d}");
        // a quarter period late is far outside
        assert!(edge_drift((&t, &square(2.5e-4)), (&t, &theirs)).unwrap() > 0.2);
        // a missing half of the edges, or a flat signal, cannot be judged by timing
        assert!(edge_drift((&t, &vec![5.0; t.len()]), (&t, &theirs)).is_err());
        assert!(edge_drift((&t, &theirs), (&t, &vec![0.1; t.len()])).is_err());
        assert_eq!(crossings(&[0.0, 1.0, 2.0], &[0.0, 2.0, 0.0], 1.0), vec![0.5, 1.5]);
    }

    #[test]
    fn timescales_come_from_the_sources() {
        let sin = parse_source_full("SIN(0 1 1k)").unwrap().wave;
        let (fast, shows) = timescale(&sin).unwrap().unwrap();
        assert!((fast - 25e-6).abs() < 1e-12 && (shows - 5e-3).abs() < 1e-12);
        assert!(timescale(&Waveform::Dc(5.0)).unwrap().is_none());
        assert!(timescale(&parse_source_full("5 + NOISE(0.1 1Meg 1)").unwrap().wave).is_err());
    }
}
