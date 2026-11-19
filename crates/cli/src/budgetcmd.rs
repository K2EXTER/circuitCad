//! `budget`: the power budget of the sheet's supply rails at the DC point (`sim_core::budget`): per rail what feeds it, its current and
//! loads, the rated current, how much more it could give and what stops it, and a regulator's heat, temperature and efficiency.

use crate::session::{obj, opt, options, Session};
use crate::Res;
use serde_json::{json, Value};

impl Session {
    /// `budget [temp=25]`.
    pub fn budget(&mut self, args: &[String]) -> Res {
        let (_, kv) = options(args);
        let ambient = match opt(&kv, "temp") {
            Some(t) => t.parse::<f64>().map_err(|_| format!("temp={t}: the surroundings' temperature in °C, for example 25"))?,
            None => 25.0,
        };
        let c = self.circuit()?;
        let dc = sim_core::dc_op(&c).map_err(|e| e.to_string())?;
        let rails = sim_core::budget::power_budget(&self.sch, &c, &dc, ambient);
        let rows: Vec<Value> = rails
            .iter()
            .map(|r| {
                json!({
                    "net": r.net, "volts": r.volts, "source": r.source, "regulator": r.regulator, "amps": r.amps,
                    "parent": r.parent.map(|p| rails[p].net.clone()), "depth": r.depth, "rated_a": r.rated,
                    "headroom_a": r.headroom.map(|(h, _)| h), "limited_by": r.headroom.map(|(_, w)| w.label()),
                    "input_v": r.input.map(|(v, _)| v), "input_a": r.input.map(|(_, i)| i),
                    "heat_w": r.heat_w, "temp_c": r.temp_c, "t_max_c": r.t_max, "efficiency": r.efficiency, "tight": r.tight(),
                    "loads": r.loads.iter().map(|l| json!({"part": l.part, "amps": l.amps, "feeds_rail": l.feeds_rail})).collect::<Vec<_>>(),
                })
            })
            .collect();
        Ok(obj(json!({"rails": rows})))
    }
}
