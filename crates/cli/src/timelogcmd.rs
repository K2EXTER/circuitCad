//! `timelog FILE [temp=COLUMN]`: a meter's log over time summed up per column (`sim_core::timelog`): the mean, the spread, the
//! drift per hour and, against a temperature column, the temperature coefficient.

use crate::session::{obj, opt, options, Session};
use crate::Res;
use serde_json::json;
use sim_core::timelog::{column, read_log, stats};

impl Session {
    pub fn time_log(&mut self, args: &[String]) -> Res {
        let (pos, kv) = options(args);
        let f = pos.first().ok_or("timelog FILE.csv [temp=COLUMN]   (a log with a time (10:00:01 or seconds) and readings per row; columns by the header's names)")?;
        let text = std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?;
        let log = read_log(&text)?;
        let temp = match opt(&kv, "temp") {
            Some(name) => Some(column(&log, name).ok_or_else(|| format!("列 {name} がありません (列: {})", log.names.join(", ")))?),
            None => None,
        };
        let cols: Vec<serde_json::Value> = stats(&log, temp)
            .iter()
            .map(|s| json!({"name": s.name, "n": s.n, "mean": s.mean, "min": s.min, "max": s.max, "pp": s.max - s.min, "std": s.std, "drift_per_hour": s.drift_per_hour, "tempco": s.tempco, "tempco_ppm": s.tempco_ppm}))
            .collect();
        Ok(obj(json!({"file": f, "rows": log.t.len(), "hours": log.t.last().copied().unwrap_or(0.0) / 3600.0, "temp": temp.map(|k| log.names[k].clone()), "columns": cols})))
    }
}
