//! Runs the real `ccad` binary on small scripts and reads the JSON answers.

use serde_json::Value;
use std::process::{Command, Stdio};

fn run(script: &str) -> (Vec<Value>, bool) {
    // one library folder per run: the tests run in parallel, and a part written (or the bundled parts copied) by one run must not be read half-written by another
    static RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let lib = std::env::temp_dir().join(format!("ccad-test-lib-{}-{}", std::process::id(), RUNS.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
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

#[test]
fn postlayout_makes_the_copper_resistance_lower_the_current() {
    let (a, ok) = run("new
add V 5 as=V1
add R 1 as=R1
add R 1 as=R2
net VIN V1.+ R1.1
net MID R1.2 R2.1
net GND V1.- R2.2
        board import
board autoplace
board route
dc
board postlayout on
dc
board postlayout off
dc
");
    assert!(ok, "{a:?}");
    let dcs: Vec<&Value> = a.iter().filter(|v| v["cmd"] == "dc").collect();
    let amps = |v: &Value| v["elements"]["R1"]["current"].as_f64().unwrap();
    assert!((amps(dcs[0]) - 2.5).abs() < 1e-6 && dcs[0]["post_layout"] == false);
    // tens of milliohm of copper in a 2 ohm loop: about 2% less current
    assert!(dcs[1]["post_layout"] == true && amps(dcs[1]) < 2.49 && amps(dcs[1]) > 2.3, "{}", amps(dcs[1]));
    assert!((amps(dcs[2]) - 2.5).abs() < 1e-6, "off restores ideal wires");
}

#[test]
fn board_currents_take_the_simulated_current_and_widen_thin_tracks() {
    let (a, ok) = run("new
add V 5 as=V1
add R 1 as=R1
add R 1 as=R2
net VIN V1.+ R1.1
net MID R1.2 R2.1
net GND V1.- R2.2
        board import
board autoplace
board route currents=0
board currents
board currents widen
board currents
");
    assert!(ok, "{a:?}");
    let runs: Vec<&Value> = a.iter().filter(|v| v["cmd"] == "board" && v.get("list").is_some_and(|l| l.is_array() && v.get("nets").is_some())).collect();
    assert_eq!(runs.len(), 3, "{a:?}");
    let vin = |r: &Value| r["list"].as_array().unwrap().iter().find(|n| n["net"] == "VIN").unwrap().clone();
    assert!((vin(runs[0])["amps"].as_f64().unwrap() - 2.5).abs() < 1e-3);
    assert_eq!(vin(runs[0])["too_thin"], true, "2.5 A needs more than the default track");
    assert!(runs[1]["widened_tracks"].as_u64().unwrap() > 0);
    assert_eq!(vin(runs[2])["too_thin"], false);
}

#[test]
fn board_currents_tran_uses_the_rms_of_the_run_and_needs_a_tran() {
    // a pulsed 10 V / 10 ohm load: 1 A peak, 0.707 A RMS
    let head = "new
add V \"PULSE(0 10 0 1n 1n 1m 2m)\" as=V1
add R 10 as=R1
net VIN V1.+ R1.1
net GND V1.- R1.2
board import
board autoplace
board route
";
    let (a, _) = run(&format!("{head}board currents tran
"));
    assert!(last(&a, "board")["error"].as_str().unwrap().contains("run 'tran' first"), "{a:?}");
    let (a, ok) = run(&format!("{head}tran 40m 20u probe=V(VIN)
board currents tran
"));
    assert!(ok, "{a:?}");
    let r = last(&a, "board");
    let vin = r["stats"].as_array().unwrap().iter().find(|s| s["net"] == "VIN").unwrap();
    assert!((vin["rms"].as_f64().unwrap() - 0.707).abs() < 0.03, "{vin}");
    assert!((vin["peak"].as_f64().unwrap() - 1.0).abs() < 0.01 && vin["spiky"] == false, "{vin}");
    let declared = r["list"].as_array().unwrap().iter().find(|n| n["net"] == "VIN").unwrap()["amps"].as_f64().unwrap();
    assert!((declared - vin["rms"].as_f64().unwrap()).abs() < 1e-3, "the RMS is what gets declared");
}

#[test]
fn postlayout_adds_the_trace_inductance_to_ac() {
    let head = "new
add V 5 as=V1
add R 2.2 as=R1
net VIN V1.+ R1.1
net GND V1.- R1.2
value V1 \"0 AC 1\"
board import
board autoplace
board route
";
    let (a, ok) = run(&format!("{head}board postlayout on
ac 100k 100Meg ppd=4 probe=V(R1.1.wire)
"));
    assert!(ok, "{a:?}");
    let pl = a.iter().find(|v| v["cmd"] == "board" && v.get("postlayout").is_some()).unwrap();
    assert!(pl["largest"][0]["nanohenry"].as_f64().unwrap() > 3.0, "{pl}");
    assert!(pl["couplings"].as_u64().unwrap() >= 1 && pl["strongest_couplings"][0]["k"].as_f64().unwrap() > 0.0, "supply and return run side by side: {pl}");
    let per = &last(&a, "ac")["signals"]["V(R1.1.wire)"]["per_decade"];
    let (lo, hi) = (per[0]["db"].as_f64().unwrap(), per[3]["db"].as_f64().unwrap());
    assert!(hi < lo - 2.0, "the load pin loses gain at 100 MHz behind the trace: {lo} dB -> {hi} dB");
}

#[test]
fn ac_with_postlayout_warns_above_1_mhz_about_the_skin_effect() {
    let head = "new
add V 5 as=V1
add R 2.2 as=R1
net VIN V1.+ R1.1
net GND V1.- R1.2
value V1 \"0 AC 1\"
board import
board autoplace
board route
board postlayout on
";
    let warned = |f1: &str| {
        let (a, ok) = run(&format!("{head}ac 1k {f1} ppd=2 probe=V(R1.1.wire)
"));
        assert!(ok, "{a:?}");
        last(&a, "ac")["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("skin effect"))
    };
    assert!(warned("100Meg"));
    assert!(!warned("100k"));
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

#[test]
fn calc_refuses_an_si_suffix_on_an_input_that_is_already_in_milli_or_kilo_units() {
    let (a, ok) = run("calc led 9 2 10m\n");
    assert!(!ok && a[0]["error"].as_str().unwrap().contains("plain number"), "{a:?}");
    let (_, ok) = run("calc rc 1k 100n\ncalc led 9 2 10\n");
    assert!(ok);
}

#[test]
fn board_check_says_ready_after_routing_and_notices_a_schematic_change() {
    let base = "new\nadd V 5 as=V1\nadd R 1k as=R1\nnet VCC V1.+ R1.1\nnet GND V1.- R1.2\nboard import\nboard autoplace\nboard route\n";
    let (a, ok) = run(&format!("{base}board check fab=jlcpcb\n"));
    assert!(ok, "{a:?}");
    let c = last(&a, "board");
    assert_eq!(c["ready"], true, "{c}");
    assert_eq!(c["fab"], "jlcpcb");
    assert_eq!(c["errors"], 0);
    // a part added to the schematic after the import: the board is another circuit, so it is not ready
    let (a, _) = run(&format!("{base}add R 2k as=R2\nnet X R2.1 R2.2\nboard check\n"));
    let c = last(&a, "board");
    assert_eq!(c["ready"], false, "{c}");
    assert!(c["list"].as_array().unwrap().iter().any(|x| x["level"] == "error" && x["title"].as_str().unwrap().contains("回路図と基板")), "{c}");
    // an unknown fab is refused with the choices
    let (a, ok) = run(&format!("{base}board check fab=nowhere\n"));
    assert!(!ok);
    assert!(a.last().unwrap()["error"].as_str().unwrap().contains("jlcpcb"));
}

#[test]
fn route_joins_straight_pieces_and_keeps_to_45_degrees() {
    let script = "new
add V 5 as=V1
add R 270 as=R1
add R 270 as=R2
add LED as=D1
add LED as=D2
net VCC V1.+ R1.1 R2.1
net A R1.2 D1.A
net B R2.2 D2.A
net GND V1.- D1.K D2.K
board import
board autoplace
board route
board drc
board status
board save {}
";
    let file = std::env::temp_dir().join(format!("ccad-route-{}.pcbjson", std::process::id()));
    let (a, ok) = run(&script.replace("{}", file.to_str().unwrap()));
    assert!(ok, "{a:?}");
    let r = a.iter().find(|v| v["cmd"] == "board" && v.get("joined_tracks").is_some()).expect("route answer");
    assert_eq!(r["unrouted_left"], 0);
    assert!(r["joined_tracks"].as_u64().is_some() && r["aligned_pads"].as_u64().is_some(), "{r}");
    let st = a.iter().find(|v| v.get("drc_violations").is_some()).expect("status answer");
    assert_eq!((st["drc_violations"].as_u64(), st["unrouted"].as_u64()), (Some(0), Some(0)));
    // every piece runs at 0, 45 or 90 degrees
    let board: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let _ = std::fs::remove_file(&file);
    for t in board["tracks"].as_array().unwrap() {
        let (dx, dy) = ((t["b"]["x"].as_f64().unwrap() - t["a"]["x"].as_f64().unwrap()).abs(), (t["b"]["y"].as_f64().unwrap() - t["a"]["y"].as_f64().unwrap()).abs());
        let len = dx.hypot(dy);
        assert!(len < 1e-6 || dx < 1e-6 || dy < 1e-6 || (dx - dy).abs() < 1e-6, "odd angle: {t}");
    }
}

#[test]
fn pour_first_then_autoplace_keeps_ground_joined_and_the_board_clean() {
    let script = "new
add V 5 as=V1
add R 270 as=R1
add R 270 as=R2
add LED as=D1
add LED as=D2
net VCC V1.+ R1.1 R2.1
net A R1.2 D1.A
net B R2.2 D2.A
net GND V1.- D1.K D2.K
board import
board pour
board autoplace
board route
board drc
board status
";
    let (a, ok) = run(script);
    assert!(ok, "{a:?}");
    let pour = a.iter().find(|v| v.get("zone_added").is_some()).expect("pour answer");
    assert_eq!(pour["zone_added"], true);
    let place = a.iter().find(|v| v.get("poured_nets_ignored").is_some()).expect("autoplace answer");
    assert_eq!(place["poured_nets_ignored"][0], "GND", "{place}");
    let st = a.iter().find(|v| v.get("drc_violations").is_some()).expect("status answer");
    assert_eq!((st["drc_violations"].as_u64(), st["unrouted"].as_u64()), (Some(0), Some(0)), "{st}");
}

#[test]
fn improve_keeps_the_board_complete_and_clean() {
    let script = "new
add V 5 as=V1
add R 270 as=R1
add R 270 as=R2
add LED as=D1
add LED as=D2
net VCC V1.+ R1.1 R2.1
net A R1.2 D1.A
net B R2.2 D2.A
net GND V1.- D1.K D2.K
board import
board autoplace
board route
board improve
board drc
board status
";
    let (a, ok) = run(script);
    assert!(ok, "{a:?}");
    let imp = a.iter().find(|v| v.get("length_mm").is_some()).expect("improve answer");
    let (before, after) = (imp["length_mm"][0].as_f64().unwrap(), imp["length_mm"][1].as_f64().unwrap());
    // what improves is the cost: a longer route may be kept when it saves vias (a via counts 10 mm)
    let (v0, v1) = (imp["vias"][0].as_f64().unwrap(), imp["vias"][1].as_f64().unwrap());
    assert!(after + 10.0 * v1 <= before + 10.0 * v0 + 1e-6, "{imp}");
    assert_eq!(imp["unrouted_left"], 0);
    let st = a.iter().find(|v| v.get("drc_violations").is_some()).expect("status answer");
    assert_eq!((st["drc_violations"].as_u64(), st["unrouted"].as_u64()), (Some(0), Some(0)), "{st}");
}

#[test]
fn route_takes_the_dc_currents_and_gives_a_heavy_net_a_wide_track() {
    let script = "new
add V 5 as=V1
add R 1 as=R1
net VIN V1.+ R1.1
net GND V1.- R1.2
erc
board import
board autoplace
board route
board drc
board status
";
    let (a, ok) = run(script);
    assert!(ok, "{a:?}");
    let r = a.iter().find(|v| v.get("currents_declared").is_some()).expect("route answer");
    assert!(r["currents_declared"].as_u64().unwrap() >= 1, "{r}");
    let vin = r["wide_nets"].as_array().unwrap().iter().find(|w| w["net"] == "VIN").expect("VIN is routed wide: 5 A through 1 ohm");
    assert!(vin["width_mm"].as_f64().unwrap() >= 2.0, "{vin}");
    let st = a.iter().find(|v| v.get("drc_violations").is_some()).expect("status answer");
    assert_eq!(st["drc_violations"], 0, "{st}");
    // and with currents=0 nothing is declared
    let (b, ok) = run(&script.replace("board route", "board route currents=0"));
    assert!(ok, "{b:?}");
    let r0 = b.iter().find(|v| v.get("currents_declared").is_some()).unwrap();
    assert_eq!(r0["currents_declared"], 0);
    assert!(r0["wide_nets"].as_array().unwrap().is_empty());
}

#[test]
fn dc_warns_when_an_op_amp_output_is_saturated() {
    // 0.5 V x 11 on a 5 V supply would be 5.5 V: the LM358 output stops about 1.5 V short of the supply
    let amp = |vin: &str| {
        format!(
            "new\nadd V 5 as=V1\nadd V {vin} as=VIN\nadd LM358 as=U1\nadd R 10k as=RF\nadd R 1k as=RG\n\
             net VCC V1.+ U1.V+\nnet IN VIN.+ U1.IN+\nnet FB U1.IN- RF.1 RG.1\nnet OUT U1.OUT RF.2\nnet GND V1.- VIN.- RG.2 U1.V-\nerc\ndc\n"
        )
    };
    let (a, ok) = run(&amp("0.5"));
    assert!(ok, "{a:?}");
    let dc = last(&a, "dc");
    assert!(dc["voltages"]["OUT"].as_f64().unwrap() < 4.0, "{dc}");
    let w = dc["warnings"].as_array().unwrap();
    assert!(w.iter().any(|m| m.as_str().unwrap().starts_with("U1: IN+ is 0.500 V but IN- is")), "{dc}");
    // in the linear range there is nothing to say
    let (a, _) = run(&amp("0.2"));
    assert!(last(&a, "dc")["warnings"].as_array().unwrap().is_empty(), "{}", last(&a, "dc"));
}

#[test]
fn dc_reports_ratings_like_the_app() {
    // 12 V across 100 ohm is 1.44 W in a 0805 resistor
    let (a, ok) = run("new\nadd V 12 as=V1\nadd R 100 as=R1\nnet VCC V1.+ R1.1\nnet GND V1.- R1.2\nerc\ndc\n");
    assert!(ok, "{a:?}");
    let dc = last(&a, "dc");
    let r = dc["ratings"].as_array().unwrap();
    assert!(r.iter().any(|i| i["ref"] == "R1" && i["level"] == "over"), "{dc}");
    assert!(dc["warnings"].as_array().unwrap().iter().any(|m| m.as_str().unwrap().contains("rating issue")), "{dc}");
    // a part within its limits adds nothing
    let (a, _) = run("new\nadd V 5 as=V1\nadd R 100k as=R1\nnet VCC V1.+ R1.1\nnet GND V1.- R1.2\nerc\ndc\n");
    assert!(last(&a, "dc")["ratings"].as_array().unwrap().is_empty());
}

#[test]
fn tran_warns_about_a_saturated_op_amp_and_stays_quiet_in_the_linear_range() {
    let amp = |vin: &str| {
        format!(
            "new\nadd V 5 as=V1\nadd V {vin} as=VIN\nadd LM358 as=U1\nadd R 10k as=RF\nadd R 1k as=RG\n\
             net VCC V1.+ U1.V+\nnet IN VIN.+ U1.IN+\nnet FB U1.IN- RF.1 RG.1\nnet OUT U1.OUT RF.2\nnet GND V1.- VIN.- RG.2 U1.V-\ntran 2m 20u samples=2 probe=V(OUT)\n"
        )
    };
    let (a, ok) = run(&amp("0.5"));
    assert!(ok, "{a:?}");
    let t = last(&a, "tran");
    assert!(t["warnings"].as_array().unwrap().iter().any(|m| m.as_str().unwrap().starts_with("U1: IN+ and IN- differ")), "{t}");
    let (a, _) = run(&amp("0.2"));
    assert!(last(&a, "tran")["warnings"].as_array().unwrap().is_empty(), "{}", last(&a, "tran"));
}

#[test]
fn tran_reports_overloaded_parts_and_ripple_like_the_app() {
    // 12 V through 100 ohm into an LED: 90 mA in the LED, 0.8 W in a 0805 resistor
    let (a, ok) = run("new\nadd V 12 as=V1\nadd R 100 as=R1\nadd LED as=D1\nnet VCC V1.+ R1.1\nnet A R1.2 D1.A\nnet GND V1.- D1.K\ntran 1m 10u samples=2\n");
    assert!(ok, "{a:?}");
    let r = last(&a, "tran")["ratings"].as_array().unwrap().clone();
    assert!(r.iter().any(|i| i["ref"] == "R1" && i["level"] == "over"), "{r:?}");
    assert!(r.iter().any(|i| i["ref"] == "D1" && i["level"] == "over"), "{r:?}");
    // a 100 uF electrolytic capacitor fed 4 V (peak) at 1 kHz through 10 ohm carries about 270 mA rms: past its rated ripple current
    let (a, ok) = run("new\nadd V \"SIN(5 4 1k)\" as=V1\nadd R 10 as=R1\nadd CE_100u_16V as=C1\nnet IN V1.+ R1.1\nnet CAP R1.2 C1.1\nnet GND V1.- C1.2\ntran 10m 20u samples=2\n");
    assert!(ok, "{a:?}");
    let r = last(&a, "tran")["ratings"].as_array().unwrap().clone();
    assert!(r.iter().any(|i| i["ref"] == "C1" && i["message"].as_str().unwrap().contains("リップル")), "{r:?}");
    // nothing to say about a calm circuit
    let (a, _) = run("new\nadd V 5 as=V1\nadd R 100k as=R1\nnet VCC V1.+ R1.1\nnet GND V1.- R1.2\ntran 1m 10u samples=2\n");
    assert!(last(&a, "tran")["ratings"].as_array().unwrap().is_empty());
}

#[test]
fn board_shopping_lists_parts_with_a_search_link_per_shop() {
    let (a, ok) = run("new\nadd V 5 as=V1\nadd R 10k as=R1\nadd R 10k as=R2\nnet VIN V1.+ R1.1\nnet MID R1.2 R2.1\nnet GND V1.- R2.2\nboard import\nboard shopping boards=3\n");
    assert!(ok, "{a:?}");
    let s = last(&a, "board");
    assert_eq!(s["boards"], 3);
    let r = s["lines"].as_array().unwrap().iter().find(|l| l["value"] == "10k").unwrap();
    assert_eq!(r["qty"], 6);
    // 6 chip resistors: a tenth is below the minimum of 2 spares
    assert_eq!((r["spare"].as_u64(), r["buy"].as_u64()), (Some(2), Some(8)), "{r}");
    for shop in ["akizuki", "digikey", "lcsc"] {
        assert!(r["shops"][shop]["search"].as_str().unwrap().starts_with("https://"), "{r}");
    }
}

#[test]
fn board_shopping_spare_can_be_turned_off() {
    let (a, ok) = run("new
add V 5 as=V1
add R 10k as=R1
add R 10k as=R2
net VIN V1.+ R1.1
net MID R1.2 R2.1
net GND V1.- R2.2
board import
board shopping spare=0
");
    assert!(ok, "{a:?}");
    let r = last(&a, "board")["lines"].as_array().unwrap().iter().find(|l| l["value"] == "10k").unwrap().clone();
    assert_eq!((r["spare"].as_u64(), r["buy"].as_u64()), (Some(0), Some(2)), "{r}");
}

#[test]
fn board_shopping_gives_the_part_number_of_a_library_part_that_has_one() {
    let (a, ok) = run("new\nadd V 5 as=V1\nadd LM358 as=U1\nnet VCC V1.+ U1.V+\nnet GND V1.- U1.V-\nboard import\nboard shopping\n");
    assert!(ok, "{a:?}");
    let s = last(&a, "board");
    let u = s["lines"].as_array().unwrap().iter().find(|l| l["refdes"][0] == "U1").unwrap();
    assert_eq!(u["shops"]["lcsc"]["number"], "C7950", "{u}");
    assert_eq!(u["shops"]["lcsc"]["url"], "https://www.lcsc.com/product-detail/C7950.html");
    assert!(u["shops"]["akizuki"]["search"].is_string(), "{u}");
}

/// A maker's model zip (made here: the real ones cannot be redistributed) -> `models` finds the part, `models use` puts it into the
/// library, and `add` places it like any library part: the capacitor charges through a resistor to the supply voltage.
#[test]
fn a_model_from_a_maker_zip_can_be_found_used_added_and_simulated() {
    use std::io::Write;
    let dir = std::env::temp_dir().join(format!("ccad-test-zip-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let zip_path = dir.join("grm.zip");
    let mut z = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
    z.start_file("High_dielectric_constant/1608_0603/GRM188R61A106MAAL.mod", zip::write::SimpleFileOptions::default()).unwrap();
    z.write_all(b"* Murata Manufacturing\r\n* Description :1608M(0603)/X5R(-55to85[deg])/1.00e+01[uF]/10[V]\r\n* Murata P/N :GRM188R61A106MAAL\r\n.SUBCKT GRM188R61A106MAAL Port1 Port2\r\nC01 Port1 N01 9.0e-06\r\nL02 N01 N02 1.0e-10\r\nR01 N02 Port2 5.0e-03\r\n.ENDS GRM188R61A106MAAL\r\n").unwrap();
    z.finish().unwrap();
    let zip = zip_path.to_str().unwrap().replace('\\', "/");
    let (a, ok) = run(&format!(
        "models zip={zip} size=0603 cmin=10u cmax=10u\nmodels use GRM188R61A106MAAL zip={zip}\nnew\nmodels use GRM188R61A106MAAL zip={zip}\n\
         add V 5 as=V1\nadd R 1k as=R1\nadd GRM188R61A106MAAL as=C1\nnet IN V1.+ R1.1\nnet OUT R1.2 C1.1\nnet GND V1.- C1.2\nerc\ntran 50m 100u probe=V(OUT) samples=2\n"
    ));
    assert!(ok, "{a:?}");
    let found = &a[0];
    assert_eq!((found["count"].as_u64(), found["parts"][0]["part_number"].as_str(), found["parts"][0]["maker"].as_str()), (Some(1), Some("GRM188R61A106MAAL"), Some("村田")));
    let used = last(&a, "models");
    assert_eq!((used["part"].as_str(), used["value"].as_str(), used["footprint"].as_str()), (Some("GRM188R61A106MAAL"), Some("10u"), Some("C_0603")));
    assert_eq!(last(&a, "erc")["errors"], 0);
    // 1 kohm x 9 uF = 9 ms: after 50 ms the capacitor is at the supply
    let v = last(&a, "tran")["signals"]["V(OUT)"]["final"].as_f64().unwrap();
    assert!((v - 5.0).abs() < 0.05, "V(OUT) = {v}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `board class` makes, lists, changes and removes net classes, and refuses values below the board's floor.
#[test]
fn board_class_makes_lists_and_removes_classes() {
    let (a, ok) = run("board new 40 30\nboard class power width=0.5 clearance=0.25 via=0.8/0.4 nets=VIN,GND\nboard class sig width=0.2 nets=SIG,GND\nboard class\nboard class sig remove=1\n");
    assert!(ok, "{a:?}");
    let list = a.iter().filter(|v| v["cmd"] == "board" && v.get("classes").is_some()).next().unwrap();
    let classes = list["classes"].as_array().unwrap();
    assert_eq!(classes.len(), 2);
    // GND moved to the later class that listed it
    assert_eq!(classes[0]["nets"], serde_json::json!(["VIN"]));
    assert_eq!(classes[1]["nets"], serde_json::json!(["SIG", "GND"]));
    assert_eq!(last(&a, "board")["removed"], 1);
    let (a, _) = run("board new 40 30\nboard class x width=0.01\n");
    assert!(a.iter().any(|v| v["ok"] == false && v["error"].as_str().is_some_and(|e| e.contains("thinner"))), "{a:?}");
}

/// `board impedance` gives the impedance of a width and the width of an impedance, for a track and for a pair.
#[test]
fn board_impedance_goes_both_ways() {
    let (a, ok) = run("board new 40 30\nboard impedance height=0.2 target=50\nboard impedance height=0.2 width=0.35\nboard impedance diff=90 space=0.2\nboard impedance diff=90 width=0.2\n");
    assert!(ok, "{a:?}");
    let answers: Vec<&Value> = a.iter().filter(|v| v["cmd"] == "board" && v.get("stackup").is_some()).collect();
    let w50 = answers[0]["width"].as_f64().unwrap();
    assert!((0.3..=0.45).contains(&w50), "{w50}");
    let z = answers[1]["z0"].as_f64().unwrap();
    assert!((z - 50.0).abs() < 4.0, "{z}");
    assert!(answers[2]["width"].as_f64().unwrap() > 0.1);
    assert!(answers[3]["space"].as_f64().unwrap() > 0.0);
    let (a, _) = run("board new 40 30\nboard impedance target=1.5\n");
    assert!(a.iter().any(|v| v["ok"] == false), "{a:?}");
}

#[test]
fn noise_of_an_rc_filter_is_the_kt_over_c_value_and_names_the_resistor() {
    // sqrt(kT/C) for 100 nF at 27 C is 0.203 uV, whatever the resistor is; all of it comes from R1
    let (a, ok) = run("new\nadd V 0 as=V1\nadd R 10k as=R1\nadd C 100n as=C1\nnet VIN V1.+ R1.1\nnet OUT R1.2 C1.1\nnet GND V1.- C1.2\nerc\nnoise out 1 10Meg ppd=20 top=3\n");
    assert!(ok, "{a:?}");
    let n = last(&a, "noise");
    let rms = n["out_rms_v"].as_f64().unwrap();
    assert!((rms - 2.03e-7).abs() / 2.03e-7 < 0.03, "{rms}");
    assert_eq!(n["top_contributors"][0]["name"], "R1");
    assert!(n["top_contributors"][0]["share_percent"].as_f64().unwrap() > 99.0, "{n}");
    // 12.9 nV/rtHz for 10k at low frequency
    let d = n["per_decade"][0]["out_v_per_rthz"].as_f64().unwrap();
    assert!((d - 12.9e-9).abs() / 12.9e-9 < 0.03, "{d}");
}

#[test]
fn noise_names_the_nets_it_could_have_used() {
    let (a, ok) = run("new\nadd V 0 as=V1\nadd R 10k as=R1\nnet VIN V1.+ R1.1\nnet GND V1.- R1.2\nnoise nowhere 1 1k\n");
    assert!(!ok);
    let e = a.last().unwrap()["error"].as_str().unwrap();
    assert!(e.contains("no net 'nowhere'") && e.contains("VIN"), "{e}");
}

#[test]
fn value_takes_a_source_with_spaces_without_quotes() {
    let (a, ok) = run("new\nadd V 5 as=V1\nvalue V1 PULSE(0 5 1m 1u 1u 2m 5m)\nvalue V1\n");
    assert!(!ok);
    assert_eq!(a[2]["value"], "PULSE(0 5 1m 1u 1u 2m 5m)", "{a:?}");
    assert!(a[3]["error"].as_str().unwrap().contains("value REF VALUE"), "{a:?}");
}

#[test]
fn dc_probe_returns_only_what_was_asked_for() {
    let (a, ok) = run("new\nadd V 12 as=V1\nadd R 10k as=R1\nadd R 4.7k as=R2\nnet VIN V1.+ R1.1\nnet MID R1.2 R2.1\nnet GND V1.- R2.2\ndc probe=V(mid),I(r1),V(vin,mid)\n");
    assert!(ok, "{a:?}");
    let d = last(&a, "dc");
    assert!(d.get("voltages").is_none() && d.get("elements").is_none(), "{d}");
    let mid = d["probes"]["V(mid)"].as_f64().unwrap();
    assert!((mid - 12.0 * 4.7 / 14.7).abs() < 1e-6, "{mid}");
    assert!((d["probes"]["I(r1)"].as_f64().unwrap() - 12.0 / 14.7e3).abs() < 1e-9);
    assert!((d["probes"]["V(vin,mid)"].as_f64().unwrap() - (12.0 - mid)).abs() < 1e-6);
    let (a, ok) = run("new\nadd V 12 as=V1\nadd R 10k as=R1\nnet VIN V1.+ R1.1\nnet GND V1.- R1.2\ndc probe=V(nowhere)\n");
    assert!(!ok && a.last().unwrap()["error"].as_str().unwrap().contains("no net 'nowhere'"), "{a:?}");
}

#[test]
fn sweep_gives_a_row_per_value_and_leaves_the_part_as_it_was() {
    let (a, ok) = run("new\nadd V 5 as=V1\nadd R 10k as=R1\nadd R 10k as=R2\nnet VIN V1.+ R1.1\nnet MID R1.2 R2.1\nnet GND V1.- R2.2\nsweep V1 0 10 steps=3 probe=V(mid)\nsweep R2 list=10k,30k probe=V(mid),I(r1)\ndc probe=V(mid)\n");
    assert!(ok, "{a:?}");
    let s = a.iter().filter(|v| v["cmd"] == "sweep").collect::<Vec<_>>();
    let mids: Vec<f64> = s[0]["rows"].as_array().unwrap().iter().map(|r| r["V(mid)"].as_f64().unwrap()).collect();
    assert_eq!(mids.len(), 3);
    assert!((mids[2] - 5.0).abs() < 1e-6 && mids[0].abs() < 1e-9, "{mids:?}");
    assert_eq!(s[0]["restored_value"], "5");
    let r2 = s[1]["rows"].as_array().unwrap();
    assert!((r2[1]["V(mid)"].as_f64().unwrap() - 5.0 * 30.0 / 40.0).abs() < 1e-6, "{r2:?}");
    // the part has its own value again
    assert!((last(&a, "dc")["probes"]["V(mid)"].as_f64().unwrap() - 2.5).abs() < 1e-6);
}

#[test]
fn a_maker_subcircuit_becomes_a_part_and_the_library_reads_again() {
    let dir = std::env::temp_dir().join(format!("ccad-import-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let good = dir.join("model.cir");
    std::fs::write(&good, "* a divider\n.SUBCKT IMPDIV 1 2 3\nR1 1 2 1k\nR2 2 3 3k\n.ENDS IMPDIV\n").unwrap();
    let enc = dir.join("enc.sub");
    std::fs::write(&enc, b"\r\n<Binary File>\r\n\x1a\x07\x01\x02\x03\x04\x05\x06\x07\x01").unwrap();
    let f = |p: &std::path::Path| p.to_str().unwrap().replace(std::path::MAIN_SEPARATOR, "/");
    let (a, ok) = run(&format!(
        "part import {} pins=TOP,MID,BOT map=1,2,3\nlib reload\nnew\nadd V 8 as=V1\nadd IMPDIV as=U1\nnet VIN V1.+ U1.TOP\nnet MID U1.MID\nnet GND V1.- U1.BOT\nerc\ndc probe=V(mid)\n",
        f(&good)
    ));
    assert!(ok, "{a:?}");
    let imp = last(&a, "part");
    assert_eq!(imp["name"], "IMPDIV");
    assert!(imp["todo"].as_array().unwrap().len() >= 2);
    assert!(last(&a, "lib")["parts"].as_i64().unwrap() > 1);
    // 8 V across 1k + 3k, the tap at 6 V
    let tap = a.iter().rev().find(|v| v["cmd"] == "dc").unwrap()["probes"].as_object().unwrap().values().next().unwrap().as_f64().unwrap();
    assert!((tap - 6.0).abs() < 1e-6, "{tap}");
    let (a, ok) = run(&format!("part import {}\n", f(&enc)));
    assert!(!ok);
    assert!(a.last().unwrap()["error"].as_str().unwrap().contains("encrypted"), "{a:?}");
    let (a, ok) = run(&format!("part import {} pins=A,B\n", f(&good)));
    assert!(!ok && a.last().unwrap()["error"].as_str().unwrap().contains("3 nodes"), "{a:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sheets_split_a_label_circuit_without_changing_its_answer() {
    let build = "new\nadd V 12 as=V1\nadd R 10k as=R1\nadd R 4.7k as=R2\nnet VIN V1.+ R1.1\nnet MID R1.2 R2.1\nnet GND V1.- R2.2\n";
    let (a, ok) = run(&format!("{build}dc probe=V(mid)\nsheet add Load\nsheet use 1\nsheet move R2 to=Load\nsheet\ndc probe=V(mid)\nerc\nsheet use Load\nlist\n"));
    assert!(ok, "{a:?}");
    let dcs: Vec<f64> = a.iter().filter(|v| v["cmd"] == "dc").map(|v| v["probes"]["V(mid)"].as_f64().unwrap()).collect();
    assert_eq!(dcs.len(), 2);
    assert!((dcs[0] - dcs[1]).abs() < 1e-12, "{dcs:?}");
    let mv = a.iter().find(|v| v["cmd"] == "sheet" && v.get("moved").is_some()).unwrap();
    assert_eq!(mv["moved"]["parts"], 1);
    assert_eq!(mv["moved"]["labels"], 2, "the labels on R2's two pins go with it: {mv}");
    let list = a.iter().rev().find(|v| v["cmd"] == "sheet").unwrap();
    assert_eq!(list["sheets"].as_array().unwrap().len(), 2);
    assert_eq!(a.iter().find(|v| v["cmd"] == "erc").unwrap()["errors"], 0);
    // after the move, 'list' shows only the active sheet's part
    let l = last(&a, "list");
    assert_eq!(l["parts"].as_array().unwrap().len(), 1, "{l}");
    let (a, ok) = run(&format!("{build}sheet move R2 to=2\n"));
    assert!(!ok && a.last().unwrap()["error"].as_str().unwrap().contains("one sheet"), "{a:?}");
}

#[test]
fn tidy_packs_a_sprawling_sheet_into_a_compact_one_and_keeps_the_circuit() {
    let mut s = String::from("new
add V 12 as=V1
");
    for i in 1..=30 {
        s += &format!("add R 1k as=R{i}
");
    }
    s += "net VIN V1.+ R1.1
net GND V1.- R30.2
";
    for i in 1..30 {
        s += &format!("net N{i} R{i}.2 R{}.1
", i + 1);
    }
    let dir = std::env::temp_dir();
    let (before_file, after_file) = (dir.join(format!("ccad-tidy-a-{}.schjson", std::process::id())), dir.join(format!("ccad-tidy-b-{}.schjson", std::process::id())));
    let f = |p: &std::path::Path| p.to_str().unwrap().replace(std::path::MAIN_SEPARATOR, "/");
    let extent = |p: &std::path::Path| {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        let at: Vec<(i64, i64)> = v["components"].as_array().unwrap().iter().map(|c| (c["pos"][0].as_i64().unwrap(), c["pos"][1].as_i64().unwrap())).collect();
        (at.iter().map(|p| p.0).max().unwrap() - at.iter().map(|p| p.0).min().unwrap(), at.iter().map(|p| p.1).max().unwrap() - at.iter().map(|p| p.1).min().unwrap())
    };
    let (before, ok) = run(&format!("{s}dc probe=I(v1)
save {}
", f(&before_file)));
    assert!(ok, "{before:?}");
    let (after, ok) = run(&format!("{s}sheet tidy
dc probe=I(v1)
erc
save {}
", f(&after_file)));
    assert!(ok, "{after:?}");
    let i = |a: &[Value]| last(a, "dc")["probes"]["I(v1)"].as_f64().unwrap();
    assert!((i(&before) - i(&after)).abs() < 1e-12, "the answer must not change");
    assert_eq!(last(&after, "erc")["errors"], 0);
    let ((w0, h0), (w, h)) = (extent(&before_file), extent(&after_file));
    assert!(w.max(h) < w0.max(h0) && w <= 3 * h.max(20), "the longest side must shrink, and a long strip is not what is wanted: {w0} x {h0} -> {w} x {h}");
    let _ = (std::fs::remove_file(before_file), std::fs::remove_file(after_file));
}

#[test]
fn board_thermal_puts_vias_in_a_heat_pad_once() {
    let (a, ok) = run("new
add V 5 as=V1
add IP2332 as=U1
net VIN V1.+ U1.VIN
net GND V1.- U1.GND
board new 30 30
board import
board pour
board autoplace
board thermal
board thermal
");
    assert!(ok, "{a:?}");
    let t: Vec<&Value> = a.iter().filter(|v| v["cmd"] == "board" && v.get("vias_added").is_some()).collect();
    assert_eq!(t.len(), 2);
    assert!(t[0]["pads_lacking"].as_i64().unwrap() >= 1 && t[0]["vias_added"].as_i64().unwrap() >= 1, "{}", t[0]);
    assert_eq!(t[0]["pads_still_lacking"], 0);
    assert_eq!(t[1]["vias_added"], 0, "a second call finds nothing lacking: {}", t[1]);
}

#[test]
fn a_latch_built_from_gates_has_two_answers_and_guess_picks_one() {
    let latch = "new
add V 5 as=V1
add 74HC00 as=U1
add R 10k as=R1
add R 10k as=R2
net VCC V1.+ U1.VCC R1.1 R2.1
net GND V1.- U1.GND
net Q U1.1Y U1.2A
net QB U1.2Y U1.1B
net S R1.2 U1.1A
net R R2.2 U1.2B
";
    let (a, ok) = run(&format!("{latch}dc guess=Q=5,QB=0 probe=V(Q),V(QB)
dc guess=Q=0,QB=5 probe=V(Q),V(QB)
dc guess=ZZ=1
"));
    assert!(!ok, "the unknown net stops the run");
    let dcs: Vec<&Value> = a.iter().filter(|v| v["cmd"] == "dc").collect();
    assert!(dcs[0]["probes"]["V(Q)"].as_f64().unwrap() > 4.9 && dcs[0]["probes"]["V(QB)"].as_f64().unwrap() < 0.1, "{:?}", dcs[0]);
    assert!(dcs[1]["probes"]["V(Q)"].as_f64().unwrap() < 0.1 && dcs[1]["probes"]["V(QB)"].as_f64().unwrap() > 4.9, "{:?}", dcs[1]);
    assert!(dcs[2]["error"].as_str().unwrap().contains("no net called 'ZZ'"), "{:?}", dcs[2]);
    // `states` finds the other answers from other starting points
    let (b, ok) = run(&format!("{latch}dc states probe=V(Q)
"));
    assert!(ok, "{b:?}");
    let d = last(&b, "dc");
    assert!(d["other_states"].as_array().unwrap().iter().any(|s| s["nets_that_differ"].as_u64().unwrap_or(0) > 0), "{d:?}");
    assert!(d["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("more than one DC answer")), "{d:?}");
}

#[test]
fn tran_settle_gives_the_time_to_stay_within_each_tolerance() {
    // an RC low-pass (tau = 1 ms) driven by a step at 1 ms: it stays within a tolerance after tau x ln(1 / tolerance)
    let (a, ok) = run("new
add V \"PWL(0 0 1m 0 1.001m 1 1 1)\" as=V1
add R 1k as=R1
add C 1u as=C1
net IN V1.+ R1.1
net OUT R1.2 C1.1
net GND V1.- C1.2
tran 20m 20u probe=V(OUT) settle=V(OUT) from=1m tol=1e-2,1e-3,1e-4
tran 20m 20u settle=V(NOPE)
");
    assert!(!ok, "an unknown signal stops the run");
    let s = &a.iter().find(|v| v["cmd"] == "tran").unwrap()["settle"]["V(OUT)"];
    let tau = 1e-3;
    for (tol, want) in [("1e-2", 4.605), ("1e-3", 6.908), ("1e-4", 9.210)] {
        let got = s["settled_within"][tol].as_f64().unwrap_or_else(|| panic!("{tol}: {s:?}"));
        // measured from the stimulus at 1 ms; the 1 us ramp adds about half of it
        assert!((got / tau - want).abs() < 0.08, "tolerance {tol}: {got} vs {}", want * tau);
    }
    assert!((s["final"].as_f64().unwrap() - 1.0).abs() < 1e-3 && s["overshoot_pct"].as_f64().unwrap() < 0.01, "{s:?}");
    assert!(a.last().unwrap()["error"].as_str().unwrap().contains("no signal"), "{:?}", a.last());
}

#[test]
fn sens_ranks_the_parts_by_how_much_they_move_the_probe() {
    // V(MID) = 12 x R2 / (R1 + R2): the elasticities are 1 (V1), R1 / (R1 + R2) (R2) and minus that (R1); a capacitor does nothing at DC
    let (a, ok) = run("new
add V 12 as=V1
add R 10k as=R1
add R 4.7k as=R2
add C 1u as=C1
net VIN V1.+ R1.1
net MID R1.2 R2.1 C1.1
net GND V1.- R2.2 C1.2
sens probe=V(MID)
sens probe=V(MID) kinds=C
sens probe=V(MID) parts=R9
");
    assert!(!ok, "an unknown part stops the run");
    let s = a.iter().find(|v| v["cmd"] == "sens").unwrap();
    let parts = s["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 3, "{s:?}");
    assert_eq!(parts[0]["part"], "V1");
    let el = |name: &str| parts.iter().find(|p| p["part"] == name).unwrap()["elasticity"].as_f64().unwrap();
    assert!((el("V1") - 1.0).abs() < 1e-3 && (el("R2") - 10.0 / 14.7).abs() < 0.01 && (el("R1") + 10.0 / 14.7).abs() < 0.01, "{parts:?}");
    let c = a.iter().filter(|v| v["cmd"] == "sens").nth(1).unwrap();
    assert_eq!(c["parts"][0]["per_percent"].as_f64().unwrap(), 0.0, "{c:?}");
    assert!(a.last().unwrap()["error"].as_str().unwrap().contains("no part R9"), "{:?}", a.last());
}

#[test]
fn a_simulation_only_part_simulates_but_is_left_off_the_board() {
    // a divider with a test source: the source drives the simulation, a header stands for the real connector
    let (a, ok) = run("new\nadd V 12 as=V1\nadd R 10k as=R1\nadd R 4.7k as=R2\nadd HEADER2 as=J1\nnet VIN V1.+ R1.1 J1.1\nnet MID R1.2 R2.1\nnet GND V1.- R2.2 J1.2\nsimonly V1\nerc\ndc\nboard import\nlist\n");
    assert!(ok, "{a:?}");
    // it still simulates
    let v = last(&a, "dc")["voltages"]["MID"].as_f64().unwrap();
    assert!((v - 12.0 * 4.7 / 14.7).abs() < 1e-6, "{v}");
    // it is marked in the list, and the board has the other three parts only
    let parts = last(&a, "list")["parts"].as_array().unwrap().clone();
    assert!(parts.iter().any(|p| p["ref"] == "V1" && p["sim_only"] == true), "{parts:?}");
    assert!(parts.iter().any(|p| p["ref"] == "R1" && p.get("sim_only").is_none()), "{parts:?}");
    let imp = a.iter().rev().find(|v| v["cmd"] == "board" && v.get("added").is_some()).unwrap();
    let added: Vec<String> = imp["added"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect();
    assert!(!added.contains(&"V1".to_string()) && added.contains(&"R1".to_string()) && added.contains(&"J1".to_string()), "{added:?}");
    // `simonly V1 off` puts it back
    let (b, ok) = run("new\nadd V 5 as=V1\nadd R 1k as=R1\nnet A V1.+ R1.1\nnet GND V1.- R1.2\nsimonly V1\nsimonly V1 off\nboard import\n");
    assert!(ok, "{b:?}");
    let imp = b.iter().rev().find(|v| v["cmd"] == "board" && v.get("added").is_some()).unwrap();
    assert!(imp["added"].as_array().unwrap().iter().any(|x| x == "V1"), "{imp:?}");
}

#[test]
fn tracks_and_vias_can_be_laid_looked_up_and_taken_away() {
    let script = "new
add V 5 as=V1
add R 1k as=R1
add R 2k as=R2
net VCC V1.+ R1.1
net MID R1.2 R2.1
net GND V1.- R2.2
board import
board autoplace
";
    let (a, ok) = run(&format!("{script}board net MID
"));
    assert!(ok, "{a:?}");
    let net = last(&a, "board");
    assert_eq!(net["net"], "MID");
    assert_eq!(net["pads"].as_array().unwrap().len(), 2);
    assert_eq!(net["routed"], false, "nothing is routed yet: {net}");
    let pads = net["pads"].as_array().unwrap();
    let (p1, p2) = (pads[0]["at"].clone(), pads[1]["at"].clone());
    let pt = |p: &Value| format!("{},{}", p[0].as_f64().unwrap(), p[1].as_f64().unwrap());
    // a track from one pad to the other, claimed for MID: laid, and the net is routed
    let (a, ok) = run(&format!("{script}board track add {} {} net=MID width=0.3
board net MID
", pt(&p1), pt(&p2)));
    assert!(ok, "{a:?}");
    let added = a.iter().find(|v| v.get("laid").is_some()).unwrap();
    assert_eq!(added["net"], "MID");
    assert_eq!(last(&a, "board")["routed"], true);
    // claimed for another net: taken back, with the reason
    let (a, ok) = run(&format!("{script}board track add {} {} net=VCC
", pt(&p1), pt(&p2)));
    assert!(!ok && a.iter().any(|v| v["error"].as_str().is_some_and(|e| e.contains("not VCC"))), "{a:?}");
    // a track that touches nothing floats: refused
    let (a, ok) = run(&format!("{script}board track add 1,1 2,2
"));
    assert!(!ok && a.iter().any(|v| v["error"].as_str().is_some_and(|e| e.contains("float"))), "{a:?}");
    // taken away again
    let (a, ok) = run(&format!("{script}board track add {} {} net=MID
board track remove net=MID
board via add 5,5
board via remove 5,5
", pt(&p1), pt(&p2)));
    assert!(ok, "{a:?}");
    assert_eq!(a.iter().find(|v| v.get("removed").is_some() && v.get("vias_removed").is_some()).unwrap()["removed"], 1);
}

#[test]
fn board_picture_writes_a_png_with_a_legend() {
    let file = std::env::temp_dir().join(format!("ccad-test-picture-{}.png", std::process::id()));
    let path = file.display().to_string().replace(std::path::MAIN_SEPARATOR, "/");
    let script = format!(
        "new\nadd V 5 as=V1\nadd R 1k as=R1\nnet VCC V1.+ R1.1\nnet GND V1.- R1.2\nboard import\nboard autoplace\nboard route\nboard picture layer=top net=VCC marks=1 px=600 out={path}\n"
    );
    let (a, ok) = run(&script);
    assert!(ok, "{a:?}");
    let pic = a.iter().find(|v| v.get("legend").is_some()).unwrap();
    assert_eq!(pic["file"], path.as_str());
    assert!(pic["width"].as_u64().unwrap() > 100 && pic["legend"].as_array().unwrap().iter().any(|l| l.as_str().unwrap().contains("ネット VCC")));
    let bytes = std::fs::read(&file).unwrap();
    assert_eq!(&bytes[1..4], b"PNG");
    let _ = std::fs::remove_file(&file);
    // a layer that is not there is said
    let (a, ok) = run("new\nadd R 1k as=R1\nnet N R1.1 R1.2\nboard import\nboard picture layer=inner9\n");
    assert!(!ok, "{a:?}");
    assert!(format!("{a:?}").contains("inner9"), "{a:?}");
}

#[test]
fn status_and_quality_give_the_numbers_of_the_wiring() {
    let script = "new
add V 5 as=V1
add R 270 as=R1
add R 270 as=R2
add LED as=D1
add LED as=D2
net VCC V1.+ R1.1 R2.1
net A R1.2 D1.A
net B R2.2 D2.A
net GND V1.- D1.K D2.K
board import
board autoplace
board route
board status
board quality top=3 sort=length
";
    let (a, ok) = run(script);
    assert!(ok, "{a:?}");
    let st = a.iter().find(|v| v.get("drc_violations").is_some()).expect("status answer");
    let q = &st["quality"];
    assert!(q["length_mm"].as_f64().unwrap() > 0.0 && q["segments"].as_u64().unwrap() > 0, "{q}");
    assert!(q["by_layer_mm"].is_object() && q["bends"].is_u64() && q["side_by_side_mm"].is_f64(), "{q}");
    let qa = a.iter().find(|v| v.get("worst").is_some()).expect("quality answer");
    let worst = qa["worst"].as_array().unwrap();
    assert!(!worst.is_empty() && worst.len() <= 3, "{qa}");
    let lengths: Vec<f64> = worst.iter().map(|r| r["length_mm"].as_f64().unwrap()).collect();
    assert!(lengths.windows(2).all(|w| w[0] >= w[1]), "sorted by length, longest first: {lengths:?}");
}

#[test]
fn a_pin_marked_nc_is_not_asked_about_by_erc_and_a_wire_on_it_is() {
    let (a, ok) = run("new\nadd R 10k as=R1\nadd R 4.7k as=R2\nnet MID R1.2 R2.1\nnet GND R2.2\nerc\nnc R1.1\nerc\nnc R1.1 remove=1\nnc\nnc R1.2\nerc\n");
    assert!(ok, "{a:?}");
    let ercs: Vec<&Value> = a.iter().filter(|v| v["cmd"] == "erc").collect();
    let asks = |v: &Value| v["problems"].as_array().unwrap().iter().filter(|p| p["message"].as_str().unwrap().contains("R1 ピン1")).count();
    assert_eq!((asks(ercs[0]), asks(ercs[1])), (1, 0), "R1.1 is open: asked before the mark, not after");
    // R1.2 is on net MID: marking it is a conflict that ERC reports
    assert!(ercs[2]["problems"].as_array().unwrap().iter().any(|p| p["message"].as_str().unwrap().contains("NC")), "{}", ercs[2]);
    let nc_lists: Vec<&Value> = a.iter().filter(|v| v.get("no_connects").is_some()).collect();
    assert_eq!(nc_lists[0]["changed"], serde_json::json!(["R1.1"]));
    assert_eq!(nc_lists[1]["no_connects"], serde_json::json!([]));
}

#[test]
fn a_bypass_row_is_listed_checked_against_the_circuit_and_removed() {
    let (a, ok) = run("new
add R 10k as=R1
add R 4.7k as=R2
net MID R1.2 R2.1
        req bypass U9 caps=R1,C9 reason=電源ピンの隣に置く
        req
        req bypass U9 remove=1
        req
");
    assert!(ok, "{a:?}");
    let tables: Vec<&Value> = a.iter().filter(|v| v.get("bypass").is_some()).collect();
    assert_eq!(tables[0]["bypass"][0]["ic"], "U9");
    assert_eq!(tables[0]["bypass"][0]["caps"], serde_json::json!(["R1", "C9"]));
    assert_eq!(tables[0]["bypass"][0]["note"], "電源ピンの隣に置く");
    // U9 and C9 are not in the circuit
    assert_eq!(tables[0]["problems"].as_array().unwrap().len(), 2, "{}", tables[0]);
    assert!(tables.last().unwrap()["bypass"].as_array().unwrap().is_empty());
}

#[test]
fn the_rows_of_the_requirements_table_without_a_reason_are_listed() {
    let (a, ok) = run("new
add R 10k as=R1
add R 4.7k as=R2
net MID R1.2 R2.1
        req net 電源 nets=MID width=0.8 reason=電流の通り道なので太く
        req group 分圧 parts=R1,R2 max=100
        req
");
    assert!(ok, "{a:?}");
    let table = last(&a, "req");
    assert_eq!(table["net_rules"][0]["note"], "電流の通り道なので太く");
    assert_eq!(table["unexplained"], serde_json::json!(["近くに置く組「分圧」"]), "{table}");
}

#[test]
fn the_requirements_table_reaches_the_board_and_is_checked() {
    let (a, ok) = run("new\nadd V 12 as=V1\nadd R 10k as=R1\nadd R 4.7k as=R2\nnet VIN V1.+ R1.1\nnet MID R1.2 R2.1\nnet GND V1.- R2.2\n\
        req net 電源 nets=VIN role=power width=0.8 current=0.5 太く\n\
        req group 分圧 parts=R1,R2 max=100\n\
        req apart 離す a=R1 b=R2 min=500\n\
        req group 欠け parts=R1,R9\n\
        req\nboard import\nboard class\nboard req\n");
    assert!(ok, "{a:?}");
    let table = last(&a, "req");
    assert_eq!(table["net_rules"][0]["note"], "太く");
    assert_eq!(table["problems"].as_array().unwrap().len(), 1, "R9 is not in the circuit: {table}");
    let classes = last(&a, "board")["classes"].as_array().cloned();
    let classes = a.iter().filter_map(|v| v["classes"].as_array()).next().cloned().or(classes).unwrap();
    assert_eq!(classes[0]["role"], "power");
    assert_eq!(classes[0]["width"], 0.8);
    let rep = a.iter().rev().find(|v| v.get("findings").is_some()).unwrap();
    assert_eq!(rep["groups"], 2);
    // two resistors a few mm apart break a distance of 500 mm, and not a group of 100 mm
    assert_eq!(rep["violations"], 1, "{rep}");
    assert_eq!(rep["findings"][0]["kind"], "apart");
    assert!(rep["missing"].as_array().unwrap().iter().any(|m| m.as_str().unwrap().contains("R9")), "{rep}");
}

#[test]
fn a_wire_between_two_parts_gets_its_width_from_the_table() {
    let (a, ok) = run("new\nadd V 5 as=V1\nadd R 1k as=R1\nadd R 1k as=R2\nadd R 1k as=R3\n\
        net N V1.+ R1.1 R2.1 R3.1\nnet GND V1.- R1.2 R2.2 R3.2\n\
        req net 電源 nets=N width=0.3 links=R1-R3:0.8,R9-R1:0.5\n\
        req\nboard import\nboard req\nboard autoplace\nboard route\nboard status\n");
    assert!(ok, "{a:?}");
    let table = last(&a, "req");
    assert_eq!(table["net_rules"][0]["links"][0]["width"], 0.8);
    assert_eq!(table["problems"].as_array().unwrap().len(), 1, "R9 is not in the circuit: {table}");
    let rep = a.iter().find(|v| v.get("findings").is_some()).unwrap();
    assert_eq!(rep["links"], 1, "only the link between two parts of the board is kept: {rep}");
    let bad = last(&a, "req")["net_rules"][0]["links"][1]["a"].clone();
    assert_eq!(bad, "R9");
    let status = a.iter().rev().find(|v| v.get("unrouted").is_some()).unwrap();
    assert_eq!(status["unrouted"], 0, "{status}");
    assert_eq!(status["drc_violations"], 0, "{status}");
}

#[test]
fn the_dc_currents_fill_the_empty_currents_of_the_table() {
    let (a, ok) = run("new\nadd V 12 as=V1\nadd R 10 as=R1\nadd R 4.7 as=R2\nnet VIN V1.+ R1.1\nnet MID R1.2 R2.1\nnet GND V1.- R2.2\n\
        req net 電源 nets=VIN width=0.5\n\
        req net 済み nets=GND current=3\n\
        req suggest\nreq suggest apply=1\nreq\nreq suggest\n");
    assert!(ok, "{a:?}");
    let sugg: Vec<&Value> = a.iter().filter(|v| v.get("suggestions").is_some()).collect();
    let first = sugg[0]["suggestions"].as_array().unwrap();
    // the current of the 電源 row (0.816 A -> 0.82), and a row for MID (the GND row states its own current)
    assert!(first.iter().any(|s| s["row"] == 0 && (s["amps"].as_f64().unwrap() - 0.82).abs() < 1e-9), "{first:?}");
    assert!(first.iter().any(|s| s["net"] == "MID"), "{first:?}");
    assert!(first.iter().all(|s| s["net"] != "GND"), "a stated current is left alone: {first:?}");
    assert_eq!(sugg[1]["applied"], true);
    let table = a.iter().rev().find(|v| v.get("net_rules").is_some()).unwrap();
    assert_eq!(table["net_rules"][0]["current"], 0.82);
    assert_eq!(table["net_rules"][1]["current"], 3.0);
    assert_eq!(table["net_rules"].as_array().unwrap().len(), 3, "a row was added for MID");
    assert_eq!(sugg.last().unwrap()["count"], 0, "nothing is left to suggest after applying");
}

/// `board route net=A` routes that net only: the rest of the board stays as it is, so no pour is added to the top unless `top=1` asks for it.
#[test]
fn routing_one_net_does_not_add_a_pour_to_the_top() {
    let script = "new
add V 5 as=V1
add R 270 as=R1
add LED as=D1
net VCC V1.+ R1.1
net A R1.2 D1.A
net GND V1.- D1.K
board import
board autoplace
board pour
board route net=A
board route net=VCC top=1
";
    let (a, ok) = run(script);
    assert!(ok, "{a:?}");
    let routes: Vec<&Value> = a.iter().filter(|v| v["cmd"] == "board" && v.get("top_pour").is_some()).collect();
    assert_eq!(routes.len(), 2, "{a:?}");
    assert_eq!(routes[0]["top_pour"], false, "{}", routes[0]);
}

/// `board backannotate`: the gate trade of `board swap` goes into the schematic, and a new import has nothing left to remember.
#[test]
fn a_gate_trade_goes_back_into_the_schematic() {
    let script = "new
add 74HC32 as=U1
add R 1k as=R1
add R 1k as=R2
net IA U1.1A R1.1
net OA U1.1Y R1.2
net IB U1.2A R2.1
net OB U1.2Y R2.2
board import
board move U1 20 20
board move R1 8 40
board move R2 8 6
board swap
board status
board backannotate
board import
board status
board swap dry=1
";
    let (a, ok) = run(script);
    assert!(ok, "{a:?}");
    let swapped = a.iter().find(|v| v.get("swapped").is_some() && v["dry"] == false).expect("swap answer");
    assert_eq!(swapped["swapped"].as_array().map(|l| l.len()), Some(1), "{swapped}");
    let back = a.iter().find(|v| v.get("moved").is_some() && v.get("parts").is_some()).expect("backannotate answer");
    assert_eq!(back["parts"][0], "U1", "{back}");
    assert_eq!(back["left"].as_array().map(|l| l.len()), Some(0), "{back}");
    let st: Vec<&Value> = a.iter().filter(|v| v.get("open_nets").is_some()).collect();
    assert_eq!(st.len(), 2, "{a:?}");
    assert_eq!(st[0]["open_nets"], st[1]["open_nets"], "the board says the same before and after");
    let dry = a.iter().find(|v| v["dry"] == true).expect("dry swap");
    assert_eq!(dry["swapped"].as_array().map(|l| l.len()), Some(0), "nothing more to win after the import: {dry}");
}

/// `coverage` says what the simulation leaves out or only guesses; the analyses carry the counts in `models`.
#[test]
fn coverage_lists_what_the_simulation_guesses_and_the_answers_carry_the_counts() {
    let script = "new
add V 5 as=V1
add R 270 as=R1
add LED as=D1
net VCC V1.+ R1.1
net A R1.2 D1.A
net GND V1.- D1.K
coverage
dc
";
    let (a, ok) = run(script);
    assert!(ok, "{a:?}");
    let c = a.iter().find(|v| v["cmd"] == "coverage").expect("coverage answer");
    assert_eq!(c["clean"], false, "{c}");
    assert_eq!(c["generic"][0]["part"], "D1", "{c}");
    assert_eq!(c["ignored"].as_array().map(|l| l.len()), Some(0));
    let dc = a.iter().find(|v| v["cmd"] == "dc").expect("dc answer");
    assert_eq!(dc["models"]["generic"], 1, "{dc}");
    assert_eq!(dc["models"]["see"], "coverage");
}

/// `req waive`: a finding the designer accepts moves to `waived` with its reason, and a waiver without a reason is refused.
#[test]
fn an_accepted_exception_keeps_its_reason_and_is_not_counted() {
    let setup = "new
add V 12 as=V1
add R 10k as=R1
add R 4.7k as=R2
net VIN V1.+ R1.1
net MID R1.2 R2.1
net GND V1.- R2.2
        req apart 離す a=R1 b=R2 min=500 近いと困る
";
    // no reason: refused
    let (a, ok) = run(&format!("{setup}req waive apart 離す
"));
    assert!(!ok && a.iter().any(|v| v["error"].as_str().is_some_and(|e| e.contains("reason"))), "{a:?}");
    // with the reason: counted as accepted
    let (a, ok) = run(&format!("{setup}req waive apart 離す reason=基板が小さいので承知
req
board import
board req
req waive apart 離す remove=1
req
"));
    assert!(ok, "{a:?}");
    let with: &Value = a.iter().find(|v| v["waivers"].as_array().is_some_and(|l| !l.is_empty())).expect("a table with the waiver");
    assert_eq!(with["waivers"][0]["note"], "基板が小さいので承知", "{with}");
    assert_eq!(with["unexplained"], serde_json::json!([]), "{with}");
    let rep = a.iter().find(|v| v.get("findings").is_some()).unwrap();
    assert_eq!(rep["violations"], 0, "{rep}");
    assert_eq!(rep["waived"][0]["reason"], "基板が小さいので承知", "{rep}");
    let last_table = a.iter().rev().find(|v| v.get("waivers").is_some()).unwrap();
    assert_eq!(last_table["waivers"].as_array().map(|l| l.len()), Some(0), "removed: {last_table}");
}

/// `req measure`: a limit on the copper of a connection (length, vias, where ways part) is measured on the routed board; a pad pair with no
/// copper is `missing`, and a finding can be accepted like the others.
#[test]
fn a_measure_of_the_copper_is_checked_on_the_routed_board() {
    let script = "new
add V 5 as=V1
add R 270 as=R1
add LED as=D1
net VCC V1.+ R1.1
net A R1.2 D1.A
net GND V1.- D1.K
req measure 短い what=length pads=R1.2,D1.A max=0.01 reason=テスト
req measure 道あり what=length pads=R1.2,D1.A max=1000 reason=テスト
req measure ビア what=vias pads=R1.2,D1.A max=0 reason=ビアなし
board import
board req
board autoplace
board route
board req
req waive measure 短い reason=試作なので承知
board import
board req
";
    let (a, ok) = run(script);
    assert!(ok, "{a:?}");
    let reps: Vec<&Value> = a.iter().filter(|v| v.get("findings").is_some()).collect();
    assert_eq!(reps.len(), 3, "{a:?}");
    // before routing there is no copper between the pads: all three are missing, none is wrong
    assert_eq!(reps[0]["violations"], 0, "{}", reps[0]);
    assert_eq!(reps[0]["missing"].as_array().map(|l| l.len()), Some(3), "{}", reps[0]);
    // routed: the 0.01 mm limit is broken, the other two hold
    assert_eq!(reps[1]["violations"], 1, "{}", reps[1]);
    assert_eq!((reps[1]["findings"][0]["kind"].as_str(), reps[1]["findings"][0]["name"].as_str()), (Some("measure"), Some("短い")));
    // accepted: not counted, listed with its reason
    assert_eq!(reps[2]["violations"], 0, "{}", reps[2]);
    assert_eq!(reps[2]["waived"][0]["reason"], "試作なので承知", "{}", reps[2]);
}

/// `req net … order= vias=`: the properties a role would set (the order of routing, vias) are set by the row itself and reach the class.
#[test]
fn a_row_sets_the_order_and_the_vias_apart_from_its_role() {
    let (a, ok) = run("new
add V 12 as=V1
add R 10k as=R1
add R 4.7k as=R2
net VIN V1.+ R1.1
net MID R1.2 R2.1
net GND V1.- R2.2
req net 検出 nets=MID role=sense reason=高インピーダンス
req net 検出 order=7 vias=allow
req
board import
board class
req net 検出 order=role vias=role
board import
board class
req net 検出 order=12
");
    assert!(!ok, "an order of 12 is refused");
    let classes: Vec<&Value> = a.iter().filter(|v| v["classes"].is_array()).collect();
    assert_eq!(classes.len(), 2, "{a:?}");
    let c = &classes[0]["classes"][0];
    assert_eq!((c["role"].as_str(), c["order"].as_u64(), c["no_vias"].as_bool()), (Some("sense"), Some(7), Some(false)), "{c}");
    let c = &classes[1]["classes"][0];
    assert_eq!((c["order"].as_u64(), c["no_vias"].as_bool()), (Some(2), Some(true)), "the role decides again: {c}");
    let table = a.iter().find(|v| v.get("net_rules").is_some_and(|n| n[0]["order"] == 7)).expect("the table keeps the order");
    assert_eq!(table["net_rules"][0]["vias"], "allow");
}

/// Layers, the length of a net and the gap between two nets: set by the table, kept by the router and found by the check.
#[test]
fn the_layers_the_length_and_the_gap_of_nets_are_set_by_the_table_and_checked() {
    let script = "new
add V 5 as=V1
add R 270 as=R1
add LED as=D1
net VCC V1.+ R1.1
net A R1.2 D1.A
net GND V1.- D1.K
req net 線 nets=A layers=top max_length=0.5 reason=短く表面だけ
req measure 間隔 what=gap pads=A,VCC min=100 reason=離す
req
board import
board autoplace
board route
board class
board req
req net 線 layers=any max_length=1000
req net 線 layers=nope
";
    let (a, ok) = run(script);
    assert!(!ok, "a layer that does not exist is refused");
    let table = a.iter().find(|v| v.get("net_rules").is_some_and(|n| n[0]["layers"] == serde_json::json!(["top"]))).expect("the table keeps the layers");
    assert_eq!(table["net_rules"][0]["max_length"], 0.5);
    let class = a.iter().filter_map(|v| v["classes"].as_array()).next().unwrap();
    assert_eq!(class[0]["layers"], serde_json::json!(["top"]), "{class:?}");
    let rep = a.iter().rev().find(|v| v.get("findings").is_some()).unwrap();
    let kinds: Vec<(&str, &str)> = rep["findings"].as_array().unwrap().iter().map(|f| (f["kind"].as_str().unwrap(), f["name"].as_str().unwrap())).collect();
    assert!(kinds.contains(&("net", "線")), "the net is longer than 0.5 mm: {rep}");
    assert!(kinds.contains(&("measure", "間隔")), "the nets are nearer than 100 mm: {rep}");
    // routed on the top only: nothing on the bottom
    assert!(!rep["findings"].as_array().unwrap().iter().any(|f| f["message"].as_str().is_some_and(|m| m.contains("許している層"))), "{rep}");
}

/// A guard asked for in the table (`what=guard`): the share of the sensitive track that has the guard net on both sides. Before the guard
/// tracks are drawn the sensitive track is bare (0 %); `board guard` draws them and the share rises.
#[test]
fn a_guard_asked_for_in_the_table_is_found_missing_and_then_present() {
    let script = "new
add V 5 as=V1
add R 270 as=R1
add LED as=D1
net VCC V1.+ R1.1
net A R1.2 D1.A
net GND V1.- D1.K
req measure 検出のガード what=guard pads=A,GND min=10 reach=1.0 reason=検出の線は、両側をガードで囲む
board import
board move R1 5 5 rot=0
board move D1 35 5 rot=0
board route
board req
board guard net=A guard=GND
board req
";
    let (a, ok) = run(script);
    assert!(ok, "{a:?}");
    let reps: Vec<&Value> = a.iter().filter(|v| v.get("findings").is_some()).collect();
    assert_eq!(reps.len(), 2, "{a:?}");
    assert_eq!(reps[0]["violations"], 1, "no guard yet: {}", reps[0]);
    assert!(reps[0]["findings"][0]["message"].as_str().is_some_and(|m| m.contains("ガードの被覆が 0.0 %")), "{}", reps[0]);
    // the tracks of `board guard` stop where copper is in the way: this small board has room for one side only, which is about 15 %
    assert_eq!(reps[1]["violations"], 0, "the guard tracks are drawn: {}", reps[1]);
}

/// `board currents declared widen`: the widths are made for the currents the board already has (here from the table), with no DC.
#[test]
fn declared_currents_widen_the_thin_tracks_without_a_dc_run() {
    let script = "new
add V 5 as=V1
add R 270 as=R1
add LED as=D1
net VCC V1.+ R1.1
net A R1.2 D1.A
net GND V1.- D1.K
req net 電流 nets=A current=1.5 reason=大きな電流の道
board import
board autoplace
board route
req net 電流 current=3
board import
board currents declared widen
";
    let (a, ok) = run(script);
    assert!(ok, "{a:?}");
    let r = a.iter().find(|v| v.get("widened_tracks").is_some()).expect("the answer");
    assert!(r["widened_tracks"].as_u64().unwrap_or(0) > 0, "a net routed for 1.5 A and now declared 3 A gets wider: {r}");
    let a_net = r["list"].as_array().unwrap().iter().find(|w| w["net"] == "A").expect("net A");
    assert_eq!(a_net["too_thin"], false, "{a_net}");
    assert_eq!(a_net["amps"], 3.0, "the declared current is kept");
}

/// `heat` warms the resistors by their own power: a 100 ohm part with +1000 ppm/K and 100 K/W under 10 V across 200 ohm settles near 25 K up
/// (a little less, its value rises), and the nodes that move are listed; a part without a coefficient still warms but keeps its value.
#[test]
fn heat_warms_a_resistor_by_its_own_power_and_moves_the_nets() {
    let (a, ok) = run("new\nadd V 10 as=V1\nadd R 100 as=R1\nadd R 100 as=R2\nnet VIN V1.+ R1.1\nnet MID R1.2 R2.1\nnet GND V1.- R2.2\nthermal R1 theta=100 tc=1000\nthermal R2 theta=100\nheat temp=27\n");
    assert!(ok, "{a:?}");
    let h = last(&a, "heat");
    let part = |n: &str| h["parts"].as_array().unwrap().iter().find(|p| p["part"] == n).unwrap_or_else(|| panic!("{n}: {h}"));
    let (r1, r2) = (part("R1"), part("R2"));
    let rise = r1["rise_k"].as_f64().unwrap();
    assert!(rise > 20.0 && rise < 25.0, "{rise}");
    assert!((r1["rise_k"].as_f64().unwrap() - 100.0 * r1["power_w"].as_f64().unwrap()).abs() < 1e-3);
    assert!(r1["change_ppm"].as_f64().unwrap() > 20000.0);
    assert!(r2["change_ppm"].as_f64().unwrap().abs() < 1e-9 && r2["rise_k"].as_f64().unwrap() > 20.0, "{r2}");
    let mid = h["nodes"].as_array().unwrap().iter().find(|n| n["net"] == "MID").expect("MID moves");
    assert!(mid["shift_v"].as_f64().unwrap() < 0.0, "{mid}");
    assert!(h["notes"].as_array().unwrap().iter().any(|n| n.as_str().unwrap().contains("R2")));
    // a wrong value is refused
    let (_, ok) = run("new\nadd R 1k as=R1\nthermal R1 theta=-3\n");
    assert!(!ok);
}
