//! `file.schjson` and its analyses (`--dc`, `--ac`, `--tran`, `--fault`, `--loop` ...), and the imported sheets (`.asc`, `.kicad_sch`).
//! The analyses run in the order below; the ones that use the AC range read it from `--ac` (or their own `f0 f1 ppd`).

use super::*;

/// `file.asc` (LTspice), `--time-log FILE.csv`, `file.kicad_sch`.
pub(super) fn imports(app: &mut App, a: &Args) {
    if let Some(p) = a.0.iter().find(|w| w.to_ascii_lowercase().ends_with(".asc")) {
        app.sch.load_ltspice(p.into(), false);
        tidy(app, a);
        // the analysis the sheet asked for (`.tran` / `.ac` filled the boxes)
        if a.has("--dc") {
            app.sch.analyze_dc();
        }
        if a.has("--ac") {
            app.sch.analyze_ac();
        }
        if a.has("--tran") {
            app.sch.analyze_tran();
        }
    }
    // --time-log FILE.csv: the window of a meter's log over time, with that log
    if let Some(f) = a.val("--time-log") {
        match std::fs::read_to_string(f).map_err(|e| e.to_string()).and_then(|t| app.sch.add_time_log("log", &t)) {
            Ok(m) => app.sch.status = m,
            Err(e) => eprintln!("--time-log {f}: {e}"),
        }
    }
    if let Some(p) = a.file(".kicad_sch") {
        app.sch.load_kicad_sch(p.into(), false);
        tidy(app, a);
    }
}

/// `--tidy`: the sheet tidied (`sch_core::tidy`).
fn tidy(app: &mut App, a: &Args) {
    if a.has("--tidy") {
        app.sch.run_act(crate::actions::Act::Tidy);
    }
}

/// `--section <key>`: the analysis tab with that section open (dc, variation, coverage ...).
pub(super) fn section(app: &mut App, a: &Args) {
    if let Some(k) = a.val("--section")
        && !app.sch.open_analysis_section(k)
    {
        eprintln!("--section {k}: no such section of the analysis tab");
    }
}

/// `--tran stop step` with what goes with it: `--power A B C D`, `--signals N,N`, `--tran-tab N`, `--spectrum`, `--digital`.
pub(super) fn tran(app: &mut App, a: &Args) {
    let Some([stop, step]) = a.vals("--tran") else { return };
    app.sch.set_tran_settings(stop, step);
    app.sch.analyze_tran();
    if let Some(n) = a.vals::<4>("--power") {
        app.sch.set_power_signals(n.map(String::from));
    }
    if let Some(n) = a.val("--signals") {
        app.sch.set_tran_signals(n);
    }
    // `--tran-tab N`: the transient viewer opens on table N (0 basic … 4 power, 5 "音で聴く")
    if let Some(t) = a.num::<usize>("--tran-tab") {
        app.sch.tran_tab_request = Some(t);
    }
    if a.has("--spectrum") {
        app.sch.show_spectrum();
    }
    if a.has("--digital") {
        app.sch.show_digital();
    }
}

/// `f0 f1 ppd` at `from` (an analysis' own frequency range, when given): into the AC settings.
fn ac_range(app: &mut App, a: &Args, from: usize) {
    if let Some([f0, f1, ppd]) = a.words(from)
        && !f0.starts_with("--")
    {
        app.sch.set_ac_settings(f0, f1, ppd);
    }
}

/// `-` for "none" (the input net, the parts taken out ...).
fn dash(s: &str) -> &str {
    if s == "-" { "" } else { s }
}

pub(super) fn apply(app: &mut App, a: &Args) {
    let Some(p) = a.file(".schjson") else { return };
    app.sch.load_path(p.into());
    // `--sheet N`: the Nth sheet (from 1) is the one shown
    if let Some(n) = a.num::<usize>("--sheet").filter(|n| *n >= 1) {
        app.sch.show_sheet(n - 1);
    }
    if a.has("--dc") {
        app.sch.analyze_dc();
    }
    if a.has("--analyze") {
        app.sch.show_analysis_tab();
    }
    windows(app, a);
    heat(app, a);
    if a.has("--requirements") {
        app.sch.open_requirements();
        if a.has("--dc") {
            app.sch.requirements_from_dc();
        }
    }
    if let Some(r) = a.val("--select") {
        app.sch.select_part(r);
        // selecting goes to the drawing tab; `--analyze` asks for the analysis tab with the part selected
        if a.has("--analyze") {
            app.sch.show_analysis_tab();
        }
    }
    analyses(app, a);
    tran(app, a);
    // "デジタル回路を動かしながら見る": `--live-logic [SW1,SW2]` flips the switches named, `--live-slow` turns slow motion on
    if a.has("--live-logic") {
        app.sch.start_live_logic();
        app.sch.live_logic_args(a.after("--live-logic").unwrap_or(""), a.has("--live-slow"));
    }
}

/// The windows over the sheet: `--results`, `--pins`, `--tidy`, `--adc-measures`, `--netlist`, `--wizard`, `--battery`, `--section`.
fn windows(app: &mut App, a: &Args) {
    // --results: the "解析結果の表" window open (after --dc)
    if a.has("--results") {
        app.sch.open_result_table();
    }
    // `--pins`: the "ピン定義" window open
    if a.has("--pins") {
        app.sch.open_pins();
    }
    tidy(app, a);
    // --adc-measures: the table of what the ADCs measure, worked out
    if a.has("--adc-measures") {
        app.sch.open_adc_measures();
    }
    if a.has("--netlist") {
        app.sch.open_netlist();
    }
    // --wizard <n>: the "値の決め方" window on its n-th calculator
    if a.has("--wizard") {
        app.sch.open_wizard(a.num("--wizard").unwrap_or(0));
    }
    // `--battery`: the battery life of the sheet with its saved settings ("電池の持ち時間")
    if a.has("--battery") {
        app.sch.run_battery();
        app.sch.open_analysis_section("battery");
    }
    section(app, a);
}

/// The heat runs (with `--section selfheat`): `--ambient C`, `--heattran PARTS TSTOP`, `--ambient-sweep FROM TO STEP`, `--selfheat`.
fn heat(app: &mut App, a: &Args) {
    // --ambient C: the surrounding temperature of the heat runs below
    if let Some(t) = a.val("--ambient") {
        app.sch.heat.ambient = t.to_string();
    }
    // --heattran PARTS TSTOP: the "温度の時間変化" worked out for those parts
    if let Some([parts, stop]) = a.vals("--heattran") {
        app.sch.heat.tran_parts = parts.to_string();
        app.sch.heat.tran_stop = stop.to_string();
        app.sch.heat.tran = Some(app.sch.run_heat_tran());
    }
    // --ambient-sweep FROM TO STEP: "周囲の温度を振る" worked out
    if let Some([from, to, step]) = a.vals("--ambient-sweep") {
        (app.sch.heat.amb_from, app.sch.heat.amb_to, app.sch.heat.amb_step) = (from.to_string(), to.to_string(), step.to_string());
        app.sch.heat.amb_view = Some(app.sch.run_ambient_sweep());
    }
    // --selfheat: the "自己発熱" result worked out
    if a.has("--selfheat") {
        app.sch.heat.view = Some(app.sch.run_self_heat());
    }
}

/// The analyses other than the DC operating point and the transient run, in this order.
fn analyses(app: &mut App, a: &Args) {
    if let Some([f0, f1, ppd]) = a.vals("--ac") {
        app.sch.set_ac_settings(f0, f1, ppd);
        app.sch.analyze_ac();
    }
    // --sweep <source> <start> <stop> <step>
    if let Some([src, from, to, step]) = a.vals("--sweep") {
        app.sch.set_sweep_settings(src, from, to, step);
        app.sch.analyze_sweep();
    }
    fault(app, a);
    // --spec <kind index 0..8> <target> <lo|-> <hi|-> [frequency]  (repeatable; put it after --dc / --tran / --ac)
    for i in a.all("--spec") {
        if let Some([k, target, lo, hi]) = a.words(i + 1)
            && let Ok(k) = k.parse()
        {
            app.sch.add_spec(k, target, lo, hi, a.word(i + 5).unwrap_or(""));
        }
    }
    // --measured <spec row index> <text>  (repeatable; after the --spec rows): a typed bench measurement
    for i in a.all("--measured") {
        if let Some([row, text]) = a.words(i + 1)
            && let Ok(row) = row.parse()
        {
            app.sch.set_spec_measured(row, text);
        }
    }
    // --param <part> <from> <to> <count> <node> [tran]  (AC unless "tran"; uses the --ac / --tran settings)
    if let Some(i) = a.pos("--param")
        && let Some([part, from, to, n, node]) = a.words(i + 1)
    {
        app.sch.set_param_settings(part, from, to, n, node, a.get(i + 6) != Some("tran"));
        app.sch.analyze_param();
    }
    // --loop <part> [output-side net|-] [f0 f1 ppd]  (the loop gain; uses the --ac frequency range, or the defaults)
    if let Some(i) = a.pos("--loop")
        && let Some(part) = a.get(i + 1)
    {
        ac_range(app, a, i + 3);
        app.sch.set_loop_settings(part, a.word(i + 2).map_or("", dash));
        app.sch.analyze_loop();
    }
    // --loop-chart nyquist|nichols [plain]  (draw the loop gain of --loop that way; "plain": the Nyquist chart unsqueezed)
    if let Some(i) = a.pos("--loop-chart")
        && let Some(kind) = a.get(i + 1)
    {
        app.sch.set_loop_chart(kind, a.get(i + 2) == Some("plain"));
    }
    // --locus <part> <from> <to> <count> <output net> <input net or -> [f0 f1 ppd]  (the root locus, by ratio steps)
    if let Some(i) = a.pos("--locus")
        && let Some([part, from, to, n, out, input]) = a.words(i + 1)
    {
        ac_range(app, a, i + 7);
        app.sch.set_locus_settings(part, from, to, n, out, dash(input));
        app.sch.analyze_locus();
        app.sch.show_analysis_section("rootlocus");
    }
    // --z <net> <reference net or -> <parts to take out, comma separated, or -> [f0 f1 ppd]  (uses the --ac frequency range)
    if let Some(i) = a.pos("--z")
        && let Some([node, reference, remove]) = a.words(i + 1)
    {
        ac_range(app, a, i + 4);
        app.sch.set_impedance_settings(node, dash(reference), dash(remove));
        app.sch.analyze_impedance();
    }
    // --pz <output net> <input net or -> [f0 f1 ppd]  (poles and zeros; uses the given frequency range, or the defaults)
    if let Some(i) = a.pos("--pz")
        && let Some(out) = a.get(i + 1)
    {
        ac_range(app, a, i + 3);
        app.sch.set_pz_settings(out, a.word(i + 2).map_or("", dash));
        app.sch.analyze_pz();
    }
    // --mc <node> [ac]  (the variation analysis; AC measures the -3 dB frequency and uses the --ac range)
    if let Some(i) = a.pos("--mc")
        && let Some(node) = a.get(i + 1)
    {
        app.sch.analyze_variation(node, a.get(i + 2) == Some("ac"));
        // its settings next to the result (the tolerances and the box for the models' min / max)
        app.sch.show_analysis_section("variation");
    }
    // --cmrr <+ source or -> <- source or -> <node>  (uses the --ac frequency range; '-' picks the first input sources)
    if let Some([plus, minus, out]) = a.vals("--cmrr") {
        app.sch.set_cmrr_settings(dash(plus), dash(minus), out);
        app.sch.analyze_cmrr();
    }
    // --psrr <supply source or -> <node>  (uses the --ac frequency range)
    if let Some([src, out]) = a.vals("--psrr") {
        app.sch.set_psrr_settings(dash(src), out);
        app.sch.analyze_psrr();
    }
    // --diagnose NODE=V,NODE=V  (the readings; the faults are solved here, then matched)
    if let Some(r) = a.val("--diagnose") {
        app.sch.set_diag_readings(r);
        app.sch.analyze_diagnosis();
        app.sch.show_analysis_section("diagnose");
    }
    // --sens <node> [ac]  (sensitivity: every part moved by 1 %; AC measures the -3 dB frequency with the --ac range)
    if let Some(i) = a.pos("--sens")
        && let Some(node) = a.get(i + 1)
    {
        app.sch.analyze_sensitivity(node, a.get(i + 2) == Some("ac"));
    }
    // --noise <out node> <input source or -> <temp C> (uses the --ac frequency range)
    if let Some([out, input, temp]) = a.vals("--noise") {
        app.sch.set_noise_settings(out, dash(input), temp);
        app.sch.analyze_noise();
    }
}

/// `--fault <part> <0 open | 1 short | 2 x10 | 3 /10 ...> [tran <node> | ac <node> | chain]`, `--fault scan`, `--fault scan-bg`.
fn fault(app: &mut App, a: &Args) {
    let Some(i) = a.pos("--fault") else { return };
    match a.get(i + 1) {
        Some("scan") => app.sch.analyze_fault_scan(),
        // the scan on its own thread (the screenshot is taken while it runs)
        Some("scan-bg") => app.sch.start_fault_scan_bg(),
        Some(part) => {
            app.sch.set_fault(part, a.get(i + 2).and_then(|k| k.parse().ok()).unwrap_or(0));
            // ... <kind> tran <node>: compare the waveform at <node> instead of the DC voltages
            // ... <kind> chain: follow the failures that come after (kind 7 / 8 / 9 = coil layer short 10 / 20 / 50%)
            let chain = a.get(i + 3) == Some("chain");
            match a.get(i + 3) {
                Some("tran") => app.sch.set_fault_tran(a.get(i + 4).unwrap_or("")),
                Some("ac") => app.sch.set_fault_ac(a.get(i + 4).unwrap_or("")),
                _ => {}
            }
            // a chain follows a transient run: the --tran time when given (it is read later for the waveform)
            if let Some([stop, step]) = a.vals("--tran") {
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
