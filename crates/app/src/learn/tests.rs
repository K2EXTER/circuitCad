#[test]
fn the_next_steps_are_filled_in_and_not_repeated() {
    assert!(NEXT_STEPS.len() >= 6);
    for s in NEXT_STEPS {
        assert!(!s.title.is_empty() && !s.about.is_empty() && !s.where_.is_empty(), "{}", s.title);
    }
    let mut titles: Vec<&str> = NEXT_STEPS.iter().map(|s| s.title).collect();
    titles.sort_unstable();
    titles.dedup();
    assert_eq!(titles.len(), NEXT_STEPS.len());
}

use super::*;
use super::paths::*;

fn temp_file(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("circuitcad-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d.join("progress.json")
}

#[test]
fn progress_is_saved_and_read_back() {
    let f = temp_file("progress");
    let mut p = Progress::load(&f);
    assert_eq!(p.count(), 0, "a missing file is an empty record");
    assert!(p.mark("divider") && p.mark("led"));
    assert!(!p.mark("divider"), "marking again is not new");
    assert!(p.error.is_none(), "{:?}", p.error);
    let q = Progress::load(&f);
    assert!(q.is_done("divider") && q.is_done("led") && !q.is_done("buck") && q.count() == 2);
    let mut q = q;
    q.reset();
    assert_eq!(Progress::load(&f).count(), 0);
    let _ = std::fs::remove_dir_all(f.parent().unwrap());
}

#[test]
fn welcome_is_shown_once_and_not_to_learners_with_progress() {
    let f = temp_file("welcome");
    let mut p = Progress::load(&f);
    assert!(p.needs_welcome(), "a fresh start shows the card");
    p.mark_welcomed();
    assert!(!Progress::load(&f).needs_welcome(), "answered once: never again");
    let mut q = Progress::load(&f);
    q.reset();
    assert!(!Progress::load(&f).needs_welcome(), "reset keeps the answer");
    let g = temp_file("welcome-old");
    let mut old = Progress::load(&g);
    old.mark("divider");
    let mut old = Progress::load(&g);
    assert!(!old.needs_welcome(), "someone with finished lessons is not welcomed again");
    old.reset();
    let _ = std::fs::remove_dir_all(f.parent().unwrap());
    let _ = std::fs::remove_dir_all(g.parent().unwrap());
}

#[test]
fn a_broken_record_file_is_treated_as_empty() {
    let f = temp_file("broken");
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(&f, "{ not json").unwrap();
    assert_eq!(Progress::load(&f).count(), 0);
    let _ = std::fs::remove_dir_all(f.parent().unwrap());
}

#[test]
fn every_lesson_of_a_path_exists_and_ids_are_unique_in_a_path() {
    let known = crate::schematic::lesson_ids();
    // an item that several paths share is the same item in each (the record is kept by its id)
    let mut seen: std::collections::HashMap<&str, (&str, Kind)> = std::collections::HashMap::new();
    for path in PATHS {
        let mut in_path = std::collections::HashSet::new();
        for item in items(path) {
            assert!(in_path.insert(item.id), "duplicate id {} in {}", item.id, path.title);
            let same = seen.entry(item.id).or_insert((item.title, item.kind));
            assert_eq!(*same, (item.title, item.kind), "{} is not the same item in every path", item.id);
            if let Kind::Lesson(id) = item.kind {
                assert!(known.contains(&id), "{} refers to the missing lesson {id}", item.title);
                assert_eq!(item.id, id);
            }
            if item.kind == Kind::Hands {
                assert!(crate::bench_guide::guide(item.id).is_some(), "{} has no hands-on guide", item.title);
            }
        }
    }
}

/// An item that two paths share has the prerequisites of the path it is shown in: the buck converter needs the 78xx and heat lessons in the
/// power path, but the MOSFET and the flyback diode in the switching path, and sits deeper there.
#[test]
fn a_shared_item_has_the_prerequisites_of_each_path() {
    let power = items(&POWER_PATH).find(|i| i.id == "buck").unwrap();
    let switching = items(&SWITCH_PATH).find(|i| i.id == "buck").unwrap();
    let ids = |path: &LearningPath, item: &Item| requirements(path, item).iter().map(|i| i.id).collect::<Vec<_>>();
    assert_eq!(ids(&POWER_PATH, power), ["regulator_78xx", "heat"]);
    assert_eq!(ids(&SWITCH_PATH, switching), ["mosfet", "flyback"]);
    // the board lesson of copper widths follows the buck converter in the switching path, so it is the deepest there
    let copper = items(&SWITCH_PATH).find(|i| i.id == "board_copper").unwrap();
    assert!(level(&SWITCH_PATH, copper, 0) > level(&SWITCH_PATH, switching, 0));
    assert_eq!(level(&SWITCH_PATH, items(&SWITCH_PATH).next().unwrap(), 0), 0);
}

/// Lessons that were reachable only from the menu are in a path now.
#[test]
fn the_lessons_that_had_no_path_are_in_one() {
    for id in ["mosfet", "pnp_highside", "tp4056", "rc_lowpass", "rlc", "ne555"] {
        assert!(find_item(id).is_some(), "the lesson {id} is in no learning path");
    }
}

/// An item that offers "作ってみよう" must have a challenge behind it.
#[test]
fn items_with_a_build_button_have_a_challenge() {
    let mut with = 0;
    for item in items(&PATHS[0]).filter(|i| i.build) {
        with += 1;
        assert!(crate::schematic::challenge_of_lesson(item.id).is_some() || crate::schematic::challenge_exists(item.id), "{} has no challenge", item.title);
    }
    assert!(with >= 5, "{with} items offer a challenge");
}

/// Every `requires` names an existing item, nothing needs itself through a loop, and the tree has several roots.
#[test]
fn the_prerequisites_of_every_path_exist_and_never_loop() {
    for path in PATHS {
        for item in items(path) {
            for id in item.requires.unwrap_or_default() {
                assert!(find_item(id).is_some(), "{} needs the missing item {id}", item.title);
                assert_ne!(*id, item.id, "{} needs itself", item.title);
            }
            // with no loop the depth is below the number of items; a loop would run into the guard
            assert!(level(path, item, 0) < items(path).count(), "{} is too deep (a loop?)", item.title);
        }
    }
}

#[test]
fn a_tree_path_recommends_several_independent_items_and_waits_for_all_prerequisites() {
    let mut progress = Progress::default();
    // planned items are never recommended, and a planned prerequisite holds nothing back. At the start only the root can
    // be done: the data sheet lesson (the others need it, or the divider of the power path)
    let ready = |p: &Progress| recommended(&ARDUINO_PATH, p).iter().map(|i| i.id).collect::<Vec<_>>();
    assert_eq!(ready(&progress), ["datasheet"]);
    assert!(recommended(&ARDUINO_PATH, &progress).iter().all(|i| i.kind != Kind::Planned));
    // the divider (a lesson of the power path) opens the two that need only it; the pull-up opens the reset; the data
    // sheet opens the crystal and the pin LED (which also needs the LED lesson of the power path)
    progress.mark("divider");
    assert!(ready(&progress).contains(&"pullup") && ready(&progress).contains(&"decoupling"));
    progress.mark("pullup");
    assert!(ready(&progress).contains(&"reset") && ready(&progress).contains(&"levels"));
    assert!(!ready(&progress).contains(&"crystal"));
    progress.mark("datasheet");
    assert!(ready(&progress).contains(&"crystal"));
    assert!(!ready(&progress).contains(&"pin_led"), "the pin LED also needs the LED lesson");
    progress.mark("led");
    assert!(ready(&progress).contains(&"pin_led"));
    let mut progress = Progress::default();
    assert!(is_tree(&ARDUINO_PATH) && is_tree(&POWER_PATH));
    // a straight line (items without `requires`) needs just the item before it: a two-item test path that reuses real ids
    const LINE: LearningPath = LearningPath {
        id: "test_line",
        title: "",
        goal: "",
        stages: &[Stage { name: "", about: "", items: &[lesson("divider", "", "", None), lesson("led", "", "", None)] }],
    };
    assert!(!is_tree(&LINE));
    let led_in_line = items(&LINE).nth(1).unwrap();
    assert_eq!(prerequisites(&LINE, led_in_line).iter().map(|i| i.id).collect::<Vec<_>>(), ["divider"], "a straight line needs the item before it");
    assert_eq!(recommended(&LINE, &progress).iter().map(|i| i.id).collect::<Vec<_>>(), ["divider"]);
    progress.mark("divider");
    assert_eq!(recommended(&LINE, &progress).iter().map(|i| i.id).collect::<Vec<_>>(), ["led"]);
    // the Arduino items name power-path items from another path
    let pin_led = find_item("pin_led").unwrap();
    assert!(requirements(&ARDUINO_PATH, pin_led).iter().any(|i| i.id == "led"));
    // roots sit on row 0, the final tasks below everything they need
    assert_eq!(level(&ARDUINO_PATH, find_item("datasheet").unwrap(), 0), 0);
    assert!(level(&ARDUINO_PATH, find_item("draw_arduino").unwrap(), 0) > level(&ARDUINO_PATH, find_item("reset").unwrap(), 0));
}

#[test]
fn the_mega_path_builds_on_the_arduino_path_and_opens_after_its_prerequisites() {
    let mega = PATHS.iter().find(|p| p.id == "arduino_mega").unwrap();
    assert!(is_tree(mega));
    let mut progress = Progress::default();
    // the ADC lesson needs the data sheet and the pin LED (which need the power path's LED lesson)
    assert!(recommended(mega, &progress).is_empty());
    for id in ["divider", "led", "datasheet", "pin_led"] {
        progress.mark(id);
    }
    assert_eq!(recommended(mega, &progress).iter().map(|i| i.id).collect::<Vec<_>>(), ["adc_input"]);
    // every item is written now (a planned one would hold nothing back and is never recommended)
    assert!(items(mega).all(|i| i.kind != Kind::Planned));
    assert!(requirements(mega, find_item("ch340g").unwrap()).iter().any(|i| i.id == "serial"));
}

#[test]
fn a_new_learner_starts_on_the_first_lesson_about_voltage_and_current() {
    let progress = Progress::default();
    let first = next_item(first_path(), &progress).unwrap();
    assert_eq!(first.id, "intro_ohm");
    assert!(item_action(first).is_some(), "the welcome card can start it");
    // once it is done the next one is the second introduction, and only then the resistor
    let mut progress = Progress::default();
    progress.mark("intro_ohm");
    assert_eq!(next_item(first_path(), &progress).unwrap().id, "intro_read");
    progress.mark("intro_read");
    assert!(recommended(first_path(), &progress).iter().any(|i| i.id == "resistor"));
}

#[test]
fn the_power_supply_path_walks_from_basics_to_building() {
    let p = &PATHS[0];
    assert_eq!(p.stages.len(), 4);
    let mut progress = Progress::default();
    let (done, available, all) = summary(p, &progress);
    assert_eq!(done, 0);
    assert!(available >= 12 && all >= available);
    // a tree: the two roots (the divider and the LED) can both be started; what finishing one opens depends on the branch
    let ready = |p: &Progress| recommended(&PATHS[0], p).iter().map(|i| i.id).collect::<Vec<_>>();
    assert_eq!(ready(&progress), ["divider", "led"]);
    assert_eq!(next_item(p, &progress).unwrap().id, "divider");
    progress.mark("divider");
    assert_eq!(ready(&progress), ["led", "coin_cell", "opamp_noninv"], "the divider opens the battery and the amplifier");
    progress.mark("led");
    assert!(ready(&progress).contains(&"diode_curve") && ready(&progress).contains(&"bjt_switch"), "{:?}", ready(&progress));
    // a lesson whose prerequisites are not all done is not recommended: the follower needs both the zener and the switch
    progress.mark("diode_curve");
    progress.mark("zener");
    assert!(!ready(&progress).contains(&"emitter_follower"));
    progress.mark("bjt_switch");
    assert!(ready(&progress).contains(&"emitter_follower"));
    // the last steps wait for the branches they join
    assert!(!ready(&progress).contains(&"draw_psu") && !ready(&progress).contains(&"build_psu"));
    // planned items are never recommended, however far the learner is
    for it in items(p).filter(|i| i.kind != Kind::Planned) {
        let key = if it.kind == Kind::Build { crate::schematic::challenge_progress_key(it.id) } else { it.id.to_string() };
        progress.mark(&key);
    }
    assert!(next_item(p, &progress).is_none());
}
