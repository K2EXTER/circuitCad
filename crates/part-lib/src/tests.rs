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
    assert_eq!(parts.len(), 3);
    assert!(errors.is_empty());

    let mut p = example("1N4148");
    p.name = "My/Diode".into();
    let path = save(&dir, &p).unwrap();
    assert!(path.file_name().unwrap().to_string_lossy().starts_with("My_Diode"));

    std::fs::write(dir.join(format!("broken{FILE_SUFFIX}")), "{ not json").unwrap();
    let (parts, errors) = load_dir(&dir);
    assert_eq!(parts.len(), 4);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].starts_with("broken"));

    // examples are only installed into an empty library
    install_examples(&dir).unwrap();
    assert_eq!(load_dir(&dir).0.len(), 4);
    let _ = std::fs::remove_dir_all(&dir);
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
