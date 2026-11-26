//! What the requirements table asks of the placement (`Wants`): the parts kept apart, the groups, the sections, the locked parts, the
//! bypass capacitors by their IC (`bypass_gap`, `bypass_pairs`).

use super::*;

/// How far (holes, straight) a bypass capacitor's leads are from its IC: of its leads, the farthest from the IC's nearest pin of the
/// lead's net (a lead with no such pin counts nothing). An estimate of `stripboard::bypass_spans`, which goes along the copper.
pub(super) fn bypass_gap(c: &Circuit, cap: &str, leads: &[(String, At)], ic: &StripPart) -> f64 {
    let mut worst = 0.0f64;
    for (pin, a) in leads {
        let net = lead_net(c, cap, pin);
        if net.is_empty() {
            continue;
        }
        let near = ic.leads.iter().filter(|(ip, _)| lead_net(c, &ic.refdes, ip) == net).map(|(_, b)| (a.0 as f64 - b.0 as f64).hypot(a.1 as f64 - b.1 as f64)).fold(f64::INFINITY, f64::min);
        if near.is_finite() {
            worst = worst.max(near);
        }
    }
    worst
}

/// The bypass capacitors' leads on a placed board and the IC's pin each serves: for each lead on a net of its IC, the IC's nearest
/// pin of that net (the pin's hole, the lead's hole), for the wiring (`Order::bypass`).
pub(super) fn bypass_pairs(c: &Circuit, wants: &Wants, l: &StripLayout) -> Vec<(At, At)> {
    let mut out = vec![];
    for (cap, ic, _) in &wants.bypass {
        let (Some(cp), Some(ip)) = (l.parts.iter().find(|p| &p.refdes == cap), l.parts.iter().find(|p| &p.refdes == ic)) else { continue };
        for (pin, a) in &cp.leads {
            let net = lead_net(c, cap, pin);
            if net.is_empty() {
                continue;
            }
            let d = |b: &At| a.0.abs_diff(b.0) as u32 + a.1.abs_diff(b.1) as u32;
            if let Some(b) = ip.leads.iter().filter(|(x, _)| lead_net(c, ic, x) == net).map(|x| x.1).min_by_key(d) {
                out.push((b, *a));
            }
        }
    }
    out
}

/// What the requirements table asks of the placement: the parts kept apart (each rule's two sets of parts and the distance in
/// holes), the groups each part is in (placed together), and the sections (each placed in a band of rows of its own).
#[derive(Default)]
pub(super) struct Wants {
    pub(super) apart: Vec<(BTreeSet<String>, BTreeSet<String>, f64)>,
    pub(super) groups: BTreeMap<String, Vec<usize>>,
    /// Each group's farthest distance between two of its parts' centres, in holes (the table's `max_dist`; 0: none).
    pub(super) group_max: Vec<f64>,
    /// The parts of a group with a farthest distance whose other parts are in a section further down (`last`: placed at the bottom of
    /// their own section) or further up (`early`: at its top, after the headers), so that the two halves of the group face each other
    /// across the boundary of the two sections (L10).
    pub(super) last: BTreeSet<String>,
    pub(super) early: BTreeSet<String>,
    /// The bypass capacitors of the table: each capacitor, its IC and the farthest its leads may be from the IC's pins, in holes (L17).
    pub(super) bypass: Vec<(String, String, f64)>,
    pub(super) sections: Vec<(String, BTreeSet<String>)>,
    /// The parts locked where they are (`perf_layout_around`): placed first as they are, never moved.
    pub(super) keep: Vec<StripPart>,
    /// The sections' boxes put by hand (`StripLayout::pinned`): each section's name, its top-left hole and its width.
    pub(super) boxes: BTreeMap<String, (At, u16)>,
    /// The holes kept free round the mounting holes (`StripOptions::keepout`), in the whole board's holes.
    pub(super) keepout: BTreeSet<At>,
}

impl Wants {
    pub(super) fn read(s: &Schematic, c: &Circuit) -> Wants {
        // a rule's item: a part by its name, else every part with a lead on that net
        let parts_of = |items: &[String]| -> BTreeSet<String> {
            c.on.iter()
                .filter(|(r, _, pins)| items.iter().any(|it| design_spec::matches_name(it, r) || pins.iter().any(|(_, n)| !n.is_empty() && design_spec::matches_name(it, n))))
                .map(|(r, _, _)| r.clone())
                .collect()
        };
        let apart = s.requirements.apart.iter().map(|k| (parts_of(&k.a), parts_of(&k.b), k.min_dist / 2.54)).collect();
        let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, g) in s.requirements.groups.iter().enumerate() {
            for p in &g.parts {
                groups.entry(p.clone()).or_default().push(i);
            }
        }
        let group_max: Vec<f64> = s.requirements.groups.iter().map(|g| g.max_dist.max(0.0) / 2.54).collect();
        let sections = crate::stripboard::sections(s, c);
        let (mut last, mut early) = (BTreeSet::new(), BTreeSet::new());
        for (g, _) in group_max.iter().enumerate().filter(|(_, m)| **m > 0.0) {
            let section_of = |r: &str| sections.iter().position(|(_, m)| m.contains(r));
            let members: Vec<(String, usize)> = c.on.iter().filter(|p| groups.get(&p.0).is_some_and(|v| v.contains(&g))).filter_map(|p| Some((p.0.clone(), section_of(&p.0)?))).collect();
            for (r, k) in &members {
                if members.iter().any(|(_, j)| j > k) {
                    last.insert(r.clone());
                } else if members.iter().any(|(_, j)| j < k) {
                    early.insert(r.clone());
                }
            }
        }
        let on = |r: &str| c.on.iter().any(|p| p.0 == r);
        let bypass = s.requirements.bypass.iter().filter(|b| on(&b.ic)).flat_map(|b| b.caps.iter().filter(|cap| on(cap)).map(move |cap| (cap.clone(), b.ic.clone(), b.limit() / 2.54))).collect();
        Wants { apart, groups, group_max, last, early, bypass, sections, keep: vec![], boxes: BTreeMap::new(), keepout: BTreeSet::new() }
    }

    /// Whether a part is locked where it is.
    pub(super) fn kept(&self, refdes: &str) -> bool {
        self.keep.iter().any(|p| p.refdes == refdes)
    }

    /// The holes the locked parts take (a chip its outline, a two-lead part its leads and the hole between).
    pub(super) fn kept_holes(&self) -> BTreeSet<At> {
        self.keep.iter().flat_map(outline).collect()
    }

    /// The narrowest board the locked parts fit on (their rightmost hole and a free column).
    pub(super) fn kept_width(&self) -> u16 {
        self.keep.iter().flat_map(|p| p.leads.iter().map(|x| x.1 .1 + 2)).max().unwrap_or(0)
    }

    /// Whether a part with these leads keeps its distance from the parts already placed.
    pub(super) fn far_enough(&self, refdes: &str, leads: &[At], placed: &[StripPart]) -> bool {
        if !self.apart.iter().any(|(a, b, _)| a.contains(refdes) || b.contains(refdes)) {
            return true;
        }
        let gap = |p: &StripPart| leads.iter().flat_map(|a| p.leads.iter().map(move |(_, b)| (a.0 as f64 - b.0 as f64).hypot(a.1 as f64 - b.1 as f64))).fold(f64::INFINITY, f64::min);
        self.apart.iter().all(|(a, b, d)| {
            placed.iter().filter(|p| p.refdes != refdes).all(|p| {
                let pair = (a.contains(refdes) && b.contains(&p.refdes)) || (b.contains(refdes) && a.contains(&p.refdes));
                !pair || gap(p) >= *d
            })
        })
    }

    /// How far the groups with a farthest distance (`group_max`) are past it among `parts` (holes, summed; a group with fewer than two
    /// of its parts there counts nothing).
    pub(super) fn group_over(&self, parts: &[StripPart]) -> f64 {
        let mut over = 0.0;
        for (g, limit) in self.group_max.iter().enumerate().filter(|(_, m)| **m > 0.0) {
            let cs: Vec<(f64, f64)> = parts
                .iter()
                .filter(|p| !p.leads.is_empty() && self.groups.get(&p.refdes).is_some_and(|v| v.contains(&g)))
                .map(|p| {
                    let n = p.leads.len() as f64;
                    (p.leads.iter().map(|x| x.1 .0 as f64).sum::<f64>() / n, p.leads.iter().map(|x| x.1 .1 as f64).sum::<f64>() / n)
                })
                .collect();
            let far = cs.iter().flat_map(|a| cs.iter().map(move |b| (a.0 - b.0).hypot(a.1 - b.1))).fold(0.0, f64::max);
            over += (far - limit).max(0.0);
        }
        over
    }

    /// The IC a bypass capacitor serves and how far (holes) its leads may be from the IC's pins.
    pub(super) fn bypass_of(&self, cap: &str) -> Option<(&str, f64)> {
        self.bypass.iter().find(|b| b.0 == cap).map(|b| (b.1.as_str(), b.2))
    }

    /// Whether two parts are in a group together.
    pub(super) fn together(&self, x: &str, y: &str) -> bool {
        self.groups.get(x).is_some_and(|gx| self.groups.get(y).is_some_and(|gy| gx.iter().any(|g| gy.contains(g))))
    }
}
