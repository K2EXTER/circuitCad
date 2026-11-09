//! Supply rejection (PSRR): how much of a ripple on one supply comes through to a node. Every AC excitation of the circuit is taken
//! away and a 1 V ripple is put on the chosen supply alone; an AC sweep then gives, for every node, the share of the ripple that
//! reaches it. The rejection is that share turned around: a gain of -60 dB is a rejection of 60 dB (a 1 V ripple leaves 1 mV).

use crate::circuit::{Circuit, Element};
use crate::dc::SimError;
use crate::waveform::Waveform;

/// The supplies that can be rippled: voltage sources of the schematic (not inside a part's model) with a DC value other than 0.
pub fn supply_sources(c: &Circuit) -> Vec<String> {
    c.elements
        .iter()
        .filter_map(|e| match e {
            Element::VSource { name, wave: Waveform::Dc(v), .. } if *v != 0.0 && !name.contains('.') => Some(name.clone()),
            _ => None,
        })
        .collect()
}

/// The circuit with a 1 V ripple on `source` and no other AC excitation.
pub fn with_supply_ripple(c: &Circuit, source: &str) -> Result<Circuit, SimError> {
    let mut out = c.clone();
    let mut found = false;
    for e in &mut out.elements {
        match e {
            Element::VSource { name, ac, .. } => {
                if name.eq_ignore_ascii_case(source) {
                    *ac = Some((1.0, 0.0));
                    found = true;
                } else {
                    *ac = None;
                }
            }
            Element::ISource { ac, .. } => *ac = None,
            _ => {}
        }
    }
    if !found {
        return Err(SimError::Invalid(format!("電源 {source} が見つかりません。電圧源 (V1 など) の部品番号を選んでください")));
    }
    Ok(out)
}

#[derive(Clone, Debug, PartialEq)]
pub struct PsrrSummary {
    /// Rejection (dB, positive = the ripple is made smaller) at the lowest frequency of the sweep.
    pub low: (f64, f64),
    /// The frequency where the rejection is smallest, and its value.
    pub worst: (f64, f64),
    /// The rejection at the round frequencies (100 Hz, 1 kHz, ...) inside the sweep.
    pub at: Vec<(f64, f64)>,
}

/// Rejection figures from the gain (dB) of the node over frequency. None for an empty sweep.
pub fn psrr_summary(freq: &[f64], gain_db: &[f64]) -> Option<PsrrSummary> {
    if freq.is_empty() || freq.len() != gain_db.len() {
        return None;
    }
    let rej: Vec<f64> = gain_db.iter().map(|g| -g).collect();
    let w = (0..rej.len()).min_by(|&a, &b| rej[a].total_cmp(&rej[b]))?;
    let at_f = |f: f64| -> Option<f64> {
        let k = freq.windows(2).position(|p| p[0] <= f && f <= p[1])?;
        let t = (f.ln() - freq[k].ln()) / (freq[k + 1].ln() - freq[k].ln()).max(1e-300);
        Some(rej[k] + t * (rej[k + 1] - rej[k]))
    };
    let at = [100.0, 1e3, 10e3, 100e3, 1e6, 10e6].iter().filter_map(|&f| at_f(f).map(|r| (f, r))).collect();
    Some(PsrrSummary { low: (freq[0], rej[0]), worst: (freq[w], rej[w]), at })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ac_analysis, AcSettings, AcSweep};

    /// A supply through an RC filter (R1 from VCC to OUT, C1 to ground) and a signal source that must be silenced.
    fn rc_supply() -> Circuit {
        let mut c = Circuit::new();
        let (vcc, out, inp) = (c.node("VCC"), c.node("OUT"), c.node("IN"));
        c.elements.push(Element::vdc("V1", vcc, 0, 5.0));
        c.elements.push(Element::VSource { name: "V2".into(), p: inp, n: 0, wave: Waveform::Dc(0.0), ac: Some((1.0, 0.0)) });
        c.elements.push(Element::Resistor { name: "R1".into(), a: vcc, b: out, r: 1e3 });
        c.elements.push(Element::Capacitor { name: "C1".into(), a: out, b: 0, c: 1e-6 });
        c.elements.push(Element::Resistor { name: "R2".into(), a: inp, b: 0, r: 1e3 });
        c
    }

    #[test]
    fn an_rc_filter_rejects_more_above_its_corner() {
        let c = with_supply_ripple(&rc_supply(), "v1").unwrap();
        let r = ac_analysis(&c, &AcSettings { fstart: 10.0, fstop: 1e6, sweep: AcSweep::Decade(20) }).unwrap();
        let s = psrr_summary(&r.freq, &r.signal("V(OUT)").unwrap().db()).unwrap();
        // corner 159 Hz: almost nothing rejected at 10 Hz, about 20 dB per decade above
        assert!(s.low.1.abs() < 0.1, "{:?}", s.low);
        let at = |f: f64| s.at.iter().find(|p| p.0 == f).unwrap().1;
        assert!((at(10e3) - 36.0).abs() < 0.5, "{}", at(10e3));
        assert!((at(100e3) - at(10e3) - 20.0).abs() < 0.5);
        assert_eq!(s.worst.0, 10.0);
        // the signal source was silenced: nothing reaches IN
        assert!(r.signal("V(IN)").unwrap().mag().iter().all(|m| *m < 1e-9));
    }

    #[test]
    fn the_supplies_are_the_dc_sources_and_a_wrong_name_is_refused() {
        assert_eq!(supply_sources(&rc_supply()), vec!["V1".to_string()]);
        assert!(with_supply_ripple(&rc_supply(), "V9").is_err());
    }
}
