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
    assert!(after <= before + 1e-6, "{imp}");
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
    for shop in ["akizuki", "digikey", "lcsc"] {
        assert!(r["shops"][shop]["search"].as_str().unwrap().starts_with("https://"), "{r}");
    }
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
