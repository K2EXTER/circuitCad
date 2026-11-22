//! `project check FILE.ccproj` / `project bom FILE.ccproj OUT.csv`: a project of several boards (E5, multi-board) checked link by link
//! (`sch_core::system::check`: the two sides of each connector carry the same signals pin by pin) and its parts counted together. The
//! file is read as it is (format 2: `designs`, `links`; format 1: one board), without changing the session's schematic or board.

use crate::Res;
use crate::session::{obj, opt, options};
use sch_core::Schematic;
use sch_core::system::BoardLink;
use serde_json::{Value, json};

const USAGE: &str = "project check FILE.ccproj | project bom FILE.ccproj OUT.csv | project step FILE.ccproj OUT.step | project dc FILE.ccproj | project tran FILE.ccproj [tstop=10m] | project ac FILE.ccproj [fstart=10] [fstop=1Meg] | project merge FILE.ccproj OUT.schjson   (a project of several boards: the links between their connectors checked pin by pin, the parts of all boards counted together, the boards placed in 3D as one STEP, the whole system's operating point pin by pin, its transient and AC sweep summed up pin by pin, the whole system as one schematic)";

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

impl crate::session::Session {
    pub fn project(&mut self, args: &[String]) -> Res {
        let (pos, kv) = options(args);
        let (Some(what), Some(file)) = (pos.first(), pos.get(1)) else { return Err(USAGE.into()) };
        let (boards, links) = read(file)?;
        let designs: Vec<(&str, &Schematic)> = boards.iter().map(|(n, _, s)| (n.as_str(), s)).collect();
        match what.as_str() {
            "check" => {
                let findings = sch_core::system::check(&designs, &links);
                let list: Vec<Value> = findings
                    .iter()
                    .map(|f| json!({"severity": if f.severity == sch_core::Severity::Error { "error" } else { "warning" }, "link": f.link, "pins": f.pins, "message": f.message, "hint": f.hint}))
                    .collect();
                let errors = findings.iter().filter(|f| f.severity == sch_core::Severity::Error).count();
                Ok(obj(json!({
                    "boards": boards.iter().map(|(n, c, s)| json!({"name": n, "copies": c, "parts": s.flat().components.len()})).collect::<Vec<_>>(),
                    "links": links.iter().map(|l| format!("{}.{} <-> {}.{}", l.a.design, l.a.refdes, l.b.design, l.b.refdes)).collect::<Vec<_>>(),
                    "findings": list, "errors": errors, "ok": errors == 0,
                })))
            }
            "bom" => {
                let out = pos.get(2).ok_or(USAGE)?;
                let copies: Vec<usize> = boards.iter().map(|b| b.1).collect();
                let rows = sch_core::system::system_bom(&designs, &copies);
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
