//! The design's requirements, kept with the schematic (one table for the whole design, not one per sheet): what the nets are for
//! (role, width, clearance, current), which parts belong close together, and what must keep its distance. The board import takes them
//! over (net classes, currents, placement groups, distances to check). They are written as the person who designs the board would
//! say them, so the same sentences can be read back from the file.

pub mod netnames;

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
    /// The place in the order of routing (0 first, up to 9) when it is not the role's; None: the role decides.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<u8>,
    /// Vias: "none" (no vias), "allow" (vias even if the role says none), "" (the role decides).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub vias: String,
    /// The layers the tracks of these nets may use ("top", "bottom", "inner1" ...); empty: any.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<String>,
    /// The most track (mm) a net of the row may have in all; 0 = no limit.
    #[serde(skip_serializing_if = "is_zero")]
    pub max_length: f64,
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

/// The bypass capacitors of one IC ("C5 and C6 are for U3"). The schematic does not say which IC a capacitor on a shared supply serves
/// (the nets are the same wherever it is drawn); this does, for the placement (the capacitor goes by the IC's supply pin) and the check.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Bypass {
    /// The IC's reference designator.
    pub ic: String,
    /// The capacitors' reference designators.
    pub caps: Vec<String>,
    pub note: String,
}

/// A measure of the copper of a connection that must stay within a limit: the length of the way from one pad to another, the vias on
/// it, or how far the ways from one pad to two others run together before they part (a sense wire that must leave the pad on its own).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Measure {
    pub name: String,
    /// One of `MEASURES`: "length", "vias", "fork", "gap", "guard" or "tap".
    pub what: String,
    /// Pads as "PART.PIN": length and vias take two (from, to), fork takes three (the pad the ways start at, and the two they go to).
    pub pads: Vec<String>,
    /// "max" (the value must not exceed `limit`, the default) or "min".
    pub op: String,
    pub limit: f64,
    /// Guard only: how far (mm) the guard's edge may be from the edge of the track it guards (0 = 1 mm).
    #[serde(skip_serializing_if = "is_zero")]
    pub reach: f64,
    pub note: String,
}

/// The kinds of measure: (key, what the table calls it, unit, pads needed).
pub const MEASURES: &[(&str, &str, &str, usize)] = &[("length", "長さ", "mm", 2), ("vias", "ビアの数", "個", 2), ("fork", "分かれ目 (一緒に走る長さ)", "mm", 3), ("tap", "検出の線のつなぎ目 (内側の縁から)", "mm", 3), ("gap", "ネット間の間隔", "mm", 2), ("guard", "ガードの被覆", "%", 2)];

impl Measure {
    pub fn is_min(&self) -> bool {
        self.op == "min"
    }

    /// (what it is called, its unit, the pads it needs); None for a kind that is not in `MEASURES`.
    /// The reach of a guard (mm).
    pub fn guard_reach(&self) -> f64 {
        if self.reach > 0.0 { self.reach } else { 1.0 }
    }

    pub fn kind(&self) -> Option<(&'static str, &'static str, usize)> {
        MEASURES.iter().find(|(k, ..)| *k == self.what).map(|(_, l, u, n)| (*l, *u, *n))
    }
}

/// A finding of the check that the designer knows about and keeps, with the reason: the check then says "accepted" instead of "wrong".
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Waiver {
    /// One of `WAIVER_KINDS` ("group", "apart", "bypass").
    pub kind: String,
    /// The row it is about: the group, the distance to keep, or the IC of a bypass row.
    pub name: String,
    /// A bypass row only: the capacitor ("" = every capacitor of the row).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub item: String,
    /// Why it is accepted (what the person who reads the table later needs to know).
    pub note: String,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// The kinds of a waiver: (key, what the table calls it).
pub const WAIVER_KINDS: &[(&str, &str)] = &[("group", "組"), ("apart", "離す距離"), ("bypass", "バイパス"), ("measure", "測定"), ("net", "ネットの性質")];

impl Waiver {
    /// "組「名前」" and the like, for a sentence.
    pub fn label(&self) -> String {
        let kind = WAIVER_KINDS.iter().find(|(k, _)| *k == self.kind).map_or(self.kind.as_str(), |(_, l)| *l);
        if self.item.is_empty() { format!("{kind}「{}」", self.name) } else { format!("{kind}「{}」の {}", self.name, self.item) }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DesignSpec {
    pub net_rules: Vec<NetRule>,
    pub groups: Vec<PlaceGroup>,
    pub apart: Vec<KeepApart>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub bypass: Vec<Bypass>,
    /// Findings of the check that are accepted, each with its reason.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub waivers: Vec<Waiver>,
    /// Limits on the copper of a connection (length, vias, where ways part).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub measures: Vec<Measure>,
}

impl DesignSpec {
    pub fn is_empty(&self) -> bool {
        self.net_rules.is_empty() && self.groups.is_empty() && self.apart.is_empty() && self.bypass.is_empty() && self.waivers.is_empty() && self.measures.is_empty()
    }

    /// The accepted exception that covers a finding of `kind` in the row `name` (for a bypass row, the capacitor `item`), if there is one.
    pub fn waiver_for(&self, kind: &str, name: &str, item: &str) -> Option<&Waiver> {
        self.waivers.iter().find(|w| w.kind == kind && w.name == name && (w.item.is_empty() || w.item == item))
    }

    /// The rows that give no reason (the note is empty): one sentence for each. A table is read by people who were not there when it was
    /// written; the numbers say what, the reason says why.
    pub fn unexplained(&self) -> Vec<String> {
        let empty = |n: &str| n.trim().is_empty();
        let mut out = vec![];
        out.extend(self.net_rules.iter().filter(|r| empty(&r.note)).map(|r| format!("ネットの役割「{}」", r.name)));
        out.extend(self.groups.iter().filter(|g| empty(&g.note)).map(|g| format!("近くに置く組「{}」", g.name)));
        out.extend(self.apart.iter().filter(|a| empty(&a.note)).map(|a| format!("離す組「{}」", a.name)));
        out.extend(self.bypass.iter().filter(|b| empty(&b.note)).map(|b| format!("バイパスの対応「{}」", b.ic)));
        out.extend(self.measures.iter().filter(|m| empty(&m.note)).map(|m| format!("測定「{}」", m.name)));
        out.extend(self.waivers.iter().filter(|w| empty(&w.note)).map(|w| format!("承知した例外 {}", w.label())));
        out
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
        for b in &self.bypass {
            for n in std::iter::once(&b.ic).chain(&b.caps).filter(|n| !part_known(n)) {
                out.push(format!("バイパスの対応「{}」の `{n}` という部品は、回路にありません", b.ic));
            }
        }
        for m in &self.measures {
            match m.kind() {
                None => out.push(format!("測定「{}」の種類 `{}` は、length / vias / fork / gap / guard / tap のどれかにしてください", m.name, m.what)),
                Some((_, _, need)) if m.pads.len() != need => out.push(format!("測定「{}」には、パッドが {need} 個 (部品.ピン) 要ります (いま {} 個)", m.name, m.pads.len())),
                Some(_) => {}
            }
            for pad in &m.pads {
                if m.what == "gap" || m.what == "guard" {
                    // a gap and a guard are between nets (several joined by `+`)
                    for n in pad.split('+').map(str::trim).filter(|n| !n.is_empty()) {
                        if !net_known(n) {
                            out.push(format!("測定「{}」の `{n}` というネットは、回路にありません", m.name));
                        }
                    }
                    continue;
                }
                match pad.trim().rsplit_once('.') {
                    Some((r, _)) if part_known(r) => {}
                    _ => out.push(format!("測定「{}」の `{pad}` は、部品.ピン の形で、回路にある部品を指してください", m.name)),
                }
            }
        }
        for w in &self.waivers {
            let row = match w.kind.as_str() {
                "group" => self.groups.iter().any(|g| g.name == w.name),
                "apart" => self.apart.iter().any(|a| a.name == w.name),
                "bypass" => self.bypass.iter().any(|b| b.ic == w.name && (w.item.is_empty() || b.caps.contains(&w.item))),
                "measure" => self.measures.iter().any(|m| m.name == w.name),
                "net" => self.net_rules.iter().any(|n| n.name == w.name),
                _ => false,
            };
            if !row {
                out.push(format!("承知した例外 {} に当てはまる行が、表にありません (行の名前を直したか、消した行かもしれません)", w.label()));
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
            ..Default::default()
        };
        let parts: Vec<String> = ["J3A", "U8"].iter().map(|s| s.to_string()).collect();
        let nets: Vec<String> = ["SA", "BKSW"].iter().map(|s| s.to_string()).collect();
        let msgs = spec.unknown_names(&parts, &nets);
        assert_eq!(msgs.len(), 3, "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("XX")) && msgs.iter().any(|m| m.contains("Z9")) && msgs.iter().any(|m| m.contains("ZZ")));
    }

    #[test]
    fn a_waiver_covers_its_row_and_is_checked_against_the_rows() {
        let mut spec = DesignSpec {
            bypass: vec![Bypass { ic: "U1".into(), caps: vec!["C1".into(), "C2".into()], note: "n".into() }],
            groups: vec![PlaceGroup { name: "ループ".into(), parts: vec!["R1".into()], max_dist: 5.0, note: "n".into() }],
            ..Default::default()
        };
        spec.waivers.push(Waiver { kind: "bypass".into(), name: "U1".into(), item: "C2".into(), note: "配線の都合".into() });
        spec.waivers.push(Waiver { kind: "group".into(), name: "ループ".into(), ..Default::default() });
        assert!(spec.waiver_for("bypass", "U1", "C2").is_some());
        assert!(spec.waiver_for("bypass", "U1", "C1").is_none(), "only the capacitor named");
        assert!(spec.waiver_for("group", "ループ", "").is_some() && spec.waiver_for("apart", "ループ", "").is_none());
        // the group waiver has no reason: it is listed as unexplained
        assert_eq!(spec.unexplained().len(), 1, "{:?}", spec.unexplained());
        assert!(spec.unknown_names(&["U1".into(), "C1".into(), "C2".into(), "R1".into()], &[]).is_empty());
        spec.waivers.push(Waiver { kind: "apart".into(), name: "なし".into(), note: "x".into(), ..Default::default() });
        let msgs = spec.unknown_names(&["U1".into(), "C1".into(), "C2".into(), "R1".into()], &[]);
        assert_eq!(msgs.len(), 1, "{msgs:?}");
        let back: DesignSpec = serde_json::from_str(&serde_json::to_string(&spec).unwrap()).unwrap();
        assert_eq!(back, spec);
    }

    #[test]
    fn a_measure_is_checked_for_its_kind_and_pads_and_can_be_waived() {
        let mut spec = DesignSpec {
            measures: vec![
                Measure { name: "検出線".into(), what: "fork".into(), pads: vec!["RS1.1".into(), "Q1.3".into(), "UAB.2".into()], op: "max".into(), limit: 0.5, reach: 0.0, note: "ケルビン".into() },
                Measure { name: "短く".into(), what: "length".into(), pads: vec!["RS1.1".into()], limit: 10.0, ..Default::default() },
                Measure { name: "変".into(), what: "area".into(), pads: vec![], ..Default::default() },
            ],
            ..Default::default()
        };
        let parts: Vec<String> = ["RS1", "Q1", "UAB"].iter().map(|s| s.to_string()).collect();
        let msgs = spec.unknown_names(&parts, &[]);
        assert_eq!(msgs.len(), 2, "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("area")) && msgs.iter().any(|m| m.contains("パッドが 2 個")));
        assert_eq!(spec.unexplained().len(), 2, "two measures have no reason: {:?}", spec.unexplained());
        assert_eq!(spec.measures[0].kind(), Some(("分かれ目 (一緒に走る長さ)", "mm", 3)));
        spec.waivers.push(Waiver { kind: "measure".into(), name: "検出線".into(), note: "x".into(), ..Default::default() });
        assert_eq!(spec.unknown_names(&parts, &[]).len(), 2, "the waiver finds its row");
        let back: DesignSpec = serde_json::from_str(&serde_json::to_string(&spec).unwrap()).unwrap();
        assert_eq!(back, spec);
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
    /// A capacitor that serves an IC (found from the circuit, not from an analysis).
    Bypass { ic: String, cap: String },
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

/// A part of the circuit as the bypass suggestions see it.
#[derive(Clone, Debug, Default)]
pub struct PartView {
    pub refdes: String,
    /// The sheet it is drawn on.
    pub sheet: String,
    /// How many pins the part has (an IC has 8 or more).
    pub pin_count: usize,
    /// The net of each pin that is on a net.
    pub nets: Vec<String>,
}

/// An IC has this many pins or more (as on the board: `autoplace`'s IC blocks).
const IC_PINS: usize = 8;

/// Which IC each bypass capacitor serves, for the capacitors the table does not place yet. A capacitor has a pin on a supply and a pin on
/// another rail (ground or a supply); it goes to an IC (`U…` with 8 pins or more) that has a pin on its supply net: of those, the ones that
/// also have its other net (a capacitor between VCC and VNEG goes to the IC that runs between them), then the ones on the same sheet,
/// then the one that has the fewest capacitors so far. Each suggestion says which of these decided. (The net labels say nothing about where
/// a capacitor is drawn, so the position on the sheet is not used; the sheet is.)
pub fn suggest_bypass(spec: &DesignSpec, parts: &[PartView]) -> Vec<Suggestion> {
    use netnames::{is_ground_net, is_negative_supply_net, is_supply_net};
    let supply = |n: &str| is_supply_net(n) || is_negative_supply_net(n);
    let rail = |n: &str| supply(n) || is_ground_net(n);
    let ics: Vec<&PartView> = parts.iter().filter(|p| p.refdes.starts_with('U') && p.pin_count >= IC_PINS).collect();
    let mut count: std::collections::BTreeMap<&str, usize> = Default::default();
    for b in &spec.bypass {
        *count.entry(b.ic.as_str()).or_default() += b.caps.len();
    }
    let mut caps: Vec<&PartView> = parts.iter().filter(|p| p.refdes.starts_with('C') && !spec.bypass.iter().any(|b| b.caps.contains(&p.refdes))).collect();
    caps.sort_by(|a, b| a.refdes.cmp(&b.refdes));
    let mut out = vec![];
    for cap in caps {
        let Some(sp) = cap.nets.iter().find(|n| supply(n) && cap.nets.iter().any(|o| o != *n && rail(o))) else { continue };
        let other = cap.nets.iter().find(|o| *o != sp && rail(o));
        let has = |ic: &PartView, net: &str| ic.nets.iter().any(|n| n == net);
        let mut fit: Vec<&PartView> = ics.iter().copied().filter(|ic| has(ic, sp)).collect();
        if fit.is_empty() {
            continue;
        }
        let mut why = if fit.len() == 1 { "その電源につながる IC は、これだけです" } else { "" };
        if fit.len() > 1 && fit.iter().any(|ic| other.is_some_and(|o| has(ic, o))) {
            fit.retain(|ic| other.is_some_and(|o| has(ic, o)));
            if fit.len() == 1 {
                why = "コンデンサの両側の電源を、ともに持つ IC です";
            }
        }
        if fit.len() > 1 && fit.iter().any(|ic| !cap.sheet.is_empty() && ic.sheet == cap.sheet) {
            fit.retain(|ic| ic.sheet == cap.sheet);
            if fit.len() == 1 {
                why = "同じシートの IC です";
            }
        }
        fit.sort_by_key(|ic| count.get(ic.refdes.as_str()).copied().unwrap_or(0));
        let ic = fit[0];
        if why.is_empty() {
            why = "候補が複数あります (持っているコンデンサの数が少ない IC を選びました。確かめてください)";
        }
        *count.entry(ic.refdes.as_str()).or_default() += 1;
        out.push(Suggestion { text: format!("{} は {} のバイパスコンデンサ ({why})", cap.refdes, ic.refdes), fill: Fill::Bypass { ic: ic.refdes.clone(), cap: cap.refdes.clone() } });
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
                    spec.net_rules.push(NetRule { name: format!("{net} (解析)"), nets: vec![net.clone()], role: role.into(), current: *amps, ..Default::default() });
                }
            }
            Fill::Bypass { ic, cap } => {
                if spec.bypass.iter().any(|b| b.caps.contains(cap)) {
                    return;
                }
                match spec.bypass.iter_mut().find(|b| &b.ic == ic) {
                    Some(b) => b.caps.push(cap.clone()),
                    None => spec.bypass.push(Bypass { ic: ic.clone(), caps: vec![cap.clone()], note: String::new() }),
                }
            }
        }
    }
}

#[cfg(test)]
mod suggest_tests {
    use super::*;

    fn view(r: &str, sheet: &str, pins: usize, nets: &[&str]) -> PartView {
        PartView { refdes: r.into(), sheet: sheet.into(), pin_count: pins, nets: nets.iter().map(|s| s.to_string()).collect() }
    }

    #[test]
    fn a_bypass_capacitor_goes_to_the_ic_of_its_sheet_and_the_choice_says_why() {
        let parts = vec![
            view("U1", "電源", 8, &["VCC", "GND", "A"]),
            view("U2", "計測", 8, &["VCC", "GND", "B"]),
            view("C1", "計測", 2, &["VCC", "GND"]),
            view("C2", "電源", 2, &["VCC", "GND"]),
            view("C3", "電源", 2, &["VCC", "VNEG"]),
            view("U3", "電源", 8, &["VCC", "VNEG"]),
            view("R1", "電源", 2, &["VCC", "A"]),
        ];
        let found = suggest_bypass(&DesignSpec::default(), &parts);
        let by = |cap: &str| found.iter().find_map(|s| match &s.fill { Fill::Bypass { ic, cap: c } if c == cap => Some((ic.clone(), s.text.clone())), _ => None });
        assert_eq!(by("C1").unwrap().0, "U2", "same sheet");
        assert_eq!(by("C2").unwrap().0, "U1", "same sheet");
        assert_eq!(by("C3").unwrap().0, "U3", "the one that has both supplies");
        assert!(by("C3").unwrap().1.contains("両側"));
        assert!(by("R1").is_none(), "a resistor is no bypass capacitor");
        // what the table says is left alone, and a suggestion that is applied joins the row of its IC
        let mut spec = DesignSpec { bypass: vec![Bypass { ic: "U1".into(), caps: vec!["C1".into()], note: "手で".into() }], ..Default::default() };
        let rest = suggest_bypass(&spec, &parts);
        assert!(rest.iter().all(|s| !matches!(&s.fill, Fill::Bypass { cap, .. } if cap == "C1")));
        for s in &rest {
            s.apply(&mut spec);
        }
        assert_eq!(spec.bypass.iter().find(|b| b.ic == "U1").unwrap().caps, vec!["C1".to_string(), "C2".to_string()]);
        assert!(spec.bypass.iter().any(|b| b.ic == "U3" && b.caps == vec!["C3".to_string()]));
    }

    #[test]
    fn the_rows_without_a_reason_are_listed() {
        let spec = DesignSpec {
            net_rules: vec![
                NetRule { name: "電源".into(), nets: vec!["VIN".into()], note: "電流が通る".into(), ..Default::default() },
                NetRule { name: "帰還".into(), nets: vec!["FB".into()], note: "  ".into(), ..Default::default() },
            ],
            groups: vec![PlaceGroup { name: "ループ".into(), parts: vec!["U1".into(), "L1".into()], ..Default::default() }],
            apart: vec![KeepApart { name: "離す".into(), a: vec!["R1".into()], b: vec!["L1".into()], min_dist: 6.0, note: "雑音".into() }],
            bypass: vec![Bypass { ic: "U1".into(), caps: vec!["C1".into()], note: String::new() }],
            ..Default::default()
        };
        assert_eq!(spec.unexplained(), vec!["ネットの役割「帰還」".to_string(), "近くに置く組「ループ」".to_string(), "バイパスの対応「U1」".to_string()]);
        assert!(DesignSpec::default().unexplained().is_empty());
    }

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
