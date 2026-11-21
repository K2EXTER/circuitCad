//! `adcmeasure`: what the ADCs measure, one way per row (`sch_core::adc_measure`, `sim_core::adcmeasure`): a way added to the
//! schematic's table and read, all of them read, or one removed.

use crate::session::{obj, opt, options, Session};
use crate::Res;
use sch_core::adc_measure::{parse_conditions, AdcMeasure, Measured};
use serde_json::{json, Value};
use sim_core::adcmeasure::{read_measure, MeasureReading};

const USAGE: &str = "adcmeasure [NAME volts|ohms|amps plus=NET minus=NET | NAME part part=R7] at=VALUE adc=U1 pos=AIN neg=AIN [gain=1] [rate=20] [filter=sinc|low_latency] [chop=0|1] [when=K1,SW2,R5=10k] [refpos=AIN refneg=AIN] | adcmeasure remove NAME";

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
            let measured = match kind {
                "volts" | "v" => Measured::Volts { plus: need("plus")?, minus: need("minus")? },
                "ohms" | "r" => Measured::Ohms { plus: need("plus")?, minus: need("minus")? },
                "amps" | "a" => Measured::Amps { plus: need("plus")?, minus: need("minus")? },
                "part" => Measured::Part { part: need("part")? },
                _ => return Err(format!("{kind}: volts・ohms・amps・part のどれか\n{USAGE}")),
            };
            let adc = need("adc")?;
            let parts = sim_core::adcread::adc_parts(&self.sch);
            let (_, _, pins) = parts.iter().find(|(r, _, _)| *r == adc).ok_or_else(|| format!("{adc} は ADC ではありません"))?;
            let pin = |v: &str| -> Result<String, String> {
                pins.iter().find(|(n, nm)| n == v || nm.eq_ignore_ascii_case(v) || nm.split('/').any(|x| x.eq_ignore_ascii_case(v))).map(|(n, _)| n.clone()).ok_or_else(|| format!("{adc} にピン {v} がありません"))
            };
            let num = |k: &str, d: f64| -> Result<f64, String> { opt(&kv, k).map(|v| sim_core::parse_value(v).ok_or_else(|| format!("{k}={v}: 数ではありません"))).unwrap_or(Ok(d)) };
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
    })
}
