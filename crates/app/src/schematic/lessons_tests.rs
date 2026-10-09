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
        if titles[i].contains("555") {
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
