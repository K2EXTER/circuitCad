//! Runs small scripts through the commands of `ccad` (`common::ccad`, inside the test) and reads the JSON answers. The last test
//! runs the binary itself.

use serde_json::Value;
use super::common::{ccad, Flags};

fn run(script: &str) -> (Vec<Value>, bool) {
    ccad(script, Flags::default())
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
    ccad(script, Flags { strict: true, ..Flags::default() })
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
fn the_cooling_calculator_inverts_the_board_copper_back_to_its_datasheet_point() {
    // the copper law is fitted to NDT3055L (SOT-223, 2 oz, θjc 12 K/W): 110 K/W on the minimum pad (10.6 mm²), 42 K/W on 1 in² (645 mm²).
    // Asking for 42 K/W (1 W, 25 C around, goal 0.8 x 77.5 + 5 = 67 C) must give back the 645 - 10.6 = 634.6 mm² beyond the pads.
    let (a, ok) = run("calc cooling 1 110 12 25 77.5 70\n");
    assert!(ok, "{a:?}");
    let area = a[0]["rows"].as_array().unwrap().iter().find(|r| r["label"] == "要る銅の面積").unwrap()["value"].as_str().unwrap().to_string();
    let mm2: f64 = area.split(' ').next().unwrap().parse().unwrap();
    assert!((mm2 - 634.6).abs() < 1.5, "{area}");
    // thinner copper spreads worse: the same goal on 35 µm needs more
    let (a, _) = run("calc cooling 1 110 12 25 77.5 35\n");
    let thin = a[0]["rows"].as_array().unwrap().iter().find(|r| r["label"] == "要る銅の面積").unwrap()["value"].as_str().unwrap().to_string();
    assert!(thin.split(' ').next().unwrap().parse::<f64>().unwrap() > mm2 * 1.5, "{thin}");
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

/// `board step` writes the board and the parts' solids as a STEP file our own reader takes back.
#[test]
fn board_step_writes_the_board_and_the_parts_as_solids() {
    let sample = format!("{}/../../docs/examples/rlc_board.pcbjson", env!("CARGO_MANIFEST_DIR"));
    let file = std::env::temp_dir().join("ccad_board_step_test.step");
    let (a, ok) = run(&format!("board open {sample}\nboard step {}\n", file.display()));
    assert!(ok, "{a:?}");
    let s = last(&a, "board");
    let names: Vec<&str> = s["names"].as_array().unwrap().iter().map(|n| n.as_str().unwrap()).collect();
    assert!(s["solids"].as_u64().unwrap() > 4 && names[0] == "rlc" && names.contains(&"R1") && names.contains(&"C1"), "{s}");
    let m = pcb_core::step::read_step(&std::fs::read(&file).unwrap()).unwrap();
    assert!(m.volume() > 0.0 && m.bounds().unwrap().0[2].abs() < 1e-3, "the board's bottom at z = 0");
    let _ = std::fs::remove_file(&file);
    let (a, ok) = run(&format!("board new 40 30\nboard step {}\n", file.display()));
    assert!(ok && last(&a, "board")["solids"] == 1, "a bare board is one solid: {a:?}");
    let _ = std::fs::remove_file(&file);
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

/// `board class NAME except=NET from=X,Y to=X,Y` keeps a stretch at its own width; the class lists it, and `except_clear` takes it away.
#[test]
fn board_class_width_exceptions() {
    let (a, ok) = run("board new 40 30
board track add 5,5 15,5 width=0.5 float=1
board class pw width=0.5 nets=A except=A from=5,5 to=15,5 except_width=0.2
");
    assert!(ok, "{a:?}");
    let c = last(&a, "board");
    let e = c["width_exceptions"].as_array().unwrap();
    assert_eq!(e.len(), 1, "{c}");
    assert_eq!(e[0]["width"], 0.2);
    assert_eq!(e[0]["net"], "A");
    let (a, _) = run("board new 40 30
board class pw nets=A except_clear=A
board class pw except=B from=1,1 to=2,2 except_width=0.2
");
    assert!(a.iter().any(|v| v["ok"] == false && v["error"].as_str().is_some_and(|e| e.contains("not in the class"))), "{a:?}");
    let (a, _) = run("board new 40 30
board class pw nets=A
board class pw except=A from=1,1 to=2,2
");
    assert!(a.iter().any(|v| v["ok"] == false && v["error"].as_str().is_some_and(|e| e.contains("except_width"))), "{a:?}");
    let (a, ok) = run("board new 40 30
board class pw nets=A except=A from=1,1 to=2,2 except_width=0.2
board class pw except_clear=A
");
    assert!(ok, "{a:?}");
    assert_eq!(last(&a, "board")["width_exceptions"].as_array().unwrap().len(), 0);
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

/// The answer is the field solver's, with εeff, the delay and the closed forms beside it; the finished shape and the mask count, and
/// ground copper beside the track lowers it. `method=formula` gives the closed forms alone.
#[test]
fn board_impedance_is_solved_from_the_field() {
    let (a, ok) = run("board new 40 30\nboard impedance height=0.2 width=0.35\nboard impedance width=0.35 mask=0 mask_top=0 etch=0\nboard impedance width=0.35 ground_gap=0.15\nboard impedance width=0.2 space=0.2\nboard impedance width=0.35 method=formula\n");
    assert!(ok, "{a:?}");
    let answers: Vec<&Value> = a.iter().filter(|v| v["cmd"] == "board" && v.get("stackup").is_some()).collect();
    let (with_mask, bare, coplanar) = (answers[0]["z0"].as_f64().unwrap(), answers[1]["z0"].as_f64().unwrap(), answers[2]["z0"].as_f64().unwrap());
    assert_eq!(answers[0]["method"], "field");
    assert!(bare > with_mask, "the mask (and the full top of an unetched track) lowers it: {bare} vs {with_mask}");
    assert!(coplanar < bare - 2.0, "ground beside it: {coplanar}");
    let ee = answers[0]["er_eff"].as_f64().unwrap();
    assert!((2.5..4.5).contains(&ee), "{ee}");
    assert!(answers[0]["formula"]["z0"].as_f64().is_some());
    let p = answers[3];
    assert!(p["zdiff"].as_f64().unwrap() > 80.0 && p["odd"]["z"].as_f64().is_some() && p["zcommon"].as_f64().is_some(), "{p}");
    assert_eq!(answers[4]["method"], "formula");
    assert!(answers[4].get("er_eff").is_none());
    let (a, _) = run("board new 40 30\nboard impedance width=0.3 ground_gap=0.2 method=formula\n");
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
fn zout_of_a_divider_with_a_capacitor_is_the_parallel_resistance_and_falls_with_frequency() {
    // the source is off: OUT sees R1 || R2 (500 ohm) at low frequency and the 1 uF below that above 3 kHz
    let (a, ok) = run("new
add V 5 as=V1
add R 1k as=R1
add R 1k as=R2
add C 1u as=C1
net VIN V1.+ R1.1
net OUT R1.2 R2.1 C1.1
net GND V1.- R2.2 C1.2
erc
zout out 1 1Meg ppd=10
zout out 1 100 remove=R2
");
    assert!(ok, "{a:?}");
    let z = a.iter().rev().find(|v| v.get("per_decade").is_some() && v["min_ohm"].as_f64().unwrap() < 10.0).unwrap();
    let first = &z["per_decade"][0];
    assert!((first["ohm"].as_f64().unwrap() - 500.0).abs() < 1.0, "{first}");
    assert_eq!(first["looks_like"], "resistive");
    let last_d = z["per_decade"].as_array().unwrap().last().unwrap();
    assert_eq!(last_d["looks_like"], "capacitive");
    assert!(last_d["ohm"].as_f64().unwrap() < 0.2, "{last_d}");
    // with R2 taken out only R1 is left
    let r = a.last().unwrap();
    assert!((r["per_decade"][0]["ohm"].as_f64().unwrap() - 1000.0).abs() < 2.0, "{r}");
}

#[test]
fn worst_gives_the_limits_of_a_divider_and_names_the_corner() {
    let (a, ok) = run("new
add V 5 as=V1
add R 10k as=R1
add R 10k as=R2
net VIN V1.+ R1.1
net MID R1.2 R2.1
net GND V1.- R2.2
erc
worst mid tol_r=1
worst mid tol_r=1 tol_v=5
worst mid measure=nope
");
    assert!(!ok);
    let rows: Vec<&Value> = a.iter().filter(|v| v.get("min_corner").is_some()).collect();
    assert_eq!(rows.len(), 2, "{a:?}");
    let w = rows[0];
    assert!((w["max"].as_f64().unwrap() - 2.525).abs() < 1e-5 && (w["min"].as_f64().unwrap() - 2.475).abs() < 1e-5, "{w}");
    assert!(w["max_corner"].as_array().unwrap().iter().any(|c| c == "R2+") && w["max_corner"].as_array().unwrap().iter().any(|c| c == "R1-"), "{w}");
    // the supply's 5 % matters more than the resistors' 1 %
    assert_eq!(rows[1]["parts"][0]["part"], "V1", "{}", rows[1]);
    assert!(a.last().unwrap()["error"].as_str().unwrap().contains("measure=nope"));
}

#[test]
fn mc_spreads_a_divider_inside_its_worst_case_and_repeats_with_the_same_seed() {
    let (a, ok) = run("new
add V 5 as=V1
add R 10k as=R1
add R 10k as=R2
net VIN V1.+ R1.1
net MID R1.2 R2.1
net GND V1.- R2.2
erc
mc mid tol_r=1 runs=500 lo=2.49 hi=2.51
mc mid tol_r=1 runs=500 lo=2.49 hi=2.51
mc mid tol_v=5
");
    assert!(!ok);
    let rows: Vec<&Value> = a.iter().filter(|v| v.get("range_95").is_some()).collect();
    assert_eq!(rows.len(), 2, "{a:?}");
    let m = rows[0];
    assert_eq!(m["solved"], 500, "{m}");
    // every draw stays inside the worst case of 1 % resistors (2.475 ... 2.525 V) and is not all at the nominal
    let (min, max) = (m["min"].as_f64().unwrap(), m["max"].as_f64().unwrap());
    assert!(min >= 2.475 - 1e-9 && max <= 2.525 + 1e-9 && max - min > 0.02, "{m}");
    let r95 = m["range_95"].as_array().unwrap();
    assert!(r95[0].as_f64().unwrap() < 2.5 && r95[1].as_f64().unwrap() > 2.5, "{m}");
    let inside = m["within_limits_percent"].as_f64().unwrap();
    assert!(inside > 50.0 && inside < 100.0, "{m}");
    // the yield against the same limits: every run solved, so it is the same share; the limits are about 2σ, Cpk well below 1
    let y = &m["yield"];
    assert_eq!(y["percent"], m["within_limits_percent"], "{m}");
    assert_eq!(y["runs"], 500);
    let cpk = y["cpk"].as_f64().unwrap();
    assert!(cpk > 0.3 && cpk < 1.0, "{y}");
    // both sides fail, each side on its own: above the limit R2 was high (and R1 low), below it the other way round
    let parts = y["failing_parts"].as_array().unwrap();
    let shift = |part: &str, side: &str| parts.iter().find(|p| p["part"] == part && p["side"] == side).and_then(|p| p["shift"].as_f64());
    assert!(shift("R2", "above").unwrap() > 0.4 && shift("R2", "below").unwrap() < -0.4, "{y}");
    assert_eq!(m["histogram"]["counts"].as_array().unwrap().iter().map(|c| c.as_u64().unwrap()).sum::<u64>(), 500);
    assert_eq!(m["parts"].as_array().unwrap().len(), 2, "{m}");
    // the same seed gives the same draws
    assert_eq!(rows[0]["mean"], rows[1]["mean"]);
    assert!(a.last().unwrap()["error"].as_str().unwrap().contains("tol_v"));
}

#[test]
fn thd_reads_a_clipped_sine_and_asks_for_tran_first() {
    let head = "new
add V 0 as=V1
value V1 SIN(0 2 1k)
add R 1k as=R1
add R 1k as=R2
net VIN V1.+ R1.1
net OUT R1.2 R2.1
net GND V1.- R2.2
erc
";
    let (a, ok) = run(&format!("{head}thd probe=V(out)
"));
    assert!(!ok && a.last().unwrap()["error"].as_str().unwrap().contains("run 'tran' first"), "{a:?}");
    // a plain divider does not distort
    let (a, ok) = run(&format!("{head}tran 20m 10u
thd probe=V(out) f=1k harmonics=5
"));
    assert!(ok, "{a:?}");
    let t = a.last().unwrap();
    assert!(t["thd_percent"].as_f64().unwrap() < 0.05, "{t}");
    assert!((t["harmonics"][0]["amplitude"].as_f64().unwrap() - 1.0).abs() < 0.01, "{t}");
    assert_eq!(t["harmonics"].as_array().unwrap().len(), 5);
}

#[test]
fn spec_worst_judges_the_whole_range_of_the_tolerances() {
    let head = "new
add V 5 as=V1
add R 10k as=R1
add R 10k as=R2
net VIN V1.+ R1.1
net MID R1.2 R2.1
net GND V1.- R2.2
erc
";
    // 1 % resistors: 2.475 ... 2.525 V
    let (a, ok) = run(&format!("{head}spec dc_voltage_worst mid 2.4 2.6 tol_r=1
spec dc_voltage_worst mid 2.48 2.6 tol_r=1
"));
    assert!(ok, "{a:?}");
    let n = a.len();
    assert_eq!(a[n - 2]["pass"], true, "{}", a[n - 2]);
    assert_eq!(a[n - 1]["pass"], false, "{}", a[n - 1]);
    assert!(a[n - 1]["note"].as_str().unwrap().contains("最小"), "{}", a[n - 1]);
}

#[test]
fn thermal_links_are_kept_and_used_by_heattran_without_couple() {
    let head = "new
add V 5 as=V1
add R 100 as=RS1
add R 1Meg as=RS2
net VIN V1.+ RS1.1 RS2.1
net GND V1.- RS1.2 RS2.2
erc
";
    let (a, ok) = run(&format!("{head}thermal link=RS1:RS2:50
thermal link=rs2:rs1:80
thermal link=RS1:NOPE:10
"));
    assert!(!ok);
    let kept = a.iter().rev().find(|v| v.get("links").is_some()).unwrap();
    assert_eq!(kept["links"], serde_json::json!(["rs2:rs1:80"]), "{kept}");
    assert!(a.last().unwrap()["error"].as_str().unwrap().contains("no part NOPE"));
    // with a link kept, RS2 is warmed by RS1 in a run without couple=; with the link removed it is not
    let run_it = |extra: &str| {
        let (a, ok) = run(&format!("{head}thermal RS1 theta=100 cth=0.1
thermal RS2 theta=100 cth=0.1
{extra}heattran 2 parts=RS1,RS2 temp=25
"));
        assert!(ok, "{a:?}");
        a.last().unwrap().clone()
    };
    let linked = run_it("thermal link=RS1:RS2:20
");
    let alone = run_it("");
    let rise = |v: &Value, k: usize| v["parts"][k]["rise_peak_k"].as_f64().unwrap();
    // RS2 carries almost no current: its warmth comes only through the link
    assert!(rise(&alone, 1) < 0.01 && rise(&linked, 1) > 0.5, "{linked} {alone}");
}

#[test]
fn loopgain_of_an_opamp_amplifier_is_large_at_dc_and_stable() {
    let head = "new
add V 5 as=V1
add V 0.1 as=VIN
add LM358 as=U1
add R 10k as=RF
add R 1k as=RG
net VCC V1.+ U1.V+
net IN VIN.+ U1.IN+
net FB U1.IN- RF.1 RG.1
net OUT U1.OUT RF.2
net GND V1.- VIN.- RG.2 U1.V-
erc
";
    let (a, ok) = run(&format!("{head}loopgain rf 1 10Meg from=out
loopgain rf 1 10Meg
loopgain rf 1 10Meg from=GND
"));
    assert!(!ok, "the last one is wrong");
    let rows: Vec<&Value> = a.iter().filter(|v| v.get("cut_part").is_some()).collect();
    assert_eq!(rows.len(), 2, "{a:?}");
    for r in &rows {
        // 100 dB op-amp / 11: about 79 dB; a single pole keeps the phase margin large
        assert!((r["dc_gain_db"].as_f64().unwrap() - 79.0).abs() < 3.0, "{r}");
        assert!(r["phase_margin_deg"].as_f64().unwrap() > 60.0, "{r}");
        assert_eq!(r["signal_goes_from"], "OUT", "{r}");
    }
    assert!((rows[0]["dc_gain_db"].as_f64().unwrap() - rows[1]["dc_gain_db"].as_f64().unwrap()).abs() < 1e-6, "auto finds the same direction");
    assert!(a.last().unwrap()["error"].as_str().unwrap().contains("from= must be one of them"));
}

#[test]
fn pz_finds_the_poles_of_two_rc_stages_and_a_resonance() {
    // two buffered-free RC stages: poles are not the plain RC corners (the second stage loads the first)
    let (a, ok) = run("new
add V as=V1
value V1 \"0 AC 1\"
add R 1k as=R1
add C 100n as=C1
add R 10k as=R2
add C 1n as=C2
net IN V1.+ R1.1
net N1 R1.2 C1.1 R2.1
net OUT R2.2 C2.1
net GND V1.- C1.2 C2.2
pz out 10 10Meg in=in
");
    assert!(ok, "{a:?}");
    let r = a.last().unwrap();
    assert_eq!(r["order"], 2, "{r}");
    assert!(r["fit_ok"].as_bool().unwrap(), "{r}");
    let poles = r["poles"].as_array().unwrap();
    assert_eq!(poles.len(), 2);
    // product of the poles (in rad/s) = 1/(R1 C1 R2 C2), the sum = (R1C1 + R2C2 + R1C2)/(R1C1 R2C2)
    let p: Vec<f64> = poles.iter().map(|p| p["real_rad_s"].as_f64().unwrap()).collect();
    let prod = p[0] * p[1];
    let exact = 1.0 / (1e3 * 100e-9 * 1e4 * 1e-9);
    assert!((prod / exact - 1.0).abs() < 1e-3, "{prod} {exact}");
    assert!((r["dc_gain"].as_f64().unwrap() - 1.0).abs() < 1e-3);
    assert!(r["zeros"].as_array().unwrap().is_empty());
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
fn a_name_given_to_the_only_sheet_is_kept_for_when_the_second_is_added() {
    let (a, ok) = run("new
add R 1k as=R1
sheet name Measure
sheet add Input
sheet use Measure
list
");
    assert!(ok, "{a:?}");
    let named = a.iter().find(|v| v["cmd"] == "sheet" && v.get("note").is_some()).unwrap();
    assert_eq!(named["sheets"][0]["name"], "Measure", "{named}");
    let list = a.iter().rev().find(|v| v["cmd"] == "sheet").unwrap();
    assert_eq!(list["sheets"][0]["name"], "Measure", "{list}");
    assert_eq!(list["sheets"][1]["name"], "Input", "{list}");
    assert_eq!(last(&a, "list")["parts"].as_array().unwrap().len(), 1);
    // 'new' forgets a name that was never used
    let (a, ok) = run("new
sheet name Old
new
sheet add Second
");
    assert!(ok, "{a:?}");
    assert_eq!(last(&a, "sheet")["sheets"][0]["name"], "シート1");
}

#[test]
fn one_name_of_a_dual_use_pin_is_enough() {
    let (a, ok) = run("new
add ADS124S08 as=U1
add R 10k as=R1
net X U1.AIN6 R1.1
net Y U1.REFN1 R1.2
net Z U1.GPIO0/AIN8
nc U1.AIN9
list
");
    assert!(ok, "{a:?}");
    let pins = |net: &str| -> Vec<String> {
        let n = last(&a, "list")["nets"].as_array().unwrap().iter().find(|n| n["net"] == net).unwrap_or_else(|| panic!("no net {net}: {a:?}")).clone();
        n["pins"].as_array().unwrap().iter().map(|p| p.as_str().unwrap().to_string()).collect()
    };
    assert!(pins("X").contains(&"U1.32".to_string()), "AIN6 is pin 32 (REFP1/AIN6)");
    assert!(pins("Y").contains(&"U1.31".to_string()), "REFN1 is pin 31 (REFN1/AIN7)");
    assert_eq!(last(&a, "nc")["changed"][0], "U1.21", "AIN9 is pin 21 (GPIO1/AIN9)");
    let (a, ok) = run("new
add ADS124S08 as=U1
net X U1.AIN99
");
    assert!(!ok && a.last().unwrap()["error"].as_str().unwrap().contains("no pin 'AIN99'"), "{a:?}");
}

#[test]
fn a_sheet_used_twice_is_two_copies_with_their_own_parts_and_nets() {
    // a divider on its own sheet, used by two symbols on the first sheet
    let script = "new\nsheet add Divider\nadd R 10k as=R1\nadd R 10k as=R2\nnet MID R1.2 R2.1\nnet GND R2.2\nsheet port IN R1.1 dir=in\nsheet port OUT R1.2 dir=out\n\
                  sheet use 1\nadd V 10 as=V1\nsheet place Divider\nsheet place Divider\nnet VIN V1.+ CH1.IN CH2.IN\nnet GND V1.-\n\
                  erc\ndc probe=V(CH1.MID),V(CH2.MID)\n";
    let (a, ok) = run(script);
    assert!(ok, "{a:?}");
    let placed: Vec<_> = a.iter().filter(|v| v.get("placed").is_some()).map(|v| v["placed"].as_str().unwrap().to_string()).collect();
    assert_eq!(placed, ["CH1", "CH2"]);
    assert_eq!(last(&a, "erc")["errors"], 0, "{a:?}");
    let dc = last(&a, "dc");
    for p in ["V(CH1.MID)", "V(CH2.MID)"] {
        assert!((dc["probes"][p].as_f64().unwrap() - 5.0).abs() < 1e-6, "{p}: {dc}");
    }
    let (a, ok) = run("new\nsheet add Other\nsheet place Other\n");
    assert!(!ok && a.last().unwrap()["error"].as_str().unwrap().contains("itself"), "{a:?}");
}

#[test]
fn one_copy_of_a_sheet_can_have_its_own_value() {
    let script = "new
sheet add Divider
add R 10k as=R1
add R 10k as=R2
net MID R1.2 R2.1
net GND R2.2
sheet port IN R1.1 dir=in
sheet port OUT R1.2 dir=out
\n                  sheet use 1
add V 10 as=V1
sheet place Divider
sheet place Divider
net VIN V1.+ CH1.IN CH2.IN
net GND V1.-
sheet value CH2 R2 30k
dc probe=V(CH1.MID),V(CH2.MID)
";
    let (a, ok) = run(script);
    assert!(ok, "{a:?}");
    let dc = last(&a, "dc");
    assert!((dc["probes"]["V(CH1.MID)"].as_f64().unwrap() - 5.0).abs() < 1e-6, "{dc}");
    assert!((dc["probes"]["V(CH2.MID)"].as_f64().unwrap() - 7.5).abs() < 1e-6, "30k under 10k: {dc}");
    let (a, ok) = run(&format!("{script}sheet value CH2 R7 1k
"));
    assert!(!ok && a.last().unwrap()["error"].as_str().unwrap().contains("no part R7"), "{a:?}");
}

#[test]
fn the_copies_of_a_child_sheet_go_onto_a_board_and_are_routed() {
    let sample = format!("{}/../../docs/examples/hierarchy_rc_stages.schjson", env!("CARGO_MANIFEST_DIR"));
    let (a, ok) = run(&format!("open {sample}
board new 60 40
board import
board autoplace
board route
board status
board backannotate
"));
    assert!(!ok, "backannotate has nothing to trade here: {a:?}");
    let imp = a.iter().find(|v| v.get("added").is_some()).unwrap();
    let added: Vec<&str> = imp["added"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(added, ["V1", "R1_CH1", "C1_CH1", "R1_CH2", "C1_CH2", "R1_CH3", "C1_CH3"]);
    let st = a.iter().find(|v| v.get("drc_violations").is_some()).unwrap();
    assert_eq!((st["drc_violations"].as_i64(), st["unrouted"].as_i64()), (Some(0), Some(0)), "{st}");
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
fn a_plain_resistor_carries_its_own_shop_number_to_the_shopping_list() {
    // a plain R has no library part to carry a number: `shop` puts it on the part itself (and keeps the tolerance analyses working)
    let (a, ok) = run("new
add V 5 as=V1
add R 1k as=R1
add R 1k as=R2
net A V1.+ R1.1
net B R1.2 R2.1
net GND V1.- R2.2
simonly V1
shop R1 digikey=RC0805FR-071KL
board import
board shopping
");
    assert!(ok, "{a:?}");
    let lines = last(&a, "board")["lines"].as_array().unwrap().clone();
    let with: Vec<_> = lines.iter().filter(|l| l["shops"]["digikey"]["number"] == "RC0805FR-071KL").collect();
    assert_eq!(with.len(), 1, "{lines:?}");
    assert_eq!(with[0]["refdes"], serde_json::json!(["R1"]), "{with:?}");
    // the other resistor has only the search link, and an unknown shop or a clear are handled
    let (b, ok) = run("new
add R 1k as=R1
shop R1 digikey=X1 lcsc=C1
shop R1 digikey=
shop R1 nowhere=1
");
    assert!(!ok, "{b:?}");
    assert!(b.last().unwrap()["error"].as_str().unwrap().contains("unknown shop"), "{b:?}");
    let shown = b.iter().rev().find(|v| v["cmd"] == "shop" && v.get("shops").is_some()).unwrap();
    assert_eq!(shown["shops"], serde_json::json!({"lcsc": "C1"}), "{shown:?}");
}

#[test]
fn a_capacitor_value_can_carry_esr_and_the_dc_bias() {
    // BIAS=50 halves the capacitance: the RC corner of 1k and 100n (1.59 kHz) moves to 3.18 kHz
    let (a, ok) = run("new
add V \"0 AC 1\" as=V1
add R 1k as=R1
add C 100n as=C1
net IN V1.+ R1.1
net OUT R1.2 C1.1
net GND V1.- C1.2
value C1 \"100n BIAS=50\"
erc
ac 100 100k ppd=40 probe=V(OUT)
");
    assert!(ok, "{a:?}");
    let fc = last(&a, "ac")["signals"]["V(OUT)"]["minus_3db_hz"].as_f64().unwrap();
    assert!((fc - 3183.0).abs() < 40.0, "{fc}");
    // a wrong option is refused with the reason, and a value that is not a number still is
    let (b, ok) = run("new
add C 100n as=C1
value C1 \"100n ESR=oops\"
");
    assert!(!ok, "{b:?}");
    let (c, ok) = run("new
add R 1k as=R1
value R1 \"lots\"
");
    assert!(!ok, "{c:?}");
}

#[test]
fn a_plain_part_can_take_another_footprint() {
    let (a, ok) = run("new
add V 5 as=V1
add R 1k as=R1
net A V1.+ R1.1
net GND V1.- R1.2
simonly V1
footprint R1 R_0805_Reflow
board import
board status
");
    assert!(ok, "{a:?}");
    let imp = a.iter().find(|v| v["cmd"] == "footprint").unwrap();
    assert_eq!(imp["footprint"], "R_0805_Reflow", "{imp:?}");
    // it reaches the board's footprint (pad 0.9 mm long) and `-` goes back to the default
    let (b, ok) = run("new
add R 1k as=R1
footprint R1 R_0805_Reflow
footprint R1 -
");
    assert!(ok, "{b:?}");
    assert_eq!(b.last().unwrap()["footprint"], "R_0805", "{b:?}");
    // an unknown name and a library part are refused with the reason
    let (c, ok) = run("new
add R 1k as=R1
footprint R1 R_9999
");
    assert!(!ok && c.last().unwrap()["error"].as_str().unwrap().contains("unknown footprint"), "{c:?}");
    let (d, ok) = run("new
add AMS1117-3.3 as=U1
footprint U1 R_0805
");
    assert!(!ok && d.last().unwrap()["error"].as_str().unwrap().contains("library part"), "{d:?}");
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
fn an_adc_reads_its_input_with_the_data_sheets_transfer_function() {
    let (a, ok) = run("new
add ADS124S08 as=U1
add V 3.3 as=V1
add V 2.5 as=V2
add V 0.1 as=V3
net AVDD V1.+ U1.AVDD U1.DVDD U1.IOVDD
net REF V2.+ U1.REFP0
net IN V3.+ U1.AIN0
net GND V1.- V2.- V3.- U1.AVSS U1.DGND U1.REFN0 U1.AIN1
adc U1
adc U1 gain=64
adc U1 rate=4000 filter=low_latency
");
    assert!(ok, "{a:?}");
    let reads: Vec<&Value> = a.iter().filter(|v| v["cmd"] == "adc").collect();
    // 2^23 x 0.1 V / 2.5 V = 335544.32
    assert_eq!(reads[0]["code"], 335544, "{}", reads[0]);
    assert_eq!(reads[0]["clipped"], false);
    assert!((reads[0]["lsb_v"].as_f64().unwrap() - 2.5 / 8388608.0).abs() < 1e-12);
    // gain 64: 6.4 V of a 2.5 V full scale, so it clips at the top
    assert_eq!(reads[1]["code"], 8388607, "{}", reads[1]);
    assert_eq!(reads[1]["clipped"], true);
    // the noise (SBAS660C Table 1, 20 SPS sinc, gain 1: 0.81 uV rms, 4.8 uV p-p), with this 2.5 V reference: 22.6 bits (Table 2)
    let n = &reads[0]["noise"];
    assert!((n["rms_v"].as_f64().unwrap() - 0.81e-6).abs() < 1e-12, "{n}");
    assert!((n["effective_bits"].as_f64().unwrap() - 22.6).abs() < 0.05, "{n}");
    // 4000 SPS with the low-latency filter (Table 3: 103 uV rms): about 15.6 bits (Table 4)
    assert!((reads[2]["noise"]["effective_bits"].as_f64().unwrap() - 15.6).abs() < 0.05, "{}", reads[2]);
}

/// A meter's log over time: the reading's drift per hour and its temperature coefficient against the temperature column.
#[test]
fn a_meters_log_gives_its_drift_and_temperature_coefficient() {
    let f = std::env::temp_dir().join(format!("ccad_timelog_{}.csv", std::process::id()));
    std::fs::write(&f, "time,reading,temp
10:00:00,2.000000,20
10:30:00,2.000010,21
11:00:00,2.000020,22
").unwrap();
    let (a, ok) = run(&format!("timelog {} temp=temp
", f.display()));
    let _ = std::fs::remove_file(&f);
    assert!(ok, "{a:?}");
    let r = a.iter().find(|v| v["cmd"] == "timelog").unwrap();
    let c = &r["columns"][0];
    assert_eq!(c["name"], "reading");
    assert!((c["drift_per_hour"].as_f64().unwrap() - 20e-6).abs() < 1e-9, "{r}");
    assert!((c["tempco_ppm"].as_f64().unwrap() - 5.0).abs() < 0.01, "10 uV per degree of 2 V: {r}");
}

/// What an ADC measures, way by way: a 10 V range that sets a latching relay onto a 1/10 divider reads 1 V at the ADC (and nothing
/// with the relay left reset); a resistance read as the ratio to a reference resistor; a sensor already drawn (a part's value) and a
/// current through a shunt. The ADC's noise (20 SPS, sinc, gain 1: 4.8 uV p-p) comes back in the measured unit, and the table is kept
/// with the schematic.
#[test]
fn an_adcs_ways_of_measuring_set_their_conditions_and_give_their_noise_in_the_measured_unit() {
    let (a, ok) = run("new
add ADS124S08 as=U1
add TQ2-L2-5V as=K1
add R 900k as=R1
add R 100k as=R2
add R 10k as=R3
add R 10k as=RT
add R 10k as=RB
add R 1 as=RS
add V 3.3 as=VA
add V 2.5 as=VR
add V 5 as=V5
net AVDD VA.+ U1.AVDD U1.DVDD U1.IOVDD R3.1 U1.AIN4 RT.1
net REF VR.+ U1.REFP0
net 5V V5.+ K1.SET+ K1.RST+
net K1S K1.SET-
net K1R K1.RST-
net HI K1.COM1
net TOP K1.SET1 R1.1
net MID R1.2 R2.1 U1.AIN0
net RX R3.2 U1.AIN2 U1.AIN5
net SENS RT.2 RB.1 U1.AIN6
net SH RS.1 U1.AIN7
net GND VA.- VR.- V5.- U1.AVSS U1.DGND U1.REFN0 U1.AIN1 U1.AIN3 R2.2 RS.2 RB.2 U1.AIN8
adcmeasure 10V volts plus=HI minus=GND at=10 adc=U1 pos=AIN0 neg=AIN1 when=K1
adcmeasure off volts plus=HI minus=GND at=10 adc=U1 pos=AIN0 neg=AIN1
adcmeasure 10k ohms plus=RX minus=GND at=10k adc=U1 pos=AIN2 neg=AIN3 refpos=AIN4 refneg=AIN5
adcmeasure sensor part part=RT at=10k adc=U1 pos=AIN6 neg=AIN8 when=R3=20k
adcmeasure shunt amps plus=SH minus=GND at=100m adc=U1 pos=AIN7 neg=AIN8 gain=16
adcmeasure remove off
adcmeasure
");
    assert!(ok, "{a:?}");
    let rows: Vec<&Value> = a.iter().filter(|v| v["cmd"] == "adcmeasure").collect();
    let (v10, off, ohm, sensor, shunt) = (rows[0], rows[1], rows[2], rows[3], rows[4]);
    assert!((v10["vin"].as_f64().unwrap() - 1.0).abs() < 1e-3 && (v10["slope"].as_f64().unwrap() - 0.1).abs() < 1e-4, "{v10}");
    assert!(v10["done"].as_array().unwrap().iter().any(|d| d == "K1 セット"), "{v10}");
    // 4.8 uV p-p at the ADC is 48 uV at the input: 10 V / 48 uV, 5.3 digits
    assert!((v10["noise_pp"].as_f64().unwrap() - 48e-6).abs() < 1e-7, "{v10}");
    assert!((v10["digits"].as_f64().unwrap() - 5.32).abs() < 0.01, "{v10}");
    assert!(off["vin"].as_f64().unwrap().abs() < 1e-3, "the relay reset, the divider is off: {off}");
    // 10k against 10k: the ratio 1, 1e-4 per ohm
    assert!((ohm["value"].as_f64().unwrap() - 1.0).abs() < 1e-3 && (ohm["slope"].as_f64().unwrap() - 1e-4).abs() < 1e-6, "{ohm}");
    // the sensor RT (10k) over RB (10k) from 3.3 V: 1.65 V, falling 3.3 V x 10k / (20k)^2 = 82.5 uV per ohm
    assert_eq!(sensor["unit"], "Ω");
    assert!((sensor["vin"].as_f64().unwrap() - 1.65).abs() < 1e-3 && (sensor["slope"].as_f64().unwrap() + 82.5e-6).abs() < 1e-7, "{sensor}");
    assert!(sensor["done"].as_array().unwrap().iter().any(|d| d == "R3 = 20k"), "{sensor}");
    // 100 mA through the 1 ohm shunt: 100 mV at gain 16, 1 V per A
    assert_eq!(shunt["unit"], "A");
    assert!((shunt["vin"].as_f64().unwrap() - 0.1).abs() < 1e-6 && (shunt["slope"].as_f64().unwrap() - 1.0).abs() < 1e-6, "{shunt}");
    let all = rows.last().unwrap()["measures"].as_array().unwrap();
    assert_eq!(all.iter().map(|r| r["name"].as_str().unwrap()).collect::<Vec<_>>(), ["10V", "10k", "sensor", "shunt"]);
}

#[test]
fn an_adc_measurement_gives_its_errors_its_leaks_and_a_sensor_in_degrees() {
    let (a, ok) = run("new
add ADS124S08 as=U1
add R 900k as=R1
add R 100k as=R2
add R 10k as=R4
add NTC_10k_B3950 as=TH
add BAV199 as=D1
add V 3.3 as=VA
add V 2.5 as=VR
net AVDD VA.+ U1.AVDD U1.DVDD U1.IOVDD R4.1 D1.K
net REF VR.+ U1.REFP0
net HI R1.1
net MID R1.2 R2.1 U1.AIN0 D1.MID
net TS R4.2 TH.1 U1.AIN9
net GND VA.- VR.- U1.AVSS U1.DGND U1.REFN0 U1.AIN1 U1.AIN8 R2.2 TH.2 D1.A
adcmeasure 10V volts plus=HI minus=GND at=10 adc=U1 pos=AIN0 neg=AIN1 tol=0.1 tc=25 dtemp=20
adcmeasure t temp part=TH at=25 adc=U1 pos=AIN9 neg=AIN8
");
    assert!(ok, "{a:?}");
    let rows: Vec<&Value> = a.iter().filter(|v| v["cmd"] == "adcmeasure").collect();
    let (v10, t) = (rows[0], rows[1]);
    let e = &v10["errors"];
    // 1/10 divider: 0.1 % on either resistor moves the gain by 0.09 %, 9 mV at 10 V, the two in opposite directions
    let parts = e["parts"].as_array().unwrap();
    assert_eq!(parts.iter().map(|p| p["part"].as_str().unwrap()).collect::<std::collections::BTreeSet<_>>(), ["R1", "R2"].into(), "{e}");
    let err = |r: &str| parts.iter().find(|p| p["part"] == r).unwrap()["tol_err"].as_f64().unwrap();
    assert!((err("R2") - 0.009).abs() < 1e-4 && (err("R1") + 0.009).abs() < 1e-4, "{e}");
    assert!((e["tol_sum"].as_f64().unwrap() - 0.018).abs() < 2e-4 && (e["tol_rss"].as_f64().unwrap() - 0.0127).abs() < 2e-4, "{e}");
    // 25 ppm/K over 20 K on each: 0.05 % moves 4.5 mV each
    assert!((e["temp_sum"].as_f64().unwrap() - 0.009).abs() < 2e-4, "{e}");
    // the leaks into the divider: the ADC's input current and the clamp diode's reverse current, both far below the noise of 48 uV
    let leaks = e["leaks"].as_array().unwrap();
    let leak = |r: &str| leaks.iter().find(|l| l["part"] == r).map(|l| l["err"].as_f64().unwrap());
    let d1 = leak("D1").expect("the clamp leaks");
    assert!(d1.abs() > 1e-8 && d1.abs() < 1e-5, "{e}");
    assert!(leak("U1").is_some(), "{e}");
    // the NTC (10k, B 3950, read from its model) under 10k from 3.3 V: 1.65 V at 25 C, the noise and the error in C
    assert_eq!(t["unit"], "°C");
    assert!((t["vin"].as_f64().unwrap() - 1.65).abs() < 1e-3, "{t}");
    // dV/dT = 3.3 x 10k x R' / (20k)^2 with R' = -B / T^2 x 10k: -36.6 mV/C
    assert!((t["slope"].as_f64().unwrap() + 0.0366).abs() < 5e-4, "{t}");
    let r4 = t["errors"]["parts"].as_array().unwrap().iter().find(|p| p["part"] == "R4").unwrap()["tol_err"].as_f64().unwrap().abs();
    assert!((0.2..0.25).contains(&r4), "1 % on the upper resistor is about a quarter of a degree: {t}");
}

#[test]
fn the_pins_of_a_cable_header_are_written_and_a_firmware_is_checked() {
    let dir = std::env::temp_dir().join(format!("ccad_pins_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (h, fw) = (dir.join("pins.h"), dir.join("sketch.ino"));
    std::fs::write(&fw, "#define LED 12  // the LED
const int BUTTON = 2;
").unwrap();
    let script = format!("new
add HEADER4 as=J1
pinnames J1 D13,D2,5V,GND
add R 330 as=R1
add LED as=D1
add R 10k as=R2
net LED J1.1 R1.1
net LEDA R1.2 D1.A
net BUTTON J1.2 R2.1
net GND J1.4 D1.K R2.2
pins export={} check={}
", h.display(), fw.display());
    let (a, ok) = run(&script);
    assert!(ok, "{a:?}");
    let r = last(&a, "pins");
    let pins = r["pins"].as_array().unwrap();
    assert_eq!(pins.iter().map(|p| (p["name"].as_str().unwrap(), p["value"].as_str().unwrap())).collect::<Vec<_>>(), [("PIN_BUTTON", "2"), ("PIN_LED", "13")], "{r}");
    let text = std::fs::read_to_string(&h).unwrap();
    assert!(text.contains("#define PIN_LED") && text.contains(" 13 "), "{text}");
    // the sketch has the LED on 12: one real difference, on line 1
    assert_eq!(r["wrong"], 1, "{r}");
    assert!(r["findings"].as_array().unwrap().iter().any(|f| f["wrong"] == true && f["line"] == 1), "{r}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The same pins as a Zephyr overlay and a chip's pins as FPGA constraints; the firmware checked and the file written are kept, and
/// the ERC says when the firmware drives another pin and when the pins change after writing.
#[test]
fn pin_files_in_other_formats_and_the_erc_follows_them() {
    let dir = std::env::temp_dir().join(format!("ccad_pinfiles_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (dts, pcf, fw) = (dir.join("board.overlay"), dir.join("top.pcf"), dir.join("sketch.ino"));
    std::fs::write(&fw, "#define LED 12
").unwrap();
    let script = format!("new
add HEADER4 as=J1
pinnames J1 D13,D2,5V,GND
add R 330 as=R1
add LED as=D1
net LED J1.1 R1.1
net LEDA R1.2 D1.A
net GND J1.4 D1.K
pins export={} check={}
erc
pins export={} part=J1
net LED J1.2
erc
", dts.display(), fw.display(), pcf.display());
    let (a, ok) = run(&script);
    assert!(ok, "{a:?}");
    let o = std::fs::read_to_string(&dts).unwrap();
    assert!(o.contains("led-gpios = <&arduino_header 19 GPIO_ACTIVE_HIGH>"), "{o}");
    let erc: Vec<&Value> = a.iter().filter(|v| v["cmd"] == "erc").collect();
    let msgs = |v: &Value| v["problems"].as_array().unwrap().iter().map(|p| p["message"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert!(msgs(erc[0]).iter().any(|m| m.contains("ファームのピン")), "{:?}", msgs(erc[0]));
    assert!(!msgs(erc[0]).iter().any(|m| m.contains("書き出したあと")), "just written: {:?}", msgs(erc[0]));
    let p = std::fs::read_to_string(&pcf).unwrap();
    assert!(p.contains("set_io led 1"), "{p}");
    // J1.2 joins the LED net too: the constraints written before are out of date
    assert!(msgs(erc[1]).iter().any(|m| m.contains("書き出したあと")), "{:?}", msgs(erc[1]));
    let _ = std::fs::remove_dir_all(&dir);
}

/// `tidy` lays a sheet built by commands out again, its nets unchanged; with references only those parts move.
#[test]
fn a_sheet_built_by_commands_is_tidied_with_its_nets_unchanged() {
    let (a, ok) = run("new
add V 5 as=V1
add R 10k as=R1
add R 10k as=R2
add C 100n as=C1
net VIN V1.+ R1.1
net MID R1.2 R2.1 C1.1
net GND V1.- R2.2 C1.2
list
tidy
list
tidy C1
list
");
    assert!(ok, "{a:?}");
    let lists: Vec<&Value> = a.iter().filter(|v| v["cmd"] == "list").collect();
    let nets = |v: &Value| {
        let mut n: Vec<(String, Vec<String>)> = v["nets"].as_array().unwrap().iter().map(|n| (n["net"].as_str().unwrap().to_string(), { let mut p: Vec<String> = n["pins"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).filter(|x| !x.starts_with('#')).collect(); p.sort(); p })).collect();
        n.retain(|x| !x.1.is_empty());
        n.sort();
        n
    };
    assert_eq!(nets(lists[1]), nets(lists[0]));
    assert_eq!(nets(lists[2]), nets(lists[0]));
    let t: Vec<&Value> = a.iter().filter(|v| v["cmd"] == "tidy").collect();
    assert_eq!(t[0]["moved"], 4);
    // VIN is a supply (a symbol on V1 and on R1) and GND a ground (symbols): MID, a signal on three pins, by a tree of wires
    assert_eq!(t[0]["wired"], 1);
    assert_eq!(t[1]["moved"], 1);
}

/// The sheets read from KiCad and LTspice tidy with their nets unchanged: the source on the left, the parts after it (a net name
/// the other program made up, N001, is no supply).
#[test]
fn imported_sheets_are_tidied_from_left_to_right() {
    for (cmd, file) in [("import_kicad", "kicad-divider.kicad_sch"), ("import_ltspice", "ltspice-rc.asc")] {
        let path = format!("{}/../../docs/examples/{file}", env!("CARGO_MANIFEST_DIR"));
        let (a, ok) = run(&format!("{cmd} {path}
list
tidy
list
"));
        assert!(ok, "{file}: {a:?}");
        let lists: Vec<&Value> = a.iter().filter(|v| v["cmd"] == "list").collect();
        let nets = |v: &Value| {
            let mut n: Vec<Vec<String>> = v["nets"].as_array().unwrap().iter().map(|n| { let mut p: Vec<String> = n["pins"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).filter(|x| !x.starts_with('#')).collect(); p.sort(); p }).filter(|p| !p.is_empty()).collect();
            n.sort();
            n
        };
        assert_eq!(nets(lists[1]), nets(lists[0]), "{file}");
        let t = a.iter().find(|v| v["cmd"] == "tidy").unwrap();
        assert!(t["columns"].as_u64().unwrap() >= 2, "{file}: {t}");
    }
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
fn a_group_is_made_a_section_and_the_one_ground_point_from_the_command() {
    let (a, ok) = run("new
add R 10k as=R1
add R 4.7k as=R2
add R 1k as=R3
add R 1k as=R4
req group アナログ parts=R1,R2 section=1
req group 電源 parts=R3,R4 gnd=1
req
req group アナログ gnd=1
req
req group アナログ gnd=0
req
");
    assert!(ok, "{a:?}");
    let tables: Vec<&Value> = a.iter().filter(|v| v.get("groups").is_some() && v.get("net_rules").is_some()).collect();
    let marks = |t: &Value| t["groups"].as_array().unwrap().iter().map(|g| (g["name"].as_str().unwrap().to_string(), g["section"].as_bool().unwrap(), g["ground_point"].as_bool().unwrap())).collect::<Vec<_>>();
    let row = |n: &str, sec: bool, gnd: bool| (n.to_string(), sec, gnd);
    assert_eq!(tables.len(), 7, "each req group answers with the table too");
    assert_eq!(marks(tables[2]), [row("アナログ", true, false), row("電源", true, true)], "gnd=1 makes it a section too");
    assert_eq!(marks(tables[4]), [row("アナログ", true, true), row("電源", true, false)], "one ground point at most");
    assert_eq!(marks(tables[6]), [row("アナログ", true, false), row("電源", true, false)]);
    let (a, ok) = run("new
add R 1k as=R1
add R 1k as=R2
req group A parts=R1,R2 gnd=2
");
    assert!(!ok && format!("{a:?}").contains("gnd=2"), "{a:?}");
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

/// The number of vias and the necked-down track of a net: set by the table (`max_vias`, `max_neck`), carried into the class, and refused when
/// they are not numbers.
#[test]
fn the_via_count_and_the_neck_limit_are_set_by_the_table_and_carried_into_the_class() {
    let script = "new
add V 5 as=V1
add R 270 as=R1
add LED as=D1
net VCC V1.+ R1.1
net A R1.2 D1.A
net GND V1.- D1.K
req net 線 nets=A max_vias=2 max_neck=3.5
req
board import
board class
req net 線 max_vias=x
";
    let (a, ok) = run(script);
    assert!(!ok, "a via count that is not a number is refused");
    let table = a.iter().find(|v| v.get("net_rules").is_some()).expect("the table");
    assert_eq!((table["net_rules"][0]["max_vias"].as_u64(), table["net_rules"][0]["max_neck"].as_f64()), (Some(2), Some(3.5)), "{table}");
    let class = a.iter().filter_map(|v| v["classes"].as_array()).next().unwrap();
    assert_eq!((class[0]["max_vias"].as_u64(), class[0]["max_neck"].as_f64()), (Some(2), Some(3.5)), "{class:?}");
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

/// `thermal REF jc= sink=` (H5): a heat sink only shortens the way beyond the case. 1 W into R1 of 100 K/W with θjc 20 and a 10 K/W sink:
/// 20 + 1/(1/80 + 1/10) = 28.9 K; `sink=0` takes it off, `jc=0` puts θjc back to the guess.
#[test]
fn a_heat_sink_cools_a_part_only_beyond_its_case() {
    let (a, ok) = run("new\nadd V 10 as=V1\nadd R 100 as=R1\nnet VIN V1.+ R1.1\nnet GND V1.- R1.2\nthermal R1 theta=100 jc=20 sink=10\nheat temp=25\nthermal R1 sink=0 jc=0\nheat temp=25\n");
    assert!(ok, "{a:?}");
    let t = a.iter().find(|v| v["cmd"] == "thermal").unwrap();
    assert_eq!((t["theta_jc"].as_f64(), t["heatsink_k_per_w"].as_f64(), t["theta_jc_guess"].as_bool()), (Some(20.0), Some(10.0), Some(false)), "{t}");
    let heats: Vec<&Value> = a.iter().filter(|v| v["cmd"] == "heat").collect();
    let r1 = |h: &Value| h["parts"].as_array().unwrap().iter().find(|p| p["part"] == "R1").cloned().unwrap();
    let (sunk, bare) = (r1(heats[0]), r1(heats[1]));
    assert!((sunk["theta_k_per_w"].as_f64().unwrap() - (20.0 + 1.0 / (1.0 / 80.0 + 0.1))).abs() < 1e-9, "{sunk}");
    assert!((sunk["rise_k"].as_f64().unwrap() - 28.9).abs() < 0.1, "1 W: {sunk}");
    assert_eq!(bare["theta_k_per_w"].as_f64().unwrap(), 100.0, "{bare}");
    assert!((bare["theta_jc_k_per_w"].as_f64().unwrap() - 100.0 * 12.0 / 110.0).abs() < 1e-9, "the guess: {bare}");
}

/// `board heatmap` (H7): the board's temperature on a grid. Two 2512 resistors of the same 0.75 W: R1's pads sit in pours of their own
/// nets, R2's on thin tracks only, so R1 runs much cooler; the hottest spot is at R2, every part is warmer than the air, and the
/// junction sits above the copper under its pads.
#[test]
fn heatmap_shows_the_pour_spreading_one_resistors_heat() {
    // the sample heat_copper: R1's pads in pours of their own nets (VIN, GND), R2 on thin tracks
    let sample = format!("{}/../../docs/examples/heat_copper.ccproj", env!("CARGO_MANIFEST_DIR"));
    let (a, ok) = run(&format!("open {sample}
board heatmap temp=25 cell=1
"));
    assert!(ok, "{a:?}");
    let h = a.iter().find(|v| v["cmd"] == "board" && v["hottest_c"].is_number()).unwrap_or_else(|| panic!("{a:?}"));
    let part = |n: &str| h["parts"].as_array().unwrap().iter().find(|p| p["part"] == n).cloned().unwrap();
    let (r1, r2) = (part("R1"), part("R2"));
    let (j1, j2) = (r1["junction_c"].as_f64().unwrap(), r2["junction_c"].as_f64().unwrap());
    assert!(j1 < 25.0 + 0.75 * (j2 - 25.0), "the pours cool R1: {r1} {r2}");
    assert!(j1 > 30.0 && r1["pads_c"].as_f64().unwrap() < j1, "{r1}");
    let at = h["hottest_at"].as_array().unwrap();
    assert!((at[0].as_f64().unwrap() - 70.0).abs() < 5.0, "the hottest spot is at R2: {h}");
    assert!((5.0..20.0).contains(&h["h_top_w_m2k"].as_f64().unwrap()), "{h}");
}

/// `board heatmap` with another sheet open: the heat sources come from every sheet, not only the one being edited (a 7-sheet design
/// once lost its 1 W transistor from the map while the sheet of its microcontroller wiring was open).
#[test]
fn heatmap_takes_the_parts_of_every_sheet() {
    let sample = format!("{}/../../docs/examples/heat_copper.ccproj", env!("CARGO_MANIFEST_DIR"));
    let (a, ok) = run(&format!("open {sample}
sheet add Other
sheet use Other
board heatmap temp=25 cell=2
"));
    assert!(ok, "{a:?}");
    let h = a.iter().find(|v| v["cmd"] == "board" && v["hottest_c"].is_number()).unwrap_or_else(|| panic!("{a:?}"));
    let names: Vec<&str> = h["parts"].as_array().unwrap().iter().filter_map(|p| p["part"].as_str()).collect();
    assert!(names.contains(&"R1") && names.contains(&"R2"), "{h}");
}

/// `board heatmap tran=`: the sample's 90 × 40 mm board holds about 11.7 J/K of FR-4 (90 × 40 × 1.6 mm³ × 2.0 J/cm³·K) and warms in
/// minutes; its parts rise moment by moment and, 20 minutes on, stand where the steady map has them.
#[test]
fn heatmap_shows_the_board_warming_up() {
    let sample = format!("{}/../../docs/examples/heat_copper.ccproj", env!("CARGO_MANIFEST_DIR"));
    let (a, ok) = run(&format!("open {sample}
board heatmap temp=25 cell=1 tran=1200
"));
    assert!(ok, "{a:?}");
    let h = a.iter().find(|v| v["warm_up"].is_object()).unwrap();
    let w = &h["warm_up"];
    let cap = w["board_heat_capacity_j_per_k"].as_f64().unwrap();
    assert!((11.5..12.5).contains(&cap), "{w}");
    assert!((60.0..400.0).contains(&w["board_time_constant_s"].as_f64().unwrap()), "{w}");
    let r2 = |m: &Value| m["parts"].as_array().unwrap().iter().find(|p| p["part"] == "R2").unwrap()["junction_c"].as_f64().unwrap();
    let moments = w["moments"].as_array().unwrap();
    assert!(moments.windows(2).all(|x| r2(&x[1]) >= r2(&x[0]) - 1e-6), "{w}");
    let steady = h["parts"].as_array().unwrap().iter().find(|p| p["part"] == "R2").unwrap()["junction_c"].as_f64().unwrap();
    let end = r2(moments.last().unwrap());
    assert!((end - steady).abs() < 0.03 * (steady - 25.0), "20 minutes on {end}, steady {steady}");
}

/// `board heatmap tran= power=circuit`: the sample's supply made a PWM (5 V for 0.5 s of every 1 s), its `heattran` run of one period
/// drives the board. Holding that run's last value (0 V) the board cools after the first half second; repeating it, the board warms
/// to about where half the steady power takes it.
#[test]
fn heatmap_warms_the_board_with_the_circuits_power_over_time() {
    let sample = format!("{}/../../docs/examples/heat_copper.ccproj", env!("CARGO_MANIFEST_DIR"));
    let head = format!("open {sample}
value V1 PULSE(0 5 0 1m 1m 0.5 1)
heattran 1 parts=R1,R2
");
    let r2_end = |a: &[Value]| {
        let w = &a.iter().find(|v| v["warm_up"].is_object()).unwrap_or_else(|| panic!("{a:?}"))["warm_up"];
        let last = w["moments"].as_array().unwrap().last().unwrap();
        (last["parts"].as_array().unwrap().iter().find(|p| p["part"] == "R2").unwrap()["junction_c"].as_f64().unwrap(), w.clone())
    };
    let (a, ok) = run(&format!("{head}board heatmap temp=25 cell=2 tran=1200 power=circuit
"));
    assert!(ok, "{a:?}");
    let (held, w) = r2_end(&a);
    assert_eq!(w["driven_by_circuit"].as_array().unwrap().len(), 2, "{w}");
    let (a, ok) = run(&format!("{head}board heatmap temp=25 cell=2 tran=1200 power=circuit repeat=1
"));
    assert!(ok, "{a:?}");
    let (repeated, _) = r2_end(&a);
    // the steady map at the full power, for the half
    let (a, _) = run(&format!("open {sample}
board heatmap temp=25 cell=2
"));
    let full = a.iter().find(|v| v["parts"].is_array() && v["hottest_c"].is_number()).unwrap()["parts"].as_array().unwrap().iter().find(|p| p["part"] == "R2").unwrap()["junction_c"].as_f64().unwrap();
    assert!(held < 30.0, "after one half second of power the board has cooled again: {held}");
    let half = 25.0 + 0.5 * (full - 25.0);
    assert!((repeated - half).abs() < 0.15 * (full - 25.0), "the PWM ends near half the steady rise: {repeated} against {half}");
    // without a heattran run first: what to do
    let (a, ok) = run(&format!("open {sample}
board heatmap tran=10 power=circuit
"));
    assert!(!ok && a.last().unwrap()["error"].as_str().unwrap().contains("heattran"));
}

/// `heatsweep`: the heat-sink sample's U2 rises 48 K above the air whatever the air, so it reaches its 125 °C (guessed) limit at about
/// 77 °C around; in the transistor sample the collector resistor of the transistor without an emitter resistor goes from a few kelvin
/// at 0 °C to past its limit at 80 °C (the transistor nearly saturates), while the other half stays nearly as it was.
#[test]
fn heatsweep_finds_the_ambient_where_a_part_reaches_its_limit() {
    let ex = |n: &str| format!("{}/../../docs/examples/{n}.schjson", env!("CARGO_MANIFEST_DIR"));
    let (a, ok) = run(&format!("open {}
heatsweep -20 85 5
", ex("regulator_heatsink")));
    assert!(ok, "{a:?}");
    let part = |v: &Value, n: &str| v["parts"].as_array().unwrap().iter().find(|p| p["part"] == n).cloned().unwrap();
    let h = a.last().unwrap();
    assert_eq!(h["points"].as_u64(), Some(22));
    let u2 = part(h, "U2");
    let at = u2["over_from_ambient_c"].as_f64().unwrap();
    assert!((74.0..80.0).contains(&at), "{u2}");
    assert_eq!(part(h, "U1")["over_from_ambient_c"].as_f64(), Some(-20.0), "U1 is past its limit from the start");
    let (a, ok) = run(&format!("open {}
heatsweep 0 80 20
", ex("bjt_self_heat")));
    assert!(ok, "{a:?}");
    let h = a.last().unwrap();
    let (r3, r6) = (part(h, "R3"), part(h, "R6"));
    assert!(r3["rise_low_k"].as_f64().unwrap() < 10.0 && r3["over_from_ambient_c"].is_number(), "{r3}");
    assert!(r6["over_from_ambient_c"].is_null() && r6["rise_high_k"].as_f64().unwrap() < 2.0 * r6["rise_low_k"].as_f64().unwrap(), "{r6}");
    let (_, ok) = run("new
heatsweep 50 0 5
");
    assert!(!ok);
}

/// `board heatmap case=`: the sample's 1.5 W in a closed 100 × 50 mm ABS box (15 mm above the board, 5 mm below) warms the air inside
/// by about 15 K, and R2 runs hotter by about what the board's surroundings rose; an aluminium box is hardly cooler (the outer surface
/// carries the heat, not the wall).
#[test]
fn heatmap_in_a_case_warms_the_board_by_what_the_case_adds() {
    let sample = format!("{}/../../docs/examples/heat_copper.ccproj", env!("CARGO_MANIFEST_DIR"));
    let (a, ok) = run(&format!("open {sample}
board heatmap temp=25 cell=1
board heatmap temp=25 cell=1 case=100,50,15,5
board heatmap temp=25 cell=1 case=100,50,15,5,2,alu
board heatmap temp=25 case=100
"));
    assert!(!ok, "a case needs four numbers");
    let maps: Vec<&Value> = a.iter().filter(|v| v["hottest_c"].is_number()).collect();
    let r2 = |v: &Value| v["parts"].as_array().unwrap().iter().find(|p| p["part"] == "R2").unwrap()["junction_c"].as_f64().unwrap();
    let (open, abs, alu) = (maps[0], maps[1], maps[2]);
    assert!(open["case"].is_null());
    let c = &abs["case"];
    let (air, seen) = (c["air_c"].as_f64().unwrap(), c["board_ambient_c"].as_f64().unwrap());
    assert!((8.0..25.0).contains(&(air - 25.0)) && seen < air && seen > c["wall_c"].as_f64().unwrap(), "{c}");
    let shift = seen - 25.0;
    assert!((r2(abs) - r2(open) - shift).abs() < 0.4 * shift, "R2 open {} in the case {}, the surroundings +{shift}", r2(open), r2(abs));
    let alu_seen = alu["case"]["board_ambient_c"].as_f64().unwrap();
    assert!(alu_seen < seen && seen - alu_seen < 0.15 * shift, "{alu_seen} {seen}");
}

/// `board heatmap` lists the parts that warm each other: R2 moved next to R1 on the sample gets about 14 K from it, so the pair comes with
/// a distance to keep; as a requirement (`req apart`, taken to the board by `board import`), the board's requirement check reports the two
/// as too close.
#[test]
fn heatmap_suggests_keeping_hot_neighbours_apart() {
    let sample = format!("{}/../../docs/examples/heat_copper.ccproj", env!("CARGO_MANIFEST_DIR"));
    let (a, ok) = run(&format!("open {sample}
board move R2 30 8
board heatmap temp=25 cell=1
"));
    assert!(ok, "{a:?}");
    let h = a.iter().find(|v| v["hot_pairs"].is_array()).unwrap();
    let pairs = h["hot_pairs"].as_array().unwrap();
    assert_eq!(pairs.len(), 1, "{h}");
    let p = &pairs[0];
    let (d, keep) = (p["dist_mm"].as_f64().unwrap(), p["suggest_mm"].as_f64().unwrap());
    assert!(p["a_from_b_k"].as_f64().unwrap() > 3.0 && keep > d + 2.0, "{p}");
    let (a, ok) = run(&format!("open {sample}
board move R2 30 8
req apart heat a=R1 b=R2 min={keep}
board import
board req
"));
    assert!(ok, "{a:?}");
    let check = a.last().unwrap();
    assert!(check["findings"].as_array().unwrap().iter().any(|f| f.to_string().contains("heat")), "{check}");
    // the sample as it is: the two are 50 mm apart, no pair
    let (a, _) = run(&format!("open {sample}
board heatmap temp=25 cell=1
"));
    assert!(a.iter().find(|v| v["hot_pairs"].is_array()).unwrap()["hot_pairs"].as_array().unwrap().is_empty());
}

/// With the board carried over (`board postlayout on`), the parts that give off heat are solved on the board's grid: a GND pour under the
/// board spreads R2's heat, so its θ falls below its own 100 K/W, while R1 beside it adds heat through the board; R1's own θ stays what it
/// was given.
#[test]
fn heat_with_postlayout_lowers_theta_by_the_copper_joined_to_the_pins() {
    let (a, ok) = run("new
add V 10 as=V1
add R 100 as=R1
add R 100 as=R2
net VIN V1.+ R1.1
net MID R1.2 R2.1
net GND V1.- R2.2
thermal R1 theta=100
thermal R2 theta=100
board import
board autoplace
board route
board pour net=GND
heat
board postlayout on
heat
");
    assert!(ok, "{a:?}");
    let heats: Vec<&Value> = a.iter().filter(|v| v["cmd"] == "heat").collect();
    let part = |h: &Value, n: &str| h["parts"].as_array().unwrap().iter().find(|p| p["part"] == n).cloned().unwrap_or_else(|| panic!("{n}: {h}"));
    let (before, after) = (part(heats[0], "R2"), part(heats[1], "R2"));
    assert_eq!(before["theta_k_per_w"].as_f64().unwrap(), 100.0, "{before}");
    assert!(before["board_copper_mm2"].is_null(), "{before}");
    let theta = after["theta_k_per_w"].as_f64().unwrap();
    assert!(theta < 90.0 && theta > 20.0, "{after}");
    assert_eq!(after["own_theta_k_per_w"].as_f64().unwrap(), 100.0);
    assert!(after["board_copper_mm2"].as_f64().unwrap() > 100.0, "{after}");
    // the board's grid: R2's own θ comes down, but R1 sits next to it and warms it through the board; the rise is both
    assert_eq!(after["board_grid"].as_bool(), Some(true), "{after}");
    let from = after["from_others_k"].as_f64().unwrap();
    assert!(from > 1.0, "R1 next to it adds heat: {after}");
    assert!((after["rise_k"].as_f64().unwrap() - (theta * after["power_w"].as_f64().unwrap() + from)).abs() < 1e-3, "{after}");
    // its copper is warm too: the wiring is worked out again at the map's temperature, a few per cent more resistive
    let warm = heats[1]["wiring_warm"].as_array().unwrap();
    assert!(!warm.is_empty() && warm.iter().all(|w| w["temp_c"].as_f64().unwrap() > 27.0 && w["resistance_factor"].as_f64().unwrap() > 1.0), "{}", heats[1]);
    assert!(heats[1]["notes"].as_array().unwrap().iter().any(|n| n.as_str().unwrap().contains("board copper")), "{}", heats[1]);
}

/// `heattran`: a 1 s pulse of 0.25 W into a part of 100 K/W and 0.05 J/K (5 s) warms it by 25 (1 - e^-0.2) = 4.5 K, far below the 25 K of the
/// steady state; a part without a heat capacity shows the steady value; a coupling to a wrong name is refused.
#[test]
fn heattran_warms_a_part_by_the_pulse_not_by_the_steady_state() {
    let head = "new\nadd V \"PULSE(0 10 0.1 1u 1u 1 100)\" as=V1\nadd R 100 as=R1\nadd R 100 as=R2\nnet VIN V1.+ R1.1\nnet MID R1.2 R2.1\nnet GND V1.- R2.2\nthermal R1 theta=100 cth=0.05\n";
    let (a, ok) = run(&format!("{head}heattran 1.1 parts=R1 tstep=5m\n"));
    assert!(ok, "{a:?}");
    let p = &last(&a, "heattran")["parts"][0];
    let rise = p["rise_peak_k"].as_f64().unwrap();
    assert!(rise > 4.2 && rise < 4.8, "{p}");
    assert!((p["steady_state_rise_k"].as_f64().unwrap() - 25.0).abs() < 0.1, "{p}");
    assert_eq!(p["temperature"], "ok", "{p}");
    // the peak (27 + 4.5 C) is what meets the limit: 30 C is passed for a moment, and the message says when
    let (a, ok) = run(&format!("{head}thermal R1 tmax=30\nheattran 1.1 parts=R1 tstep=5m\n"));
    assert!(ok, "{a:?}");
    let h = last(&a, "heattran");
    assert!(h["parts"][0]["temperature"] == "over" && h["parts"][0]["margin_at_peak_k"].as_f64().unwrap() < 0.0, "{h}");
    assert!(h["temperature_problems"][0]["message"].as_str().unwrap().contains("秒"), "{h}");
    let (a, ok) = run(&format!("{head}thermal R1 cth=0\nheattran 1.1 parts=R1 tstep=5m\n"));
    assert!(ok, "{a:?}");
    assert!(last(&a, "heattran")["parts"][0]["rise_peak_k"].as_f64().unwrap() > 24.0);
    let (_, ok) = run(&format!("{head}heattran 1.1 parts=R1 couple=R1:R9:50\n"));
    assert!(!ok);
    // Zth stages (10 K/W in 1 ms, 40 K/W in 100 ms) with the rest of θ (50 K/W) on the slow capacity: the 1 s pulse now warms by
    // 0.25 x (10 + 40 (1 - e^-10)) = 12.5 K plus the slow stage's 0.25 x 50 x (1 - e^-0.4) = 4.1 K
    let (a, ok) = run(&format!("{head}thermal R1 zth=10:1m,40:100m\nheattran 1.1 parts=R1 tstep=5m\n"));
    assert!(ok, "{a:?}");
    let p = &last(&a, "heattran")["parts"][0];
    assert_eq!(p["heat_stages"], 3, "{p}");
    let rise = p["rise_peak_k"].as_f64().unwrap();
    assert!((rise - 16.6).abs() < 0.6, "{p}");
    // the same stages fitted from points of their curve
    let pts: Vec<String> = [1e-4, 3e-4, 1e-3, 3e-3, 1e-2, 3e-2, 0.1, 0.3, 1.0, 3.0].iter().map(|t: &f64| format!("{t}:{}", 10.0 * (1.0 - (-t / 1e-3).exp()) + 40.0 * (1.0 - (-t / 0.1).exp()))).collect();
    let (a, ok) = run(&format!("{head}thermal R1 zthfit={}\n", pts.join(",")));
    assert!(ok, "{a:?}");
    let t = last(&a, "thermal");
    assert!(t["fit"]["rms_error_pct"].as_f64().unwrap() < 1.0 && (t["fit"]["total_k_per_w"].as_f64().unwrap() - 50.0).abs() < 1.0, "{t}");
    let (a, _) = run(&format!("{head}thermal R1 zth=10:0\n"));
    assert!(a.last().unwrap()["error"].as_str().unwrap().contains("above zero"), "{a:?}");
}

/// `board matchpairs` brings the shorter differential pair to the length of the longer one, on both lines.
#[test]
fn matchpairs_lengthens_the_shorter_pair() {
    use pcb_core::diffpair::DiffPair;
    use pcb_core::model::*;
    let mut b = Board::new("t", 60.0, 40.0);
    for (name, y, len) in [("D1", 10.0, 40.0), ("D2", 25.0, 30.0)] {
        for (net, dy) in [(format!("{name}_P"), 0.0), (format!("{name}_N"), 0.4)] {
            for x in [5.0, 5.0 + len] {
                let mut p = Pad::new(Point::new(x, y + dy), PadShape::Rect { w: 0.3, h: 0.3 }, None, Side::Top);
                p.net = net.clone();
                b.pads.push(p);
            }
            b.tracks.push(Track { side: Side::Top, a: Point::new(5.0, y + dy), b: Point::new(5.0 + len, y + dy), width: 0.2 });
        }
        b.diff_pairs.push(DiffPair::new(name, &format!("{name}_P"), &format!("{name}_N")));
    }
    let path = std::env::temp_dir().join(format!("ccad-matchpairs-{}.pcbjson", std::process::id()));
    std::fs::write(&path, serde_json::to_string(&b).unwrap()).unwrap();
    let (a, ok) = run(&format!("board open {}\nboard matchpairs\nboard matchpairs pairs=NOPE\n", path.display()));
    assert!(!ok, "a pair name that does not exist is an error: {a:?}");
    let first = a.iter().find(|v| v["target_mm"].is_number()).expect("an answer");
    assert!((first["target_mm"].as_f64().unwrap() - 40.0).abs() < 1e-6, "{first}");
    let added = first["pairs"][1]["added_mm"].as_f64().expect("D2 was lengthened");
    assert!((added - 10.0).abs() < 0.3, "{first}");
    std::fs::remove_file(&path).ok();
}

/// `board break` puts a fault into a good board, `board health` says it is not mended until the checks agree with the board before the fault again.
#[test]
fn break_health_and_reveal_work_as_a_repair_practice() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/work_in_progress/attiny_blinker_board.pcbjson");
    let p = path.display();
    let (a, ok) = run(&format!("board open {p}\nboard health\nboard break fault=cut seed=3\nboard health\nboard reveal\nboard open {p}\nboard health\n"));
    assert!(ok, "{a:?}");
    let healths: Vec<&Value> = a.iter().filter(|v| v["health"].is_object() && v.get("repaired").is_some()).collect();
    // before the fault: nothing to find, nothing to compare with
    assert_eq!(healths[0]["health"]["total"], 0, "{:?}", healths[0]);
    assert_eq!(healths[0]["repaired"], Value::Null);
    // `board break` answers with the symptom and what the checks find now
    let broke = a.iter().find(|v| v["symptom"].is_string()).expect("an answer of break");
    assert!(broke["health"]["total"].as_u64().unwrap() > 0, "{broke}");
    // the next health says: not mended
    let after = healths.iter().find(|v| v["repaired"] == Value::Bool(false)).expect("not mended");
    assert!(after["health"]["total"].as_u64().unwrap() > 0);
    // the cause is told only when asked
    let reveal = a.iter().find(|v| v["cause"].is_string()).expect("reveal");
    assert!(reveal["cause"].as_str().unwrap().contains("切れて"), "{reveal}");
    // the good board again: mended
    assert!(a.iter().any(|v| v["repaired"] == Value::Bool(true)), "{a:?}");
}

#[test]
fn record_probes_keeps_only_the_probed_signals_and_record_all_keeps_everything() {
    let rc = "new\nadd V 5 as=V1\nadd R 1k as=R1\nadd C 1u as=C1\nnet VCC V1.+ R1.1\nnet OUT R1.2 C1.1\nnet GND V1.- C1.2\n";
    // all signals, the probed ones shown
    let (a, ok) = run(&format!("{rc}tran 5m 50u probe=V(out) record=all\n"));
    assert!(ok, "{a:?}");
    let all = last(&a, "tran")["points"].as_u64().unwrap();
    // only the probed ones are kept: the same points and the same numbers, and a note says so
    let (b, ok) = run(&format!("{rc}tran 5m 50u probe=V(out) record=probes\nspec tran_pp V(VCC) 0 1\n"));
    assert!(ok, "{b:?}");
    let sp = last(&b, "spec");
    assert!(sp["pass"].is_null() && sp["note"].as_str().unwrap().contains("結果にありません"), "a spec on a signal that was not kept says so: {sp:?}");
    let t = last(&b, "tran");
    assert_eq!(t["points"].as_u64().unwrap(), all);
    assert_eq!(t["signals"].as_object().unwrap().len(), 1, "{t:?}");
    assert!(t["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("記録は probe= の信号だけ")), "{t:?}");
    let (c, _) = run(&format!("{rc}tran 5m 50u probe=V(out) record=all\n"));
    assert_eq!(last(&c, "tran")["signals"]["V(OUT)"]["final"], t["signals"]["V(OUT)"]["final"]);
    // errors
    let (d, ok) = run(&format!("{rc}tran 5m 50u record=probes\n"));
    assert!(!ok && d.iter().any(|v| v["error"].as_str().is_some_and(|e| e.contains("record=probes needs probe="))), "{d:?}");
    let (e, ok) = run(&format!("{rc}tran 5m 50u probe=V(out) record=some\n"));
    assert!(!ok && e.iter().any(|v| v["error"].as_str().is_some_and(|e| e.contains("not 'all' or 'probes'"))), "{e:?}");
}

#[test]
fn psrr_of_an_rc_filter_grows_twenty_db_a_decade_above_its_corner() {
    // R1 1k and C1 1u from the supply: corner 159 Hz; the AC 1 on the signal source V2 is silenced
    let (a, ok) = run("new
add V 5 as=V1
add V 0 AC 1 as=V2
add R 1k as=R1
add C 1u as=C1
add R 1k as=R2
net VCC V1.+ R1.1
net OUT R1.2 C1.1
net IN V2.+ R2.1
net GND V1.- C1.2 V2.- R2.2
erc
psrr out 10 100k supply=V1
psrr in 10 100k
psrr out 10 100k supply=V9
");
    assert!(!ok, "the last line names no supply");
    let p = a.iter().find(|v| v["net"] == "out").unwrap();
    assert_eq!(p["supply"], "V1");
    let at = |f: f64| p["at"].as_array().unwrap().iter().find(|x| x["hz"].as_f64() == Some(f)).unwrap()["rejection_db"].as_f64().unwrap();
    assert!((at(10e3) - 36.0).abs() < 0.5, "{p}");
    assert!((at(100e3) - at(10e3) - 20.0).abs() < 0.5);
    // the first supply is taken when none is named, and nothing reaches IN from it
    let q = a.iter().find(|v| v["net"] == "in").unwrap();
    assert_eq!(q["supply"], "V1");
    assert!(q["worst"]["rejection_db"].as_f64().unwrap() > 150.0, "{q}");
}

#[test]
fn cmrr_of_the_difference_amplifier_sample_and_its_spread_with_one_percent_resistors() {
    let sample = format!("{}/../../docs/examples/cmrr_diffamp.schjson", env!("CARGO_MANIFEST_DIR"));
    let (a, ok) = run(&format!("open {sample}\ncmrr OUT 10 100k tol_r=1 runs=100\nvalue R4 101k\ncmrr OUT 100 100 tol_r=0\ncmrr OUT 10 100k plus=V3 minus=V3\n"));
    assert!(!ok, "the last line drives both inputs from one source");
    let rows: Vec<&Value> = a.iter().filter(|v| v.get("per_decade").is_some()).collect();
    assert_eq!(rows.len(), 2, "{a:?}");
    let m = rows[0];
    assert_eq!((m["plus"].as_str(), m["minus"].as_str()), (Some("V3"), Some("V4")), "{m}");
    assert!((m["differential_gain"].as_f64().unwrap() - 10.0).abs() < 0.05, "{m}");
    assert!(m["lowest_frequency"]["cmrr_db"].as_f64().unwrap() > 100.0, "matched resistors: {m}");
    // 1 % resistors: the worst case of a gain-10 difference amplifier is (1 + 10) / 0.04 → 48.8 dB
    let sp = &m["resistor_spread"];
    let (lo, med) = (sp["min_db"].as_f64().unwrap(), sp["median_db"].as_f64().unwrap());
    assert!(lo > 47.0 && med > lo && med < 90.0, "{sp}");
    assert_eq!(sp["one_resistor_high"].as_array().unwrap().len(), 4);
    // R4 1 % high alone: about 61 dB
    let one = rows[1]["lowest_frequency"]["cmrr_db"].as_f64().unwrap();
    assert!((one - 60.9).abs() < 1.0, "{}", rows[1]);
    assert!(rows[1]["resistor_spread"].is_null());
}

#[test]
fn open_and_save_keep_what_the_app_stores_next_to_the_circuit() {
    // the sample carries a spec table and bench recordings; a tolerance is added to it here
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/rc_lowpass_measured.schjson");
    let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&src).unwrap()).unwrap();
    v["tolerances"] = serde_json::json!({"r": "1", "c": "10", "l": "10", "v": "0"});
    let dir = std::env::temp_dir().join(format!("ccad_extras_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (a, b) = (dir.join("a.schjson"), dir.join("b.schjson"));
    std::fs::write(&a, serde_json::to_string(&v).unwrap()).unwrap();
    let (out, ok) = run(&format!("open {}
value R1 2k
save {}
", a.display(), b.display()));
    assert!(ok, "{out:?}");
    let w: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&b).unwrap()).unwrap();
    for key in ["specs", "bench", "tolerances"] {
        assert_eq!(w[key], v[key], "{key} kept");
    }
    // a new sheet starts without them
    let (_, ok) = run(&format!("open {}
new
save {}
", a.display(), b.display()));
    assert!(ok);
    let w: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&b).unwrap()).unwrap();
    assert!(w.get("specs").is_none() && w.get("tolerances").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn diagnose_names_the_open_resistor_and_the_point_that_tells_it_apart() {
    // 10 V, R1 10k to MID, R2 10k to GND, R3 1k MID→OUT, R4 10k OUT→GND; OUT measured dead
    let (a, ok) = run("new
add V 10 as=V1
add R 10k as=R1
add R 10k as=R2
add R 1k as=R3
add R 10k as=R4
net VIN V1.+ R1.1
net MID R1.2 R2.1 R3.1
net OUT R3.2 R4.1
net GND V1.- R2.2 R4.2
diagnose OUT=0
diagnose OUT=0 MID=5
");
    assert!(ok, "{a:?}");
    let first = &a[a.len() - 2];
    let faults: Vec<&str> = first["suspects"].as_array().unwrap().iter().take(3).map(|s| s["fault"].as_str().unwrap()).collect();
    assert!(faults.iter().any(|f| f.starts_with("R1")) && faults.iter().any(|f| f.starts_with("R3")), "{faults:?}");
    assert!(first["measure_next"].as_array().unwrap().iter().take(2).any(|n| n["net"] == "MID"), "{first}");
    let second = a.last().unwrap();
    assert!(second["suspects"][0]["fault"].as_str().unwrap().starts_with("R3"), "{second}");
}

#[test]
fn worst_and_mc_move_an_op_amps_offset_between_its_data_sheet_ends() {
    // an LM358 follower at 2.5 V: the data sheet's offset is 3 mV typ, 7 mV max either way
    let follower = "new
add LM358 as=U1
add V 5 as=VP
add V 2.5 as=VI
net VCC VP.+ U1.V+
net GND VP.- VI.- U1.V-
net IN VI.+ U1.IN+
net OUT U1.OUT U1.IN-
erc
";
    let (a, ok) = run(&format!("{follower}worst out tol_r=0 tol_c=0 tol_l=0
mc out runs=200 tol_r=0 tol_c=0 tol_l=0
worst out tol_r=0 tol_c=0 tol_l=0 spread=off
"));
    assert!(!ok, "without the spreads there is nothing to vary");
    let w = a.iter().find(|v| v.get("min_corner").is_some()).unwrap();
    let span = w["max"].as_f64().unwrap() - w["min"].as_f64().unwrap();
    assert!((span - 14e-3).abs() < 0.2e-3, "the follower moves with the offset, -7..+7 mV: {w}");
    assert_eq!(w["parts"][0]["part"], "U1:offset", "{w}");
    assert_eq!(w["parts"][0]["data_sheet_range"], serde_json::json!([-0.007, 0.007]), "{w}");
    let mc = a.iter().find(|v| v["cmd"] == "mc").unwrap();
    assert!(mc.to_string().contains("U1:offset"), "{mc}");
    assert!(a.last().unwrap()["error"].as_str().unwrap().contains("許容差のある部品"), "{a:?}");
    let (b, _) = run(&format!("{follower}worst out spread=maybe
"));
    assert!(b.last().unwrap()["error"].as_str().unwrap().contains("spread=maybe"));
}

#[test]
fn worst_moves_an_op_amps_bias_current_from_typ_to_max_through_a_high_source_resistance() {
    // the same follower fed through 1 MΩ: the LM358's bias current (out of the pin) is 20 nA typ, 250 nA max, so IN+ sits
    // 20 mV above 2.5 V at typ and 250 mV at max; the offset adds its -7..+7 mV
    let (a, ok) = run("new
add LM358 as=U1
add V 5 as=VP
add V 2.5 as=VI
add R 1Meg as=RS
net VCC VP.+ U1.V+
net GND VP.- VI.- U1.V-
net SRC VI.+ RS.1
net IN RS.2 U1.IN+
net OUT U1.OUT U1.IN-
erc
worst out tol_r=0 tol_c=0 tol_l=0
");
    assert!(ok, "{a:?}");
    let w = a.iter().find(|v| v.get("min_corner").is_some()).unwrap();
    let span = w["max"].as_f64().unwrap() - w["min"].as_f64().unwrap();
    // (the offset's share is a little more than 14 mV: the 2 MΩ between the inputs shares the source resistance)
    assert!(span > 0.24 && span < 0.26, "the bias current's 20..250 nA through 1 MΩ and the offset: {w}");
    assert_eq!(w["parts"][0]["part"], "U1:ib+", "{w}");
    let r = &w["parts"][0]["data_sheet_range"];
    assert!((r[0].as_f64().unwrap() + 250e-9).abs() < 1e-12 && (r[1].as_f64().unwrap() + 20e-9).abs() < 1e-12, "{w}");
    assert!(w.to_string().contains("U1:iq"), "the supply current is varied too (it does not move OUT): {w}");
}

#[test]
fn a_makers_series_is_listed_once_and_placed_by_its_value() {
    // Panasonic ERJ-3EKF: 10 kΩ in E24/E96 is ERJ-3EKF1002V; 4.7 k and 4.7 k as a divider of 12 V gives 6 V
    let (a, ok) = run("new\nparts ERJ\nadd V 12 as=V1\nadd ERJ-3EKF 4.7k as=R1\nadd ERJ-3EKF 4k7 as=R2\nnet VIN V1.+ R1.1\nnet MID R1.2 R2.1\nnet GND V1.- R2.2\ndc\n");
    assert!(ok, "{a:?}");
    let parts = last(&a, "parts");
    let erj = parts["series"].as_array().unwrap().iter().find(|s| s["series"] == "ERJ-3EKF").unwrap();
    assert_eq!(erj["example"], "ERJ-3EKF1002V");
    assert_eq!(parts["series"].as_array().unwrap().len(), 4, "the four sizes, once each");
    assert_eq!(parts["count"], 0, "the values are not listed one by one");
    let adds: Vec<&Value> = a.iter().filter(|v| v["cmd"] == "add" && v["ref"] != "V1").collect();
    assert_eq!(adds[0]["type"], "ERJ-3EKF4701V");
    assert_eq!(adds[1]["type"], "ERJ-3EKF4701V");
    assert_eq!(adds[0]["footprint"], "R_0603_Panasonic");
    let mid = last(&a, "dc")["voltages"]["MID"].as_f64().unwrap();
    assert!((mid - 6.0).abs() < 1e-6, "MID = {mid}");
    // a value the series does not make is refused with the values next to it
    let (b, ok) = run("new\nadd ERJ-3EKF 4.8k as=R1\n");
    assert!(!ok);
    let err = b.last().unwrap()["error"].as_str().unwrap();
    assert!(err.contains("ERJ-3EKF4751V") && err.contains("ERJ-3EKF4871V"), "{err}");
}

#[test]
fn a_transistor_without_an_emitter_resistor_draws_more_current_as_it_warms() {
    // the sample bjt_self_heat: Q1 has its base held by a divider, Q2 has 100 ohm in its emitter; both start at about 5 mA
    let sample = format!("{}/../../docs/examples/bjt_self_heat.schjson", env!("CARGO_MANIFEST_DIR"));
    let (a, ok) = run(&format!("open {sample}
heat
"));
    assert!(ok, "{a:?}");
    let h = last(&a, "heat");
    let part = |r: &str| h["parts"].as_array().unwrap().iter().find(|p| p["part"] == r).unwrap().clone();
    let ic = |vce: f64, ve: f64| (12.0 - vce - ve) / 330.0;
    let node = |n: &str, k: &str| h["nodes"].as_array().unwrap().iter().find(|x| x["net"] == n).map_or(0.0, |x| x[k].as_f64().unwrap());
    let (q1, q2) = (part("Q1"), part("Q2"));
    let v = |p: &serde_json::Value, k: &str| p[k].as_f64().unwrap();
    let (q1c, q1w) = (ic(v(&q1, "junction_cold_v"), 0.0), ic(v(&q1, "junction_warm_v"), 0.0));
    let (q2c, q2w) = (ic(v(&q2, "junction_cold_v"), node("E2", "cold_v")), ic(v(&q2, "junction_warm_v"), node("E2", "warm_v")));
    assert!((q1c - 5.5e-3).abs() < 0.5e-3 && (q2c - 5.3e-3).abs() < 0.5e-3, "cold {q1c} {q2c}");
    // the junction at a fixed base voltage: the current more than doubles; with the emitter resistor it moves a few percent
    assert!(q1w / q1c > 2.0 && q2w / q2c < 1.08 && q2w > q2c, "Q1 {q1c}->{q1w}, Q2 {q2c}->{q2w}");
    assert!(v(&q1, "rise_k") > 25.0 && (v(&q1, "rise_k") - v(&q1, "theta_k_per_w") * v(&q1, "power_w")).abs() < 1e-3, "{q1}");
    assert_eq!(q1["junctions"], 1);
}

#[test]
fn heat_judges_each_part_against_its_temperature_limit_and_lowered_rating() {
    let sample = format!("{}/../../docs/examples/bjt_self_heat.schjson", env!("CARGO_MANIFEST_DIR"));
    let (a, ok) = run(&format!("open {sample}\nheat\nheat temp=110\nthermal Q1 tmax=115\nheat temp=110\n"));
    assert!(ok, "{a:?}");
    let heats: Vec<&serde_json::Value> = a.iter().filter(|v| v["cmd"] == "heat").collect();
    let part = |h: &serde_json::Value, r: &str| h["parts"].as_array().unwrap().iter().find(|p| p["part"] == r).unwrap().clone();
    let problem = |h: &serde_json::Value, r: &str| h["temperature_problems"].as_array().unwrap().iter().find(|p| p["part"] == r).cloned();
    // 27 C around: everything well inside its (guessed) limit
    assert!(heats[0]["temperature_problems"].as_array().unwrap().is_empty(), "{}", heats[0]);
    assert_eq!((part(heats[0], "Q1")["t_max_c"].as_f64(), part(heats[0], "Q1")["t_max_from"].as_str()), (Some(150.0), Some("guess")));
    // 110 C around: Q1 (no emitter resistor) draws far more, and its collector resistor R3 (0603) goes past its limit
    let r3 = problem(heats[1], "R3").expect("R3 over its limit");
    assert!(r3["level"] == "over" && r3["message"].as_str().unwrap().contains("超えます"), "{r3}");
    // a 115 C limit on Q1 puts it over
    let q1 = part(heats[2], "Q1");
    assert!(q1["t_max_c"] == 115.0 && q1["t_max_from"] == "part" && q1["temperature"] == "over" && q1["margin_k"].as_f64().unwrap() < 0.0, "{q1}");
}

#[test]
fn a_murata_mlcc_loses_capacitance_under_dc_bias_as_its_data_sheet_shows() {
    // GRM188R61C106MA73 (10 µF X5R 16 V 0603): the data sheet's DC bias curve gives -49 % at 4 V, -76 % at 8 V and -89 % at 16 V.
    // With 100 Ω the cutoff 1 / (2π R C) moves up by the same factor.
    let f3 = |bias: &str| {
        let (a, ok) = run(&format!("new\nadd V 1 as=V1\nvalue V1 \"{bias} AC 1\"\nadd R 100 as=R1\nadd GRM188R61C106MA73 as=C1\nnet IN V1.+ R1.1\nnet OUT R1.2 C1.1\nnet GND V1.- C1.2\nac 10 10Meg ppd=40 probe=V(OUT)\n"));
        assert!(ok, "{a:?}");
        last(&a, "ac")["signals"]["V(OUT)"]["minus_3db_hz"].as_f64().unwrap()
    };
    let c = |f: f64| 1.0 / (2.0 * std::f64::consts::PI * 100.0 * f);
    let c0 = c(f3("0"));
    assert!((c0 / 10e-6 - 1.0).abs() < 0.02, "C at 0 V = {c0}");
    for (v, change) in [("4", -0.49), ("8", -0.76), ("16", -0.89)] {
        let got = c(f3(v)) / c0 - 1.0;
        assert!((got - change).abs() < 0.04, "{v} V: {got:.3} against the data sheet's {change}");
    }
    // a class 1 part (CH) does not change
    let (a, ok) = run("new\nadd V 1 as=V1\nvalue V1 \"40 AC 1\"\nadd R 1k as=R1\nadd GRM1882C1H103JA01 as=C1\nnet IN V1.+ R1.1\nnet OUT R1.2 C1.1\nnet GND V1.- C1.2\nac 100 10Meg ppd=40 probe=V(OUT)\n");
    assert!(ok, "{a:?}");
    let f = last(&a, "ac")["signals"]["V(OUT)"]["minus_3db_hz"].as_f64().unwrap();
    assert!((c(f) * 100.0 / 1000.0 / 10e-9 - 1.0).abs() < 0.02, "10 nF CH at 40 V: {}", c(f) / 10.0);
}

/// The binary itself: a script on standard input, one JSON answer per line, exit code 1 at the first failing line (which stops
/// the run). The other tests run the same commands inside the test.
#[test]
fn the_binary_reads_standard_input_and_exits_1_on_a_failure() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let lib = std::env::temp_dir().join(format!("ccad-test-bin-lib-{}", std::process::id()));
    let go = |script: &str| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_ccad")).args(["--lib", lib.to_str().unwrap(), "-"]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(script.as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        let answers: Vec<Value> = String::from_utf8(out.stdout).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        (answers, out.status.code())
    };
    let (a, code) = go("new
add V 12 as=V1
add R 10k as=R1
add R 4.7k as=R2
net VIN V1.+ R1.1
net MID R1.2 R2.1
net GND V1.- R2.2
dc
");
    assert_eq!(code, Some(0), "{a:?}");
    let mid = last(&a, "dc")["voltages"]["MID"].as_f64().unwrap();
    assert!((mid - 12.0 * 4.7 / 14.7).abs() < 1e-6, "MID = {mid}");
    let (a, code) = go("new
add R abc
add R 1k
");
    assert_eq!(code, Some(1));
    assert_eq!(a.len(), 2, "the run stops at the failing line: {a:?}");
    let _ = std::fs::remove_dir_all(&lib);
}

#[test]
fn fit_makes_a_model_from_data_sheet_readings_and_writes_it_into_the_part() {
    // the bundled 2N7000's readings give the bundled model, with each reading next to the model's value
    let spec = concat!(env!("CARGO_MANIFEST_DIR"), "/../part-lib/fits/2N7000_TO-92.fit.json");
    let (a, ok) = run(&format!("fit \"{spec}\"\n"));
    assert!(ok, "{a:?}");
    let f = last(&a, "fit");
    // the series resistance is a resistor of a .subckt with the temperature coefficients fitted to Figure 3 (rds_temp)
    assert_eq!(f["model"], ".subckt 2N7000 d g s\nRD1 d di 0.933 TC1=0.007106 TC2=8.755e-06\nM1 di g s s MCH\n.model MCH NMOS(VTO=2.035 KP=0.4742 CGS=1.6e-11 CGD=4e-12)\n.ends 2N7000");
    let rows = f["rows"].as_array().unwrap();
    assert!(rows.iter().any(|r| r["item"] == "RDS(on)" && r["error_pct"].as_f64().unwrap().abs() < 1.0), "{f}");
    assert!(rows.iter().any(|r| r["item"] == "gFS" && r["compare_only"] == true), "{f}");
    assert!(rows.iter().any(|r| r["item"] == "RDS(on) の温度" && r["error_pct"].as_f64().unwrap().abs() < 1.0), "{f}");
    assert!(f["notes"][0].as_str().unwrap().contains("gFS"), "{f}");
    // 'write' puts the model back into a copy of the part whose model was changed; the rest of the file stays
    let dir = std::env::temp_dir().join(format!("ccad-fit-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bundled = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../part-lib/examples/2N7000_TO-92.part.json")).unwrap();
    std::fs::write(dir.join("2N7000_TO-92.part.json"), bundled.replace("KP=0.4742", "KP=1")).unwrap();
    let (a, ok) = run(&format!("fit \"{spec}\" write dir=\"{}\"\n", dir.display()));
    let after = std::fs::read_to_string(dir.join("2N7000_TO-92.part.json")).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    assert!(ok, "{a:?}");
    assert_eq!(after, bundled);
    let (a, ok) = run("fit nowhere.fit.json\n");
    assert!(!ok && a.last().unwrap()["error"].as_str().unwrap().contains("nowhere"), "{a:?}");
}

#[test]
fn poweron_gives_the_range_and_the_limit_and_judges_a_reading() {
    let (a, ok) = run("new
add V 5 as=V1
add R 250 as=R1
add R 250 as=R2
net VIN V1.+ R1.1
net MID R1.2 R2.1
net GND V1.- R2.2
poweron tol_r=5
poweron tol_r=5 measured=10m
poweron tol_r=5 measured=0.5m supply=v1
poweron measured=abc
");
    assert!(!ok, "measured=abc is refused");
    let p = last(&a, "poweron");
    assert!(p["error"].as_str().unwrap().contains("measured=abc"), "{p}");
    let rows: Vec<&Value> = a.iter().filter(|v| v["cmd"] == "poweron" && v["ok"] == true).collect();
    let s = &rows[0]["supplies"][0];
    assert_eq!(s["supply"], "V1");
    let (min, max, limit) = (s["min_a"].as_f64().unwrap(), s["max_a"].as_f64().unwrap(), s["limit_a"].as_f64().unwrap());
    assert!((min - 5.0 / 525.0).abs() < 1e-9 && (max - 5.0 / 475.0).abs() < 1e-9 && (limit - 0.02).abs() < 1e-12, "{s}");
    assert!(rows[0]["reading"].is_null());
    assert_eq!(rows[1]["reading"]["verdict"], "normal");
    assert_eq!(rows[2]["reading"]["verdict"], "nothing");
    assert!(!rows[2]["reading"]["causes"].as_array().unwrap().is_empty());
}
