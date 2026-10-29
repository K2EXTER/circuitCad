//! `req`: the table of requirements of the design (net roles, parts that belong together, distances to keep). One table for all the
//! sheets; the board import takes it (`board import`), and `board req` checks the placement against it.

use crate::session::{obj, opt, options, Session};
use crate::Res;
use design_spec::{KeepApart, LinkWidth, NetRule, PlaceGroup, ROLES};
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

/// The note of a row: `note=` and any words left over after the name (a sentence needs no quotes).
fn note(pos: &[String], kv: &[(String, String)]) -> Option<String> {
    let mut words: Vec<&str> = opt(kv, "note").into_iter().collect();
    words.extend(pos.iter().skip(2).map(|s| s.as_str()));
    if words.is_empty() { None } else { Some(words.join(" ")) }
}

impl Session {
    fn req_list(&self) -> Res {
        let r = &self.sch.requirements;
        Ok(obj(json!({
            "net_rules": r.net_rules.iter().map(|n| json!({"name": n.name, "nets": n.nets, "role": n.role, "width": n.width, "clearance": n.clearance, "current": n.current, "links": n.links.iter().map(|l| json!({"a": l.a, "b": l.b, "width": l.width})).collect::<Vec<Value>>(), "note": n.note})).collect::<Vec<Value>>(),
            "groups": r.groups.iter().map(|g| json!({"name": g.name, "parts": g.parts, "max_dist": g.max_dist, "note": g.note})).collect::<Vec<Value>>(),
            "apart": r.apart.iter().map(|a| json!({"name": a.name, "a": a.a, "b": a.b, "min_dist": a.min_dist, "note": a.note})).collect::<Vec<Value>>(),
            "problems": self.sch.requirement_problems(),
        })))
    }

    /// `req suggest [apply=1]` (what the DC analysis says about the currents); `req` (list); `req net NAME [nets=A,B,GN*] [role=] [width=mm] [clearance=mm] [current=A] [links=A-B:mm,..] [note TEXT]`;
    /// `req group NAME [parts=R1,R2] [max=mm] [note TEXT]`; `req apart NAME [a=..] [b=..] [min=mm] [note TEXT]`; add `remove=1` to delete a row.
    /// A row that exists is changed only in what is given; a new row needs its names.
    pub fn req(&mut self, args: &[String]) -> Res {
        let (pos, kv) = options(args);
        let Some(kind) = pos.first().map(|s| s.to_ascii_lowercase()) else { return self.req_list() };
        if kind == "suggest" {
            return self.req_suggest(opt(&kv, "apply") == Some("1"));
        }
        let name = pos.get(1).ok_or("req net|group|apart NAME ...")?.clone();
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
            other => return Err(format!("req {other}: use net, group or apart")),
        }
        self.req_list()
    }

    /// `req suggest [apply=1]`: what the DC analysis says about the table (the current of each net): the empty currents of the rows, and
    /// the big nets that have no row. `apply=1` writes them into the table (what the table states is left alone).
    fn req_suggest(&mut self, apply: bool) -> Res {
        let c = self.circuit().map_err(|e| format!("req suggest needs a circuit that can be analysed: {e}"))?;
        let dc = sim_core::dc_op(&c).map_err(|e| format!("DC analysis failed: {e}"))?;
        let currents = sim_core::net_currents(&c, &dc);
        let found = design_spec::suggest_from_currents(&self.sch.requirements, &currents);
        let list: Vec<Value> = found.iter().map(|s| json!({"text": s.text, "row": match &s.fill { design_spec::Fill::RowCurrent { row, .. } => json!(row), _ => Value::Null }, "net": match &s.fill { design_spec::Fill::NewRow { net, .. } => json!(net), _ => Value::Null }, "amps": match &s.fill { design_spec::Fill::RowCurrent { amps, .. } | design_spec::Fill::NewRow { amps, .. } => *amps }})).collect();
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
            .map(|f| json!({"kind": if f.kind == pcb_core::requirements::FindingKind::Group { "group" } else { "apart" }, "name": f.name, "message": f.message, "actual_mm": (f.actual * 10.0).round() / 10.0, "limit_mm": f.limit}))
            .collect();
        Ok(obj(json!({"groups": board.place_groups.len(), "apart": board.keep_apart.len(), "links": board.link_widths.len(), "checked": rep.checked, "violations": findings.len(), "findings": findings, "missing": rep.missing})))
    }
}
