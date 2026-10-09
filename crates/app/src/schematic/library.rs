//! Loading and importing library parts.

use super::*;

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
}
