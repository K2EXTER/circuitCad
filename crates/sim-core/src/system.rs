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
        let link = BoardLink { a: LinkEnd { design: "main".into(), refdes: "J1".into() }, b: LinkEnd { design: "sensor".into(), refdes: "J1".into() }, pairs: vec![] };
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
        let both = BoardLink { a: LinkEnd { design: "main".into(), refdes: "J2".into() }, b: LinkEnd { design: "sensor".into(), refdes: "J2".into() }, pairs: vec![] };
        let readings = system_dc(&[("main", &m2), ("sensor", &s2)], &[both]).unwrap();
        assert_eq!(readings.len(), 2);
        assert!(readings.iter().all(|r| r.probe.shared == 2 && (r.current.unwrap() - 5e-3).abs() < 1e-6), "{readings:?}");
        // the merged circuit: designators with the board's name, no clash between the two J1
        let (m, _) = merge(&[("main", &main), ("sensor", &sensor)], &[link]).unwrap();
        let names: Vec<&str> = m.components.iter().map(|c| c.refdes.as_str()).collect();
        assert!(names.contains(&"J1_main") && names.contains(&"J1_sensor") && names.contains(&"R1_sensor") && names.contains(&"VLINK1_1"), "{names:?}");
    }
}
