//! The board checked against the schematic (what it joins or keeps apart) and the requirements table's rules.

use super::*;

/// Where the layout leaves a net of the schematic apart: for each net on more than one group of joined holes (pieces of strip and
/// the wires between them), the lines that would join its groups, nearest leads first, as on a board's ratsnest.
pub fn strip_unjoined(s: &Schematic, l: &StripLayout) -> Vec<(String, At, At)> {
    let pieces = l.pieces();
    let piece_of = |at: At| pieces.iter().position(|(r, a, b)| *r == at.0 && at.1 >= *a && at.1 <= *b);
    let mut parent: Vec<usize> = (0..pieces.len()).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut i = i;
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for (ja, jb) in l.joins() {
        if let (Some(a), Some(b)) = (piece_of(ja), piece_of(jb)) {
            let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
            parent[ra] = rb;
        }
    }
    let nets = extract(&s.flat());
    let net_of = |r: &str, pin: &str| nets.iter().find(|n| n.pins.iter().any(|p| p.refdes == r && p.pin == pin)).map(|n| n.name.clone()).unwrap_or_default();
    let mut groups_of: BTreeMap<String, BTreeMap<usize, Vec<At>>> = BTreeMap::new();
    let leads = l.parts.iter().flat_map(|p| p.leads.iter().map(move |(pin, at)| (p.refdes.clone(), pin.clone(), *at))).chain(l.off.iter().flat_map(|(r, v)| v.iter().map(move |(pin, _, at)| (r.clone(), pin.clone(), *at))));
    for (r, pin, at) in leads {
        let net = net_of(&r, &pin);
        if net.is_empty() {
            continue;
        }
        let Some(p) = piece_of(at) else { continue };
        let g = find(&mut parent, p);
        groups_of.entry(net).or_default().entry(g).or_default().push(at);
    }
    let mut out = vec![];
    for (net, gs) in groups_of.into_iter().filter(|(_, gs)| gs.len() > 1) {
        let gs: Vec<Vec<At>> = gs.into_values().collect();
        for (a, b) in nearest_joins(&gs, |a: At, b: At| (a.0 as f64 - b.0 as f64).hypot(a.1 as f64 - b.1 as f64)) {
            out.push((net.clone(), a, b));
        }
    }
    out
}

/// What the layout joins that the schematic does not, or keeps apart that it joins: one sentence each (empty when it matches).
pub fn strip_check(s: &Schematic, l: &StripLayout) -> Vec<String> {
    let mut problems = vec![];
    // two things in one hole, or a lead in a cut
    let mut seen: BTreeMap<At, String> = BTreeMap::new();
    // a chip on the copper side shares its pad with a lead of its net (soldered on it): not two things in one hole (a lead of another
    // net there shows as the nets joined, below)
    let backs: BTreeSet<&str> = l.parts.iter().filter(|p| matches!(p.mount, Mount::Back { .. })).map(|p| p.refdes.as_str()).collect();
    let on_back = |who: &str| who.split_once('.').is_some_and(|(r, _)| backs.contains(r));
    for (at, who) in l.used() {
        if l.cuts.contains(&at) {
            problems.push(format!("{who} が切った穴 {} にあります", hole_name(at)));
        }
        if let Some(other) = seen.insert(at, who.clone())
            && !(other.starts_with("link") && who.starts_with("link") && other == who)
            && !(on_back(&other) != on_back(&who))
        {
            problems.push(format!("{} に {other} と {who} の 2 つが入っています", hole_name(at)));
        }
    }
    // the groups of joined holes: pieces of strip, links, the off-board wires of one net
    let pieces = l.pieces();
    let piece_of = |at: At| pieces.iter().position(|(r, a, b)| *r == at.0 && at.1 >= *a && at.1 <= *b);
    let mut parent: Vec<usize> = (0..pieces.len()).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut i = i;
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for (ja, jb) in l.joins() {
        if let (Some(a), Some(b)) = (piece_of(ja), piece_of(jb)) {
            let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
            parent[ra] = rb;
        }
    }
    let nets = extract(&s.flat());
    let net_of = |r: &str, pin: &str| nets.iter().find(|n| n.pins.iter().any(|p| p.refdes == r && p.pin == pin)).map(|n| n.name.clone()).unwrap_or_default();
    let mut group_nets: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
    let mut net_groups: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    let leads = l.parts.iter().flat_map(|p| p.leads.iter().map(move |(pin, at)| (p.refdes.clone(), pin.clone(), *at))).chain(l.off.iter().flat_map(|(r, v)| v.iter().map(move |(pin, _, at)| (r.clone(), pin.clone(), *at))));
    for (r, pin, at) in leads {
        let net = net_of(&r, &pin);
        if net.is_empty() {
            continue;
        }
        let Some(p) = piece_of(at) else {
            problems.push(format!("{r}.{pin} の穴 {} が帯の上にありません", hole_name(at)));
            continue;
        };
        let g = find(&mut parent, p);
        group_nets.entry(g).or_default().insert(net.clone());
        net_groups.entry(net).or_default().insert(g);
    }
    for nets in group_nets.values().filter(|n| n.len() > 1) {
        problems.push(format!("ショート: {} がつながっています", nets.iter().cloned().collect::<Vec<_>>().join(" と ")));
    }
    for (net, groups) in net_groups.iter().filter(|(_, g)| g.len() > 1) {
        problems.push(format!("{net} が {} か所に分かれています (つなぎ忘れ)", groups.len()));
    }
    problems
}

/// How a rule of the requirements table fares on the board: its line, and whether it holds.
#[derive(Clone, Debug, PartialEq)]
pub struct RuleCheck {
    pub ok: bool,
    pub message: String,
}

/// The requirements table's rules that a board built by hand can keep: the distances to keep (`KeepApart`: the closest two holes of
/// the two sets, 2.54 mm a hole) and, with sections, the one ground point (every section's ground goes back to the first section's
/// ground strip by a wire of its own).
pub fn strip_rules(s: &Schematic, l: &StripLayout) -> Vec<RuleCheck> {
    let mut out = vec![];
    let nets = extract(&s.flat());
    let net_of = |r: &str, pin: &str| nets.iter().find(|n| n.pins.iter().any(|p| p.refdes == r && p.pin == pin)).map(|n| n.name.clone()).unwrap_or_default();
    // the holes of an item: a part's leads, or everything on a net (leads, the ends of its wires)
    let holes_of = |items: &[String]| -> Vec<At> {
        let mut v = vec![];
        for item in items {
            if let Some(p) = l.parts.iter().find(|p| &p.refdes == item) {
                v.extend(p.leads.iter().map(|(_, a)| *a));
                continue;
            }
            for p in &l.parts {
                v.extend(p.leads.iter().filter(|(pin, _)| design_spec::matches_name(item, &net_of(&p.refdes, pin))).map(|(_, a)| *a));
            }
            for k in l.links.iter().filter(|k| design_spec::matches_name(item, &k.net)) {
                v.push(k.a);
                v.push(k.b);
            }
        }
        v
    };
    for rule in &s.requirements.apart {
        let (a, b) = (holes_of(&rule.a), holes_of(&rule.b));
        let closest = a.iter().flat_map(|x| b.iter().map(move |y| (x.0 as f64 - y.0 as f64).hypot(x.1 as f64 - y.1 as f64) * 2.54)).filter(|d| *d > 0.0).fold(f64::INFINITY, f64::min);
        if !closest.is_finite() {
            continue;
        }
        let ok = closest >= rule.min_dist;
        let name = if rule.name.is_empty() { format!("{} と {}", rule.a.join(","), rule.b.join(",")) } else { rule.name.clone() };
        out.push(RuleCheck {
            ok,
            message: if ok {
                format!("離す「{name}」: いちばん近い所で {closest:.1} mm ({:.1} mm 以上)", rule.min_dist)
            } else {
                format!("離す「{name}」: いちばん近い所が {closest:.1} mm で、{:.1} mm に足りません。部品か線を、四角で囲んで離してください", rule.min_dist)
            },
        });
    }
    for (m, _, v) in forks(s, l) {
        let name = if m.name.is_empty() { m.pads.join(",") } else { m.name.clone() };
        let (start, a, b) = (&m.pads[0], &m.pads[1], &m.pads[2]);
        let Some(v) = v else {
            out.push(RuleCheck { ok: false, message: format!("ケルビン「{name}」: {start} から {a} か {b} へ、帯と線でつながっていません") });
            continue;
        };
        let ok = if m.op == "min" { v >= m.limit } else { v <= m.limit + 1e-9 };
        out.push(RuleCheck {
            ok,
            message: if ok {
                format!("ケルビン「{name}」: {a} と {b} への道は {start} の足から {v:.1} mm で分かれます ({:.1} mm 以下)", m.limit)
            } else {
                format!("ケルビン「{name}」: {a} と {b} への道が {start} の足から {v:.1} mm 一緒に走り、{:.1} mm を超えます。{start} の足を、帯の上で 2 本の線の間に置いてください (片方の電流が流れる帯から、もう片方の線を取らない)", m.limit)
            },
        });
    }
    if l.sections.len() > 1 {
        let inside = |k: usize, a: At| {
            let (_, p, q) = &l.sections[k];
            a.0 >= p.0 && a.0 <= q.0 && a.1 >= p.1 && a.1 <= q.1
        };
        // a wire's end belongs to a section when it is in the section's box, or (a stripboard) on a piece of strip that runs into
        // it: a strip often goes on a hole or two past the section's last part, and the wire back to the ground point starts there
        let pieces = l.pieces();
        let in_sec = |k: usize, a: At| {
            inside(k, a)
                || (!l.perf && {
                    let (_, p, q) = &l.sections[k];
                    pieces.iter().any(|(r, c0, c1)| *r == a.0 && a.1 >= *c0 && a.1 <= *c1 && *r >= p.0 && *r <= q.0 && *c0 <= q.1 && *c1 >= p.1)
                })
        };
        // a section's ground: its ground strip (named where it starts), or on a perfboard (no strips) a lead on ground
        let ground_leads: Vec<At> = l.parts.iter().flat_map(|p| p.leads.iter().filter(|(pin, _)| is_ground_name(&net_of(&p.refdes, pin))).map(|(_, a)| *a)).chain(l.off.iter().flat_map(|(_, v)| v.iter().filter(|w| is_ground_name(&w.1)).map(|w| w.2))).collect();
        let ground_in = |k: usize| l.names.iter().any(|(a, n)| is_ground_name(n) && inside(k, *a)) || (l.perf && ground_leads.iter().any(|a| inside(k, *a)));
        // the section chosen in the table, else the first with a ground
        let chosen = ground_point(s).and_then(|g| l.sections.iter().position(|x| x.0 == g)).filter(|k| ground_in(*k));
        let hub = chosen.or_else(|| (0..l.sections.len()).find(|k| ground_in(*k)));
        if let Some(h) = hub {
            for k in (0..l.sections.len()).filter(|k| *k != h) {
                let has_ground = ground_in(k);
                if !has_ground {
                    continue;
                }
                let back = l.links.iter().filter(|w| is_ground_name(&w.net) && ((in_sec(h, w.a) && in_sec(k, w.b)) || (in_sec(h, w.b) && in_sec(k, w.a)))).count();
                let other = l.links.iter().filter(|w| is_ground_name(&w.net) && (in_sec(k, w.a) != in_sec(k, w.b)) && !(in_sec(h, w.a) || in_sec(h, w.b))).count();
                let ok = back == 1 && other == 0;
                out.push(RuleCheck {
                    ok,
                    message: if ok {
                        format!("GND: 「{}」の GND は、1 本の線で「{}」の GND の帯 (1 点) へ戻ります", l.sections[k].0, l.sections[h].0)
                    } else {
                        format!("GND: 「{}」の GND から「{}」の GND への線が {back} 本、ほかの区画への線が {other} 本あります。1 点へ 1 本で戻すと、ほかの区画の電流が GND を通りません", l.sections[k].0, l.sections[h].0)
                    },
                });
            }
        }
    }
    out
}
