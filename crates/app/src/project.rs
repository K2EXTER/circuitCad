//! A project: the schematic and the board saved together in one `.ccproj` file. A project of several boards (E5, multi-board) holds a
//! schematic and a board for each, the links between their connectors (`sch_core::system::BoardLink`) and how many of each are built:
//! format 2 (`designs`, `active`, `links`). A project of one board without links is still written as format 1, which older versions read.

use pcb_core::Board;
use crate::schematic::{take_bench, take_specs, take_settings, with_bench, with_specs, with_settings, BenchData, SpecItem, SheetSettings};
use sch_core::Schematic;
use std::path::Path;

pub const EXTENSION: &str = "ccproj";

/// What the file holds: the board in the editors (`schematic` ... `settings`), and the others of a multi-board project in `system`.
/// `version` lets a later format still read this one.
pub struct Project {
    pub schematic: Schematic,
    pub board: Board,
    /// The spec table (empty in files saved before it was stored).
    pub specs: Vec<SpecItem>,
    /// Bench recordings drawn over the plots (empty in older files).
    pub bench: BenchData,
    /// The part tolerances and the analysis settings of the sheet (the defaults in older files).
    pub settings: SheetSettings,
    pub system: System,
}

/// One board of a multi-board project, kept aside while another is in the editors.
#[derive(Clone)]
pub struct Design {
    pub schematic: Schematic,
    pub board: Board,
    pub specs: Vec<SpecItem>,
    pub bench: BenchData,
    pub settings: SheetSettings,
}

/// The boards of the project: their names and how many of each are built, in order; which is in the editors; the others' contents
/// (`None` at the one in the editors); and the links between their connectors.
#[derive(Clone)]
pub struct System {
    pub names: Vec<String>,
    pub copies: Vec<usize>,
    pub active: usize,
    pub parked: Vec<Option<Design>>,
    pub links: Vec<sch_core::system::BoardLink>,
    /// Where each board stands in 3D, in the first placed board's frame (`None`: not put together in 3D yet).
    pub places: Vec<Option<pcb_core::stack::Place>>,
}

impl Default for System {
    fn default() -> Self {
        System { names: vec!["基板1".into()], copies: vec![1], active: 0, parked: vec![None], links: vec![], places: vec![None] }
    }
}

impl System {
    /// A project of more than one board (or with links)?
    pub fn is_multi(&self) -> bool {
        self.names.len() > 1 || !self.links.is_empty()
    }
}

const VERSION: u64 = 2;

/// The schematic with the app's own tables put into it (as format 1 stores them).
fn sheet_value(schematic: &Schematic, specs: &[SpecItem], bench: &BenchData, settings: &SheetSettings) -> Result<serde_json::Value, String> {
    let mut sheet = serde_json::to_value(schematic).map_err(|e| e.to_string())?;
    with_specs(&mut sheet, specs);
    with_bench(&mut sheet, bench);
    with_settings(&mut sheet, settings);
    Ok(sheet)
}

/// A project of one board (format 1).
pub fn save(path: &Path, schematic: &Schematic, board: &Board, specs: &[SpecItem], bench: &BenchData, settings: &SheetSettings) -> Result<(), String> {
    let value = serde_json::json!({ "version": 1, "schematic": sheet_value(schematic, specs, bench, settings)?, "board": board });
    let text = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

/// The whole project: `current` is the board in the editors (the system's `active`), the others come from the system. One board without
/// links is written as format 1.
pub fn save_system(path: &Path, current: &Design, system: &System) -> Result<(), String> {
    if !system.is_multi() {
        return save(path, &current.schematic, &current.board, &current.specs, &current.bench, &current.settings);
    }
    let mut designs = vec![];
    for (k, name) in system.names.iter().enumerate() {
        let d = if k == system.active { current } else { system.parked.get(k).and_then(Option::as_ref).ok_or_else(|| format!("基板「{name}」の中身がありません"))? };
        designs.push(serde_json::json!({
            "name": name,
            "copies": system.copies.get(k).copied().unwrap_or(1),
            "place": system.places.get(k).copied().flatten(),
            "schematic": sheet_value(&d.schematic, &d.specs, &d.bench, &d.settings)?,
            "board": d.board,
        }));
    }
    let value = serde_json::json!({ "version": VERSION, "designs": designs, "active": system.active, "links": system.links });
    let text = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

pub fn load(path: &Path) -> Result<Project, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    parse(&text)
}

/// A project from its JSON text (the lessons carry one inside the program).
pub fn parse(text: &str) -> Result<Project, String> {
    let mut value: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("プロジェクトの形式が読めません ({e})"))?;
    if value.get("version").and_then(|v| v.as_u64()).is_some_and(|v| v > VERSION) {
        return Err("新しい版の circuitCad で作られたプロジェクトです。circuitCad を更新してから開いてください".into());
    }
    // format 2: several boards
    if let Some(serde_json::Value::Array(list)) = value.get_mut("designs").map(serde_json::Value::take) {
        let mut system = System { names: vec![], copies: vec![], active: 0, parked: vec![], links: vec![], places: vec![] };
        for (k, mut d) in list.into_iter().enumerate() {
            let name = d.get("name").and_then(|n| n.as_str()).map(str::to_string).unwrap_or_else(|| format!("基板{}", k + 1));
            let copies = d.get("copies").and_then(|c| c.as_u64()).unwrap_or(1).max(1) as usize;
            let one = design_of(&mut d).map_err(|e| format!("基板「{name}」: {e}"))?;
            system.names.push(name);
            system.copies.push(copies);
            system.parked.push(Some(one));
            system.places.push(d.get("place").cloned().and_then(|v| serde_json::from_value(v).ok()));
        }
        if system.names.is_empty() {
            return Err("プロジェクトに基板がありません".into());
        }
        system.links = value.get_mut("links").map(serde_json::Value::take).map(serde_json::from_value).transpose().map_err(|e| format!("基板どうしのつなぎが読めません ({e})"))?.unwrap_or_default();
        system.active = value.get("active").and_then(|a| a.as_u64()).map_or(0, |a| a as usize).min(system.names.len() - 1);
        let d = system.parked[system.active].take().ok_or("プロジェクトの形式が読めません")?;
        return Ok(Project { schematic: d.schematic, board: d.board, specs: d.specs, bench: d.bench, settings: d.settings, system });
    }
    let d = design_of(&mut value)?;
    let system = System { names: vec![if d.board.name.is_empty() { "基板1".into() } else { d.board.name.clone() }], ..Default::default() };
    Ok(Project { schematic: d.schematic, board: d.board, specs: d.specs, bench: d.bench, settings: d.settings, system })
}

/// One board's schematic (with the app's tables) and board.
fn design_of(value: &mut serde_json::Value) -> Result<Design, String> {
    let mut part = |key: &str| value.get_mut(key).map(serde_json::Value::take).ok_or_else(|| format!("プロジェクトに「{key}」がありません (circuitCad のプロジェクトファイルではないようです)"));
    let mut sheet = part("schematic")?;
    let specs = take_specs(&mut sheet);
    let bench = take_bench(&mut sheet);
    let settings = take_settings(&mut sheet);
    let schematic = serde_json::from_value(sheet).map_err(|e| format!("回路図の部分が読めません ({e})"))?;
    let board = serde_json::from_value(part("board")?).map_err(|e| format!("基板の部分が読めません ({e})"))?;
    Ok(Design { schematic, board, specs, bench, settings })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_round_trips() {
        let mut s = Schematic::default();
        s.add(sch_core::Kind::Resistor, (4, 4), 0);
        let b = Board::new("demo", 30.0, 20.0);
        let dir = std::env::temp_dir().join(format!("cc_project_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("demo.ccproj");
        let spec = SpecItem { kind: "dc_voltage".into(), target: "OUT".into(), lo: "4.75".into(), hi: "5.25".into(), at: String::new(), measured: "4.93".into() };
        let tol = SheetSettings { r: "1".into(), ..Default::default() };
        save(&f, &s, &b, std::slice::from_ref(&spec), &BenchData::default(), &tol).unwrap();
        let p = load(&f).unwrap();
        assert_eq!(p.settings, tol);
        assert_eq!(p.schematic.components.len(), 1);
        assert_eq!(p.specs, vec![spec]);
        assert_eq!(p.board.name, "demo");
        // a file that is not a project is refused with a message
        std::fs::write(&f, "{\"version\": 1}").unwrap();
        assert!(load(&f).is_err());
        std::fs::write(&f, serde_json::to_string(&serde_json::json!({"version": 9, "schematic": s, "board": b})).unwrap()).unwrap();
        assert!(load(&f).err().unwrap().contains("新しい版"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Two boards and a link: format 2 keeps both, which one was open, how many of each, and the link; one board alone stays format 1.
    #[test]
    fn a_project_of_two_boards_round_trips() {
        let dir = std::env::temp_dir().join(format!("cc_system_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("two.ccproj");
        let mk = |n: &str| {
            let mut s = Schematic::default();
            s.add(sch_core::Kind::Resistor, (4, 4), 0);
            Design { schematic: s, board: Board::new(n, 30.0, 20.0), specs: vec![], bench: BenchData::default(), settings: SheetSettings::default() }
        };
        let link = sch_core::system::BoardLink { a: sch_core::system::LinkEnd { design: "main".into(), refdes: "J1".into() }, b: sch_core::system::LinkEnd { design: "sensor".into(), refdes: "J2".into() }, pairs: vec![] };
        let up = pcb_core::stack::Place::new(1, false, [3.0, 4.0, 11.0]);
        let system = System { names: vec!["main".into(), "sensor".into()], copies: vec![1, 3], active: 1, parked: vec![Some(mk("main")), None], links: vec![link.clone()], places: vec![Some(Default::default()), Some(up)] };
        save_system(&f, &mk("sensor"), &system).unwrap();
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.contains("\"version\": 2"));
        let p = load(&f).unwrap();
        assert_eq!(p.board.name, "sensor", "the board that was open");
        assert_eq!((p.system.names.clone(), p.system.copies.clone(), p.system.active), (vec!["main".to_string(), "sensor".to_string()], vec![1, 3], 1));
        assert!(p.system.parked[0].as_ref().is_some_and(|d| d.board.name == "main") && p.system.parked[1].is_none());
        assert_eq!(p.system.links, vec![link]);
        assert_eq!(p.system.places, vec![Some(Default::default()), Some(up)], "where the boards stand in 3D");
        // one board, no link: format 1 as before
        save_system(&f, &mk("solo"), &System::default()).unwrap();
        assert!(std::fs::read_to_string(&f).unwrap().contains("\"version\": 1"));
        assert!(!load(&f).unwrap().system.is_multi());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
