//! The placement made better by annealing (`anneal`): parts moved and swapped on an estimate of the wiring.

use super::*;

/// How many moves the annealing tries.
pub(super) const ANNEAL_MOVES: usize = 30_000;

/// The placement made better by annealing (simulated annealing on an estimate, not on the wires themselves: a wiring takes too long
/// to try each move): a two-lead part moved to a free place near it (lying or standing, either way round), two two-lead parts
/// swapped, or two chips of one size swapped; a move that makes the nets' spread (each net's half the perimeter round its leads, the
/// supplies and ground counting less) smaller is kept, a worse one now and then while it is still warm. Every part stays within its
/// section's rows (or the board as placed), each lead keeps two free holes beside it, and the table's distances are kept.
pub(super) fn anneal(c: &Circuit, wants: &Wants, parts: &mut [StripPart], off: &[(String, Vec<crate::stripboard::OffWire>)], width: u16, margin: u16, bands: &[(u16, u16)]) {
    if parts.is_empty() {
        return;
    }
    let net_of = |p: &StripPart, pin: &str| lead_net(c, &p.refdes, pin);
    // each net's leads (part, lead), the fixed holes of the off-board wires
    let mut net_ids: Vec<String> = vec![];
    let mut leads_of: Vec<Vec<(usize, usize)>> = vec![];
    let mut fixed_of: Vec<Vec<At>> = vec![];
    let id = |n: &str, net_ids: &mut Vec<String>, leads_of: &mut Vec<Vec<(usize, usize)>>, fixed_of: &mut Vec<Vec<At>>| -> usize {
        match net_ids.iter().position(|x| x == n) {
            Some(i) => i,
            None => {
                net_ids.push(n.to_string());
                leads_of.push(vec![]);
                fixed_of.push(vec![]);
                net_ids.len() - 1
            }
        }
    };
    let mut part_nets: Vec<Vec<usize>> = vec![vec![]; parts.len()];
    for (pi, p) in parts.iter().enumerate() {
        for (li, (pin, _)) in p.leads.iter().enumerate() {
            let n = net_of(p, pin);
            if n.is_empty() {
                continue;
            }
            let i = id(&n, &mut net_ids, &mut leads_of, &mut fixed_of);
            leads_of[i].push((pi, li));
            if !part_nets[pi].contains(&i) {
                part_nets[pi].push(i);
            }
        }
    }
    for (_, v) in off {
        for (_, n, a) in v {
            let i = id(n, &mut net_ids, &mut leads_of, &mut fixed_of);
            fixed_of[i].push(*a);
        }
    }
    let weight: Vec<f64> = net_ids.iter().map(|n| if is_ground_name(n) || c.supplies.contains(n) { 0.2 } else { 1.0 }).collect();
    // the groups with a farthest distance (`max=`): each a cost after the nets' (index `net_ids.len() + k`), how far its two farthest
    // parts' centres are past the distance, weighed heavily (L10)
    let nn = net_ids.len();
    let limited: Vec<(Vec<usize>, f64)> = wants
        .group_max
        .iter()
        .enumerate()
        .filter(|(_, m)| **m > 0.0)
        .map(|(g, m)| ((0..parts.len()).filter(|pi| wants.groups.get(&parts[*pi].refdes).is_some_and(|v| v.contains(&g))).collect::<Vec<usize>>(), *m))
        .filter(|(v, _)| v.len() >= 2)
        .collect();
    for (k, (members, _)) in limited.iter().enumerate() {
        for pi in members {
            part_nets[*pi].push(nn + k);
        }
    }
    // the bypass capacitors (L17): each a cost after the groups', how far its farthest lead is past the distance from the IC's pin of
    // its net, weighed as heavily
    let caps: Vec<(usize, usize, f64)> = wants
        .bypass
        .iter()
        .filter_map(|(cap, ic, limit)| Some((parts.iter().position(|p| &p.refdes == cap)?, parts.iter().position(|p| &p.refdes == ic)?, *limit)))
        .collect();
    let ng = nn + limited.len();
    for (k, (cp, ip, _)) in caps.iter().enumerate() {
        part_nets[*cp].push(ng + k);
        part_nets[*ip].push(ng + k);
    }
    let centre = |p: &StripPart| -> (f64, f64) {
        let n = p.leads.len().max(1) as f64;
        (p.leads.iter().map(|x| x.1 .0 as f64).sum::<f64>() / n, p.leads.iter().map(|x| x.1 .1 as f64).sum::<f64>() / n)
    };
    let group_cost = |k: usize, parts: &[StripPart]| -> f64 {
        let (members, limit) = &limited[k];
        let cs: Vec<(f64, f64)> = members.iter().map(|pi| centre(&parts[*pi])).collect();
        let far = cs.iter().flat_map(|a| cs.iter().map(move |b| (a.0 - b.0).hypot(a.1 - b.1))).fold(0.0, f64::max);
        (far - limit).max(0.0) * 4.0
    };
    let spread = |i: usize, parts: &[StripPart]| -> f64 {
        if i >= ng {
            let (cp, ip, limit) = caps[i - ng];
            return (bypass_gap(c, &parts[cp].refdes, &parts[cp].leads, &parts[ip]) - limit).max(0.0) * 4.0;
        }
        if i >= nn {
            return group_cost(i - nn, parts);
        }
        let pts = leads_of[i].iter().map(|(p, l)| parts[*p].leads[*l].1).chain(fixed_of[i].iter().copied());
        let (mut r0, mut r1, mut c0, mut c1, mut n) = (u16::MAX, 0u16, u16::MAX, 0u16, 0);
        for a in pts {
            r0 = r0.min(a.0);
            r1 = r1.max(a.0);
            c0 = c0.min(a.1);
            c1 = c1.max(a.1);
            n += 1;
        }
        if n < 2 {
            0.0
        } else {
            weight[i] * ((r1 - r0) + (c1 - c0)) as f64
        }
    };
    let nets_cost = |nets: &[usize], parts: &[StripPart]| -> f64 { nets.iter().map(|i| spread(*i, parts)).sum() };
    // the holes each part takes (a chip its whole outline, a two-lead part its leads and the hole between), and who takes each
    let holes_of = |p: &StripPart| -> Vec<At> {
        let (r0, r1) = (p.leads.iter().map(|x| x.1 .0).min().unwrap_or(0), p.leads.iter().map(|x| x.1 .0).max().unwrap_or(0));
        let (c0, c1) = (p.leads.iter().map(|x| x.1 .1).min().unwrap_or(0), p.leads.iter().map(|x| x.1 .1).max().unwrap_or(0));
        (r0..=r1).flat_map(|r| (c0..=c1).map(move |cc| (r, cc))).collect()
    };
    let mut taken: BTreeMap<At, usize> = BTreeMap::new();
    for (pi, p) in parts.iter().enumerate() {
        for h in holes_of(p) {
            taken.insert(h, pi);
        }
    }
    for (_, v) in off {
        for (_, _, a) in v {
            taken.insert(*a, usize::MAX);
        }
    }
    for a in &wants.keepout {
        taken.insert(*a, usize::MAX);
    }
    let band_of = |a: At| bands.iter().copied().find(|(t, b)| a.0 >= *t && a.0 <= *b).unwrap_or((1, u16::MAX));
    let twos: Vec<usize> = (0..parts.len()).filter(|i| parts[*i].look == Look::TwoLead && parts[*i].leads.len() == 2 && !wants.kept(&parts[*i].refdes)).collect();
    let chips: Vec<usize> = (0..parts.len()).filter(|i| parts[*i].look != Look::TwoLead && !wants.kept(&parts[*i].refdes)).collect();
    let size = |p: &StripPart| {
        let (r0, r1) = (p.leads.iter().map(|x| x.1 .0).min().unwrap_or(0), p.leads.iter().map(|x| x.1 .0).max().unwrap_or(0));
        let (c0, c1) = (p.leads.iter().map(|x| x.1 .1).min().unwrap_or(0), p.leads.iter().map(|x| x.1 .1).max().unwrap_or(0));
        (r1 - r0, c1 - c0, p.leads.len(), r0, c0)
    };
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    // whether the leads of part `pi` (its holes taken out of `taken` first) each keep two free holes beside them
    let room = |pi: usize, parts: &[StripPart], taken: &BTreeMap<At, usize>| -> bool {
        let own: Vec<At> = holes_of(&parts[pi]);
        parts[pi].leads.iter().all(|(_, a)| {
            [(0i32, 1i32), (1, 0), (0, -1), (-1, 0)]
                .iter()
                .filter(|(dr, dc)| {
                    let (y, x) = (a.0 as i32 + dr, a.1 as i32 + dc);
                    y >= 0 && x >= 0 && x < width as i32 && !own.contains(&(y as u16, x as u16)) && !taken.contains_key(&(y as u16, x as u16))
                })
                .count()
                >= 2
        })
    };
    let (t0, t1) = (3.0f64, 0.05f64);
    for step in 0..ANNEAL_MOVES {
        let temp = t0 * (t1 / t0).powf(step as f64 / ANNEAL_MOVES as f64);
        let accept = |delta: f64, r: u64| delta <= 0.0 || ((r % 1_000_000) as f64 / 1_000_000.0) < (-delta / temp).exp();
        let kind = rnd() % 10;
        if kind < 7 && !twos.is_empty() {
            // a two-lead part to a free place near it
            let pi = twos[(rnd() % twos.len() as u64) as usize];
            let old = parts[pi].leads.clone();
            let (band_top, band_bottom) = band_of(old[0].1);
            let (ar, ac) = old[0].1;
            let (dr, dc) = ((rnd() % 13) as i32 - 6, (rnd() % 13) as i32 - 6);
            let (r, cc) = (ar as i32 + dr, ac as i32 + dc);
            let standing = rnd() % 2 == 0;
            let flip = rnd() % 2 == 0;
            let (end_r, end_c) = if standing { (r + SPAN as i32, cc) } else { (r, cc + SPAN as i32) };
            // (the last column is left free, as the chips leave it: else the board is a hole wider than asked)
            if r < band_top as i32 || end_r > band_bottom as i32 || cc < margin as i32 || end_c + 1 >= width as i32 {
                continue;
            }
            let (a, b) = ((r as u16, cc as u16), (end_r as u16, end_c as u16));
            let (p, q) = if flip { (b, a) } else { (a, b) };
            let new_holes: Vec<At> = if standing { (a.0..=b.0).map(|y| (y, a.1)).collect() } else { (a.1..=b.1).map(|x| (a.0, x)).collect() };
            if new_holes.iter().any(|h| taken.get(h).is_some_and(|o| *o != pi)) {
                continue;
            }
            let before = nets_cost(&part_nets[pi], parts);
            for h in holes_of(&parts[pi]) {
                taken.remove(&h);
            }
            parts[pi].leads[0].1 = p;
            parts[pi].leads[1].1 = q;
            for h in &new_holes {
                taken.insert(*h, pi);
            }
            let ok = room(pi, parts, &taken) && wants.far_enough(&parts[pi].refdes, &[p, q], parts);
            let delta = nets_cost(&part_nets[pi], parts) - before;
            if !(ok && accept(delta, rnd())) {
                for h in &new_holes {
                    taken.remove(h);
                }
                parts[pi].leads = old;
                for h in holes_of(&parts[pi]) {
                    taken.insert(h, pi);
                }
            }
        } else if kind < 9 && twos.len() > 1 {
            // two two-lead parts swapped (each takes the other's holes)
            let (i, j) = (twos[(rnd() % twos.len() as u64) as usize], twos[(rnd() % twos.len() as u64) as usize]);
            if i == j || band_of(parts[i].leads[0].1) != band_of(parts[j].leads[0].1) {
                continue;
            }
            let mut nets = part_nets[i].clone();
            nets.extend(part_nets[j].iter().copied());
            let before = nets_cost(&nets, parts);
            let (li, lj) = (parts[i].leads.clone(), parts[j].leads.clone());
            for k in 0..2 {
                parts[i].leads[k].1 = lj[k].1;
                parts[j].leads[k].1 = li[k].1;
            }
            let ok = wants.far_enough(&parts[i].refdes, &[lj[0].1, lj[1].1], parts) && wants.far_enough(&parts[j].refdes, &[li[0].1, li[1].1], parts);
            let delta = nets_cost(&nets, parts) - before;
            if ok && accept(delta, rnd()) {
                for h in holes_of(&parts[i]) {
                    taken.insert(h, i);
                }
                for h in holes_of(&parts[j]) {
                    taken.insert(h, j);
                }
            } else {
                parts[i].leads = li;
                parts[j].leads = lj;
            }
        } else if chips.len() > 1 {
            // two chips of one size and shape swapped
            let (i, j) = (chips[(rnd() % chips.len() as u64) as usize], chips[(rnd() % chips.len() as u64) as usize]);
            let (si, sj) = (size(&parts[i]), size(&parts[j]));
            if i == j || (si.0, si.1, si.2) != (sj.0, sj.1, sj.2) || parts[i].look != parts[j].look || band_of((si.3, si.4)) != band_of((sj.3, sj.4)) {
                continue;
            }
            let mut nets = part_nets[i].clone();
            nets.extend(part_nets[j].iter().copied());
            let before = nets_cost(&nets, parts);
            let (li, lj) = (parts[i].leads.clone(), parts[j].leads.clone());
            let shift = |l: &[(String, At)], from: (u16, u16), to: (u16, u16)| -> Vec<(String, At)> { l.iter().map(|(p, a)| (p.clone(), (a.0 - from.0 + to.0, a.1 - from.1 + to.1))).collect() };
            parts[i].leads = shift(&li, (si.3, si.4), (sj.3, sj.4));
            parts[j].leads = shift(&lj, (sj.3, sj.4), (si.3, si.4));
            let ok = wants.far_enough(&parts[i].refdes, &parts[i].leads.iter().map(|x| x.1).collect::<Vec<_>>(), parts) && wants.far_enough(&parts[j].refdes, &parts[j].leads.iter().map(|x| x.1).collect::<Vec<_>>(), parts);
            let delta = nets_cost(&nets, parts) - before;
            if ok && accept(delta, rnd()) {
                for h in holes_of(&parts[i]) {
                    taken.insert(h, i);
                }
                for h in holes_of(&parts[j]) {
                    taken.insert(h, j);
                }
            } else {
                parts[i].leads = li;
                parts[j].leads = lj;
            }
        }
    }
}
