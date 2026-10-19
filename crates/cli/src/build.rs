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
        let [r, v] = args else { return Err("value REF VALUE".into()) };
        let c = self.sch.components.iter_mut().find(|c| c.refdes.eq_ignore_ascii_case(r)).ok_or_else(|| format!("no part {r}"))?;
        if matches!(c.kind, Kind::Resistor | Kind::Capacitor | Kind::Inductor) && sim_core::parse_value(v).is_none() {
            return Err(bad_number(v));
        }
        c.value = v.clone();
        Ok(obj(json!({"ref": c.refdes, "value": c.value})))
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
                json!({"ref": c.refdes, "type": type_name, "value": c.value, "footprint": c.footprint_name(), "pins": pins})
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

    pub(crate) fn circuit(&self) -> Result<sim_core::Circuit, String> {
        let wiring: Vec<sim_core::WiringR> = match (&self.board, self.post_layout) {
            (Some(b), true) => pcb_core::irdrop::wiring_resistances(b, &pcb_core::irdrop::IrSpec::default())
                .into_iter()
                .map(|w| sim_core::WiringR { refdes: w.refdes, pin: w.pin, ohm: w.ohm })
                .collect(),
            _ => vec![],
        };
        sim_core::from_schematic_with_wiring(&self.sch, &wiring).map_err(|es| es.iter().map(|e| format!("{}: {}", e.refdes, e.message)).collect::<Vec<_>>().join("; "))
    }

    pub fn dc(&mut self, args: &[String]) -> Res {
        let gate = self.erc_gate()?;
        let circuit_ok = gate.is_none();
        let full = args.iter().any(|a| a.eq_ignore_ascii_case("full"));
        let c = self.circuit()?;
        let r = sim_core::dc_op(&c).map_err(|e| cli_sim_error(e.to_string()))?;
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
        let (hidden, hidden_nodes) = if full { (0, 0) } else { (internal, internal_nodes) };
        Ok(obj(json!({"circuit_ok": circuit_ok, "voltages": volts, "elements": elems, "internal_elements_hidden": hidden, "internal_nodes_hidden": hidden_nodes, "post_layout": self.post_layout && self.board.is_some(), "warnings": warnings})))
    }

    pub fn tran(&mut self, args: &[String]) -> Res {
        let (pos, kv) = options(args);
        if pos.len() < 2 {
            return Err("tran TSTOP TSTEP [probe=V(out),I(R1)] [samples=6] [uic]   (TSTEP is the largest step; the solver refines it)".into());
        }
        let num = |s: &str| sim_core::parse_value(s).filter(|v| *v > 0.0).ok_or_else(|| format!("'{s}' is not a positive number (write 10m, 20u ...)"));
        let (tstop, tstep) = (num(&pos[0])?, num(&pos[1])?);
        let uic = pos.iter().skip(2).any(|w| w.eq_ignore_ascii_case("uic"));
        let samples: usize = opt(&kv, "samples").and_then(|s| s.parse().ok()).unwrap_or(6).clamp(0, 200);
        let probes: Vec<String> = opt(&kv, "probe").map(|p| p.split(',').map(lower).collect()).unwrap_or_default();
        let gate = self.erc_gate()?;
        let circuit_ok = gate.is_none();
        let c = self.circuit()?;
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
        Ok(obj(json!({"circuit_ok": circuit_ok, "points": n, "tstop": tstop, "signals": sigs, "signals_not_shown": chosen.len().saturating_sub(cap), "post_layout": self.post_layout && self.board.is_some(), "warnings": r.warnings.iter().cloned().chain(gate).collect::<Vec<_>>()})))
    }
}
