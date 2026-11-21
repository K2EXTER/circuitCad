//! `libsync`: the placed parts whose copy is older than the library (`part_lib::placed`, candidate Y1), listed, and brought up to
//! date with `apply=1` (the copy's pins kept where the library's differ).

use crate::session::{obj, opt, options, Session};
use crate::Res;
use serde_json::json;
use part_lib::placed::{refresh, stale};

impl Session {
    /// `libsync [apply=1] [parts=R1,K1]`.
    pub fn libsync(&mut self, args: &[String]) -> Res {
        let (_, kv) = options(args);
        let found: Vec<serde_json::Value> = stale(&self.sch, &self.lib).iter().map(|s| json!({"part": s.refdes, "library": s.part, "changes": s.changes, "pins_kept": s.pins_differ})).collect();
        let mut out = json!({"stale": found});
        if opt(&kv, "apply").is_some_and(|v| v == "1") {
            let only: Vec<String> = opt(&kv, "parts").map(|v| v.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()).unwrap_or_default();
            let lib = self.lib.clone();
            out["updated"] = json!(refresh(&mut self.sch, &lib, &only));
        }
        Ok(obj(out))
    }
}
