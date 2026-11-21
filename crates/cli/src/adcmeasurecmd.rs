//! `adcmeasure`: what the ADCs measure, one way per row (`sch_core::adc_measure`, `sim_core::adcmeasure`): a way added to the
//! schematic's table and read, all of them read, or one removed.

use crate::session::{obj, opt, options, Session};
use crate::Res;
use sch_core::adc_measure::{parse_conditions, AdcMeasure, Measured, DEFAULT_DTEMP, DEFAULT_TC_PPM, DEFAULT_TOL_PCT};
use serde_json::{json, Value};
use sim_core::adcmeasure::{read_measure, MeasureReading};

const USAGE: &str = "adcmeasure [NAME volts|ohms|amps plus=NET minus=NET | NAME part part=R7 | NAME temp part=RT1 [r0=10k b=3950 | r0=100 alpha=0.00385]] at=VALUE adc=U1 pos=AIN neg=AIN [gain=1] [rate=20] [filter=sinc|low_latency] [chop=0|1] [when=K1,SW2,R5=10k] [refpos=AIN refneg=AIN] [tol=1] [tc=100] [dtemp=10] | adcmeasure remove NAME";

impl Session {
    /// `adcmeasure` (read every way), `adcmeasure NAME KIND ...` (add or replace one, then read it), `adcmeasure remove NAME`.
    pub fn adc_measure(&mut self, args: &[String]) -> Res {
        let (pos, kv) = options(args);
        if pos.first().map(String::as_str) == Some("remove") {
            let name = pos.get(1).ok_or(USAGE)?;
            let before = self.sch.adc_measures.len();
            self.sch.adc_measures.retain(|r| r.name != *name);
            return if self.sch.adc_measures.len() < before { Ok(obj(json!({"removed": name}))) } else { Err(format!("測り方 {name} はありません")) };
        }
        if let Some(name) = pos.first() {
            let need = |k: &str| opt(&kv, k).map(str::to_string).ok_or_else(|| format!("{k}= がありません\n{USAGE}"));
            let kind = pos.get(1).map(String::as_str).ok_or(USAGE)?;
            let num = |k: &str, d: f64| -> Result<f64, String> { opt(&kv, k).map(|v| sim_core::parse_value(v).ok_or_else(|| format!("{k}={v}: 数ではありません"))).unwrap_or(Ok(d)) };
            let measured = match kind {
                "volts" | "v" => Measured::Volts { plus: need("plus")?, minus: need("minus")? },
                "ohms" | "r" => Measured::Ohms { plus: need("plus")?, minus: need("minus")? },
                "amps" | "a" => Measured::Amps { plus: need("plus")?, minus: need("minus")? },
                "part" => Measured::Part { part: need("part")? },
                "temp" | "t" => {
                    let part = need("part")?;
                    // an NTC's law from its model, unless given
                    let (r0, b) = sim_core::adcmeasure::sensor_law(&self.sch, &part).unwrap_or((0.0, 0.0));
                    let alpha = num("alpha", 0.0)?;
                    let b = if alpha != 0.0 { 0.0 } else { num("b", b)? };
                    Measured::Temp { part, r0: num("r0", r0)?, b, alpha }
                }
                _ => return Err(format!("{kind}: volts・ohms・amps・part・temp のどれか\n{USAGE}")),
            };
            let adc = need("adc")?;
            let parts = sim_core::adcread::adc_parts(&self.sch);
            let (_, _, pins) = parts.iter().find(|(r, _, _)| *r == adc).ok_or_else(|| format!("{adc} は ADC ではありません"))?;
            let pin = |v: &str| -> Result<String, String> {
                pins.iter().find(|(n, nm)| n == v || nm.eq_ignore_ascii_case(v) || nm.split('/').any(|x| x.eq_ignore_ascii_case(v))).map(|(n, _)| n.clone()).ok_or_else(|| format!("{adc} にピン {v} がありません"))
            };
            let m = AdcMeasure {
                name: name.clone(),
                measured,
                at: num("at", f64::NAN)?,
                conditions: opt(&kv, "when").map(parse_conditions).unwrap_or_default(),
                adc: adc.clone(),
                pos: pin(&need("pos")?)?,
                neg: pin(&need("neg")?)?,
                ref_pos: opt(&kv, "refpos").map(pin).transpose()?.unwrap_or_default(),
                ref_neg: opt(&kv, "refneg").map(pin).transpose()?.unwrap_or_default(),
                gain: num("gain", 1.0)?,
                rate: num("rate", 20.0)?,
                low_latency: matches!(opt(&kv, "filter"), Some("low_latency" | "lowlatency")),
                chop: matches!(opt(&kv, "chop"), Some("1" | "on" | "true")),
                tol_pct: num("tol", DEFAULT_TOL_PCT)?,
                tc_ppm: num("tc", DEFAULT_TC_PPM)?,
                dtemp: num("dtemp", DEFAULT_DTEMP)?,
            };
            let reading = read_measure(&self.sch, &m)?;
            let out = row(&m, &reading);
            match self.sch.adc_measures.iter_mut().find(|r| r.name == m.name) {
                Some(r) => *r = m,
                None => self.sch.adc_measures.push(m),
            }
            return Ok(obj(out));
        }
        if self.sch.adc_measures.is_empty() {
            return Err(format!("測り方がまだありません\n{USAGE}"));
        }
        let rows: Vec<Value> = self
            .sch
            .adc_measures
            .iter()
            .map(|m| match read_measure(&self.sch, m) {
                Ok(x) => row(m, &x),
                Err(e) => json!({"name": m.name, "error": e}),
            })
            .collect();
        Ok(obj(json!({"measures": rows})))
    }
}

fn row(m: &AdcMeasure, x: &MeasureReading) -> Value {
    json!({
        "name": m.name, "unit": x.unit, "at": m.at, "when": m.conditions,
        "vin": x.main.vin, "code": x.main.code, "clipped": x.main.clipped || x.reference.as_ref().is_some_and(|q| q.clipped),
        "ref_vin": x.reference.as_ref().map(|q| q.vin), "value": x.value, "slope": x.slope,
        "noise_rms": x.noise_rms, "noise_pp": x.noise_pp, "digits": x.digits, "done": x.done,
        "errors": x.errors.as_ref().map(|e| json!({
            "tol_sum": e.tol_sum, "tol_rss": e.tol_rss, "temp_sum": e.temp_sum, "temp_rss": e.temp_rss, "dtemp": m.dtemp,
            "parts": e.parts.iter().map(|p| json!({"part": p.part, "tol": p.tol, "tc_ppm": p.tc_ppm, "own": p.own, "tol_err": p.tol_err, "temp_err": p.temp_err})).collect::<Vec<_>>(),
            "leaks": e.leaks.iter().map(|l| json!({"part": l.part, "err": l.err})).collect::<Vec<_>>(), "leak_sum": e.leak_sum,
        })),
    })
}
