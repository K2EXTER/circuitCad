//! Digital cells (`Y` lines, our own element) for ngspice: each becomes XSPICE digital code models between bridges.
//!
//! Our cell reads its inputs as a fraction of its own supply (`vss`..`vcc`) against the threshold `vt`, and drives its output
//! to `vss` or `vcc` through `rout`. In ngspice: a `B` source scales each input to that fraction, an `adc_bridge` at `vt`
//! turns it into a digital level, the XSPICE model (`d_and`, `d_dff`, `d_lut`, ...) does the logic with the same delays, a
//! `dac_bridge` gives 0 or 1, and a `B` source scales that back to the supply and drives the output through `rout` (a current,
//! so an output enable or an open drain can let go of the node). The timing checks (`tsu` `th` `tw`) and the enable delays
//! (`ten` `tdis`) are ours alone and are left out.

/// One `Y` line as XSPICE lines (in our `B` dialect: `convert_text` turns the expressions into ngspice's afterwards), or
/// `None` when it is not a `Y` line or cannot be read (the line is then left as it is, and ngspice refuses it).
pub(crate) fn cell_to_xspice(line: &str) -> Option<Vec<String>> {
    let t: Vec<&str> = line.split_whitespace().collect();
    if t.len() < 11 || !t[0].starts_with(['Y', 'y']) {
        return None;
    }
    let name = t[0].to_ascii_lowercase();
    let kind = t[1].to_ascii_lowercase();
    let (out, outb, vss, vcc) = (t[2], t[3], t[4], t[5]);
    let rout = t[6];
    let vt = crate::ngspice::spice_number(t[7])?;
    let vh = crate::ngspice::spice_number(t[8])?;
    let (set_low, reset_low) = (t[9] == "1", t[10] == "1");
    let (mut tplh, mut tphl) = (0.0, 0.0);
    let (mut oe, mut oe_low, mut od) = (None, false, false);
    let (mut outs, mut table): (Vec<&str>, Option<Vec<u64>>) = (vec![], None);
    let mut ins: Vec<&str> = vec![];
    for w in &t[11..] {
        match w.split_once('=') {
            Some((k, v)) => match k.to_ascii_lowercase().as_str() {
                "td" => {
                    tplh = crate::ngspice::spice_number(v)?;
                    tphl = tplh;
                }
                "tplh" => tplh = crate::ngspice::spice_number(v)?,
                "tphl" => tphl = crate::ngspice::spice_number(v)?,
                "oe" => oe = Some(v),
                "oel" => oe_low = v == "1",
                "od" => od = v == "1",
                "outs" => outs = v.split(',').collect(),
                "table" => table = Some(v.split(',').map(|h| u64::from_str_radix(h.trim_start_matches("0x"), 16).ok()).collect::<Option<Vec<_>>>()?),
                _ => {}
            },
            None => ins.push(w),
        }
    }
    let p = name.as_str();
    let mut l: Vec<String> = vec![format!("* {line}")];
    // XSPICE delays must be above zero
    let (rise, fall) = (tplh.max(1e-12), tphl.max(1e-12));
    let span = format!("max(V({vcc})-V({vss}),1m)");
    // an input as a digital node: scaled to the supply, then bridged at `at` (a fraction of the supply)
    let mut bridged = 0;
    let mut bridge = |l: &mut Vec<String>, node: &str, at: f64| -> String {
        bridged += 1;
        let (a, d) = (format!("{p}_a{bridged}"), format!("{p}_d{bridged}"));
        l.push(format!("B{p}_a{bridged} {a} 0 V=(V({node})-V({vss}))/{span}"));
        l.push(format!("A{p}_b{bridged} [{a}] [{d}] {p}_b{bridged}"));
        l.push(format!(".model {p}_b{bridged} adc_bridge(in_low={at:e} in_high={at:e})"));
        d
    };
    // a set / reset input: none when it is tied low and active high, else the bridged node (turned round when active low)
    let control = |l: &mut Vec<String>, node: Option<&&str>, low: bool, tag: &str, bridge: &mut dyn FnMut(&mut Vec<String>, &str, f64) -> String| -> String {
        match node {
            None => "null".to_string(),
            Some(n) if *n == "0" && !low => "null".to_string(),
            Some(n) => {
                let d = bridge(l, n, vt);
                if !low {
                    return d;
                }
                let inv = format!("{p}_{tag}n");
                l.push(format!("A{p}_{tag}i {d} {inv} {p}_{tag}i"));
                l.push(format!(".model {p}_{tag}i d_inverter(rise_delay=1e-12 fall_delay=1e-12)"));
                inv
            }
        }
    };
    let one = |l: &mut Vec<String>, bridge: &mut dyn FnMut(&mut Vec<String>, &str, f64) -> String| bridge(l, vcc, 0.5);
    let (q, qb) = (format!("{p}_q"), format!("{p}_qb"));
    let gate = || format!("rise_delay={rise:e} fall_delay={fall:e}");
    let mut lut_outs: Vec<(String, String)> = vec![];
    match kind.as_str() {
        "and" | "nand" | "or" | "nor" | "xor" | "xnor" => {
            let d: Vec<String> = ins.iter().map(|n| bridge(&mut l, n, vt)).collect();
            l.push(format!("A{p} [{}] {q} {p}_m", d.join(" ")));
            l.push(format!(".model {p}_m d_{kind}({})", gate()));
        }
        "buf" | "inv" => {
            let d = bridge(&mut l, ins.first()?, vt);
            l.push(format!("A{p} {d} {q} {p}_m"));
            l.push(format!(".model {p}_m d_{}({})", if kind == "buf" { "buffer" } else { "inverter" }, gate()));
        }
        "dff" | "tff" | "dlatch" => {
            let d = bridge(&mut l, ins.first()?, vt);
            let c = bridge(&mut l, ins.get(1)?, vt);
            let s = control(&mut l, ins.get(2), set_low, "s", &mut bridge);
            let r = control(&mut l, ins.get(3), reset_low, "r", &mut bridge);
            l.push(format!("A{p} {d} {c} {s} {r} {q} {qb} {p}_m"));
            let delay = if kind == "dlatch" { format!("data_delay={rise:e} enable_delay={rise:e}") } else { format!("clk_delay={rise:e}") };
            l.push(format!(".model {p}_m d_{kind}({delay} set_delay={rise:e} reset_delay={rise:e})"));
        }
        "jkff" | "srff" => {
            let a = bridge(&mut l, ins.first()?, vt);
            let b = bridge(&mut l, ins.get(1)?, vt);
            let s = control(&mut l, ins.get(3), set_low, "s", &mut bridge);
            let r = control(&mut l, ins.get(4), reset_low, "r", &mut bridge);
            if kind == "srff" && ins.get(2) == Some(&"0") {
                // an SR flip-flop with its clock tied low follows S and R at once: a latch that is always enabled
                let e = one(&mut l, &mut bridge);
                l.push(format!("A{p} {a} {b} {e} {s} {r} {q} {qb} {p}_m"));
                l.push(format!(".model {p}_m d_srlatch(sr_delay={rise:e} enable_delay={rise:e} set_delay={rise:e} reset_delay={rise:e})"));
            } else {
                let c = bridge(&mut l, ins.get(2)?, vt);
                l.push(format!("A{p} {a} {b} {c} {s} {r} {q} {qb} {p}_m"));
                l.push(format!(".model {p}_m d_{kind}(clk_delay={rise:e} set_delay={rise:e} reset_delay={rise:e})"));
            }
        }
        "srlatch" => {
            let a = bridge(&mut l, ins.first()?, vt);
            let b = bridge(&mut l, ins.get(1)?, vt);
            let e = match ins.get(2) {
                Some(&"0") | None => one(&mut l, &mut bridge),
                Some(n) => bridge(&mut l, n, vt),
            };
            let s = control(&mut l, ins.get(3), set_low, "s", &mut bridge);
            let r = control(&mut l, ins.get(4), reset_low, "r", &mut bridge);
            l.push(format!("A{p} {a} {b} {e} {s} {r} {q} {qb} {p}_m"));
            l.push(format!(".model {p}_m d_srlatch(sr_delay={rise:e} enable_delay={rise:e} set_delay={rise:e} reset_delay={rise:e})"));
        }
        "schmitt" => {
            // high above vt + vh, low below vt - vh, kept in between: a latch set by one bridge and reset by the other
            let n = ins.first()?;
            let s = bridge(&mut l, n, vt + vh);
            let above = bridge(&mut l, n, vt - vh);
            let r = format!("{p}_rn");
            l.push(format!("A{p}_ri {above} {r} {p}_ri"));
            l.push(format!(".model {p}_ri d_inverter(rise_delay=1e-12 fall_delay=1e-12)"));
            let e = one(&mut l, &mut bridge);
            l.push(format!("A{p} {s} {r} {e} null null {q} {qb} {p}_m"));
            l.push(format!(".model {p}_m d_srlatch(sr_delay={rise:e} enable_delay={rise:e} set_delay={rise:e} reset_delay={rise:e})"));
        }
        "lut" | "rom" => {
            let words = table?;
            let d: Vec<String> = ins.iter().map(|n| bridge(&mut l, n, vt)).collect();
            let outs: Vec<&str> = if outs.is_empty() { vec![out] } else { outs };
            for (k, o) in outs.iter().enumerate() {
                let bits: String = (0..1usize << d.len()).map(|a| if words.get(a).is_some_and(|w| w >> k & 1 == 1) { '1' } else { '0' }).collect();
                let qk = format!("{p}_q{k}");
                l.push(format!("A{p}_{k} [{}] {qk} {p}_m{k}", d.join(" ")));
                l.push(format!(".model {p}_m{k} d_lut({} table_values=\"{bits}\")", gate()));
                lut_outs.push((qk, o.to_string()));
            }
        }
        _ => return None,
    }
    // the output enable as a factor 0..1 (a narrow ramp around the threshold, like our switches)
    let en = match oe {
        None => String::new(),
        Some(n) => {
            let x = format!("(V({n})-V({vss}))/{span}-{vt:e}");
            if oe_low { format!("*step(-({x}),1m)") } else { format!("*step({x},1m)") }
        }
    };
    let drive = |l: &mut Vec<String>, dq: &str, node: &str, tag: &str| {
        if node == "0" {
            return;
        }
        let a = format!("{p}_{tag}a");
        l.push(format!("A{p}_{tag}o [{dq}] [{a}] {p}_{tag}o"));
        l.push(format!(".model {p}_{tag}o dac_bridge(out_low=0 out_high=1 out_undef=0.5 t_rise=1e-12 t_fall=1e-12)"));
        // the level the output heads for; open drain: only the low side pulls
        let level = format!("V({vss})+V({a})*(V({vcc})-V({vss}))");
        let open = if od { format!("*(1-V({a}))") } else { String::new() };
        l.push(format!("B{p}_{tag}r {node} 0 I=-(({level})-V({node}))/{rout}{open}{en}"));
    };
    if lut_outs.is_empty() {
        drive(&mut l, &q, out, "q");
        if !matches!(kind.as_str(), "and" | "nand" | "or" | "nor" | "xor" | "xnor" | "buf" | "inv") {
            drive(&mut l, &qb, outb, "n");
        }
    } else {
        for (k, (dq, node)) in lut_outs.iter().enumerate() {
            drive(&mut l, dq, node, &format!("q{k}"));
        }
    }
    Some(l)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gate_is_bridged_in_and_out_with_its_delays() {
        let l = cell_to_xspice("Y1 nand y 0 gnd vcc 60 0.5 0 0 0 td=8n a b").unwrap().join("\n");
        assert!(l.contains("Ay1 [y1_d1 y1_d2] y1_q y1_m") && l.contains("d_nand(rise_delay=8e-9 fall_delay=8e-9)"), "{l}");
        assert!(l.contains("By1_a1 y1_a1 0 V=(V(a)-V(gnd))/max(V(vcc)-V(gnd),1m)"), "{l}");
        assert!(l.contains("adc_bridge(in_low=5e-1 in_high=5e-1)"), "{l}");
        assert!(l.contains("By1_qr y 0 I=-((V(gnd)+V(y1_qa)*(V(vcc)-V(gnd)))-V(y))/60"), "{l}");
        assert!(cell_to_xspice("R1 a b 1k").is_none());
    }

    #[test]
    fn a_flip_flop_ties_off_unused_set_and_reset_and_turns_active_low_ones_round() {
        let l = cell_to_xspice("Yf dff q qn gnd vcc 60 0.5 0 0 0 td=14n d clk 0 0").unwrap().join("\n");
        assert!(l.contains("Ayf yf_d1 yf_d2 null null yf_q yf_qb yf_m"), "{l}");
        assert!(l.contains("By") && l.contains("Byf_nr qn 0"), "{l}");
        let l = cell_to_xspice("Yf dff q 0 gnd vcc 60 0.5 0 1 1 d clk pre clr").unwrap().join("\n");
        assert!(l.contains("Ayf_si yf_d3 yf_sn") && l.contains("Ayf_ri yf_d4 yf_rn"), "{l}");
        assert!(!l.contains("Byf_nr"), "no inverted output: {l}");
    }

    #[test]
    fn a_rom_gives_one_table_per_output_bit_with_the_first_input_lowest() {
        // two address lines, words 1, 2, 3, 0: bit 0 is 1,0,1,0 and bit 1 is 0,1,1,0
        let l = cell_to_xspice("Yr rom d0 0 gnd vcc 60 0.5 0 0 0 outs=d0,d1 table=1,2,3,0 a0 a1").unwrap().join("\n");
        assert!(l.contains("table_values=\"1010\"") && l.contains("table_values=\"0110\""), "{l}");
        assert!(l.contains("Byr_q0r d0 0") && l.contains("Byr_q1r d1 0"), "{l}");
    }

    #[test]
    fn an_output_enable_and_an_open_drain_scale_the_drive() {
        let l = cell_to_xspice("Yb buf y 0 gnd vcc 60 0.5 0 0 0 oe=en oel=1 od=1 a").unwrap().join("\n");
        assert!(l.contains("*(1-V(yb_qa))*step(-((V(en)-V(gnd))/max(V(vcc)-V(gnd),1m)-5e-1),1m)"), "{l}");
    }
}
