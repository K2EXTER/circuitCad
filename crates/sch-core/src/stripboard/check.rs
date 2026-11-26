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

/// How a rule of the requirements table fares on the board: its line, whether it holds, and the parts it is about (the two farthest
/// apart of a group, the two nearest of a distance to keep: picked when the line is clicked).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct RuleCheck {
    pub ok: bool,
    pub message: String,
    pub parts: Vec<String>,
}

/// The groups of the requirements table with a farthest distance (`max_dist` > 0) and how far apart their two farthest parts on the
/// board are (the centres of their leads, mm): (the group's name, the distance, the most it may be, the two parts that far apart). A
/// group with fewer than two of its parts on the board is left out.
pub fn group_spreads(s: &Schematic, l: &StripLayout) -> Vec<(String, f64, f64, (String, String))> {
    let mut out = vec![];
    for g in s.requirements.groups.iter().filter(|g| g.max_dist > 0.0) {
        let centres: Vec<(&str, (f64, f64))> = l
            .parts
            .iter()
            .filter(|p| g.parts.iter().any(|it| design_spec::matches_name(it, &p.refdes)) && !p.leads.is_empty())
            .map(|p| {
                let n = p.leads.len() as f64;
                (p.refdes.as_str(), (p.leads.iter().map(|x| x.1 .0 as f64).sum::<f64>() / n, p.leads.iter().map(|x| x.1 .1 as f64).sum::<f64>() / n))
            })
            .collect();
        if centres.len() < 2 {
            continue;
        }
        let mut far = (0.0, (String::new(), String::new()));
        for (i, (ra, a)) in centres.iter().enumerate() {
            for (rb, b) in centres.iter().skip(i + 1) {
                let d = (a.0 - b.0).hypot(a.1 - b.1) * 2.54;
                if d > far.0 || far.1 .0.is_empty() {
                    far = (d, (ra.to_string(), rb.to_string()));
                }
            }
        }
        let name = if g.name.is_empty() { g.parts.join(",") } else { g.name.clone() };
        out.push((name, far.0, g.max_dist, far.1));
    }
    out
}

/// The requirements table's rules that a board built by hand can keep: the groups' farthest distances (`group_spreads`), the distances to keep (`KeepApart`: the closest two holes of
/// the two sets, 2.54 mm a hole), the bypass capacitors' distances to their IC's pins (`bypass_spans`) and, with sections, the one ground point (every section's ground goes back to the first section's
/// ground strip by a wire of its own).
pub fn strip_rules(s: &Schematic, l: &StripLayout) -> Vec<RuleCheck> {
    let mut out = vec![];
    let nets = extract(&s.flat());
    let net_of = |r: &str, pin: &str| nets.iter().find(|n| n.pins.iter().any(|p| p.refdes == r && p.pin == pin)).map(|n| n.name.clone()).unwrap_or_default();
    // the holes of an item and what is on each (a part's leads; everything on a net: its leads, the ends of its wires on top)
    let holes_of = |items: &[String]| -> Vec<(At, String)> {
        let mut v = vec![];
        for item in items {
            if let Some(p) = l.parts.iter().find(|p| &p.refdes == item) {
                v.extend(p.leads.iter().map(|(_, a)| (*a, p.refdes.clone())));
                continue;
            }
            for p in &l.parts {
                v.extend(p.leads.iter().filter(|(pin, _)| design_spec::matches_name(item, &net_of(&p.refdes, pin))).map(|(_, a)| (*a, p.refdes.clone())));
            }
            for k in l.links.iter().filter(|k| design_spec::matches_name(item, &k.net)) {
                v.push((k.a, format!("{} の表の線", k.net)));
                v.push((k.b, format!("{} の表の線", k.net)));
            }
        }
        v
    };
    // (the numbers as typed: 40 mm, 2.54 mm)
    let mm = |x: f64| format!("{}", (x * 10.0).round() / 10.0);
    // a group to keep together: what it must be within, then the two parts farthest apart and how much too far
    for (name, far, max, (a, b)) in group_spreads(s, l) {
        let ok = far <= max + 1e-9;
        out.push(RuleCheck {
            ok,
            message: if ok {
                format!("「{name}」を {} mm 以内に集める: できています (いちばん遠い {a} と {b} で {} mm)", mm(max), mm(far))
            } else {
                format!("「{name}」を {} mm 以内に集める: {a} と {b} が {} mm 離れています ({} mm 遠すぎ)。どちらかを相手の方へ寄せてください", mm(max), mm(far), mm(far - max))
            },
            parts: vec![a, b],
        });
    }
    // a distance to keep: what it must be at least, then the two nearest and how much too near
    for rule in &s.requirements.apart {
        let (xs, ys) = (holes_of(&rule.a), holes_of(&rule.b));
        let mut near: Option<(f64, String, String)> = None;
        for (x, wx) in &xs {
            for (y, wy) in &ys {
                let d = (x.0 as f64 - y.0 as f64).hypot(x.1 as f64 - y.1 as f64) * 2.54;
                if d > 0.0 && near.as_ref().is_none_or(|n| d < n.0) {
                    near = Some((d, wx.clone(), wy.clone()));
                }
            }
        }
        let Some((closest, a, b)) = near else { continue };
        let ok = closest >= rule.min_dist;
        let name = if rule.name.is_empty() { format!("{} と {}", rule.a.join(","), rule.b.join(",")) } else { rule.name.clone() };
        let parts: Vec<String> = [a.clone(), b.clone()].into_iter().filter(|w| l.parts.iter().any(|p| p.refdes == *w)).collect();
        out.push(RuleCheck {
            ok,
            message: if ok {
                format!("「{name}」を {} mm 以上離す: できています (いちばん近い {a} と {b} で {} mm)", mm(rule.min_dist), mm(closest))
            } else {
                format!("「{name}」を {} mm 以上離す: {a} と {b} が {} mm しか離れていません ({} mm 足りない)。どちらかを遠ざけてください", mm(rule.min_dist), mm(closest), mm(rule.min_dist - closest))
            },
            parts,
        });
    }
    // a bypass capacitor: its farthest lead from the IC's nearest pin of the lead's net
    for sp in bypass_spans(s, l) {
        let (d, ic, cap) = (sp.judged(), &sp.ic, &sp.cap);
        let along = sp.along.map(|a| format!(" (帯と線で {} mm)", mm(a))).unwrap_or_default();
        let waived = s.requirements.waiver_for("bypass", ic, cap);
        let ok = d <= sp.limit + 1e-9 || waived.is_some();
        out.push(RuleCheck {
            ok,
            message: if d <= sp.limit + 1e-9 {
                format!("バイパス {cap} ({ic} 用) を {} mm 以内に: できています (いちばん遠い {} の足も {} から {} mm)", mm(sp.limit), sp.net, sp.pin, mm(d))
            } else if let Some(w) = waived {
                format!("バイパス {cap} ({ic} 用): {} の足が {} から {} mm{along}。承知した例外: {}", sp.net, sp.pin, mm(sp.straight), w.note)
            } else {
                format!(
                    "バイパス {cap} ({ic} 用) を {} mm 以内に: {} の足が {} から {} mm{along}。遠いと線のインダクタンスで速い電流に追いつけません。{cap} を {} のそばへ寄せてください",
                    mm(sp.limit),
                    sp.net,
                    sp.pin,
                    mm(sp.straight),
                    sp.pin
                )
            },
            parts: vec![cap.clone(), ic.clone()],
        });
    }
    for (m, _, v) in forks(s, l) {
        let name = if m.name.is_empty() { m.pads.join(",") } else { m.name.clone() };
        let (start, a, b) = (&m.pads[0], &m.pads[1], &m.pads[2]);
        let Some(v) = v else {
            out.push(RuleCheck { ok: false, message: format!("ケルビン「{name}」: {start} から {a} か {b} へ、帯と線でつながっていません"), ..Default::default() });
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
            parts: m.pads.iter().filter_map(|p| p.split_once('.').map(|x| x.0.to_string())).take(1).collect(),
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
                    ..Default::default()
                });
            }
        }
    }
    out
}

/// How far a bypass capacitor of the requirements table is from the IC it serves, on a board built by hand: of the capacitor's leads,
/// the one farthest from the IC's nearest pin of the same net (the supply side and the ground side are both looked at).
#[derive(Clone, Debug, PartialEq)]
pub struct BypassSpan {
    pub cap: String,
    pub ic: String,
    /// The lead's net and the IC's pin it is measured to ("U1.26").
    pub net: String,
    pub pin: String,
    /// Straight from hole to hole (mm), and along the strips and wires when they join them.
    pub straight: f64,
    pub along: Option<f64>,
    /// The most it may be (mm): the row's, else `design_spec::DEFAULT_BYPASS_MM`.
    pub limit: f64,
}

impl BypassSpan {
    /// The distance judged: along the copper when joined (what the current runs), else straight.
    pub fn judged(&self) -> f64 {
        self.along.unwrap_or(self.straight)
    }
}

/// The bypass rows' capacitors on the board and how far each is from its IC (`BypassSpan`). A part on an adapter board or a module has
/// the adapter's holes as its leads, so those holes stand for the IC's pins. A capacitor or IC not on the board, or a capacitor with no
/// lead on a net of the IC, is left out.
pub fn bypass_spans(s: &Schematic, l: &StripLayout) -> Vec<BypassSpan> {
    let nets = extract(&s.flat());
    let net_of = |r: &str, pin: &str| nets.iter().find(|n| n.pins.iter().any(|p| p.refdes == r && p.pin == pin)).map(|n| n.name.clone()).unwrap_or_default();
    let mut out = vec![];
    for b in &s.requirements.bypass {
        let Some(ic) = l.parts.iter().find(|p| p.refdes == b.ic) else { continue };
        for cap in &b.caps {
            let Some(c) = l.parts.iter().find(|p| &p.refdes == cap) else { continue };
            let mut worst: Option<BypassSpan> = None;
            for (pin, at) in &c.leads {
                let net = net_of(cap, pin);
                if net.is_empty() {
                    continue;
                }
                let targets: Vec<&(String, At)> = ic.leads.iter().filter(|(ip, _)| net_of(&ic.refdes, ip) == net).collect();
                if targets.is_empty() {
                    continue;
                }
                let (dist, _) = copper_ways(l, *at);
                let mut best: Option<BypassSpan> = None;
                for (ip, ia) in targets {
                    let straight = (at.0 as f64 - ia.0 as f64).hypot(at.1 as f64 - ia.1 as f64) * 2.54;
                    let along = dist.get(ia).map(|um| *um as f64 / 1000.0);
                    let sp = BypassSpan { cap: cap.clone(), ic: b.ic.clone(), net: net.clone(), pin: format!("{}.{ip}", b.ic), straight, along, limit: b.limit() };
                    if best.as_ref().is_none_or(|x| sp.judged() < x.judged()) {
                        best = Some(sp);
                    }
                }
                if let Some(sp) = best
                    && worst.as_ref().is_none_or(|w| sp.judged() > w.judged())
                {
                    worst = Some(sp);
                }
            }
            out.extend(worst);
        }
    }
    out
}
