//! Development check: our AC small-signal analysis against ngspice's `ac` (`circuitCad --xcheck <dir> --ac`).
//! The stored answers (`docs/xcheck/ngspice_ac.json`) let CI repeat the comparison without ngspice.

use crate::xcheck::run_ngspice_script;
use sch_core::{extract, spice_node, to_spice_for, Dialect, Kind, Schematic};
use sim_core::{ac_analysis, from_schematic, AcSettings, AcSweep};
use std::path::{Path, PathBuf};

/// Every example is swept over the same band, so the stored answers only need the frequencies once.
const F_START: f64 = 10.0;
const F_STOP: f64 = 10e6;
const PER_DECADE: usize = 5;
/// Two complex answers agree when they differ by less than this fraction of the larger one ...
const REL_TOL: f64 = 1e-3;
/// ... plus this fraction of the node's largest magnitude over the sweep (a deep notch or a node that
/// is zero by symmetry must not be judged by its own, tiny, magnitude) ...
const PEAK_TOL: f64 = 1e-6;
/// ... plus this absolute floor (volts or amps).
const ABS_TOL: f64 = 1e-12;

/// One node's complex answer at every frequency.
pub type Phasors = Vec<(f64, f64)>;

pub enum AcOutcome {
    Match { nodes: usize, worst: String, err: f64 },
    Differ { nodes: usize, worst: String, err: f64, bad: usize },
    Skipped(String),
    Failed(String),
}

impl AcOutcome {
    pub fn label(&self) -> &'static str {
        match self {
            AcOutcome::Match { .. } => "一致",
            AcOutcome::Differ { .. } => "ズレ",
            AcOutcome::Skipped(_) => "対象外",
            AcOutcome::Failed(_) => "失敗",
        }
    }
}

/// ngspice's answer for one circuit: the frequencies and, per SPICE node name (lower case), the phasors.
pub struct AcGolden {
    pub freq: Vec<f64>,
    pub nodes: Vec<(String, Phasors)>,
}

fn settings() -> AcSettings {
    AcSettings { fstart: F_START, fstop: F_STOP, sweep: AcSweep::Decade(PER_DECADE) }
}

/// A circuit is worth sweeping when some source carries an `AC` magnitude.
fn has_ac_source(s: &Schematic) -> bool {
    s.components.iter().any(|c| c.value.split_whitespace().any(|w| w.eq_ignore_ascii_case("AC")))
}

/// `wrdata` of complex vectors: a frequency column, then a real and an imaginary column per vector.
/// The frequencies and, per signal, its phasors.
type Columns = (Vec<f64>, Vec<(String, Phasors)>);

pub fn parse_ac_wrdata(text: &str) -> Option<Columns> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header: Vec<String> = lines.next()?.split_whitespace().map(|n| n.to_ascii_lowercase()).collect();
    if header.len() < 3 || !(header.len() - 1).is_multiple_of(2) {
        return None;
    }
    let mut rows: Vec<Vec<f64>> = vec![];
    for l in lines {
        let vals: Vec<f64> = l.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        if vals.len() != header.len() {
            return None;
        }
        rows.push(vals);
    }
    let freq = rows.iter().map(|r| r[0]).collect();
    let nodes = (0..(header.len() - 1) / 2).map(|k| (header[1 + 2 * k].clone(), rows.iter().map(|r| (r[1 + 2 * k], r[2 + 2 * k])).collect())).collect();
    Some((freq, nodes))
}

fn mag(p: (f64, f64)) -> f64 {
    p.0.hypot(p.1)
}

/// Worst error of `ours` against `theirs`, as a fraction of the allowed difference (≤ 1 is a match).
pub fn phasor_error(ours: &[(f64, f64)], theirs: &[(f64, f64)]) -> f64 {
    let peak = theirs.iter().map(|p| mag(*p)).fold(0.0, f64::max);
    ours.iter()
        .zip(theirs)
        .map(|(a, b)| {
            let diff = (a.0 - b.0).hypot(a.1 - b.1);
            diff / (REL_TOL * mag(*a).max(mag(*b)) + PEAK_TOL * peak + ABS_TOL)
        })
        .fold(0.0, f64::max)
}

/// Our sweep of a circuit: SPICE node name (lower case) -> phasors.
/// The frequencies and, per signal, its name in ngspice, its own name and its phasors.
type OurColumns = (Vec<f64>, Vec<(String, String, Phasors)>);

fn our_side(s: &Schematic) -> Result<OurColumns, String> {
    let circuit = from_schematic(s).map_err(|e| format!("自前エンジン: 回路を作れません ({})", e.first().map(|x| x.to_string()).unwrap_or_default()))?;
    let r = ac_analysis(&circuit, &settings()).map_err(|e| format!("自前エンジン: {e}"))?;
    let nodes = extract(s)
        .iter()
        .map(|n| (n.name.clone(), spice_node(&n.name)))
        .filter(|(_, sp)| sp != "0")
        .filter_map(|(name, sp)| r.signal(&format!("V({name})")).map(|g| (name, sp.to_ascii_lowercase(), g.re.iter().cloned().zip(g.im.iter().cloned()).collect())))
        .collect();
    Ok((r.freq, nodes))
}

pub fn check_ac_keeping(s: &Schematic, exe: &Path, work: &Path, tag: &str, keep: &mut Option<AcGolden>) -> AcOutcome {
    if s.components.iter().any(|c| c.kind == Kind::Custom && c.custom.as_ref().is_none_or(|x| x.sim.is_none())) {
        return AcOutcome::Skipped("シミュレーションモデルのない部品があります".into());
    }
    if !has_ac_source(s) {
        return AcOutcome::Skipped("AC の電源がありません".into());
    }
    let (freq, ours) = match our_side(s) {
        Ok(x) => x,
        Err(m) => return AcOutcome::Failed(m),
    };
    let body = to_spice_for(s, tag, Dialect::Ngspice);
    let body = body.strip_suffix(".end\n").unwrap_or(&body);
    let out_file = work.join(format!("{tag}_ac.txt"));
    let _ = std::fs::remove_file(&out_file);
    let vecs: Vec<String> = ours.iter().map(|(_, sp, _)| format!("v({sp})")).collect();
    let script = format!(
        "{body}.options reltol=1e-6 vntol=1e-8 abstol=1e-12\n.control\nset noaskquit\nset wr_singlescale\nset wr_vecnames\nac dec {PER_DECADE} {F_START:e} {F_STOP:e}\nwrdata {} {}\n.endc\n.end\n",
        out_file.to_string_lossy().replace('\\', "/"),
        vecs.join(" ")
    );
    if let Err(m) = run_ngspice_script(exe, work, tag, &script) {
        return AcOutcome::Failed(m);
    }
    let Some((nf, theirs)) = std::fs::read_to_string(&out_file).ok().and_then(|x| parse_ac_wrdata(&x)) else {
        return AcOutcome::Failed("ngspice: 結果ファイルを読めません".into());
    };
    if nf.len() != freq.len() || nf.iter().zip(&freq).any(|(a, b)| (a / b - 1.0).abs() > 1e-6) {
        return AcOutcome::Failed(format!("ngspice: 周波数の点が合いません (自前{}点 ngspice{}点)", freq.len(), nf.len()));
    }
    let (mut n, mut bad, mut worst) = (0, 0, (0.0, String::new()));
    let mut kept = vec![];
    for (name, sp, mine) in &ours {
        let Some((_, t)) = theirs.iter().find(|(c, _)| *c == format!("v({sp})")) else { continue };
        let e = phasor_error(mine, t);
        n += 1;
        bad += usize::from(e > 1.0);
        if e >= worst.0 {
            worst = (e, name.clone());
        }
        kept.push((sp.clone(), t.clone()));
    }
    if n == 0 {
        return AcOutcome::Skipped("名前が一致するノードがありません".into());
    }
    if bad > 0 {
        return AcOutcome::Differ { nodes: n, worst: worst.1, err: worst.0, bad };
    }
    *keep = Some(AcGolden { freq, nodes: kept });
    AcOutcome::Match { nodes: n, worst: worst.1, err: worst.0 }
}

/// Run our sweep and compare with the stored ngspice phasors (no ngspice needed).
#[cfg(test)]
pub fn check_against_stored(s: &Schematic, g: &AcGolden) -> Result<(), String> {
    let (freq, ours) = our_side(s)?;
    if freq.len() != g.freq.len() {
        return Err(format!("周波数の点の数が変わった (保存={} 今={})", g.freq.len(), freq.len()));
    }
    let mut errors = vec![];
    for (key, theirs) in &g.nodes {
        match ours.iter().find(|(_, sp, _)| sp == key) {
            None => errors.push(format!("{key}: 自前の結果がありません")),
            Some((_, _, mine)) => {
                let e = phasor_error(mine, theirs);
                if e > 1.0 {
                    errors.push(format!("{key}: 許容の{e:.1}倍ずれた"));
                }
            }
        }
    }
    if errors.is_empty() { Ok(()) } else { Err(errors.join(" / ")) }
}

#[cfg(test)]
pub const GOLDEN_AC: &str = "docs/xcheck/ngspice_ac.json";

fn round8(x: f64) -> f64 {
    format!("{x:.8e}").parse().unwrap_or(x)
}

fn example_files(dir: &Path) -> Vec<PathBuf> {
    if !dir.is_dir() {
        return vec![dir.to_path_buf()];
    }
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "schjson")).collect();
    v.sort();
    v
}

fn load(f: &Path) -> Result<Schematic, String> {
    std::fs::read_to_string(f).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<Schematic>(&t).map_err(|e| e.to_string()))
}

/// Check a file or a folder; returns the report. With `golden` set, the circuits that match are stored there.
pub fn run_path(path: &Path, exe: &Path, golden: Option<&Path>) -> String {
    let work = std::env::temp_dir().join("circuitcad_xcheck_ac");
    let _ = std::fs::create_dir_all(&work);
    let mut report = format!("AC解析の検算 (自前エンジン vs ngspice, {F_START}Hz〜{F_STOP:e}Hz)  合格: 複素数の差が 相対{REL_TOL} + 最大値の{PEAK_TOL}倍 以内\n\n");
    let mut counts = [0usize; 4];
    let (mut stored, mut left_out) = (vec![], vec![]);
    for f in example_files(path) {
        let tag = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let mut keep = None;
        let outcome = match load(&f) {
            Ok(s) => check_ac_keeping(&s, exe, &work, &tag, &mut keep),
            Err(e) => AcOutcome::Failed(format!("読み込めません: {e}")),
        };
        counts[match outcome {
            AcOutcome::Match { .. } => 0,
            AcOutcome::Differ { .. } => 1,
            AcOutcome::Skipped(_) => 2,
            AcOutcome::Failed(_) => 3,
        }] += 1;
        let text = match &outcome {
            AcOutcome::Match { nodes, worst, err } => format!("{nodes} ノード。最大: {worst} (許容の{:.1}%)", err * 100.0),
            AcOutcome::Differ { nodes, worst, err, bad } => format!("{bad}/{nodes} ノードがズレ。最大: {worst} (許容の{err:.1}倍)"),
            AcOutcome::Skipped(m) | AcOutcome::Failed(m) => m.clone(),
        };
        report.push_str(&format!("[{}] {tag}: {text}\n", outcome.label()));
        match keep {
            Some(g) => stored.push((tag, g)),
            None if !matches!(outcome, AcOutcome::Skipped(_)) => left_out.push(format!("{tag} ({})", outcome.label())),
            None => {}
        }
    }
    report.push_str(&format!("\n一致 {} / ズレ {} / 対象外 {} / 失敗 {}\n", counts[0], counts[1], counts[2], counts[3]));
    if let Some(out) = golden {
        let lines: Vec<String> = stored
            .iter()
            .map(|(tag, g)| {
                let nodes: serde_json::Map<String, serde_json::Value> =
                    g.nodes.iter().map(|(k, p)| (k.clone(), serde_json::json!(p.iter().map(|(re, im)| [round8(*re), round8(*im)]).collect::<Vec<_>>()))).collect();
                format!("    {}: {}", serde_json::json!(tag), serde_json::json!({ "freq": g.freq.iter().map(|x| round8(*x)).collect::<Vec<_>>(), "nodes": nodes }))
            })
            .collect();
        let text = format!(
            "{{\n  \"about\": \"ngspice's AC small-signal answers (real, imaginary per node and frequency) for the shipped examples; written by `circuitCad --xcheck docs/examples --ac --golden docs/xcheck/ngspice_ac.json`\",\n  \"circuits\": {{\n{}\n  }}\n}}\n",
            lines.join(",\n")
        );
        let written = std::fs::create_dir_all(out.parent().unwrap_or(Path::new("."))).and_then(|_| std::fs::write(out, text));
        report.push_str(&format!("{} 回路を {} に保存 ({:?})\n保存しなかった: {}\n", lines.len(), out.display(), written.map_err(|e| e.to_string()), left_out.join(", ")));
    }
    report
}

/// Read the stored file: circuit name -> answer.
#[cfg(test)]
pub fn read_golden(text: &str) -> Vec<(String, AcGolden)> {
    let json: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
    let num = |v: &serde_json::Value| v.as_f64().unwrap_or(f64::NAN);
    json["circuits"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, c)| {
            let nodes = c["nodes"].as_object().into_iter().flatten().map(|(k, p)| (k.clone(), p.as_array().into_iter().flatten().map(|x| (num(&x[0]), num(&x[1]))).collect())).collect();
            (name.clone(), AcGolden { freq: c["freq"].as_array().into_iter().flatten().map(num).collect(), nodes })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The AC sweep of every stored example must still agree with ngspice's, without running ngspice.
    /// After a deliberate model change regenerate the file with
    /// `circuitCad --xcheck docs/examples --ac --golden docs/xcheck/ngspice_ac.json` and read the diff.
    #[test]
    fn ac_sweeps_still_match_the_stored_ngspice_answers() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let stored = read_golden(&std::fs::read_to_string(root.join(GOLDEN_AC)).expect("stored ngspice AC answers missing: see the doc comment of this test"));
        assert!(stored.len() >= 5, "only {} circuits are stored", stored.len());
        let mut bad = vec![];
        for (name, g) in &stored {
            let s = load(&root.join(format!("docs/examples/{name}.schjson"))).unwrap();
            if let Err(m) = check_against_stored(&s, g) {
                bad.push(format!("{name}: {m}"));
            }
        }
        assert!(bad.is_empty(), "differs from ngspice:\n{}", bad.join("\n"));
    }

    #[test]
    fn wrdata_pairs_and_error_measure() {
        let (f, n) = parse_ac_wrdata(" frequency v(a) v(a) v(b) v(b)\n 1 1 0 0.5 -0.5\n 10 1 0 0.1 -0.1\n").unwrap();
        assert_eq!(f, vec![1.0, 10.0]);
        assert_eq!(n[1], ("v(b)".to_string(), vec![(0.5, -0.5), (0.1, -0.1)]));
        assert!(phasor_error(&n[1].1, &n[1].1) == 0.0);
        let off: Vec<(f64, f64)> = n[1].1.iter().map(|(re, im)| (re * 1.01, *im)).collect();
        assert!(phasor_error(&off, &n[1].1) > 1.0);
        // a 1 ppm wobble on a node that is far below its own peak is no mismatch
        let notch = [(1.0, 0.0), (1e-12, 0.0)];
        assert!(phasor_error(&[(1.0, 0.0), (5e-10, 0.0)], &notch) < 1.0);
    }
}
