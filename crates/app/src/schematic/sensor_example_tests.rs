//! The sensor sample circuits (temperature, light, current) and the generator that writes them.

use super::*;
use sim_core::{dc_op, Circuit};

fn library_part(name: &str) -> PartDef {
    part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<PartDef>(t).unwrap()).find(|d| d.name == name).unwrap()
}

fn label_pin(s: &mut Schematic, idx: usize, pin: &str, text: &str) {
    let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap_or_else(|| panic!("no pin {pin}"));
    let c = s.components[idx].pos;
    let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
    let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
    s.wires.push(Wire { a: pt, b: end });
    s.labels.push(Label { pos: end, text: text.into() });
}

fn part_at(s: &mut Schematic, name: &str, pos: Pt, rot: u8) -> usize {
    let d = library_part(name);
    s.add_custom(d.symbol(), &d.default_value(), &d.footprint_name(), pos, rot)
}

fn source(s: &mut Schematic, pos: Pt, value: &str, plus: &str) {
    let v = s.add(Kind::VSource, pos, 0);
    s.components[v].value = value.into();
    label_pin(s, v, "+", plus);
    label_pin(s, v, "-", "GND");
}

fn resistor(s: &mut Schematic, pos: Pt, rot: u8, value: &str, a: &str, b: &str) {
    let r = s.add(Kind::Resistor, pos, rot);
    s.components[r].value = value.into();
    label_pin(s, r, "1", a);
    label_pin(s, r, "2", b);
}

/// Temperature sensors side by side: LM35, TMP36 and an NTC thermistor divider.
/// `cargo test -p app write_sensor_examples -- --ignored`
fn temperature_sensors() -> Schematic {
    let mut s = Schematic::default();
    source(&mut s, (0, 0), "5", "VCC");
    let u1 = part_at(&mut s, "LM35", (24, 0), 0);
    label_pin(&mut s, u1, "1", "VCC");
    label_pin(&mut s, u1, "2", "VLM35");
    label_pin(&mut s, u1, "3", "GND");
    resistor(&mut s, (44, 0), 1, "100k", "VLM35", "GND");
    let u2 = part_at(&mut s, "TMP36", (24, 24), 0);
    label_pin(&mut s, u2, "1", "VCC");
    label_pin(&mut s, u2, "2", "VTMP36");
    label_pin(&mut s, u2, "3", "GND");
    resistor(&mut s, (44, 24), 1, "100k", "VTMP36", "GND");
    resistor(&mut s, (64, 0), 1, "10k", "VCC", "VNTC");
    let rt = part_at(&mut s, "NTC_10k_B3950", (64, 24), 1);
    label_pin(&mut s, rt, "1", "VNTC");
    label_pin(&mut s, rt, "2", "GND");
    s
}

/// A photocell divider and a reverse-biased photodiode, each driven by a voltage that stands for the light.
fn light_sensors() -> Schematic {
    let mut s = Schematic::default();
    source(&mut s, (0, 0), "5", "VCC");
    source(&mut s, (0, 16), "0.5", "LUX");
    source(&mut s, (0, 32), "1", "IRRAD");
    resistor(&mut s, (28, 0), 1, "10k", "VCC", "VLDR");
    let rl = part_at(&mut s, "LDR_GL5528", (28, 22), 0);
    label_pin(&mut s, rl, "1", "VLDR");
    label_pin(&mut s, rl, "2", "GND");
    label_pin(&mut s, rl, "L", "LUX");
    resistor(&mut s, (58, 0), 1, "22k", "VCC", "VPD");
    let d1 = part_at(&mut s, "BPW34", (58, 22), 0);
    label_pin(&mut s, d1, "K", "VPD");
    label_pin(&mut s, d1, "A", "GND");
    label_pin(&mut s, d1, "L", "IRRAD");
    s
}

/// A 10 mohm shunt read by an INA180A2 and the same current read by an ACS712-05B.
fn current_sensors() -> Schematic {
    let mut s = Schematic::default();
    source(&mut s, (0, 0), "5", "VCC");
    source(&mut s, (0, 16), "1", "BUS");
    resistor(&mut s, (24, 0), 0, "10m", "BUS", "N1");
    s.components.last_mut().unwrap().footprint = "R_2512".into();
    let ina = part_at(&mut s, "INA180A2", (44, 14), 0);
    label_pin(&mut s, ina, "3", "BUS");
    label_pin(&mut s, ina, "4", "N1");
    label_pin(&mut s, ina, "5", "VCC");
    label_pin(&mut s, ina, "2", "GND");
    label_pin(&mut s, ina, "1", "VINA");
    let acs = part_at(&mut s, "ACS712-05B", (74, 14), 0);
    label_pin(&mut s, acs, "1", "N1");
    label_pin(&mut s, acs, "3", "LOAD");
    label_pin(&mut s, acs, "5", "GND");
    label_pin(&mut s, acs, "6", "FILT");
    label_pin(&mut s, acs, "7", "VACS");
    label_pin(&mut s, acs, "8", "VCC");
    let c1 = s.add(Kind::Capacitor, (96, 14), 1);
    s.components[c1].value = "1n".into();
    label_pin(&mut s, c1, "1", "FILT");
    label_pin(&mut s, c1, "2", "GND");
    resistor(&mut s, (24, 30), 1, "2", "LOAD", "GND");
    s.components.last_mut().unwrap().footprint = "R_2512".into();
    s
}

/// The input of an ADC: a source with its resistance Rs (R1), the pin capacitor C2 (small at first), a switch (a voltage-controlled switch, closed for
/// 12 us = 1.5 clocks at 125 kHz) and the 14 pF sample capacitor C1, which holds 0 V from the channel before.
fn adc_input() -> Schematic {
    let mut s = Schematic::default();
    source(&mut s, (0, 0), "2.5", "SIG");
    let v2 = s.add(Kind::VSource, (0, 16), 0);
    s.components[v2].value = "PULSE(0 5 1u 10n 10n 12u 100u)".into();
    label_pin(&mut s, v2, "+", "GATE");
    label_pin(&mut s, v2, "-", "GND");
    resistor(&mut s, (24, 0), 0, "10k", "SIG", "IN");
    let c1 = s.add(Kind::Capacitor, (64, 14), 1);
    s.components[c1].value = "14p IC=0".into();
    label_pin(&mut s, c1, "1", "SH");
    label_pin(&mut s, c1, "2", "GND");
    let c2 = s.add(Kind::Capacitor, (44, 14), 1);
    s.components[c2].value = "1p".into();
    label_pin(&mut s, c2, "1", "IN");
    label_pin(&mut s, c2, "2", "GND");
    let q = part_at(&mut s, "SW_SAMPLE_DEMO", (54, 0), 0);
    label_pin(&mut s, q, "1", "IN");
    label_pin(&mut s, q, "2", "SH");
    label_pin(&mut s, q, "3", "GATE");
    label_pin(&mut s, q, "4", "GND");
    s
}

/// One power pin pair of a big chip: the supply reaches VCC through a thin wire (L1, 20 nH), the chip switches a load
/// (the voltage-controlled switch, 100 ohm on) in 2 ns, and a bypass capacitor C1 sits behind its own wire L2.
fn power_pin() -> Schematic {
    let mut s = Schematic::default();
    source(&mut s, (0, 0), "5", "VIN");
    let v2 = s.add(Kind::VSource, (0, 16), 0);
    s.components[v2].value = "PULSE(0 5 20n 4n 4n 100n 400n)".into();
    label_pin(&mut s, v2, "+", "GATE");
    label_pin(&mut s, v2, "-", "GND");
    let l1 = s.add(Kind::Inductor, (24, 0), 0);
    s.components[l1].value = "20n".into();
    label_pin(&mut s, l1, "1", "VIN");
    label_pin(&mut s, l1, "2", "VCC");
    let l2 = s.add(Kind::Inductor, (44, 0), 0);
    s.components[l2].value = "20n".into();
    label_pin(&mut s, l2, "1", "VCC");
    label_pin(&mut s, l2, "2", "CAP");
    let c1 = s.add(Kind::Capacitor, (64, 14), 1);
    s.components[c1].value = "1p".into();
    label_pin(&mut s, c1, "1", "CAP");
    label_pin(&mut s, c1, "2", "GND");
    let sw = part_at(&mut s, "SW_SAMPLE_DEMO", (84, 0), 0);
    label_pin(&mut s, sw, "1", "VCC");
    label_pin(&mut s, sw, "2", "GND");
    label_pin(&mut s, sw, "3", "GATE");
    label_pin(&mut s, sw, "4", "GND");
    s
}

#[test]
#[ignore]
fn write_sensor_examples() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    for (name, s) in [("temperature_sensors", temperature_sensors()), ("light_sensors", light_sensors()), ("current_sensors", current_sensors()), ("adc_input", adc_input()), ("power_pin", power_pin())] {
        std::fs::write(root.join(format!("{name}.schjson")), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    }
}

fn load(name: &str) -> Circuit {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples").join(name);
    let sch: Schematic = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    from_schematic(&sch).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn volt(c: &Circuit, net: &str) -> f64 {
    dc_op(c).unwrap().voltage(net).unwrap_or_else(|| panic!("no net {net}"))
}

#[test]
fn temperature_sample_reads_the_same_temperature_three_ways() {
    let c = load("temperature_sensors.schjson");
    let at = |t: f64, net: &str| volt(&c.at_temperature(t), net);
    // 25 degrees: LM35 250 mV, TMP36 750 mV, NTC 10k in a 10k divider -> half the supply
    assert!((at(25.0, "VLM35") - 0.25).abs() < 0.01);
    assert!((at(25.0, "VTMP36") - 0.75).abs() < 0.01);
    assert!((at(25.0, "VNTC") - 2.5).abs() < 0.05);
    // 100 degrees: the NTC falls to about 680 ohm
    assert!((at(100.0, "VLM35") - 1.0).abs() < 0.01);
    assert!((at(100.0, "VTMP36") - 1.5).abs() < 0.01);
    assert!(at(100.0, "VNTC") < 0.4 && at(100.0, "VNTC") > 0.25, "{}", at(100.0, "VNTC"));
}

#[test]
fn light_sample_gets_darker_and_brighter() {
    let c = load("light_sensors.schjson");
    let set = |c: &mut Circuit, name: &str, v: f64| {
        for e in &mut c.elements {
            if let sim_core::Element::VSource { name: n, wave, .. } = e
                && n == name
            {
                *wave = sim_core::Waveform::Dc(v);
            }
        }
    };
    // the shipped values: 50 lx and 1 mW/cm2
    let (ldr, pd) = (volt(&c, "VLDR"), volt(&c, "VPD"));
    assert!(ldr > 1.5 && ldr < 2.2, "VLDR {ldr}");
    assert!((pd - 3.35).abs() < 0.1, "VPD {pd}");
    // dark: the photocell is 1 Meg, the photodiode passes nothing
    let mut dark = c.clone();
    set(&mut dark, "V2", 0.0);
    set(&mut dark, "V3", 0.0);
    assert!(volt(&dark, "VLDR") > 4.9 && volt(&dark, "VPD") > 4.95);
    // bright: the photocell falls well below the sample value
    let mut bright = c.clone();
    set(&mut bright, "V2", 10.0);
    assert!(volt(&bright, "VLDR") < 0.7, "{}", volt(&bright, "VLDR"));
}

#[test]
fn current_sample_reads_the_same_current_with_a_shunt_and_a_hall_sensor() {
    let c = load("current_sensors.schjson");
    // 1 V across 2 ohm + 10 mohm + 1.2 mohm
    let i = 1.0 / (2.0 + 0.01 + 0.0012);
    assert!((volt(&c, "VINA") - 50.0 * 0.01 * i).abs() < 0.01, "VINA {}", volt(&c, "VINA"));
    assert!((volt(&c, "VACS") - (2.5 + 0.185 * i)).abs() < 0.01, "VACS {}", volt(&c, "VACS"));
}

/// Voltage of net `net` at time `at` in a transient run from zero.
fn adc_at(sch: &Schematic, net: &str, at: f64) -> f64 {
    let c = from_schematic(sch).unwrap();
    let r = sim_core::transient(&c, &sim_core::TranSettings { tstop: 30e-6, tstep: 0.05e-6, uic: false }).unwrap();
    let sig = r.signal(&format!("V({net})")).unwrap_or_else(|| panic!("no net {net}: {:?}", r.signals.iter().map(|s| &s.name).collect::<Vec<_>>()));
    let i = r.time.iter().position(|t| *t >= at).unwrap();
    sig.values[i]
}

#[test]
fn adc_sample_fills_the_sample_capacitor_only_for_a_low_source_resistance() {
    let mut s: Schematic = serde_json::from_str(&std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/adc_input.schjson")).unwrap()).unwrap();
    let set = |s: &mut Schematic, r: &str, v: &str| s.components.iter_mut().find(|c| c.refdes == r).unwrap().value = v.into();
    // the switch opens at 13 us: the voltage of the sample capacitor is the reading
    let read = |s: &Schematic| adc_at(s, "SH", 13e-6);
    assert!((read(&s) - 2.5).abs() < 0.01, "10k: {}", read(&s));
    set(&mut s, "R1", "100k");
    assert!((read(&s) - 2.5).abs() < 0.02, "100k: {}", read(&s));
    set(&mut s, "R1", "1Meg");
    assert!((1.3..1.6).contains(&read(&s)), "1M is not full: {}", read(&s));
    set(&mut s, "C2", "100n IC=2.5");
    assert!((read(&s) - 2.5).abs() < 0.01, "a 100n at the pin fills it: {}", read(&s));
}

/// (lowest, highest) V(VCC) of the power pin sample with the given C1 and L2.
fn power_swing(c1: &str, l2: &str) -> (f64, f64) {
    let mut s: Schematic = serde_json::from_str(&std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/power_pin.schjson")).unwrap()).unwrap();
    for (r, v) in [("C1", c1), ("L2", l2)] {
        s.components.iter_mut().find(|c| c.refdes == r).unwrap().value = v.into();
    }
    let c = from_schematic(&s).unwrap();
    let r = sim_core::transient(&c, &sim_core::TranSettings { tstop: 200e-9, tstep: 0.2e-9, uic: false }).unwrap();
    r.signal("V(VCC)").unwrap().values.iter().fold((f64::MAX, f64::MIN), |a, x| (a.0.min(*x), a.1.max(*x)))
}

#[test]
fn a_near_small_bypass_beats_a_far_big_one() {
    let width = |c: &str, l: &str| {
        let (lo, hi) = power_swing(c, l);
        hi - lo
    };
    let none = width("1p", "20n");
    let far = width("100n", "20n");
    let near = width("100n", "2n");
    let near_small = width("1n", "2n");
    assert!(none > 3.0 && far < 0.6 * none, "none {none} far {far}");
    assert!(near < 0.25 * far && near < 0.5, "near {near}");
    assert!(near_small < far, "1n near {near_small} beats 100n far {far}");
}
