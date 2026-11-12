//! A snapshot of the simulator's results and speed, for comparing a change of `sim-core` before and after
//! (`tools/sim_compare.py`, `.claude/skills/sim-compare`). Writes into `$SIM_SNAPSHOT_DIR`:
//! - `waves_<case>.tsv`: every signal of the combined demo without and with its board wiring and of the op-amp circuit, at every
//!   point (compared byte for byte), and `waves.txt` with each case's time (best of `$SIM_SNAPSHOT_REPEAT`, default 3) and points;
//! - `examples.tsv`: for every bundled example the transient `xcheck_tran` would run: best time, points, retries, rejected steps
//!   and a fingerprint of all its values (equal fingerprints: the same waveform to the last bit);
//! - with `$SIM_SNAPSHOT_NGSPICE` (the ngspice console executable): `ngspice.tsv`, ngspice's own transient time and points for
//!   the same examples and settings (its process start is left out).
//!
//! `$SIM_SNAPSHOT_ONLY` (names separated by commas) limits the examples and leaves the demos out.
//! `cargo test --release -p app sim_snapshot -- --ignored --nocapture` (the script sets the variables).

use crate::xcheck_tran::{own_dynamics, pick_settings};
use sch_core::{to_spice_for, Dialect, Kind, Schematic};
use sim_core::{from_schematic, transient, TranResult, TranSettings};
use std::path::PathBuf;

fn repeat() -> usize {
    std::env::var("SIM_SNAPSHOT_REPEAT").ok().and_then(|v| v.parse().ok()).unwrap_or(3).max(1)
}

/// Best wall time of `repeat()` runs and the last result.
fn timed(c: &sim_core::Circuit, s: &TranSettings) -> (f64, Result<TranResult, sim_core::SimError>) {
    let mut best = f64::MAX;
    let mut last = None;
    for _ in 0..repeat() {
        let t = std::time::Instant::now();
        let r = transient(c, s);
        best = best.min(t.elapsed().as_secs_f64());
        last = Some(r);
    }
    (best, last.unwrap())
}

/// FNV-1a over the bits of the time axis and every value: equal when the waveforms are equal to the last bit.
fn fingerprint(r: &TranResult) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |x: f64| {
        for b in x.to_bits().to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    };
    r.time.iter().for_each(|t| eat(*t));
    for s in &r.signals {
        s.values.iter().for_each(|v| eat(*v));
    }
    h
}

fn tsv(r: &TranResult) -> String {
    let mut s = String::from("t");
    for sig in &r.signals {
        s.push('\t');
        s.push_str(&sig.name);
    }
    s.push('\n');
    for (i, t) in r.time.iter().enumerate() {
        s.push_str(&format!("{t:e}"));
        for sig in &r.signals {
            s.push_str(&format!("\t{:e}", sig.values[i]));
        }
        s.push('\n');
    }
    s
}

fn examples_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/examples"))
}

/// The bundled examples `xcheck_tran` can run, with its settings.
fn examples() -> Vec<(String, Schematic, f64, f64)> {
    let only: Option<Vec<String>> = std::env::var("SIM_SNAPSHOT_ONLY").ok().filter(|v| !v.is_empty()).map(|v| v.split(',').map(|s| s.trim().to_string()).collect());
    let mut names: Vec<PathBuf> = std::fs::read_dir(examples_dir()).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "schjson")).collect();
    names.sort();
    let mut out = vec![];
    for p in names {
        let tag = p.file_stem().unwrap().to_string_lossy().to_string();
        if only.as_ref().is_some_and(|o| !o.contains(&tag)) {
            continue;
        }
        let Ok(s) = serde_json::from_str::<Schematic>(&std::fs::read_to_string(&p).unwrap()) else { continue };
        if s.components.iter().any(|c| c.kind == Kind::Custom && c.custom.as_ref().is_none_or(|x| x.sim.is_none())) {
            continue;
        }
        let (tstop, tstep) = match pick_settings(&s) {
            Ok(Some(x)) => x,
            Ok(None) => own_dynamics(&tag),
            Err(_) => continue,
        };
        out.push((tag, s, tstop, tstep));
    }
    out
}

/// The demos: the combined demo without and with its board wiring, and the op-amp circuit.
fn demo_waves(dir: &std::path::Path) {
    let p = crate::project::load(&examples_dir().join("mixed_signal.ccproj")).unwrap();
    let spec = pcb_core::irdrop::IrSpec::default();
    let series = pcb_core::irdrop::wiring_resistances(&p.board, &spec);
    let k: Vec<sim_core::WiringK> = pcb_core::irdrop::wiring_couplings(&series, &spec).into_iter().map(|c| sim_core::WiringK { a: c.a, b: c.b, k: c.k }).collect();
    let w: Vec<sim_core::WiringR> = series.into_iter().map(|w| sim_core::WiringR { refdes: w.refdes, pin: w.pin, ohm: w.ohm, henry: w.henry, farad: w.farad }).collect();
    let opamp: Schematic = serde_json::from_str(&std::fs::read_to_string(examples_dir().join("opamp_inverting.schjson")).unwrap()).unwrap();
    let cases = [
        ("plain", from_schematic(&p.schematic).unwrap(), 10e-3, 10e-6),
        ("postlayout", sim_core::from_schematic_with_layout(&p.schematic, &w, &k).unwrap(), 10e-3, 10e-6),
        ("opamp", from_schematic(&opamp).unwrap(), 2e-3, 1e-6),
    ];
    let mut summary = String::new();
    for (label, c, tstop, tstep) in cases {
        let (secs, r) = timed(&c, &TranSettings { tstop, tstep, uic: false });
        let r = r.unwrap();
        summary.push_str(&format!("{label}\t{secs:.4}\t{}\n", r.time.len()));
        std::fs::write(dir.join(format!("waves_{label}.tsv")), tsv(&r)).unwrap();
        println!("SNAPSHOT wave {label}: {secs:.3} s, {} points", r.time.len());
    }
    std::fs::write(dir.join("waves.txt"), summary).unwrap();
}

/// ngspice's own transient time and accepted points for the same examples and settings.
fn ngspice(dir: &std::path::Path, exe: &str, list: &[(String, Schematic, f64, f64)]) {
    let work = dir.join("ngspice_work");
    std::fs::create_dir_all(&work).unwrap();
    let mut rows = String::from("example\tseconds\tpoints\tnote\n");
    for (tag, s, tstop, tstep) in list {
        if s.components.iter().any(|c| c.value.to_ascii_uppercase().contains("IC=")) {
            continue;
        }
        let Ok(c) = from_schematic(s) else { continue };
        // an oscillator without an operating point starts from zero in both (as `xcheck_tran` does)
        let zero_start = transient(&c, &TranSettings { tstop: *tstop, tstep: *tstep, uic: false }).is_ok_and(|r| r.warnings.iter().any(|w| w.contains("すべてゼロ")));
        let body = to_spice_for(s, tag, Dialect::Ngspice);
        let body = body.strip_suffix(".end\n").unwrap_or(&body);
        let uic = if zero_start { " uic" } else { "" };
        let script = format!("{body}.options reltol=1e-6 vntol=1e-8 abstol=1e-12\n.control\nset noaskquit\ntran {tstep:e} {tstop:e} 0 {tstep:e}{uic}\nrusage everything\n.endc\n.end\n");
        let cir = work.join(format!("{tag}.cir"));
        std::fs::write(&cir, &script).unwrap();
        let Ok(out) = std::process::Command::new(exe).arg("-b").arg(&cir).output() else { continue };
        let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
        let grab = |key: &str| text.lines().find(|l| l.contains(key)).and_then(|l| l.split('=').nth(1)).map(|v| v.trim().to_string()).unwrap_or_default();
        let failed = text.lines().find(|l| l.contains("doAnalyses") || l.to_ascii_lowercase().contains("timestep too small")).map(|l| l.trim().to_string()).unwrap_or_default();
        rows.push_str(&format!("{tag}\t{}\t{}\t{failed}\n", grab("Transient analysis time"), grab("Accepted timepoints")));
    }
    std::fs::write(dir.join("ngspice.tsv"), rows).unwrap();
    println!("SNAPSHOT ngspice done");
}

#[test]
#[ignore]
fn sim_snapshot() {
    let dir = PathBuf::from(std::env::var("SIM_SNAPSHOT_DIR").expect("SIM_SNAPSHOT_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    if !std::env::var("SIM_SNAPSHOT_ONLY").is_ok_and(|v| !v.is_empty()) {
        demo_waves(&dir);
    }
    let list = examples();
    let mut rows = String::from("example\tseconds\tpoints\tretries\trejected\tfingerprint\n");
    for (tag, s, tstop, tstep) in &list {
        let Ok(c) = from_schematic(s) else { continue };
        let (secs, r) = timed(&c, &TranSettings { tstop: *tstop, tstep: *tstep, uic: false });
        match r {
            Ok(r) => rows.push_str(&format!("{tag}\t{secs:.5}\t{}\t{}\t{}\t{:016x}\n", r.time.len(), r.retries, r.rejected, fingerprint(&r))),
            Err(e) => rows.push_str(&format!("{tag}\t{secs:.5}\t0\t0\t0\tERR {e}\n")),
        }
    }
    std::fs::write(dir.join("examples.tsv"), rows).unwrap();
    println!("SNAPSHOT {} examples", list.len());
    if let Ok(exe) = std::env::var("SIM_SNAPSHOT_NGSPICE") {
        ngspice(&dir, &exe, &list);
    }
}
