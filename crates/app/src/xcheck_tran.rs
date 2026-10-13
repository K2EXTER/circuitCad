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
const MIN_SPAN: f64 = 0.1;
/// Most output points per run (keeps both simulators quick).
const MAX_STEPS: f64 = 20_000.0;

#[derive(Debug, PartialEq)]
pub enum TranOutcome {
    Match { nodes: usize, tstop: f64, worst: String, nrmse: f64 },
    Differ { nodes: usize, tstop: f64, worst: String, nrmse: f64, peak: f64, bad: usize },
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
pub fn pick_settings(s: &Schematic) -> Result<(f64, f64), String> {
    let (mut step, mut stop): (f64, f64) = (f64::INFINITY, 0.0);
    for c in s.components.iter().filter(|c| c.kind == Kind::VSource) {
        let src = parse_source_full(&c.value).map_err(|m| format!("{} の値を読めません ({}): {m}", c.refdes, c.value))?;
        if let Some((fast, shows)) = timescale(&src.wave).map_err(|m| format!("{}: {m}", c.refdes))? {
            step = step.min(fast);
            stop = stop.max(shows);
        }
    }
    if !step.is_finite() || stop <= 0.0 {
        return Err("時間変化する電源がありません (--tstop と --tstep で指定できます)".into());
    }
    Ok((stop, step.max(stop / MAX_STEPS)))
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

/// (normalised RMS error, peak error) of `ours` against `theirs`, both on the time axis `t`.
pub fn waveform_error(ours: &[f64], theirs: &[f64]) -> (f64, f64) {
    let (lo, hi) = theirs.iter().fold((f64::MAX, f64::MIN), |(l, h), &x| (l.min(x), h.max(x)));
    let span = (hi - lo).max(MIN_SPAN);
    let n = ours.len().max(1) as f64;
    let rms = (ours.iter().zip(theirs).map(|(a, b)| (a - b) * (a - b)).sum::<f64>() / n).sqrt();
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
}

pub fn check_tran(s: &Schematic, exe: &Path, work: &Path, tag: &str, given: Given) -> TranOutcome {
    if s.components.iter().any(|c| c.kind == Kind::Custom && c.custom.as_ref().is_none_or(|x| x.sim.is_none())) {
        return TranOutcome::Skipped("シミュレーションモデルのない部品があります".into());
    }
    if s.components.iter().any(|c| c.value.to_ascii_uppercase().contains("IC=")) {
        return TranOutcome::Skipped("初期電圧 (IC=) のある部品は比べられません".into());
    }
    let (tstop, tstep) = match (given.tstop.zip(given.tstep), pick_settings(s)) {
        (Some(g), _) => g,
        (None, Ok((x, y))) => (given.tstop.unwrap_or(x), given.tstep.unwrap_or(y)),
        (None, Err(m)) => return TranOutcome::Skipped(m),
    };
    let (ours_step, ng_step) = (given.ours_step.unwrap_or(tstep), given.ng_step.unwrap_or(tstep));
    let circuit = match from_schematic(s) {
        Ok(c) => c,
        Err(e) => return TranOutcome::Failed(format!("自前エンジン: 回路を作れません ({})", e.first().map(|x| x.to_string()).unwrap_or_default())),
    };
    let ours = match transient(&circuit, &TranSettings { tstop, tstep: ours_step, uic: false }) {
        Ok(r) => r,
        Err(e) => return TranOutcome::Failed(format!("自前エンジン: {e}")),
    };

    // only nets of the schematic, under the names to_spice gave them
    let nets: Vec<(String, String)> = extract(s).iter().map(|n| (n.name.clone(), spice_node(&n.name))).filter(|(_, sp)| sp != "0").collect();
    let body = to_spice_for(s, tag, Dialect::Ngspice);
    let body = body.strip_suffix(".end\n").unwrap_or(&body);
    let out_file = work.join(format!("{tag}_tran.txt"));
    let _ = std::fs::remove_file(&out_file);
    let vecs: Vec<String> = nets.iter().map(|(_, sp)| format!("v({sp})")).collect();
    let script = format!(
        "{body}.options reltol=1e-6 vntol=1e-8 abstol=1e-12\n.control\nset noaskquit\nset wr_singlescale\nset wr_vecnames\ntran {ng_step:e} {tstop:e} 0 {ng_step:e}\nwrdata {} {}\n.endc\n.end\n",
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
    let mut worst: Option<(f64, String, f64)> = None;
    for (name, sp) in &nets {
        let (Some(a), Some((_, b))) = (ours.signal(&format!("V({name})")), cols.iter().find(|(c, _)| *c == format!("v({})", sp.to_ascii_lowercase()))) else { continue };
        // compare on our time axis, inside the span both reached
        let end = ours.time.last().copied().unwrap_or(0.0).min(*t.last().unwrap());
        let (mine, theirs): (Vec<f64>, Vec<f64>) = ours.time.iter().zip(&a.values).filter(|(x, _)| **x <= end).map(|(x, v)| (*v, interpolate(&t, b, *x))).unzip();
        let (nrmse, peak) = waveform_error(&mine, &theirs);
        n += 1;
        if nrmse > NRMSE_TOL {
            bad += 1;
        }
        if worst.as_ref().is_none_or(|w| nrmse > w.0) {
            worst = Some((nrmse, name.clone(), peak));
        }
    }
    match worst {
        None => TranOutcome::Skipped("名前が一致するノードがありません".into()),
        Some((nrmse, name, peak)) if bad > 0 => TranOutcome::Differ { nodes: n, tstop, worst: name, nrmse, peak, bad },
        Some((nrmse, name, _)) => TranOutcome::Match { nodes: n, tstop, worst: name, nrmse },
    }
}

fn describe(o: &TranOutcome) -> String {
    match o {
        TranOutcome::Match { nodes, tstop, worst, nrmse } => format!("{nodes} ノード ({tstop:.3e} 秒)。最大誤差 {worst} {:.3}%", nrmse * 100.0),
        TranOutcome::Differ { nodes, tstop, worst, nrmse, peak, bad } => {
            format!("{bad}/{nodes} ノードがズレ ({tstop:.3e} 秒)。最大: {worst} 誤差 {:.2}%、ピーク差 {peak:.3e} V", nrmse * 100.0)
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
        assert!(waveform_error(&near, &theirs).0 < NRMSE_TOL);
        assert!(waveform_error(&far, &theirs).0 > NRMSE_TOL);
        // a flat node is judged against MIN_SPAN, not against its tiny swing
        assert!(waveform_error(&[0.0005; 10], &[0.0; 10]).0 < NRMSE_TOL);
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
