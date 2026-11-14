//! How a bundled equivalent model compares with its data sheet (docs/model-policy.md, section 5).
//!
//! `model_checks.json` is written by the tests that measure the models (`crates/cli/tests/integration/opamp_models.rs`,
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
    /// The data sheet's value the model is compared with: the typ, or "max 1.5 V" when only the max is given.
    #[serde(default)]
    pub target: String,
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

/// Where a bundled model comes from (`sim.origin`), for a copy of it that has no origin of its own: a part placed in a
/// circuit or installed in a library before the field existed. Only when `text` is the bundled model (same fingerprint).
pub fn bundled_origin(name: &str, text: &str) -> Option<&'static sch_core::ModelOrigin> {
    static ALL: OnceLock<BTreeMap<String, (String, sch_core::ModelOrigin)>> = OnceLock::new();
    let all = ALL.get_or_init(|| {
        crate::EXAMPLES
            .iter()
            .filter_map(|(_, t)| serde_json::from_str::<crate::PartDef>(t).ok())
            .filter_map(|d| d.sim.and_then(|m| m.origin.map(|o| (d.name, (fingerprint(&m.text), o)))))
            .collect()
    });
    all.get(name).filter(|(f, _)| *f == fingerprint(text)).map(|(_, o)| o)
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

    /// Bundled models whose source was never recorded (SPICE parameters of diodes, transistors and MOSFETs): they show
    /// "記録なし" and are the next to be rebuilt from their data sheets (docs/model-policy.md). Every other bundled model
    /// says where it comes from.
    const UNRECORDED: &[&str] = &[
        "1N4728A", "1N4733A", "1N4742A", "1N5408", "1N5822", "BAT54", "BC547", "BD139", "BD140", "BPW34", "BSS138", "IRF540N", "IRF9540N", "IRFZ44N", "IRLZ44N", "PC817", "PN2222A", "S8050",
        "S8550", "SS14", "TIP120", "TIP125", "UF4007",
    ];

    #[test]
    fn every_bundled_model_says_where_it_comes_from_or_is_listed_as_unrecorded() {
        let mut wrong = vec![];
        for (file, text) in crate::EXAMPLES {
            let Ok(def) = serde_json::from_str::<crate::PartDef>(text) else { continue };
            let Some(sim) = &def.sim else { continue };
            match (sim.origin.is_some(), UNRECORDED.contains(&def.name.as_str())) {
                (false, false) => wrong.push(format!("{file}: no sim.origin (say where the model comes from, or list it in UNRECORDED)")),
                (true, true) => wrong.push(format!("{file}: has an origin now; take it out of UNRECORDED")),
                _ => {}
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    #[test]
    fn a_copy_without_an_origin_takes_the_bundled_parts_when_it_is_the_same_model() {
        let text = crate::EXAMPLES.iter().find(|(n, _)| *n == "NE555_DIP-8").unwrap().1;
        let m = serde_json::from_str::<crate::PartDef>(text).unwrap().sim.unwrap();
        assert_eq!(bundled_origin("NE555", &m.text).map(|o| o.kind), Some(sch_core::OriginKind::Generic));
        assert!(bundled_origin("NE555", &format!("{}\n* mine", m.text)).is_none(), "an edited model is not the bundled one");
        assert!(bundled_origin("2N3904", ".model 2N3904 NPN(IS=6.7f BF=300 BR=7.5 RB=10)").is_none(), "no origin recorded");
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
        assert!(stale.is_empty(), "write the tables again (cargo test -p cli --test integration -- --ignored write_):\n{}", stale.join("\n"));
    }
}
