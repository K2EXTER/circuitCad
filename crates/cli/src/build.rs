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
        "I" | "ISOURCE" => Kind::ISource,
        "NPN" => Kind::Npn,
        "VR" | "POT" | "POTENTIOMETER" => Kind::Pot,
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

/// The numbers of the pins of `c` called `want`: the whole name first ("REFP1/AIN6"), else one of the names a
/// slash-separated name of a dual-use pin is made of ("AIN6" or "REFP1"). A name shared by several pins gives them all.
pub(crate) fn pins_named(c: &Component, want: &str) -> Vec<String> {
    let names = pin_names(c);
    let whole: Vec<String> = names.iter().filter(|(_, name)| name.eq_ignore_ascii_case(want)).map(|(n, _)| n.clone()).collect();
    if !whole.is_empty() || want.contains('/') {
        return whole;
    }
    names.iter().filter(|(_, name)| name.contains('/') && name.split('/').any(|part| part.trim().eq_ignore_ascii_case(want))).map(|(n, _)| n.clone()).collect()
}

/// Period and duty of a repeating wave, measured as the app's result table does (`sim_core::wave_measure`: crossings of
/// the mid level with a little hysteresis, the first period left out when it is not a steady one). None for a wave that
/// does not repeat. (ccad had a copy of its own that counted the long first cycle of a 555 started from rest.)
fn periodicity(t: &[f64], v: &[f64]) -> Option<(f64, f64)> {
    let m = sim_core::wave_measure(t, v, *t.first()?, *t.last()?)?;
    let f = m.freq.filter(|f| f.is_finite() && *f > 0.0)?;
    Some((1.0 / f, m.duty.unwrap_or(0.0)))
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
pub(crate) fn apply_guess(c: &mut sim_core::Circuit, spec: &str) -> Result<(), String> {
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
    // nets that sit between the rails in the first answer (a latch resting at its unstable middle point): another start splits them,
    // every other one high and the rest low, because a start that treats them alike keeps a symmetric latch in the middle
    let span = hi - lo;
    let mid: Vec<usize> = (0..base.len()).filter(|&i| span > 1.0 && base[i].1 > lo + 0.2 * span && base[i].1 < hi - 0.2 * span).collect();
    let mut starts: Vec<(&str, Vec<f64>)> = vec![("all nets high", vec![hi; base.len()]), ("all nets low", vec![lo; base.len()])];
    if mid.len() >= 2 {
        for (label, first_high) in [("middle nets split (first high)", true), ("middle nets split (first low)", false)] {
            let mut levels: Vec<f64> = base.iter().map(|(_, v)| *v).collect();
            for (k, &i) in mid.iter().enumerate() {
                levels[i] = if (k % 2 == 0) == first_high { hi } else { lo };
            }
            starts.push((label, levels));
        }
    }
    for (label, levels) in starts {
        let mut cc = c.clone();
        cc.nodesets = base.iter().zip(&levels).map(|((n, _), l)| (n.clone(), *l)).collect();
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
        let matches = |d: &part_lib::PartDef| {
            let hay = lower(&format!("{} {} {}", d.name, d.category, d.description));
            words.iter().all(|w| hay.contains(w.as_str()))
        };
        // a series' parts (one per value) are listed once, as the series; a member is listed itself only when asked by its full name
        let mut series: Vec<&part_lib::series::SeriesDef> = vec![];
        let hits: Vec<&part_lib::PartDef> = self
            .lib
            .iter()
            .filter(|d| matches(d))
            .filter(|d| match part_lib::series::member(&d.name) {
                Some((s, _)) => {
                    if !series.iter().any(|x| x.series == s.series) {
                        series.push(s);
                    }
                    words.iter().any(|w| *w == lower(&d.name))
                }
                None => true,
            })
            .collect();
        let shown: Vec<Value> = hits
            .iter()
            .take(40)
            .map(|d| json!({"name": d.name, "category": d.category, "pins": d.pins.len(), "simulates": d.sim.is_some(), "description": d.description}))
            .collect();
        let series: Vec<Value> = series
            .iter()
            .map(|s| json!({"series": s.series, "maker": s.maker, "summary": s.summary, "values": s.range_text(), "quality": s.quality, "example": s.part_name(s.default_value()), "add": format!("add {} 4.7k", s.series)}))
            .collect();
        obj(json!({"count": hits.len(), "shown": shown.len(), "parts": shown, "series": series, "basic": ["R", "C", "L", "D", "LED", "V", "NPN", "HEADERn"], "library_errors": self.lib_errors}))
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
        let t = pos.first().ok_or("add TYPE [VALUE] [as=REF] [unit=A] [rot=0..3] [at=X,Y]")?;
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
        // a series and a value ("add ERJ-3EKF 4.7k"): the series' part of that value
        let series_part = if kind.is_none() && let Some(s) = part_lib::series::by_name(t) {
            let v = match pos.get(1) {
                Some(text) => part_lib::series::parse_ohms(text).ok_or_else(|| bad_number(text))?,
                None => s.default_value(),
            };
            match s.nearest(v) {
                Some((x, true)) => Some(s.part_name(x)),
                _ => {
                    let (lo, hi) = s.neighbours(v);
                    let near: Vec<String> = [lo, hi].into_iter().flatten().map(|x| format!("{} ({})", part_lib::series::ohms_text(x), s.part_name(x))).collect();
                    return Err(format!("{} is not made in {} ({}); nearest: {}", part_lib::series::ohms_text(v), s.series, s.range_text(), near.join(", ")));
                }
            }
        } else {
            None
        };
        let t = series_part.as_ref().unwrap_or(t);
        let lib_part = if kind.is_none() { Some(self.find_part(t)?.clone()) } else { None };
        let value = if series_part.is_some() { None } else { pos.get(1).cloned() };
        if let (Some(k), Some(v)) = (kind, &value)
            && matches!(k, Kind::Resistor | Kind::Capacitor | Kind::Inductor)
            && sim_core::parse_part_value(v).is_err()
        {
            return Err(bad_number(v));
        }
        // unit=B: one unit of a part of several (a dual op amp's second channel, Q6); with as= it may share the number of the
        // same part's other units
        let unit = match opt(&kv, "unit") {
            None => None,
            Some(u) => {
                let n = lib_part.as_ref().map_or(1, |d| d.symbol().units());
                let k = u.trim().to_ascii_uppercase().bytes().next().filter(|b| b.is_ascii_uppercase() && u.trim().len() == 1).map(|b| b - b'A');
                match k {
                    Some(k) if k < n => Some(k),
                    _ => return Err(format!("unit={u}: this part has {} (A{})", if n > 1 { format!("units A to {}", sch_core::CustomSymbol::unit_letter(n - 1)) } else { "one unit".into() }, if n > 1 { "..." } else { "" })),
                }
            }
        };
        if let Some(r) = opt(&kv, "as") {
            let same_part = |c: &sch_core::Component| unit.is_some() && c.custom.as_ref().is_some_and(|x| lib_part.as_ref().is_some_and(|d| d.name == x.name));
            if let Some(c) = self.sch.components.iter().find(|c| c.refdes.eq_ignore_ascii_case(r)) {
                if !same_part(c) {
                    return Err(format!("{r} is already used"));
                }
                if self.sch.components.iter().any(|o| o.refdes.eq_ignore_ascii_case(r) && Some(o.unit().0) == unit) {
                    return Err(format!("{r} already has unit {}", sch_core::CustomSymbol::unit_letter(unit.unwrap_or(0))));
                }
            }
        }
        let idx = match (&lib_part, kind) {
            (Some(d), _) => self.sch.add_custom(d.symbol(), &d.default_value(), &d.footprint_name(), (0, 0), rot),
            (None, Some(k)) => self.sch.add(k, (0, 0), rot),
            _ => unreachable!(),
        };
        if let (Some(u), Some(s)) = (unit, self.sch.components[idx].custom.as_mut()) {
            s.unit = u;
        }
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
        let (u, n) = c.unit();
        if n > 1 {
            return Ok(obj(json!({"ref": c.refdes, "unit": sch_core::CustomSymbol::unit_letter(u).to_string(), "units": n, "shown": c.shown_refdes(), "type": type_name, "value": c.value, "footprint": c.footprint_name(), "at": [c.pos.0, c.pos.1], "pins": pins})));
        }
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
        if matches!(c.kind, Kind::Resistor | Kind::Capacitor | Kind::Inductor) && sim_core::parse_part_value(v).is_err() {
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

    /// `footprint REF NAME` / `footprint REF -`: the PCB footprint of a plain part (a resistor, capacitor, diode ...), for example
    /// `R_0805_Reflow`. `-` goes back to the kind's default. A library part brings its own footprint, so it is refused.
    pub fn footprint(&mut self, args: &[String]) -> Res {
        let usage = "footprint REF NAME   (`footprint REF -` goes back to the default; names: see `parts`, e.g. R_0805, R_0805_Reflow)";
        let [r, name] = args else { return Err(usage.into()) };
        let c = self.sch.components.iter_mut().find(|c| !c.kind.is_power_symbol() && c.refdes.eq_ignore_ascii_case(r)).ok_or_else(|| format!("no part {r}"))?;
        if c.custom.is_some() {
            return Err(format!("{} is a library part: its footprint ({}) comes with the part", c.refdes, c.footprint_name()));
        }
        if name == "-" {
            c.footprint = c.kind.default_footprint();
        } else {
            if pcb_core::footprints::get(name).is_none() {
                return Err(format!("unknown footprint '{name}' (some of them: {})", pcb_core::footprints::names().into_iter().take(14).collect::<Vec<_>>().join(", ")));
            }
            c.footprint = name.clone();
        }
        Ok(obj(json!({"ref": c.refdes, "footprint": c.footprint_name()})))
    }

    /// `shop REF [akizuki=I-00941] [digikey=RC0805FR-071KL] [lcsc=C123] | shop REF -`: the shop part numbers of one part (for a plain
    /// resistor or capacitor, which has no library part to carry them). They reach the board's shopping list and the BOM.
    pub fn shop(&mut self, args: &[String]) -> Res {
        let usage = "shop REF [akizuki=NUMBER] [digikey=NUMBER] [lcsc=NUMBER]   (`shop REF -` clears them; `shop REF` shows them). Write only numbers you read on the shop's page.";
        let (r, rest) = args.split_first().ok_or(usage)?;
        let c = self.sch.components.iter_mut().find(|c| !c.kind.is_power_symbol() && c.refdes.eq_ignore_ascii_case(r)).ok_or_else(|| format!("no part {r}"))?;
        if rest.first().is_some_and(|w| w == "-") {
            c.shops.clear();
        }
        for a in rest.iter().filter(|a| *a != "-") {
            let (shop, number) = a.split_once('=').ok_or_else(|| format!("'{a}': write shop=NUMBER (shops: akizuki, digikey, lcsc)"))?;
            let shop = shop.trim().to_ascii_lowercase();
            if pcb_core::shops::shop(&shop).is_none() {
                return Err(format!("unknown shop '{shop}' (shops: akizuki, digikey, lcsc)"));
            }
            if number.trim().is_empty() {
                c.shops.remove(&shop);
            } else {
                c.shops.insert(shop, number.trim().to_string());
            }
        }
        Ok(obj(json!({"ref": c.refdes, "shops": c.shop_numbers()})))
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

    /// `thermal REF [theta=K/W] [jc=K/W] [sink=K/W] [tc=ppm] [cth=J/K] [tmax=C] [zth=R:TAU,...] [zthfit=T:Z,...]`: the heat resistance, its part from the junction to the case, a heat sink, the temperature coefficient, the heat capacity, the highest temperature and the transient heat resistance (Foster stages, or fitted to points of the data sheet's Zth curve) of a part for `heat` / `heattran` (0 = back to the guess).
    pub fn thermal(&mut self, args: &[String]) -> Res {
        let usage = "thermal REF [theta=K/W] [jc=K/W] [sink=K/W] [tc=ppm/K] [cth=J/K] [tmax=C] [zth=R:TAU,R:TAU...] [zthfit=T:Z,T:Z...]   (jc is theta from the junction to the case, which the board copper and a heat sink cannot help; sink is a heat sink's theta from the case to the air, the mounting included. 0 puts a value back to the default: theta from the package, jc a share of theta, no heat sink, tc from the model, no heat capacity, the highest temperature from the library part or a guess from the kind, zth=0 back to the library part's stages; zthfit fits up to 4 stages to points read off the data sheet's single-pulse Zth curve)
thermal link=Q1:RS1:150[,A:B:K/W...]   (a heat resistance between two parts through the board's copper, kept in the file and used by heattran; K/W of 0 removes it)";
        let (pos, kv) = options(args);
        if let Some(spec) = opt(&kv, "link") {
            for item in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                let w: Vec<&str> = item.split(':').collect();
                let [a, b, th] = w.as_slice() else { return Err(format!("link '{item}': write PART:PART:K/W, for example Q1:RS1:150")) };
                for p in [a, b] {
                    if !self.sch.components.iter().any(|c| !c.kind.is_power_symbol() && c.refdes.eq_ignore_ascii_case(p.trim())) {
                        return Err(format!("no part {}", p.trim()));
                    }
                }
                let theta = sim_core::parse_value(th).filter(|x| *x >= 0.0 && x.is_finite()).ok_or_else(|| format!("link '{item}': '{th}' is not a heat resistance (K/W, 0 removes the link)"))?;
                self.sch.set_thermal_link(a.trim(), b.trim(), theta);
            }
            let all: Vec<String> = self.sch.thermal_links.iter().map(|(a, b, t)| format!("{a}:{b}:{t}")).collect();
            return Ok(obj(json!({"links": all})));
        }
        let [r] = pos.as_slice() else { return Err(usage.into()) };
        let num = |k: &str| opt(&kv, k).map(|s| sim_core::parse_value(s).filter(|x| x.is_finite() && (k == "tc" || *x >= 0.0)).ok_or_else(|| format!("{k}={s} is not a number{}", if k == "tc" { "" } else { " (zero or more)" }))).transpose();
        let (theta, tc, cth, tmax, jc, sink) = (num("theta")?, num("tc")?, num("cth")?, num("tmax")?, num("jc")?, num("sink")?);
        // pairs "a:b,a:b" (Zth stages R:tau, or curve points t:z)
        let pairs = |k: &str| -> Result<Option<Vec<(f64, f64)>>, String> {
            let Some(text) = opt(&kv, k) else { return Ok(None) };
            if text.trim() == "0" {
                return Ok(Some(vec![]));
            }
            text.split(',').map(str::trim).filter(|s| !s.is_empty()).map(|item| {
                let (a, b) = item.split_once(':').ok_or_else(|| format!("{k}: '{item}' is not a pair: write A:B, for example 0.5:1m"))?;
                let (a, b) = (sim_core::parse_value(a.trim()), sim_core::parse_value(b.trim()));
                a.zip(b).filter(|(a, b)| *a > 0.0 && *b > 0.0).ok_or_else(|| format!("{k}: '{item}' needs two numbers above zero"))
            }).collect::<Result<Vec<_>, String>>().map(Some)
        };
        let mut zth: Option<Vec<[f64; 2]>> = pairs("zth")?.map(|v| v.into_iter().map(|(r, t)| [r, t]).collect());
        let mut fit_note = None;
        if let Some(points) = pairs("zthfit")? {
            let (st, err) = sim_core::fit_foster(&points, 4)?;
            fit_note = Some(json!({"stages": st.len(), "rms_error_pct": err * 100.0, "total_k_per_w": st.iter().map(|s| s[0]).sum::<f64>()}));
            zth = Some(st);
        }
        if theta.is_none() && tc.is_none() && cth.is_none() && tmax.is_none() && zth.is_none() && jc.is_none() && sink.is_none() {
            return Err(usage.into());
        }
        let c = self.sch.components.iter_mut().find(|c| !c.kind.is_power_symbol() && c.refdes.eq_ignore_ascii_case(r)).ok_or_else(|| format!("no part {r}"))?;
        if let Some(x) = theta {
            c.theta = x;
        }
        if let Some(x) = tc {
            c.tc_ppm = x;
        }
        if let Some(x) = cth {
            c.cth = x;
        }
        if let Some(x) = tmax {
            c.t_max = x;
        }
        if let Some(z) = zth {
            c.zth = z;
        }
        if let Some(x) = jc {
            c.theta_jc = x;
        }
        if let Some(x) = sink {
            c.heatsink = x;
        }
        let (jc, jc_guess) = sim_core::theta_jc(c);
        Ok(obj(json!({"ref": c.refdes, "theta": c.theta, "theta_jc": jc, "theta_jc_guess": jc_guess, "heatsink_k_per_w": c.heatsink, "theta_with_sink": sim_core::part_theta(c, &Default::default()), "tc_ppm": c.tc_ppm, "cth_j_per_k": c.cth, "t_max_c": c.t_max, "zth": c.zth, "fit": fit_note})))
    }

    /// `heatsweep FROM TO STEP`: the ambient swept with the self-heating (`sim_core::ambient_sweep`): each part's hottest, the ambient
    /// where it reaches its limit, its rise at both ends, and the nets that move most.
    pub fn heatsweep(&mut self, args: &[String]) -> Res {
        let usage = "heatsweep FROM TO STEP [guess=NET=V,...]   (°C: the surrounding temperature swept with the self-heating, e.g. heatsweep -20 85 5; each part's hottest, the ambient where it reaches its temperature limit, its rise above the air at both ends; with 'board postlayout on' the board's grid as in 'heat')";
        let (pos, kv) = options(args);
        let nums: Vec<f64> = pos.iter().map(|s| sim_core::parse_value(s).filter(|x| x.is_finite()).ok_or_else(|| format!("'{s}' is not a number\n{usage}"))).collect::<Result<_, _>>()?;
        let [t0, t1, step] = nums.as_slice() else { return Err(usage.into()) };
        let gate = self.erc_gate()?;
        let mut c = self.circuit()?;
        if let Some(spec) = opt(&kv, "guess") {
            apply_guess(&mut c, spec)?;
        }
        let copper = self.heat_copper();
        let g: std::collections::BTreeMap<String, f64> = copper.iter().map(|(k, v)| (k.clone(), v.0)).collect();
        let mut heaters = sim_core::heaters_from_schematic(&self.sch, &c, &g);
        let mutual = self.board_matrix(&c, &heaters, 0.5 * (t0 + t1))?.map(|(names, m)| sim_core::heaters_with_matrix(&mut heaters, &names, &m)).unwrap_or_default();
        let r = sim_core::ambient_sweep(&self.sch, &c, &heaters, &mutual, *t0, *t1, *step).map_err(|e| dc_fail(&c, e))?;
        let parts: Vec<Value> = r
            .worst
            .iter()
            .map(|w| json!({"part": w.name, "hottest_c": w.hottest_c, "at_ambient_c": w.at_ambient, "t_max_c": w.t_max, "t_max_guessed": w.guessed_limit, "over_from_ambient_c": w.over_from, "caution_from_ambient_c": w.caution_from, "rise_low_k": w.rise_low, "rise_high_k": w.rise_high, "derated_power_over": w.derated_over}))
            .collect();
        let nets: Vec<Value> = r.nets.iter().map(|n| json!({"net": n.net, "at_low_v": n.at_low, "at_high_v": n.at_high, "min_v": n.low, "max_v": n.high})).collect();
        let notes: Vec<String> = gate.into_iter().collect();
        Ok(obj(json!({"points": r.points.len(), "parts": parts, "nets": nets, "runaway": r.runaway.as_ref().map(|(t, why)| json!({"ambient_c": t, "why": why})), "notes": notes})))
    }

    /// `heattran TSTOP parts=Q1,RS1 [couple=Q1:RS1:300,...] (else the links kept by `thermal link=`) [tstep=] [temp=27] [uic] [samples=8]`: a transient run whose power becomes
    /// temperatures over time (`sim_core::heat_tran`). Each part is one heat resistance (`thermal REF theta=`) and heat capacity
    /// (`thermal REF cth=`) to the surroundings; `couple=A:B:K/W` ties two parts by a heat resistance of their own.
    pub fn heattran(&mut self, args: &[String]) -> Res {
        let usage = "heattran TSTOP parts=Q1,RS1 [couple=Q1:RS1:300] [tstep=] [temp=27] [uic] [samples=8]   (theta and cth per part: thermal REF theta=K/W cth=J/K)";
        let (pos, kv) = options(args);
        let pos_num = |s: &str| sim_core::parse_value(s).filter(|v| *v > 0.0 && v.is_finite()).ok_or_else(|| format!("'{s}' is not a positive number (write 10m, 20u ...)"));
        let tstop = pos.first().map(|s| pos_num(s)).transpose()?.ok_or(usage)?;
        let tstep = opt(&kv, "tstep").map(pos_num).transpose()?.unwrap_or_else(|| sim_core::auto_tstep(tstop));
        let uic = pos.iter().skip(1).any(|w| w.eq_ignore_ascii_case("uic"));
        let temp = opt(&kv, "temp").map_or(Ok(27.0), |s| sim_core::parse_value(s).filter(|x| x.is_finite()).ok_or_else(|| format!("temp={s} is not a number")))?;
        let samples: usize = opt(&kv, "samples").and_then(|s| s.parse().ok()).unwrap_or(8).clamp(2, 200);
        let refs: Vec<String> = opt(&kv, "parts").ok_or(usage)?.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        // links given here are for this run; without them, the ones kept in the schematic (`thermal link=`)
        let mut links: Vec<sim_core::ThermalLink> = if opt(&kv, "couple").is_none() { self.sch.thermal_links.iter().map(|(a, b, theta)| sim_core::ThermalLink { a: a.clone(), b: b.clone(), theta: *theta }).collect() } else { vec![] };
        for spec in opt(&kv, "couple").map(|c| c.split(',').collect::<Vec<_>>()).unwrap_or_default() {
            let w: Vec<&str> = spec.split(':').collect();
            let [a, b, th] = w.as_slice() else { return Err(format!("couple '{spec}': write PART:PART:K/W, for example Q1:RS1:300")) };
            let theta = sim_core::parse_value(th).filter(|x| *x > 0.0 && x.is_finite()).ok_or_else(|| format!("couple '{spec}': '{th}' is not a heat resistance above zero (K/W)"))?;
            links.push(sim_core::ThermalLink { a: a.trim().to_string(), b: b.trim().to_string(), theta });
        }
        let gate = self.erc_gate()?;
        let c = self.circuit()?;
        let copper = self.heat_copper();
        let g: std::collections::BTreeMap<String, f64> = copper.iter().map(|(k, v)| (k.clone(), v.0)).collect();
        let mut nodes = sim_core::thermal_nodes_from_schematic(&self.sch, &c, &refs, &g)?;
        let heaters = sim_core::heaters_from_schematic(&self.sch, &c, &g);
        if let Some((names, m)) = self.board_matrix(&c, &heaters, temp)? {
            links = sim_core::nodes_with_matrix(&mut nodes, links, &names, &m);
        }
        let r = sim_core::heat_tran(&c, &nodes, &links, temp, &sim_core::TranSettings { tstop, tstep, uic }).map_err(|e| cli_sim_error(e.to_string()))?;
        self.last_tran = Some(r.tran.clone());
        self.last_heat_power = Some((r.time.clone(), r.parts.iter().map(|p| (p.name.clone(), p.power.clone())).collect()));
        let n = r.time.len();
        let at = |k: usize| k * (n - 1) / (samples - 1);
        let checks = sim_core::check_peak_temperatures(&self.sch, &r.parts, temp);
        let check_of = |name: &str| checks.iter().find(|c| c.name == name);
        let level = |l: Option<sim_core::RatingLevel>| match l {
            Some(sim_core::RatingLevel::Over) => "over",
            Some(sim_core::RatingLevel::Caution) => "caution",
            None => "ok",
        };
        let rows: Vec<Value> = r
            .parts
            .iter()
            .map(|p| {
                let series: Vec<Value> = (0..samples).map(|k| json!([r.time[at(k)], p.power[at(k)], p.rise[at(k)]])).collect();
                json!({"part": p.name, "theta_k_per_w": p.theta, "own_theta_k_per_w": self.own_theta(&p.name), "board_copper_mm2": copper.get(&p.name).map(|c| c.1), "cth_j_per_k": p.cth, "tau_s": p.theta * p.cth,
                    "power_peak_w": p.power_peak, "power_mean_w": p.power_mean,
                    "rise_peak_k": p.rise_peak, "peak_at_s": p.peak_time, "rise_end_k": p.rise_end,
                    "temp_peak_c": temp + p.rise_peak, "added_by_neighbours_k": p.rise_peak - p.rise_alone,
                    "working_temp_c": p.temp_working_c, "change_ppm": p.change * 1e6,
                    "steady_state_rise_k": p.power_peak * p.theta, "samples_time_power_rise": series,
                    "heat_stages": p.stages,
                    "t_max_c": check_of(&p.name).map(|c| c.limit.t_max), "t_max_from": check_of(&p.name).map(|c| format!("{:?}", c.limit.source).to_lowercase()), "margin_at_peak_k": check_of(&p.name).map(|c| c.margin_k), "derated_power_w": check_of(&p.name).and_then(|c| c.p_derated), "temperature": check_of(&p.name).map(|c| level(c.level))})
            })
            .collect();
        let mut notes = vec!["each part is one heat capacity and one heat resistance to the surroundings: a rough picture, not a field solution".to_string(), copper_note(&copper, self.board.is_some(), self.post_layout)];
        let no_cap: Vec<&str> = r.parts.iter().filter(|p| p.cth == 0.0).map(|p| p.name.as_str()).collect();
        if !no_cap.is_empty() {
            notes.push(format!("{} has no heat capacity (thermal REF cth=J/K), so it follows its power at once: the rise is the steady-state value, too high for a short pulse", no_cap.join(", ")));
        }
        if links.is_empty() && r.parts.len() > 1 {
            notes.push("no couple=: the parts do not warm each other".to_string());
        }
        notes.push("a resistor's value and a transistor's or diode's model follow the temperature the part had on average while it dissipated (the model's own temperature rules: a junction gives up about 2 mV per K, a MOSFET threshold about 2 mV per K), so the circuit is run again at that temperature".to_string());
        // what the warming moved: the node voltages of the first (cold) run against the last
        let (t_at, moved) = r.moved(8);
        let nets: Vec<Value> = moved.iter().map(|(n, a, b)| json!({"signal": n, "cold": a, "warm": b, "shift": b - a})).collect();
        notes.extend(gate);
        let problems: Vec<Value> = checks.iter().filter(|c| c.level.is_some()).map(|c| json!({"part": c.name, "level": level(c.level), "message": c.message, "hint": c.hint})).collect();
        Ok(obj(json!({"ambient_c": temp, "tstop": tstop, "rounds": r.rounds, "parts": rows, "temperature_problems": problems, "moved_at_s": t_at, "moved": nets, "notes": notes})))
    }

    /// `heat [temp=27] [top=10]`: the DC point with the resistors, diodes and transistors warmed by their own power (`sim_core::self_heat`): per part the power,
    /// the rise, the temperature and the change of its value, and the nodes that move because of it.
    pub fn heat(&mut self, args: &[String]) -> Res {
        let gate = self.erc_gate()?;
        let (_, kv) = options(args);
        let num = |k: &str, d: f64| opt(&kv, k).map_or(Ok(d), |s| sim_core::parse_value(s).filter(|x| x.is_finite()).ok_or_else(|| format!("{k}={s} is not a number")));
        let (temp, top) = (num("temp", 27.0)?, num("top", 10.0)? as usize);
        let mut c = self.circuit()?;
        if let Some(spec) = opt(&kv, "guess") {
            apply_guess(&mut c, spec)?;
        }
        let copper = self.heat_copper();
        let g: std::collections::BTreeMap<String, f64> = copper.iter().map(|(k, v)| (k.clone(), v.0)).collect();
        let mut heaters = sim_core::heaters_from_schematic(&self.sch, &c, &g);
        let on_grid = self.board_matrix(&c, &heaters, temp)?;
        let mutual = on_grid.as_ref().map(|(names, m)| sim_core::heaters_with_matrix(&mut heaters, names, m)).unwrap_or_default();
        let grid_parts: Vec<String> = on_grid.map(|(n, _)| n).unwrap_or_default();
        let first = sim_core::self_heat_coupled(&c, &heaters, temp, &mutual).map_err(|e| dc_fail(&c, e))?;
        // with the board's grid, its copper is as warm as the map says: once more on the warmer wiring (cold stays the 25 °C copper)
        let mut wiring_warm = vec![];
        let r = match self.warm_wiring(&first.parts, temp, !grid_parts.is_empty(), &first.warm) {
            Some((wiring, warm)) if warm.iter().any(|w| (w.factor - 1.0).abs() > 1e-4) => {
                let (_, couplings) = self.layout_parts();
                let c2 = circuit_of(&self.sch, &wiring, &couplings)?;
                let mut again = sim_core::self_heat_coupled(&c2, &heaters, temp, &mutual).map_err(|e| dc_fail(&c2, e))?;
                again.cold = first.cold.clone();
                wiring_warm = warm;
                wiring_warm.sort_by(|a, b| b.temp_c.total_cmp(&a.temp_c));
                again
            }
            _ => first,
        };
        self.last_dc = Some(r.warm.clone());
        let mut parts: Vec<&sim_core::HeatedPart> = r.parts.iter().filter(|p| p.power > 1e-9).collect();
        parts.sort_by(|a, b| b.rise.total_cmp(&a.rise));
        let owned: Vec<sim_core::HeatedPart> = parts.iter().map(|p| (*p).clone()).collect();
        let checks = sim_core::check_temperatures(&self.sch, &owned, temp);
        let check_of = |name: &str| checks.iter().find(|c| c.name == name);
        let level = |l: Option<sim_core::RatingLevel>| match l {
            Some(sim_core::RatingLevel::Over) => "over",
            Some(sim_core::RatingLevel::Caution) => "caution",
            None => "ok",
        };
        let rows: Vec<Value> = parts
            .iter()
            .take(top)
            .map(|p| json!({"t_max_c": check_of(&p.name).map(|c| c.limit.t_max), "t_max_from": check_of(&p.name).map(|c| format!("{:?}", c.limit.source).to_lowercase()), "margin_k": check_of(&p.name).map(|c| c.margin_k), "derated_power_w": check_of(&p.name).and_then(|c| c.p_derated), "temperature": check_of(&p.name).map(|c| level(c.level)), "part": p.name, "power_w": p.power, "rise_k": p.rise, "temp_c": p.temp_c, "change_ppm": p.change * 1e6, "theta_k_per_w": p.theta, "own_theta_k_per_w": self.own_theta(&p.name), "theta_jc_k_per_w": self.case_of(&p.name).map(|c| c.0), "heatsink_k_per_w": self.case_of(&p.name).map(|c| c.1), "board_copper_mm2": copper.get(&p.name).map(|c| c.1), "tc_ppm_per_k": p.tc1 * 1e6, "junctions": p.junctions, "junction_cold_v": p.junction_v.map(|v| v.0), "junction_warm_v": p.junction_v.map(|v| v.1), "board_grid": grid_parts.contains(&p.name), "from_others_k": p.from_others}))
            .collect();
        let mut moved: Vec<(String, f64, f64)> = r.warm.voltages.iter().filter(|(n, _)| !n.contains('.')).filter_map(|(n, v)| r.cold.voltage(n).map(|c0| (n.clone(), c0, *v))).filter(|(_, a, b)| (a - b).abs() > 1e-12).collect();
        moved.sort_by(|a, b| (b.2 - b.1).abs().total_cmp(&(a.2 - a.1).abs()));
        let nodes: Vec<Value> = moved.iter().take(top).map(|(n, a, b)| json!({"net": n, "cold_v": a, "warm_v": b, "shift_v": b - a})).collect();
        let mut notes = vec![];
        let hot: Vec<&str> = parts.iter().filter(|p| p.tc1 == 0.0 && p.tc2 == 0.0 && p.junctions == 0).map(|p| p.name.as_str()).collect();
        if !hot.is_empty() {
            notes.push(format!("{} warm up but have no temperature coefficient, so their value does not change (set one: thermal REF tc=PPM): the rise is still real, for the neighbours and the thermal EMF", hot.join(", ")));
        }
        notes.push("own_theta is a guess from the package unless set (thermal REF theta=K/W): the heat resistance from the resistor to the air on the smallest pads".to_string());
        notes.push(copper_note(&copper, self.board.is_some(), self.post_layout));
        if checks.iter().any(|c| c.limit.source == sim_core::LimitSource::Guess) {
            notes.push("t_max_from 'guess' is a typical limit for the kind of part (chip resistor 155 C derated from 70 C, diode/transistor 150 C, LED 100 C, IC 125 C): set the data sheet's with thermal REF tmax=C".to_string());
        }
        notes.extend(gate);
        let problems: Vec<Value> = checks.iter().filter(|c| c.level.is_some()).map(|c| json!({"part": c.name, "level": level(c.level), "message": c.message, "hint": c.hint})).collect();
        let warm_rows: Vec<Value> = wiring_warm.iter().take(top).map(|w| json!({"pad": format!("{}.{}", w.refdes, w.pin), "net": w.net, "temp_c": w.temp_c, "resistance_factor": w.factor})).collect();
        Ok(obj(json!({"ambient_c": temp, "rounds": r.rounds, "parts": rows, "temperature_problems": problems, "nodes": nodes, "wiring_warm": warm_rows, "notes": notes})))
    }

    /// The points of the pins `REF.PIN` stands for (a pin name shared by several pins means all of them).
    fn pin_points(&self, spec: &str) -> Result<Vec<Pt>, String> {
        let (r, p) = spec.split_once('.').ok_or_else(|| format!("'{spec}': write REF.PIN, for example R1.1 or U1.GND"))?;
        // the placements REF stands for: one unit by its drawn name (U1B.IN+), else every unit of the part (Q6)
        let parts = |want: &dyn Fn(&sch_core::Component) -> bool| -> Vec<&sch_core::Component> { self.sch.components.iter().filter(|c| !c.kind.is_power_symbol() && want(c)).collect() };
        let mut cands = parts(&|c| c.unit().1 > 1 && c.shown_refdes().eq_ignore_ascii_case(r));
        if cands.is_empty() {
            cands = parts(&|c| c.refdes.eq_ignore_ascii_case(r));
        }
        let c = *cands.first().ok_or_else(|| format!("no part {r}"))?;
        // a pin number in any of them; a pin name in the first that has it (unit A before B)
        for c in &cands {
            if let Some((_, pt)) = c.pins().iter().find(|(n, _)| n.eq_ignore_ascii_case(p)) {
                return Ok(vec![*pt]);
            }
        }
        let by_name: Vec<Pt> = cands
            .iter()
            .map(|c| {
                let pins = c.pins();
                pins_named(c, p).iter().filter_map(|n| pins.iter().find(|(m, _)| m == n).map(|(_, pt)| *pt)).collect::<Vec<Pt>>()
            })
            .find(|v| !v.is_empty())
            .unwrap_or_default();
        if by_name.is_empty() {
            let have: Vec<String> = pin_names(c).iter().map(|(n, name)| if n == name { n.clone() } else { format!("{n}={name}") }).collect();
            return Err(format!("{} has no pin '{p}' (pins: {})", c.refdes, have.join(" ")));
        }
        Ok(by_name)
    }

    /// `nc REF.PIN [REF.PIN ...] [remove=1]`: pins left unconnected on purpose (ERC does not ask about them); no pins: the list. A pin name
    /// shared by several pins marks all of them.
    pub fn nc(&mut self, args: &[String]) -> Res {
        let remove = args.iter().any(|a| a == "remove=1");
        let specs: Vec<&String> = args.iter().filter(|a| !a.contains('=')).collect();
        let mut changed = vec![];
        for spec in specs {
            let (r, p) = spec.split_once('.').ok_or_else(|| format!("'{spec}': write REF.PIN, for example U8.3 or U8.TEMP"))?;
            let c = self.sch.components.iter().find(|c| !c.kind.is_power_symbol() && c.refdes.eq_ignore_ascii_case(r)).ok_or_else(|| format!("no part {r}"))?;
            let refdes = c.refdes.clone();
            let numbers: Vec<String> = {
                let pins = c.pins();
                if let Some((n, _)) = pins.iter().find(|(n, _)| n.eq_ignore_ascii_case(p)) {
                    vec![n.clone()]
                } else {
                    let by_name = pins_named(c, p);
                    if by_name.is_empty() {
                        return Err(format!("{refdes} has no pin '{p}'"));
                    }
                    by_name
                }
            };
            for n in numbers {
                if self.sch.set_no_connect(&refdes, &n, !remove) {
                    changed.push(format!("{refdes}.{n}"));
                }
            }
        }
        let all: Vec<String> = self.sch.no_connects.iter().map(|(r, p)| format!("{r}.{p}")).collect();
        Ok(obj(json!({"changed": changed, "no_connects": all})))
    }

    /// `companions REF`: the parts the library part's data sheet recommends beside it (its input and output capacitors), placed next
    /// to their pins and wired (`Schematic::place_companions`).
    pub fn companions(&mut self, args: &[String]) -> Res {
        let r = args.first().ok_or("companions REF")?;
        let i = self.sch.components.iter().position(|c| c.refdes.eq_ignore_ascii_case(r)).ok_or_else(|| format!("no part {r}"))?;
        let list: Vec<Value> = self.sch.components[i].custom.as_ref().map_or(vec![], |s| s.companions.iter().map(|c| json!({"name": c.name, "kind": c.kind, "value": c.value, "pin": c.pin, "to": c.to, "note": c.note})).collect());
        let placed = self.sch.place_companions(i)?;
        Ok(obj(json!({"placed": placed, "companions": list})))
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

    /// What the simulation leaves out or only guesses (`sim_core::model_coverage`): the parts without a model, the general-purpose ones,
    /// the library models with caveats and the test sources.
    pub fn coverage(&self) -> Res {
        let c = sim_core::model_coverage(&self.sch);
        let list = |v: &[sim_core::PartNote]| -> Vec<Value> { v.iter().map(|p| json!({"part": p.refdes, "value": p.value, "notes": p.notes})).collect() };
        Ok(obj(json!({
            "clean": c.is_clean(),
            "total": c.total,
            "ignored": list(&c.ignored),
            "generic": list(&c.generic),
            "caveats": list(&c.caveats),
            "test_sources": list(&c.test_sources),
            "hint": "ignored = left out of the calculation (no model); generic = built-in standard parameters, not the part number's data; caveats = what the library model leaves out; test_sources = for the simulation only",
        })))
    }

    /// The counts of `coverage`, for the answers of the analyses: how much of the result rests on parts that are left out or only guessed.
    pub fn model_gaps(&self) -> Value {
        let c = sim_core::model_coverage(&self.sch);
        json!({"ignored": c.ignored.iter().map(|p| p.refdes.clone()).collect::<Vec<_>>(), "generic": c.generic.len(), "caveats": c.caveats.len(), "see": "coverage"})
    }

    /// `tidy [REF ...]`: the sheet laid out again (`sch_core::tidy`): every part, or the parts named (below the others).
    pub fn tidy(&mut self, args: &[String]) -> Res {
        let only: std::collections::BTreeSet<String> = args.iter().map(|a| a.to_string()).collect();
        let (s, r) = sch_core::tidy::tidy(&self.sch, (!only.is_empty()).then_some(&only))?;
        self.sch = s;
        Ok(obj(json!({"moved": r.moved, "columns": r.columns, "wired": r.wired})))
    }

    pub fn erc(&self) -> Res {
        let mut probs = sch_core::erc(&self.sch);
        probs.extend(sch_core::pindefs::firmware_problems(&self.sch, None));
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
    pub(crate) fn layout_parts(&self) -> (Vec<sim_core::WiringR>, Vec<sim_core::WiringK>) {
        match (&self.board, self.post_layout) {
            (Some(b), true) => post_layout_parts(b),
            _ => (vec![], vec![]),
        }
    }

    /// A part's own θ (K/W, smallest pads): what it was given, or the guess from its package.
    fn own_theta(&self, refdes: &str) -> Option<f64> {
        self.sch.flat().components.iter().find(|c| c.refdes == refdes).map(sim_core::own_theta)
    }

    /// A part's θjc (K/W, given or the guess) and heat sink (K/W, 0 = none).
    fn case_of(&self, refdes: &str) -> Option<(f64, f64)> {
        self.sch.flat().components.iter().find(|c| c.refdes == refdes).map(|c| (sim_core::theta_jc(c).0, c.heatsink))
    }

    /// The board copper beside each part's θ, by reference: (W/K, mm² beyond its pads); empty unless `board postlayout on`.
    /// With the board carried over (`board postlayout on`), the parts that give off heat on the board's grid: (parts, matrix K/W).
    #[allow(clippy::type_complexity)]
    fn board_matrix(&self, c: &sim_core::Circuit, heaters: &[sim_core::Heater], ambient: f64) -> Result<Option<(Vec<String>, Vec<Vec<f64>>)>, String> {
        let (Some(b), true) = (&self.board, self.post_layout) else { return Ok(None) };
        let parts = sim_core::board_heaters(&self.sch, c, heaters, |n| b.pads.iter().any(|p| p.refdes == n)).map_err(|e| e.to_string())?;
        if parts.is_empty() {
            return Ok(None);
        }
        let sources: Vec<pcb_core::thermal::HeatSource> = parts.iter().map(|p| pcb_core::thermal::HeatSource::new(&p.name, p.power, p.own_theta, p.theta_jc, p.heatsink)).collect();
        let spec = pcb_core::thermal::ThermalSpec { ambient_c: ambient, ..Default::default() };
        let (r, _) = pcb_core::thermal::thermal_matrix(b, &sources, b.stackup.copper_um, &spec)?;
        Ok(Some((parts.into_iter().map(|p| p.name).collect(), r)))
    }

    /// The board's wiring as warm as its map at the parts' powers (each pad's way at its mean temperature), and how warm each is; `None`
    /// unless the board is carried over and parts are on its grid.
    fn warm_wiring(&self, parts: &[sim_core::HeatedPart], ambient: f64, on_grid: bool, dc: &sim_core::DcResult) -> Option<(Vec<sim_core::WiringR>, Vec<pcb_core::thermal::WarmWiring>)> {
        let (Some(b), true, true) = (&self.board, self.post_layout, on_grid) else { return None };
        let flat = self.sch.flat();
        let sources: Vec<pcb_core::thermal::HeatSource> = parts
            .iter()
            .filter(|p| p.power > 1e-9)
            .filter_map(|p| {
                let c = flat.components.iter().find(|c| c.refdes == p.name)?;
                Some(pcb_core::thermal::HeatSource::new(&p.name, p.power, sim_core::own_theta(c), sim_core::theta_jc(c).0, c.heatsink))
            })
            .collect();
        let spec = pcb_core::thermal::ThermalSpec { ambient_c: ambient, ..Default::default() };
        let series = pcb_core::irdrop::wiring_resistances(b, &pcb_core::irdrop::IrSpec::default());
        // the wiring's own I²R, along its way (shared over the pads of a pin)
        let traces: Vec<pcb_core::thermal::TraceHeat> = series
            .iter()
            .filter_map(|s| {
                let e = dc.element(&format!("{}.{}.RW", s.refdes, s.pin))?;
                let ways = series.iter().filter(|x| x.refdes == s.refdes && x.pin == s.pin).count().max(1);
                (e.power > 0.0).then(|| pcb_core::thermal::TraceHeat { path: s.path.clone(), power: e.power / ways as f64 })
            })
            .collect();
        let map = pcb_core::thermal::thermal_map_with(b, &sources, &traces, b.stackup.copper_um, &spec).ok()?;
        let warm = pcb_core::thermal::wiring_heat_factors(b, &series, &map);
        let (wiring, _) = self.layout_parts();
        let wiring = wiring
            .into_iter()
            .map(|w| {
                let f = warm.iter().find(|x| x.refdes == w.refdes && x.pin == w.pin).map_or(1.0, |x| x.factor);
                sim_core::WiringR { ohm: w.ohm * f, ..w }
            })
            .collect();
        Some((wiring, warm))
    }

    pub(crate) fn heat_copper(&self) -> std::collections::BTreeMap<String, (f64, f64)> {
        match (&self.board, self.post_layout) {
            (Some(b), true) => pcb_core::heat_copper::conductances(b, pcb_core::irdrop::IrSpec::default().thickness_um),
            _ => Default::default(),
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
        // temp=: the operating point with every part at that temperature (°C; the models' own terms and the semiconductors)
        if let Some(t) = opt(&kv, "temp") {
            let t = sim_core::parse_value(t).filter(|x| x.is_finite()).ok_or_else(|| format!("temp={t} is not a number"))?;
            c = c.at_temperature(t);
        }
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
        let mut answer = json!({"circuit_ok": circuit_ok, "voltages": volts, "elements": elems, "ratings": ratings, "internal_elements_hidden": hidden, "internal_nodes_hidden": hidden_nodes, "models": self.model_gaps(), "post_layout": self.post_layout && self.board.is_some(), "solver": {"strategy": r.strategy, "iterations": r.iterations, "seconds": seconds}, "warnings": warnings});
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
        // `record=probes` keeps only the probed signals (a large circuit would store a value of every node and element at every step);
        // `record=all` keeps everything; without it a circuit big enough that the record would pass ten million values is cut to the probes
        let est_values = (c.names.len() + c.elements.len()) as f64 * (tstop / tstep).max(1.0);
        let (restrict, auto) = match (opt(&kv, "record").map(lower).as_deref(), probes.is_empty()) {
            (Some("probes"), true) => return Err("record=probes needs probe=V(net),I(part) to say what to keep".to_string()),
            (Some("probes"), false) => (true, false),
            (Some("all"), _) => (false, false),
            (None, probes_empty) => (!probes_empty && est_values > 1.0e7, true),
            (Some(other), _) => return Err(format!("record: '{other}' is not 'all' or 'probes'")),
        };
        let keep = |name: &str| {
            let n = lower(name);
            probes.iter().any(|p| n == *p || n.contains(p.as_str()))
        };
        let r = sim_core::transient_with(&c, &sim_core::TranSettings { tstop, tstep, uic }, if restrict { Some(&keep) } else { None }).map_err(|e| cli_sim_error(e.to_string()))?;
        let restricted_note: Option<String> = restrict.then(|| {
            if auto {
                "記録は probe= の信号だけです (回路が大きいので、自動で絞りました。全部を記録するには record=all を付けてください)。probe にない信号を使う spec・settle・定格の注意は出ません".to_string()
            } else {
                "記録は probe= の信号だけです (record=probes)。probe にない信号を使う spec・settle・定格の注意は出ません".to_string()
            }
        });
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
                if let Some((period, duty)) = periodicity(&r.time, &s.values) {
                    o["period"] = json!(period);
                    o["frequency"] = json!(1.0 / period);
                    o["duty"] = json!(duty);
                }
                (s.name.clone(), o)
            })
            .collect();
        let (ratings, notes) = self.tran_notes(&r);
        let mut answer = json!({"circuit_ok": circuit_ok, "points": n, "tstop": tstop, "signals": sigs, "signals_not_shown": chosen.len().saturating_sub(cap), "models": self.model_gaps(), "ratings": ratings, "post_layout": self.post_layout && self.board.is_some(), "warnings": r.warnings.iter().cloned().chain(gate).chain(notes).chain(restricted_note).collect::<Vec<_>>()});
        if !r.timing.violations.is_empty() || r.timing.tightest.is_some() {
            let kind = |k: sim_core::TimingKind| match k {
                sim_core::TimingKind::Setup => "setup",
                sim_core::TimingKind::Hold => "hold",
                sim_core::TimingKind::PulseWidth => "pulse_width",
            };
            let violations: Vec<Value> = r
                .timing
                .violations
                .iter()
                .map(|v| json!({"flip_flop": v.cell, "kind": kind(v.kind), "count": v.count, "first_at_s": v.first_time, "worst_margin_s": v.worst_margin, "required_s": v.required}))
                .collect();
            let mut timing = json!({"violations": violations});
            if let Some((cell, margin, interval)) = &r.timing.tightest {
                // how fast the clock could be if only this flip-flop's data path limited it (an estimate: the data path is taken as it was)
                let max_hz = (interval.is_finite() && *interval > *margin).then(|| 1.0 / (interval - margin));
                timing["tightest"] = json!({"flip_flop": cell, "margin_to_setup_s": margin, "clock_interval_s": if interval.is_finite() { json!(interval) } else { Value::Null }, "max_clock_hz_estimate": max_hz});
            }
            if let Some(o) = answer.as_object_mut() {
                o.insert("timing".into(), timing);
            }
        }
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

/// What the heat answers say about the board's copper.
fn copper_note(copper: &std::collections::BTreeMap<String, (f64, f64)>, board: bool, post_layout: bool) -> String {
    if !(board && post_layout) {
        "theta is the part's own (smallest pads): with a board loaded, `board postlayout on` adds the copper joined to each part's pins within 20 mm (estimate fitted to one SOT-223 datasheet curve)".to_string()
    } else if copper.is_empty() {
        "postlayout is on but no part has copper beyond its pads, so theta is the part's own".to_string()
    } else {
        "theta includes the board copper joined to each part's pins within 20 mm (board_copper_mm2 beyond its pads, 35 um copper): an estimate fitted to one SOT-223 datasheet curve, own_theta is without it".to_string()
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
