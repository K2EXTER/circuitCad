//! Display preferences kept between launches (`%APPDATA%\circuitCad\settings.json`, next to `progress.json`).

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Prefs {
    pub icon_only: bool,
    pub show_hints: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { icon_only: false, show_hints: true }
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
        Self { icon_only: get("icon_only", d.icon_only), show_hints: get("show_hints", d.show_hints) }
    }

    /// Writes the file; the error text says what to do (the setting then only lasts until the app closes).
    pub fn save(&self, file: &Path) -> Result<(), String> {
        let json = serde_json::json!({ "version": 1, "icon_only": self.icon_only, "show_hints": self.show_hints });
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
        let p = Prefs { icon_only: true, show_hints: false };
        p.save(&f).unwrap();
        assert_eq!(Prefs::load(&f), p);
        std::fs::write(&f, r#"{"icon_only": true}"#).unwrap();
        assert_eq!(Prefs::load(&f), Prefs { icon_only: true, show_hints: true }, "missing key: its default");
        std::fs::write(&f, "not json").unwrap();
        assert_eq!(Prefs::load(&f), Prefs::default());
        let _ = std::fs::remove_dir_all(&d);
    }
}
