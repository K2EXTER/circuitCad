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
    assert!(app.sch.status.contains("収束"), "{}", app.sch.status);
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
    app.board_ed.autoroute(None);
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
        app.board_ed.autoroute(None);
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
    app.board_ed.autoroute(None);
    assert_eq!(progress(&mut app), 3, "{}", app.board_ed.status);
    app.board_ed.run_drc();
    assert_eq!(progress(&mut app), 4, "{}", app.board_ed.status);
    app.board_ed.open_order();
    assert_eq!(progress(&mut app), 5);
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
    app.board_ed.autoroute(None);
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
    app.board_ed.autoroute(None);
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
    app.board_ed.autoroute(None);
    let b = &app.board_ed.board;
    assert!(pcb_core::ratsnest::ratsnest(b).is_empty(), "{}", app.board_ed.status);
    assert!(pcb_core::drc::check(b).is_empty(), "{:?}", pcb_core::drc::check(b));
    let steps: Vec<_> = b.tracks.iter().filter(|t| t.a.dist(t.b) < 0.25 && (t.b.x - t.a.x).abs() > 1e-6 && (t.b.y - t.a.y).abs() > 1e-6).collect();
    assert!(steps.is_empty(), "{steps:?}");
}
