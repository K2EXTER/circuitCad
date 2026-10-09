use crate::*;

fn vsrc(c: &mut Circuit, name: &str, p: &str, v: f64) {
    let p = c.node(p);
    c.add(Element::vdc(name, p, 0, v));
}

fn res(c: &mut Circuit, name: &str, a: &str, b: &str, r: f64) {
    let (a, b) = (c.node(a), c.node(b));
    c.add(Element::Resistor { name: name.into(), a, b, r });
}

/// 5 V -- R(1k) -- out -- switch -- GND, switch controlled by `ctl` volts.
fn switched_divider(ctl: f64, model: SwitchModel) -> Circuit {
    let mut c = Circuit::new();
    vsrc(&mut c, "V1", "in", 5.0);
    vsrc(&mut c, "Vc", "ctl", ctl);
    res(&mut c, "R1", "in", "out", 1e3);
    let (o, k) = (c.node("out"), c.node("ctl"));
    c.add(Element::Switch { name: "S1".into(), a: o, b: 0, cp: k, cn: 0, model });
    c
}

#[test]
fn model_eval_goes_from_off_to_on_through_the_geometric_mean() {
    let m = SwitchModel { ron: 10.0, roff: 1e6, vt: 2.0, vw: 0.1 };
    let (g_lo, _) = m.eval(0.0);
    let (g_hi, _) = m.eval(5.0);
    let (g_mid, dg) = m.eval(2.0);
    assert!((1.0 / g_lo - 1e6).abs() < 1.0 && (1.0 / g_hi - 10.0).abs() < 1e-9);
    // half way in the log domain
    assert!((g_mid - (1e-6f64 * 0.1).sqrt()).abs() < 1e-9, "{g_mid}");
    assert!(dg > 0.0);
    // derivative against a finite difference
    for vc in [1.91, 1.95, 2.0, 2.04, 2.09] {
        let (g, d) = m.eval(vc);
        let h = 1e-7;
        let fd = (m.eval(vc + h).0 - m.eval(vc - h).0) / (2.0 * h);
        assert!((d - fd).abs() <= 1e-4 * fd.abs().max(g * 1e-3), "vc={vc}: {d} vs {fd}");
    }
}

#[test]
fn switch_acts_as_a_resistor_when_on_and_an_open_circuit_when_off() {
    let m = SwitchModel { ron: 100.0, roff: 1e9, vt: 1.0, vw: 0.05 };
    let on = dc_op(&switched_divider(3.0, m.clone())).unwrap();
    let off = dc_op(&switched_divider(0.0, m)).unwrap();
    assert!((on.voltage("out").unwrap() - 5.0 * 100.0 / 1100.0).abs() < 1e-6);
    assert!((off.voltage("out").unwrap() - 5.0).abs() < 1e-3);
    // the report carries the control voltage and the current
    let s = on.element("S1").unwrap();
    assert!((s.current - on.voltage("out").unwrap() / 100.0).abs() < 1e-9);
    assert!(s.extra.iter().any(|(n, v)| n == "Vc" && (*v - 3.0).abs() < 1e-9));
}

#[test]
fn sweeping_the_control_voltage_is_smooth_and_converges() {
    let c = switched_divider(0.0, SwitchModel { ron: 10.0, roff: 1e9, vt: 1.0, vw: 0.02 });
    let r = dc_sweep(&c, &DcSweepSettings { source: "Vc".into(), start: 0.0, stop: 2.0, step: 0.002 }).unwrap();
    let out = &r.signal("V(out)").unwrap().values;
    assert!(out.windows(2).all(|w| w[1] <= w[0] + 1e-9), "monotonic falling");
    assert!(out[0] > 4.99 && out[out.len() - 1] < 0.06);
    // the whole change happens inside the transition band
    let k0 = r.x.iter().position(|v| *v >= 0.97).unwrap();
    let k1 = r.x.iter().position(|v| *v >= 1.03).unwrap();
    assert!(out[k0] > 4.9 && out[k1] < 0.2, "{} {}", out[k0], out[k1]);
}

#[test]
fn ac_small_signal_uses_the_on_resistance() {
    let mut c = Circuit::new();
    let (i, o, k) = (c.node("in"), c.node("out"), c.node("ctl"));
    let s = parse_source_full("AC 1").unwrap();
    c.add(Element::VSource { name: "V1".into(), p: i, n: 0, wave: s.wave, ac: s.ac });
    vsrc(&mut c, "Vc", "ctl", 3.0);
    res(&mut c, "R1", "in", "out", 900.0);
    c.add(Element::Switch { name: "S1".into(), a: o, b: 0, cp: k, cn: 0, model: SwitchModel { ron: 100.0, roff: 1e9, vt: 1.0, vw: 0.05 } });
    let r = ac_analysis(&c, &AcSettings { fstart: 10.0, fstop: 1e3, sweep: AcSweep::Decade(2) }).unwrap();
    let out = r.signal("V(out)").unwrap();
    assert!((out.re[0] - 0.1).abs() < 1e-6, "divider 100/1000: {}", out.re[0]);
}

#[test]
fn switch_cards_and_models_are_read() {
    let text = ".subckt SWT a b c d\nS1 a b c d MYSW\n.model MYSW SW(RON=5 ROFF=1Meg VT=0.5 VW=0.02)\n.ends SWT";
    let mut c = Circuit::new();
    let nodes: Vec<usize> = ["x", "0", "y", "0"].iter().map(|n| c.node(n)).collect();
    add_subckt_instance(&mut c, "U1", "SWT", text, &nodes).unwrap();
    match &c.elements[0] {
        Element::Switch { name, model, .. } => {
            assert_eq!(name, "U1.S1");
            assert_eq!((model.ron, model.roff, model.vt, model.vw), (5.0, 1e6, 0.5, 0.02));
        }
        other => panic!("{other:?}"),
    }
    // a bare switch part: terminals then the control pair
    let mut c = Circuit::new();
    let n: Vec<usize> = ["p", "0", "q", "0"].iter().map(|n| c.node(n)).collect();
    add_model_instance(&mut c, "S9", "S", "MYSW", ".model MYSW SW(RON=2)", &n).unwrap();
    assert!(matches!(c.elements[0], Element::Switch { .. }));
    assert!(add_subckt_instance(&mut Circuit::new(), "U2", "SWT", ".subckt SWT a b c d\nS1 a b c d NOPE\n.ends SWT", &[1, 0, 2, 0]).is_err());
}

#[test]
fn a_conducting_switch_contributes_thermal_noise() {
    let c = switched_divider(3.0, SwitchModel { ron: 100.0, roff: 1e9, vt: 1.0, vw: 0.05 });
    let st = NoiseSettings {
        sweep: AcSettings { fstart: 10.0, fstop: 1e3, sweep: AcSweep::Decade(5) },
        output: "out".into(),
        output_ref: None,
        input: None,
        temp_c: 26.85,
    };
    let r = noise_analysis(&c, &st).unwrap();
    let s = r.contribution("S1").expect("switch noise source");
    assert!(s.rms > 0.0);
    assert_eq!(describe_source("U1.S1").title, "スイッチの抵抗の熱雑音");
}
