//! Parts made by the part form's 「表とグラフから全部読む」 from data sheets, placed in circuits and simulated
//! (`form_parts/`, saved by `circuitCad --new-part --form-pdf FILE --form-draft --form-graph-all SECTION --form-save DIR`
//! and renamed FORM_*, so they do not meet the bundled parts of the same name). The data sheets cannot be shipped, so
//! the saved parts are: what this checks is that a part as the form saves it (its pins, a simulation-only pin, the
//! model's terminal order) works in a circuit, and gives the data sheet's numbers. Made again after a change to the
//! reading or the fits, these numbers say whether the models still match (`docs/datasheet-viewer-design.md`, 段階17).

use serde_json::Value;

/// The script's answers, run on a library of the bundled parts and the form's parts.
fn run(script: &str) -> Vec<Value> {
    // a folder per script: the tests run side by side
    static RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = RUNS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("ccad-test-form-parts-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let parts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/integration/form_parts");
    for e in std::fs::read_dir(parts).unwrap() {
        let p = e.unwrap().path();
        std::fs::copy(&p, dir.join(p.file_name().unwrap())).unwrap();
    }
    let (answers, failed) = std::thread::scope(|s| {
        s.spawn(|| {
            let mut session = cli::session::Session::new(Some(dir.clone()));
            cli::runner::run_script(&mut session, script, false)
        })
        .join()
        .unwrap()
    });
    let _ = std::fs::remove_dir_all(&dir);
    let answers: Vec<Value> = answers.into_iter().map(Value::Object).collect();
    assert!(!failed, "{answers:?}");
    answers
}

/// The `dc` answers, in order.
fn dcs(a: &[Value]) -> Vec<&Value> {
    let v: Vec<&Value> = a.iter().filter(|v| v["cmd"] == "dc").collect();
    assert!(!v.is_empty(), "{a:?}");
    v
}

fn current(dc: &Value, element: &str) -> f64 {
    dc["elements"][element]["current"].as_f64().unwrap_or_else(|| panic!("no current of {element}: {dc}")).abs()
}

fn probe(dc: &Value, name: &str) -> f64 {
    dc["probes"][name].as_f64().unwrap_or_else(|| panic!("no probe {name}: {dc}"))
}

fn near(what: &str, got: f64, want: f64, tol: f64) {
    assert!((got / want - 1.0).abs() <= tol, "{what}: {got} (want {want} ± {:.0}%)", tol * 100.0);
}

#[test]
fn a_transistor_read_from_its_data_sheet_has_its_gain() {
    // 2N3903 (onsemi 2N3903-D): base current 0.1 mA at VCE = 5 V. Figure 15 (normalized, scaled by the table's hFE
    // 50..150 at 10 mA) gives about 87 at 8.6 mA; VBE(on) from Figure 17 about 0.72 V
    let a = run("new\nadd FORM_2N3903 as=Q1\nadd I 100u as=IB\nadd V 5 as=VC\n\
                 net B IB.+ Q1.B\nnet C VC.+ Q1.C\nnet GND IB.- VC.- Q1.E\nerc\ndc probe=V(b),I(VC)\n");
    let dc = dcs(&a)[0];
    near("hFE", probe(dc, "I(VC)").abs() / 100e-6, 87.0, 0.15);
    near("VBE", probe(dc, "V(b)"), 0.72, 0.06);
}

#[test]
fn a_darlington_read_from_its_data_sheet_has_its_gain() {
    // TIP120 (onsemi TIP120-D): R1 8 kΩ and R2 120 Ω read off the circuit drawing; base current 1 mA at VCE = 4 V.
    // Figure 9 gives an hFE of about 3700 at 3 A (the model 3100); Figure 11 a VBE(on) of about 1.9 V at 3.5 A.
    // The fit had made RB 6.6e-97 Ω and the circuit did not converge
    let a = run("new\nadd FORM_TIP120 as=Q1\nadd I 1m as=IB\nadd V 4 as=VC\n\
                 net B IB.+ Q1.B\nnet C VC.+ Q1.C\nnet GND IB.- VC.- Q1.E\nerc\ndc probe=V(b),I(VC)\n");
    let dc = dcs(&a)[0];
    near("hFE", probe(dc, "I(VC)").abs() / 1e-3, 3400.0, 0.2);
    near("VBE", probe(dc, "V(b)"), 1.85, 0.06);
}

#[test]
fn the_capacitors_a_data_sheet_recommends_are_placed_on_their_pins() {
    // LM317L (TI): "CIN = 1µF" and "COUT = 1µF" in the conditions of its tables, saved as the part's companions on INPUT and
    // OUTPUT; `companions U1` puts each between its pin and a ground symbol
    let a = run("new\nadd FORM_LM317L as=U1\ncompanions U1\nlist\n");
    let placed = a.iter().find(|v| v["cmd"] == "companions").unwrap();
    assert_eq!(placed["placed"], serde_json::json!(["C1", "C2"]), "{placed}");
    let list = a.iter().find(|v| v["cmd"] == "list").unwrap();
    let net_of = |pin: &str| list["nets"].as_array().unwrap().iter().find(|n| n["pins"].as_array().unwrap().iter().any(|p| p == pin)).map(|n| n["net"].as_str().unwrap().to_string());
    assert_eq!(net_of("C1.1"), net_of("U1.1"), "CIN on INPUT: {list}");
    assert_eq!(net_of("C2.1"), net_of("U1.2"), "COUT on OUTPUT: {list}");
    assert_eq!((net_of("C1.2").as_deref(), net_of("C2.2").as_deref()), (Some("GND"), Some("GND")));
}

#[test]
fn a_diode_read_from_its_data_sheet_has_its_forward_voltage() {
    // 1N4148 (Vishay): the forward curve (Tj = 25 °C) about 0.72 V at 10 mA, 0.87 V at 100 mA
    let a = run("new\nadd FORM_1N4148 as=D1\nadd I 10m as=IF\nnet A IF.+ D1.A\nnet GND IF.- D1.K\nerc\ndc probe=V(a)\nvalue IF 100m\ndc probe=V(a)\n");
    let d = dcs(&a);
    near("VF at 10 mA", probe(d[0], "V(a)"), 0.72, 0.06);
    near("VF at 100 mA", probe(d[1], "V(a)"), 0.87, 0.06);
}

#[test]
fn a_photodiode_read_from_its_data_sheet_takes_light_on_its_simulation_pin() {
    // BPW34 (Vishay): the light pin IRRAD the form added (1 V = 1 mW/cm², no pad on the board); 50 µA at 1 mW/cm²
    // (table: Ira typ 50 µA), proportional to the light, dark current 2 nA at 10 V
    let a = run("new\nadd FORM_BPW34 as=D1\nadd V 5 as=VB\nadd V 1 as=VL\nadd R 10k as=RL\n\
                 net VCC VB.+ D1.K\nnet OUT D1.A RL.1\nnet GND VB.- RL.2 VL.-\nnet LIGHT VL.+ D1.IRRAD\nerc\n\
                 dc\nvalue VL 0.1\ndc\nvalue VL 0\ndc\n");
    let d = dcs(&a);
    near("photocurrent at 1 mW/cm²", current(d[0], "RL"), 50e-6, 0.05);
    near("photocurrent at 0.1 mW/cm²", current(d[1], "RL"), 5e-6, 0.05);
    assert!(current(d[2], "RL") < 5e-9, "dark: {}", current(d[2], "RL"));
    // the light pin is the model's only, and has no pad
    let add = a.iter().find(|v| v["cmd"] == "add" && v["ref"] == "D1").unwrap();
    assert!(add["pins"].as_array().unwrap().iter().any(|p| p["name"] == "IRRAD"), "{add}");
    let part: Value = serde_json::from_str(&std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/integration/form_parts/FORM_BPW34.part.json")).unwrap()).unwrap();
    assert_eq!(part["package"]["pins"], 2, "the pads are A and K");
    assert!(part["pins"].as_array().unwrap().iter().any(|p| p["name"] == "IRRAD" && p["sim_only"] == true));
}
