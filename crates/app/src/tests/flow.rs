use super::*;

/// V1 -> R1 -> R2 -> GND drawn as a schematic, imported into the board.
#[test]
fn schematic_to_board_flow() {
    let mut app = App::default();
    let s = &mut app.sch.sch;
    s.add(Kind::VSource, (0, 0), 0);
    s.add(Kind::Resistor, (6, -6), 0);
    s.add(Kind::Resistor, (8, 0), 1);
    s.add(Kind::Ground, (0, 6), 0);
    let r2 = s.components[2].pins();
    for (a, b) in [((0, -2), (0, -6)), ((0, -6), (4, -6)), ((8, -6), r2[0].1), (r2[1].1, (8, 4)), ((8, 4), (0, 4)), ((0, 2), (0, 6))] {
        s.wires.push(Wire { a, b });
    }

    app.board_ed.import_from_schematic(&app.sch);
    let board = &mut app.board_ed.board;
    assert_eq!(board.parts.len(), 3); // V1, R1, R2 (GND symbol is not a part)
    let net = |r: &str, p: &str| board.pads.iter().find(|x| x.refdes == r && x.pin == p).unwrap().net.clone();
    assert_eq!(net("V1", "-"), "GND");
    assert_eq!(net("R2", "2"), "GND");
    assert_eq!(net("R1", "2"), net("R2", "1"));
    // GND: V1.- and R2.2 ; nets +V/R1.1 and R1.2/R2.1 : three 2-pad nets => 3 airwires
    assert_eq!(ratsnest(board).len(), 3);

    // route one net by hand and the airwire disappears
    let a = board.pads.iter().find(|p| p.refdes == "R1" && p.pin == "2").unwrap().pos;
    let b = board.pads.iter().find(|p| p.refdes == "R2" && p.pin == "1").unwrap().pos;
    board.tracks.push(Track { side: Side::Top, a, b, width: 0.25 });
    assert_eq!(ratsnest(board).len(), 2);
}

/// A library part (custom symbol + its own package) travels schematic -> netlist -> board.
#[test]
fn library_part_reaches_the_board() {
    let mut app = App::default();
    let def = part_lib::EXAMPLES
        .iter()
        .map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap())
        .find(|d| d.name == "LM358")
        .unwrap();
    app.sch.library = vec![def.clone()];
    let u = app.sch.sch.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (20, 10), 0);
    app.sch.sch.add(Kind::Resistor, (34, 10), 0);
    let out = app.sch.sch.components[u].pins().iter().find(|p| p.0 == "1").unwrap().1;
    app.sch.sch.wires.push(Wire { a: out, b: (32, 10) });

    app.board_ed.import_from_schematic(&app.sch);
    let board = &mut app.board_ed.board;
    assert!(board.footprints.iter().any(|f| f.name == "SOIC-8_P1.27"));
    assert_eq!(board.pads.iter().filter(|p| p.refdes == "U1").count(), 8);
    let u1_out = board.pads.iter().find(|p| p.refdes == "U1" && p.pin == "1").unwrap().clone();
    let r1_in = board.pads.iter().find(|p| p.refdes == "R1" && p.pin == "1").unwrap();
    assert!(!u1_out.net.is_empty() && u1_out.net == r1_in.net);
    // moving the part keeps its nets
    board.place_part("U1", Point::new(10.0, 10.0), 0);
    let again = board.pads.iter().find(|p| p.refdes == "U1" && p.pin == "1").unwrap();
    assert_eq!(again.net, u1_out.net);
    assert_eq!(ratsnest(board).len(), 1);
}

/// The 3-phase source of the rectifier sample reaches the board as a 4-pad terminal row, one net per pin.
#[test]
fn three_phase_source_reaches_the_board() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.sch.load_path(root.join("three_phase_rectifier.schjson"));
    app.board_ed.import_from_schematic(&app.sch);
    let board = &app.board_ed.board;
    assert!(board.footprints.iter().any(|f| f.name == "TerminalBlock_4P_5.08"));
    let mut pads: Vec<_> = board.pads.iter().filter(|p| p.refdes == "V1").collect();
    pads.sort_by(|a, b| a.pin.cmp(&b.pin));
    assert_eq!(pads.iter().map(|p| p.pin.as_str()).collect::<Vec<_>>(), ["1", "2", "3", "4"]);
    assert_eq!(pads.iter().map(|p| p.net.as_str()).collect::<Vec<_>>(), ["A", "B", "C", "N"]);
    // one row, 5.08 mm apart, with holes (through-hole terminals)
    for w in pads.windows(2) {
        let (dx, dy) = ((w[1].pos.x - w[0].pos.x).abs(), (w[1].pos.y - w[0].pos.y).abs());
        assert!((dx - 5.08).abs() < 1e-6 && dy < 1e-6 || (dy - 5.08).abs() < 1e-6 && dx < 1e-6, "{dx} {dy}");
    }
    assert!(pads.iter().all(|p| p.drill.is_some_and(|d| d > 0.0)));
    // the diodes' pads carry the same nets, so the ratsnest has something to connect
    assert!(!ratsnest(board).is_empty());
}

#[test]
fn dc_analysis_from_the_editor() {
    let mut app = App::default();
    let s = &mut app.sch.sch;
    s.add(Kind::VSource, (0, 0), 0);
    s.components[0].value = "10".into();
    s.add(Kind::Resistor, (6, -6), 0);
    s.add(Kind::Resistor, (12, 0), 1);
    s.add(Kind::Ground, (0, 6), 0);
    let r2 = s.components[2].pins();
    for (a, b) in [((0, -2), (0, -6)), ((0, -6), (4, -6)), ((8, -6), (12, -6)), ((12, -6), r2[0].1), (r2[1].1, (12, 6)), ((12, 6), (0, 6)), ((0, 2), (0, 6))] {
        s.wires.push(Wire { a, b });
    }
    app.sch.analyze_dc();
    assert!(app.sch.status.contains("計算しました"), "{}", app.sch.status);
    // R1 = R2 = 10k default: the midpoint is at half the supply
    assert!((app.sch.pin_voltage("R2", "1").unwrap() - 5.0).abs() < 1e-6);
    assert!(app.sch.pin_voltage("V1", "-").unwrap().abs() < 1e-9);

    // a broken value is reported instead of a result
    app.sch.sch.components[1].value = "oops".into();
    app.sch.analyze_dc();
    assert!(app.sch.status.contains("失敗"));
}

#[test]
fn command_line_opens_files_in_the_right_mode() {
    let dir = std::env::temp_dir().join(format!("circuitcad_cli_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut b = pcb_core::Board::new("cli", 20.0, 10.0);
    b.pads.push(pcb_core::Pad::new(Point::new(5.0, 5.0), pcb_core::PadShape::Circle { d: 1.6 }, Some(0.8), Side::Top));
    let path = dir.join("b.pcbjson");
    std::fs::write(&path, serde_json::to_string(&b).unwrap()).unwrap();
    let out = dir.join("fab");
    std::fs::create_dir_all(&out).unwrap();

    let mut app = App::default();
    let args: Vec<String> = [path.to_str().unwrap(), "--drc", "--export", out.to_str().unwrap()].iter().map(|s| s.to_string()).collect();
    apply_args(&mut app, &args);
    assert!(app.mode == Mode::Board);
    assert_eq!(app.board_ed.board.name, "cli");
    assert!(app.board_ed.status.contains("書き出し完了"), "{}", app.board_ed.status);
    for f in ["cli.GTL", "cli.GKO", "cli-PTH.DRL", "cli-BOM.csv", "cli-CPL.csv", "cli_gerber.zip"] {
        assert!(out.join(f).exists(), "{f} missing");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The router on a real circuit: the NE555 blinker laid out on a grid of parts.
#[test]
fn autoroute_handles_the_ne555_blinker() {
    use pcb_core::ratsnest::ratsnest;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.sch.load_path(root.join("ne555_blinker.schjson"));
    app.board_ed.import_from_schematic(&app.sch);
    let b = &mut app.board_ed.board;
    b.set_rect_outline(0.0, 0.0, 60.0, 40.0);
    let refs: Vec<String> = b.parts.iter().map(|p| p.refdes.clone()).collect();
    for (k, r) in refs.iter().enumerate() {
        b.place_part(r, Point::new(8.0 + 10.0 * (k % 5) as f64, 10.0 + 14.0 * (k / 5) as f64), 0);
    }
    let before = ratsnest(b).len();
    assert!(before > 0);
    app.board_ed.autoroute_now(None);
    let left = ratsnest(&app.board_ed.board);
    println!("{} -> {} unrouted; {}", before, left.len(), app.board_ed.status);
    assert!(left.len() < before, "{}", app.board_ed.status);
    // whatever it routed keeps the rules
    assert!(pcb_core::drc::check(&app.board_ed.board).is_empty(), "{:?}", pcb_core::drc::check(&app.board_ed.board));
    app.board_ed.undo();
    assert_eq!(ratsnest(&app.board_ed.board).len(), before, "one undo takes the whole run back");
}

/// Schematic -> import -> autoplace -> autoroute on every shipped example: the placement is
/// always rule-clean and the router finishes most of the work.
#[test]
fn autoplace_then_autoroute_on_the_shipped_examples() {
    use pcb_core::ratsnest::ratsnest;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    for name in ["rc_lowpass", "led_blinker", "ne555_blinker", "opamp_amp", "regulator_led", "tp5100_charger", "ip2332_charger"] {
        let mut app = App::default();
        app.sch.load_path(root.join(format!("{name}.schjson")));
        app.board_ed.import_from_schematic(&app.sch);
        app.board_ed.autoplace();
        assert!(app.board_ed.status.starts_with("自動配置"), "{name}: {}", app.board_ed.status);
        assert!(pcb_core::drc::check(&app.board_ed.board).is_empty(), "{name}: {:?}", pcb_core::drc::check(&app.board_ed.board));
        let before = ratsnest(&app.board_ed.board).len();
        app.board_ed.autoroute_now(None);
        let left = ratsnest(&app.board_ed.board).len();
        println!("{name}: {before} -> {left} unrouted");
        assert!(left * 2 <= before, "{name}: {before} -> {left}");
        app.board_ed.undo();
        app.board_ed.undo();
        assert!(app.board_ed.board.parts.iter().all(|p| p.pos.x > 0.0 || p.pos.y > 0.0));
    }
}

/// A board opened on its own (no schematic loaded) is not "different from the schematic".
#[test]
fn a_board_opened_alone_is_not_reported_as_different_from_the_schematic() {
    let mut app = App::default();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    app.board_ed.load_path(root.join("rlc_board.pcbjson"));
    assert!(!app.board_ed.board.parts.is_empty());
    assert!(app.board_ed.schematic_diff(&app.sch).is_empty(), "{:?}", app.board_ed.schematic_diff(&app.sch));
    // with the matching schematic loaded it is compared as before (and agrees)
    app.sch.load_path(root.join("rlc_pulse.schjson"));
    assert!(app.board_ed.schematic_diff(&app.sch).is_empty(), "{:?}", app.board_ed.schematic_diff(&app.sch));
    // ... and a different schematic is noticed
    assert!(app.sch.load_sample(board_editor::BoardEditor::lesson_circuit()));
    assert!(!app.board_ed.schematic_diff(&app.sch).is_empty());
}

/// The board lesson ticks all its steps when the learner does what it asks.
#[test]
fn the_board_lesson_walks_from_schematic_to_the_order_window() {
    let mut app = App::default();
    assert!(app.sch.load_sample(BoardEditor::lesson_circuit()));
    app.board_ed.start_lesson();
    let progress = |a: &mut App| a.board_ed.lesson_progress();
    assert_eq!(progress(&mut app), 0);
    app.board_ed.import_from_schematic(&app.sch);
    assert_eq!(progress(&mut app), 1);
    app.board_ed.autoplace();
    assert_eq!(progress(&mut app), 2);
    app.board_ed.autoroute_now(None);
    assert_eq!(progress(&mut app), 3, "{}", app.board_ed.status);
    app.board_ed.run_drc();
    assert_eq!(progress(&mut app), 4, "{}", app.board_ed.status);
    app.board_ed.open_order();
    assert_eq!(progress(&mut app), 5);
}

/// The copper lesson ticks its four steps in order, and finishing it is recorded under its own key.
#[test]
fn the_copper_lesson_walks_from_the_drop_to_the_fixed_copper() {
    let mut app = App::default();
    app.start_board_lesson(1);
    assert_eq!(app.board_ed.board.name, "thin_power_line");
    let progress = |a: &mut App| a.board_ed.lesson_progress();
    assert_eq!(progress(&mut app), 0, "the sample starts without declared currents");
    app.apply_post_layout(true);
    assert_eq!(progress(&mut app), 1);
    let currents = app.sch.dc_net_currents();
    app.board_ed.use_simulated_currents(currents, vec![]);
    assert_eq!(progress(&mut app), 2);
    for net in ["VIN", "GND"] {
        app.board_ed.widen_net_to_need(net);
    }
    assert_eq!(progress(&mut app), 3, "{}", app.board_ed.status);
    assert!(app.board_ed.finished_board_lesson().is_none());
    app.apply_post_layout(true);
    assert_eq!(progress(&mut app), 4);
    assert_eq!(app.board_ed.finished_board_lesson(), Some("board_copper"));
}

/// The pre-order check challenge: the board starts with a part outside, two nets unrouted, thin copper for the declared current and
/// rules looser than the fab's; each fix ticks the next step, and the end state is a check without error or warning.
#[test]
fn the_order_challenge_ticks_off_as_the_faults_are_fixed() {
    use pcb_core::fab::Level;
    let mut app = App::default();
    app.start_board_lesson(2);
    assert_eq!(app.board_ed.board.name, "order_fix");
    let progress = |a: &mut App| {
        for _ in 0..6 {
            a.board_ed.lesson_progress();
        }
        a.board_ed.lesson_progress()
    };
    assert_eq!(progress(&mut app), 0);
    app.board_ed.open_order();
    assert_eq!(progress(&mut app), 1, "the order window is open");
    let bad: Vec<Level> = app.board_ed.order_checks().iter().map(|c| c.level).collect();
    assert!(bad.contains(&Level::Error) && bad.contains(&Level::Warn));
    app.board_ed.board.place_part("D1", pcb_core::Point::new(14.5, 14.0), 0);
    assert_eq!(progress(&mut app), 2, "D1 is back on the board");
    app.board_ed.autoroute_now(None);
    assert_eq!(progress(&mut app), 3, "{}", app.board_ed.status);
    for net in ["N$1", "GND"] {
        app.board_ed.widen_net_to_need(net);
    }
    assert_eq!(progress(&mut app), 4, "{}", app.board_ed.status);
    assert!(app.board_ed.finished_board_lesson().is_none());
    app.board_ed.choose_fab_for_test("jlcpcb");
    assert_eq!(progress(&mut app), 5, "{:?}", app.board_ed.order_checks().iter().filter(|c| c.level != Level::Ok && c.level != Level::Info).map(|c| (&c.title, &c.detail)).collect::<Vec<_>>());
    assert_eq!(app.board_ed.finished_board_lesson(), Some("board_fix"));
}

/// The ringing-trace sample (docs/examples/ringing_trace.ccproj): a 3.3 V edge through 22 ohm into a load at the end of a
/// 90 mm trace over a ground pour. Ideal wiring gives a clean step; the copper's inductance and capacitance make the load pin ring.
#[test]
fn ringing_trace_project_overshoots_only_with_the_copper() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.load_project(root.join("ringing_trace.ccproj"));
    assert!(pcb_core::drc::check(&app.board_ed.board).is_empty());
    app.sch.set_tran_settings("100n", "0.1n");
    app.sch.analyze_tran();
    let ideal = app.sch.tran_signal_max("V(SIG)").unwrap();
    assert!((3.2..3.4).contains(&ideal), "ideal wiring: {ideal}");
    app.apply_post_layout(true);
    assert!(app.sch.wiring.iter().any(|w| w.refdes == "R2" && w.pin == "1" && w.farad > 3e-12 && w.henry > 50e-9), "{:?}", app.sch.wiring);
    app.sch.analyze_tran();
    let ring = app.sch.tran_signal_max("V(R2.1.wire)").unwrap();
    assert!(ring > 1.4 * 3.3, "the load pin should overshoot: {ring}");
}

/// A pulsed load has no DC current; the width panel's transient button takes the RMS of the settled part of the run.
#[test]
fn transient_currents_come_from_the_last_run_and_flag_a_surge() {
    let mut app = App::default();
    app.start_board_lesson(1);
    assert!(app.sch.tran_net_currents().is_err(), "no transient run yet");
    app.sch.sch.components.iter_mut().find(|c| c.refdes == "V1").unwrap().value = "PULSE(0 5 0 1n 1n 1m 2m)".into();
    app.sch.set_tran_settings("40m", "20u");
    app.sch.analyze_tran();
    let (currents, stats) = app.sch.tran_net_currents().unwrap();
    let vin = stats.iter().find(|s| s.net == "VIN").unwrap();
    // 5 V into 2.2 ohm for half of the time: peak 2.27 A, RMS 1.61 A
    assert!((vin.peak - 2.27).abs() < 0.05 && (vin.rms - 1.61).abs() < 0.08 && !vin.spiky(), "{vin:?}");
    app.board_ed.use_simulated_currents(Ok(currents), stats);
    assert!((app.board_ed.board.net_currents["VIN"] - 1.61).abs() < 0.08);
    assert!(!app.board_ed.current_stats.is_empty());
}

/// End-to-end trial: schematic -> board -> autoplace -> autoroute -> GND pour -> DRC -> preflight -> fab files.
/// `CIRCUITCAD_TRIAL=power_supply cargo test -p app fab_trial -- --ignored --nocapture`
/// Writes to `target/fab_trial/<name>/`.
#[test]
#[ignore]
fn fab_trial() {
    let name = std::env::var("CIRCUITCAD_TRIAL").unwrap_or("power_supply".into());
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut app = App::default();
    app.sch.load_path(root.join(format!("docs/examples/{name}.schjson")));
    app.board_ed.import_from_schematic(&app.sch);
    if name == "power_supply" && std::env::var("CIRCUITCAD_HAND").is_ok() {
        // a hand layout: input on the left, diode, regulator, capacitors beside the regulator pins
        let b = &mut app.board_ed.board;
        b.set_rect_outline(0.0, 0.0, 36.0, 20.0);
        for (r, x, y, rot) in [("V1", 4.0, 10.0, 0), ("D1", 14.0, 15.0, 0), ("U1", 23.0, 9.0, 0), ("C1", 17.0, 5.0, 0), ("C2", 29.0, 5.0, 0)] {
            b.place_part(r, Point::new(x, y), rot);
        }
    } else {
        app.board_ed.autoplace();
    }
    println!("{}", app.board_ed.status);
    if std::env::var("CIRCUITCAD_CURRENTS").is_ok() {
        for n in ["VPROT", "VOUT", "GND"] {
            app.board_ed.board.net_currents.insert(n.into(), 1.0);
        }
    }
    app.board_ed.autoroute_now(None);
    println!("{}", app.board_ed.status);
    let b = &mut app.board_ed.board;
    b.name = name.clone();
    let conn = pcb_core::drc::connectivity(b);
    for w in pcb_core::current::net_widths(b, &conn, &Default::default()) {
        println!("  WIDTH {} needs {:.2} mm, thinnest drawn {:?}", w.net, w.needed, w.thinnest);
    }
    let (w, h) = (b.outline.iter().map(|p| p.x).fold(0.0, f64::max), b.outline.iter().map(|p| p.y).fold(0.0, f64::max));
    let poly = vec![Point::new(0.5, 0.5), Point::new(w - 0.5, 0.5), Point::new(w - 0.5, h - 0.5), Point::new(0.5, h - 0.5)];
    let mut z = pcb_core::Zone::new("GND", Side::Bottom, poly);
    z.remove_islands = true;
    b.zones.push(z);
    let viol = pcb_core::drc::check(&app.board_ed.board);
    println!("DRC {} violations", viol.len());
    for v in &viol {
        println!("  DRC ({:.1},{:.1}) {}", v.at.x, v.at.y, v.message);
    }
    for c in pcb_core::fab::preflight(&app.board_ed.board, pcb_core::fab::PRESETS.first()) {
        println!("  PRE {:?} {}", c.level, c.title);
    }
    let out = root.join("target/fab_trial").join(&name);
    std::fs::create_dir_all(&out).unwrap();
    println!("{:?}", app.board_ed.write_fab_files(&out));
    std::fs::write(out.join(format!("{name}.pcbjson")), serde_json::to_string_pretty(&app.board_ed.board).unwrap()).unwrap();
}

#[test]
fn autoroute_uses_the_width_a_declared_net_current_needs() {
    use pcb_core::{Pad, PadShape};
    let mut app = App::default();
    let b = &mut app.board_ed.board;
    *b = pcb_core::Board::new("w", 30.0, 20.0);
    for (x, net) in [(5.0, "P"), (25.0, "P"), (5.0, "S"), (25.0, "S")] {
        let mut p = Pad::new(Point::new(x, if net == "P" { 6.0 } else { 14.0 }), PadShape::Rect { w: 1.5, h: 1.5 }, None, Side::Top);
        p.net = net.into();
        b.pads.push(p);
    }
    b.net_currents.insert("P".into(), 2.0);
    let need = pcb_core::current::round_up_width(pcb_core::current::width_for_current(2.0, &Default::default()));
    app.board_ed.autoroute_now(None);
    let b = &app.board_ed.board;
    let conn = pcb_core::drc::connectivity(b);
    for (i, t) in b.tracks.iter().enumerate() {
        let expect = if conn.track_net(i) == Some("P") { need } else { 0.25 };
        assert!((t.width - expect).abs() < 1e-9, "track {i} is {} mm, expected {expect}", t.width);
    }
    assert!(need > 0.25 && b.tracks.len() >= 2);
    assert!(pcb_core::drc::check(b).is_empty());
}

/// A fresh app has nothing to save; edits make the editors dirty and saving (or opening) makes them clean again.
#[test]
fn unsaved_changes_are_tracked() {
    let mut app = App::default();
    assert!(!app.dirty());
    // a schematic edit counts once it is committed (the frame loop does this)
    app.sch.sch.add(Kind::Resistor, (6, -6), 0);
    assert!(app.sch.dirty() && app.dirty());
    app.sch.mark_saved();
    assert!(!app.sch.dirty());
    // a board edit counts through `checkpoint`; importing is one
    app.board_ed.import_from_schematic(&app.sch);
    assert!(app.board_ed.dirty());
    app.board_ed.mark_saved();
    assert!(!app.dirty());
    // a sheet or board that was just opened is clean, whatever was there before
    app.sch.sch.add(Kind::Resistor, (12, -6), 0);
    app.sch.adopt(sch_core::Schematic::default(), None);
    app.board_ed.import_from_schematic(&app.sch);
    app.board_ed.adopt(pcb_core::Board::new("x", 20.0, 20.0), None);
    assert!(!app.dirty());
}

/// The order window's checks list a board that no longer matches the schematic (a value only warns, a new part blocks).
#[test]
fn the_order_checks_list_a_board_that_differs_from_the_schematic() {
    let mut app = App::default();
    assert!(app.sch.load_sample(board_editor::BoardEditor::lesson_circuit()));
    app.board_ed.import_from_schematic(&app.sch);
    let sync = |app: &mut App| {
        app.board_ed.note_schematic_diff(&app.sch);
        app.board_ed.open_order();
        app.board_ed.order_checks().iter().find(|c| c.title.contains("回路図と基板")).map(|c| c.level)
    };
    assert_eq!(sync(&mut app), None);
    let first = app.sch.sch.components.iter().position(|c| matches!(c.kind, Kind::Resistor)).unwrap();
    app.sch.sch.components[first].value = "123k".into();
    app.sch.commit_history_for_test();
    assert_eq!(sync(&mut app), Some(pcb_core::fab::Level::Warn));
    app.sch.sch.add(Kind::Resistor, (60, 60), 0);
    app.sch.commit_history_for_test();
    assert_eq!(sync(&mut app), Some(pcb_core::fab::Level::Error));
    app.board_ed.import_from_schematic(&app.sch);
    assert_eq!(sync(&mut app), None);
}

/// The board notices when the schematic changed after the import, and stops noticing after a new import.
#[test]
fn the_board_notices_a_changed_schematic() {
    let mut app = App::default();
    assert!(app.sch.load_sample(board_editor::BoardEditor::lesson_circuit()));
    assert!(app.board_ed.schematic_diff(&app.sch).is_empty(), "nothing imported yet: nothing to compare");
    app.board_ed.import_from_schematic(&app.sch);
    assert!(app.board_ed.schematic_diff(&app.sch).is_empty(), "{:?}", app.board_ed.schematic_diff(&app.sch));
    // a value changed
    let first = app.sch.sch.components.iter().position(|c| matches!(c.kind, Kind::Resistor)).unwrap();
    let name = app.sch.sch.components[first].refdes.clone();
    app.sch.sch.components[first].value = "123k".into();
    app.sch.commit_history_for_test();
    assert_eq!(app.board_ed.schematic_diff(&app.sch).changed, vec![name]);
    // a new part
    let n = app.sch.sch.add(Kind::Resistor, (60, 60), 0);
    app.sch.sch.components[n].refdes = "R99".into();
    app.sch.commit_history_for_test();
    assert!(app.board_ed.schematic_diff(&app.sch).added.contains(&"R99".to_string()));
    assert!(app.board_ed.schematic_diff(&app.sch).summary().contains("R99"));
    // importing again brings the board up to date
    app.board_ed.import_from_schematic(&app.sch);
    assert!(app.board_ed.schematic_diff(&app.sch).is_empty(), "{:?}", app.board_ed.schematic_diff(&app.sch));
}

/// The TP5100 sample is routed completely with the pad leads on, without the tiny diagonal steps that an off-grid pad
/// used to get between the end of its lead and the grid.
#[test]
fn the_tp5100_sample_routes_completely_with_clean_pad_leads() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.sch.load_path(root.join("tp5100_charger.schjson"));
    app.board_ed.import_from_schematic(&app.sch);
    app.board_ed.autoplace();
    app.board_ed.autoroute_now(None);
    let b = &app.board_ed.board;
    assert!(pcb_core::ratsnest::ratsnest(b).is_empty(), "{}", app.board_ed.status);
    assert!(pcb_core::drc::check(b).is_empty(), "{:?}", pcb_core::drc::check(b));
    let steps: Vec<_> = b.tracks.iter().filter(|t| t.a.dist(t.b) < 0.25 && (t.b.x - t.a.x).abs() > 1e-6 && (t.b.y - t.a.y).abs() > 1e-6).collect();
    assert!(steps.is_empty(), "{steps:?}");
    // every piece is horizontal, vertical or at 45 degrees (also where a pad has no room for a straight lead)
    let odd: Vec<_> = b.tracks.iter().filter(|t| { let (dx, dy) = ((t.b.x - t.a.x).abs(), (t.b.y - t.a.y).abs()); dx > 1e-6 && dy > 1e-6 && (dx - dy).abs() > 1e-6 }).collect();
    assert!(odd.is_empty(), "{odd:?}");
    // a big pad (the IC's exposed pad) is entered where the route meets it, not from its centre with a bend inside the copper
    for p in b.pads.iter().filter(|p| { let (w, h) = p.shape.size(); w >= 2.0 && h >= 2.0 }) {
        let at_centre: Vec<_> = b.tracks.iter().filter(|t| t.a == p.pos || t.b == p.pos).collect();
        assert!(at_centre.is_empty(), "{} {}: {at_centre:?}", p.refdes, p.pin);
    }
}

/// The GND pour on the TP5100 sample: the ground needs only the short pieces to its vias, the rest routes as usual.
#[test]
fn the_ground_pour_takes_the_ground_wires_off_the_tp5100_sample() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.sch.load_path(root.join("tp5100_charger.schjson"));
    app.board_ed.import_from_schematic(&app.sch);
    app.board_ed.autoplace();
    app.board_ed.ground_pour();
    assert!(app.board_ed.status.starts_with("GND ベタ"), "{}", app.board_ed.status);
    app.board_ed.autoroute_now(None);
    let b = &app.board_ed.board;
    assert!(pcb_core::ratsnest::ratsnest(b).is_empty(), "{}", app.board_ed.status);
    let c = b.clone();
    assert!(pcb_core::drc::check_final(&c).is_empty(), "{:?}", pcb_core::drc::check_final(&c));
    let conn = pcb_core::drc::connectivity(b);
    let gnd: f64 = b.tracks.iter().enumerate().filter(|(i, _)| conn.track_net(*i) == Some("GND")).map(|(_, t)| t.a.dist(t.b)).sum();
    assert!(gnd < 20.0, "{gnd} mm of ground wire");
    // one undo takes the pour and its vias back (the routing was a step of its own)
    app.board_ed.undo();
    app.board_ed.undo();
    assert!(app.board_ed.board.zones.is_empty() && app.board_ed.board.vias.is_empty());
}

/// The thin-power-line sample (docs/examples/thin_power_line.ccproj): the copper eats about 0.4 V of the 5 V,
/// the simulated current says the tracks are too thin, and widening them brings the load back near 5 V.
#[test]
fn thin_power_line_project_shows_the_copper_drop_and_the_fix() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.load_project(root.join("thin_power_line.ccproj"));
    assert!(pcb_core::drc::check(&app.board_ed.board).is_empty());
    let load = |app: &mut App| {
        app.sch.analyze_dc();
        app.sch.pin_voltage("R1", "1").unwrap() - app.sch.pin_voltage("R1", "2").unwrap()
    };
    let ideal = load(&mut app);
    assert!((ideal - 5.0).abs() < 1e-3, "{ideal}");

    app.apply_post_layout(true);
    let thin = load(&mut app);
    assert!((4.3..4.8).contains(&thin), "thin copper: {thin}");

    let currents = app.sch.dc_net_currents();
    app.board_ed.use_simulated_currents(currents, vec![]);
    let conn = pcb_core::drc::connectivity(&app.board_ed.board);
    let report = pcb_core::current::net_widths(&app.board_ed.board, &conn, &pcb_core::current::CopperSpec::default());
    assert!(report.iter().all(|w| w.too_thin()), "{report:?}");
    for net in ["VIN", "GND"] {
        app.board_ed.widen_net_to_need(net);
    }
    app.apply_post_layout(true);
    let wide = load(&mut app);
    assert!(wide > 4.9 && wide > thin + 0.3, "widened: {wide}");
}

/// Opening the window of a net with no current asks the schematic's DC analysis for that net alone and declares it (the others are left
/// as they are); the window that lists all the currents draws.
#[test]
fn a_net_window_fetches_the_dc_current_of_its_net_and_the_list_window_draws() {
    let mut app = App::default();
    app.start_board_lesson(1);
    let b = &mut app.board_ed;
    b.board.net_currents.insert("GND".into(), 0.123);
    b.open_net_window("VIN");
    let ctx = egui::Context::default();
    let frame = |app: &mut App| {
        let mut input = egui::RawInput::default();
        input.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0)));
        let mut out = ctx.run_ui(input, |ui| {
            app.board_ed.net_window(ui.ctx());
            app.board_ed.current_window(ui.ctx());
        });
        out.textures_delta.clear();
    };
    // the DC currents are at hand (the app says so each frame): the window asks for them at once
    app.board_ed.dc_currents_ready = true;
    frame(&mut app);
    // what App does with the request
    let net = app.board_ed.net_current_request.take().expect("the window asked for the DC current of VIN");
    assert_eq!(net, "VIN");
    let currents = app.sch.dc_net_currents();
    app.board_ed.fill_net_current(&net, currents);
    assert!(app.board_ed.board.net_currents["VIN"] > 0.0, "the DC current of VIN is declared");
    assert_eq!(app.board_ed.board.net_currents["GND"], 0.123, "the other nets keep what they had");
    // asked once: another frame does not ask again
    frame(&mut app);
    assert!(app.board_ed.net_current_request.is_none());
    app.board_ed.current_win_open = true;
    frame(&mut app);
    assert!(app.board_ed.current_win_open);
}
