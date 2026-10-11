//! Loading and importing library parts.

use super::*;

const PREFS_FILE: &str = "favorites.json";
const RECENT_MAX: usize = 8;

/// Which library parts the learner starred and placed last. Kept by part name in `favorites.json` beside the parts.
#[derive(Default, Clone)]
pub(super) struct LibPrefs {
    pub fav: Vec<String>,
    pub recent: Vec<String>,
}

impl LibPrefs {
    pub fn load(dir: &Path) -> Self {
        let v: serde_json::Value = std::fs::read_to_string(dir.join(PREFS_FILE)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let list = |k: &str| v[k].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        Self { fav: list("fav"), recent: list("recent") }
    }

    pub fn save(&self, dir: &Path) {
        if let Ok(s) = serde_json::to_string_pretty(&serde_json::json!({ "fav": self.fav, "recent": self.recent })) {
            let _ = std::fs::write(dir.join(PREFS_FILE), s);
        }
    }

    pub fn is_fav(&self, name: &str) -> bool {
        self.fav.iter().any(|n| n == name)
    }

    pub fn toggle_fav(&mut self, name: &str) {
        match self.fav.iter().position(|n| n == name) {
            Some(i) => {
                self.fav.remove(i);
            }
            None => self.fav.push(name.to_string()),
        }
    }

    pub fn touch(&mut self, name: &str) {
        self.recent.retain(|n| n != name);
        self.recent.insert(0, name.to_string());
        self.recent.truncate(RECENT_MAX);
    }
}

impl SchEditor {
    pub(super) fn reload_library(&mut self) {
        let (lib, errs) = part_lib::load_dir(&self.lib_dir);
        self.library = lib;
        self.lib_errors = errs;
        self.place_lib = None;
    }

    pub(super) fn import_part_json(&mut self) {
        let Some(p) = rfd::FileDialog::new().add_filter("部品定義 JSON", &["json"]).pick_file() else { return };
        let def: Result<PartDef, String> =
            std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()));
        self.status = match def {
            Err(e) => format!("読み込み失敗: {e}"),
            Ok(d) => match d.validate().into_iter().find(|i| i.error) {
                Some(i) => format!("{} に問題があります: {}", d.name, i.message),
                None => match part_lib::save(&self.lib_dir, &d) {
                    Ok(_) => {
                        self.reload_library();
                        format!("ライブラリに追加しました: {}", d.name)
                    }
                    Err(e) => format!("保存失敗: {e}"),
                },
            },
        };
    }

    /// Put a library part on the cursor, and remember it as recently used.
    pub(super) fn choose_library_part(&mut self, i: usize) {
        self.tool = Tool::Place;
        self.place_lib = Some(i);
        self.pending = None;
        if let Some(d) = self.library.get(i) {
            let name = d.name.clone();
            self.lib_prefs.touch(&name);
            self.lib_prefs.save(&self.lib_dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn favorites_and_recent_parts_are_kept_between_runs() {
        let dir = std::env::temp_dir().join(format!("circuitcad_prefs_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut p = LibPrefs::default();
        p.toggle_fav("LM358");
        p.toggle_fav("2N7000");
        p.toggle_fav("LM358");
        for n in ["A", "B", "A"] {
            p.touch(n);
        }
        for i in 0..20 {
            p.touch(&format!("P{i}"));
        }
        assert_eq!(p.fav, vec!["2N7000"]);
        assert_eq!(p.recent.len(), RECENT_MAX);
        p.save(&dir);
        let q = LibPrefs::load(&dir);
        assert_eq!((q.fav, q.recent[0].as_str()), (vec!["2N7000".to_string()], "P19"));
        assert!(LibPrefs::load(&dir.join("none")).fav.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
