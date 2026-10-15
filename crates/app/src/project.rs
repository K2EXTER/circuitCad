//! A project: the schematic and the board saved together in one `.ccproj` file.

use pcb_core::Board;
use crate::schematic::{take_specs, with_specs, SpecItem};
use sch_core::Schematic;
use std::path::Path;

pub const EXTENSION: &str = "ccproj";

/// What the file holds. `version` lets a later format still read this one.
pub struct Project {
    pub schematic: Schematic,
    pub board: Board,
    /// The spec table (empty in files saved before it was stored).
    pub specs: Vec<SpecItem>,
}

const VERSION: u64 = 1;

pub fn save(path: &Path, schematic: &Schematic, board: &Board, specs: &[SpecItem]) -> Result<(), String> {
    let mut sheet = serde_json::to_value(schematic).map_err(|e| e.to_string())?;
    with_specs(&mut sheet, specs);
    let value = serde_json::json!({ "version": VERSION, "schematic": sheet, "board": board });
    let text = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

pub fn load(path: &Path) -> Result<Project, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut value: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("プロジェクトの形式が読めません ({e})"))?;
    if value.get("version").and_then(|v| v.as_u64()).is_some_and(|v| v > VERSION) {
        return Err("新しい版の circuitCad で作られたプロジェクトです。circuitCad を更新してから開いてください".into());
    }
    let mut part = |key: &str| value.get_mut(key).map(serde_json::Value::take).ok_or_else(|| format!("プロジェクトに「{key}」がありません (circuitCad のプロジェクトファイルではないようです)"));
    let mut sheet = part("schematic")?;
    let specs = take_specs(&mut sheet);
    let schematic = serde_json::from_value(sheet).map_err(|e| format!("回路図の部分が読めません ({e})"))?;
    let board = serde_json::from_value(part("board")?).map_err(|e| format!("基板の部分が読めません ({e})"))?;
    Ok(Project { schematic, board, specs })
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
        save(&f, &s, &b, std::slice::from_ref(&spec)).unwrap();
        let p = load(&f).unwrap();
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
}
