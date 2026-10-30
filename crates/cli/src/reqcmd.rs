//! `req`: the table of requirements of the design (net roles, parts that belong together, distances to keep). One table for all the
//! sheets; the board import takes it (`board import`), and `board req` checks the placement against it.

use crate::session::{obj, opt, options, Session};
use crate::Res;
use design_spec::{Bypass, KeepApart, LinkWidth, NetRule, PlaceGroup, ROLES};
use serde_json::{json, Value};

fn names(v: &str) -> Vec<String> {
    v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

fn number(kv: &[(String, String)], key: &str) -> Result<Option<f64>, String> {
    match opt(kv, key) {
        None => Ok(None),
        Some(v) => v.parse::<f64>().ok().filter(|x| x.is_finite() && *x >= 0.0).map(Some).ok_or_else(|| format!("{key}={v}: a number of 0 or more")),
    }
}

/// `links=CB1-U7:0.6,U7-U4:0.3`: the wire between two parts and its width (mm).
fn links(v: &str) -> Result<Vec<LinkWidth>, String> {
    let mut out = vec![];
    for item in v.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let bad = || format!("links={item}: write PART-PART:WIDTH, e.g. links=CB1-U7:0.6,U7-U4:0.3");
        let (pair, w) = item.split_once(':').ok_or_else(bad)?;
        let (a, b) = pair.split_once('-').ok_or_else(bad)?;
        let width = w.parse::<f64>().ok().filter(|x| x.is_finite() && *x >= 0.0).ok_or_else(bad)?;
        out.push(LinkWidth { a: a.trim().into(), b: b.trim().into(), width });
    }
    Ok(out)
}

/// The reason of a row (kept in `note`): `reason=` (or `note=`) and any words left over after the name (a sentence needs no quotes).
fn note(pos: &[String], kv: &[(String, String)]) -> Option<String> {
    let mut words: Vec<&str> = opt(kv, "reason").into_iter().chain(opt(kv, "note")).collect();
    words.extend(pos.iter().skip(2).map(|s| s.as_str()));
    if words.is_empty() { None } else { Some(words.join(" ")) }
}

impl Session {
    /// `req waive group|apart|bypass NAME [item=CAP] reason=TEXT [remove=1]`: accept a finding of the check (the row `NAME`; for a bypass row,
    /// `item=` names one capacitor, else all of them). A new one needs its reason: the point of the table is that the next reader knows why.
    fn req_waive(&mut self, pos: &[String], kv: &[(String, String)]) -> Res {
        let usage = "req waive group|apart|bypass NAME [item=CAP] reason=TEXT [remove=1]";
        let kind = pos.get(1).map(|s| s.to_ascii_lowercase()).filter(|k| design_spec::WAIVER_KINDS.iter().any(|(x, _)| x == k)).ok_or(usage)?;
        let name = pos.get(2).ok_or(usage)?.clone();
        let item = opt(kv, "item").unwrap_or("").to_string();
        let mut words: Vec<&str> = opt(kv, "reason").into_iter().chain(opt(kv, "note")).collect();
        words.extend(pos.iter().skip(3).map(|s| s.as_str()));
        let reason = words.join(" ");
        let req = &mut self.sch.requirements;
        let same = |w: &design_spec::Waiver| w.kind == kind && w.name == name && w.item == item;
        if opt(kv, "remove") == Some("1") {
            req.waivers.retain(|w| !same(w));
            return self.req_list();
        }
        match req.waivers.iter_mut().find(|w| same(w)) {
            Some(w) => {
                if !reason.is_empty() {
                    w.note = reason;
                }
            }
            None => {
                if reason.trim().is_empty() {
                    return Err(format!("a waiver needs its reason (what the next reader must know): {usage}"));
                }
                req.waivers.push(design_spec::Waiver { kind, name, item, note: reason });
            }
        }
        self.req_list()
    }

    fn req_list(&self) -> Res {
        let r = &self.sch.requirements;
        Ok(obj(json!({
            "net_rules": r.net_rules.iter().map(|n| json!({"name": n.name, "nets": n.nets, "role": n.role, "width": n.width, "clearance": n.clearance, "current": n.current, "order": n.order, "vias": n.vias, "layers": n.layers, "max_length": n.max_length, "links": n.links.iter().map(|l| json!({"a": l.a, "b": l.b, "width": l.width})).collect::<Vec<Value>>(), "note": n.note})).collect::<Vec<Value>>(),
            "groups": r.groups.iter().map(|g| json!({"name": g.name, "parts": g.parts, "max_dist": g.max_dist, "note": g.note})).collect::<Vec<Value>>(),
            "apart": r.apart.iter().map(|a| json!({"name": a.name, "a": a.a, "b": a.b, "min_dist": a.min_dist, "note": a.note})).collect::<Vec<Value>>(),
            "bypass": r.bypass.iter().map(|b| json!({"ic": b.ic, "caps": b.caps, "note": b.note})).collect::<Vec<Value>>(),
            "measures": r.measures.iter().map(|m| json!({"name": m.name, "what": m.what, "pads": m.pads, "op": if m.is_min() { "min" } else { "max" }, "limit": m.limit, "reach": m.reach, "except": m.except, "note": m.note})).collect::<Vec<Value>>(),
            "waivers": r.waivers.iter().map(|w| json!({"kind": w.kind, "name": w.name, "item": w.item, "note": w.note})).collect::<Vec<Value>>(),
            "problems": self.sch.requirement_problems(),
            "unexplained": r.unexplained(),
        })))
    }

    /// `req suggest [apply=1]` (what the DC analysis says about the currents); `req` (list); `req net NAME [nets=A,B,GN*] [role=] [width=mm] [clearance=mm] [current=A] [links=A-B:mm,..] [reason=TEXT]`;
    /// `req group NAME [parts=R1,R2] [max=mm] [reason=TEXT]`; `req apart NAME [a=..] [b=..] [min=mm] [reason=TEXT]`; add `remove=1` to delete a row.
    /// A row that exists is changed only in what is given; a new row needs its names.
    pub fn req(&mut self, args: &[String]) -> Res {
        let (pos, kv) = options(args);
        let Some(kind) = pos.first().map(|s| s.to_ascii_lowercase()) else { return self.req_list() };
        if kind == "suggest" {
            return self.req_suggest(opt(&kv, "apply") == Some("1"), opt(&kv, "bypass") == Some("1"));
        }
        if kind == "waive" {
            return self.req_waive(&pos, &kv);
        }
        let name = pos.get(1).ok_or("req net|group|apart|bypass|waive NAME ...")?.clone();
        let remove = opt(&kv, "remove") == Some("1");
        let text = note(&pos, &kv);
        let req = &mut self.sch.requirements;
        match kind.as_str() {
            "net" => {
                let k = req.net_rules.iter().position(|r| r.name == name);
                if remove {
                    req.net_rules.retain(|r| r.name != name);
                    return self.req_list();
                }
                let (width, clearance, current) = (number(&kv, "width")?, number(&kv, "clearance")?, number(&kv, "current")?);
                let new_links = opt(&kv, "links").map(links).transpose()?;
                // the properties a role would set: the place in the order of routing (0-9; `order=role` gives it back to the role), vias
                let order = match opt(&kv, "order") {
                    None => None,
                    Some("role") | Some("auto") => Some(None),
                    Some(v) => Some(Some(v.parse::<u8>().ok().filter(|n| *n <= 9).ok_or_else(|| format!("order={v}: a number from 0 to 9, or `role`"))?)),
                };
                let layers: Option<Vec<String>> = match opt(&kv, "layers") {
                    None => None,
                    Some("any") | Some("") => Some(vec![]),
                    Some(v) => {
                        let list = names(v);
                        if let Some(bad) = list.iter().find(|k| pcb_core::model::Side::from_key(k).is_none()) {
                            return Err(format!("layers={bad}: use top, bottom, inner1 ... (comma separated), or `any`"));
                        }
                        Some(list.into_iter().map(|k| k.to_ascii_lowercase()).collect())
                    }
                };
                let max_length = number(&kv, "max_length")?;
                let vias = match opt(&kv, "vias") {
                    None => None,
                    Some("role") => Some(String::new()),
                    Some(v @ ("none" | "allow")) => Some(v.to_string()),
                    Some(v) => return Err(format!("vias={v}: use none, allow or role")),
                };
                if let Some(role) = opt(&kv, "role").filter(|r| !r.is_empty() && *r != "none" && !ROLES.contains(r)) {
                    return Err(format!("role={role}: use power, switching, sense, analog, logic or none"));
                }
                let k = match k {
                    Some(k) => k,
                    None => {
                        let nets = opt(&kv, "nets").map(names).filter(|n| !n.is_empty()).ok_or("req net NAME nets=A,B,GN* [role=] [width=] [clearance=] [current=]  (a new row needs its nets)")?;
                        req.net_rules.push(NetRule { name: name.clone(), nets, ..Default::default() });
                        req.net_rules.len() - 1
                    }
                };
                let r = &mut req.net_rules[k];
                if let Some(v) = opt(&kv, "nets") {
                    r.nets = names(v);
                }
                if let Some(v) = opt(&kv, "role") {
                    r.role = if v == "none" { String::new() } else { v.to_string() };
                }
                if let Some(l) = new_links {
                    r.links = l;
                }
                if let Some(o) = order {
                    r.order = o;
                }
                if let Some(v) = vias {
                    r.vias = v;
                }
                if let Some(l) = layers {
                    r.layers = l;
                }
                if let Some(v) = max_length {
                    r.max_length = v;
                }
                (r.width, r.clearance, r.current) = (width.unwrap_or(r.width), clearance.unwrap_or(r.clearance), current.unwrap_or(r.current));
                if let Some(t) = text {
                    r.note = t;
                }
            }
            "group" => {
                if remove {
                    req.groups.retain(|g| g.name != name);
                    return self.req_list();
                }
                let max = number(&kv, "max")?;
                let k = match req.groups.iter().position(|g| g.name == name) {
                    Some(k) => k,
                    None => {
                        let parts = opt(&kv, "parts").map(names).filter(|n| n.len() >= 2).ok_or("req group NAME parts=R1,R2,.. [max=mm]  (a new group needs two parts or more)")?;
                        req.groups.push(PlaceGroup { name: name.clone(), parts, ..Default::default() });
                        req.groups.len() - 1
                    }
                };
                let g = &mut req.groups[k];
                if let Some(v) = opt(&kv, "parts") {
                    g.parts = names(v);
                }
                g.max_dist = max.unwrap_or(g.max_dist);
                if let Some(t) = text {
                    g.note = t;
                }
            }
            "measure" => {
                // req measure NAME what=length|vias|fork|tap pads=A.1,B.1[,C.1] max=N | min=N [reason=...]
                if remove {
                    req.measures.retain(|m| m.name != name);
                    return self.req_list();
                }
                let (max, min, reach) = (number(&kv, "max")?, number(&kv, "min")?, number(&kv, "reach")?);
                let what = opt(&kv, "what").map(str::to_ascii_lowercase);
                let pads = opt(&kv, "pads").map(names);
                let k = match req.measures.iter().position(|m| m.name == name) {
                    Some(k) => k,
                    None => {
                        let usage = "req measure NAME what=length|vias|fork|gap|guard|tap|area|width|parallel|thermal_vias|loop|match|ring|resistance|under|overlap|drop|return|stub|balance pads=A.1,B.1[,C.1] max=N|min=N [reach=mm] [except=U1,pads] [reason=...]  (length, vias and width take 2 pads, loop and match take 4 (two ways: from,to,from,to), thermal_vias takes 1 pad, ring takes a pad and the guard nets, resistance takes 2 pads, under takes the two ends of a way and the ground nets, overlap takes 2 sets of nets, drop takes 2 pads (reach= is the current in A), return takes the two ends of a way and the ground nets, stub takes 1 set of nets, balance takes 4 pads (two ways), fork and tap take 3: the start and the two ends / the shunt pad, where the sense wire goes and the other pad of the shunt; area takes 1 pad, reach= is its radius; gap, guard and parallel take 2 nets, reach= is how near; except= leaves the pads of those parts out of a gap or a parallel run, pads = every pad)";
                        let what = what.clone().filter(|w| design_spec::MEASURES.iter().any(|(k, ..)| k == w)).ok_or(usage)?;
                        let need = design_spec::MEASURES.iter().find(|(k, ..)| *k == what).map_or(2, |m| m.3);
                        let pads = pads.clone().filter(|p| p.len() == need).ok_or(usage)?;
                        if max.is_none() && min.is_none() {
                            return Err(usage.into());
                        }
                        req.measures.push(design_spec::Measure { name: name.clone(), what, pads, ..Default::default() });
                        req.measures.len() - 1
                    }
                };
                let m = &mut req.measures[k];
                if let Some(w) = what.filter(|w| design_spec::MEASURES.iter().any(|(k, ..)| k == w)) {
                    m.what = w;
                }
                if let Some(p) = pads {
                    m.pads = p;
                }
                if let Some(v) = reach {
                    m.reach = v;
                }
                if let Some(list) = opt(&kv, "except") {
                    m.except = if list == "-" { vec![] } else { names(list) };
                }
                if let Some(v) = max {
                    (m.op, m.limit) = ("max".into(), v);
                }
                if let Some(v) = min {
                    (m.op, m.limit) = ("min".into(), v);
                }
                if let Some(t) = text {
                    m.note = t;
                }
            }
            "bypass" => {
                if remove {
                    req.bypass.retain(|b| b.ic != name);
                    return self.req_list();
                }
                let caps = opt(&kv, "caps").map(names);
                let k = match req.bypass.iter().position(|b| b.ic == name) {
                    Some(k) => k,
                    None => {
                        let caps = caps.clone().filter(|c| !c.is_empty()).ok_or("req bypass IC caps=C1,C2 [reason=...]  (a new row needs its capacitors)")?;
                        req.bypass.push(Bypass { ic: name.clone(), caps, ..Default::default() });
                        req.bypass.len() - 1
                    }
                };
                let b = &mut req.bypass[k];
                if let Some(c) = caps {
                    b.caps = c;
                }
                if let Some(t) = text {
                    b.note = t;
                }
            }
            "apart" => {
                if remove {
                    req.apart.retain(|a| a.name != name);
                    return self.req_list();
                }
                let min = number(&kv, "min")?;
                let k = match req.apart.iter().position(|a| a.name == name) {
                    Some(k) => k,
                    None => {
                        let (a, b) = (opt(&kv, "a").map(names).filter(|n| !n.is_empty()), opt(&kv, "b").map(names).filter(|n| !n.is_empty()));
                        let (Some(a), Some(b)) = (a, b) else { return Err("req apart NAME a=BKSW,.. b=U8,.. min=mm  (parts or nets on both sides)".into()) };
                        req.apart.push(KeepApart { name: name.clone(), a, b, ..Default::default() });
                        req.apart.len() - 1
                    }
                };
                let a = &mut req.apart[k];
                if let Some(v) = opt(&kv, "a") {
                    a.a = names(v);
                }
                if let Some(v) = opt(&kv, "b") {
                    a.b = names(v);
                }
                a.min_dist = min.unwrap_or(a.min_dist);
                if let Some(t) = text {
                    a.note = t;
                }
            }
            other => return Err(format!("req {other}: use net, group, apart or bypass")),
        }
        self.req_list()
    }

    /// `req suggest [apply=1]`: what the DC analysis says about the table (the current of each net): the empty currents of the rows, and
    /// the big nets that have no row. `apply=1` writes them into the table (what the table states is left alone).
    fn req_suggest(&mut self, apply: bool, bypass: bool) -> Res {
        if bypass {
            let found = self.sch.bypass_suggestions();
            let list: Vec<Value> = found.iter().map(|s| match &s.fill {
                design_spec::Fill::Bypass { ic, cap } => json!({"text": s.text, "ic": ic, "cap": cap}),
                _ => json!({"text": s.text}),
            }).collect();
            if apply {
                for s in &found {
                    s.apply(&mut self.sch.requirements);
                }
            }
            return Ok(obj(json!({"suggestions": list, "applied": apply, "count": found.len()})));
        }
        let c = self.circuit().map_err(|e| format!("req suggest needs a circuit that can be analysed: {e}"))?;
        let dc = sim_core::dc_op(&c).map_err(|e| format!("DC analysis failed: {e}"))?;
        let currents = sim_core::net_currents(&c, &dc);
        let found = design_spec::suggest_from_currents(&self.sch.requirements, &currents);
        let list: Vec<Value> = found.iter().map(|s| json!({"text": s.text, "row": match &s.fill { design_spec::Fill::RowCurrent { row, .. } => json!(row), _ => Value::Null }, "net": match &s.fill { design_spec::Fill::NewRow { net, .. } => json!(net), _ => Value::Null }, "amps": match &s.fill { design_spec::Fill::RowCurrent { amps, .. } | design_spec::Fill::NewRow { amps, .. } => *amps, design_spec::Fill::Bypass { .. } => 0.0 }})).collect();
        if apply {
            for s in &found {
                s.apply(&mut self.sch.requirements);
            }
        }
        Ok(obj(json!({"suggestions": list, "applied": apply, "count": found.len()})))
    }

    /// `board req`: the board's groups and distances checked against the placement.
    pub(crate) fn board_req(&mut self) -> Res {
        let board = self.board_mut()?;
        let rep = pcb_core::requirements::check(board);
        let findings: Vec<Value> = rep
            .findings
            .iter()
            .map(|f| json!({"kind": f.kind.key(), "name": f.name, "message": f.message, "actual_mm": (f.actual * 10.0).round() / 10.0, "item": f.item, "limit_mm": f.limit}))
            .collect();
        // every measure of the table with its value (the ones within the limit too), so that a pass can be read as a number
        let measured: Vec<Value> = pcb_core::requirements::measure_marks(board)
            .iter()
            .map(|m| json!({"name": m.name, "value": (m.value * 100.0).round() / 100.0, "unit": m.unit, "limit": m.limit, "op": if m.is_min { "min" } else { "max" }, "ok": m.ok, "waived": m.waived}))
            .collect();
        let waived: Vec<Value> = rep.waived.iter().map(|(f, why)| json!({"kind": f.kind.key(), "name": f.name, "item": f.item, "message": f.message, "reason": why})).collect();
        Ok(obj(json!({"groups": board.place_groups.len(), "apart": board.keep_apart.len(), "links": board.link_widths.len(), "bypass": board.bypass.iter().map(|b| b.caps.len()).sum::<usize>(), "checked": rep.checked, "violations": findings.len(), "findings": findings, "measured": measured, "waived": waived, "missing": rep.missing})))
    }
}
