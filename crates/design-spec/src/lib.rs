//! The design's requirements, kept with the schematic (one table for the whole design, not one per sheet): what the nets are for
//! (role, width, clearance, current), which parts belong close together, and what must keep its distance. The board import takes them
//! over (net classes, currents, placement groups, distances to check). They are written as the person who designs the board would
//! say them, so the same sentences can be read back from the file.

use serde::{Deserialize, Serialize};

/// The words a net's role can have (the board's `Role`, by name); empty = none.
pub const ROLES: [&str; 5] = ["power", "switching", "sense", "analog", "logic"];

/// A rule for a set of nets: what they are for and how they are routed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NetRule {
    pub name: String,
    /// The net names (the labels and the power symbols), a name that ends in `*` stands for all that begin so (`GN*`).
    pub nets: Vec<String>,
    /// One of `ROLES`, or empty.
    pub role: String,
    /// Track width (mm); 0 = the board's default.
    pub width: f64,
    /// Distance (mm) other copper keeps from these nets; 0 = the board's default.
    pub clearance: f64,
    /// Current (A) the nets carry; 0 = not stated.
    pub current: f64,
    /// Why (shown beside the rule, for the person who reads it later).
    pub note: String,
}

/// Parts that belong close together (a protection next to the connector, a loop that has to be small).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaceGroup {
    pub name: String,
    pub parts: Vec<String>,
    /// How far (mm) apart the farthest two parts of the group may be (the centres); 0 = as close as they go.
    pub max_dist: f64,
    pub note: String,
}

/// Two sets that keep their distance (a switching node from the reference and the sensing lines).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeepApart {
    pub name: String,
    /// Part reference designators or net names (a name that is a part is that part, otherwise a net: all its pads); `*` as in `NetRule`.
    pub a: Vec<String>,
    pub b: Vec<String>,
    /// The least distance (mm) between any pad of `a` and any pad of `b`.
    pub min_dist: f64,
    pub note: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DesignSpec {
    pub net_rules: Vec<NetRule>,
    pub groups: Vec<PlaceGroup>,
    pub apart: Vec<KeepApart>,
}

impl DesignSpec {
    pub fn is_empty(&self) -> bool {
        self.net_rules.is_empty() && self.groups.is_empty() && self.apart.is_empty()
    }

    /// Check the table against the names that exist: the parts and the nets a row mentions but the design does not have.
    /// Returns one sentence for each (a row, a name) that matches nothing.
    pub fn unknown_names(&self, parts: &[String], nets: &[String]) -> Vec<String> {
        let net_known = |n: &str| nets.iter().any(|x| matches_name(n, x));
        let part_known = |n: &str| parts.iter().any(|x| x == n);
        let mut out = vec![];
        for r in &self.net_rules {
            for n in r.nets.iter().filter(|n| !net_known(n)) {
                out.push(format!("ネットの役割「{}」の `{n}` というネットは、回路にありません", r.name));
            }
            if !r.role.is_empty() && !ROLES.contains(&r.role.as_str()) {
                out.push(format!("ネットの役割「{}」の役割 `{}` は、power / switching / sense / analog / logic のどれかにしてください", r.name, r.role));
            }
        }
        for g in &self.groups {
            for n in g.parts.iter().filter(|n| !part_known(n)) {
                out.push(format!("近くに置く組「{}」の `{n}` という部品は、回路にありません", g.name));
            }
        }
        for a in &self.apart {
            for n in a.a.iter().chain(&a.b).filter(|n| !part_known(n) && !net_known(n)) {
                out.push(format!("離す組「{}」の `{n}` は、部品にもネットにもありません", a.name));
            }
        }
        out
    }
}

/// Does the name `pattern` (a plain name, or one ending in `*`) stand for `name`?
pub fn matches_name(pattern: &str, name: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => pattern == name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_with_a_star_stands_for_all_that_begin_so() {
        assert!(matches_name("GN*", "GN10"));
        assert!(matches_name("VCC", "VCC"));
        assert!(!matches_name("VCC", "VCC2"));
        assert!(!matches_name("GN*", "SEL2"));
    }

    #[test]
    fn unknown_names_are_reported_by_row() {
        let spec = DesignSpec {
            net_rules: vec![NetRule { name: "検出".into(), nets: vec!["SA".into(), "XX".into()], role: "sense".into(), ..Default::default() }],
            groups: vec![PlaceGroup { name: "入口".into(), parts: vec!["J3A".into(), "Z9".into()], ..Default::default() }],
            apart: vec![KeepApart { name: "降圧".into(), a: vec!["BKSW".into()], b: vec!["U8".into(), "ZZ".into()], min_dist: 20.0, ..Default::default() }],
        };
        let parts: Vec<String> = ["J3A", "U8"].iter().map(|s| s.to_string()).collect();
        let nets: Vec<String> = ["SA", "BKSW"].iter().map(|s| s.to_string()).collect();
        let msgs = spec.unknown_names(&parts, &nets);
        assert_eq!(msgs.len(), 3, "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("XX")) && msgs.iter().any(|m| m.contains("Z9")) && msgs.iter().any(|m| m.contains("ZZ")));
    }

    #[test]
    fn an_empty_table_is_not_written_and_a_full_one_comes_back() {
        let empty = DesignSpec::default();
        assert!(empty.is_empty());
        let spec = DesignSpec { groups: vec![PlaceGroup { name: "g".into(), parts: vec!["R1".into()], max_dist: 5.0, note: "n".into() }], ..Default::default() };
        let back: DesignSpec = serde_json::from_str(&serde_json::to_string(&spec).unwrap()).unwrap();
        assert_eq!(back, spec);
    }
}
