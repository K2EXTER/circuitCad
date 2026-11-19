//! `inrush`: the current each supply gives the moment it is switched on (`sim_core::inrush`): the peak, I²t, how long it lasts, and
//! the parts on the way against their rated current.

use crate::session::{obj, opt, options, Session};
use crate::Res;
use serde_json::{json, Value};

impl Session {
    /// `inrush [r=0.1] [tstop=20m]`.
    pub fn inrush(&mut self, args: &[String]) -> Res {
        let (_, kv) = options(args);
        let num = |k: &str, d: f64| opt(&kv, k).map_or(Ok(d), |s| sim_core::parse_value(s).ok_or_else(|| format!("{k}={s}: a number")));
        let settings = sim_core::inrush::InrushSettings { r_source: num("r", 0.1)?, tstop: num("tstop", 20e-3)? };
        let c = self.circuit()?;
        let list = sim_core::inrush::inrush(&self.sch, &c, &settings)?;
        let rows: Vec<Value> = list
            .iter()
            .map(|x| {
                json!({
                    "source": x.source, "volts": x.volts, "peak_a": x.peak, "t_peak_s": x.t_peak, "i2t": x.i2t, "steady_a": x.steady,
                    "surge_s": x.surge_s, "end_a": x.end, "large": x.large(), "did_not_recover": x.did_not_recover(),
                    "limit_ohms": x.large().then(|| x.limit_ohms(x.target(), settings.r_source)),
                    "parts": x.parts.iter().map(|p| json!({"part": p.part, "peak_a": p.peak, "rated_a": p.rated, "over": p.over()})).collect::<Vec<_>>(),
                })
            })
            .collect();
        Ok(obj(json!({"supplies": rows})))
    }
}
