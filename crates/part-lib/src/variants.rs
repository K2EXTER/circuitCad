//! A family of parts that one data sheet orders in several versions (an LDO's output voltages, the packages of each): one base
//! part and, per version, what differs (its part number, value, model and package). Like a series (`series.rs`) it is written
//! once and expanded into ordinary library parts, one per version; unlike a series its versions are listed, not computed, as
//! the data sheet's ordering table lists them. The part form makes one from the voltages and packages it reads
//! (`part_form/reg_fill.rs`), and saves the parts it expands to.

use crate::PartDef;
use pcb_core::package::PackageSpec;
use sch_core::SimModel;
use serde::{Deserialize, Serialize};

/// One version of the family.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Variant {
    /// The maker's part number ("LP5907MFX-3.3").
    pub name: String,
    /// The value shown on the schematic ("3.3V").
    pub value: String,
    /// Its own model (an LDO fitted at its voltage); none keeps the base's.
    #[serde(default)]
    pub sim: Option<SimModel>,
    /// Its own package: the footprint's name and pads; none keeps the base's.
    #[serde(default)]
    pub package: Option<(String, PackageSpec)>,
    /// A few words added to the description ("出力 3.3 V、SOT-23").
    #[serde(default)]
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VariantSeries {
    /// The family's name ("LP5907").
    pub series: String,
    pub base: PartDef,
    pub variants: Vec<Variant>,
}

impl VariantSeries {
    /// One library part per version: the base with the version's name, value, model and package. The description says which
    /// family it is of, so the versions are found together in the library.
    pub fn expand(&self) -> Vec<PartDef> {
        self.variants
            .iter()
            .map(|v| {
                let mut p = self.base.clone();
                p.name = v.name.clone();
                p.value = Some(v.value.clone());
                if let Some(sim) = &v.sim {
                    p.sim = Some(sim.clone());
                }
                if let Some((fp, pkg)) = &v.package {
                    p.footprint_name = Some(fp.clone());
                    p.package = pkg.clone();
                }
                let head = format!("{} の {}", self.series, v.note);
                p.description = if self.base.description.trim().is_empty() { head } else { format!("{head}。{}", self.base.description.trim()) };
                p
            })
            .collect()
    }

    /// The versions' names that are written twice (two versions would overwrite each other in the library).
    pub fn duplicates(&self) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        for (k, v) in self.variants.iter().enumerate() {
            if self.variants[..k].iter().any(|w| w.name == v.name) && !out.contains(&v.name) {
                out.push(v.name.clone());
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_expand_into_parts_of_their_own() {
        let base = crate::series::by_name("ERJ-3EKF").unwrap().part(1000.0);
        let other = PackageSpec::Chip2 { pitch: 2.0, pad_len: 1.0, pad_width: 1.2 };
        let sim = |n: &str| SimModel { kind: sch_core::SimKind::Subckt, element: String::new(), name: n.into(), text: format!(".subckt {n} 1 2\nR1 1 2 1\n.ends"), pins: vec!["1".into(), "2".into()], params: Default::default(), limits: vec![], origin: None };
        let s = VariantSeries {
            series: "FAM".into(),
            base: base.clone(),
            variants: vec![
                Variant { name: "FAM-1".into(), value: "1V".into(), sim: Some(sim("FAM1")), package: None, note: "出力 1 V".into() },
                Variant { name: "FAM-2".into(), value: "2V".into(), sim: None, package: Some(("CHIP".into(), other.clone())), note: "出力 2 V".into() },
            ],
        };
        let parts = s.expand();
        assert_eq!(parts.len(), 2);
        assert_eq!((parts[0].name.as_str(), parts[0].value.as_deref()), ("FAM-1", Some("1V")));
        assert_eq!(parts[0].sim.as_ref().unwrap().name, "FAM1");
        assert_eq!(parts[0].package, base.package);
        assert_eq!(parts[1].sim, base.sim, "the base's model kept");
        assert_eq!((parts[1].package.clone(), parts[1].footprint_name.as_deref()), (other, Some("CHIP")));
        assert!(parts[1].description.starts_with("FAM の 出力 2 V"));
        assert!(s.duplicates().is_empty());
        let twice = VariantSeries { variants: vec![s.variants[0].clone(), s.variants[0].clone()], ..s };
        assert_eq!(twice.duplicates(), vec!["FAM-1"]);
        // it reads back as written
        let json = serde_json::to_string(&twice).unwrap();
        assert_eq!(serde_json::from_str::<VariantSeries>(&json).unwrap(), twice);
    }
}
