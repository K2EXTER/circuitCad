//! What an ADC reads for each way of measuring in the schematic's table (`sch_core::adc_measure::AdcMeasure`, candidate L1): the
//! conditions are set (a latching relay set, a switch thrown, a part's value replaced), the thing measured is put in (a voltage,
//! a resistance or a current between two nets, or a part's own value), and the DC point gives what the ADC reads (`adcread`). Read
//! at the value and at 0.1 % more, the slope turns the ADC's noise (the data sheet's table) into the measured unit: how much the
//! reading wanders, and how many of its digits stay still at that value.
//!
//! A latching relay is set by putting its coil's voltage across its set coil (the model's `.subckt` ports `setp` and `setn`); a
//! switch is thrown by the source its model keeps its position in (`Vpress` of a push button, `Vpos` of a slide switch) at 1 V. A
//! board fed through its header (no supply of its own on the schematic) is fed from the supply-named nets at its connectors.

use crate::adcread::{adc_parts, default_setup, read, AdcFilter, AdcReading, AdcSetup};
use crate::circuit::{Circuit, Element};
use crate::waveform::Waveform;
use sch_core::adc_measure::{condition, AdcMeasure, Measured};
use sch_core::{extract, Schematic};

/// What one way of measuring reads.
#[derive(Clone, Debug, PartialEq)]
pub struct MeasureReading {
    /// The ADC's reading at the value (the first input), and of the second input of a ratio reading.
    pub main: AdcReading,
    pub reference: Option<AdcReading>,
    /// What is computed from it: the input's volts, or the ratio of the two.
    pub value: f64,
    /// How much `value` changes per unit measured.
    pub slope: f64,
    /// The unit measured (V, Ω, A).
    pub unit: &'static str,
    /// The noise in the measured unit: RMS and peak-to-peak. None when the ADC has no noise table for the setup.
    pub noise_rms: Option<f64>,
    pub noise_pp: Option<f64>,
    /// The decimal digits that stay still at the value: log10(value / peak-to-peak noise).
    pub digits: Option<f64>,
    /// What was done to the circuit: the supplies fed from net names, the relays set, the switches thrown, the values replaced.
    pub done: Vec<String>,
}

/// The ADC setup of the first (or, with `second`, the reference) input.
fn setup(s: &Schematic, m: &AdcMeasure, second: bool) -> Result<AdcSetup, String> {
    let (_, _, pins) = adc_parts(s).into_iter().find(|(x, _, _)| *x == m.adc).ok_or_else(|| format!("{} は ADC (モデルに adc_bits のある部品) ではありません", m.adc))?;
    let d = default_setup(&pins);
    let (pos, neg) = if second { (m.ref_pos.clone(), m.ref_neg.clone()) } else { (m.pos.clone(), m.neg.clone()) };
    Ok(AdcSetup { pos, neg, refp: d.refp, refn: d.refn, gain: m.gain, rate: m.rate, filter: if m.low_latency { AdcFilter::LowLatency } else { AdcFilter::Sinc }, chop: m.chop })
}

/// The coil voltage of a relay: the number before V at the end of its part name (TQ2-L2-5V → 5), else 5.
fn coil_volts(name: &str) -> f64 {
    let up = name.to_ascii_uppercase();
    let body = up.strip_suffix('V').unwrap_or("");
    let digits: String = body.chars().rev().take_while(|c| c.is_ascii_digit() || *c == '.').collect::<Vec<_>>().into_iter().rev().collect();
    digits.parse().ok().filter(|v: &f64| *v > 0.0).unwrap_or(5.0)
}

/// Sets the value of the circuit's element `name` (a resistor, a voltage or current source). Returns its unit, or None when there
/// is no such element.
fn set_value(c: &mut Circuit, name: &str, x: f64) -> Option<&'static str> {
    for e in c.elements.iter_mut() {
        match e {
            Element::Resistor { name: n, r, .. } if n.eq_ignore_ascii_case(name) => {
                *r = x;
                return Some("Ω");
            }
            Element::VSource { name: n, wave, .. } if n.eq_ignore_ascii_case(name) => {
                *wave = Waveform::Dc(x);
                return Some("V");
            }
            Element::ISource { name: n, wave, .. } if n.eq_ignore_ascii_case(name) => {
                *wave = Waveform::Dc(x);
                return Some("A");
            }
            _ => {}
        }
    }
    None
}

/// The circuit of the schematic with the conditions set and the supplies fed; returns what was done.
fn circuit(s: &Schematic, m: &AdcMeasure) -> Result<(Circuit, Vec<String>), String> {
    let mut c = crate::from_schematic(s).map_err(|e| e.first().map(|e| e.to_string()).unwrap_or_default())?;
    let mut done = vec![];
    if crate::supply_sources(&c).is_empty() {
        for (net, v) in sch_core::build_order::connector_rails(s) {
            if c.names.iter().any(|n| n.eq_ignore_ascii_case(&net)) {
                let p = c.node(&net);
                c.elements.push(Element::VSource { name: format!("V_{net}"), p, n: 0, wave: Waveform::Dc(v), ac: None });
                done.push(format!("{net} = {v} V"));
            }
        }
    }
    let flat = s.flat();
    let nets = extract(&flat);
    for text in &m.conditions {
        let (k, value) = condition(text);
        if let Some(v) = value {
            let x = crate::parse_value(&v).ok_or_else(|| format!("{text}: 値 {v} を数にできません"))?;
            set_value(&mut c, &k, x).ok_or_else(|| format!("{text}: {k} は値を置き換えられる部品 (抵抗・電圧源・電流源) ではありません"))?;
            done.push(format!("{k} = {v}"));
            continue;
        }
        let comp = flat.components.iter().find(|c| c.refdes == k).ok_or_else(|| format!("{k} が回路図にありません"))?;
        let sim = comp.custom.as_ref().and_then(|s| s.sim.as_ref()).ok_or_else(|| format!("{k} にはモデルがないので、状態を変えられません"))?;
        // a switch: the source its model keeps the position in, at 1 V
        let holds = |src: &str| sim.text.lines().any(|l| l.split_whitespace().next().is_some_and(|w| w.eq_ignore_ascii_case(src)));
        if let Some(src) = ["Vpress", "Vpos"].into_iter().find(|s| holds(s)) {
            set_value(&mut c, &format!("{k}.{src}"), 1.0).ok_or_else(|| format!("{k} のスイッチの位置 ({src}) が回路にありません"))?;
            done.push(format!("{k} {}", if src == "Vpress" { "押す" } else { "B 側" }));
            continue;
        }
        // a latching relay: its set coil energized
        let ports: Vec<String> = sim.text.lines().find(|l| l.trim_start().to_ascii_lowercase().starts_with(".subckt")).map(|l| l.split_whitespace().skip(2).map(|x| x.to_ascii_lowercase()).collect()).unwrap_or_default();
        let pin_of = |port: &str| ports.iter().position(|p| p == port).and_then(|i| sim.pins.get(i)).cloned();
        let (Some(sp), Some(sn)) = (pin_of("setp"), pin_of("setn")) else {
            return Err(format!("{k} は、状態を保つリレーでもスイッチでもありません (値を変えるときは {k}=値 と書きます)"));
        };
        let net_of = |pin: &str| nets.iter().find(|n| n.pins.iter().any(|p| p.refdes == k && p.pin == pin)).map(|n| n.name.clone()).ok_or_else(|| format!("{k} のピン {pin} がどこにもつながっていません"));
        let (a, b) = (net_of(&sp)?, net_of(&sn)?);
        let v = coil_volts(&comp.custom.as_ref().map(|s| s.name.clone()).unwrap_or_default());
        let (p, n) = (c.node(&a), c.node(&b));
        c.elements.push(Element::VSource { name: format!("V_SET_{k}"), p, n, wave: Waveform::Dc(v), ac: None });
        done.push(format!("{k} セット"));
    }
    Ok((c, done))
}

/// The circuit with the measured thing at value `x`, and its unit.
fn with_value(c: &Circuit, m: &AdcMeasure, x: f64) -> Result<(Circuit, &'static str), String> {
    let mut c = c.clone();
    let unit = match &m.measured {
        Measured::Volts { plus, minus } | Measured::Ohms { plus, minus } | Measured::Amps { plus, minus } => {
            let (a, b) = (c.node(plus), c.node(minus));
            c.elements.push(match m.measured {
                Measured::Volts { .. } => Element::VSource { name: "V_MEASURED".into(), p: a, n: b, wave: Waveform::Dc(x), ac: None },
                Measured::Ohms { .. } => Element::Resistor { name: "R_MEASURED".into(), a, b, r: x },
                _ => Element::ISource { name: "I_MEASURED".into(), from: b, to: a, wave: Waveform::Dc(x), ac: None },
            });
            m.measured.unit()
        }
        Measured::Part { part } => set_value(&mut c, part, x).ok_or_else(|| format!("{part} は値を動かせる部品 (抵抗・電圧源・電流源) ではありません"))?,
    };
    Ok((c, unit))
}

/// The readings at value `x`.
fn reading_at(s: &Schematic, base: &Circuit, m: &AdcMeasure, x: f64) -> Result<(AdcReading, Option<AdcReading>, &'static str), String> {
    let (c, unit) = with_value(base, m, x)?;
    let dc = crate::dc_op(&c).map_err(|e| e.to_string())?;
    let main = read(s, &dc, &m.adc, &setup(s, m, false)?)?;
    let reference = if m.ratio() { Some(read(s, &dc, &m.adc, &setup(s, m, true)?)?) } else { None };
    Ok((main, reference, unit))
}

fn value_of(main: &AdcReading, reference: &Option<AdcReading>) -> f64 {
    match reference {
        Some(q) if q.vin.abs() > 0.0 => main.vin / q.vin,
        _ => main.vin,
    }
}

/// What way of measuring `m` of schematic `s` reads.
pub fn read_measure(s: &Schematic, m: &AdcMeasure) -> Result<MeasureReading, String> {
    if !(m.at.is_finite() && m.at != 0.0) || (matches!(m.measured, Measured::Ohms { .. }) && m.at <= 0.0) {
        return Err(format!("{}: 試す値は 0 でない数にしてください (抵抗は正の数)", m.name));
    }
    if let Measured::Volts { plus, minus } | Measured::Ohms { plus, minus } | Measured::Amps { plus, minus } = &m.measured {
        let nets = extract(&s.flat());
        for net in [plus, minus] {
            if !nets.iter().any(|n| n.name == *net) {
                return Err(format!("{}: ネット {net} が回路図にありません (測る物をつなぐネットの名前を入れます)", m.name));
            }
        }
    }
    let (base, done) = circuit(s, m)?;
    let (main, reference, unit) = reading_at(s, &base, m, m.at)?;
    let x2 = m.at * 1.001;
    let (m2, r2, _) = reading_at(s, &base, m, x2)?;
    let value = value_of(&main, &reference);
    let slope = (value_of(&m2, &r2) - value) / (x2 - m.at);
    // the noise of the value: one input's, or the two of a ratio added as their relative parts
    let noise = |pick: fn(&crate::adcread::AdcNoise) -> f64| -> Option<f64> {
        let n1 = pick(main.noise.as_ref()?);
        match &reference {
            None => Some(n1),
            Some(q) => {
                let n2 = pick(q.noise.as_ref()?);
                Some(value.abs() * ((n1 / main.vin).powi(2) + (n2 / q.vin).powi(2)).sqrt())
            }
        }
    };
    let to_unit = |v: Option<f64>| v.filter(|_| slope.abs() > 0.0).map(|v| v / slope.abs());
    let noise_rms = to_unit(noise(|n| n.rms));
    let noise_pp = to_unit(noise(|n| n.pp));
    let digits = noise_pp.filter(|p| *p > 0.0).map(|p| (m.at.abs() / p).log10());
    Ok(MeasureReading { main, reference, value, slope, unit, noise_rms, noise_pp, digits, done })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_coil_voltage_is_read_from_the_part_name() {
        assert_eq!(coil_volts("TQ2-L2-5V"), 5.0);
        assert_eq!(coil_volts("G6K-2F-Y-12V"), 12.0);
        assert_eq!(coil_volts("RELAY"), 5.0);
    }
}
