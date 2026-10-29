use super::*;

pub(super) fn example_schematics_for_tests() -> Vec<(String, Schematic)> {
    example_schematics()
}

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
        assert!(out.primary.is_some() || wizard::CALCS[k].title.contains("配線") || wizard::CALCS[k].title.contains("電池") || wizard::CALCS[k].title.starts_with("熱") || wizard::CALCS[k].title.starts_with("ヒューズ"), "{}", wizard::CALCS[k].title);
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
    assert!(e.fault_scan.windows(2).all(|w| w[0].danger > w[1].danger || (w[0].danger == w[1].danger && w[0].severity >= w[1].severity)));
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
fn three_phase_wye_sample_has_three_phases_120_degrees_apart() {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "three_phase_wye.schjson").unwrap().1;
    let c = from_schematic(&sch).unwrap();
    let r = transient(&c, &TranSettings { tstop: 0.1, tstep: 100e-6, uic: false }).unwrap();
    let (a, b, cc) = (r.signal("V(A)").unwrap(), r.signal("V(B)").unwrap(), r.signal("V(C)").unwrap());
    for s in [a, b, cc] {
        let m = wave_measure(&r.time, &s.values, 0.02, 0.1).unwrap();
        assert!((m.max - 14.142).abs() < 0.2 && (m.freq.unwrap() - 50.0).abs() < 2.0, "{m:?}");
    }
    // a balanced set sums to zero at every instant (so no current would flow in N)
    for i in 0..r.time.len() {
        assert!((a.values[i] + b.values[i] + cc.values[i]).abs() < 1e-6, "t={}", r.time[i]);
    }
    // B follows A by a third of a period (6.67 ms): B(t) = A(t - T/3)
    let k = r.time.iter().position(|t| *t >= 0.02 + 20e-3 / 3.0).unwrap();
    let j = r.time.iter().position(|t| *t >= 0.02).unwrap();
    assert!((b.values[k] - a.values[j]).abs() < 0.5, "{} vs {}", b.values[k], a.values[j]);
}

#[test]
fn three_phase_rectifier_sample_ripples_at_six_times_the_line_frequency() {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "three_phase_rectifier.schjson").unwrap().1;
    let c = from_schematic(&sch).unwrap();
    let r = transient(&c, &TranSettings { tstop: 0.2, tstep: 50e-6, uic: false }).unwrap();
    let m = wave_measure(&r.time, &r.signal("V(VDC)").unwrap().values, 0.1, 0.2).unwrap();
    // line-to-line peak 24.5 V minus two diode drops
    assert!(m.max > 21.0 && m.max < 24.5 && m.min > 19.0, "{m:?}");
    // from a cold start the capacitor charges smoothly: the output never rises above the line-to-line peak
    // (an ideal source with no series resistance made the first step ring up to 44 V)
    let all = wave_measure(&r.time, &r.signal("V(VDC)").unwrap().values, 0.0, 0.2).unwrap();
    assert!(all.max < 24.5, "{all:?}");
    // much smoother than the single-phase bridge: ripple is 6 humps per cycle
    assert!(m.pp > 0.1 && m.pp < 2.0, "ripple {:?}", m.pp);
    assert!((m.freq.unwrap() - 300.0).abs() < 20.0, "{m:?}");
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
fn a_blank_time_step_is_chosen_from_the_end_time() {
    let mut e = editor_with("rlc_pulse.schjson");
    assert_eq!(e.tran_tstep, "", "the step is automatic by default");
    e.tran_tstop = "10m".into();
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-9 * b.abs();
    let (tstop, tstep) = e.tran_times().unwrap();
    assert!(close(tstop, 0.01) && close(tstep, 0.01 / 1000.0), "{tstop} {tstep}");
    e.analyze_tran();
    assert!(e.status.contains("完了"), "{}", e.status);
    // a typed step still wins, and a typo is reported instead of silently falling back to the automatic one
    e.tran_tstep = "5u".into();
    assert!(close(e.tran_times().unwrap().1, 5e-6));
    e.tran_tstep = "abc".into();
    assert_eq!(e.tran_times(), None);
    e.analyze_tran();
    assert!(e.status.contains("数値"), "{}", e.status);
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

#[test]
fn coin_cell_sample_sags_under_load_and_runs_out_when_the_capacity_is_used() {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "coin_cell_led.schjson").unwrap().1;
    let c = from_schematic(&sch).unwrap();
    // 3 V cell, 15 ohm inside: about 9 mA through the LED pulls the terminal down to about 2.86 V
    let dc = dc_op(&c).unwrap();
    let v = dc.voltage("VBAT").unwrap();
    assert!((2.80..2.92).contains(&v), "{v}");
    assert!((8e-3..10.5e-3).contains(&dc.element("D1").unwrap().current));
    // a bigger current: a bigger sag
    let mut heavy = sch.clone();
    heavy.components.iter_mut().find(|x| x.refdes == "R1").unwrap().value = "10".into();
    let vh = dc_op(&from_schematic(&heavy).unwrap()).unwrap().voltage("VBAT").unwrap();
    assert!((2.40..2.56).contains(&vh), "{vh}");
    // 230 mAh at about 9 mA: roughly 25 hours, then the voltage collapses
    let t = transient(&c, &TranSettings { tstop: 40.0 * 3600.0, tstep: 600.0, uic: false }).unwrap();
    let vb = t.signal("V(VBAT)").unwrap();
    let at = |h: f64| vb.values[t.time.iter().position(|x| *x >= h * 3600.0).unwrap()];
    assert!(at(10.0) > 2.78, "still healthy after 10 h: {}", at(10.0));
    assert!(at(35.0) < 2.1, "empty after 35 h: {}", at(35.0));
    let drop_start = t.time.iter().zip(&vb.values).find(|(_, v)| **v < 2.5).map(|(x, _)| x / 3600.0).unwrap();
    assert!((22.0..30.0).contains(&drop_start), "the cliff comes after {drop_start} h");
}

#[test]
fn right_click_leaves_the_tool_and_the_probe_adds_any_net_to_the_viewer() {
    let mut e = editor_with("rc_lowpass.schjson");
    // the right click: confirm the wire being drawn, then (nothing in progress) return to the Select tool
    e.tool = Tool::Wire;
    e.pending = Some((0, 0));
    assert!(e.leave_tool(), "the first right click confirms the wire");
    assert!(e.tool == Tool::Wire && e.pending.is_none(), "and the Wire tool stays");
    assert!(e.leave_tool() && e.tool == Tool::Select, "the second one, with nothing being drawn, goes back to Select");
    assert!(!e.leave_tool(), "already in the Select tool: the right click then opens the part window instead");
    e.tool = Tool::Place;
    assert!(e.leave_tool() && e.tool == Tool::Select);
    // the probe: pick a pin on the sheet and its voltage joins the transient viewer
    e.set_tran_settings("2m", "10u");
    e.analyze_tran();
    let before = e.tran.as_ref().unwrap().selected.clone();
    let c1 = e.sch.components.iter().find(|c| c.refdes == "C1").unwrap();
    let pt = c1.pins().into_iter().find(|(n, _)| n == "1").unwrap().1;
    let net = sch_core::net_at(&e.sch, pt).unwrap();
    e.tran.as_mut().unwrap().selected.clear();
    e.start_probe();
    assert!(e.noise_pick && e.pick_probe);
    let gf = |p: sch_core::Pt| (p.0 as f32, p.1 as f32);
    e.pick_noise_node(gf(pt), pt, false);
    assert!(e.tran.as_ref().unwrap().selected.contains(&format!("V({net})")), "{} / {:?}", e.status, before);
    assert!(e.noise_pick, "picking goes on, so several points can be added");
    // the body of a part adds its current; Shift+click on a pin does too
    let r1 = e.sch.components.iter().find(|c| c.refdes == "R1").unwrap();
    let body = gf(r1.pos);
    e.pick_noise_node(body, r1.pos, false);
    assert!(e.tran.as_ref().unwrap().selected.contains("I(R1)"), "{}", e.status);
    e.pick_noise_node(gf(pt), pt, true);
    assert!(e.tran.as_ref().unwrap().selected.contains("I(C1)"), "{}", e.status);
    // a spot with nothing on it is explained, and GND (no waveform) too
    e.pick_noise_node((200.0, 200.0), (200, 200), false);
    assert!(e.status.contains("何もありません"), "{}", e.status);
    let g = e.sch.components.iter().find(|c| c.kind == sch_core::Kind::Ground).unwrap().pins()[0].1;
    e.pick_noise_node(gf(g), g, false);
    assert!(e.status.contains("GND"), "{}", e.status);
}

/// Runs the buck sample for 5 ms and measures the last 0.6 ms: (mean Vout, Vout p-p, inductor current min / max, efficiency).
fn buck_run(l: &str, rl: &str) -> (f64, f64, f64, f64, f64) {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "buck_converter.schjson").unwrap().1;
    let mut s2 = sch;
    s2.components.iter_mut().find(|c| c.refdes == "L1").unwrap().value = l.into();
    s2.components.iter_mut().find(|c| c.refdes == "R3").unwrap().value = rl.into();
    let c = from_schematic(&s2).unwrap();
    let t = transient(&c, &TranSettings { tstop: 5e-3, tstep: 100e-9, uic: false }).unwrap();
    let w = (4.4e-3, 5e-3);
    let v = t.signal("V(VOUT)").unwrap();
    let vo = wave_measure(&t.time, &v.values, w.0, w.1).unwrap();
    let il = wave_measure(&t.time, &t.signal("I(L1)").unwrap().values, w.0, w.1).unwrap();
    let iin = signal_stats(&t.time, &t.signal("I(V1)").unwrap().values, w.0, w.1).unwrap();
    let vm = signal_stats(&t.time, &v.values, w.0, w.1).unwrap().mean;
    let pout = vm * vm / rl.parse::<f64>().unwrap();
    (vm, vo.pp, il.min, il.max, pout / (12.0 * -iin.mean))
}

#[test]
fn buck_sample_regulates_5v_switches_at_150khz_and_the_lesson_numbers_hold() {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "buck_converter.schjson").unwrap().1;
    // at DC the switch is open: nothing at the output, and the DC solution exists
    assert!(dc_op(&from_schematic(&sch).unwrap()).unwrap().voltage("VOUT").unwrap().abs() < 0.05);
    let (v, pp, imin, imax, eff) = buck_run("100u DCR=0.05", "20");
    assert!((4.5..5.3).contains(&v), "Vout {v}");
    assert!(pp < 0.3, "output ripple {pp}");
    assert!(imin > 0.0 && imax - imin < 0.45, "continuous inductor current {imin}..{imax}");
    assert!((0.88..0.98).contains(&eff), "efficiency {eff}");
    // a small coil: the current swings more and touches zero (discontinuous)
    let (_, _, imin2, imax2, _) = buck_run("22u DCR=0.05", "20");
    assert!(imin2 < 0.02 && imax2 - imin2 > 0.55, "{imin2}..{imax2}");
    // twice the load current: the output still holds, and the efficiency is higher (the fixed losses of the
    // control circuit weigh less). ngspice agrees: 0.900 at 20 ohm, 0.916 at 10 ohm; ours 0.894 and 0.911.
    let (v3, _, _, _, eff3) = buck_run("100u DCR=0.05", "10");
    assert!((4.6..5.3).contains(&v3) && (0.82..0.95).contains(&eff3) && eff3 > eff, "{v3} {eff3} vs {eff}");
}

/// Runs a buck sample variant for 5 ms and measures the last 0.6 ms: (mean Vout, Vout p-p).
fn cout_run(file: &str) -> (f64, f64) {
    let sch = example_schematics().into_iter().find(|(n, _)| n == file).unwrap().1;
    let c = from_schematic(&sch).unwrap();
    let t = transient(&c, &TranSettings { tstop: 5e-3, tstep: 100e-9, uic: false }).unwrap();
    let v = t.signal("V(VOUT)").unwrap();
    let vo = wave_measure(&t.time, &v.values, 4.4e-3, 5e-3).unwrap();
    (signal_stats(&t.time, &v.values, 4.4e-3, 5e-3).unwrap().mean, vo.pp)
}

#[test]
fn buck_output_cap_swap_samples_differ_in_ripple() {
    let (ve, ppe) = cout_run("buck_cout_electrolytic.schjson");
    let (vc, ppc) = cout_run("buck_cout_ceramic.schjson");
    // both regulate the same; the electrolytic's 0.3 ohm ESR (x the ~0.3 A coil ripple) dominates its ripple even though it has 10x the capacitance
    assert!((4.5..5.3).contains(&ve) && (4.5..5.3).contains(&vc), "{ve} {vc}");
    // ngspice measures 0.104 V (electrolytic) and 0.070 V (ceramic) in this window
    // (the ceramic ripple varies with the platform's float rounding: 0.083 V on macOS arm64 CI, so the margin is 1.2x, not more)
    assert!(ppe > 1.2 * ppc && ppe < 0.15, "electrolytic {ppe} V vs ceramic {ppc} V");
}

#[test]
fn cap_impedance_sample_electrolytic_wins_low_ceramic_wins_high() {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "cap_impedance.schjson").unwrap().1;
    let r = ac_analysis(&from_schematic(&sch).unwrap(), &AcSettings { fstart: 100.0, fstop: 100e6, sweep: AcSweep::Decade(20) }).unwrap();
    // with the 1 ohm series resistor V(node) ~ |Z| while it is well below 1 ohm
    let z = |n: &str, f: f64| {
        let m = r.signal(n).unwrap().mag();
        m[r.freq.iter().position(|&x| x >= f * 0.999).unwrap()]
    };
    // low frequencies: the 100 uF electrolytic is the better short; the 6 uF (after DC bias) ceramic barely conducts
    assert!(z("V(ZE)", 1e3) < z("V(ZC)", 1e3) - 0.1 && z("V(ZE)", 1e4) < z("V(ZC)", 1e4) / 3.0);
    // at 1 MHz the ceramic is ten times better; the electrolytic stays near its 0.3 ohm ESR and its ESL takes over above
    assert!(z("V(ZC)", 1e6) * 8.0 < z("V(ZE)", 1e6) && z("V(ZE)", 1e6) > 0.2 && z("V(ZE)", 1e7) > 0.5);
    // together they cover both ends, and never do worse than the better of the two by more than a little
    for f in [1e3, 1e4, 1e5, 1e6, 1e7] {
        assert!(z("V(ZP)", f) < z("V(ZE)", f).min(z("V(ZC)", f)) * 1.1, "{f}");
    }
}

/// The crystal sample (source -> crystal -> 50 ohm to GND): it passes only around its series resonance, which sits at
/// 16 MHz (the Arduino board's clock), and a deep notch follows just above it (the parallel resonance).
#[test]
fn crystal_sample_passes_only_near_16mhz_and_has_a_notch_just_above() {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "crystal_response.schjson").unwrap().1;
    let run = |rl: &str, f0: f64, f1: f64, ppd: usize| {
        let mut s = sch.clone();
        s.components.iter_mut().find(|c| c.refdes == "R1").unwrap().value = rl.into();
        let ac = ac_analysis(&from_schematic(&s).unwrap(), &AcSettings { fstart: f0, fstop: f1, sweep: AcSweep::Decade(ppd) }).unwrap();
        let mag = ac.signal("V(OUT)").unwrap().mag();
        (ac.freq.clone(), mag)
    };
    let peak = |f: &[f64], m: &[f64]| m.iter().copied().enumerate().fold((f[0], 0.0), |a, (i, x)| if x > a.1 { (f[i], x) } else { a });
    // narrow range, fine steps: series resonance 1 / (2 pi sqrt(Lm Cm)) = 16.0 MHz; only Rm = 40 ohm is left, so 50 / (50 + 40) = 0.556
    let (f, m) = run("50", 15.9e6, 16.1e6, 100_000);
    let (f_peak, g_peak) = peak(&f, &m);
    assert!((f_peak - 16e6).abs() < 2e3, "peak at {f_peak} Hz");
    assert!((0.5..0.6).contains(&g_peak), "peak gain {g_peak}");
    // the notch above it: the parallel resonance (a little higher, 16.027 MHz) blocks the line almost completely
    let (i_min, g_min) = m.iter().copied().enumerate().filter(|(i, _)| f[*i] > f_peak).fold((0, f64::MAX), |a, (i, x)| if x < a.1 { (i, x) } else { a });
    assert!((16.01e6..16.05e6).contains(&f[i_min]) && g_min < 0.01 * g_peak, "notch {g_min} at {}", f[i_min]);
    // the peak is about 1 kHz wide: 20 kHz away nothing passes
    let at = |f: &[f64], m: &[f64], hz: f64| m[f.iter().position(|&x| x >= hz).unwrap()];
    assert!(at(&f, &m, 15.98e6) < 0.05 && at(&f, &m, 16.0e6) > 0.4);
    // the wide view (1 MHz - 100 MHz, 100 points per decade) steps over it: nothing near 0.5 shows, only the small leak of the 3 pF
    let (fw, mw) = run("50", 1e6, 100e6, 100);
    assert!(mw.iter().all(|g| *g < 0.15), "wide max {}", mw.iter().copied().fold(0.0, f64::max));
    // a 5 kohm load makes the peak about 80 kHz wide and nearly 1 (the load, not the crystal, then sets the loss)
    let (f5, m5) = run("5k", 15.9e6, 16.1e6, 100_000);
    let (_, g5) = peak(&f5, &m5);
    assert!(g5 > 0.95 && at(&f5, &m5, 15.96e6) > 0.7 * g5, "5k: peak {g5}, 40 kHz below {}", at(&f5, &m5, 15.96e6));
    let _ = fw;
}

#[test]
fn cell_charge_sample_runs_cc_then_cv_and_stops_at_full() {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "li_ion_cell_charge.schjson").unwrap().1;
    let c = from_schematic(&sch).unwrap();
    let t = transient(&c, &TranSettings { tstop: 4.0 * 3600.0, tstep: 20.0, uic: false }).unwrap();
    let sig = |n: &str| t.signal(n).unwrap().values.clone();
    let (bat, chrg, stdby) = (sig("V(BAT)"), sig("V(CHRG)"), sig("V(STDBY)"));
    let at = |v: &[f64], h: f64| v[t.time.iter().position(|x| *x >= h * 3600.0 - 1e-9).unwrap_or(t.time.len() - 1)];
    // starts at 20 %: about 3.6 V open circuit plus 1 A through 0.06 ohm
    assert!((3.55..3.80).contains(&at(&bat, 0.0)), "start {}", at(&bat, 0.0));
    // constant current (1 A): the terminal voltage climbs steadily, status LED CHRG lit (low), STDBY off (high)
    assert!(at(&bat, 0.5) < at(&bat, 1.0) && at(&bat, 1.0) < at(&bat, 2.0));
    assert!(at(&chrg, 1.0) < 1.0 && at(&stdby, 1.0) > 2.0);
    // constant voltage: it sits at 4.2 V once full, and the charger reports done (CHRG off, STDBY lit)
    assert!((4.15..4.22).contains(&at(&bat, 4.0)) && at(&bat, 4.0) <= 4.2 + 1e-3, "end {}", at(&bat, 4.0));
    assert!(at(&chrg, 4.0) > 2.0 && at(&stdby, 4.0) < 1.0);
    // 80 % of 2600 mAh at 1 A: the end comes between 2 and 3.2 hours
    let done = t.time.iter().zip(&stdby).find(|(h, v)| **h > 3600.0 && **v < 1.0).map(|(h, _)| h / 3600.0).unwrap();
    assert!((2.0..3.2).contains(&done), "done after {done} h");
}

#[test]
fn tp5100_cell_charge_sample_runs_cc_then_cv_and_stops_at_full() {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "tp5100_cell_charge.schjson").unwrap().1;
    let t = transient(&from_schematic(&sch).unwrap(), &TranSettings { tstop: 4.0 * 3600.0, tstep: 20.0, uic: false }).unwrap();
    let sig = |n: &str| t.signal(n).unwrap().values.clone();
    let (bat, chrg, stdby, i) = (sig("V(BAT)"), sig("V(CHRG)"), sig("V(STDBY)"), sig("I(R1)"));
    let at = |v: &[f64], h: f64| v[t.time.iter().position(|x| *x >= h * 3600.0 - 1e-9).unwrap_or(t.time.len() - 1)];
    // 0.1 / 0.1 ohm = 1 A while the cell is below the float voltage, then the current tapers
    assert!((0.9..1.05).contains(&at(&i, 1.0)), "{}", at(&i, 1.0));
    assert!(at(&i, 3.0) < 0.2 && at(&i, 4.0) < 0.05);
    assert!(at(&bat, 0.5) < at(&bat, 1.0) && at(&bat, 1.0) < at(&bat, 2.0));
    assert!((4.15..4.22).contains(&at(&bat, 4.0)) && at(&bat, 4.0) <= 4.2 + 1e-3);
    assert!(at(&chrg, 1.0) < 1.0 && at(&stdby, 1.0) > 2.0 && at(&chrg, 4.0) > 2.0 && at(&stdby, 4.0) < 1.0);
}

/// A learner reads every warning: the bundled samples should not raise warnings that come from inside a model.
#[test]
fn shipped_examples_have_no_warnings_about_nodes_inside_models() {
    let mut bad = vec![];
    for (name, sch) in example_schematics() {
        let Ok(c) = from_schematic(&sch) else { continue };
        if let Ok(dc) = dc_op(&c) {
            for w in dc.warnings.iter().filter(|w| w.contains('.')) {
                bad.push(format!("{name}: {w}"));
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

/// The MOSFET lesson: off below the threshold, a low but non-zero drop when on, and a heavy load that runs the parts hot.
#[test]
fn mosfet_sample_switches_around_2v_and_the_lesson_numbers_hold() {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "mosfet_switch.schjson").unwrap().1;
    let run = |vg: &str, r2: &str| {
        let mut s = sch.clone();
        s.components.iter_mut().find(|c| c.refdes == "V2").unwrap().value = vg.into();
        s.components.iter_mut().find(|c| c.refdes == "R2").unwrap().value = r2.into();
        let dc = dc_op(&from_schematic(&s).unwrap()).unwrap();
        let issues = sim_core::check_ratings(&s, &dc);
        (dc.voltage("OUT").unwrap(), dc.element("Q1").unwrap().current, issues)
    };
    assert!((run("1.5", "47").0 - 5.0).abs() < 0.01, "off below the threshold");
    assert!((2.4..2.9).contains(&run("3", "47").0), "half on at 3 V");
    let (v, i, issues) = run("5", "47");
    assert!((0.2..0.4).contains(&v) && (0.09..0.11).contains(&i), "{v} {i}");
    assert!(issues.is_empty(), "the sample is within ratings: {issues:?}");
    let (v, i, issues) = run("5", "10");
    assert!((1.1..1.6).contains(&v) && (0.3..0.4).contains(&i), "{v} {i}");
    assert!(issues.iter().any(|x| x.refdes == "R2" && x.level == sim_core::RatingLevel::Over), "{issues:?}");
    assert!(issues.iter().any(|x| x.refdes == "Q1"), "{issues:?}");
}

/// The PNP lesson: on when the input is Low, partly on with a large base resistor, a hot load at 47 ohms.
#[test]
fn pnp_sample_switches_the_high_side_and_the_lesson_numbers_hold() {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "pnp_highside.schjson").unwrap().1;
    let run = |vin: &str, r1: &str, r2: &str| {
        let mut s = sch.clone();
        for (name, v) in [("V2", vin), ("R1", r1), ("R3", r2)] {
            s.components.iter_mut().find(|c| c.refdes == name).unwrap().value = v.into();
        }
        let dc = dc_op(&from_schematic(&s).unwrap()).unwrap();
        (dc.voltage("OUT").unwrap(), sim_core::check_ratings(&s, &dc))
    };
    let (on, issues) = run("0", "4.7k", "220");
    assert!((4.8..5.0).contains(&on) && issues.is_empty(), "{on} {issues:?}");
    assert!(run("5", "4.7k", "220").0 < 0.01, "off when the input is High");
    let (part, _) = run("0", "47k", "220");
    assert!((0.6..1.3).contains(&part), "partly on with a large base resistor: {part}");
    let (hot, issues) = run("0", "4.7k", "47");
    assert!(hot > 4.7, "{hot}");
    assert!(issues.iter().any(|x| x.refdes == "R3"), "{issues:?}");
}

/// The inverting amplifier lesson: gain -10 with 180 degrees of phase, a virtual ground, clipping at gain -100.
#[test]
fn inverting_amplifier_sample_gives_minus_ten_and_a_virtual_ground() {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "opamp_inverting.schjson").unwrap().1;
    let with_r1 = |r1: &str| {
        let mut s = sch.clone();
        s.components.iter_mut().find(|c| c.refdes == "R1").unwrap().value = r1.into();
        from_schematic(&s).unwrap()
    };
    let r = ac_analysis(&with_r1("1k"), &AcSettings { fstart: 10.0, fstop: 1e6, sweep: AcSweep::Decade(10) }).unwrap();
    let sg = r.signal("V(OUT)").unwrap();
    assert!((sg.db()[0] - 20.0).abs() < 0.1 && (sg.phase_deg()[0].abs() - 180.0).abs() < 1.0, "{} {}", sg.db()[0], sg.phase_deg()[0]);
    let peaks = |c: &sim_core::Circuit, node: &str| {
        let t = transient(c, &TranSettings { tstop: 3e-3, tstep: 5e-6, uic: false }).unwrap();
        let v = &t.signal(node).unwrap().values;
        (v.iter().cloned().fold(f64::MAX, f64::min), v.iter().cloned().fold(f64::MIN, f64::max))
    };
    let (lo, hi) = peaks(&with_r1("1k"), "V(OUT)");
    assert!((hi - 1.0).abs() < 0.05 && (lo + 1.0).abs() < 0.05, "{lo}..{hi}");
    let (nlo, nhi) = peaks(&with_r1("1k"), "V(N)");
    assert!(nlo.abs() < 0.003 && nhi.abs() < 0.003, "virtual ground {nlo}..{nhi}");
    // gain -100 clips against the supply and the inverting input leaves 0 V
    let (lo, hi) = peaks(&with_r1("100"), "V(OUT)");
    assert!(lo > -5.1 && lo < -4.5 && hi > 3.0 && hi < 4.0, "{lo}..{hi}");
    let (nlo, nhi) = peaks(&with_r1("100"), "V(N)");
    assert!(nhi.max(-nlo) > 0.05, "{nlo}..{nhi}");
}

/// "自動で再計算": a value that cannot be solved keeps the last good result instead of replacing it with an error.
#[test]
fn live_recalculation_keeps_the_last_good_result_when_a_value_fails() {
    let mut e = editor_with("rc_lowpass.schjson");
    e.set_tran_settings("2m", "10u");
    e.analyze_dc();
    e.analyze_tran();
    e.live = true;
    e.live_update();
    let r1 = e.sch.components.iter().position(|c| c.refdes == "R1").unwrap();
    e.sch.components[r1].value = "1.5k".into();
    e.live_update();
    assert!(e.tran.as_ref().unwrap().outcome.is_ok());
    // an unreadable value cannot be simulated: the old picture stays and the status says why
    e.sch.components[r1].value = "abc".into();
    e.live_update();
    assert!(e.dc.as_ref().unwrap().outcome.is_ok(), "the DC result is kept");
    assert!(e.tran.as_ref().unwrap().outcome.is_ok(), "the waveform is kept");
    assert!(e.status.contains("収束しませんでした") && e.status.contains("直前の結果"), "{}", e.status);
    // and the next good value updates again
    e.sch.components[r1].value = "2k".into();
    e.live_update();
    assert!(e.tran.as_ref().unwrap().outcome.is_ok());
}

/// The power-supply path's three new samples: the numbers the lessons quote.
#[test]
fn diode_zener_and_follower_samples_give_the_numbers_of_their_lessons() {
    let load = |n: &str| example_schematics().into_iter().find(|(m, _)| m == n).unwrap().1;
    let set = |s: &mut Schematic, r: &str, v: &str| s.components.iter_mut().find(|c| c.refdes == r).unwrap().value = v.into();
    let dc = |s: &Schematic| dc_op(&from_schematic(s).unwrap()).unwrap();
    // diodes: silicon flattens near 0.72 V, the Schottky near 0.30 V, at 3 V
    let d = load("diode_iv.schjson");
    let r = dc(&d);
    assert!((0.70..0.80).contains(&r.voltage("A1").unwrap()) && (0.28..0.38).contains(&r.voltage("A2").unwrap()));
    let drift = |t: f64| {
        let r = dc_op(&from_schematic(&d).unwrap().at_temperature(t)).unwrap();
        (r.voltage("A1").unwrap(), r.voltage("A2").unwrap())
    };
    assert!(drift(75.0).0 < drift(0.0).0 - 0.08 && drift(75.0).1 < drift(0.0).1 - 0.15, "hotter: lower Vf, the Schottky more");
    let mut t = d.clone();
    set(&mut t, "R1", "1.5k");
    let slow = dc(&t);
    assert!(slow.element("D1").unwrap().current < 0.003 && r.voltage("A1").unwrap() - slow.voltage("A1").unwrap() < 0.15, "10x less current, ~0.1 V less");
    // zener: ~5.2 V at 1k, falls apart at 100 ohm; the input moves the output only a little
    let z = load("zener_regulator.schjson");
    let vout = |s: &Schematic| dc(s).voltage("VOUT").unwrap();
    assert!((5.0..5.4).contains(&vout(&z)));
    let mut heavy = z.clone();
    set(&mut heavy, "R2", "100");
    assert!((3.3..3.9).contains(&vout(&heavy)), "starved zener");
    let mut mid = z.clone();
    set(&mut mid, "R2", "220");
    assert!((4.95..5.2).contains(&vout(&mid)));
    let (mut lo, mut hi) = (z.clone(), z.clone());
    set(&mut lo, "V1", "6");
    set(&mut hi, "V1", "15");
    assert!(vout(&hi) - vout(&lo) < 0.7 && vout(&hi) > vout(&lo), "input moves 9 V, output < 0.7 V");
    // emitter follower: Vz - 0.7 V, holds under load, then the transistor runs hot
    let f = load("emitter_follower.schjson");
    let r = dc(&f);
    assert!((4.3..4.5).contains(&r.voltage("VOUT").unwrap()) && (r.voltage("Z").unwrap() - r.voltage("VOUT").unwrap() - 0.7).abs() < 0.15);
    assert!(sim_core::check_ratings(&f, &r).is_empty(), "the sample itself is within ratings");
    let mut g = f.clone();
    set(&mut g, "R2", "47");
    let r47 = dc(&g);
    assert!(r.voltage("VOUT").unwrap() - r47.voltage("VOUT").unwrap() < 0.1);
    let issues = sim_core::check_ratings(&g, &r47);
    assert!(issues.iter().any(|i| i.refdes == "Q1" && i.level == sim_core::RatingLevel::Caution), "{issues:?}");
    set(&mut g, "R2", "22");
    let issues = sim_core::check_ratings(&g, &dc(&g));
    assert!(issues.iter().any(|i| i.refdes == "Q1" && i.level == sim_core::RatingLevel::Over), "{issues:?}");
}

/// A circuit saved before parts carried ratings still gets them: they are taken from the library by part name.
#[test]
fn old_circuits_borrow_the_ratings_of_their_library_parts() {
    let mut s = example_schematics().into_iter().find(|(n, _)| n == "emitter_follower.schjson").unwrap().1;
    for c in &mut s.components {
        if let Some(sym) = c.custom.as_mut() {
            sym.ratings = None;
        }
        if c.refdes == "R2" {
            c.value = "22".into();
        }
    }
    let mut e = SchEditor::default();
    e.sch = s;
    e.library = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).collect();
    e.run_dc();
    let issues = &e.dc.as_ref().unwrap().ratings;
    assert!(issues.iter().any(|i| i.refdes == "Q1" && i.message.contains("最大定格")), "{issues:?}");
    // without the library the old circuit has only the package rule of thumb (TO-92: 0.5 W), so the wording differs
    e.library.clear();
    e.run_dc();
    let issues = &e.dc.as_ref().unwrap().ratings;
    assert!(issues.iter().any(|i| i.refdes == "Q1" && i.message.contains("この形の目安")), "{issues:?}");
}

/// The series regulator sample (lesson 18): 5 V that does not care about the load, until the input is too low.
#[test]
fn series_regulator_sample_holds_5v_under_load_until_dropout() {
    let s = example_schematics().into_iter().find(|(n, _)| n == "series_regulator.schjson").unwrap().1;
    let run = |r2: &str, v1: &str| {
        let mut t = s.clone();
        for c in &mut t.components {
            match c.refdes.as_str() {
                "R2" => c.value = r2.into(),
                "V1" => c.value = v1.into(),
                _ => {}
            }
        }
        let r = dc_op(&from_schematic(&t).unwrap()).unwrap();
        let issues = sim_core::check_ratings(&t, &r);
        (r.voltage("VOUT").unwrap(), issues)
    };
    let (v, issues) = run("100", "9");
    assert!((4.9..5.1).contains(&v) && issues.is_empty(), "{v} {issues:?}");
    let (v47, issues) = run("47", "9");
    assert!((v - v47).abs() < 0.02, "load regulation: {v} vs {v47}");
    assert!(issues.iter().any(|i| i.refdes == "Q1" && i.level == sim_core::RatingLevel::Caution), "{issues:?}");
    let (v22, issues) = run("22", "9");
    assert!((v - v22).abs() < 0.02);
    assert!(issues.iter().any(|i| i.refdes == "Q1" && i.level == sim_core::RatingLevel::Over), "{issues:?}");
    let (hi, _) = run("100", "12");
    assert!((hi - v).abs() < 0.2, "line regulation: {hi} vs {v}");
    let (low, _) = run("100", "6");
    assert!(low < 4.0, "dropout: the pass transistor and op-amp need headroom, got {low}");
}

/// The 78xx sample (lesson 19): 5 V, the dropout, and the heat that (input - output) x current makes.
#[test]
fn regulator_78xx_sample_shows_dropout_and_heat() {
    let s = example_schematics().into_iter().find(|(n, _)| n == "regulator_78xx.schjson").unwrap().1;
    let run = |r1: &str, v1: &str| {
        let mut t = s.clone();
        for c in &mut t.components {
            match c.refdes.as_str() {
                "R1" => c.value = r1.into(),
                "V1" => c.value = v1.into(),
                _ => {}
            }
        }
        let r = dc_op(&from_schematic(&t).unwrap()).unwrap();
        let issues = sim_core::check_ratings(&t, &r);
        (r.voltage("VOUT").unwrap(), issues)
    };
    let (v, issues) = run("100", "12");
    assert!((4.9..5.1).contains(&v) && issues.is_empty(), "{v} {issues:?}");
    let (v22, issues) = run("22", "12");
    assert!((v - v22).abs() < 0.05);
    assert!(issues.iter().any(|i| i.refdes == "U1" && i.level == sim_core::RatingLevel::Caution), "{issues:?}");
    let (v10, issues) = run("10", "12");
    assert!((v - v10).abs() < 0.05);
    assert!(issues.iter().any(|i| i.refdes == "U1" && i.level == sim_core::RatingLevel::Over), "{issues:?}");
    // lesson 20: lowering the input to 8 V turns the red warning into the orange one
    let (v8, issues) = run("10", "8");
    assert!((v - v8).abs() < 0.05);
    assert!(issues.iter().any(|i| i.refdes == "U1" && i.level == sim_core::RatingLevel::Caution), "{issues:?}");
    assert!(!issues.iter().any(|i| i.refdes == "U1" && i.level == sim_core::RatingLevel::Over), "{issues:?}");
    let (low, _) = run("100", "6");
    assert!((3.8..4.2).contains(&low), "dropout 2 V: input - 2 V, got {low}");
}

/// The variable supply sample (lesson 23): the dial R2 sets the output, until the smoothed DC (minus 2 V) is out of reach.
#[test]
fn variable_supply_sample_follows_the_dial_until_headroom_runs_out() {
    let s = example_schematics().into_iter().find(|(n, _)| n == "variable_supply.schjson").unwrap().1;
    let run = |r2: &str, r3: &str| {
        let mut t = s.clone();
        for c in &mut t.components {
            match c.refdes.as_str() {
                "R2" => c.value = r2.into(),
                "R3" => c.value = r3.into(),
                _ => {}
            }
        }
        let tr = transient(&from_schematic(&t).unwrap(), &TranSettings { tstop: 0.3, tstep: 100e-6, uic: false }).unwrap();
        wave_measure(&tr.time, &tr.signal("V(VOUT)").unwrap().values, 0.2, 0.3).unwrap()
    };
    let mid = run("1.2k", "100");
    assert!((7.3..7.9).contains(&((mid.max + mid.min) / 2.0)) && mid.pp < 0.05, "{mid:?}");
    let high = run("2k", "100");
    assert!((11.4..12.2).contains(&((high.max + high.min) / 2.0)) && high.pp < 0.05, "{high:?}");
    // 3k asks for 17 V but the smoothed DC is only about 15 V: the output follows the ripple
    let over = run("3k", "100");
    assert!(over.max < 14.0 && over.pp > 0.3, "{over:?}");
    let low = run("100", "100");
    assert!((1.7..2.1).contains(&((low.max + low.min) / 2.0)), "{low:?}");
}

/// The protection sample (lesson 21): R1 bare, R2 behind a Schottky diode, R3 behind a P-channel MOSFET.
#[test]
fn reverse_protection_sample_blocks_a_backwards_supply() {
    let s = example_schematics().into_iter().find(|(n, _)| n == "reverse_protection.schjson").unwrap().1;
    let run = |v1: &str, r2: &str| {
        let mut t = s.clone();
        for c in &mut t.components {
            match c.refdes.as_str() {
                "V1" => c.value = v1.into(),
                "R2" => c.value = r2.into(),
                _ => {}
            }
        }
        let r = dc_op(&from_schematic(&t).unwrap()).unwrap();
        let issues = sim_core::check_ratings(&t, &r);
        (r, issues)
    };
    let (r, issues) = run("9", "330");
    let (d, m) = (r.voltage("OUT_D").unwrap(), r.voltage("OUT_M").unwrap());
    assert!((8.5..8.8).contains(&d) && (8.95..9.01).contains(&m), "diode {d} V, MOSFET {m} V");
    assert!(issues.is_empty(), "{issues:?}");
    // backwards: the bare load sees -9 V, both protected loads see nothing
    let (r, _) = run("-9", "330");
    assert!(r.voltage("OUT_D").unwrap().abs() < 0.01 && r.voltage("OUT_M").unwrap().abs() < 0.01);
    assert!(r.voltage("VIN").unwrap() < -8.9, "the bare load is wired straight to the supply");
    assert!(9.0 - d > 0.2 && 9.0 - m < 0.02, "the diode loses ~0.3 V, the MOSFET almost nothing");
    // a short behind the diode: a huge current, the rating check complains about D1
    let (_, issues) = run("9", "1");
    assert!(issues.iter().any(|i| i.refdes == "D1" && i.level == sim_core::RatingLevel::Over), "{issues:?}");
}

/// The relay-coil sample: half the turns of L1 shorted takes the driver past its 200 mA, then the turns burn.
#[test]
fn chain_failure_on_the_coil_example_runs_from_the_panel() {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "coil_layer_short.schjson").unwrap().1;
    let mut e = SchEditor::default();
    e.sch = sch;
    e.tran_tstop = "10m".into();
    e.tran_tstep = "5u".into();
    e.set_fault("L1", 9); // ALL_FAULTS[9] = 50% of the turns
    e.analyze_fault_chain();
    let r = e.fault_chain.as_ref().unwrap().as_ref().unwrap_or_else(|m| panic!("{m:?}"));
    let text = r.summary().join("\n");
    assert!(text.contains("Q1") && text.contains("L1") && text.contains("ヒューズ"), "{text}");
    // a pin fault cannot be chained: the panel says so instead of guessing
    e.fault_pin = "1".into();
    e.analyze_fault_chain();
    assert!(e.fault_chain.as_ref().unwrap().is_err());
}

/// The electrolytic output capacitor of the buck sample: its ripple current and life come out of the run,
/// from the part's data sheet figures (the sample was saved before the part had them: the library supplies them).
#[test]
fn the_electrolytic_capacitor_of_the_buck_sample_reports_ripple_current_and_life() {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "buck_cout_electrolytic.schjson").unwrap().1;
    let mut e = SchEditor::default();
    e.library = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).collect();
    e.adopt(sch, None);
    e.set_tran_settings("2m", "200n");
    e.analyze_tran();
    let parts = &e.tran.as_ref().unwrap().parts;
    assert_eq!(parts.len(), 1, "{parts:?}");
    let r = &parts[0];
    assert_eq!(r.refdes, "C1");
    // the 150 kHz ripple of the coil current flows in the capacitor: tens of mA to a few hundred mA rms
    assert!(r.i_rms > 0.02 && r.i_rms < 1.0, "{}", r.i_rms);
    // counted from the zero crossings of the current (a few of the ripple's cycles are lost in the switching noise)
    assert!((100e3..200e3).contains(&r.freq), "{}", r.freq);
    assert!(r.allowed.is_some() && r.life_hours.is_some() && r.core_temp_c >= 27.0);
    assert!(r.message().contains("C1") && r.life_text().is_some());
}

/// With the board's copper resistance in front of a pin, the pin shows the voltage that reaches it, not the net's.
#[test]
fn pin_voltage_is_taken_behind_the_wiring_resistance() {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "divider_tolerance.schjson").unwrap().1;
    let mut e = SchEditor::default();
    e.sch = sch;
    e.run_dc();
    let before = e.pin_voltage("R1", "2").unwrap();
    e.wiring = vec![sim_core::WiringR { refdes: "R1".into(), pin: "2".into(), ohm: 1000.0, henry: 0.0, farad: 0.0 }];
    e.run_dc();
    let after = e.pin_voltage("R1", "2").unwrap();
    assert!((before - after).abs() > 1e-3, "{before} {after}");
    assert!(e.pin_voltage("R1", "1").is_some());
}

/// Behind the wiring resistance, the wire that ends at the pin is coloured with the pin's voltage, not the net's.
#[test]
fn wire_colours_follow_the_pin_voltage_behind_the_wiring_resistance() {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "divider_tolerance.schjson").unwrap().1;
    let mut e = SchEditor::default();
    e.sch = sch;
    e.run_dc();
    let pt = {
        let c = e.sch.components.iter().find(|c| c.refdes == "R1").unwrap();
        c.pins().into_iter().find(|(n, _)| n == "2").unwrap().1
    };
    let wire = e.sch.wires.iter().position(|w| w.a == pt || w.b == pt).expect("a wire ends at R1.2");
    let net_v = e.dc.as_ref().unwrap().wire_v[wire].unwrap();
    e.wiring = vec![sim_core::WiringR { refdes: "R1".into(), pin: "2".into(), ohm: 1000.0, henry: 0.0, farad: 0.0 }];
    e.run_dc();
    let dc = e.dc.as_ref().unwrap();
    let pin_v = dc.pin_v.iter().find(|(p, _)| *p == pt).unwrap().1;
    let w = &e.sch.wires[wire];
    let other = if w.a == pt { w.b } else { w.a };
    let expect = match dc.pin_v.iter().find(|(p, _)| *p == other) {
        Some((_, v)) => (pin_v + v) / 2.0,
        None => pin_v,
    };
    assert!((dc.wire_v[wire].unwrap() - expect).abs() < 1e-9);
    assert!((pin_v - net_v).abs() > 1e-3, "the pin is behind a resistor, so it differs from the net: {pin_v} {net_v}");
}

/// The scan on its own thread gives the rows of the scan on the calling thread, in the same order, and can be cancelled.
#[test]
fn the_fault_scan_on_its_own_thread_gives_the_same_rows_and_can_be_cancelled() {
    use std::sync::atomic::Ordering;
    let sch = example_schematics().into_iter().find(|(n, _)| n == "series_regulator.schjson").unwrap().1;
    let mut e = SchEditor::default();
    e.sch = sch;
    e.analyze_fault_scan();
    let rows = |e: &SchEditor| e.fault_scan.iter().map(|r| (r.part.clone(), r.pin.clone(), r.label.clone(), r.severity.to_bits(), r.danger)).collect::<Vec<_>>();
    let blocking = rows(&e);
    assert!(blocking.len() >= 4);
    // on a thread
    e.fault_scan.clear();
    e.start_fault_scan();
    assert!(e.scan_run.is_some());
    let ctx = egui::Context::default();
    let t0 = std::time::Instant::now();
    while e.scan_run.is_some() {
        e.poll_fault_scan(&ctx);
        assert!(t0.elapsed().as_secs() < 60, "the scan did not finish");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(rows(&e), blocking);
    // cancelled at once: nothing is taken over, and the old rows stay
    e.start_fault_scan();
    e.scan_run.as_ref().unwrap().cancel.store(true, Ordering::Relaxed);
    while e.scan_run.is_some() {
        e.poll_fault_scan(&ctx);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(e.status.contains("中止"), "{}", e.status);
    assert_eq!(rows(&e), blocking);
}

/// The scan tries the faults that usually happen; the ones that hardly ever do (a resistor shorting) come only with the box ticked.
#[test]
fn the_fault_scan_leaves_out_the_rare_faults_unless_asked() {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "series_regulator.schjson").unwrap().1;
    let mut e = SchEditor::default();
    e.sch = sch;
    e.analyze_fault_scan();
    let usual = e.fault_scan.len();
    // each row says which node moved most (what the table shows: "VOUT が 5V → 0V")
    assert!(e.fault_scan.iter().any(|r| r.severity > 0.01 && r.worst.as_ref().is_some_and(|(_, a, b)| (b - a).abs() > 0.01)));
    assert!(!e.fault_scan.iter().any(|r| r.pin.is_empty() && r.label == "値が 10 倍" && r.part.starts_with('R')), "a resistor ten times larger is rare");
    assert!(e.fault_scan.iter().any(|r| r.pin.is_empty() && r.part.starts_with('R') && r.label.contains("断線")), "an open resistor is not");
    e.scan_rare = true;
    e.analyze_fault_scan();
    assert!(e.fault_scan.len() > usual, "{} vs {usual}", e.fault_scan.len());
}

/// The two-sheet sample is the regulator sample cut in two: every net voltage must come out the same.
#[test]
fn two_sheet_sample_simulates_like_the_single_sheet_one() {
    let all = example_schematics();
    let find = |n: &str| all.iter().find(|(f, _)| f == n).unwrap().1.clone();
    let (one, two) = (find("regulator_led.schjson"), find("two_sheets.schjson"));
    assert_eq!(two.sheet_count(), 2);
    let dc = |s: &sch_core::Schematic| sim_core::dc_op(&sim_core::from_schematic(s).unwrap()).unwrap();
    let (a, b) = (dc(&one), dc(&two));
    let led = |r: &sim_core::DcResult, s: &sch_core::Schematic| {
        let d = s.flat().components.iter().find(|c| c.kind == sch_core::Kind::Led).unwrap().refdes.clone();
        r.element(&d).unwrap().current
    };
    assert!((led(&a, &one) - led(&b, &two)).abs() < 1e-9, "{} vs {}", led(&a, &one), led(&b, &two));
    assert!(led(&b, &two) > 5e-3, "the LED lights");
    assert!(erc_errors(&two).is_empty(), "{:?}", erc_errors(&two));
}

fn erc_errors(s: &sch_core::Schematic) -> Vec<String> {
    sch_core::erc(s).into_iter().filter(|p| p.severity == sch_core::Severity::Error).map(|p| p.message).collect()
}

/// A part on another sheet can be broken too: the fault tools look at every sheet, not only the open one.
#[test]
fn fault_simulation_reaches_parts_on_the_other_sheets() {
    let mut e = SchEditor::default();
    e.sch = example_schematics().into_iter().find(|(n, _)| n == "two_sheets.schjson").unwrap().1;
    assert!(e.sch.components.iter().all(|c| c.refdes != "R1"), "R1 is on the sheet that is not open");
    e.set_fault("R1", 0);
    e.analyze_fault();
    let report = e.fault_view.as_ref().unwrap().report.as_ref().unwrap_or_else(|m| panic!("{m:?}"));
    assert!(report.summary().iter().any(|l| l.contains("電流が止まります") || l.contains("→")), "{:?}", report.summary());
    e.analyze_fault_scan();
    assert!(e.fault_scan.iter().any(|f| f.part == "R1"), "the scan includes the other sheet's parts");
}

/// The currents of a DC analysis fill what the table of requirements leaves empty (the "解析から入れる" button of the window).
#[test]
fn the_dc_currents_of_the_buck_sample_suggest_what_the_table_lacks() {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "buck_converter.schjson").unwrap().1;
    let mut e = SchEditor::default();
    e.sch = sch;
    assert!(!e.sch.requirements.is_empty(), "the sample carries a table");
    for r in &mut e.sch.requirements.net_rules {
        r.current = 0.0;
    }
    let currents = e.dc_net_currents().expect("the buck sample has a DC operating point");
    let found = design_spec::suggest_from_currents(&e.sch.requirements, &currents);
    assert!(found.iter().any(|s| matches!(s.fill, design_spec::Fill::RowCurrent { .. })), "{found:?}");
    for s in &found {
        s.apply(&mut e.sch.requirements);
    }
    assert!(e.sch.requirements.net_rules.iter().any(|r| r.current > 0.0));
    assert!(design_spec::suggest_from_currents(&e.sch.requirements, &currents).is_empty(), "nothing is left to suggest");
}
