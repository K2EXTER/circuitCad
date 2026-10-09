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
