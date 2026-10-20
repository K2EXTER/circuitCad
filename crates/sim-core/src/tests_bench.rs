//! Timings of the analyses that repeat independent work (candidates for running on several threads). Run with
//! `cargo test -p sim-core bench_ -- --ignored --nocapture --test-threads=1` and read the `BENCH` lines.

use crate::fault::applicable;
use crate::*;
use std::time::Instant;

fn example(name: &str) -> Circuit {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let text = std::fs::read_to_string(dir.join(format!("{name}.schjson"))).unwrap();
    let sch: sch_core::Schematic = serde_json::from_str(&text).unwrap();
    from_schematic(&sch).unwrap()
}

fn timed<T: std::fmt::Debug>(label: &str, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let r = f();
    println!("BENCH {label}: {:.3} s  -> {r:?}", t.elapsed().as_secs_f64());
    r
}

#[test]
#[ignore]
fn bench_monte_carlo() {
    let c = example("series_regulator");
    let st = |runs, measure| McSettings { runs, seed: 1, tol_r: 0.05, tol_c: 0.2, tol_l: 0.2, output: "VOUT".into(), measure };
    timed("monte_carlo DC voltage, series_regulator, 1000 runs", || monte_carlo(&c, &st(1000, McMeasure::DcVoltage)).map(|r| (r.values.len(), r.failed)));
    let c = example("rc_lowpass");
    println!("BENCH rc_lowpass nodes: {:?}", c.names);
    let node = c.names.iter().find(|n| n.eq_ignore_ascii_case("out") || n.eq_ignore_ascii_case("vout")).cloned().unwrap_or_else(|| c.names[c.names.len() - 1].clone());
    let mut s = st(300, McMeasure::Cutoff { fstart: 1.0, fstop: 1e6, points_per_decade: 20 });
    s.output = node;
    timed("monte_carlo cutoff, rc_lowpass, 300 runs", || monte_carlo(&c, &s).map(|r| (r.values.len(), r.failed)));
}

#[test]
#[ignore]
fn bench_fault_scan() {
    for name in ["series_regulator", "power_supply", "regulator_78xx", "emitter_follower"] {
        let c = example(name);
        let parts = breakable(&c);
        let n = timed(&format!("fault scan DC, {name}, {} parts", parts.len()), || {
            let mut n = 0;
            for p in &parts {
                let e = c.elements.iter().find(|e| e.name() == p).unwrap();
                for f in applicable(e) {
                    let _ = simulate_fault(&c, p, f);
                    n += 1;
                }
            }
            n
        });
        println!("BENCH   ({n} faults)");
    }
}

#[test]
#[ignore]
fn bench_ac_and_noise() {
    let c = example("opamp_amp");
    let st = AcSettings { fstart: 1.0, fstop: 1e8, sweep: AcSweep::Decade(400) };
    timed("ac_analysis, opamp_amp, 3200 points", || ac_analysis(&c, &st).map(|r| r.freq.len()));
    let c = example("opamp_loop_gain");
    let st = AcSettings { fstart: 1.0, fstop: 1e8, sweep: AcSweep::Decade(400) };
    timed("ac_analysis, opamp_loop_gain, 3200 points", || ac_analysis(&c, &st).map(|r| r.freq.len()));
    let c = example("cap_impedance");
    let st = AcSettings { fstart: 10.0, fstop: 1e8, sweep: AcSweep::Decade(400) };
    timed("ac_analysis, cap_impedance, 2800 points", || ac_analysis(&c, &st).map(|r| r.freq.len()));
    let c = example("bjt_noise");
    println!("BENCH bjt_noise nodes: {:?}", c.names);
    let node = c.names.iter().find(|n| n.eq_ignore_ascii_case("out") || n.eq_ignore_ascii_case("vout")).cloned().unwrap_or_else(|| c.names[c.names.len() - 1].clone());
    let ns = NoiseSettings { sweep: AcSettings { fstart: 1.0, fstop: 1e6, sweep: AcSweep::Decade(100) }, output: node, output_ref: None, input: None, temp_c: 27.0 };
    timed("noise_analysis, bjt_noise, 600 points", || noise_analysis(&c, &ns).map(|r| r.freq.len()));
}

#[test]
#[ignore]
fn bench_sweeps() {
    let c = example("zener_regulator");
    timed("temp_sweep -40..125 C in 0.25 C steps, zener_regulator", || temp_sweep(&c, -40.0, 125.0, 0.25).map(|r| r.x.len()));
}

#[test]
#[ignore]
fn bench_transients() {
    for (name, tstop, tstep) in [("led_blinker", 5.0, 2e-3), ("ne555_blinker", 0.05, 5e-6), ("buck_converter", 2e-3, 2e-7)] {
        let c = example(name);
        timed(&format!("transient {name}, tstop {tstop}"), || transient(&c, &TranSettings { tstop, tstep, uic: false }).map(|r| r.time.len()));
    }
}

/// The fault scan of the 41-part demo: 118 faults took 361 s one after the other when every fault solved the healthy circuit again.
#[test]
#[ignore]
fn bench_fault_scan_big() {
    let c = example("mixed_signal");
    let base = timed("dc_op mixed_signal (the healthy circuit)", || dc_op(&c).map(|r| r.iterations));
    let _ = base;
    let baseline = dc_op(&c).unwrap();
    let parts = breakable(&c);
    let mut n = 0;
    let t0 = Instant::now();
    let mut times = vec![];
    for p in &parts {
        let e = c.elements.iter().find(|e| e.name() == p).unwrap();
        for f in applicable(e) {
            let t = Instant::now();
            if let Some(b) = break_part(&c, p, f) {
                let _ = simulate_broken_with(&c, &b, &baseline);
            }
            times.push(t.elapsed().as_secs_f64());
            n += 1;
        }
    }
    times.sort_by(|a, b| b.total_cmp(a));
    println!("BENCH fault scan mixed_signal with the healthy circuit solved once: {n} faults, total {:.1} s, slowest {:.2} s, median {:.2} s", t0.elapsed().as_secs_f64(), times[0], times[times.len() / 2]);
}
