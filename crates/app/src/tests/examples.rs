use super::*;

/// Generates docs/examples/rlc_board.pcbjson from the RLC example schematic:
/// `cargo test -p app write_example_board -- --ignored`
#[test]
#[ignore]
fn write_example_board() {
    use pcb_core::{Pad, Zone};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.sch.load_path(root.join("rlc_pulse.schjson"));
    app.board_ed.import_from_schematic(&app.sch);
    let b = &mut app.board_ed.board;
    b.name = "rlc".into();
    b.set_rect_outline(0.0, 0.0, 40.0, 25.0);
    for (r, x, y, rot) in [("V1", 6.0, 14.0, 0), ("R1", 16.0, 18.0, 0), ("L1", 25.0, 18.0, 0), ("C1", 33.0, 12.0, 3)] {
        b.place_part(r, Point::new(x, y), rot);
    }
    let pad = |b: &pcb_core::Board, r: &str, pin: &str| b.pads.iter().find(|p| p.refdes == r && p.pin == pin).map(|p: &Pad| p.pos).unwrap();
    let route = |b: &mut pcb_core::Board, side: Side, pts: &[Point]| {
        for w in pts.windows(2) {
            b.tracks.push(Track { side, a: w[0], b: w[1], width: 0.4 });
        }
    };
    let (v_p, v_m) = (pad(b, "V1", "+"), pad(b, "V1", "-"));
    let (r1, r2) = (pad(b, "R1", "1"), pad(b, "R1", "2"));
    let (l1, l2) = (pad(b, "L1", "1"), pad(b, "L1", "2"));
    let (c1, c2) = (pad(b, "C1", "1"), pad(b, "C1", "2"));
    route(b, Side::Top, &[v_p, Point::new(v_p.x, r1.y), r1]);
    route(b, Side::Top, &[r2, l1]);
    route(b, Side::Top, &[l2, Point::new(c1.x, l2.y), c1]);
    // GND: C1.2 reaches the bottom pour through a via, V1.- is a through-hole pad
    b.vias.push(pcb_core::Via { pos: Point::new(c2.x, c2.y - 1.8), diameter: 0.8, drill: 0.4 });
    route(b, Side::Top, &[c2, Point::new(c2.x, c2.y - 1.8)]);
    let _ = v_m;
    b.zones.push(Zone::new("GND", Side::Bottom, vec![Point::new(2.0, 2.0), Point::new(38.0, 2.0), Point::new(38.0, 23.0), Point::new(2.0, 23.0)]));
    b.holes.push(pcb_core::Hole::new(Point::new(3.0, 3.0), 3.0));
    b.holes.push(pcb_core::Hole::new(Point::new(37.0, 3.0), 3.0));
    let v = pcb_core::drc::check(b);
    println!("DRC: {v:?}");
    std::fs::write(root.join("rlc_board.pcbjson"), serde_json::to_string_pretty(&*b).unwrap()).unwrap();
}

/// Writes docs/examples/divider_tolerance.schjson: `cargo test -p app write_divider_example -- --ignored`
#[test]
#[ignore]
fn write_divider_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut s = sch_core::Schematic::default();
    let v = s.add(Kind::VSource, (0, 0), 0);
    let r1 = s.add(Kind::Resistor, (6, -6), 0);
    let r2 = s.add(Kind::Resistor, (12, 0), 1);
    s.add(Kind::Ground, (0, 8), 0);
    s.components[v].value = "5".into();
    s.components[r1].value = "10k".into();
    s.components[r2].value = "10k".into();
    let rp = s.components[r2].pins();
    for (a, b) in [((0, -2), (0, -6)), ((0, -6), (4, -6)), ((8, -6), (12, -6)), ((12, -6), rp[0].1), (rp[1].1, (12, 6)), ((12, 6), (0, 6)), ((0, 2), (0, 6)), ((0, 6), (0, 8))] {
        s.wires.push(Wire { a, b });
    }
    s.labels.push(sch_core::Label { pos: (12, -6), text: "MID".into() });
    std::fs::write(root.join("divider_tolerance.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Writes docs/examples/li_ion_charger.schjson: `cargo test -p app write_charger_example -- --ignored`
#[test]
#[ignore]
fn write_charger_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let app = App::default();
    let lib = &app.sch.library;
    let tp = lib.iter().find(|d| d.name == "TP4056").expect("TP4056 is bundled");
    let mut s = sch_core::Schematic::default();
    let label = |s: &mut sch_core::Schematic, comp: usize, pin: &str, text: &str| {
        let at = s.components[comp].pins().into_iter().find(|p| p.0 == pin).unwrap().1;
        s.labels.push(sch_core::Label { pos: at, text: text.into() });
    };
    let v = s.add(Kind::VSource, (0, 0), 0);
    s.components[v].value = "5".into();
    label(&mut s, v, "+", "VIN");
    label(&mut s, v, "-", "GND");
    let u = s.add_custom(tp.symbol(), &tp.default_value(), &tp.footprint_name(), (30, 0), 0);
    for (pin, net) in [("1", "GND"), ("2", "PROG"), ("3", "GND"), ("4", "VIN"), ("5", "BAT"), ("6", "STDBY"), ("7", "CHRG"), ("8", "VIN")] {
        label(&mut s, u, pin, net);
    }
    let rp = s.add(Kind::Resistor, (30, 22), 0);
    s.components[rp].value = "6k".into();
    label(&mut s, rp, "1", "PROG");
    label(&mut s, rp, "2", "GND");
    let cell = s.add(Kind::Capacitor, (56, 0), 1);
    s.components[cell].value = "470u IC=2.6 ESR=0.5".into();
    label(&mut s, cell, "1", "BAT");
    label(&mut s, cell, "2", "GND");
    for (k, (net, name)) in [("CHRG", "充電中"), ("STDBY", "充電完了")].iter().enumerate() {
        let y = 14 + 12 * k as i32;
        let r = s.add(Kind::Resistor, (70, y), 0);
        let d = s.add(Kind::Led, (84, y), 0);
        s.components[r].value = "1k".into();
        label(&mut s, r, "1", "VIN");
        let tag = format!("A{k}");
        label(&mut s, r, "2", &tag);
        label(&mut s, d, "A", &tag);
        label(&mut s, d, "K", net);
        let _ = name;
    }
    std::fs::write(root.join("li_ion_charger.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Writes docs/examples/li_ion_cell_charge.schjson: the TP4056 charger sample with the 470 uF stand-in replaced
/// by the 18650 cell model (starts at 20 %): `cargo test -p app write_cell_charge_example -- --ignored`
#[test]
#[ignore]
fn write_cell_charge_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut s: sch_core::Schematic = serde_json::from_str(&std::fs::read_to_string(root.join("li_ion_charger.schjson")).unwrap()).unwrap();
    let cell = s.components.iter().position(|c| c.kind == Kind::Capacitor && c.value.starts_with("470u")).unwrap();
    let pins: Vec<_> = s.components[cell].pins().into_iter().map(|p| p.1).collect();
    s.labels.retain(|l| !pins.contains(&l.pos));
    s.components.remove(cell);
    let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "LIION_18650_CHG").unwrap();
    let bt = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (62, 0), 0);
    for (pin, net) in [("1", "BAT"), ("2", "GND")] {
        let at = s.components[bt].pins().into_iter().find(|p| p.0 == pin).unwrap().1;
        s.labels.push(sch_core::Label { pos: at, text: net.into() });
    }
    // the PROG resistor sets 1 A (1200 / 1.2k); 6k would be 0.2 A
    s.components.iter_mut().find(|c| c.value == "6k").unwrap().value = "1.2k".into();
    std::fs::write(root.join("li_ion_cell_charge.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Writes docs/examples/tp5100_cell_charge.schjson: the TP5100 charger sample with the 470 uF stand-in replaced
/// by the 18650 cell model (starts at 20 %): `cargo test -p app write_tp5100_cell_charge_example -- --ignored`
#[test]
#[ignore]
fn write_tp5100_cell_charge_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut s: sch_core::Schematic = serde_json::from_str(&std::fs::read_to_string(root.join("tp5100_charger.schjson")).unwrap()).unwrap();
    let cell = s.components.iter().position(|c| c.refdes == "C2").unwrap();
    let (pos, pins) = (s.components[cell].pos, s.components[cell].pins().into_iter().map(|p| p.1).collect::<Vec<_>>());
    s.labels.retain(|l| !pins.contains(&l.pos));
    s.components.remove(cell);
    let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "LIION_18650_CHG").unwrap();
    let bt = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), pos, 0);
    for (pin, net) in [("1", "BAT"), ("2", "GND")] {
        let at = s.components[bt].pins().into_iter().find(|p| p.0 == pin).unwrap().1;
        s.labels.push(sch_core::Label { pos: at, text: net.into() });
    }
    std::fs::write(root.join("tp5100_cell_charge.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Writes docs/examples/tp5100_charger.schjson: `cargo test -p app write_tp5100_example -- --ignored`
#[test]
#[ignore]
fn write_tp5100_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let app = App::default();
    let tp = app.sch.library.iter().find(|d| d.name == "TP5100").expect("TP5100 is bundled");
    let mut s = sch_core::Schematic::default();
    let label = |s: &mut sch_core::Schematic, comp: usize, pin: &str, text: &str| {
        let at = s.components[comp].pins().into_iter().find(|p| p.0 == pin).unwrap().1;
        s.labels.push(sch_core::Label { pos: at, text: text.into() });
    };
    let v = s.add(Kind::VSource, (0, 0), 0);
    s.components[v].value = "9".into();
    label(&mut s, v, "+", "VIN");
    label(&mut s, v, "-", "GND");
    let cin = s.add(Kind::Capacitor, (10, 0), 1);
    s.components[cin].value = "10u".into();
    label(&mut s, cin, "1", "VIN");
    label(&mut s, cin, "2", "GND");
    let u = s.add_custom(tp.symbol(), &tp.default_value(), &tp.footprint_name(), (34, 0), 0);
    for (pin, net) in [("1", "VIN"), ("2", "LX"), ("7", "GND"), ("8", "VS"), ("9", "BAT"), ("10", "VREG"), ("11", "GND"), ("12", "RTRICK"), ("14", "STDBY"), ("15", "CHRG")] {
        label(&mut s, u, pin, net);
    }
    // the inductor and the catch diode of the buck stage, and the current sense resistor
    let l = s.add(Kind::Inductor, (52, -8), 0);
    s.components[l].value = "10u".into();
    label(&mut s, l, "1", "LX");
    label(&mut s, l, "2", "VS");
    let d = s.add(Kind::Diode, (52, 6), 1);
    s.components[d].value = "1N5817".into();
    label(&mut s, d, "A", "GND");
    label(&mut s, d, "K", "LX");
    let rs = s.add(Kind::Resistor, (66, -8), 0);
    s.components[rs].value = "0.1".into();
    label(&mut s, rs, "1", "VS");
    label(&mut s, rs, "2", "BAT");
    let cell = s.add(Kind::Capacitor, (80, 0), 1);
    s.components[cell].value = "470u IC=2.6 ESR=0.5".into();
    label(&mut s, cell, "1", "BAT");
    label(&mut s, cell, "2", "GND");
    let creg = s.add(Kind::Capacitor, (10, 16), 1);
    s.components[creg].value = "100n".into();
    label(&mut s, creg, "1", "VREG");
    label(&mut s, creg, "2", "GND");
    let rt = s.add(Kind::Resistor, (22, 16), 1);
    s.components[rt].value = "50k".into();
    label(&mut s, rt, "1", "RTRICK");
    label(&mut s, rt, "2", "GND");
    for (k, net) in ["CHRG", "STDBY"].iter().enumerate() {
        let y = 22 + 12 * k as i32;
        let r = s.add(Kind::Resistor, (44, y), 0);
        let d = s.add(Kind::Led, (58, y), 0);
        s.components[r].value = "1k".into();
        label(&mut s, r, "1", "VIN");
        let tag = format!("A{k}");
        label(&mut s, r, "2", &tag);
        label(&mut s, d, "A", &tag);
        label(&mut s, d, "K", net);
    }
    std::fs::write(root.join("tp5100_charger.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Writes docs/examples/ip2332_charger.schjson: `cargo test -p app write_ip2332_example -- --ignored`
#[test]
#[ignore]
fn write_ip2332_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let app = App::default();
    let ip = app.sch.library.iter().find(|d| d.name == "IP2332").expect("IP2332 is bundled");
    let mut s = sch_core::Schematic::default();
    let label = |s: &mut sch_core::Schematic, comp: usize, pin: &str, text: &str| {
        let at = s.components[comp].pins().into_iter().find(|p| p.0 == pin).unwrap().1;
        s.labels.push(sch_core::Label { pos: at, text: text.into() });
    };
    let v = s.add(Kind::VSource, (0, 0), 0);
    s.components[v].value = "5".into();
    label(&mut s, v, "+", "VIN");
    label(&mut s, v, "-", "GND");
    let cin = s.add(Kind::Capacitor, (10, 0), 1);
    s.components[cin].value = "10u".into();
    label(&mut s, cin, "1", "VIN");
    label(&mut s, cin, "2", "GND");
    let u = s.add_custom(ip.symbol(), &ip.default_value(), &ip.footprint_name(), (34, 0), 0);
    for (pin, net) in [("8", "VIN"), ("6", "LX"), ("5", "BAT"), ("7", "GND"), ("1", "NTC"), ("2", "ISET"), ("4", "LED1"), ("3", "LED2")] {
        label(&mut s, u, pin, net);
    }
    let l = s.add(Kind::Inductor, (52, -8), 0);
    s.components[l].value = "2.2u".into();
    label(&mut s, l, "1", "LX");
    label(&mut s, l, "2", "BAT");
    let cout = s.add(Kind::Capacitor, (64, 0), 1);
    s.components[cout].value = "10u".into();
    label(&mut s, cout, "1", "BAT");
    label(&mut s, cout, "2", "GND");
    let cell = s.add(Kind::Capacitor, (76, 0), 1);
    s.components[cell].value = "470u IC=2.6 ESR=0.1".into();
    label(&mut s, cell, "1", "BAT");
    label(&mut s, cell, "2", "GND");
    let riset = s.add(Kind::Resistor, (10, 16), 1);
    s.components[riset].value = "4.3k".into();
    label(&mut s, riset, "1", "ISET");
    label(&mut s, riset, "2", "GND");
    let rntc = s.add(Kind::Resistor, (22, 16), 1);
    s.components[rntc].value = "10k".into();
    label(&mut s, rntc, "1", "NTC");
    label(&mut s, rntc, "2", "GND");
    // LED1 is a constant-current output (no resistor), LED2 needs one
    let d1 = s.add(Kind::Led, (48, 18), 0);
    label(&mut s, d1, "A", "LED1");
    label(&mut s, d1, "K", "GND");
    let r2 = s.add(Kind::Resistor, (48, 30), 0);
    s.components[r2].value = "2k".into();
    label(&mut s, r2, "1", "LED2");
    label(&mut s, r2, "2", "A2");
    let d2 = s.add(Kind::Led, (62, 30), 0);
    label(&mut s, d2, "A", "A2");
    label(&mut s, d2, "K", "GND");
    std::fs::write(root.join("ip2332_charger.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Writes scratch boards (imported + auto-placed + auto-routed) for looking at the 3D view:
/// `cargo test -p app write_3d_demos -- --ignored`
#[test]
#[ignore]
fn write_3d_demos() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let dir = std::env::temp_dir().join("claude");
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["opamp_amp", "regulator_led", "bridge_rectifier", "ne555_blinker"] {
        let mut app = App::default();
        app.sch.load_path(root.join(format!("{name}.schjson")));
        app.board_ed.import_from_schematic(&app.sch);
        app.board_ed.autoplace();
        app.board_ed.autoroute_now(None);
        std::fs::write(dir.join(format!("3d_{name}.pcbjson")), serde_json::to_string_pretty(&app.board_ed.board).unwrap()).unwrap();
        println!("{name}: {} parts", app.board_ed.board.parts.len());
    }
}

/// Writes a scratch board with parts on both sides (not part of the repository):
/// `cargo test -p app write_bottom_demo -- --ignored`
#[test]
#[ignore]
fn write_bottom_demo() {
    let mut app = App::default();
    app.sch.load_path(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/rlc_pulse.schjson"));
    app.board_ed.import_from_schematic(&app.sch);
    let b = &mut app.board_ed.board;
    b.name = "bottom".into();
    b.set_rect_outline(0.0, 0.0, 40.0, 25.0);
    for (r, x, y, rot) in [("V1", 6.0, 14.0, 0), ("R1", 16.0, 18.0, 0), ("L1", 25.0, 18.0, 0), ("C1", 33.0, 12.0, 3)] {
        b.place_part(r, Point::new(x, y), rot);
    }
    b.flip_part("L1");
    b.flip_part("C1");
    let dir = std::env::temp_dir().join("claude");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("bottom_demo.pcbjson"), serde_json::to_string_pretty(&*b).unwrap()).unwrap();
}

/// Generates docs/examples/buck_converter.schjson (12 V -> BUCK_DEMO -> 5 V):
/// `cargo test -p app write_buck_example -- --ignored`
#[test]
#[ignore]
fn write_buck_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let find = |n: &str| part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == n).unwrap();
    let (buck, schottky) = (find("BUCK_DEMO"), find("1N5819"));
    let mut s = sch_core::Schematic::default();
    let v = s.add(Kind::VSource, (0, 0), 0);
    s.components[v].value = "12".into();
    let u = s.add_custom(buck.symbol(), &buck.default_value(), &buck.footprint_name(), (24, 0), 0);
    let l = s.add(Kind::Inductor, (44, -6), 0);
    s.components[l].value = "100u DCR=0.05".into();
    let d = s.add_custom(schottky.symbol(), &schottky.default_value(), &schottky.footprint_name(), (44, 8), 0);
    let c = s.add(Kind::Capacitor, (62, 4), 1);
    s.components[c].value = "100u ESR=0.05".into();
    let r1 = s.add(Kind::Resistor, (74, -4), 1);
    s.components[r1].value = "30k".into();
    let r2 = s.add(Kind::Resistor, (74, 8), 1);
    s.components[r2].value = "10k".into();
    let rl = s.add(Kind::Resistor, (86, 4), 1);
    s.components[rl].value = "20".into();
    let label = |s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str| {
        let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap_or_else(|| panic!("pin {pin}"));
        let c = s.components[idx].pos;
        let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
        let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
        s.wires.push(Wire { a: pt, b: end });
        s.labels.push(sch_core::Label { pos: end, text: text.into() });
    };
    label(&mut s, v, "+", "VIN");
    label(&mut s, v, "-", "GND");
    label(&mut s, u, "1", "VIN");
    label(&mut s, u, "2", "GND");
    label(&mut s, u, "3", "SW");
    label(&mut s, u, "4", "FB");
    label(&mut s, l, "1", "SW");
    label(&mut s, l, "2", "VOUT");
    for (pin, net) in [("A", "GND"), ("K", "SW")] {
        label(&mut s, d, pin, net);
    }
    label(&mut s, c, "1", "VOUT");
    label(&mut s, c, "2", "GND");
    label(&mut s, r1, "1", "VOUT");
    label(&mut s, r1, "2", "FB");
    label(&mut s, r2, "1", "FB");
    label(&mut s, r2, "2", "GND");
    label(&mut s, rl, "1", "VOUT");
    label(&mut s, rl, "2", "GND");
    std::fs::write(root.join("buck_converter.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/buck_cout_electrolytic.schjson and buck_cout_ceramic.schjson: the buck sample with
/// its output capacitor swapped for a library part (the rest is identical, so the two can be compared):
/// `cargo test -p app write_buck_cout_examples -- --ignored`
#[test]
#[ignore]
fn write_buck_cout_examples() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let base: sch_core::Schematic = serde_json::from_str(&std::fs::read_to_string(root.join("buck_converter.schjson")).unwrap()).unwrap();
    let find = |n: &str| part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == n).unwrap();
    for (file, part) in [("buck_cout_electrolytic.schjson", "CE_100u_16V"), ("buck_cout_ceramic.schjson", "C_10u_0805")] {
        let def = find(part);
        let mut s = base.clone();
        let c = s.components.iter_mut().find(|c| c.refdes == "C1").unwrap();
        c.kind = Kind::Custom;
        c.custom = Some(def.symbol());
        c.value = def.default_value();
        c.footprint = def.footprint_name();
        std::fs::write(root.join(file), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    }
}

/// Generates docs/examples/cap_impedance.schjson: a 1 V AC source feeds three 1 ohm resistors, each loaded by a
/// capacitor (electrolytic / ceramic / both in parallel). With the 1 ohm series resistor, V(node) ~ |Z| while |Z| << 1 ohm:
/// `cargo test -p app write_cap_impedance_example -- --ignored`
#[test]
#[ignore]
fn write_cap_impedance_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let find = |n: &str| part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == n).unwrap();
    let (elec, cer) = (find("CE_100u_16V"), find("C_10u_0805"));
    let mut s = sch_core::Schematic::default();
    let v = s.add(Kind::VSource, (0, 0), 0);
    s.components[v].value = "SIN(0 1 1k) AC 1".into();
    let label = |s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str| {
        let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap_or_else(|| panic!("pin {pin}"));
        let c = s.components[idx].pos;
        let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
        let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
        s.wires.push(Wire { a: pt, b: end });
        s.labels.push(sch_core::Label { pos: end, text: text.into() });
    };
    label(&mut s, v, "+", "IN");
    label(&mut s, v, "-", "GND");
    // (node, x, parts on that node)
    for (node, x, caps) in [("ZE", 20, vec![(&elec, 0)]), ("ZC", 36, vec![(&cer, 0)]), ("ZP", 52, vec![(&elec, 0), (&cer, 12)])] {
        let r = s.add(Kind::Resistor, (x, 0), 1);
        s.components[r].value = "1".into();
        label(&mut s, r, "1", "IN");
        label(&mut s, r, "2", node);
        for (def, dx) in caps {
            let c = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (x + dx, 14), 1);
            label(&mut s, c, "1", node);
            label(&mut s, c, "2", "GND");
        }
    }
    std::fs::write(root.join("cap_impedance.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/crystal_response.schjson: the 16 MHz crystal in a 50 ohm line (V1 -> Y1 -> R1 to GND), for the
/// crystal lesson and its AC test: `cargo test -p app write_crystal_example -- --ignored`
#[test]
#[ignore]
fn write_crystal_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "XTAL_16MHz_3225").unwrap();
    let mut s = sch_core::Schematic::default();
    let label = |s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str| {
        let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap_or_else(|| panic!("pin {pin}"));
        let c = s.components[idx].pos;
        let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
        let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
        s.wires.push(Wire { a: pt, b: end });
        s.labels.push(sch_core::Label { pos: end, text: text.into() });
    };
    let v = s.add(Kind::VSource, (0, 0), 0);
    s.components[v].value = "SIN(0 1 16Meg) AC 1".into();
    label(&mut s, v, "+", "IN");
    label(&mut s, v, "-", "GND");
    let y = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (20, 0), 0);
    label(&mut s, y, "1", "IN");
    label(&mut s, y, "3", "OUT");
    label(&mut s, y, "2", "GND");
    label(&mut s, y, "4", "GND");
    let r = s.add(Kind::Resistor, (40, 0), 1);
    s.components[r].value = "50".into();
    label(&mut s, r, "1", "OUT");
    label(&mut s, r, "2", "GND");
    std::fs::write(root.join("crystal_response.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/reset_circuit.schjson: the auto-reset of an Arduino board. V1 = 5 V supply, V2 = the DTR line of
/// the USB-serial adapter (high, then low for good at 1 ms), R1 = 10k pull-up of RESET, C1 = 100n coupling DTR to RESET:
/// `cargo test -p app write_reset_example -- --ignored`
#[test]
#[ignore]
fn write_reset_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut s = sch_core::Schematic::default();
    let label = |s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str| {
        let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap_or_else(|| panic!("pin {pin}"));
        let c = s.components[idx].pos;
        let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
        let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
        s.wires.push(Wire { a: pt, b: end });
        s.labels.push(sch_core::Label { pos: end, text: text.into() });
    };
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "5".into();
    label(&mut s, v1, "+", "VCC");
    label(&mut s, v1, "-", "GND");
    let v2 = s.add(Kind::VSource, (12, 0), 0);
    s.components[v2].value = "PULSE(5 0 1m 1u 1u 8m 20m)".into();
    label(&mut s, v2, "+", "DTR");
    label(&mut s, v2, "-", "GND");
    let r1 = s.add(Kind::Resistor, (24, 0), 1);
    s.components[r1].value = "10k".into();
    label(&mut s, r1, "1", "VCC");
    label(&mut s, r1, "2", "RESET");
    let c1 = s.add(Kind::Capacitor, (34, 0), 1);
    s.components[c1].value = "100n".into();
    label(&mut s, c1, "1", "DTR");
    label(&mut s, c1, "2", "RESET");
    std::fs::write(root.join("reset_circuit.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/pullup_input.schjson: an input pin read through a pull-up. V1 = 5 V, R1 = 10k pull-up, R2 = the
/// button (100Meg = open, a few ohms = pressed; open it is also the pin's leakage), C2 = pin capacitance, C3 = a 2 pF
/// coupling from a noisy line V2 (5 V, 100 kHz): `cargo test -p app write_pullup_example -- --ignored`
#[test]
#[ignore]
fn write_pullup_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut s = sch_core::Schematic::default();
    let label = |s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str| {
        let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap_or_else(|| panic!("pin {pin}"));
        let c = s.components[idx].pos;
        let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
        let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
        s.wires.push(Wire { a: pt, b: end });
        s.labels.push(sch_core::Label { pos: end, text: text.into() });
    };
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "5".into();
    label(&mut s, v1, "+", "VCC");
    label(&mut s, v1, "-", "GND");
    let v2 = s.add(Kind::VSource, (12, 0), 0);
    s.components[v2].value = "SIN(0 5 100k)".into();
    label(&mut s, v2, "+", "NOISE");
    label(&mut s, v2, "-", "GND");
    let r1 = s.add(Kind::Resistor, (24, 0), 1);
    s.components[r1].value = "10k".into();
    label(&mut s, r1, "1", "VCC");
    label(&mut s, r1, "2", "IN");
    let r2 = s.add(Kind::Resistor, (34, 0), 1);
    s.components[r2].value = "100Meg".into();
    label(&mut s, r2, "1", "IN");
    label(&mut s, r2, "2", "GND");
    let c2 = s.add(Kind::Capacitor, (44, 0), 1);
    s.components[c2].value = "5p".into();
    label(&mut s, c2, "1", "IN");
    label(&mut s, c2, "2", "GND");
    let c3 = s.add(Kind::Capacitor, (54, 0), 1);
    s.components[c3].value = "2p".into();
    label(&mut s, c3, "1", "NOISE");
    label(&mut s, c3, "2", "IN");
    std::fs::write(root.join("pullup_input.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/pin_led.schjson: an output pin (High = V1 5 V behind R2 = 25 ohm, the pin's output resistance)
/// driving a LED through R1: `cargo test -p app write_pin_led_example -- --ignored`
#[test]
#[ignore]
fn write_pin_led_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut s = sch_core::Schematic::default();
    let label = |s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str| {
        let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap_or_else(|| panic!("pin {pin}"));
        let c = s.components[idx].pos;
        let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
        let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
        s.wires.push(Wire { a: pt, b: end });
        s.labels.push(sch_core::Label { pos: end, text: text.into() });
    };
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "5".into();
    label(&mut s, v1, "+", "HIGH");
    label(&mut s, v1, "-", "GND");
    // R1 = the series resistor (added first, so it gets the number 1), R2 = the pin's output resistance
    let r1 = s.add(Kind::Resistor, (22, 0), 1);
    s.components[r1].value = "330".into();
    label(&mut s, r1, "1", "PIN");
    label(&mut s, r1, "2", "LED");
    let r2 = s.add(Kind::Resistor, (12, 0), 1);
    s.components[r2].value = "25".into();
    label(&mut s, r2, "1", "HIGH");
    label(&mut s, r2, "2", "PIN");
    let d1 = s.add(Kind::Led, (32, 0), 1);
    label(&mut s, d1, "A", "LED");
    label(&mut s, d1, "K", "GND");
    std::fs::write(root.join("pin_led.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/level_shift.schjson: a 5 V logic signal (V3, 0 - 5 V, 10 kHz, behind 100 ohm) brought down to 3.3 V
/// two ways: a divider (R1 1k + R2 2k, net D33) and a BSS138 shifter (gate on 3.3 V, 10k pull-ups to 5 V on net H and to
/// 3.3 V on net L): `cargo test -p app write_level_shift_example -- --ignored`
#[test]
#[ignore]
fn write_level_shift_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "BSS138").unwrap();
    let mut s = sch_core::Schematic::default();
    let label = |s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str| {
        let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap_or_else(|| panic!("pin {pin}"));
        let c = s.components[idx].pos;
        let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
        let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
        s.wires.push(Wire { a: pt, b: end });
        s.labels.push(sch_core::Label { pos: end, text: text.into() });
    };
    // supplies and the 5 V side signal: V1 = 5 V, V2 = 3.3 V, V3 = the signal
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "5".into();
    label(&mut s, v1, "+", "V5");
    label(&mut s, v1, "-", "GND");
    let v2 = s.add(Kind::VSource, (10, 0), 0);
    s.components[v2].value = "3.3".into();
    label(&mut s, v2, "+", "V33");
    label(&mut s, v2, "-", "GND");
    let v3 = s.add(Kind::VSource, (24, 0), 0);
    s.components[v3].value = "PULSE(0 5 10u 20n 20n 40u 100u)".into();
    label(&mut s, v3, "+", "SIG");
    label(&mut s, v3, "-", "GND");
    // the signal's output resistance, then the two ways down
    let rs = s.add(Kind::Resistor, (34, 0), 1);
    s.components[rs].value = "100".into();
    label(&mut s, rs, "1", "SIG");
    label(&mut s, rs, "2", "H");
    // divider: R1 from H to D33, R2 from D33 to GND
    let r1 = s.add(Kind::Resistor, (44, 0), 1);
    s.components[r1].value = "1k".into();
    label(&mut s, r1, "1", "H");
    label(&mut s, r1, "2", "D33");
    let r2 = s.add(Kind::Resistor, (54, 0), 1);
    s.components[r2].value = "2k".into();
    label(&mut s, r2, "1", "D33");
    label(&mut s, r2, "2", "GND");
    // BSS138 shifter (pins: 1 = G, 3 = D, 2 = S): drain on H (5 V side), source on L (3.3 V side), gate on 3.3 V; pull-ups on both sides
    let q = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (66, 0), 0);
    label(&mut s, q, "1", "V33");
    label(&mut s, q, "3", "H");
    label(&mut s, q, "2", "L");
    let rh = s.add(Kind::Resistor, (78, 0), 1);
    s.components[rh].value = "10k".into();
    label(&mut s, rh, "1", "V5");
    label(&mut s, rh, "2", "H");
    let rl = s.add(Kind::Resistor, (88, 0), 1);
    s.components[rl].value = "10k".into();
    label(&mut s, rl, "1", "V33");
    label(&mut s, rl, "2", "L");
    std::fs::write(root.join("level_shift.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/uart_line.schjson: the serial frame of the letter 'A' (0x41, 9600 baud, 8N1) from a 5 V pin
/// (V1, PWL), 100 ohm output resistance (R1), a cable (R2 1k + C1 100p) down to the receiver's pin RX:
/// `cargo test -p app write_uart_example -- --ignored`
#[test]
#[ignore]
fn write_uart_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut s = sch_core::Schematic::default();
    let label = |s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str| {
        let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap_or_else(|| panic!("pin {pin}"));
        let c = s.components[idx].pos;
        let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
        let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
        s.wires.push(Wire { a: pt, b: end });
        s.labels.push(sch_core::Label { pos: end, text: text.into() });
    };
    // the frame: idle high, start bit (low), 8 data bits LSB first, stop bit (high)
    let bit_us = 1e6 / 9600.0;
    let t0 = 100.0;
    let mut levels = vec![0.0]; // start
    levels.extend((0..8).map(|i| if (0x41u32 >> i) & 1 == 1 { 5.0 } else { 0.0 }));
    levels.push(5.0); // stop
    let mut pwl = String::from("PWL(0 5");
    let mut prev = 5.0;
    for (k, lv) in levels.iter().enumerate() {
        let t = t0 + k as f64 * bit_us;
        if *lv != prev {
            pwl += &format!(" {:.3}u {prev} {:.3}u {lv}", t, t + 0.02);
            prev = *lv;
        }
    }
    pwl += ")";
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = pwl;
    label(&mut s, v1, "+", "TX");
    label(&mut s, v1, "-", "GND");
    let r1 = s.add(Kind::Resistor, (12, 0), 1);
    s.components[r1].value = "100".into();
    label(&mut s, r1, "1", "TX");
    label(&mut s, r1, "2", "LINE");
    let r2 = s.add(Kind::Resistor, (22, 0), 1);
    s.components[r2].value = "1k".into();
    label(&mut s, r2, "1", "LINE");
    label(&mut s, r2, "2", "RX");
    let c1 = s.add(Kind::Capacitor, (32, 0), 1);
    s.components[c1].value = "100p".into();
    label(&mut s, c1, "1", "RX");
    label(&mut s, c1, "2", "GND");
    std::fs::write(root.join("uart_line.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/datasheet_diode.schjson: 5 V -> R1 100 ohm (R_2512, so the resistor stays inside its own rating) ->
/// a 1N4148 (data sheet: 100 V, 0.2 A, 0.5 W), for the
/// data sheet lesson ("部品選びのヒント" and the rating check): `cargo test -p app write_datasheet_example -- --ignored`
#[test]
#[ignore]
fn write_datasheet_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "1N4148").unwrap();
    let pin = |name: &str| def.pins.iter().find(|p| p.name == name).unwrap().number.clone();
    let mut s = sch_core::Schematic::default();
    let label = |s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str| {
        let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap_or_else(|| panic!("pin {pin}"));
        let c = s.components[idx].pos;
        let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
        let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
        s.wires.push(Wire { a: pt, b: end });
        s.labels.push(sch_core::Label { pos: end, text: text.into() });
    };
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "5".into();
    label(&mut s, v1, "+", "VIN");
    label(&mut s, v1, "-", "GND");
    let r1 = s.add(Kind::Resistor, (12, 0), 1);
    s.components[r1].value = "100".into();
    s.components[r1].footprint = "R_2512".into();
    label(&mut s, r1, "1", "VIN");
    label(&mut s, r1, "2", "A");
    let d1 = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (24, 0), 1);
    label(&mut s, d1, &pin("A"), "A");
    label(&mut s, d1, &pin("K"), "GND");
    std::fs::write(root.join("datasheet_diode.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/usb_vbus.schjson: the 5 V of a USB port (V1) behind the host's port switch (R1 0.5 ohm), a cable
/// (L1 0.5 uH, R2 0.25 ohm), the device end VBUS with the bulk capacitor C1 (10 uF) and the device's load R3 (50 ohm = 100 mA); the
/// resistors are R_2512 so that the default stays inside their power rating:
/// `cargo test -p app write_usb_vbus_example -- --ignored`
#[test]
#[ignore]
fn write_usb_vbus_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut s = sch_core::Schematic::default();
    let label = |s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str| {
        let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap_or_else(|| panic!("pin {pin}"));
        let c = s.components[idx].pos;
        let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
        let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
        s.wires.push(Wire { a: pt, b: end });
        s.labels.push(sch_core::Label { pos: end, text: text.into() });
    };
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "5".into();
    label(&mut s, v1, "+", "HOST");
    label(&mut s, v1, "-", "GND");
    let r1 = s.add(Kind::Resistor, (12, 0), 1);
    s.components[r1].value = "0.5".into();
    s.components[r1].footprint = "R_2512".into();
    label(&mut s, r1, "1", "HOST");
    label(&mut s, r1, "2", "PORT");
    let l1 = s.add(Kind::Inductor, (24, 0), 1);
    s.components[l1].value = "500n".into();
    label(&mut s, l1, "1", "PORT");
    label(&mut s, l1, "2", "CABLE");
    let r2 = s.add(Kind::Resistor, (36, 0), 1);
    s.components[r2].value = "0.25".into();
    s.components[r2].footprint = "R_2512".into();
    label(&mut s, r2, "1", "CABLE");
    label(&mut s, r2, "2", "VBUS");
    let c1 = s.add(Kind::Capacitor, (48, 0), 1);
    s.components[c1].value = "10u".into();
    label(&mut s, c1, "1", "VBUS");
    label(&mut s, c1, "2", "GND");
    let r3 = s.add(Kind::Resistor, (60, 0), 1);
    s.components[r3].value = "50".into();
    s.components[r3].footprint = "R_2512".into();
    label(&mut s, r3, "1", "VBUS");
    label(&mut s, r3, "2", "GND");
    std::fs::write(root.join("usb_vbus.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/usb_speed.schjson: how a USB host tells the speed of a device. V1 = 3.3 V (the device's supply), R1 = the
/// 1.5k pull-up on D+ (a full-speed device; R2 = 100Meg on D- is "no pull-up"), R3 and R4 = the host's 15k pull-downs on D+ / D-:
/// `cargo test -p app write_usb_speed_example -- --ignored`
#[test]
#[ignore]
fn write_usb_speed_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut s = sch_core::Schematic::default();
    let label = |s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str| {
        let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap_or_else(|| panic!("pin {pin}"));
        let c = s.components[idx].pos;
        let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
        let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
        s.wires.push(Wire { a: pt, b: end });
        s.labels.push(sch_core::Label { pos: end, text: text.into() });
    };
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "3.3".into();
    label(&mut s, v1, "+", "V33");
    label(&mut s, v1, "-", "GND");
    // the device: pull-ups (R1 on D+, R2 on D-)
    let r1 = s.add(Kind::Resistor, (14, 0), 1);
    s.components[r1].value = "1.5k".into();
    label(&mut s, r1, "1", "V33");
    label(&mut s, r1, "2", "DP");
    let r2 = s.add(Kind::Resistor, (26, 0), 1);
    s.components[r2].value = "100Meg".into();
    label(&mut s, r2, "1", "V33");
    label(&mut s, r2, "2", "DM");
    // the host: pull-downs (R3 on D+, R4 on D-)
    let r3 = s.add(Kind::Resistor, (38, 0), 1);
    s.components[r3].value = "15k".into();
    label(&mut s, r3, "1", "DP");
    label(&mut s, r3, "2", "GND");
    let r4 = s.add(Kind::Resistor, (50, 0), 1);
    s.components[r4].value = "15k".into();
    label(&mut s, r4, "1", "DM");
    label(&mut s, r4, "2", "GND");
    std::fs::write(root.join("usb_speed.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/usb_signal.schjson: a full-speed D+ driver (V1 0 - 3.3 V, 12 Mbit/s bits: 83 ns) behind R1 = 40 ohm,
/// a cable (C1 = 100 pF) and the ESD protection (C2 = 5 pF), seen at the receiver pin DP: `cargo test -p app write_usb_signal_example -- --ignored`
#[test]
#[ignore]
fn write_usb_signal_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut s = sch_core::Schematic::default();
    let label = |s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str| {
        let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap_or_else(|| panic!("pin {pin}"));
        let c = s.components[idx].pos;
        let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
        let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
        s.wires.push(Wire { a: pt, b: end });
        s.labels.push(sch_core::Label { pos: end, text: text.into() });
    };
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "PULSE(0 3.3 20n 1n 1n 82n 166n)".into();
    label(&mut s, v1, "+", "DRV");
    label(&mut s, v1, "-", "GND");
    let r1 = s.add(Kind::Resistor, (12, 0), 1);
    s.components[r1].value = "40".into();
    label(&mut s, r1, "1", "DRV");
    label(&mut s, r1, "2", "DP");
    let c1 = s.add(Kind::Capacitor, (24, 0), 1);
    s.components[c1].value = "100p".into();
    label(&mut s, c1, "1", "DP");
    label(&mut s, c1, "2", "GND");
    let c2 = s.add(Kind::Capacitor, (34, 0), 1);
    s.components[c2].value = "5p".into();
    label(&mut s, c2, "1", "DP");
    label(&mut s, c2, "2", "GND");
    std::fs::write(root.join("usb_signal.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/pwm_filter.schjson: an Arduino PWM pin (V1: 0 / 5 V, 490 Hz, 50 % duty) through an RC low-pass filter
/// (R1 10k, C1 1 uF) to an analog output OUT with the load R2 (1 Meg): `cargo test -p app write_pwm_example -- --ignored`
#[test]
#[ignore]
fn write_pwm_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut s = sch_core::Schematic::default();
    let label = |s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str| {
        let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap_or_else(|| panic!("pin {pin}"));
        let c = s.components[idx].pos;
        let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
        let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
        s.wires.push(Wire { a: pt, b: end });
        s.labels.push(sch_core::Label { pos: end, text: text.into() });
    };
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "PULSE(0 5 0 20n 20n 1.02m 2.04m)".into();
    label(&mut s, v1, "+", "PWM");
    label(&mut s, v1, "-", "GND");
    let r1 = s.add(Kind::Resistor, (12, 0), 1);
    s.components[r1].value = "10k".into();
    label(&mut s, r1, "1", "PWM");
    label(&mut s, r1, "2", "OUT");
    let c1 = s.add(Kind::Capacitor, (24, 0), 1);
    s.components[c1].value = "1u".into();
    label(&mut s, c1, "1", "OUT");
    label(&mut s, c1, "2", "GND");
    let r2 = s.add(Kind::Resistor, (36, 0), 1);
    s.components[r2].value = "1Meg".into();
    label(&mut s, r2, "1", "OUT");
    label(&mut s, r2, "2", "GND");
    std::fs::write(root.join("pwm_filter.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/ldo_compare.schjson: two 3.3 V low-dropout regulators on one input (V1 = 5 V): U1 = AMS1117-3.3 (dropout
/// 1.1 V, up to 15 V in), U2 = MCP1700-3302 (dropout 0.2 V, up to 6 V in), each with a 33 ohm load (100 mA) and its capacitors:
/// `cargo test -p app write_ldo_example -- --ignored`
#[test]
#[ignore]
fn write_ldo_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let find = |n: &str| part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == n).unwrap();
    let (ams, mcp) = (find("AMS1117-3.3"), find("MCP1700-3302"));
    let pin = |d: &part_lib::PartDef, name: &str| d.pins.iter().find(|p| p.name == name).unwrap().number.clone();
    let mut s = sch_core::Schematic::default();
    let label = |s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str| {
        let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap_or_else(|| panic!("pin {pin}"));
        let c = s.components[idx].pos;
        let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
        let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
        s.wires.push(Wire { a: pt, b: end });
        s.labels.push(sch_core::Label { pos: end, text: text.into() });
    };
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "5".into();
    label(&mut s, v1, "+", "VIN");
    label(&mut s, v1, "-", "GND");
    let u1 = s.add_custom(ams.symbol(), &ams.default_value(), &ams.footprint_name(), (20, 0), 0);
    label(&mut s, u1, &pin(&ams, "IN"), "VIN");
    label(&mut s, u1, &pin(&ams, "GND"), "GND");
    label(&mut s, u1, &pin(&ams, "OUT"), "OUT1");
    let u2 = s.add_custom(mcp.symbol(), &mcp.default_value(), &mcp.footprint_name(), (20, 16), 0);
    label(&mut s, u2, &pin(&mcp, "VIN"), "VIN");
    label(&mut s, u2, &pin(&mcp, "GND"), "GND");
    label(&mut s, u2, &pin(&mcp, "VOUT"), "OUT2");
    // loads (33 ohm = 100 mA) and the capacitors the data sheets ask for
    for (k, (net, x)) in [("OUT1", 40), ("OUT2", 52)].into_iter().enumerate() {
        let r = s.add(Kind::Resistor, (x, 0), 1);
        s.components[r].value = "33".into();
        s.components[r].footprint = "R_2512".into();
        label(&mut s, r, "1", net);
        label(&mut s, r, "2", "GND");
        let c = s.add(Kind::Capacitor, (x, 14), 1);
        s.components[c].value = if k == 0 { "10u" } else { "1u" }.into();
        label(&mut s, c, "1", net);
        label(&mut s, c, "2", "GND");
    }
    let c = s.add(Kind::Capacitor, (64, 0), 1);
    s.components[c].value = "1u".into();
    label(&mut s, c, "1", "VIN");
    label(&mut s, c, "2", "GND");
    std::fs::write(root.join("ldo_compare.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/tl431_reference.schjson: a shunt regulator. V1 = 12 V, R1 = 390 ohm (from VIN to VOUT), U1 = TL431 (cathode on
/// VOUT, anode on GND, REF on the divider R2 = 10k from VOUT and R3 = 10k to GND: VOUT = 2.495 V x (1 + R2 / R3) = 5 V), R4 = the load (100k):
/// `cargo test -p app write_tl431_example -- --ignored`
#[test]
#[ignore]
fn write_tl431_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "TL431").unwrap();
    let pin = |name: &str| def.pins.iter().find(|p| p.name == name).unwrap().number.clone();
    let mut s = sch_core::Schematic::default();
    let label = |s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str| {
        let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap_or_else(|| panic!("pin {pin}"));
        let c = s.components[idx].pos;
        let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
        let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
        s.wires.push(Wire { a: pt, b: end });
        s.labels.push(sch_core::Label { pos: end, text: text.into() });
    };
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "12".into();
    label(&mut s, v1, "+", "VIN");
    label(&mut s, v1, "-", "GND");
    let r1 = s.add(Kind::Resistor, (12, 0), 1);
    s.components[r1].value = "390".into();
    s.components[r1].footprint = "R_2512".into();
    label(&mut s, r1, "1", "VIN");
    label(&mut s, r1, "2", "VOUT");
    let u1 = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (26, 4), 0);
    label(&mut s, u1, &pin("CATHODE"), "VOUT");
    label(&mut s, u1, &pin("REF"), "REF");
    label(&mut s, u1, &pin("ANODE"), "GND");
    let r2 = s.add(Kind::Resistor, (40, 0), 1);
    s.components[r2].value = "10k".into();
    label(&mut s, r2, "1", "VOUT");
    label(&mut s, r2, "2", "REF");
    let r3 = s.add(Kind::Resistor, (50, 0), 1);
    s.components[r3].value = "10k".into();
    label(&mut s, r3, "1", "REF");
    label(&mut s, r3, "2", "GND");
    let r4 = s.add(Kind::Resistor, (62, 0), 1);
    s.components[r4].value = "100k".into();
    s.components[r4].footprint = "R_1206".into();
    label(&mut s, r4, "1", "VOUT");
    label(&mut s, r4, "2", "GND");
    std::fs::write(root.join("tl431_reference.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/flyback_coil.schjson: a relay coil (L1 100 mH + R1 100 ohm) switched on the low side by a 2N7000 (Q1) from
/// 12 V (V1); the gate is driven by a 5 V pin (V2, on at 1 ms for 3 ms). No flyback diode yet: the learner adds one (anode on DRAIN,
/// cathode on VCC): `cargo test -p app write_flyback_example -- --ignored`
#[test]
#[ignore]
fn write_flyback_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "2N7000").unwrap();
    let pin = |name: &str| def.pins.iter().find(|p| p.name == name).unwrap().number.clone();
    let mut s = sch_core::Schematic::default();
    let label = |s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str| {
        let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap_or_else(|| panic!("pin {pin}"));
        let c = s.components[idx].pos;
        let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
        let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
        s.wires.push(Wire { a: pt, b: end });
        s.labels.push(sch_core::Label { pos: end, text: text.into() });
    };
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "12".into();
    label(&mut s, v1, "+", "VCC");
    label(&mut s, v1, "-", "GND");
    let v2 = s.add(Kind::VSource, (12, 0), 0);
    s.components[v2].value = "PULSE(0 5 1m 1u 1u 3m 10m)".into();
    label(&mut s, v2, "+", "GATE");
    label(&mut s, v2, "-", "GND");
    let l1 = s.add(Kind::Inductor, (26, 0), 1);
    s.components[l1].value = "100m".into();
    label(&mut s, l1, "1", "VCC");
    label(&mut s, l1, "2", "COIL");
    let r1 = s.add(Kind::Resistor, (38, 0), 1);
    s.components[r1].value = "100".into();
    s.components[r1].footprint = "R_2512".into();
    label(&mut s, r1, "1", "COIL");
    label(&mut s, r1, "2", "DRAIN");
    let q1 = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (52, 0), 0);
    label(&mut s, q1, &pin("G"), "GATE");
    label(&mut s, q1, &pin("D"), "DRAIN");
    label(&mut s, q1, &pin("S"), "GND");
    std::fs::write(root.join("flyback_coil.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/coin_cell_led.schjson (CR2032 -> resistor -> LED):
/// `cargo test -p app write_coin_cell_example -- --ignored`
#[test]
#[ignore]
fn write_coin_cell_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "CR2032").unwrap();
    let mut s = sch_core::Schematic::default();
    let bt = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (0, 0), 0);
    let r = s.add(Kind::Resistor, (20, 0), 0);
    s.components[r].value = "100".into();
    let d = s.add(Kind::Led, (34, 0), 0);
    let label = |s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str| {
        let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap();
        let c = s.components[idx].pos;
        let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
        let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
        s.wires.push(Wire { a: pt, b: end });
        s.labels.push(sch_core::Label { pos: end, text: text.into() });
    };
    label(&mut s, bt, "1", "VBAT");
    label(&mut s, bt, "2", "GND");
    label(&mut s, r, "1", "VBAT");
    label(&mut s, r, "2", "N1");
    label(&mut s, d, "A", "N1");
    label(&mut s, d, "K", "GND");
    std::fs::write(root.join("coin_cell_led.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/bridge_rectifier.schjson (100 V AC -> transformer -> bridge -> smoothing):
/// `cargo test -p app write_rectifier_example -- --ignored`
#[test]
#[ignore]
fn write_rectifier_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = part_lib::EXAMPLES
        .iter()
        .map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap())
        .find(|d| d.name == "TR100_12V_5VA")
        .unwrap();
    let mut s = sch_core::Schematic::default();
    let v = s.add(Kind::VSource, (0, 0), 0);
    s.components[v].value = "SIN(0 141.4 50) AC 1".into();
    let t = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (16, 0), 0);
    let mut diodes = vec![];
    for (k, x) in [(0, 40), (1, 52), (2, 64), (3, 76)] {
        diodes.push(s.add(Kind::Diode, (x, 0), 0));
        s.components[diodes[k]].value = "1N4007".into();
    }
    let c = s.add(Kind::Capacitor, (92, 0), 1);
    s.components[c].value = "1000u".into();
    let r = s.add(Kind::Resistor, (104, 0), 1);
    s.components[r].value = "100".into();
    // a short wire out of the pin, with the net name at its end (so the name does not sit on the symbol)
    let label = |s: &mut sch_core::Schematic, idx: usize, pin: usize, text: &str| {
        let pt = s.components[idx].pins()[pin].1;
        let c = s.components[idx].pos;
        let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
        let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
        s.wires.push(Wire { a: pt, b: end });
        s.labels.push(sch_core::Label { pos: end, text: text.into() });
    };
    for (i, p) in s.components[t].pins().iter().enumerate() { println!("T pin {i} {:?} {:?}", p.0, p.1); }
    for (i, p) in s.components[diodes[0]].pins().iter().enumerate() { println!("D pin {i} {:?} {:?}", p.0, p.1); }
    label(&mut s, v, 0, "MAINS");
    label(&mut s, v, 1, "GND");
    label(&mut s, t, 0, "MAINS");
    label(&mut s, t, 1, "GND");
    label(&mut s, t, 2, "AC1");
    label(&mut s, t, 3, "AC2");
    // pins of a diode: 0 = anode, 1 = cathode
    for (k, (a, kk)) in [("AC1", "VDC"), ("AC2", "VDC"), ("GND", "AC1"), ("GND", "AC2")].iter().enumerate() {
        label(&mut s, diodes[k], 0, a);
        label(&mut s, diodes[k], 1, kk);
    }
    label(&mut s, c, 0, "VDC");
    label(&mut s, c, 1, "GND");
    label(&mut s, r, 0, "VDC");
    label(&mut s, r, 1, "GND");
    std::fs::write(root.join("bridge_rectifier.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/variable_supply.schjson (100 V AC -> transformer -> bridge -> smoothing -> LM317 whose
/// R2 is the output dial, 100 ohm load): `cargo test -p app write_variable_supply_example -- --ignored`
#[test]
#[ignore]
fn write_variable_supply_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let (tr, reg) = (library_part("TR100_12V_5VA"), library_part("LM317"));
    let mut s = sch_core::Schematic::default();
    let v = s.add(Kind::VSource, (0, 0), 0);
    s.components[v].value = "SIN(0 141.4 50) AC 1".into();
    let t = s.add_custom(tr.symbol(), &tr.default_value(), &tr.footprint_name(), (16, 0), 0);
    let mut diodes = vec![];
    for x in [40, 52, 64, 76] {
        let d = s.add(Kind::Diode, (x, 0), 0);
        s.components[d].value = "1N4007".into();
        diodes.push(d);
    }
    let c1 = s.add(Kind::Capacitor, (92, 0), 1);
    s.components[c1].value = "1000u".into();
    let u1 = s.add_custom(reg.symbol(), &reg.default_value(), &reg.footprint_name(), (116, -4), 0);
    // R1 (OUT-ADJ) is fixed, R2 (ADJ-GND) is the dial
    let r1 = s.add(Kind::Resistor, (134, -10), 1);
    s.components[r1].value = "240".into();
    let r2 = s.add(Kind::Resistor, (134, 6), 1);
    s.components[r2].value = "1.2k".into();
    let c2 = s.add(Kind::Capacitor, (148, 4), 1);
    s.components[c2].value = "1u".into();
    let r3 = s.add(Kind::Resistor, (162, 4), 1);
    s.components[r3].value = "100".into();
    s.components[r3].footprint = "R_2512".into();
    // a short wire out of the pin, with the net name at its end (so the name does not sit on the symbol)
    let label = |s: &mut sch_core::Schematic, idx: usize, pin: usize, text: &str| {
        let pt = s.components[idx].pins()[pin].1;
        let c = s.components[idx].pos;
        let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
        let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
        s.wires.push(Wire { a: pt, b: end });
        s.labels.push(sch_core::Label { pos: end, text: text.into() });
    };
    label(&mut s, v, 0, "MAINS");
    label(&mut s, v, 1, "GND");
    label(&mut s, t, 0, "MAINS");
    label(&mut s, t, 1, "GND");
    label(&mut s, t, 2, "AC1");
    label(&mut s, t, 3, "AC2");
    // pins of a diode: 0 = anode, 1 = cathode
    for (k, (a, kk)) in [("AC1", "VDC"), ("AC2", "VDC"), ("GND", "AC1"), ("GND", "AC2")].iter().enumerate() {
        label(&mut s, diodes[k], 0, a);
        label(&mut s, diodes[k], 1, kk);
    }
    label(&mut s, c1, 0, "VDC");
    label(&mut s, c1, 1, "GND");
    label_pin(&mut s, u1, "3", "VDC");
    label_pin(&mut s, u1, "2", "VOUT");
    label_pin(&mut s, u1, "1", "ADJ");
    label(&mut s, r1, 0, "VOUT");
    label(&mut s, r1, 1, "ADJ");
    label(&mut s, r2, 0, "ADJ");
    label(&mut s, r2, 1, "GND");
    label(&mut s, c2, 0, "VOUT");
    label(&mut s, c2, 1, "GND");
    label(&mut s, r3, 0, "VOUT");
    label(&mut s, r3, 1, "GND");
    std::fs::write(root.join("variable_supply.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/mosfet_switch.schjson (5 V pulse -> 2N7000 gate -> 47 ohm load to 5 V):
/// `cargo test -p app write_mosfet_example -- --ignored`
#[test]
#[ignore]
fn write_mosfet_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = part_lib::EXAMPLES
        .iter()
        .map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap())
        .find(|d| d.name == "2N7000")
        .unwrap();
    let mut s = sch_core::Schematic::default();
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "5".into();
    let v2 = s.add(Kind::VSource, (0, 14), 0);
    s.components[v2].value = "PULSE(0 5 100u 1u 1u 400u 1m)".into();
    let r1 = s.add(Kind::Resistor, (10, 8), 0);
    s.components[r1].value = "100".into();
    let r2 = s.add(Kind::Resistor, (20, -4), 1);
    s.components[r2].value = "47".into();
    s.components[r2].footprint = "R_2512".into();
    let q = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (20, 8), 0);
    // a short wire out of the pin with the net name at its end, so the name does not sit on the reference text
    let label_at = |s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str| {
        let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap();
        let c = s.components[idx].pos;
        let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
        let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
        s.wires.push(Wire { a: pt, b: end });
        s.labels.push(sch_core::Label { pos: end, text: text.into() });
    };
    label_at(&mut s, v1, "+", "VCC");
    label_at(&mut s, v1, "-", "GND");
    label_at(&mut s, v2, "+", "IN");
    label_at(&mut s, v2, "-", "GND");
    label_at(&mut s, r1, "1", "IN");
    label_at(&mut s, r1, "2", "G");
    label_at(&mut s, r2, "1", "VCC");
    label_at(&mut s, r2, "2", "OUT");
    label_at(&mut s, q, "2", "G");
    label_at(&mut s, q, "3", "OUT");
    label_at(&mut s, q, "1", "GND");
    std::fs::write(root.join("mosfet_switch.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/pnp_highside.schjson (PNP 2N3906 switching a 220 ohm load from the 5 V rail):
/// `cargo test -p app write_pnp_example -- --ignored`
#[test]
#[ignore]
fn write_pnp_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = library_part("2N3906");
    let mut s = sch_core::Schematic::default();
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "5".into();
    let v2 = s.add(Kind::VSource, (0, 16), 0);
    s.components[v2].value = "PULSE(5 0 100u 1u 1u 400u 1m)".into();
    let r1 = s.add(Kind::Resistor, (12, 10), 0);
    s.components[r1].value = "4.7k".into();
    let r3 = s.add(Kind::Resistor, (20, -2), 1);
    s.components[r3].value = "10k".into();
    let q = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (28, 6), 0);
    let r2 = s.add(Kind::Resistor, (28, 20), 1);
    s.components[r2].value = "220".into();
    s.components[r2].footprint = "R_1206".into();
    label_pin(&mut s, v1, "+", "VCC");
    label_pin(&mut s, v1, "-", "GND");
    label_pin(&mut s, v2, "+", "IN");
    label_pin(&mut s, v2, "-", "GND");
    label_pin(&mut s, r1, "1", "IN");
    label_pin(&mut s, r1, "2", "B");
    label_pin(&mut s, r3, "1", "VCC");
    label_pin(&mut s, r3, "2", "B");
    label_pin(&mut s, q, "2", "B");
    label_pin(&mut s, q, "1", "VCC");
    label_pin(&mut s, q, "3", "OUT");
    label_pin(&mut s, r2, "1", "OUT");
    label_pin(&mut s, r2, "2", "GND");
    std::fs::write(root.join("pnp_highside.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/coil_layer_short.schjson (a relay coil driven by a 2N7000 with a flyback diode):
/// `cargo test -p app write_coil_example -- --ignored`
#[test]
#[ignore]
fn write_coil_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = library_part("2N7000");
    let mut s = sch_core::Schematic::default();
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "12".into();
    let v2 = s.add(Kind::VSource, (0, 16), 0);
    s.components[v2].value = "PULSE(0 5 0 10u 10u 1m 2m)".into();
    let r1 = s.add(Kind::Resistor, (12, 10), 0);
    s.components[r1].value = "100".into();
    let l1 = s.add(Kind::Inductor, (28, -6), 1);
    s.components[l1].value = "20m DCR=80".into();
    let d1 = s.add(Kind::Diode, (38, -6), 1);
    let q = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (28, 10), 0);
    label_pin(&mut s, v1, "+", "VCC");
    label_pin(&mut s, v1, "-", "GND");
    label_pin(&mut s, v2, "+", "IN");
    label_pin(&mut s, v2, "-", "GND");
    label_pin(&mut s, r1, "1", "IN");
    label_pin(&mut s, r1, "2", "G");
    label_pin(&mut s, l1, "1", "VCC");
    label_pin(&mut s, l1, "2", "OUT");
    label_pin(&mut s, d1, "A", "OUT");
    label_pin(&mut s, d1, "K", "VCC");
    label_pin(&mut s, q, "2", "G");
    label_pin(&mut s, q, "3", "OUT");
    label_pin(&mut s, q, "1", "GND");
    std::fs::write(root.join("coil_layer_short.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/opamp_inverting.schjson (LM358, gain -10, +-5 V supplies):
/// `cargo test -p app write_inverting_example -- --ignored`
#[test]
#[ignore]
fn write_inverting_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = library_part("LM358");
    let mut s = sch_core::Schematic::default();
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "SIN(0 0.1 1k) AC 1".into();
    let r1 = s.add(Kind::Resistor, (14, 0), 0);
    s.components[r1].value = "1k".into();
    let r2 = s.add(Kind::Resistor, (26, -10), 0);
    s.components[r2].value = "10k".into();
    let u = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (28, 2), 0);
    let vp = s.add(Kind::VSource, (44, -6), 0);
    s.components[vp].value = "5".into();
    let vn = s.add(Kind::VSource, (44, 10), 0);
    s.components[vn].value = "5".into();
    label_pin(&mut s, v1, "+", "IN");
    label_pin(&mut s, v1, "-", "GND");
    label_pin(&mut s, r1, "1", "IN");
    label_pin(&mut s, r1, "2", "N");
    label_pin(&mut s, r2, "1", "N");
    label_pin(&mut s, r2, "2", "OUT");
    label_pin(&mut s, u, "2", "N");
    label_pin(&mut s, u, "3", "GND");
    label_pin(&mut s, u, "1", "OUT");
    label_pin(&mut s, u, "8", "VCC");
    label_pin(&mut s, u, "4", "VEE");
    label_pin(&mut s, vp, "+", "VCC");
    label_pin(&mut s, vp, "-", "GND");
    label_pin(&mut s, vn, "+", "GND");
    label_pin(&mut s, vn, "-", "VEE");
    std::fs::write(root.join("opamp_inverting.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/diode_iv.schjson (two diodes, each behind 100 ohm, fed by one source to sweep):
/// `cargo test -p app write_diode_example -- --ignored`
#[test]
#[ignore]
fn write_diode_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let (si, schottky) = (library_part("1N4007"), library_part("1N5819"));
    let mut s = sch_core::Schematic::default();
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "3".into();
    let r1 = s.add(Kind::Resistor, (14, -6), 0);
    s.components[r1].value = "150".into();
    let d1 = s.add_custom(si.symbol(), &si.default_value(), &si.footprint_name(), (28, -6), 0);
    let r2 = s.add(Kind::Resistor, (14, 8), 0);
    s.components[r2].value = "150".into();
    let d2 = s.add_custom(schottky.symbol(), &schottky.default_value(), &schottky.footprint_name(), (28, 8), 0);
    label_pin(&mut s, v1, "+", "VIN");
    label_pin(&mut s, v1, "-", "GND");
    label_pin(&mut s, r1, "1", "VIN");
    label_pin(&mut s, r1, "2", "A1");
    label_pin(&mut s, d1, "A", "A1");
    label_pin(&mut s, d1, "K", "GND");
    label_pin(&mut s, r2, "1", "VIN");
    label_pin(&mut s, r2, "2", "A2");
    label_pin(&mut s, d2, "A", "A2");
    label_pin(&mut s, d2, "K", "GND");
    std::fs::write(root.join("diode_iv.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/zener_regulator.schjson (9 V -> 150 ohm -> 5.1 V zener, 1 k load):
/// `cargo test -p app write_zener_example -- --ignored`
#[test]
#[ignore]
fn write_zener_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let z = library_part("1N4733A");
    let mut s = sch_core::Schematic::default();
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "9".into();
    let r1 = s.add(Kind::Resistor, (14, -8), 0);
    s.components[r1].value = "150".into();
    s.components[r1].footprint = "R_1206".into();
    let d1 = s.add_custom(z.symbol(), &z.default_value(), &z.footprint_name(), (28, 2), 1);
    let r2 = s.add(Kind::Resistor, (42, 2), 1);
    s.components[r2].value = "1k".into();
    label_pin(&mut s, v1, "+", "VIN");
    label_pin(&mut s, v1, "-", "GND");
    label_pin(&mut s, r1, "1", "VIN");
    label_pin(&mut s, r1, "2", "VOUT");
    label_pin(&mut s, d1, "K", "VOUT");
    label_pin(&mut s, d1, "A", "GND");
    label_pin(&mut s, r2, "1", "VOUT");
    label_pin(&mut s, r2, "2", "GND");
    std::fs::write(root.join("zener_regulator.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/emitter_follower.schjson (zener reference + NPN follower, 220 ohm load):
/// `cargo test -p app write_follower_example -- --ignored`
#[test]
#[ignore]
fn write_follower_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let (z, q) = (library_part("1N4733A"), library_part("2N3904"));
    let mut s = sch_core::Schematic::default();
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "9".into();
    let r1 = s.add(Kind::Resistor, (14, -8), 0);
    s.components[r1].value = "330".into();
    let d1 = s.add_custom(z.symbol(), &z.default_value(), &z.footprint_name(), (28, 2), 1);
    let q1 = s.add_custom(q.symbol(), &q.default_value(), &q.footprint_name(), (44, -2), 0);
    let r2 = s.add(Kind::Resistor, (58, 6), 1);
    s.components[r2].value = "220".into();
    s.components[r2].footprint = "R_1206".into();
    label_pin(&mut s, v1, "+", "VIN");
    label_pin(&mut s, v1, "-", "GND");
    label_pin(&mut s, r1, "1", "VIN");
    label_pin(&mut s, r1, "2", "Z");
    label_pin(&mut s, d1, "K", "Z");
    label_pin(&mut s, d1, "A", "GND");
    for (pin, net) in [("C", "VIN"), ("B", "Z"), ("E", "VOUT")] {
        // the library part names its pins by number: look the number up by name
        let num = q.pins.iter().find(|p| p.name == pin).map(|p| p.number.clone()).unwrap();
        label_pin(&mut s, q1, &num, net);
    }
    label_pin(&mut s, r2, "1", "VOUT");
    label_pin(&mut s, r2, "2", "GND");
    std::fs::write(root.join("emitter_follower.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/series_regulator.schjson (3.3 V zener reference + LM358 + NPN pass transistor
/// and feedback divider -> 5 V, 100 ohm load):
/// `cargo test -p app write_series_regulator_example -- --ignored`
#[test]
#[ignore]
fn write_series_regulator_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let (z, q, op) = (library_part("1N4728A"), library_part("2N3904"), library_part("LM358"));
    let mut s = sch_core::Schematic::default();
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "9".into();
    let r1 = s.add(Kind::Resistor, (14, -10), 0);
    s.components[r1].value = "1k".into();
    s.components[r1].footprint = "R_1206".into();
    let d1 = s.add_custom(z.symbol(), &z.default_value(), &z.footprint_name(), (28, 0), 1);
    let u = s.add_custom(op.symbol(), &op.default_value(), &op.footprint_name(), (44, -4), 0);
    let q1 = s.add_custom(q.symbol(), &q.default_value(), &q.footprint_name(), (62, -8), 0);
    let r2 = s.add(Kind::Resistor, (78, 4), 1);
    s.components[r2].value = "100".into();
    s.components[r2].footprint = "R_2512".into();
    let r3 = s.add(Kind::Resistor, (62, 10), 1);
    s.components[r3].value = "5.1k".into();
    let r4 = s.add(Kind::Resistor, (50, 14), 1);
    s.components[r4].value = "10k".into();
    label_pin(&mut s, v1, "+", "VIN");
    label_pin(&mut s, v1, "-", "GND");
    label_pin(&mut s, r1, "1", "VIN");
    label_pin(&mut s, r1, "2", "VREF");
    label_pin(&mut s, d1, "K", "VREF");
    label_pin(&mut s, d1, "A", "GND");
    label_pin(&mut s, u, "3", "VREF");
    label_pin(&mut s, u, "2", "FB");
    label_pin(&mut s, u, "1", "DRIVE");
    label_pin(&mut s, u, "8", "VIN");
    label_pin(&mut s, u, "4", "GND");
    for (pin, net) in [("C", "VIN"), ("B", "DRIVE"), ("E", "VOUT")] {
        let num = q.pins.iter().find(|p| p.name == pin).map(|p| p.number.clone()).unwrap();
        label_pin(&mut s, q1, &num, net);
    }
    label_pin(&mut s, r3, "1", "VOUT");
    label_pin(&mut s, r3, "2", "FB");
    label_pin(&mut s, r4, "1", "FB");
    label_pin(&mut s, r4, "2", "GND");
    label_pin(&mut s, r2, "1", "VOUT");
    label_pin(&mut s, r2, "2", "GND");
    std::fs::write(root.join("series_regulator.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/regulator_78xx.schjson (12 V -> LM7805 with 0.33 uF / 0.1 uF capacitors, 100 ohm load):
/// `cargo test -p app write_78xx_example -- --ignored`
#[test]
#[ignore]
fn write_78xx_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let u = library_part("LM7805");
    let mut s = sch_core::Schematic::default();
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "12".into();
    let c1 = s.add(Kind::Capacitor, (14, 4), 1);
    s.components[c1].value = "330n".into();
    let u1 = s.add_custom(u.symbol(), &u.default_value(), &u.footprint_name(), (30, -4), 0);
    let c2 = s.add(Kind::Capacitor, (46, 4), 1);
    s.components[c2].value = "100n".into();
    let r2 = s.add(Kind::Resistor, (60, 4), 1);
    s.components[r2].value = "100".into();
    s.components[r2].footprint = "R_2512".into();
    label_pin(&mut s, v1, "+", "VIN");
    label_pin(&mut s, v1, "-", "GND");
    label_pin(&mut s, c1, "1", "VIN");
    label_pin(&mut s, c1, "2", "GND");
    label_pin(&mut s, u1, "1", "VIN");
    label_pin(&mut s, u1, "2", "GND");
    label_pin(&mut s, u1, "3", "VOUT");
    label_pin(&mut s, c2, "1", "VOUT");
    label_pin(&mut s, c2, "2", "GND");
    label_pin(&mut s, r2, "1", "VOUT");
    label_pin(&mut s, r2, "2", "GND");
    std::fs::write(root.join("regulator_78xx.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/reverse_protection.schjson (9 V source feeding three 330 ohm loads: bare, behind a
/// Schottky diode, behind a P-channel MOSFET; set V1 to -9 to connect the supply backwards):
/// `cargo test -p app write_protection_example -- --ignored`
#[test]
#[ignore]
fn write_protection_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let (d, q) = (library_part("1N5819"), library_part("AO3401A"));
    let mut s = sch_core::Schematic::default();
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "9".into();
    let r3 = s.add(Kind::Resistor, (20, -16), 1);
    s.components[r3].value = "330".into();
    s.components[r3].footprint = "R_2512".into();
    let d1 = s.add_custom(d.symbol(), &d.default_value(), &d.footprint_name(), (16, 0), 0);
    let r1 = s.add(Kind::Resistor, (34, 4), 1);
    s.components[r1].value = "330".into();
    s.components[r1].footprint = "R_2512".into();
    let q1 = s.add_custom(q.symbol(), &q.default_value(), &q.footprint_name(), (16, 16), 0);
    let r2 = s.add(Kind::Resistor, (34, 20), 1);
    s.components[r2].value = "330".into();
    s.components[r2].footprint = "R_2512".into();
    label_pin(&mut s, v1, "+", "VIN");
    label_pin(&mut s, v1, "-", "GND");
    label_pin(&mut s, r3, "1", "VIN");
    label_pin(&mut s, r3, "2", "GND");
    label_pin(&mut s, d1, "A", "VIN");
    label_pin(&mut s, d1, "K", "OUT_D");
    label_pin(&mut s, r1, "1", "OUT_D");
    label_pin(&mut s, r1, "2", "GND");
    for (pin, net) in [("D", "VIN"), ("G", "GND"), ("S", "OUT_M")] {
        let num = q.pins.iter().find(|p| p.name == pin).map(|p| p.number.clone()).unwrap();
        label_pin(&mut s, q1, &num, net);
    }
    label_pin(&mut s, r2, "1", "OUT_M");
    label_pin(&mut s, r2, "2", "GND");
    std::fs::write(root.join("reverse_protection.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/power_supply.schjson (the final challenge's answer: 12 V -> reverse-protection
/// Schottky diode -> LM7805 with its two capacitors -> 5 V):
/// `cargo test -p app write_power_supply_example -- --ignored`
#[test]
#[ignore]
fn write_power_supply_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let (d, u) = (library_part("1N5819"), library_part("LM7805"));
    let mut s = sch_core::Schematic::default();
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "12".into();
    let d1 = s.add_custom(d.symbol(), &d.default_value(), &d.footprint_name(), (14, -8), 0);
    let c1 = s.add(Kind::Capacitor, (28, 4), 1);
    s.components[c1].value = "330n".into();
    let u1 = s.add_custom(u.symbol(), &u.default_value(), &u.footprint_name(), (42, -4), 0);
    let c2 = s.add(Kind::Capacitor, (58, 4), 1);
    s.components[c2].value = "100n".into();
    label_pin(&mut s, v1, "+", "VIN");
    label_pin(&mut s, v1, "-", "GND");
    label_pin(&mut s, d1, "A", "VIN");
    label_pin(&mut s, d1, "K", "VPROT");
    label_pin(&mut s, c1, "1", "VPROT");
    label_pin(&mut s, c1, "2", "GND");
    label_pin(&mut s, u1, "1", "VPROT");
    label_pin(&mut s, u1, "2", "GND");
    label_pin(&mut s, u1, "3", "VOUT");
    label_pin(&mut s, c2, "1", "VOUT");
    label_pin(&mut s, c2, "2", "GND");
    std::fs::write(root.join("power_supply.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/regulator_led.schjson (12 V -> LM7805 -> resistor -> LED):
/// `cargo test -p app write_library_demo -- --ignored`
#[test]
#[ignore]
fn write_library_demo() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = part_lib::EXAMPLES
        .iter()
        .map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap())
        .find(|d| d.name == "LM7805")
        .unwrap();
    let mut sch = sch_core::Schematic::default();
    sch.add(Kind::VSource, (0, 0), 0);
    sch.components[0].value = "12".into();
    sch.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (14, 0), 0);
    sch.add(Kind::Resistor, (22, 0), 0);
    sch.components[2].value = "330".into();
    sch.add(Kind::Led, (28, 0), 0);
    sch.add(Kind::Ground, (0, 8), 0);
    for (a, b) in [
        ((0, -2), (0, -6)), ((0, -6), (10, -6)), ((10, -6), (10, 0)), // V1+ -> IN
        ((18, 0), (20, 0)), ((24, 0), (26, 0)),                       // OUT -> R1 -> LED
        ((30, 0), (30, 8)), ((14, 4), (14, 8)), ((0, 2), (0, 8)), ((0, 8), (30, 8)),
    ] {
        sch.wires.push(Wire { a, b });
    }
    std::fs::write(root.join("regulator_led.schjson"), serde_json::to_string_pretty(&sch).unwrap()).unwrap();
}

/// Scratch board with one of every bundled footprint (not part of the repository):
/// `cargo test -p app write_footprint_sheet -- --ignored`
#[test]
#[ignore]
fn write_footprint_sheet() {
    let mut b = pcb_core::Board::new("sheet", 90.0, 50.0);
    let mut seen = std::collections::HashSet::new();
    let (mut x, mut y, mut n) = (8.0, 42.0, 0);
    for (_, text) in part_lib::EXAMPLES {
        let def: part_lib::PartDef = serde_json::from_str(text).unwrap();
        if !seen.insert(def.footprint_name()) {
            continue;
        }
        b.add_footprint(def.footprint().unwrap());
        n += 1;
        let refdes = format!("U{n}");
        b.set_part(pcb_core::Part { refdes, value: def.name.clone(), footprint: def.footprint_name(), pos: Point::new(x, y), rot: 0, side: Side::Top }).unwrap();
        x += if def.footprint_name().contains("TQFP") { 16.0 } else { 11.0 };
        if x > 80.0 {
            x = 8.0;
            y -= 14.0;
        }
    }
    let dir = std::env::temp_dir().join("claude");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("sheet.pcbjson"), serde_json::to_string_pretty(&b).unwrap()).unwrap();
}

/// Generates docs/examples/three_phase_wye.schjson (3-phase source -> three 100 ohm loads, wye) and
/// docs/examples/three_phase_rectifier.schjson (3-phase source -> 6-diode bridge -> 470 uF + 100 ohm):
/// `cargo test -p app write_three_phase_examples -- --ignored`
#[test]
#[ignore]
fn write_three_phase_examples() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let src = library_part("AC3P_10V_50Hz");

    // wye load: the source's N is ground, each phase drives its own resistor
    let mut s = sch_core::Schematic::default();
    let u = s.add_custom(src.symbol(), &src.default_value(), &src.footprint_name(), (0, 0), 0);
    for (pin, net) in [("1", "A"), ("2", "B"), ("3", "C"), ("4", "GND")] {
        label_pin(&mut s, u, pin, net);
    }
    for (x, net) in [(30, "A"), (42, "B"), (54, "C")] {
        let r = s.add(Kind::Resistor, (x, 0), 1);
        s.components[r].value = "100".into();
        label_pin(&mut s, r, "1", net);
        label_pin(&mut s, r, "2", "GND");
    }
    std::fs::write(root.join("three_phase_wye.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();

    // 6-pulse bridge: upper diodes feed VDC, lower diodes return from GND (N is tied to GND only through 1 Mohm,
    // so the source has a voltage reference without carrying load current)
    let mut s = sch_core::Schematic::default();
    let rn = s.add(Kind::Resistor, (20, 16), 1);
    s.components[rn].value = "1Meg".into();
    label_pin(&mut s, rn, "1", "N");
    label_pin(&mut s, rn, "2", "GND");
    for (k, phase) in ["A", "B", "C"].iter().enumerate() {
        let x = 40 + 12 * k as i32;
        let up = s.add(Kind::Diode, (x, -8), 0);
        s.components[up].value = "1N4007".into();
        // pins of a diode: 0 = anode, 1 = cathode
        label_pin(&mut s, up, "A", phase);
        label_pin(&mut s, up, "K", "VDC");
        let low = s.add(Kind::Diode, (x, 8), 0);
        s.components[low].value = "1N4007".into();
        label_pin(&mut s, low, "A", "GND");
        label_pin(&mut s, low, "K", phase);
    }
    let c = s.add(Kind::Capacitor, (84, 0), 1);
    s.components[c].value = "470u".into();
    let r = s.add(Kind::Resistor, (96, 0), 1);
    s.components[r].value = "100".into();
    for comp in [c, r] {
        label_pin(&mut s, comp, "1", "VDC");
        label_pin(&mut s, comp, "2", "GND");
    }
    // the source goes in last so that VDC comes first in the signal list (the transient view shows the first four)
    let u = s.add_custom(src.symbol(), &src.default_value(), &src.footprint_name(), (0, 0), 0);
    for (pin, net) in [("1", "A"), ("2", "B"), ("3", "C"), ("4", "N")] {
        label_pin(&mut s, u, pin, net);
    }
    std::fs::write(root.join("three_phase_rectifier.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}
