//! Schematic commands: parts, add, net, list, erc, dc, tran.

use crate::session::{obj, opt, options, Session};
use crate::Res;
use sch_core::{Component, Kind, Label, Pt};
use serde_json::{json, Map, Value};

fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

fn basic_kind(t: &str) -> Option<Kind> {
    Some(match t.to_ascii_uppercase().as_str() {
        "R" | "RESISTOR" => Kind::Resistor,
        "C" | "CAPACITOR" => Kind::Capacitor,
        "L" | "INDUCTOR" => Kind::Inductor,
        "D" | "DIODE" => Kind::Diode,
        "LED" => Kind::Led,
        "V" | "VSOURCE" => Kind::VSource,
        "NPN" => Kind::Npn,
        s if s.starts_with("HEADER") => Kind::Header(s["HEADER".len()..].parse().ok().filter(|n| (1..=40).contains(n))?),
        _ => return None,
    })
}

fn pin_names(c: &Component) -> Vec<(String, String)> {
    match &c.custom {
        Some(sym) => sym.pins.iter().map(|p| (p.number.clone(), p.name.clone())).collect(),
        None => c.kind.pins().into_iter().map(|(n, _)| (n.clone(), n)).collect(),
    }
}

/// Period and duty of a repeating wave: rising crossings of the mid level (at least two periods), and the share of
/// time spent above it between the first and the last crossing. None for a wave that does not repeat.
fn periodicity(t: &[f64], v: &[f64], mn: f64, mx: f64) -> Option<(f64, f64)> {
    let span = mx - mn;
    if span.is_nan() || span <= 1e-9 * (1.0 + mx.abs().max(mn.abs())) {
        return None;
    }
    let mid = (mn + mx) / 2.0;
    let mut rises: Vec<(usize, f64)> = vec![];
    for i in 1..v.len() {
        if v[i - 1] < mid && v[i] >= mid {
            let f = (mid - v[i - 1]) / (v[i] - v[i - 1]);
            rises.push((i, t[i - 1] + f * (t[i] - t[i - 1])));
        }
    }
    if rises.len() < 3 {
        return None;
    }
    let (first, last) = (rises[0], rises[rises.len() - 1]);
    let period = (last.1 - first.1) / (rises.len() - 1) as f64;
    let (mut high, mut total) = (0.0, 0.0);
    for i in first.0..=last.0.min(v.len() - 1) {
        let dt = t[i] - t[i - 1];
        total += dt;
        if v[i] >= mid {
            high += dt;
        }
    }
    Some((period, if total > 0.0 { high / total } else { 0.0 }))
}

/// The simulator's errors are written for the GUI; where the fix differs in ccad, say it.
fn cli_sim_error(e: String) -> String {
    if e.contains("GNDにつながる") {
        format!("{e} (in ccad, name one net GND, for example 'net GND V1.- R1.2'; run 'erc' for the full list)")
    } else {
        e
    }
}

/// A failed DC solve with the simulator's hints (where it was still unsettled, what to check) appended.
/// `ic=NAME=1,NAME=0`: the starting state of flip-flops and latches (the names are those of `dc`'s `memory` answer; a part of a
/// name is enough, and it sets every memory element whose name contains it). Returns how many were set.
fn apply_ic(c: &mut sim_core::Circuit, spec: &str) -> Result<usize, String> {
    let seqs: Vec<String> = c.elements.iter().filter_map(|e| if let sim_core::Element::Seq { name, .. } = e { Some(name.clone()) } else { None }).collect();
    if seqs.is_empty() {
        return Err("ic: this circuit has no flip-flop or latch (a library part with a memory, such as a 74HC74 or 74HC4017)".into());
    }
    let mut n = 0;
    for item in spec.split(',').filter(|s| !s.trim().is_empty()) {
        let (pat, val) = item.rsplit_once('=').ok_or_else(|| format!("ic: '{item}' should be NAME=1 or NAME=0"))?;
        let on = match val.trim() {
            "1" => true,
            "0" => false,
            other => return Err(format!("ic: the state of '{}' must be 0 or 1, not '{other}'", pat.trim())),
        };
        let want = pat.trim().to_ascii_lowercase();
        let exact: Vec<&String> = seqs.iter().filter(|s| s.to_ascii_lowercase() == want).collect();
        let hits: Vec<&String> = if exact.is_empty() { seqs.iter().filter(|s| s.to_ascii_lowercase().contains(&want)).collect() } else { exact };
        if hits.is_empty() {
            let sample: Vec<&str> = seqs.iter().take(12).map(|s| s.as_str()).collect();
            return Err(format!("ic: no flip-flop or latch matches '{}' (they are: {}{})", pat.trim(), sample.join(", "), if seqs.len() > 12 { ", ..." } else { "" }));
        }
        for h in hits {
            c.initial.retain(|(m, _)| m != h);
            c.initial.push((h.clone(), if on { 1.0 } else { 0.0 }));
            n += 1;
        }
    }
    Ok(n)
}

/// `guess=NET=V,NET=V`: where the DC solve starts for these nets (SPICE `.nodeset`). A circuit with a latch built from gates has
/// more than one answer; the guess picks the one near it.
fn apply_guess(c: &mut sim_core::Circuit, spec: &str) -> Result<(), String> {
    for item in spec.split(',').filter(|s| !s.trim().is_empty()) {
        let (net, val) = item.rsplit_once('=').ok_or_else(|| format!("guess: '{item}' should be NET=VOLTS"))?;
        let volts = sim_core::parse_value(val.trim()).ok_or_else(|| format!("guess: '{}' is not a voltage (write 3, -1.8, 500m ...)", val.trim()))?;
        let name = net.trim();
        if !c.names.iter().skip(1).any(|n| n.eq_ignore_ascii_case(name)) {
            let sample: Vec<&str> = c.names.iter().skip(1).filter(|n| !n.contains('.')).take(10).map(|s| s.as_str()).collect();
            return Err(format!("guess: no net called '{name}' (nets: {}, ...)", sample.join(", ")));
        }
        c.nodesets.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
        c.nodesets.push((name.to_string(), volts));
    }
    Ok(())
}

/// Solve again from other starting points (every net at the highest, then at the lowest voltage of the first answer) and report the
/// nets that settle at another voltage: a circuit with a latch built from gates has several answers, and `dc` found only one.
fn other_states(c: &sim_core::Circuit, first: &sim_core::DcResult) -> Value {
    let named = |r: &sim_core::DcResult| -> Vec<(String, f64)> { r.voltages.iter().filter(|(n, _)| !n.contains('.') && !n.eq_ignore_ascii_case("gnd")).cloned().collect() };
    let base = named(first);
    let (lo, hi) = base.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), (_, v)| (a.min(*v), b.max(*v)));
    let mut out = vec![];
    for (label, level) in [("all nets high", hi), ("all nets low", lo)] {
        let mut cc = c.clone();
        cc.nodesets = base.iter().map(|(n, _)| (n.clone(), level)).collect();
        match sim_core::dc_op(&cc) {
            Ok(r) => {
                let diff: Vec<(String, f64, f64)> = named(&r).into_iter().zip(&base).filter(|((_, v), (_, w))| (v - w).abs() > 0.2).map(|((n, v), (_, w))| (n, *w, v)).collect();
                let examples: Map<String, Value> = diff.iter().take(8).map(|(n, a, b)| (n.clone(), json!({"first": a, "here": b}))).collect();
                out.push(json!({"start": label, "nets_that_differ": diff.len(), "examples": examples}));
            }
            Err(e) => out.push(json!({"start": label, "error": e.to_string()})),
        }
    }
    json!(out)
}

fn dc_fail(c: &sim_core::Circuit, e: sim_core::SimError) -> String {
    let mut msg = cli_sim_error(e.to_string());
    let hints = sim_core::diagnose(c, &e);
    if !hints.is_empty() {
        msg += &format!(" | ヒント: {}", hints.join(" / "));
    }
    msg
}

/// `V(a),I(R1),V(a,b)` split at the commas outside the brackets.
fn split_probes(list: &str) -> Vec<&str> {
    let (mut out, mut depth, mut from) = (vec![], 0i32, 0);
    for (i, c) in list.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&list[from..i]);
                from = i + 1;
            }
            _ => {}
        }
    }
    out.push(&list[from..]);
    out.into_iter().map(str::trim).filter(|s| !s.is_empty()).collect()
}

/// The simulator's circuit for a schematic (the errors as one line).
fn circuit_of(sch: &sch_core::Schematic, wiring: &[sim_core::WiringR], couplings: &[sim_core::WiringK]) -> Result<sim_core::Circuit, String> {
    sim_core::from_schematic_with_layout(sch, wiring, couplings).map_err(|es| es.iter().map(|e| format!("{}: {}", e.refdes, e.message)).collect::<Vec<_>>().join("; "))
}

/// `V(net)`, `V(a,b)` or `I(part)` from a DC answer (names in any case).
fn probe_value(r: &sim_core::DcResult, spec: &str) -> Result<f64, String> {
    let s = spec.trim();
    let inner = |prefix: &str| s.get(..prefix.len()).filter(|p| p.eq_ignore_ascii_case(prefix)).and_then(|_| s.strip_suffix(')')).map(|x| x[prefix.len()..].to_string());
    let volt = |n: &str| r.voltage(n.trim()).ok_or_else(|| format!("probe {s}: no net '{}' (nets: {})", n.trim(), r.voltages.iter().map(|(n, _)| n.as_str()).filter(|n| !n.contains('.')).take(30).collect::<Vec<_>>().join(", ")));
    if let Some(x) = inner("V(") {
        return match x.split_once(',') {
            Some((a, b)) => Ok(volt(a)? - volt(b)?),
            None => volt(&x),
        };
    }
    if let Some(x) = inner("I(") {
        return r.elements.iter().find(|e| e.name.eq_ignore_ascii_case(x.trim())).map(|e| e.current).ok_or_else(|| format!("probe {s}: no part '{}'", x.trim()));
    }
    Err(format!("probe {s}: write V(net), V(a,b) or I(part)"))
}

fn bad_number(v: &str) -> String {
    format!("'{v}' is not a number (write 4.7k, 100n, 10u, 1Meg ...)")
}

impl Session {
    pub fn parts(&self, args: &[String]) -> crate::Out {
        let words: Vec<String> = args.iter().map(|w| lower(w)).collect();
        let hits: Vec<&part_lib::PartDef> = self
            .lib
            .iter()
            .filter(|d| {
                let hay = lower(&format!("{} {} {}", d.name, d.category, d.description));
                words.iter().all(|w| hay.contains(w.as_str()))
            })
            .collect();
        let shown: Vec<Value> = hits
            .iter()
            .take(40)
            .map(|d| json!({"name": d.name, "category": d.category, "pins": d.pins.len(), "simulates": d.sim.is_some(), "description": d.description}))
            .collect();
        obj(json!({"count": hits.len(), "shown": shown.len(), "parts": shown, "basic": ["R", "C", "L", "D", "LED", "V", "NPN", "HEADERn"], "library_errors": self.lib_errors}))
    }

    pub fn part(&self, args: &[String]) -> Res {
        let name = args.first().ok_or("part NAME")?;
        if let Some(k) = basic_kind(name) {
            let pins: Vec<Value> = k.pins().iter().map(|(n, _)| json!({"pin": n, "name": n})).collect();
            return Ok(obj(json!({"name": k.label(), "basic": true, "prefix": k.prefix(), "default_value": k.default_value(), "footprint": k.default_footprint(), "pins": pins, "simulates": true})));
        }
        let d = self.find_part(name)?;
        let pins: Vec<Value> = d.pins.iter().map(|p| json!({"pin": p.number, "name": p.name})).collect();
        Ok(obj(json!({
            "name": d.name, "category": d.category, "description": d.description, "prefix": d.prefix,
            "footprint": d.footprint_name(), "default_value": d.default_value(), "datasheet": d.datasheet,
            "pins": pins, "simulates": d.sim.is_some(),
            "sim_limits": d.sim.as_ref().map(|s| s.limits.clone()).unwrap_or_default(),
        })))
    }

    fn find_part(&self, name: &str) -> Result<&part_lib::PartDef, String> {
        if let Some(d) = self.lib.iter().find(|d| d.name.eq_ignore_ascii_case(name)) {
            return Ok(d);
        }
        let near: Vec<&str> = self.lib.iter().filter(|d| lower(&d.name).contains(&lower(name))).take(8).map(|d| d.name.as_str()).collect();
        Err(if near.is_empty() {
            format!("part '{name}' is not in the library (use 'parts WORD' to search)")
        } else {
            format!("part '{name}' is not in the library; similar: {}", near.join(", "))
        })
    }

    pub fn add(&mut self, args: &[String]) -> Res {
        let (pos, kv) = options(args);
        let t = pos.first().ok_or("add TYPE [VALUE] [as=REF] [rot=0..3] [at=X,Y]")?;
        let rot: u8 = match opt(&kv, "rot") {
            Some(r) => r.parse::<u8>().ok().filter(|r| *r < 4).ok_or("rot must be 0, 1, 2 or 3 (quarter turns clockwise)")?,
            None => 0,
        };
        let at: Option<Pt> = match opt(&kv, "at") {
            Some(a) => {
                let whole = "at=X,Y needs whole numbers (grid steps)";
                let (x, y) = a.split_once(',').ok_or(whole)?;
                Some((x.trim().parse().map_err(|_| whole)?, y.trim().parse().map_err(|_| whole)?))
            }
            None => None,
        };
        let kind = basic_kind(t);
        let lib_part = if kind.is_none() { Some(self.find_part(t)?.clone()) } else { None };
        let value = pos.get(1).cloned();
        if let (Some(k), Some(v)) = (kind, &value)
            && matches!(k, Kind::Resistor | Kind::Capacitor | Kind::Inductor)
            && sim_core::parse_value(v).is_none()
        {
            return Err(bad_number(v));
        }
        if let Some(r) = opt(&kv, "as")
            && self.sch.components.iter().any(|c| c.refdes.eq_ignore_ascii_case(r))
        {
            return Err(format!("{r} is already used"));
        }
        let idx = match (&lib_part, kind) {
            (Some(d), _) => self.sch.add_custom(d.symbol(), &d.default_value(), &d.footprint_name(), (0, 0), rot),
            (None, Some(k)) => self.sch.add(k, (0, 0), rot),
            _ => unreachable!(),
        };
        // find the room: the part's pins and body relative to its origin
        let c = &self.sch.components[idx];
        let mut xs: Vec<i32> = c.pins().iter().map(|(_, p)| p.0).collect();
        let mut ys: Vec<i32> = c.pins().iter().map(|(_, p)| p.1).collect();
        if let Some(sym) = &c.custom {
            let l = sym.layout();
            for (cx, cy) in [(-l.hw, -l.hh), (l.hw, l.hh)] {
                let r = sch_core::rotate((cx, cy), rot);
                xs.push(r.0);
                ys.push(r.1);
            }
        }
        let (minx, maxx) = (*xs.iter().min().unwrap(), *xs.iter().max().unwrap());
        let (miny, maxy) = (*ys.iter().min().unwrap(), *ys.iter().max().unwrap());
        let (w, h) = (maxx - minx, maxy - miny);
        let place = match at {
            Some(p) => p,
            None => {
                let (mut x, mut y, mut rowh) = self.cursor;
                if x > 8 && x + w > 110 {
                    x = 8;
                    y += rowh + 10;
                    rowh = 0;
                }
                self.cursor = (x + w + 10, y, rowh.max(h));
                (x - minx, y - miny)
            }
        };
        let c = &mut self.sch.components[idx];
        c.pos = place;
        if let Some(v) = value {
            c.value = v;
        }
        if let Some(r) = opt(&kv, "as") {
            c.refdes = r.to_string();
        }
        let c = &self.sch.components[idx];
        let pins: Vec<Value> = pin_names(c).iter().map(|(n, name)| json!({"pin": n, "name": name})).collect();
        let type_name = lib_part.as_ref().map(|d| d.name.clone()).unwrap_or_else(|| c.kind.label());
        Ok(obj(json!({"ref": c.refdes, "type": type_name, "value": c.value, "footprint": c.footprint_name(), "at": [c.pos.0, c.pos.1], "pins": pins})))
    }

    pub fn set_value(&mut self, args: &[String]) -> Res {
        let [r, rest @ ..] = args else { return Err("value REF VALUE   (for example: value R1 4.7k, value V1 5, value V2 PULSE(0 5 1m 1u 1u 2m 5m))".into()) };
        if rest.is_empty() {
            return Err("value REF VALUE   (for example: value R1 4.7k, value V1 5, value V2 PULSE(0 5 1m 1u 1u 2m 5m))".into());
        }
        // a source with spaces in it (PULSE(0 5 1m ...), "0 AC 1") is one value whether or not it was written in quotes
        let v = &rest.join(" ");
        let c = self.sch.components.iter_mut().find(|c| c.refdes.eq_ignore_ascii_case(r)).ok_or_else(|| format!("no part {r}"))?;
        if matches!(c.kind, Kind::Resistor | Kind::Capacitor | Kind::Inductor) && sim_core::parse_value(v).is_none() {
            return Err(bad_number(v));
        }
        c.value = v.clone();
        Ok(obj(json!({"ref": c.refdes, "value": c.value})))
    }

    /// `simonly REF [off]`: the part is only for the simulation (a test source, the stand-in for a device under test): it is not put on the board
    /// or in the bill of materials. `off` puts it back.
    pub fn sim_only(&mut self, args: &[String]) -> Res {
        let usage = "simonly REF [off]   (a part only for the simulation is left off the board and the bill of materials)";
        let (r, rest) = args.split_first().ok_or(usage)?;
        let on = match rest.first().map(|s| s.to_ascii_lowercase()) {
            None => true,
            Some(w) if w == "on" => true,
            Some(w) if w == "off" => false,
            Some(_) => return Err(usage.into()),
        };
        let c = self.sch.components.iter_mut().find(|c| !c.kind.is_power_symbol() && c.refdes.eq_ignore_ascii_case(r)).ok_or_else(|| format!("no part {r}"))?;
        c.sim_only = on;
        Ok(obj(json!({"ref": c.refdes, "sim_only": on})))
    }

    /// `pinnames REF NAME1,NAME2,...`: what each pin of a pin header is for (`-` clears them).
    pub fn pin_names(&mut self, args: &[String]) -> Res {
        let usage = "pinnames REF NAME1,NAME2,...   (the names of the pins of a pin header, pin 1 first; `-` clears them)";
        let [r, list] = args else { return Err(usage.into()) };
        let c = self.sch.components.iter_mut().find(|c| !c.kind.is_power_symbol() && c.refdes.eq_ignore_ascii_case(r)).ok_or_else(|| format!("no part {r}"))?;
        let Kind::Header(n) = c.kind else { return Err(format!("{} is not a pin header (pinnames is for headers)", c.refdes)) };
        c.pin_names = if list == "-" { vec![] } else { list.split(',').map(|s| s.trim().to_string()).collect() };
        if c.pin_names.len() > n as usize {
            let k = c.pin_names.len();
            c.pin_names.clear();
            return Err(format!("{} has {n} pins, but {k} names were given", c.refdes));
        }
        Ok(obj(json!({"ref": c.refdes, "pins": n, "names": c.pin_names})))
    }

    /// The points of the pins `REF.PIN` stands for (a pin name shared by several pins means all of them).
    fn pin_points(&self, spec: &str) -> Result<Vec<Pt>, String> {
        let (r, p) = spec.split_once('.').ok_or_else(|| format!("'{spec}': write REF.PIN, for example R1.1 or U1.GND"))?;
        let c = self.sch.components.iter().find(|c| !c.kind.is_power_symbol() && c.refdes.eq_ignore_ascii_case(r)).ok_or_else(|| format!("no part {r}"))?;
        let pins = c.pins();
        if let Some((_, pt)) = pins.iter().find(|(n, _)| n.eq_ignore_ascii_case(p)) {
            return Ok(vec![*pt]);
        }
        let by_name: Vec<Pt> =
            pin_names(c).iter().filter(|(_, name)| name.eq_ignore_ascii_case(p)).filter_map(|(n, _)| pins.iter().find(|(m, _)| m == n).map(|(_, pt)| *pt)).collect();
        if by_name.is_empty() {
            let have: Vec<String> = pin_names(c).iter().map(|(n, name)| if n == name { n.clone() } else { format!("{n}={name}") }).collect();
            return Err(format!("{} has no pin '{p}' (pins: {})", c.refdes, have.join(" ")));
        }
        Ok(by_name)
    }

    pub fn net(&mut self, args: &[String]) -> Res {
        if args.len() < 2 {
            return Err("net NAME REF.PIN [REF.PIN ...]".into());
        }
        let name = if args[0].eq_ignore_ascii_case("gnd") { "GND".to_string() } else { args[0].clone() };
        let mut points: Vec<Pt> = vec![];
        for spec in &args[1..] {
            for p in self.pin_points(spec)? {
                if !points.contains(&p) {
                    points.push(p);
                }
            }
        }
        // a pin already named something else is a mistake worth stopping on, not silently shorting two nets
        for p in &points {
            if let Some(l) = self.sch.labels.iter().find(|l| l.pos == *p && l.text != name) {
                let who = self
                    .sch
                    .components
                    .iter()
                    .flat_map(|c| c.pins().into_iter().map(move |(n, q)| (c.refdes.clone(), n, q)))
                    .find(|(_, _, q)| q == p)
                    .map(|(r, n, _)| format!("{r}.{n}"))
                    .unwrap_or_default();
                return Err(format!("{who} is already on net {} (remove the part and add it again, or use one name)", l.text));
            }
        }
        let mut added = 0;
        for p in &points {
            if !self.sch.labels.iter().any(|l| l.pos == *p && l.text == name) {
                self.sch.labels.push(Label { pos: *p, text: name.clone() });
                added += 1;
            }
        }
        // one ground symbol makes the sheet look and simulate like a normal one
        if name == "GND" && !self.sch.components.iter().any(|c| c.kind == Kind::Ground) {
            self.sch.add(Kind::Ground, points[0], 0);
        }
        Ok(obj(json!({"net": name, "pins": points.len(), "new": added})))
    }

    pub fn remove(&mut self, args: &[String]) -> Res {
        let r = args.first().ok_or("remove REF")?;
        let i = self.sch.components.iter().position(|c| !c.kind.is_power_symbol() && c.refdes.eq_ignore_ascii_case(r)).ok_or_else(|| format!("no part {r}"))?;
        let c = self.sch.components.remove(i);
        let pts: Vec<Pt> = c.pins().into_iter().map(|(_, p)| p).collect();
        self.sch.labels.retain(|l| !pts.contains(&l.pos));
        self.sch.components.retain(|g| !(g.kind == Kind::Ground && pts.contains(&g.pos)));
        Ok(obj(json!({"removed": c.refdes})))
    }

    pub fn list(&self) -> crate::Out {
        let nets = sch_core::extract(&self.sch);
        let net_of = |r: &str, p: &str| nets.iter().find(|n| n.pins.iter().any(|q| q.refdes == r && q.pin == p)).map(|n| n.name.clone());
        let parts: Vec<Value> = self
            .sch
            .components
            .iter()
            .filter(|c| !c.kind.is_power_symbol())
            .map(|c| {
                let pins: Vec<Value> = pin_names(c).iter().map(|(n, name)| json!({"pin": n, "name": name, "net": net_of(&c.refdes, n)})).collect();
                let type_name = c.custom.as_ref().map(|s| s.name.clone()).unwrap_or_else(|| c.kind.label());
                let mut part = json!({"ref": c.refdes, "type": type_name, "value": c.value, "footprint": c.footprint_name(), "pins": pins});
                if c.sim_only {
                    part["sim_only"] = json!(true);
                }
                part
            })
            .collect();
        let nets_out: Vec<Value> = nets.iter().map(|n| json!({"net": n.name, "pins": n.pins.iter().map(|p| format!("{}.{}", p.refdes, p.pin)).collect::<Vec<_>>()})).collect();
        obj(json!({"parts": parts, "nets": nets_out}))
    }

    fn erc_errors(&self) -> usize {
        sch_core::erc(&self.sch).iter().filter(|p| p.severity == sch_core::Severity::Error).count()
    }

    /// `--strict` refuses to go on with ERC errors; otherwise the caller adds a warning to its answer.
    pub(crate) fn erc_gate(&self) -> Result<Option<String>, String> {
        let n = self.erc_errors();
        if n == 0 {
            return Ok(None);
        }
        let msg = format!("ERC reports {n} error(s); run 'erc' and fix them first");
        if self.strict { Err(msg) } else { Ok(Some(msg)) }
    }

    pub fn erc(&self) -> Res {
        let probs = sch_core::erc(&self.sch);
        let errors = probs.iter().filter(|p| p.severity == sch_core::Severity::Error).count();
        if self.strict && errors > 0 {
            let first = probs.iter().find(|p| p.severity == sch_core::Severity::Error).map(|p| p.message.clone()).unwrap_or_default();
            return Err(format!("ERC reports {errors} error(s), for example: {first} (run 'erc' without --strict for the full list and hints)"));
        }
        let list: Vec<Value> =
            probs.iter().map(|p| json!({"severity": format!("{:?}", p.severity).to_lowercase(), "message": p.message, "hint": p.hint})).collect();
        let mut tips: Vec<&str> = vec![];
        if probs.iter().any(|p| p.message.contains("GND")) {
            tips.push("In ccad the 0 V reference is the net named GND, for example 'net GND V1.- R2.2'. The first net called GND also places the ground symbol; do not place one by hand.");
        }
        if probs.iter().any(|p| p.message.contains("未接続")) {
            tips.push("A pin with no net: connect it with 'net NAME REF.PIN', or give it a net of its own if it is meant to float.");
        }
        Ok(obj(json!({"errors": errors, "warnings": probs.len() - errors, "circuit_ok": errors == 0, "problems": list, "tips": tips, "note": "'ok' only says the command ran; the circuit has no ERC error when errors is 0"})))
    }

    /// The copper of the board as extra resistances and couplings, when `board postlayout on` asks for it.
    fn layout_parts(&self) -> (Vec<sim_core::WiringR>, Vec<sim_core::WiringK>) {
        match (&self.board, self.post_layout) {
            (Some(b), true) => post_layout_parts(b),
            _ => (vec![], vec![]),
        }
    }

    pub(crate) fn circuit(&self) -> Result<sim_core::Circuit, String> {
        let (wiring, couplings) = self.layout_parts();
        circuit_of(&self.sch, &wiring, &couplings)
    }

    pub fn dc(&mut self, args: &[String]) -> Res {
        let gate = self.erc_gate()?;
        let circuit_ok = gate.is_none();
        let (pos, kv) = options(args);
        let full = pos.iter().any(|a| a.eq_ignore_ascii_case("full"));
        let mut c = self.circuit()?;
        let ic = opt(&kv, "ic").map(|s| apply_ic(&mut c, s)).transpose()?;
        if let Some(spec) = opt(&kv, "guess") {
            apply_guess(&mut c, spec)?;
        }
        let clock = std::time::Instant::now();
        let r = sim_core::dc_op(&c).map_err(|e| dc_fail(&c, e))?;
        let seconds = clock.elapsed().as_secs_f64();
        let states = pos.iter().any(|a| a.eq_ignore_ascii_case("states")).then(|| other_states(&c, &r));
        self.last_dc = Some(r.clone());
        let volts: Map<String, Value> = r.voltages.iter().filter(|(n, _)| full || !n.contains('.')).map(|(n, v)| (n.clone(), json!(v))).collect();
        let internal_nodes = r.voltages.iter().filter(|(n, _)| n.contains('.')).count();
        // elements inside a library part's macro model are named 'U1.Ex': their values are model internals, not parts of the circuit
        let internal = r.elements.iter().filter(|e| e.name.contains('.')).count();
        let elems: Map<String, Value> = r
            .elements
            .iter()
            .filter(|e| full || !e.name.contains('.'))
            .map(|e| (e.name.clone(), json!({"current": e.current, "voltage": e.voltage, "power": e.power})))
            .collect();
        let mut warnings = c.warnings.clone();
        warnings.extend(r.warnings.iter().cloned());
        warnings.extend(gate);
        if !r.seq_states.is_empty() && ic.is_none() {
            warnings.push("To start from other memory states: dc ic=NAME=1,NAME=0 (the names are in 'memory'; a part of a name is enough).".to_string());
        }
        let (ratings, notes) = self.dc_notes(&r);
        warnings.extend(notes);
        let (hidden, hidden_nodes) = if full { (0, 0) } else { (internal, internal_nodes) };
        let mut answer = json!({"circuit_ok": circuit_ok, "voltages": volts, "elements": elems, "ratings": ratings, "internal_elements_hidden": hidden, "internal_nodes_hidden": hidden_nodes, "post_layout": self.post_layout && self.board.is_some(), "solver": {"strategy": r.strategy, "iterations": r.iterations, "seconds": seconds}, "warnings": warnings});
        if !r.seq_states.is_empty()
            && let Some(o) = answer.as_object_mut()
        {
            o.insert("memory".into(), Value::Object(r.seq_states.iter().map(|(n, s)| (n.clone(), json!(u8::from(*s)))).collect()));
        }
        if let (Some(s), Some(o)) = (&states, answer.as_object_mut()) {
            let differ = s.as_array().is_some_and(|a| a.iter().any(|x| x["nets_that_differ"].as_u64().unwrap_or(0) > 0));
            o.insert("other_states".into(), s.clone());
            if differ && let Some(w) = o.get_mut("warnings").and_then(|w| w.as_array_mut()) {
                w.push(json!("This circuit has more than one DC answer (a latch or counter built from gates): another starting point settles at other voltages, see 'other_states'. Pick the state you mean with dc guess=NET=VOLTS,..."));
            }
        }
        if let Some(list) = opt(&kv, "probe") {
            let mut probes = Map::new();
            for spec in split_probes(list) {
                probes.insert(spec.trim().to_string(), json!(probe_value(&r, spec)?));
            }
            if let Some(o) = answer.as_object_mut() {
                o.remove("voltages");
                o.remove("elements");
                o.insert("probes".into(), Value::Object(probes));
            }
        }
        Ok(obj(answer))
    }

    /// What a DC answer says besides the numbers, so that "it ran" is not read as "it is fine":
    /// the app's rating check (`ratings`: part, level over/caution, message, hint) and a warning for every op-amp
    /// whose two inputs differ (negative feedback is not holding them together: the output is saturated or there is no feedback).
    fn dc_notes(&self, r: &sim_core::DcResult) -> (Vec<Value>, Vec<String>) {
        let ratings: Vec<Value> = sim_core::check_ratings(&self.sch, r)
            .into_iter()
            .map(|i| json!({"ref": i.refdes, "level": if i.level == sim_core::RatingLevel::Over { "over" } else { "caution" }, "message": i.message, "hint": i.hint}))
            .collect();
        let mut notes = vec![];
        if !ratings.is_empty() {
            let over = ratings.iter().filter(|v| v["level"] == "over").count();
            notes.push(format!("{} rating issue(s), {over} over: see 'ratings' (typical package limits, not the data sheet)", ratings.len()));
        }
        let nets = sch_core::extract(&self.sch);
        for c in &self.sch.components {
            let Some(sym) = &c.custom else { continue };
            let pin = |name: &str| sym.pins.iter().find(|p| p.name.eq_ignore_ascii_case(name)).map(|p| p.number.clone());
            let (Some(pp), Some(pn), Some(po)) = (pin("IN+"), pin("IN-"), pin("OUT")) else { continue };
            let volt = |p: &str| {
                let net = nets.iter().find(|n| n.pins.iter().any(|q| q.refdes == c.refdes && q.pin == p))?;
                r.voltage(&net.name)
            };
            let (Some(vp), Some(vn), Some(vo)) = (volt(&pp), volt(&pn), volt(&po)) else { continue };
            if (vp - vn).abs() > 0.01 {
                notes.push(format!(
                    "{}: IN+ is {vp:.3} V but IN- is {vn:.3} V (OUT {vo:.3} V). With negative feedback they match, so the output is probably saturated (an op-amp output stops short of the supply: see 'part {}') or there is no feedback. Ignore this if the part is meant to be a comparator.",
                    c.refdes, sym.name
                ));
            }
        }
        (ratings, notes)
    }

    pub fn tran(&mut self, args: &[String]) -> Res {
        let (pos, kv) = options(args);
        if pos.is_empty() {
            return Err("tran TSTOP [TSTEP] [probe=V(out),I(R1)] [samples=6] [uic]   (TSTEP is the largest step, default TSTOP/1000; the solver refines it)".into());
        }
        let num = |s: &str| sim_core::parse_value(s).filter(|v| *v > 0.0).ok_or_else(|| format!("'{s}' is not a positive number (write 10m, 20u ...)"));
        let tstop = num(&pos[0])?;
        // TSTEP is optional: a second word that is "uic" is the flag, not a step
        let has_step = pos.get(1).is_some_and(|w| !w.eq_ignore_ascii_case("uic"));
        let tstep = if has_step { num(&pos[1])? } else { sim_core::auto_tstep(tstop) };
        let uic = pos.iter().skip(1).any(|w| w.eq_ignore_ascii_case("uic"));
        let samples: usize = opt(&kv, "samples").and_then(|s| s.parse().ok()).unwrap_or(6).clamp(0, 200);
        let probes: Vec<String> = opt(&kv, "probe").map(|p| p.split(',').map(lower).collect()).unwrap_or_default();
        let gate = self.erc_gate()?;
        let circuit_ok = gate.is_none();
        let mut c = self.circuit()?;
        if let Some(spec) = opt(&kv, "ic") {
            apply_ic(&mut c, spec)?;
        }
        if let Some(spec) = opt(&kv, "guess") {
            apply_guess(&mut c, spec)?;
        }
        let r = sim_core::transient(&c, &sim_core::TranSettings { tstop, tstep, uic }).map_err(|e| cli_sim_error(e.to_string()))?;
        self.last_tran = Some(r.clone());
        let n = r.time.len();
        let pick = |v: &[f64]| -> Vec<Value> {
            (0..samples).map(|k| if samples == 1 { n - 1 } else { k * (n - 1) / (samples - 1) }).map(|i| json!([r.time[i], v[i]])).collect()
        };
        let chosen: Vec<&sim_core::Signal> =
            r.signals.iter().filter(|s| probes.is_empty() || probes.iter().any(|p| lower(&s.name) == *p || lower(&s.name).contains(p.as_str()))).collect();
        let cap = if probes.is_empty() { 40 } else { usize::MAX };
        let sigs: Map<String, Value> = chosen
            .iter()
            .take(cap)
            .filter(|s| !s.values.is_empty())
            .map(|s| {
                let (mn, mx) = s.values.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
                let mean = s.values.iter().sum::<f64>() / s.values.len() as f64;
                let mut o = json!({"unit": s.unit, "min": mn, "max": mx, "mean": mean, "final": s.values[n - 1], "samples": pick(&s.values)});
                if let Some((period, duty)) = periodicity(&r.time, &s.values, mn, mx) {
                    o["period"] = json!(period);
                    o["frequency"] = json!(1.0 / period);
                    o["duty"] = json!(duty);
                }
                (s.name.clone(), o)
            })
            .collect();
        let (ratings, notes) = self.tran_notes(&r);
        let mut answer = json!({"circuit_ok": circuit_ok, "points": n, "tstop": tstop, "signals": sigs, "signals_not_shown": chosen.len().saturating_sub(cap), "ratings": ratings, "post_layout": self.post_layout && self.board.is_some(), "warnings": r.warnings.iter().cloned().chain(gate).chain(notes).collect::<Vec<_>>()});
        if let Some(list) = opt(&kv, "settle") {
            let from = match opt(&kv, "from") {
                Some(s) => sim_core::parse_value(s).filter(|v| *v >= 0.0).ok_or_else(|| format!("from: '{s}' is not a time (write 20u, 1m ...)"))?,
                None => 0.0,
            };
            let tols: Vec<f64> = match opt(&kv, "tol") {
                Some(s) => s.split(',').map(|x| sim_core::parse_value(x.trim()).filter(|v| *v > 0.0 && *v < 1.0).ok_or_else(|| format!("tol: '{x}' is not a fraction between 0 and 1 (write 1e-3, 3e-5 ...)"))).collect::<Result<_, _>>()?,
                None => vec![1e-2, 1e-3, 1e-4],
            };
            let mut settle = Map::new();
            for want in list.split(',').map(|s| lower(s.trim())).filter(|s| !s.is_empty()) {
                let sig = r.signals.iter().find(|s| lower(&s.name) == want).or_else(|| r.signals.iter().find(|s| lower(&s.name).contains(want.as_str()))).ok_or_else(|| format!("settle: no signal '{want}' (probe it with probe=...)"))?;
                let one = match sim_core::settling_times(&r.time, &sig.values, from, &tols) {
                    Ok(s) => {
                        let times: Map<String, Value> = s.times.iter().map(|(tol, when)| (format!("{tol:e}"), when.map_or(Value::Null, |w| json!(w)))).collect();
                        json!({"initial": s.initial, "final": s.final_value, "overshoot_pct": s.overshoot_pct, "settled_within": times, "from": from})
                    }
                    Err(e) => json!({"error": e}),
                };
                settle.insert(sig.name.clone(), one);
            }
            if let Some(o) = answer.as_object_mut() {
                o.insert("settle".into(), Value::Object(settle));
            }
        }
        Ok(obj(answer))
    }

    /// The `tran` counterpart of `dc_notes`, over the second half of the run (the start-up is left out):
    /// resistors by rms power against the package, LEDs by peak current, electrolytic capacitors by ripple current
    /// (the app's `ripple_reports`), and op-amps whose inputs stay apart for more than a fifth of the time
    /// (saturated output, no feedback, or a comparator / oscillator, which the message says to ignore).
    fn tran_notes(&self, r: &sim_core::TranResult) -> (Vec<Value>, Vec<String>) {
        let n = r.time.len();
        if n < 4 {
            return (vec![], vec![]);
        }
        let start = n / 2;
        // time-weighted mean of f(i) over the second half (the solver's steps are not equal)
        let tmean = |f: &dyn Fn(usize) -> f64| -> f64 {
            let (mut sum, mut span) = (0.0, 0.0);
            for i in start..n - 1 {
                let dt = r.time[i + 1] - r.time[i];
                sum += f(i) * dt;
                span += dt;
            }
            if span > 0.0 { sum / span } else { f(n - 1) }
        };
        let signal = |name: String| r.signals.iter().find(|s| s.name.eq_ignore_ascii_case(&name)).map(|s| s.values.clone());
        let nets = sch_core::extract(&self.sch);
        let net_of = |refdes: &str, pin: &str| nets.iter().find(|x| x.pins.iter().any(|q| q.refdes == refdes && q.pin == pin)).map(|x| x.name.clone());
        let mut ratings: Vec<Value> = vec![];
        let mut push = |refdes: &str, over: bool, message: String, hint: &str| {
            ratings.push(json!({"ref": refdes, "level": if over { "over" } else { "caution" }, "message": message, "hint": hint}));
        };
        let mut notes = vec![];
        for c in &self.sch.components {
            match c.kind {
                Kind::Resistor => {
                    let (Some(i), Some(ohm), Some(limit)) = (signal(format!("I({})", c.refdes)), sim_core::parse_value(&c.value), sim_core::package_power_limit(&c.footprint_name())) else { continue };
                    let p = tmean(&|k| i[k] * i[k]) * ohm;
                    if p > 0.5 * limit {
                        push(
                            &c.refdes,
                            p > limit,
                            format!("{} ({}) の消費電力 (実効値) は {:.0} mW で、この形の目安 {:.0} mW に対して{}", c.refdes, c.footprint_name(), p * 1e3, limit * 1e3, if p > limit { "目安を超えています" } else { "余裕が少ないです" }),
                            "抵抗値を大きくして電流を減らすか、大きい形 (1206 / 2512 や 1/2W 品) に変えます。実際の定格はデータシートで確認してください。",
                        );
                    }
                }
                Kind::Led => {
                    let Some(i) = signal(format!("I({})", c.refdes)) else { continue };
                    let peak = i.iter().cloned().fold(0.0, f64::max);
                    if peak > 0.02 {
                        push(&c.refdes, peak > 0.03, format!("{} の電流が最大 {:.1} mA になっています。一般的な LED は 20 mA 前後が上限です", c.refdes, peak * 1e3), "電流制限抵抗を大きくします。R = (電源電圧 − LED の順電圧) ÷ 目標電流 で決めます。");
                    }
                }
                _ => {}
            }
            let Some(sym) = &c.custom else { continue };
            let pin = |name: &str| sym.pins.iter().find(|p| p.name.eq_ignore_ascii_case(name)).map(|p| p.number.clone());
            let (Some(pp), Some(pn), Some(po)) = (pin("IN+"), pin("IN-"), pin("OUT")) else { continue };
            // GND has no V(GND) signal: it is 0 V
            let volts = |p: &str| -> Option<Vec<f64>> {
                let net = net_of(&c.refdes, p)?;
                if net.eq_ignore_ascii_case("GND") { Some(vec![0.0; n]) } else { signal(format!("V({net})")) }
            };
            let (Some(vp), Some(vn)) = (volts(&pp), volts(&pn)) else { continue };
            let apart = tmean(&|k| if (vp[k] - vn[k]).abs() > 0.01 { 1.0 } else { 0.0 });
            if apart > 0.2 {
                let worst = (start..n).map(|k| (vp[k] - vn[k]).abs()).fold(0.0, f64::max);
                let out = volts(&po).map(|v| format!(" (OUT {:.3} V at the end)", v[n - 1])).unwrap_or_default();
                notes.push(format!(
                    "{}: IN+ and IN- differ by more than 10 mV for {:.0}% of the second half of the run (up to {worst:.3} V){out}. With negative feedback they match, so the output is probably saturated (an op-amp output stops short of the supply: see 'part {}') or there is no feedback. Ignore this if the part is meant to be a comparator or an oscillator.",
                    c.refdes, apart * 100.0, sym.name
                ));
            }
        }
        for rp in sim_core::ripple_reports(&self.sch, r, 27.0) {
            if let Some(level) = rp.level {
                push(&rp.refdes, level == sim_core::RatingLevel::Over, rp.message(), rp.hint());
            }
        }
        if !ratings.is_empty() {
            let over = ratings.iter().filter(|v| v["level"] == "over").count();
            notes.insert(0, format!("{} rating issue(s), {over} over, in the second half of the run: see 'ratings' (typical package limits, not the data sheet)", ratings.len()));
        }
        (ratings, notes)
    }
}

/// The board's copper as the simulator wants it: series resistance and inductance per pad, and the coupling between wires.
pub(crate) fn post_layout_parts(b: &pcb_core::Board) -> (Vec<sim_core::WiringR>, Vec<sim_core::WiringK>) {
    let spec = pcb_core::irdrop::IrSpec::default();
    let series = pcb_core::irdrop::wiring_resistances(b, &spec);
    let couplings = pcb_core::irdrop::wiring_couplings(&series, &spec).into_iter().map(|c| sim_core::WiringK { a: c.a, b: c.b, k: c.k }).collect();
    let wiring = series.into_iter().map(|w| sim_core::WiringR { refdes: w.refdes, pin: w.pin, ohm: w.ohm, henry: w.henry, farad: w.farad }).collect();
    (wiring, couplings)
}

impl Session {
    /// `sweep REF START STOP [steps=11] [log=1] probe=V(out),I(R1)` or `sweep REF list=a,b,c probe=...`: a DC point for each value of one part.
    pub fn sweep(&mut self, args: &[String]) -> Res {
        let (pos, kv) = options(args);
        let usage = "sweep REF START STOP [steps=11] [log=1] probe=V(out),I(R1)   (or: sweep REF list=1,2,5 probe=...; REF is a V source, R, C or L)";
        let Some(refdes) = pos.first() else { return Err(usage.into()) };
        let num = |s: &str| sim_core::parse_value(s).ok_or_else(|| format!("'{s}' is not a number (write 4.7k, 100n, 0.5 ...)"));
        let values: Vec<f64> = if let Some(l) = opt(&kv, "list") {
            l.split(',').map(|s| num(s.trim())).collect::<Result<_, _>>()?
        } else {
            if pos.len() < 3 {
                return Err(usage.into());
            }
            let (a, b) = (num(&pos[1])?, num(&pos[2])?);
            let steps: usize = opt(&kv, "steps").and_then(|s| s.parse().ok()).unwrap_or(11).clamp(2, 200);
            let log = opt(&kv, "log").is_some_and(|s| s != "0");
            if log && (a <= 0.0 || b <= 0.0) {
                return Err("log=1 needs positive START and STOP".into());
            }
            (0..steps).map(|i| {
                let f = i as f64 / (steps - 1) as f64;
                if log { a * (b / a).powf(f) } else { a + (b - a) * f }
            }).collect()
        };
        if values.is_empty() || values.len() > 200 {
            return Err("sweep: 1 to 200 values".into());
        }
        let probes: Vec<String> = opt(&kv, "probe").map(|p| split_probes(p).into_iter().map(str::to_string).collect()).unwrap_or_default();
        if probes.is_empty() {
            return Err(format!("sweep needs probe=V(net),I(part) to say what to record. {usage}"));
        }
        let idx = self.sch.components.iter().position(|c| c.refdes.eq_ignore_ascii_case(refdes)).ok_or_else(|| format!("no part {refdes}"))?;
        let original = self.sch.components[idx].value.clone();
        let gate = self.erc_gate()?;
        let mut rows = vec![];
        let mut failed = 0;
        for x in &values {
            self.sch.components[idx].value = format!("{x}");
            let row = match self.circuit().and_then(|c| sim_core::dc_op(&c).map_err(|e| e.to_string())) {
                Ok(r) => {
                    let mut o = Map::new();
                    o.insert("value".into(), json!(x));
                    for p in &probes {
                        match probe_value(&r, p) {
                            Ok(v) => o.insert(p.clone(), json!(v)),
                            Err(e) => {
                                self.sch.components[idx].value = original;
                                return Err(e);
                            }
                        };
                    }
                    Value::Object(o)
                }
                Err(e) => {
                    failed += 1;
                    json!({"value": x, "error": e})
                }
            };
            rows.push(row);
        }
        self.sch.components[idx].value = original.clone();
        Ok(obj(json!({"ref": self.sch.components[idx].refdes, "restored_value": original, "points": rows.len(), "failed": failed, "rows": rows, "warnings": gate.into_iter().collect::<Vec<_>>()})))
    }

    /// `sens probe=V(out) [step=1] [parts=R1,R2] [kinds=R,V,I] [top=15]`: how much the probe moves when each part's value moves by
    /// `step` percent (the DC operating point is solved again for every part). Returns the parts ordered by the size of the move,
    /// with the move per percent and the elasticity (relative change of the probe per relative change of the part).
    pub fn sens(&mut self, args: &[String]) -> Res {
        let (_, kv) = options(args);
        let usage = "sens probe=V(out) [step=1] [parts=R1,R2 | kinds=R,V,I,C,L] [top=15]   (a probe is V(net), V(a,b) or I(part); step is in percent)";
        let probe = opt(&kv, "probe").ok_or_else(|| usage.to_string())?.trim().to_string();
        let step: f64 = match opt(&kv, "step") {
            Some(s) => s.parse().ok().filter(|v: &f64| *v > 0.0 && *v <= 50.0).ok_or("step is a percentage between 0 and 50 (1 = a 1 % change)")?,
            None => 1.0,
        };
        let top: usize = opt(&kv, "top").and_then(|s| s.parse().ok()).unwrap_or(15).clamp(1, 200);
        // the parts to move: those named, else every part of the wanted kinds (resistors and DC sources unless `kinds=` says otherwise)
        let named: Vec<String> = opt(&kv, "parts").map(|p| p.split(',').map(|s| s.trim().to_ascii_lowercase()).filter(|s| !s.is_empty()).collect()).unwrap_or_default();
        let kinds: Vec<String> = opt(&kv, "kinds").map(|p| p.split(',').map(|s| s.trim().to_ascii_uppercase()).collect()).unwrap_or_else(|| vec!["R".into(), "V".into(), "I".into()]);
        let mut targets: Vec<(usize, f64)> = vec![];
        for (i, c) in self.sch.components.iter().enumerate() {
            let wanted = if named.is_empty() { kinds.iter().any(|k| c.refdes.to_ascii_uppercase().starts_with(k.as_str())) && c.custom.is_none() } else { named.contains(&c.refdes.to_ascii_lowercase()) };
            // a value like "5 AC 1" or "PULSE(...)" has no single number to move
            if wanted && let Some(x) = c.value.split_whitespace().next().and_then(sim_core::parse_value).filter(|x| *x != 0.0) {
                targets.push((i, x));
            }
        }
        if let Some(missing) = named.iter().find(|n| !self.sch.components.iter().any(|c| c.refdes.eq_ignore_ascii_case(n))) {
            let spelled = opt(&kv, "parts").and_then(|p| p.split(',').map(str::trim).find(|s| s.eq_ignore_ascii_case(missing))).unwrap_or(missing);
            return Err(format!("no part {spelled}"));
        }
        if targets.is_empty() {
            return Err("sens: no part with a value to move (name them with parts=R1,R2, or choose kinds=R,C,L,V,I)".into());
        }
        if targets.len() > 150 {
            return Err(format!("sens: {} parts would take {} DC solutions; name the parts you care about with parts=..., or lower the count with kinds=", targets.len(), targets.len() + 1));
        }
        let gate = self.erc_gate()?;
        let base = {
            let c = self.circuit()?;
            let r = sim_core::dc_op(&c).map_err(|e| dc_fail(&c, e))?;
            probe_value(&r, &probe)?
        };
        let mut rows = vec![];
        let mut failed = vec![];
        // one DC solution per part, each on its own copy of the schematic: they are independent, so they run on all the cores
        let (wiring, couplings) = self.layout_parts();
        let base_sch = &self.sch;
        let answers = sim_core::par_map(&targets, |&(i, x)| {
            let mut sch = base_sch.clone();
            let original = sch.components[i].value.clone();
            let rest: String = original.split_whitespace().skip(1).collect::<Vec<_>>().join(" ");
            let moved = x * (1.0 + step / 100.0);
            sch.components[i].value = if rest.is_empty() { format!("{moved}") } else { format!("{moved} {rest}") };
            let answer = circuit_of(&sch, &wiring, &couplings).and_then(|c| sim_core::dc_op(&c).map_err(|e| e.to_string()));
            (sch.components[i].refdes.clone(), original, answer.and_then(|r| probe_value(&r, &probe)))
        });
        for (refdes, original, answer) in answers {
            match answer {
                Ok(v) => {
                    let per_percent = (v - base) / step;
                    let elasticity = if base.abs() > 1e-30 { Some((v - base) / base / (step / 100.0)) } else { None };
                    rows.push((per_percent.abs(), json!({"part": refdes, "value": original, "per_percent": per_percent, "elasticity": elasticity})));
                }
                Err(e) => failed.push(json!({"part": refdes, "error": e})),
            }
        }
        rows.sort_by(|a, b| b.0.total_cmp(&a.0));
        let shown: Vec<Value> = rows.iter().take(top).map(|(_, v)| v.clone()).collect();
        Ok(obj(json!({"probe": probe, "base": base, "step_percent": step, "parts_tried": rows.len() + failed.len(), "parts": shown, "not_shown": rows.len().saturating_sub(top), "failed": failed, "warnings": gate.into_iter().collect::<Vec<_>>()})))
    }
}
