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
    let ign = ignored_model_params(".model Q1 NPN(IS=1f BF=100 VAF=80 TF=1n CJE=1p KF=1f ITF=1m)
.model D1 D(IS=1n RS=1)");
    assert_eq!(ign, vec![("Q1".to_string(), vec!["KF".to_string(), "ITF".to_string()])]);
}

/// Developer check with real vendor files: `SPICE_MODEL_DIR=<dir> cargo test -p sim-core vendor_models -- --ignored --nocapture`.
/// Prints what each model does in a simple circuit (not asserted: the files are not part of the repository).
#[test]
#[ignore]
fn vendor_models_behave_in_simple_circuits() {
    let Ok(dir) = std::env::var("SPICE_MODEL_DIR") else { return };
    let read = |f: &str| std::fs::read(std::path::Path::new(&dir).join(f)).ok().map(|b| String::from_utf8_lossy(&b).to_string());
    let run = |label: &str, file: &str, top: &str, ports: &[&str], extra: &dyn Fn(&mut Circuit), probe: &[&str]| {
        let Some(text) = read(file) else { return };
        let mut c = Circuit::new();
        let nodes: Vec<usize> = ports.iter().map(|p| if *p == "0" { 0 } else { c.node(p) }).collect();
        if let Err(e) = add_subckt_instance(&mut c, "U1", top, &text, &nodes) {
            println!("{label}: READ ERROR {e}");
            return;
        }
        extra(&mut c);
        match dc_op(&c) {
            Ok(r) if probe == ["*"] => println!("{label}: {}", c.names.iter().skip(1).map(|n| format!("{n}={:.4}", r.voltage(n).unwrap_or(f64::NAN))).collect::<Vec<_>>().join("  ")),
            Ok(r) => println!("{label}: {}", probe.iter().map(|p| format!("V({p})={:.4}", r.voltage(p).unwrap_or(f64::NAN))).collect::<Vec<_>>().join("  ")),
            Err(e) => println!("{label}: DC ERROR {e:?} {:?}", crate::diagnose(&c, &e)),
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
    // op-amps as unity-gain buffers (+ in, - in, V+, V-, out, ...): the pin order is the file's own
    for (label, file, top, order) in [
        ("ua741 buffer", "ua741.mod", "UA741", ["inp", "out", "vcc", "vee", "out"]),
        ("TL082 buffer (JFET input)", "tl082.mod", "TL082", ["inp", "out", "vcc", "vee", "out"]),
        ("OPA211 buffer", "OPA211.mod", "OPA211", ["inp", "out", "vcc", "vee", "out"]),
        ("MCP6001 buffer", "MCP6001.lib", "MCP6001", ["inp", "out", "vcc", "0", "out"]),
    ] {
        run(&format!("{label} (in 2 V, +/-5 V)"), file, top, &order, &|c| {
            vdc(c, "Vcc", "vcc", 5.0);
            if order[3] == "vee" {
                vdc(c, "Vee", "vee", -5.0);
            }
            vdc(c, "Vin", "inp", 2.0);
            res(c, "Rl", "out", "0", 10e3);
        }, &["out"]);
    }
    run("OPA211 all nodes (in 2 V)", "OPA211.mod", "OPA211", &["inp", "out", "vcc", "vee", "out"], &|c| {
        vdc(c, "Vcc", "vcc", 5.0);
        vdc(c, "Vee", "vee", -5.0);
        vdc(c, "Vin", "inp", 2.0);
        res(c, "Rl", "out", "0", 10e3);
    }, &["*"]);
    run("LM337 (-12 V in, 100 ohm load)", "lm337-1.lib", "LM337-1", &["in", "adj", "out"], &|c| {
        vdc(c, "Vin", "in", -12.0);
        res(c, "Rl", "out", "0", 100.0);
        res(c, "Rs", "out", "adj", 240.0);
        res(c, "Rg", "adj", "0", 1000.0);
    }, &["out", "adj"]);
    run("2N3904 (Vbe drive 5 V via 10k, collector 5 V via 1k)", "2n3904.lib", "Q2N3904", &["c", "b", "0"], &|c| {
        vdc(c, "Vb", "vb", 5.0);
        res(c, "Rb", "vb", "b", 10e3);
        vdc(c, "Vc", "vc", 5.0);
        res(c, "Rc", "vc", "c", 1e3);
    }, &["c", "b"]);
    run("IRFZ44N (Vgs 10 V, 12 V via 10 ohm)", "irfz44n.spi", "irfz44n", &["d", "g", "0"], &|c| {
        vdc(c, "Vg", "g", 10.0);
        vdc(c, "Vs", "vs", 12.0);
        res(c, "Rd", "vs", "d", 10.0);
    }, &["d"]);
    run("LM35 (supply 5 V)", "LM35_my_model.lib", "LM35", &["out", "vcc", "0"], &|c| {
        vdc(c, "Vcc", "vcc", 5.0);
        res(c, "Rl", "out", "0", 100e3);
    }, &["*"]);
}

/// Developer helper: print the flattened text of `SPICE_DUMP_FILE` (top subcircuit `SPICE_DUMP_TOP`) from `SPICE_MODEL_DIR`.
#[test]
#[ignore]
fn dump_flat_text() {
    let (Ok(dir), Ok(file), Ok(top)) = (std::env::var("SPICE_MODEL_DIR"), std::env::var("SPICE_DUMP_FILE"), std::env::var("SPICE_DUMP_TOP")) else { return };
    let text = String::from_utf8_lossy(&std::fs::read(std::path::Path::new(&dir).join(file)).unwrap()).to_string();
    let lines = crate::spice_in::logical_lines(&text);
    match crate::spice_flat::flatten(&text, &top, &lines) {
        Ok(Some(s)) => println!("{s}"),
        other => println!("{other:?}"),
    }
}

/// Developer helper: find the voltage-defining elements (V, E, L, voltage-type B) that close a loop in a subcircuit.
#[test]
#[ignore]
fn find_voltage_loops() {
    let (Ok(dir), Ok(file), Ok(top)) = (std::env::var("SPICE_MODEL_DIR"), std::env::var("SPICE_DUMP_FILE"), std::env::var("SPICE_DUMP_TOP")) else { return };
    let text = String::from_utf8_lossy(&std::fs::read(std::path::Path::new(&dir).join(file)).unwrap()).to_string();
    let mut c = Circuit::new();
    let probe = text.lines().find(|l| l.to_ascii_lowercase().starts_with(".subckt")).map(|l| l.split_whitespace().count() - 2).unwrap_or(0);
    let ports: Vec<usize> = (0..probe).map(|i| c.node(&format!("p{i}"))).collect();
    add_subckt_instance(&mut c, "U1", &top, &text, &ports).unwrap();
    let mut parent: Vec<usize> = (0..c.names.len()).collect();
    fn find(p: &mut Vec<usize>, i: usize) -> usize {
        if p[i] != i {
            let r = find(p, p[i]);
            p[i] = r;
        }
        p[i]
    }
    for e in &c.elements {
        let pair = match e {
            Element::VSource { name, p, n, .. } => Some((name.clone(), *p, *n)),
            Element::Vcvs { name, p, n, .. } => Some((name.clone(), *p, *n)),
            Element::Inductor { name, a, b, .. } => Some((name.clone(), *a, *b)),
            Element::BSource { name, p, n, voltage: true, .. } => Some((name.clone(), *p, *n)),
            _ => None,
        };
        if let Some((name, a, b)) = pair {
            let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
            if ra == rb {
                println!("LOOP closes at {name}: {} -> {}", c.names[a], c.names[b]);
            } else {
                parent[ra] = rb;
            }
        }
    }
}

#[test]
fn a_source_with_temp_in_its_value_follows_the_circuit_temperature() {
    // the LM35 trick: V = temp * 10 mV (temp in degrees C)
    let t = ".subckt T a b gnd\nV1 a gnd {temp*0.01}\nR1 a b 1k\nR2 b gnd 1k\n.ends T";
    let mut c = Circuit::new();
    let nodes: Vec<usize> = ["a", "b"].iter().map(|p| c.node(p)).collect();
    add_subckt_instance(&mut c, "U1", "T", t, &[nodes[0], nodes[1], 0]).unwrap_or_else(|e| panic!("{e}"));
    let at = |temp: f64| dc_op(&c.at_temperature(temp)).unwrap().voltage("a").unwrap();
    assert!((at(27.0) - 0.27).abs() < 1e-9 && (at(100.0) - 1.0).abs() < 1e-9, "{} {}", at(27.0), at(100.0));
}

#[test]
fn nested_subckt_definitions_resistor_models_jfets_and_ota_are_read() {
    // a .subckt inside a .subckt; `R1 a b MODEL value`; an NJF; the LTspice OTA of the universal op-amp
    let t = ".subckt T a b gnd\nXI a b gnd INNER\n.subckt INNER p q g\nR1 p q RM 2k\n.model RM RES(T_ABS=-273)\n.ends INNER\n.ends T";
    let t = t.replace("\\n", "\n");
    assert!((run(&t, "T", &["a", "b", "gnd"], 1.0, "a") - 1.0).abs() < 1e-9);
    // an n-channel JFET with a pinch-off of -2 V: with the gate at 0 V it conducts, at -3 V it is off
    let j = ".subckt T d g s\nJ1 d g s JM\n.model JM NJF(Vto=-2 Beta=1m)\n.ends T";
    let on = {
        let mut c = Circuit::new();
        let (d, g) = (c.node("d"), c.node("g"));
        add_subckt_instance(&mut c, "U1", "T", &j.replace("\\n", "\n"), &[d, g, 0]).unwrap_or_else(|e| panic!("{e}"));
        c.add(Element::vdc("Vd", d, 0, 5.0));
        c.add(Element::vdc("Vg", g, 0, 0.0));
        -dc_op(&c).unwrap().element("Vd").unwrap().current
    };
    // saturation: beta * vp^2 = 1 mA * 4 = 4 mA
    assert!((on / 4e-3 - 1.0).abs() < 0.02, "{on}");
    let off = {
        let mut c = Circuit::new();
        let (d, g) = (c.node("d"), c.node("g"));
        add_subckt_instance(&mut c, "U1", "T", &j.replace("\\n", "\n"), &[d, g, 0]).unwrap();
        c.add(Element::vdc("Vd", d, 0, 5.0));
        c.add(Element::vdc("Vg", g, 0, -3.0));
        -dc_op(&c).unwrap().element("Vd").unwrap().current
    };
    assert!(off.abs() < 1e-6, "{off}");
}

#[test]
fn the_ota_a_device_is_a_transconductance_and_the_other_a_devices_are_explained() {
    // I = G * (V(second node) - V(first node)) into the output: a follower with a 1 kohm load reads G R / (1 + G R)
    let t = ".subckt T inp out gnd\nA1 out inp 0 0 0 0 out 0 OTA G=1 Cout=1p Iout=1\nR1 out gnd 1k\n.ends T";
    let v = run(t, "T", &["inp", "out", "gnd"], 1.0, "out");
    assert!((v - 1000.0 / 1001.0).abs() < 1e-6, "{v}");
    // a device with no meaning here (not a gate, flip-flop or Schmitt trigger) is named in the error
    let e = err(".subckt T a b gnd\nA1 a 0 0 0 0 0 b 0 ONESHOT Vt=0 Vh=1m\n.ends T", "T", &["a", "b"]);
    assert!(e.contains("ONESHOT") && e.contains("未対応"), "{e}");
}

/// Developer helper: for a subcircuit that does not solve, list the elements whose removal makes the DC point solvable.
/// `SPICE_MODEL_DIR=<dir> SPICE_DUMP_FILE=<file> SPICE_DUMP_TOP=<name> SPICE_DUMP_PORTS="inp out vcc 0 out" cargo test -p sim-core bisect_singular -- --ignored --nocapture`
#[test]
#[ignore]
fn bisect_singular() {
    let (Ok(dir), Ok(file), Ok(top), Ok(ports)) = (std::env::var("SPICE_MODEL_DIR"), std::env::var("SPICE_DUMP_FILE"), std::env::var("SPICE_DUMP_TOP"), std::env::var("SPICE_DUMP_PORTS")) else { return };
    let text = String::from_utf8_lossy(&std::fs::read(std::path::Path::new(&dir).join(file)).unwrap()).to_string();
    let mut c = Circuit::new();
    let nodes: Vec<usize> = ports.split_whitespace().map(|p| if p == "0" { 0 } else { c.node(p) }).collect();
    add_subckt_instance(&mut c, "U1", &top, &text, &nodes).unwrap();
    for (name, node, v) in [("Vcc", "vcc", 5.0), ("Vin", "inp", 2.0)] {
        let n = c.node(node);
        c.add(Element::vdc(name, n, 0, v));
    }
    let out = c.node("out");
    c.add(Element::Resistor { name: "Rl".into(), a: out, b: 0, r: 10e3 });
    println!("full circuit: {:?}", dc_op(&c).map(|_| "ok"));
    let mut fixers = vec![];
    for i in 0..c.elements.len() {
        let mut c2 = c.clone();
        let name = c2.elements.remove(i).name().to_string();
        if dc_op(&c2).is_ok() {
            fixers.push(name);
        }
    }
    println!("removing one of these makes it solvable: {fixers:?}");
}

#[test]
fn rpar_and_cpar_on_a_b_source_become_a_resistor_and_a_capacitor() {
    // a node fed only by a current source needs Rpar to have a voltage: 1 mA * 2k = 2 V
    let t = ".subckt T a b gnd\nB1 gnd b I=1m*V(a) Rpar=2k Cpar=1p\n.ends T";
    assert!((run(t, "T", &["a", "b", "gnd"], 1.0, "b") - 2.0).abs() < 1e-6);
}

#[test]
fn blanks_around_a_parameter_equals_sign_are_squeezed_but_comparisons_stay() {
    use crate::spice_in::logical_lines;
    let l = logical_lines(".MODEL DMOS NMOS ( KP= {MOS_KP}  VTO = 3  W =1u)\nB1 o 0 V = IF(V(a) >= 1 & V(b) == 2, 1, 0)");
    assert_eq!(l[0], ".MODEL DMOS NMOS ( KP={MOS_KP} VTO=3 W=1u)");
    assert_eq!(l[1], "B1 o 0 V=IF(V(a) >= 1 & V(b) == 2, 1, 0)");
}

/// A transformer from two coupled inductors (`K`): the voltage ratio follows the turns ratio sqrt(L2/L1).
#[test]
fn coupled_inductors_make_a_transformer_in_ac_and_in_the_time_domain() {
    let t = ".subckt TR a b gnd
L1 a gnd 1m
L2 b gnd 4m
K1 L1 L2 0.999
.ends TR";
    let build = |wave: Waveform| {
        let mut c = Circuit::new();
        let (vin, a, b) = (c.node("vin"), c.node("a"), c.node("b"));
        c.add(Element::VSource { name: "V1".into(), p: vin, n: 0, wave, ac: Some((1.0, 0.0)) });
        c.add(Element::Resistor { name: "Rs".into(), a: vin, b: a, r: 1.0 });
        add_subckt_instance(&mut c, "T1", "TR", t, &[a, b, 0]).unwrap_or_else(|e| panic!("{e}"));
        c.add(Element::Resistor { name: "Rl".into(), a: b, b: 0, r: 100e3 });
        c
    };
    // AC at 100 kHz (the inductances are far above the source resistance): ratio 2
    let ac = crate::ac_analysis(&build(Waveform::Dc(0.0)), &crate::AcSettings { fstart: 1e5, fstop: 2e5, sweep: crate::AcSweep::Linear(2) }).unwrap_or_else(|e| panic!("{e:?}"));
    let gain = ac.signals.iter().find(|s| s.name == "V(b)").map(|s| s.mag()[0]).unwrap();
    assert!((gain - 2.0).abs() < 0.05, "AC gain {gain}");
    // transient: a 1 V, 100 kHz sine comes out about 2 V in amplitude
    let sine = Waveform::Sin { vo: 0.0, va: 1.0, freq: 1e5, td: 0.0, theta: 0.0, phase: 0.0 };
    let r = transient(&build(sine), &TranSettings { tstop: 60e-6, tstep: 0.1e-6, uic: false }).unwrap();
    let out = r.signal("V(b)").unwrap();
    let late = &out.values[out.values.len() / 2..];
    let peak = late.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    assert!((peak - 2.0).abs() < 0.15, "transient peak {peak}");
}

/// `POLY(2)` with second-order terms: E = 1 + 2 x + 3 y + 4 x^2 + 5 x y + 6 y^2.
#[test]
fn poly_with_product_terms_follows_the_spice_term_order() {
    let t = ".subckt T x y out gnd
E1 out gnd POLY(2) x gnd y gnd 1 2 3 4 5 6
.ends T";
    let mut c = Circuit::new();
    let (x, y, out) = (c.node("x"), c.node("y"), c.node("out"));
    c.add(Element::vdc("Vx", x, 0, 2.0));
    c.add(Element::vdc("Vy", y, 0, 3.0));
    add_subckt_instance(&mut c, "U1", "T", t, &[x, y, out, 0]).unwrap_or_else(|e| panic!("{e}"));
    c.add(Element::Resistor { name: "Rl".into(), a: out, b: 0, r: 1e3 });
    let r = dc_op(&c).unwrap();
    // 1 + 4 + 9 + 16 + 30 + 54 = 114
    assert!((r.voltage("out").unwrap() - 114.0).abs() < 1e-6, "{}", r.voltage("out").unwrap());
}

/// Vendor-file habits that used to stop the whole read: one unreadable `.model` card among many, `.params`, blanks around
/// `=`, `DC 0 PULSE(..)`, a value-less probe source, `POLY 2` without parentheses, `TABLE` points without parentheses and a
/// comma after a header parameter.
#[test]
fn vendor_file_habits_are_read() {
    let t = ".param unused=1
.model BAD D(IS=)
.model DREV D(IS=1e-14 RS=1)
.subckt T a b gnd params: gain=2, off=0.5,
.params k=3
Vsense a n 
Vp p 0 DC 0 PULSE(0 1 0 1n 1n 1m 2m)
R1 n gnd 1k
D1 b gnd DREV
E1 b2 gnd POLY 1 n gnd 0 {gain}
E2 b3 gnd TABLE {V(n)}
+ 0 0
+ 1 10
R2 b2 gnd 1k
R3 b3 gnd 1k
R4 b gnd 1k
.ends T";
    let mut c = Circuit::new();
    let (a, b) = (c.node("a"), c.node("b"));
    c.add(Element::vdc("V1", a, 0, 1.0));
    add_subckt_instance(&mut c, "U1", "T", t, &[a, b, 0]).unwrap_or_else(|e| panic!("{e}"));
    let r = dc_op(&c).unwrap();
    // the probe is 0 V, so n follows a; E1 = gain * V(n) = 2; E2 = 10 * V(n)
    assert!((r.voltage("U1.b2").unwrap() - 2.0).abs() < 1e-6, "{:?}", r.voltage("U1.b2"));
    assert!((r.voltage("U1.b3").unwrap() - 10.0).abs() < 1e-6, "{:?}", r.voltage("U1.b3"));
    // an element that does use the unreadable card says so instead of "not found"
    let bad = ".model BAD D(IS=)\n.subckt X a gnd\nD1 a gnd BAD\n.ends X";
    let mut c2 = Circuit::new();
    let n = c2.node("n");
    let e = add_subckt_instance(&mut c2, "U1", "X", bad, &[n, 0]).unwrap_err();
    assert!(e.contains("読めません"), "{e}");
}

/// Combinational gates written as `A` devices: ngspice's XSPICE form (with `d_and` / `d_inverter` models) and LTspice's
/// eight-pin form. The levels are LTspice's 0 and 1 V.
#[test]
fn combinational_a_devices_are_logic_gates() {
    let xspice = ".subckt G a b y gnd
.model and d_and
.model inv d_inverter
aand1 [a b] p and
ainv p y inv
.ends G";
    let lt = ".subckt H a b y gnd
A1 a b 0 0 0 0 y 0 NAND
.ends H";
    let run = |text: &str, name: &str, va: f64, vb: f64| {
        let mut c = Circuit::new();
        let (a, b, y) = (c.node("a"), c.node("b"), c.node("y"));
        c.add(Element::vdc("Va", a, 0, va));
        c.add(Element::vdc("Vb", b, 0, vb));
        add_subckt_instance(&mut c, "U1", name, text, &[a, b, y, 0]).unwrap_or_else(|e| panic!("{e}"));
        c.add(Element::Resistor { name: "Rl".into(), a: y, b: 0, r: 10e3 });
        dc_op(&c).unwrap().voltage("y").unwrap()
    };
    // AND then inverter = NAND: low only when both inputs are high
    for (text, name) in [(xspice, "G"), (lt, "H")] {
        assert!(run(text, name, 1.0, 1.0) < 0.02, "{name} 1,1");
        assert!(run(text, name, 1.0, 0.0) > 0.98, "{name} 1,0");
        assert!(run(text, name, 0.0, 0.0) > 0.98, "{name} 0,0");
    }
}

/// A PSpice 7400 (`optional:` supply pins, `U nand(2)`), an open-collector output and a tri-state buffer.
#[test]
fn pspice_u_gates_open_collector_and_tristate_read() {
    let nand = ".subckt 7400 1A 1B 1Y
+     optional: DPWR=$G_DPWR DGND=$G_DGND
+     params: MNTYMXDLY=0 IO_LEVEL=0
U1 nand(2) DPWR DGND
+     1A 1B 1Y
+     DLY_00 IO_STD MNTYMXDLY={MNTYMXDLY} IO_LEVEL={IO_LEVEL}
.model DLY_00 ugate (tplhTY=11ns)
.ends 7400";
    let mut c = Circuit::new();
    let (a, b, y) = (c.node("a"), c.node("b"), c.node("y"));
    c.add(Element::vdc("Va", a, 0, 5.0));
    c.add(Element::vdc("Vb", b, 0, 5.0));
    add_subckt_instance(&mut c, "U1", "7400", nand, &[a, b, y]).unwrap_or_else(|e| panic!("{e}"));
    c.add(Element::Resistor { name: "Rl".into(), a: y, b: 0, r: 10e3 });
    let low = dc_op(&c).unwrap().voltage("y").unwrap();
    assert!(low < 0.1, "NAND(1,1) {low}");
    // open collector (pulled up by 1 k to 5 V): low when the input is false, the pull-up level when it is true
    let oc = ".subckt OC in out gnd
.model oc d_open_c
aoc in out oc
.ends OC";
    for (vin, want) in [(0.0, 0.0), (1.0, 5.0)] {
        let mut c = Circuit::new();
        let (i, o, vcc) = (c.node("i"), c.node("o"), c.node("vcc"));
        c.add(Element::vdc("Vi", i, 0, vin));
        c.add(Element::vdc("Vcc", vcc, 0, 5.0));
        c.add(Element::Resistor { name: "Rp".into(), a: vcc, b: o, r: 1e3 });
        add_subckt_instance(&mut c, "U1", "OC", oc, &[i, o, 0]).unwrap_or_else(|e| panic!("{e}"));
        let v = dc_op(&c).unwrap().voltage("o").unwrap();
        assert!((v - want).abs() < 0.6, "open collector in {vin}: {v}");
    }
}

/// Flip-flops and latches keep a stored state: a toggling D flip-flop (D tied to its own Q-bar) divides the clock by two, a
/// Schmitt trigger has hysteresis, and an S-R latch holds what was set.
#[test]
fn flip_flops_latches_and_schmitt_triggers_have_a_memory() {
    // ripple stage: q toggles on every rising clock edge
    let tff = ".subckt T clk q qb gnd
.model ff d_dff
a1 qb clk NULL NULL q qb ff
.ends T";
    let mut c = Circuit::new();
    let (clk, q, qb) = (c.node("clk"), c.node("q"), c.node("qb"));
    c.add(Element::VSource { name: "Vc".into(), p: clk, n: 0, wave: Waveform::Pulse { v1: 0.0, v2: 1.0, td: 0.5e-6, tr: 10e-9, tf: 10e-9, pw: 1e-6, per: 2e-6 }, ac: None });
    add_subckt_instance(&mut c, "U1", "T", tff, &[clk, q, qb, 0]).unwrap_or_else(|e| panic!("{e}"));
    c.add(Element::Resistor { name: "Rq".into(), a: q, b: 0, r: 10e3 });
    c.add(Element::Resistor { name: "Rqb".into(), a: qb, b: 0, r: 10e3 });
    let r = transient(&c, &TranSettings { tstop: 9e-6, tstep: 20e-9, uic: false }).unwrap();
    let vq = r.signal("V(q)").unwrap();
    let rising = vq.values.windows(2).filter(|w| w[0] < 0.5 && w[1] >= 0.5).count();
    let (lo, hi) = vq.values.iter().fold((f64::MAX, f64::MIN), |a, v| (a.0.min(*v), a.1.max(*v)));
    // 4 clock edges (at 0.5, 2.5, 4.5, 6.5, 8.5 µs = 5): q rises on edges 1, 3, 5
    assert!(lo < 0.05 && hi > 0.95, "swing {lo} {hi}");
    assert_eq!(rising, 3, "q rose {rising} times in 5 clock edges");
    // Schmitt trigger (LTspice A device): trips at Vt + Vh going up, back at Vt - Vh going down
    let sch = ".subckt S in out gnd
A1 in 0 0 0 0 0 out 0 SCHMITT Vt=0.5 Vh=0.1
.ends S";
    let mut c = Circuit::new();
    let (i, o) = (c.node("i"), c.node("o"));
    let ramp = Waveform::Pwl(vec![(0.0, 0.0), (1e-3, 1.0), (2e-3, 0.0)]);
    c.add(Element::VSource { name: "Vi".into(), p: i, n: 0, wave: ramp, ac: None });
    add_subckt_instance(&mut c, "U1", "S", sch, &[i, o, 0]).unwrap_or_else(|e| panic!("{e}"));
    c.add(Element::Resistor { name: "Ro".into(), a: o, b: 0, r: 10e3 });
    let r = transient(&c, &TranSettings { tstop: 2e-3, tstep: 5e-6, uic: false }).unwrap();
    let (vi, vo) = (r.signal("V(i)").unwrap(), r.signal("V(o)").unwrap());
    let level_at = |lo: f64, hi: f64, up: bool| {
        // the input voltage at the first point where the output is high (rising ramp) / low again (falling ramp)
        let n = r.time.len();
        let range: Vec<usize> = (0..n).filter(|&k| (r.time[k] < 1e-3) == up).collect();
        range.iter().copied().find(|&k| if up { vo.values[k] > 0.5 } else { vo.values[k] < 0.5 }).map(|k| vi.values[k]).filter(|v| *v > lo && *v < hi)
    };
    assert!(level_at(0.55, 0.7, true).is_some(), "rising trip near 0.6 V");
    assert!(level_at(0.3, 0.45, false).is_some(), "falling trip near 0.4 V");
}

/// A vendor model's `C1 V+ V- 10p IC=0` with the supply connected: the initial voltage contradicts the voltage sources, so it
/// is not held (SPICE ignores `IC` without UIC) and the transient starts from the ordinary operating point.
#[test]
fn an_initial_voltage_across_fixed_nodes_is_dropped_not_singular() {
    let t = ".subckt M vp gnd out
C1 vp gnd 10p IC=0
R1 vp out 1k
.ends M";
    let mut c = Circuit::new();
    let (vp, out) = (c.node("vp"), c.node("out"));
    c.add(Element::vdc("Vs", vp, 0, 5.0));
    add_subckt_instance(&mut c, "U1", "M", t, &[vp, 0, out]).unwrap_or_else(|e| panic!("{e}"));
    c.add(Element::Resistor { name: "Rl".into(), a: out, b: 0, r: 1e3 });
    let r = transient(&c, &TranSettings { tstop: 1e-6, tstep: 1e-8, uic: false }).unwrap_or_else(|e| panic!("{e:?}"));
    let v = r.signal("V(out)").unwrap();
    assert!((v.values[v.values.len() - 1] - 2.5).abs() < 1e-3, "{}", v.values[v.values.len() - 1]);
}

/// LTspice's gate pins: inputs 1-5, the true output on pin 7 and the inverted one on pin 6, so `A a b 0 0 0 y 0 0 AND` is a
/// NAND and `A a 0 0 0 0 y 0 0 BUF` an inverter; `Ref` is the switching level and an output pin tied to 0 is unused.
#[test]
fn ltspice_gate_outputs_on_pin_six_are_inverted() {
    let t = ".subckt G a b y1 y2 gnd
A1 a b 0 0 0 y1 0 0 AND Ref=0.5 Vhigh=1 Vlow=0
A2 a 0 0 0 0 0 y2 0 BUF Ref=0.5 Vhigh=1 Vlow=0
A3 a 0 0 0 0 0 0 0 BUF
.ends G";
    let run = |va: f64, vb: f64| {
        let mut c = Circuit::new();
        let (a, b, y1, y2) = (c.node("a"), c.node("b"), c.node("y1"), c.node("y2"));
        c.add(Element::vdc("Va", a, 0, va));
        c.add(Element::vdc("Vb", b, 0, vb));
        add_subckt_instance(&mut c, "U1", "G", t, &[a, b, y1, y2, 0]).unwrap_or_else(|e| panic!("{e}"));
        c.add(Element::Resistor { name: "R1".into(), a: y1, b: 0, r: 10e3 });
        c.add(Element::Resistor { name: "R2".into(), a: y2, b: 0, r: 10e3 });
        let r = dc_op(&c).unwrap();
        (r.voltage("y1").unwrap(), r.voltage("y2").unwrap())
    };
    let (y1, y2) = run(1.0, 1.0);
    assert!(y1 < 0.02 && y2 > 0.98, "NAND(1,1) {y1}, BUF {y2}");
    let (y1, _) = run(1.0, 0.0);
    assert!(y1 > 0.98, "NAND(1,0) {y1}");
}

#[test]
fn pspice_dff_preset_and_clear_are_active_low() {
    // preb comes from the port `a` (driven by the test), clrb is the constant $D_HI (inactive)
    let t = ".subckt T a q gnd\nU1 dff(1) $G_DPWR $G_DGND a $D_HI clk d q qb D_DFF IO_STD\nR1 q gnd 10k\n.ends T";
    assert!((run(t, "T", &["a", "q", "gnd"], 0.0, "q") - 5.0).abs() < 0.1, "preset low sets Q");
    assert!(run(t, "T", &["a", "q", "gnd"], 5.0, "q").abs() < 0.1, "preset high leaves the initial 0");
    let t = ".subckt T a q gnd\nU1 dff(1) $G_DPWR $G_DGND $D_HI a clk d q qb D_DFF IO_STD\nR1 q gnd 10k\n.ends T";
    assert!(run(t, "T", &["a", "q", "gnd"], 0.0, "q").abs() < 0.1, "clear low resets Q");
}

#[test]
fn pspice_jkff_and_latch_are_read() {
    let t = ".subckt T a q gnd\nU1 jkff(1) $G_DPWR $G_DGND a $D_HI clk j k q qb D_JK IO_STD\nR1 q gnd 10k\n.ends T";
    assert!((run(t, "T", &["a", "q", "gnd"], 0.0, "q") - 5.0).abs() < 0.1);
    let t = ".subckt T a q gnd\nU1 dltch(1) $G_DPWR $G_DGND a $D_HI g d q qb D_LATCH IO_STD\nR1 q gnd 10k\n.ends T";
    assert!((run(t, "T", &["a", "q", "gnd"], 0.0, "q") - 5.0).abs() < 0.1);
}

#[test]
fn a_transmission_line_passes_dc_and_delays_a_step() {
    let t = ".subckt T a b gnd\nT1 a gnd b gnd Z0=50 TD=10n\nR1 b gnd 50\n.ends T";
    // DC: the line is a wire, matched load: half of the source through a 50 ohm source resistor is not used here, so b = a
    assert!((run(t, "T", &["a", "b", "gnd"], 1.0, "b") - 1.0).abs() < 1e-6);
    assert!(err(".subckt T a b gnd\nT1 a gnd b gnd Z0=50\n.ends T", "T", &["a", "b", "gnd"]).contains("遅れ時間"));
}

#[test]
fn a_matched_transmission_line_delays_a_step_by_td() {
    let t = ".subckt T a b gnd\nT1 a gnd b gnd Z0=50 TD=20n\n.ends T";
    let mut c = Circuit::new();
    let (src, a, b) = (c.node("src"), c.node("a"), c.node("b"));
    c.add(Element::VSource { name: "Vs".into(), p: src, n: 0, wave: Waveform::Pulse { v1: 0.0, v2: 1.0, td: 5e-9, tr: 1e-9, tf: 1e-9, pw: 1.0, per: 2.0 }, ac: None });
    c.add(Element::Resistor { name: "Rs".into(), a: src, b: a, r: 50.0 });
    c.add(Element::Resistor { name: "Rl".into(), a: b, b: 0, r: 50.0 });
    add_subckt_instance(&mut c, "U1", "T", t, &[a, b, 0]).unwrap_or_else(|e| panic!("{e}"));
    let r = transient(&c, &TranSettings { tstop: 80e-9, tstep: 0.5e-9, uic: false }).unwrap();
    let vb = r.signal("V(b)").unwrap();
    let at = |tt: f64| vb.values[r.time.iter().position(|x| *x >= tt).unwrap()];
    assert!(at(20e-9).abs() < 0.05, "before the delay: {}", at(20e-9));
    assert!((at(70e-9) - 0.5).abs() < 0.05, "after the delay: {}", at(70e-9));
    // the front crosses half height about TD after the source's: 5 ns + 20 ns
    let cross = r.time.iter().zip(&vb.values).find(|(_, v)| **v > 0.25).map(|(t, _)| *t).unwrap();
    assert!((cross - 25.5e-9).abs() < 6e-9, "front at {cross}");
}

#[test]
fn a_charge_type_capacitor_is_open_in_dc_and_charges_in_time() {
    // Q = 1n * v: a plain 1 nF capacitor, written as a charge. RC = 1 us.
    let t = ".subckt T a o gnd\nR1 a o 1k\nC1 o gnd Q=1n*v(o,gnd)\n.ends T";
    let mut c = Circuit::new();
    let (src, a, o) = (c.node("src"), c.node("a"), c.node("o"));
    c.add(Element::VSource { name: "Vs".into(), p: src, n: 0, wave: Waveform::Pulse { v1: 0.0, v2: 1.0, td: 0.1e-6, tr: 1e-9, tf: 1e-9, pw: 1.0, per: 2.0 }, ac: None });
    c.add(Element::Resistor { name: "Rs".into(), a: src, b: a, r: 1.0 });
    add_subckt_instance(&mut c, "U1", "T", t, &[a, o, 0]).unwrap_or_else(|e| panic!("{e}"));
    assert!(dc_op(&c).unwrap().voltage("o").unwrap().abs() < 1e-9);
    let r = transient(&c, &TranSettings { tstop: 5e-6, tstep: 20e-9, uic: false }).unwrap();
    let vo = r.signal("V(o)").unwrap();
    let at = |tt: f64| vo.values[r.time.iter().position(|x| *x >= tt).unwrap()];
    // 1 - exp(-t/RC) with RC = 1.001 us after the step at 0.1 us
    assert!((at(1.1e-6) - (1.0 - (-1.0f64 / 1.001).exp())).abs() < 0.02, "{}", at(1.1e-6));
    assert!(at(5e-6) > 0.98);
}

#[test]
fn a_nonlinear_charge_capacitor_has_the_small_signal_capacitance_dq_dv() {
    // Q = 1n*v + 1n*v^2 / 2 ... at 1 V bias: C = 1n + 1n*1 = 2 nF; corner = 1/(2 pi 1k 2n)
    let t = ".subckt T a o gnd\nR1 a o 1k\nC1 o gnd Q=1n*v(o,gnd)+0.5n*v(o,gnd)*v(o,gnd)\n.ends T";
    let mut c = Circuit::new();
    let (a, o) = (c.node("a"), c.node("o"));
    c.add(Element::VSource { name: "Vs".into(), p: a, n: 0, wave: Waveform::Dc(1.0), ac: Some((1.0, 0.0)) });
    add_subckt_instance(&mut c, "U1", "T", t, &[a, o, 0]).unwrap_or_else(|e| panic!("{e}"));
    let f = 1.0 / (2.0 * std::f64::consts::PI * 1e3 * 2e-9);
    let r = ac_analysis(&c, &AcSettings { fstart: f, fstop: f * 1.0001, sweep: AcSweep::Linear(2) }).unwrap();
    let m = r.signal("V(o)").unwrap().mag()[0];
    assert!((m - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.01, "{m}");
}

#[test]
fn a_capacitor_value_with_a_micro_sign_is_not_mistaken_for_a_charge() {
    // "1µ" has a multi-byte character where "Q=" would be looked for
    let t = ".subckt T a b gnd\nR1 a b 1k\nC1 b gnd 1µ\n.ends T";
    assert!((run(t, "T", &["a", "b", "gnd"], 1.0, "b") - 1.0).abs() < 1e-6);
}

#[test]
fn an_ltspice_modulator_makes_a_sine_with_the_frequency_set_by_the_fm_pin() {
    // fm = 0.5 V between space = 0 and mark = 100 kHz -> 50 kHz; am = 2 V
    let t = ".subckt T fm am o\nA1 fm am 0 0 0 0 o 0 MODULATOR mark=100k space=0\n.ends T";
    let mut c = Circuit::new();
    let (fm, am, o) = (c.node("fm"), c.node("am"), c.node("o"));
    c.add(Element::vdc("Vfm", fm, 0, 0.5));
    c.add(Element::vdc("Vam", am, 0, 2.0));
    add_subckt_instance(&mut c, "U1", "T", t, &[fm, am, o]).unwrap_or_else(|e| panic!("{e}"));
    c.add(Element::Resistor { name: "Rl".into(), a: o, b: 0, r: 1e3 });
    let r = transient(&c, &TranSettings { tstop: 100e-6, tstep: 0.2e-6, uic: false }).unwrap();
    let vo = r.signal("V(o)").unwrap();
    let rising = vo.values.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
    let hi = vo.values.iter().cloned().fold(f64::MIN, f64::max);
    assert!((rising as i32 - 5).abs() <= 1, "{rising} cycles in 100 us at 50 kHz");
    assert!((hi - 2.0).abs() < 0.05, "amplitude {hi}");
}

#[test]
fn braces_inside_a_param_expression_only_group() {
    let t = ".subckt T a b gnd vcc1=5 sp=2\n.param td=1e3*({vcc1}-1)*{sp}\nR1 a b {td}\nR2 b gnd 8k\n.ends T";
    // td = 1e3 * 4 * 2 = 8k: a divider of 8k / 8k
    assert!((run(t, "T", &["a", "b", "gnd"], 2.0, "b") - 1.0).abs() < 1e-6);
}

#[test]
fn old_pspice_headers_put_their_parameters_in_one_brace_group() {
    let t = ".subckt T a b gnd {Rx=2k Ry=2k}\nR1 a b {Rx}\nR2 b gnd {Ry}\n.ends T";
    assert!((run(t, "T", &["a", "b", "gnd"], 2.0, "b") - 1.0).abs() < 1e-6);
}

#[test]
fn an_x_call_may_pass_its_parameters_in_one_brace_group() {
    let t = ".subckt DIV in out gnd {r=1k k=2}\nR1 in out {r}\nR2 out gnd {r*k}\n.ends\n.subckt T a b gnd\nX1 a b gnd DIV {r=1k k=3}\n.ends T";
    assert!((run(t, "T", &["a", "b", "gnd"], 1.0, "b") - 0.75).abs() < 1e-6);
}

#[test]
fn a_question_mark_default_means_the_caller_must_give_the_parameter() {
    let t = ".subckt T a b gnd {Rx=???}\nR1 a b {Rx}\nR2 b gnd 1k\n.ends T";
    assert!(err(t, "T", &["a", "b", "gnd"]).contains("Rx"));
}

#[test]
fn table_sources_read_tabs_element_names_with_table_in_them_and_table_equals() {
    // points separated by tabs, and an element called E_Table
    let t = ".subckt T a b gnd\nE_Table b 0 TABLE {V(a)}\n+ 0\t0\n+ 2\t1\nRl b gnd 1meg\n.ends T";
    assert!((run(t, "T", &["a", "b", "gnd"], 1.0, "b") - 0.5).abs() < 1e-6);
    // TABLE=(x0,y0,x1,y1) after the control nodes
    let t = ".subckt T a b gnd\nG1 b 0 a 0 TABLE=(-1,-1m,1,1m)\nRl b gnd 1k\n.ends T";
    assert!((run(t, "T", &["a", "b", "gnd"], 0.5, "b").abs() - 0.5).abs() < 1e-6);
}

#[test]
fn a_table_keyword_is_found_as_a_word_and_a_pwl_may_repeat_a_time() {
    // the node name VAKTABLE must not be taken for the keyword
    let t = ".subckt T a b gnd\nE_Vak b 0 TABLE { V(a) } ( (0,0) (2,1) )\nRl b gnd 1meg\n.ends T";
    assert!((run(t, "T", &["a", "b", "gnd"], 1.0, "b") - 0.5).abs() < 1e-6);
    assert!(matches!(crate::waveform::parse_source("pwl(0 0 0 1 1m 1)"), Ok(Waveform::Pwl(p)) if p.len() == 3 && p[1].0 > 0.0));
    assert!(crate::waveform::parse_source("pwl(1m 0 0 1)").is_err());
}

#[test]
fn a_source_may_have_a_series_resistance_rser() {
    // 2 V behind 1k into 1k: 1 V
    let t = ".subckt T a b gnd\nV1 b gnd 2 Rser=1k\nR1 b a 1\n.ends T";
    let mut c = Circuit::new();
    let (a, b) = (c.node("a"), c.node("b"));
    add_subckt_instance(&mut c, "U1", "T", t, &[a, b, 0]).unwrap_or_else(|e| panic!("{e}"));
    c.add(Element::Resistor { name: "Rl".into(), a, b: 0, r: 1e3 });
    let v = dc_op(&c).unwrap().voltage("a").unwrap();
    assert!((v - 1.0).abs() < 2e-3, "{v}");
}

#[test]
fn a_value_expression_may_name_parameters_inside_its_braces() {
    let t = ".subckt T a b gnd\n.param Gn=2\nEx b 0 value = {({Gn} * V(a) +\n+ 1) / 2}\nRl b gnd 1meg\n.ends T";
    // (2 * 1 + 1) / 2 = 1.5
    assert!((run(t, "T", &["a", "b", "gnd"], 1.0, "b") - 1.5).abs() < 1e-6);
}

#[test]
fn a_vswitch_written_with_an_a_name_is_a_switch() {
    let t = ".subckt T a b gnd\nV1 c gnd 5\nAS1 a b c gnd SW1\n.model SW1 VSWITCH(RON=1 ROFF=1e9 VON=4 VOFF=1)\n.ends T";
    // closed (control 5 V > VON): 1 ohm in series with the 1k load
    let mut c = Circuit::new();
    let (a, b) = (c.node("a"), c.node("b"));
    add_subckt_instance(&mut c, "U1", "T", t, &[a, b, 0]).unwrap_or_else(|e| panic!("{e}"));
    c.add(Element::vdc("Va", a, 0, 1.0));
    c.add(Element::Resistor { name: "Rl".into(), a: b, b: 0, r: 1e3 });
    let v = dc_op(&c).unwrap().voltage("b").unwrap();
    assert!((v - 1000.0 / 1001.0).abs() < 1e-3, "{v}");
}

#[test]
fn func_defines_a_function_that_other_lines_can_call() {
    let t = ".func P (F) {1/F}\n.func Twice(a, b) {2*(a+b)}\n.subckt T a b gnd\nE1 b 0 value = {Twice(V(a), P(4))}\nRl b gnd 1meg\n.ends T";
    // 2 * (1 + 0.25) = 2.5
    assert!((run(t, "T", &["a", "b", "gnd"], 1.0, "b") - 2.5).abs() < 1e-6);
}

#[test]
fn idt_and_idtmod_integrate_in_a_transient() {
    // d/dt of the integral of 1000 (V) is 1000 V/s: after 1 ms, 1 V; idtmod wraps it into [0, 0.4)
    let t = ".subckt T o w gnd\nB1 o 0 V=idt(1000, 0)\nB2 w 0 V=idtmod(1000, 0, 0.4)\nR1 o gnd 1meg\nR2 w gnd 1meg\n.ends T";
    let mut c = Circuit::new();
    let (o, w) = (c.node("o"), c.node("w"));
    add_subckt_instance(&mut c, "U1", "T", t, &[o, w, 0]).unwrap_or_else(|e| panic!("{e}"));
    let r = transient(&c, &TranSettings { tstop: 1e-3, tstep: 1e-5, uic: false }).unwrap();
    let last = |name: &str| *r.signal(name).unwrap().values.last().unwrap();
    assert!((last("V(o)") - 1.0).abs() < 0.01, "{}", last("V(o)"));
    assert!((last("V(w)") - 0.2).abs() < 0.01, "{}", last("V(w)"));
}

#[test]
fn ternary_and_inverse_trig_in_expressions() {
    let t = ".subckt T a b gnd\nB1 b 0 V=V(a) > 0.5 ? asin(0.5) + sinh(0) : 7\nR1 b gnd 1meg\n.ends T";
    let want = 0.5f64.asin();
    assert!((run(t, "T", &["a", "b", "gnd"], 1.0, "b") - want).abs() < 1e-9);
    assert!((run(t, "T", &["a", "b", "gnd"], 0.0, "b") - 7.0).abs() < 1e-9);
}

#[test]
fn model_cards_with_spaces_around_equals_and_continuations() {
    let m = parse_models(".MODEL DFMOD D N = 1.82488 IS = 5.01399E-008 RS = 0.00763893\n+ EG = 1.11 XTI = 3\n+ CJO = 4E-010 VJ = 1.43005 M = 0.35 FC = 0.5\n+ TT = 1E-008 TNOM=25\n").unwrap();
    assert!(m.contains_key("dfmod"));
    let m = parse_models(".MODEL DIODE1 D\n+ IS = 9.79E-015\n+ N = 1.101\n+ BV = 18\n+ IBV = 0.005\n+ RS = 0.3363\n").unwrap();
    assert!(m.contains_key("diode1"));
}

#[test]
fn model_text_params_and_negative_coupling_are_read() {
    let m = parse_models(".model DX D(IS=1e-14 mfg=Rohm type=LED)").unwrap();
    assert!(m.contains_key("dx"));
    let t = ".subckt T a b gnd\nL1 a gnd 1m\nL2 b gnd 1m\nK1 L1 L2 -0.5\n.ends T";
    let mut c = Circuit::new();
    let (a, b) = (c.node("a"), c.node("b"));
    add_subckt_instance(&mut c, "U1", "T", t, &[a, b, 0]).unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn exp_source_pwrs_sgn_pulldn_and_multibit_dff() {
    // EXP rises towards v2 with the time constant tau1
    let w = crate::waveform::parse_source("exp(0 1 0 1m 10m 1m)").unwrap();
    assert!((w.eval(1e-3) - (1.0 - (-1.0f64).exp())).abs() < 1e-9);
    assert!(w.eval(5e-3) > 0.99 && w.eval(30e-3) < 0.01);
    // pwrs(x, y) = sgn(x) |x|^y
    let t = ".subckt T a b gnd\nB1 b 0 V=pwrs(V(a), 2) + sgn(V(a))\nR1 b gnd 1meg\n.ends T";
    assert!((run(t, "T", &["a", "b", "gnd"], -2.0, "b") - (-4.0 - 1.0)).abs() < 1e-9);
    // a pull-down on a floating pin
    let t = ".subckt T a b gnd\nR1 a b 1k\nUp pulldn(1) $G_DPWR $G_DGND b\n.ends T";
    assert!((run(t, "T", &["a", "b", "gnd"], 2.0, "b") - 2.0 / 2.0).abs() < 1e-6);
    // dff(2): preb clrb clk d1 d2 q1 q2 qb1 qb2, preset low sets both
    let t = ".subckt T a q1 q2 gnd\nU1 dff(2) $G_DPWR $G_DGND a $D_HI clk d1 d2 q1 q2 qb1 qb2 D_DFF IO_STD\nR1 q1 gnd 10k\nR2 q2 gnd 10k\n.ends T";
    let mut c = Circuit::new();
    let (a, q1, q2) = (c.node("a"), c.node("q1"), c.node("q2"));
    add_subckt_instance(&mut c, "U1", "T", t, &[a, q1, q2, 0]).unwrap_or_else(|e| panic!("{e}"));
    c.add(Element::vdc("Va", a, 0, 0.0));
    let r = dc_op(&c).unwrap();
    assert!((r.voltage("q1").unwrap() - 5.0).abs() < 0.1 && (r.voltage("q2").unwrap() - 5.0).abs() < 0.1);
}

#[test]
fn an_expression_may_read_the_current_of_an_e_source_given_by_value() {
    let t = ".subckt T a b gnd\nE_Ed c 0 value = {V(a)}\nR1 c gnd 1k\nB1 b 0 V=I(E_Ed)*1000\nR2 b gnd 1meg\n.ends T";
    // E_Ed drives 1 V into 1 k: 1 mA out of the + node; SPICE counts the current flowing through the source from + to -
    let v = run(t, "T", &["a", "b", "gnd"], 1.0, "b");
    assert!((v.abs() - 1.0).abs() < 1e-6, "{v}");
}

#[test]
fn a_resistance_given_by_an_expression() {
    let t = ".subckt T a b gnd {Rn=500}\nR1 a b R = {Rn} * 2 + Freq\nR2 b gnd 1k\n.ends T";
    // 1k + 1k divider
    assert!((run(t, "T", &["a", "b", "gnd"], 2.0, "b") - 1.0).abs() < 1e-6);
}

#[test]
fn a_lossy_line_has_a_series_resistance() {
    // 100 ohm per metre over 1 m with L = 250 nH/m and C = 100 pF/m (Z0 = 50): 100 ohm in series at DC
    let t = ".subckt T a b gnd\nO1 a gnd b gnd LOSSY\n.model LOSSY LTRA(R=100 G=0 L=250n C=100p LEN=1)\nRl b gnd 100\n.ends T";
    let v = run(t, "T", &["a", "b", "gnd"], 2.0, "b");
    assert!((v - 1.0).abs() < 1e-3, "{v}");
}

#[test]
fn pwl_plus_times_are_relative_and_load_is_ignored() {
    let w = crate::waveform::parse_source("pwl(0 0 1m 0 +1u 5 +1m 5)").unwrap();
    assert!((w.eval(1e-3 + 1e-6) - 5.0).abs() < 1e-9 && (w.eval(2e-3) - 5.0).abs() < 1e-9);
    // an ordinary positive number that does come later stays as it is
    let w = crate::waveform::parse_source("pwl(0 0 +1m 5)").unwrap();
    assert!((w.eval(1e-3) - 5.0).abs() < 1e-9);
    let t = ".subckt T a b gnd\nI1 gnd b 1m load\nR1 b gnd 1k\n.ends T";
    assert!((run(t, "T", &["a", "b", "gnd"], 0.0, "b") - 1.0).abs() < 1e-6);
}

#[test]
fn an_xspice_pwl_block_and_a_table_with_a_stray_bracket() {
    let t = ".subckt T a b gnd\nAx a b PW\n.model PW PWL2 xy_array=[-1k -1e-14 0 0 10 100]\nRl b gnd 1meg\n.ends T";
    // 10 V input maps to 100 V; 5 V to 50 V
    assert!((run(t, "T", &["a", "b", "gnd"], 5.0, "b") - 50.0).abs() < 1e-6);
    let t = ".subckt T a b gnd\nEx b 0 TABLE {V(a)} (0 0 (1 10)\nRl b gnd 1meg\n.ends T";
    assert!((run(t, "T", &["a", "b", "gnd"], 0.5, "b") - 5.0).abs() < 1e-6);
}

#[test]
fn ddt_is_the_time_derivative() {
    // v(a) rises 1 V in 1 ms (a ramp), so ddt(V(a)) = 1000 V/s
    let t = ".subckt T a o gnd\nB1 o 0 V=ddt(V(a))\nR1 o gnd 1meg\n.ends T";
    let mut c = Circuit::new();
    let (a, o) = (c.node("a"), c.node("o"));
    c.add(Element::VSource { name: "Vr".into(), p: a, n: 0, wave: Waveform::Pwl(vec![(0.0, 0.0), (1e-3, 1.0), (2e-3, 1.0)]), ac: None });
    add_subckt_instance(&mut c, "U1", "T", t, &[a, o, 0]).unwrap_or_else(|e| panic!("{e}"));
    let r = transient(&c, &TranSettings { tstop: 1.5e-3, tstep: 2e-5, uic: false }).unwrap();
    let vo = r.signal("V(o)").unwrap();
    let at = |tt: f64| vo.values[r.time.iter().position(|x| *x >= tt).unwrap()];
    assert!((at(0.5e-3) - 1000.0).abs() < 20.0, "{}", at(0.5e-3));
    assert!(at(1.5e-3).abs() < 20.0, "{}", at(1.5e-3));
}
