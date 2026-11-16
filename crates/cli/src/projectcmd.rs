//! `project check FILE.ccproj` / `project bom FILE.ccproj OUT.csv`: a project of several boards (E5, multi-board) checked link by link
//! (`sch_core::system::check`: the two sides of each connector carry the same signals pin by pin) and its parts counted together. The
//! file is read as it is (format 2: `designs`, `links`; format 1: one board), without changing the session's schematic or board.

use crate::Res;
use crate::session::{obj, options};
use sch_core::Schematic;
use sch_core::system::BoardLink;
use serde_json::{Value, json};

const USAGE: &str = "project check FILE.ccproj | project bom FILE.ccproj OUT.csv | project step FILE.ccproj OUT.step   (a project of several boards: the links between their connectors checked pin by pin, the parts of all boards counted together, the boards placed in 3D as one STEP)";

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
        let (pos, _) = options(args);
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
