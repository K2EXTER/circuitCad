//! A multimeter's current and resistance ranges on a circuit (the breadboard's tester, B6 ③), done the way a real meter does them.
//!
//! - **Resistance**: the meter pushes a small test current (`OHM_TEST_A`) from its red probe through the circuit back to the black
//!   one and reads the voltage it takes. The circuit must be off and its supplies taken off, as one does before measuring: every supply
//!   of the sheet (the voltage and current sources that are not inside a part's model) becomes an open circuit. Above `OHM_OPEN_V` the meter shows "OL" (open). A diode
//!   reads whatever its forward voltage at that current makes, as on a real meter.
//! - **Current**: the meter's probes are joined through its shunt (`AMMETER_OHM`), so it must go in series: on two ends of a gap it
//!   reads the current that flows through it; across a supply it is a short circuit, and above `FUSE_A` its fuse blows.

use crate::circuit::{Circuit, Element};
use crate::waveform::Waveform;

/// The test current of the resistance range (A).
pub const OHM_TEST_A: f64 = 100e-6;
/// Above this voltage between the probes the resistance range shows OL.
pub const OHM_OPEN_V: f64 = 2.0;
/// The meter's own resistance between the probes on the resistance range (its input): what an open circuit sees.
const METER_INPUT_OHM: f64 = 100e6;
/// The shunt of the current range (Ω).
pub const AMMETER_OHM: f64 = 0.1;
/// The current range's fuse (A).
pub const FUSE_A: f64 = 10.0;

/// The node of a net, `None` when the circuit has no such net.
fn node(c: &Circuit, net: &str) -> Option<usize> {
    if net.eq_ignore_ascii_case("GND") || net == "0" {
        return Some(0);
    }
    c.names.iter().position(|n| n == net)
}

/// The resistance between the nets `red` and `black` with every supply off: `Ok(None)` is open (OL).
pub fn resistance(c: &Circuit, red: &str, black: &str) -> Result<Option<f64>, String> {
    let (Some(r), Some(b)) = (node(c, red), node(c, black)) else { return Ok(None) };
    if r == b {
        return Ok(Some(0.0));
    }
    let mut off = c.clone();
    for e in &mut off.elements {
        match e {
            // a supply taken off: nothing flows through where it was
            Element::VSource { name, p, n, .. } if !name.contains('.') => *e = Element::ISource { name: name.clone(), from: *n, to: *p, wave: Waveform::Dc(0.0), ac: None },
            Element::ISource { name, wave, .. } if !name.contains('.') => *wave = Waveform::Dc(0.0),
            _ => {}
        }
    }
    off.add(Element::Resistor { name: "METER.Rin".into(), a: r, b, r: METER_INPUT_OHM });
    off.add(Element::ISource { name: "METER.I".into(), from: b, to: r, wave: Waveform::Dc(OHM_TEST_A), ac: None });
    let dc = crate::dc_op(&off).map_err(|e| e.to_string())?;
    let v = |n: usize| if n == 0 { 0.0 } else { dc.voltage(&off.names[n]).unwrap_or(0.0) };
    let volts = v(r) - v(b);
    Ok((volts.abs() < OHM_OPEN_V).then(|| volts / OHM_TEST_A))
}

/// What the current range reads with its probes on the nets `red` and `black`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Amps {
    /// The current through the meter from the red probe to the black (A).
    Reading(f64),
    /// Both probes on one net: nothing flows through the meter (the circuit has to be opened to put it in series).
    SameNet,
    /// More than the fuse takes: the meter is a short across a supply.
    FuseBlown(f64),
}

/// The current range with its probes on `red` and `black` of the circuit as it is (powered).
pub fn current(c: &Circuit, red: &str, black: &str) -> Result<Amps, String> {
    let (Some(r), Some(b)) = (node(c, red), node(c, black)) else { return Ok(Amps::Reading(0.0)) };
    if r == b {
        return Ok(Amps::SameNet);
    }
    let mut on = c.clone();
    on.add(Element::Resistor { name: "METER.Rsh".into(), a: r, b, r: AMMETER_OHM });
    let dc = crate::dc_op(&on).map_err(|e| e.to_string())?;
    let i = dc.element("METER.Rsh").map_or(0.0, |e| e.current);
    Ok(if i.abs() > FUSE_A { Amps::FuseBlown(i) } else { Amps::Reading(i) })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 5 V → R1 1 kΩ → (MID) → R2 2 kΩ → GND, and an LED from MID through 1 kΩ.
    fn circuit() -> Circuit {
        let mut c = Circuit::default();
        let (vcc, mid) = (c.node("VCC"), c.node("MID"));
        c.add(Element::VSource { name: "V1".into(), p: vcc, n: 0, wave: Waveform::Dc(5.0), ac: None });
        c.add(Element::Resistor { name: "R1".into(), a: vcc, b: mid, r: 1e3 });
        c.add(Element::Resistor { name: "R2".into(), a: mid, b: 0, r: 2e3 });
        c
    }

    #[test]
    fn the_ohm_range_reads_with_the_supply_off() {
        let c = circuit();
        // R2 alone (R1 goes on to the supply, which is taken off: open)
        let r = resistance(&c, "MID", "GND").unwrap().unwrap();
        assert!((r - 2000.0).abs() < 1.0, "{r}");
        // where the supply was: R1 and R2 in series
        let r = resistance(&c, "VCC", "GND").unwrap().unwrap();
        assert!((r - 3000.0).abs() < 1.0, "{r}");
        // a net that is not there: open
        assert_eq!(resistance(&c, "NOWHERE", "GND").unwrap(), None);
    }

    #[test]
    fn an_open_reads_ol() {
        let mut c = Circuit::default();
        let (a, b) = (c.node("A"), c.node("B"));
        c.add(Element::Capacitor { name: "C1".into(), a, b, c: 1e-6 });
        c.add(Element::Resistor { name: "R1".into(), a: b, b: 0, r: 1e3 });
        // a capacitor is open at DC: past 2 V at 100 µA
        assert_eq!(resistance(&c, "A", "B").unwrap(), None);
    }

    #[test]
    fn the_current_range_goes_in_series_and_blows_its_fuse_across_a_supply() {
        let c = circuit();
        // across R1 (in place of it, as a meter bridging a gap): R1's 1 kΩ is shorted by the shunt, 5 V / 2 kΩ = 2.5 mA
        match current(&c, "VCC", "MID").unwrap() {
            Amps::Reading(i) => assert!((i - 2.5e-3).abs() < 1e-5, "{i}"),
            x => panic!("{x:?}"),
        }
        assert_eq!(current(&c, "MID", "MID").unwrap(), Amps::SameNet);
        // across the supply: 5 V into 0.1 Ω
        assert!(matches!(current(&c, "VCC", "GND").unwrap(), Amps::FuseBlown(i) if i > 40.0));
    }
}
