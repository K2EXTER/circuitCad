//! Bulk compatibility check with a folder of real vendor SPICE files (not part of the repository):
//! `SPICE_MODEL_DIR=<dir> [SPICE_BULK_OUT=<csv>] cargo test -p sim-core vendor_bulk -- --ignored --nocapture`.
//! Every `.subckt` of every file is read, wired into a generic bench (supply pins to 5 V, ground pins to 0,
//! input-like pins to 2 V through 1 k, the rest to ground through 10 k) and solved for its operating point.

use crate::*;
use std::sync::mpsc;
use std::time::Duration;

fn files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, out);
        } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| ["lib", "sub", "mod", "cir", "spi", "txt", "fam"].contains(&x.to_ascii_lowercase().as_str())) {
            out.push(p);
        }
    }
}

/// (name, ports) of every `.subckt` in the text.
fn subckts(text: &str) -> Vec<(String, Vec<String>)> {
    let mut v = vec![];
    for l in crate::spice_in::logical_lines(text) {
        let mut it = l.split_whitespace();
        if it.next().is_some_and(|w| w.eq_ignore_ascii_case(".subckt")) {
            let name = it.next().unwrap_or("").to_string();
            let ports: Vec<String> = it.take_while(|w| !w.contains('=') && !w.eq_ignore_ascii_case("params:") && !w.eq_ignore_ascii_case("optional:")).map(str::to_string).collect();
            if !name.is_empty() && !ports.is_empty() {
                v.push((name, ports));
            }
        }
    }
    v
}

#[derive(Clone, Copy, PartialEq)]
enum Role {
    Ground,
    Supply,
    Input,
    Other,
}

fn role(port: &str) -> Role {
    let p = port.to_ascii_lowercase();
    let is = |names: &[&str]| names.iter().any(|n| p == *n);
    if is(&["0", "gnd", "agnd", "dgnd", "vss", "vee", "v-", "vminus", "-v", "vs-", "vneg", "vn", "sub", "gnd1", "gnd2", "pgnd", "com", "vcom", "vm", "v_", "vsm", "vssa", "vsub"]) {
        return Role::Ground;
    }
    if is(&["vcc", "vdd", "v+", "vplus", "+v", "vs+", "vs", "vpos", "vp", "vin", "vccp", "vcc1", "vdd1", "v_plus", "vsp", "vccq", "vdda", "avcc", "vcca", "vbat", "vi", "vsup", "vpp"]) {
        return Role::Supply;
    }
    if p.starts_with("in") || p.starts_with('+') || p.starts_with('-') || p.starts_with("en") || p.starts_with("sd") || p == "a" || p == "b" || p.starts_with("ctrl") || p.starts_with("clk") || p.starts_with("sel") || p.starts_with("d") && p.len() <= 2 {
        return Role::Input;
    }
    Role::Other
}

enum Outcome {
    Ok,
    Read(String),
    Dc(String),
    /// Read fine, but the pin names say nothing about how to power it, so the DC result would not mean anything.
    Unjudged,
    /// DC was fine, the short transient run (`SPICE_BULK_TRAN=1`) was not.
    Tran(String),
}

/// A plausible value for a parameter the caller would give: supplies 5, resistances 1k, capacitances 1n, times 1n, else 1.
fn guess_param(name: &str) -> &'static str {
    let n = name.to_ascii_lowercase();
    if n.starts_with('v') && n != "value" {
        "5"
    } else if n.starts_with('r') || n.starts_with("dcr") || n.starts_with("zin") || n.starts_with("rf") {
        "1k"
    } else if n.starts_with('c') {
        "1n"
    } else if n.starts_with('t') {
        "1n"
    } else {
        "1"
    }
}

/// The generic bench of `try_one`: the subcircuit wired up by its pin names.
fn build_bench(text: &str, name: &str, ports: &[String]) -> Result<Circuit, String> {
    // a parameter the caller is meant to define (`vcc1={vcc}` with `.param vcc=5` in the top-level netlist): when the read
    // fails for want of one, define it with a plausible value and try again
    let mut text = text.to_string();
    let mut tries = 0;
    let (mut c, nodes) = loop {
        let mut c = Circuit::new();
        let nodes: Vec<usize> = ports.iter().map(|p| if role(p) == Role::Ground { 0 } else { c.node(&format!("p_{p}")) }).collect();
        match crate::spice_in::add_subckt_instance(&mut c, "U1", name, &text, &nodes) {
            Ok(()) => break (c, nodes),
            Err(e) => {
                let missing = e
                    .split_once("パラメータ「")
                    .and_then(|(_, r)| r.split_once('」'))
                    // an expression that uses a name nobody defined (`{SENS}` inside a `VALUE={...}`)
                    .or_else(|| e.split_once("」は使えません。電圧は").and_then(|(l, _)| l.rsplit_once('「')).map(|(_, n)| (n, "")))
                    .map(|(n, _)| n.to_string());
                tries += 1;
                match missing {
                    Some(n) if tries <= 12 => text = format!(".param {n}={}
{text}", guess_param(&n)),
                    _ => {
                        let e: String = e.chars().take(std::env::var("SPICE_BULK_LONG").ok().and_then(|v| v.parse().ok()).unwrap_or(90)).collect();
                        return Err(e);
                    }
                }
            }
        }
    };
    for (i, p) in ports.iter().enumerate() {
        let n = nodes[i];
        if n == 0 {
            continue;
        }
        match role(p) {
            Role::Supply => c.add(Element::vdc(&format!("Vp{i}"), n, 0, 5.0)),
            Role::Input => {
                let src = c.node(&format!("src_{i}"));
                c.add(Element::vdc(&format!("Vi{i}"), src, 0, 2.0));
                c.add(Element::Resistor { name: format!("Ri{i}"), a: src, b: n, r: 1e3 });
            }
            _ => c.add(Element::Resistor { name: format!("Rl{i}"), a: n, b: 0, r: 10e3 }),
        }
    }
    Ok(c)
}

fn try_one(text: String, name: String, ports: Vec<String>) -> Outcome {
    let c = match build_bench(&text, &name, &ports) {
        Ok(c) => c,
        Err(e) => return Outcome::Read(e),
    };
    let known = ports.iter().any(|p| role(p) == Role::Supply) && ports.iter().any(|p| role(p) == Role::Ground);
    match dc_op(&c) {
        Ok(_) if known && std::env::var("SPICE_BULK_TRAN").is_ok() => match crate::transient(&c, &crate::TranSettings { tstop: 2e-6, tstep: 1e-7, uic: false }) {
            Ok(_) => Outcome::Ok,
            Err(e) => Outcome::Tran(format!("{e:?}")),
        },
        Ok(_) => Outcome::Ok,
        Err(_) if !known => Outcome::Unjudged,
        Err(e) => Outcome::Dc(format!("{e:?}")),
    }
}

#[test]
#[ignore]
fn vendor_bulk() {
    let Ok(dir) = std::env::var("SPICE_MODEL_DIR") else { return };
    let mut list = vec![];
    files(std::path::Path::new(&dir), &mut list);
    list.sort();
    let (mut ok, mut read, mut dc, mut timeout, mut total) = (0, 0, 0, 0, 0);
    let mut buckets: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    let mut csv = String::from("file,subckt,ports,outcome,detail\n");
    for f in &list {
        let text = crate::decode_spice_text(&std::fs::read(f).unwrap_or_default());
        let rel = f.strip_prefix(&dir).unwrap_or(f).to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/");
        for (name, ports) in subckts(&text).into_iter().take(6) {
            total += 1;
            let (tx, rx) = mpsc::channel();
            let (t2, n2, p2) = (text.clone(), name.clone(), ports.clone());
            std::thread::spawn(move || {
                let _ = tx.send(try_one(t2, n2, p2));
            });
            let (kind, detail) = match rx.recv_timeout(Duration::from_secs(20)) {
                Ok(Outcome::Ok) => {
                    ok += 1;
                    ("ok", String::new())
                }
                Ok(Outcome::Read(e)) => {
                    read += 1;
                    ("read", e)
                }
                Ok(Outcome::Dc(e)) => {
                    dc += 1;
                    ("dc", e)
                }
                Ok(Outcome::Tran(e)) => {
                    dc += 1;
                    ("tran", e)
                }
                Ok(Outcome::Unjudged) => {
                    ok += 1;
                    ("unjudged", String::new())
                }
                Err(_) => {
                    timeout += 1;
                    ("timeout", String::new())
                }
            };
            if kind != "ok" && kind != "unjudged" {
                buckets.entry(format!("{kind}: {detail}")).or_default().push(format!("{rel}:{name}"));
            }
            csv.push_str(&format!("{rel},{name},{},{kind},{}\n", ports.join(" "), detail.replace(',', ";")));
        }
    }
    println!("TOTAL {total}: ok {ok}, read errors {read}, dc failures {dc}, timeouts {timeout}");
    let mut b: Vec<_> = buckets.into_iter().collect();
    b.sort_by_key(|(_, v)| std::cmp::Reverse(v.len()));
    for (k, v) in b.iter().take(40) {
        println!("BUCKET {:4} {k}   e.g. {}", v.len(), v.iter().take(2).cloned().collect::<Vec<_>>().join(" | "));
    }
    if let Ok(p) = std::env::var("SPICE_BULK_OUT") {
        let _ = std::fs::write(p, csv);
    }
}

/// Which single line of subcircuit `SPICE_DUMP_TOP` in `SPICE_DUMP_FILE` (a path under `SPICE_MODEL_DIR`) makes the read fail:
/// the lines whose removal lets it through are printed with the error.
#[test]
#[ignore]
fn bisect_read() {
    let (Ok(dir), Ok(file), Ok(top)) = (std::env::var("SPICE_MODEL_DIR"), std::env::var("SPICE_DUMP_FILE"), std::env::var("SPICE_DUMP_TOP")) else { return };
    let text = String::from_utf8_lossy(&std::fs::read(std::path::Path::new(&dir).join(file)).unwrap()).to_string();
    let ports = subckts(&text).into_iter().find(|(n, _)| *n == top).map(|(_, p)| p).unwrap_or_default();
    let read = |t: &str| {
        let mut c = Circuit::new();
        let nodes: Vec<usize> = ports.iter().map(|p| c.node(&format!("p_{p}"))).collect();
        crate::spice_in::add_subckt_instance(&mut c, "U1", &top, t, &nodes)
    };
    println!("FULL: {:?}", read(&text));
    let lines: Vec<&str> = text.lines().collect();
    for i in 0..lines.len() {
        let t: String = lines.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, l)| *l).collect::<Vec<_>>().join("\n");
        if read(&t).is_ok() {
            println!("REMOVING line {} lets it read: {}", i + 1, lines[i]);
        }
    }
}

/// Run the transient of one model with the generic bench and say what fails:
/// `SPICE_MODEL_DIR=<dir> SPICE_DUMP_FILE=<file> SPICE_DUMP_TOP=<name> cargo test -p sim-core tran_debug -- --ignored --nocapture`
#[test]
#[ignore]
fn tran_debug() {
    let (Ok(dir), Ok(file), Ok(top)) = (std::env::var("SPICE_MODEL_DIR"), std::env::var("SPICE_DUMP_FILE"), std::env::var("SPICE_DUMP_TOP")) else { return };
    let text = crate::decode_spice_text(&std::fs::read(std::path::Path::new(&dir).join(file)).unwrap());
    let ports = subckts(&text).into_iter().find(|(n, _)| *n == top).map(|(_, p)| p).unwrap_or_default();
    println!("PORTS {ports:?} roles {:?}", ports.iter().map(|p| match role(p) { Role::Ground => "gnd", Role::Supply => "sup", Role::Input => "in", Role::Other => "other" }).collect::<Vec<_>>());
    let c = build_bench(&text, &top, &ports).unwrap();
    println!("elements {} nodes {}", c.elements.len(), c.names.len());
    println!("DC {:?}", dc_op(&c).map(|r| r.strategy));
    if let Ok(idx) = std::env::var("SPICE_UNKNOWNS") {
        let cm = crate::dc::compile(&c);
        for i in idx.split(',').filter_map(|x| x.trim().parse::<usize>().ok()) {
            if i < cm.n_unknowns && i < c.names.len() - 1 && i < cm.node_names.len() - 1 {
                println!("UNKNOWN {i}: node {}", cm.node_names[i + 1]);
            }
            for e in &cm.ce {
                let br = match e {
                    crate::dc::Ce::V { br, .. } | crate::dc::Ce::L { br, .. } | crate::dc::Ce::E { br, .. } | crate::dc::Ce::B { br, .. } => Some(*br),
                    _ => None,
                };
                if br == Some(i) {
                    let name = match e {
                        crate::dc::Ce::V { name, .. } | crate::dc::Ce::L { name, .. } | crate::dc::Ce::E { name, .. } | crate::dc::Ce::B { name, .. } => name.clone(),
                        _ => String::new(),
                    };
                    println!("UNKNOWN {i}: branch of {name}");
                }
            }
        }
    }
    for tstop in [1e-9, 1e-8, 1e-7, 5e-7, 2e-6] {
        println!("TRAN {tstop:e}: {:?}", crate::transient(&c, &crate::TranSettings { tstop, tstep: tstop / 20.0, uic: false }).map(|r| r.time.len()));
    }
}

/// Which DC strategies solve one subcircuit of the bench. Temporary debugging aid:
/// `SPICE_MODEL_DIR=<dir> SPICE_DUMP_FILE=<file> SPICE_DUMP_TOP=<name> [SPICE_VIN=<volts>] cargo test -p sim-core strategy_debug -- --ignored --nocapture`
#[test]
#[ignore]
fn strategy_debug() {
    let (Ok(dir), Ok(file), Ok(top)) = (std::env::var("SPICE_MODEL_DIR"), std::env::var("SPICE_DUMP_FILE"), std::env::var("SPICE_DUMP_TOP")) else { return };
    let text = crate::decode_spice_text(&std::fs::read(std::path::Path::new(&dir).join(file)).unwrap());
    let ports = subckts(&text).into_iter().find(|(n, _)| *n == top).map(|(_, p)| p).unwrap_or_default();
    let mut c = build_bench(&text, &top, &ports).unwrap();
    if let Some(v) = std::env::var("SPICE_VIN").ok().and_then(|v| v.parse::<f64>().ok()) {
        for e in c.elements.iter_mut() {
            if let Element::VSource { name, wave, .. } = e
                && name.starts_with("Vi")
            {
                *wave = Waveform::Dc(v);
            }
        }
    }
    if let Some(n) = std::env::var("SPICE_ITERS").ok().and_then(|v| v.parse::<usize>().ok()) {
        let mut o = crate::options::options();
        o.max_iter = n;
        crate::options::set_options(o);
    }
    use crate::dc::Strategy::*;
    for (label, s) in [("direct", Direct), ("gmin", Gmin), ("source", Source), ("widen", Widen)] {
        let r = crate::dc::dc_op_with(&c, &[s]);
        println!("{label}: {:?}", r.as_ref().map(|_| ()).map_err(|e| format!("{e:?}")));
    }
}

/// Which single line of the subcircuit, when removed, lets the DC converge (`SPICE_DUMP_*` as in `strategy_debug`).
#[test]
#[ignore]
fn dc_bisect() {
    let (Ok(dir), Ok(file), Ok(top)) = (std::env::var("SPICE_MODEL_DIR"), std::env::var("SPICE_DUMP_FILE"), std::env::var("SPICE_DUMP_TOP")) else { return };
    let text = crate::decode_spice_text(&std::fs::read(std::path::Path::new(&dir).join(file)).unwrap());
    let ports = subckts(&text).into_iter().find(|(n, _)| *n == top).map(|(_, p)| p).unwrap_or_default();
    let lines = crate::spice_in::logical_lines(&text);
    let start = lines.iter().position(|l| l.to_ascii_lowercase().starts_with(".subckt") && l.split_whitespace().nth(1).is_some_and(|n| n.eq_ignore_ascii_case(&top))).unwrap();
    let end = start + lines[start..].iter().position(|l| l.to_ascii_lowercase().starts_with(".end")).unwrap();
    for i in start + 1..end {
        if lines[i].starts_with('.') {
            continue;
        }
        let kept: Vec<&str> = lines.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, l)| l.as_str()).collect();
        let t = kept.join("\n");
        if let Ok(c) = build_bench(&t, &top, &ports)
            && dc_op(&c).is_ok()
        {
            println!("REMOVING makes DC converge: {}", lines[i]);
        }
    }
}
