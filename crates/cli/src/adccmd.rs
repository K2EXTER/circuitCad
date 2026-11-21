//! `adc`: what an ADC on the schematic reads at the DC point (`sim_core::adcread`): the code from the data sheet's ideal transfer
//! function for the inputs, the reference and the gain its registers would set.

use crate::session::{obj, opt, options, Session};
use crate::Res;
use serde_json::json;
use sim_core::adcread::{adc_parts, default_setup, read, AdcFilter};

impl Session {
    /// `adc REF [pos=AIN0] [neg=AIN1] [refp=REFP0] [refn=REFN0] [gain=1] [rate=20] [filter=sinc|low_latency] [chop=0|1]` (pins by name or number).
    pub fn adc(&mut self, args: &[String]) -> Res {
        let (pos_args, kv) = options(args);
        let usage = "adc REF [pos=AIN0] [neg=AIN1] [refp=REFP0] [refn=REFN0] [gain=1] [rate=20] [filter=sinc|low_latency] [chop=0|1]   (an ADC: a part whose model has adc_bits)";
        let refdes = pos_args.first().ok_or(usage)?.to_string();
        let parts = adc_parts(&self.sch);
        let (_, bits, pins) = parts.iter().find(|(r, _, _)| *r == refdes).ok_or_else(|| format!("{refdes} は ADC ではありません。ADC: {}", parts.iter().map(|p| p.0.as_str()).collect::<Vec<_>>().join(", ")))?;
        let pin = |v: &str| -> Result<String, String> {
            pins.iter().find(|(n, name)| n == v || name.eq_ignore_ascii_case(v) || name.split('/').any(|x| x.eq_ignore_ascii_case(v))).map(|(n, _)| n.clone()).ok_or_else(|| format!("{refdes} にピン {v} がありません"))
        };
        let mut setup = default_setup(pins);
        for (key, slot) in [("pos", &mut setup.pos), ("neg", &mut setup.neg), ("refp", &mut setup.refp), ("refn", &mut setup.refn)] {
            if let Some(v) = opt(&kv, key) {
                *slot = pin(v)?;
            }
        }
        if let Some(g) = opt(&kv, "gain") {
            setup.gain = g.parse().map_err(|_| format!("gain={g}: 1、2、4 … 128 のような数"))?;
        }
        if let Some(r) = opt(&kv, "rate") {
            setup.rate = r.parse().map_err(|_| format!("rate={r}: 1 秒あたりの変換の回数 (例: 20)"))?;
        }
        if let Some(f) = opt(&kv, "filter") {
            setup.filter = match f {
                "sinc" => AdcFilter::Sinc,
                "low_latency" | "lowlatency" => AdcFilter::LowLatency,
                _ => return Err(format!("filter={f}: sinc か low_latency")),
            };
        }
        if let Some(v) = opt(&kv, "chop") {
            setup.chop = matches!(v, "1" | "on" | "true");
        }
        let c = self.circuit()?;
        let dc = sim_core::dc_op(&c).map_err(|e| e.to_string())?;
        let r = read(&self.sch, &dc, &refdes, &setup)?;
        let name = |n: &str| pins.iter().find(|(p, _)| p == n).map(|x| x.1.clone()).unwrap_or_default();
        Ok(obj(json!({
            "part": refdes, "bits": bits, "pos": name(&setup.pos), "neg": name(&setup.neg), "refp": name(&setup.refp), "refn": name(&setup.refn), "gain": setup.gain,
            "vin": r.vin, "vref": r.vref, "code": r.code, "clipped": r.clipped, "lsb_v": r.lsb, "reads_v": r.volts,
            "rate_sps": setup.rate, "filter": if setup.filter == AdcFilter::Sinc { "sinc" } else { "low_latency" }, "chop": setup.chop,
            "noise": r.noise.as_ref().map(|n| json!({
                "rms_v": n.rms, "pp_v": n.pp, "rms_codes": n.rms_codes, "pp_codes": n.pp_codes,
                "effective_bits": n.effective_bits, "noise_free_bits": n.noise_free_bits, "still_digits": n.still_digits,
            })),
        })))
    }
}
