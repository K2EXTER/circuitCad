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
