//! What an ADC on the schematic reads (candidate L1): its input's voltage at the DC point turned into the output code by the
//! data sheet's ideal transfer function, code = 2^(bits-1) · V(in) · gain / V(ref) (a differential bipolar ADC: the ADS124S08,
//! SBAS660C 9.5.2 "Data Format": 1 LSB = (2·V(ref) / gain) / 2^24), clipped to the full scale. The ADC's part says it is one with the model parameter `adc_bits`;
//! the inputs, the reference and the gain are what its registers set (they are chosen here, not in the model).

use crate::dc::DcResult;
use sch_core::{extract, Schematic};

/// An ADC on the schematic and how it is set up: its inputs, its reference pins (pin numbers of the part) and the gain.
#[derive(Clone, Debug, PartialEq)]
pub struct AdcSetup {
    pub pos: String,
    pub neg: String,
    pub refp: String,
    pub refn: String,
    pub gain: f64,
}

/// What it reads.
#[derive(Clone, Debug, PartialEq)]
pub struct AdcReading {
    /// The input (pos − neg) and the reference (refp − refn), V.
    pub vin: f64,
    pub vref: f64,
    /// The output code, and whether it is at the end of the scale (the input beyond ±V(ref) / gain).
    pub code: i64,
    pub clipped: bool,
    /// One step of the code, as a voltage at the input (V(ref) / gain / 2^(bits-1)).
    pub lsb: f64,
    /// The code back as the input voltage it stands for.
    pub volts: f64,
}

/// An ADC on the schematic: its reference, its resolution in bits, and its pins (number, name).
pub type AdcPart = (String, u32, Vec<(String, String)>);

/// The ADCs on the schematic: each part whose model says `adc_bits`.
pub fn adc_parts(s: &Schematic) -> Vec<AdcPart> {
    s.flat()
        .components
        .iter()
        .filter_map(|c| {
            let sym = c.custom.as_ref()?;
            let bits = *sym.sim.as_ref()?.params.get("adc_bits")?;
            Some((c.refdes.clone(), bits as u32, sym.pins.iter().map(|p| (p.number.clone(), p.name.clone())).collect()))
        })
        .collect()
}

/// The setup a part starts with: the first two analog inputs (AIN0 − AIN1, or the first two pins named AIN...), the reference
/// pins REFP0 / REFN0 (else the first REFP / REFN), gain 1.
pub fn default_setup(pins: &[(String, String)]) -> AdcSetup {
    let named = |pred: &dyn Fn(&str) -> bool| pins.iter().find(|(_, n)| pred(&n.to_ascii_uppercase())).map(|(p, _)| p.clone()).unwrap_or_default();
    let ains: Vec<&(String, String)> = pins.iter().filter(|(_, n)| n.to_ascii_uppercase().starts_with("AIN") && !n.to_ascii_uppercase().starts_with("AINCOM")).collect();
    let refp = named(&|n| n == "REFP0" || n.starts_with("REFP0"));
    let refn = named(&|n| n == "REFN0" || n.starts_with("REFN0"));
    AdcSetup {
        pos: ains.first().map(|x| x.0.clone()).unwrap_or_default(),
        neg: ains.get(1).map(|x| x.0.clone()).unwrap_or_default(),
        refp: if refp.is_empty() { named(&|n| n.starts_with("REFP")) } else { refp },
        refn: if refn.is_empty() { named(&|n| n.starts_with("REFN")) } else { refn },
        gain: 1.0,
    }
}

/// What ADC `refdes` reads at the DC point `dc`.
pub fn read(s: &Schematic, dc: &DcResult, refdes: &str, setup: &AdcSetup) -> Result<AdcReading, String> {
    let (_, bits, _) = adc_parts(s).into_iter().find(|(r, _, _)| r == refdes).ok_or_else(|| format!("{refdes} は ADC (モデルに adc_bits のある部品) ではありません"))?;
    let flat = s.flat();
    let nets = extract(&flat);
    let volts = |pin: &str| -> Result<f64, String> {
        let net = nets.iter().find(|n| n.pins.iter().any(|p| p.refdes == refdes && p.pin == pin)).ok_or_else(|| format!("{refdes} のピン {pin} は、どこにもつながっていません"))?;
        if sch_core::breadboard::is_ground_name(&net.name) {
            return Ok(0.0);
        }
        dc.voltage(&net.name).ok_or_else(|| format!("{} の電圧が DC の結果にありません", net.name))
    };
    if !(setup.gain > 0.0) {
        return Err("利得は 0 より大きい数にしてください (例: 1、2、4 … 128)".into());
    }
    let vin = volts(&setup.pos)? - volts(&setup.neg)?;
    let vref = volts(&setup.refp)? - volts(&setup.refn)?;
    if vref.abs() < 1e-6 {
        return Err(format!("基準電圧 (ピン {} と {} の差) が 0 V です。基準電圧のピンを確かめます", setup.refp, setup.refn));
    }
    let full = 2f64.powi(bits as i32 - 1);
    let raw = (vin * setup.gain / vref * full).round();
    let (lo, hi) = (-full, full - 1.0);
    let code = raw.clamp(lo, hi);
    let lsb = vref / setup.gain / full;
    Ok(AdcReading { vin, vref, code: code as i64, clipped: raw < lo || raw > hi, lsb, volts: code * lsb })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_code_follows_the_data_sheets_transfer_function() {
        let pins: Vec<(String, String)> = [("7", "AIN0"), ("6", "AIN1"), ("1", "AINCOM"), ("30", "REFP0"), ("29", "REFN0")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        let d = default_setup(&pins);
        assert_eq!((d.pos.as_str(), d.neg.as_str(), d.refp.as_str(), d.refn.as_str(), d.gain), ("7", "6", "30", "29", 1.0));
    }
}
