//! Developer tool: cross-check the DC operating point of our own engine against ngspice.
//!
//! `circuitCad --xcheck <file.schjson | folder> [--ngspice <exe>] [--out report.txt]`
//! The app has no console on Windows, so the report always goes to a file (default `xcheck_report.txt`).

use sch_core::{extract, spice_node, to_spice, to_spice_for, Dialect, Kind, Schematic};
use sim_core::{dc_op, from_schematic, parse_source_full, parse_value};
use std::path::{Path, PathBuf};
use std::process::Command;

/// A node agrees when |a-b| <= ABS_TOL + REL_TOL*max(|a|,|b|) (ngspice's own reltol is 1e-3, vntol 1 uV).
const REL_TOL: f64 = 1e-3;
const ABS_TOL: f64 = 1e-5;

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Match { nodes: usize },
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
            Outcome::Differ { .. } => "ズレ",
            Outcome::Skipped(_) => "対象外",
            Outcome::Failed(_) => "失敗",
        }
    }
}

/// Node voltages from ngspice's batch output (`.op` prints `name<TAB>value` lines). Names are lower-cased.
pub fn parse_op_output(text: &str) -> Vec<(String, f64)> {
    let mut v = vec![];
    for line in text.lines() {
        let mut t = line.split_whitespace();
        let (Some(name), Some(val), None) = (t.next(), t.next(), t.next()) else { continue };
        // branch currents (`v1#branch`) and device internals are not node voltages
        if name.contains('#') || name.contains('[') {
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
        let excess = d / (ABS_TOL + REL_TOL * a.abs().max(b.abs()));
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

/// Check one circuit. `exe` is the ngspice executable, `work` a scratch folder.
pub fn check_schematic(s: &Schematic, exe: &Path, work: &Path, tag: &str) -> Outcome {
    // The SPICE export writes sources as `DC <value>`, so SIN/PULSE/PWL/AC sources are replaced by
    // the number our DC solver uses for them. This checks the solvers, not the source parsing.
    let mut flat = s.clone();
    for c in flat.components.iter_mut().filter(|c| c.kind == Kind::VSource && parse_value(&c.value).is_none()) {
        match parse_source_full(&c.value) {
            Ok(src) => c.value = format!("{:e}", src.wave.dc_value()),
            Err(m) => return Outcome::Skipped(format!("{} の値を読めません ({}): {m}", c.refdes, c.value)),
        }
    }
    let s = &flat;
    if s.components.iter().any(|c| c.kind == Kind::Custom && c.custom.as_ref().is_none_or(|x| x.sim.is_none())) {
        return Outcome::Skipped("シミュレーションモデルのない部品があります".into());
    }
    let circuit = match from_schematic(s) {
        Ok(c) => c,
        Err(e) => return Outcome::Failed(format!("自前エンジン: 回路を作れません ({})", e.first().map(|x| x.to_string()).unwrap_or_default())),
    };
    let ours = dc_op(&circuit);

    let mut deck = to_spice_for(s, tag, Dialect::Ngspice);
    deck = deck.replace("\n.end\n", "\n.op\n.end\n");
    let cir = work.join(format!("{tag}.cir"));
    if std::fs::write(&cir, &deck).is_err() {
        return Outcome::Failed("作業ファイルを書けません".into());
    }
    let out = match Command::new(exe).arg("-b").arg(&cir).output() {
        Ok(o) => o,
        Err(e) => return Outcome::Failed(format!("ngspice を起動できません: {e}")),
    };
    let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    let theirs = parse_op_output(&text);
    let ours = match ours {
        Ok(r) => r,
        Err(e) => {
            let other = if theirs.is_empty() { "ngspice も解けない".to_string() } else { format!("ngspice は解けた ({} ノード)", theirs.len()) };
            return Outcome::Failed(format!("自前エンジン: {e} / {other}"));
        }
    };
    if theirs.is_empty() {
        let why = text.lines().find(|l| l.to_ascii_lowercase().contains("error")).unwrap_or("結果が出ませんでした");
        return Outcome::Failed(format!("ngspice: {}", why.trim()));
    }

    // only nets of the schematic, under the names to_spice gave them
    let nets: Vec<String> = extract(s).iter().map(|n| n.name.clone()).collect();
    let mine: Vec<(String, f64)> = ours
        .voltages
        .iter()
        .filter(|(n, _)| nets.contains(n))
        .map(|(n, v)| (spice_node(n), *v))
        .filter(|(n, _)| n != "0")
        .collect();
    compare(&mine, &theirs)
}

fn describe(o: &Outcome) -> String {
    match o {
        Outcome::Match { nodes } => format!("{nodes} ノード"),
        Outcome::Differ { nodes, worst, ours, theirs, bad } => {
            format!("{bad}/{nodes} ノードがズレ。最大: {worst} 自前={ours:.6} ngspice={theirs:.6} (差 {:.3e})", (ours - theirs).abs())
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
    let mut report = format!("DC動作点の検算 (自前エンジン vs ngspice)  許容: 相対{REL_TOL} + 絶対{ABS_TOL}V\n\n");
    let mut counts = [0usize; 4];
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
        }] += 1;
        report.push_str(&format!("[{}] {tag}: {}\n", outcome.label(), describe(&outcome)));
    }
    report.push_str(&format!("\n一致 {} / ズレ {} / 対象外 {} / 失敗 {}\n", counts[0], counts[1], counts[2], counts[3]));
    report
}

/// Handle `--xcheck ...`; returns true when the flag was present (the GUI must not start then).
pub fn run_cli(args: &[String]) -> bool {
    let Some(i) = args.iter().position(|a| a == "--xcheck") else { return false };
    let val = |flag: &str| args.iter().position(|a| a == flag).and_then(|j| args.get(j + 1)).map(|s| s.as_str());
    let out = val("--out").unwrap_or("xcheck_report.txt");
    let report = match (args.get(i + 1).filter(|a| !a.starts_with("--")), find_ngspice(val("--ngspice"))) {
        (None, _) => "使い方: circuitCad --xcheck <file.schjson | フォルダ> [--ngspice <exe>] [--out report.txt]\n".to_string(),
        (_, None) => "ngspice が見つかりません。--ngspice <exe> か環境変数 NGSPICE で指定してください\n".to_string(),
        (Some(p), Some(exe)) => run_path(Path::new(p), &exe),
    };
    let _ = std::fs::write(out, &report);
    print!("{report}");
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ngspice_op_lines() {
        let text = "Doing analysis at TEMP = 27.000000\n\tin                               1.000000e+01\n\tOut\t5.000000e+00\n\tv1#branch                        -5.00000e-03\nNo. of Data Rows : 1\n";
        let v = parse_op_output(text);
        assert_eq!(v, vec![("in".to_string(), 10.0), ("out".to_string(), 5.0)]);
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
