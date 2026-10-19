//! Runs the real `ccad` binary on small scripts and reads the JSON answers.

use serde_json::Value;
use std::process::{Command, Stdio};

fn run(script: &str) -> (Vec<Value>, bool) {
    let lib = std::env::temp_dir().join(format!("ccad-test-lib-{}", std::process::id()));
    let mut child = Command::new(env!("CARGO_BIN_EXE_ccad"))
        .args(["--lib", lib.to_str().unwrap(), "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child.stdin.take().unwrap().write_all(script.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    let answers = String::from_utf8(out.stdout).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    (answers, out.status.success())
}

fn last<'a>(a: &'a [Value], cmd: &str) -> &'a Value {
    a.iter().rev().find(|v| v["cmd"] == cmd).unwrap_or_else(|| panic!("no answer for {cmd}"))
}

#[test]
fn divider_gives_the_textbook_voltage() {
    let (a, ok) = run("new\nadd V 12 as=V1\nadd R 10k as=R1\nadd R 4.7k as=R2\nnet VIN V1.+ R1.1\nnet MID R1.2 R2.1\nnet GND V1.- R2.2\nerc\ndc\n");
    assert!(ok, "{a:?}");
    assert_eq!(last(&a, "erc")["errors"], 0);
    let mid = last(&a, "dc")["voltages"]["MID"].as_f64().unwrap();
    assert!((mid - 12.0 * 4.7 / 14.7).abs() < 1e-6, "MID = {mid}");
}

#[test]
fn a_pin_on_two_nets_is_refused_and_stops_the_run() {
    let (a, ok) = run("new\nadd R 1k as=R1\nadd R 1k as=R2\nnet A R1.1 R2.1\nnet B R1.1\nadd R 1k\n");
    assert!(!ok);
    let bad = a.last().unwrap();
    assert_eq!(bad["ok"], false);
    assert!(bad["error"].as_str().unwrap().contains("already on net A"), "{bad}");
    // the run stopped there: the part after the failing line was never added
    assert!(a.iter().all(|v| v["cmd"] != "add" || v["line"].as_i64().unwrap() <= 3));
}

#[test]
fn mistakes_name_the_fix() {
    let (a, _) = run("--x\n");
    assert!(a[0]["error"].as_str().unwrap().contains("unknown command"));
    let (a, _) = run("new\nadd R abc\n");
    assert!(a[1]["error"].as_str().unwrap().contains("not a number"));
    let (a, _) = run("new\nadd NE5555\n");
    assert!(a[1]["error"].as_str().unwrap().contains("parts WORD"));
}

#[test]
fn astable_555_runs_near_the_textbook_period_and_goes_to_fab_files() {
    let dir = std::env::temp_dir().join(format!("ccad-test-fab-{}", std::process::id()));
    let script = format!(
        "new\nadd V 5 as=V1\nadd NE555 as=U1\nadd R 1k as=R1\nadd R 68k as=R2\nadd C 10n as=C1\nadd C 100n as=C2\nadd LED as=D1\nadd R 330 as=R3\n\
         net VCC V1.+ U1.VCC U1.RESET R1.1\nnet GND V1.- U1.GND C1.2 C2.2 D1.K\nnet DIS R1.2 R2.1 U1.DISCH\nnet TIMING R2.2 C1.1 U1.TRIG U1.THRES\n\
         net CTL U1.CONT C2.1\nnet OUT U1.OUT R3.1\nnet LEDA R3.2 D1.A\nerc\ntran 5m 5u probe=V(OUT)\nboard import\nboard autoplace\nboard route\nboard drc\nboard fab {}\n",
        dir.display().to_string().replace(std::path::MAIN_SEPARATOR, "/")
    );
    let (a, ok) = run(&script);
    assert!(ok, "{a:?}");
    assert_eq!(last(&a, "erc")["errors"], 0);
    let period = last(&a, "tran")["signals"]["V(OUT)"]["period"].as_f64().unwrap();
    let theory = 0.693 * (1e3 + 2.0 * 68e3) * 10e-9;
    assert!((period / theory - 1.0).abs() < 0.05, "period {period} vs {theory}");
    let routes: Vec<&Value> = a.iter().filter(|v| v["cmd"] == "board" && v.get("routed").is_some()).collect();
    assert_eq!(routes[0]["unrouted_left"], 0);
    let drc = a.iter().find(|v| v.get("violations").is_some()).unwrap();
    assert_eq!(drc["violations"], 0);
    assert!(dir.join("board_gerber.zip").exists() && dir.join("board.GTL").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

fn run_strict(script: &str) -> (Vec<Value>, bool) {
    let lib = std::env::temp_dir().join(format!("ccad-test-lib-{}", std::process::id()));
    let mut child = Command::new(env!("CARGO_BIN_EXE_ccad"))
        .args(["--strict", "--lib", lib.to_str().unwrap(), "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child.stdin.take().unwrap().write_all(script.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    let answers = String::from_utf8(out.stdout).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    (answers, out.status.success())
}

const NO_GROUND: &str = "new\nadd V 5 as=V1\nadd R 1k as=R1\nnet VIN V1.+ R1.1\nnet X V1.- R1.2\n";

#[test]
fn erc_errors_are_visible_in_the_answer_and_dc_warns() {
    let (a, ok) = run(&format!("{NO_GROUND}erc\ndc\n"));
    assert!(ok || a.last().unwrap()["cmd"] == "dc", "{a:?}");
    let erc = last(&a, "erc");
    assert_eq!(erc["ok"], true, "ok only says the command ran");
    assert_eq!(erc["circuit_ok"], false);
    assert!(erc["errors"].as_i64().unwrap() >= 1);
    assert!(erc["tips"].to_string().contains("named GND"), "{erc}");
    let dc = last(&a, "dc");
    if dc["ok"] == true {
        assert!(dc["warnings"].to_string().contains("ERC reports"), "{dc}");
    }
}

#[test]
fn strict_stops_on_an_erc_error() {
    let (a, ok) = run_strict(&format!("{NO_GROUND}erc\ndc\n"));
    assert!(!ok);
    let bad = a.last().unwrap();
    assert_eq!(bad["cmd"], "erc");
    assert_eq!(bad["ok"], false);
    assert!(bad["error"].as_str().unwrap().contains("ERC reports"), "{bad}");
    // a clean circuit still passes
    let (a, ok) = run_strict("new\nadd V 5 as=V1\nadd R 1k as=R1\nnet VIN V1.+ R1.1\nnet GND V1.- R1.2\nerc\ndc\n");
    assert!(ok, "{a:?}");
}

#[test]
fn macro_model_internals_are_hidden_from_dc_unless_asked() {
    let script = "new\nadd V 9 as=V1\nadd LM7805 as=U1\nadd R 1k as=R1\nnet IN V1.+ U1.IN\nnet GND V1.- U1.GND R1.2\nnet OUT U1.OUT R1.1\ndc\ndc full\n";
    let (a, ok) = run(script);
    assert!(ok, "{a:?}");
    let dcs: Vec<&Value> = a.iter().filter(|v| v["cmd"] == "dc").collect();
    let names = |v: &Value| v["elements"].as_object().unwrap().keys().cloned().collect::<Vec<_>>();
    assert!(names(dcs[0]).iter().all(|n| !n.contains('.')), "{:?}", names(dcs[0]));
    assert!(dcs[0]["internal_elements_hidden"].as_i64().unwrap() > 0);
    assert!(names(dcs[1]).iter().any(|n| n.contains('.')));
}

#[test]
fn basic_types_have_pins_in_part() {
    let (a, ok) = run("part LED\npart R\n");
    assert!(ok, "{a:?}");
    assert_eq!(a[0]["pins"][0]["pin"], "A");
    assert_eq!(a[1]["pins"].as_array().unwrap().len(), 2);
}

#[test]
fn internal_nodes_are_hidden_and_circuit_ok_is_in_every_analysis_answer() {
    let script = "new\nadd V 9 as=V1\nadd LM7805 as=U1\nadd R 1k as=R1\nnet IN V1.+ U1.IN\nnet GND V1.- U1.GND R1.2\nnet OUT U1.OUT R1.1\ndc\ndc full\n";
    let (a, ok) = run(script);
    assert!(ok, "{a:?}");
    let dcs: Vec<&Value> = a.iter().filter(|v| v["cmd"] == "dc").collect();
    assert!(dcs[0]["voltages"].as_object().unwrap().keys().all(|n| !n.contains('.')));
    assert!(dcs[0]["internal_nodes_hidden"].as_i64().unwrap() > 0);
    assert!(dcs[1]["voltages"].as_object().unwrap().keys().any(|n| n.contains('.')));
    assert_eq!(dcs[0]["circuit_ok"], true);
}

#[test]
fn missing_ground_says_how_to_fix_it_in_dc_and_tran_and_marks_circuit_ok_false() {
    for (cmd, line) in [("dc", "dc"), ("tran", "tran 1m 10u")] {
        // the run stops at the first failure, so each analysis gets its own run
        let (a, _) = run(&format!("{NO_GROUND}{line}\n"));
        let v = last(&a, cmd);
        let text = v.to_string();
        assert!(v["ok"] == false && text.contains("name one net GND"), "{cmd}: {text}");
    }
}

#[test]
fn rc_step_needs_uic_and_reaches_63_percent() {
    let base = "new\nadd V 1 as=V1\nadd R 1.6k as=R1\nadd C 100n as=C1\nnet IN V1.+ R1.1\nnet OUT R1.2 C1.1\nnet GND V1.- C1.2\n";
    let (a, ok) = run(&format!("{base}tran 160u 1u probe=V(OUT) samples=2 uic\ntran 160u 1u probe=V(OUT) samples=2\n"));
    assert!(ok, "{a:?}");
    let trans: Vec<&Value> = a.iter().filter(|v| v["cmd"] == "tran").collect();
    let fin = |v: &Value| v["signals"]["V(OUT)"]["final"].as_f64().unwrap();
    assert!((fin(trans[0]) - 0.63212).abs() < 1e-3, "{}", fin(trans[0]));
    assert!(fin(trans[1]) > 0.999, "without uic the capacitor starts charged");
    assert_eq!(trans[0]["circuit_ok"], true);
}

#[test]
fn calc_lists_runs_and_refuses_wrong_arguments() {
    let (a, ok) = run("calc\ncalc led 5 2 10\n");
    assert!(ok, "{a:?}");
    assert!(a[0]["calculators"].as_array().unwrap().len() >= 14);
    // 3 V across the resistor at 10 mA is 300 ohm; the E24 neighbour is 300 itself or 330
    let v = a[1]["value"].as_str().unwrap();
    assert!(v.starts_with("300") || v.starts_with("330"), "{v}");
    let (a, ok) = run("calc led 5 2\n");
    assert!(!ok && a[0]["error"].as_str().unwrap().contains("needs 3 numbers"), "{a:?}");
    let (a, _) = run("calc nope\n");
    assert!(a[0]["error"].as_str().unwrap().contains("one of:"));
}

#[test]
fn ac_and_spec_judge_an_rc_filter_against_the_designed_cutoff() {
    let build = "new\nadd V as=V1\nadd R 1.6k as=R1\nadd C 100n as=C1\nvalue V1 \"0 AC 1\"\nnet IN V1.+ R1.1\nnet OUT R1.2 C1.1\nnet GND V1.- C1.2\n";
    // 1 / (2 pi 1.6k 100n) = 995 Hz
    let (a, ok) = run(&format!("{build}ac 10 100k probe=V(out)\nspec ac_cutoff v(out) 950 1050\nspec ac_cutoff V(OUT) 2k 3k\nspec ac_gain_at V(out) -4 -2 at=1k\n"));
    assert!(ok, "{a:?}");
    let specs: Vec<&Value> = a.iter().filter(|v| v["cmd"] == "spec").collect();
    assert_eq!(specs[0]["pass"], true, "{}", specs[0]);
    assert_eq!(specs[1]["pass"], false);
    assert!(specs[1]["note"].as_str().unwrap().contains("下限"));
    assert_eq!(specs[2]["pass"], true);
    let f3 = last(&a, "ac")["signals"]["V(OUT)"]["minus_3db_hz"].as_f64().unwrap();
    assert!((f3 / 994.7 - 1.0).abs() < 0.01, "{f3}");
}

#[test]
fn spec_needs_a_fresh_analysis_of_the_current_circuit() {
    let (a, ok) = run("new\nadd V 5 as=V1\nadd R 1k as=R1\nnet A V1.+ R1.1\nnet GND V1.- R1.2\nspec dc_voltage A 4.9 5.1\ndc\nspec dc_voltage a 4.9 5.1\nvalue V1 3\nspec dc_voltage A 4.9 5.1\n");
    assert!(ok, "{a:?}");
    let specs: Vec<&Value> = a.iter().filter(|v| v["cmd"] == "spec").collect();
    assert!(specs[0]["pass"].is_null() && !specs[0]["note"].as_str().unwrap().is_empty(), "no analysis yet: {}", specs[0]);
    assert_eq!(specs[1]["pass"], true);
    assert_eq!(specs[1]["target"], "A");
    assert!(specs[2]["pass"].is_null(), "an edit clears the answer: {}", specs[2]);
}
