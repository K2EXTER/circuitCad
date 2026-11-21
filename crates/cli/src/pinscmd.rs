//! `pins`: the signals on the microcontroller board's pins (`sch_core::pindefs`, candidates W1 and W2): listed, written as a C
//! header for Arduino, or a firmware file checked against the schematic.

use crate::session::{obj, opt, options, Session};
use crate::Res;
use serde_json::json;
use sch_core::pindefs::{check_firmware, header_text, pin_defs};

impl Session {
    /// `pins [export=FILE.h] [check=FILE.ino]`.
    pub fn pins(&mut self, args: &[String]) -> Res {
        let (_, kv) = options(args);
        let defs: Vec<serde_json::Value> = pin_defs(&self.sch).iter().map(|d| json!({"name": d.name, "pin": d.pin, "value": d.value, "at": d.at, "net": d.net, "others": d.others})).collect();
        let mut out = json!({"pins": defs});
        if let Some(path) = opt(&kv, "export") {
            let title = std::path::Path::new(path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            std::fs::write(path, header_text(&self.sch, &title)).map_err(|e| format!("export={path}: {e}"))?;
            out["written"] = json!(path);
        }
        if let Some(path) = opt(&kv, "check") {
            let text = std::fs::read_to_string(path).map_err(|e| format!("check={path}: {e}"))?;
            let findings: Vec<serde_json::Value> = check_firmware(&self.sch, &text).iter().map(|f| json!({"message": f.message, "hint": f.hint, "line": f.line, "wrong": f.wrong})).collect();
            out["wrong"] = json!(findings.iter().filter(|f| f["wrong"] == true).count());
            out["findings"] = json!(findings);
        }
        Ok(obj(out))
    }
}
