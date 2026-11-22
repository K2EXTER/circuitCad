//! The whole system of several boards simulated as one circuit (E5, stage 3): `sch_core::system::merge` joins the boards' schematics with a
//! 0 V source on every connector pin, and the operating point gives, pin by pin, the voltage on each side and the current through it.

use sch_core::system::{merge, BoardLink, LinkProbe};
use sch_core::Schematic;

/// One connector pin of the system at its operating point.
#[derive(Clone, Debug, PartialEq)]
pub struct PinReading {
    pub probe: LinkProbe,
    /// The voltage of the pin's net on board A and on board B (V, to ground). The same through a joined pin; both 0 on ground.
    pub v_a: f64,
    pub v_b: f64,
    /// The current through the pin from board A into board B (A, positive when A feeds B); for pins that share a source (`probe.shared` > 1,
    /// two pins of one supply), their total. `None` on a ground pin (both sides are the one ground of the simulation, so no source measures it).
    pub current: Option<f64>,
}

/// The operating point of the whole system: every linked connector pin's voltage and current. Errors say what could not be built or solved.
pub fn system_dc(designs: &[(&str, &Schematic)], links: &[BoardLink]) -> Result<Vec<PinReading>, String> {
    let (sch, probes) = merge(designs, links)?;
    let circuit = crate::from_schematic(&sch).map_err(|e| e.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" / "))?;
    let r = crate::dc_op(&circuit).map_err(|e| e.to_string())?;
    let v = |net: &str| if net == "GND" || net == "0" { 0.0 } else { r.voltage(net).unwrap_or(f64::NAN) };
    Ok(probes
        .into_iter()
        .map(|p| {
            let current = (!p.source.is_empty()).then(|| r.elements.iter().find(|e| e.name == p.source).map_or(f64::NAN, |e| e.current));
            PinReading { v_a: v(&p.node_a), v_b: v(&p.node_b), current, probe: p }
        })
        .collect())
}

/// One connector pin's waveform in the whole system: its voltage on side A (to ground) and the current through it from A to B (`None` on a
/// pin both of whose sides are ground). Pins sharing one source (`probe.shared` > 1) carry their total.
#[derive(Clone, Debug)]
pub struct PinWave {
    pub probe: LinkProbe,
    pub v: Vec<f64>,
    pub i: Option<Vec<f64>>,
}

/// The transient run of the whole system: the time points and every linked pin's waveform.
#[derive(Clone, Debug)]
pub struct SystemTran {
    pub time: Vec<f64>,
    pub pins: Vec<PinWave>,
    pub warnings: Vec<String>,
}

/// One connector pin in the AC sweep of the whole system: the gain of its voltage on side A (dB re 1 V, so dB re the input when the AC
/// source is 1 V) and its phase (degrees).
#[derive(Clone, Debug)]
pub struct PinAc {
    pub probe: LinkProbe,
    pub db: Vec<f64>,
    pub phase: Vec<f64>,
}

/// The AC sweep of the whole system.
#[derive(Clone, Debug)]
pub struct SystemAc {
    pub freq: Vec<f64>,
    pub pins: Vec<PinAc>,
    pub warnings: Vec<String>,
}

/// The merged circuit, built for simulation, and its probes without the pins that are ground on both sides (nothing to show on them).
fn merged_circuit(designs: &[(&str, &Schematic)], links: &[BoardLink]) -> Result<(crate::Circuit, Vec<LinkProbe>), String> {
    let (sch, probes) = merge(designs, links)?;
    let circuit = crate::from_schematic(&sch).map_err(|e| e.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" / "))?;
    Ok((circuit, probes.into_iter().filter(|p| !p.source.is_empty()).collect()))
}

/// The transient analysis of the whole system to `tstop` (s): how the voltage on each linked pin and the current through it change in
/// time (a supply switched on, a clock passed from one board to another). Errors say what could not be built or solved.
pub fn system_tran(designs: &[(&str, &Schematic)], links: &[BoardLink], tstop: f64) -> Result<SystemTran, String> {
    if !(tstop.is_finite() && tstop > 0.0) {
        return Err("期間は正の値にしてください (例: 10m)".into());
    }
    let (circuit, probes) = merged_circuit(designs, links)?;
    let keep: std::collections::HashSet<String> =
        probes.iter().flat_map(|p| [format!("V({})", p.node_a), format!("I({})", p.source)]).collect();
    let settings = crate::TranSettings { tstop, tstep: crate::tran::auto_tstep(tstop), uic: false };
    let r = crate::tran::transient_with(&circuit, &settings, Some(&|n: &str| keep.contains(n))).map_err(|e| e.to_string())?;
    let n = r.time.len();
    let pins = probes
        .into_iter()
        .map(|p| {
            let v = r.signal(&format!("V({})", p.node_a)).map_or_else(|| vec![f64::NAN; n], |s| s.values.clone());
            let i = Some(r.signal(&format!("I({})", p.source)).map_or_else(|| vec![f64::NAN; n], |s| s.values.clone()));
            PinWave { probe: p, v, i }
        })
        .collect();
    Ok(SystemTran { time: r.time, pins, warnings: r.warnings })
}

/// The AC sweep of the whole system from `fstart` to `fstop` (Hz, 20 points a decade): the gain and phase on each linked pin, from the
/// boards' AC sources (`AC 1` on a source of one board). Without any AC source every gain would be zero, so that is an error saying so.
pub fn system_ac(designs: &[(&str, &Schematic)], links: &[BoardLink], fstart: f64, fstop: f64) -> Result<SystemAc, String> {
    let (circuit, probes) = merged_circuit(designs, links)?;
    let has_ac = circuit.elements.iter().any(|e| matches!(e, crate::Element::VSource { ac: Some(_), .. } | crate::Element::ISource { ac: Some(_), .. }));
    if !has_ac {
        return Err("どの基板にも AC の信号源がありません。入力にする電源の「AC 振幅」を 1 などにしてから解析してください".into());
    }
    let settings = crate::AcSettings { fstart, fstop, sweep: crate::AcSweep::Decade(20) };
    let r = crate::ac_analysis(&circuit, &settings).map_err(|e| e.to_string())?;
    let n = r.freq.len();
    let pins = probes
        .into_iter()
        .map(|p| match r.signal(&format!("V({})", p.node_a)) {
            Some(s) => PinAc { db: s.db(), phase: s.phase_deg(), probe: p },
            None => PinAc { db: vec![f64::NAN; n], phase: vec![f64::NAN; n], probe: p },
        })
        .collect();
    Ok(SystemAc { freq: r.freq, pins, warnings: r.warnings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sch_core::system::LinkEnd;
    use sch_core::{Kind, Label, Wire};

    /// A wire from a pin outwards and a label at its end.
    fn label(s: &mut Schematic, idx: usize, pin: &str, text: &str) {
        let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap();
        let c = s.components[idx].pos;
        let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
        let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
        s.wires.push(Wire { a: pt, b: end });
        s.labels.push(Label { pos: end, text: text.into() });
    }

    /// A 5 V supply on the main board feeds a 1 kΩ load on the sensor board through pin 1 of the two headers: 5 V on both sides and 5 mA from
    /// main into sensor. The same net names on the two boards stay apart until the link joins them: the sensor's own +5V is not the main's.
    #[test]
    fn current_flows_from_one_board_to_the_other_through_the_link() {
        let mut main = Schematic::default();
        let j = main.add(Kind::Header(2), (0, 0), 0);
        label(&mut main, j, "1", "+5V");
        label(&mut main, j, "2", "GND");
        let v = main.add(Kind::VSource, (30, 0), 0);
        main.components[v].value = "5".into();
        label(&mut main, v, "+", "+5V");
        label(&mut main, v, "-", "GND");
        let mut sensor = Schematic::default();
        let j = sensor.add(Kind::Header(2), (0, 0), 0);
        label(&mut sensor, j, "1", "+5V");
        label(&mut sensor, j, "2", "GND");
        let r = sensor.add(Kind::Resistor, (30, 0), 1);
        sensor.components[r].value = "1k".into();
        label(&mut sensor, r, "1", "+5V");
        label(&mut sensor, r, "2", "GND");
        let link = BoardLink { a: LinkEnd { design: "main".into(), refdes: "J1".into() }, b: LinkEnd { design: "sensor".into(), refdes: "J1".into() }, pairs: vec![], aliases: vec![], cable: None };
        let readings = system_dc(&[("main", &main), ("sensor", &sensor)], std::slice::from_ref(&link)).unwrap();
        let pin1 = readings.iter().find(|r| r.probe.pins.0 == "1").unwrap();
        assert!((pin1.v_a - 5.0).abs() < 1e-6 && (pin1.v_b - 5.0).abs() < 1e-6, "{pin1:?}");
        assert!((pin1.current.unwrap() - 5e-3).abs() < 1e-6, "5 mA from main into sensor: {pin1:?}");
        assert_eq!((pin1.probe.net_a.as_str(), pin1.probe.net_b.as_str()), ("main.+5V", "sensor.+5V"));
        let gnd = readings.iter().find(|r| r.probe.pins.0 == "2").unwrap();
        assert!(gnd.current.is_none() && gnd.v_a == 0.0, "ground is the one ground: {gnd:?}");
        // without the link, the sensor's load has no supply (its +5V is its own): the operating point has no current to show
        assert!(system_dc(&[("main", &main), ("sensor", &sensor)], &[]).unwrap().is_empty());
        // the sensor's pin 1 wired straight to its load with no name: the net gets one on the pin (sensor.J1-1) and is still measured
        let mut bare = Schematic::default();
        let j = bare.add(Kind::Header(2), (0, 0), 0);
        label(&mut bare, j, "2", "GND");
        let r = bare.add(Kind::Resistor, (30, 0), 1);
        bare.components[r].value = "1k".into();
        label(&mut bare, r, "2", "GND");
        let (p1, r1) = (bare.components[j].pins()[0].1, bare.components[r].pins().into_iter().find(|p| p.0 == "1").unwrap().1);
        bare.wires.push(Wire { a: p1, b: r1 });
        let readings = system_dc(&[("main", &main), ("sensor", &bare)], std::slice::from_ref(&link)).unwrap();
        let pin1 = readings.iter().find(|r| r.probe.pins.0 == "1").unwrap();
        assert_eq!(pin1.probe.node_b, "sensor.J1-1");
        assert!((pin1.v_b - 5.0).abs() < 1e-6 && (pin1.current.unwrap() - 5e-3).abs() < 1e-6, "{pin1:?}");
        // two pins of the same supply on each side: one source for both (two side by side could not be solved), the total through it
        let twice = |s: &Schematic| {
            let mut s = s.clone();
            let j = s.add(Kind::Header(2), (0, 30), 0);
            s.components[j].refdes = "J2".into();
            for (pin, net) in [("1", "+5V"), ("2", "+5V")] {
                let (_, pt) = s.components[j].pins().into_iter().find(|(n, _)| n == pin).unwrap();
                s.wires.push(Wire { a: pt, b: (pt.0 + 3, pt.1) });
                s.labels.push(Label { pos: (pt.0 + 3, pt.1), text: net.into() });
            }
            s
        };
        let (m2, s2) = (twice(&main), twice(&sensor));
        let both = BoardLink { a: LinkEnd { design: "main".into(), refdes: "J2".into() }, b: LinkEnd { design: "sensor".into(), refdes: "J2".into() }, pairs: vec![], aliases: vec![], cable: None };
        let readings = system_dc(&[("main", &m2), ("sensor", &s2)], &[both]).unwrap();
        assert_eq!(readings.len(), 2);
        assert!(readings.iter().all(|r| r.probe.shared == 2 && (r.current.unwrap() - 5e-3).abs() < 1e-6), "{readings:?}");
        // the merged circuit: designators with the board's name, no clash between the two J1
        let (m, _) = merge(&[("main", &main), ("sensor", &sensor)], &[link]).unwrap();
        let names: Vec<&str> = m.components.iter().map(|c| c.refdes.as_str()).collect();
        assert!(names.contains(&"J1_main") && names.contains(&"J1_sensor") && names.contains(&"R1_sensor") && names.contains(&"VLINK1_1"), "{names:?}");
    }

    /// A source on the main board through 1 kΩ to pin 1 of its header, a 1 µF capacitor on the sensor board's pin 1: an RC low pass split
    /// over two boards (159 Hz). The source is `src` (a pulse for the transient, `DC 0 AC 1` for the sweep).
    fn rc_over_two_boards(src: &str) -> (Schematic, Schematic, BoardLink) {
        let mut main = Schematic::default();
        let j = main.add(Kind::Header(2), (0, 0), 0);
        label(&mut main, j, "1", "OUT");
        label(&mut main, j, "2", "GND");
        let v = main.add(Kind::VSource, (30, 0), 0);
        main.components[v].value = src.into();
        label(&mut main, v, "+", "IN");
        label(&mut main, v, "-", "GND");
        let r = main.add(Kind::Resistor, (60, 0), 1);
        main.components[r].value = "1k".into();
        label(&mut main, r, "1", "IN");
        label(&mut main, r, "2", "OUT");
        let mut sensor = Schematic::default();
        let j = sensor.add(Kind::Header(2), (0, 0), 0);
        label(&mut sensor, j, "1", "VIN");
        label(&mut sensor, j, "2", "GND");
        let c = sensor.add(Kind::Capacitor, (30, 0), 1);
        sensor.components[c].value = "1u".into();
        label(&mut sensor, c, "1", "VIN");
        label(&mut sensor, c, "2", "GND");
        let link = BoardLink { a: LinkEnd { design: "main".into(), refdes: "J1".into() }, b: LinkEnd { design: "sensor".into(), refdes: "J1".into() }, pairs: vec![], aliases: vec![], cable: None };
        (main, sensor, link)
    }

    /// The pulse charges the sensor's capacitor through the main board's resistor: on pin 1 the voltage rises as 5·(1 − e^(−t/τ)), τ = 1 ms,
    /// and the current from main into sensor falls from 5 mA. The ground pin is left out (nothing to show).
    #[test]
    fn the_transient_of_the_system_charges_a_capacitor_through_the_link() {
        let (main, sensor, link) = rc_over_two_boards("PULSE(0 5 0 1n 1n 1 2)");
        let r = system_tran(&[("main", &main), ("sensor", &sensor)], &[link], 5e-3).unwrap();
        assert_eq!(r.pins.len(), 1, "only pin 1: {:?}", r.pins.iter().map(|p| &p.probe.pins).collect::<Vec<_>>());
        let p = &r.pins[0];
        let at = |t: f64, w: &[f64]| w[r.time.iter().position(|&x| x >= t).unwrap()];
        let want = 5.0 * (1.0 - (-1.0f64).exp());
        assert!((at(1e-3, &p.v) - want).abs() < 0.05, "1 τ: {} vs {want}", at(1e-3, &p.v));
        let i = p.i.as_ref().unwrap();
        assert!((at(1e-3, i) - (5.0 - want) / 1e3).abs() < 5e-5, "the current after 1 τ: {}", at(1e-3, i));
        assert!(at(5e-3, &p.v) > 4.9 && at(5e-3, i) < 1e-4);
        assert!(system_tran(&[("main", &main), ("sensor", &sensor)], &[], 0.0).is_err());
    }

    /// The same low pass swept: 0 dB well below 159 Hz, −3 dB at it, −20 dB a decade above; without an AC source the sweep says so.
    #[test]
    fn the_ac_sweep_of_the_system_sees_a_filter_split_over_two_boards() {
        let (main, sensor, link) = rc_over_two_boards("DC 0 AC 1");
        let r = system_ac(&[("main", &main), ("sensor", &sensor)], std::slice::from_ref(&link), 1.0, 100e3).unwrap();
        let p = &r.pins[0];
        let fc = 1.0 / (2.0 * std::f64::consts::PI * 1e-3);
        for (k, &f) in r.freq.iter().enumerate() {
            let want = -10.0 * (1.0 + (f / fc).powi(2)).log10();
            assert!((p.db[k] - want).abs() < 0.01, "{f} Hz: {} dB vs {want}", p.db[k]);
        }
        let (main, sensor, link) = rc_over_two_boards("5");
        let e = system_ac(&[("main", &main), ("sensor", &sensor)], &[link], 1.0, 100e3).unwrap_err();
        assert!(e.contains("AC"), "{e}");
    }
}
