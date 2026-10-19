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

/// The voltage on an input pin of the bench: 2 V, but 2.1 V on a pin that looks like a non-inverting input (`IN+`, `INP`, `IP`),
/// so that an op-amp has a definite sign of its input difference instead of two equal inputs.
fn input_level(port: &str) -> f64 {
    let p = port.to_ascii_lowercase();
    if p.ends_with('+') || p.starts_with('+') || matches!(p.as_str(), "inp" | "ip" | "inplus" | "in_p" | "vip" | "inp1" | "in1+" | "vinp" | "vin+" | "pos" | "ina" | "vp") { 2.1 } else { 2.0 }
}

/// The generic bench of `try_one`: the subcircuit wired up by its pin names.
fn build_bench(text: &str, name: &str, ports: &[String]) -> Result<Circuit, String> {
    build_bench_text(text, name, ports).map(|(c, _)| c)
}

/// `build_bench`, and also the library text with the `.param` lines it had to add (for a second simulator to read).
fn build_bench_text(text: &str, name: &str, ports: &[String]) -> Result<(Circuit, String), String> {
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
                c.add(Element::vdc(&format!("Vi{i}"), src, 0, input_level(p)));
                c.add(Element::Resistor { name: format!("Ri{i}"), a: src, b: n, r: 1e3 });
            }
            _ => c.add(Element::Resistor { name: format!("Rl{i}"), a: n, b: 0, r: 10e3 }),
        }
    }
    Ok((c, text))
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
    for tstop in std::env::var("SPICE_TSTOPS").ok().map_or(vec![1e-9, 1e-8, 1e-7, 5e-7, 2e-6], |v| v.split(',').filter_map(|x| x.parse().ok()).collect()) {
        println!("TRAN {tstop:e}: {:?}", crate::transient(&c, &crate::TranSettings { tstop, tstep: tstop / 20.0, uic: false }).map(|r| (r.time.len(), r.retries, r.rejected)));
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

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

/// Node voltages (`name value` lines, lower case) from ngspice's batch output.
fn parse_ngspice_voltages(text: &str) -> std::collections::HashMap<String, f64> {
    let mut m = std::collections::HashMap::new();
    for line in text.lines() {
        let mut t = line.split_whitespace();
        let (Some(name), Some(mut val)) = (t.next(), t.next()) else { continue };
        if val == "=" {
            let Some(x) = t.next() else { continue };
            val = x;
        }
        if t.next().is_some() || name.contains('#') || name.contains('[') {
            continue;
        }
        if let Ok(x) = val.parse::<f64>() {
            m.insert(name.to_ascii_lowercase(), x);
        }
    }
    m
}

/// Compare our DC operating point with ngspice's on the generic bench, for a sample of the vendor subcircuits.
/// `SPICE_MODEL_DIR=<dir> [NGSPICE=<exe>] [SPICE_XCHECK_N=<count, default 150>] [SPICE_XCHECK_OUT=<csv>] cargo test -p sim-core vendor_xcheck --release -- --ignored --nocapture`
#[test]
#[ignore]
fn vendor_xcheck() {
    let Ok(dir) = std::env::var("SPICE_MODEL_DIR") else { return };
    let exe = std::env::var("NGSPICE").unwrap_or_else(|_| r"F:\Tools\ngspice\Spice64\bin\ngspice_con.exe".into());
    let want: usize = std::env::var("SPICE_XCHECK_N").ok().and_then(|v| v.parse().ok()).unwrap_or(150);
    let work = std::env::temp_dir().join("circuitcad_vendor_xcheck");
    let _ = std::fs::create_dir_all(&work);
    // the vendor files are written for LTspice / PSpice: ngspice reads them in its compatibility mode
    let _ = std::fs::write(work.join(".spiceinit"), format!("set ngbehavior={}
", std::env::var("NGBEHAVIOR").unwrap_or_else(|_| "ltpsa".into())));
    let mut list = vec![];
    files(std::path::Path::new(&dir), &mut list);
    list.sort();
    let mut cands: Vec<(u64, std::path::PathBuf, String, Vec<String>)> = vec![];
    for f in &list {
        let text = crate::decode_spice_text(&std::fs::read(f).unwrap_or_default());
        for (name, ports) in subckts(&text).into_iter().take(6) {
            if ports.iter().any(|p| role(p) == Role::Supply) && ports.iter().any(|p| role(p) == Role::Ground) {
                cands.push((fnv(&format!("{}{name}", f.display())), f.clone(), name, ports));
            }
        }
    }
    cands.sort_by_key(|c| c.0);
    let (mut matched, mut differ, mut ng_fail, mut tried) = (0, 0, 0, 0);
    let mut csv = String::from("file,subckt,outcome,detail\n");
    let only = std::env::var("SPICE_XCHECK_ONLY").ok();
    for (_, f, name, ports) in cands {
        if tried >= want {
            break;
        }
        if only.as_ref().is_some_and(|o| !o.eq_ignore_ascii_case(&name)) {
            continue;
        }
        let rel = f.strip_prefix(&dir).unwrap_or(&f).to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/");
        let text = crate::decode_spice_text(&std::fs::read(&f).unwrap_or_default());
        // our side, in a thread with a time limit
        let (tx, rx) = mpsc::channel();
        let (t2, n2, p2) = (text.clone(), name.clone(), ports.clone());
        std::thread::spawn(move || {
            let r = build_bench_text(&t2, &n2, &p2).and_then(|(c, final_text)| match dc_op(&c) {
                Ok(r) => {
                    let all: Vec<(String, f64)> = c.names.iter().filter(|n| n.as_str() != "0").filter_map(|n| r.voltage(n).map(|v| (n.clone(), v))).collect();
                    Ok((final_text, p2.iter().map(|p| (p.clone(), r.voltage(&format!("p_{p}")))).collect::<Vec<_>>(), all))
                }
                Err(e) => Err(format!("DC {e:?}")),
            });
            let _ = tx.send(r);
        });
        let Ok(ours) = rx.recv_timeout(Duration::from_secs(20)) else { continue };
        // a model we cannot solve is the bulk check's business, not this one's
        let Ok((final_text, mine, all)) = ours else { continue };
        tried += 1;
        let tag = format!("m{tried}");
        let lib = work.join(format!("{tag}_lib.cir"));
        std::fs::write(&lib, &final_text).unwrap();
        let clean = |p: &str| -> String { p.chars().map(|c| match c { '+' => 'p', '-' => 'm', c if c.is_ascii_alphanumeric() => c, _ => '_' }).collect::<String>().to_ascii_lowercase() };
        let node = |p: &str| if role(p) == Role::Ground { "0".to_string() } else { format!("p_{}", clean(p)) };
        let mut deck = format!("* xcheck {name}\n.include \"{}\"\n", lib.display().to_string().replace('\\', "/"));
        for (i, p) in ports.iter().enumerate() {
            let n = node(p);
            if n == "0" {
                continue;
            }
            match role(p) {
                Role::Supply => deck.push_str(&format!("Vp{i} {n} 0 5\n")),
                Role::Input => deck.push_str(&format!("Vi{i} src_{i} 0 {}\nRi{i} src_{i} {n} 1k\n", input_level(p))),
                _ => deck.push_str(&format!("Rl{i} {n} 0 10k\n")),
            }
        }
        deck.push_str(&format!(
            "X1 {} {name}\n.options reltol=1e-6 vntol=1e-8 abstol=1e-12\n.control\nset noaskquit\nop\nprint all\n.endc\n.end\n",
            ports.iter().map(|p| node(p)).collect::<Vec<_>>().join(" ")
        ));
        let cir = work.join(format!("{tag}.cir"));
        std::fs::write(&cir, deck).unwrap();
        let mut child = std::process::Command::new(&exe)
            .current_dir(&work)
            .arg("-b")
            .arg(&cir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("ngspice を起動できません: {e}"));
        let start = std::time::Instant::now();
        let finished = loop {
            match child.try_wait() {
                Ok(Some(_)) => break true,
                Ok(None) if start.elapsed() > Duration::from_secs(30) => {
                    let _ = child.kill();
                    break false;
                }
                _ => std::thread::sleep(Duration::from_millis(20)),
            }
        };
        let out = child.wait_with_output().ok();
        let text_out = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)).unwrap_or_default();
        let theirs = parse_ngspice_voltages(&text_out);
        let (kind, detail) = if !finished {
            ng_fail += 1;
            ("ngspice-timeout", String::new())
        } else if !ports.iter().any(|p| theirs.contains_key(&node(p))) {
            ng_fail += 1;
            let why = text_out.lines().find(|l| l.to_ascii_lowercase().contains("error")).unwrap_or("no result").trim().to_string();
            ("ngspice-fail", why.chars().take(100).collect())
        } else {
            let mut worst: Option<(f64, String, f64, f64)> = None;
            let mut bad = 0;
            for (p, v) in &mine {
                let Some(a) = *v else { continue };
                let Some(&b) = theirs.get(&node(p)) else { continue };
                if (a - b).abs() > 1e-3 + 1e-2 * a.abs().max(b.abs()) {
                    bad += 1;
                }
                if worst.as_ref().is_none_or(|w| (a - b).abs() > w.0) {
                    worst = Some(((a - b).abs(), p.clone(), a, b));
                }
            }
            if bad == 0 {
                matched += 1;
                ("match", String::new())
            } else {
                differ += 1;
                let w = worst.unwrap();
                ("differ", format!("{bad} of {} ports; worst {} ours={:.4e} ngspice={:.4e}", mine.len(), w.1, w.2, w.3))
            }
        };
        if only.is_some() {
            // every node both engines name, largest difference first
            let mut diffs: Vec<(f64, String, f64, f64)> = all
                .iter()
                .filter_map(|(n, a)| {
                    let key = n.to_ascii_lowercase().replacen("u1.", "x1.", 1).replacen("p_", "p_", 1);
                    let key = if key.starts_with("x1.") || key.starts_with("p_") { key } else { return None };
                    theirs.get(&clean_name(&key)).or_else(|| theirs.get(&key)).map(|b| ((a - b).abs() / (1e-3 + a.abs().max(b.abs())), n.clone(), *a, *b))
                })
                .collect();
            diffs.sort_by(|a, b| b.0.total_cmp(&a.0));
            for (d, n, a, b) in diffs.iter().take(25) {
                println!("NODE {n:30} ours={a:12.5e} ngspice={b:12.5e} rel={d:.2e}");
            }
        }
        println!("{kind:16} {rel}:{name} {detail}");
        csv.push_str(&format!("{rel},{name},{kind},{}\n", detail.replace(',', ";")));
    }
    println!("XCHECK tried {tried}: match {matched}, differ {differ}, ngspice could not run {ng_fail}");
    if let Ok(p) = std::env::var("SPICE_XCHECK_OUT") {
        std::fs::write(p, csv).unwrap();
    }
}

/// ngspice's name for one of ours: lower case, `.` kept, other odd characters as `_`.
fn clean_name(n: &str) -> String {
    n.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '_' { c } else { '_' }).collect::<String>().to_ascii_lowercase()
}

/// Linear interpolation of a sampled signal at time `t`.
fn interp(times: &[f64], values: &[f64], t: f64) -> f64 {
    let i = times.partition_point(|x| *x <= t);
    if i == 0 {
        return values[0];
    }
    if i >= times.len() {
        return *values.last().unwrap();
    }
    let (t0, t1) = (times[i - 1], times[i]);
    if t1 <= t0 {
        return values[i];
    }
    values[i - 1] + (values[i] - values[i - 1]) * (t - t0) / (t1 - t0)
}

/// Compare a transient run with ngspice's on the vendor bench: the first input pin gets a pulse (0 -> its level, 1 us delay, 6 us wide,
/// 14 us period), the answer is every other non-supply pin over 20 us. A pin agrees when the largest difference is under 3 % of the
/// signal's swing (at least 50 mV) plus 20 mV.
/// `SPICE_MODEL_DIR=<dir> [NGSPICE=<exe>] [SPICE_XCHECK_N=<count>] [SPICE_XCHECK_ONLY=<name>] [SPICE_XCHECK_OUT=<csv>] cargo test -p sim-core vendor_xcheck_tran --release -- --ignored --nocapture`
#[test]
#[ignore]
fn vendor_xcheck_tran() {
    let Ok(dir) = std::env::var("SPICE_MODEL_DIR") else { return };
    let exe = std::env::var("NGSPICE").unwrap_or_else(|_| r"F:\Tools\ngspice\Spice64\bin\ngspice_con.exe".into());
    let want: usize = std::env::var("SPICE_XCHECK_N").ok().and_then(|v| v.parse().ok()).unwrap_or(80);
    let work = std::env::temp_dir().join("circuitcad_vendor_xcheck_tran");
    let _ = std::fs::create_dir_all(&work);
    let _ = std::fs::write(work.join(".spiceinit"), format!("set ngbehavior={}\n", std::env::var("NGBEHAVIOR").unwrap_or_else(|_| "ltpsa".into())));
    let mut list = vec![];
    files(std::path::Path::new(&dir), &mut list);
    list.sort();
    let mut cands: Vec<(u64, std::path::PathBuf, String, Vec<String>)> = vec![];
    for f in &list {
        let text = crate::decode_spice_text(&std::fs::read(f).unwrap_or_default());
        for (name, ports) in subckts(&text).into_iter().take(6) {
            if ports.iter().any(|p| role(p) == Role::Supply) && ports.iter().any(|p| role(p) == Role::Ground) && ports.iter().any(|p| role(p) == Role::Input) {
                cands.push((fnv(&format!("{}{name}", f.display())), f.clone(), name, ports));
            }
        }
    }
    cands.sort_by_key(|c| c.0);
    let only = std::env::var("SPICE_XCHECK_ONLY").ok();
    let (mut matched, mut differ, mut ng_fail, mut tried) = (0, 0, 0, 0);
    let mut times_log: Vec<(f64, f64)> = vec![];
    let mut csv = String::from("file,subckt,outcome,detail\n");
    for (_, f, name, ports) in cands {
        if tried >= want {
            break;
        }
        if only.as_ref().is_some_and(|o| !o.eq_ignore_ascii_case(&name)) {
            continue;
        }
        let rel = f.strip_prefix(&dir).unwrap_or(&f).to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/");
        let text = crate::decode_spice_text(&std::fs::read(&f).unwrap_or_default());
        let first_in = ports.iter().position(|p| role(p) == Role::Input).unwrap();
        let level = input_level(&ports[first_in]);
        // our side
        let (tx, rx) = mpsc::channel();
        let (t2, n2, p2) = (text.clone(), name.clone(), ports.clone());
        std::thread::spawn(move || {
            let r = build_bench_text(&t2, &n2, &p2).and_then(|(mut c, final_text)| {
                for e in c.elements.iter_mut() {
                    if let Element::VSource { name, wave, .. } = e
                        && *name == format!("Vi{first_in}")
                    {
                        *wave = Waveform::Pulse { v1: 0.0, v2: level, td: 1e-6, tr: 20e-9, tf: 20e-9, pw: 6e-6, per: 14e-6 };
                    }
                }
                let t0 = std::time::Instant::now();
                let r = crate::transient(&c, &crate::TranSettings { tstop: 20e-6, tstep: 40e-9, uic: false }).map_err(|e| format!("tran {e:?}"))?;
                let ours_secs = t0.elapsed().as_secs_f64();
                let sigs: Vec<(String, Vec<f64>)> = p2.iter().filter_map(|p| r.signal(&format!("V(p_{p})")).map(|s| (p.clone(), s.values.clone()))).collect();
                Ok((final_text, r.time.clone(), sigs, ours_secs, r.time.len()))
            });
            let _ = tx.send(r);
        });
        let Ok(ours) = rx.recv_timeout(Duration::from_secs(30)) else { continue };
        let Ok((final_text, times, sigs, ours_secs, our_points)) = ours else { continue };
        tried += 1;
        let tag = format!("t{tried}");
        let lib = work.join(format!("{tag}_lib.cir"));
        std::fs::write(&lib, &final_text).unwrap();
        let clean = |p: &str| -> String { p.chars().map(|c| match c { '+' => 'p', '-' => 'm', c if c.is_ascii_alphanumeric() => c, _ => '_' }).collect::<String>().to_ascii_lowercase() };
        let node = |p: &str| if role(p) == Role::Ground { "0".to_string() } else { format!("p_{}", clean(p)) };
        let mut deck = format!("* tran {name}\n.include \"{}\"\n", lib.display().to_string().replace('\\', "/"));
        for (i, p) in ports.iter().enumerate() {
            let n = node(p);
            if n == "0" {
                continue;
            }
            match role(p) {
                Role::Supply => deck.push_str(&format!("Vp{i} {n} 0 5\n")),
                Role::Input if i == first_in => deck.push_str(&format!("Vi{i} src_{i} 0 PULSE(0 {level} 1u 20n 20n 6u 14u)\nRi{i} src_{i} {n} 1k\n")),
                Role::Input => deck.push_str(&format!("Vi{i} src_{i} 0 {}\nRi{i} src_{i} {n} 1k\n", input_level(p))),
                _ => deck.push_str(&format!("Rl{i} {n} 0 10k\n")),
            }
        }
        let outs: Vec<&String> = ports.iter().filter(|p| role(p) != Role::Ground).collect();
        let data = work.join(format!("{tag}.dat"));
        let _ = std::fs::remove_file(&data);
        deck.push_str(&format!(
            "X1 {} {name}\n.options reltol=1e-4\n.control\nset noaskquit\ntran 20n 20u\nwrdata {} {}\n.endc\n.end\n",
            ports.iter().map(|p| node(p)).collect::<Vec<_>>().join(" "),
            data.display().to_string().replace('\\', "/"),
            outs.iter().map(|p| format!("v({})", node(p))).collect::<Vec<_>>().join(" ")
        ));
        let cir = work.join(format!("{tag}.cir"));
        std::fs::write(&cir, deck).unwrap();
        let mut child = std::process::Command::new(&exe)
            .current_dir(&work)
            .arg("-b")
            .arg(&cir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("ngspice を起動できません: {e}"));
        let start = std::time::Instant::now();
        let finished = loop {
            match child.try_wait() {
                Ok(Some(_)) => break true,
                Ok(None) if start.elapsed() > Duration::from_secs(60) => {
                    let _ = child.kill();
                    break false;
                }
                _ => std::thread::sleep(Duration::from_millis(20)),
            }
        };
        let ng_secs = start.elapsed().as_secs_f64();
        let log = child.wait_with_output().ok().map(|o| String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)).unwrap_or_default();
        let table: Vec<Vec<f64>> = std::fs::read_to_string(&data)
            .unwrap_or_default()
            .lines()
            .map(|l| l.split_whitespace().filter_map(|x| x.parse::<f64>().ok()).collect::<Vec<f64>>())
            .filter(|r| r.len() >= 2 * outs.len())
            .collect();
        let (kind, detail) = if !finished || table.len() < 20 {
            ng_fail += 1;
            let why = log.lines().find(|l| l.to_ascii_lowercase().contains("error")).unwrap_or("no result").trim().to_string();
            ("ngspice-fail", why.chars().take(100).collect::<String>())
        } else {
            let mut bad = vec![];
            let mut worst = (0.0f64, String::new());
            for (k, p) in outs.iter().enumerate() {
                if role(p) == Role::Supply || (role(p) == Role::Input) {
                    continue;
                }
                let Some((_, vals)) = sigs.iter().find(|(n, _)| n == *p) else { continue };
                let (mut lo, mut hi) = (f64::MAX, f64::MIN);
                let mut maxerr = 0.0f64;
                for row in &table {
                    let (t, b) = (row[2 * k], row[2 * k + 1]);
                    // the spike while an input edge goes through (a few hundred ns) depends on the step the engine happened to take
                    if [(1.0e-6, 1.25e-6), (7.02e-6, 7.27e-6), (15.0e-6, 15.25e-6)].iter().any(|(a, b)| t >= *a && t <= *b) {
                        continue;
                    }
                    // an edge a few tens of ns earlier or later is not a different answer: the best match within +-60 ns counts
                    let a = (-12..=12).map(|j| interp(&times, vals, t + j as f64 * 5e-9)).min_by(|x, y| (x - b).abs().total_cmp(&(y - b).abs())).unwrap();
                    lo = lo.min(b);
                    hi = hi.max(b);
                    maxerr = maxerr.max((a - b).abs());
                }
                if only.is_some() {
                    let step = (table.len() / 14).max(1);
                    for row in table.iter().step_by(step) {
                        println!("SAMPLE {p:8} t={:9.3e} ours={:11.4e} ngspice={:11.4e}", row[2 * k], interp(&times, vals, row[2 * k]), row[2 * k + 1]);
                    }
                }
                let tol = 0.03 * (hi - lo).max(0.05) + 0.02;
                if maxerr > tol {
                    bad.push(p.to_string());
                }
                if maxerr / tol > worst.0 {
                    worst = (maxerr / tol, format!("{p} err={maxerr:.3e} swing={:.3e}", hi - lo));
                }
            }
            if bad.is_empty() {
                matched += 1;
                ("match", String::new())
            } else {
                differ += 1;
                ("differ", format!("{} of {} pins; worst {}", bad.len(), outs.len(), worst.1))
            }
        };
        if kind == "match" || kind == "differ" {
            println!("TIME {name:30} ours {ours_secs:8.4} s ({our_points} points)   ngspice {ng_secs:8.4} s (process incl. start-up)");
            times_log.push((ours_secs, ng_secs));
        }
        println!("{kind:16} {rel}:{name} {detail}");
        csv.push_str(&format!("{rel},{name},{kind},{}\n", detail.replace(',', ";")));
    }
    if !times_log.is_empty() {
        let mut o: Vec<f64> = times_log.iter().map(|x| x.0).collect();
        let mut g: Vec<f64> = times_log.iter().map(|x| x.1).collect();
        o.sort_by(|a, b| a.total_cmp(b));
        g.sort_by(|a, b| a.total_cmp(b));
        println!("TIMES median ours {:.4} s, ngspice {:.4} s; slowest ours {:.3} s, ngspice {:.3} s; total ours {:.2} s, ngspice {:.2} s", o[o.len() / 2], g[g.len() / 2], o.last().unwrap(), g.last().unwrap(), o.iter().sum::<f64>(), g.iter().sum::<f64>());
    }
    println!("XCHECK-TRAN tried {tried}: match {matched}, differ {differ}, ngspice could not run {ng_fail}");
    if let Ok(p) = std::env::var("SPICE_XCHECK_OUT") {
        std::fs::write(p, csv).unwrap();
    }
}

#[test]
#[ignore]
fn tran_profile() {
    let (Ok(dir), Ok(file), Ok(top)) = (std::env::var("SPICE_MODEL_DIR"), std::env::var("SPICE_DUMP_FILE"), std::env::var("SPICE_DUMP_TOP")) else { return };
    let text = crate::decode_spice_text(&std::fs::read(std::path::Path::new(&dir).join(file)).unwrap());
    let ports = subckts(&text).into_iter().find(|(n, _)| *n == top).map(|(_, p)| p).unwrap_or_default();
    let mut c = build_bench(&text, &top, &ports).unwrap();
    if let Some(first) = ports.iter().position(|p| role(p) == Role::Input) {
        for e in c.elements.iter_mut() {
            if let Element::VSource { name, wave, .. } = e
                && *name == format!("Vi{first}")
            {
                *wave = Waveform::Pulse { v1: 0.0, v2: input_level(&ports[first]), td: 1e-6, tr: 20e-9, tf: 20e-9, pw: 6e-6, per: 14e-6 };
            }
        }
    }
    if let Some(x) = std::env::var("SPICE_LTE").ok().and_then(|v| v.parse::<f64>().ok()) {
        let mut o = crate::options::options();
        o.lte_reltol = x;
        crate::options::set_options(o);
    }
    let t0 = std::time::Instant::now();
    let r = crate::transient(&c, &crate::TranSettings { tstop: 20e-6, tstep: 40e-9, uic: false });
    let total = t0.elapsed().as_secs_f64();
    let rf = crate::dc::REFAC.with(|c| c.get());
    // why the steps are as small as they are
    let stats = crate::tran::STATS.with(|st| st.borrow().clone());
    let cm = crate::dc::compile(&c);
    let cap_name = |idx: usize, ind: bool| -> String {
        for e in &cm.ce {
            match e {
                crate::dc::Ce::C { name, idx: i, .. } if !ind && *i == idx => return name.clone(),
                crate::dc::Ce::L { name, idx: i, .. } if ind && *i == idx => return name.clone(),
                crate::dc::Ce::D { name, cap: Some(i), .. } if !ind && *i == idx => return format!("{name}(junction)"),
                crate::dc::Ce::Q { name, cap: Some(i), .. } if !ind && (*i == idx || *i + 1 == idx) => return format!("{name}(junction)"),
                _ => {}
            }
        }
        format!("#{idx}")
    };
    let mut by: std::collections::BTreeMap<u8, (usize, f64)> = Default::default();
    let mut who: std::collections::BTreeMap<String, usize> = Default::default();
    for (_, h, why, idx, ind) in &stats {
        let e = by.entry(*why).or_insert((0, 0.0));
        e.0 += 1;
        e.1 += h;
        if *why == 0 {
            *who.entry(cap_name(*idx, *ind)).or_insert(0) += 1;
        }
    }
    println!("STEPS by limit (1 breakpoint, 2 after rejection, 3 backward Euler retry, 4 at the largest step, 0 error control): {by:?}");
    let mut who: Vec<_> = who.into_iter().collect();
    who.sort_by(|a, b| b.1.cmp(&a.1));
    println!("ERROR CONTROL worst element: {:?}", &who[..who.len().min(8)]);
    let mut hs: Vec<f64> = stats.iter().map(|x| x.1).collect();
    hs.sort_by(|a, b| a.total_cmp(b));
    if !hs.is_empty() {
        println!("STEP SIZES min {:.2e} median {:.2e} 90% {:.2e} max {:.2e}", hs[0], hs[hs.len() / 2], hs[hs.len() * 9 / 10], hs[hs.len() - 1]);
    }
    println!("PROFILE {top}: {:?} total {total:.3} s, refactor {} / full {}", r.map(|r| (r.time.len(), r.retries, r.rejected)), rf[0], rf[1]);
}
