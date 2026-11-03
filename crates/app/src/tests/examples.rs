use super::*;

/// Puts a short wire on pin `pin` of component `idx`, pointing away from the part, and names its end `text` (the sample generators
/// connect everything by net labels).
fn label(s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str) {
    let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap_or_else(|| panic!("pin {pin}"));
    let c = s.components[idx].pos;
    let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
    let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
    s.wires.push(Wire { a: pt, b: end });
    s.labels.push(sch_core::Label { pos: end, text: text.into() });
}

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
    b.vias.push(pcb_core::Via { pos: Point::new(c2.x, c2.y - 1.8), diameter: 0.8, drill: 0.4, net: String::new() });
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
    // what the board has to do for this circuit (the "設計の要件" table): the switching node small and away from the feedback line, the loop close
    s.requirements.net_rules.push(design_spec::NetRule { name: "スイッチングノード".into(), nets: vec!["SW".into()], role: "switching".into(), width: 0.5, note: "面積を小さく。帰還やアナログの線から離す".into(), ..Default::default() });
    s.requirements.net_rules.push(design_spec::NetRule { name: "電源の電流".into(), nets: vec!["VIN".into(), "VOUT".into()], role: "power".into(), width: 0.5, current: 0.5, note: "電流が流れる経路。太く短く".into(), ..Default::default() });
    s.requirements.net_rules.push(design_spec::NetRule { name: "帰還".into(), nets: vec!["FB".into()], role: "sense".into(), width: 0.2, note: "細くてよいが、短く。スイッチングノードに近づけない".into(), ..Default::default() });
    s.requirements.groups.push(design_spec::PlaceGroup { name: "降圧のループ".into(), parts: vec!["U1".into(), "L1".into(), "D1".into(), "C1".into()], max_dist: 25.0, note: "スイッチの電流が流れるループを小さくする".into() });
    s.requirements.apart.push(design_spec::KeepApart { name: "帰還をスイッチングノードから離す".into(), a: vec!["R1".into(), "R2".into()], b: vec!["L1".into(), "D1".into()], min_dist: 6.0, note: "スイッチングの雑音を、帰還の分圧に入れない".into() });
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

/// Generates docs/examples/rail_impedance.schjson: a 5 V rail seen through a cable (L1 100 nH, R1 50 mohm) with an electrolytic,
/// a 10 uF and a 100 nF ceramic on it and a 1 kohm load. The impedance analysis of net RAIL shows the dips of the three
/// capacitors and the peaks between them: `cargo test -p app write_rail_impedance_example -- --ignored`
#[test]
#[ignore]
fn write_rail_impedance_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let find = |n: &str| part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == n).unwrap();
    let (elec, cer, small) = (find("CE_100u_16V"), find("C_10u_0805"), find("C_100n_0603"));
    let mut s = sch_core::Schematic::default();
    let v = s.add(Kind::VSource, (0, 0), 0);
    s.components[v].value = "5".into();
    label(&mut s, v, "+", "VIN");
    label(&mut s, v, "-", "GND");
    let l = s.add(Kind::Inductor, (20, 0), 1);
    s.components[l].value = "100n".into();
    label(&mut s, l, "1", "VIN");
    label(&mut s, l, "2", "CABLE");
    let r = s.add(Kind::Resistor, (36, 0), 1);
    s.components[r].value = "50m".into();
    label(&mut s, r, "1", "CABLE");
    label(&mut s, r, "2", "RAIL");
    for (def, x) in [(&elec, 52), (&cer, 66), (&small, 80)] {
        let c = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (x, 0), 1);
        label(&mut s, c, "1", "RAIL");
        label(&mut s, c, "2", "GND");
    }
    let load = s.add(Kind::Resistor, (94, 0), 1);
    s.components[load].value = "1k".into();
    label(&mut s, load, "1", "RAIL");
    label(&mut s, load, "2", "GND");
    std::fs::write(root.join("rail_impedance.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/amp_clipping.schjson: the op-amp amplifier (gain 11) with a 0.4 V, 1 kHz sine, which is more than the
/// 5 V supply allows at the output, so the wave is clipped: `cargo test -p app write_amp_clipping_example -- --ignored`
#[test]
#[ignore]
fn write_amp_clipping_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut s: sch_core::Schematic = serde_json::from_str(&std::fs::read_to_string(root.join("opamp_amp.schjson")).unwrap()).unwrap();
    s.components.iter_mut().find(|c| c.refdes == "V1").unwrap().value = "SIN(0 0.4 1k)".into();
    std::fs::write(root.join("amp_clipping.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/loop_cap_load.schjson: `series_regulator` with C5 = 10 uF on VOUT and R2 = 100 ohm as the load, a regulator whose
/// feedback loop has too little phase margin: `cargo test -p app write_loop_cap_load_example -- --ignored`
#[test]
#[ignore]
fn write_loop_cap_load_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut s: sch_core::Schematic = serde_json::from_str(&std::fs::read_to_string(root.join("series_regulator.schjson")).unwrap()).unwrap();
    s.components.iter_mut().find(|c| c.refdes == "R2").unwrap().value = "100".into();
    let c = s.add(Kind::Capacitor, (60, 40), 1);
    s.components[c].value = "10u".into();
    let (p1, p2) = (s.components[c].pins()[0].1, s.components[c].pins()[1].1);
    s.labels.push(sch_core::Label { pos: p1, text: "VOUT".into() });
    s.labels.push(sch_core::Label { pos: p2, text: "GND".into() });
    std::fs::write(root.join("loop_cap_load.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/rlc_poles.schjson: a 10 ohm / 1 mH / 1 uF low-pass (AC source 1 V) whose pair of complex poles is at
/// 5.03 kHz with Q = 3.16, for the poles-and-zeros section: `cargo test -p app write_rlc_poles_example -- --ignored`
#[test]
#[ignore]
fn write_rlc_poles_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut s = sch_core::Schematic::default();
    let v = s.add(Kind::VSource, (0, 0), 0);
    s.components[v].value = "0 AC 1".into();
    label(&mut s, v, "+", "IN");
    label(&mut s, v, "-", "GND");
    let r = s.add(Kind::Resistor, (20, 0), 1);
    s.components[r].value = "10".into();
    label(&mut s, r, "1", "IN");
    label(&mut s, r, "2", "N1");
    let l = s.add(Kind::Inductor, (36, 0), 1);
    s.components[l].value = "1m".into();
    label(&mut s, l, "1", "N1");
    label(&mut s, l, "2", "OUT");
    let c = s.add(Kind::Capacitor, (52, 0), 1);
    s.components[c].value = "1u".into();
    label(&mut s, c, "1", "OUT");
    label(&mut s, c, "2", "GND");
    std::fs::write(root.join("rlc_poles.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/capacitor_basics.schjson for the capacitor lesson: a 5 V triangle wave (1 ms up, 1 ms down) through a
/// 10 ohm sense resistor into C1 = 1 uF (the current is C*dV/dt = +-5 mA), and a 5 V step through R2 = 1 kohm into C2 = 1 uF
/// (tau = 1 ms): `cargo test -p app write_capacitor_basics_example -- --ignored`
#[test]
#[ignore]
fn write_capacitor_basics_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut s = sch_core::Schematic::default();
    // (source value, series resistor, capacitor, x, nets: input / capacitor node)
    for (src, r, c, x, input, node) in [("PULSE(0 5 0 1m 1m 0 2m)", "10", "1u", 0, "RAMP", "CA"), ("PULSE(0 5 0 1u 1u 10m 20m)", "1k", "1u", 48, "STEP", "CB")] {
        let v = s.add(Kind::VSource, (x, 0), 0);
        s.components[v].value = src.into();
        label(&mut s, v, "+", input);
        label(&mut s, v, "-", "GND");
        let ri = s.add(Kind::Resistor, (x + 16, 0), 1);
        s.components[ri].value = r.into();
        label(&mut s, ri, "1", input);
        label(&mut s, ri, "2", node);
        let ci = s.add(Kind::Capacitor, (x + 32, 0), 1);
        s.components[ci].value = c.into();
        label(&mut s, ci, "1", node);
        label(&mut s, ci, "2", "GND");
    }
    std::fs::write(root.join("capacitor_basics.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/inductor_basics.schjson for the inductor lesson: a 5 V step through R1 = 1 ohm into L1 = 100 mH
/// (the current climbs 50 mA per ms, V = L*di/dt) and a 5 V step through R2 = 100 ohm into L2 = 100 mH (tau = L/R = 1 ms):
/// `cargo test -p app write_inductor_basics_example -- --ignored`
#[test]
#[ignore]
fn write_inductor_basics_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut s = sch_core::Schematic::default();
    for (r, x, input, node) in [("1", 0, "STEPA", "LB"), ("100", 48, "STEPB", "LA")] {
        let v = s.add(Kind::VSource, (x, 0), 0);
        s.components[v].value = "PULSE(0 5 0 1u 1u 10m 20m)".into();
        label(&mut s, v, "+", input);
        label(&mut s, v, "-", "GND");
        let ri = s.add(Kind::Resistor, (x + 16, 0), 1);
        s.components[ri].value = r.into();
        label(&mut s, ri, "1", input);
        label(&mut s, ri, "2", node);
        let li = s.add(Kind::Inductor, (x + 32, 0), 1);
        s.components[li].value = "100m".into();
        label(&mut s, li, "1", node);
        label(&mut s, li, "2", "GND");
    }
    std::fs::write(root.join("inductor_basics.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/resistor_basics.schjson for the resistor lesson: 5 V through R1 = 100 ohm and R2 = 220 ohm in series
/// (node A), and 5 V across R3 = 100 ohm and R4 = 220 ohm in parallel (node P):
/// `cargo test -p app write_resistor_basics_example -- --ignored`
#[test]
#[ignore]
fn write_resistor_basics_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut s = sch_core::Schematic::default();
    let mut put = |s: &mut sch_core::Schematic, x: i32, value: &str, a: &str, b: &str| {
        let r = s.add(Kind::Resistor, (x, 0), 1);
        s.components[r].value = value.into();
        label(s, r, "1", a);
        label(s, r, "2", b);
    };
    let v = s.add(Kind::VSource, (0, 0), 0);
    s.components[v].value = "5".into();
    label(&mut s, v, "+", "IN");
    label(&mut s, v, "-", "GND");
    put(&mut s, 16, "100", "IN", "A");
    put(&mut s, 32, "220", "A", "GND");
    let v = s.add(Kind::VSource, (48, 0), 0);
    s.components[v].value = "5".into();
    label(&mut s, v, "+", "P");
    label(&mut s, v, "-", "GND");
    put(&mut s, 64, "100", "P", "GND");
    put(&mut s, 80, "220", "P", "GND");
    // big resistor bodies, so that the defaults stay inside their power rating
    for c in s.components.iter_mut().filter(|c| c.kind == Kind::Resistor) {
        c.footprint = "R_2512".into();
    }
    std::fs::write(root.join("resistor_basics.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/transistor_basics.schjson for the transistor lesson: Q1 (NPN, hFE 200) switches a 1 kohm load R1 on
/// VCC = 5 V, the base is fed from V2 through R2 = 100 kohm: `cargo test -p app write_transistor_basics_example -- --ignored`
#[test]
#[ignore]
fn write_transistor_basics_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut s = sch_core::Schematic::default();
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "5".into();
    label(&mut s, v1, "+", "VCC");
    label(&mut s, v1, "-", "GND");
    let rc = s.add(Kind::Resistor, (16, 0), 1);
    s.components[rc].value = "1k".into();
    label(&mut s, rc, "1", "VCC");
    label(&mut s, rc, "2", "COL");
    let q = s.add(Kind::Npn, (36, 0), 0);
    label(&mut s, q, "C", "COL");
    label(&mut s, q, "B", "BASE");
    label(&mut s, q, "E", "GND");
    let v2 = s.add(Kind::VSource, (52, 0), 0);
    s.components[v2].value = "1.7".into();
    label(&mut s, v2, "+", "VB");
    label(&mut s, v2, "-", "GND");
    let rb = s.add(Kind::Resistor, (68, 0), 1);
    s.components[rb].value = "100k".into();
    label(&mut s, rb, "1", "VB");
    label(&mut s, rb, "2", "BASE");
    std::fs::write(root.join("transistor_basics.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/mosfet_basics.schjson for the MOSFET lesson: Q1 (2N7000) switches a 100 ohm load R1 on VDD = 5 V,
/// the gate is driven directly by V2 (5 V): `cargo test -p app write_mosfet_basics_example -- --ignored`
#[test]
#[ignore]
fn write_mosfet_basics_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "2N7000").unwrap();
    let mut s = sch_core::Schematic::default();
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "5".into();
    label(&mut s, v1, "+", "VDD");
    label(&mut s, v1, "-", "GND");
    let rd = s.add(Kind::Resistor, (16, 0), 1);
    s.components[rd].value = "100".into();
    label(&mut s, rd, "1", "VDD");
    label(&mut s, rd, "2", "DRAIN");
    let q = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (36, 0), 0);
    label(&mut s, q, "3", "DRAIN"); // pins are numbered: 3 = D, 2 = G, 1 = S
    label(&mut s, q, "2", "GATE");
    label(&mut s, q, "1", "GND");
    let v2 = s.add(Kind::VSource, (52, 0), 0);
    s.components[v2].value = "5".into();
    label(&mut s, v2, "+", "GATE");
    label(&mut s, v2, "-", "GND");
    // big resistor bodies, so that the defaults stay inside their power rating
    for c in s.components.iter_mut().filter(|c| c.kind == Kind::Resistor) {
        c.footprint = "R_2512".into();
    }
    std::fs::write(root.join("mosfet_basics.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/opamp_basics.schjson for the op-amp lesson: three LM358 non-inverting amplifiers on +-5 V with the
/// gains 2, 11 and 101 (Rf = 1k / 10k / 100k, Rg = 1k) fed by one source (SIN 0.5 V, 1 kHz, AC 1):
/// `cargo test -p app write_opamp_basics_example -- --ignored`
#[test]
#[ignore]
fn write_opamp_basics_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "LM358").unwrap();
    let mut s = sch_core::Schematic::default();
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "SIN(0 0.5 1k) AC 1".into();
    label(&mut s, v1, "+", "IN");
    label(&mut s, v1, "-", "GND");
    let v2 = s.add(Kind::VSource, (0, 24), 0);
    s.components[v2].value = "5".into();
    label(&mut s, v2, "+", "VCC");
    label(&mut s, v2, "-", "GND");
    let v3 = s.add(Kind::VSource, (0, 48), 0);
    s.components[v3].value = "5".into();
    label(&mut s, v3, "+", "GND");
    label(&mut s, v3, "-", "VEE");
    for (k, rf) in ["1k", "10k", "100k"].into_iter().enumerate() {
        let (n, y) = (k + 1, 28 * k as i32);
        let u = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (30, y), 0);
        label(&mut s, u, "3", "IN");
        label(&mut s, u, "2", &format!("FB{n}"));
        label(&mut s, u, "1", &format!("OUT{n}"));
        label(&mut s, u, "8", "VCC");
        label(&mut s, u, "4", "VEE");
        let r = s.add(Kind::Resistor, (50, y), 1);
        s.components[r].value = rf.into();
        label(&mut s, r, "1", &format!("OUT{n}"));
        label(&mut s, r, "2", &format!("FB{n}"));
        let r = s.add(Kind::Resistor, (66, y), 1);
        s.components[r].value = "1k".into();
        label(&mut s, r, "1", &format!("FB{n}"));
        label(&mut s, r, "2", "GND");
    }
    std::fs::write(root.join("opamp_basics.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/diode_basics.schjson for the diode lesson: V1 (5 V DC + 1 V AC) feeds a diode in the forward direction
/// through R1 = 1 kohm (node A) and a diode in the reverse direction through R2 = 1 kohm (node B):
/// `cargo test -p app write_diode_basics_example -- --ignored`
#[test]
#[ignore]
fn write_diode_basics_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut s = sch_core::Schematic::default();
    let v = s.add(Kind::VSource, (0, 0), 0);
    s.components[v].value = "5 AC 1".into();
    label(&mut s, v, "+", "IN");
    label(&mut s, v, "-", "GND");
    // forward row, then reverse row
    for (y, node, forward) in [(0, "A", true), (20, "B", false)] {
        let r = s.add(Kind::Resistor, (20, y), 1);
        s.components[r].value = "1k".into();
        label(&mut s, r, "1", "IN");
        label(&mut s, r, "2", node);
        let d = s.add(Kind::Diode, (40, y), 0);
        let (a, k) = if forward { (node, "GND") } else { ("GND", node) };
        label(&mut s, d, "A", a);
        label(&mut s, d, "K", k);
    }
    std::fs::write(root.join("diode_basics.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/led_basics.schjson for the LED lesson: three LEDs with 330 ohm resistors on 5 V, 3.3 V and 2 V:
/// `cargo test -p app write_led_basics_example -- --ignored`
#[test]
#[ignore]
fn write_led_basics_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut s = sch_core::Schematic::default();
    for (k, volts) in ["5", "3.3", "2"].into_iter().enumerate() {
        let (n, y) = (k + 1, 20 * k as i32);
        let v = s.add(Kind::VSource, (0, y), 0);
        s.components[v].value = volts.into();
        label(&mut s, v, "+", &format!("S{n}"));
        label(&mut s, v, "-", "GND");
        let r = s.add(Kind::Resistor, (20, y), 1);
        s.components[r].value = "330".into();
        label(&mut s, r, "1", &format!("S{n}"));
        label(&mut s, r, "2", &format!("A{n}"));
        let d = s.add(Kind::Led, (40, y), 0);
        label(&mut s, d, "A", &format!("A{n}"));
        label(&mut s, d, "K", "GND");
    }
    std::fs::write(root.join("led_basics.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/zener_basics.schjson for the Zener lesson: V1 (12 V) -> R1 = 330 ohm -> 1N4733A (5.1 V) to GND (node Z):
/// `cargo test -p app write_zener_basics_example -- --ignored`
#[test]
#[ignore]
fn write_zener_basics_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "1N4733A").unwrap();
    let mut s = sch_core::Schematic::default();
    let v = s.add(Kind::VSource, (0, 0), 0);
    s.components[v].value = "12".into();
    label(&mut s, v, "+", "IN");
    label(&mut s, v, "-", "GND");
    let r = s.add(Kind::Resistor, (20, 0), 1);
    s.components[r].value = "330".into();
    label(&mut s, r, "1", "IN");
    label(&mut s, r, "2", "Z");
    let d = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (40, 0), 0);
    label(&mut s, d, "K", "Z");
    label(&mut s, d, "A", "GND");
    // big resistor bodies, so that the defaults stay inside their power rating
    for c in s.components.iter_mut().filter(|c| c.kind == Kind::Resistor) {
        c.footprint = "R_2512".into();
    }
    std::fs::write(root.join("zener_basics.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/relay_basics.schjson for the relay lesson: a 5 V relay coil (R 70 ohm + L 100 mH) driven by a 5 V
/// step (R1, L1), and the same coil behind a 47 ohm series resistor R2 (R3, L2): `cargo test -p app write_relay_basics_example -- --ignored`
#[test]
#[ignore]
fn write_relay_basics_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut s = sch_core::Schematic::default();
    let part = |s: &mut sch_core::Schematic, kind: Kind, x: i32, y: i32, value: &str, a: &str, b: &str| {
        let i = s.add(kind, (x, y), 1);
        s.components[i].value = value.into();
        label(s, i, "1", a);
        label(s, i, "2", b);
    };
    let v = s.add(Kind::VSource, (0, 0), 0);
    s.components[v].value = "PULSE(0 5 0.5m 1u 1u 20m 40m)".into();
    label(&mut s, v, "+", "DRV1");
    label(&mut s, v, "-", "GND");
    part(&mut s, Kind::Resistor, 20, 0, "70", "DRV1", "C1");
    part(&mut s, Kind::Inductor, 36, 0, "100m", "C1", "GND");
    let v = s.add(Kind::VSource, (0, 24), 0);
    s.components[v].value = "PULSE(0 5 0.5m 1u 1u 20m 40m)".into();
    label(&mut s, v, "+", "DRV2");
    label(&mut s, v, "-", "GND");
    part(&mut s, Kind::Resistor, 20, 24, "47", "DRV2", "S2");
    part(&mut s, Kind::Resistor, 36, 24, "70", "S2", "C2");
    part(&mut s, Kind::Inductor, 52, 24, "100m", "C2", "GND");
    std::fs::write(root.join("relay_basics.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/crystal_q.schjson for the crystal "face" lesson: the same 1 V source feeds a 16 MHz crystal (Y1) into
/// 50 ohm (R1), and an ordinary LC series resonator (L1 1 uH, C1 98.9 pF, loss R2 1 ohm) into a 1 ohm load (R3), for comparing Q:
/// `cargo test -p app write_crystal_q_example -- --ignored`
#[test]
#[ignore]
fn write_crystal_q_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "XTAL_16MHz_3225").unwrap();
    let mut s = sch_core::Schematic::default();
    let part = |s: &mut sch_core::Schematic, kind: Kind, x: i32, y: i32, value: &str, a: &str, b: &str| {
        let i = s.add(kind, (x, y), 1);
        s.components[i].value = value.into();
        label(s, i, "1", a);
        label(s, i, "2", b);
    };
    let v = s.add(Kind::VSource, (0, 0), 0);
    s.components[v].value = "SIN(0 1 16Meg) AC 1".into();
    label(&mut s, v, "+", "IN");
    label(&mut s, v, "-", "GND");
    let y = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (20, 0), 0);
    label(&mut s, y, "1", "IN");
    label(&mut s, y, "3", "OUTX");
    label(&mut s, y, "2", "GND");
    label(&mut s, y, "4", "GND");
    part(&mut s, Kind::Resistor, 40, 0, "50", "OUTX", "GND");
    part(&mut s, Kind::Inductor, 20, 28, "1u", "IN", "N1");
    part(&mut s, Kind::Capacitor, 36, 28, "98.9p", "N1", "N2");
    part(&mut s, Kind::Resistor, 52, 28, "1", "N2", "OUTL");
    part(&mut s, Kind::Resistor, 68, 28, "1", "OUTL", "GND");
    std::fs::write(root.join("crystal_q.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/transformer_basics.schjson for the transformer lesson: V1 (100 V rms, 50 Hz) -> R2 = 1 ohm (primary
/// current sense) -> the 100 V : 12 V, 5 VA transformer T1, load R1 on the secondary (1 Mohm = no load at first):
/// `cargo test -p app write_transformer_basics_example -- --ignored`
#[test]
#[ignore]
fn write_transformer_basics_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "TR100_12V_5VA").unwrap();
    let mut s = sch_core::Schematic::default();
    // the load is made first so that it is R1 and the sense resistor R2
    let load = s.add(Kind::Resistor, (70, 0), 1);
    s.components[load].value = "1Meg".into();
    label(&mut s, load, "1", "OUT");
    label(&mut s, load, "2", "GND");
    let sense = s.add(Kind::Resistor, (20, 0), 1);
    s.components[sense].value = "1".into();
    label(&mut s, sense, "1", "IN");
    label(&mut s, sense, "2", "PRI");
    let v = s.add(Kind::VSource, (0, 0), 0);
    s.components[v].value = "SIN(0 141.4 50) AC 1".into();
    label(&mut s, v, "+", "IN");
    label(&mut s, v, "-", "GND");
    let t = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (45, 0), 0);
    label(&mut s, t, "1", "PRI");
    label(&mut s, t, "2", "GND");
    label(&mut s, t, "3", "OUT");
    label(&mut s, t, "4", "GND");
    std::fs::write(root.join("transformer_basics.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/thermistor_basics.schjson for the thermistor lesson: 5 V over three dividers, each a series resistor
/// (R1 10k -> A, R2 1k -> B, R3 100k -> C) above a 10k B3950 NTC thermistor to GND:
/// `cargo test -p app write_thermistor_basics_example -- --ignored`
#[test]
#[ignore]
fn write_thermistor_basics_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "NTC_10k_B3950").unwrap();
    let mut s = sch_core::Schematic::default();
    let v = s.add(Kind::VSource, (0, 0), 0);
    s.components[v].value = "5".into();
    label(&mut s, v, "+", "VCC");
    label(&mut s, v, "-", "GND");
    for (y, r, node) in [(0, "10k", "A"), (20, "1k", "B"), (40, "100k", "C")] {
        let ri = s.add(Kind::Resistor, (20, y), 1);
        s.components[ri].value = r.into();
        label(&mut s, ri, "1", "VCC");
        label(&mut s, ri, "2", node);
        let t = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (40, y), 1);
        label(&mut s, t, "1", node);
        label(&mut s, t, "2", "GND");
    }
    std::fs::write(root.join("thermistor_basics.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/phototransistor_basics.schjson for the phototransistor lesson: V2 is the light (1 V = 1 mW/cm2, 0.02 at first)
/// shared by two BPW34 photodiodes: D1 alone into 100 kohm (R1, node PDOUT), and D2 between the collector and the base of an NPN (Q1,
/// hFE 200) with a 1 kohm collector load (R2, node COL) - that is a phototransistor:
/// `cargo test -p app write_phototransistor_basics_example -- --ignored`
#[test]
#[ignore]
fn write_phototransistor_basics_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "BPW34").unwrap();
    let mut s = sch_core::Schematic::default();
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "5".into();
    label(&mut s, v1, "+", "VCC");
    label(&mut s, v1, "-", "GND");
    let v2 = s.add(Kind::VSource, (0, 24), 0);
    s.components[v2].value = "0.02".into();
    label(&mut s, v2, "+", "LIGHT");
    label(&mut s, v2, "-", "GND");
    // photodiode alone
    let d1 = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (30, 0), 0);
    label(&mut s, d1, "K", "VCC");
    label(&mut s, d1, "A", "PDOUT");
    label(&mut s, d1, "L", "LIGHT");
    let r1 = s.add(Kind::Resistor, (50, 0), 1);
    s.components[r1].value = "100k".into();
    label(&mut s, r1, "1", "PDOUT");
    label(&mut s, r1, "2", "GND");
    // the phototransistor: the photodiode feeds the base
    let d2 = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (30, 28), 0);
    label(&mut s, d2, "K", "COL");
    label(&mut s, d2, "A", "BASE");
    label(&mut s, d2, "L", "LIGHT");
    let q1 = s.add(Kind::Npn, (60, 28), 0);
    label(&mut s, q1, "C", "COL");
    label(&mut s, q1, "B", "BASE");
    label(&mut s, q1, "E", "GND");
    let r2 = s.add(Kind::Resistor, (80, 28), 1);
    s.components[r2].value = "1k".into();
    label(&mut s, r2, "1", "VCC");
    label(&mut s, r2, "2", "COL");
    std::fs::write(root.join("phototransistor_basics.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/regulator_basics.schjson for the three-terminal regulator lesson: V1 (12 V) in series with V2 (0 V, AC 1 =
/// the ripple) feeds an LM7805 (U1) that drives a 50 ohm load R1: `cargo test -p app write_regulator_basics_example -- --ignored`
#[test]
#[ignore]
fn write_regulator_basics_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "LM7805").unwrap();
    let mut s = sch_core::Schematic::default();
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "12".into();
    label(&mut s, v1, "+", "SUPPLY");
    label(&mut s, v1, "-", "GND");
    let v2 = s.add(Kind::VSource, (0, 24), 0);
    s.components[v2].value = "0 AC 1".into();
    label(&mut s, v2, "+", "VIN");
    label(&mut s, v2, "-", "SUPPLY");
    let u = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (30, 0), 0);
    label(&mut s, u, "1", "VIN");
    label(&mut s, u, "2", "GND");
    label(&mut s, u, "3", "VOUT");
    let r = s.add(Kind::Resistor, (50, 0), 1);
    s.components[r].value = "50".into();
    label(&mut s, r, "1", "VOUT");
    label(&mut s, r, "2", "GND");
    // a big resistor body, so that the default stays inside its power rating
    for c in s.components.iter_mut().filter(|c| c.kind == Kind::Resistor) {
        c.footprint = "R_2512".into();
    }
    std::fs::write(root.join("regulator_basics.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/photocoupler_basics.schjson for the photocoupler lesson: V1 (5 V) -> R1 = 330 ohm -> the LED of a PC817 (U1)
/// on the input side; on the isolated output side V2 (5 V, its minus is GND2) -> R2 = 220 ohm -> the collector, the emitter on GND2
/// (R3 = 1 Mohm ties GND2 to GND only so that the simulation has a reference): `cargo test -p app write_photocoupler_basics_example -- --ignored`
#[test]
#[ignore]
fn write_photocoupler_basics_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "PC817").unwrap();
    let mut s = sch_core::Schematic::default();
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "5".into();
    label(&mut s, v1, "+", "VIN");
    label(&mut s, v1, "-", "GND");
    let r1 = s.add(Kind::Resistor, (20, 0), 1);
    s.components[r1].value = "330".into();
    label(&mut s, r1, "1", "VIN");
    label(&mut s, r1, "2", "LEDA");
    let v2 = s.add(Kind::VSource, (0, 28), 0);
    s.components[v2].value = "5".into();
    label(&mut s, v2, "+", "VCC2");
    label(&mut s, v2, "-", "GND2");
    let r2 = s.add(Kind::Resistor, (20, 28), 1);
    s.components[r2].value = "220".into();
    label(&mut s, r2, "1", "VCC2");
    label(&mut s, r2, "2", "OUT");
    let r3 = s.add(Kind::Resistor, (40, 28), 1);
    s.components[r3].value = "1Meg".into();
    label(&mut s, r3, "1", "GND2");
    label(&mut s, r3, "2", "GND");
    let u = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (60, 10), 0);
    label(&mut s, u, "1", "LEDA");
    label(&mut s, u, "2", "GND");
    label(&mut s, u, "3", "GND2");
    label(&mut s, u, "4", "OUT");
    std::fs::write(root.join("photocoupler_basics.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/fuse_basics.schjson for the fuse lesson: a 12 V supply (it rises at 10 ms, so that the DC point has no
/// current) feeds three FUSE_1A fuses (F1..F3) with the loads R1 = 12 ohm (1 A), R2 = 4 ohm (3 A) and R3 = 0.5 ohm (24 A):
/// `cargo test -p app write_fuse_basics_example -- --ignored`
#[test]
#[ignore]
fn write_fuse_basics_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "FUSE_1A").unwrap();
    let mut s = sch_core::Schematic::default();
    let v = s.add(Kind::VSource, (0, 0), 0);
    s.components[v].value = "PULSE(0 12 10m 1u 1u 100 200)".into();
    label(&mut s, v, "+", "SUP");
    label(&mut s, v, "-", "GND");
    for (k, ohm) in ["12", "4", "0.5"].into_iter().enumerate() {
        let (n, y) = (k + 1, 20 * k as i32);
        let f = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (30, y), 0);
        label(&mut s, f, "1", "SUP");
        label(&mut s, f, "2", &format!("X{n}"));
        let r = s.add(Kind::Resistor, (50, y), 1);
        s.components[r].value = ohm.into();
        s.components[r].footprint = "R_2512".into();
        label(&mut s, r, "1", &format!("X{n}"));
        label(&mut s, r, "2", "GND");
    }
    std::fs::write(root.join("fuse_basics.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/logic_basics.schjson for the logic IC lesson: a slow triangle wave (0 - 5 V in 5 ms) with 0.4 V of 50 kHz
/// noise on top (V1 + V2) goes to a 74HC04 inverter (U1, gate 1) and a 74HC14 Schmitt inverter (U2, gate 1); gate 2 of U1 has its
/// input on GND and drives an LED through R1 = 330 ohm: `cargo test -p app write_logic_basics_example -- --ignored`
#[test]
#[ignore]
fn write_logic_basics_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let find = |n: &str| part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == n).unwrap();
    let (hc04, hc14) = (find("74HC04"), find("74HC14"));
    let mut s = sch_core::Schematic::default();
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "PULSE(0 5 0 5m 5m 0 10m)".into();
    label(&mut s, v1, "+", "RAMP");
    label(&mut s, v1, "-", "GND");
    let v2 = s.add(Kind::VSource, (0, 24), 0);
    s.components[v2].value = "SIN(0 0.4 50k)".into();
    label(&mut s, v2, "+", "IN");
    label(&mut s, v2, "-", "RAMP");
    let v3 = s.add(Kind::VSource, (0, 48), 0);
    s.components[v3].value = "5".into();
    label(&mut s, v3, "+", "VCC");
    label(&mut s, v3, "-", "GND");
    // U1: 74HC04, gate 1 = the input under test, gate 2 = the LED driver; pins 1A 1Y 2A 2Y 3A 3Y GND 4Y 4A 5Y 5A 6Y 6A VCC
    let u1 = s.add_custom(hc04.symbol(), &hc04.default_value(), &hc04.footprint_name(), (40, 0), 0);
    for (pin, net) in [("1", "IN"), ("2", "OUT1"), ("3", "GND"), ("4", "DRV"), ("5", "GND"), ("7", "GND"), ("9", "GND"), ("11", "GND"), ("13", "GND"), ("14", "VCC")] {
        label(&mut s, u1, pin, net);
    }
    let u2 = s.add_custom(hc14.symbol(), &hc14.default_value(), &hc14.footprint_name(), (40, 50), 0);
    for (pin, net) in [("1", "IN"), ("2", "OUT2"), ("3", "GND"), ("5", "GND"), ("7", "GND"), ("9", "GND"), ("11", "GND"), ("13", "GND"), ("14", "VCC")] {
        label(&mut s, u2, pin, net);
    }
    let r1 = s.add(Kind::Resistor, (75, 0), 1);
    s.components[r1].value = "330".into();
    label(&mut s, r1, "1", "DRV");
    label(&mut s, r1, "2", "LEDA");
    let d1 = s.add(Kind::Led, (90, 0), 0);
    label(&mut s, d1, "A", "LEDA");
    label(&mut s, d1, "K", "GND");
    std::fs::write(root.join("logic_basics.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/speaker_basics.schjson for the speaker / buzzer lesson: each is driven by its own 1 V AC source, so that
/// I(V1) and I(V2) tell the impedance (1 V / current). V1 -> an 8 ohm speaker: voice coil R1 8 ohm + L1 0.1 mH in series with the
/// mechanical resonance (R2 24 ohm || L2 2.12 mH || C1 33 uF, about 600 Hz). V2 -> a piezo buzzer: C2 15 nF in parallel with the series
/// resonance R3 1 kohm + L3 0.53 H + C3 3 nF (about 4 kHz): `cargo test -p app write_speaker_basics_example -- --ignored`
#[test]
#[ignore]
fn write_speaker_basics_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut s = sch_core::Schematic::default();
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "0 AC 1".into();
    label(&mut s, v1, "+", "SPIN");
    label(&mut s, v1, "-", "GND");
    let v2 = s.add(Kind::VSource, (0, 30), 0);
    s.components[v2].value = "0 AC 1".into();
    label(&mut s, v2, "+", "PZIN");
    label(&mut s, v2, "-", "GND");
    let part = |s: &mut sch_core::Schematic, kind: Kind, x: i32, y: i32, value: &str, a: &str, b: &str| {
        let i = s.add(kind, (x, y), 1);
        s.components[i].value = value.into();
        label(s, i, "1", a);
        label(s, i, "2", b);
    };
    // the speaker
    part(&mut s, Kind::Resistor, 20, 0, "8", "SPIN", "SP1");
    part(&mut s, Kind::Inductor, 36, 0, "0.1m", "SP1", "SPM");
    part(&mut s, Kind::Resistor, 52, 0, "24", "SPM", "GND");
    part(&mut s, Kind::Inductor, 68, 0, "2.12m", "SPM", "GND");
    part(&mut s, Kind::Capacitor, 84, 0, "33u", "SPM", "GND");
    // the piezo buzzer
    part(&mut s, Kind::Capacitor, 20, 30, "15n", "PZIN", "GND");
    part(&mut s, Kind::Resistor, 36, 30, "1k", "PZIN", "PZ1");
    part(&mut s, Kind::Inductor, 52, 30, "0.53", "PZ1", "PZ2");
    part(&mut s, Kind::Capacitor, 68, 30, "3n", "PZ2", "GND");
    std::fs::write(root.join("speaker_basics.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/varistor_basics.schjson for the varistor lesson: V1 (12 V) -> R1 = 10 ohm -> node M with the varistor RV1
/// (DC sweep of V1 shows the clamping); V2 (a 100 V, 100 us surge) -> 10 ohm (R2) -> load R3 = 1 kohm (node A, unprotected) and ->
/// 10 ohm (R4) -> load R5 = 1 kohm with the varistor RV2 across it (node B): `cargo test -p app write_varistor_basics_example -- --ignored`
#[test]
#[ignore]
fn write_varistor_basics_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "MOV_14V").unwrap();
    let mut s = sch_core::Schematic::default();
    let part = |s: &mut sch_core::Schematic, kind: Kind, x: i32, y: i32, value: &str, a: &str, b: &str| {
        let i = s.add(kind, (x, y), 1);
        s.components[i].value = value.into();
        label(s, i, "1", a);
        label(s, i, "2", b);
    };
    let mov = |s: &mut sch_core::Schematic, x: i32, y: i32, a: &str, b: &str| {
        let i = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (x, y), 1);
        label(s, i, "1", a);
        label(s, i, "2", b);
    };
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "12".into();
    label(&mut s, v1, "+", "RAIL");
    label(&mut s, v1, "-", "GND");
    part(&mut s, Kind::Resistor, 20, 0, "10", "RAIL", "M");
    mov(&mut s, 36, 0, "M", "GND");
    let v2 = s.add(Kind::VSource, (0, 30), 0);
    s.components[v2].value = "PULSE(0 100 1m 10u 10u 100u 5m)".into();
    label(&mut s, v2, "+", "SURGE");
    label(&mut s, v2, "-", "GND");
    part(&mut s, Kind::Resistor, 20, 30, "10", "SURGE", "A");
    part(&mut s, Kind::Resistor, 36, 30, "1k", "A", "GND");
    part(&mut s, Kind::Resistor, 20, 50, "10", "SURGE", "B");
    part(&mut s, Kind::Resistor, 36, 50, "1k", "B", "GND");
    mov(&mut s, 52, 50, "B", "GND");
    // the 12 V rail resistor and the surge source impedance do not carry power: the smallest body is enough
    std::fs::write(root.join("varistor_basics.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/hall_basics.schjson for the Hall sensor lesson: V1 (5 V) powers an SS49E linear Hall sensor (U1); V2 gives
/// the magnetic field to its B pin (1 V = 100 G, 5 V at first = a magnet at 500 G); R1 = 100 kohm is the load (the ADC input):
/// `cargo test -p app write_hall_basics_example -- --ignored`
#[test]
#[ignore]
fn write_hall_basics_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "SS49E").unwrap();
    let mut s = sch_core::Schematic::default();
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "5".into();
    label(&mut s, v1, "+", "VCC");
    label(&mut s, v1, "-", "GND");
    let v2 = s.add(Kind::VSource, (0, 24), 0);
    s.components[v2].value = "5".into();
    label(&mut s, v2, "+", "FIELD");
    label(&mut s, v2, "-", "GND");
    let u = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (30, 0), 0);
    label(&mut s, u, "1", "VCC");
    label(&mut s, u, "2", "GND");
    label(&mut s, u, "3", "OUT");
    label(&mut s, u, "B", "FIELD");
    let r = s.add(Kind::Resistor, (50, 0), 1);
    s.components[r].value = "100k".into();
    label(&mut s, r, "1", "OUT");
    label(&mut s, r, "2", "GND");
    std::fs::write(root.join("hall_basics.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/switch_basics.schjson for the switch lesson (a closed contact is a resistor): V1 (5 V) -> R1 = 0.05 ohm
/// (the contact) -> R2 = 47 ohm (the load); V2 (3.3 V) -> R3 = 10 kohm (pull-up) -> node SIG -> R4 = 0.05 ohm (the contact) -> GND:
/// `cargo test -p app write_switch_basics_example -- --ignored`
#[test]
#[ignore]
fn write_switch_basics_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut s = sch_core::Schematic::default();
    let part = |s: &mut sch_core::Schematic, x: i32, y: i32, value: &str, a: &str, b: &str| {
        let i = s.add(Kind::Resistor, (x, y), 1);
        s.components[i].value = value.into();
        s.components[i].footprint = "R_2512".into();
        label(s, i, "1", a);
        label(s, i, "2", b);
    };
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "5".into();
    label(&mut s, v1, "+", "VCC");
    label(&mut s, v1, "-", "GND");
    part(&mut s, 20, 0, "0.05", "VCC", "LOAD");
    part(&mut s, 36, 0, "47", "LOAD", "GND");
    let v2 = s.add(Kind::VSource, (0, 28), 0);
    s.components[v2].value = "3.3".into();
    label(&mut s, v2, "+", "V33");
    label(&mut s, v2, "-", "GND");
    part(&mut s, 20, 28, "10k", "V33", "SIG");
    part(&mut s, 36, 28, "0.05", "SIG", "GND");
    std::fs::write(root.join("switch_basics.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/battery_basics.schjson for the battery lesson: a fresh AA alkaline cell (BT1) on 10 ohm (R1), a fresh 18650
/// Li-ion cell (BT2) on 22 ohm (R2) and a fresh CR2032 coin cell (BT3) on 1 kohm (R3), side by side:
/// `cargo test -p app write_battery_basics_example -- --ignored`
#[test]
#[ignore]
fn write_battery_basics_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let find = |n: &str| {
        let (_, t) = part_lib::EXAMPLES.iter().find(|(k, _)| k.starts_with(n)).unwrap();
        serde_json::from_str::<part_lib::PartDef>(t).unwrap()
    };
    let mut s = sch_core::Schematic::default();
    for (y, name, ohm, plus, load) in [(0, "AA_ALKALINE", "10", "VAA", 1), (24, "LIION_18650", "22", "VLI", 2), (48, "CR2032", "1k", "VCOIN", 3)] {
        let def = find(name);
        let b = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (0, y), 0);
        label(&mut s, b, "1", plus);
        label(&mut s, b, "2", "GND");
        let r = s.add(Kind::Resistor, (24, y), 1);
        s.components[r].value = ohm.into();
        s.components[r].footprint = "R_2512".into();
        label(&mut s, r, "1", plus);
        label(&mut s, r, "2", "GND");
        let _ = load;
    }
    std::fs::write(root.join("battery_basics.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/segment_basics.schjson for the 7-segment display lesson (a segment is an LED; only the lit ones are drawn):
/// V1 (5 V) -> R1 = 100 ohm shared by the 2 lit segments of a "1" (D1, D2); -> R2 = 100 ohm shared by the 7 segments of an "8"
/// (D3 - D9); -> a 330 ohm resistor for each of the 7 segments of an "8" (R3 - R9, D10 - D16):
/// `cargo test -p app write_segment_basics_example -- --ignored`
#[test]
#[ignore]
fn write_segment_basics_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut s = sch_core::Schematic::default();
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "5".into();
    label(&mut s, v1, "+", "VCC");
    label(&mut s, v1, "-", "GND");
    // a shared resistor and the LEDs of the lit segments in parallel after it
    let shared = |s: &mut sch_core::Schematic, y: i32, node: &str, leds: usize| {
        let r = s.add(Kind::Resistor, (20, y), 1);
        s.components[r].value = "100".into();
        s.components[r].footprint = "R_2512".into();
        label(s, r, "1", "VCC");
        label(s, r, "2", node);
        for k in 0..leds {
            let d = s.add(Kind::Led, (40 + 14 * k as i32, y), 0);
            label(s, d, "A", node);
            label(s, d, "K", "GND");
        }
    };
    shared(&mut s, 0, "C1", 2);
    shared(&mut s, 24, "C8", 7);
    // a resistor for every segment
    for k in 0..7 {
        let y = 48 + 12 * k;
        let r = s.add(Kind::Resistor, (20, y), 1);
        s.components[r].value = "330".into();
        label(&mut s, r, "1", "VCC");
        label(&mut s, r, "2", &format!("S{}", k + 1));
        let d = s.add(Kind::Led, (40, y), 0);
        label(&mut s, d, "A", &format!("S{}", k + 1));
        label(&mut s, d, "K", "GND");
    }
    std::fs::write(root.join("segment_basics.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/analog_switch_basics.schjson for the analog switch lesson: two TMUX1101 (VDD = 5 V). U1 passes the 3 V
/// signal V1 to OUT, with the load R1 = 1 kohm (SEL V2 = 5 V: on). U2 is a sample-and-hold: V1 -> R2 = 1 kohm -> U2 -> HOLD with C1 = 10 nF,
/// SEL V3 is a pulse that is high from 10 us to 110 us: `cargo test -p app write_analog_switch_basics_example -- --ignored`
#[test]
#[ignore]
fn write_analog_switch_basics_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "TMUX1101").unwrap();
    let mut s = sch_core::Schematic::default();
    let src = |s: &mut sch_core::Schematic, y: i32, value: &str, net: &str| {
        let v = s.add(Kind::VSource, (0, y), 0);
        s.components[v].value = value.into();
        label(s, v, "+", net);
        label(s, v, "-", "GND");
    };
    src(&mut s, 0, "3", "SIG");
    src(&mut s, 24, "5", "ON");
    src(&mut s, 48, "PULSE(0 5 10u 1n 1n 100u 400u)", "SAMPLE");
    src(&mut s, 72, "5", "VDD");
    // U1: the signal through the switch into a load
    let u1 = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (30, 0), 0);
    label(&mut s, u1, "2", "SIG");
    label(&mut s, u1, "1", "OUT");
    label(&mut s, u1, "4", "ON");
    label(&mut s, u1, "5", "VDD");
    label(&mut s, u1, "3", "GND");
    let r1 = s.add(Kind::Resistor, (50, 0), 1);
    s.components[r1].value = "1k".into();
    label(&mut s, r1, "1", "OUT");
    label(&mut s, r1, "2", "GND");
    // U2: sample and hold
    let r2 = s.add(Kind::Resistor, (20, 40), 1);
    s.components[r2].value = "1k".into();
    label(&mut s, r2, "1", "SIG");
    label(&mut s, r2, "2", "SIN");
    let u2 = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (40, 40), 0);
    label(&mut s, u2, "2", "SIN");
    label(&mut s, u2, "1", "HOLD");
    label(&mut s, u2, "4", "SAMPLE");
    label(&mut s, u2, "5", "VDD");
    label(&mut s, u2, "3", "GND");
    let c1 = s.add(Kind::Capacitor, (60, 40), 1);
    s.components[c1].value = "10n".into();
    label(&mut s, c1, "1", "HOLD");
    label(&mut s, c1, "2", "GND");
    // the leakage of the hold capacitor (and the way to a defined 0 V at the start)
    let r3 = s.add(Kind::Resistor, (80, 40), 1);
    s.components[r3].value = "100Meg".into();
    label(&mut s, r3, "1", "HOLD");
    label(&mut s, r3, "2", "GND");
    std::fs::write(root.join("analog_switch_basics.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/bridge_basics.schjson for the diode bridge lesson: a 12 V rms (17 V peak) 50 Hz source V1 feeds a bridge of four
/// 1N4007 (D1 - D4: ACP/ACN to OUTA/GND) with R1 = 100 ohm and C1 = 470 uF; a second source V2 feeds a single 1N4007 (D5) with R2 = 100 ohm
/// and C2 = 470 uF (OUTB) for comparison; R3 = 1 Mohm ties the floating source V1 to GND:
/// `cargo test -p app write_bridge_basics_example -- --ignored`
#[test]
#[ignore]
fn write_bridge_basics_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "1N4007").unwrap();
    let mut s = sch_core::Schematic::default();
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "SIN(0 17 50)".into();
    label(&mut s, v1, "+", "ACP");
    label(&mut s, v1, "-", "ACN");
    // the four diodes of the bridge: (anode, cathode)
    for (k, (a, c)) in [("ACP", "OUTA"), ("ACN", "OUTA"), ("GND", "ACP"), ("GND", "ACN")].into_iter().enumerate() {
        let d = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (24 + 20 * k as i32, 0), 0);
        label(&mut s, d, "A", a);
        label(&mut s, d, "K", c);
    }
    let r1 = s.add(Kind::Resistor, (110, 0), 1);
    s.components[r1].value = "100".into();
    s.components[r1].footprint = "R_2512".into();
    label(&mut s, r1, "1", "OUTA");
    label(&mut s, r1, "2", "GND");
    let c1 = s.add(Kind::Capacitor, (126, 0), 1);
    s.components[c1].value = "470u".into();
    label(&mut s, c1, "1", "OUTA");
    label(&mut s, c1, "2", "GND");
    let r3 = s.add(Kind::Resistor, (142, 0), 1);
    s.components[r3].value = "1Meg".into();
    label(&mut s, r3, "1", "ACN");
    label(&mut s, r3, "2", "GND");
    // the half-wave rectifier
    let v2 = s.add(Kind::VSource, (0, 40), 0);
    s.components[v2].value = "SIN(0 17 50)".into();
    label(&mut s, v2, "+", "HWIN");
    label(&mut s, v2, "-", "GND");
    let d5 = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (24, 40), 0);
    label(&mut s, d5, "A", "HWIN");
    label(&mut s, d5, "K", "OUTB");
    let r2 = s.add(Kind::Resistor, (110, 40), 1);
    s.components[r2].value = "100".into();
    s.components[r2].footprint = "R_2512".into();
    label(&mut s, r2, "1", "OUTB");
    label(&mut s, r2, "2", "GND");
    let c2 = s.add(Kind::Capacitor, (126, 40), 1);
    s.components[c2].value = "470u".into();
    label(&mut s, c2, "1", "OUTB");
    label(&mut s, c2, "2", "GND");
    std::fs::write(root.join("bridge_basics.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/eeprom_basics.schjson for the EEPROM (I2C) lesson: the SDA line is pulled up by R1 = 4.7 kohm to VCC = 3.3 V (V1)
/// and loaded by the bus capacitance C1 = 100 pF; an open-drain driver (a 2N7000, Q1) pulls it low while its gate V2 is high (a 100 kHz pulse):
/// `cargo test -p app write_eeprom_basics_example -- --ignored`
#[test]
#[ignore]
fn write_eeprom_basics_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "2N7000").unwrap();
    let mut s = sch_core::Schematic::default();
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "3.3".into();
    label(&mut s, v1, "+", "VCC");
    label(&mut s, v1, "-", "GND");
    let v2 = s.add(Kind::VSource, (0, 24), 0);
    s.components[v2].value = "PULSE(0 3.3 1u 5n 5n 4u 10u)".into();
    label(&mut s, v2, "+", "DRV");
    label(&mut s, v2, "-", "GND");
    let r1 = s.add(Kind::Resistor, (24, 0), 1);
    s.components[r1].value = "4.7k".into();
    label(&mut s, r1, "1", "VCC");
    label(&mut s, r1, "2", "SDA");
    let c1 = s.add(Kind::Capacitor, (40, 0), 1);
    s.components[c1].value = "100p".into();
    label(&mut s, c1, "1", "SDA");
    label(&mut s, c1, "2", "GND");
    let q1 = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (60, 0), 0);
    label(&mut s, q1, "3", "SDA");
    label(&mut s, q1, "2", "DRV");
    label(&mut s, q1, "1", "GND");
    std::fs::write(root.join("eeprom_basics.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/electrolytic_life.schjson for the electrolytic capacitor life lesson: a 5 V rail with a 0.5 V, 100 kHz ripple
/// (V1 = SIN(5 0.5 100k)) through R1 = 2 ohm into the 100 uF / 16 V electrolytic capacitor C1 (ripple 0.3 A rms at 100 kHz, 6000 h at 105 C):
/// `cargo test -p app write_electrolytic_life_example -- --ignored`
#[test]
#[ignore]
fn write_electrolytic_life_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "CE_100u_16V").unwrap();
    let mut s = sch_core::Schematic::default();
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "SIN(5 0.5 100k)".into();
    label(&mut s, v1, "+", "RIPPLE");
    label(&mut s, v1, "-", "GND");
    let r1 = s.add(Kind::Resistor, (20, 0), 1);
    s.components[r1].value = "2".into();
    label(&mut s, r1, "1", "RIPPLE");
    label(&mut s, r1, "2", "CAP");
    let c1 = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (40, 0), 1);
    label(&mut s, c1, "1", "CAP");
    label(&mut s, c1, "2", "GND");
    std::fs::write(root.join("electrolytic_life.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/crystal_response.schjson: the 16 MHz crystal in a 50 ohm line (V1 -> Y1 -> R1 to GND), for the
/// crystal lesson and its AC test: `cargo test -p app write_crystal_example -- --ignored`
#[test]
#[ignore]
fn write_crystal_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "XTAL_16MHz_3225").unwrap();
    let mut s = sch_core::Schematic::default();
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

/// Generates docs/examples/button_bounce.schjson: a button that bounces. V1 is what the contact does (PWL: open = 5 V, closes at 1 ms,
/// bounces back and forth until 2.1 ms), R1 = 10k stands for the pull-up, C1 = the debounce capacitor on the input pin IN (1 nF = hardly any):
/// `cargo test -p app write_bounce_example -- --ignored`
#[test]
#[ignore]
fn write_bounce_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut s = sch_core::Schematic::default();
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "PWL(0 5 1m 5 1.001m 0 1.1m 0 1.101m 5 1.25m 5 1.251m 0 1.4m 0 1.401m 5 1.6m 5 1.601m 0 1.8m 0 1.801m 5 2.1m 5 2.101m 0 6m 0)".into();
    label(&mut s, v1, "+", "CONTACT");
    label(&mut s, v1, "-", "GND");
    let r1 = s.add(Kind::Resistor, (14, 0), 1);
    s.components[r1].value = "10k".into();
    label(&mut s, r1, "1", "CONTACT");
    label(&mut s, r1, "2", "IN");
    let c1 = s.add(Kind::Capacitor, (28, 0), 1);
    s.components[c1].value = "1n".into();
    label(&mut s, c1, "1", "IN");
    label(&mut s, c1, "2", "GND");
    std::fs::write(root.join("button_bounce.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
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

/// Generates docs/examples/uln2803a_model.schjson: the same relay-coil load (120 ohm from 12 V) switched by the library ULN2803A (U1, the
/// packaged equivalent model, a reference) and by a hand-built equivalent of one channel: R3 2.7k input, R4 7.2k and R5 3k turn-off resistors,
/// two 2N3904 as a Darlington (Q1, Q2), the clamp diode D1 and R6 = 0.01 ohm in Q2's emitter (the learner raises it to about 1.5 ohm). Both are
/// driven from the same 5 V (V2). `cargo test -p app write_uln2803a_example -- --ignored`
#[test]
#[ignore]
fn write_uln2803a_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let part = |name: &str| part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == name).unwrap();
    let (uln, npn) = (part("ULN2803A"), part("2N3904"));
    let pin = |def: &part_lib::PartDef, name: &str| def.pins.iter().find(|p| p.name == name).unwrap_or_else(|| panic!("pin {name}")).number.clone();
    let mut s = sch_core::Schematic::default();
    // a short wire out of the pin on the side of the symbol the pin belongs to (a tall symbol: the offset alone would point sideways pins up)
    let label = |s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str| {
        let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap_or_else(|| panic!("pin {pin}"));
        let side = s.components[idx].custom.as_ref().and_then(|c| c.pins.iter().find(|p| p.number == pin)).map(|p| p.side);
        let end = match side {
            Some(sch_core::PinSide::Left) => (pt.0 - 3, pt.1),
            Some(sch_core::PinSide::Right) => (pt.0 + 3, pt.1),
            Some(sch_core::PinSide::Top) => (pt.0, pt.1 - 3),
            Some(sch_core::PinSide::Bottom) => (pt.0, pt.1 + 3),
            None => {
                let c = s.components[idx].pos;
                let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
                if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) }
            }
        };
        s.wires.push(Wire { a: pt, b: end });
        s.labels.push(sch_core::Label { pos: end, text: text.into() });
    };
    // sources: 12 V supply (also the relay supply), 5 V from a microcontroller pin
    let v1 = s.add(Kind::VSource, (0, 0), 0);
    s.components[v1].value = "12".into();
    label(&mut s, v1, "+", "VCC");
    label(&mut s, v1, "-", "GND");
    let v2 = s.add(Kind::VSource, (12, 0), 0);
    s.components[v2].value = "5".into();
    label(&mut s, v2, "+", "IN");
    label(&mut s, v2, "-", "GND");
    // the packaged part (reference): IN1 -> OUT1, unused inputs on GND, unused outputs named
    let u1 = s.add_custom(uln.symbol(), &uln.default_value(), &uln.footprint_name(), (36, 14), 0);
    label(&mut s, u1, &pin(&uln, "IN1"), "IN");
    for i in 2..=8 {
        label(&mut s, u1, &pin(&uln, &format!("IN{i}")), "GND");
        label(&mut s, u1, &pin(&uln, &format!("OUT{i}")), &format!("NC{i}"));
    }
    label(&mut s, u1, &pin(&uln, "GND"), "GND");
    label(&mut s, u1, &pin(&uln, "COM"), "VCC");
    label(&mut s, u1, &pin(&uln, "OUT1"), "OUTA");
    let r1 = s.add(Kind::Resistor, (70, 0), 1);
    s.components[r1].value = "120".into();
    // a relay coil is not a chip resistor: a name the rating check does not know, so that the coil's 1 W is not "over the limit"
    s.components[r1].footprint = "Relay_coil".into();
    label(&mut s, r1, "1", "VCC");
    label(&mut s, r1, "2", "OUTA");
    // the hand-built channel
    let two = |s: &mut sch_core::Schematic, x: i32, value: &str, a: &str, b: &str| {
        let r = s.add(Kind::Resistor, (x, 54), 1);
        s.components[r].value = value.into();
        label(s, r, "1", a);
        label(s, r, "2", b);
        r
    };
    let r2 = two(&mut s, 98, "120", "VCC", "OUTB");
    s.components[r2].footprint = "Relay_coil".into();
    two(&mut s, 0, "2.7k", "IN", "B1");
    two(&mut s, 12, "7.2k", "B1", "GND");
    two(&mut s, 50, "3k", "E1", "GND");
    // 1.5 ohm at 320 mA is 0.15 W: a 1206 (0.25 W), not the default 0805 (0.125 W)
    let r6 = two(&mut s, 80, "0.01", "QE", "GND");
    s.components[r6].footprint = "R_1206".into();
    let q1 = s.add_custom(npn.symbol(), &npn.default_value(), &npn.footprint_name(), (30, 56), 0);
    label(&mut s, q1, &pin(&npn, "B"), "B1");
    label(&mut s, q1, &pin(&npn, "C"), "OUTB");
    label(&mut s, q1, &pin(&npn, "E"), "E1");
    let q2 = s.add_custom(npn.symbol(), &npn.default_value(), &npn.footprint_name(), (62, 56), 0);
    label(&mut s, q2, &pin(&npn, "B"), "E1");
    label(&mut s, q2, &pin(&npn, "C"), "OUTB");
    label(&mut s, q2, &pin(&npn, "E"), "QE");
    let d1 = s.add(Kind::Diode, (110, 54), 1);
    s.components[d1].value = "1N4007".into();
    label(&mut s, d1, "A", "OUTB");
    label(&mut s, d1, "K", "VCC");
    std::fs::write(root.join("uln2803a_model.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Writes docs/examples/two_sheets.schjson (the regulator_led circuit split over two sheets, joined by the net label 5V):
/// `cargo test -p app write_two_sheets_example -- --ignored`
#[test]
#[ignore]
fn write_two_sheets_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let src: sch_core::Schematic = serde_json::from_str(&std::fs::read_to_string(root.join("regulator_led.schjson")).unwrap()).unwrap();
    let part = |r: &str| src.components.iter().find(|c| c.refdes == r).unwrap().clone();
    let mut s = sch_core::Schematic::default();
    // sheet 1: the supply (12 V -> LM7805), its output named 5V
    s.components = vec![part("V1"), part("U1"), part("#PWR01")];
    for (a, b) in [((0, -2), (0, -6)), ((0, -6), (10, -6)), ((10, -6), (10, 0)), ((14, 4), (14, 8)), ((0, 2), (0, 8)), ((0, 8), (14, 8)), ((18, 0), (22, 0))] {
        s.wires.push(Wire { a, b });
    }
    s.labels.push(sch_core::Label { pos: (22, 0), text: "5V".into() });
    s.add_sheet();
    // sheet 2: the load, taking 5V by the same label
    s.components = vec![part("R1"), part("D1")];
    let gnd = s.add(Kind::Ground, (30, 8), 0);
    s.components[gnd].refdes = "#PWR02".into();
    for (a, b) in [((24, 0), (26, 0)), ((30, 0), (30, 8))] {
        s.wires.push(Wire { a, b });
    }
    s.labels.push(sch_core::Label { pos: (20, 0), text: "5V".into() });
    s.rename_sheet(0, "電源");
    s.rename_sheet(1, "負荷");
    s.switch_sheet(0);
    std::fs::write(root.join("two_sheets.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
}

/// Generates docs/examples/work_in_progress/diff_lanes_routed.pcbjson for the board lesson on pairs of differential pairs:
/// two headers 52 mm apart carry two lanes (LANE0, LANE1) as pairs, routed side by side on the top layer; lane 0 has a
/// serpentine already (bent together, as `match_pairs` does it: it had to go round something), lane 1 is straight and so shorter.
/// `cargo test -p app write_diff_lanes_board -- --ignored --nocapture` (after `docs/examples/diff_lanes.schjson` exists)
#[test]
#[ignore]
fn write_diff_lanes_board() {
    use pcb_core::autoroute::{route_pair, RouteOptions};
    use pcb_core::diffpair::{match_pairs, pair_report, DiffPair};
    use pcb_core::meander::MeanderOptions;
    use pcb_core::Zone;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.sch.load_path(root.join("diff_lanes.schjson"));
    app.board_ed.import_from_schematic(&app.sch);
    let b = &mut app.board_ed.board;
    b.name = "diff_lanes".into();
    b.set_rect_outline(0.0, 0.0, 70.0, 24.0);
    b.place_part("J1", Point::new(8.0, 18.0), 0);
    b.place_part("J2", Point::new(60.0, 18.0), 0);
    b.place_part("C1", Point::new(66.0, 6.0), 0);
    let mut pairs = vec![DiffPair::new("LANE0", "LANE0_P", "LANE0_N"), DiffPair::new("LANE1", "LANE1_P", "LANE1_N")];
    for p in &mut pairs {
        p.gap = 0.3;
        // a slow signal: the router leaves the lines of a pair about 1.8 mm apart in length, which is within what is asked here
        p.max_skew = 2.0;
    }
    b.diff_pairs = pairs.clone();
    let opt = RouteOptions { track_width: 0.25, grid: 0.1, only_layers: Some(vec![Side::Top]), no_vias: true, ..RouteOptions::default() };
    for p in &pairs {
        let r = route_pair(b, p, &opt);
        println!("{}: routed {} failed {:?}", p.name, r.routed, r.failed);
    }
    b.zones.push(Zone::new("GND", Side::Bottom, vec![Point::new(1.0, 1.0), Point::new(69.0, 1.0), Point::new(69.0, 23.0), Point::new(1.0, 23.0)]));
    // the supply (3.3 V to the capacitor): the rest of the board, routed after the lanes
    let rest = pcb_core::autoroute::autoroute(b, &RouteOptions { track_width: 0.25, grid: 0.1, ..RouteOptions::default() }, Some("V3V3"));
    println!("V3V3: routed {} failed {:?}", rest.routed, rest.failed);
    let gnd = pcb_core::autoroute::autoroute(b, &RouteOptions { track_width: 0.25, grid: 0.1, ..RouteOptions::default() }, Some("GND"));
    println!("GND: routed {} failed {:?}", gnd.routed, gnd.failed);
    let before: Vec<_> = pairs.iter().map(|p| pair_report(b, p)).collect();
    println!("before: {before:?}");
    let target = before[0].len_p.max(before[0].len_n) + 12.0;
    let r = match_pairs(b, &pairs[..1], Some(target), &MeanderOptions::default());
    println!("lane 0 lengthened: {r:?}");
    for p in &pairs {
        println!("{}: {:?}", p.name, pair_report(b, p));
    }
    println!("DRC: {:?}", pcb_core::drc::check(b));
    println!("open: {}", pcb_core::ratsnest::ratsnest(b).len());
    std::fs::create_dir_all(root.join("work_in_progress")).unwrap();
    std::fs::write(root.join("work_in_progress/diff_lanes_routed.pcbjson"), serde_json::to_string_pretty(&*b).unwrap()).unwrap();
}
