//! Developer tool: cross-check the DC operating point of our own engine against ngspice.
//!
//! `circuitCad --xcheck <file.schjson | folder> [--ngspice <exe>] [--out report.txt]`
//! The app has no console on Windows, so the report always goes to a file (default `xcheck_report.txt`).

use sch_core::{extract, spice_node, to_spice_for, Dialect, Kind, Schematic};
use sim_core::{dc_op, from_schematic, parse_source_full, parse_value};
use std::path::{Path, PathBuf};
use std::process::Command;

/// A node agrees when |a-b| <= ABS_TOL + REL_TOL*max(|a|,|b|) (ngspice's own reltol is 1e-3, vntol 1 uV).
const REL_TOL: f64 = 1e-3;
const ABS_TOL: f64 = 1e-5;
/// Currents are compared as `i(<source>)` entries, with their own absolute tolerance (ngspice's abstol is 1 pA).
const ABS_TOL_I: f64 = 1e-9;

fn tolerance(name: &str, a: f64, b: f64) -> f64 {
    (if name.starts_with("i(") { ABS_TOL_I } else { ABS_TOL }) + REL_TOL * a.abs().max(b.abs())
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Match { nodes: usize },
    /// Differs under ngspice's default gmin (1e-12) but agrees with gmin=1e-15: ngspice's own leakage
    /// moves a node that sits on a steep part of a behavioural source.
    MatchLowGmin { nodes: usize, node: String, ours: f64, theirs: f64 },
    Differ { nodes: usize, worst: String, ours: f64, theirs: f64, bad: usize },
    /// Not comparable (and why): the file is left out of the pass/fail count.
    Skipped(String),
    /// One of the engines failed on a circuit that should be solvable.
    Failed(String),
}

impl Outcome {
    fn label(&self) -> &'static str {
        match self {
            Outcome::Match { .. } => "一致",
            Outcome::MatchLowGmin { .. } => "gmin依存",
            Outcome::Differ { .. } => "ズレ",
            Outcome::Skipped(_) => "対象外",
            Outcome::Failed(_) => "失敗",
        }
    }
}

/// Node voltages and source/inductor branch currents from ngspice's batch output (`.op` prints
/// `name<TAB>value` lines). Names are lower-cased; `v1#branch` becomes `i(v1)`.
pub fn parse_op_output(text: &str) -> Vec<(String, f64)> {
    let mut v = vec![];
    for line in text.lines() {
        let mut t = line.split_whitespace();
        let (Some(name), Some(mut val)) = (t.next(), t.next()) else { continue };
        // `print` lines read `name = value`, the older table `name value`
        if val == "=" {
            let Some(x) = t.next() else { continue };
            val = x;
        }
        if t.next().is_some() {
            continue;
        }
        // `@r1[i]`, `@d1[id]`, `@q1[ic]`: a device current, named like a source current
        if let Some(dev) = name.strip_prefix('@').and_then(|n| n.split('[').next()) {
            if let Ok(x) = val.parse::<f64>() {
                v.push((format!("i({})", dev.to_ascii_lowercase()), x));
            }
            continue;
        }
        if name.contains('[') {
            continue;
        }
        if let Some(src) = name.strip_suffix("#branch") {
            if let Ok(x) = val.parse::<f64>() {
                v.push((format!("i({})", src.to_ascii_lowercase()), x));
            }
            continue;
        }
        if let Ok(x) = val.parse::<f64>()
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '$') {
                v.push((name.to_ascii_lowercase(), x));
            }
    }
    v
}

/// Compare node voltages by name (`ours` keyed by the SPICE node name). Returns the outcome.
pub fn compare(ours: &[(String, f64)], theirs: &[(String, f64)]) -> Outcome {
    let (mut n, mut bad) = (0, 0);
    let mut worst: Option<(f64, String, f64, f64)> = None;
    for (name, a) in ours {
        let Some((_, b)) = theirs.iter().find(|(m, _)| m == &name.to_ascii_lowercase()) else { continue };
        n += 1;
        let d = (a - b).abs();
        // rank by how far past its own tolerance a node is, so the worst offender is named
        let excess = d / tolerance(&name.to_ascii_lowercase(), *a, *b);
        if excess > 1.0 {
            bad += 1;
        }
        if worst.as_ref().is_none_or(|w| excess > w.0) {
            worst = Some((excess, name.clone(), *a, *b));
        }
    }
    match worst {
        None => Outcome::Skipped("名前が一致するノードがありません".into()),
        Some((_, name, ours, theirs)) if bad > 0 => Outcome::Differ { nodes: n, worst: name, ours, theirs, bad },
        Some(_) => Outcome::Match { nodes: n },
    }
}

fn find_ngspice(arg: Option<&str>) -> Option<PathBuf> {
    let cands = [
        arg.map(PathBuf::from),
        std::env::var_os("NGSPICE").map(PathBuf::from),
        Some(PathBuf::from(r"F:\Tools\ngspice\Spice64\bin\ngspice_con.exe")),
        Some(PathBuf::from("ngspice_con")),
        Some(PathBuf::from("ngspice")),
    ];
    cands.into_iter().flatten().find(|p| Command::new(p).arg("-v").output().is_ok())
}

/// Our side of the check: the schematic with time-varying sources replaced by their DC value, and our
/// DC solution as (SPICE name, value) pairs. `Err(outcome)` when the circuit cannot be compared at all;
/// the inner `Err` is our solver's failure (ngspice may still be run to see whether it solves it).
#[allow(clippy::type_complexity)]
fn our_side(s: &Schematic) -> Result<(Schematic, Result<Vec<(String, f64)>, String>), Outcome> {
    // The SPICE export writes sources as `DC <value>`, so SIN/PULSE/PWL/AC sources are replaced by
    // the number our DC solver uses for them. This checks the solvers, not the source parsing.
    let mut flat = s.clone();
    for c in flat.components.iter_mut().filter(|c| c.kind == Kind::VSource && parse_value(&c.value).is_none()) {
        match parse_source_full(&c.value) {
            Ok(src) => c.value = format!("{:e}", src.wave.dc_value()),
            Err(m) => return Err(Outcome::Skipped(format!("{} の値を読めません ({}): {m}", c.refdes, c.value))),
        }
    }
    if flat.components.iter().any(|c| c.kind == Kind::Custom && c.custom.as_ref().is_none_or(|x| x.sim.is_none())) {
        return Err(Outcome::Skipped("シミュレーションモデルのない部品があります".into()));
    }
    let circuit = match from_schematic(&flat) {
        Ok(c) => c,
        Err(e) => return Err(Outcome::Failed(format!("自前エンジン: 回路を作れません ({})", e.first().map(|x| x.to_string()).unwrap_or_default()))),
    };
    let ours = dc_op(&circuit).map_err(|e| e.to_string()).map(|r| {
        // only nets of the schematic, under the names to_spice gave them
        let nets: Vec<String> = extract(&flat).iter().map(|n| n.name.clone()).collect();
        r.voltages
            .iter()
            .filter(|(n, _)| nets.contains(n))
            .map(|(n, v)| (spice_node(n), *v))
            .filter(|(n, _)| n != "0")
            .chain(r.elements.iter().map(|e| (format!("i({})", e.name.to_ascii_lowercase()), e.current)))
            .collect()
    });
    Ok((flat, ours))
}

/// Check one circuit. `exe` is the ngspice executable, `work` a scratch folder.
pub fn check_schematic(s: &Schematic, exe: &Path, work: &Path, tag: &str) -> Outcome {
    check_with_raw(s, exe, work, tag).0
}

/// Like `check_schematic`, and also ngspice's raw answer under its default settings (empty when ngspice
/// was not run or failed): the values stored in `docs/xcheck/ngspice_dc.json`.
pub fn check_with_raw(s: &Schematic, exe: &Path, work: &Path, tag: &str) -> (Outcome, Vec<(String, f64)>) {
    let (s, ours) = match our_side(s) {
        Ok(x) => x,
        Err(o) => return (o, vec![]),
    };
    let body = to_spice_for(&s, tag, Dialect::Ngspice);
    let body = body.strip_suffix(".end
").unwrap_or(&body);
    let deck = analysis_deck(body, None);
    let theirs = match run_ngspice(exe, work, tag, &deck) {
        Ok(v) => v,
        Err(m) => {
            return (
                Outcome::Failed(match &ours {
                    Ok(_) => m,
                    Err(e) => format!("自前エンジン: {e} / {m}"),
                }),
                vec![],
            )
        }
    };
    let raw = theirs.clone();
    let mut mine = match ours {
        Ok(r) => r,
        Err(e) => return (Outcome::Failed(format!("自前エンジン: {e} / ngspice は解けた ({} ノード)", theirs.len())), raw),
    };
    let theirs = alias_cards(theirs, &mut mine);
    let first = compare(&mine, &theirs);
    let Outcome::Differ { nodes, worst, ours: a, theirs: b, .. } = &first else { return (first, raw) };
    // retry with ngspice's leakage turned (almost) off
    let low = analysis_deck(body, Some(1e-15));
    let outcome = match run_ngspice(exe, work, &format!("{tag}_lowgmin"), &low) {
        Ok(t2) if matches!(compare(&mine, &alias_cards(t2.clone(), &mut mine.clone())), Outcome::Match { .. }) => {
            Outcome::MatchLowGmin { nodes: *nodes, node: worst.clone(), ours: *a, theirs: *b }
        }
        _ => first,
    };
    (outcome, raw)
}

#[cfg(test)]
/// Compare our DC solution of `s` with ngspice's answer stored earlier (no ngspice needed).
pub fn check_against_stored(s: &Schematic, stored: &[(String, f64)]) -> Outcome {
    match our_side(s) {
        Err(o) => o,
        Ok((_, Err(e))) => Outcome::Failed(format!("自前エンジン: {e}")),
        Ok((_, Ok(mut mine))) => {
            let theirs = alias_cards(stored.to_vec(), &mut mine);
            compare(&mine, &theirs)
        }
    }
}

/// The deck plus an operating-point run that prints every node voltage, every source current and
/// the current of each top-level resistor, diode, BJT and MOSFET (`.op` alone prints no device currents).
fn analysis_deck(body: &str, gmin: Option<f64>) -> String {
    let mut deck = body.to_string();
    // ngspice's default reltol (1e-3) leaves its own currents inconsistent with each other (a diode
    // 3.6 % off the resistor in series with it), so solve as tightly as our engine does
    deck.push_str(".options reltol=1e-6 vntol=1e-8 abstol=1e-12\n");
    if let Some(g) = gmin {
        deck.push_str(&format!(".options gmin={g:e}\n"));
    }
    deck.push_str(".control\nset noaskquit\nop\nprint all\n");
    // element cards come first, before the first dot card
    for line in body.lines().skip(1).take_while(|l| !l.starts_with('.')) {
        let Some(card) = line.split_whitespace().next().filter(|c| !c.starts_with(['*', '+'])) else { continue };
        let prop = match card.chars().next().map(|c| c.to_ascii_uppercase()) {
            Some('R') => "i",
            Some('D' | 'M') => "id",
            Some('Q') => "ic",
            _ => continue,
        };
        deck.push_str(&format!("print @{}[{prop}]\n", card.to_ascii_lowercase()));
    }
    deck.push_str(".endc\n.end\n");
    deck
}

/// Our MOSFET is `Q1` while the card is `MQ1`: let the ngspice name follow ours when only that differs.
///
/// ngspice reports a PMOS drain current with the opposite sign when the channel conducts the other
/// way round (drain above source), while the node voltages and the neighbours' currents agree with
/// ours. So MOSFET currents are compared by size: both sides are made positive.
fn alias_cards(theirs: Vec<(String, f64)>, mine: &mut [(String, f64)]) -> Vec<(String, f64)> {
    let mut mos = vec![];
    let out = theirs
        .into_iter()
        .map(|(n, v)| {
            let inner = n.strip_prefix("i(m").and_then(|r| r.strip_suffix(')')).map(|r| format!("i({r})"));
            match inner {
                Some(alias) if mine.iter().any(|(m, _)| *m == alias) && !mine.iter().any(|(m, _)| *m == n) => {
                    mos.push(alias.clone());
                    (alias, v.abs())
                }
                _ => (n, v),
            }
        })
        .collect();
    for (m, v) in mine.iter_mut() {
        if mos.contains(m) {
            *v = v.abs();
        }
    }
    out
}

/// Run an ngspice deck whose results go to a file (`wrdata`); fails with ngspice's first error line.
pub(crate) fn run_ngspice_script(exe: &Path, work: &Path, tag: &str, script: &str) -> Result<(), String> {
    let cir = work.join(format!("{tag}_script.cir"));
    std::fs::write(&cir, script).map_err(|_| "作業ファイルを書けません".to_string())?;
    let out = Command::new(exe).arg("-b").arg(&cir).output().map_err(|e| format!("ngspice を起動できません: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    // "doAnalyses: TRAN: Timestep too small ..." says more than the "empty netlist" error that follows it
    let why = text.lines().find(|l| l.contains("doAnalyses")).or_else(|| text.lines().find(|l| l.to_ascii_lowercase().contains("error")));
    match why {
        Some(why) => Err(format!("ngspice: {}", why.trim())),
        None => Ok(()),
    }
}

/// Run ngspice in batch mode on a deck; returns its node voltages or why it gave none.
fn run_ngspice(exe: &Path, work: &Path, tag: &str, deck: &str) -> Result<Vec<(String, f64)>, String> {
    let cir = work.join(format!("{tag}.cir"));
    std::fs::write(&cir, deck).map_err(|_| "作業ファイルを書けません".to_string())?;
    let out = Command::new(exe).arg("-b").arg(&cir).output().map_err(|e| format!("ngspice を起動できません: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    let v = parse_op_output(&text);
    if v.is_empty() {
        let why = text.lines().find(|l| l.to_ascii_lowercase().contains("error")).unwrap_or("結果が出ませんでした");
        return Err(format!("ngspice: {}", why.trim()));
    }
    Ok(v)
}

fn describe(o: &Outcome) -> String {
    match o {
        Outcome::Match { nodes } => format!("{nodes} 値 (電圧と電流)"),
        Outcome::MatchLowGmin { nodes, node, ours, theirs } => format!(
            "{nodes} 値。ngspice の既定gmin(1e-12)では {node} が 自前={ours:.4e} ngspice={theirs:.4e} とずれるが、gmin=1e-15 なら一致 (自前は gmin→0 の値)"
        ),
        Outcome::Differ { nodes, worst, ours, theirs, bad } => {
            format!("{bad}/{nodes} 値がズレ。最大: {worst} 自前={ours:.6e} ngspice={theirs:.6e} (差 {:.3e})", (ours - theirs).abs())
        }
        Outcome::Skipped(m) | Outcome::Failed(m) => m.clone(),
    }
}

/// Check a file or every `.schjson` in a folder; returns the report text.
pub fn run_path(path: &Path, exe: &Path) -> String {
    let files: Vec<PathBuf> = if path.is_dir() {
        let mut v: Vec<PathBuf> = std::fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "schjson"))
            .collect();
        v.sort();
        v
    } else {
        vec![path.to_path_buf()]
    };
    let work = std::env::temp_dir().join("circuitcad_xcheck");
    let _ = std::fs::create_dir_all(&work);
    let mut report = format!("DC動作点の検算 (自前エンジン vs ngspice)  許容: 相対{REL_TOL} + 絶対{ABS_TOL}V (電流は{ABS_TOL_I}A)\n\n");
    let mut counts = [0usize; 5];
    for f in &files {
        let tag = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let outcome = match std::fs::read_to_string(f).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<Schematic>(&t).map_err(|e| e.to_string())) {
            Ok(s) => check_schematic(&s, exe, &work, &tag),
            Err(e) => Outcome::Failed(format!("読み込めません: {e}")),
        };
        counts[match outcome {
            Outcome::Match { .. } => 0,
            Outcome::Differ { .. } => 1,
            Outcome::Skipped(_) => 2,
            Outcome::Failed(_) => 3,
            Outcome::MatchLowGmin { .. } => 4,
        }] += 1;
        report.push_str(&format!("[{}] {tag}: {}\n", outcome.label(), describe(&outcome)));
    }
    report.push_str(&format!("\n一致 {} / gmin依存 {} / ズレ {} / 対象外 {} / 失敗 {}\n", counts[0], counts[4], counts[1], counts[2], counts[3]));
    report
}

/// Where ngspice's answers for the shipped examples are kept (see `write_golden`).
#[cfg(test)]
pub const GOLDEN_DC: &str = "docs/xcheck/ngspice_dc.json";

/// Run ngspice on every example of a folder and store its raw answer for each circuit that agrees with
/// ours under ngspice's default settings. CI compares against this file, so it needs no ngspice.
pub fn write_golden(dir: &Path, exe: &Path, out: &Path) -> String {
    let work = std::env::temp_dir().join("circuitcad_xcheck");
    let _ = std::fs::create_dir_all(&work);
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "schjson")).collect();
    files.sort();
    let mut circuits = serde_json::Map::new();
    let mut left_out = vec![];
    for f in &files {
        let tag = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let Ok(s) = std::fs::read_to_string(f).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<Schematic>(&t).map_err(|e| e.to_string())) else { continue };
        let (outcome, raw) = check_with_raw(&s, exe, &work, &tag);
        if matches!(outcome, Outcome::Match { .. }) {
            let values: serde_json::Map<String, serde_json::Value> = raw.into_iter().map(|(n, v)| (n, serde_json::json!(v))).collect();
            circuits.insert(tag, values.into());
        } else {
            left_out.push(format!("{tag} ({})", outcome.label()));
        }
    }
    let json = serde_json::json!({ "about": "ngspice's DC operating point (raw output, default settings) for the shipped examples; written by `circuitCad --xcheck docs/examples --golden docs/xcheck/ngspice_dc.json`", "circuits": circuits });
    let written = std::fs::create_dir_all(out.parent().unwrap_or(Path::new("."))).and_then(|_| std::fs::write(out, serde_json::to_string_pretty(&json).unwrap_or_default()));
    format!("{} 回路を {} に保存 ({:?})
保存しなかった: {}
", circuits.len(), out.display(), written.map_err(|e| e.to_string()), left_out.join(", "))
}

/// Handle `--xcheck ...`; returns true when the flag was present (the GUI must not start then).
pub fn run_cli(args: &[String]) -> bool {
    let Some(i) = args.iter().position(|a| a == "--xcheck") else { return false };
    let val = |flag: &str| args.iter().position(|a| a == flag).and_then(|j| args.get(j + 1)).map(|s| s.as_str());
    let tran = args.iter().any(|a| a == "--tran");
    if let (Some(dir), Some(out), Some(exe)) = (args.get(i + 1).filter(|a| !a.starts_with("--")), val("--golden"), find_ngspice(val("--ngspice"))) {
        let report = if tran { crate::xcheck_tran::write_golden(Path::new(dir), &exe, Path::new(out)) } else { write_golden(Path::new(dir), &exe, Path::new(out)) };
        let _ = std::fs::write("xcheck_golden_report.txt", &report);
        print!("{report}");
        return true;
    }
    let out = val("--out").unwrap_or(if tran { "xcheck_tran_report.txt" } else { "xcheck_report.txt" });
    let report = match (args.get(i + 1).filter(|a| !a.starts_with("--")), find_ngspice(val("--ngspice"))) {
        (None, _) => "使い方: circuitCad --xcheck <file.schjson | フォルダ> [--tran [--tstop 秒] [--tstep 秒] [--ourstep 秒] [--ngstep 秒] [--lte 許容]] [--ngspice <exe>] [--out report.txt]\n".to_string(),
        (_, None) => "ngspice が見つかりません。--ngspice <exe> か環境変数 NGSPICE で指定してください\n".to_string(),
        (Some(p), Some(exe)) if tran => {
            let num = |flag: &str| val(flag).and_then(parse_value);
            crate::xcheck_tran::run_path(
                Path::new(p),
                &exe,
                crate::xcheck_tran::Given { tstop: num("--tstop"), tstep: num("--tstep"), ours_step: num("--ourstep"), ng_step: num("--ngstep"), lte: num("--lte") },
            )
        }
        (Some(p), Some(exe)) => run_path(Path::new(p), &exe),
    };
    let _ = std::fs::write(out, &report);
    print!("{report}");
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use sch_core::to_spice;

    #[test]
    fn parses_ngspice_op_lines() {
        let text = "Doing analysis at TEMP = 27.000000\n\tin                               1.000000e+01\n\tOut\t5.000000e+00\n\tv1#branch                        -5.00000e-03\nNo. of Data Rows : 1\n";
        let v = parse_op_output(text);
        assert_eq!(v, vec![("in".to_string(), 10.0), ("out".to_string(), 5.0), ("i(v1)".to_string(), -5e-3)]);
    }

    #[test]
    fn parses_print_lines_and_builds_the_analysis_deck() {
        let v = parse_op_output("n_2 = 1.948002e+00\nv1#branch = -9.24848e-03\n@r1[i] = 9.248479e-03\n@mq1[id] = -2.7e-02\n");
        assert_eq!(
            v,
            vec![("n_2".to_string(), 1.948002), ("i(v1)".to_string(), -9.24848e-3), ("i(r1)".to_string(), 9.248479e-3), ("i(mq1)".to_string(), -2.7e-2)]
        );
        let deck = analysis_deck("* t\nV1 a 0 DC 5\nR1 a b 1k\nD1 b 0 DDEF\nQ1 c b 0 QNPN\nMQ2 a b 0 0 NM\nXU1 a b SUB\n.model DDEF D\n", None);
        for want in ["print @r1[i]\n", "print @d1[id]\n", "print @q1[ic]\n", "print @mq2[id]\n", "reltol=1e-6"] {
            assert!(deck.contains(want), "{want} missing in {deck}");
        }
        assert!(!deck.contains("@v1") && !deck.contains("@xu1"));
        assert!(analysis_deck("* t\nR1 a 0 1\n", Some(1e-15)).contains("gmin=1e-15"));
    }

    #[test]
    fn mosfet_currents_are_compared_by_size() {
        let mut mine = vec![("i(q1)".to_string(), 0.027)];
        let theirs = alias_cards(vec![("i(mq1)".to_string(), -0.027)], &mut mine);
        assert_eq!(theirs, vec![("i(q1)".to_string(), 0.027)]);
        assert!(matches!(compare(&mine, &theirs), Outcome::Match { .. }));
    }

    #[test]
    fn compare_flags_only_real_differences() {
        let theirs = vec![("a".to_string(), 5.0), ("b".to_string(), 0.7)];
        assert!(matches!(compare(&[("a".into(), 5.0004), ("b".into(), 0.7)], &theirs), Outcome::Match { nodes: 2 }));
        match compare(&[("a".into(), 5.0), ("B".into(), 0.65)], &theirs) {
            Outcome::Differ { worst, bad, .. } => assert_eq!((worst.as_str(), bad), ("B", 1)),
            o => panic!("{o:?}"),
        }
        assert!(matches!(compare(&[("zz".into(), 1.0)], &theirs), Outcome::Skipped(_)));
    }

    /// Needs ngspice (see `find_ngspice`); run with `cargo test -p app xcheck_examples -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn xcheck_examples() {
        let exe = find_ngspice(None).expect("ngspice not found");
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        println!("{}", run_path(&dir, &exe));
    }

    /// The DC operating point of every example stored in `docs/xcheck/ngspice_dc.json` (ngspice's own answer)
    /// must still match ours. Needs no ngspice. After a deliberate model change, regenerate the file with
    /// `circuitCad --xcheck docs/examples --golden docs/xcheck/ngspice_dc.json` and read the diff.
    #[test]
    fn dc_points_still_match_the_stored_ngspice_answers() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let text = std::fs::read_to_string(root.join(GOLDEN_DC)).expect("stored ngspice answers missing: see the doc comment of this test");
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        let circuits = json["circuits"].as_object().unwrap();
        assert!(circuits.len() >= 20, "only {} circuits are stored", circuits.len());
        let mut bad = vec![];
        for (name, values) in circuits {
            let stored: Vec<(String, f64)> = values.as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_f64().unwrap())).collect();
            let s: Schematic = serde_json::from_str(&std::fs::read_to_string(root.join(format!("docs/examples/{name}.schjson"))).unwrap()).unwrap();
            let o = check_against_stored(&s, &stored);
            if !matches!(o, Outcome::Match { .. }) {
                bad.push(format!("{name}: {}", describe(&o)));
            }
        }
        assert!(bad.is_empty(), "differs from ngspice:
{}", bad.join("
"));
    }

    /// These examples used to end in "did not converge" in the DC panel (found by the ngspice cross-check).
    #[test]
    fn switch_heavy_examples_have_a_dc_point() {
        for name in ["ne555_blinker", "ip2332_charger", "tp5100_charger", "tp5100_cell_charge"] {
            let p = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../docs/examples/{name}.schjson"));
            let s: Schematic = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
            let r = dc_op(&from_schematic(&s).unwrap());
            assert!(r.is_ok(), "{name}: {:?}", r.err());
        }
    }

    #[test]
    fn mosfet_cards_are_exported_with_four_nodes() {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/mosfet_switch.schjson");
        let s: Schematic = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        assert!(to_spice(&s, "t").contains("MQ1 OUT G 0 0 2N7000"));
    }
}
