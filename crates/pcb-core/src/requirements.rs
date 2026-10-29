//! The schematic's table of requirements on the board: the net rules become net classes and currents, the groups and the distances are kept
//! with the board and checked (`check`: a group that is too spread out, two sets that are too near). The table is the schematic's:
//! what it states is written over what the board has for the same nets; what it leaves at 0 or does not mention stays.

use crate::model::*;
use design_spec::{matches_name, DesignSpec, PlaceGroup};
use std::collections::BTreeSet;

/// What `import_requirements` took.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RequirementsImport {
    pub classes: usize,
    pub currents: usize,
    pub groups: usize,
    pub apart: usize,
    /// Wires between two parts with a width of their own.
    pub links: usize,
}

/// The nets of the board that a rule's names stand for (the names ending in `*` count as prefixes).
fn nets_matching(board: &Board, patterns: &[String]) -> Vec<String> {
    let all: BTreeSet<&str> = board.pads.iter().filter(|p| !p.net.is_empty()).map(|p| p.net.as_str()).collect();
    all.into_iter().filter(|n| patterns.iter().any(|p| matches_name(p, n))).map(|n| n.to_string()).collect()
}

/// Take the table: each net rule makes (or updates) the net class of its name, with the nets on the board that it names (they leave
/// other classes), and sets the current of those nets when it states one; the groups and the distances replace the board's.
pub fn import_requirements(board: &mut Board, spec: &DesignSpec) -> RequirementsImport {
    let mut out = RequirementsImport::default();
    board.link_widths.clear();
    for rule in &spec.net_rules {
        let nets = nets_matching(board, &rule.nets);
        if nets.is_empty() || rule.name.is_empty() {
            continue;
        }
        for c in board.net_classes.iter_mut().filter(|c| c.name != rule.name) {
            c.nets.retain(|n| !nets.contains(n));
        }
        let k = match board.net_classes.iter().position(|c| c.name == rule.name) {
            Some(k) => k,
            None => {
                board.net_classes.push(NetClass { name: rule.name.clone(), ..Default::default() });
                board.net_classes.len() - 1
            }
        };
        let c = &mut board.net_classes[k];
        c.nets = nets.clone();
        if rule.width > 0.0 {
            c.track_width = rule.width;
        }
        if rule.clearance > 0.0 {
            c.clearance = rule.clearance;
        }
        if let Some(r) = Role::parse(&rule.role) {
            c.role = r;
        }
        out.classes += 1;
        for l in rule.links.iter().filter(|l| l.width > 0.0) {
            for n in &nets {
                let on = |r: &str| board.pads.iter().any(|p| p.refdes == r && &p.net == n);
                if on(&l.a) && on(&l.b) && l.a != l.b {
                    board.link_widths.push(LinkWidth { net: n.clone(), a: l.a.clone(), b: l.b.clone(), width: l.width });
                    out.links += 1;
                }
            }
        }
        if rule.current > 0.0 {
            for n in &nets {
                board.net_currents.insert(n.clone(), rule.current);
                out.currents += 1;
            }
        }
    }
    board.place_groups = spec.groups.clone();
    board.keep_apart = spec.apart.clone();
    out.groups = spec.groups.len();
    out.apart = spec.apart.len();
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindingKind {
    /// A group whose parts are farther apart than it allows.
    Group,
    /// Two sets nearer to each other than they may be.
    Apart,
}

#[derive(Clone, Debug)]
pub struct Finding {
    pub kind: FindingKind,
    pub name: String,
    /// What is wrong, in a sentence (the parts or nets, the distance, the limit).
    pub message: String,
    /// The distance found (mm) and the limit (mm).
    pub actual: f64,
    pub limit: f64,
}

#[derive(Clone, Debug, Default)]
pub struct RequirementsReport {
    pub findings: Vec<Finding>,
    /// Groups and distances that were looked at.
    pub checked: usize,
    /// Names the board does not have (a part that is not on it, a net with no pad): those rows are left out.
    pub missing: Vec<String>,
}

/// The pads a name stands for: the pads of the part of that reference, otherwise of the nets (a `*` as a prefix).
fn pads_of<'a>(board: &'a Board, name: &str) -> Vec<&'a Pad> {
    if board.parts.iter().any(|p| p.refdes == name) {
        return board.pads.iter().filter(|p| p.refdes == name).collect();
    }
    board.pads.iter().filter(|p| !p.net.is_empty() && matches_name(name, &p.net)).collect()
}

/// The middle of a part's pads.
fn centre(pads: &[&Pad]) -> Option<Point> {
    if pads.is_empty() {
        return None;
    }
    let n = pads.len() as f64;
    Some(Point::new(pads.iter().map(|p| p.pos.x).sum::<f64>() / n, pads.iter().map(|p| p.pos.y).sum::<f64>() / n))
}

/// Check the groups (the farthest two parts, from middle to middle, within `max_dist`; a group that says 0 is not checked) and the
/// distances (the nearest two pads of the two sets, centre to centre, at least `min_dist`).
pub fn check(board: &Board) -> RequirementsReport {
    let mut rep = RequirementsReport::default();
    for g in &board.place_groups {
        let mids: Vec<(String, Point)> = g.parts.iter().filter_map(|r| centre(&pads_of(board, r)).map(|c| (r.clone(), c))).collect();
        for r in g.parts.iter().filter(|r| !mids.iter().any(|(m, _)| m == *r)) {
            rep.missing.push(format!("{} (組「{}」)", r, g.name));
        }
        if g.max_dist <= 0.0 || mids.len() < 2 {
            continue;
        }
        rep.checked += 1;
        let mut worst: Option<(f64, &str, &str)> = None;
        for (i, (ra, pa)) in mids.iter().enumerate() {
            for (rb, pb) in &mids[i + 1..] {
                let d = pa.dist(*pb);
                if worst.is_none_or(|w| d > w.0) {
                    worst = Some((d, ra, rb));
                }
            }
        }
        if let Some((d, a, b)) = worst.filter(|w| w.0 > g.max_dist) {
            rep.findings.push(Finding {
                kind: FindingKind::Group,
                name: g.name.clone(),
                message: format!("組「{}」: {a} と {b} が {d:.1} mm 離れています (上限 {} mm)", g.name, g.max_dist),
                actual: d,
                limit: g.max_dist,
            });
        }
    }
    for a in &board.keep_apart {
        let side = |names: &[String], rep: &mut RequirementsReport| -> Vec<(String, Point)> {
            let mut v = vec![];
            for n in names {
                let pads = pads_of(board, n);
                if pads.is_empty() {
                    rep.missing.push(format!("{} (離す組「{}」)", n, a.name));
                }
                v.extend(pads.iter().map(|p| (n.clone(), p.pos)));
            }
            v
        };
        let (pa, pb) = (side(&a.a, &mut rep), side(&a.b, &mut rep));
        if pa.is_empty() || pb.is_empty() || a.min_dist <= 0.0 {
            continue;
        }
        rep.checked += 1;
        let mut best: Option<(f64, &str, &str)> = None;
        for (na, qa) in &pa {
            for (nb, qb) in &pb {
                let d = qa.dist(*qb);
                if best.is_none_or(|b| d < b.0) {
                    best = Some((d, na, nb));
                }
            }
        }
        if let Some((d, na, nb)) = best.filter(|b| b.0 < a.min_dist) {
            rep.findings.push(Finding {
                kind: FindingKind::Apart,
                name: a.name.clone(),
                message: format!("離す組「{}」: {na} と {nb} が {d:.1} mm まで近づいています (下限 {} mm)", a.name, a.min_dist),
                actual: d,
                limit: a.min_dist,
            });
        }
    }
    rep
}

/// A group of the table as it stands on the board, for drawing: the parts that are on the board, how far apart the farthest two are
/// (0 for fewer than two parts), the limit (0: not checked) and whether it is kept.
#[derive(Clone, Debug)]
pub struct GroupMark {
    pub name: String,
    pub parts: Vec<String>,
    pub actual: f64,
    pub limit: f64,
    pub ok: bool,
}

/// A distance to keep as it stands: the nearest two pads of the two sides (`a` and `b`), their distance, the least allowed, and whether it is kept.
#[derive(Clone, Debug)]
pub struct ApartMark {
    pub name: String,
    pub a: Point,
    pub b: Point,
    pub actual: f64,
    pub limit: f64,
    pub ok: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Marks {
    pub groups: Vec<GroupMark>,
    pub apart: Vec<ApartMark>,
}

/// What `check` works out, as numbers to draw: every group and every distance with where it stands (a row that names nothing the board has is left out).
pub fn marks(board: &Board) -> Marks {
    let mut out = Marks::default();
    for g in &board.place_groups {
        let mids: Vec<Point> = g.parts.iter().filter_map(|r| centre(&pads_of(board, r))).collect();
        let parts: Vec<String> = g.parts.iter().filter(|r| board.parts.iter().any(|p| &p.refdes == *r)).cloned().collect();
        if parts.len() < 2 {
            continue;
        }
        let actual = mids.iter().enumerate().flat_map(|(i, a)| mids[i + 1..].iter().map(move |b| a.dist(*b))).fold(0.0, f64::max);
        out.groups.push(GroupMark { name: g.name.clone(), parts, actual, limit: g.max_dist, ok: g.max_dist <= 0.0 || actual <= g.max_dist });
    }
    for a in &board.keep_apart {
        let pads = |names: &[String]| -> Vec<Point> { names.iter().flat_map(|n| pads_of(board, n)).map(|p| p.pos).collect() };
        let (pa, pb) = (pads(&a.a), pads(&a.b));
        let mut best: Option<(f64, Point, Point)> = None;
        for qa in &pa {
            for qb in &pb {
                let d = qa.dist(*qb);
                if best.is_none_or(|b| d < b.0) {
                    best = Some((d, *qa, *qb));
                }
            }
        }
        if let Some((d, qa, qb)) = best {
            out.apart.push(ApartMark { name: a.name.clone(), a: qa, b: qb, actual: d, limit: a.min_dist, ok: a.min_dist <= 0.0 || d >= a.min_dist });
        }
    }
    out
}

/// The distances to keep, for the placement: the references of the parts on each side (a name is a part, or the parts that have a pad on
/// the net) and the least distance. A row with an empty side, or that says 0, is left out.
pub fn apart_on_board(board: &Board) -> Vec<(Vec<String>, Vec<String>, f64)> {
    let side = |names: &[String]| -> Vec<String> {
        let mut v: Vec<String> = names.iter().flat_map(|n| pads_of(board, n)).map(|p| p.refdes.clone()).collect();
        v.sort();
        v.dedup();
        v
    };
    board.keep_apart.iter().map(|a| (side(&a.a), side(&a.b), a.min_dist)).filter(|(a, b, d)| *d > 0.0 && !a.is_empty() && !b.is_empty()).collect()
}

/// The groups of the board that name at least two parts, for the placement: each as the list of references that are on the board.
pub fn groups_on_board(board: &Board) -> Vec<(&PlaceGroup, Vec<String>)> {
    board
        .place_groups
        .iter()
        .map(|g| (g, g.parts.iter().filter(|r| board.parts.iter().any(|p| &p.refdes == *r)).cloned().collect::<Vec<_>>()))
        .filter(|(_, v)| v.len() >= 2)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::netlist::*;
    use crate::package::PackageSpec;
    use design_spec::{KeepApart, NetRule};

    fn board() -> Board {
        let mut b = Board::new("t", 60.0, 40.0);
        b.footprints.push(PackageSpec::Chip2 { pitch: 1.0, pad_len: 0.6, pad_width: 0.6 }.build("R0402").unwrap());
        let parts: Vec<NetlistPart> = ["R1", "R2", "R3"].iter().map(|r| NetlistPart { refdes: r.to_string(), value: "1k".into(), footprint: "R0402".into() }).collect();
        let nets = vec![
            NetlistNet { name: "SA".into(), pins: vec![("R1".into(), "1".into()), ("R2".into(), "1".into())] },
            NetlistNet { name: "SB".into(), pins: vec![("R2".into(), "2".into()), ("R3".into(), "1".into())] },
        ];
        import(&mut b, &parts, &nets);
        b.place_part("R1", Point::new(10.0, 10.0), 0);
        b.place_part("R2", Point::new(14.0, 10.0), 0);
        b.place_part("R3", Point::new(40.0, 10.0), 0);
        b
    }

    #[test]
    fn the_marks_say_where_each_group_and_distance_stands() {
        let mut b = board();
        b.place_groups.push(design_spec::PlaceGroup { name: "g".into(), parts: vec!["R1".into(), "R2".into(), "R3".into()], max_dist: 10.0, ..Default::default() });
        b.place_groups.push(design_spec::PlaceGroup { name: "near".into(), parts: vec!["R1".into(), "R2".into()], max_dist: 10.0, ..Default::default() });
        b.keep_apart.push(KeepApart { name: "far".into(), a: vec!["R1".into()], b: vec!["R3".into()], min_dist: 20.0, ..Default::default() });
        b.keep_apart.push(KeepApart { name: "close".into(), a: vec!["R1".into()], b: vec!["R2".into()], min_dist: 20.0, ..Default::default() });
        let m = marks(&b);
        assert_eq!(m.groups.iter().map(|g| (g.name.as_str(), g.ok)).collect::<Vec<_>>(), vec![("g", false), ("near", true)]);
        assert_eq!(m.apart.iter().map(|a| (a.name.as_str(), a.ok)).collect::<Vec<_>>(), vec![("far", true), ("close", false)]);
        // the line joins the nearest two pads
        assert!(m.apart[1].actual < 4.0 && m.apart[1].a.dist(m.apart[1].b) == m.apart[1].actual);
    }

    #[test]
    fn a_net_rule_makes_a_class_and_a_current_and_leaves_zero_values_alone() {
        let mut b = board();
        b.net_classes.push(NetClass { name: "検出".into(), track_width: 0.4, ..Default::default() });
        let spec = DesignSpec { net_rules: vec![NetRule { name: "検出".into(), nets: vec!["S*".into(), "NOPE".into()], role: "sense".into(), current: 0.05, ..Default::default() }], ..Default::default() };
        let r = import_requirements(&mut b, &spec);
        assert_eq!((r.classes, r.currents), (1, 2));
        let c = b.net_classes.iter().find(|c| c.name == "検出").unwrap();
        assert_eq!(c.nets, vec!["SA".to_string(), "SB".to_string()]);
        assert_eq!(c.role, Role::Sense);
        assert_eq!(c.track_width, 0.4, "a width of 0 in the table leaves the board's");
        assert_eq!(b.net_currents.get("SA"), Some(&0.05));
    }

    #[test]
    fn a_spread_out_group_and_a_near_pair_are_found() {
        let mut b = board();
        let spec = DesignSpec {
            groups: vec![PlaceGroup { name: "入口".into(), parts: vec!["R1".into(), "R2".into(), "R3".into(), "R9".into()], max_dist: 10.0, ..Default::default() }],
            apart: vec![KeepApart { name: "離す".into(), a: vec!["R1".into()], b: vec!["R2".into()], min_dist: 20.0, ..Default::default() }],
            ..Default::default()
        };
        import_requirements(&mut b, &spec);
        let rep = check(&b);
        assert_eq!(rep.findings.len(), 2, "{:?}", rep.findings);
        assert!(rep.findings.iter().any(|f| f.kind == FindingKind::Group && f.actual > 29.0));
        assert!(rep.findings.iter().any(|f| f.kind == FindingKind::Apart && f.actual < 5.0));
        assert!(rep.missing.iter().any(|m| m.contains("R9")), "{:?}", rep.missing);
        // moved together and apart, nothing is found
        b.place_part("R3", Point::new(18.0, 10.0), 0);
        b.place_part("R1", Point::new(0.0, 10.0), 0);
        b.place_part("R2", Point::new(25.0, 10.0), 0);
        b.place_groups[0].max_dist = 40.0;
        assert!(check(&b).findings.is_empty());
    }
}
