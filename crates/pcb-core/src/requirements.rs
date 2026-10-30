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
    /// Capacitors that the table places at an IC.
    pub bypass: usize,
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
    let copper = board.copper_layers();
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
        // the properties the role would otherwise decide: the place in the order of routing, and whether a via may be used
        c.order = rule.order.map(|o| o.min(9));
        c.layers = rule.layers.iter().filter_map(|k| Side::from_key(k)).filter(|s| copper.contains(s)).collect();
        c.max_length = rule.max_length.max(0.0);
        c.no_vias = match rule.vias.as_str() {
            "none" => Some(true),
            "allow" => Some(false),
            _ => None,
        };
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
    board.bypass = spec.bypass.clone();
    board.waivers = spec.waivers.clone();
    board.measures = spec.measures.clone();
    out.bypass = spec.bypass.iter().map(|b| b.caps.len()).sum();
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
    /// A bypass capacitor farther from the supply pin of its IC than the check allows.
    Bypass,
    /// A measure of the copper of a connection (length, vias, where ways part) outside its limit.
    Measure,
    /// A property of a net class that the copper does not keep: tracks on a layer it does not allow, more track than it lets a net have.
    Net,
}

impl FindingKind {
    /// The key a waiver of the table uses for this kind (`design_spec::WAIVER_KINDS`).
    pub fn key(self) -> &'static str {
        match self {
            FindingKind::Group => "group",
            FindingKind::Apart => "apart",
            FindingKind::Bypass => "bypass",
            FindingKind::Measure => "measure",
            FindingKind::Net => "net",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Finding {
    pub kind: FindingKind,
    pub name: String,
    /// A bypass finding: the capacitor. Empty for the others.
    pub item: String,
    /// What is wrong, in a sentence (the parts or nets, the distance, the limit).
    pub message: String,
    /// The distance found (mm) and the limit (mm).
    pub actual: f64,
    pub limit: f64,
}

#[derive(Clone, Debug, Default)]
pub struct RequirementsReport {
    /// What is wrong and not accepted.
    pub findings: Vec<Finding>,
    /// What is wrong but accepted by a waiver of the table, with its reason (not counted as a violation).
    pub waived: Vec<(Finding, String)>,
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

/// A bypass capacitor and the IC the table says it serves: the pad of the capacitor on a supply (its other pad on a rail) and the pad of the
/// IC on that net that is nearest to it, with their distance. None when the board lacks the parts or the capacitor has no such pad.
fn bypass_pair(board: &Board, cap: &str, ic: &str) -> Option<(Point, Point, f64)> {
    use crate::drc::{is_ground_net, is_negative_supply_net, is_supply_net};
    let rail = |n: &str| is_ground_net(n) || is_supply_net(n) || is_negative_supply_net(n);
    let cap_pads: Vec<&Pad> = board.pads.iter().filter(|p| p.refdes == cap).collect();
    let mut best: Option<(Point, Point, f64)> = None;
    for cp in cap_pads.iter().filter(|p| (is_supply_net(&p.net) || is_negative_supply_net(&p.net)) && cap_pads.iter().any(|q| q.net != p.net && rail(&q.net))) {
        for ip in board.pads.iter().filter(|p| p.refdes == ic && p.net == cp.net) {
            let d = cp.pos.dist(ip.pos);
            if best.is_none_or(|b| d < b.2) {
                best = Some((cp.pos, ip.pos, d));
            }
        }
    }
    best
}

/// Check the groups (the farthest two parts, from middle to middle, within `max_dist`; a group that says 0 is not checked) and the
/// distances (the nearest two pads of the two sets, centre to centre, at least `min_dist`).
impl RequirementsReport {
    /// A finding goes to `waived` when the board has a waiver for it, else to `findings`.
    fn add(&mut self, board: &Board, f: Finding) {
        match board.waivers.iter().find(|w| w.kind == f.kind.key() && w.name == f.name && (w.item.is_empty() || w.item == f.item)) {
            Some(w) => self.waived.push((f, w.note.clone())),
            None => self.findings.push(f),
        }
    }
}

/// What a measure of the table finds on the board: its value, the points of the ways to draw, and (for a fork) where they part.
#[derive(Clone, Debug)]
pub struct Measured {
    pub value: f64,
    /// The ways (each a run of points) that were measured: one for a length or vias, two for a fork.
    pub ways: Vec<Vec<Point>>,
    pub at: Option<Point>,
}

/// The pad of the part and pin; with several (an exposed pad) the one that holds `near`.
fn find_pad<'a>(board: &'a Board, r: &crate::drc::PadRef, near: Option<Point>) -> Option<&'a crate::model::Pad> {
    let mut all = board.pads.iter().filter(|p| p.refdes == r.refdes && p.pin == r.pin);
    let first = all.next()?;
    let Some(n) = near else { return Some(first) };
    let holds = |p: &crate::model::Pad| { let (w, h) = p.shape.size(); (n.x - p.pos.x).abs() <= w / 2.0 + 1e-6 && (n.y - p.pos.y).abs() <= h / 2.0 + 1e-6 };
    Some(std::iter::once(first).chain(all).find(|p| holds(p)).unwrap_or(first))
}

/// Measure `m` on the copper graph `g`. Err: why it cannot be measured (a kind or pads the table got wrong, no copper between the pads).
pub fn measure_one(board: &Board, model: &crate::drc::CopperModel, m: &design_spec::Measure) -> Result<Measured, String> {
    let g = &model.graph;
    use crate::drc::PadRef;
    let Some((_, _, need)) = m.kind() else { return Err(format!("測定「{}」の種類 `{}` が分かりません", m.name, m.what)) };
    if m.what == "gap" || m.what == "guard" || m.what == "parallel" {
        if m.pads.len() != need {
            return Err(format!("測定「{}」には、ネットが {need} 個要ります", m.name));
        }
        let (a, b) = (crate::drc::net_set(&m.pads[0]), crate::drc::net_set(&m.pads[1]));
        if a.is_empty() || b.is_empty() {
            return Err(format!("測定「{}」のネットの名前が空です", m.name));
        }
        if m.what == "gap" {
            // the nearest copper of the two sets of nets (the line to draw runs between the two items)
            let (d, pa, pb) = crate::drc::net_gap_except(&model.index, Some(board), &a, &b, &m.except).ok_or_else(|| format!("測定「{}」: {} と {} の銅が、基板にありません", m.name, m.pads[0], m.pads[1]))?;
            return Ok(Measured { value: d, ways: vec![vec![pa, pb]], at: None });
        }
        if m.what == "parallel" {
            let run = crate::drc::parallel_run(&model.index, board, &a, &b, m.guard_reach(), &m.except).ok_or_else(|| format!("測定「{}」: {} に配線がありません (先に配線してください)", m.name, m.pads[0]))?;
            return Ok(Measured { value: run.length, ways: run.runs, at: None });
        }
        // a guard: how much of the tracks of the sensitive nets has the guard on both sides; the runs without are drawn
        if !board.pads.iter().any(|p| p.net != "" && crate::drc::net_set(&m.pads[1]).iter().any(|g| design_spec::matches_name(g, &p.net))) {
            return Err(format!("測定「{}」: ガードのネット {} につながるパッドが、基板にありません", m.name, m.pads[1]));
        }
        let cover = crate::drc::guard_cover_in(&model.index, board, &a, &b, m.guard_reach()).ok_or_else(|| format!("測定「{}」: {} に配線がありません (先に配線してください)", m.name, m.pads[0]))?;
        return Ok(Measured { value: cover.percent(), ways: cover.bare, at: None });
    }
    if m.what == "area" {
        let Some(pad) = m.pads.first().and_then(|p| PadRef::parse(p)) else { return Err(format!("測定「{}」には、パッドが 1 個 (部品.ピン) 要ります", m.name)) };
        let (area, at) = crate::drc::pad_copper_area(board, &pad, m.reach).ok_or_else(|| format!("測定「{}」: {pad} のパッドが、基板にありません", m.name))?;
        return Ok(Measured { value: area, ways: vec![], at: Some(at) });
    }
    let pads: Vec<PadRef> = m.pads.iter().filter_map(|p| PadRef::parse(p)).collect();
    if pads.len() != need || m.pads.len() != need {
        return Err(format!("測定「{}」には、パッドが {need} 個 (部品.ピン) 要ります", m.name));
    }
    let way = |a: &PadRef, b: &PadRef| g.path(a, b).ok_or_else(|| format!("測定「{}」: {a} と {b} をつなぐ銅の道筋がありません (未配線か、ベタだけでつながっています)", m.name));
    match m.what.as_str() {
        "width" => {
            let p = way(&pads[0], &pads[1])?;
            let (w, at) = crate::drc::narrowest_on_way(board, &p.points).ok_or_else(|| format!("測定「{}」: {} と {} の間に配線がありません", m.name, pads[0], pads[1]))?;
            Ok(Measured { value: w, ways: vec![p.points.iter().map(|(_, q)| *q).collect()], at: Some(at) })
        }
        "length" | "vias" => {
            let p = way(&pads[0], &pads[1])?;
            let value = if m.what == "length" { p.length } else { p.vias as f64 };
            Ok(Measured { value, ways: vec![p.points.iter().map(|(_, q)| *q).collect()], at: None })
        }
        "tap" => {
            let p = way(&pads[0], &pads[1])?;
            let (shunt, other) = (find_pad(board, &pads[0], p.points.first().map(|q| q.1)), find_pad(board, &pads[2], None));
            let (Some(a), Some(b)) = (shunt, other) else { return Err(format!("測定「{}」: {} か {} のパッドが、基板にありません", m.name, pads[0], pads[2])) };
            let (w, h) = a.shape.size();
            let (dx, dy) = (b.pos.x - a.pos.x, b.pos.y - a.pos.y);
            // the side that faces the partner pad: along x or along y
            let (ux, uy, half) = if dx.abs() > dy.abs() { (dx.signum(), 0.0, w / 2.0) } else { (0.0, dy.signum(), h / 2.0) };
            let inside = |q: &Point| (q.x - a.pos.x).abs() <= w / 2.0 + 1e-6 && (q.y - a.pos.y).abs() <= h / 2.0 + 1e-6;
            // the point where the way leaves the pad: the last of its first points that lie on the pad's copper
            let tap = p.points.iter().map(|(_, q)| *q).take_while(inside).last().unwrap_or(a.pos);
            let depth = half - ((tap.x - a.pos.x) * ux + (tap.y - a.pos.y) * uy);
            let edge = Point { x: tap.x + ux * depth, y: tap.y + uy * depth };
            Ok(Measured { value: depth.max(0.0), ways: vec![vec![tap, edge]], at: Some(tap) })
        }
        _ => {
            let f = g.fork(&pads[0], &pads[1], &pads[2]).ok_or_else(|| format!("測定「{}」: {} から {} か {} へ、銅の道筋がありません (未配線か、ベタだけでつながっています)", m.name, pads[0], pads[1], pads[2]))?;
            let (a, b) = (way(&pads[0], &pads[1])?, way(&pads[0], &pads[2])?);
            Ok(Measured { value: f.length, ways: vec![a.points.iter().map(|(_, q)| *q).collect(), b.points.iter().map(|(_, q)| *q).collect()], at: Some(f.at) })
        }
    }
}

/// The check and the marks of the table together, on one copper graph (what the editor asks for when the board has changed).
pub fn analyse(board: &Board) -> (RequirementsReport, Vec<MeasureMark>) {
    let model = needs_model(board).then(|| crate::drc::CopperModel::build(board));
    (check_with(board, model.as_ref()), measure_marks_with(board, model.as_ref()))
}

/// A number that changes when anything the check reads besides the copper changes: the rows of the table that reached the board and the
/// net classes (the editor keeps the answer of `analyse` until the copper or this changes).
pub fn requirements_hash(board: &Board) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let parts = (&board.place_groups, &board.keep_apart, &board.bypass, &board.waivers, &board.measures, &board.net_classes);
    serde_json::to_string(&parts).unwrap_or_default().hash(&mut h);
    h.finish()
}

/// Is there anything in the board for the requirements check to look at?
pub fn any(board: &Board) -> bool {
    !board.place_groups.is_empty()
        || !board.keep_apart.is_empty()
        || !board.bypass.is_empty()
        || !board.measures.is_empty()
        || board.net_classes.iter().any(|c| !c.layers.is_empty() || c.max_length > 0.0)
}

/// The properties of the net classes that the copper can break: tracks of a net on a layer its class does not allow (one finding for the
/// class, with the length), and a net with more track than its class lets it have (one for each such net).
fn net_findings(board: &Board, model: Option<&crate::drc::CopperModel>) -> (Vec<Finding>, usize) {
    let (mut out, mut checked) = (vec![], 0);
    if !board.net_classes.iter().any(|c| !c.layers.is_empty() || c.max_length > 0.0) {
        return (out, 0);
    }
    let built;
    let index = match model {
        Some(m) => &m.index,
        None => {
            built = crate::drc::CopperIndex::build(board);
            &built
        }
    };
    // per net: the length on each layer
    let mut by_net: std::collections::BTreeMap<String, Vec<(Side, f64)>> = Default::default();
    for (i, t) in board.tracks.iter().enumerate() {
        if let Some(n) = index.track_net(i) {
            by_net.entry(n.to_string()).or_default().push((t.side, t.a.dist(t.b)));
        }
    }
    for c in &board.net_classes {
        if !c.layers.is_empty() {
            checked += 1;
            let mut off: Vec<(Side, f64)> = vec![];
            for n in &c.nets {
                for (side, len) in by_net.get(n).into_iter().flatten().filter(|(s, _)| !c.layers.contains(s)) {
                    match off.iter_mut().find(|(s, _)| s == side) {
                        Some(e) => e.1 += len,
                        None => off.push((*side, *len)),
                    }
                }
            }
            let total: f64 = off.iter().map(|(_, l)| l).sum();
            if total > 1e-6 {
                let allowed: Vec<String> = c.layers.iter().map(|s| s.label()).collect();
                let on: Vec<String> = off.iter().map(|(s, l)| format!("{} {l:.1} mm", s.label())).collect();
                out.push(Finding {
                    kind: FindingKind::Net,
                    name: c.name.clone(),
                    item: String::new(),
                    message: format!("ネット「{}」: 許している層 ({}) の外に配線があります: {}", c.name, allowed.join("・"), on.join("、")),
                    actual: total,
                    limit: 0.0,
                });
            }
        }
        if c.max_length > 0.0 {
            for n in &c.nets {
                checked += 1;
                let len: f64 = by_net.get(n).into_iter().flatten().map(|(_, l)| l).sum();
                if len > c.max_length + 1e-9 {
                    out.push(Finding {
                        kind: FindingKind::Net,
                        name: c.name.clone(),
                        item: n.clone(),
                        message: format!("ネット {n} (「{}」): 配線の長さが {len:.1} mm です (上限 {} mm)", c.name, c.max_length),
                        actual: len,
                        limit: c.max_length,
                    });
                }
            }
        }
    }
    (out, checked)
}

pub fn check(board: &Board) -> RequirementsReport {
    check_with(board, None)
}

/// Does the check need the copper model (measures, or a net class with layers or a length limit)?
fn needs_model(board: &Board) -> bool {
    !board.measures.is_empty() || board.net_classes.iter().any(|c| !c.layers.is_empty() || c.max_length > 0.0)
}

/// [`check`], with the copper model when the caller has built it (a few ms on a big board; it is shared by all the rows).
pub fn check_with(board: &Board, model: Option<&crate::drc::CopperModel>) -> RequirementsReport {
    let mut rep = RequirementsReport::default();
    let built = (model.is_none() && needs_model(board)).then(|| crate::drc::CopperModel::build(board));
    let model = model.or(built.as_ref());
    let (nets, n) = net_findings(board, model);
    rep.checked += n;
    for f in nets {
        rep.add(board, f);
    }
    // the measures of the copper of a connection (the graph is built once, and only when there is something to measure)
    if let Some(model) = model.filter(|_| !board.measures.is_empty()) {
        for m in &board.measures {
            rep.checked += 1;
            match measure_one(board, model, m) {
                Err(why) => {
                    rep.checked -= 1;
                    rep.missing.push(why);
                }
                Ok(v) => {
                    let (label, unit, _) = m.kind().unwrap_or(("", "", 0));
                    let bad = if m.is_min() { v.value < m.limit - 1e-9 } else { v.value > m.limit + 1e-9 };
                    if bad {
                        let shown = match unit {
                            "個" => format!("{:.0}", v.value),
                            "%" => format!("{:.1}", v.value),
                            _ => format!("{:.2}", v.value),
                        };
                        rep.add(board, Finding {
                            kind: FindingKind::Measure,
                            name: m.name.clone(),
                            item: String::new(),
                            message: format!("測定「{}」: {label}が {shown} {unit} です ({} {} {unit})", m.name, if m.is_min() { "下限" } else { "上限" }, m.limit),
                            actual: v.value,
                            limit: m.limit,
                        });
                    }
                }
            }
        }
    }
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
            rep.add(board, Finding {
                kind: FindingKind::Group,
                name: g.name.clone(),
                item: String::new(),
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
            rep.add(board, Finding {
                kind: FindingKind::Apart,
                name: a.name.clone(),
                item: String::new(),
                message: format!("離す組「{}」: {na} と {nb} が {d:.1} mm まで近づいています (下限 {} mm)", a.name, a.min_dist),
                actual: d,
                limit: a.min_dist,
            });
        }
    }
    for b in &board.bypass {
        for cap in &b.caps {
            let on = |r: &str| board.parts.iter().any(|p| p.refdes == r);
            if !on(cap) || !on(&b.ic) {
                rep.missing.push(format!("{} (バイパスの対応「{}」)", if on(cap) { &b.ic } else { cap }, b.ic));
                continue;
            }
            let Some((_, _, d)) = bypass_pair(board, cap, &b.ic) else { continue };
            rep.checked += 1;
            if d > crate::drc::MAX_DECOUPLING_DISTANCE {
                rep.add(board, Finding {
                    kind: FindingKind::Bypass,
                    name: b.ic.clone(),
                    item: cap.clone(),
                    message: format!("バイパスコンデンサ {cap} ({} 用): {} の電源ピンから {d:.1} mm 離れています (目安 {} mm 以内)", b.ic, b.ic, crate::drc::MAX_DECOUPLING_DISTANCE),
                    actual: d,
                    limit: crate::drc::MAX_DECOUPLING_DISTANCE,
                });
            }
        }
    }
    rep
}

/// A measure of the table as it stands on the board, for drawing: the ways measured, where they part, the value and whether it is within the limit.
#[derive(Clone, Debug)]
pub struct MeasureMark {
    pub name: String,
    pub ways: Vec<Vec<Point>>,
    pub at: Option<Point>,
    pub value: f64,
    pub limit: f64,
    pub is_min: bool,
    pub unit: String,
    pub ok: bool,
    pub waived: bool,
}

/// The measures of the table, measured (those that cannot be measured are left out). Builds the copper graph: ask for it only when the
/// board changed.
pub fn measure_marks(board: &Board) -> Vec<MeasureMark> {
    measure_marks_with(board, None)
}

/// [`measure_marks`], with the copper graph when the caller has built it.
pub fn measure_marks_with(board: &Board, model: Option<&crate::drc::CopperModel>) -> Vec<MeasureMark> {
    if board.measures.is_empty() {
        return vec![];
    }
    let built;
    let g = match model {
        Some(m) => m,
        None => {
            built = crate::drc::CopperModel::build(board);
            &built
        }
    };
    board
        .measures
        .iter()
        .filter_map(|m| {
            let v = measure_one(board, g, m).ok()?;
            let ok = if m.is_min() { v.value >= m.limit - 1e-9 } else { v.value <= m.limit + 1e-9 };
            let waived = board.waivers.iter().any(|w| w.kind == "measure" && w.name == m.name);
            Some(MeasureMark { name: m.name.clone(), ways: v.ways, at: v.at, value: v.value, limit: m.limit, is_min: m.is_min(), unit: m.kind().map_or("", |k| k.1).to_string(), ok, waived })
        })
        .collect()
}

/// A group of the table as it stands on the board, for drawing: the parts that are on the board, how far apart the farthest two are
/// (0 for fewer than two parts), the limit (0: not checked) and whether it is kept.
#[derive(Clone, Debug)]
pub struct GroupMark {
    pub name: String,
    pub parts: Vec<String>,
    /// The middle of the parts' pads.
    pub at: Point,
    pub actual: f64,
    pub limit: f64,
    pub ok: bool,
    /// Not kept but accepted by a waiver of the table.
    pub waived: bool,
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
    pub waived: bool,
}

/// A bypass capacitor and the IC pin it serves (from the table): the capacitor's supply pad, the pin, their distance and whether it is near enough.
#[derive(Clone, Debug)]
pub struct BypassMark {
    pub cap: String,
    pub ic: String,
    pub a: Point,
    pub b: Point,
    pub actual: f64,
    pub ok: bool,
    pub waived: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Marks {
    pub groups: Vec<GroupMark>,
    pub apart: Vec<ApartMark>,
    pub bypass: Vec<BypassMark>,
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
        let n = mids.len().max(1) as f64;
        let at = Point::new(mids.iter().map(|p| p.x).sum::<f64>() / n, mids.iter().map(|p| p.y).sum::<f64>() / n);
        out.groups.push(GroupMark { name: g.name.clone(), parts, at, actual, limit: g.max_dist, ok: g.max_dist <= 0.0 || actual <= g.max_dist, waived: board.waivers.iter().any(|w| w.kind == "group" && w.name == g.name) });
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
            out.apart.push(ApartMark { name: a.name.clone(), a: qa, b: qb, actual: d, limit: a.min_dist, ok: a.min_dist <= 0.0 || d >= a.min_dist, waived: board.waivers.iter().any(|w| w.kind == "apart" && w.name == a.name) });
        }
    }
    for b in &board.bypass {
        for cap in &b.caps {
            if let Some((a, p, d)) = bypass_pair(board, cap, &b.ic) {
                out.bypass.push(BypassMark { cap: cap.clone(), ic: b.ic.clone(), a, b: p, actual: d, ok: d <= crate::drc::MAX_DECOUPLING_DISTANCE, waived: board.waivers.iter().any(|w| w.kind == "bypass" && w.name == b.ic && (w.item.is_empty() || w.item == *cap)) });
            }
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
    fn the_pre_order_check_reports_the_requirements_that_are_not_met() {
        use crate::fab::{preflight, Level};
        let has = |b: &Board, level: Level| preflight(b, None).iter().any(|c| c.level == level && c.title.contains("設計の要件"));
        let mut b = board();
        assert!(!has(&b, Level::Warn) && !has(&b, Level::Ok), "no table, nothing to say");
        b.place_groups.push(design_spec::PlaceGroup { name: "g".into(), parts: vec!["R1".into(), "R3".into()], max_dist: 10.0, ..Default::default() });
        assert!(has(&b, Level::Warn), "R1 and R3 are 30 mm apart");
        b.place_groups[0].max_dist = 50.0;
        assert!(has(&b, Level::Ok) && !has(&b, Level::Warn));
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

    fn pad_at(b: &mut Board, x: f64, y: f64, r: &str, pin: &str) {
        let mut p = Pad::new(Point::new(x, y), PadShape::Circle { d: 1.0 }, None, Side::Top);
        p.refdes = r.into();
        p.pin = pin.into();
        p.net = "N".into();
        b.pads.push(p);
    }

    fn wire(b: &mut Board, a: (f64, f64), c: (f64, f64)) {
        b.tracks.push(Track { side: Side::Top, a: Point::new(a.0, a.1), b: Point::new(c.0, c.1), width: 0.25 });
    }

    /// The measures of the copper of a connection: a branch that taps the force track 3 mm from the pad breaks a fork limit of 0.5 mm, a way
    /// that is longer than its limit is found, the pads without copper between them are `missing`, and the marks carry the ways to draw.
    #[test]
    fn measures_of_the_copper_are_checked_against_their_limits() {
        let mut b = Board::new("t", 50.0, 50.0);
        pad_at(&mut b, 5.0, 5.0, "R", "1");
        pad_at(&mut b, 25.0, 5.0, "F", "1");
        pad_at(&mut b, 8.0, 15.0, "S", "1");
        pad_at(&mut b, 40.0, 40.0, "Z", "1");
        wire(&mut b, (5.0, 5.0), (25.0, 5.0));
        wire(&mut b, (8.0, 5.0), (8.0, 15.0));
        let m = |name: &str, what: &str, pads: &[&str], op: &str, limit: f64| design_spec::Measure { name: name.into(), what: what.into(), pads: pads.iter().map(|s| s.to_string()).collect(), op: op.into(), limit, reach: 0.0, except: vec![], note: String::new() };
        let spec = DesignSpec {
            measures: vec![
                m("検出線", "fork", &["R.1", "F.1", "S.1"], "max", 0.5),
                m("短く", "length", &["R.1", "F.1"], "max", 25.0),
                m("長い", "length", &["R.1", "F.1"], "max", 10.0),
                m("ビアなし", "vias", &["R.1", "S.1"], "max", 0.0),
                m("下限", "length", &["R.1", "S.1"], "min", 20.0),
                m("道なし", "length", &["R.1", "Z.1"], "max", 5.0),
            ],
            ..Default::default()
        };
        import_requirements(&mut b, &spec);
        let rep = check(&b);
        let names: Vec<&str> = rep.findings.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["検出線", "長い", "下限"], "{:?}", rep.findings);
        assert!(rep.findings.iter().all(|f| f.kind == FindingKind::Measure));
        assert!(rep.findings[0].message.contains("3.00 mm"), "{}", rep.findings[0].message);
        assert!(rep.missing.iter().any(|m| m.contains("道なし")), "{:?}", rep.missing);
        let marks = measure_marks(&b);
        assert_eq!(marks.len(), 5, "the one without copper is left out");
        assert_eq!(marks[0].ways.len(), 2);
        assert!(marks[0].at.is_some_and(|p| p.dist(Point::new(8.0, 5.0)) < 1e-3) && !marks[0].ok);
        assert!(marks[1].ok && marks[3].ok);
        // a waiver on a measure moves it aside
        b.waivers.push(design_spec::Waiver { kind: "measure".into(), name: "検出線".into(), note: "試作".into(), ..Default::default() });
        let rep = check(&b);
        assert_eq!(rep.findings.len(), 2);
        assert_eq!(rep.waived.len(), 1);
        assert!(measure_marks(&b)[0].waived);
    }

    /// A tap: how far from the inner edge of the shunt pad (the edge that faces the other pad) a sense wire meets the pad's copper.
    #[test]
    fn a_sense_tap_is_measured_from_the_inner_edge_of_the_pad() {
        let mut b = Board::new("t", 50.0, 50.0);
        // two 3.0 x 2.7 pads, 3.3 apart (centres), the sense pad far away
        for (x, y, r, pin) in [(10.0, 20.0, "RS", "1"), (10.0, 16.7, "RS", "2"), (30.0, 20.0, "SS", "1")] {
            let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w: 3.0, h: 2.7 }, None, Side::Top);
            p.refdes = r.into();
            p.pin = pin.into();
            p.net = "N".into();
            b.pads.push(p);
        }
        let m = |name: &str, limit: f64| design_spec::Measure { name: name.into(), what: "tap".into(), pads: ["RS.1", "SS.1", "RS.2"].iter().map(|s| s.to_string()).collect(), op: "max".into(), limit, reach: 0.0, except: vec![], note: String::new() };
        // the sense wire starts at the outer side of the pad (y 20.5 is 0.5 below the centre, away from the partner above): 1.35 + 0.5 = 1.85 mm
        wire(&mut b, (10.0, 20.5), (30.0, 20.5));
        wire(&mut b, (30.0, 20.5), (30.0, 20.0));
        let spec = DesignSpec { measures: vec![m("外側", 0.5)], ..Default::default() };
        import_requirements(&mut b, &spec);
        let rep = check(&b);
        assert_eq!(rep.findings.len(), 1, "{:?} {:?}", rep.findings, rep.missing);
        assert!(rep.findings[0].message.contains("1.85 mm"), "{}", rep.findings[0].message);
        // moved to the inner side (y 19.0, 0.35 mm from the inner edge at 18.65): the tap is 0.35 mm in
        b.tracks.clear();
        wire(&mut b, (10.0, 19.0), (30.0, 19.0));
        wire(&mut b, (30.0, 19.0), (30.0, 20.0));
        let rep = check(&b);
        assert!(rep.findings.is_empty(), "{:?} {:?}", rep.findings, rep.missing);
        let marks = measure_marks(&b);
        assert!((marks[0].value - 0.35).abs() < 1e-6, "{}", marks[0].value);
    }

    /// The gap between the copper of two nets (names or patterns): the nearest items, edge to edge.
    #[test]
    fn the_gap_between_two_nets_is_measured_edge_to_edge() {
        let mut b = Board::new("t", 50.0, 50.0);
        pad_at(&mut b, 5.0, 5.0, "A", "1");
        pad_at(&mut b, 25.0, 5.0, "A", "2");
        pad_at(&mut b, 5.0, 8.0, "B", "1");
        pad_at(&mut b, 25.0, 8.0, "B", "2");
        b.pads[0].net = "SA".into();
        b.pads[1].net = "SA".into();
        b.pads[2].net = "SW1".into();
        b.pads[3].net = "SW1".into();
        wire(&mut b, (5.0, 5.0), (25.0, 5.0));
        wire(&mut b, (5.0, 8.0), (25.0, 8.0));
        let m = |name: &str, pads: &[&str], op: &str, limit: f64| design_spec::Measure { name: name.into(), what: "gap".into(), pads: pads.iter().map(|s| s.to_string()).collect(), op: op.into(), limit, reach: 0.0, except: vec![], note: String::new() };
        let spec = DesignSpec {
            measures: vec![m("検出を離す", &["SA", "SW*"], "min", 5.0), m("十分", &["SA", "SW1"], "min", 1.0), m("なし", &["SA", "ZZ"], "min", 1.0)],
            ..Default::default()
        };
        import_requirements(&mut b, &spec);
        let rep = check(&b);
        assert_eq!(rep.findings.len(), 1, "{:?}", rep.findings);
        // the nearest copper is the pads (1 mm wide, 3 mm apart): 2.00 mm edge to edge, closer than the tracks (2.75 mm)
        assert!(rep.findings[0].message.contains("2.00 mm"), "{}", rep.findings[0].message);
        assert!(rep.missing.iter().any(|m| m.contains("なし")), "{:?}", rep.missing);
        let marks = measure_marks(&b);
        assert_eq!(marks.len(), 2);
        assert_eq!(marks[0].ways[0].len(), 2, "a line between the two nearest items");
    }

    /// A gap can leave the pads of some parts out (or all pads): what is left is the wiring.
    #[test]
    fn a_gap_can_leave_pads_out_and_an_area_is_measured() {
        let mut b = Board::new("t", 50.0, 50.0);
        pad_at(&mut b, 5.0, 5.0, "A", "1");
        pad_at(&mut b, 25.0, 5.0, "A", "2");
        pad_at(&mut b, 5.0, 8.0, "B", "1");
        pad_at(&mut b, 25.0, 8.0, "B", "2");
        for (k, net) in ["SA", "SA", "SW1", "SW1"].iter().enumerate() {
            b.pads[k].net = (*net).into();
        }
        wire(&mut b, (5.0, 5.0), (25.0, 5.0));
        wire(&mut b, (5.0, 8.0), (25.0, 8.0));
        let m = |what: &str, pads: &[&str], except: &[&str], op: &str, limit: f64| design_spec::Measure { name: "m".into(), what: what.into(), pads: pads.iter().map(|s| s.to_string()).collect(), op: op.into(), limit, reach: 0.0, except: except.iter().map(|s| s.to_string()).collect(), note: String::new() };
        let model = crate::drc::CopperModel::build(&b);
        // pads count: the nearest copper is the pads (2.00 mm); without any pad, the tracks (2.75 mm); without the pads of B, the pad of A against the track of B (2.375 mm)
        let all = measure_one(&b, &model, &m("gap", &["SA", "SW1"], &[], "min", 1.0)).unwrap().value;
        let wiring = measure_one(&b, &model, &m("gap", &["SA", "SW1"], &["pads"], "min", 1.0)).unwrap().value;
        let part = measure_one(&b, &model, &m("gap", &["SA", "SW1"], &["B"], "min", 1.0)).unwrap().value;
        assert!((all - 2.0).abs() < 0.01 && (wiring - 2.75).abs() < 0.01 && (part - 2.375).abs() < 0.01, "{all} {wiring} {part}");
        // the area of the copper at a pad: the pad (1 x 1 mm, as pad_at makes it) and the track of 20 mm by its width
        let a = measure_one(&b, &model, &m("area", &["A.1"], &[], "min", 10.0)).unwrap().value;
        assert!(a > 1.0 && a < 40.0, "{a}");
        assert!(measure_one(&b, &model, &m("area", &["Z.9"], &[], "min", 10.0)).is_err());
    }

    /// `width`: the narrowest track on the way between two pads; `parallel`: how long two nets run side by side.
    #[test]
    fn the_narrowest_width_and_the_parallel_length_are_measured() {
        let mut b = Board::new("t", 50.0, 50.0);
        pad_at(&mut b, 5.0, 5.0, "A", "1");
        pad_at(&mut b, 25.0, 5.0, "A", "2");
        pad_at(&mut b, 5.0, 8.0, "B", "1");
        pad_at(&mut b, 25.0, 8.0, "B", "2");
        for (k, net) in ["SA", "SA", "SW1", "SW1"].iter().enumerate() {
            b.pads[k].net = (*net).into();
        }
        // SA: 20 mm of 0.5 mm track with a 0.2 mm piece in the middle; SW1: 20 mm of 0.25 mm track 3 mm away
        b.tracks.push(Track { side: Side::Top, a: Point::new(5.0, 5.0), b: Point::new(12.0, 5.0), width: 0.5 });
        b.tracks.push(Track { side: Side::Top, a: Point::new(12.0, 5.0), b: Point::new(15.0, 5.0), width: 0.2 });
        b.tracks.push(Track { side: Side::Top, a: Point::new(15.0, 5.0), b: Point::new(25.0, 5.0), width: 0.5 });
        wire(&mut b, (5.0, 8.0), (25.0, 8.0));
        let m = |what: &str, pads: &[&str], reach: f64, op: &str, limit: f64| design_spec::Measure { name: "m".into(), what: what.into(), pads: pads.iter().map(|s| s.to_string()).collect(), op: op.into(), limit, reach, except: vec![], note: String::new() };
        let model = crate::drc::CopperModel::build(&b);
        let w = measure_one(&b, &model, &m("width", &["A.1", "A.2"], 0.0, "min", 0.4)).unwrap();
        assert!((w.value - 0.2).abs() < 1e-9, "{}", w.value);
        assert!(w.at.is_some_and(|p| (p.x - 13.5).abs() < 0.01), "{:?}", w.at);
        // the nets run 3 mm apart (edge to edge 2.75 mm): within 1 mm nothing, within 3 mm the whole of SA
        let near = measure_one(&b, &model, &m("parallel", &["SA", "SW1"], 1.0, "max", 0.0)).unwrap();
        assert!(near.value < 1e-9, "{}", near.value);
        let far = measure_one(&b, &model, &m("parallel", &["SA", "SW1"], 3.0, "max", 0.0)).unwrap();
        assert!((far.value - 20.0).abs() < 0.3 && !far.ways.is_empty(), "{}", far.value);
        // without the pads of B (they sit at the ends of its track) it is the same track that counts
        let mut pm = m("parallel", &["SA", "SW1"], 3.0, "max", 0.0);
        pm.except = vec!["pads".into()];
        assert!((measure_one(&b, &model, &pm).unwrap().value - 20.0).abs() < 0.3);
        // a net with no track cannot be measured
        assert!(measure_one(&b, &model, &m("parallel", &["ZZ", "SW1"], 1.0, "max", 0.0)).is_err());
    }

    /// The properties of a net class that the copper can break: tracks on a layer the class does not allow, and a net with more track than
    /// its class lets it have. A waiver of kind "net" accepts them.
    #[test]
    fn the_layers_and_the_length_of_a_net_class_are_checked() {
        let mut b = Board::new("t", 50.0, 50.0);
        pad_at(&mut b, 5.0, 5.0, "R", "1");
        pad_at(&mut b, 25.0, 5.0, "F", "1");
        b.pads[0].net = "SA".into();
        b.pads[1].net = "SA".into();
        // 20 mm of track on the top, 10 mm more on the bottom (joined by a via), all of net SA
        wire(&mut b, (5.0, 5.0), (25.0, 5.0));
        b.vias.push(Via { pos: Point::new(25.0, 5.0), diameter: 0.6, drill: 0.3, net: "SA".into() });
        b.tracks.push(Track { side: Side::Bottom, a: Point::new(25.0, 5.0), b: Point::new(35.0, 5.0), width: 0.25 });
        let spec = DesignSpec {
            net_rules: vec![design_spec::NetRule { name: "検出".into(), nets: vec!["SA".into()], layers: vec!["top".into()], max_length: 25.0, ..Default::default() }],
            ..Default::default()
        };
        import_requirements(&mut b, &spec);
        assert!(any(&b));
        let rep = check(&b);
        assert_eq!(rep.findings.len(), 2, "{:?}", rep.findings);
        assert!(rep.findings.iter().all(|f| f.kind == FindingKind::Net));
        assert!(rep.findings.iter().any(|f| f.item.is_empty() && f.message.contains("裏面") && f.message.contains("10.0 mm")), "{:?}", rep.findings);
        assert!(rep.findings.iter().any(|f| f.item == "SA" && f.message.contains("30.0 mm")), "{:?}", rep.findings);
        // accepted: the layers finding by the class, the length by the net
        b.waivers.push(design_spec::Waiver { kind: "net".into(), name: "検出".into(), item: "SA".into(), note: "試作".into() });
        let rep = check(&b);
        assert_eq!((rep.findings.len(), rep.waived.len()), (1, 1));
        // allowing both layers: only the length is left; a long enough limit: nothing
        b.net_classes[0].layers = vec![Side::Top, Side::Bottom];
        b.net_classes[0].max_length = 40.0;
        b.waivers.clear();
        assert!(check(&b).findings.is_empty());
    }

    /// A waiver of the table moves a finding from "wrong" to "accepted, and why": it is not counted, the mark says so, and the pre-order
    /// check lists the reason.
    #[test]
    fn an_accepted_exception_is_listed_apart_with_its_reason() {
        let mut b = board();
        let mut spec = DesignSpec {
            groups: vec![PlaceGroup { name: "入口".into(), parts: vec!["R1".into(), "R2".into(), "R3".into()], max_dist: 10.0, ..Default::default() }],
            apart: vec![KeepApart { name: "離す".into(), a: vec!["R1".into()], b: vec!["R2".into()], min_dist: 20.0, ..Default::default() }],
            ..Default::default()
        };
        spec.waivers.push(design_spec::Waiver { kind: "apart".into(), name: "離す".into(), note: "基板が小さいので承知".into(), ..Default::default() });
        import_requirements(&mut b, &spec);
        let rep = check(&b);
        assert_eq!(rep.findings.len(), 1, "{:?}", rep.findings);
        assert_eq!(rep.findings[0].kind, FindingKind::Group);
        assert_eq!(rep.waived.len(), 1);
        assert_eq!((rep.waived[0].0.kind, rep.waived[0].1.as_str()), (FindingKind::Apart, "基板が小さいので承知"));
        let marks = marks(&b);
        assert!(marks.apart[0].waived && !marks.apart[0].ok);
        assert!(!marks.groups[0].waived && !marks.groups[0].ok);
        let list = crate::fab::preflight(&b, None);
        assert!(list.iter().any(|c| c.title.contains("承知した例外") && c.detail.contains("基板が小さいので承知")), "{list:?}");
        // a waiver for another row does not cover it
        b.waivers[0].name = "別の行".into();
        assert_eq!(check(&b).findings.len(), 2);
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
