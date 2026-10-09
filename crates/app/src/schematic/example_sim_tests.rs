use super::*;

fn example_schematics() -> Vec<(String, Schematic)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "schjson"))
        .map(|e| {
            let sch: Schematic = serde_json::from_str(&std::fs::read_to_string(e.path()).unwrap()).unwrap();
            (e.file_name().to_string_lossy().into_owned(), sch)
        })
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

/// Every shipped example must simulate: it reaches a DC operating point, or (oscillators and
/// chargers that never settle at DC) runs in the time domain. A model change that breaks a
/// sample is then caught by CI instead of by a learner.
#[test]
fn shipped_examples_simulate() {
    let examples = example_schematics();
    assert!(!examples.is_empty(), "no examples found");
    let mut bad = vec![];
    for (name, sch) in examples {
        let mut e = SchEditor::default();
        e.sch = sch;
        match from_schematic(&e.sch) {
            Err(errs) => bad.push(format!("{name}: 変換できません: {}", errs.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", "))),
            Ok(c) if dc_op(&c).is_ok() => {}
            Ok(_) => {
                e.run_tran();
                match e.tran.as_ref().map(|t| &t.outcome) {
                    Some(Ok(_)) => {}
                    _ => bad.push(format!("{name}: DCも過渡も失敗: {}", e.status)),
                }
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn parameter_sweep_runs_from_the_editor_in_both_modes() {
    let sch = example_schematics().into_iter().find(|(n, _)| n == "rc_lowpass.schjson").unwrap().1;
    let node = extract(&sch).into_iter().find(|n| n.pins.iter().any(|p| p.refdes == "C1") && n.name != "GND").unwrap().name;
    let mut e = SchEditor::default();
    e.sch = sch;
    e.set_ac_settings("10", "1Meg", "10");
    e.set_tran_settings("2m", "10u");
    for ac in [true, false] {
        e.set_param_settings("R1", "100", "10k", "3", &node, ac);
        e.analyze_param();
        assert!(e.status.contains("完了") && e.status.contains("3 本"), "ac={ac}: {}", e.status);
        assert!(e.has_wave());
    }
    // a part that does not exist is explained, not a panic
    e.set_param_settings("R99", "100", "10k", "3", &node, true);
    e.analyze_param();
    assert!(e.status.contains("失敗"), "{}", e.status);
}
