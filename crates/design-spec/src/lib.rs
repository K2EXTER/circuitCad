//! The design's requirements, kept with the schematic (one table for the whole design, not one per sheet): what the nets are for
//! (role, width, clearance, current), which parts belong close together, and what must keep its distance. The board import takes them
//! over (net classes, currents, placement groups, distances to check). They are written as the person who designs the board would
//! say them, so the same sentences can be read back from the file.

use serde::{Deserialize, Serialize};

/// The words a net's role can have (the board's `Role`, by name); empty = none.
pub const ROLES: [&str; 5] = ["power", "switching", "sense", "analog", "logic"];

/// The width (mm) of the wire between two parts on the nets of a rule ("from the battery to the regulator, thick; on to the filter, thin").
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LinkWidth {
    /// The two parts' reference designators.
    pub a: String,
    pub b: String,
    /// 0 = the rule's width.
    pub width: f64,
}

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
    /// Wires between two parts that have a width of their own (the router makes these connections first, with that width).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<LinkWidth>,
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
            for p in r.links.iter().flat_map(|l| [&l.a, &l.b]).filter(|p| !part_known(p)) {
                out.push(format!("ネットの役割「{}」の部品間の線幅の `{p}` という部品は、回路にありません", r.name));
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

/// A current found by an analysis, and what to put in the table because of it.
#[derive(Clone, Debug, PartialEq)]
pub enum Fill {
    /// The row `row` carries no current yet: it gets the biggest of its nets'.
    RowCurrent { row: usize, amps: f64 },
    /// A net that carries a big current and is in no row: a row of its own.
    NewRow { net: String, amps: f64 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    /// One sentence for the person who decides (what, and why).
    pub text: String,
    pub fill: Fill,
}

/// The nets with no row that carry at least this current (A) get a row of their own.
pub const BIG_CURRENT: f64 = 0.1;
/// The current (A) from which a net of a suggested row is of the role `power`.
pub const POWER_CURRENT: f64 = 0.5;
/// A current below this (A) is noise of the analysis.
const MIN_CURRENT: f64 = 1e-3;

/// A current rounded up to two figures (0.234 -> 0.24 A): what is written in the table is a round number that is not below the current.
pub fn round_up_amps(a: f64) -> f64 {
    if a <= 0.0 {
        return 0.0;
    }
    let scale = 10f64.powf(a.log10().floor() - 1.0);
    ((a / scale - 1e-9).ceil() * scale * 1e6).round() / 1e6
}

/// What the currents of an analysis (net name, amperes; the sign does not matter) say about the table: rows that state no current get
/// the biggest current of their nets, and the nets that carry `BIG_CURRENT` or more and are in no row get a row (role `power` from
/// `POWER_CURRENT`). Only what is empty is filled: a current the table states is the person's.
pub fn suggest_from_currents(spec: &DesignSpec, currents: &[(String, f64)]) -> Vec<Suggestion> {
    let mut out = vec![];
    let amps = |n: &str| currents.iter().find(|(m, _)| m == n).map_or(0.0, |(_, a)| a.abs());
    for (k, r) in spec.net_rules.iter().enumerate() {
        // (a switching node carries a current that only exists in time: the DC operating point says nothing about it)
        if r.current > 0.0 || r.role == "switching" {
            continue;
        }
        let best = currents.iter().filter(|(n, _)| r.nets.iter().any(|p| matches_name(p, n))).map(|(n, a)| (n.as_str(), a.abs())).max_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((net, a)) = best.filter(|b| b.1 >= MIN_CURRENT) {
            let up = round_up_amps(a);
            out.push(Suggestion { text: format!("「{}」の電流を {up} A にする (解析で {net} に {a:.3} A)", r.name), fill: Fill::RowCurrent { row: k, amps: up } });
        }
    }
    let mut big: Vec<(&str, f64)> = currents.iter().map(|(n, _)| (n.as_str(), amps(n))).filter(|(n, a)| *a >= BIG_CURRENT && !spec.net_rules.iter().any(|r| r.nets.iter().any(|p| matches_name(p, n)))).collect();
    big.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(b.0)));
    big.dedup_by(|a, b| a.0 == b.0);
    for (net, a) in big {
        let up = round_up_amps(a);
        out.push(Suggestion { text: format!("ネット {net} の行を足す: 電流 {up} A (解析で {a:.3} A。どの行にも入っていません)"), fill: Fill::NewRow { net: net.to_string(), amps: up } });
    }
    out
}

impl Suggestion {
    /// Write it into the table. A row that was found by its number is looked up again by the table's size (the table may have changed).
    pub fn apply(&self, spec: &mut DesignSpec) {
        match &self.fill {
            Fill::RowCurrent { row, amps } => {
                if let Some(r) = spec.net_rules.get_mut(*row).filter(|r| r.current <= 0.0) {
                    r.current = *amps;
                }
            }
            Fill::NewRow { net, amps } => {
                if !spec.net_rules.iter().any(|r| r.nets.iter().any(|p| matches_name(p, net))) {
                    let role = if *amps >= POWER_CURRENT { "power" } else { "" };
                    spec.net_rules.push(NetRule { name: format!("{net} (解析)"), nets: vec![net.clone()], role: role.into(), current: *amps, note: "DC解析の電流から".into(), ..Default::default() });
                }
            }
        }
    }
}

#[cfg(test)]
mod suggest_tests {
    use super::*;

    #[test]
    fn a_current_is_rounded_up_to_two_figures() {
        assert_eq!(round_up_amps(0.234), 0.24);
        assert_eq!(round_up_amps(1.0), 1.0);
        assert_eq!(round_up_amps(1.033), 1.1);
        assert_eq!(round_up_amps(0.0), 0.0);
    }

    #[test]
    fn the_empty_current_of_a_row_and_the_big_nets_without_a_row_are_suggested() {
        let spec = DesignSpec {
            net_rules: vec![
                NetRule { name: "電源".into(), nets: vec!["VIN".into(), "VOUT".into()], ..Default::default() },
                NetRule { name: "済".into(), nets: vec!["X".into()], current: 2.0, ..Default::default() },
            ],
            ..Default::default()
        };
        // a switching row is not suggested a DC current
        let mut spec = spec;
        spec.net_rules.push(NetRule { name: "SW".into(), nets: vec!["SW".into()], role: "switching".into(), ..Default::default() });
        let currents: Vec<(String, f64)> = [("SW", 0.3), ("VIN", 0.234), ("VOUT", -0.5), ("X", 5.0), ("BAT", 1.2), ("SIG", 0.002), ("SMALL", 0.0001)].iter().map(|(n, a)| (n.to_string(), *a)).collect();
        let s = suggest_from_currents(&spec, &currents);
        assert_eq!(s.len(), 2, "{s:?}");
        assert_eq!(s[0].fill, Fill::RowCurrent { row: 0, amps: 0.5 });
        assert_eq!(s[1].fill, Fill::NewRow { net: "BAT".into(), amps: 1.2 });
        let mut t = spec.clone();
        for x in &s {
            x.apply(&mut t);
        }
        assert_eq!(t.net_rules[0].current, 0.5);
        assert_eq!(t.net_rules[1].current, 2.0, "a stated current is left alone");
        assert_eq!(t.net_rules.iter().find(|r| r.nets == ["BAT"]).unwrap().role, "power");
        assert_eq!(t.net_rules[2].current, 0.0, "a switching row gets no DC current");
        // applying again changes nothing
        let again = t.clone();
        for x in &s {
            x.apply(&mut t);
        }
        assert_eq!(t, again);
        assert!(suggest_from_currents(&t, &currents).is_empty());
    }
}
