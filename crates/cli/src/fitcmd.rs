//! `fit FILE.fit.json [write] [dir=DIR]`: an equivalent model from data sheet readings (`part_lib::fit`). Answers the
//! `.model` card, the notes shown with it, and how far the model lands from each reading; `write` puts the model into
//! `<DIR>/<part>.part.json` (DIR defaults to the session's library) and reloads that part.

use crate::Res;
use crate::session::{obj, opt, options};
use part_lib::fit;
use serde_json::json;
use std::path::PathBuf;

impl crate::session::Session {
    pub fn fit(&mut self, args: &[String]) -> Res {
        let (words, kv) = options(args);
        let file = words.iter().find(|w| !w.eq_ignore_ascii_case("write")).ok_or("fit FILE.fit.json [write] [dir=DIR] (examples: crates/part-lib/fits/)")?;
        let spec = fit::Spec::load(&PathBuf::from(file.as_str()))?;
        let f = fit::fit(&spec)?;
        let rows: Vec<_> = f
            .rows
            .iter()
            .map(|r| {
                let mut v = json!({"item": r.item, "at": r.at, "datasheet": r.datasheet, "model": r.model, "unit": r.unit, "error_pct": (r.error_pct() * 100.0).round() / 100.0});
                if r.compare_only {
                    v["compare_only"] = json!(true);
                }
                v
            })
            .collect();
        let worst = f.rows.iter().filter(|r| !r.compare_only).map(|r| r.error_pct().abs()).fold(0.0, f64::max);
        let mut out = json!({"part": f.part, "model": f.text, "worst_error_pct": (worst * 10.0).round() / 10.0, "rows": rows, "notes": f.notes, "limits": f.limits});
        if words.iter().any(|w| w.eq_ignore_ascii_case("write")) {
            let dir = opt(&kv, "dir").map(PathBuf::from).unwrap_or_else(|| self.lib_dir.clone());
            let path = fit::write_part(&dir, &f)?;
            if dir == self.lib_dir
                && let Ok(def) = std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<part_lib::PartDef>(&t).map_err(|e| e.to_string()))
            {
                match self.lib.iter_mut().find(|p| p.name == def.name) {
                    Some(p) => *p = def,
                    None => self.lib.push(def),
                }
            }
            out["written"] = json!(path.display().to_string());
        }
        Ok(obj(out))
    }
}
