use super::*;

fn example_schematics() -> Vec<(String, Schematic)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "schjson"))
        .map(|e| {
            let sch: Schematic = serde_json::from_str(&std::fs::read_to_string(e.path()).unwrap()).unwrap();
            (e.file_name().to_string_lossy().into_owned(), sch)
        })
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

/// Every shipped example must simulate: it reaches a DC operating point, or (oscillators and
/// chargers that never settle at DC) runs in the time domain. A model change that breaks a
/// sample is then caught by CI instead of by a learner.
#[test]
fn shipped_examples_simulate() {
    let examples = example_schematics();
    assert!(!examples.is_empty(), "no examples found");
    let mut bad = vec![];
    for (name, sch) in examples {
        let mut e = SchEditor::default();
        e.sch = sch;
        match from_schematic(&e.sch) {
            Err(errs) => bad.push(format!("{name}: 変換できません: {}", errs.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", "))),
            Ok(c) if dc_op(&c).is_ok() => {}
            Ok(_) => {
                e.run_tran();
                match e.tran.as_ref().map(|t| &t.outcome) {
                    Some(Ok(_)) => {}
                    _ => bad.push(format!("{name}: DCも過渡も失敗: {}", e.status)),
                }
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn parameter_sweep_runs_from_the_editor_in_both_modes() {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "rc_lowpass.schjson").unwrap().1;
    let node = extract(&sch).into_iter().find(|n| n.pins.iter().any(|p| p.refdes == "C1") && n.name != "GND").unwrap().name;
    let mut e = SchEditor::default();
    e.sch = sch;
    e.set_ac_settings("10", "1Meg", "10");
    e.set_tran_settings("2m", "10u");
    for ac in [true, false] {
        e.set_param_settings("R1", "100", "10k", "3", &node, ac);
        e.analyze_param();
        assert!(e.status.contains("完了") && e.status.contains("3 本"), "ac={ac}: {}", e.status);
        assert!(e.has_wave());
    }
    // a part that does not exist is explained, not a panic
    e.set_param_settings("R99", "100", "10k", "3", &node, true);
    e.analyze_param();
    assert!(e.status.contains("失敗"), "{}", e.status);
}

#[test]
fn every_wizard_calculator_works_with_its_defaults_and_explains_failures() {
    let mut e = SchEditor::default();
    for k in 0..wizard::CALCS.len() {
        e.open_wizard(k);
        let out = e.wizard_result().unwrap_or_else(|m| panic!("{}: {m}", wizard::CALCS[k].title));
        assert!(!out.steps.is_empty() && !out.cautions.is_empty() && !out.faults.is_empty(), "{}", wizard::CALCS[k].title);
        assert!(out.primary.is_some(), "{}", wizard::CALCS[k].title);
    }
    // a typo is explained instead of panicking
    e.open_wizard(0);
    e.wiz_in[0] = "abc".into();
    assert!(e.wizard_result().unwrap_err().contains("電源電圧"));
}

#[test]
fn fault_simulation_on_the_regulator_example() {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "regulator_led.schjson").unwrap().1;
    let mut e = SchEditor::default();
    e.sch = sch;
    // find the LED-side resistor and open it: the LED current must stop
    let r = e.sch.components.iter().find(|c| c.kind == sch_core::Kind::Resistor).unwrap().refdes.clone();
    e.set_fault(&r, 0);
    e.analyze_fault();
    let v = e.fault_view.as_ref().unwrap();
    let report = v.report.as_ref().unwrap_or_else(|m| panic!("{m:?}"));
    assert!(report.summary().iter().any(|l| l.contains("電流が止まります") || l.contains("→")), "{:?}", report.summary());
    // showing it on the sheet marks the overlay as a what-if and restoring clears the mark
    e.show_fault_on_sheet();
    assert!(e.dc.as_ref().unwrap().fault.is_some());
    e.run_dc();
    assert!(e.dc.as_ref().unwrap().fault.is_none());
    // the scan covers every breakable part and puts the worst case first
    e.analyze_fault_scan();
    assert!(e.fault_scan.len() >= 4);
    assert!(e.fault_scan.windows(2).all(|w| w[0].severity >= w[1].severity));
}

/// The op-amp loop-gain sample: a capacitive load adds a pole near the crossover and eats the phase margin.
#[test]
fn loop_gain_sample_shows_the_phase_margin_falling_with_a_capacitive_load() {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "opamp_loop_gain.schjson").unwrap().1;
    let pm = |cap: &str| {
        let mut s = sch.clone();
        s.components.iter_mut().find(|c| c.refdes == "C1").unwrap().value = cap.into();
        let c = from_schematic(&s).unwrap();
        let r = ac_analysis(&c, &AcSettings { fstart: 1.0, fstop: 100e6, sweep: AcSweep::Decade(40) }).unwrap();
        let sg = r.signal("V(OUT)").unwrap();
        stability_margins(&r.freq, &sg.db(), &sg.phase_deg()).unwrap()
    };
    let loaded = pm("10n");
    assert!(loaded.inverted);
    let (heavy, light) = (loaded.phase_margin.unwrap(), pm("1p").phase_margin.unwrap());
    assert!(heavy < 45.0 && light > 70.0 && heavy < light - 30.0, "10n: {heavy}, 1p: {light}");
}
