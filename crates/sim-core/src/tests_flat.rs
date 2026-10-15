//! Vendor-style SPICE text: `.param`, `{expr}`, `params:` defaults and nested `X` calls.

use crate::*;

fn run(text: &str, top: &str, ports: &[&str], vin: f64, probe: &str) -> f64 {
    let mut c = Circuit::new();
    let nodes: Vec<usize> = ports.iter().map(|p| if *p == "gnd" { 0 } else { c.node(p) }).collect();
    add_subckt_instance(&mut c, "U1", top, text, &nodes).unwrap_or_else(|e| panic!("{e}"));
    let a = c.node(ports[0]);
    c.add(Element::vdc("Vin", a, 0, vin));
    dc_op(&c).unwrap().voltage(probe).unwrap()
}

fn err(text: &str, top: &str, ports: &[&str]) -> String {
    let mut c = Circuit::new();
    let nodes: Vec<usize> = ports.iter().map(|p| c.node(p)).collect();
    add_subckt_instance(&mut c, "U1", top, text, &nodes).unwrap_err()
}

#[test]
fn param_expressions_set_values() {
    let t = ".param Rx=2k\n.subckt T a b gnd\nR1 a b {Rx*2}\nR2 b gnd '4k'\n.ends T";
    assert!((run(t, "T", &["a", "b", "gnd"], 1.0, "b") - 0.5).abs() < 1e-9);
}

#[test]
fn params_defaults_and_call_arguments() {
    let t = ".subckt DIV in out gnd params: r=1k k=2\nR1 in out {r}\nR2 out gnd {r*k}\n.ends\n\
             .subckt T a b gnd\nX1 a b gnd DIV r=1k k=3\n.ends T";
    // 1k over 3k: 0.75
    assert!((run(t, "T", &["a", "b", "gnd"], 1.0, "b") - 0.75).abs() < 1e-9);
    // the default k=2 when the caller gives only r
    let t2 = t.replace("r=1k k=3", "r=1k");
    assert!((run(&t2, "T", &["a", "b", "gnd"], 1.0, "b") - 2.0 / 3.0).abs() < 1e-9);
}

#[test]
fn nested_calls_keep_internal_nodes_and_expressions_apart() {
    // a gain-of-2 stage with an internal node, used twice in a row
    let t = ".subckt BUF in out\nR1 in n 1\nB1 out 0 V=2*V(n)\nRl out 0 1Meg\n.ends\n\
             .subckt T a b\nX1 a m BUF\nX2 m b BUF\n.ends T";
    let v = run(t, "T", &["a", "b"], 1.0, "b");
    // R1 is a 1 ohm into a node that nothing else loads, so V(n)=V(in): gain 2 per stage
    assert!((v - 4.0).abs() < 1e-3, "{v}");
}

#[test]
fn the_top_subckt_need_not_be_first() {
    let t = ".subckt INNER p q\nR1 p q 1k\n.ends\n.subckt T a b gnd\nX1 a b INNER\nR2 b gnd 1k\n.ends T";
    assert!((run(t, "T", &["a", "b", "gnd"], 2.0, "b") - 1.0).abs() < 1e-9);
}

#[test]
fn an_unknown_inner_subckt_is_explained() {
    let e = err(".subckt T a b\nX1 a b NOPE\n.ends T", "T", &["a", "b"]);
    assert!(e.contains("NOPE") && e.contains("貼って"), "{e}");
}

#[test]
fn an_unknown_parameter_is_named() {
    let e = err(".subckt T a b\nR1 a b {missing}\n.ends T", "T", &["a", "b"]);
    assert!(e.contains("missing"), "{e}");
}

#[test]
fn a_subckt_calling_itself_stops() {
    let e = err(".subckt T a b\nX1 a b T\n.ends T", "T", &["a", "b"]);
    assert!(e.contains("深すぎ"), "{e}");
}
