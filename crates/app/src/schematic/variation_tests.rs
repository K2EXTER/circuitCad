use super::*;

fn led_example() -> SchEditor {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/led.schjson");
    let mut e = SchEditor::default();
    e.load_path(path);
    e
}

#[test]
fn temperature_sweep_fills_the_sweep_viewer() {
    let mut e = led_example();
    e.run_temp_sweep();
    let v = e.sweep.as_ref().expect("sweep view");
    assert!(v.temp);
    let r = v.outcome.as_ref().expect("converges");
    assert_eq!((r.x.len(), r.x[0], *r.x.last().unwrap()), (21, -20.0, 80.0));
    assert!(matches!(e.wave_mode, WaveMode::Sweep));
}

#[test]
fn the_panel_temperature_changes_the_dc_result() {
    let mut e = led_example();
    e.run_dc();
    let cold = e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltages.clone();
    e.temp_c = "85".into();
    e.run_dc();
    let hot = e.dc.as_ref().unwrap().outcome.as_ref().unwrap().voltages.clone();
    assert_ne!(cold, hot, "a warm LED has a lower forward voltage");
    e.temp_c = "warm".into();
    e.run_dc();
    assert!(e.status.contains("失敗"), "{}", e.status);
}

#[test]
fn monte_carlo_runs_on_a_chosen_node() {
    let mut e = led_example();
    let node = e.node_names().into_iter().next().expect("the example has a named node");
    e.mc_out = node;
    e.mc_runs = "40".into();
    e.run_mc();
    let r = e.mc.as_ref().unwrap().outcome.as_ref().expect("runs");
    assert_eq!(r.values.len() + r.failed, 40);
    assert!(matches!(e.wave_mode, WaveMode::Mc));

    e.mc_tol_r = "abc".into();
    e.run_mc();
    assert!(e.status.contains("許容差"), "{}", e.status);
    e.mc_tol_r = "5".into();
    e.mc_out = String::new();
    e.run_mc();
    assert!(e.status.contains("失敗"));
}

#[test]
fn the_divider_example_shows_a_spread_around_half_the_supply() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/divider_tolerance.schjson");
    let mut e = SchEditor::default();
    e.load_path(path);
    assert!(e.node_names().contains(&"MID".to_string()), "{:?}", e.node_names());
    e.mc_out = "MID".into();
    e.run_mc();
    let r = e.mc.as_ref().unwrap().outcome.as_ref().unwrap();
    assert!((r.nominal - 2.5).abs() < 1e-3);
    let st = r.stats().unwrap();
    assert!(st.min < 2.5 && st.max > 2.5 && st.max - st.min < 0.3, "{st:?}");
}

#[test]
fn monte_carlo_can_measure_the_cutoff_of_the_rc_filter() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/rc_lowpass.schjson");
    let mut e = SchEditor::default();
    e.load_path(path);
    e.mc_kind = McKind::Ac;
    e.mc_runs = "30".into();
    let mut found = None;
    for n in e.node_names() {
        e.mc_out = n.clone();
        e.run_mc();
        if let Some(Ok(r)) = e.mc.as_ref().map(|m| &m.outcome) {
            found = Some((n, r.unit, r.nominal));
        }
    }
    let (_, unit, nominal) = found.expect("the filter output has a -3 dB point");
    assert_eq!(unit, "Hz");
    assert!((nominal / 1000.0 - 1.0).abs() < 0.02, "{nominal}");
}

#[test]
fn a_bad_value_shows_up_among_the_erc_findings_on_the_part() {
    let mut e = led_example();
    assert!(e.conversion_problems().is_empty());
    set_value(&mut e, "R1", "oops");
    let found = e.conversion_problems();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].severity, Severity::Error);
    assert!(found[0].at.is_some() && found[0].hint.contains("10k"));
}

fn set_value(e: &mut SchEditor, refdes: &str, v: &str) {
    e.sch.components.iter_mut().find(|c| c.refdes == refdes).unwrap().value = v.into();
}

#[test]
fn the_li_ion_charger_example_charges_through_all_phases() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/li_ion_charger.schjson");
    let mut e = SchEditor::default();
    e.load_path(path);
    e.set_tran_settings("12m", "20u");
    e.run_tran();
    let res = e.tran.as_ref().unwrap().outcome.as_ref().expect(&e.status);
    let vb = &res.signal("V(BAT)").unwrap().values;
    assert!(vb[0] > 2.6 && vb[0] < 2.9, "starts near 2.6 V (plus the drop across the cell resistance): {}", vb[0]);
    assert!((vb.last().unwrap() - 4.2).abs() < 0.05, "{}", vb.last().unwrap());
    let ch = &res.signal("V(CHRG)").unwrap().values;
    let sb = &res.signal("V(STDBY)").unwrap().values;
    assert!(ch[1] < 1.0 && sb[1] > 3.0, "charging at the start: the LED on /CHRG is lit, the other is dark");
    assert!(sb.last().unwrap() < &1.0 && ch.last().unwrap() > &3.0, "done at the end: {} {}", sb.last().unwrap(), ch.last().unwrap());
}
