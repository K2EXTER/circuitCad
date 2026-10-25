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
