//! Renumbering the parts: new reference designators (R1, R2, ...) for the parts of the sheets, and everything that names them follows
//! (the "not connected" marks, the heat links, the table of requirements).

use crate::*;
use std::collections::{BTreeMap, BTreeSet};

impl Schematic {
    /// Give parts new reference designators. `map`: old -> new, applied all at once (two parts can swap). Parts that are not in the
    /// schematic are left out. On an error nothing is changed. Returns how many parts got another number.
    /// Not for a circuit with sheet symbols (hierarchy): the copies are named from the child sheet's parts, so the numbers are
    /// kept there.
    pub fn rename_parts(&mut self, map: &BTreeMap<String, String>) -> Result<usize, String> {
        if self.sheets_components().any(|c| c.sub.is_some()) {
            return Err("シートの記号(階層)がある回路図は、部品番号の振り直しにまだ対応していません。コピーの部品番号は子シートの部品から決まるためです".into());
        }
        let present: BTreeSet<&str> = self.sheets_components().map(|c| c.refdes.as_str()).collect();
        let map: BTreeMap<String, String> = map.iter().filter(|(a, b)| a != b && present.contains(a.as_str())).map(|(a, b)| (a.clone(), b.clone())).collect();
        if map.is_empty() {
            return Ok(0);
        }
        // the numbers after the change must all be different
        let mut seen = BTreeSet::new();
        for c in self.sheets_components() {
            let n = map.get(&c.refdes).unwrap_or(&c.refdes);
            if !seen.insert(n.clone()) {
                return Err(format!("{n} が2つの部品に付いてしまうので、振り直しを取りやめました"));
            }
        }
        for c in self.sheets_components_mut() {
            if let Some(n) = map.get(&c.refdes) {
                c.refdes = n.clone();
            }
        }
        for (r, _) in &mut self.no_connects {
            if let Some(n) = map.get(r) {
                *r = n.clone();
            }
        }
        for (a, b, _) in &mut self.thermal_links {
            for r in [a, b] {
                if let Some(n) = map.get(r) {
                    *r = n.clone();
                }
            }
        }
        self.requirements.rename_parts(&map);
        Ok(map.len())
    }

    fn sheets_components(&self) -> impl Iterator<Item = &Component> {
        self.components.iter().chain(self.sheets.iter().flat_map(|s| s.components.iter()))
    }

    fn sheets_components_mut(&mut self) -> impl Iterator<Item = &mut Component> {
        self.components.iter_mut().chain(self.sheets.iter_mut().flat_map(|s| s.components.iter_mut()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two() -> Schematic {
        let mut s = Schematic::default();
        s.add(Kind::Resistor, (0, 0), 0);
        s.add(Kind::Resistor, (10, 0), 0);
        assert_eq!((s.components[0].refdes.as_str(), s.components[1].refdes.as_str()), ("R1", "R2"));
        s.no_connects.push(("R1".into(), "1".into()));
        s.thermal_links.push(("R1".into(), "R2".into(), 5.0));
        s.requirements.groups.push(design_spec::PlaceGroup { name: "g".into(), parts: vec!["R1".into(), "R2".into()], ..Default::default() });
        s
    }

    fn map(p: &[(&str, &str)]) -> BTreeMap<String, String> {
        p.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn two_parts_swap_numbers_and_the_marks_and_the_table_follow() {
        let mut s = two();
        assert_eq!(s.rename_parts(&map(&[("R1", "R2"), ("R2", "R1")])), Ok(2));
        assert_eq!(s.components[0].refdes, "R2");
        assert_eq!(s.components[1].refdes, "R1");
        assert_eq!(s.no_connects, vec![("R2".to_string(), "1".to_string())]);
        assert_eq!((s.thermal_links[0].0.as_str(), s.thermal_links[0].1.as_str()), ("R2", "R1"));
        assert_eq!(s.requirements.groups[0].parts, vec!["R2", "R1"]);
    }

    #[test]
    fn a_clash_changes_nothing() {
        let mut s = two();
        let before = s.clone();
        assert!(s.rename_parts(&map(&[("R1", "R2")])).is_err());
        assert_eq!(s, before);
    }

    #[test]
    fn parts_that_are_not_here_are_left_out() {
        let mut s = two();
        assert_eq!(s.rename_parts(&map(&[("R9", "R3"), ("R1", "R1")])), Ok(0));
    }
}
