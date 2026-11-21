//! What an ADC on the schematic reads (candidate L1): its input's voltage at the DC point turned into the output code by the
//! data sheet's ideal transfer function, code = 2^(bits-1) · V(in) · gain / V(ref) (a differential bipolar ADC: the ADS124S08,
//! SBAS660C 9.5.2 "Data Format": 1 LSB = (2·V(ref) / gain) / 2^24), clipped to the full scale. The ADC's part says it is one with the model parameter `adc_bits`;
//! the inputs, the reference and the gain are what its registers set (they are chosen here, not in the model).
//!
//! How much the reading wanders (the noise): for a part whose data sheet tables are here (`NOISE_TABLES`: the ADS124S06 / 08,
//! SBAS660C 8.1 Tables 1, 3, 5, 7), the input-referred noise at the chosen data rate, filter, global chop and gain, as volts, as
//! codes, and as the effective and noise-free resolutions of the data sheet's Equations 1 and 2 (with the reference actually used).

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
    /// The data rate (samples per second), the digital filter and global chop: what the noise depends on.
    pub rate: f64,
    pub filter: AdcFilter,
    pub chop: bool,
}

/// The ADC's digital filter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AdcFilter {
    #[default]
    Sinc,
    LowLatency,
}

/// How much the reading wanders, referred to the input.
#[derive(Clone, Debug, PartialEq)]
pub struct AdcNoise {
    /// RMS and peak-to-peak, V.
    pub rms: f64,
    pub pp: f64,
    /// The same in codes (steps of the reading).
    pub rms_codes: f64,
    pub pp_codes: f64,
    /// Effective resolution, ln(2·V(ref)/gain / V(rms)) / ln 2, and noise-free resolution (the same with V(pp)), bits.
    pub effective_bits: f64,
    pub noise_free_bits: f64,
    /// The decimal digits that stay still: log10 of the span over the peak-to-peak noise.
    pub still_digits: f64,
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
    /// The noise at this setup, when the part's data sheet table is here.
    pub noise: Option<AdcNoise>,
}

/// The data sheets' noise tables: the part names they are for (the start of the name), and the table.
const NOISE_TABLES: &[(&str, &str)] = &[("ADS124S0", include_str!("adcnoise_ads124s0x.txt"))];

/// The gains the noise tables have, in the order of their columns.
pub const NOISE_GAINS: [f64; 8] = [1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0];

/// The rows of one table: rate, then (rms, pp) in uV for each of `NOISE_GAINS`.
fn noise_rows(text: &str, filter: AdcFilter, chop: bool) -> Vec<Vec<f64>> {
    let want = match (filter, chop) {
        (AdcFilter::Sinc, false) => "sinc",
        (AdcFilter::LowLatency, false) => "low_latency",
        (AdcFilter::Sinc, true) => "sinc_chop",
        (AdcFilter::LowLatency, true) => "low_latency_chop",
    };
    let mut on = false;
    let mut out = vec![];
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        if let Some(name) = line.strip_prefix("table ") {
            on = name.trim() == want;
        } else if on {
            out.push(line.split_whitespace().filter_map(|x| x.parse().ok()).collect());
        }
    }
    out
}

fn noise_table(part: &str) -> Option<&'static str> {
    let up = part.to_ascii_uppercase();
    NOISE_TABLES.iter().find(|(p, _)| up.starts_with(p)).map(|x| x.1)
}

/// The data rates the part's noise table has (empty when there is no table).
pub fn noise_rates(part: &str) -> Vec<f64> {
    noise_table(part).map(|t| noise_rows(t, AdcFilter::Sinc, false).iter().map(|r| r[0]).collect()).unwrap_or_default()
}

/// The noise of part `part` (its library name) at `setup`, with the reference `vref` and `bits` of resolution: the data
/// sheet's figure at that rate, filter, chop and gain (None when the part has no table here, or the rate or gain is not in it).
pub fn noise(part: &str, setup: &AdcSetup, vref: f64, bits: u32) -> Option<AdcNoise> {
    let rows = noise_rows(noise_table(part)?, setup.filter, setup.chop);
    let col = NOISE_GAINS.iter().position(|g| (g - setup.gain).abs() < 1e-9)?;
    let row = rows.iter().find(|r| (r[0] - setup.rate).abs() < 1e-6)?;
    let (rms, pp) = (row[1 + 2 * col] * 1e-6, row[2 + 2 * col] * 1e-6);
    let span = 2.0 * vref.abs() / setup.gain;
    let lsb = span / 2f64.powi(bits as i32);
    Some(AdcNoise {
        rms,
        pp,
        rms_codes: rms / lsb,
        pp_codes: pp / lsb,
        effective_bits: (span / rms).log2(),
        noise_free_bits: (span / pp).log2(),
        still_digits: (span / pp).log10(),
    })
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
        rate: 20.0,
        filter: AdcFilter::Sinc,
        chop: false,
    }
}

/// What ADC `refdes` reads at the DC point `dc`.
pub fn read(s: &Schematic, dc: &DcResult, refdes: &str, setup: &AdcSetup) -> Result<AdcReading, String> {
    let (_, bits, _) = adc_parts(s).into_iter().find(|(r, _, _)| r == refdes).ok_or_else(|| format!("{refdes} は ADC (モデルに adc_bits のある部品) ではありません"))?;
    let flat = s.flat();
    let part = flat.components.iter().find(|c| c.refdes == refdes).and_then(|c| c.custom.as_ref()).map(|c| c.name.clone()).unwrap_or_default();
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
    Ok(AdcReading { vin, vref, code: code as i64, clipped: raw < lo || raw > hi, lsb, volts: code * lsb, noise: noise(&part, setup, vref, bits) })
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

    fn setup_at(rate: f64, gain: f64, filter: AdcFilter, chop: bool) -> AdcSetup {
        AdcSetup { pos: String::new(), neg: String::new(), refp: String::new(), refn: String::new(), gain, rate, filter, chop }
    }

    /// The noise read from the tables gives back the data sheet's own resolution tables (Tables 2, 4 and 6, 2.5 V reference) to
    /// their rounding, at a few rates, gains, filters and chop.
    #[test]
    fn the_noise_tables_give_back_the_data_sheets_resolutions() {
        // (rate, gain, filter, chop, effective, noise-free) from Tables 2, 4 and 6
        for (rate, gain, filter, chop, eff, nf) in [
            (20.0, 1.0, AdcFilter::Sinc, false, 22.6, 20.0),
            (2.5, 128.0, AdcFilter::Sinc, false, 21.0, 18.4),
            (4000.0, 1.0, AdcFilter::Sinc, false, 18.4, 15.7),
            (100.0, 16.0, AdcFilter::LowLatency, false, 20.0, 17.5),
            (20.0, 1.0, AdcFilter::Sinc, true, 23.0, 20.5),
            (2.5, 1.0, AdcFilter::LowLatency, false, 23.4, 20.8),
        ] {
            let n = noise("ADS124S08_VQFN", &setup_at(rate, gain, filter, chop), 2.5, 24).expect("in the table");
            assert!((n.effective_bits - eff).abs() < 0.11 && (n.noise_free_bits - nf).abs() < 0.11, "{rate} SPS gain {gain} {filter:?} chop {chop}: {n:?}");
        }
        let n = noise("ADS124S08", &setup_at(20.0, 1.0, AdcFilter::Sinc, false), 2.5, 24).unwrap();
        assert!((n.rms - 0.81e-6).abs() < 1e-12 && (n.rms_codes - 0.81e-6 / (5.0 / 2f64.powi(24))).abs() < 1e-6);
        assert_eq!(noise_rates("ADS124S08_TQFP").len(), 14);
        assert!(noise_rates("LM358").is_empty());
        assert!(noise("ADS124S08", &setup_at(30.0, 1.0, AdcFilter::Sinc, false), 2.5, 24).is_none(), "a rate not in the table");
    }
}
