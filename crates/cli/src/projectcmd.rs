//! `project check FILE.ccproj` / `project bom FILE.ccproj OUT.csv`: a project of several boards (E5, multi-board) checked link by link
//! (`sch_core::system::check`: the two sides of each connector carry the same signals pin by pin) and its parts counted together. The
//! file is read as it is (format 2: `designs`, `links`; format 1: one board), without changing the session's schematic or board.

use crate::Res;
use crate::session::{obj, opt, options};
use sch_core::Schematic;
use sch_core::system::BoardLink;
use serde_json::{Value, json};

const USAGE: &str = "project new FILE.ccproj | project add FILE.ccproj NAME [SCHEMATIC.schjson] [board=BOARD.pcbjson] [copies=N] | project link FILE.ccproj BOARD.REF BOARD.REF [cable=MM] | project check FILE.ccproj | project bom FILE.ccproj OUT.csv | project step FILE.ccproj OUT.step | project dc FILE.ccproj | project tran FILE.ccproj [tstop=10m] | project ac FILE.ccproj [fstart=10] [fstop=1Meg] | project merge FILE.ccproj OUT.schjson | project alias FILE.ccproj LINK NET_A NET_B | project mirror FILE.ccproj LINK | project cable FILE.ccproj LINK [gap=20] | project harness FILE.ccproj OUT.svg|OUT.csv | project color FILE.ccproj LINK PIN COLOR   (a project of several boards: the links between their connectors checked pin by pin, the parts of all boards counted together, the boards placed in 3D as one STEP, the whole system's operating point pin by pin, its transient and AC sweep summed up pin by pin, the whole system as one schematic; alias, mirror and cable change link LINK (from 1) in the file: two names as one signal, the pins paired the other way round, the second board laid beside the first for a cable; harness draws every cable (the wires pin to pin, their colours, the mating housings and contacts) as SVG or lists its wires as CSV; color sets the wire on pin PIN of link LINK's first connector to black red orange yellow green blue violet grey white brown, or auto)";

/// Lays the second board of link `k` (from 1) beside the first for a cable, in the file: the second board's `place` (the first's kept, or
/// set at the origin) and the link's `cable` length.
fn cable(file: &str, k: &str, gap: f64) -> Result<Value, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    let mut v: Value = serde_json::from_str(&text).map_err(|e| format!("{file}: not a project ({e})"))?;
    let i = k.parse::<usize>().ok().filter(|&i| i >= 1).ok_or_else(|| format!("LINK {k}: the link's number, from 1"))? - 1;
    let link: BoardLink = serde_json::from_value(v.get("links").and_then(|l| l.get(i)).cloned().ok_or_else(|| format!("{file}: no link {k}"))?).map_err(|e| e.to_string())?;
    let designs = v.get("designs").and_then(Value::as_array).ok_or("not a project of several boards (format 2)")?;
    let find = |n: &str| designs.iter().position(|d| d.get("name").and_then(Value::as_str) == Some(n)).ok_or_else(|| format!("no board {n}"));
    let (ia, ib) = (find(&link.a.design)?, find(&link.b.design)?);
    if ia == ib {
        return Err("both ends on the same board".into());
    }
    let board = |i: usize| -> Result<pcb_core::Board, String> { serde_json::from_value(designs[i].get("board").cloned().ok_or("a board without its board")?).map_err(|e| e.to_string()) };
    let base: pcb_core::stack::Place = designs[ia].get("place").cloned().and_then(|p| serde_json::from_value(p).ok()).unwrap_or_default();
    let s = pcb_core::stack::beside(&board(ia)?, &link.a.refdes, &board(ib)?, &link.b.refdes, gap)?;
    let place_b = base.then(&s.place);
    let ds = v.get_mut("designs").and_then(Value::as_array_mut).ok_or("no designs")?;
    ds[ia]["place"] = serde_json::to_value(base).map_err(|e| e.to_string())?;
    ds[ib]["place"] = serde_json::to_value(place_b).map_err(|e| e.to_string())?;
    v["links"][i]["cable"] = json!(s.cable);
    std::fs::write(file, serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?).map_err(|e| format!("{file}: {e}"))?;
    Ok(json!({"file": file, "distance_mm": s.distance, "cable_mm": s.cable, "offset": place_b.offset}))
}

/// Changes link `k` (from 1) of a project file in place with `edit`, keeping the rest of the file as it is.
fn edit_link(file: &str, k: &str, edit: impl FnOnce(&mut BoardLink) -> Result<(), String>) -> Result<BoardLink, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    let mut v: Value = serde_json::from_str(&text).map_err(|e| format!("{file}: not a project ({e})"))?;
    let i = k.parse::<usize>().ok().filter(|&i| i >= 1).ok_or_else(|| format!("LINK {k}: the link's number, from 1"))? - 1;
    let slot = v.get_mut("links").and_then(Value::as_array_mut).and_then(|a| a.get_mut(i)).ok_or_else(|| format!("{file}: no link {k}"))?;
    let mut link: BoardLink = serde_json::from_value(slot.clone()).map_err(|e| format!("link {k}: {e}"))?;
    edit(&mut link)?;
    *slot = serde_json::to_value(&link).map_err(|e| e.to_string())?;
    std::fs::write(file, serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?).map_err(|e| format!("{file}: {e}"))?;
    Ok(link)
}

/// A `key=value` option as a number, or `default` without it.
fn number(kv: &[(String, String)], key: &str, default: f64) -> Result<f64, String> {
    opt(kv, key).map_or(Ok(default), |v| sim_core::parse_value(v).ok_or_else(|| format!("{key}={v}: not a number")))
}

/// The name of a linked pin in the output: `main.J1-3 <-> sensor.J1-3`.
fn pin_name(links: &[BoardLink], p: &sch_core::system::LinkProbe) -> String {
    let l = &links[p.link];
    format!("{}.{}-{} <-> {}.{}-{}", l.a.design, l.a.refdes, p.pins.0, l.b.design, l.b.refdes, p.pins.1)
}

/// The boards of a project file that are placed in 3D: (name, board, where it stands).
fn placed(file: &str) -> Result<Vec<(String, pcb_core::Board, pcb_core::stack::Place)>, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{file}: not a project ({e})"))?;
    let mut out = vec![];
    for (k, d) in v.get("designs").and_then(Value::as_array).into_iter().flatten().enumerate() {
        let Some(place) = d.get("place").cloned().and_then(|p| serde_json::from_value(p).ok()) else { continue };
        let name = d.get("name").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| format!("board{}", k + 1));
        let board = serde_json::from_value(d.get("board").cloned().ok_or("a board without its board")?).map_err(|e| format!("{name}: the board could not be read: {e}"))?;
        out.push((name, board, place));
    }
    Ok(out)
}

/// The boards of a project file: (name, how many are built, schematic), and the links.
type Boards = (Vec<(String, usize, Schematic)>, Vec<BoardLink>);
fn read(file: &str) -> Result<Boards, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{file}: not a project ({e})"))?;
    let sheet = |d: &Value| -> Result<Schematic, String> { serde_json::from_value(d.get("schematic").cloned().ok_or("a board without its schematic")?).map_err(|e| format!("a schematic could not be read: {e}")) };
    let mut boards = vec![];
    match v.get("designs").and_then(Value::as_array) {
        Some(list) => {
            for (k, d) in list.iter().enumerate() {
                let name = d.get("name").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| format!("board{}", k + 1));
                let copies = d.get("copies").and_then(Value::as_u64).unwrap_or(1).max(1) as usize;
                boards.push((name, copies, sheet(d)?));
            }
        }
        None => boards.push(("board".to_string(), 1, sheet(&v)?)),
    }
    let links = v.get("links").cloned().map(serde_json::from_value).transpose().map_err(|e| format!("the links could not be read: {e}"))?.unwrap_or_default();
    Ok((boards, links))
}

/// A project file as JSON, for `add` and `link` (format 2; a file of format 1 or none is refused, so a board is never lost).
fn read_value(file: &str) -> Result<Value, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e} (make it with 'project new {file}')"))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{file}: not a project ({e})"))?;
    if !v.get("designs").is_some_and(Value::is_array) {
        return Err(format!("{file}: a project of one board (format 1); open it in the app and add a board there, or make a new one with 'project new'"));
    }
    Ok(v)
}

fn write_value(file: &str, v: &Value) -> Result<(), String> {
    std::fs::write(file, serde_json::to_string_pretty(v).map_err(|e| e.to_string())?).map_err(|e| format!("{file}: {e}"))
}

/// `BOARD.REF`: a connector on a board of the project.
fn link_end(designs: &[Value], text: &str) -> Result<sch_core::system::LinkEnd, String> {
    let (d, r) = text.rsplit_once('.').ok_or_else(|| format!("{text}: write BOARD.REF, for example main.J4"))?;
    let design = designs.iter().find(|x| x.get("name").and_then(Value::as_str) == Some(d)).ok_or_else(|| {
        let names: Vec<&str> = designs.iter().filter_map(|x| x.get("name").and_then(Value::as_str)).collect();
        format!("{text}: no board {d} in the project (boards: {})", names.join(", "))
    })?;
    let s: Schematic = serde_json::from_value(design.get("schematic").cloned().unwrap_or_default()).map_err(|e| format!("{d}: the schematic could not be read: {e}"))?;
    let c = s.flat().components.iter().find(|c| !c.kind.is_power_symbol() && c.refdes.eq_ignore_ascii_case(r)).map(|c| c.refdes.clone()).ok_or_else(|| format!("{text}: board {d} has no part {r}"))?;
    Ok(sch_core::system::LinkEnd { design: d.to_string(), refdes: c })
}

impl crate::session::Session {
    /// `project new FILE` / `project add FILE NAME [SCHEMATIC.schjson] [board=B.pcbjson] [copies=N]` / `project link FILE A.REF B.REF [cable=MM]`:
    /// a project of several boards made from a script (L5). `add` without a file takes the session's schematic and board; a board of
    /// the same name is replaced (a script run again gives the same file), and its links are kept.
    fn project_make(&self, what: &str, file: &str, pos: &[String], kv: &[(String, String)]) -> Res {
        match what {
            "new" => {
                let replaced = std::path::Path::new(file).exists();
                write_value(file, &json!({"version": 2, "designs": [], "active": 0, "links": []}))?;
                Ok(obj(json!({"file": file, "replaced": replaced})))
            }
            "add" => {
                let name = pos.get(2).ok_or("project add FILE.ccproj NAME [SCHEMATIC.schjson] [board=BOARD.pcbjson] [copies=N]   (without a schematic file: the session's circuit and board)")?;
                if name.is_empty() || name.contains('.') || name.contains(char::is_whitespace) {
                    return Err(format!("{name}: a board's name has no dot or space (links write BOARD.REF)"));
                }
                let mut v = read_value(file)?;
                let schematic: Value = match pos.get(3) {
                    Some(f) => {
                        let text = std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?;
                        let s: Value = serde_json::from_str(&text).map_err(|e| format!("{f}: not a schematic ({e})"))?;
                        serde_json::from_value::<Schematic>(s.clone()).map_err(|e| format!("{f}: not a schematic ({e})"))?;
                        s
                    }
                    None => {
                        let mut s = serde_json::to_value(&self.sch).map_err(|e| e.to_string())?;
                        if let Value::Object(m) = &mut s {
                            m.extend(self.extras.clone());
                        }
                        s
                    }
                };
                let board: Value = match opt(kv, "board") {
                    Some(f) => {
                        let text = std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?;
                        let b: pcb_core::Board = serde_json::from_str(&text).map_err(|e| format!("{f}: not a board ({e})"))?;
                        serde_json::to_value(b).map_err(|e| e.to_string())?
                    }
                    None => match (&self.board, pos.get(3)) {
                        (Some(b), None) => serde_json::to_value(b).map_err(|e| e.to_string())?,
                        // the app needs a board: an empty one, made with `board import` there or here
                        _ => serde_json::to_value(pcb_core::Board::new(name, 100.0, 80.0)).map_err(|e| e.to_string())?,
                    },
                };
                let copies = match opt(kv, "copies") {
                    Some(c) => c.parse::<u64>().ok().filter(|&c| c >= 1).ok_or_else(|| format!("copies={c}: how many are built, 1 or more"))?,
                    None => 1,
                };
                let designs = v.get_mut("designs").and_then(Value::as_array_mut).ok_or("no designs")?;
                let at = designs.iter().position(|d| d.get("name").and_then(Value::as_str) == Some(name.as_str()));
                let mut d = json!({"name": name, "copies": copies, "place": null, "schematic": schematic, "board": board});
                let replaced = at.is_some();
                match at {
                    Some(i) => {
                        // keep where the board stands in 3D
                        d["place"] = designs[i].get("place").cloned().unwrap_or(Value::Null);
                        designs[i] = d;
                    }
                    None => designs.push(d),
                }
                let count = designs.len();
                write_value(file, &v)?;
                Ok(obj(json!({"file": file, "board": name, "replaced": replaced, "boards": count})))
            }
            "link" => {
                let (Some(a), Some(b)) = (pos.get(2), pos.get(3)) else { return Err("project link FILE.ccproj BOARD.REF BOARD.REF [cable=MM]   (two connectors joined: plugged into each other, or by a cable of MM)".into()) };
                let mut v = read_value(file)?;
                let designs = v.get("designs").and_then(Value::as_array).cloned().unwrap_or_default();
                let (ea, eb) = (link_end(&designs, a)?, link_end(&designs, b)?);
                if ea.design == eb.design {
                    return Err("both ends on the same board".into());
                }
                let cable = opt(kv, "cable").map(|c| sim_core::parse_value(c).filter(|x| *x > 0.0).ok_or_else(|| format!("cable={c}: the cable's length in mm"))).transpose()?;
                let links = v.get_mut("links").and_then(Value::as_array_mut).ok_or("no links")?;
                let same = |l: &Value| -> bool {
                    serde_json::from_value::<BoardLink>(l.clone()).is_ok_and(|l| (l.a == ea && l.b == eb) || (l.a == eb && l.b == ea))
                };
                let existed = links.iter().position(same);
                let n = match existed {
                    Some(i) => {
                        if cable.is_some() {
                            links[i]["cable"] = json!(cable);
                        }
                        i + 1
                    }
                    None => {
                        let l = BoardLink { a: ea, b: eb, cable, ..Default::default() };
                        links.push(serde_json::to_value(l).map_err(|e| e.to_string())?);
                        links.len()
                    }
                };
                write_value(file, &v)?;
                let (boards, links) = read(file)?;
                let ds: Vec<(&str, &Schematic)> = boards.iter().map(|(n, _, s)| (n.as_str(), s)).collect();
                let f = sch_core::system::check(&ds, &links);
                let errors = f.iter().filter(|f| f.severity == sch_core::Severity::Error).count();
                Ok(obj(json!({"file": file, "link": n, "existed": existed.is_some(), "findings": f.len(), "errors": errors, "check": format!("project check {file}")})))
            }
            _ => unreachable!(),
        }
    }

    pub fn project(&mut self, args: &[String]) -> Res {
        let (pos, kv) = options(args);
        let (Some(what), Some(file)) = (pos.first(), pos.get(1)) else { return Err(USAGE.into()) };
        if matches!(what.as_str(), "new" | "add" | "link") {
            return self.project_make(what, file, &pos, &kv);
        }
        let (boards, links) = read(file)?;
        let designs: Vec<(&str, &Schematic)> = boards.iter().map(|(n, _, s)| (n.as_str(), s)).collect();
        match what.as_str() {
            "check" => {
                let findings = sch_core::system::check(&designs, &links);
                let list: Vec<Value> = findings
                    .iter()
                    .map(|f| {
                        let fix = match &f.fix {
                            Some(sch_core::system::Fix::Alias(a, b)) => json!({"alias": [a, b], "command": format!("project alias {file} {} {a} {b}", f.link + 1)}),
                            Some(sch_core::system::Fix::Pairs(p)) => json!({"pairs": p, "command": format!("project mirror {file} {}", f.link + 1)}),
                            None => Value::Null,
                        };
                        json!({"severity": if f.severity == sch_core::Severity::Error { "error" } else { "warning" }, "link": f.link, "pins": f.pins, "message": f.message, "hint": f.hint, "fix": fix})
                    })
                    .collect();
                let errors = findings.iter().filter(|f| f.severity == sch_core::Severity::Error).count();
                Ok(obj(json!({
                    "boards": boards.iter().enumerate().map(|(k, (n, c, s))| {
                        // each board's ERC judged with the whole project (a board that gets its ground from a linked one is not asked for one)
                        let erc = sch_core::system::erc_in_project(&designs, &links, k);
                        let erc_errors: Vec<&str> = erc.iter().filter(|p| p.severity == sch_core::Severity::Error).map(|p| p.message.as_str()).collect();
                        json!({"name": n, "copies": c, "parts": s.flat().components.len(), "erc_errors": erc_errors, "ground_from": sch_core::system::ground_from(&designs, &links, k)})
                    }).collect::<Vec<_>>(),
                    "links": links.iter().map(|l| format!("{}.{} <-> {}.{}", l.a.design, l.a.refdes, l.b.design, l.b.refdes)).collect::<Vec<_>>(),
                    "findings": list, "errors": errors, "ok": errors == 0,
                })))
            }
            "bom" => {
                let out = pos.get(2).ok_or(USAGE)?;
                let copies: Vec<usize> = boards.iter().map(|b| b.1).collect();
                let mut rows = sch_core::system::system_bom(&designs, &copies);
                // the cables' housings, contacts and wire
                rows.extend(sch_core::harness::harness_bom(&sch_core::harness::harnesses(&designs, &links, &copies)));
                let names: Vec<&str> = designs.iter().map(|d| d.0).collect();
                std::fs::write(out, sch_core::system::bom_csv(&rows, &names)).map_err(|e| format!("{out}: {e}"))?;
                Ok(obj(json!({"file": out, "rows": rows.len(), "parts": rows.iter().map(|r| r.total).sum::<usize>()})))
            }
            "merge" => {
                let out = pos.get(2).ok_or(USAGE)?;
                let (s, probes) = sch_core::system::merge(&designs, &links)?;
                std::fs::write(out, serde_json::to_string_pretty(&s).map_err(|e| e.to_string())?).map_err(|e| format!("{out}: {e}"))?;
                Ok(obj(json!({"file": out, "parts": s.components.len(), "link_sources": probes.iter().filter(|p| !p.source.is_empty()).map(|p| p.source.clone()).collect::<Vec<_>>()})))
            }
            "dc" => {
                let readings = sim_core::system::system_dc(&designs, &links)?;
                let list: Vec<Value> = readings
                    .iter()
                    .map(|r| {
                        let l = &links[r.probe.link];
                        json!({"pin": format!("{}.{}-{} <-> {}.{}-{}", l.a.design, l.a.refdes, r.probe.pins.0, l.b.design, l.b.refdes, r.probe.pins.1), "net_a": r.probe.net_a, "net_b": r.probe.net_b, "v_a": r.v_a, "v_b": r.v_b, "current_a_to_b": r.current})
                    })
                    .collect();
                Ok(obj(json!({"pins": list})))
            }
            "tran" => {
                let r = sim_core::system::system_tran(&designs, &links, number(&kv, "tstop", 10e-3)?)?;
                let span = |w: &[f64]| w.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &x| (lo.min(x), hi.max(x)));
                let list: Vec<Value> = r
                    .pins
                    .iter()
                    .map(|p| {
                        let (lo, hi) = span(&p.v);
                        let i = p.i.as_ref().map(|i| json!({"end": i.last(), "peak": i.iter().copied().fold(0.0f64, |m, x| if x.abs() > m.abs() { x } else { m })}));
                        json!({"pin": pin_name(&links, &p.probe), "net_a": p.probe.net_a, "net_b": p.probe.net_b, "v_end": p.v.last(), "v_min": lo, "v_max": hi, "current_a_to_b": i})
                    })
                    .collect();
                Ok(obj(json!({"tstop": r.time.last(), "points": r.time.len(), "pins": list, "warnings": r.warnings})))
            }
            "ac" => {
                let r = sim_core::system::system_ac(&designs, &links, number(&kv, "fstart", 10.0)?, number(&kv, "fstop", 1e6)?)?;
                let list: Vec<Value> = r
                    .pins
                    .iter()
                    .map(|p| {
                        // the first frequency 3 dB below the gain at the start of the sweep (a low pass's corner)
                        let f3 = p.db.first().and_then(|&d0| p.db.iter().position(|&d| d < d0 - 3.0).map(|k| r.freq[k]));
                        json!({"pin": pin_name(&links, &p.probe), "net_a": p.probe.net_a, "net_b": p.probe.net_b, "db_start": p.db.first(), "db_end": p.db.last(), "f_minus_3db": f3})
                    })
                    .collect();
                Ok(obj(json!({"fstart": r.freq.first(), "fstop": r.freq.last(), "pins": list, "warnings": r.warnings})))
            }
            "alias" => {
                let (Some(k), Some(a), Some(b)) = (pos.get(2), pos.get(3), pos.get(4)) else { return Err(USAGE.into()) };
                let link = edit_link(file, k, |l| {
                    l.aliases.push((a.clone(), b.clone()));
                    Ok(())
                })?;
                let (boards, links) = read(file)?;
                let designs: Vec<(&str, &Schematic)> = boards.iter().map(|(n, _, s)| (n.as_str(), s)).collect();
                let left = sch_core::system::check(&designs, &links).len();
                Ok(obj(json!({"file": file, "aliases": link.aliases, "findings_left": left})))
            }
            "mirror" => {
                let k = pos.get(2).ok_or(USAGE)?;
                let link = edit_link(file, k, |l| {
                    let pins = |d: &str, r: &str| designs.iter().find(|x| x.0 == d).and_then(|x| sch_core::system::pins_of(x.1, r));
                    let (Some(pa), Some(pb)) = (pins(&l.a.design, &l.a.refdes), pins(&l.b.design, &l.b.refdes)) else { return Err("the link's connectors are not in the schematics".into()) };
                    let rows = |d: &str, r: &str| designs.iter().find(|x| x.0 == d).is_some_and(|x| sch_core::system::two_rows(x.1, r));
                    l.pairs = sch_core::system::next_mirror(&l.pairs, &pa, &pb, rows(&l.a.design, &l.a.refdes) && rows(&l.b.design, &l.b.refdes)).1;
                    Ok(())
                })?;
                Ok(obj(json!({"file": file, "pairs": link.pairs})))
            }
            "harness" => {
                let out = pos.get(2).ok_or(USAGE)?;
                let copies: Vec<usize> = boards.iter().map(|b| b.1).collect();
                let hs = sch_core::harness::harnesses(&designs, &links, &copies);
                if hs.is_empty() {
                    return Err("no link of this project is joined by a cable (project cable FILE LINK first)".into());
                }
                let text = if out.to_ascii_lowercase().ends_with(".csv") { sch_core::harness::harness_csv(&hs) } else { sch_core::harness::harness_svg(&hs) };
                std::fs::write(out, text).map_err(|e| format!("{out}: {e}"))?;
                let list: Vec<Value> = hs
                    .iter()
                    .map(|h| {
                        json!({"name": h.name, "from": h.end_a, "to": h.end_b, "length_mm": h.length_mm, "copies": h.copies, "awg": h.awg(), "ribbon": h.ribbon,
                            "housing_a": h.mate_a.housing, "housing_b": h.mate_b.housing, "contact_a": h.mate_a.contact, "contact_b": h.mate_b.contact,
                            "wires": h.wires.iter().map(|w| json!({"n": w.n, "pin_a": w.pin_a, "pin_b": w.pin_b, "signal": w.signal(), "color": w.color})).collect::<Vec<_>>()})
                    })
                    .collect();
                Ok(obj(json!({"file": out, "cables": list})))
            }
            "color" => {
                let (Some(k), Some(pin), Some(color)) = (pos.get(2), pos.get(3), pos.get(4)) else { return Err(USAGE.into()) };
                let keys: Vec<&str> = sch_core::harness::COLORS.iter().map(|c| c.0).collect();
                // `auto`: back to the colour by what the wire carries
                let key = if color == "auto" { "auto" } else { sch_core::harness::COLORS.iter().find(|c| c.0 == color.as_str() || c.1 == color.as_str()).map(|c| c.0).ok_or_else(|| format!("{color}: not a colour ({} auto)", keys.join(" ")))? };
                let link = edit_link(file, k, |l| {
                    l.colors.retain(|(p, _)| p != pin);
                    if key != "auto" {
                        l.colors.push((pin.clone(), key.to_string()));
                    }
                    Ok(())
                })?;
                Ok(obj(json!({"file": file, "colors": link.colors})))
            }
            "cable" => {
                let k = pos.get(2).ok_or(USAGE)?;
                Ok(obj(cable(file, k, number(&kv, "gap", 20.0)?)?))
            }
            "step" => {
                let out = pos.get(2).ok_or(USAGE)?;
                let boards = placed(file)?;
                if boards.is_empty() {
                    return Err("no board of this project is placed in 3D (in the app: the link's 3D で重ねる)".into());
                }
                let list: Vec<(&str, &pcb_core::Board, pcb_core::stack::Place)> = boards.iter().map(|(n, b, p)| (n.as_str(), b, *p)).collect();
                let name = std::path::Path::new(file).file_stem().map_or_else(|| "system".to_string(), |s| s.to_string_lossy().to_string());
                std::fs::write(out, pcb_core::step_out::write_system(&name, &list)).map_err(|e| format!("{out}: {e}"))?;
                let clashes = pcb_core::stack::clashes(&list);
                Ok(obj(json!({"file": out, "boards": boards.iter().map(|b| b.0.clone()).collect::<Vec<_>>(), "clashes": clashes})))
            }
            _ => Err(USAGE.into()),
        }
    }
}
