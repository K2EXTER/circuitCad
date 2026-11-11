//! Writes the measured comparison tables into `crates/part-lib/model_checks.json` (shown next to the part on the screen).

use part_lib::model_checks::{CheckRow, ModelChecks, fingerprint};
use std::collections::BTreeMap;

/// The model text of the bundled part `name`.
pub fn bundled_model(name: &str) -> String {
    part_lib::EXAMPLES
        .iter()
        .filter_map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).ok())
        .find(|d| d.name == name)
        .and_then(|d| d.sim)
        .unwrap_or_else(|| panic!("no bundled model for {name}"))
        .text
}

/// Replaces the tables of the given parts, keeping the others'.
pub fn write(tables: Vec<(String, String, Vec<CheckRow>)>, date: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../part-lib/model_checks.json");
    let mut all: BTreeMap<String, ModelChecks> = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    for (name, source, rows) in tables {
        let fingerprint = fingerprint(&bundled_model(&name));
        all.insert(name, ModelChecks { fingerprint, source, date: date.into(), rows });
    }
    std::fs::write(&path, serde_json::to_string_pretty(&all).unwrap() + "\n").unwrap();
}
