use crate::*;

fn vsrc(c: &mut Circuit, name: &str, p: &str, v: f64) {
    let p = c.node(p);
    c.add(Element::vdc(name, p, 0, v));
}

fn res(c: &mut Circuit, name: &str, a: &str, b: &str, r: f64) {
    let (a, b) = (c.node(a), c.node(b));
    c.add(Element::Resistor { name: name.into(), a, b, r });
}

fn cmos_inverter(vdd: f64) -> Circuit {
    let mut c = Circuit::new();
    vsrc(&mut c, "Vdd", "vdd", vdd);
    vsrc(&mut c, "Vin", "in", 0.0);
    let (o, i, d) = (c.node("out"), c.node("in"), c.node("vdd"));
    c.add(Element::Mosfet { name: "Mn".into(), d: o, g: i, s: 0, model: MosModel { kp: 1e-3, vto: 1.0, lambda: 0.01, ..MosModel::default() } });
    c.add(Element::Mosfet { name: "Mp".into(), d: o, g: i, s: d, model: MosModel { kp: 1e-3, vto: -1.0, lambda: 0.01, pmos: true, ..MosModel::default() } });
    c
}

fn sweep(source: &str, a: f64, b: f64, step: f64) -> DcSweepSettings {
    DcSweepSettings { source: source.into(), start: a, stop: b, step }
}

#[test]
fn divider_follows_the_swept_source() {
    let mut c = Circuit::new();
    vsrc(&mut c, "V1", "in", 0.0);
    res(&mut c, "R1", "in", "out", 1e3);
    res(&mut c, "R2", "out", "0", 3e3);
    let r = dc_sweep(&c, &sweep("V1", 0.0, 10.0, 1.0)).unwrap();
    assert_eq!(r.x.len(), 11);
    let out = r.signal("V(out)").unwrap();
    for (x, v) in r.x.iter().zip(&out.values) {
        assert!((v - 0.75 * x).abs() < 1e-6, "{x}: {v}");
    }
    let i = r.signal("I(R1)").unwrap();
    assert!((i.values[10] - 10.0 / 4e3).abs() < 1e-9);
}

#[test]
fn sweep_points_include_the_end_and_may_descend() {
    let mut c = Circuit::new();
    vsrc(&mut c, "V1", "in", 0.0);
    res(&mut c, "R1", "in", "0", 1e3);
    let up = dc_sweep(&c, &sweep("V1", 0.0, 1.0, 0.3)).unwrap();
    assert_eq!(up.x.len(), 5, "0, 0.3, 0.6, 0.9 and the end 1.0");
    assert_eq!(*up.x.last().unwrap(), 1.0);
    let down = dc_sweep(&c, &sweep("V1", 5.0, 0.0, -1.0)).unwrap();
    assert_eq!(down.x, vec![5.0, 4.0, 3.0, 2.0, 1.0, 0.0]);
}

#[test]
fn bad_settings_are_errors() {
    let mut c = Circuit::new();
    vsrc(&mut c, "V1", "in", 0.0);
    res(&mut c, "R1", "in", "0", 1e3);
    assert!(dc_sweep(&c, &sweep("V9", 0.0, 1.0, 0.1)).is_err(), "unknown source");
    assert!(dc_sweep(&c, &sweep("R1", 0.0, 1.0, 0.1)).is_err(), "not a source");
    assert!(dc_sweep(&c, &sweep("V1", 0.0, 1.0, 0.0)).is_err(), "zero step");
    assert!(dc_sweep(&c, &sweep("V1", 0.0, 1e9, 1e-3)).is_err(), "too many points");
}

#[test]
fn cmos_inverter_curve_and_noise_margins() {
    let c = cmos_inverter(5.0);
    let r = dc_sweep(&c, &sweep("Vin", 0.0, 5.0, 0.05)).unwrap();
    let out = &r.signal("V(out)").unwrap().values;
    let m = inverter_metrics(&r.x, out).expect("falling curve");
    assert!((m.voh - 5.0).abs() < 1e-3 && m.vol.abs() < 1e-3, "{m:?}");
    // symmetric devices switch at half the supply
    assert!((m.vth - 2.5).abs() < 0.1, "{m:?}");
    assert!(m.vil < m.vth && m.vth < m.vih, "{m:?}");
    assert!(m.max_gain < -5.0, "high gain in the transition: {m:?}");
    // both noise margins are healthy and, by symmetry, close to each other
    assert!(m.nml() > 1.5 && m.nmh() > 1.5 && (m.nml() - m.nmh()).abs() < 0.3, "{m:?}");
    // supply current peaks near the threshold (shoot-through) and is tiny at the rails
    let i = &r.signal("I(Vdd)").unwrap().values;
    let peak = i.iter().fold(0.0f64, |a, b| a.max(b.abs()));
    assert!(i[0].abs() < peak * 1e-3 && i[i.len() - 1].abs() < peak * 1e-3);
}

#[test]
fn weaker_pmos_moves_the_threshold_down() {
    let mut c = cmos_inverter(5.0);
    for e in c.elements.iter_mut() {
        if let Element::Mosfet { name, model, .. } = e
            && name == "Mp" {
                model.kp = 2.5e-4;
            }
    }
    let r = dc_sweep(&c, &sweep("Vin", 0.0, 5.0, 0.05)).unwrap();
    let m = inverter_metrics(&r.x, &r.signal("V(out)").unwrap().values).unwrap();
    assert!(m.vth < 2.3, "a weak pull-up lets the input win earlier: {m:?}");
}

#[test]
fn non_inverting_curves_have_no_metrics() {
    let x: Vec<f64> = (0..=10).map(|i| i as f64).collect();
    let up: Vec<f64> = x.iter().map(|v| v * 0.5).collect();
    assert!(inverter_metrics(&x, &up).is_none());
    assert!(inverter_metrics(&x[..3], &up[..3]).is_none());
}

#[test]
fn npn_inverter_has_ordered_thresholds() {
    let mut c = Circuit::new();
    vsrc(&mut c, "Vcc", "vcc", 5.0);
    vsrc(&mut c, "Vin", "in", 0.0);
    res(&mut c, "Rb", "in", "b", 10e3);
    res(&mut c, "Rc", "vcc", "out", 1e3);
    let (o, b) = (c.node("out"), c.node("b"));
    c.add(Element::Bjt { name: "Q1".into(), c: o, b, e: 0, model: BjtModel { bf: 200.0, ..BjtModel::default() } });
    let r = dc_sweep(&c, &sweep("Vin", 0.0, 5.0, 0.05)).unwrap();
    let out = &r.signal("V(out)").unwrap().values;
    assert!((out[0] - 5.0).abs() < 1e-3 && out[out.len() - 1] < 0.3);
    let m = inverter_metrics(&r.x, out).unwrap();
    assert!(m.vil < m.vth && m.vth < m.vih && m.vth > 0.6 && m.vth < 2.5, "{m:?}");
}

#[test]
fn cmos_inverter_example_sweeps() {
    let sch: sch_core::Schematic = serde_json::from_str(include_str!("../../../docs/examples/cmos_inverter.schjson")).unwrap();
    let c = from_schematic(&sch).unwrap();
    let r = dc_sweep(&c, &sweep("V2", 0.0, 3.3, 0.03)).unwrap();
    let m = inverter_metrics(&r.x, &r.signal("V(OUT)").unwrap().values).expect("an inverting curve");
    assert!((m.voh - 3.3).abs() < 1e-3 && m.vol < 1e-3, "{m:?}");
    // AO3400A / AO3401A switch near the middle of a 3.3 V supply with healthy margins
    assert!(m.vth > 1.2 && m.vth < 2.1, "{m:?}");
    assert!(m.nml() > 1.0 && m.nmh() > 1.0, "{m:?}");
}

#[test]
fn npn_not_example_sweeps_and_switches() {
    let sch: sch_core::Schematic = serde_json::from_str(include_str!("../../../docs/examples/npn_not.schjson")).unwrap();
    let c = from_schematic(&sch).unwrap();
    let r = dc_sweep(&c, &sweep("V2", 0.0, 5.0, 0.05)).unwrap();
    let out = &r.signal("V(OUT)").unwrap().values;
    let m = inverter_metrics(&r.x, out).expect("an inverting curve");
    assert!((m.voh - 5.0).abs() < 1e-3 && m.vol < 0.3, "{m:?}");
    assert!(m.vil < m.vth && m.vth < m.vih && m.vth > 0.6 && m.vth < 2.5, "{m:?}");
    // the same circuit driven by its pulse: high in -> low out, then back
    let t = transient(&c, &TranSettings { tstop: 1e-3, tstep: 5e-6, uic: false }).unwrap();
    let o = t.signals.iter().find(|s| s.name == "V(OUT)").unwrap();
    let at = |tt: f64| o.values[t.time.iter().position(|x| *x >= tt).unwrap()];
    assert!(at(50e-6) > 4.9, "input low: output high");
    assert!(at(300e-6) < 0.3, "input high: output low");
    assert!(at(800e-6) > 4.9, "input low again");
}
