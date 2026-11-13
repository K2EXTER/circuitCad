//! Display preferences kept between launches (`%APPDATA%\circuitCad\settings.json`, next to `progress.json`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

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
    /// The colours of the schematic sheets on the board.
    pub sheet_colors: bool,
    /// Smooth the edges of the lines with multisampling (4 samples). It is fixed when the window is made, so a change is taken up at the
    /// next start. Off for a graphics card that is slow or has trouble with it (`--no-msaa` does the same for one start).
    pub smooth_edges: bool,
    /// What is picked in the schematic, the board or the 3D view is shown in the others (`crate::link`).
    pub link_selection: bool,
    /// The language of the screen: "ja" or "en" (`i18n`).
    pub lang: String,
    /// The size of everything on the screen (egui's zoom factor): 1.0 as made, 1.25 a quarter larger.
    pub ui_scale: f32,
    /// The rotating tip at the right of the status line (豆知識).
    pub show_tips: bool,
    /// How often a copy of what is not saved is kept, in seconds; 0 = never (`autosave.rs`).
    pub autosave_secs: u32,
    /// How fast the wheel zooms the schematic, the board, the 3D view and the waveforms (1.0 as made), and whether
    /// it zooms the other way round.
    pub wheel_speed: f32,
    pub wheel_invert: bool,
    /// What a new title block's "作者" gets (empty: nothing).
    pub default_author: String,
    /// The board's grid when the app starts, mm.
    pub board_grid: f64,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { icon_only: false, show_hints: true, model_zips: vec![], req_frames: true, req_groups: true, req_lines: true, sheet_colors: true, smooth_edges: true, link_selection: true, lang: "ja".into(), ui_scale: 1.0, show_tips: true, autosave_secs: 60, wheel_speed: 1.0, wheel_invert: false, default_author: String::new(), board_grid: 0.5 }
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
        // a number out of its range (a hand-edited file) is taken as missing
        let num = |k: &str, d: f64, lo: f64, hi: f64| json.as_ref().and_then(|v| v.get(k)).and_then(|n| n.as_f64()).filter(|x| (lo..=hi).contains(x)).unwrap_or(d);
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
        Self { icon_only: get("icon_only", d.icon_only), show_hints: get("show_hints", d.show_hints), model_zips, req_frames: get("req_frames", d.req_frames), req_groups: get("req_groups", d.req_groups), req_lines: get("req_lines", d.req_lines), sheet_colors: get("sheet_colors", d.sheet_colors), smooth_edges: get("smooth_edges", d.smooth_edges), link_selection: get("link_selection", d.link_selection), lang: json.as_ref().and_then(|v| v.get("lang")).and_then(|s| s.as_str()).and_then(i18n::Lang::parse).unwrap_or(i18n::Lang::Ja).code().to_string(),
            ui_scale: num("ui_scale", d.ui_scale as f64, 0.5, 3.0) as f32,
            show_tips: get("show_tips", d.show_tips),
            autosave_secs: num("autosave_secs", d.autosave_secs as f64, 0.0, 3600.0) as u32,
            wheel_speed: num("wheel_speed", d.wheel_speed as f64, 0.1, 10.0) as f32,
            wheel_invert: get("wheel_invert", d.wheel_invert),
            default_author: json.as_ref().and_then(|v| v.get("default_author")).and_then(|s| s.as_str()).unwrap_or_default().to_string(),
            board_grid: num("board_grid", d.board_grid, 0.0, 5.0),
        }
    }

    /// Writes the file; the error text says what to do (the setting then only lasts until the app closes).
    pub fn save(&self, file: &Path) -> Result<(), String> {
        let json = serde_json::json!({ "version": 1, "icon_only": self.icon_only, "show_hints": self.show_hints, "model_zips": self.model_zips, "req_frames": self.req_frames, "req_groups": self.req_groups, "req_lines": self.req_lines, "sheet_colors": self.sheet_colors, "smooth_edges": self.smooth_edges, "link_selection": self.link_selection, "lang": self.lang,
            "ui_scale": self.ui_scale, "show_tips": self.show_tips, "autosave_secs": self.autosave_secs, "wheel_speed": self.wheel_speed, "wheel_invert": self.wheel_invert, "default_author": self.default_author, "board_grid": self.board_grid });
        file.parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|_| std::fs::write(file, serde_json::to_string_pretty(&json).unwrap_or_default()))
            .map_err(|e| format!("設定を保存できませんでした: {e}。今回の起動中だけ有効です"))
    }
}

/// The wheel's zoom factor (speed, negative when turned round), shared by every view that zooms with the wheel.
static WHEEL: AtomicU32 = AtomicU32::new(1.0f32.to_bits());

pub fn set_wheel(speed: f32, invert: bool) {
    WHEEL.store((if invert { -speed } else { speed }).to_bits(), Ordering::Relaxed);
}

/// This frame's wheel turn for zooming (positive = in), with the speed and direction of the settings.
pub fn zoom_scroll(ui: &eframe::egui::Ui) -> f32 {
    ui.input(|i| i.smooth_scroll_delta.y) * f32::from_bits(WHEEL.load(Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_defaults() {
        let d = std::env::temp_dir().join(format!("cc_prefs_{}", std::process::id()));
        let f = d.join("settings.json");
        assert_eq!(Prefs::load(&f), Prefs::default(), "no file: defaults");
        let p = Prefs { icon_only: true, show_hints: false, model_zips: vec!["C:/m/grm.zip".into()], req_frames: false, req_groups: true, req_lines: false, sheet_colors: false, smooth_edges: false, link_selection: false, lang: "en".into(), ui_scale: 1.25, show_tips: false, autosave_secs: 0, wheel_speed: 2.0, wheel_invert: true, default_author: "山田".into(), board_grid: 1.27 };
        p.save(&f).unwrap();
        assert_eq!(Prefs::load(&f), p);
        std::fs::write(&f, r#"{"icon_only": true}"#).unwrap();
        assert_eq!(Prefs::load(&f), Prefs { icon_only: true, ..Prefs::default() }, "missing key: its default");
        std::fs::write(&f, r#"{"murata_zip": "C:/old/grm.zip", "model_zips": ["D:/tdk.zip"]}"#).unwrap();
        assert_eq!(Prefs::load(&f).model_zips, vec!["D:/tdk.zip".to_string(), "C:/old/grm.zip".to_string()], "the old single-zip key is kept");
        std::fs::write(&f, r#"{"ui_scale": 40, "autosave_secs": -5}"#).unwrap();
        assert_eq!(Prefs::load(&f), Prefs::default(), "numbers out of range: the defaults");
        std::fs::write(&f, "not json").unwrap();
        assert_eq!(Prefs::load(&f), Prefs::default());
        let _ = std::fs::remove_dir_all(&d);
    }
}
