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
    for i in 0..titles.len() {
        let mut e = SchEditor::default();
        e.start_lesson(i);
        assert!(e.lesson.is_some(), "{}", titles[i]);
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
    let mcu = e.library.iter().find(|d| d.sim.is_none()).unwrap();
    assert_eq!(super::panel::lib_hover_for_test(mcu), mcu.description);
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
