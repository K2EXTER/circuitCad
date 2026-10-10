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
        assert!(out.primary.is_some() || wizard::CALCS[k].title.contains("配線"), "{}", wizard::CALCS[k].title);
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

#[test]
fn bridge_rectifier_sample_gives_a_smoothed_dc_with_ripple() {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "bridge_rectifier.schjson").unwrap().1;
    let c = from_schematic(&sch).unwrap();
    let r = transient(&c, &TranSettings { tstop: 0.3, tstep: 100e-6, uic: false }).unwrap();
    let v = r.signal("V(VDC)").unwrap();
    let m = wave_measure(&r.time, &v.values, 0.2, 0.3).unwrap();
    // 12 V rated transformer: about 17-20 V peak minus two diode drops, smoothed by 1000 uF into 100 ohm
    assert!(m.min > 10.0 && m.max > 14.0 && m.max < 20.0, "{m:?}");
    assert!(m.pp > 0.5 && m.pp < 6.0, "ripple {:?}", m.pp);
    // the ripple of a full-wave rectifier is at twice the mains frequency
    let ac = wave_measure(&r.time, &r.signal("V(AC1)").unwrap().values, 0.2, 0.3).unwrap();
    assert!((ac.freq.unwrap() - 50.0).abs() < 2.0, "{ac:?}");
}

#[test]
fn rectifier_lesson_numbers_hold() {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "bridge_rectifier.schjson").unwrap().1;
    let run = |c: &str, r: &str| {
        let mut s = sch.clone();
        s.components.iter_mut().find(|x| x.refdes == "C1").unwrap().value = c.into();
        s.components.iter_mut().find(|x| x.refdes == "R1").unwrap().value = r.into();
        let circ = from_schematic(&s).unwrap();
        let t = transient(&circ, &TranSettings { tstop: 0.3, tstep: 100e-6, uic: false }).unwrap();
        let vdc = wave_measure(&t.time, &t.signal("V(VDC)").unwrap().values, 0.2, 0.3).unwrap();
        let ac = wave_measure(&t.time, &t.signal("V(AC1)").unwrap().values, 0.2, 0.3).unwrap();
        (vdc, ac)
    };
    let (base, ac) = run("1000u", "100");
    println!("base: {base:?} ac max {}", ac.max);
    let (small, _) = run("220u", "100");
    let (heavy, _) = run("1000u", "33");
    println!("220u: {small:?}\n33R: {heavy:?}");
    assert!(ac.max > 15.0 && ac.max < 18.0, "AC1 peak {}", ac.max);
    assert!((14.5..16.0).contains(&((base.max + base.min) / 2.0)), "mean {base:?}");
    let k = small.pp / base.pp;
    assert!((3.5..5.0).contains(&k), "220u ripple ratio {k}");
    let k2 = heavy.pp / base.pp;
    assert!((1.6..2.5).contains(&k2), "33R ripple ratio {k2}");
    assert!(heavy.max < base.max && (11.5..13.0).contains(&((heavy.max + heavy.min) / 2.0)));
}

#[test]
fn a_broken_bridge_diode_doubles_the_ripple() {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "bridge_rectifier.schjson").unwrap().1;
    let ripple = |c: &sim_core::Circuit| {
        let t = transient(c, &TranSettings { tstop: 0.3, tstep: 100e-6, uic: false }).unwrap();
        wave_measure(&t.time, &t.signal("V(VDC)").unwrap().values, 0.2, 0.3).unwrap()
    };
    let good = from_schematic(&sch).unwrap();
    let healthy = ripple(&good);
    let broken = ripple(&sim_core::with_fault(&good, "D1", sim_core::Fault::Open).unwrap());
    println!("{healthy:?}\n{broken:?}");
    assert!(broken.pp > 1.6 * healthy.pp, "{} vs {}", broken.pp, healthy.pp);
    // half-wave: the ripple is at the mains frequency
    assert!((broken.freq.unwrap() - 50.0).abs() < 3.0, "{:?}", broken.freq);
}

#[test]
fn transient_fault_on_the_rectifier_is_explained_in_the_panel_flow() {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "bridge_rectifier.schjson").unwrap().1;
    let mut e = SchEditor::default();
    e.sch = sch;
    e.set_tran_settings("0.3", "100u");
    e.set_fault("D1", 0);
    e.set_fault_tran("VDC");
    e.analyze_fault();
    assert!(e.status.contains("過渡"), "{}", e.status);
    assert!(e.has_wave());
    let Ok(fault::FaultWave::Tran(r)) = &e.fault_wave.as_ref().unwrap().report else { panic!("expected a transient report") };
    let text = r.summary().join("\n");
    assert!(text.contains("振幅 (p-p)") && text.contains("周波数: 100Hz → 50Hz"), "{text}");
    // with no node chosen, the likely output net is found; a wrong node is explained
    e.fault_node.clear();
    assert_eq!(e.fault_watch_node().as_deref(), Some("VDC"));
    e.set_fault_tran("NOPE");
    e.analyze_fault();
    assert!(e.status.contains("失敗"), "{}", e.status);
}

#[test]
fn ac_fault_on_the_lowpass_example_moves_the_cutoff() {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "rc_lowpass.schjson").unwrap().1;
    let node = extract(&sch).into_iter().find(|n| n.pins.iter().any(|p| p.refdes == "C1") && n.name != "GND").unwrap().name;
    let mut e = SchEditor::default();
    e.sch = sch;
    e.set_ac_settings("10", "1Meg", "20");
    e.set_fault("R1", 2); // R1 x10
    e.set_fault_ac(&node);
    e.analyze_fault();
    assert!(e.status.contains("AC"), "{}", e.status);
    let Ok(fault::FaultWave::Ac(r)) = &e.fault_wave.as_ref().unwrap().report else { panic!("expected an AC report") };
    let text = r.summary().join("\n");
    assert!(text.contains("-3dB点 (カットオフ)"), "{text}");
    // C1 open: the filter stops filtering
    e.set_fault("C1", 0);
    e.analyze_fault();
    let Ok(fault::FaultWave::Ac(r)) = &e.fault_wave.as_ref().unwrap().report else { panic!("expected an AC report") };
    assert!(r.summary().join("\n").contains("なくなりました"), "{:?}", r.summary());
    // a bad frequency setting is explained
    e.set_ac_settings("abc", "1Meg", "20");
    e.analyze_fault();
    assert!(e.status.contains("失敗"), "{}", e.status);
}

fn editor_with(example: &str) -> SchEditor {
    let sch = example_schematics().into_iter().find(|(n, _)| n == example).unwrap_or_else(|| panic!("{example}")).1;
    let mut e = SchEditor::default();
    e.sch = sch;
    e
}

#[test]
fn pin_faults_break_one_pin_of_an_ic_and_of_a_transistor() {
    // op-amp amplifier: the gain network hangs on IN-; opening that pin of the LM358 changes the output
    let mut e = editor_with("opamp_amp.schjson");
    e.fault_part = "U1".into();
    e.fault_pin = "2".into();
    assert!(e.pin_choices("U1").iter().any(|(n, d)| n == "2" && d.contains("IN-")), "{:?}", e.pin_choices("U1"));
    // at DC the input is 0 V so nothing moves; in the time domain the open feedback pin is plain to see
    e.set_tran_settings("3m", "5u");
    e.set_fault_tran("OUT");
    e.analyze_fault();
    let Ok(fault::FaultWave::Tran(r)) = &e.fault_wave.as_ref().unwrap().report else { panic!("expected a transient report") };
    assert!(r.label.contains("ピン 2 IN-") && r.label.contains("断線"), "{}", r.label);
    assert!(r.summary().len() > 1, "{:?}", r.summary());
    e.fault_domain = fault::FaultDomain::Dc;
    // bridging a pin to ground, and a missing net name is explained
    e.fault_pin_how = 2;
    e.analyze_fault();
    assert!(e.status.contains("失敗"), "{}", e.status);
    e.fault_bridge = "GND".into();
    e.analyze_fault();
    assert!(e.fault_view.as_ref().unwrap().report.as_ref().unwrap().label.contains("ブリッジ"));
    // the scan includes pin rows for the IC
    e.analyze_fault_scan();
    assert!(e.fault_scan.iter().any(|row| row.part == "U1" && !row.pin.is_empty()), "no pin rows");
    // a library transistor (2N3904: pin 2 = B): the base pin has a readable name, and the scan covers its pins
    let mut q = editor_with("npn_not.schjson");
    q.fault_part = "Q1".into();
    q.fault_pin = "2".into();
    q.analyze_fault();
    let r = q.fault_view.as_ref().unwrap().report.as_ref().unwrap_or_else(|m| panic!("{m:?}"));
    assert!(r.label.contains("ピン 2 B") && r.element == "Q1", "{}", r.label);
    q.analyze_fault_scan();
    assert_eq!(q.fault_scan.iter().filter(|row| row.part == "Q1" && !row.pin.is_empty()).count(), 3);
}

#[test]
fn reversed_parts_and_supply_faults_work_in_the_panel() {
    let mut e = editor_with("bridge_rectifier.schjson");
    e.set_tran_settings("0.3", "100u");
    // D1 fitted backwards: tran compare at VDC
    let kind = 4; // ALL_FAULTS[4] = Reverse
    e.set_fault("D1", kind);
    e.set_fault_tran("VDC");
    e.analyze_fault();
    let Ok(fault::FaultWave::Tran(r)) = &e.fault_wave.as_ref().unwrap().report else { panic!("expected a transient report") };
    assert!(r.label.contains("逆向き"), "{}", r.label);
    assert!(!r.summary().is_empty());
    // the supply of a DC circuit: half voltage
    let mut s = editor_with("rc_lowpass.schjson");
    s.sch.components.iter_mut().find(|c| c.refdes == "V1").unwrap().value = "5".into();
    s.set_fault("V1", 5); // Scale(0.5)
    s.analyze_fault();
    let rep = s.fault_view.as_ref().unwrap().report.as_ref().unwrap_or_else(|m| panic!("{m:?}"));
    assert!(rep.label.contains("電圧が 0.5 倍"), "{}", rep.label);
}
