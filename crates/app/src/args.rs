use super::*;

/// Command line: `circuitCad [file.schjson | file.pcbjson | file.ccproj] [--dc] [--ac f0 f1 ppd] [--sweep src start stop step] [--param part from to count node [tran]] [--fault part kind [tran|ac node|chain] | scan] [--mc node [ac]] [--noise out in|- tempC] [--z net ref|- remove,list|- [f0 f1 ppd]] [--loop part [net|- [f0 f1 ppd]]] [--tran stop step [--spectrum]] [--post-layout [--schematic]]
/// [--drc] [--round R] [--finish] [--select-track N] [--select-board text:N|label:R1|silk:N|R1] [--3d] [--order] [--guide [schematic|waveform|board|3d]] [--learn [path id]] [--lesson <id>] [--board-lesson 0..7 [--repair seed]] [--select <refdes>] [--new-part] [--form-step file.step] [--form-pdf file.pdf [--form-find text] [--form-pick field]] [--challenge <id> [0|1] [--judge]] [--export dir] [--screenshot file.png] [--svg file.svg] [--pdf file.pdf]`. Mostly useful for scripts and for checking a build.
pub(super) fn apply_args(app: &mut App, args: &[String]) {
    if let Some(i) = args.iter().position(|a| a == "--lesson")
        && let Some(id) = args.get(i + 1)
        && let Some(n) = schematic::lesson_ids().iter().position(|l| l == id) {
            app.sch.start_lesson(n);
            // `--lesson repair_zener --probe VOUT,VIN`: readings of the virtual meter (the repair lessons)
            if let Some(j) = args.iter().position(|a| a == "--probe")
                && let Some(nets) = args.get(j + 1) {
                    app.sch.probe_nets(nets);
                }
            // `--lesson <id> --dc`: the first analysis is done already (to see the second step of a lesson)
            if args.iter().any(|a| a == "--dc") {
                app.sch.analyze_dc();
            }
        }
    if let Some(i) = args.iter().position(|a| a == "--board-lesson") {
        // --board-lesson [0|1]: 0 = schematic to order files, 1 = copper width and voltage drop
        app.start_board_lesson(args.get(i + 1).and_then(|n| n.parse().ok()).unwrap_or(0));
    }
    if let Some(i) = args.iter().position(|a| a == "--repair") {
        // --board-lesson 7 --repair [seed]: put a fault into the board of the repair lesson (the seed picks the kind: 0..4 are the five kinds)
        app.board_ed.repair_inject(args.get(i + 1).and_then(|n| n.parse().ok()).unwrap_or(1));
    }
    if let Some(i) = args.iter().position(|a| a == "--challenge")
        && let Some(id) = args.get(i + 1) {
            app.sch.start_challenge(id, args.get(i + 2).and_then(|l| l.parse().ok()).unwrap_or(0));
            if args.iter().any(|a| a == "--judge") {
                app.sch.run_judge();
            }
        }
    if let Some(i) = args.iter().position(|a| a == "--new-part") {
        // --new-part [ic|transistor|diode|chip (= ceramic)|ceramic|electrolytic|tantalum|film|resistor|inductor|connector|connector2 (2-row header)]
        app.sch.open_part_form_for(args.get(i + 1).filter(|a| !a.starts_with("--")).map_or("", |s| s.as_str()));
    }
    if let Some(i) = args.iter().position(|a| a == "--form-tab")
        && let Some(t) = args.get(i + 1) {
            app.sch.set_form_tab(t);
        }
    if let Some(i) = args.iter().position(|a| a == "--form-step")
        && let Some(f) = args.get(i + 1) {
            app.sch.load_step_into_form(std::path::Path::new(f));
        }
    if let Some(i) = args.iter().position(|a| a == "--form-pdf")
        && let Some(f) = args.get(i + 1) {
            app.sch.load_pdf_into_form(std::path::Path::new(f));
        }
    if let Some(i) = args.iter().position(|a| a == "--form-find")
        && let Some(t) = args.get(i + 1) {
            app.sch.find_in_form_pdf(t);
        }
    if let Some(i) = args.iter().position(|a| a == "--form-pick")
        && let Some(t) = args.get(i + 1) {
            app.sch.pick_in_form_pdf(t);
        }
    if args.iter().any(|a| a == "--measure-guide") {
        app.measure.open = true;
    }
    if let Some(i) = args.iter().position(|a| a == "--glossary") {
        // --glossary [term]: the glossary window, on a term
        app.glossary.open_at(args.get(i + 1).filter(|a| !a.starts_with("--")).map_or("", |s| s.as_str()));
    }
    if args.iter().any(|a| a == "--icons-only") {
        crate::icons::set_icon_only(true);
    }
    if let Some(i) = args.iter().position(|a| a == "--learn") {
        app.learn.open = true;
        // `--learn arduino_board` opens that learning path
        if let Some(id) = args.get(i + 1).filter(|a| !a.starts_with("--")) {
            app.learn.select(id);
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--quick") {
        // open the quick-add box (key A) with a word already typed, for screenshots
        app.sch.open_quick_add_with(args.get(i + 1).filter(|a| !a.starts_with("--")).map_or("", |s| s.as_str()));
    }
    if let Some(i) = args.iter().position(|a| a == "--lib-part") {
        // open the library list on a part (a series' name with a value: "ERJ-3EKF 4.8k"), for screenshots
        let arg = |k: usize| args.get(i + k).filter(|a| !a.starts_with("--")).map_or("", |s| s.as_str());
        app.sch.open_library_part(arg(1), arg(2));
    }
    if let Some(i) = args.iter().position(|a| a == "--lib-search") {
        // the library list's search box, for screenshots
        app.sch.set_library_search(args.get(i + 1).filter(|a| !a.starts_with("--")).map_or("", |s| s.as_str()));
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
        // `--select R1` also picks the part of a project's schematic (the board and the 3D view show it: the linked pick)
        if let Some(i) = args.iter().position(|a| a == "--select")
            && let Some(r) = args.get(i + 1) {
                app.sch.select_part(r);
            }
        // a project has both halves: the board's copper can go into the schematic's transient run (`--post-layout --tran stop step`)
        if args.iter().any(|a| a == "--post-layout") {
            app.apply_post_layout(true);
        }
        if let Some(i) = args.iter().position(|a| a == "--tran")
            && let (Some(stop), Some(step)) = (args.get(i + 1), args.get(i + 2)) {
                app.sch.set_tran_settings(stop, step);
                app.sch.analyze_tran();
                if let Some(j) = args.iter().position(|a| a == "--signals")
                    && let Some(n) = args.get(j + 1) {
                        app.sch.set_tran_signals(n);
                    }
            }
        if ["--drc", "--finish", "--3d", "--order", "--export", "--ir", "--select-track", "--net-window", "--nets"].iter().any(|f| args.iter().any(|a| a == f)) {
            app.mode = Mode::Board;
            apply_board_args(app, args);
        }
    }
    if let Some(p) = args.iter().find(|a| a.to_ascii_lowercase().ends_with(".asc")) {
        app.sch.load_ltspice(p.into(), false);
        // the analysis the sheet asked for (`.tran` / `.ac` filled the boxes)
        if args.iter().any(|a| a == "--dc") {
            app.sch.analyze_dc();
        }
        if args.iter().any(|a| a == "--ac") {
            app.sch.analyze_ac();
        }
        if args.iter().any(|a| a == "--tran") {
            app.sch.analyze_tran();
        }
    }
    if let Some(p) = args.iter().find(|a| a.ends_with(".kicad_sch")) {
        app.sch.load_kicad_sch(p.into(), false);
    }
    if let Some(p) = args.iter().find(|a| a.ends_with(".schjson")) {
        app.sch.load_path(p.into());
        // `--sheet N`: the Nth sheet (from 1) is the one shown
        if let Some(i) = args.iter().position(|a| a == "--sheet")
            && let Some(n) = args.get(i + 1).and_then(|s| s.parse::<usize>().ok()).filter(|n| *n >= 1) {
                app.sch.show_sheet(n - 1);
            }
        if args.iter().any(|a| a == "--dc") {
            app.sch.analyze_dc();
        }
        if args.iter().any(|a| a == "--analyze") {
            app.sch.show_analysis_tab();
        }
        // --section <key>: the analysis tab with that section open (dc, variation, coverage ...)
        if let Some(i) = args.iter().position(|a| a == "--section")
            && let Some(k) = args.get(i + 1)
            && !app.sch.open_analysis_section(k)
        {
            eprintln!("--section {k}: no such section of the analysis tab");
        }
        if args.iter().any(|a| a == "--requirements") {
            app.sch.open_requirements();
            if args.iter().any(|a| a == "--dc") {
                app.sch.requirements_from_dc();
            }
        }
        if let Some(i) = args.iter().position(|a| a == "--select")
            && let Some(r) = args.get(i + 1) {
                app.sch.select_part(r);
                // selecting goes to the drawing tab; `--analyze` asks for the analysis tab with the part selected
                if args.iter().any(|a| a == "--analyze") {
                    app.sch.show_analysis_tab();
                }
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
                // the scan on its own thread (the screenshot is taken while it runs)
                Some("scan-bg") => app.sch.start_fault_scan_bg(),
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
                    // a chain follows a transient run: the --tran time when given (it is read later for the waveform)
                    if let Some(t) = args.iter().position(|a| a == "--tran")
                        && let (Some(stop), Some(step)) = (args.get(t + 1), args.get(t + 2)) {
                            app.sch.set_tran_settings(stop, step);
                        }
                    if chain {
                        app.sch.analyze_fault_chain();
                    } else {
                        app.sch.analyze_fault();
                    }
                    // the result is in the section: show it
                    app.sch.show_analysis_section("fault");
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
        if let Some(i) = args.iter().position(|a| a == "--loop")
            && let Some(part) = args.get(i + 1) {
                // --loop <part> [output-side net]  (the loop gain; uses the --ac frequency range, or the defaults)
                if let (Some(f0), Some(f1), Some(p)) = (args.get(i + 3), args.get(i + 4), args.get(i + 5))
                    && !f0.starts_with("--") {
                    app.sch.set_ac_settings(f0, f1, p);
                }
                app.sch.set_loop_settings(part, args.get(i + 2).filter(|a| !a.starts_with("--") && *a != "-").map_or("", |a| a.as_str()));
                app.sch.analyze_loop();
            }
        if let Some(i) = args.iter().position(|a| a == "--z") {
            // --z <net> <reference net or -> <parts to take out, comma separated, or -> (uses the --ac frequency range)
            if let (Some(node), Some(rf), Some(rm)) = (args.get(i + 1), args.get(i + 2), args.get(i + 3)) {
                let dash = |s: &String| if s == "-" { String::new() } else { s.clone() };
                if let (Some(f0), Some(f1), Some(p)) = (args.get(i + 4), args.get(i + 5), args.get(i + 6))
                    && !f0.starts_with("--") {
                    app.sch.set_ac_settings(f0, f1, p);
                }
                app.sch.set_impedance_settings(node, &dash(rf), &dash(rm));
                app.sch.analyze_impedance();
            }
        }
        if let Some(i) = args.iter().position(|a| a == "--pz")
            && let Some(out) = args.get(i + 1) {
                // --pz <output net> <input net or -> [f0 f1 ppd]  (poles and zeros; uses the given frequency range, or the defaults)
                if let (Some(f0), Some(f1), Some(p)) = (args.get(i + 3), args.get(i + 4), args.get(i + 5))
                    && !f0.starts_with("--") {
                    app.sch.set_ac_settings(f0, f1, p);
                }
                app.sch.set_pz_settings(out, args.get(i + 2).filter(|a| !a.starts_with("--") && *a != "-").map_or("", |a| a.as_str()));
                app.sch.analyze_pz();
            }
        if let Some(i) = args.iter().position(|a| a == "--mc")
            && let Some(node) = args.get(i + 1) {
                // --mc <node> [ac]  (the variation analysis; AC measures the -3 dB frequency and uses the --ac range)
                app.sch.analyze_variation(node, args.get(i + 2).is_some_and(|m| m == "ac"));
                // its settings next to the result (the tolerances and the box for the models' min / max)
                app.sch.show_analysis_section("variation");
            }
        if let Some(i) = args.iter().position(|a| a == "--psrr")
            && let (Some(src), Some(out)) = (args.get(i + 1), args.get(i + 2)) {
                // --psrr <supply source or -> <node>  (uses the --ac frequency range)
                app.sch.set_psrr_settings(if src == "-" { "" } else { src }, out);
                app.sch.analyze_psrr();
            }
        if let Some(i) = args.iter().position(|a| a == "--diagnose")
            && let Some(r) = args.get(i + 1) {
                // --diagnose NODE=V,NODE=V  (the readings; the faults are solved here, then matched)
                app.sch.set_diag_readings(r);
                app.sch.analyze_diagnosis();
                app.sch.show_analysis_section("diagnose");
            }
        if let Some(i) = args.iter().position(|a| a == "--sens")
            && let Some(node) = args.get(i + 1) {
                // --sens <node> [ac]  (sensitivity: every part moved by 1 %; AC measures the -3 dB frequency with the --ac range)
                app.sch.analyze_sensitivity(node, args.get(i + 2).is_some_and(|m| m == "ac"));
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
                if let Some(j) = args.iter().position(|a| a == "--signals")
                    && let Some(n) = args.get(j + 1) {
                        app.sch.set_tran_signals(n);
                    }
                if args.iter().any(|a| a == "--spectrum") {
                    app.sch.show_spectrum();
                }
            }
    }
    if let Some(p) = args.iter().find(|a| a.ends_with(".pcbjson")) {
        app.mode = Mode::Board;
        app.board_ed.load_path(p.into());
        apply_board_args(app, args);
        if args.iter().any(|a| a == "--post-layout") {
            // the board's copper resistance goes into the schematic's analyses; show the schematic (with --dc, solved again)
            app.apply_post_layout(true);
            if args.iter().any(|a| a == "--dc") {
                app.sch.analyze_dc();
            }
            if args.iter().any(|a| a == "--schematic") {
                app.mode = Mode::Schematic;
            }
        }
    }
    // `--pdf file.pdf`: the same sheet as a one-page PDF, then quit
    if let Some(i) = args.iter().position(|a| a == "--pdf")
        && let Some(p) = args.get(i + 1) {
            match app.sch.sheet_pdf() {
                Some(pdf) => match std::fs::write(p, pdf) {
                    Ok(()) => println!("PDF を保存しました: {p}"),
                    Err(e) => {
                        eprintln!("PDF を保存できませんでした: {e}。保存先のフォルダに書き込めるか確認してください");
                        std::process::exit(1);
                    }
                },
                None => {
                    eprintln!("回路図が空なので、PDF に書き出せません");
                    std::process::exit(1);
                }
            }
            std::process::exit(0);
        }
    // `--svg file.svg`: the shown sheet as an SVG picture (after the other options, so `--dc` figures are in it), then quit
    if let Some(i) = args.iter().position(|a| a == "--svg")
        && let Some(p) = args.get(i + 1) {
            match app.sch.sheet_svg() {
                Some(svg) => match std::fs::write(p, svg) {
                    Ok(()) => println!("SVG を保存しました: {p}"),
                    Err(e) => {
                        eprintln!("SVG を保存できませんでした: {e}。保存先のフォルダに書き込めるか確認してください");
                        std::process::exit(1);
                    }
                },
                None => {
                    eprintln!("回路図が空なので、SVG に書き出せません");
                    std::process::exit(1);
                }
            }
            std::process::exit(0);
        }
}

/// The options that act on the board (`--drc`, `--finish`, `--3d`, `--order` ...): after a `.pcbjson` is opened, and after a
/// `.ccproj` (a project with the board in it) when one of them is given.
fn apply_board_args(app: &mut App, args: &[String]) {
    if args.iter().any(|a| a == "--drc") {
        app.board_ed.run_drc();
    }
    if let Some(i) = args.iter().position(|a| a == "--net-window")
        && let Some(net) = args.get(i + 1) {
            // --net-window NET [TAB]: the net's settings window, on the tab 0 (width), 1 (class) or 2 (finish)
            app.board_ed.open_net_window(net);
            if let Some(t) = args.get(i + 2).and_then(|t| t.parse::<u8>().ok()) {
                app.board_ed.set_net_tab(t);
            }
        }
    if let Some(i) = args.iter().position(|a| a == "--select-track")
        && let Some(n) = args.get(i + 1).and_then(|n| n.parse::<usize>().ok()) {
            app.board_ed.select_track(n);
        }
    // --board-tool track | via | zone ...: the "手で描く" tab with that tool picked (its settings open)
    if let Some(i) = args.iter().position(|a| a == "--board-tool")
        && let Some(name) = args.get(i + 1)
        && !app.board_ed.pick_tool_named(name)
    {
        eprintln!("--board-tool {name}: no such tool (select, track, via, measure, zone, keepout, padth, padsmd, hole, rect, poly, silk, text)");
    }
    // --board-view-open: the "表示とロック" box over the board unfolded
    if args.iter().any(|a| a == "--board-view-open") {
        app.board_ed.open_view_box = true;
    }
    // --select-board text:N | label:R1 | silk:N | R1: something on the board picked (its settings in the panel)
    if let Some(i) = args.iter().position(|a| a == "--select-board")
        && let Some(w) = args.get(i + 1) {
            app.board_ed.select_named(w);
        }
    if let Some(i) = args.iter().position(|a| a == "--view")
        && let (Some(x), Some(y), Some(z)) = (args.get(i + 1).and_then(|v| v.parse::<f32>().ok()), args.get(i + 2).and_then(|v| v.parse::<f32>().ok()), args.get(i + 3).and_then(|v| v.parse::<f32>().ok()))
    {
        // --view <x mm> <y mm> <pixels per mm>: where to look (for a screenshot of a spot)
        app.board_ed.set_view(x, y, z);
    }
    if args.iter().any(|a| a == "--finish") {
        app.board_ed.set_finish(true);
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
        // --3d-net <net>: light that net's copper in the 3D view (for screenshots)
        if let Some(i) = args.iter().position(|a| a == "--3d-net")
            && let Some(n) = args.get(i + 1) {
                app.board_ed.set_view3d_net(n);
            }
    }
    if args.iter().any(|a| a == "--order") {
        app.board_ed.open_order();
    }
    // --nets: the window with the table of the nets
    if args.iter().any(|a| a == "--nets") {
        app.board_ed.net_table.open = true;
    }
    if let Some(i) = args.iter().position(|a| a == "--export")
        && let Some(dir) = args.get(i + 1) {
            app.board_ed.status = match app.board_ed.write_fab_files(std::path::Path::new(dir)) {
                Ok(n) => format!("書き出し完了: {dir} ({n} ファイル + zip)"),
                Err(e) => format!("書き出し失敗: {e}"),
            };
        }
}
