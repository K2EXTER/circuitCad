//! `poweron`: the first power-on of a built board (`sim_core::power_on`): what each supply should deliver, the range of a working
//! board, the current limit to set, and the verdict on a reading with the causes to look at.

use crate::session::{obj, opt, options, Session};
use crate::Res;
use serde_json::{json, Value};
use sim_core::PowerOnReading as Reading;

fn verdict_key(r: Reading) -> &'static str {
    match r {
        Reading::Nothing => "nothing",
        Reading::MuchLow => "much_low",
        Reading::Low => "low",
        Reading::Normal => "normal",
        Reading::High => "high",
        Reading::MuchHigh => "much_high",
    }
}

impl Session {
    /// `poweron [tol_r=5] [tol_v=0] [temp=25] [margin=1.5] [spread=on|off] [measured=12m] [supply=V1]`.
    pub fn poweron(&mut self, args: &[String]) -> Res {
        let (_, kv) = options(args);
        let num = |k: &str, d: f64| opt(&kv, k).map_or(Ok(d), |s| sim_core::parse_value(s).filter(|x| x.is_finite()).ok_or_else(|| format!("{k}={s} is not a number")));
        let spreads = match opt(&kv, "spread") {
            None | Some("on") => true,
            Some("off") => false,
            Some(s) => return Err(format!("spread={s}: on (the models' data sheet min / max take part) or off (the models' own values)")),
        };
        let s = sim_core::PowerOnSettings { tol_r: num("tol_r", 5.0)? / 100.0, tol_v: num("tol_v", 0.0)? / 100.0, ambient: num("temp", 25.0)?, margin: num("margin", 1.5)?, spreads, ..Default::default() };
        let measured = opt(&kv, "measured").map(|m| sim_core::parse_value(m).filter(|x| x.is_finite()).ok_or_else(|| format!("measured={m}: a current in A, for example 12m or 0.012"))).transpose()?;
        let gate = self.erc_gate()?;
        let c = self.circuit()?;
        let g: std::collections::BTreeMap<String, f64> = self.heat_copper().iter().map(|(k, v)| (k.clone(), v.0)).collect();
        let heaters = sim_core::heaters_from_schematic(&self.sch, &c, &g);
        let p = sim_core::power_on(&c, &heaters, &s).map_err(|e| e.to_string())?;
        let supplies: Vec<Value> = p
            .supplies
            .iter()
            .map(|x| {
                let parts: Vec<Value> = x.parts.iter().take(5).map(|(n, lo, hi)| json!({"part": n, "at_low_end_a": lo, "at_high_end_a": hi})).collect();
                json!({"supply": x.name, "volts": x.volts, "nominal_a": x.nominal, "warm_a": x.warm, "min_a": x.min, "max_a": x.max, "limit_a": x.limit, "parts": parts})
            })
            .collect();
        let reading = match measured {
            None => Value::Null,
            Some(m) => {
                let want = opt(&kv, "supply");
                let x = match want {
                    Some(n) => p.supplies.iter().find(|x| x.name.eq_ignore_ascii_case(n)).ok_or_else(|| format!("supply={n}: no such supply (have: {})", p.supplies.iter().map(|x| x.name.as_str()).collect::<Vec<_>>().join(" ")))?,
                    None => &p.supplies[0],
                };
                let v = x.judge(m);
                json!({"supply": x.name, "measured_a": m, "verdict": verdict_key(v), "title": v.title(), "cut_power_now": v.cut_now(), "causes": v.causes()})
            }
        };
        let mut notes = vec![
            "min_a..max_a: the parts at the ends of their tolerance (tol_r, tol_v) and the models' data sheet min / max (spread), solved as corners, widened by the self-heating (warm_a) on the side it moves".to_string(),
            "limit_a: max_a times margin, rounded up to 1-1.2-1.5-2-2.5-3-4-5-6-8, never under 10 mA; a moment at the limit when the output turns on is the capacitors charging".to_string(),
            "parts without a model (a microcontroller's firmware, what is plugged into a connector) are not in the range: compare with their data sheet's supply current too".to_string(),
        ];
        if !p.heated {
            notes.push("no part warms itself here (or the warm point did not solve): the range has no self-heating".to_string());
        }
        notes.extend(gate.clone());
        Ok(obj(json!({
            "circuit_ok": gate.is_none(), "supplies": supplies, "heated": p.heated, "unsolved_parts": p.unsolved, "reading": reading,
            "steps": [
                "check for solder bridges and the direction of electrolytics, diodes and IC pin 1; measure supply to GND with the ohmmeter (no short)",
                "set the supply voltage and limit_a, connect with the output off",
                "turn the output on; read the current once it settles",
                "run again with measured=<current> for the verdict"
            ],
            "notes": notes
        })))
    }
}
