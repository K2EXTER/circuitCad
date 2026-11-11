//! How a bundled equivalent model compares with its data sheet (docs/model-policy.md, section 5).
//!
//! `model_checks.json` is written by the tests that measure the models (`crates/cli/tests/opamp_models.rs`,
//! `regulator_models.rs`): one table per part. A table is shown only for the model text it was measured on
//! (the fingerprint), so an edited model, or an old copy kept in a circuit, never shows a table that is not its own.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// The measured comparison of one part's model with its data sheet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelChecks {
    /// `fingerprint` of the model text the rows were measured on.
    pub fingerprint: String,
    /// The data sheet the rows come from.
    pub source: String,
    /// The day the data sheet's figures were copied (YYYY-MM-DD).
    #[serde(default)]
    pub date: String,
    pub rows: Vec<CheckRow>,
}

/// One figure: the data sheet's value next to what the simulator measures on the model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CheckRow {
    pub item: String,
    /// "min / typ / max" with the unit ("—" where the data sheet gives none).
    pub datasheet: String,
    /// The data sheet's conditions and where in it.
    #[serde(default)]
    pub condition: String,
    /// The model's value, measured under those conditions.
    pub model: String,
    /// Model minus the data sheet's typ (or max).
    pub diff: String,
    /// A test fails when the model leaves the allowed range for this figure (false: only reported).
    #[serde(default)]
    pub tested: bool,
}

const TABLES: &str = include_str!("../model_checks.json");

/// Every table, by part name.
pub fn all() -> &'static BTreeMap<String, ModelChecks> {
    static ALL: OnceLock<BTreeMap<String, ModelChecks>> = OnceLock::new();
    ALL.get_or_init(|| serde_json::from_str(TABLES).unwrap_or_default())
}

/// Short fingerprint of a model text: FNV-1a over its lines with spaces trimmed and blank lines left out, so that a
/// change of line ends or indentation does not count as another model.
pub fn fingerprint(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        for b in line.bytes().chain(std::iter::once(b'\n')) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

/// The table of part `name` when `text` is the model it was measured on.
pub fn for_model(name: &str, text: &str) -> Option<&'static ModelChecks> {
    all().get(name).filter(|c| c.fingerprint == fingerprint(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_read() {
        assert!(serde_json::from_str::<BTreeMap<String, ModelChecks>>(TABLES).is_ok());
    }

    #[test]
    fn fingerprint_ignores_line_ends_and_indentation() {
        assert_eq!(fingerprint("a\r\n  b \n\n"), fingerprint("a\nb"));
        assert_ne!(fingerprint("a\nb"), fingerprint("a\nc"));
    }

    /// Every bundled part whose model is marked as an equivalent model with a table has the table of its current
    /// model: a rebuilt model whose table was not written again fails here.
    #[test]
    fn tables_match_the_bundled_models() {
        let mut stale = vec![];
        for (file, text) in crate::EXAMPLES {
            let Ok(def) = serde_json::from_str::<crate::PartDef>(text) else { continue };
            let Some(sim) = &def.sim else { continue };
            if let Some(c) = all().get(&def.name)
                && c.fingerprint != fingerprint(&sim.text)
            {
                stale.push(format!("{file}: the table in model_checks.json was measured on another model"));
            }
        }
        assert!(stale.is_empty(), "write the tables again (cargo test -p cli --test opamp_models / regulator_models -- --ignored write_):\n{}", stale.join("\n"));
    }
}
