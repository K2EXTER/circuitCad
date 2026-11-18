//! `TranRun`: a transient run kept open and solved on in pieces (the live logic view of the schematic), with sources changed
//! between the pieces. Solved in pieces, it must give what the run solved at once gives.

use crate::*;

/// A two-bit ripple counter of D flip-flops (each fed back from its inverted output, 14 ns from clock to Q) on a 1 kHz clock,
/// its last bit driving a 1 kohm load: the digital nets, a delayed output on an analog node and the clock corners together.
fn counter() -> Circuit {
    let mut c = Circuit::new();
    let names = ["vcc", "vss", "clk", "q2"];
    let nodes: Vec<usize> = names.iter().map(|n| c.node(n)).collect();
    let body = "Y1 dff q1 qb1 vss vcc 60 0.5 0 0 0 td=14n qb1 clk 0 0\nY2 dff q2 qb2 vss vcc 60 0.5 0 0 0 td=14n qb2 qb1 0 0";
    let text = format!(".subckt CELLS {}\n{body}\n.ends CELLS", names.join(" "));
    add_subckt_instance(&mut c, "U1", "CELLS", &text, &nodes).unwrap();
    c.add(Element::vdc("Vcc", nodes[0], 0, 5.0));
    c.add(Element::vdc("Vss", nodes[1], 0, 0.0));
    let wave = parse_source("PULSE(0 5 0.1m 1u 1u 0.5m 1m)").unwrap();
    c.add(Element::VSource { name: "Vclk".into(), p: nodes[2], n: 0, wave, ac: None });
    c.add(Element::Resistor { name: "Rl".into(), a: nodes[3], b: 0, r: 1e3 });
    c
}

/// The value of `name` at the first point at or after `t`.
fn at(r: &TranResult, name: &str, t: f64) -> f64 {
    let k = r.time.iter().position(|x| *x >= t).unwrap();
    r.signal(name).unwrap().values[k]
}

/// Appends the points of a piece to the points before it.
fn join(all: &mut Option<TranResult>, part: TranResult) {
    match all {
        None => *all = Some(part),
        Some(a) => {
            a.time.extend(part.time);
            for (s, p) in a.signals.iter_mut().zip(part.signals) {
                s.values.extend(p.values);
            }
        }
    }
}

/// The counter solved in 37 uneven pieces (some ending in the middle of a clock edge or a flip-flop's delay) reads exactly what
/// the run solved at once reads, at the middle of every clock phase.
#[test]
fn a_digital_run_solved_in_pieces_matches_the_run_solved_at_once() {
    let c = counter();
    let s = TranSettings { tstop: 10e-3, tstep: 10e-6, uic: false };
    let whole = transient(&c, &s).unwrap();
    let mut run = TranRun::start(&c, &s, None).unwrap();
    let mut pieces = None;
    let mut t = 0.0;
    for k in 0..37 {
        t += s.tstop / 37.0 * (0.4 + 1.2 * ((k * 7 % 11) as f64 / 10.0));
        run.advance_to(t.min(s.tstop)).unwrap();
        join(&mut pieces, run.drain());
    }
    run.advance_to(s.tstop).unwrap();
    assert_eq!(run.time(), s.tstop);
    join(&mut pieces, run.drain());
    let pieces = pieces.unwrap();
    for k in 0..20 {
        let tm = 0.35e-3 + k as f64 * 0.5e-3;
        for name in ["V(q2)", "V(U1.q1)", "V(clk)"] {
            assert_eq!(at(&whole, name, tm), at(&pieces, name, tm), "{name} at {tm}");
        }
    }
    // the counter did count: q2 is a quarter of the clock
    assert!(at(&whole, "V(q2)", 0.35e-3) < 1.0 && at(&whole, "V(q2)", 1.35e-3) > 4.0 && at(&whole, "V(q2)", 2.35e-3) > 4.0 && at(&whole, "V(q2)", 3.35e-3) < 1.0);
}

/// An RC low-pass on a pulse, solved in pieces: the pieces end on steps the run at once does not take, so the answers agree to
/// the accuracy of the step control, not to the last bit.
#[test]
fn an_rc_run_solved_in_pieces_agrees_with_the_run_solved_at_once() {
    let mut c = Circuit::new();
    let (a, b) = (c.node("a"), c.node("b"));
    let wave = parse_source("PULSE(0 1 0.1m 1u 1u 1m 2m)").unwrap();
    c.add(Element::VSource { name: "V1".into(), p: a, n: 0, wave, ac: None });
    c.add(Element::Resistor { name: "R1".into(), a, b, r: 1e3 });
    c.add(Element::Capacitor { name: "C1".into(), a: b, b: 0, c: 100e-9 });
    let s = TranSettings { tstop: 4e-3, tstep: 20e-6, uic: false };
    let whole = transient(&c, &s).unwrap();
    let mut run = TranRun::start(&c, &s, None).unwrap();
    for k in 1..=13 {
        run.advance_to(s.tstop * k as f64 / 13.0).unwrap();
    }
    let pieces = run.finish();
    for k in 0..40 {
        let tm = 0.05e-3 + k as f64 * 0.1e-3;
        let (w, p) = (at(&whole, "V(b)", tm), at(&pieces, "V(b)", tm));
        // the points found are not at the same times: compare with the slope over the gap between them
        let (tw, tp) = (whole.time[whole.time.iter().position(|x| *x >= tm).unwrap()], pieces.time[pieces.time.iter().position(|x| *x >= tm).unwrap()]);
        let slope = 1.0 / 100e-6;
        assert!((w - p).abs() < 2e-3 + slope * (tw - tp).abs(), "V(b) at {tm}: {w} at once, {p} in pieces");
    }
}

/// A source changed between two pieces acts from then on, and the delayed output it starts is an event to step to.
#[test]
fn a_source_changed_between_pieces_acts_from_then_on() {
    let mut c = Circuit::new();
    let names = ["vcc", "vss", "a", "y"];
    let nodes: Vec<usize> = names.iter().map(|n| c.node(n)).collect();
    let text = ".subckt CELLS vcc vss a y\nY1 buf y 0 vss vcc 60 0.5 0 0 0 td=10n a\n.ends CELLS";
    add_subckt_instance(&mut c, "U1", "CELLS", text, &nodes).unwrap();
    c.add(Element::vdc("Vcc", nodes[0], 0, 5.0));
    c.add(Element::vdc("Vss", nodes[1], 0, 0.0));
    c.add(Element::vdc("Va", nodes[2], 0, 0.0));
    c.add(Element::Resistor { name: "Rl".into(), a: nodes[3], b: 0, r: 1e3 });
    let mut run = TranRun::start(&c, &TranSettings { tstop: 1.0, tstep: 1e-3, uic: false }, Some(&|n: &str| n == "V(y)")).unwrap();
    run.advance_to(0.1).unwrap();
    assert!(run.voltage("y").unwrap() < 0.1);
    assert!(run.next_digital_event().is_none());
    assert!(run.set_source("Va", Waveform::Dc(5.0)));
    assert!(!run.set_source("Vnone", Waveform::Dc(5.0)));
    // a nanosecond on: the buffer has seen its input and its output is on its way
    run.advance_to(0.1 + 1e-9).unwrap();
    let due = run.next_digital_event().expect("the buffer's output is on its way");
    assert!(due > 0.1 && due < 0.1 + 12e-9, "{due}");
    assert!(run.voltage("y").unwrap() < 0.1, "not yet");
    // an output on an analog node that arrives at the very end of a piece is in the circuit equations of the next solve: step just past it
    run.advance_to(due + 1e-10).unwrap();
    assert!(run.voltage("y").unwrap() > 4.5, "arrived: {:?}", run.voltage("y"));
    assert!(run.next_digital_event().is_none());
    assert!(run.logic_supply().is_some_and(|v| (v - 5.0).abs() < 1e-9));
    // only the signal asked for is recorded, and the end can be pushed on
    assert_eq!(run.drain().signals.len(), 1);
    run.extend(2.0);
    run.advance_to(1.5).unwrap();
    assert_eq!(run.time(), 1.5);
}

/// The snapshot of a run is its operating point now: at the start it is the DC point (a 5 V source, 1 kohm and an LED-like diode),
/// and after the source is raised it carries the larger current, which the rating check reads.
#[test]
fn a_snapshot_is_the_operating_point_now() {
    let mut c = Circuit::new();
    let (a, k) = (c.node("a"), c.node("k"));
    c.add(Element::vdc("V1", a, 0, 5.0));
    c.add(Element::Resistor { name: "R1".into(), a, b: k, r: 1e3 });
    c.add(Element::Resistor { name: "R2".into(), a: k, b: 0, r: 1e3 });
    let dc = dc_op(&c).unwrap();
    let mut run = TranRun::start(&c, &TranSettings { tstop: 1.0, tstep: 1e-3, uic: false }, None).unwrap();
    let snap = run.snapshot();
    assert!((snap.element("R1").unwrap().current - dc.element("R1").unwrap().current).abs() < 1e-9);
    assert_eq!(snap.voltages[0], dc.voltages[0], "ground is in it, named as the DC point names it");
    assert!((snap.voltage("k").unwrap() - 2.5).abs() < 1e-6, "{:?} / dc {:?}", snap.voltages, dc.voltages);
    run.set_source("V1", Waveform::Dc(10.0));
    run.advance_to(0.01).unwrap();
    let snap = run.snapshot();
    assert!((snap.element("R1").unwrap().current - 5e-3).abs() < 1e-8, "{}", snap.element("R1").unwrap().current);
    assert!((snap.element("R1").unwrap().power - 25e-3).abs() < 1e-7);
}

/// A resistor changed while the run goes on (a potentiometer's knob turned) moves the divider at once, also in a linear circuit
/// whose factorised matrix the run keeps.
#[test]
fn a_resistance_changed_while_running_moves_the_divider() {
    let mut c = Circuit::new();
    let (a, k) = (c.node("a"), c.node("k"));
    c.add(Element::vdc("V1", a, 0, 10.0));
    c.add(Element::Resistor { name: "VR1.A".into(), a, b: k, r: 5e3 });
    c.add(Element::Resistor { name: "VR1.B".into(), a: k, b: 0, r: 5e3 });
    let mut run = TranRun::start(&c, &TranSettings { tstop: 1.0, tstep: 1e-3, uic: false }, None).unwrap();
    run.advance_to(0.01).unwrap();
    assert!((run.voltage("k").unwrap() - 5.0).abs() < 1e-6);
    assert!(run.set_resistance("VR1.A", 2e3) && run.set_resistance("VR1.B", 8e3));
    assert!(!run.set_resistance("nothing", 1e3) && !run.set_resistance("VR1.A", 0.0));
    run.advance_to(0.02).unwrap();
    assert!((run.voltage("k").unwrap() - 8.0).abs() < 1e-6, "{:?}", run.voltage("k"));
}
