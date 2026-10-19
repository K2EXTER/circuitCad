//! A big mixed-signal sample (for the README picture and as a benchmark): one 12 V input feeds a protected
//! 5 V / 3.3 V supply, a 555 clock with a divide-by-4 counter and LEDs, an over-temperature alarm
//! (NTC + comparator + transistor) and a single-supply audio amplifier with a low-pass filter.
//! `cargo test -p app write_mixed_signal_example -- --ignored`

use super::*;

/// Like `label_pin`, but leaves a tall part's side pins sideways (the shared helper goes up or down for the pins
/// far from the centre, where neighbouring wires would run into each other).
fn label_pin(s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str) {
    let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap_or_else(|| panic!("no pin {pin}"));
    let c = s.components[idx].pos;
    let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
    let end = if dx.abs() >= 3 || (dx != 0 && dx.abs() >= dy.abs()) { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
    s.wires.push(sch_core::Wire { a: pt, b: end });
    s.labels.push(sch_core::Label { pos: end, text: text.into() });
}

/// Several pins on one side of a part joined by one short bus and named once (the text of neighbouring pins,
/// two grid units apart, would run into each other).
fn tie(s: &mut sch_core::Schematic, idx: usize, pins: &[&str], text: &str) {
    let c = s.components[idx].pos;
    let pts: Vec<(i32, i32)> = pins.iter().map(|p| s.components[idx].pins().into_iter().find(|(n, _)| n == p).unwrap_or_else(|| panic!("no pin {p}")).1).collect();
    let dx = if pts[0].0 < c.0 { -3 } else { 3 };
    for p in &pts {
        s.wires.push(sch_core::Wire { a: *p, b: (p.0 + dx, p.1) });
    }
    let (top, bottom) = (pts.iter().map(|p| p.1).min().unwrap(), pts.iter().map(|p| p.1).max().unwrap());
    let x = pts[0].0 + dx;
    s.wires.push(sch_core::Wire { a: (x, top), b: (x, bottom) });
    s.labels.push(sch_core::Label { pos: (x, bottom), text: text.into() });
}

fn part_at(s: &mut sch_core::Schematic, name: &str, pos: (i32, i32), rot: u8) -> usize {
    let d = library_part(name);
    s.add_custom(d.symbol(), &d.default_value(), &d.footprint_name(), pos, rot)
}

fn two_pin(s: &mut sch_core::Schematic, kind: Kind, pos: (i32, i32), rot: u8, value: &str, a: &str, b: &str) {
    let c = s.add(kind, pos, rot);
    s.components[c].value = value.into();
    label_pin(s, c, "1", a);
    label_pin(s, c, "2", b);
}

fn pins(s: &mut sch_core::Schematic, idx: usize, list: &[(&str, &str)]) {
    for (pin, net) in list {
        label_pin(s, idx, pin, net);
    }
}

pub(super) fn mixed_signal() -> sch_core::Schematic {
    let mut s = sch_core::Schematic::default();
    // ---- power: 12 V -> Schottky -> bulk -> 5 V -> 3.3 V
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "12".into();
    pins(&mut s, v1, &[("+", "VIN"), ("-", "GND")]);
    let d1 = part_at(&mut s, "1N5819", (16, -8), 0);
    pins(&mut s, d1, &[("A", "VIN"), ("K", "VPROT")]);
    let c1 = part_at(&mut s, "CE_100u_16V", (30, 4), 1);
    pins(&mut s, c1, &[("1", "VPROT"), ("2", "GND")]);
    let u1 = part_at(&mut s, "LM7805", (46, -4), 0);
    pins(&mut s, u1, &[("1", "VPROT"), ("2", "GND"), ("3", "V5")]);
    two_pin(&mut s, Kind::Capacitor, (62, 4), 1, "330n", "VPROT", "GND");
    two_pin(&mut s, Kind::Capacitor, (74, 4), 1, "100n", "V5", "GND");
    let u2 = part_at(&mut s, "AMS1117-3.3", (90, -4), 0);
    pins(&mut s, u2, &[("3", "V5"), ("1", "GND"), ("2", "V3V3")]);
    two_pin(&mut s, Kind::Capacitor, (106, 4), 1, "10u", "V3V3", "GND");

    // ---- 555 clock (about 700 Hz) and a divide-by-4 counter with two LEDs
    let u3 = part_at(&mut s, "NE555", (20, 40), 0);
    pins(&mut s, u3, &[("8", "V5"), ("1", "GND"), ("4", "V5"), ("7", "DIS"), ("5", "CT"), ("3", "OSC")]);
    tie(&mut s, u3, &["2", "6"], "TH");
    two_pin(&mut s, Kind::Resistor, (38, 34), 1, "1k", "V5", "DIS");
    two_pin(&mut s, Kind::Resistor, (50, 40), 1, "10k", "DIS", "TH");
    two_pin(&mut s, Kind::Capacitor, (62, 46), 1, "100n", "TH", "GND");
    two_pin(&mut s, Kind::Capacitor, (74, 46), 1, "10n", "CT", "GND");
    // the 555's output only reaches VCC - 1.7 V; a CMOS inverter squares it up to the full 5 V for the counter
    let u7 = part_at(&mut s, "74HC04", (80, 20), 0);
    pins(&mut s, u7, &[("14", "V5"), ("7", "GND"), ("1", "OSC"), ("2", "CLK")]);
    tie(&mut s, u7, &["3", "5", "9", "11", "13"], "GND");
    let u4 = part_at(&mut s, "74HC74", (100, 44), 0);
    pins(&mut s, u4, &[("14", "V5"), ("7", "GND"), ("1", "RST"), ("4", "V5"), ("10", "V5"), ("13", "RST"),
        ("3", "CLK"), ("2", "QB1"), ("5", "Q1"), ("6", "QB1"), ("11", "Q1"), ("12", "QB2"), ("9", "Q2"), ("8", "QB2")]);
    // power-on reset: CLR is held low for about a millisecond so that both flip-flops start at 0
    two_pin(&mut s, Kind::Resistor, (86, 66), 1, "10k", "V5", "RST");
    two_pin(&mut s, Kind::Capacitor, (98, 66), 1, "100n", "RST", "GND");
    for (i, net) in ["Q1", "Q2"].into_iter().enumerate() {
        let x = 128 + 18 * i as i32;
        two_pin(&mut s, Kind::Resistor, (x, 40), 1, "330", net, &format!("LA{}", i + 1));
        let d = s.add(Kind::Led, (x, 54), 1);
        pins(&mut s, d, &[("A", &format!("LA{}", i + 1)), ("K", "GND")]);
    }

    // ---- over-temperature alarm: NTC divider vs. a reference, comparator, transistor, LED
    two_pin(&mut s, Kind::Resistor, (14, 84), 1, "10k", "V3V3", "TEMP");
    let rt = part_at(&mut s, "NTC_10k_B3950", (14, 100), 1);
    pins(&mut s, rt, &[("1", "TEMP"), ("2", "GND")]);
    two_pin(&mut s, Kind::Resistor, (28, 84), 1, "10k", "V3V3", "TH_REF");
    two_pin(&mut s, Kind::Resistor, (28, 100), 1, "4.7k", "TH_REF", "GND");
    let u6 = part_at(&mut s, "LM393", (50, 92), 0);
    pins(&mut s, u6, &[("3", "TH_REF"), ("2", "TEMP"), ("8", "V5"), ("4", "GND"), ("1", "ALARM")]);
    two_pin(&mut s, Kind::Resistor, (68, 84), 1, "10k", "V5", "ALARM");
    two_pin(&mut s, Kind::Resistor, (80, 92), 0, "10k", "ALARM", "QB");
    let q1 = part_at(&mut s, "2N3904", (96, 96), 0);
    pins(&mut s, q1, &[("2", "QB"), ("1", "GND"), ("3", "QC")]);
    two_pin(&mut s, Kind::Resistor, (116, 84), 1, "330", "V5", "LAL");
    let d4 = s.add(Kind::Led, (116, 98), 1);
    pins(&mut s, d4, &[("A", "LAL"), ("K", "QC")]);

    // ---- audio: 1 kHz source, AC coupling, single-supply inverting amplifier (x10), RC low-pass
    let v2 = s.add(Kind::VSource, (0, 130), 0);
    s.components[v2].value = "SIN(0 0.1 1k) AC 1".into();
    pins(&mut s, v2, &[("+", "AIN"), ("-", "GND")]);
    two_pin(&mut s, Kind::Capacitor, (16, 124), 0, "1u", "AIN", "AC");
    two_pin(&mut s, Kind::Resistor, (32, 124), 0, "1k", "AC", "INV");
    two_pin(&mut s, Kind::Resistor, (50, 116), 0, "10k", "INV", "AOUT");
    two_pin(&mut s, Kind::Resistor, (64, 134), 1, "10k", "V5", "VMID");
    two_pin(&mut s, Kind::Resistor, (76, 134), 1, "10k", "VMID", "GND");
    two_pin(&mut s, Kind::Capacitor, (88, 134), 1, "10u", "VMID", "GND");
    let u5 = part_at(&mut s, "LM358", (110, 126), 0);
    pins(&mut s, u5, &[("3", "VMID"), ("2", "INV"), ("1", "AOUT"), ("8", "V5"), ("4", "GND")]);
    two_pin(&mut s, Kind::Resistor, (130, 120), 0, "1.6k", "AOUT", "FILT");
    two_pin(&mut s, Kind::Capacitor, (146, 132), 1, "100n", "FILT", "GND");
    s
}

#[test]
#[ignore]
fn write_mixed_signal_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    std::fs::write(root.join("mixed_signal.schjson"), serde_json::to_string_pretty(&mixed_signal()).unwrap()).unwrap();
}

/// Every block of the sample works: the supplies, the alarm, the clock and counter, the amplifier.
#[test]
fn mixed_signal_sample_works() {
    let s = mixed_signal();
    let errors: Vec<_> = sch_core::erc(&s).into_iter().filter(|p| p.severity == sch_core::Severity::Error).map(|p| p.message).collect();
    assert!(errors.is_empty(), "{errors:?}");
    let c = sim_core::from_schematic(&s).unwrap_or_else(|e| panic!("{e:?}"));
    let op = sim_core::dc_op(&c).unwrap_or_else(|e| panic!("dc: {e:?}"));
    let v = |n: &str| op.voltage(n).unwrap_or_else(|| panic!("no net {n}"));
    assert!((v("V5") - 5.0).abs() < 0.1 && (v("V3V3") - 3.3).abs() < 0.1);
    assert!((v("VPROT") - 11.7).abs() < 0.3, "12 V minus the Schottky drop");
    assert!(v("TEMP") > v("TH_REF") && v("ALARM") < 0.5, "cold: the alarm stays off");
    assert!((v("VMID") - 2.5).abs() < 0.05);
    // the same circuit at the temperature extremes (the NTC and every junction move)
    for temp in [-40.0, 100.0] {
        sim_core::dc_op(&c.at_temperature(temp)).unwrap_or_else(|e| panic!("dc at {temp}: {e:?}"));
    }
    let t = sim_core::transient(&c, &sim_core::TranSettings { tstop: 8e-3, tstep: 10e-6, uic: false }).unwrap();
    let swing = |n: &str| {
        let sig = t.signal(&format!("V({n})")).unwrap_or_else(|| panic!("no signal {n}"));
        sig.values.iter().cloned().fold(f64::MIN, f64::max) - sig.values.iter().cloned().fold(f64::MAX, f64::min)
    };
    assert!(swing("CLK") > 4.0 && swing("Q1") > 3.0 && swing("Q2") > 3.0, "clock and both counter stages move: {} {} {}", swing("CLK"), swing("Q1"), swing("Q2"));
    assert!(swing("AOUT") > 1.2, "gain of 10 on 0.2 V peak-to-peak");
}

/// Whole-window frame time (menus, panels, canvas, Japanese font) with the sample on the board:
/// `cargo test -p app app_frame_time -- --ignored --nocapture`
#[test]
#[ignore]
fn app_frame_time() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.sch.load_path(root.join("mixed_signal.schjson"));
    app.board_ed.import_from_schematic(&app.sch);
    app.mode = Mode::Board;
    let ctx = egui::Context::default();
    crate::startup::install_japanese_font(&ctx);
    let mut frame = eframe::Frame::_new_kittest();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
    let mut run = |app: &mut App, events: Vec<egui::Event>| -> f64 {
        let t = std::time::Instant::now();
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| {
            eframe::App::ui(app, ui, &mut frame);
        });
        out.textures_delta.clear();
        let _ = ctx.tessellate(out.shapes, out.pixels_per_point);
        t.elapsed().as_secs_f64() * 1000.0
    };
    for _ in 0..3 {
        run(&mut app, vec![]);
    }
    let times: Vec<f64> = (0..40).map(|i| run(&mut app, vec![egui::Event::PointerMoved(egui::pos2(600.0 + i as f32 * 5.0, 400.0 + i as f32 * 3.0))])).collect();
    println!("APP hover: avg {:.1} ms, worst {:.1} ms", times.iter().sum::<f64>() / 40.0, times.iter().cloned().fold(0.0, f64::max));
    app.mode = Mode::Schematic;
    for _ in 0..3 {
        run(&mut app, vec![]);
    }
    let times: Vec<f64> = (0..40).map(|i| run(&mut app, vec![egui::Event::PointerMoved(egui::pos2(600.0 + i as f32 * 5.0, 400.0 + i as f32 * 3.0))])).collect();
    println!("APP schematic hover: avg {:.1} ms, worst {:.1} ms", times.iter().sum::<f64>() / 40.0, times.iter().cloned().fold(0.0, f64::max));
}

/// How long the transient run of the big sample takes (the step control used to redo two steps in three):
/// `cargo test -p app tran_speed -- --ignored --nocapture`
#[test]
#[ignore]
fn tran_speed() {
    let c = sim_core::from_schematic(&mixed_signal()).unwrap();
    let t = std::time::Instant::now();
    let r = sim_core::transient(&c, &sim_core::TranSettings { tstop: 8e-3, tstep: 10e-6, uic: false }).unwrap();
    println!("TRAN {:.2} s, {} points, {} rejected steps", t.elapsed().as_secs_f64(), r.time.len(), r.rejected);
    assert!(r.rejected < r.time.len() / 2, "the step control keeps retrying: {} rejected of {}", r.rejected, r.time.len());
}

/// The hand-routed board of the sample (docs/examples/mixed_signal.ccproj) opens with its schematic and passes the DRC.
#[test]
fn mixed_signal_project_opens_with_its_routed_board() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.load_project(root.join("mixed_signal.ccproj"));
    assert_eq!(app.sch.sch.components.len(), mixed_signal().components.len(), "the project carries the same schematic as the sample");
    let b = &app.board_ed.board;
    assert_eq!(b.parts.len(), 41);
    assert!(b.tracks.len() > 200 && !b.zones.is_empty());
    let v = pcb_core::drc::check(b);
    assert!(v.is_empty(), "{:?}", v.iter().map(|x| &x.message).collect::<Vec<_>>());
}

/// Speed of the combined demo's transient (8 ms, 10 us steps): `cargo test -p app --release mixed_signal_speed -- --ignored --nocapture`.
#[test]
#[ignore]
fn mixed_signal_speed() {
    let text = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/mixed_signal.schjson")).unwrap();
    let s: sch_core::Schematic = serde_json::from_str(&text).unwrap();
    let c = sim_core::from_schematic(&s).unwrap();
    let t0 = std::time::Instant::now();
    let r = sim_core::transient(&c, &sim_core::TranSettings { tstop: 8e-3, tstep: 10e-6, uic: false });
    let secs = t0.elapsed().as_secs_f64();
    match r {
        Ok(r) => println!("MIXED transient: {secs:.3} s, {} points, {} retries, {} rejected", r.time.len(), r.retries, r.rejected),
        Err(e) => println!("MIXED transient failed after {secs:.3} s: {e:?}"),
    }
}

/// The 555 oscillator of the combined demo against ngspice and against theory (T = 0.693 (R1 + 2 R2) C).
/// `cargo test -p app --release mixed_signal_oscillator -- --ignored --nocapture`
#[test]
#[ignore]
fn mixed_signal_oscillator() {
    let text = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/mixed_signal.schjson")).unwrap();
    let s: sch_core::Schematic = serde_json::from_str(&text).unwrap();
    let c = sim_core::from_schematic(&s).unwrap();
    let t0 = std::time::Instant::now();
    let r = sim_core::transient(&c, &sim_core::TranSettings { tstop: 8e-3, tstep: 10e-6, uic: false }).unwrap();
    let ours_secs = t0.elapsed().as_secs_f64();
    let osc = r.signal("V(OSC)").unwrap();
    let hi = osc.values.iter().cloned().fold(f64::MIN, f64::max);
    let lo = osc.values.iter().cloned().fold(f64::MAX, f64::min);
    let mid = (hi + lo) / 2.0;
    let cross = |t: &[f64], v: &[f64]| -> Vec<f64> {
        let mut up = vec![];
        for i in 1..v.len() {
            if v[i - 1] < mid && v[i] >= mid {
                up.push(t[i - 1] + (mid - v[i - 1]) / (v[i] - v[i - 1]) * (t[i] - t[i - 1]));
            }
        }
        up
    };
    let periods = |u: &[f64]| -> Vec<f64> { u.windows(2).map(|w| w[1] - w[0]).collect() };
    let up = cross(&r.time, &osc.values);
    let p = periods(&up);
    println!("OSC ours: swing {lo:.3}..{hi:.3}, {} rising edges, mean period {:.4} ms (first {:.4}, last {:.4}), {ours_secs:.2} s", up.len(), p.iter().sum::<f64>() / p.len().max(1) as f64 * 1e3, p.first().copied().unwrap_or(0.0) * 1e3, p.last().copied().unwrap_or(0.0) * 1e3);
    // ngspice
    let body = sch_core::to_spice_for(&s, "mixed", sch_core::Dialect::Ngspice);
    let body = body.strip_suffix(".end\n").unwrap_or(&body).to_string();
    let work = std::env::temp_dir().join("circuitcad_mixed_osc");
    let _ = std::fs::create_dir_all(&work);
    let data = work.join("osc.dat");
    let deck = format!("{body}.options reltol=1e-4\n.control\nset noaskquit\nset wr_vecnames\ntran 10u 8m\nwrdata {} v(osc)\n.endc\n.end\n", data.display().to_string().replace('\\', "/"));
    let cir = work.join("mixed.cir");
    std::fs::write(&cir, deck).unwrap();
    let exe = r"F:\Tools\ngspice\Spice64\bin\ngspice_con.exe";
    let t1 = std::time::Instant::now();
    let _ = std::process::Command::new(exe).arg("-b").arg(&cir).output().unwrap();
    let ng_secs = t1.elapsed().as_secs_f64();
    let Some((t, cols)) = crate::xcheck_tran::parse_wrdata(&std::fs::read_to_string(&data).unwrap_or_default()) else {
        println!("ngspice gave no data");
        return;
    };
    let v = &cols[0].1;
    println!("OSC ngspice: swing {:.3}..{:.3}", v.iter().cloned().fold(f64::MAX, f64::min), v.iter().cloned().fold(f64::MIN, f64::max));
    let up = cross(&t, v);
    let p = periods(&up);
    println!("OSC ngspice: {} rising edges, mean period {:.4} ms (first {:.4}, last {:.4}), {ng_secs:.2} s, {} points", up.len(), p.iter().sum::<f64>() / p.len().max(1) as f64 * 1e3, p.first().copied().unwrap_or(0.0) * 1e3, p.last().copied().unwrap_or(0.0) * 1e3, t.len());
}
