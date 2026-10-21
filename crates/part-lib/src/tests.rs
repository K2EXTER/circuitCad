use super::*;

fn example(name: &str) -> PartDef {
    let text = EXAMPLES.iter().find(|(n, _)| *n == name).unwrap().1;
    serde_json::from_str(text).unwrap()
}

#[test]
fn bundled_examples_are_valid() {
    for (n, text) in EXAMPLES {
        let p: PartDef = serde_json::from_str(text).unwrap_or_else(|e| panic!("{n}: {e}"));
        let errors: Vec<_> = p.validate().into_iter().filter(|i| i.error).collect();
        assert!(errors.is_empty(), "{n}: {errors:?}");
    }
}

#[test]
fn lm358_example_warns_about_unused_pads_only() {
    let issues = example("LM358_SOIC-8").validate();
    assert!(issues.iter().all(|i| !i.error));
    assert_eq!(issues.iter().filter(|i| !i.error).count(), 3); // pads 5,6,7 belong to channel 2
}

#[test]
fn symbol_and_footprint_follow_definition() {
    let p = example("NE555_DIP-8");
    let sym = p.symbol();
    assert_eq!(sym.pins.len(), 8);
    assert_eq!(sym.pins[0].side, PinSide::Left);
    let fp = p.footprint().unwrap();
    assert_eq!(fp.name, "DIP-8_W7.62");
    assert_eq!(fp.pads.len(), 8);
}

#[test]
fn default_sides_split_in_half() {
    let mut p = example("NE555_DIP-8");
    for pin in &mut p.pins {
        pin.side = None;
    }
    let sym = p.symbol();
    assert_eq!(sym.pins.iter().filter(|s| s.side == PinSide::Left).count(), 4);
    assert_eq!(sym.pins.iter().filter(|s| s.side == PinSide::Right).count(), 4);
}

#[test]
fn validation_catches_datasheet_transcription_errors() {
    let mut p = example("NE555_DIP-8");
    p.pins[0].number = "9".into(); // no pad 9
    p.pins[1].number = "9".into(); // and a duplicate
    let msgs: Vec<_> = p.validate().into_iter().filter(|i| i.error).map(|i| i.message).collect();
    assert!(msgs.iter().any(|m| m.contains("重複")));
    assert!(msgs.iter().any(|m| m.contains("対応するパッドがありません") || m.contains("パッケージにありません")));

    let mut q = example("LM358_SOIC-8");
    q.sim.as_mut().unwrap().pins.pop();
    assert!(q.validate().iter().any(|i| i.error && i.message.contains("ポート数")));
    q.sim.as_mut().unwrap().pins.push("7".into()); // pin 7 is not on the symbol
    assert!(q.validate().iter().any(|i| i.error && i.message.contains("記号のピンにありません")));

    let mut d = example("1N4148");
    d.sim.as_mut().unwrap().name = "OTHER".into();
    assert!(d.validate().iter().any(|i| i.error && i.message.contains(".model")));
}

#[test]
fn bad_package_is_an_error() {
    let mut p = example("NE555_DIP-8");
    p.package = PackageSpec::Dual { pins: 7, pitch: 2.54, row_spacing: 7.62, pad_len: 1.6, pad_width: 1.6, drill: Some(0.8) };
    assert!(p.validate().iter().any(|i| i.error && i.message.starts_with("package")));
}

#[test]
fn save_load_round_trip_and_error_reporting() {
    let dir = std::env::temp_dir().join(format!("circuitcad_lib_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    install_examples(&dir).unwrap();
    let (parts, errors) = load_dir(&dir);
    assert_eq!(parts.len(), EXAMPLES.len());
    assert!(errors.is_empty());

    let mut p = example("1N4148");
    p.name = "My/Diode".into();
    let path = save(&dir, &p).unwrap();
    assert!(path.file_name().unwrap().to_string_lossy().starts_with("My_Diode"));

    std::fs::write(dir.join(format!("broken{FILE_SUFFIX}")), "{ not json").unwrap();
    let (parts, errors) = load_dir(&dir);
    assert_eq!(parts.len(), EXAMPLES.len() + 1);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].starts_with("broken"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn deleted_and_edited_parts_are_left_alone_but_new_ones_arrive() {
    let dir = std::env::temp_dir().join(format!("circuitcad_lib_inst_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    install_examples(&dir).unwrap();

    // the user deletes one part and edits another
    std::fs::remove_file(dir.join(format!("NE555_DIP-8{FILE_SUFFIX}"))).unwrap();
    let mine = dir.join(format!("TL072_SOIC-8{FILE_SUFFIX}"));
    let mut d: PartDef = serde_json::from_str(&std::fs::read_to_string(&mine).unwrap()).unwrap();
    d.description = "mein Bauteil".into();
    std::fs::write(&mine, serde_json::to_string_pretty(&d).unwrap()).unwrap();

    // a later program version ships another part: simulate by forgetting one marker line
    let marker = dir.join(".bundled");
    let kept: Vec<String> = std::fs::read_to_string(&marker).unwrap().lines().filter(|l| *l != "2N7000_TO-92").map(String::from).collect();
    std::fs::write(&marker, kept.join("\n")).unwrap();
    std::fs::remove_file(dir.join(format!("2N7000_TO-92{FILE_SUFFIX}"))).unwrap();

    install_examples(&dir).unwrap();
    assert!(!dir.join(format!("NE555_DIP-8{FILE_SUFFIX}")).exists(), "a deleted part stays deleted");
    assert!(dir.join(format!("2N7000_TO-92{FILE_SUFFIX}")).exists(), "a part that is new to this library is installed");
    let again: PartDef = serde_json::from_str(&std::fs::read_to_string(&mine).unwrap()).unwrap();
    assert_eq!(again.description, "mein Bauteil", "edits survive");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_existing_library_gets_the_new_parts_and_the_old_lm358_is_upgraded() {
    let dir = std::env::temp_dir().join(format!("circuitcad_lib_upg_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // a library from the first release: three parts, no marker file, the old LM358 model
    let mut lm: PartDef = example("LM358_SOIC-8");
    lm.category = String::new();
    lm.sim.as_mut().unwrap().text = LEGACY[0].1.to_string();
    std::fs::write(dir.join(format!("LM358_SOIC-8{FILE_SUFFIX}")), serde_json::to_string_pretty(&lm).unwrap()).unwrap();
    let mut ne = example("NE555_DIP-8");
    ne.description = "my own description".into();
    ne.category = String::new(); // as written by the first release
    std::fs::write(dir.join(format!("NE555_DIP-8{FILE_SUFFIX}")), serde_json::to_string_pretty(&ne).unwrap()).unwrap();
    // an unmodified copy of the first release (no category) is brought up to date
    let mut diode = example("1N4148");
    diode.category = String::new();
    std::fs::write(dir.join(format!("1N4148{FILE_SUFFIX}")), serde_json::to_string_pretty(&diode).unwrap()).unwrap();

    install_examples(&dir).unwrap();
    let (parts, errors) = load_dir(&dir);
    assert!(errors.is_empty());
    assert_eq!(parts.len(), EXAMPLES.len());
    let lm_new = parts.iter().find(|p| p.name == "LM358").unwrap();
    assert!(lm_new.sim.as_ref().unwrap().text.contains("DLIM"), "unmodified old model replaced by the new one");
    assert_eq!(parts.iter().find(|p| p.name == "NE555").unwrap().description, "my own description");
    assert!(!parts.iter().find(|p| p.name == "1N4148").unwrap().category.is_empty(), "unmodified old copy gains its category");
    assert!(parts.iter().find(|p| p.name == "NE555").unwrap().category.is_empty(), "an edited part keeps the user's version");

    // a modified LM358 is never replaced
    let mut custom = lm.clone();
    custom.sim.as_mut().unwrap().text = format!("{}\n* tuned by me", LEGACY[0].1);
    let dir2 = dir.join("second");
    std::fs::create_dir_all(&dir2).unwrap();
    std::fs::write(dir2.join(format!("LM358_SOIC-8{FILE_SUFFIX}")), serde_json::to_string_pretty(&custom).unwrap()).unwrap();
    install_examples(&dir2).unwrap();
    let kept: PartDef = serde_json::from_str(&std::fs::read_to_string(dir2.join(format!("LM358_SOIC-8{FILE_SUFFIX}"))).unwrap()).unwrap();
    assert!(kept.sim.unwrap().text.contains("tuned by me"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bundled_parts_are_well_formed() {
    let mut names = std::collections::HashSet::new();
    for (n, text) in EXAMPLES {
        let p: PartDef = serde_json::from_str(text).unwrap();
        assert!(names.insert(p.name.clone()), "duplicate part {}", p.name);
        assert!(!p.category.is_empty(), "{n} has no category");
        assert!(!p.description.is_empty(), "{n} has no description");
        // every part either has a simulation model or says so
        if p.sim.is_none() {
            assert!(p.description.contains("モデルなし"), "{n}: say that there is no model");
        }
        // sensible footprint: round-trips through the board footprint builder
        let fp = p.footprint().unwrap();
        assert!(fp.pads.iter().all(|pad| pad.shape.size().0 > 0.0));
    }
    assert!(EXAMPLES.len() >= 19);
    for c in ["MOSFET", "オペアンプ", "レギュレータ", "ツェナー", "トランジスタ", "マイコン"] {
        assert!(EXAMPLES.iter().any(|(_, t)| serde_json::from_str::<PartDef>(t).unwrap().category == c), "no part in {c}");
    }
}

#[test]
fn the_tab_of_the_sot223_regulator_shares_pin_2() {
    let p = example("AMS1117-3.3_SOT-223");
    let fp = p.footprint().unwrap();
    assert_eq!(fp.pads.iter().filter(|pad| pad.pin == "2").count(), 2);
    assert!(p.validate().iter().all(|i| !i.error), "{:?}", p.validate());
}

#[test]
fn placed_part_exports_spice_with_its_model() {
    let mut s = sch_core::Schematic::default();
    let d = example("1N4148");
    s.add_custom(d.symbol(), &d.default_value(), &d.footprint_name(), (0, 0), 0);
    let deck = sch_core::to_spice(&s, "t");
    assert!(deck.contains(".model 1N4148 D("), "{deck}");
    assert!(deck.lines().any(|l| l.starts_with("D1 ") && l.ends_with(" 1N4148")), "{deck}");
}

#[test]
fn model_less_original_is_upgraded_once() {
    let dir = std::env::temp_dir().join(format!("circuitcad_upgrade_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // an installed library from before the 555 had a model: the part, marked as installed
    let mut old = example("NE555_DIP-8");
    old.sim = None;
    old.description = "old text".into();
    std::fs::write(dir.join(format!("NE555_DIP-8{FILE_SUFFIX}")), serde_json::to_string_pretty(&old).unwrap()).unwrap();
    std::fs::write(dir.join(".bundled"), "NE555_DIP-8
").unwrap();
    install_examples(&dir).unwrap();
    let (parts, _) = load_dir(&dir);
    assert!(parts.iter().find(|p| p.name == "NE555").unwrap().sim.is_some(), "the model-less original gains its model");
    // the user then deletes the model on purpose: it is not put back
    let mut again = example("NE555_DIP-8");
    again.sim = None;
    std::fs::write(dir.join(format!("NE555_DIP-8{FILE_SUFFIX}")), serde_json::to_string_pretty(&again).unwrap()).unwrap();
    install_examples(&dir).unwrap();
    assert!(load_dir(&dir).0.iter().find(|p| p.name == "NE555").unwrap().sim.is_none());
    // a modified copy (different pins) is never overwritten
    let dir2 = std::env::temp_dir().join(format!("circuitcad_upgrade2_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir2);
    std::fs::create_dir_all(&dir2).unwrap();
    let mut mine = example("NE555_DIP-8");
    mine.sim = None;
    mine.pins.pop();
    std::fs::write(dir2.join(format!("NE555_DIP-8{FILE_SUFFIX}")), serde_json::to_string_pretty(&mine).unwrap()).unwrap();
    std::fs::write(dir2.join(".bundled"), "NE555_DIP-8
").unwrap();
    install_examples(&dir2).unwrap();
    assert!(load_dir(&dir2).0.iter().find(|p| p.name == "NE555").unwrap().sim.is_none());
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&dir2);
}

#[test]
fn every_bundled_model_carries_notes_about_what_it_leaves_out() {
    for (name, text) in EXAMPLES {
        let d: PartDef = serde_json::from_str(text).unwrap();
        if let Some(sim) = &d.sim {
            assert!(sim.limits.len() >= 2, "{name}: only {} notes", sim.limits.len());
            for l in &sim.limits {
                assert!(l.chars().count() > 15 && l.ends_with('。'), "{name}: {l}");
            }
        }
    }
}

#[test]
fn an_installed_copy_from_before_the_notes_gets_them_once() {
    let dir = std::env::temp_dir().join(format!("circuitcad_limits_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    install_examples(&dir).unwrap();
    let path = |n: &str| dir.join(format!("{n}{FILE_SUFFIX}"));
    let read = |n: &str| -> PartDef { serde_json::from_str(&std::fs::read_to_string(path(n)).unwrap()).unwrap() };
    assert!(!read("TP4056_ESOP-8").sim.unwrap().limits.is_empty(), "a fresh install has the notes");

    // an old library: the notes are missing from two parts, and the user edited one of them
    for n in ["TP4056_ESOP-8", "LM358_SOIC-8"] {
        let mut d = read(n);
        d.sim.as_mut().unwrap().limits.clear();
        if n == "LM358_SOIC-8" {
            d.description = "mein LM358".into();
        }
        std::fs::write(path(n), serde_json::to_string_pretty(&d).unwrap()).unwrap();
    }
    let kept: Vec<String> = std::fs::read_to_string(dir.join(".bundled")).unwrap().lines().filter(|l| !l.ends_with("+limits")).map(String::from).collect();
    std::fs::write(dir.join(".bundled"), kept.join("
")).unwrap();
    install_examples(&dir).unwrap();
    assert!(!read("TP4056_ESOP-8").sim.unwrap().limits.is_empty(), "the unmodified copy gains the notes");
    let mine = read("LM358_SOIC-8");
    assert!(mine.sim.unwrap().limits.is_empty() && mine.description == "mein LM358", "an edited copy is left alone");

    // once offered, a user who removes the notes is not given them again
    let mut d = read("TP4056_ESOP-8");
    d.sim.as_mut().unwrap().limits.clear();
    std::fs::write(path("TP4056_ESOP-8"), serde_json::to_string_pretty(&d).unwrap()).unwrap();
    install_examples(&dir).unwrap();
    assert!(read("TP4056_ESOP-8").sim.unwrap().limits.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_installed_copy_from_before_the_ratings_gets_them_once() {
    let dir = std::env::temp_dir().join(format!("circuitcad_ratings_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    install_examples(&dir).unwrap();
    let path = |n: &str| dir.join(format!("{n}{FILE_SUFFIX}"));
    let read = |n: &str| -> PartDef { serde_json::from_str(&std::fs::read_to_string(path(n)).unwrap()).unwrap() };
    assert_eq!(read("1N5819_DO-41").ratings.unwrap().v_max, Some(40.0), "a fresh install has the ratings");

    // an old library: no ratings; the user edited one of the parts
    for n in ["1N5819_DO-41", "2N3904_TO-92"] {
        let mut d = read(n);
        d.ratings = None;
        if n == "2N3904_TO-92" {
            d.description = "mein 2N3904".into();
        }
        std::fs::write(path(n), serde_json::to_string_pretty(&d).unwrap()).unwrap();
    }
    let kept: Vec<String> = std::fs::read_to_string(dir.join(".bundled")).unwrap().lines().filter(|l| !l.ends_with("+ratings")).map(String::from).collect();
    std::fs::write(dir.join(".bundled"), kept.join("
")).unwrap();
    install_examples(&dir).unwrap();
    assert!(read("1N5819_DO-41").ratings.is_some(), "the unmodified copy gains the ratings");
    assert!(read("2N3904_TO-92").ratings.is_none(), "an edited copy is left alone");

    // once offered, a user who removes them is not given them again
    let mut d = read("1N5819_DO-41");
    d.ratings = None;
    std::fs::write(path("1N5819_DO-41"), serde_json::to_string_pretty(&d).unwrap()).unwrap();
    install_examples(&dir).unwrap();
    assert!(read("1N5819_DO-41").ratings.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bundled_ratings_are_sane_and_summarise_in_japanese() {
    let mut n = 0;
    for (name, text) in EXAMPLES {
        let d: PartDef = serde_json::from_str(text).unwrap();
        let Some(r) = &d.ratings else { continue };
        n += 1;
        for v in [r.v_max, r.i_max, r.p_max].into_iter().flatten() {
            assert!(v > 0.0 && v.is_finite(), "{name}");
        }
        assert!(!r.summary().is_empty(), "{name}");
        // the ratings travel with the placed symbol
        assert_eq!(d.symbol().ratings.as_ref(), Some(r), "{name}");
    }
    assert!(n >= 20, "{n} parts carry ratings");
    let d: PartDef = serde_json::from_str(EXAMPLES.iter().find(|(n, _)| n.starts_with("1N5819")).unwrap().1).unwrap();
    let s = d.ratings.unwrap().summary();
    assert!(s.contains("電圧 40V") && s.contains("電流 1A") && s.contains("順方向電圧"), "{s}");
}

/// Regenerates the bundled chip part samples: `cargo test -p part-lib write_chip_examples -- --ignored`.
#[test]
#[ignore]
fn write_chip_examples() {
    use crate::passive::{PassiveKind, PassiveTemplate};
    use pcb_core::package::{PackageSpec, PadSpec};
    use sch_core::PartRatings;
    let note = " 【見本】数値は代表的な例で、実際の部品のデータシートの値ではありません。「新規作成」のチップ部品の欄から、手元の部品の数値でつくり直してください";
    let mut cases: Vec<(String, PassiveTemplate, &str, PartRatings)> = vec![];
    let mut c1 = PassiveTemplate::new(PassiveKind::Capacitor, 100e-9);
    c1.esr = Some(0.03);
    c1.esl = Some(0.6e-9);
    cases.push(("C_100n_0603".into(), c1, "C_0603", PartRatings { v_max: Some(16.0), ..Default::default() }));
    let mut c2 = PassiveTemplate::new(PassiveKind::Capacitor, 10e-6);
    c2.esr = Some(5e-3);
    c2.esl = Some(1e-9);
    c2.bias_loss = 0.4;
    cases.push(("C_10u_0805".into(), c2, "C_0805", PartRatings { v_max: Some(10.0), ..Default::default() }));
    let mut l1 = PassiveTemplate::new(PassiveKind::Inductor, 10e-6);
    l1.dcr = Some(0.15);
    l1.cp = Some(crate::passive::cp_from_srf(10e-6, 40e6).unwrap());
    cases.push(("L_10u_0805".into(), l1, "L_0805", PartRatings { i_max: Some(0.8), ..Default::default() }));
    let mut r1 = PassiveTemplate::new(PassiveKind::Resistor, 10e3);
    r1.esl = Some(1e-9);
    r1.cp = Some(100e-15);
    cases.push(("R_10k_0603".into(), r1, "R_0603", PartRatings { p_max: Some(0.1), ..Default::default() }));
    // an aluminium electrolytic: large ESR/ESL, polarised symbol, radial footprint
    let mut e1 = PassiveTemplate::new(PassiveKind::Capacitor, 100e-6);
    e1.esr = Some(0.3);
    e1.esl = Some(15e-9);
    // ripple current and endurance as an aluminium electrolytic data sheet gives them (representative figures,
    // not those of a real series: a 5 x 11 mm 100 uF part carries about 0.3 A rms at 100 kHz, 105 degrees)
    let ripple = sch_core::RippleRating { i_rms: 0.3, freq_hz: 100e3, coeffs: vec![(120.0, 0.55), (1e3, 0.77), (10e3, 0.94), (100e3, 1.0)] };
    let life = sch_core::LifeRating { hours: 6000.0, temp_c: 105.0 };
    cases.push(("CE_100u_16V".into(), e1, "CP_Radial_D5_P2.0", PartRatings { v_max: Some(16.0), ripple: Some(ripple), life: Some(life), ..Default::default() }));
    // a tantalum (polarised, ESR of about an ohm) and a metallised film capacitor (non-polar, tiny ESR/ESL, leaded)
    let mut t1 = PassiveTemplate::new(PassiveKind::Capacitor, 10e-6);
    t1.esr = Some(1.5);
    t1.esl = Some(2e-9);
    cases.push(("CT_10u_16V".into(), t1, "CP_Radial_D5_P2.0", PartRatings { v_max: Some(16.0), ..Default::default() }));
    let mut f1 = PassiveTemplate::new(PassiveKind::Capacitor, 100e-9);
    f1.esr = Some(0.02);
    f1.esl = Some(8e-9);
    cases.push(("CF_100n_63V".into(), f1, "C_Disc_5.08", PartRatings { v_max: Some(63.0), ..Default::default() }));
    for (name, t, fp, ratings) in cases {
        let g = t.generate(&name).unwrap();
        let built = pcb_core::footprints::get(fp).unwrap();
        // keep the footprint's pad shape and hole (a leaded part must stay through-hole)
        let shape = |p: &pcb_core::footprints::FpPad| {
            let (w, h) = p.shape.size();
            let kind = match p.shape {
                pcb_core::model::PadShape::Circle { .. } => "circle",
                pcb_core::model::PadShape::Oval { .. } => "oval",
                pcb_core::model::PadShape::Rect { .. } => "rect",
            };
            PadSpec { pin: p.pin.clone(), x: p.pos.x, y: p.pos.y, shape: kind.into(), w, h, drill: p.drill, slot: p.slot, slot_v: p.slot_v }
        };
        let def = PartDef {
            name: name.clone(),
            category: t.kind.label().into(),
            description: format!("{}{}{note}", g.description, if name.starts_with("CE_") { " 電解コンデンサ: 極性があります (四角いパッド・記号の + が プラス側)。耐圧の 2 倍ほどの余裕を見て選び、逆向きにつなぐと壊れます。" } else if name.starts_with("CT_") { " タンタルコンデンサ: 極性があります (記号の + が プラス側)。ESR は電解より小さめですが、耐圧の 2 倍以上の余裕を見ないと、ショートして燃えることがあります。" } else if name.starts_with("CF_") { " フィルムコンデンサ: 極性はありません。ESR・ESL が小さく、容量の温度変化や電圧による変化も小さいので、フィルタやタイミング回路に向きます。" } else { "" }),
            datasheet: String::new(),
            prefix: t.kind.prefix().into(),
            value: Some(g.value_text.clone()),
            pins: g.pins.clone(),
            package: PackageSpec::Explicit { pads: built.pads.iter().map(shape).collect(), holes: vec![] },
            footprint_name: Some(fp.into()),
            height: match name.as_str() {
                "CE_100u_16V" => Some(11.0),
                "CT_10u_16V" => Some(7.0),
                "CF_100n_63V" => Some(9.0),
                _ => None,
            },
            sim: Some(sch_core::SimModel {
                kind: sch_core::SimKind::Subckt,
                element: String::new(),
                name: g.sim_name.clone(),
                text: g.sim_text.clone(),
                pins: g.sim_pins.clone(),
                params: Default::default(),
                limits: g.limits.clone(),
            }),
            ratings: Some(ratings),
            glyph: if name.starts_with("CE_") || name.starts_with("CT_") { "electrolytic".into() } else { t.kind.glyph().into() },
            body: None,
        };
        assert!(!def.has_errors(), "{name}: {:?}", def.validate().iter().map(|i| &i.message).collect::<Vec<_>>());
        let path = format!("{}/examples/{name}.part.json", env!("CARGO_MANIFEST_DIR"));
        std::fs::write(path, serde_json::to_string_pretty(&def).unwrap()).unwrap();
    }
}

const KICAD_SYM: &str = r##"(kicad_symbol_lib (version 20231120) (generator "kicad_symbol_editor")
  (symbol "LM358" (pin_names (offset 1.016)) (in_bom yes) (on_board yes)
    (property "Reference" "U" (at 0 0 0))
    (property "Value" "LM358" (at 0 0 0))
    (property "Footprint" "Package_SO:SOIC-8" (at 0 0 0))
    (property "Datasheet" "http://www.ti.com/lit/ds/symlink/lm358.pdf" (at 0 0 0))
    (property "Description" "Dual op amp" (at 0 0 0))
    (symbol "LM358_1_1"
      (pin output line (at 7.62 0 180) (length 2.54) (name "~" (effects)) (number "1" (effects)))
      (pin input line (at -7.62 2.54 0) (length 2.54) (name "-" (effects)) (number "2" (effects)))
      (pin input line (at -7.62 -2.54 0) (length 2.54) (name "+" (effects)) (number "3" (effects))))
    (symbol "LM358_3_1"
      (pin power_in line (at -2.54 -7.62 90) (length 3.81) (name "V-" (effects)) (number "4" (effects)))
      (pin power_in line (at -2.54 7.62 270) (length 3.81) (name "V+" (effects)) (number "8" (effects)))))
  (symbol "R" (property "Reference" "R") (property "Value" "R") (property "Footprint" "")
    (symbol "R_1_1"
      (pin passive line (at 0 3.81 270) (length 1.27) (name "~") (number "1"))
      (pin passive line (at 0 -3.81 90) (length 1.27) (name "~") (number "2"))))
  (symbol "R_Small" (extends "R") (property "Reference" "R") (property "Value" "R_Small"))
  (symbol "PWR_FLAG" (property "Reference" "#FLG") (symbol "PWR_FLAG_1_1" (pin power_out line (at 0 0 90) (length 0) (name "pwr") (number "1")))))"##;

const KICAD_MOD: &str = r#"(footprint "SOIC-8_3.9x4.9mm_P1.27mm" (version 20221018) (descr "SOIC, 8 Pin")
  (pad "1" smd roundrect (at -2.475 -1.905) (size 1.95 0.6) (layers "F.Cu" "F.Paste" "F.Mask"))
  (pad "2" smd roundrect (at -2.475 -0.635) (size 1.95 0.6) (layers "F.Cu"))
  (pad "3" smd roundrect (at -2.475 0.635) (size 1.95 0.6) (layers "F.Cu"))
  (pad "4" smd roundrect (at -2.475 1.905) (size 1.95 0.6) (layers "F.Cu"))
  (pad "8" smd roundrect (at 2.475 -1.905) (size 1.95 0.6) (layers "F.Cu"))
  (pad "" np_thru_hole circle (at 0 0) (size 1 1) (drill 1) (layers "*.Cu"))
  (pad "9" thru_hole oval (at 5 0 90) (size 1.5 3) (drill oval 1 2) (layers "*.Cu")))"#;

#[test]
fn kicad_symbol_pins_and_sides() {
    let syms = kicad::parse_symbols(KICAD_SYM).unwrap();
    let names: Vec<_> = syms.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["LM358", "R", "R_Small"], "power flag is skipped");
    let lm = &syms[0];
    assert_eq!(lm.prefix, "U");
    assert_eq!(lm.units, 2);
    let side = |n: &str| lm.pins.iter().find(|p| p.number == n).unwrap().side.unwrap();
    assert_eq!(side("1"), PinSide::Right);
    assert_eq!(side("2"), PinSide::Left);
    assert_eq!(side("4"), PinSide::Bottom);
    assert_eq!(side("8"), PinSide::Top);
    // left side is listed top to bottom
    let left: Vec<_> = lm.pins.iter().filter(|p| p.side == Some(PinSide::Left)).map(|p| p.number.as_str()).collect();
    assert_eq!(left, ["2", "3"]);
    assert_eq!(syms[2].pins.len(), 2, "extends takes the parent's pins");
}

#[test]
fn kicad_footprint_pads_and_holes() {
    let fp = kicad::parse_footprint(KICAD_MOD).unwrap();
    assert_eq!(fp.pads.len(), 6);
    assert_eq!(fp.holes.len(), 1);
    assert_eq!(fp.pads[0].y, 1.905, "y is flipped (up is positive)");
    let slot = fp.pads.iter().find(|p| p.pin == "9").unwrap();
    assert_eq!((slot.w, slot.h), (3.0, 1.5), "rotated 90 degrees swaps the size");
    assert_eq!((slot.drill, slot.slot, slot.slot_v), (Some(1.0), 2.0, false), "slot turned with the pad");
}

#[test]
fn kicad_part_is_valid_and_keeps_the_footprint() {
    let syms = kicad::parse_symbols(KICAD_SYM).unwrap();
    let fp = kicad::parse_footprint(KICAD_MOD).unwrap();
    let with_fp = kicad::to_part(&syms[0], Some(&fp)).unwrap();
    assert!(!with_fp.has_errors(), "{:?}", with_fp.validate());
    assert_eq!(with_fp.footprint_name(), "SOIC-8_3.9x4.9mm_P1.27mm");
    let tiny = kicad::parse_footprint("(footprint \"X\" (pad \"1\" smd rect (at 0 0) (size 1 1)))").unwrap();
    assert!(kicad::to_part(&syms[0], Some(&tiny)).is_err(), "pins without pads are refused");
    let p = kicad::to_part(&syms[0], None).unwrap();
    assert!(!p.has_errors(), "{:?}", p.validate());
    assert_eq!(p.category, "KiCad");
    let r = kicad::to_part(&syms[1], None).unwrap();
    assert_eq!(r.glyph, "resistor");
    assert!(!r.has_errors(), "{:?}", r.validate());
}

#[test]
fn kicad_rejects_other_files() {
    assert!(kicad::parse_symbols("(footprint \"x\")").is_err());
    assert!(kicad::parse_footprint("(kicad_symbol_lib)").is_err());
    assert!(kicad::parse_sexpr("(a (b").is_err());
}

/// The leaded capacitors are through-hole: their pads carry a hole (they were once generated as surface-mount pads).
#[test]
fn leaded_capacitors_are_through_hole() {
    for name in ["CE_100u_16V", "CT_10u_16V", "CF_100n_63V"] {
        let (_, text) = EXAMPLES.iter().find(|(n, _)| *n == name).unwrap();
        let d: PartDef = serde_json::from_str(text).unwrap();
        let fp = d.footprint().unwrap();
        assert!(fp.pads.iter().all(|p| p.drill.is_some()), "{name}: pads without a hole");
    }
}

/// The bundled electrolytic carries ripple current and endurance, and an installed copy that is still the
/// original gets them once; a copy the user edited is left alone.
#[test]
fn the_bundled_electrolytic_has_ripple_and_life_and_old_copies_get_them_once() {
    let bundled = example("CE_100u_16V");
    let r = bundled.ratings.clone().unwrap();
    assert!(r.ripple.is_some() && r.life.is_some() && r.v_max == Some(16.0));
    let text = serde_json::to_string_pretty(&bundled).unwrap();
    let dir = std::env::temp_dir().join(format!("cc_ripple_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // an installed copy from before: the same part with only the voltage
    let mut old = bundled.clone();
    old.ratings = Some(sch_core::PartRatings { v_max: Some(16.0), ..Default::default() });
    let file = dir.join(format!("CE_100u_16V{}", crate::FILE_SUFFIX));
    std::fs::write(&file, serde_json::to_string_pretty(&old).unwrap()).unwrap();
    std::fs::write(dir.join(".bundled"), "CE_100u_16V").unwrap();
    install_examples(&dir).unwrap();
    let after: PartDef = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(after.ratings.unwrap().ripple, r.ripple, "{text}");
    // an edited copy (another description) stays as the user left it
    let mut edited = old.clone();
    edited.description = "my own words".into();
    std::fs::write(&file, serde_json::to_string_pretty(&edited).unwrap()).unwrap();
    std::fs::write(dir.join(".bundled"), "CE_100u_16V").unwrap();
    install_examples(&dir).unwrap();
    let kept: PartDef = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(kept.description, "my own words");
    assert!(kept.ratings.unwrap().ripple.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The bundled 18650 cells have two pads 2.54 mm apart (wire or connector), and an installed copy that is still the
/// original with the 77 mm battery holder's pads gets them once; an edited copy is left alone.
#[test]
fn the_bundled_18650_has_wire_pads_and_old_holder_copies_get_them_once() {
    let bundled = example("LIION_18650");
    assert_eq!(bundled.footprint_name(), "Battery_Wire_1x2_P2.54");
    let fp = bundled.footprint().unwrap();
    assert_eq!(fp.pads.len(), 2);
    assert!((fp.pads[0].pos.dist(fp.pads[1].pos) - 2.54).abs() < 1e-9);
    let dir = std::env::temp_dir().join(format!("cc_wire_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // an installed copy from before: the same part on the battery holder's pads
    let mut old = bundled.clone();
    old.footprint_name = Some("BatteryHolder_18650".into());
    if let PackageSpec::Explicit { pads, .. } = &mut old.package {
        pads[1].x = 77.0;
    }
    let file = dir.join(format!("LIION_18650{}", crate::FILE_SUFFIX));
    std::fs::write(&file, serde_json::to_string_pretty(&old).unwrap()).unwrap();
    std::fs::write(dir.join(".bundled"), "LIION_18650").unwrap();
    install_examples(&dir).unwrap();
    let after: PartDef = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(after.footprint_name(), "Battery_Wire_1x2_P2.54");
    // an edited copy stays as the user left it
    let mut edited = old.clone();
    edited.description = "my own words".into();
    std::fs::write(&file, serde_json::to_string_pretty(&edited).unwrap()).unwrap();
    std::fs::write(dir.join(".bundled"), "LIION_18650").unwrap();
    install_examples(&dir).unwrap();
    let kept: PartDef = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(kept.footprint_name(), "BatteryHolder_18650");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Reads the KiCad files named by the environment variable CCAD_KICAD_DIR (downloaded from the official KiCad libraries, see
/// docs/library.md) and prints what the importer makes of them. Not part of the normal run:
/// `CCAD_KICAD_DIR=<dir> cargo test -p part-lib import_downloaded_kicad_parts -- --ignored --nocapture`
#[test]
#[ignore]
fn import_downloaded_kicad_parts() {
    let dir = std::path::PathBuf::from(std::env::var("CCAD_KICAD_DIR").expect("CCAD_KICAD_DIR"));
    let read = |n: &str| std::fs::read_to_string(dir.join(n)).unwrap_or_else(|e| panic!("{n}: {e}"));
    let mega = kicad::parse_symbols(&read("ATmega_merged.kicad_sym")).unwrap();
    let s = mega.iter().find(|s| s.name == "ATmega2560-16A").expect("ATmega2560-16A");
    let fp = kicad::parse_footprint(&read("TQFP-100_14x14mm_P0.5mm.kicad_mod")).unwrap();
    println!("ATmega2560: {} pins, footprint {} with {} pads, warnings {:?}", s.pins.len(), fp.name, fp.pads.len(), fp.warnings);
    {
        // the land pattern's measures (what a `quad` spec needs): the pitch, the distance between opposite pad centres, the pad size
        let pad = |n: &str| fp.pads.iter().find(|p| p.pin == n).unwrap();
        let (p1, p2, p26, p76) = (pad("1"), pad("2"), pad("26"), pad("76"));
        println!("  pad 1 ({:.3},{:.3}) {}x{}; pad 2 ({:.3},{:.3}); pad 26 ({:.3},{:.3}) {}x{}; pad 76 ({:.3},{:.3}); shape {}", p1.x, p1.y, p1.w, p1.h, p2.x, p2.y, p26.x, p26.y, p26.w, p26.h, p76.x, p76.y, p1.shape);
    }
    let part = kicad::to_part(s, Some(&fp)).unwrap();
    println!("  first pins: {:?}", part.pins.iter().take(6).map(|p| (p.number.clone(), p.name.clone(), p.side)).collect::<Vec<_>>());
    let ch = kicad::parse_symbols(&read("CH340G.kicad_sym")).unwrap();
    let c = ch.iter().find(|s| s.name == "CH340G").expect("CH340G");
    let fp2 = kicad::parse_footprint(&read("SOIC-16_3.9x9.9mm_P1.27mm.kicad_mod")).unwrap();
    let part2 = kicad::to_part(c, Some(&fp2)).unwrap();
    println!("CH340G: {} pins, footprint {} with {} pads", c.pins.len(), fp2.name, fp2.pads.len());
    {
        let pad = |n: &str| fp2.pads.iter().find(|p| p.pin == n).unwrap();
        let (a, b, c9) = (pad("1"), pad("2"), pad("9"));
        println!("  pad 1 ({:.3},{:.3}) {}x{}; pad 2 ({:.3},{:.3}); pad 9 ({:.3},{:.3}); shape {}", a.x, a.y, a.w, a.h, b.x, b.y, c9.x, c9.y, a.shape);
    }
    println!("  pins: {:?}", part2.pins.iter().map(|p| (p.number.clone(), p.name.clone(), p.side)).collect::<Vec<_>>());
}

/// Writes the two parts of the Arduino Mega board from the same downloaded KiCad files: the pin tables (numbers, names, the side the
/// KiCad symbol puts them on) are used as they are, the packages are written as our own `quad` / `dual` specs from the measures of
/// the land patterns (not the KiCad pads). `CCAD_KICAD_DIR=<dir> cargo test -p part-lib write_kicad_mega_parts -- --ignored`
#[test]
#[ignore]
fn write_kicad_mega_parts() {
    let dir = std::path::PathBuf::from(std::env::var("CCAD_KICAD_DIR").expect("CCAD_KICAD_DIR"));
    let read = |n: &str| std::fs::read_to_string(dir.join(n)).unwrap_or_else(|e| panic!("{n}: {e}"));
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let plain = |n: &str| n.replace("~{", "").replace('}', "");
    let make = |sym: &kicad::KicadSymbol, package_is_quad: bool, name: &str, description: &str, footprint_name: &str, package: serde_json::Value, body: serde_json::Value, height: f64, file: &str| {
        let part = kicad::to_part(sym, None).unwrap();
        let mut v = serde_json::to_value(&part).unwrap();
        v["name"] = name.into();
        v["category"] = "マイコン".into();
        v["description"] = description.into();
        v["footprint_name"] = footprint_name.into();
        v["package"] = package;
        v["body"] = body;
        v["height"] = height.into();
        v["value"] = name.into();
        let pins = v["pins"].as_array_mut().unwrap();
        for p in pins.iter_mut() {
            let n = plain(p["name"].as_str().unwrap());
            p["name"] = n.into();
        }
        // a quad part is drawn like its package: the pins 1.. down the left, then along the bottom, up the right and along the top
        // (the order of the list is the order along a side: top to bottom, left to right)
        if package_is_quad {
            let n = pins.len();
            let q = n / 4;
            let num = |p: &serde_json::Value| p["number"].as_str().unwrap().parse::<usize>().unwrap();
            pins.sort_by_key(|p| {
                let k = num(p) - 1;
                let (side, along) = (k / q, k % q);
                // left: 1..q as they are; bottom: left to right; right: top to bottom is q*3..q*2; top: left to right is n..q*3+1
                (side, if side >= 2 { q - 1 - along } else { along })
            });
            // the order above sorts the right side and the top side backwards: top-to-bottom / left-to-right
            for p in pins.iter_mut() {
                let side = (num(p) - 1) / q;
                p["side"] = ["left", "bottom", "right", "top"][side].into();
            }
        }
        let def: PartDef = serde_json::from_value(v).unwrap();
        // the file is checked by the same validation the app uses when a part is added
        let issues = def.validate();
        assert!(issues.iter().all(|i| !i.error), "{name}: {issues:?}");
        std::fs::write(out.join(file), serde_json::to_string_pretty(&def).unwrap() + "
").unwrap();
        println!("{name}: {} pins written to {file}", def.pins.len());
    };
    let mega = kicad::parse_symbols(&read("ATmega_merged.kicad_sym")).unwrap();
    let s = mega.iter().find(|s| s.name == "ATmega2560-16A").unwrap();
    make(
        s,
        true,
        "ATmega2560-16AU",
        "8ビットAVRマイコン (256KB フラッシュ、8KB SRAM、4KB EEPROM、TQFP-100、0.5mm ピッチ)。Arduino Mega 2560 のマイコン。ピン表は KiCad 公式ライブラリのシンボルから、ランドパターンの寸法 (ピッチ 0.5mm、向かい合うパッドの中心間 15.325mm、パッド 1.475x0.3mm) は同じくフットプリントから確かめて、自分の形式で書いたものです。必ずデータシートで確認してください。シミュレーションモデルなし",
        "TQFP-100_14x14_P0.5",
        serde_json::json!({"type": "quad", "pins": 100, "pitch": 0.5, "span": 15.325, "pad_len": 1.475, "pad_width": 0.3}),
        serde_json::json!({"shape": "rect", "w": 14.0, "h": 14.0, "mark": "dot", "mark_pin": "1"}),
        1.2,
        "ATmega2560-16AU_TQFP-100.part.json",
    );
    let usb = kicad::parse_symbols(&read("CH340G.kicad_sym")).unwrap();
    let c = usb.iter().find(|s| s.name == "CH340G").unwrap();
    make(
        c,
        false,
        "CH340G",
        "USB-シリアル変換 IC (WCH、SOP-16、1.27mm ピッチ)。UD+ / UD- が USB の D+ / D-、TXD / RXD がシリアル、XI / XO に 12MHz の水晶 (両端に 22pF 前後)、V3 は電源が 5V のとき 0.1µF を GND へ、DTR は Arduino の自動リセットに使う。CTS / DSR / RI / DCD / DTR / RTS は、名前の上線 (アクティブ Low) を省いて書いてある。ピン表は KiCad 公式ライブラリのシンボルから、ランドパターンの寸法は同じくフットプリントから確かめて、自分の形式で書いたものです。必ずデータシートで確認してください。シミュレーションモデルなし",
        "SOIC-16_3.9x9.9_P1.27",
        serde_json::json!({"type": "dual", "pins": 16, "pitch": 1.27, "row_spacing": 4.95, "pad_len": 1.95, "pad_width": 0.6}),
        serde_json::json!({"shape": "rect", "w": 3.9, "h": 9.9, "mark": "dot", "mark_pin": "1"}),
        1.75,
        "CH340G_SOIC-16.part.json",
    );
}
