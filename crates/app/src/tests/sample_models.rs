//! The samples (`docs/examples/*.schjson`, `*.ccproj`) keep a copy of each library part's model inside the circuit. When a
//! bundled model is rebuilt, the copies stay as they were: `sample_models_match_the_library` lists them, and
//! `cargo test -p app refresh_sample_models -- --ignored` replaces their model text with the library's.
//!
//! Only the model text is replaced, as a string inside the file, so that nothing else in the file changes (the files are
//! written in two key orders, and a rewrite through the types would reorder one of them). The notes (`limits`) of an up to
//! date copy are taken from the library on the screen (`panel/properties.rs`). After a refresh, run the samples' tests,
//! and the stored ngspice answers with `--keep-set` (docs/code-notes/samples.md).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Samples that carry a model of their own on purpose: (file, part, why).
const OWN_MODELS: &[(&str, &str, &str)] = &[
    ("diode_recovery.schjson", "1N4007", "the reverse recovery lesson adds CJO and TT (the library model has no charge storage)"),
    ("diode_recovery.schjson", "UF4007", "the reverse recovery lesson adds CJO and TT"),
    ("diode_recovery.schjson", "1N5819", "the reverse recovery lesson adds CJO"),
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples")
}

/// The bundled models: part name -> (model text, model pins).
fn library() -> BTreeMap<String, (String, Vec<String>)> {
    part_lib::EXAMPLES
        .iter()
        .filter_map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).ok())
        .filter_map(|d| d.sim.map(|m| (d.name, (m.text, m.pins))))
        .collect()
}

fn samples() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(root()).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "schjson" || x == "ccproj")).collect();
    v.sort();
    v
}

/// One placed library part whose model differs from the library's.
struct Stale {
    file: String,
    part: String,
    text: String,
    /// The model pins differ too: replacing the text alone would wire the model wrongly.
    pins_differ: bool,
}

/// Every placed part (on any sheet) with the name of a library part and another model text, leaving out `OWN_MODELS`.
fn stale_copies() -> Vec<Stale> {
    fn walk(v: &serde_json::Value, out: &mut Vec<(String, String, Vec<String>)>) {
        match v {
            serde_json::Value::Object(m) => {
                if let Some(c) = m.get("custom")
                    && let (Some(name), Some(text)) = (c["name"].as_str(), c["sim"]["text"].as_str())
                {
                    let pins = c["sim"]["pins"].as_array().map(|a| a.iter().filter_map(|p| p.as_str().map(String::from)).collect()).unwrap_or_default();
                    out.push((name.to_string(), text.to_string(), pins));
                }
                m.values().for_each(|x| walk(x, out));
            }
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    let lib = library();
    let mut stale = vec![];
    for f in samples() {
        let file = f.file_name().unwrap().to_string_lossy().to_string();
        let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        let mut copies = vec![];
        walk(&json, &mut copies);
        for (part, text, pins) in copies {
            let Some((now, now_pins)) = lib.get(&part) else { continue };
            let own = OWN_MODELS.iter().any(|(f, p, _)| *f == file && *p == part);
            if !own && text != *now && !stale.iter().any(|s: &Stale| s.file == file && s.part == part && s.text == text) {
                stale.push(Stale { file: file.clone(), part, text, pins_differ: pins != *now_pins });
            }
        }
    }
    stale
}

#[test]
fn sample_models_match_the_library() {
    let stale = stale_copies();
    let list: Vec<String> = stale.iter().map(|s| format!("{}: {}{}", s.file, s.part, if s.pins_differ { " (the model's pins differ too)" } else { "" })).collect();
    assert!(
        stale.is_empty(),
        "these samples carry an older model than the library part (cargo test -p app refresh_sample_models -- --ignored, \
         or list a deliberate variant in OWN_MODELS):\n{}",
        list.join("\n")
    );
}

/// Every sample that names a part in `OWN_MODELS` still has that part with its own model, so the list does not go stale.
#[test]
fn own_models_are_still_their_own() {
    let lib = library();
    for (file, part, _) in OWN_MODELS {
        let text = std::fs::read_to_string(root().join(file)).unwrap();
        let copy = serde_json::from_str::<sch_core::Schematic>(&text).unwrap().components.into_iter().find_map(|c| c.custom.filter(|s| s.name == *part).and_then(|s| s.sim)).unwrap();
        assert_ne!(copy.text, lib[*part].0, "{file}: {part} has the library model now; take it out of OWN_MODELS");
    }
}

#[test]
#[ignore]
fn refresh_sample_models() {
    let lib = library();
    let mut done = vec![];
    for s in stale_copies() {
        if s.pins_differ {
            println!("{}: {} left as it is: the library model has other pins (fix it by hand)", s.file, s.part);
            continue;
        }
        let path = root().join(&s.file);
        let text = std::fs::read_to_string(&path).unwrap();
        // the string exactly as serde_json wrote it into the file
        let (old, new) = (serde_json::to_string(&s.text).unwrap(), serde_json::to_string(&lib[&s.part].0).unwrap());
        let n = text.matches(&old).count();
        assert!(n > 0, "{}: the model of {} is not found as written", s.file, s.part);
        std::fs::write(&path, text.replace(&old, &new)).unwrap();
        done.push(format!("{}: {} ({n})", s.file, s.part));
    }
    println!("replaced:\n{}", done.join("\n"));
    assert!(stale_copies().iter().all(|s| s.pins_differ), "every copy with the same pins is now the library's");
}
