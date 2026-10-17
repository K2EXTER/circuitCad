//! Convergence stress: every shipped sample circuit at the temperature extremes and with its resistors scaled
//! at random must still have a DC operating point (`cargo test -p app converge_explore -- --ignored --nocapture`
//! lists the failures; `shipped_samples_keep_their_operating_point` is the guard).

use super::*;

/// A small deterministic random source (xorshift): the same circuits every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn samples() -> Vec<(String, sch_core::Schematic)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "schjson"))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), serde_json::from_str::<sch_core::Schematic>(&std::fs::read_to_string(e.path()).unwrap()).unwrap()))
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

/// The failures of one circuit: (what was tried, the error).
fn trouble(s: &sch_core::Schematic, tries: usize, seed: u64) -> Vec<(String, String)> {
    let mut out = vec![];
    let Ok(base) = sim_core::from_schematic(s) else { return out };
    if sim_core::dc_op(&base).is_err() {
        // no operating point to start with (an oscillator, a latch): nothing to compare
        return out;
    }
    for t in [-40.0, 125.0] {
        if let Err(e) = sim_core::dc_op(&base.at_temperature(t)) {
            out.push((format!("{t} C"), format!("{e:?}")));
        }
    }
    let mut rng = Rng(seed | 1);
    for k in 0..tries {
        let mut t = s.clone();
        let mut note = vec![];
        for c in t.components.iter_mut().filter(|c| c.kind == Kind::Resistor) {
            if let Some(v) = sim_core::parse_value(&c.value) {
                let f = 10f64.powf(rng.next() * 1.6 - 0.8);
                c.value = format!("{:e}", v * f);
                note.push(format!("{}x{f:.2}", c.refdes));
            }
        }
        if let Ok(c) = sim_core::from_schematic(&t)
            && let Err(e) = sim_core::dc_op(&c)
        {
            out.push((format!("try {k}: {}", note.join(" ")), format!("{e:?}")));
        }
    }
    out
}

#[test]
#[ignore]
fn converge_explore() {
    for (name, s) in samples() {
        let t = std::time::Instant::now();
        let bad = trouble(&s, 12, 12345);
        println!("{name}: {} failures ({:.1}s)", bad.len(), t.elapsed().as_secs_f64());
        for (what, e) in bad.iter().take(4) {
            println!("    {what} -> {e}");
        }
    }
}

/// Circuits whose operating point is an unstable balance (a 555 astable, a counter clocked by one): the DC
/// panel may legitimately report "no convergence" for them with other resistor values; they start from zero in
/// a transient run instead. Everything else must keep a DC operating point.
const OSCILLATORS: [&str; 2] = ["ne555_blinker.schjson", "mixed_signal.schjson"];

#[test]
fn shipped_samples_keep_their_operating_point() {
    let mut bad = vec![];
    for (name, s) in samples().into_iter().filter(|(n, _)| !OSCILLATORS.contains(&n.as_str())) {
        for (what, e) in trouble(&s, 4, 777) {
            bad.push(format!("{name}: {what} -> {e}"));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}
