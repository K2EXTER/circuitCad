//! Display preferences kept between launches (`%APPDATA%\circuitCad\settings.json`, next to `progress.json`).

use std::path::{Path, PathBuf};

#[derive(Clone, PartialEq, Debug)]
pub struct Prefs {
    pub icon_only: bool,
    pub show_hints: bool,
    /// Where the user keeps the makers' SPICE model zips (Murata's netlist zip, ...). They are not part of the app: the
    /// makers' terms forbid redistribution.
    pub model_zips: Vec<String>,
    /// The frames of the design's requirements on the schematic sheet, the frames of the groups on the board, and the lines of the distances to keep on the board.
    pub req_frames: bool,
    pub req_groups: bool,
    pub req_lines: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { icon_only: false, show_hints: true, model_zips: vec![], req_frames: true, req_groups: true, req_lines: true }
    }
}

impl Prefs {
    pub fn default_file() -> PathBuf {
        part_lib::default_dir().parent().map_or_else(|| PathBuf::from("settings.json"), |d| d.join("settings.json"))
    }

    /// A missing, unreadable or partly missing file gives the defaults for what it lacks.
    pub fn load(file: &Path) -> Self {
        let json = std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
        let get = |k: &str, d: bool| json.as_ref().and_then(|v| v.get(k)).and_then(|b| b.as_bool()).unwrap_or(d);
        let d = Self::default();
        let mut model_zips: Vec<String> = json
            .as_ref()
            .and_then(|v| v.get("model_zips"))
            .and_then(|a| a.as_array())
            .map(|a| a.iter().filter_map(|s| s.as_str()).filter(|s| !s.is_empty()).map(String::from).collect())
            .unwrap_or_default();
        // the first version kept a single Murata zip under its own key
        if let Some(old) = json.as_ref().and_then(|v| v.get("murata_zip")).and_then(|s| s.as_str()).filter(|s| !s.is_empty())
            && !model_zips.iter().any(|z| z == old)
        {
            model_zips.push(old.to_string());
        }
        Self { icon_only: get("icon_only", d.icon_only), show_hints: get("show_hints", d.show_hints), model_zips, req_frames: get("req_frames", d.req_frames), req_groups: get("req_groups", d.req_groups), req_lines: get("req_lines", d.req_lines) }
    }

    /// Writes the file; the error text says what to do (the setting then only lasts until the app closes).
    pub fn save(&self, file: &Path) -> Result<(), String> {
        let json = serde_json::json!({ "version": 1, "icon_only": self.icon_only, "show_hints": self.show_hints, "model_zips": self.model_zips, "req_frames": self.req_frames, "req_groups": self.req_groups, "req_lines": self.req_lines });
        file.parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|_| std::fs::write(file, serde_json::to_string_pretty(&json).unwrap_or_default()))
            .map_err(|e| format!("設定を保存できませんでした: {e}。今回の起動中だけ有効です"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_defaults() {
        let d = std::env::temp_dir().join(format!("cc_prefs_{}", std::process::id()));
        let f = d.join("settings.json");
        assert_eq!(Prefs::load(&f), Prefs::default(), "no file: defaults");
        let p = Prefs { icon_only: true, show_hints: false, model_zips: vec!["C:/m/grm.zip".into()], req_frames: false, req_groups: true, req_lines: false };
        p.save(&f).unwrap();
        assert_eq!(Prefs::load(&f), p);
        std::fs::write(&f, r#"{"icon_only": true}"#).unwrap();
        assert_eq!(Prefs::load(&f), Prefs { icon_only: true, ..Prefs::default() }, "missing key: its default");
        std::fs::write(&f, r#"{"murata_zip": "C:/old/grm.zip", "model_zips": ["D:/tdk.zip"]}"#).unwrap();
        assert_eq!(Prefs::load(&f).model_zips, vec!["D:/tdk.zip".to_string(), "C:/old/grm.zip".to_string()], "the old single-zip key is kept");
        std::fs::write(&f, "not json").unwrap();
        assert_eq!(Prefs::load(&f), Prefs::default());
        let _ = std::fs::remove_dir_all(&d);
    }
}
