//! `battery`: how long the battery parts of the schematic last (`sim_core::battery`): the average and peak current of a transient
//! run, the charge at which a condition (a net staying at or above a voltage) fails, and the hours.

use crate::session::{obj, opt, options, Session};
use crate::Res;
use serde_json::{json, Value};

impl Session {
    /// `battery tran=STOP[:STEP] [from=T] [to=T] [node=NET v_min=V]`.
    pub fn battery(&mut self, args: &[String]) -> Res {
        let (_, kv) = options(args);
        let usage = "battery tran=STOP[:STEP] [from=T] [to=T] [node=NET v_min=V]   (how long the battery parts last: the average current over from..to of the transient run (default: its second half; for an intermittent load take whole periods), the charge at which node falls under v_min (default: the battery's own end), the hours)";
        let tran = opt(&kv, "tran").ok_or(usage)?;
        let (stop, step) = tran.split_once(':').unwrap_or((tran, ""));
        let tstop = sim_core::parse_value(stop).filter(|t| *t > 0.0).ok_or_else(|| format!("tran={tran}: the end time in s, for example 300m"))?;
        let tstep = if step.is_empty() { sim_core::auto_tstep(tstop) } else { sim_core::parse_value(step).filter(|t| *t > 0.0).ok_or_else(|| format!("tran={tran}: the step after ':' in s, for example 50u"))? };
        let time = |k: &str| opt(&kv, k).map_or(Ok(0.0), |s| sim_core::parse_value(s).filter(|t| *t >= 0.0).ok_or_else(|| format!("{k}={s}: a time in s")));
        let (from, to) = (time("from")?, time("to")?);
        let node = opt(&kv, "node").unwrap_or("").to_string();
        let v_min = match (node.is_empty(), opt(&kv, "v_min")) {
            (true, _) => 0.0,
            (false, Some(v)) => sim_core::parse_value(v).ok_or_else(|| format!("v_min={v}: a voltage, for example 3.2"))?,
            (false, None) => return Err("node= needs v_min= (the lowest voltage at which the circuit still works)".into()),
        };
        let c = self.circuit()?;
        let tr = sim_core::transient(&c, &sim_core::TranSettings { tstop, tstep, uic: false }).map_err(|e| e.to_string())?;
        let (wiring, couplings) = self.layout_parts();
        let build = |s: &sch_core::Schematic| sim_core::from_schematic_with_layout(s, &wiring, &couplings).map_err(|es| es.iter().map(|e| format!("{}: {}", e.refdes, e.message)).collect::<Vec<_>>().join("; "));
        let list = sim_core::battery::battery_life(&self.sch, &tr, from, to, &sim_core::battery::BatteryCondition { node, v_min }, &build)?;
        let rows: Vec<Value> = list
            .iter()
            .map(|b| {
                json!({
                    "battery": b.battery.refdes, "part": b.battery.part, "capacity_mah": b.battery.capacity_mah,
                    "from_s": b.from, "to_s": b.to, "avg_a": b.i_avg, "peak_a": b.i_peak,
                    "full_v": b.full_volts, "stop_charge": b.stop_soc, "stop_v": b.stop_volts, "stopped_by_condition": b.by_condition,
                    "hours": b.hours, "days": b.hours / 24.0, "text": sim_core::battery::hours_text(b.hours), "notes": b.notes,
                })
            })
            .collect();
        Ok(obj(json!({"batteries": rows})))
    }
}
