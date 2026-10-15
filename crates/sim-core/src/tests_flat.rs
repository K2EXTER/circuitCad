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

#[test]
fn params_may_be_used_before_the_line_that_defines_them() {
    let t = ".subckt T a b gnd
R1 a b {Rx}
R2 b gnd {Rx}
.param Rx={Ry*2}
.param Ry=500
.ends T";
    assert!((run(t, "T", &["a", "b", "gnd"], 1.0, "b") - 0.5).abs() < 1e-9);
}

#[test]
fn behavioural_sources_with_value_expressions_and_param_names() {
    // G with VALUE={...}: 1 mA per volt of V(a) into 1k from node 0 to b; the expression keeps V(a) and takes {gain} from .param
    let t = ".subckt T a b gnd
.param gain=2m
G1 gnd b VALUE={gain*V(a,gnd)}
R2 b gnd 1k
.ends T";
    assert!((run(t, "T", &["a", "b", "gnd"], 1.5, "b") - 3.0).abs() < 1e-6);
    let e = ".subckt T a b gnd
E1 b gnd VALUE={3*V(a)+1}
R2 b gnd 1k
.ends T";
    assert!((run(e, "T", &["a", "b", "gnd"], 0.5, "b") - 2.5).abs() < 1e-6);
}

#[test]
fn poly_sources_and_the_current_controlled_f_and_h() {
    // E with POLY(2): V = 0 + .5 V(a) + .5 V(c) ; here both controls read V(a)
    let t = ".subckt T a b gnd
E1 b gnd poly(2) (a,gnd) (a,gnd) 0 .5 .5
R2 b gnd 1k
.ends T";
    assert!((run(t, "T", &["a", "b", "gnd"], 1.0, "b") - 1.0).abs() < 1e-6);
    // one control, cubic: p0 + p1 x + p2 x^2 = 1 + 2x + 3x^2 at x=1 -> 6
    let c = ".subckt T a b gnd
E1 b gnd poly(1) a gnd 1 2 3
R2 b gnd 1k
.ends T";
    assert!((run(c, "T", &["a", "b", "gnd"], 1.0, "b") - 6.0).abs() < 1e-6);
    // F: current gain 3 of the current through Vs (1 mA through 1k from a) into a 1k load; H: 2000 ohm transresistance
    let f = ".subckt T a b gnd
Ra a s 1k
Vs s gnd 0
F1 gnd b Vs 3
R2 b gnd 1k
.ends T";
    assert!((run(f, "T", &["a", "b", "gnd"], 1.0, "b") - 3.0).abs() < 1e-6, "{}", run(f, "T", &["a", "b", "gnd"], 1.0, "b"));
    let h = ".subckt T a b gnd
Ra a s 1k
Vs s gnd 0
H1 b gnd Vs 2000
R2 b gnd 1k
.ends T";
    assert!((run(h, "T", &["a", "b", "gnd"], 1.0, "b") - 2.0).abs() < 1e-6, "{}", run(h, "T", &["a", "b", "gnd"], 1.0, "b"));
}

#[test]
fn table_sources_and_tbl_are_piecewise_linear() {
    let t = ".subckt T a b gnd
E1 b gnd TABLE {V(a)} = (0,0) (1,2) (2,2.5)
R2 b gnd 1k
.ends T";
    for (vin, want) in [(0.5, 1.0), (1.5, 2.25), (5.0, 2.5), (-1.0, 0.0)] {
        assert!((run(t, "T", &["a", "b", "gnd"], vin, "b") - want).abs() < 1e-6, "{vin}");
    }
    let b = ".subckt T a b gnd
B1 b gnd V=tbl(V(a), 0,0, 1,2, 2,2.5)
R2 b gnd 1k
.ends T";
    assert!((run(b, "T", &["a", "b", "gnd"], 1.5, "b") - 2.25).abs() < 1e-6);
}

#[test]
fn ltspice_logic_functions_and_options_after_a_b_expression() {
    // & | ! buf u pwr, and `Rpar=...` after the expression
    let t = ".subckt T a b gnd
B1 b gnd V=(!V(a) | buf(V(a))&V(a)) + u(V(a)-0.5) + pwr(2,3) Rpar=25k tripdt=30n
R2 b gnd 1k
.ends T";
    // V(a)=1: (0 | 1&1)=1, u(0.5)=1, pwr=8 -> 10
    assert!((run(t, "T", &["a", "b", "gnd"], 1.0, "b") - 10.0).abs() < 1e-3, "{}", run(t, "T", &["a", "b", "gnd"], 1.0, "b"));
}

#[test]
fn a_four_node_transistor_lpnp_area_and_end_are_read() {
    let t = ".subckt T c b e
Q1 c b e 0 QM 2
R1 b e 100k
.model QM LPNP(BF=50 IS=1e-15)
.end";
    let mut c = Circuit::new();
    let nodes: Vec<usize> = ["c", "b", "e"].iter().map(|p| c.node(p)).collect();
    add_subckt_instance(&mut c, "U1", "T", t, &nodes).unwrap_or_else(|e| panic!("{e}"));
    // area 2 doubles IS
    let is = c.elements.iter().find_map(|e| match e { Element::Bjt { model, .. } => Some((model.is, model.pnp)), _ => None }).unwrap();
    assert!((is.0 / 2e-15 - 1.0).abs() < 1e-9 && is.1, "{is:?}");
}

#[test]
fn model_cards_with_params_and_vswitch_and_unused_parameters() {
    let t = ".subckt X a b gnd params: ron=10
S1 a b a gnd SW1
.model SW1 VSWITCH(Ron={ron} Roff=1Meg Von=1 Voff=0)
.ends X
             .subckt T a b gnd
X1 a b gnd X ron=2
.ends T";
    let mut c = Circuit::new();
    let nodes: Vec<usize> = ["a", "b"].iter().map(|p| c.node(p)).collect();
    add_subckt_instance(&mut c, "U1", "T", t, &[nodes[0], nodes[1], 0]).unwrap_or_else(|e| panic!("{e}"));
    let ron = c.elements.iter().find_map(|e| match e { Element::Switch { model, .. } => Some(model.ron), _ => None }).unwrap();
    assert_eq!(ron, 2.0);
    let ign = ignored_model_params(".model Q1 NPN(IS=1f BF=100 VAF=80 TF=1n CJE=1p)
.model D1 D(IS=1n RS=1)");
    assert_eq!(ign, vec![("Q1".to_string(), vec!["TF".to_string(), "CJE".to_string()])]);
}

/// Developer check with real vendor files: `SPICE_MODEL_DIR=<dir> cargo test -p sim-core vendor_models -- --ignored --nocapture`.
/// Prints what each model does in a simple circuit (not asserted: the files are not part of the repository).
#[test]
#[ignore]
fn vendor_models_behave_in_simple_circuits() {
    let Ok(dir) = std::env::var("SPICE_MODEL_DIR") else { return };
    let read = |f: &str| String::from_utf8_lossy(&std::fs::read(std::path::Path::new(&dir).join(f)).unwrap()).to_string();
    let run = |label: &str, file: &str, top: &str, ports: &[&str], extra: &dyn Fn(&mut Circuit), probe: &[&str]| {
        let text = read(file);
        let mut c = Circuit::new();
        let nodes: Vec<usize> = ports.iter().map(|p| if *p == "0" { 0 } else { c.node(p) }).collect();
        if let Err(e) = add_subckt_instance(&mut c, "U1", top, &text, &nodes) {
            println!("{label}: READ ERROR {e}");
            return;
        }
        extra(&mut c);
        match dc_op(&c) {
            Ok(r) => println!("{label}: {}", probe.iter().map(|p| format!("V({p})={:.4}", r.voltage(p).unwrap_or(f64::NAN))).collect::<Vec<_>>().join("  ")),
            Err(e) => println!("{label}: DC ERROR {e:?}"),
        }
    };
    let vdc = |c: &mut Circuit, n: &str, node: &str, v: f64| {
        let a = c.node(node);
        c.add(Element::vdc(n, a, 0, v));
    };
    let res = |c: &mut Circuit, n: &str, a: &str, b: &str, r: f64| {
        let (a, b) = (c.node(a), c.node(b));
        c.add(Element::Resistor { name: n.into(), a, b, r });
    };
    run("LM7805 (10 V in, 100 ohm load)", "LM78xx.txt", "LM7805", &["in", "0", "out"], &|c| {
        vdc(c, "Vin", "in", 10.0);
        res(c, "Rl", "out", "0", 100.0);
    }, &["out"]);
    run("LM358 buffer (in 2.5 V, supply 5 V)", "lm358.mod", "LM358", &["inp", "out", "vcc", "0", "out"], &|c| {
        vdc(c, "Vcc", "vcc", 5.0);
        vdc(c, "Vin", "inp", 2.5);
        res(c, "Rl", "out", "0", 10e3);
    }, &["out"]);
    run("TL431 (5 V via 1k, K tied to ADJ)", "TL431ED.sub", "TL431ED", &["k", "0", "k"], &|c| {
        vdc(c, "Vs", "vs", 5.0);
        res(c, "Rs", "vs", "k", 1e3);
    }, &["k"]);
    run("IRF540 (Vgs 10 V, 12 V via 10 ohm)", "irf540.spi", "irf540", &["d", "g", "0"], &|c| {
        vdc(c, "Vg", "g", 10.0);
        vdc(c, "Vs", "vs", 12.0);
        res(c, "Rd", "vs", "d", 10.0);
    }, &["d"]);
    run("LM324LV buffer (in 1 V, supply 3.3 V)", "LM324LV.cir", "LM324LV", &["inp", "out", "vcc", "0", "out"], &|c| {
        vdc(c, "Vcc", "vcc", 3.3);
        vdc(c, "Vin", "inp", 1.0);
        res(c, "Rl", "out", "0", 10e3);
    }, &["out"]);
}
