//! What an ADC reads for each way of measuring in the schematic's table (`sch_core::adc_measure::AdcMeasure`, candidate L1): the
//! conditions are set (a latching relay set, a switch thrown, a part's value replaced), the thing measured is put in (a voltage,
//! a resistance or a current between two nets, or a part's own value), and the DC point gives what the ADC reads (`adcread`). Read
//! at the value and at 0.1 % more, the slope turns the ADC's noise (the data sheet's table) into the measured unit: how much the
//! reading wanders, and how many of its digits stay still at that value. A temperature sensor is put in as the resistor it is at
//! the temperature (an NTC's B constant, a resistance thermometer's coefficient), so the noise reads in °C.
//!
//! The error estimate: each resistor is moved by a little and the reading's change, through the slope, gives what its tolerance
//! (before calibration) and its temperature coefficient over the temperature span do to the measured value, part by part, added as
//! a sum (the worst) and as the root of the sum of squares (likely). A resistor's figures are its own (a library part's spread
//! and data sheet rows, the part's temperature coefficient) or the row's for parts that have none. The leaks: each other part
//! (a clamp diode, an open relay contact, the ADC's input current) is taken out (an open contact opened for good) and what the
//! reading moves by is what it leaks into it; a part whose removal moves the reading by more than 1 % is in the path, not a leak.
//!
//! A latching relay is set by putting its coil's voltage across its set coil (the model's `.subckt` ports `setp` and `setn`); a
//! switch is thrown by the source its model keeps its position in (`Vpress` of a push button, `Vpos` of a slide switch) at 1 V. A
//! board fed through its header (no supply of its own on the schematic) is fed from the supply-named nets at its connectors.

use crate::adcread::{adc_parts, default_setup, read, AdcFilter, AdcReading, AdcSetup};
use crate::circuit::{Circuit, Element};
use crate::waveform::Waveform;
use crate::dc::DcResult;
use sch_core::adc_measure::{condition, sensor_ohms, AdcMeasure, Measured};
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
    /// What the parts' tolerances, temperature coefficients and leaks do to the measured value (None when the slope is 0).
    pub errors: Option<MeasureErrors>,
}

/// What one resistor does to the measured value (in its unit, signed: the value read when the part is at the high end).
#[derive(Clone, Debug, PartialEq)]
pub struct PartError {
    pub part: String,
    /// The tolerance (fraction) and temperature coefficient (ppm/K), and whether they are the part's own (else the row's).
    pub tol: f64,
    pub tc_ppm: f64,
    pub own: bool,
    pub tol_err: f64,
    pub temp_err: f64,
}

/// What a part leaks into the reading: the measured value with it minus without it.
#[derive(Clone, Debug, PartialEq)]
pub struct Leak {
    pub part: String,
    pub err: f64,
}

/// The error estimate of one way of measuring, in the measured unit.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeasureErrors {
    /// The resistors, the one that matters most first.
    pub parts: Vec<PartError>,
    /// Tolerances (before calibration): the sum of their sizes and their root of the sum of squares.
    pub tol_sum: f64,
    pub tol_rss: f64,
    /// Temperature coefficients over the row's temperature span, the same two ways.
    pub temp_sum: f64,
    pub temp_rss: f64,
    /// The leaks, the largest first, and their sum (signed: they add as they are).
    pub leaks: Vec<Leak>,
    pub leak_sum: f64,
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

/// Whether element `e` belongs to part `part` (its own name, or inside its model).
fn of_part(e: &str, part: &str) -> bool {
    e.eq_ignore_ascii_case(part) || e.len() > part.len() && e.as_bytes()[part.len()] == b'.' && e[..part.len()].eq_ignore_ascii_case(part)
}

/// The nets of a two-pin part's pins.
fn two_nets(s: &Schematic, part: &str) -> Result<(String, String), String> {
    let nets = extract(&s.flat());
    let on: Vec<String> = nets.iter().filter(|n| n.pins.iter().any(|p| p.refdes == part)).map(|n| n.name.clone()).collect();
    match &on[..] {
        [a, b] => Ok((a.clone(), b.clone())),
        _ => Err(format!("{part} は 2 本のピンが別々のネットにつながった部品ではありません (温度のセンサーは 2 端子の部品)")),
    }
}

/// The circuit with the measured thing at value `x`, and its unit. `ends`: the nets of a temperature sensor.
fn with_value(c: &Circuit, m: &AdcMeasure, x: f64, ends: Option<&(String, String)>) -> Result<(Circuit, &'static str), String> {
    let mut c = c.clone();
    if let (Measured::Temp { part, r0, b, alpha }, Some((na, nb))) = (&m.measured, ends) {
        c.elements.retain(|e| !of_part(e.name(), part));
        let (a, bn) = (c.node(na), c.node(nb));
        c.elements.push(Element::Resistor { name: "R_MEASURED".into(), a, b: bn, r: sensor_ohms(*r0, *b, *alpha, x) });
        return Ok((c, "°C"));
    }
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
        Measured::Temp { part, .. } => return Err(format!("{part}: 温度のセンサーのネットが分かりません")),
    };
    Ok((c, unit))
}

/// The readings of a solved circuit.
fn readings(s: &Schematic, m: &AdcMeasure, dc: &DcResult) -> Result<(AdcReading, Option<AdcReading>), String> {
    let main = read(s, dc, &m.adc, &setup(s, m, false)?)?;
    let reference = if m.ratio() { Some(read(s, dc, &m.adc, &setup(s, m, true)?)?) } else { None };
    Ok((main, reference))
}

/// The readings at value `x`, with the circuit and its solution.
type AtValue = (AdcReading, Option<AdcReading>, &'static str, Circuit, DcResult);

fn reading_at(s: &Schematic, base: &Circuit, m: &AdcMeasure, x: f64, ends: Option<&(String, String)>) -> Result<AtValue, String> {
    let (c, unit) = with_value(base, m, x, ends)?;
    let dc = crate::dc_op(&c).map_err(|e| e.to_string())?;
    let (main, reference) = readings(s, m, &dc)?;
    Ok((main, reference, unit, c, dc))
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
    if let Measured::Temp { r0, b, alpha, .. } = &m.measured
        && !(*r0 > 0.0 && (*b > 0.0 || *alpha != 0.0))
    {
        return Err(format!("{}: 温度のセンサーは、基準の抵抗 (r0) と B 定数か温度係数 (alpha) を入れてください", m.name));
    }
    let ends = match &m.measured {
        Measured::Temp { part, .. } => Some(two_nets(s, part)?),
        _ => None,
    };
    let (base, done) = circuit(s, m)?;
    let (main, reference, unit, at_circuit, at_dc) = reading_at(s, &base, m, m.at, ends.as_ref())?;
    // a temperature near 0 °C moves by a step, not a share
    let x2 = if matches!(m.measured, Measured::Temp { .. }) { m.at + 0.01 } else { m.at * 1.001 };
    let (m2, r2, _, _, _) = reading_at(s, &base, m, x2, ends.as_ref())?;
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
    let errors = if slope.abs() > 0.0 { Some(errors(s, m, &at_circuit, &at_dc, value, slope)) } else { None };
    Ok(MeasureReading { main, reference, value, slope, unit, noise_rms, noise_pp, digits, done, errors })
}

/// The first number in a data sheet row's text ("±1 %" → 1, "±100 ppm/K" → 100), unsigned.
fn first_number(text: &str) -> Option<f64> {
    let start = text.find(|c: char| c.is_ascii_digit())?;
    let tail = &text[start..];
    let end = tail.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(tail.len());
    tail[..end].parse().ok()
}

/// A part's data sheet row by its name.
fn row(comp: &sch_core::Component, name: &str) -> Option<f64> {
    comp.custom.as_ref()?.ratings.as_ref()?.rows.iter().find(|r| r.name == name).and_then(|r| first_number(&r.value))
}

/// A resistor that moves the reading: its part, element index, tolerance (fraction), ppm/K and whether those are its own.
type Resistor = (String, usize, f64, f64, bool);

/// The resistors whose tolerance and temperature coefficient move the reading.
fn resistors(s: &Schematic, m: &AdcMeasure, c: &Circuit) -> Vec<Resistor> {
    let measured = m.measured.part();
    let mut out = vec![];
    for comp in &s.flat().components {
        if Some(comp.refdes.as_str()) == measured {
            continue;
        }
        let mine: Vec<usize> = c.elements.iter().enumerate().filter(|(_, e)| matches!(e, Element::Resistor { .. }) && of_part(e.name(), &comp.refdes)).map(|(i, _)| i).collect();
        let plain = comp.kind == sch_core::Kind::Resistor;
        let library = comp.custom.as_ref().is_some_and(|x| x.glyph == "resistor");
        let [i] = mine[..] else { continue };
        if !(plain || library) {
            continue;
        }
        let name = c.elements[i].name().to_string();
        let spread = c.spreads.iter().find(|x| x.element == name).filter(|x| x.hi + x.lo > 0.0).map(|x| (x.hi - x.lo) / (x.hi + x.lo));
        let tol = spread.or_else(|| row(comp, "許容差").map(|p| p / 100.0));
        let tc = Some(comp.tc_ppm.abs()).filter(|t| *t > 0.0).or_else(|| row(comp, "温度係数"));
        let own = tol.is_some() || tc.is_some();
        out.push((comp.refdes.clone(), i, tol.unwrap_or(m.tol_pct / 100.0), tc.unwrap_or(m.tc_ppm), own));
    }
    out
}

/// What the parts' tolerances, temperature coefficients and leaks do to the value read from circuit `c` (solved: `dc`).
fn errors(s: &Schematic, m: &AdcMeasure, c: &Circuit, dc: &DcResult, value: f64, slope: f64) -> MeasureErrors {
    let value_of_circuit = |x: &Circuit, from: bool| -> Option<f64> {
        let d = if from { crate::dc_op_from(x, dc) } else { crate::dc_op(x) }.ok()?;
        let (main, reference) = readings(s, m, &d).ok()?;
        Some(value_of(&main, &reference))
    };
    // the resistors: the reading's change for a 0.1 % change of each
    const H: f64 = 1e-3;
    let list = resistors(s, m, c);
    let moved = crate::par::par_map(&list, |(_, i, _, _, _)| {
        let mut x = c.clone();
        if let Element::Resistor { r, .. } = &mut x.elements[*i] {
            *r *= 1.0 + H;
        }
        value_of_circuit(&x, true).map(|v| (v - value) / H / slope)
    });
    let mut parts: Vec<PartError> = list
        .iter()
        .zip(moved)
        .filter_map(|((part, _, tol, tc, own), d)| d.map(|d| PartError { part: part.clone(), tol: *tol, tc_ppm: *tc, own: *own, tol_err: d * tol, temp_err: d * tc * 1e-6 * m.dtemp }))
        // a resistor away from this reading does nothing to it
        .filter(|p| p.tol_err.abs() + p.temp_err.abs() > 1e-9 * m.at.abs())
        .collect();
    parts.sort_by(|a, b| (b.tol_err.abs() + b.temp_err.abs()).total_cmp(&(a.tol_err.abs() + a.temp_err.abs())));
    let sum = |f: fn(&PartError) -> f64| parts.iter().map(|p| f(p).abs()).sum::<f64>();
    let rss = |f: fn(&PartError) -> f64| parts.iter().map(|p| f(p).powi(2)).sum::<f64>().sqrt();
    // the leaks: every other part taken out, an open contact opened for good
    let skip: Vec<String> = list.iter().map(|x| x.0.clone()).chain(m.measured.part().map(str::to_string)).collect();
    let others: Vec<String> = s
        .flat()
        .components
        .iter()
        .map(|x| x.refdes.clone())
        .filter(|r| !skip.contains(r))
        .filter(|r| c.elements.iter().any(|e| of_part(e.name(), r)))
        // a supply is not a leak
        .filter(|r| !c.elements.iter().any(|e| e.name().eq_ignore_ascii_case(r) && matches!(e, Element::VSource { .. } | Element::ISource { .. })))
        .collect();
    let leaked = crate::par::par_map(&others, |r| {
        let mut x = c.clone();
        let switches = x.elements.iter().any(|e| of_part(e.name(), r) && matches!(e, Element::Switch { .. }));
        if switches {
            for e in x.elements.iter_mut() {
                if let Element::Switch { name, model, .. } = e
                    && of_part(name, r)
                {
                    model.roff = 1e18;
                }
            }
        } else {
            x.elements.retain(|e| !of_part(e.name(), r));
        }
        value_of_circuit(&x, switches).map(|v| (value - v) / slope)
    });
    let mut leaks: Vec<Leak> = others
        .into_iter()
        .zip(leaked)
        .filter_map(|(part, e)| e.map(|err| Leak { part, err }))
        .filter(|l| l.err.abs() <= 0.01 * m.at.abs() && l.err.abs() > 1e-12 * m.at.abs())
        .collect();
    leaks.sort_by(|a, b| b.err.abs().total_cmp(&a.err.abs()));
    let leak_sum = leaks.iter().map(|l| l.err).sum();
    MeasureErrors { tol_sum: sum(|p| p.tol_err), tol_rss: rss(|p| p.tol_err), temp_sum: sum(|p| p.temp_err), temp_rss: rss(|p| p.temp_err), parts, leaks, leak_sum }
}

/// A temperature sensor's law from its model: an NTC written as `R0*exp(B*(1/(temp+273.15)-1/298.15))` gives (R0, B).
pub fn ntc_of(model: &str) -> Option<(f64, f64)> {
    let k = model.find("exp(")?;
    let after = &model[k + 4..];
    let b = crate::parse_value(after.split('*').next()?.trim())?;
    let before = model[..k].trim_end().strip_suffix('*')?;
    let r0 = crate::parse_value(before.rsplit(['(', '/', '=', ' ']).next()?.trim())?;
    (r0 > 0.0 && b > 0.0 && after.contains("273.15")).then_some((r0, b))
}

/// The law of the part `part` when it is an NTC (`ntc_of` on its model): (R at 25 °C, B).
pub fn sensor_law(s: &Schematic, part: &str) -> Option<(f64, f64)> {
    let flat = s.flat();
    let comp = flat.components.iter().find(|c| c.refdes == part)?;
    ntc_of(&comp.custom.as_ref()?.sim.as_ref()?.text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ntcs_law_is_read_from_its_model() {
        assert_eq!(ntc_of("Br a b I=V(a,b)/(10k*exp(3950*(1/(temp+273.15)-1/298.15)))"), Some((10e3, 3950.0)));
        assert_eq!(ntc_of("R1 a b 10k"), None);
        assert_eq!(first_number("±100 ppm/K"), Some(100.0));
        assert_eq!(first_number("±0.5 %"), Some(0.5));
    }

    #[test]
    fn the_coil_voltage_is_read_from_the_part_name() {
        assert_eq!(coil_volts("TQ2-L2-5V"), 5.0);
        assert_eq!(coil_volts("G6K-2F-Y-12V"), 12.0);
        assert_eq!(coil_volts("RELAY"), 5.0);
    }
}
