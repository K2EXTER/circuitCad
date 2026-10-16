use super::*;

/// Command line: `circuitCad [file.schjson | file.pcbjson] [--dc] [--ac f0 f1 ppd] [--sweep src start stop step] [--param part from to count node [tran]] [--fault part kind [tran|ac node|chain] | scan] [--noise out in|- tempC] [--tran stop step [--spectrum]]
/// [--drc] [--round R] [--finish] [--smooth] [--select-track N] [--3d] [--order] [--guide [schematic|waveform|board|3d]] [--learn] [--lesson <id>] [--select <refdes>] [--new-part] [--challenge <id> [0|1] [--judge]] [--export dir] [--screenshot file.png]`. Mostly useful for scripts and for checking a build.
pub(super) fn apply_args(app: &mut App, args: &[String]) {
    if let Some(i) = args.iter().position(|a| a == "--lesson")
        && let Some(id) = args.get(i + 1)
        && let Some(n) = schematic::lesson_ids().iter().position(|l| l == id) {
            app.sch.start_lesson(n);
        }
    if let Some(i) = args.iter().position(|a| a == "--challenge")
        && let Some(id) = args.get(i + 1) {
            app.sch.start_challenge(id, args.get(i + 2).and_then(|l| l.parse().ok()).unwrap_or(0));
            if args.iter().any(|a| a == "--judge") {
                app.sch.run_judge();
            }
        }
    if let Some(i) = args.iter().position(|a| a == "--new-part") {
        // --new-part [ic|transistor|diode|chip|connector]
        app.sch.open_part_form_for(args.get(i + 1).filter(|a| !a.starts_with("--")).map_or("", |s| s.as_str()));
    }
    if let Some(i) = args.iter().position(|a| a == "--form-tab")
        && let Some(t) = args.get(i + 1) {
            app.sch.set_form_tab(t);
        }
    if args.iter().any(|a| a == "--measure-guide") {
        app.measure.open = true;
    }
    if args.iter().any(|a| a == "--icons-only") {
        crate::icons::set_icon_only(true);
    }
    if args.iter().any(|a| a == "--learn") {
        app.learn.open = true;
    }
    if let Some(i) = args.iter().position(|a| a == "--quick") {
        // open the quick-add box (key A) with a word already typed, for screenshots
        app.sch.open_quick_add_with(args.get(i + 1).filter(|a| !a.starts_with("--")).map_or("", |s| s.as_str()));
    }
    if let Some(i) = args.iter().position(|a| a == "--guide") {
        app.help.show(match args.get(i + 1).map(|s| s.as_str()) {
            Some("waveform") => help::HelpTab::Waveform,
            Some("board") => help::HelpTab::Board,
            Some("3d") => help::HelpTab::View3d,
            _ => help::HelpTab::Schematic,
        });
    }
    if let Some(i) = args.iter().position(|a| a == "--screenshot")
        && let Some(p) = args.get(i + 1) {
            app.auto_shot = Some((p.into(), 40));
        }
    if let Some(p) = args.iter().find(|a| a.ends_with(".ccproj")) {
        app.load_project(p.into());
    }
    if let Some(p) = args.iter().find(|a| a.ends_with(".schjson")) {
        app.sch.load_path(p.into());
        if args.iter().any(|a| a == "--dc") {
            app.sch.analyze_dc();
        }
        if let Some(i) = args.iter().position(|a| a == "--select")
            && let Some(r) = args.get(i + 1) {
                app.sch.select_part(r);
            }
        if let Some(i) = args.iter().position(|a| a == "--ac")
            && let (Some(f0), Some(f1), Some(p)) = (args.get(i + 1), args.get(i + 2), args.get(i + 3)) {
                app.sch.set_ac_settings(f0, f1, p);
                app.sch.analyze_ac();
            }
        if let Some(i) = args.iter().position(|a| a == "--sweep") {
            // --sweep <source> <start> <stop> <step>
            if let (Some(src), Some(a), Some(b), Some(st)) = (args.get(i + 1), args.get(i + 2), args.get(i + 3), args.get(i + 4)) {
                app.sch.set_sweep_settings(src, a, b, st);
                app.sch.analyze_sweep();
            }
        }
        if let Some(i) = args.iter().position(|a| a == "--fault") {
            // --fault <part> <0 open | 1 short | 2 x10 | 3 /10>   or   --fault scan
            match args.get(i + 1).map(|s| s.as_str()) {
                Some("scan") => app.sch.analyze_fault_scan(),
                Some(part) => {
                    app.sch.set_fault(part, args.get(i + 2).and_then(|k| k.parse().ok()).unwrap_or(0));
                    let mut chain = false;
                    // ... <kind> tran <node>: compare the waveform at <node> instead of the DC voltages
                    // ... <kind> chain: follow the failures that come after (kind 7 / 8 / 9 = coil layer short 10 / 20 / 50%)
                    match args.get(i + 3).map(|m| m.as_str()) {
                        Some("chain") => chain = true,
                        Some("tran") => app.sch.set_fault_tran(args.get(i + 4).map_or("", |n| n.as_str())),
                        Some("ac") => app.sch.set_fault_ac(args.get(i + 4).map_or("", |n| n.as_str())),
                        _ => {}
                    }
                    if chain {
                        app.sch.analyze_fault_chain();
                    } else {
                        app.sch.analyze_fault();
                    }
                }
                None => {}
            }
        }
        for (i, _) in args.iter().enumerate().filter(|(_, a)| *a == "--spec") {
            // --spec <kind index 0..8> <target> <lo|-> <hi|-> [frequency]  (repeatable; put it after --dc / --tran / --ac)
            if let (Some(k), Some(t), Some(lo), Some(hi)) = (args.get(i + 1).and_then(|k| k.parse().ok()), args.get(i + 2), args.get(i + 3), args.get(i + 4)) {
                app.sch.add_spec(k, t, lo, hi, args.get(i + 5).filter(|a| !a.starts_with("--")).map_or("", |a| a.as_str()));
            }
        }
        for (i, _) in args.iter().enumerate().filter(|(_, a)| *a == "--measured") {
            // --measured <spec row index> <text>  (repeatable; after the --spec rows): a typed bench measurement, for screenshots
            if let (Some(row), Some(text)) = (args.get(i + 1).and_then(|k| k.parse().ok()), args.get(i + 2)) {
                app.sch.set_spec_measured(row, text);
            }
        }
        if let Some(i) = args.iter().position(|a| a == "--param") {
            // --param <part> <from> <to> <count> <node> [tran]  (AC unless "tran"; uses the --ac / --tran settings)
            if let (Some(part), Some(a), Some(b), Some(n), Some(node)) = (args.get(i + 1), args.get(i + 2), args.get(i + 3), args.get(i + 4), args.get(i + 5)) {
                app.sch.set_param_settings(part, a, b, n, node, args.get(i + 6).is_none_or(|m| m != "tran"));
                app.sch.analyze_param();
            }
        }
        if let Some(i) = args.iter().position(|a| a == "--noise") {
            // --noise <out node> <input source or -> <temp C> (uses the --ac frequency range)
            if let (Some(out), Some(inp), Some(t)) = (args.get(i + 1), args.get(i + 2), args.get(i + 3)) {
                let inp = if inp == "-" { "" } else { inp.as_str() };
                app.sch.set_noise_settings(out, inp, t);
                app.sch.analyze_noise();
            }
        }
        if let Some(i) = args.iter().position(|a| a == "--tran")
            && let (Some(stop), Some(step)) = (args.get(i + 1), args.get(i + 2)) {
                app.sch.set_tran_settings(stop, step);
                app.sch.analyze_tran();
                if let Some(j) = args.iter().position(|a| a == "--power")
                    && let Some(n) = args.get(j + 1..j + 5) {
                        app.sch.set_power_signals([n[0].clone(), n[1].clone(), n[2].clone(), n[3].clone()]);
                    }
                if args.iter().any(|a| a == "--spectrum") {
                    app.sch.show_spectrum();
                }
            }
    }
    if let Some(p) = args.iter().find(|a| a.ends_with(".pcbjson")) {
        app.mode = Mode::Board;
        app.board_ed.load_path(p.into());
        if args.iter().any(|a| a == "--drc") {
            app.board_ed.run_drc();
        }
        if let Some(i) = args.iter().position(|a| a == "--select-track")
            && let Some(n) = args.get(i + 1).and_then(|n| n.parse::<usize>().ok()) {
                app.board_ed.select_track(n);
            }
        if args.iter().any(|a| a == "--finish") {
            app.board_ed.set_finish(true);
        }
        if args.iter().any(|a| a == "--smooth") {
            app.board_ed.smooth_tracks();
        }
        if args.iter().any(|a| a == "--autoplace") {
            app.board_ed.autoplace();
        }
        if let Some(i) = args.iter().position(|a| a == "--round")
            && let Some(r) = args.get(i + 1).and_then(|r| r.parse::<f64>().ok()) {
                app.board_ed.round_outline(r);
            }
        for (i, _) in args.iter().enumerate().filter(|(_, a)| *a == "--ir") {
            // --ir <net> <amps>  (repeatable): declare the net's current and show the voltage drop of its copper
            if let (Some(net), Some(a)) = (args.get(i + 1), args.get(i + 2).and_then(|a| a.parse::<f64>().ok())) {
                app.board_ed.show_ir_drop(net, a);
            }
        }
        if args.iter().any(|a| a == "--3d") {
            app.board_ed.open_view3d();
        }
        if args.iter().any(|a| a == "--order") {
            app.board_ed.open_order();
        }
        if let Some(i) = args.iter().position(|a| a == "--export")
            && let Some(dir) = args.get(i + 1) {
                app.board_ed.status = match app.board_ed.write_fab_files(std::path::Path::new(dir)) {
                    Ok(n) => format!("書き出し完了: {dir} ({n} ファイル + zip)"),
                    Err(e) => format!("書き出し失敗: {e}"),
                };
            }
    }
}
