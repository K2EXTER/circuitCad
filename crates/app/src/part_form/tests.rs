use super::*;

#[cfg(test)]
mod charger_tests {
    use super::*;
    use sim_core::{add_subckt_instance, dc_op, Circuit, Element};

    struct Rig {
        c: Circuit,
    }

    /// The generated model with `vin` on VIN, a stiff cell source on BAT and `rprog` ohms on PROG
    /// (None = open); other pins get light loads. Returns the circuit.
    fn rig(t: &ChargerTemplate, vin: f64, vbat: f64, rprog: Option<f64>) -> Rig {
        let g = t.generate("TPL").unwrap();
        let mut c = Circuit::new();
        let ports: Vec<String> = g.pins.iter().map(|p| p.name.to_ascii_lowercase()).collect();
        let nodes: Vec<usize> = ports.iter().map(|p| if p == "gnd" { 0 } else { c.node(p) }).collect();
        add_subckt_instance(&mut c, "U1", &g.sim_name, &g.sim_text, &nodes).unwrap_or_else(|e| panic!("{e}"));
        let node = |c: &mut Circuit, n: &str| c.node(n);
        let (vin_n, bat_n) = (node(&mut c, "vin"), node(&mut c, "bat"));
        c.add(Element::vdc("Vin", vin_n, 0, vin));
        c.add(Element::vdc("Vbat", bat_n, 0, vbat));
        if ports.iter().any(|p| p == "lx") {
            let lx = node(&mut c, "lx");
            c.add(Element::Inductor { name: "L1".into(), a: lx, b: bat_n, l: 2.2e-6 });
        }
        if let (Some(r), true) = (rprog, ports.iter().any(|p| p == "prog")) {
            let prog = node(&mut c, "prog");
            c.add(Element::Resistor { name: "Rp".into(), a: prog, b: 0, r });
        }
        for (n, pull) in [("chrg", true), ("stdby", true), ("ntc", false)] {
            if ports.iter().any(|p| p == n) {
                let id = node(&mut c, n);
                if pull {
                    c.add(Element::Resistor { name: format!("Rpull_{n}"), a: vin_n, b: id, r: 1e3 });
                } else {
                    c.add(Element::Resistor { name: "Rntc_ext".into(), a: id, b: 0, r: 10e3 });
                }
            }
        }
        Rig { c }
    }

    fn charge(t: &ChargerTemplate, vin: f64, vbat: f64, rprog: Option<f64>) -> f64 {
        dc_op(&rig(t, vin, vbat, rprog).c).unwrap_or_else(|e| panic!("{e:?}")).element("Vbat").unwrap().current
    }

    fn near(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1e-3)
    }

    #[test]
    fn the_default_template_is_a_working_linear_charger() {
        let t = ChargerTemplate::default();
        // I = 1200 / R
        for (r, want) in [(1.2e3, 1.0), (2.4e3, 0.5), (4.8e3, 0.25)] {
            let i = charge(&t, 5.0, 3.7, Some(r));
            assert!(near(i, want, 0.04), "R {r}: {i}");
        }
        // pre-charge: a tenth below 2.9 V; taper near 4.2 V; nothing when full or without supply
        assert!(near(charge(&t, 5.0, 2.5, Some(2.4e3)), 0.05, 0.1));
        let mid = charge(&t, 5.0, 4.19, Some(2.4e3));
        assert!(mid < 0.45 && mid > 0.05, "{mid}");
        assert!(charge(&t, 5.0, 4.21, Some(2.4e3)).abs() < 0.02);
        assert!(charge(&t, 3.5, 3.0, Some(2.4e3)).abs() < 5e-3);
        // an open PROG pin gives the open-pin current (0 here)
        assert!(charge(&t, 5.0, 3.7, None).abs() < 5e-3);
    }

    #[test]
    fn the_ip2332_style_template_reproduces_the_bundled_part() {
        let t = ChargerTemplate {
            topology: Topology::Buck,
            current: 2.0,
            k: 2750.0,
            offset: 0.35,
            precharge_fraction: 0.2,
            vin_min: 4.3,
            vin_max: 6.0,
            dropout: 0.1,
            ntc: Some((0.3, 2.73)),
            ..Default::default()
        };
        // same table as the datasheet: 4.3k -> 1 A, 2.4k -> 1.5 A, open -> 2 A
        assert!(near(charge(&t, 5.0, 3.7, Some(4.3e3)), 0.99, 0.05));
        assert!(near(charge(&t, 5.0, 3.7, Some(2.4e3)), 1.5, 0.05));
        assert!(near(charge(&t, 5.0, 3.7, None), 2.0, 0.05));
        assert!(charge(&t, 6.5, 3.7, Some(4.3e3)).abs() < 5e-3, "over-voltage");
        assert!(charge(&t, 4.0, 3.7, Some(4.3e3)).abs() < 5e-3, "under-voltage");
    }

    #[test]
    fn two_cells_float_at_8v4_and_pre_charge_at_5v8() {
        let t = ChargerTemplate { cells: 2, vin_min: 9.0, ..Default::default() };
        assert!(near(charge(&t, 12.0, 7.4, Some(2.4e3)), 0.5, 0.05));
        assert!(charge(&t, 12.0, 8.45, Some(2.4e3)).abs() < 0.02);
        assert!(near(charge(&t, 12.0, 5.0, Some(2.4e3)), 0.05, 0.1), "below 5.8 V");
    }

    #[test]
    fn a_fixed_current_part_has_no_prog_pin_and_status_lines_follow() {
        let t = ChargerTemplate { setting: CurrentSetting::Fixed, current: 0.8, ..Default::default() };
        assert!(near(charge(&t, 5.0, 3.7, None), 0.8, 0.04));
        let r = rig(&t, 5.0, 3.7, None);
        let op = dc_op(&r.c).unwrap();
        assert!(op.voltage("chrg").unwrap() < 1.0 && op.voltage("stdby").unwrap() > 4.0, "charging");
        let r = rig(&t, 5.0, 4.2, None);
        let op = dc_op(&r.c).unwrap();
        assert!(op.voltage("stdby").unwrap() < 1.0 && op.voltage("chrg").unwrap() > 4.0, "full");
    }

    #[test]
    fn applying_the_template_fills_the_form_and_the_result_passes_the_checks() {
        let mut f = PartForm { name: "MYCHG".into(), tpl: ChargerTemplate { status: false, ..Default::default() }, ..Default::default() };
        f.apply_template();
        assert!(f.tpl_message.contains("ピン 4 本"), "{}", f.tpl_message);
        assert!(f.pins_text.contains("2 GND bottom") && f.pins_text.contains("4 PROG left"));
        assert!(f.sim == SimChoice::Subckt);
        assert_eq!(f.sim_pins, "1 2 3 4");
        assert_eq!(f.category, "電源管理");
        // with a 4-pin dual package the definition has no errors
        f.pkg = PkgKind::Dual;
        let def = f.to_def().unwrap();
        assert!(!def.has_errors(), "{:?}", def.validate().iter().map(|i| &i.message).collect::<Vec<_>>());
        // the notes about what the model leaves out go into the saved part
        assert!(f.sim_limits.lines().count() >= 4, "{}", f.sim_limits);
        assert_eq!(def.sim.as_ref().unwrap().limits.len(), f.sim_limits.lines().count());
        assert!(def.sim.unwrap().limits.iter().any(|l| l.contains("リニア") || l.contains("LX")));
        // a missing name is explained
        let mut g = PartForm::default();
        g.apply_template();
        assert!(g.tpl_message.contains("部品名"), "{}", g.tpl_message);
    }
}

#[cfg(test)]
mod ratings_tests {
    use super::*;

    fn form() -> PartForm {
        let mut f = PartForm::default();
        f.name = "TEST".into();
        f.pins_text = "1 A\n2 K".into();
        f.pkg = PkgKind::Chip2;
        f
    }

    /// The checklist adds the items to look up; typing a value turns a line into a data sheet row, and the
    /// note to the learner (※...) is not saved as a condition.
    #[test]
    fn the_checklist_turns_into_rows_as_values_are_filled_in() {
        let mut f = form();
        f.rt_kind = 0; // diode
        f.rt_rows = merge_checklist(&f.rt_rows, 0);
        assert!(f.rt_rows.lines().count() >= 4 && f.rt_rows.contains("順方向電圧 Vf") && f.rt_rows.contains("※"));
        // nothing is looked up yet: the part has no ratings, and saving is still possible
        let d = f.to_def().unwrap();
        assert!(d.ratings.is_none());
        assert_eq!(f.ratings_progress(), (0, 0, f.rt_rows.lines().count()));
        // the learner fills in the forward voltage and the reverse voltage
        f.rt_rows = f.rt_rows.replace("順方向電圧 Vf |  | ※", "順方向電圧 Vf | 0.6V (最大) | IF = 1A ※");
        f.rt_rows = f.rt_rows.replace("順方向電圧 Vf |  | ", "順方向電圧 Vf | 0.6V (最大) | ");
        f.rt_v = "40".into();
        let r = f.to_def().unwrap().ratings.unwrap();
        assert_eq!(r.v_max, Some(40.0));
        let vf = r.rows.iter().find(|x| x.name == "順方向電圧 Vf").expect("the filled row");
        assert_eq!(vf.value, "0.6V (最大)");
        assert_eq!(vf.condition, "IF = 1A", "the note after ※ is dropped");
        let (max_given, filled, open) = f.ratings_progress();
        assert_eq!((max_given, filled), (1, 1));
        assert!(open >= 3);
        // asking for the checklist again does not repeat the items that are already there
        let again = merge_checklist(&f.rt_rows, 0);
        assert_eq!(again.lines().count(), f.rt_rows.lines().count());
        // every kind has a hint and items
        for (name, hint, items) in CHECKLISTS {
            assert!(!name.is_empty() && !hint.is_empty() && !items.is_empty(), "{name}");
        }
    }

    #[test]
    fn a_line_without_a_name_or_a_separator_is_explained() {
        let mut f = form();
        f.rt_rows = "順方向電圧 0.6V".into();
        assert!(f.to_def().unwrap_err().contains("項目 | 値 | 条件"));
        f.rt_rows = " | 0.6V".into();
        assert!(f.to_def().is_err());
        f.rt_rows = String::new();
        f.rt_v = "abc".into();
        assert!(f.to_def().unwrap_err().contains("最大電圧"));
    }
}

#[cfg(test)]
mod spice_paste_tests {
    use super::*;

    #[test]
    fn a_pasted_diode_model_fills_the_form() {
        let mut f = PartForm::default();
        f.sim_text = "* vendor\n.MODEL DX D (IS=1n\n+ RS=1)".into();
        f.autofill_from_spice().unwrap();
        assert_eq!((f.sim, f.sim_name.as_str(), f.sim_element.as_str(), f.sim_pins.as_str()), (SimChoice::Model, "DX", "D", "1 2"));
        assert_eq!(f.name, "DX");
        let d = f.to_def().unwrap();
        assert!(!d.has_errors(), "{:?}", d.validate().iter().map(|i| &i.message).collect::<Vec<_>>());
    }

    #[test]
    fn a_transistor_keeps_its_pin_names_in_node_order() {
        let mut f = PartForm::default();
        f.name = "BC".into();
        f.pins_text = "1 E\n2 B\n3 C".into();
        f.pkg = PkgKind::Row;
        f.sim_text = ".model BC NPN(BF=200)".into();
        f.autofill_from_spice().unwrap();
        assert_eq!(f.sim_pins, "3 2 1", "C B E");
        assert_eq!(f.sim_element, "Q");
        assert!(!f.to_def().unwrap().has_errors());
    }

    #[test]
    fn a_subcircuit_makes_one_pin_per_port() {
        let mut f = PartForm::default();
        f.sim_text = ".subckt AMP IN+ IN- OUT VCC GND\nR1 IN+ OUT 1k\n.ends".into();
        f.autofill_from_spice().unwrap();
        assert_eq!(f.sim, SimChoice::Subckt);
        assert_eq!(f.parse_pins().unwrap().len(), 5);
        assert_eq!(f.sim_pins, "1 2 3 4 5");
        let d = f.to_def().unwrap();
        assert!(!d.has_errors(), "{:?}", d.validate().iter().map(|i| &i.message).collect::<Vec<_>>());
    }

    #[test]
    fn bad_text_and_mismatched_pins_are_explained() {
        let mut f = PartForm::default();
        f.sim_text = "R1 1 2 1k".into();
        assert!(f.autofill_from_spice().unwrap_err().contains(".model"));
        f.sim_text = ".model Z TRIAC(X=1)".into();
        assert!(f.autofill_from_spice().unwrap_err().contains("未対応"));
        f.sim_text = ".model BC NPN()".into();
        f.pins_text = "1 X\n2 Y".into();
        assert!(f.autofill_from_spice().unwrap_err().contains("3 本"));
    }
}

#[cfg(test)]
mod extras_tests {
    use super::*;

    fn usb_form() -> PartForm {
        let mut f = PartForm::default();
        f.name = "USB_TEST".into();
        f.prefix = "J".into();
        f.pins_text = "1 VBUS\n2 D-\n3 D+\n4 GND\nSH SHIELD".into();
        f.pkg = PkgKind::Row;
        f.pitch = 2.5;
        f.extras = "脚 SH -6.5 0 2.0 3.0 1.2\n脚 SH 6.5 0 2.0 3.0 1.2\n# a comment\n穴 0 -3 1.0\n穴 0 3 1.0 2.0".into();
        f
    }

    #[test]
    fn shield_legs_and_pegs_join_the_package() {
        let d = usb_form().to_def().unwrap();
        assert!(!d.has_errors(), "{:?}", d.validate().iter().map(|i| &i.message).collect::<Vec<_>>());
        let fp = d.footprint().unwrap();
        // 4 signal pads from the row, 2 shield legs sharing the pin SH
        assert_eq!(fp.pads.len(), 6);
        assert_eq!(fp.pads.iter().filter(|p| p.pin == "SH").count(), 2);
        assert_eq!(fp.holes.len(), 2);
        assert!(fp.holes[1].slot > fp.holes[1].drill, "the second peg hole is a slot");
    }

    /// The data sheet's body and exposed pad go into the footprint: an 8-pin SOIC with a pin 1 dot, a QFN-like
    /// part with a thermal pad, and the mistakes the form refuses.
    #[test]
    fn the_body_and_the_thermal_pad_come_from_the_form() {
        let mut f = PartForm { name: "SOIC8_TEST".into(), pins_text: "1 A
2 B
3 C
4 D
5 E
6 F
7 G
8 H".into(), ..PartForm::default() };
        f.pkg = PkgKind::Dual;
        f.body_on = true;
        let plain = f.to_def().unwrap().footprint().unwrap();
        assert!(!plain.silk.is_empty(), "the body outline is drawn");
        // the thermal pad is one more pad, in the middle, sharing nothing with the row
        f.pins_text.push_str("
9 EP");
        f.thermal_on = true;
        f.thermal_pin = "9".into();
        f.thermal_w = 2.4;
        f.thermal_h = 3.4;
        let d = f.to_def().unwrap();
        assert!(!d.has_errors(), "{:?}", d.validate().iter().map(|i| &i.message).collect::<Vec<_>>());
        let fp = d.footprint().unwrap();
        assert_eq!(fp.pads.len(), 9);
        let ep = fp.pads.iter().find(|p| p.pin == "9").unwrap();
        assert_eq!((ep.pos.x, ep.pos.y), (0.0, 0.0));
        assert_eq!(ep.shape.size(), (2.4, 3.4));
        // the outline stays clear of every pad
        assert!(fp.silk.iter().all(|l| fp.pads.iter().all(|p| {
            let (w, h) = p.shape.size();
            [l.a, l.b].iter().all(|q| (q.x - p.pos.x).abs() > w / 2.0 || (q.y - p.pos.y).abs() > h / 2.0)
        })));
        // a pin that is not in the pin table, a missing pin and a bad body are refused with a message
        f.thermal_pin = "99".into();
        assert!(f.to_def().unwrap_err().contains("ピン表"));
        f.thermal_pin = "".into();
        assert!(f.to_def().unwrap_err().contains("ピン番号"));
        f.thermal_on = false;
        f.body_w = 0.0;
        assert!(f.to_def().unwrap().validate().iter().any(|i| i.error && i.message.contains("幅")));
        f.body_w = 3.9;
        f.body_pin = "Z".into();
        assert!(f.to_def().unwrap().validate().iter().any(|i| i.error && i.message.contains("向きの印")));
        // the definition survives saving as JSON
        f.body_pin = "1".into();
        let d = f.to_def().unwrap();
        let back: PartDef = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(back.body, d.body);
    }

    /// Every starting point of the "kind of part" bar makes a part with no errors once it has a name, with its
    /// own symbol, and the transistor / diode choices change the symbol.
    #[test]
    fn each_kind_of_part_starts_from_a_valid_definition() {
        for name in ["ic", "transistor", "diode", "capacitor", "resistor", "inductor", "connector"] {
            let mut f = PartForm::for_type(name);
            assert!(f.ptype.is_some(), "{name}");
            f.name = format!("TEST_{name}");
            let d = f.to_def().unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!d.has_errors(), "{name}: {:?}", d.validate().iter().map(|i| &i.message).collect::<Vec<_>>());
            assert!(d.footprint().is_ok(), "{name}");
            assert!(!crate::schematic::symbol_lines(&d.symbol()).is_empty(), "{name}: a symbol to draw");
        }
        // a transistor of each kind gets its own pin sides and a standard symbol with the small layout
        for (kind, glyph) in [("npn", "npn"), ("pnp", "pnp"), ("nmos", "nmos"), ("pmos", "pmos")] {
            let mut f = PartForm::for_type("transistor");
            f.set_transistor(kind);
            f.name = "Q_TEST".into();
            let d = f.to_def().unwrap();
            assert_eq!(d.glyph, glyph);
            assert!(!d.has_errors(), "{kind}: {:?}", d.validate().iter().map(|i| &i.message).collect::<Vec<_>>());
            let sym = d.symbol();
            assert_eq!((sym.layout().hw, sym.layout().hh), (0, 0), "{kind}: the small transistor layout");
        }
        // each kind of capacitor: the polarity, the footprint and the name prefix, and the template keeps them
        for (kind, polar, head, fp) in [("ceramic", false, "C_", ""), ("electrolytic", true, "CE_", "CP_Radial_D5_P2.0"), ("tantalum", true, "CT_", "CP_Radial_D5_P2.0"), ("film", false, "CF_", "C_Disc_5.08")] {
            let mut f = PartForm::for_type("capacitor");
            f.set_capacitor(kind);
            assert!(f.name.starts_with(head), "{kind}: {}", f.name);
            assert_eq!(f.fp_preset, fp);
            f.name = format!("{head}100u_TEST");
            f.pas_value = "100u".into();
            f.apply_passive().unwrap();
            let d = f.to_def().unwrap();
            assert!(!d.has_errors(), "{kind}: {:?}", d.validate().iter().map(|i| &i.message).collect::<Vec<_>>());
            assert_eq!(d.glyph == "electrolytic", polar, "{kind}");
            assert_eq!(d.pins.iter().any(|p| p.name == "+"), polar, "{kind}");
            assert!(d.footprint().unwrap().pads.iter().all(|p| (p.drill.is_some()) == !fp.is_empty()), "{kind}: leaded parts are through-hole");
            assert_eq!(d.capacitor_family(), Some(match kind { "ceramic" => "積層セラミック", "electrolytic" => "電解", "tantalum" => "タンタル", _ => "フィルム" }), "{kind}");
        }
        // a leaded resistor and a chip coil
        let mut f = PartForm::for_type("resistor");
        f.set_resistor_or_inductor(PartType::Resistor, "leaded");
        f.name = "R_TEST".into();
        assert_eq!(f.to_def().unwrap().footprint().unwrap().pads.len(), 2);
        // a diode keeps its 2 leads small
        let mut f = PartForm::for_type("diode");
        f.name = "D_TEST".into();
        f.glyph = "zener".into();
        let sym = f.to_def().unwrap().symbol();
        assert_eq!((sym.layout().hw, sym.layout().hh), (0, 1));
    }

    /// The electrolytic capacitor's figures: rated ripple current with its frequency table, and the endurance.
    #[test]
    fn ripple_current_and_life_are_read_from_the_ratings_tab() {
        let mut f = PartForm::for_type("electrolytic");
        f.name = "CE_TEST".into();
        let name_of = |f: &PartForm| CHECKLISTS[f.checklist_for_type().unwrap()].0;
        assert!(name_of(&f).starts_with("電解コンデンサ"), "the checklist follows the kind");
        // the kind at the top settles the checklist; an IC leaves the question open
        let mut t = PartForm::for_type("transistor");
        assert!(name_of(&t).starts_with("トランジスタ"));
        t.set_transistor("nmos");
        assert_eq!(name_of(&t), "MOSFET");
        let mut d = PartForm::for_type("diode");
        assert_eq!(name_of(&d), "ダイオード");
        d.glyph = "zener".into();
        assert!(name_of(&d).starts_with("ツェナー"));
        assert!(PartForm::for_type("ic").checklist_for_type().is_none() && PartForm::default().checklist_for_type().is_none());
        assert_eq!(name_of(&PartForm::for_type("ceramic")), "そのほか");
        f.rt_v = "16".into();
        f.rt_ripple = "540m".into();
        f.rt_ripple_f = "100k".into();
        f.rt_ripple_table = "120 0.55
1k 0.77
10k 0.94
100k 1.0".into();
        f.rt_life_h = "6000".into();
        f.rt_life_t = "105".into();
        let r = f.to_def().unwrap().ratings.unwrap();
        let rip = r.ripple.clone().unwrap();
        assert!((rip.i_rms - 0.54).abs() < 1e-9 && rip.freq_hz == 100e3 && rip.coeffs.len() == 4);
        assert!((rip.allowed_at(120.0) - 0.54 * 0.55).abs() < 1e-9);
        assert_eq!(r.life, Some(sch_core::LifeRating { hours: 6000.0, temp_c: 105.0 }));
        assert!(r.summary().contains("定格リップル電流") && r.summary().contains("寿命"));
        // mistakes are told in words
        f.rt_ripple_table = "120 abc".into();
        assert!(f.to_def().unwrap_err().contains("1行目"));
        f.rt_ripple_table.clear();
        f.rt_ripple_f = "x".into();
        assert!(f.to_def().unwrap_err().contains("周波数"));
        f.rt_ripple_f = "100k".into();
        f.rt_life_t = "hot".into();
        assert!(f.to_def().unwrap_err().contains("温度"));
        // with nothing typed there are no such figures
        let mut g = PartForm::for_type("electrolytic");
        g.name = "CE_X".into();
        assert!(g.to_def().unwrap().ratings.is_none());
    }

    #[test]
    fn a_vertical_plated_slot_and_smd_pad_are_read() {
        let mut f = usb_form();
        f.extras = "脚 SH 6.5 0 1.6 3.6 0.8 2.4 v\nパッド SH -4 -2 1.5 2.0".into();
        let fp = f.to_def().unwrap().footprint().unwrap();
        let slot = fp.pads.iter().find(|p| p.slot > 0.0).unwrap();
        assert!(slot.slot_v && slot.drill == Some(0.8));
        assert!(fp.pads.iter().any(|p| p.drill.is_none() && p.pin == "SH"));
    }

    #[test]
    fn mistakes_name_the_line() {
        let mut f = usb_form();
        f.extras = "脚 SH -6.5 0 2.0 3.0".into();
        assert!(f.to_def().unwrap_err().contains("1行目"));
        f.extras = "ねじ 1 2 3".into();
        assert!(f.to_def().unwrap_err().contains("脚 / 穴 / パッド"));
        f.extras = "脚 SH 0 0 2.0 3.0 1.2 0.5".into();
        let d = f.to_def().unwrap();
        assert!(d.validate().iter().any(|i| i.error && i.message.contains("slot")), "a slot shorter than its width is refused by the package");
    }
}

#[cfg(test)]
mod passive_form_tests {
    use super::*;

    #[test]
    fn a_chip_capacitor_from_data_sheet_numbers() {
        let mut f = PartForm::default();
        f.fp_preset = "C_0603".into();
        f.pas_value = "100n".into();
        f.pas_esr = "20m".into();
        f.pas_srf = "30M".into();
        f.pas_bias = "30%".into();
        let msg = f.apply_passive().unwrap();
        assert!(msg.contains("ESL は自己共振周波数から"), "{msg}");
        assert_eq!((f.name.as_str(), f.prefix.as_str(), f.glyph.as_str(), f.value.as_str()), ("C_100n_0603", "C", "capacitor", "100n"));
        let d = f.to_def().unwrap();
        assert!(!d.has_errors(), "{:?}", d.validate().iter().map(|i| &i.message).collect::<Vec<_>>());
        assert_eq!(d.footprint_name(), "C_0603");
        assert_eq!(d.footprint().unwrap().pads.len(), 2);
        let sim = d.sim.clone().unwrap();
        assert!(sim.text.contains("Resr") && sim.text.contains("Lesl") && sim.text.contains("C1 n_esl 2 70n"), "{}", sim.text);
        assert!(d.symbol().glyph == "capacitor");
    }

    #[test]
    fn the_impedance_table_gives_esr_and_esl() {
        let mut f = PartForm::default();
        f.pas_value = "100n".into();
        f.pas_table = "100k abc".into();
        assert!(f.fit_from_table().unwrap_err().contains("1行目"));
        // |Z| of 100 nF with ESR 30 mΩ and ESL 0.9 nH
        let z = |hz: f64| passive::cap_impedance(100e-9, 0.03, 0.9e-9, hz);
        f.pas_table = [1e5, 1e6, 5e6, 2.4e7, 1e8, 3e8].iter().map(|&hz| format!("{} {:.5}", hz, z(hz))).collect::<Vec<_>>().join("\n");
        let m = f.fit_from_table().unwrap();
        assert!(m.contains("求めました"), "{m}");
        let esr = sim_core::parse_value(&f.pas_esr).unwrap();
        let esl = sim_core::parse_value(&f.pas_esl).unwrap();
        assert!((esr / 0.03 - 1.0).abs() < 0.05 && (esl / 0.9e-9 - 1.0).abs() < 0.05, "{esr} {esl}");
    }

    #[test]
    fn frequencies_read_m_as_mega_and_resistances_as_milli() {
        assert_eq!(parse_freq("30M"), Some(30e6));
        assert_eq!(parse_freq("100kHz"), Some(100e3));
        assert_eq!(parse_freq("1.5MHz"), Some(1.5e6));
        assert_eq!(parse_ohm("20m"), Some(0.02));
        assert_eq!(parse_ohm("0.05Ω"), Some(0.05));
    }

    #[test]
    fn mistakes_in_the_chip_section_are_explained() {
        let mut f = PartForm::default();
        assert!(f.apply_passive().unwrap_err().contains("値を入力"));
        f.pas_value = "100n".into();
        f.pas_esr = "abc".into();
        assert!(f.apply_passive().unwrap_err().contains("ESR"));
        f.pas_esr.clear();
        f.pas_bias = "200".into();
        assert!(f.apply_passive().unwrap_err().contains("0〜95"));
    }
}
