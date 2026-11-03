use super::lessons::value_of;
use sch_core::extract;
use super::*;

fn progress(e: &SchEditor) -> usize {
    e.lesson.as_ref().map_or(0, |l| l.done.iter().filter(|d| **d).count())
}

fn set_value(e: &mut SchEditor, refdes: &str, v: &str) {
    e.sch.components.iter_mut().find(|c| c.refdes == refdes).unwrap().value = v.into();
}

#[test]
fn every_lesson_loads_and_its_circuit_simulates() {
    let titles = lesson_titles();
    assert!(titles.len() >= 5);
    for (i, title) in titles.iter().enumerate() {
        let mut e = SchEditor::default();
        e.start_lesson(i);
        assert!(e.lesson.is_some(), "{}", title);
        assert!(!e.sch.components.is_empty(), "{}", titles[i]);
        // the 555 is a circuit that changes state by itself: it only makes sense in the time domain
        if titles[i].contains("555") || titles[i].contains("整流") {
            e.run_tran();
            assert!(e.tran.as_ref().unwrap().outcome.is_ok(), "{}", e.status);
        } else {
            e.run_dc();
            assert!(e.dc.as_ref().unwrap().outcome.is_ok(), "{}: {:?}", titles[i], e.dc.as_ref().unwrap().outcome.as_ref().err());
        }
        assert_eq!(progress(&e), 0);
    }
}

#[test]
fn the_divider_lesson_ticks_steps_as_the_learner_acts() {
    let mut e = SchEditor::default();
    e.start_lesson(0);
    e.update_lesson();
    assert_eq!(progress(&e), 0, "nothing done yet");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    // changing R2 without measuring again does not count
    set_value(&mut e, "R2", "30k");
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    e.live = true;
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    e.run_mc();
    e.update_lesson();
    assert_eq!(progress(&e), 4, "{}", e.status);
    // a done step stays done even if the circuit is edited afterwards
    set_value(&mut e, "R1", "1k");
    e.update_lesson();
    assert_eq!(progress(&e), 4);
}

#[test]
fn the_filter_and_555_lessons_run_with_their_prepared_settings() {
    let mut e = SchEditor::default();
    e.start_lesson(2);
    e.run_ac();
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    set_value(&mut e, "C1", "1.59u");
    e.run_ac();
    e.update_lesson();
    assert_eq!(progress(&e), 2);

    let mut e = SchEditor::default();
    e.start_lesson(4);
    e.run_tran();
    assert!(e.tran.as_ref().unwrap().outcome.is_ok(), "{}", e.status);
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    set_value(&mut e, "C1", "1u");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
}

#[test]
fn restarting_restores_the_original_circuit() {
    let mut e = SchEditor::default();
    e.start_lesson(1);
    set_value(&mut e, "R1", "100");
    e.start_lesson(1);
    assert_eq!(value_of(&e.sch, "R1"), Some("330"));
    assert!(e.undo.is_empty());
}

fn net_of(e: &SchEditor, refdes: &str, pin: &str) -> String {
    extract(&e.sch).into_iter().find(|n| n.pins.iter().any(|p| p.refdes == refdes && p.pin == pin)).unwrap().name
}

#[test]
fn the_ringing_lesson_needs_a_fresh_run_after_the_change() {
    let mut e = SchEditor::default();
    let idx = lesson_titles().iter().position(|t| t.contains("リンギング")).unwrap();
    e.start_lesson(idx);
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    // changing R1 alone does not tick the step: the waveform on screen is stale
    set_value(&mut e, "R1", "100");
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    set_value(&mut e, "R1", "20");
    e.mc_out = net_of(&e, "C1", "1");
    e.mc_kind = McKind::Tran;
    e.mc_metric = TranMetric::Overshoot;
    e.mc_runs = "20".into();
    e.run_mc();
    e.update_lesson();
    assert_eq!(progress(&e), 3, "{}", e.status);
    let r = e.mc.as_ref().unwrap().outcome.as_ref().unwrap();
    // zeta = 0.316 => exp(-pi zeta / sqrt(1 - zeta^2)) = 35 %
    assert!((r.nominal - 35.0).abs() < 3.0, "{}", r.nominal);
    assert_eq!(r.unit, "%");
}

#[test]
fn the_charger_lesson_runs_and_ticks_its_steps() {
    let mut e = SchEditor::default();
    let idx = lesson_titles().iter().position(|t| t.contains("TP4056")).unwrap();
    e.start_lesson(idx);
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    set_value(&mut e, "R1", "12k");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    set_value(&mut e, "R1", "6k");
    set_value(&mut e, "C1", "470u IC=3.6 ESR=0.5");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 3, "{}", e.status);
}

#[test]
fn the_left_panel_follows_the_lesson_between_drawing_and_analysis() {
    let mut e = SchEditor::default();
    assert!(e.panel_tab == PanelTab::Draw, "a fresh editor starts on the drawing page");
    e.start_lesson(0);
    e.update_lesson();
    assert!(e.panel_tab == PanelTab::Analyze, "step 1 runs an analysis");
    e.run_dc();
    e.update_lesson();
    assert!(e.panel_tab == PanelTab::Draw, "step 2 changes a value");
    // the learner may look around; the page changes again only when the step does
    e.panel_tab = PanelTab::Analyze;
    e.update_lesson();
    assert!(e.panel_tab == PanelTab::Analyze);
    set_value(&mut e, "R2", "30k");
    e.run_dc();
    e.update_lesson();
    assert!(e.panel_tab == PanelTab::Draw, "step 3 is the slider");
}

#[test]
fn the_opamp_lesson_shows_gain_clipping_and_bandwidth() {
    let mut e = SchEditor::default();
    let idx = lesson_titles().iter().position(|t| t.contains("非反転増幅")).unwrap();
    e.start_lesson(idx);
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    let peak = |e: &SchEditor, name: &str| {
        let r = e.tran.as_ref().unwrap().outcome.as_ref().unwrap();
        let s = r.signals.iter().find(|s| s.name == name).unwrap_or_else(|| panic!("{:?}", r.signals.iter().map(|s| s.name.clone()).collect::<Vec<_>>()));
        s.values.iter().fold(0.0f64, |m, v| m.max(v.abs()))
    };
    let gain = peak(&e, "V(OUT)") / peak(&e, "V(IN)");
    assert!((gain - 11.0).abs() < 0.5, "gain {gain}");
    set_value(&mut e, "R1", "100k");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    // the output cannot reach 101 x 0.1 V: the supply limits it
    assert!(peak(&e, "V(OUT)") < 5.0, "{}", peak(&e, "V(OUT)"));
    set_value(&mut e, "R1", "10k");
    e.run_ac();
    e.update_lesson();
    assert_eq!(progress(&e), 3, "{}", e.status);
}

/// The TP5100 sample charges its 470 uF "cell" through pre-charge, constant current and the taper.
#[test]
fn the_tp5100_sample_charges_the_cell_to_4v2() {
    let mut e = SchEditor::default();
    assert!(e.load_sample(include_str!("../../../../docs/examples/tp5100_charger.schjson")));
    e.set_tran_settings("8m", "10u");
    e.run_tran();
    let r = e.tran.as_ref().unwrap().outcome.as_ref().unwrap_or_else(|err| panic!("{err:?}"));
    let last = |name: &str| *r.signals.iter().find(|s| s.name == name).unwrap_or_else(|| panic!("{name}")).values.last().unwrap();
    assert!((last("V(BAT)") - 4.2).abs() < 0.1, "BAT ends at {}", last("V(BAT)"));
    assert_settled(r, "V(LX)");
    assert!(last("V(STDBY)") < 1.0 && last("V(CHRG)") > 7.0, "full: STDBY low, CHRG high");
    let bat = r.signals.iter().find(|s| s.name == "V(BAT)").unwrap();
    assert!(bat.values.iter().cloned().fold(0.0, f64::max) < 4.3, "no overshoot");
}

/// The IP2332 sample charges the cell at 1 A (RISET 4.3k): LED1 while charging, LED2 when it is full.
#[test]
fn the_ip2332_sample_charges_the_cell_and_lights_the_leds() {
    let mut e = SchEditor::default();
    assert!(e.load_sample(include_str!("../../../../docs/examples/ip2332_charger.schjson")));
    e.set_tran_settings("4m", "10u");
    e.run_tran();
    let r = e.tran.as_ref().unwrap().outcome.as_ref().unwrap_or_else(|err| panic!("{err:?}"));
    let sig = |name: &str| &r.signals.iter().find(|s| s.name == name).unwrap_or_else(|| panic!("{name}")).values;
    let at = |name: &str, t: f64| sig(name)[r.time.iter().position(|x| *x >= t).unwrap()];
    assert!((sig("V(BAT)").last().unwrap() - 4.2).abs() < 0.1);
    assert_settled(r, "V(LX)");
    assert!(sig("V(BAT)").iter().cloned().fold(0.0, f64::max) < 4.3, "no overshoot");
    // charging at 0.5 ms: LED1 lit (a 5 mA source, about the LED's forward voltage), LED2 off
    assert!(at("V(LED1)", 0.5e-3) > 1.5 && at("V(LED2)", 0.5e-3) < 0.5, "{} {}", at("V(LED1)", 0.5e-3), at("V(LED2)", 0.5e-3));
    // full at the end: LED2 lit, LED1 off (only a nanoamp of leakage, far below the LED's forward voltage)
    let (l1, l2) = (*sig("V(LED1)").last().unwrap(), *sig("V(LED2)").last().unwrap());
    assert!(l2 > 2.0 && l1 < 1.2, "LED1 {l1} LED2 {l2} BAT {}", sig("V(BAT)").last().unwrap());
}

/// After the charge has settled (2 ms on) a node must not ring from step to step: a numerical
/// oscillation of the averaged buck model would show up as a thick band in the waveform viewer.
fn assert_settled(r: &sim_core::TranResult, name: &str) {
    let v = &r.signals.iter().find(|s| s.name == name).unwrap().values;
    let i0 = r.time.iter().position(|t| *t >= 2e-3).unwrap();
    let (mn, mx) = v[i0..].iter().fold((f64::MAX, f64::MIN), |a, x| (a.0.min(*x), a.1.max(*x)));
    assert!(mx - mn < 0.05, "{name} rings by {} V", mx - mn);
}

#[test]
fn every_lesson_says_where_the_model_differs_from_the_real_part() {
    for (i, title) in lesson_titles().iter().enumerate() {
        let notes = super::lessons::lesson_limits(i);
        assert!(notes.len() >= 3, "{title}: only {} notes", notes.len());
        for n in notes {
            assert!(n.chars().count() > 20 && n.chars().count() < 400, "{title}: {n}");
            assert!(n.ends_with('。'), "{title}: a note ends with 。: {n}");
        }
    }
}

#[test]
fn the_notes_match_what_the_models_really_assume() {
    // lesson 2: the LED model has a forward voltage of about 2 V at 20 mA
    let mut e = SchEditor::default();
    e.start_lesson(1);
    e.run_dc();
    let d = e.dc.as_ref().unwrap().outcome.as_ref().unwrap();
    let led = d.elements.iter().find(|x| x.name == "D1").unwrap();
    assert!((1.8..2.2).contains(&led.voltage), "LED forward voltage {}", led.voltage);
    // lesson 4: the 2N3904 has a fixed gain of 300, and its model has no capacitance at all
    let mut e = SchEditor::default();
    e.start_lesson(3);
    let c = sim_core::from_schematic(&e.sch).unwrap_or_else(|_| panic!("lesson 4 converts"));
    let q = c.elements.iter().find_map(|x| match x {
        sim_core::Element::Bjt { model, .. } => Some(model.clone()),
        _ => None,
    });
    assert_eq!(q.unwrap().bf, 300.0);
    assert!(!c.elements.iter().any(|x| matches!(x, sim_core::Element::Capacitor { .. })), "no capacitor appears next to the transistor");
}

#[test]
fn a_library_part_shows_its_model_notes_in_the_tooltip_and_old_circuits_borrow_them() {
    let e = SchEditor::default();
    let tp = e.library.iter().find(|d| d.name == "TP4056").expect("TP4056 is bundled");
    let tip = super::panel::lib_hover_for_test(tp);
    assert!(tip.contains("このモデルの限界") && tip.contains("再充電のしきい値"), "{tip}");
    // a part without a model has nothing to add to its description
    let mcu = e.library.iter().find(|d| d.sim.is_none() && !d.land_unverified).unwrap();
    assert_eq!(super::panel::lib_hover_for_test(mcu), mcu.description);
    // a part whose pads are an estimate says so
    let adc = e.library.iter().find(|d| d.name == "ADS124S08").expect("ADS124S08 is bundled");
    assert!(super::panel::lib_hover_for_test(adc).contains("ランドパターン"));
}

/// The three power-supply lessons tick their steps as the learner acts and carry a real-experiment card.
#[test]
fn the_power_supply_lessons_walk_their_steps() {
    let idx = |id: &str| lesson_ids().iter().position(|l| *l == id).unwrap();
    // diode: a sweep, a temperature sweep, then R1 changed and measured
    let mut e = SchEditor::default();
    e.start_lesson(idx("diode_curve"));
    e.run_sweep();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    e.run_temp_sweep();
    e.update_lesson();
    assert_eq!(progress(&e), 2, "{}", e.status);
    set_value(&mut e, "R1", "1.5k");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    assert_eq!(e.finished_lesson(), Some("diode_curve"));
    // zener: DC, a heavy load measured, then the input sweep
    let mut e = SchEditor::default();
    e.start_lesson(idx("zener"));
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    set_value(&mut e, "R2", "100");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    set_value(&mut e, "R2", "1k");
    e.run_sweep();
    e.update_lesson();
    assert_eq!(progress(&e), 3, "{}", e.status);
    // follower: DC, load 47, load 22 (only a load of 30 ohm or less counts for the last step)
    let mut e = SchEditor::default();
    e.start_lesson(idx("emitter_follower"));
    e.run_dc();
    e.update_lesson();
    set_value(&mut e, "R2", "47");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    e.update_lesson();
    assert_eq!(progress(&e), 2, "47 ohm is not enough for the last step");
    set_value(&mut e, "R2", "22");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    // series regulator: DC, load 47, load 22, back to 100 and the input sweep
    let mut e = SchEditor::default();
    e.start_lesson(idx("series_regulator"));
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    set_value(&mut e, "R2", "47");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    set_value(&mut e, "R2", "22");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    set_value(&mut e, "R2", "100");
    e.run_sweep();
    e.update_lesson();
    assert_eq!(progress(&e), 4, "{}", e.status);
    // 78xx: DC, load 22, load 10 (only 12 ohm or less counts for the last load step), then the input sweep
    let mut e = SchEditor::default();
    e.start_lesson(idx("regulator_78xx"));
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    set_value(&mut e, "R1", "22");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    e.update_lesson();
    assert_eq!(progress(&e), 2, "22 ohm is not enough for the heat step");
    set_value(&mut e, "R1", "10");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    set_value(&mut e, "R1", "100");
    e.run_sweep();
    e.update_lesson();
    assert_eq!(progress(&e), 4, "{}", e.status);
    // heat: heavy load, the heat calculator, then a lower input
    let mut e = SchEditor::default();
    e.start_lesson(idx("heat"));
    set_value(&mut e, "R1", "10");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    e.open_wizard(super::wizard::CALCS.iter().position(|c| c.title.starts_with("熱")).unwrap());
    e.wiz_open = true;
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    set_value(&mut e, "V1", "8");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3, "{}", e.status);
    // protection: DC, backwards supply, a shorted branch, then the fuse calculator
    let mut e = SchEditor::default();
    e.start_lesson(idx("protection"));
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    set_value(&mut e, "V1", "-9");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2, "{}", e.status);
    set_value(&mut e, "V1", "9");
    set_value(&mut e, "R2", "1");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3, "{}", e.status);
    e.open_wizard(super::wizard::CALCS.iter().position(|c| c.title.starts_with("ヒューズ")).unwrap());
    e.wiz_open = true;
    e.update_lesson();
    assert_eq!(progress(&e), 4);
    // every new lesson has a real-experiment card
    for id in ["diode_curve", "zener", "emitter_follower", "series_regulator", "regulator_78xx", "heat", "protection"] {
        assert!(!super::lessons::lesson_real(idx(id)).is_empty(), "{id}");
    }
}

/// Lesson 23: transient, turn the dial up, turn it too far, then the heat calculator.
#[test]
fn the_variable_supply_lesson_walks_its_steps() {
    let idx = lesson_ids().iter().position(|l| *l == "variable_supply").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    set_value(&mut e, "R2", "2k");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 2, "{}", e.status);
    e.update_lesson();
    assert_eq!(progress(&e), 2, "2k is not too far");
    set_value(&mut e, "R2", "3k");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 3, "{}", e.status);
    e.open_wizard(super::wizard::CALCS.iter().position(|c| c.title.starts_with("熱")).unwrap());
    e.wiz_open = true;
    e.update_lesson();
    assert_eq!(progress(&e), 4);
    assert!(!super::lessons::lesson_real(idx).is_empty());
}

/// Lesson 22: run the transient, trace a 10% layer short (the coil burns alone), then 50% (the driver dies first), then the fuse calculator.
#[test]
fn the_coil_layer_short_lesson_walks_its_steps() {
    let idx = lesson_ids().iter().position(|l| *l == "coil_layer_short").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    // a bigger short does not satisfy the "small short" step
    e.set_fault("L1", 9);
    e.analyze_fault_chain();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "the 50% trace is not a small short");
    e.set_fault("L1", 7);
    e.analyze_fault_chain();
    e.update_lesson();
    assert_eq!(progress(&e), 2, "{}", e.status);
    e.set_fault("L1", 9);
    e.analyze_fault_chain();
    e.update_lesson();
    assert_eq!(progress(&e), 3, "{}", e.status);
    e.open_wizard(super::wizard::CALCS.iter().position(|c| c.title.starts_with("ヒューズ")).unwrap());
    e.wiz_open = true;
    e.update_lesson();
    assert_eq!(progress(&e), 4);
    assert!(!super::lessons::lesson_real(idx).is_empty());
}

/// The crystal lesson: the three steps tick as the learner runs the narrow sweep, widens it, and changes the load.
#[test]
fn the_crystal_lesson_ticks_its_steps_and_the_wide_sweep_misses_the_peak() {
    let idx = lesson_ids().iter().position(|l| *l == "crystal").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    e.analyze_ac();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    // the narrow run shows the peak: about 0.5 at 16 MHz
    let peak = |e: &SchEditor| e.ac.as_ref().unwrap().outcome.as_ref().unwrap().signal("V(OUT)").unwrap().mag().iter().copied().fold(0.0, f64::max);
    assert!((0.5..0.6).contains(&peak(&e)), "{}", peak(&e));
    // widening without running again does not count; running the wide sweep does, and it misses the peak
    e.set_ac_settings("1Meg", "100Meg", "100");
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    e.analyze_ac();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    assert!(peak(&e) < 0.15, "{}", peak(&e));
    // changing R1 while the sweep is still wide does not finish the last step
    set_value(&mut e, "R1", "5k");
    e.analyze_ac();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    e.set_ac_settings("15.9Meg", "16.1Meg", "100000");
    e.analyze_ac();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    assert!(peak(&e) > 0.95, "{}", peak(&e));
}

/// Adds the clamp diode of the reset lesson: anode on RESET, cathode on VCC (labels, like the sample's own wiring).
fn add_reset_clamp(s: &mut Schematic) {
    let d = s.add(sch_core::Kind::Diode, (44, 0), 1);
    for (pin, net) in [("A", "RESET"), ("K", "VCC")] {
        let (_, pt) = s.components[d].pins().into_iter().find(|(n, _)| n == pin).unwrap();
        let c = s.components[d].pos;
        let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
        let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
        s.wires.push(sch_core::Wire { a: pt, b: end });
        s.labels.push(sch_core::Label { pos: end, text: net.into() });
    }
}

/// The numbers the reset lesson quotes, from the sample itself.
#[test]
fn the_reset_sample_gives_the_numbers_of_its_lesson() {
    let sch = super::example_sim_tests::example_schematics_for_tests().into_iter().find(|(n, _)| n == "reset_circuit.schjson").unwrap().1;
    // (R1, C1, with the clamp diode) -> (lowest RESET, time below 1 V in us, highest RESET)
    let run = |r: &str, c: &str, clamp: bool| {
        let mut s = sch.clone();
        set_comp(&mut s, "R1", r);
        set_comp(&mut s, "C1", c);
        if clamp {
            add_reset_clamp(&mut s);
        }
        let t = sim_core::transient(&sim_core::from_schematic(&s).unwrap(), &sim_core::TranSettings { tstop: 15e-3, tstep: 15e-6, uic: false }).unwrap();
        let v = &t.signal("V(RESET)").unwrap().values;
        let below: f64 = (1..v.len()).filter(|&i| v[i - 1] < 1.0 || v[i] < 1.0).map(|i| t.time[i] - t.time[i - 1]).sum();
        (v.iter().copied().fold(f64::MAX, f64::min), below * 1e6, v.iter().copied().fold(f64::MIN, f64::max))
    };
    let (lo, us, hi) = run("10k", "100n", false);
    assert!(lo < 0.05 && (180.0..280.0).contains(&us) && (9.5..10.5).contains(&hi), "{lo} {us} {hi}");
    // 220 pF: RESET only falls to about 1.6 V and never goes below 1 V
    let (lo, us, _) = run("10k", "220p", false);
    assert!((1.3..1.9).contains(&lo) && us == 0.0, "{lo} {us}");
    // 100k pull-up: about ten times longer below 1 V
    let (_, us, _) = run("100k", "100n", false);
    assert!((1800.0..2700.0).contains(&us), "{us}");
    // the diode holds the jump at about VCC + 0.6 V
    let (_, _, hi) = run("10k", "100n", true);
    assert!((5.3..6.2).contains(&hi), "{hi}");
}

fn set_comp(s: &mut Schematic, refdes: &str, v: &str) {
    s.components.iter_mut().find(|c| c.refdes == refdes).unwrap().value = v.into();
}

#[test]
fn the_reset_lesson_ticks_its_four_steps() {
    let idx = lesson_ids().iter().position(|l| *l == "reset").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    // step 2: a small C1, run again
    set_value(&mut e, "C1", "220p");
    e.update_lesson();
    assert_eq!(progress(&e), 1, "not run yet");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    // step 3: C1 back, R1 weak
    set_value(&mut e, "C1", "100n");
    set_value(&mut e, "R1", "100k");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    // step 4: the clamp diode is wired and the run is fresh; a diode alone, without a run, does not count
    set_value(&mut e, "R1", "10k");
    add_reset_clamp(&mut e.sch);
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 4, "{}", e.status);
}

#[test]
fn the_decoupling_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "decoupling").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    e.analyze_ac();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    let at = |e: &SchEditor, name: &str, f: f64| {
        let r = e.ac.as_ref().unwrap().outcome.as_ref().unwrap();
        r.signal(name).unwrap().mag()[r.freq.iter().position(|&x| x >= f * 0.999).unwrap()]
    };
    // the numbers of the lesson text: the electrolytic wins at 10 kHz, the ceramic at 1 MHz, the pair at both
    assert!(at(&e, "V(ZE)", 1e4) < 0.3 && at(&e, "V(ZC)", 1e4) > 0.9, "10k {} {}", at(&e, "V(ZE)", 1e4), at(&e, "V(ZC)", 1e4));
    assert!(at(&e, "V(ZC)", 1e6) < 0.03 && at(&e, "V(ZE)", 1e6) > 0.2, "1M");
    assert!(at(&e, "V(ZP)", 1e5) < at(&e, "V(ZE)", 1e5) && at(&e, "V(ZP)", 1e6) <= at(&e, "V(ZC)", 1e6) * 1.05);
    // and the electrolytic rises again toward 100 MHz (its ESL)
    assert!(at(&e, "V(ZE)", 1e7) > 2.0 * at(&e, "V(ZE)", 1e5));
    e.ac.as_mut().unwrap().selected.insert("V(ZP)".into());
    e.update_lesson();
    assert_eq!(progress(&e), 2);
}

#[test]
fn the_pullup_lesson_ticks_its_four_steps_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "pullup").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let vin = |e: &SchEditor| e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltage("IN").unwrap();
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    assert!((4.9..5.01).contains(&vin(&e)), "open: {}", vin(&e));
    set_value(&mut e, "R2", "10");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    assert!(vin(&e) < 0.01, "pressed: {}", vin(&e));
    // the wasted current: 5 V / 10 k
    let i = e.dc.as_ref().unwrap().outcome.as_ref().unwrap().elements.iter().find(|x| x.name == "R1").unwrap().current.abs();
    assert!((0.45e-3..0.55e-3).contains(&i), "{i}");
    // with the pull-up the noise moves IN by well under 0.1 V; without it IN swings across half the supply
    let swing = |e: &SchEditor| {
        let t = e.tran.as_ref().unwrap().outcome.as_ref().unwrap();
        let v = &t.signal("V(IN)").unwrap().values;
        let i0 = t.time.iter().position(|x| *x >= 50e-6).unwrap();
        v[i0..].iter().fold((f64::MAX, f64::MIN), |a, x| (a.0.min(*x), a.1.max(*x)))
    };
    set_value(&mut e, "R2", "100Meg");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 3, "{}", e.status);
    let (lo, hi) = swing(&e);
    assert!(lo > 4.8 && hi < 5.2 && hi - lo < 0.2, "{lo} {hi}");
    set_value(&mut e, "R1", "100Meg");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 4);
    let (lo, hi) = swing(&e);
    assert!(lo < 1.5 && hi > 3.5, "floating: {lo} {hi}");
}

#[test]
fn the_pin_led_lesson_ticks_its_four_steps_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "pin_led").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let led = |e: &SchEditor| e.dc.as_ref().unwrap().outcome.as_ref().unwrap().elements.iter().find(|x| x.name == "D1").unwrap().current.abs() * 1e3;
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    assert!((8.0..9.2).contains(&led(&e)), "330: {} mA", led(&e));
    let vpin = e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltage("PIN").unwrap();
    assert!((4.7..4.9).contains(&vpin), "{vpin}");
    set_value(&mut e, "R1", "100");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    assert!((22.0..25.0).contains(&led(&e)), "100: {} mA", led(&e));
    set_value(&mut e, "R1", "47");
    e.run_dc();
    assert!((37.0..41.0).contains(&led(&e)), "47: {} mA", led(&e));
    set_value(&mut e, "R1", "1k");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    assert!((2.5..3.5).contains(&led(&e)), "1k: {} mA", led(&e));
    // the calculator step: open the LED resistor calculator
    assert_eq!(progress(&e), 3);
    e.wiz_open = true;
    e.wiz_kind = super::wizard::CALCS.iter().position(|c| c.title.starts_with("LED")).unwrap();
    e.update_lesson();
    assert_eq!(progress(&e), 4);
}

#[test]
fn the_level_shift_lesson_ticks_its_three_steps_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "levels").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    // (lowest, highest) of a signal in the second half, and its 10 - 90 % rise time
    let look = |e: &SchEditor, n: &str| {
        let t = e.tran.as_ref().unwrap().outcome.as_ref().unwrap();
        let v = &t.signal(n).unwrap().values;
        let i0 = t.time.iter().position(|x| *x >= 100e-6).unwrap();
        let (lo, hi) = v[i0..].iter().fold((f64::MAX, f64::MIN), |a, x| (a.0.min(*x), a.1.max(*x)));
        let (a, b) = (lo + 0.1 * (hi - lo), lo + 0.9 * (hi - lo));
        let j0 = t.time.iter().position(|x| *x >= 110e-6).unwrap();
        let ia = (j0..v.len()).find(|&i| v[i] >= a).unwrap();
        let ib = (j0..v.len()).find(|&i| v[i] >= b).unwrap();
        (lo, hi, t.time[ib] - t.time[ia])
    };
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    let (_, d_hi, d_rise) = look(&e, "V(D33)");
    let (l_lo, l_hi, l_rise) = look(&e, "V(L)");
    assert!((3.1..3.35).contains(&d_hi) && d_rise < 50e-9, "divider {d_hi} {d_rise}");
    assert!(l_lo < 0.1 && (3.25..3.35).contains(&l_hi) && (300e-9..700e-9).contains(&l_rise), "BSS138 {l_lo} {l_hi} {l_rise}");
    set_value(&mut e, "R3", "10k");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    assert!(look(&e, "V(D33)").1 > 4.3, "wrong ratio {}", look(&e, "V(D33)").1);
    set_value(&mut e, "R3", "2k");
    set_value(&mut e, "R5", "100k");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    let (_, _, slow) = look(&e, "V(L)");
    assert!(slow > 8.0 * l_rise, "{slow} vs {l_rise}");
}

#[test]
fn the_serial_lesson_ticks_its_three_steps_and_the_receiver_misreads_the_slow_cable() {
    let idx = lesson_ids().iter().position(|l| *l == "serial").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    // what a receiver reads: the level in the middle of each bit against half the supply
    let read = |e: &SchEditor, net: &str| {
        let t = e.tran.as_ref().unwrap().outcome.as_ref().unwrap();
        let v = &t.signal(net).unwrap().values;
        let bit = 1.0 / 9600.0;
        let at = |tt: f64| v[t.time.iter().position(|x| *x >= tt).unwrap()];
        let mut byte = 0u32;
        for k in 1..=8 {
            if at(100e-6 + (k as f64 + 0.5) * bit) > 2.5 {
                byte |= 1 << (k - 1);
            }
        }
        byte
    };
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    assert_eq!(read(&e, "V(TX)"), 0x41, "the sent byte is 'A'");
    assert_eq!(read(&e, "V(RX)"), 0x41, "a short cable keeps the byte");
    set_value(&mut e, "C1", "100n");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    assert_ne!(read(&e, "V(RX)"), 0x41, "a slow cable garbles it");
    assert_eq!(read(&e, "V(RX)"), 0x01);
    set_value(&mut e, "R2", "100");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    assert_eq!(read(&e, "V(RX)"), 0x41, "a smaller resistance reads right again");
}

#[test]
fn the_datasheet_lesson_ticks_its_four_steps_and_the_ratings_are_judged_as_it_says() {
    let idx = lesson_ids().iter().position(|l| *l == "datasheet").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let d1 = e.sch.components.iter().position(|c| c.refdes == "D1").unwrap();
    let current = |e: &SchEditor| e.dc.as_ref().unwrap().outcome.as_ref().unwrap().elements.iter().find(|x| x.name == "D1").unwrap().current.abs() * 1e3;
    let issues = |e: &SchEditor| sim_core::check_ratings(&e.sch, e.dc.as_ref().unwrap().outcome.as_ref().unwrap()).into_iter().filter(|i| i.refdes == "D1").map(|i| format!("{:?}", i.level)).collect::<Vec<_>>();
    // a fresh run alone is not enough: D1 has to be selected
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    e.selected = Some(super::Sel::Comp(d1));
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    assert!((38.0..46.0).contains(&current(&e)) && issues(&e).is_empty(), "100: {} mA {:?}", current(&e), issues(&e));
    // 33 ohm: 125 mA, inside 200 mA, no warning, but short of the margin of 2 (250 mA)
    set_value(&mut e, "R1", "33");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    assert!((120.0..130.0).contains(&current(&e)) && issues(&e).is_empty(), "33: {} mA {:?}", current(&e), issues(&e));
    let res = e.dc.as_ref().unwrap().outcome.as_ref().unwrap();
    let st = sim_core::stress(res, "D1");
    let req = sim_core::requirement(&st, 2.0);
    let ratings = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "1N4148").unwrap().ratings.unwrap();
    assert!(!sim_core::meets(&req, &ratings, true), "no margin of 2 at 33 ohm");
    set_value(&mut e, "R1", "22");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    assert!((180.0..190.0).contains(&current(&e)) && issues(&e) == ["Caution"], "22: {} mA {:?}", current(&e), issues(&e));
    set_value(&mut e, "R1", "15");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 4);
    assert!(current(&e) > 200.0 && issues(&e).contains(&"Over".to_string()), "15: {} mA {:?}", current(&e), issues(&e));
}

#[test]
fn the_usb_power_lesson_ticks_its_four_steps_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "usb_power").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    assert!(e.tran_uic, "the lesson starts the transient from zero (a plug-in)");
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let vbus = |e: &SchEditor| e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltage("VBUS").unwrap();
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    assert!((4.9..4.96).contains(&vbus(&e)), "100 mA: {}", vbus(&e));
    set_value(&mut e, "R3", "5");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    assert!((4.30..4.40).contains(&vbus(&e)), "0.9 A: {}", vbus(&e));
    set_value(&mut e, "R2", "1.2");
    set_value(&mut e, "R3", "10");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    assert!((4.2..4.35).contains(&vbus(&e)), "thin cable: {}", vbus(&e));
    // the plug-in: the peak of I(C1) hardly changes with the capacitor, the time it lasts does
    set_value(&mut e, "R2", "0.25");
    set_value(&mut e, "R3", "50");
    let inrush = |e: &mut SchEditor| {
        e.run_tran();
        let t = e.tran.as_ref().unwrap().outcome.as_ref().unwrap();
        let i = &t.signal("I(C1)").unwrap().values;
        let pk = i.iter().fold(0.0f64, |a, x| a.max(x.abs()));
        let tp = t.time[i.iter().position(|x| x.abs() >= pk * 0.999).unwrap()];
        let t10 = t.time.iter().zip(i).find(|(tt, x)| **tt > tp && x.abs() < pk * 0.1).map(|(tt, _)| *tt).unwrap_or(f64::MAX);
        (pk, t10)
    };
    let (pk10, t10) = inrush(&mut e);
    assert!((4.5..6.5).contains(&pk10) && t10 < 40e-6, "10u: {pk10} A, {t10} s");
    assert_eq!(progress(&e), 3, "a 10 uF capacitor is not the big one");
    set_value(&mut e, "C1", "100u");
    let (pk100, t100) = inrush(&mut e);
    e.update_lesson();
    assert_eq!(progress(&e), 4);
    assert!((pk100 / pk10 - 1.0).abs() < 0.3 && t100 > 8.0 * t10, "100u: {pk100} A {t100} s vs {pk10} A {t10} s");
}

#[test]
fn the_usb_speed_lesson_ticks_its_three_steps_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "usb_speed").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let lines = |e: &SchEditor| {
        let d = e.dc.as_ref().unwrap().outcome.as_ref().unwrap();
        (d.voltage("DP").unwrap(), d.voltage("DM").unwrap())
    };
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    let (dp, dm) = lines(&e);
    assert!((2.95..3.05).contains(&dp) && dm < 0.05, "full speed: D+ {dp} D- {dm}");
    set_value(&mut e, "R1", "100Meg");
    set_value(&mut e, "R2", "1.5k");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    let (dp, dm) = lines(&e);
    assert!((2.95..3.05).contains(&dm) && dp < 0.05, "low speed: D+ {dp} D- {dm}");
    set_value(&mut e, "R2", "100Meg");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    let (dp, dm) = lines(&e);
    assert!(dp < 0.1 && dm < 0.1, "nothing plugged: {dp} {dm}");
}

#[test]
fn the_usb_signal_lesson_ticks_its_three_steps_and_a_big_esd_part_slows_the_edge() {
    let idx = lesson_ids().iter().position(|l| *l == "usb_signal").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    // (10 - 90 % rise time in ns, the level in the middle of the first high bit)
    let edge = |e: &SchEditor| {
        let t = e.tran.as_ref().unwrap().outcome.as_ref().unwrap();
        let v = &t.signal("V(DP)").unwrap().values;
        let i0 = t.time.iter().position(|x| *x >= 15e-9).unwrap();
        let (lo, hi) = v[i0..].iter().fold((f64::MAX, f64::MIN), |a, x| (a.0.min(*x), a.1.max(*x)));
        let (a, b) = (lo + 0.1 * (hi - lo), lo + 0.9 * (hi - lo));
        let ia = (i0..v.len()).find(|&i| v[i] >= a).unwrap();
        let ib = (i0..v.len()).find(|&i| v[i] >= b).unwrap();
        let mid = v[t.time.iter().position(|x| *x >= 60e-9).unwrap()];
        ((t.time[ib] - t.time[ia]) * 1e9, mid)
    };
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    let (rise, _) = edge(&e);
    assert!((8.0..11.0).contains(&rise), "5 pF: {rise} ns, inside the 4 - 20 ns range");
    set_value(&mut e, "C2", "200p");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    let (rise, mid) = edge(&e);
    assert!((22.0..30.0).contains(&rise) && mid > 3.1, "200 pF: {rise} ns, mid {mid} V: out of range but still readable");
    set_value(&mut e, "C2", "500p");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    let (rise, mid) = edge(&e);
    assert!((40.0..55.0).contains(&rise) && mid < 2.9, "500 pF: {rise} ns, mid {mid} V: the bit does not settle");
}

#[test]
fn the_pwm_lesson_ticks_its_four_steps_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "pwm").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    // (mean over the last tenth of the run, ripple over the last two PWM periods, time to reach 90 % of the mean)
    let look = |e: &SchEditor| {
        let t = e.tran.as_ref().unwrap().outcome.as_ref().unwrap();
        let v = &t.signal("V(OUT)").unwrap().values;
        let tend = *t.time.last().unwrap();
        let i0 = t.time.iter().position(|x| *x >= tend * 0.9).unwrap();
        // the ripple is read over the last two PWM periods (a slow filter is still rising over a longer window)
        let ir = t.time.iter().position(|x| *x >= tend - 4.08e-3).unwrap();
        let (mn, mx) = v[ir..].iter().fold((f64::MAX, f64::MIN), |a, x| (a.0.min(*x), a.1.max(*x)));
        let mean = v[i0..].iter().sum::<f64>() / (v.len() - i0) as f64;
        let t90 = t.time.iter().zip(v).find(|(_, x)| **x >= 0.9 * mean).map(|(tt, _)| *tt).unwrap_or(f64::MAX);
        (mean, mx - mn, t90)
    };
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    let (mean, ripple, t90) = look(&e);
    assert!((2.4..2.52).contains(&mean) && (0.2..0.32).contains(&ripple) && (0.012..0.026).contains(&t90), "1u: mean {mean} ripple {ripple} t90 {t90}");
    set_value(&mut e, "C1", "100n");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    let (_, ripple, t90) = look(&e);
    assert!(ripple > 2.0 && t90 < 0.002, "100n: ripple {ripple} t90 {t90}");
    set_value(&mut e, "C1", "10u");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    let (_, ripple, _) = look(&e);
    assert!(ripple < 0.1, "10u: ripple {ripple}");
    // the 100 ms of the run end before the 10 uF filter has settled: the last tenth is still rising, so the level is below the final one
    assert!(e.tran.as_ref().unwrap().outcome.as_ref().unwrap().signal("V(OUT)").unwrap().values.last().unwrap() < &2.0);
    set_value(&mut e, "C1", "1u");
    set_value(&mut e, "R2", "10k");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 4);
    let (mean, _, _) = look(&e);
    assert!((1.15..1.3).contains(&mean), "10k load: {mean}");
}

#[test]
fn the_ldo_lesson_ticks_its_four_steps_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "ldo").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let outs = |e: &SchEditor| {
        let d = e.dc.as_ref().unwrap().outcome.as_ref().unwrap();
        (d.voltage("OUT1").unwrap(), d.voltage("OUT2").unwrap())
    };
    let issues = |e: &SchEditor| sim_core::check_ratings(&e.sch, e.dc.as_ref().unwrap().outcome.as_ref().unwrap()).into_iter().filter(|i| i.refdes.starts_with('U')).map(|i| format!("{} {:?}", i.refdes, i.level)).collect::<Vec<_>>();
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    let (a, m) = outs(&e);
    assert!((3.25..3.35).contains(&a) && (3.2..3.35).contains(&m), "5 V: {a} {m}");
    assert!(issues(&e).is_empty(), "inside the ratings at 5 V: {:?}", issues(&e));
    set_value(&mut e, "V1", "4");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    let (a, m) = outs(&e);
    assert!((2.85..2.95).contains(&a) && (3.2..3.35).contains(&m), "4 V: {a} {m}");
    set_value(&mut e, "V1", "3.3");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    let (a, m) = outs(&e);
    assert!((2.1..2.3).contains(&a) && (3.0..3.15).contains(&m), "3.3 V: {a} {m}");
    set_value(&mut e, "V1", "12");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 4);
    let i = issues(&e);
    assert!(i.contains(&"U2 Over".to_string()) && i.contains(&"U1 Caution".to_string()), "12 V: {i:?}");
}

#[test]
fn the_tl431_lesson_ticks_its_four_steps_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "tl431_ref").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let vout = |e: &SchEditor| e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltage("VOUT").unwrap();
    let vref = |e: &SchEditor| e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltage("REF").unwrap();
    // the current that flows through the TL431 itself (its internal current source `Bk`)
    let ik = |e: &SchEditor| e.dc.as_ref().unwrap().outcome.as_ref().unwrap().elements.iter().find(|x| x.name == "U1.Bk").unwrap().current * 1e3;
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    assert!((4.95..5.1).contains(&vout(&e)) && (2.49..2.5).contains(&vref(&e)), "5 V: {} {}", vout(&e), vref(&e));
    assert!((15.0..20.0).contains(&ik(&e)), "light load: the TL431 takes nearly all: {}", ik(&e));
    set_value(&mut e, "R2", "15k");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    assert!((6.2..6.35).contains(&vout(&e)) && (2.49..2.5).contains(&vref(&e)), "15k: {} {}", vout(&e), vref(&e));
    set_value(&mut e, "R2", "10k");
    set_value(&mut e, "R4", "330");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    assert!((4.95..5.1).contains(&vout(&e)), "330 ohm: {}", vout(&e));
    assert!((1.5..3.5).contains(&ik(&e)), "the load takes 15 mA: {}", ik(&e));
    set_value(&mut e, "R4", "100");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 4);
    assert!(vout(&e) < 3.0 && ik(&e) < 0.1, "100 ohm: {} {}", vout(&e), ik(&e));
}

#[test]
fn the_flyback_lesson_ticks_its_three_steps_and_the_diode_holds_the_drain() {
    let idx = lesson_ids().iter().position(|l| *l == "flyback").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let peak = |e: &SchEditor, name: &str| e.tran.as_ref().unwrap().outcome.as_ref().unwrap().signal(name).unwrap().values.iter().copied().fold(f64::MIN, f64::max);
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    // without a diode the drain goes far above the 12 V supply (the model has no breakdown: the size is only a hint)
    assert!(peak(&e, "V(DRAIN)") > 100.0, "no diode: {} V", peak(&e, "V(DRAIN)"));
    // a diode in the wrong direction does not count (anode on VCC, cathode on DRAIN)
    let add = |e: &mut SchEditor, anode: &str, cathode: &str| {
        let d = e.sch.add(sch_core::Kind::Diode, (70, 0), 1);
        for (pin, net) in [("A", anode), ("K", cathode)] {
            let (_, pt) = e.sch.components[d].pins().into_iter().find(|(n, _)| n == pin).unwrap();
            let c = e.sch.components[d].pos;
            let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
            let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
            e.sch.wires.push(sch_core::Wire { a: pt, b: end });
            e.sch.labels.push(sch_core::Label { pos: end, text: net.into() });
        }
        d
    };
    add(&mut e, "VCC", "DRAIN");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "a backwards diode is no flyback diode");
    // a fresh sheet with the diode the right way round: anode on DRAIN, cathode on VCC
    let mut e2 = SchEditor::default();
    e2.start_lesson(idx);
    e2.run_tran();
    e2.update_lesson();
    assert_eq!(progress(&e2), 1);
    add(&mut e2, "DRAIN", "VCC");
    e2.run_tran();
    e2.update_lesson();
    assert!(peak(&e2, "V(DRAIN)") < 14.0 && peak(&e2, "V(DRAIN)") > 12.0, "with the diode: {} V", peak(&e2, "V(DRAIN)"));
    assert_eq!(progress(&e2), 2, "the diode, wired the right way, holds the drain");
    set_value(&mut e2, "L1", "1");
    e2.run_tran();
    e2.update_lesson();
    assert_eq!(progress(&e2), 3);
}

#[test]
fn the_debounce_lesson_ticks_its_three_steps_and_the_microcontroller_counts_as_it_says() {
    let idx = lesson_ids().iter().position(|l| *l == "debounce").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    // how many times IN crosses 2.5 V downward (the presses a microcontroller counts), and when it last does
    let presses = |e: &SchEditor| {
        let t = e.tran.as_ref().unwrap().outcome.as_ref().unwrap();
        let v = &t.signal("V(IN)").unwrap().values;
        let mut n = 0;
        let mut last = f64::NAN;
        for i in 1..v.len() {
            if v[i - 1] >= 2.5 && v[i] < 2.5 {
                n += 1;
                last = t.time[i];
            }
        }
        (n, last, *v.last().unwrap())
    };
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    let (n, _, _) = presses(&e);
    assert_eq!(n, 4, "1 nF: one press is counted four times");
    set_value(&mut e, "C1", "100n");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    let (n, at, _) = presses(&e);
    assert!(n == 1 && (0.0022..0.0030).contains(&at), "100 nF: {n} press at {at} s (about 1.5 ms after the contact closed)");
    set_value(&mut e, "C1", "1u");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    let (n, _, fin) = presses(&e);
    assert!(n == 0 && (3.0..3.5).contains(&fin), "1 uF: {n} presses, IN ends at {fin} V: too slow to notice");
}

#[test]
fn the_temperature_lesson_ticks_its_four_steps_and_the_voltages_are_as_it_says() {
    let idx = lesson_ids().iter().position(|l| *l == "temp_sensors").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let v = |e: &SchEditor, n: &str| e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltage(n).unwrap();
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    assert!((v(&e, "VLM35") - 0.27).abs() < 0.01 && (v(&e, "VTMP36") - 0.77).abs() < 0.01 && (2.3..2.5).contains(&v(&e, "VNTC")), "27: {} {} {}", v(&e, "VLM35"), v(&e, "VTMP36"), v(&e, "VNTC"));
    e.temp_c = "100".into();
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    assert!((v(&e, "VLM35") - 1.0).abs() < 0.01 && (v(&e, "VTMP36") - 1.5).abs() < 0.01 && (0.28..0.38).contains(&v(&e, "VNTC")), "100: {} {} {}", v(&e, "VLM35"), v(&e, "VTMP36"), v(&e, "VNTC"));
    let ntc_hot_10k = v(&e, "VNTC");
    e.temp_c = "-20".into();
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    assert!(v(&e, "VLM35").abs() < 0.01, "the LM35 cannot go below 0 V: {}", v(&e, "VLM35"));
    assert!((v(&e, "VTMP36") - 0.3).abs() < 0.01, "TMP36 at -20: {}", v(&e, "VTMP36"));
    // the sensitivity of the thermistor's divider near 100 degrees, with 10k and with 1k
    let slope = |e: &mut SchEditor| {
        let mut lo = 0.0;
        let mut hi = 0.0;
        for (t, out) in [("99", &mut lo), ("101", &mut hi)] {
            e.temp_c = t.into();
            e.run_dc();
            *out = v(e, "VNTC");
        }
        (hi - lo).abs() / 2.0 * 1e3
    };
    let s10 = slope(&mut e);
    set_value(&mut e, "R3", "1k");
    let s1 = slope(&mut e);
    assert!((6.0..12.0).contains(&s10) && (28.0..40.0).contains(&s1), "sensitivity at 100 degrees: 10k {s10} mV/C, 1k {s1} mV/C");
    e.temp_c = "100".into();
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 4);
    assert!((1.9..2.2).contains(&v(&e, "VNTC")) && v(&e, "VNTC") > 5.0 * ntc_hot_10k, "1k at 100: {}", v(&e, "VNTC"));
}

#[test]
fn the_light_lesson_ticks_its_four_steps_and_the_voltages_are_as_it_says() {
    let idx = lesson_ids().iter().position(|l| *l == "light_sensors").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let v = |e: &SchEditor, n: &str| e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltage(n).unwrap();
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    assert!((1.7..1.95).contains(&v(&e, "VLDR")) && (3.25..3.45).contains(&v(&e, "VPD")), "first light: {} {}", v(&e, "VLDR"), v(&e, "VPD"));
    set_value(&mut e, "V2", "0");
    set_value(&mut e, "V3", "0");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    assert!(v(&e, "VLDR") > 4.9 && v(&e, "VPD") > 4.99, "dark: {} {}", v(&e, "VLDR"), v(&e, "VPD"));
    set_value(&mut e, "V2", "5");
    set_value(&mut e, "V3", "2");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    assert!((0.55..0.72).contains(&v(&e, "VLDR")) && (1.6..1.8).contains(&v(&e, "VPD")), "brighter: {} {}", v(&e, "VLDR"), v(&e, "VPD"));
    // the photodiode: twice the light, twice the current (5 V - VPD over 22k)
    let current = |vpd: f64| (5.0 - vpd) / 22e3;
    assert!((current(1.7) / current(3.35) - 2.0).abs() < 0.1, "twice the light, twice the current");
    set_value(&mut e, "V3", "5");
    e.run_dc();
    assert!(v(&e, "VPD") < 0.5, "saturated at 5: {}", v(&e, "VPD"));
    set_value(&mut e, "R2", "4.7k");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 4);
    assert!((3.0..3.5).contains(&v(&e, "VPD")), "a 4.7k resistor keeps it out of saturation: {}", v(&e, "VPD"));
}

#[test]
fn the_current_lesson_ticks_its_four_steps_and_the_amplifier_runs_out_of_range() {
    let idx = lesson_ids().iter().position(|l| *l == "current_sensors").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let v = |e: &SchEditor, n: &str| e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltage(n).unwrap();
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    assert!((v(&e, "VINA") - 0.249).abs() < 0.01 && (v(&e, "VACS") - 2.592).abs() < 0.01, "0.5 A: {} {}", v(&e, "VINA"), v(&e, "VACS"));
    set_value(&mut e, "V2", "0.05");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    // the Hall sensor moves 5 mV off its 2.5 V, the shunt amplifier 12 mV: the shunt reads a small current better
    let (ina, acs) = (v(&e, "VINA"), v(&e, "VACS") - 2.5);
    assert!((0.010..0.015).contains(&ina) && (0.003..0.007).contains(&acs) && ina > 2.0 * acs, "25 mA: INA {ina} ACS {acs}");
    set_value(&mut e, "V2", "5");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    assert!((v(&e, "VINA") - 1.243).abs() < 0.03 && (v(&e, "VACS") - 2.96).abs() < 0.03, "2.5 A: {} {}", v(&e, "VINA"), v(&e, "VACS"));
    set_value(&mut e, "R1", "100m");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 4);
    // 50 x 0.1 ohm x 2.4 A would be 11.9 V: the amplifier stops near its supply, so the reading is no longer the current
    assert!((4.6..5.0).contains(&v(&e, "VINA")), "clipped: {}", v(&e, "VINA"));
    assert!((v(&e, "VACS") - 2.94).abs() < 0.03, "the Hall sensor does not care about the shunt: {}", v(&e, "VACS"));
}

#[test]
fn the_adc_lesson_ticks_its_four_steps_and_the_reading_is_short_for_a_high_source_resistance() {
    let idx = lesson_ids().iter().position(|l| *l == "adc_input").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    // the sample capacitor voltage when the switch has just opened (13 us)
    let sh = |e: &SchEditor| {
        let t = e.tran.as_ref().unwrap().outcome.as_ref().unwrap();
        let i = t.time.iter().position(|x| *x >= 13e-6).unwrap();
        t.signal("V(SH)").unwrap().values[i]
    };
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    assert!((sh(&e) - 2.5).abs() < 0.01, "10k: {}", sh(&e));
    set_value(&mut e, "R1", "100k");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    assert!((sh(&e) - 2.5).abs() < 0.02, "100k: {}", sh(&e));
    set_value(&mut e, "R1", "1Meg");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    assert!((1.3..1.6).contains(&sh(&e)), "1 Meg is not full: {}", sh(&e));
    set_value(&mut e, "C2", "100n IC=2.5");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 4);
    assert!((sh(&e) - 2.5).abs() < 0.01, "100n at the pin: {}", sh(&e));
}

#[test]
fn the_power_pin_lesson_ticks_its_four_steps_and_the_swing_shrinks_with_distance() {
    let idx = lesson_ids().iter().position(|l| *l == "mega_power").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let swing = |e: &SchEditor| {
        let t = e.tran.as_ref().unwrap().outcome.as_ref().unwrap();
        let v = &t.signal("V(VCC)").unwrap().values;
        v.iter().cloned().fold(f64::MIN, f64::max) - v.iter().cloned().fold(f64::MAX, f64::min)
    };
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    let none = swing(&e);
    set_value(&mut e, "C1", "100n");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    let far = swing(&e);
    set_value(&mut e, "L2", "2n");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    let near = swing(&e);
    set_value(&mut e, "C1", "1n");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 4);
    let small = swing(&e);
    assert!(none > far && far > small && small > near, "{none} {far} {small} {near}");
}

#[test]
fn the_crystal_load_lesson_ticks_its_three_steps_and_the_peak_moves_with_the_capacitor() {
    let idx = lesson_ids().iter().position(|l| *l == "crystal_load").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let peak = |e: &SchEditor| {
        let a = e.ac.as_ref().unwrap().outcome.as_ref().unwrap();
        let m = a.signal("V(OUT)").unwrap().mag();
        let (i, _) = m.iter().copied().enumerate().fold((0, 0.0), |p, (i, x)| if x > p.1 { (i, x) } else { p });
        a.freq[i]
    };
    e.run_ac();
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    let mid = peak(&e);
    set_value(&mut e, "C1", "5p");
    e.run_ac();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    let small = peak(&e);
    set_value(&mut e, "C1", "47p");
    e.run_ac();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    let big = peak(&e);
    assert!(small > mid && mid > big && (small - big) > 5e3, "{small} {mid} {big}");
}

#[test]
fn the_usb_noise_lesson_ticks_its_four_steps_and_only_one_sided_noise_adds_edges() {
    let idx = lesson_ids().iter().position(|l| *l == "ch340g").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let edges = |e: &SchEditor| {
        let rx = &e.tran.as_ref().unwrap().outcome.as_ref().unwrap().signal("V(RX)").unwrap().values;
        (1..rx.len()).filter(|k| rx[k - 1] < 2.5 && rx[*k] >= 2.5).count()
    };
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    assert_eq!(edges(&e), 12);
    set_value(&mut e, "V4", "SIN(0 4 5Meg)");
    set_value(&mut e, "V5", "SIN(0 4 5Meg)");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    assert_eq!(edges(&e), 12, "common noise is cancelled");
    set_value(&mut e, "V4", "SIN(0 2 5Meg)");
    set_value(&mut e, "V5", "SIN(0 0 5Meg)");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    assert_eq!(edges(&e), 12);
    set_value(&mut e, "V4", "SIN(0 4 5Meg)");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 4);
    assert!(edges(&e) > 12, "one-sided noise beyond the margin is misread: {}", edges(&e));
}

#[test]
fn the_equivalent_model_lesson_ticks_its_four_steps_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "equiv_model").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let v = |e: &SchEditor, net: &str| e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltage(net).unwrap();
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    // light load (about 93 mA): both are near 0.8 V, as the text says
    assert!((0.75..0.9).contains(&v(&e, "OUTA")) && (0.7..0.85).contains(&v(&e, "OUTB")), "120 ohm: {} {}", v(&e, "OUTA"), v(&e, "OUTB"));
    for r in ["R1", "R2"] {
        set_value(&mut e, r, "34");
    }
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    // the part is near the data sheet's 1.3 V at about 320 mA; the hand-built one is far below it
    assert!((1.2..1.35).contains(&v(&e, "OUTA")), "part at 34 ohm: {}", v(&e, "OUTA"));
    assert!(v(&e, "OUTB") < 0.9, "hand-built without R6: {}", v(&e, "OUTB"));
    set_value(&mut e, "R6", "1.5");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    assert!((1.2..1.4).contains(&v(&e, "OUTB")), "hand-built with 1.5 ohm: {}", v(&e, "OUTB"));
    // the relay coils (34 ohm, about 3.5 W) are not chip resistors, so the only warning is the true one: a 2N3904 (200 mA) carrying 320 mA
    let over: Vec<_> = sim_core::check_ratings(&e.sch, e.dc.as_ref().unwrap().outcome.as_ref().unwrap()).into_iter().filter(|i| i.level == sim_core::RatingLevel::Over).map(|i| i.refdes).collect();
    assert_eq!(over, vec!["Q2".to_string()], "the lesson text tells about this warning");
    set_value(&mut e, "V2", "3.3");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 4);
    assert!((1.2..1.4).contains(&v(&e, "OUTA")) && (1.2..1.4).contains(&v(&e, "OUTB")), "3.3 V input: {} {}", v(&e, "OUTA"), v(&e, "OUTB"));
}

/// The app starts with the resistor ready to place. A lesson, a sample or an opened file is first for looking at, so a click on
/// the circuit must not add a resistor: they start in the select tool.
#[test]
fn lessons_samples_and_opened_circuits_start_in_the_select_tool() {
    let mut e = SchEditor::default();
    assert!(matches!(e.tool, Tool::Place), "a blank sheet keeps the part ready to place");
    e.start_lesson(0);
    assert!(matches!(e.tool, Tool::Select), "lesson");
    e.tool = Tool::Place;
    assert!(e.open_sample("t", include_str!("../../../../docs/examples/led.schjson")));
    assert!(matches!(e.tool, Tool::Select), "sample");
    e.tool = Tool::Place;
    let s = e.sch.clone();
    e.adopt(s, None);
    assert!(matches!(e.tool, Tool::Select), "opened file");
}

/// The lesson window floats over the right of the canvas: fitting the circuit to the view leaves room for it.
#[test]
fn a_lesson_circuit_is_fitted_left_of_the_lesson_window() {
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::new(1500.0, 900.0));
    let mut e = SchEditor::default();
    e.start_lesson(0);
    e.fit_view(rect);
    let (pan_with, zoom_with) = (e.pan, e.zoom);
    e.lesson = None;
    e.fit_view(rect);
    assert!(pan_with.x > e.pan.x, "the view's centre moves right, so the circuit moves left: {} vs {}", pan_with.x, e.pan.x);
    assert!(zoom_with <= e.zoom, "and the circuit is no bigger than it would be on the whole canvas");
}

#[test]
fn a_condition_says_when_the_result_it_needs_is_missing_or_old() {
    use super::lessons::Unmet;
    let mut e = SchEditor::default();
    e.start_lesson(0);
    // no analysis yet: the freshness check finds nothing
    e.update_lesson();
    assert_eq!(e.lesson_unmet.borrow().clone(), None, "the first step asks for any DC result, not a fresh one");
    assert!(!super::lessons::dc_fresh_for_test(&e));
    assert_eq!(e.lesson_unmet.borrow().clone(), Some(Unmet::Analysis { button: "DC動作点を解析", stale: false }));
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    // the step asks for a changed R2 first: nothing is said about the analysis while R2 is as it was
    assert_eq!(e.lesson_unmet.borrow().clone(), None);
    // R2 changed but the picture is the old one: the condition says so
    set_value(&mut e, "R2", "30k");
    e.update_lesson();
    assert_eq!(e.lesson_unmet.borrow().clone(), Some(Unmet::Analysis { button: "DC動作点を解析", stale: true }));
    assert!(e.lesson_unmet.borrow().clone().unwrap().message().contains("「DC動作点を解析」"));
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    assert_eq!(e.lesson_unmet.borrow().clone(), None, "a ticked step leaves nothing to say");
}

#[test]
fn the_hint_comes_after_a_quiet_time_and_starts_over_with_every_change() {
    let ctx = egui::Context::default();
    let at = |t: f64, e: &mut SchEditor| -> Option<String> {
        let mut out = None;
        let input = egui::RawInput { time: Some(t), ..Default::default() };
        let mut frame = ctx.run_ui(input, |ui| {
            let cur = e.lesson.as_ref().and_then(|a| a.done.iter().position(|d| !d));
            out = e.stuck_hint(ui.ctx(), cur);
        });
        frame.textures_delta.clear();
        out
    };
    let mut e = SchEditor::default();
    e.start_lesson(0);
    e.run_dc();
    e.update_lesson();
    set_value(&mut e, "R2", "30k");
    e.sig_cache = e.signature();
    e.update_lesson();
    assert!(at(100.0, &mut e).is_none(), "the wait starts when the window first looks");
    assert!(at(110.0, &mut e).is_none(), "10 s is not long");
    let hint = at(130.0, &mut e).expect("30 s without a change");
    assert!(hint.contains("DC動作点を解析"), "{hint}");
    // a change of the circuit starts the wait again
    set_value(&mut e, "R2", "20k");
    e.sig_cache = e.signature();
    e.update_lesson();
    assert!(at(131.0, &mut e).is_none());
    assert!(at(160.0, &mut e).is_some());
    // once the result is fresh the step is ticked and there is nothing to say
    e.run_dc();
    e.update_lesson();
    assert!(at(300.0, &mut e).is_none());
}

#[test]
fn a_step_that_wants_a_value_says_which_value_and_what_it_is_now() {
    use super::lessons::Unmet;
    // the follower lesson's last step wants the load R2 at 30 ohm or less (and a fresh result)
    let idx = lesson_ids().iter().position(|l| *l == "emitter_follower").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.run_dc();
    e.update_lesson();
    set_value(&mut e, "R2", "47");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    e.update_lesson();
    let u = e.lesson_unmet.borrow().clone();
    assert_eq!(u, Some(Unmet::Value { refdes: "R2", want: "30Ω 以下".into(), have: Some("47Ω".into()) }));
    let text = u.unwrap().message();
    assert!(text.contains("R2 を 30Ω 以下にします (いまは 47Ω)"), "{text}");
    // the value is right, the result is not fresh: it is the analysis that is missing now
    set_value(&mut e, "R2", "22");
    e.update_lesson();
    assert_eq!(e.lesson_unmet.borrow().clone(), Some(Unmet::Analysis { button: "DC動作点を解析", stale: true }));
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    assert_eq!(e.lesson_unmet.borrow().clone(), None);
}

#[test]
fn value_hints_use_the_unit_of_the_part_and_say_when_a_range_is_wanted() {
    use super::lessons::Unmet;
    let mut e = SchEditor::default();
    e.start_lesson(0); // the divider: R1 10k, R2 10k, V1 5
    assert!(!super::lessons::value_between_for_test(&e, "R1", 1e3, 2e3));
    let m = e.lesson_unmet.borrow().clone().unwrap();
    assert_eq!(m, Unmet::Value { refdes: "R1", want: "1kΩ から 2kΩ の間".into(), have: Some("10kΩ".into()) });
    // a part that is not there
    assert!(!super::lessons::value_between_for_test(&e, "R99", 1.0, 2.0));
    let m = e.lesson_unmet.borrow().clone().unwrap();
    assert!(matches!(m, Unmet::Value { have: None, .. }), "{m:?}");
    assert!(m.message().contains("R99 を 1Ω から 2Ω の間にします"), "{}", m.message());
}

/// What the learner does on the step a question is asked on: (lesson, step, part, new value).
const PREDICTION_ACTIONS: [(&str, usize, &str, &str); 10] = [
    ("divider", 1, "R2", "30k"),
    ("led", 1, "R1", "100"),
    ("zener", 1, "R2", "100"),
    ("emitter_follower", 1, "R2", "47"),
    ("coin_cell", 1, "R1", "10"),
    ("diode_curve", 2, "R1", "1.5k"),
    ("phase_margin", 1, "C1", "1p"),
    ("noise", 1, "R1", "10k"),
    ("rc_lowpass", 1, "C1", "1.59u"),
    ("rlc", 1, "R1", "100"),
];

/// The analysis a question's number comes from (the DC operating point unless the lesson is about another one).
fn run_for_prediction(e: &mut SchEditor, lesson: &str) {
    match lesson {
        "phase_margin" | "rc_lowpass" => e.run_ac(),
        "noise" => e.run_noise(),
        "rlc" => e.run_tran(),
        _ => e.run_dc(),
    }
}

#[test]
fn every_question_has_an_option_that_is_close_to_what_the_simulation_gives() {
    use super::lessons::{prediction_for_step, PREDICTIONS};
    assert_eq!(PREDICTIONS.len(), PREDICTION_ACTIONS.len(), "every question has its action listed here");
    for (lesson, step, refdes, value) in PREDICTION_ACTIONS {
        let p = prediction_for_step(lesson, step).unwrap_or_else(|| panic!("{lesson} step {step} has no question"));
        let idx = lesson_ids().iter().position(|l| *l == lesson).unwrap();
        let mut e = SchEditor::default();
        e.start_lesson(idx);
        run_for_prediction(&mut e, lesson);
        set_value(&mut e, refdes, value);
        run_for_prediction(&mut e, lesson);
        let actual = (p.measure)(&e).unwrap_or_else(|| panic!("{lesson}: nothing to measure"));
        let right = p.nearest(actual);
        let (label, number) = p.options[right];
        // the option that is the right one says a number that the simulation really gives (within 8% or 0.03 of it)
        assert!((number - actual).abs() <= (0.08 * actual.abs()).max(0.03), "{lesson}: the nearest option 「{label}」 says {number}, the simulation gives {actual}");
        // and no other option is about as near (the answer is not a toss-up)
        let others: Vec<f64> = p.options.iter().enumerate().filter(|(i, _)| *i != right).map(|(_, o)| (o.1 - actual).abs()).collect();
        assert!(others.iter().all(|d| *d > 3.0 * (number - actual).abs().max(0.01)), "{lesson}: two options are near {actual}: {:?}", p.options);
    }
}

#[test]
fn a_guess_is_judged_by_the_number_when_the_step_is_done() {
    let divider = lesson_ids().iter().position(|l| *l == "divider").unwrap();
    // a right guess (3.75V: option 1)
    let mut e = SchEditor::default();
    e.start_lesson(divider);
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    e.lesson.as_mut().unwrap().predicted[1] = Some(1);
    set_value(&mut e, "R2", "30k");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    let v = e.lesson.as_ref().unwrap().verdicts[1].clone().expect("a verdict");
    assert!(v.right && (v.actual - 3.75).abs() < 0.01, "{v:?}");
    // a wrong guess (stays 2.5V: option 0)
    let mut e = SchEditor::default();
    e.start_lesson(divider);
    e.run_dc();
    e.update_lesson();
    e.lesson.as_mut().unwrap().predicted[1] = Some(0);
    set_value(&mut e, "R2", "30k");
    e.run_dc();
    e.update_lesson();
    let v = e.lesson.as_ref().unwrap().verdicts[1].clone().expect("a verdict");
    assert!(!v.right && v.chosen == 0, "{v:?}");
    // no guess, no verdict; the step is done all the same
    let mut e = SchEditor::default();
    e.start_lesson(divider);
    e.run_dc();
    e.update_lesson();
    set_value(&mut e, "R2", "30k");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    assert!(e.lesson.as_ref().unwrap().verdicts[1].is_none());
    // "予測せずに進める" is no guess either
    let mut e = SchEditor::default();
    e.start_lesson(divider);
    e.run_dc();
    e.update_lesson();
    e.lesson.as_mut().unwrap().predicted[1] = Some(usize::MAX);
    set_value(&mut e, "R2", "30k");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    assert!(e.lesson.as_ref().unwrap().verdicts[1].is_none());
    // the step of a lesson that has no question is not asked
    assert!(super::lessons::prediction_for_step("divider", 0).is_none());
}

#[test]
fn the_poles_lesson_goes_from_a_peak_to_a_peak_that_is_gone() {
    let idx = lesson_ids().iter().position(|l| *l == "poles").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    e.run_ac();
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    // the poles of the series RLC: one complex pair at 5.03 kHz with Q = 3.16 (the lesson's own numbers)
    e.run_pz();
    e.update_lesson();
    assert_eq!(progress(&e), 2, "{}", e.status);
    let q = e.pz.as_ref().unwrap().max_q().unwrap();
    assert!((q - 3.16).abs() < 0.1, "Q = {q}");
    // 100 ohm: the poles are real, the peak is gone
    set_value(&mut e, "R1", "100");
    e.update_lesson();
    assert_eq!(progress(&e), 2, "the result is old");
    e.run_pz();
    e.update_lesson();
    assert_eq!(progress(&e), 3, "{}", e.status);
    assert!(e.pz.as_ref().unwrap().max_q().is_none());
}

#[test]
fn the_phase_margin_lesson_shows_the_margin_falling_with_the_load_capacitor() {
    let idx = lesson_ids().iter().position(|l| *l == "phase_margin").unwrap();
    let margin = |e: &SchEditor| {
        let r = e.ac.as_ref().unwrap().outcome.as_ref().unwrap();
        let sg = r.signal("V(OUT)").unwrap();
        stability_margins(&r.freq, &sg.db(), &sg.phase_deg()).unwrap().phase_margin.unwrap()
    };
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.run_ac();
    e.update_lesson();
    assert_eq!(progress(&e), 1);
    let loaded = margin(&e);
    assert!(loaded < 45.0, "10n: {loaded}");
    set_value(&mut e, "C1", "1p");
    e.run_ac();
    e.update_lesson();
    assert_eq!(progress(&e), 2, "{}", e.status);
    let light = margin(&e);
    assert!(light > 70.0 && light > loaded + 30.0, "1p: {light}");
    set_value(&mut e, "C1", "100n");
    e.run_ac();
    e.update_lesson();
    assert_eq!(progress(&e), 3, "{}", e.status);
    assert!(margin(&e) < loaded, "100n: {}", margin(&e));
}

#[test]
fn a_value_that_is_not_there_yet_is_named_in_the_hint_of_the_new_lessons() {
    use super::lessons::Unmet;
    let idx = lesson_ids().iter().position(|l| *l == "phase_margin").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.run_ac();
    e.update_lesson();
    // step 2 wants C1 at 100pF or less: the hint says so, with the unit of a capacitor
    e.update_lesson();
    assert_eq!(e.lesson_unmet.borrow().clone(), Some(Unmet::Value { refdes: "C1", want: "100pF 以下".into(), have: Some("10nF".into()) }));
}

#[test]
fn the_noise_lesson_shows_the_main_source_changing_with_the_source_resistance() {
    let idx = lesson_ids().iter().position(|l| *l == "noise").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    e.run_noise();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    let r = e.noise.as_ref().unwrap().outcome.as_ref().unwrap();
    assert_eq!(r.contributions[0].name, "Q1.ib");
    let at_100k = r.in_rms.unwrap();
    assert!((at_100k * 1e6 - 32.0).abs() < 3.0, "{at_100k}");
    set_value(&mut e, "R1", "10k");
    e.update_lesson();
    assert_eq!(progress(&e), 1, "the result is old");
    e.run_noise();
    e.update_lesson();
    assert_eq!(progress(&e), 2, "{}", e.status);
    let at_10k = e.noise.as_ref().unwrap().outcome.as_ref().unwrap().in_rms.unwrap();
    assert!(at_10k < at_100k / 4.0, "{at_10k}");
    set_value(&mut e, "R1", "1k");
    e.run_noise();
    e.update_lesson();
    assert_eq!(progress(&e), 3, "{}", e.status);
    assert_ne!(e.noise.as_ref().unwrap().outcome.as_ref().unwrap().contributions[0].name, "Q1.ib");
}

#[test]
fn a_guess_about_an_ac_or_noise_number_is_judged_too() {
    // phase margin: the margin goes to about 90 degrees (option 2)
    let idx = lesson_ids().iter().position(|l| *l == "phase_margin").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.run_ac();
    e.update_lesson();
    e.lesson.as_mut().unwrap().predicted[1] = Some(2);
    set_value(&mut e, "C1", "1p");
    e.run_ac();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    assert!(e.lesson.as_ref().unwrap().verdicts[1].as_ref().unwrap().right);
    // noise: 1/√10 is the wrong guess (option 1)
    let idx = lesson_ids().iter().position(|l| *l == "noise").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.run_noise();
    e.update_lesson();
    e.lesson.as_mut().unwrap().predicted[1] = Some(1);
    set_value(&mut e, "R1", "10k");
    e.run_noise();
    e.update_lesson();
    let v = e.lesson.as_ref().unwrap().verdicts[1].clone().expect("a verdict");
    assert!(!v.right && v.chosen == 1 && (v.actual - 5.6).abs() < 0.5, "{v:?}");
}

#[test]
fn the_repair_lesson_starts_broken_and_each_fault_can_be_mended() {
    let idx = lesson_ids().iter().position(|l| *l == "debug_circuit").unwrap();
    let wire = |e: &mut SchEditor, a: (i32, i32), b: (i32, i32)| e.sch.wires.push(sch_core::Wire { a, b });
    for seed in 0..5u64 {
        let mut e = SchEditor::default();
        e.start_lesson(idx);
        e.debug_inject(seed);
        e.run_dc();
        e.update_lesson();
        e.update_lesson();
        assert_eq!(progress(&e), 1, "seed {seed}: the first step is only to look");
        // broken: the output is not where the Zener holds it, and the hint says where to look
        assert!(e.lesson_unmet.borrow().is_some(), "seed {seed}");
        assert!(e.debug_cause().is_some());
        match seed {
            0 => set_value(&mut e, "R1", "150"),
            1 => set_value(&mut e, "R2", "1k"),
            2 => set_value(&mut e, "V1", "9"),
            3 => wire(&mut e, (16, -8), (19, -8)),
            _ => wire(&mut e, (28, -2), (28, -5)),
        }
        e.update_lesson();
        assert_eq!(progress(&e), 1, "seed {seed}: the result is old");
        e.run_dc();
        e.update_lesson();
        assert_eq!(progress(&e), 2, "seed {seed}: {}", e.status);
        set_value(&mut e, "V1", "12");
        e.run_dc();
        e.update_lesson();
        assert_eq!(progress(&e), 3, "seed {seed}");
    }
}

#[test]
fn the_repair_lesson_names_the_kind_of_place_in_its_hint() {
    let idx = lesson_ids().iter().position(|l| *l == "debug_circuit").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.debug_inject(3);
    e.run_dc();
    e.update_lesson();
    e.update_lesson();
    let hint = e.lesson_unmet.borrow().as_ref().map(|u| u.message()).unwrap();
    assert!(hint.contains("VOUT") && hint.contains("ERC"), "{hint}");
}

#[test]
fn the_capacitor_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "capacitor").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    let sig = |e: &SchEditor, n: &str| e.tran.as_ref().unwrap().outcome.as_ref().unwrap().signal(n).unwrap().values.clone();
    let (t, cb) = (e.tran.as_ref().unwrap().outcome.as_ref().unwrap().time.clone(), sig(&e, "V(CB)"));
    let at = |t: &[f64], v: &[f64], x: f64| v[t.iter().position(|&s| s >= x).unwrap()];
    // I = C dV/dt: the current of the ramp is about 5 mA in the middle of the rise, and tau = 1 ms gives 63 %
    let i = sig(&e, "I(C1)");
    let mid = at(&t, &i, 0.5e-3);
    assert!((mid - 5e-3).abs() < 0.3e-3, "rising current {mid}");
    assert!((at(&t, &cb, 1e-3) - 3.16).abs() < 0.1, "tau");
    set_value(&mut e, "C1", "4.7u");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    let (t, i) = (e.tran.as_ref().unwrap().outcome.as_ref().unwrap().time.clone(), sig(&e, "I(C1)"));
    assert!((at(&t, &i, 0.5e-3) - 23.5e-3).abs() < 2e-3, "bigger C1");
    set_value(&mut e, "C2", "220n");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    let (t, cb) = (e.tran.as_ref().unwrap().outcome.as_ref().unwrap().time.clone(), sig(&e, "V(CB)"));
    assert!((at(&t, &cb, 0.22e-3) - 3.16).abs() < 0.15, "smaller C2");
}

#[test]
fn the_inductor_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "inductor").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    let get = |e: &SchEditor, n: &str| {
        let r = e.tran.as_ref().unwrap().outcome.as_ref().unwrap();
        (r.time.clone(), r.signal(n).unwrap().values.clone())
    };
    let at = |(t, v): &(Vec<f64>, Vec<f64>), x: f64| v[t.iter().position(|&s| s >= x).unwrap()];
    // V = L di/dt: 50 mA per ms with 1 ohm; tau = L/R = 1 ms gives 63 % of 50 mA
    assert!((at(&get(&e, "I(L1)"), 1e-3) - 0.049).abs() < 3e-3, "ramp");
    assert!((at(&get(&e, "I(L2)"), 1e-3) - 31.6e-3).abs() < 2e-3, "tau");
    set_value(&mut e, "L1", "22m");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    assert!((at(&get(&e, "I(L1)"), 1e-3) - 0.22).abs() < 0.02, "faster");
    set_value(&mut e, "R2", "1k");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    assert!((at(&get(&e, "I(L2)"), 0.1e-3) - 3.16e-3).abs() < 0.3e-3, "short tau");
}

#[test]
fn the_resistor_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "resistor").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let v = |e: &SchEditor, n: &str| e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltage(n).unwrap();
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    assert!((v(&e, "A") - 3.4375).abs() < 0.01 && (v(&e, "P") - 5.0).abs() < 0.01);
    set_value(&mut e, "R2", "1k");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    assert!((v(&e, "A") - 4.545).abs() < 0.01);
    set_value(&mut e, "R4", "22");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
}

#[test]
fn the_transistor_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "transistor").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let v = |e: &SchEditor, n: &str| e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltage(n).unwrap();
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    assert!((v(&e, "COL") - 3.2).abs() < 0.15 && (0.7..0.85).contains(&v(&e, "BASE")), "active");
    e.run_sweep();
    e.update_lesson();
    assert_eq!(progress(&e), 2, "{}", e.status);
    set_value(&mut e, "R2", "10k");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    assert!(v(&e, "COL") < 0.2, "saturated");
}

#[test]
fn the_mosfet_basics_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "mosfet_basics").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let v = |e: &SchEditor, n: &str| e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltage(n).unwrap();
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    assert!((v(&e, "DRAIN") - 0.14).abs() < 0.02, "on");
    e.run_sweep();
    e.update_lesson();
    assert_eq!(progress(&e), 2, "{}", e.status);
    set_value(&mut e, "V2", "2.5");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    assert!((v(&e, "DRAIN") - 4.0).abs() < 0.2, "half on");
}

#[test]
fn the_opamp_basics_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "opamp_basics").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    e.analyze_ac();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    {
        let r = e.ac.as_ref().unwrap().outcome.as_ref().unwrap();
        for n in ["V(OUT1)", "V(OUT2)", "V(OUT3)"] {
            let m = r.signal(n).unwrap().mag();
            let f3 = r.freq[m.iter().position(|x| *x < m[0] * std::f64::consts::FRAC_1_SQRT_2).unwrap()];
            let (gain, want) = match n { "V(OUT1)" => (2.0, 5e5), "V(OUT2)" => (11.0, 1e5), _ => (101.0, 1e4) };
            assert!((m[0] / gain - 1.0).abs() < 0.02 && (f3 / want - 1.0).abs() < 0.15, "{n}: {} {f3}", m[0]);
        }
    }
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 2, "{}", e.status);
    let peak = |e: &SchEditor, n: &str| {
        let r = e.tran.as_ref().unwrap().outcome.as_ref().unwrap().signal(n).unwrap().values.clone();
        (r.iter().cloned().fold(f64::MIN, f64::max), r.iter().cloned().fold(f64::MAX, f64::min))
    };
    assert!(peak(&e, "V(OUT1)").0 < 1.01 && peak(&e, "V(OUT2)").0 < 3.6 && peak(&e, "V(OUT3)").1 < -4.9, "clipping");
    set_value(&mut e, "R3", "2k");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    assert!(peak(&e, "V(OUT2)").0 > 1.4 && peak(&e, "V(OUT2)").0 < 1.51, "clean");
}

#[test]
fn the_diode_basics_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "diode_basics").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let v = |e: &SchEditor, n: &str| e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltage(n).unwrap();
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    assert!((v(&e, "A") - 0.693).abs() < 0.01 && v(&e, "B") > 4.99, "dc");
    e.analyze_ac();
    e.update_lesson();
    assert_eq!(progress(&e), 2, "{}", e.status);
    let mag = |e: &SchEditor, n: &str| e.ac.as_ref().unwrap().outcome.as_ref().unwrap().signal(n).unwrap().mag()[0];
    assert!((mag(&e, "V(A)") - 0.006).abs() < 0.0005 && mag(&e, "V(B)") > 0.99, "ac1");
    set_value(&mut e, "R1", "10k");
    e.analyze_ac();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    assert!((mag(&e, "V(A)") - 0.0059).abs() < 0.0005, "ratio stays");
    e.run_dc();
    assert!((v(&e, "A") - 0.633).abs() < 0.01, "0.63 V");
}

#[test]
fn the_led_basics_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "led_basics").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let amps = |e: &SchEditor, a: &str, src: f64, r: f64| (src - e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltage(a).unwrap()) / r;
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    assert!((amps(&e, "A1", 5.0, 330.0) - 9.2e-3).abs() < 0.5e-3 && (amps(&e, "A2", 3.3, 330.0) - 4.3e-3).abs() < 0.5e-3);
    assert!((amps(&e, "A3", 2.0, 330.0) - 0.7e-3).abs() < 0.3e-3);
    set_value(&mut e, "R1", "150");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    assert!((amps(&e, "A1", 5.0, 150.0) - 19.7e-3).abs() < 1e-3);
    set_value(&mut e, "V3", "3");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    assert!((amps(&e, "A3", 3.0, 330.0) - 3.4e-3).abs() < 0.4e-3);
}

#[test]
fn the_zener_basics_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "zener_basics").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    e.run_sweep();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    let r = e.sweep.as_ref().unwrap().outcome.as_ref().unwrap();
    let z = &r.signal("V(Z)").unwrap().values;
    let at = |x: f64| z[r.x.iter().position(|v| *v >= x - 1e-9).unwrap()];
    assert!((at(4.0) - 4.0).abs() < 0.01 && (4.9..5.1).contains(&at(5.0)) && (5.15..5.25).contains(&at(12.0)), "knee");
    let v = |e: &SchEditor| e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltage("Z").unwrap();
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    assert!((v(&e) - 5.21).abs() < 0.05);
    set_value(&mut e, "R1", "22");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    assert!((v(&e) - 6.7).abs() < 0.15);
}

#[test]
fn the_relay_basics_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "relay_basics").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    // (final current, time from the 0.5 ms start until the pull-in current of 53.6 mA)
    let probe = |e: &SchEditor, n: &str| {
        let r = e.tran.as_ref().unwrap().outcome.as_ref().unwrap();
        let v = &r.signal(n).unwrap().values;
        (*v.last().unwrap(), r.time.iter().zip(v).find(|(_, i)| **i >= 0.0536).map(|(t, _)| t - 0.5e-3))
    };
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    let (f1, t1) = probe(&e, "I(L1)");
    assert!((f1 - 0.0714).abs() < 0.002 && (1.8e-3..2.2e-3).contains(&t1.unwrap()), "5 V coil");
    let (f2, t2) = probe(&e, "I(L2)");
    assert!((f2 - 0.0427).abs() < 0.002 && t2.is_none(), "3 V coil never pulls in");
    set_value(&mut e, "R2", "15");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    let (f2, t2) = probe(&e, "I(L2)");
    assert!((f2 - 0.0588).abs() < 0.002 && (2.5e-3..3.2e-3).contains(&t2.unwrap()), "82 %");
    set_value(&mut e, "R1", "35");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    let (f1, t1) = probe(&e, "I(L1)");
    assert!((f1 - 0.140).abs() < 0.004 && t1.unwrap() < 1.6e-3, "low resistance coil");
}

#[test]
fn the_crystal_basics_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "crystal_basics").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    // (peak value, peak frequency, width at 0.7 of the peak) of a signal
    let probe = |e: &SchEditor, n: &str| {
        let r = e.ac.as_ref().unwrap().outcome.as_ref().unwrap();
        let m = r.signal(n).unwrap().mag();
        let (k, pk) = m.iter().enumerate().fold((0, 0.0), |a, (i, v)| if *v > a.1 { (i, *v) } else { a });
        let lo = (0..k).rev().find(|i| m[*i] < pk * std::f64::consts::FRAC_1_SQRT_2).map_or(r.freq[0], |i| r.freq[i]);
        let hi = (k..m.len()).find(|i| m[*i] < pk * std::f64::consts::FRAC_1_SQRT_2).map_or(*r.freq.last().unwrap(), |i| r.freq[i]);
        (pk, r.freq[k], hi - lo)
    };
    e.analyze_ac();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    let (x, l) = (probe(&e, "V(OUTX)"), probe(&e, "V(OUTL)"));
    assert!((x.0 - 0.53).abs() < 0.03 && x.2 < 3e3, "crystal peak {x:?}");
    assert!((l.0 - 0.5).abs() < 0.02 && l.2 >= 190e3, "LC is flat over the window {l:?}");
    e.set_ac_settings("10Meg", "25Meg", "2000");
    e.analyze_ac();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    let l = probe(&e, "V(OUTL)");
    assert!((l.0 - 0.5).abs() < 0.02 && (250e3..450e3).contains(&l.2), "LC hump {l:?}");
    set_value(&mut e, "R2", "0.1");
    e.analyze_ac();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    let l = probe(&e, "V(OUTL)");
    assert!(l.0 > 0.85 && l.2 < 260e3, "narrower and higher {l:?}");
}

#[test]
fn the_transformer_basics_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "transformer_basics").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    // rms over the last two cycles (the first cycle is the start-up)
    let rms = |e: &SchEditor, n: &str| {
        let r = e.tran.as_ref().unwrap().outcome.as_ref().unwrap();
        let v = &r.signal(n).unwrap().values;
        let t1 = *r.time.last().unwrap();
        let (mut sum, mut dur) = (0.0, 0.0);
        for k in 1..r.time.len() {
            if r.time[k] >= t1 - 0.04 {
                let dt = r.time[k] - r.time[k - 1];
                sum += v[k] * v[k] * dt;
                dur += dt;
            }
        }
        (sum / dur).sqrt()
    };
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    assert!((rms(&e, "V(OUT)") - 14.5).abs() < 0.3 && rms(&e, "I(R2)") < 0.02, "no load");
    set_value(&mut e, "R1", "28.8");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    let (vo, is, ip) = (rms(&e, "V(OUT)"), rms(&e, "I(R1)"), rms(&e, "I(R2)"));
    assert!((vo - 12.0).abs() < 0.3 && (is - 0.416).abs() < 0.02 && (ip / is - 0.145).abs() < 0.01, "rated {vo} {is} {ip}");
    set_value(&mut e, "R1", "5");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    let (vo, is) = (rms(&e, "V(OUT)"), rms(&e, "I(R1)"));
    assert!((vo - 6.6).abs() < 0.3 && (is - 1.31).abs() < 0.06 && vo * is > 7.5, "overload {vo} {is}");
}

#[test]
fn the_thermistor_basics_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "thermistor_basics").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    e.run_temp_sweep();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    let at = |e: &SchEditor, n: &str, t: f64| {
        let r = e.sweep.as_ref().unwrap().outcome.as_ref().unwrap();
        let k = r.x.iter().position(|x| *x >= t - 1e-9).unwrap();
        r.signal(n).unwrap().values[k]
    };
    assert!((at(&e, "V(A)", -20.0) - 4.57).abs() < 0.03 && (at(&e, "V(A)", 25.0) - 2.5).abs() < 0.01 && (at(&e, "V(A)", 100.0) - 0.33).abs() < 0.03);
    assert!((at(&e, "V(B)", 25.0) - 4.55).abs() < 0.02 && (at(&e, "V(C)", 25.0) - 0.45).abs() < 0.02);
    // the slope at 25 C: 55 mV/K for the 10k divider, 18 mV/K for the others
    let slope = |n: &str| (at(&e, n, 30.0) - at(&e, n, 20.0)).abs() / 10.0;
    assert!((slope("V(A)") - 0.055).abs() < 0.004 && (slope("V(B)") - 0.018).abs() < 0.003 && (slope("V(C)") - 0.018).abs() < 0.003);
    for n in ["V(A)", "V(B)", "V(C)"] {
        e.sweep.as_mut().unwrap().selected.insert(n.into());
    }
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    e.temp_c = "-20".into();
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3, "{}", e.status);
    let v = |e: &SchEditor, n: &str| e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltage(n).unwrap();
    assert!((v(&e, "A") - 4.57).abs() < 0.03, "105 kohm at -20 C");
}

#[test]
fn the_phototransistor_basics_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "phototransistor_basics").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let v = |e: &SchEditor, n: &str| e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltage(n).unwrap();
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    assert!((v(&e, "PDOUT") - 0.15).abs() < 0.01 && (v(&e, "COL") - 4.70).abs() < 0.03, "photodiode alone vs transistor");
    e.run_sweep();
    e.update_lesson();
    assert_eq!(progress(&e), 2, "{}", e.status);
    {
        let r = e.sweep.as_ref().unwrap().outcome.as_ref().unwrap();
        let c = &r.signal("V(COL)").unwrap().values;
        let at = |x: f64| c[r.x.iter().position(|v| *v >= x - 1e-9).unwrap()];
        assert!((at(0.1) - 3.49).abs() < 0.05 && (at(0.3) - 0.63).abs() < 0.1 && (at(0.5) - 0.54).abs() < 0.05, "linear, then flat");
    }
    set_value(&mut e, "V2", "0.5");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    assert!((v(&e, "PDOUT") - 3.75).abs() < 0.05 && (v(&e, "COL") - 0.54).abs() < 0.05, "saturated");
}

#[test]
fn the_regulator_basics_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "regulator_basics").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let ripple = |e: &SchEditor, f: f64| {
        let r = e.ac.as_ref().unwrap().outcome.as_ref().unwrap();
        r.signal("V(VOUT)").unwrap().mag()[r.freq.iter().position(|x| *x >= f * 0.999).unwrap()]
    };
    let v = |e: &SchEditor, n: &str| e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltage(n).unwrap();
    e.analyze_ac();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    assert!(ripple(&e, 100.0) < 1e-4 && ripple(&e, 1e5) < 1e-4, "rejects the ripple");
    set_value(&mut e, "V1", "6");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2, "{}", e.status);
    assert!((v(&e, "VOUT") - 4.0).abs() < 0.05, "input - 2 V");
    e.analyze_ac();
    e.update_lesson();
    assert_eq!(progress(&e), 3, "{}", e.status);
    assert!(ripple(&e, 100.0) > 0.99 && ripple(&e, 1e4) > 0.99, "passes the ripple");
}

#[test]
fn the_photocoupler_basics_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "photocoupler_basics").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let v = |e: &SchEditor, n: &str| e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltage(n).unwrap();
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    // CTR 100 %: the output current is the input current
    let (i_in, i_out) = ((5.0 - v(&e, "LEDA")) / 330.0, (5.0 - v(&e, "OUT")) / 220.0);
    assert!((v(&e, "LEDA") - 1.23).abs() < 0.05 && (i_out / i_in - 1.0).abs() < 0.05, "ctr {i_in} {i_out}");
    e.run_sweep();
    e.update_lesson();
    assert_eq!(progress(&e), 2, "{}", e.status);
    {
        let r = e.sweep.as_ref().unwrap().outcome.as_ref().unwrap();
        let o = &r.signal("V(OUT)").unwrap().values;
        let at = |x: f64| o[r.x.iter().position(|v| *v >= x - 1e-9).unwrap()];
        assert!(at(0.5) > 4.99 && (at(3.0) - 3.73).abs() < 0.05 && (at(5.0) - 2.43).abs() < 0.05, "linear fall");
    }
    set_value(&mut e, "R2", "1k");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    assert!((v(&e, "OUT") - 0.13).abs() < 0.05, "saturated");
}

#[test]
fn the_fuse_basics_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "fuse_basics").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    // (current at the end of the run, time of the last sample with more than 10 percent of the current of the first millisecond after the supply rises)
    let probe = |e: &SchEditor, n: &str| {
        let r = e.tran.as_ref().unwrap().outcome.as_ref().unwrap();
        let v = &r.signal(n).unwrap().values;
        let peak = v.iter().cloned().fold(0.0, f64::max);
        let last_on = r.time.iter().zip(v).filter(|(_, i)| **i > 0.1 * peak).map(|(t, _)| *t).last().unwrap();
        (peak, *v.last().unwrap(), last_on - 10e-3)
    };
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    // 1 A holds; 3 A blows after about 60 ms; the 24 A short after about 1.4 ms; blown means blown for good
    let (p1, end1, _) = probe(&e, "I(R1)");
    assert!((p1 - 0.99).abs() < 0.02 && (end1 - 0.99).abs() < 0.02, "1 A holds");
    let (p2, end2, off2) = probe(&e, "I(R2)");
    assert!((p2 - 2.89).abs() < 0.1 && end2 < 1e-3 && (0.04..0.09).contains(&off2), "3 A: {off2}");
    let (p3, end3, off3) = probe(&e, "I(R3)");
    assert!(p3 > 15.0 && end3 < 1e-3 && (0.0005..0.003).contains(&off3), "short: {off3}");
    set_value(&mut e, "R2", "7");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 2);
    let (_, end, off) = probe(&e, "I(R2)");
    assert!(end < 1e-3 && (0.25..0.4).contains(&off), "1.7 A blows slowly: {off}");
    set_value(&mut e, "R1", "6");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    let (_, end, off) = probe(&e, "I(R1)");
    assert!(end < 1e-3 && (0.12..0.25).contains(&off), "2 A load blows the fuse: {off}");
}

#[test]
fn the_logic_basics_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "logic_basics").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    // how many times a signal crosses the middle of the supply
    let flips = |e: &SchEditor, n: &str| {
        let r = e.tran.as_ref().unwrap().outcome.as_ref().unwrap();
        let v = &r.signal(n).unwrap().values;
        v.windows(2).filter(|w| (w[0] - 2.5) * (w[1] - 2.5) < 0.0).count()
    };
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    assert!(flips(&e, "V(OUT1)") > 100 && flips(&e, "V(OUT2)") == 2, "chatter {} vs clean {}", flips(&e, "V(OUT1)"), flips(&e, "V(OUT2)"));
    let v = |e: &SchEditor, n: &str| e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltage(n).unwrap();
    e.run_dc();
    e.update_lesson();
    assert!((v(&e, "DRV") - 4.4).abs() < 0.1, "drop of about 0.6 V");
    assert_eq!(progress(&e), 2, "{}", e.status);
    set_value(&mut e, "R1", "100");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3);
    assert!((v(&e, "DRV") - 3.67).abs() < 0.1, "heavier load");
}

#[test]
fn the_speaker_basics_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "speaker_basics").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let z = |e: &SchEditor, src: &str, f: f64| {
        let r = e.ac.as_ref().unwrap().outcome.as_ref().unwrap();
        let i = r.signal(src).unwrap().mag()[r.freq.iter().position(|x| *x >= f * 0.999).unwrap()];
        1.0 / i
    };
    e.analyze_ac();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    // the impedance of the speaker: 8 ohm at low frequencies, about 32 ohm at the resonance, rising again with the coil
    assert!((z(&e, "I(V1)", 20.0) - 8.0).abs() < 0.3 && (z(&e, "I(V1)", 600.0) - 32.0).abs() < 2.0, "speaker");
    assert!((z(&e, "I(V1)", 3000.0) - 8.1).abs() < 0.5 && (z(&e, "I(V1)", 30000.0) - 20.4).abs() < 1.5, "speaker, high");
    // the piezo buzzer: a capacitor at 100 Hz (88 kohm), about 0.9 kohm at the resonance
    assert!((z(&e, "I(V2)", 100.0) - 88e3).abs() < 3e3 && z(&e, "I(V2)", 3900.0) < 1.1e3, "piezo");
    e.ac.as_mut().unwrap().selected.insert("I(V2)".into());
    e.update_lesson();
    set_value(&mut e, "C1", "100u");
    e.analyze_ac();
    e.update_lesson();
    assert_eq!(progress(&e), 3, "{}", e.status);
    assert!((z(&e, "I(V1)", 350.0) - 28.6).abs() < 3.0 && z(&e, "I(V1)", 600.0) < 10.0, "the resonance moved down to 350 Hz");
}

#[test]
fn the_varistor_basics_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "varistor_basics").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    e.run_sweep();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    {
        let r = e.sweep.as_ref().unwrap().outcome.as_ref().unwrap();
        let m = &r.signal("V(M)").unwrap().values;
        let at = |x: f64| m[r.x.iter().position(|v| *v >= x - 1e-9).unwrap()];
        assert!((at(12.0) - 12.0).abs() < 0.01 && (at(30.0) - 29.7).abs() < 0.3 && (at(60.0) - 42.2).abs() < 0.8, "clamps");
    }
    e.run_tran();
    {
        let r = e.tran.as_ref().unwrap().outcome.as_ref().unwrap();
        let peak = |n: &str| r.signal(n).unwrap().values.iter().cloned().fold(f64::MIN, f64::max);
        assert!(peak("V(A)") > 98.0 && (peak("V(B)") - 47.3).abs() < 2.0, "the surge: {} {}", peak("V(A)"), peak("V(B)"));
    }
    e.update_lesson();
    assert_eq!(progress(&e), 2, "{}", e.status);
    set_value(&mut e, "V1", "20");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3, "{}", e.status);
    let m = e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltage("M").unwrap();
    assert!(((20.0 - m) / 10.0 - 3.3e-4).abs() < 1e-4, "leakage at 20 V: {}", (20.0 - m) / 10.0);
}

#[test]
fn the_hall_basics_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "hall_basics").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let v = |e: &SchEditor, n: &str| e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltage(n).unwrap();
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    assert!((v(&e, "OUT") - 3.2).abs() < 0.01, "2.5 V + 500 G x 1.4 mV/G");
    e.run_sweep();
    e.update_lesson();
    assert_eq!(progress(&e), 2, "{}", e.status);
    {
        let r = e.sweep.as_ref().unwrap().outcome.as_ref().unwrap();
        let o = &r.signal("V(OUT)").unwrap().values;
        let at = |x: f64| o[r.x.iter().position(|v| *v >= x - 1e-9).unwrap()];
        assert!((at(-10.0) - 1.1).abs() < 0.01 && (at(0.0) - 2.5).abs() < 0.01 && (at(10.0) - 3.9).abs() < 0.01, "linear around Vs/2");
    }
    set_value(&mut e, "V1", "3.3");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3, "{}", e.status);
    assert!((v(&e, "OUT") - 2.112).abs() < 0.01 && (v(&e, "OUT") / 3.3 - 0.64).abs() < 0.005, "ratiometric");
}

#[test]
fn the_switch_basics_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "switch_basics").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let v = |e: &SchEditor, n: &str| e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltage(n).unwrap();
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    assert!((v(&e, "LOAD") - 4.995).abs() < 0.002 && v(&e, "SIG") < 1e-3, "a new contact drops 5 mV");
    set_value(&mut e, "R1", "5");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2, "{}", e.status);
    assert!((v(&e, "LOAD") - 4.52).abs() < 0.01, "a worn contact");
    set_value(&mut e, "R4", "5k");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3, "{}", e.status);
    assert!((v(&e, "SIG") - 1.1).abs() < 0.01, "a dirty contact does not give a Low");
}

#[test]
fn the_battery_basics_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "battery_basics").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let v = |e: &SchEditor, n: &str| e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltage(n).unwrap();
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    assert!((v(&e, "VAA") - 1.526).abs() < 0.01 && (v(&e, "VLI") - 4.19).abs() < 0.02 && (v(&e, "VCOIN") - 2.96).abs() < 0.02, "light loads");
    set_value(&mut e, "R3", "10");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2, "{}", e.status);
    assert!((v(&e, "VCOIN") - 1.2).abs() < 0.01 && v(&e, "VAA") > 1.5, "the coin cell sags, the AA cell does not");
    set_value(&mut e, "R3", "15");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3, "{}", e.status);
    assert!((v(&e, "VCOIN") - 1.5).abs() < 0.01, "half of the open voltage: the maximum power");
}

#[test]
fn the_segment_basics_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "segment_basics").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let v = |e: &SchEditor, n: &str| e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltage(n).unwrap();
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    // the current of one lit segment: a "1" shares 30 mA between 2, an "8" 31 mA between 7, a resistor each gives 9.2 mA
    let (one, eight, own) = ((5.0 - v(&e, "C1")) / 100.0 / 2.0, (5.0 - v(&e, "C8")) / 100.0 / 7.0, (5.0 - v(&e, "S1")) / 330.0);
    assert!((one - 15e-3).abs() < 0.5e-3 && (eight - 4.45e-3).abs() < 0.3e-3 && (own - 9.2e-3).abs() < 0.3e-3, "{one} {eight} {own}");
    set_value(&mut e, "R2", "33");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2, "{}", e.status);
    assert!(((5.0 - v(&e, "C8")) / 33.0 / 7.0 - 13.1e-3).abs() < 0.5e-3, "the 8 gets near to the 1");
    set_value(&mut e, "R3", "1k");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3, "{}", e.status);
    assert!(((5.0 - v(&e, "S1")) / 1e3 - 3.1e-3).abs() < 0.3e-3 && ((5.0 - v(&e, "S2")) / 330.0 - 9.2e-3).abs() < 0.3e-3, "only that segment dims");
}

#[test]
fn the_analog_switch_basics_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "analog_switch_basics").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let v = |e: &SchEditor, n: &str| e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltage(n).unwrap();
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    assert!((v(&e, "OUT") - 2.9946).abs() < 0.001 && v(&e, "HOLD").abs() < 0.01, "5 mV lost in 1.8 ohm; nothing sampled yet");
    set_value(&mut e, "R1", "100");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 2, "{}", e.status);
    assert!((v(&e, "OUT") - 2.947).abs() < 0.002, "1.8 % lost into 100 ohm");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 3, "{}", e.status);
    {
        let r = e.tran.as_ref().unwrap().outcome.as_ref().unwrap();
        let h = &r.signal("V(HOLD)").unwrap().values;
        let at = |t: f64| h[r.time.iter().position(|x| *x >= t).unwrap()];
        // tau = (1 kohm + 1.8 ohm) x 10 nF = 10 us, the sampling starts at 10 us
        assert!(at(5e-6) < 0.01 && (at(30e-6) - 3.0 * (1.0 - (-2.0f64).exp())).abs() < 0.05 && at(110e-6) > 2.995 && at(390e-6) > 2.99, "sample and hold");
    }
}

#[test]
fn the_bridge_basics_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "bridge_basics").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    // (maximum, minimum) of a signal over the last two cycles
    let band = |e: &SchEditor, n: &str| {
        let r = e.tran.as_ref().unwrap().outcome.as_ref().unwrap();
        let v = &r.signal(n).unwrap().values;
        let t1 = *r.time.last().unwrap();
        let w: Vec<f64> = r.time.iter().zip(v).filter(|(t, _)| **t >= t1 - 0.04).map(|(_, x)| *x).collect();
        (w.iter().cloned().fold(f64::MIN, f64::max), w.iter().cloned().fold(f64::MAX, f64::min))
    };
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    let (a, b) = (band(&e, "V(OUTA)"), band(&e, "V(OUTB)"));
    assert!((a.0 - 15.35).abs() < 0.2 && (b.0 - 16.18).abs() < 0.2 && b.0 > a.0, "the bridge peak is lower by one diode drop: {a:?} {b:?}");
    assert!((b.0 - b.1) / (a.0 - a.1) > 1.8 && (b.0 - b.1) / (a.0 - a.1) < 2.4, "half wave has twice the ripple: {a:?} {b:?}");
    e.tran.as_mut().unwrap().selected.insert("I(D1)".into());
    e.update_lesson();
    assert_eq!(progress(&e), 2, "{}", e.status);
    assert!((band(&e, "I(D1)").0 - 1.23).abs() < 0.15, "the peak current of a bridge diode");
    set_value(&mut e, "R1", "33");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 3, "{}", e.status);
    assert!((band(&e, "V(OUTA)").1 - 9.8).abs() < 0.4 && (band(&e, "I(D1)").0 - 2.0).abs() < 0.2, "a heavier load");
}

#[test]
fn the_eeprom_basics_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "eeprom_basics").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    // the 30 % - 70 % rise time of the first rising edge of SDA after it was pulled low (ns), the low level
    let rise = |e: &SchEditor| {
        let r = e.tran.as_ref().unwrap().outcome.as_ref().unwrap();
        let v = &r.signal("V(SDA)").unwrap().values;
        let start = r.time.iter().position(|t| *t >= 5.0e-6).unwrap();
        let t_at = |lvl: f64| {
            let k = (start..v.len()).find(|k| v[*k] >= lvl).unwrap();
            let (t0, t1, v0, v1) = (r.time[k - 1], r.time[k], v[k - 1], v[k]);
            t0 + (t1 - t0) * (lvl - v0) / (v1 - v0)
        };
        let low = v[r.time.iter().position(|t| *t >= 4.9e-6).unwrap()];
        ((t_at(0.7 * 3.3) - t_at(0.3 * 3.3)) * 1e9, low)
    };
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    let (tr, low) = rise(&e);
    assert!((tr - 414.0).abs() < 30.0 && low < 0.05 && tr > 300.0 && tr < 1000.0, "inside 100 kHz, outside 400 kHz: {tr}");
    set_value(&mut e, "C1", "400p");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 2, "{}", e.status);
    assert!((rise(&e).0 - 1609.0).abs() < 80.0, "a long bus is too slow even for 100 kHz");
    set_value(&mut e, "R1", "1k");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 3, "{}", e.status);
    let (tr, low) = rise(&e);
    assert!((tr - 342.0).abs() < 25.0 && low < 0.05, "a strong pull-up: {tr}");
}

#[test]
fn the_electrolytic_life_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "electrolytic_life").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    let rep = |e: &SchEditor| {
        let p = &e.tran.as_ref().unwrap().parts;
        assert_eq!(p.len(), 1, "{p:?}");
        (p[0].i_rms, p[0].allowed, p[0].core_temp_c, p[0].life_hours)
    };
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    let (i, allowed, core, life) = rep(&e);
    assert!((i - 0.154).abs() < 0.01 && allowed == Some(0.3) && (core - 29.6).abs() < 0.3 && life == Some(131400.0), "15 years at 27 C: {i} {core} {life:?}");
    e.temp_c = "85".into();
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 2, "{}", e.status);
    let (_, _, core, life) = rep(&e);
    assert!((core - 87.6).abs() < 0.3 && (life.unwrap() - 20009.0).abs() < 300.0, "20000 hours at 85 C: {core} {life:?}");
    set_value(&mut e, "R1", "1");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 3, "{}", e.status);
    let (i, _, core, life) = rep(&e);
    assert!((i - 0.272).abs() < 0.01 && (core - 93.2).abs() < 0.3 && (life.unwrap() - 13583.0).abs() < 300.0, "more ripple: {i} {core} {life:?}");
}

#[test]
fn the_ldo_vs_buck_lesson_ticks_and_its_numbers_hold() {
    let idx = lesson_ids().iter().position(|l| *l == "ldo_vs_buck").unwrap();
    let mut e = SchEditor::default();
    e.start_lesson(idx);
    e.update_lesson();
    assert_eq!(progress(&e), 0);
    // (efficiency of the linear regulator, of the buck converter, mean output voltage of both) over the last 0.6 ms
    let eff = |e: &SchEditor| {
        let r = e.tran.as_ref().unwrap().outcome.as_ref().unwrap();
        let t1 = *r.time.last().unwrap();
        let w = (t1 - 0.6e-3, t1);
        let mean = |n: &str| signal_stats(&r.time, &r.signal(n).unwrap().values, w.0, w.1).unwrap().mean;
        let (v1, v2) = (e.sch.components.iter().find(|c| c.refdes == "V1").unwrap().value.parse::<f64>().unwrap(), 12.0);
        let (pin1, pin2) = (v1 * -mean("I(V1)"), v2 * -mean("I(V2)"));
        let (po1, po2) = (mean("V(VOUT1)").powi(2) / 40.0, mean("V(VOUT2)").powi(2) / 40.0);
        (po1 / pin1, po2 / pin2, mean("V(VOUT1)"), mean("V(VOUT2)"))
    };
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 1, "{}", e.status);
    let (l, b, v1, v2) = eff(&e);
    assert!((l - 0.416).abs() < 0.02 && (b - 0.85).abs() < 0.03 && (v1 - 4.99).abs() < 0.05 && (v2 - 4.85).abs() < 0.1, "efficiency {l} {b}");
    set_value(&mut e, "V1", "6.5");
    e.run_tran();
    e.update_lesson();
    assert_eq!(progress(&e), 2, "{}", e.status);
    assert!((eff(&e).0 - 0.768).abs() < 0.03, "a small difference suits the linear regulator");
    set_value(&mut e, "V1", "12");
    set_value(&mut e, "R1", "20");
    e.run_dc();
    e.update_lesson();
    assert_eq!(progress(&e), 3, "{}", e.status);
    let dc = e.dc.as_ref().unwrap().outcome.as_ref().unwrap();
    assert!(sim_core::check_ratings(&e.sch, dc).iter().any(|i| i.refdes == "U1" && i.level == sim_core::RatingLevel::Over), "the linear regulator is over its package rating");
}
