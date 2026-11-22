//! Tidying a schematic (candidate N3): the parts laid out again so the sheet reads like a hand-drawn one: the signal flows from
//! left to right (each part in a column by how many signal nets it is from an input: a source, a connector), the parts of a column
//! ordered after the ones they join in the column before (fewer crossings), each two- or three-lead part turned so its supply pin is
//! on top, its ground pin at the bottom and its signal pins toward the parts they join. Made for the sheets `ccad` and the MCP
//! build (every part in a row, joined by net labels) and the ones read from KiCad and LTspice.
//!
//! The joints are drawn again as the `ccad` sheets draw them: a label with the net's name on each pin, a ground symbol on a ground
//! pin at the bottom of its part, a power symbol on a supply pin at its top. The nets never change: the pins' grouping into nets
//! before and after is compared, and a tidy that would change it is not made. Either the whole sheet or only some parts (the
//! others stay where they are; the chosen ones go below them).

use crate::{extract, on_segment, Component, Kind, Label, Pt, Schematic, Wire};
use std::collections::{BTreeMap, BTreeSet};

/// What a tidy did.
#[derive(Clone, Debug, PartialEq)]
pub struct Tidied {
    pub moved: usize,
    pub columns: usize,
    /// The nets drawn as a wire between their two pins (instead of a label on each).
    pub wired: usize,
}

/// The pins' grouping into nets (the parts' pins only; power and ground symbols are drawn again).
fn grouping(s: &Schematic) -> BTreeSet<Vec<(String, String)>> {
    let power: BTreeSet<&str> = s.components.iter().filter(|c| c.kind.is_power_symbol()).map(|c| c.refdes.as_str()).collect();
    extract(s)
        .into_iter()
        .map(|n| {
            let mut v: Vec<(String, String)> = n.pins.iter().filter(|p| !power.contains(p.refdes.as_str())).map(|p| (p.refdes.clone(), p.pin.clone())).collect();
            v.sort();
            v
        })
        .filter(|v| !v.is_empty())
        .collect()
}

/// A part's box at turn `rot` around its position: the pins and the body (grid units, relative to the position).
fn part_box(c: &Component, rot: u8) -> (i32, i32, i32, i32) {
    let mut t = c.clone();
    t.rot = rot;
    t.pos = (0, 0);
    let (hw, hh) = match &c.custom {
        Some(sym) if !sym.is_two_lead_glyph() => {
            let l = sym.layout();
            (l.hw, l.hh)
        }
        _ => (2, 1),
    };
    let body = [(-hw, -hh), (hw, hh)].map(|p| crate::rotate(p, rot));
    let pts = t.pins().into_iter().map(|p| p.1).chain(body);
    pts.fold((i32::MAX, i32::MAX, i32::MIN, i32::MIN), |b, (x, y)| (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y)))
}

/// Whether a part may be turned: the basic two- and three-lead parts and the library parts drawn as them (a chip keeps its symbol
/// the way it was drawn, inputs on the left).
fn turnable(c: &Component) -> bool {
    match &c.custom {
        Some(sym) => sym.is_two_lead_glyph() || sym.pins.len() <= 3,
        None => !matches!(c.kind, Kind::Header(_)),
    }
}

/// The sheet with its parts (all, or those in `only`) laid out again; Err when it cannot be tidied (a child sheet's ports and buses
/// keep their places, so a sheet with them is left alone) or the nets would change.
pub fn tidy(s: &Schematic, only: Option<&BTreeSet<String>>) -> Result<(Schematic, Tidied), String> {
    // the wires are checked like the rest; should one join what it must not, the sheet is drawn with labels only
    tidy_with(s, only, true).or_else(|_| tidy_with(s, only, false))
}

/// Does the open inside of box `r` (x0, y0, x1, y1) meet the level or upright segment `a`-`b`?
fn crosses_box(a: Pt, b: Pt, r: (i32, i32, i32, i32)) -> bool {
    let (x0, x1, y0, y1) = (a.0.min(b.0), a.0.max(b.0), a.1.min(b.1), a.1.max(b.1));
    let across = |lo: i32, hi: i32, r0: i32, r1: i32| if lo == hi { r0 < lo && lo < r1 } else { hi > r0 && lo < r1 };
    across(x0, x1, r.0, r.2) && across(y0, y1, r.1, r.3)
}

/// The ways a wire may go from pin `a` to pin `b`, the plainest first: straight, a step half way across, an L, a step half way down.
fn ways(a: Pt, b: Pt) -> Vec<Vec<Pt>> {
    let (a, b) = if a <= b { (a, b) } else { (b, a) };
    if a.0 == b.0 || a.1 == b.1 {
        return vec![vec![a, b]];
    }
    let mut v: Vec<Vec<Pt>> = vec![];
    // the step across, from the middle outward
    let mid = (a.0 + b.0) / 2;
    for d in 0..=(b.0 - a.0) / 2 {
        for x in [mid - d, mid + d] {
            if x > a.0 && x < b.0 && !v.iter().any(|w| w[1].0 == x) {
                v.push(vec![a, (x, a.1), (x, b.1), b]);
            }
        }
    }
    v.push(vec![a, (b.0, a.1), b]);
    v.push(vec![a, (a.0, b.1), b]);
    let (top, bottom) = (a.1.min(b.1), a.1.max(b.1));
    let mid = (top + bottom) / 2;
    for d in 0..=(bottom - top) / 2 {
        for y in [mid - d, mid + d] {
            if y > top && y < bottom && !v.iter().any(|w| w.len() == 4 && w[1].0 == a.0 && w[1].1 == y) {
                v.push(vec![a, (a.0, y), (b.0, y), b]);
            }
        }
    }
    v
}

/// The grid points along the level and upright wires `segs`: (on a level one, on an upright one).
fn along(segs: &[(Pt, Pt)]) -> std::collections::HashMap<Pt, (bool, bool)> {
    let mut m: std::collections::HashMap<Pt, (bool, bool)> = std::collections::HashMap::new();
    for (a, b) in segs {
        let level = a.1 == b.1;
        let (dx, dy) = ((b.0 - a.0).signum(), (b.1 - a.1).signum());
        let mut p = *a;
        loop {
            let e = m.entry(p).or_default();
            if level { e.0 = true } else { e.1 = true }
            if p == *b || (dx == 0 && dy == 0) {
                break;
            }
            p = (p.0 + dx, p.1 + dy);
        }
    }
    m
}

/// The shortest way on the grid from pin `from` to one of `targets` (a maze search, Dijkstra's, where a corner costs 3 steps and
/// crossing another wire 2): past the parts' bodies, through none of the `blocked` points (other nets' pins, labels, symbols and
/// wire ends), never along another wire and never turning on one (either would join it). None when no way is found near the pins.
fn maze(from: Pt, targets: &BTreeSet<Pt>, blocked: &BTreeSet<Pt>, wires: &std::collections::HashMap<Pt, (bool, bool)>, boxes: &[(i32, i32, i32, i32)]) -> Option<Vec<Pt>> {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let all = targets.iter().chain(std::iter::once(&from));
    let (x0, x1) = (all.clone().map(|p| p.0).min()? - 6, all.clone().map(|p| p.0).max()? + 6);
    let (y0, y1) = (all.clone().map(|p| p.1).min()? - 6, all.map(|p| p.1).max()? + 6);
    let (w, h) = ((x1 - x0 + 1) as usize, (y1 - y0 + 1) as usize);
    if w * h > 40_000 {
        return None;
    }
    let cell = |p: Pt| ((p.1 - y0) as usize) * w + (p.0 - x0) as usize;
    // the grid: a point not to pass, a wire along it (level, upright), a target; a step to the right (down) from a cell across a body
    let mut stop = vec![false; w * h];
    let mut level = vec![false; w * h];
    let mut upright = vec![false; w * h];
    let mut goal = vec![false; w * h];
    let mut right_cut = vec![false; w * h];
    let mut down_cut = vec![false; w * h];
    let inside = |p: &Pt| p.0 >= x0 && p.0 <= x1 && p.1 >= y0 && p.1 <= y1;
    for p in blocked.iter().filter(|p| inside(p)) {
        stop[cell(*p)] = true;
    }
    for (p, c) in wires.iter().filter(|(p, _)| inside(p)) {
        level[cell(*p)] = c.0;
        upright[cell(*p)] = c.1;
    }
    for p in targets.iter().filter(|p| inside(p)) {
        goal[cell(*p)] = true;
    }
    for r in boxes.iter().filter(|r| r.2 >= x0 && r.0 <= x1 && r.3 >= y0 && r.1 <= y1) {
        // a level step from x to x + 1 at a height strictly inside, an upright one likewise
        for y in (r.1 + 1).max(y0)..=(r.3 - 1).min(y1) {
            for x in r.0.max(x0)..=(r.2 - 1).min(x1) {
                right_cut[cell((x, y))] = true;
            }
        }
        for x in (r.0 + 1).max(x0)..=(r.2 - 1).min(x1) {
            for y in r.1.max(y0)..=(r.3 - 1).min(y1) {
                down_cut[cell((x, y))] = true;
            }
        }
    }
    const STEP: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
    let mut dist = vec![i32::MAX; w * h * 5];
    let mut prev = vec![u32::MAX; w * h * 5];
    let mut heap = BinaryHeap::new();
    let start = cell(from) * 5 + 4;
    dist[start] = 0;
    heap.push(Reverse((0, start as u32)));
    while let Some(Reverse((d, k))) = heap.pop() {
        let k = k as usize;
        if dist[k] < d {
            continue;
        }
        let (c, dir) = (k / 5, (k % 5) as u8);
        let p = ((c % w) as i32 + x0, (c / w) as i32 + y0);
        if p != from && goal[c] {
            let mut way = vec![p];
            let mut at = k;
            while prev[at] != u32::MAX {
                at = prev[at] as usize;
                let c = at / 5;
                way.push(((c % w) as i32 + x0, (c / w) as i32 + y0));
            }
            way.reverse();
            // the corners only
            let mut v: Vec<Pt> = vec![way[0]];
            for k in 1..way.len() - 1 {
                let (a, b, c) = (way[k - 1], way[k], way[k + 1]);
                if (b.0 - a.0, b.1 - a.1) != (c.0 - b.0, c.1 - b.1) {
                    v.push(b);
                }
            }
            v.push(p);
            return Some(v);
        }
        let on_wire = level[c] || upright[c];
        for (nd, (dx, dy)) in STEP.iter().enumerate() {
            let nd = nd as u8;
            if dir != 4 && nd == dir ^ 1 {
                continue;
            }
            let turn = dir != 4 && nd != dir;
            if turn && on_wire {
                continue;
            }
            let q = (p.0 + dx, p.1 + dy);
            if !inside(&q) {
                continue;
            }
            let qc = cell(q);
            if stop[qc] || (nd < 2 && level[qc]) || (nd >= 2 && upright[qc]) {
                continue;
            }
            let cut = match nd {
                0 => right_cut[c],
                1 => right_cut[qc],
                2 => down_cut[c],
                _ => down_cut[qc],
            };
            if cut {
                continue;
            }
            let cost = d + 1 + if turn { 3 } else { 0 } + if level[qc] || upright[qc] { 2 } else { 0 };
            let nk = qc * 5 + nd as usize;
            if cost < dist[nk] {
                dist[nk] = cost;
                prev[nk] = k as u32;
                heap.push(Reverse((cost, nk as u32)));
            }
        }
    }
    None
}

/// The most pins of a net drawn as a tree of wires (more: by its name on each pin).
const TREE_PINS: usize = 5;
/// The room between two columns (for the names written to the right of the pins), between two parts of a column, the width of a
/// shunt's place in the gap between columns, and how far above its part a feedback part goes.
const COLUMN_GAP: i32 = 9;
const ROW_GAP: i32 = 2;
const SHUNT_SLOT: i32 = 5;
const FEEDBACK_STEP: i32 = 4;

fn tidy_with(s: &Schematic, only: Option<&BTreeSet<String>>, wires: bool) -> Result<(Schematic, Tidied), String> {
    if !s.ports.is_empty() || !s.buses.is_empty() {
        return Err("ポートかバスのあるシートは、まだ整えられません (位置でつながっているので)".into());
    }
    let before = grouping(s);
    let nets = extract(s);
    // a source's net without a name is a signal (a symbol would give it the made-up name)
    // a name another program made up (LTspice's N001, KiCad's Net-(R1-Pad1)) is no supply's name either
    let made_up = |n: &str| n.starts_with("N$") || n.starts_with("Net-(") || n.starts_with("unconnected-") || (n.len() == 4 && n.starts_with('N') && n[1..].chars().all(|c| c.is_ascii_digit()));
    let mut supplies: BTreeSet<String> = crate::breadboard::read(s).supplies.into_iter().filter(|n| !made_up(n)).collect();
    let is_ground = |n: &str| crate::breadboard::is_ground_name(n);
    let net_of: BTreeMap<(String, String), String> = nets.iter().flat_map(|n| n.pins.iter().map(move |p| ((p.refdes.clone(), p.pin.clone()), n.name.clone()))).collect();
    // a named net of a DC source is a supply too (VEE on the minus side of a source whose plus is ground)
    for c in s.components.iter().filter(|c| c.kind == Kind::VSource && !c.value.contains('(') && !c.value.to_ascii_uppercase().starts_with("AC")) {
        for (p, _) in c.pins() {
            if let Some(n) = net_of.get(&(c.refdes.clone(), p))
                && !made_up(n)
                && !is_ground(n)
            {
                supplies.insert(n.clone());
            }
        }
    }
    let chosen = |c: &Component| !c.kind.is_power_symbol() && only.is_none_or(|o| o.contains(&c.refdes));
    let parts: Vec<usize> = (0..s.components.len()).filter(|i| chosen(&s.components[*i])).collect();
    if parts.is_empty() {
        return Err("整える部品がありません".into());
    }
    let pins_of = |c: &Component| -> Vec<(String, String)> { c.pins().into_iter().filter_map(|(p, _)| net_of.get(&(c.refdes.clone(), p.clone())).map(|n| (p, n.clone()))).collect() };
    let signal = |n: &str| !is_ground(n) && !supplies.contains(n);
    // the parts each signal net joins
    let mut on_net: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for &i in &parts {
        for (_, n) in pins_of(&s.components[i]) {
            if !is_ground(&n) {
                let v = on_net.entry(n).or_default();
                if !v.contains(&i) {
                    v.push(i);
                }
            }
        }
    }
    // the parts a part joins by its signal nets (with `supply`, a source also by the supply it gives: the parts it feeds come after it)
    let source = |i: usize| matches!(s.components[i].kind, Kind::VSource | Kind::ISource);
    let neighbours_by = |i: usize, supply: bool| -> Vec<usize> {
        let mut v: Vec<usize> = pins_of(&s.components[i]).iter().filter(|(_, n)| signal(n) || (supply && source(i) && !is_ground(n))).flat_map(|(_, n)| on_net.get(n).cloned().unwrap_or_default()).filter(|j| *j != i && (!source(*j) || !source(i))).collect();
        v.sort();
        v.dedup();
        v
    };
    let neighbours = |i: usize| neighbours_by(i, true);
    // the inputs: the sources and the connectors (a source of a supply too: it goes first, on top)
    let input = |c: &Component| matches!(c.kind, Kind::VSource | Kind::ISource | Kind::Header(_)) || c.refdes.starts_with('J') || c.refdes.starts_with('P') && c.kind == Kind::Custom && c.custom.as_ref().is_some_and(|x| x.prefix == "P");
    // the parts hung off the line of the signal: a two-lead part from a signal net to the ground or a supply is a shunt, drawn
    // below the net (above it, to a supply) between the columns; a two-lead part between two signal nets that one part of three
    // pins or more is also on (an op amp, a transistor) is that part's feedback, drawn above it. Only when the whole sheet is tidied
    let two_lead = |i: usize| turnable(&s.components[i]) && !source(i) && !input(&s.components[i]) && pins_of(&s.components[i]).len() == 2;
    let mut feedback: BTreeMap<usize, usize> = BTreeMap::new();
    let mut shunt: BTreeMap<usize, String> = BTreeMap::new();
    if only.is_none() {
        for &i in &parts {
            if !two_lead(i) {
                continue;
            }
            let ps = pins_of(&s.components[i]);
            let (a, b) = (&ps[0].1, &ps[1].1);
            if a == b {
                continue;
            }
            if signal(a) && signal(b) {
                let on_both = |u: usize| {
                    let n = pins_of(&s.components[u]);
                    n.len() >= 3 && n.iter().any(|p| &p.1 == a) && n.iter().any(|p| &p.1 == b)
                };
                if let Some(&u) = parts.iter().find(|u| **u != i && !two_lead(**u) && on_both(**u)) {
                    feedback.insert(i, u);
                }
            } else if signal(a) != signal(b) {
                shunt.insert(i, if signal(a) { a.clone() } else { b.clone() });
            }
        }
        // a shunt hangs off a part of the line on its net (with none, it is the line)
        let hung: Vec<usize> = shunt.keys().copied().collect();
        for i in hung {
            let n = shunt[&i].clone();
            if !on_net.get(&n).is_some_and(|v| v.iter().any(|j| *j != i && !shunt.contains_key(j) && !feedback.contains_key(j))) {
                shunt.remove(&i);
            }
        }
    }
    // a source of supplies only (VCC, VEE) is no part of the signal's flow: it goes to the bottom left, by itself
    let rail: BTreeSet<usize> = if only.is_none() { parts.iter().copied().filter(|i| source(*i) && pins_of(&s.components[*i]).iter().all(|(_, n)| !signal(n))).collect() } else { BTreeSet::new() };
    let main: Vec<usize> = parts.iter().copied().filter(|i| !shunt.contains_key(i) && !feedback.contains_key(i) && !rail.contains(i)).collect();
    let is_main = |i: usize| !shunt.contains_key(&i) && !feedback.contains_key(&i) && !rail.contains(&i);
    // columns by distance from an input over the signal nets. The supplies come after: a part a signal reaches takes its place
    // in the signal's flow (an op amp after the resistors before it, not next to its supply), one fed only by a supply goes
    // after that supply's source, and a group of parts nothing reaches starts from its first part
    let mut col: BTreeMap<usize, usize> = BTreeMap::new();
    let mut queue: std::collections::VecDeque<usize> = main.iter().copied().filter(|i| input(&s.components[*i])).collect();
    for &i in &queue {
        col.insert(i, 0);
    }
    loop {
        while let Some(i) = queue.pop_front() {
            for j in neighbours_by(i, false) {
                if !col.contains_key(&j) && is_main(j) {
                    col.insert(j, col[&i] + 1);
                    queue.push_back(j);
                }
            }
        }
        let fed = main.iter().filter(|i| source(**i) && col.contains_key(i)).find_map(|&i| neighbours_by(i, true).into_iter().find(|j| !col.contains_key(j) && is_main(*j)).map(|j| (j, col[&i] + 1)));
        match fed.or_else(|| main.iter().find(|i| !col.contains_key(i)).map(|&i| (i, 0))) {
            Some((i, c)) => {
                col.insert(i, c);
                queue.push_back(i);
            }
            None => break,
        }
    }
    // shorter lines (what the network simplex of a layered drawing does, in its simplest form): a part, not an input, whose
    // neighbours are all two columns or more to its right moves to the column just before them; the empty columns go
    for _ in 0..3 {
        let mut moved = false;
        for &i in &main {
            if input(&s.components[i]) {
                continue;
            }
            let next: Vec<usize> = neighbours(i).into_iter().filter(|j| is_main(*j)).map(|j| col[&j]).collect();
            if let Some(&m) = next.iter().min()
                && m > col[&i] + 1
            {
                col.insert(i, m - 1);
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    let used: BTreeSet<usize> = main.iter().map(|i| col[i]).collect();
    let renumber: BTreeMap<usize, usize> = used.iter().enumerate().map(|(k, c)| (*c, k)).collect();
    for i in &main {
        col.insert(*i, renumber[&col[i]]);
    }
    let columns = col.values().max().map_or(0, |m| m + 1);
    for i in &rail {
        col.insert(*i, 0);
    }
    // a feedback part takes its part's column; a shunt the gap after the first column of the line parts on its net
    for (i, u) in &feedback {
        col.insert(*i, col[u]);
    }
    // a shunt counts as in the first column of the line parts on its net
    for (i, n) in &shunt {
        let k = on_net.get(n).map(|v| v.iter().filter(|j| is_main(**j)).map(|j| col[j]).min().unwrap_or(0)).unwrap_or(0);
        col.insert(*i, k);
    }
    // the order within the columns (the crossing reduction of a layered drawing): sweeps to the right and back, each column by the
    // median of its neighbours' rows on the side swept from, then neighbours swapped where that crosses fewer lines; the order with
    // the fewest crossings kept
    let mut order: Vec<Vec<usize>> = vec![vec![]; columns];
    for &i in &main {
        order[col[&i]].push(i);
    }
    let adj: BTreeMap<usize, Vec<usize>> = main.iter().map(|&i| (i, neighbours(i).into_iter().filter(|j| is_main(*j)).collect())).collect();
    let rows = |order: &Vec<Vec<usize>>| -> BTreeMap<usize, usize> { order.iter().flat_map(|c| c.iter().enumerate().map(|(r, i)| (*i, r))).collect() };
    // the crossings between each two neighbouring columns, a line across several columns counted in each gap it passes (at the
    // share of the column's height between its ends)
    let crossings = |order: &Vec<Vec<usize>>| -> usize {
        let row = rows(order);
        let share = |i: usize| (row[&i] as f64 + 0.5) / order[col[&i]].len().max(1) as f64;
        let mut lines: Vec<(usize, usize, f64, f64)> = vec![];
        for (i, v) in &adj {
            for j in v {
                if col[i] < col[j] {
                    lines.push((col[i], col[j], share(*i), share(*j)));
                }
            }
        }
        let at = |l: &(usize, usize, f64, f64), c: usize| l.2 + (l.3 - l.2) * (c - l.0) as f64 / (l.1 - l.0) as f64;
        let mut n = 0;
        for c in 0..order.len().saturating_sub(1) {
            let e: Vec<(f64, f64)> = lines.iter().filter(|l| l.0 <= c && l.1 > c).map(|l| (at(l, c), at(l, c + 1))).collect();
            for a in 0..e.len() {
                for b in a + 1..e.len() {
                    if (e[a].0 - e[b].0) * (e[a].1 - e[b].1) < -1e-9 {
                        n += 1;
                    }
                }
            }
        }
        n
    };
    let mut best = (crossings(&order), order.clone());
    for sweep in 0..6 {
        let right = sweep % 2 == 0;
        let ks: Vec<usize> = if right { (1..columns).collect() } else { (0..columns.saturating_sub(1)).rev().collect() };
        for k in ks {
            let row = rows(&order);
            let key = |i: usize| -> f64 {
                let mut v: Vec<f64> = adj[&i].iter().filter(|j| if right { col[*j] < k } else { col[*j] > k }).map(|j| row[j] as f64).collect();
                if v.is_empty() {
                    return row[&i] as f64;
                }
                v.sort_by(f64::total_cmp);
                let m = v.len() / 2;
                if v.len() % 2 == 1 { v[m] } else { (v[m - 1] + v[m]) / 2.0 }
            };
            order[k].sort_by(|a, b| key(*a).total_cmp(&key(*b)));
        }
        // neighbours swapped while that helps
        let mut now = crossings(&order);
        for _ in 0..3 {
            let mut better = false;
            for k in 0..columns {
                for r in 0..order[k].len().saturating_sub(1) {
                    order[k].swap(r, r + 1);
                    let n = crossings(&order);
                    if n < now {
                        now = n;
                        better = true;
                    } else {
                        order[k].swap(r, r + 1);
                    }
                }
            }
            if !better {
                break;
            }
        }
        if now < best.0 {
            best = (now, order.clone());
        }
    }
    let order = best.1;
    // each part's turn: supply up, ground down, signal pins toward the columns they join
    let turn_of = |i: usize| -> u8 {
        let c = &s.components[i];
        if !turnable(c) {
            return c.rot;
        }
        let pins = pins_of(c);
        let score = |rot: u8| -> i32 {
            let mut t = c.clone();
            t.rot = rot;
            t.pos = (0, 0);
            let at: BTreeMap<String, Pt> = t.pins().into_iter().collect();
            pins.iter()
                .map(|(p, n)| {
                    let (x, y) = at[p];
                    if is_ground(n) {
                        -4 * y
                    } else if supplies.contains(n) {
                        4 * y
                    } else {
                        // toward the earlier columns on the left, the later on the right
                        let others: Vec<usize> = on_net.get(n).map(|v| v.iter().copied().filter(|j| *j != i).collect()).unwrap_or_default();
                        let dir: i32 = others.iter().map(|j| (col[j] as i32 - col[&i] as i32).signum()).sum();
                        -dir * x
                    }
                })
                .sum::<i32>()
                + if rot == 0 { 0 } else { 1 }
        };
        (0..4u8).min_by_key(|r| score(*r)).unwrap_or(0)
    };
    // where the block of parts goes: the sheet's top left, or (only some) below the parts that stay
    let stay: Vec<&Component> = s.components.iter().filter(|c| !c.kind.is_power_symbol() && !chosen(c)).collect();
    let origin: Pt = match only {
        Some(_) if !stay.is_empty() => {
            let left = stay.iter().map(|c| c.pos.0).min().unwrap_or(0);
            let bottom = stay.iter().map(|c| c.pos.1 + part_box(c, c.rot).3).max().unwrap_or(0);
            (left, bottom + 8)
        }
        _ => s.components.iter().filter(|c| !c.kind.is_power_symbol()).map(|c| c.pos).fold((i32::MAX, i32::MAX), |a, p| (a.0.min(p.0), a.1.min(p.1))),
    };
    let origin = if origin.0 == i32::MAX { (8, 8) } else { origin };
    let mut out = s.clone();
    let turns: BTreeMap<usize, u8> = parts.iter().map(|i| (*i, turn_of(*i))).collect();
    let power_refs: BTreeSet<&str> = s.components.iter().filter(|c| c.kind.is_power_symbol()).map(|c| c.refdes.as_str()).collect();
    // the gap a shunt hangs in: after the first column of the line parts on its net, or, on a net within one column, on the side of
    // the pin (a base on the left of its transistor: the gap before)
    let mut gap_of: BTreeMap<usize, usize> = BTreeMap::new();
    for (i, n) in &shunt {
        let line: Vec<usize> = on_net.get(n).map(|v| v.iter().copied().filter(|j| is_main(*j)).collect()).unwrap_or_default();
        let k = col[i];
        let left_side = !line.iter().any(|j| col[j] > k)
            && line.iter().filter(|j| col[*j] == k).any(|j| {
                let c = &s.components[*j];
                let mut t = c.clone();
                t.rot = turns[j];
                t.pos = (0, 0);
                let b = part_box(c, t.rot);
                t.pins().into_iter().any(|q| net_of.get(&(c.refdes.clone(), q.0.clone())) == Some(n) && 2 * q.1 .0 < b.0 + b.2)
            });
        gap_of.insert(*i, if left_side && k > 0 { k - 1 } else { k });
    }
    let mut by_gap: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, k) in &gap_of {
        by_gap.entry(*k).or_default().push(*i);
    }
    let mut fb_of: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, u) in &feedback {
        fb_of.entry(*u).or_default().push(*i);
    }
    // the columns side by side, the parts of a column centred on one line
    let mut placed: BTreeSet<usize> = BTreeSet::new();
    let mut right_of: Vec<i32> = vec![];
    let mut x = origin.0;
    // each part's reach above its position (room for the names and symbols above it, and its feedback parts) and below
    let mut reach: BTreeMap<usize, (i32, i32)> = BTreeMap::new();
    for (k, column) in order.iter().enumerate() {
        let width = column.iter().map(|i| {
            let b = part_box(&s.components[*i], turns[i]);
            b.2 - b.0
        });
        let width = width.max().unwrap_or(0);
        let mut y = origin.1;
        for &i in column {
            let b = part_box(&s.components[i], turns[&i]);
            let above = 3 + fb_of.get(&i).map_or(0, |f| FEEDBACK_STEP * f.len() as i32 + 2);
            reach.insert(i, (b.1 - above, b.3));
            let c = &mut out.components[i];
            c.rot = turns[&i];
            c.pos = (x + (width - (b.2 - b.0)) / 2 - b.0, y - (b.1 - above));
            y = c.pos.1 + b.3 + ROW_GAP;
            placed.insert(i);
        }
        right_of.push(x + width);
        // room for the labels on the pins (written to the right of a pin) and for the shunts hung in the gap
        x += width + COLUMN_GAP + SHUNT_SLOT * by_gap.get(&k).map_or(0, |v| v.len() as i32);
    }
    // the heights (the coordinate assignment of a layered drawing): each part where its signal pins are level with the pins they
    // join in the other columns (the median of those), the column kept in order and apart by the least squares fit that keeps the
    // order (pool adjacent violators); sweeps to the right, back, and twice both ways
    for sweep in 0..4 {
        for k in 0..columns {
            let k = if sweep == 1 { columns - 1 - k } else { k };
            let column = &order[k];
            if column.is_empty() {
                continue;
            }
            // what each part would like: its position moved so a pin is level with a joined pin
            let wish: Vec<(f64, f64)> = column
                .iter()
                .map(|&i| {
                    let c = &out.components[i];
                    let mut v: Vec<f64> = vec![];
                    for (p, at) in c.pins() {
                        let Some(n) = net_of.get(&(c.refdes.clone(), p)) else { continue };
                        if !signal(n) {
                            continue;
                        }
                        for j in on_net.get(n).into_iter().flatten() {
                            let side = match sweep {
                                0 => col[j] < k,
                                1 => col[j] > k,
                                _ => col[j] != k,
                            };
                            if *j == i || !is_main(*j) || !side {
                                continue;
                            }
                            let oc = &out.components[*j];
                            for (q, there) in oc.pins() {
                                if net_of.get(&(oc.refdes.clone(), q)) == Some(n) {
                                    v.push((c.pos.1 + there.1 - at.1) as f64);
                                }
                            }
                        }
                    }
                    if v.is_empty() {
                        (c.pos.1 as f64, 0.05)
                    } else {
                        v.sort_by(f64::total_cmp);
                        let m = v.len() / 2;
                        (if v.len() % 2 == 1 { v[m] } else { (v[m - 1] + v[m]) / 2.0 }, 1.0)
                    }
                })
                .collect();
            // the spacing each part needs after the one before: z = position - the sum of the spacings must not go down
            let mut offset = vec![0i32; column.len()];
            for r in 1..column.len() {
                let (a, b) = (reach[&column[r - 1]], reach[&column[r]]);
                offset[r] = offset[r - 1] + a.1 + ROW_GAP - b.0;
            }
            let mut pools: Vec<(f64, f64, usize)> = vec![];
            for (r, (want, w)) in wish.iter().enumerate() {
                pools.push((want - offset[r] as f64, *w, 1));
                while pools.len() > 1 {
                    let (m2, w2, n2) = pools[pools.len() - 1];
                    let (m1, w1, n1) = pools[pools.len() - 2];
                    if m1 <= m2 {
                        break;
                    }
                    pools.pop();
                    pools.pop();
                    pools.push(((m1 * w1 + m2 * w2) / (w1 + w2), w1 + w2, n1 + n2));
                }
            }
            let mut r = 0;
            for (m, _, n) in pools {
                for _ in 0..n {
                    out.components[column[r]].pos.1 = m.round() as i32 + offset[r];
                    r += 1;
                }
            }
        }
    }
    // the whole block back to the top of its place
    let top = main.iter().map(|i| out.components[*i].pos.1 + reach[i].0).min().unwrap_or(origin.1);
    for i in &main {
        out.components[*i].pos.1 += origin.1 - top;
    }
    // a shunt below its net (above it, to a supply), in its own slot of the gap: its signal pin a few steps from the pin of the line
    // nearest the gap
    let anchors: BTreeMap<usize, Pt> = gap_of
        .iter()
        .map(|(i, k)| {
            let n = &shunt[i];
            let edge = right_of[*k];
            let at = out
                .components
                .iter()
                .enumerate()
                .filter(|(j, _)| is_main(*j) && placed.contains(j))
                .flat_map(|(_, c)| c.pins().into_iter().filter(|q| net_of.get(&(c.refdes.clone(), q.0.clone())) == Some(n)).map(|q| q.1).collect::<Vec<_>>())
                .min_by_key(|p| ((p.0 - edge).abs(), p.1))
                .unwrap_or((edge, origin.1));
            (*i, at)
        })
        .collect();
    let anchor = |i: usize| anchors[&i];
    for (k, list) in &by_gap {
        // the gap has a place more than it has shunts: the one left out is chosen so each shunt is nearest its pin (a shunt off the
        // minus input of an op amp at the right of the gap, close to it)
        let mut list = list.clone();
        list.sort_by_key(|i| (anchor(*i).0, anchor(*i).1));
        let spot = |s: usize| right_of[*k] + 3 + SHUNT_SLOT * s as i32;
        let skip = (0..=list.len()).min_by_key(|skip| list.iter().enumerate().map(|(r, i)| (spot(if r < *skip { r } else { r + 1 }) - anchor(*i).0).abs()).sum::<i32>()).unwrap_or(list.len());
        for (r, &i) in list.iter().enumerate() {
            let slot = if r < skip { r } else { r + 1 };
            let (_, ay) = anchor(i);
            let mut c = out.components[i].clone();
            c.rot = turns[&i];
            c.pos = (0, 0);
            let n = &shunt[&i];
            let pins = c.pins();
            let sig = pins.iter().find(|q| net_of.get(&(c.refdes.clone(), q.0.clone())) == Some(n)).map(|q| q.1).unwrap_or((0, 0));
            let other = pins.iter().map(|q| q.1).find(|q| *q != sig).unwrap_or((0, 0));
            let px = spot(slot);
            let py = if sig.1 <= other.1 { ay + 3 } else { ay - 3 };
            c.pos = (px - sig.0, py - sig.1);
            out.components[i] = c;
            placed.insert(i);
        }
    }
    // a feedback part above its part, level, the end on the net of the part's left pin on the left; several, one above another
    for (u, list) in &fb_of {
        let chip = out.components[*u].clone();
        let at = chip.pins();
        let top = at.iter().map(|q| q.1 .1).min().unwrap_or(chip.pos.1);
        let b = part_box(&chip, chip.rot);
        let mid = chip.pos.0 + (b.0 + b.2) / 2;
        let x_on = |n: &String| at.iter().filter(|q| net_of.get(&(chip.refdes.clone(), q.0.clone())) == Some(n)).map(|q| q.1 .0).min().unwrap_or(mid);
        for (step, &i) in list.iter().enumerate() {
            let mut c = out.components[i].clone();
            let nets_of: Vec<(String, String)> = pins_of(&s.components[i]);
            let left_net = nets_of.iter().min_by_key(|(_, n)| x_on(n)).map(|(p, _)| p.clone()).unwrap_or_default();
            let rot = [0u8, 2].into_iter().find(|r| {
                let mut t = c.clone();
                t.rot = *r;
                t.pos = (0, 0);
                let ps = t.pins();
                let lx = ps.iter().find(|q| q.0 == left_net).map(|q| q.1 .0).unwrap_or(0);
                ps.iter().all(|q| q.1 .0 >= lx)
            });
            c.rot = rot.unwrap_or(0);
            // clear of the supply symbol and its name on the part's top pin
            let y = top - 5 - FEEDBACK_STEP * step as i32;
            c.pos = (mid, y);
            out.components[i] = c;
            placed.insert(i);
        }
    }
    // a hung part that lands on another moves on away from its net (a shunt further down, or up to a supply; a feedback part up)
    // until it is clear
    let abs_box = |c: &Component| {
        let b = part_box(c, c.rot);
        (c.pos.0 + b.0, c.pos.1 + b.1, c.pos.0 + b.2, c.pos.1 + b.3)
    };
    let apart = |a: (i32, i32, i32, i32), b: (i32, i32, i32, i32)| a.2 < b.0 || b.2 < a.0 || a.3 < b.1 || b.3 < a.1;
    let mut settled: Vec<(i32, i32, i32, i32)> = main.iter().map(|i| abs_box(&out.components[*i])).collect();
    let hung: Vec<(usize, i32)> = shunt
        .keys()
        .map(|i| {
            let c = &out.components[*i];
            let ps = c.pins();
            let n = &shunt[i];
            let sig = ps.iter().find(|q| net_of.get(&(c.refdes.clone(), q.0.clone())) == Some(n)).map_or(0, |q| q.1 .1);
            (*i, if ps.iter().any(|q| q.1 .1 > sig) { 1 } else { -1 })
        })
        .chain(feedback.keys().map(|i| (*i, -1)))
        .collect();
    for (i, dir) in hung {
        for _ in 0..200 {
            let b = abs_box(&out.components[i]);
            if settled.iter().all(|o| apart(b, *o)) {
                break;
            }
            out.components[i].pos.1 += dir;
        }
        settled.push(abs_box(&out.components[i]));
    }
    // the sources of supplies in a row below everything else, from the left
    if !rail.is_empty() {
        let bottom = settled.iter().map(|b| b.3).max().unwrap_or(origin.1);
        let mut x = origin.0;
        for &i in &rail {
            let c = &mut out.components[i];
            c.rot = turns[&i];
            let b = part_box(c, c.rot);
            c.pos = (x - b.0, bottom + 7 - b.1);
            x += b.2 - b.0 + COLUMN_GAP;
        }
    }
    // the joints drawn again: what touched the moved parts' old pins goes, then a label, ground or power symbol on each pin
    let old_pins: BTreeSet<Pt> = parts.iter().flat_map(|i| s.components[*i].pins().into_iter().map(|p| p.1)).collect();
    let all = only.is_none();
    out.wires.retain(|w| !all && !old_pins.contains(&w.a) && !old_pins.contains(&w.b));
    out.labels.retain(|l| !all && !old_pins.contains(&l.pos));
    out.components.retain(|c| !(c.kind.is_power_symbol() && (all || old_pins.contains(&c.pos))));
    // a pin left open on a net without a name of its own needs nothing (a named one keeps its label: the name may be used, the ADC
    // measure table's nets)
    let alone: BTreeSet<String> = nets.iter().filter(|n| n.name.starts_with("N$") && n.pins.iter().filter(|p| !power_refs.contains(p.refdes.as_str())).count() <= 1).map(|n| n.name.clone()).collect();
    let mut joints: Vec<(Pt, String, Option<Kind>)> = vec![];
    let mut added: Vec<Wire> = vec![];
    for i in &parts {
        let c = out.components.iter().find(|x| x.refdes == s.components[*i].refdes).expect("kept");
        let at = c.pins();
        let (top, bottom) = (at.iter().map(|p| p.1 .1).min().unwrap_or(0), at.iter().map(|p| p.1 .1).max().unwrap_or(0));
        let pins = pins_of(&s.components[*i]);
        let mut mine: Vec<(Pt, String, Option<Kind>)> = vec![];
        for (p, n) in pins.iter().cloned() {
            if alone.contains(&n) {
                continue;
            }
            let pos = at.iter().find(|x| x.0 == p).map(|x| x.1).expect("pin");
            let symbol = if is_ground(&n) && pos.1 == bottom && top != bottom {
                Some(Kind::Ground)
            } else if supplies.contains(&n) && pos.1 == top && top != bottom {
                Some(Kind::Power)
            } else {
                None
            };
            mine.push((pos, n, symbol));
        }
        if wires {
            // a chip's neighbouring supply (or ground) pins of one net on its top (bottom) row: one symbol in the middle, the
            // others joined to it by a wire along the row (the names of several symbols side by side would overlap)
            let net_at = |q: &(String, Pt)| pins.iter().find(|r| r.0 == q.0).map(|r| &r.1);
            let rows: BTreeSet<(String, i32)> = mine.iter().filter(|j| j.2.is_some()).map(|j| (j.1.clone(), j.0 .1)).collect();
            for (n, y) in rows {
                let mut xs: Vec<i32> = mine.iter().filter(|j| j.2.is_some() && j.1 == n && j.0 .1 == y).map(|j| j.0 .0).collect();
                xs.sort();
                let mut runs: Vec<Vec<i32>> = vec![];
                for x in xs {
                    // no pin of another net between
                    let joins = runs.last().and_then(|r| r.last()).is_some_and(|&px| !at.iter().any(|q| q.1 .1 == y && q.1 .0 > px && q.1 .0 < x && net_at(q) != Some(&n)));
                    match runs.last_mut() {
                        Some(r) if joins => r.push(x),
                        _ => runs.push(vec![x]),
                    }
                }
                for r in runs.iter().filter(|r| r.len() > 1) {
                    let keep = r[r.len() / 2];
                    mine.retain(|j| !(j.2.is_some() && j.1 == n && j.0 .1 == y && r.contains(&j.0 .0) && j.0 .0 != keep));
                    added.push(Wire { a: (r[0], y), b: (*r.last().expect("run"), y) });
                }
            }
            // supply symbols of different nets close side by side: every other one raised on a short wire, so their names (written
            // above them) do not overlap
            let mut tops: Vec<usize> = (0..mine.len()).filter(|k| mine[*k].2 == Some(Kind::Power)).collect();
            tops.sort_by_key(|k| mine[*k].0);
            let mut placed: Vec<(i32, i32, usize)> = vec![];
            for k in tops {
                let (x, y) = mine[k].0;
                let len = mine[k].1.chars().count();
                let clash = |lift: i32| placed.iter().any(|(px, pl, plen)| *pl == lift && ((x - px).abs() as f32) < 0.375 * (len + plen) as f32 + 0.5);
                let lift = [0, 2, 4].into_iter().find(|l| !clash(*l)).unwrap_or(0);
                if lift > 0 {
                    added.push(Wire { a: (x, y), b: (x, y - lift) });
                    mine[k].0 = (x, y - lift);
                }
                placed.push((x, lift, len));
            }
        }
        joints.extend(mine);
    }
    // a net on two pins only (no symbol, none of the parts that stay): a wire between them where one fits, past the bodies of the
    // parts and touching nothing else; the net keeps its name by a label on one of them
    let mut wired = 0;
    let stay_nets: BTreeSet<&String> = stay.iter().flat_map(|c| c.pins().into_iter().filter_map(|(p, _)| net_of.get(&(c.refdes.clone(), p)))).collect();
    if wires {
        let boxes: Vec<(i32, i32, i32, i32)> = out
            .components
            .iter()
            .filter(|c| !c.kind.is_power_symbol())
            .map(|c| {
                let b = part_box(c, c.rot);
                (c.pos.0 + b.0, c.pos.1 + b.1, c.pos.0 + b.2, c.pos.1 + b.3)
            })
            .collect();
        let mut points: BTreeSet<Pt> = out.components.iter().filter(|c| !c.kind.is_power_symbol()).flat_map(|c| c.pins().into_iter().map(|p| p.1)).collect();
        points.extend(joints.iter().map(|j| j.0));
        points.extend(out.labels.iter().map(|l| l.pos));
        points.extend(out.components.iter().filter(|c| c.kind.is_power_symbol()).map(|c| c.pos));
        let mut segs: Vec<(Pt, Pt)> = out.wires.iter().chain(added.iter()).map(|w| (w.a, w.b)).collect();
        points.extend(segs.iter().flat_map(|s| [s.0, s.1]));
        let mut by_net: BTreeMap<&String, Vec<Pt>> = BTreeMap::new();
        for j in &joints {
            if j.2.is_none() {
                by_net.entry(&j.1).or_default().push(j.0);
            }
        }
        // the column of each pin
        let col_of: BTreeMap<&str, usize> = parts.iter().map(|i| (s.components[*i].refdes.as_str(), col[i])).collect();
        let col_at: BTreeMap<Pt, usize> = out.components.iter().filter_map(|c| col_of.get(c.refdes.as_str()).map(|k| (c, *k))).flat_map(|(c, k)| c.pins().into_iter().map(move |p| (p.1, k))).collect();
        // the nets for a wire: a signal on two pins, or on a few pins within two neighbouring columns (a longer one reads better by
        // its name than by a wire across the sheet; a supply and the ground by their symbols and names)
        let mut nets_to_wire: Vec<(String, Vec<Pt>)> = by_net
            .iter()
            .filter(|(n, v)| {
                let cols: BTreeSet<usize> = v.iter().filter_map(|p| col_at.get(p).copied()).collect();
                let span = cols.iter().max().unwrap_or(&0) - cols.iter().min().unwrap_or(&0);
                v.len() >= 2 && !stay_nets.contains(*n) && signal(n) && (v.len() == 2 || (v.len() <= TREE_PINS && span <= 1))
            })
            .map(|(n, v)| {
                let mut v = v.clone();
                v.sort();
                ((*n).clone(), v)
            })
            .collect();
        // the small ones first: they take the plain ways
        let spread = |v: &Vec<Pt>| {
            let (x0, x1) = (v.iter().map(|p| p.0).min().unwrap_or(0), v.iter().map(|p| p.0).max().unwrap_or(0));
            let (y0, y1) = (v.iter().map(|p| p.1).min().unwrap_or(0), v.iter().map(|p| p.1).max().unwrap_or(0));
            x1 - x0 + y1 - y0
        };
        nets_to_wire.sort_by_key(|t| (t.1.len(), spread(&t.1)));
        let mut drop: BTreeSet<Pt> = BTreeSet::new();
        let mut labels: Vec<Label> = vec![];
        for (n, pins) in nets_to_wire {
            // the tree grows from the leftmost pin: each time, the pin nearest to it by the shortest way that fits (the plainest
            // of equal ones), to a pin of the tree, a corner, or a point along one of its pieces
            let mut tree: Vec<Pt> = vec![pins[0]];
            let mut tree_segs: Vec<(Pt, Pt)> = vec![];
            let mut left: Vec<Pt> = pins[1..].to_vec();
            let own: BTreeSet<Pt> = pins.iter().copied().collect();
            while !left.is_empty() {
                let fits = |w: &Vec<Pt>, mine: &BTreeSet<Pt>| -> bool {
                    w.windows(2).all(|s| points.iter().all(|p| mine.contains(p) || !on_segment(*p, s[0], s[1])) && boxes.iter().all(|r| !crosses_box(s[0], s[1], *r)))
                        && w.iter().all(|p| segs.iter().all(|s| !on_segment(*p, s.0, s.1)))
                };
                let mine: BTreeSet<Pt> = own.iter().chain(tree.iter()).copied().collect();
                let mut best: Option<(i32, usize, Pt, Vec<Pt>)> = None;
                for &p in &left {
                    let mut targets: Vec<Pt> = tree.clone();
                    for s in &tree_segs {
                        let (x0, x1, y0, y1) = (s.0 .0.min(s.1 .0), s.0 .0.max(s.1 .0), s.0 .1.min(s.1 .1), s.0 .1.max(s.1 .1));
                        if y0 == y1 && p.0 > x0 && p.0 < x1 {
                            targets.push((p.0, y0));
                        }
                        if x0 == x1 && p.1 > y0 && p.1 < y1 {
                            targets.push((x0, p.1));
                        }
                    }
                    for t in targets {
                        let len = (p.0 - t.0).abs() + (p.1 - t.1).abs();
                        if best.as_ref().is_some_and(|b| len >= b.0) {
                            continue;
                        }
                        if let Some(w) = ways(p, t).into_iter().find(|w| fits(w, &mine)) {
                            best = Some((len, 0, p, w));
                        }
                    }
                }
                // no plain way fits: the shortest way around on the grid (to a chip's pin, past other parts)
                let best = best.or_else(|| {
                    let mut targets: BTreeSet<Pt> = tree.iter().copied().collect();
                    targets.extend(along(&tree_segs).into_keys());
                    let blocked: BTreeSet<Pt> = points.iter().filter(|p| !mine.contains(p)).copied().collect();
                    let wires = along(&segs);
                    left.iter()
                        .filter_map(|&p| maze(p, &targets, &blocked, &wires, &boxes).map(|w| (w.windows(2).map(|s| (s[1].0 - s[0].0).abs() + (s[1].1 - s[0].1).abs()).sum::<i32>() + 3 * w.len() as i32, 0, p, w)))
                        .filter(|b| fits(&b.3, &mine))
                        .min_by_key(|b| b.0)
                });
                let Some((_, _, p, w)) = best else { break };
                // a way ending on a piece of the tree splits it there
                let (first, last) = (w[0], w[w.len() - 1]);
                for end in [first, last] {
                    if let Some(k) = tree_segs.iter().position(|s| end != s.0 && end != s.1 && on_segment(end, s.0, s.1)) {
                        let s = tree_segs.remove(k);
                        tree_segs.extend([(s.0, end), (end, s.1)]);
                    }
                }
                tree_segs.extend(w.windows(2).map(|s| (s[0], s[1])));
                tree.extend(w.iter().copied());
                left.retain(|q| *q != p);
            }
            if !left.is_empty() {
                // a pin the tree cannot reach: the net keeps its labels
                continue;
            }
            wired += 1;
            drop.extend(pins.iter().copied());
            // a net with a name (or a symbol elsewhere) keeps it by a label on the wire: one step into its longest level piece, so
            // the name is written along the wire, clear of the parts' bodies (the piece is split there, a label names a point)
            let named = !n.starts_with("N$") || joints.iter().any(|j| j.1 == n && j.2.is_some());
            if named {
                let level = (0..tree_segs.len()).filter(|k| tree_segs[*k].0 .1 == tree_segs[*k].1 .1).max_by_key(|k| (tree_segs[*k].1 .0 - tree_segs[*k].0 .0).abs());
                // a point inside the piece, on no other wire and no corner of this one (a label there would join it)
                let spot = level.and_then(|k| {
                    let (a, b) = tree_segs[k];
                    let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
                    (x0 + 1..x1).map(|x| (x, a.1)).find(|p| !tree.contains(p) && segs.iter().chain(tree_segs.iter()).filter(|s| **s != (a, b)).all(|s| !on_segment(*p, s.0, s.1))).map(|p| (k, p))
                });
                match spot {
                    Some((k, at)) => {
                        let (a, b) = tree_segs.remove(k);
                        tree_segs.extend([(a, at), (at, b)]);
                        tree.push(at);
                        labels.push(Label { pos: at, text: n.clone() });
                    }
                    // no room along the wire: on a pin
                    None => labels.push(Label { pos: pins[pins.len() - 1], text: n.clone() }),
                }
            }
            for s in &tree_segs {
                added.push(Wire { a: s.0, b: s.1 });
            }
            segs.extend(tree_segs.iter().copied());
            points.extend(tree.iter().copied());
        }
        joints.retain(|j| !(j.2.is_none() && drop.contains(&j.0)));
        out.labels.extend(labels);
    }
    out.wires.extend(added);
    for (pos, n, symbol) in &joints {
        match symbol {
            Some(k) => {
                let i = out.add(*k, *pos, 0);
                if *k == Kind::Power {
                    out.components[i].value = n.clone();
                }
            }
            None => out.labels.push(Label { pos: *pos, text: n.clone() }),
        }
    }
    if !all {
        // the pins that stayed on a net the moved parts are on, now apart from them: a label of the net's name on each
        let moved_nets: BTreeSet<&String> = joints.iter().map(|j| &j.1).collect();
        for c in &stay {
            for (p, pos) in c.pins() {
                if let Some(n) = net_of.get(&(c.refdes.clone(), p))
                    && moved_nets.contains(n)
                    && !out.labels.iter().any(|l| l.pos == pos)
                {
                    out.labels.push(Label { pos, text: n.clone() });
                }
            }
        }
    }
    if grouping(&out) != before {
        return Err("整えると、つながりが変わってしまうので、やめました (階層の記号のように、位置でつながる物がある回路図は、まだ整えられません)".into());
    }
    Ok((out, Tidied { moved: parts.len(), columns, wired }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(name: &str) -> Schematic {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// Every sample tidies with its nets unchanged, and its parts no longer overlap.
    #[test]
    fn every_sample_tidies_without_changing_its_nets() {
        let mut n = 0;
        let mut refused = vec![];
        let mut wired = 0;
        for e in std::fs::read_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples")).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_none_or(|x| x != "schjson") {
                continue;
            }
            let s: Schematic = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
            match tidy(&s, None) {
                Ok((t, r)) => {
                    n += 1;
                    wired += r.wired;
                    assert!(tidy_with(&s, None, true).is_ok(), "{}: the wires join nothing they must not", p.display());
                    assert_eq!(grouping(&t), grouping(&s), "{}", p.display());
                    let names = |s: &Schematic| extract(s).into_iter().map(|n| n.name).filter(|n| !n.starts_with("N$")).collect::<BTreeSet<_>>();
                    assert_eq!(names(&t), names(&s), "{}: the nets keep their names", p.display());
                    let boxes: Vec<(i32, i32, i32, i32)> = t.components.iter().filter(|c| !c.kind.is_power_symbol()).map(|c| {
                        let b = part_box(c, c.rot);
                        (c.pos.0 + b.0, c.pos.1 + b.1, c.pos.0 + b.2, c.pos.1 + b.3)
                    }).collect();
                    // the names of the supply symbols do not overlap
                    let tops: Vec<&Component> = t.components.iter().filter(|c| c.kind == Kind::Power).collect();
                    for (i, a) in tops.iter().enumerate() {
                        for b in &tops[i + 1..] {
                            let room = 0.375 * (a.value.chars().count() + b.value.chars().count()) as f32 + 0.5;
                            assert!(a.pos.1 != b.pos.1 || ((a.pos.0 - b.pos.0).abs() as f32) >= room, "{}: {} and {} side by side", p.display(), a.value, b.value);
                        }
                    }
                    for (i, a) in boxes.iter().enumerate() {
                        for b in &boxes[i + 1..] {
                            assert!(a.2 < b.0 || b.2 < a.0 || a.3 < b.1 || b.3 < a.1, "{}: parts overlap {a:?} {b:?}", p.display());
                        }
                    }
                }
                Err(e) => refused.push(format!("{}: {e}", p.display())),
            }
        }
        assert!(n > 100, "{n} tidied; refused: {refused:?}");
        assert!(wired > 100, "{wired} nets wired");
        eprintln!("{n} tidied, {wired} nets wired");
    }

    /// The divider from a supply: the source on the left, the resistors turned with the supply pin up and ground down, the signal
    /// flowing to the right; a part chosen alone goes below the others.
    #[test]
    fn a_divider_reads_from_left_to_right() {
        let s = load("divider_tolerance.schjson");
        let (t, r) = tidy(&s, None).unwrap();
        assert!(r.columns >= 2, "{r:?}");
        // the source to R1 and R1 to R2 by wires, MID keeping its name on its wire
        assert_eq!(r.wired, 2, "{r:?}");
        assert!(t.labels.iter().any(|l| l.text == "MID" && t.wires.iter().any(|w| w.a == l.pos || w.b == l.pos)));
        let source = t.components.iter().find(|c| c.kind == Kind::VSource).unwrap();
        assert!(t.components.iter().filter(|c| c.kind == Kind::Resistor).all(|c| c.pos.0 > source.pos.0), "the source on the left");
        let only: BTreeSet<String> = [t.components.iter().find(|c| c.kind == Kind::Resistor).unwrap().refdes.clone()].into();
        let (u, r) = tidy(&t, Some(&only)).unwrap();
        assert_eq!(r.moved, 1);
        assert_eq!(grouping(&u), grouping(&s));
    }

    /// A non-inverting amplifier drawn as in a textbook: the feedback resistor level above the op amp, the resistor from its minus
    /// input to the ground and the load hung below their nets (upright, the ground at the bottom); the divider's lower resistor hangs
    /// below the joint, to the right of the upper one.
    #[test]
    fn shunts_hang_below_and_feedback_goes_above() {
        let s = load("opamp_amp.schjson");
        let (t, _) = tidy(&s, None).unwrap();
        let part = |r: &str| t.components.iter().find(|c| c.refdes == r).unwrap();
        let abs = |c: &Component| {
            let b = part_box(c, c.rot);
            (c.pos.0 + b.0, c.pos.1 + b.1, c.pos.0 + b.2, c.pos.1 + b.3)
        };
        let (u1, r1, r2, r3) = (abs(part("U1")), part("R1"), part("R2"), part("R3"));
        let level = |c: &Component| c.pins()[0].1 .1 == c.pins()[1].1 .1;
        assert!(level(r1) && abs(r1).3 < u1.1, "R1 level above U1: {:?} {u1:?}", abs(r1));
        assert!(abs(r1).0 >= u1.0 - 4 && abs(r1).2 <= u1.2 + 4, "R1 over U1");
        for r in [r2, r3] {
            assert!(!level(r), "{} upright", r.refdes);
        }
        let minus = part("U1").pins().into_iter().map(|p| p.1).filter(|p| p.0 < u1.0 + 1).max_by_key(|p| p.1).unwrap();
        assert!(abs(r2).1 > minus.1 && abs(r2).2 < u1.0, "R2 below and before the minus input");
        let d = load("divider_tolerance.schjson");
        let (t, _) = tidy(&d, None).unwrap();
        let part = |r: &str| t.components.iter().find(|c| c.refdes == r).unwrap();
        let (r1, r2) = (part("R1"), part("R2"));
        assert!(r2.pos.0 > r1.pos.0 && r2.pos.1 > r1.pos.1 && r2.pins()[0].1 .0 == r2.pins()[1].1 .0, "R2 hangs below the joint");
    }

    /// Supplies of different names on neighbouring pins of a chip: every other symbol raised on a short wire, so the names (written
    /// above the symbols) do not overlap.
    #[test]
    fn neighbouring_supplies_keep_their_names_apart() {
        let mut s = load("adc_measure.schjson");
        // the ADC's middle AVDD pin on a supply of its own
        s.labels.iter_mut().find(|l| l.pos == (16, 8)).unwrap().text = "DVDD".into();
        let mut v = s.components.iter().find(|c| c.refdes == "VA").unwrap().clone();
        v.refdes = "VD".into();
        v.pos = (110, 48);
        s.components.push(v);
        s.labels.push(Label { pos: (110, 46), text: "DVDD".into() });
        s.labels.push(Label { pos: (110, 50), text: "GND".into() });
        let (t, _) = tidy(&s, None).unwrap();
        assert_eq!(grouping(&t), grouping(&s));
        let u1 = t.components.iter().find(|c| c.refdes == "U1").unwrap();
        let top = u1.pins().iter().map(|p| p.1 .1).min().unwrap();
        let on_u1: Vec<&Component> = t.components.iter().filter(|c| c.kind == Kind::Power && (c.pos.0 - u1.pos.0).abs() < 8 && c.pos.1 < top + 1).collect();
        let names: BTreeSet<&str> = on_u1.iter().map(|c| c.value.as_str()).collect();
        assert!(names.contains("AVDD") && names.contains("DVDD"), "{names:?}");
        assert!(on_u1.iter().any(|c| c.pos.1 < top), "one raised");
        for a in &on_u1 {
            for b in &on_u1 {
                assert!(a.pos == b.pos || a.pos.1 != b.pos.1 || (a.pos.0 - b.pos.0).abs() >= 4, "{} and {} side by side", a.value, b.value);
            }
        }
    }

    /// The nets keep their names (the ADC measure table finds its nets by name), a pin left open on a named net included.
    #[test]
    fn the_nets_keep_their_names() {
        let s = load("adc_measure.schjson");
        let (t, _) = tidy(&s, None).unwrap();
        let names = |s: &Schematic| extract(s).into_iter().map(|n| n.name).filter(|n| !n.starts_with("N$")).collect::<BTreeSet<_>>();
        assert_eq!(names(&t), names(&s));
        assert!(names(&t).contains("HI"), "HI is on one pin only");
        // the ADC's three AVDD pins on its top row: one symbol, the others joined to it by a wire
        let avdd: Vec<&Component> = t.components.iter().filter(|c| c.kind == Kind::Power && c.value == "AVDD").collect();
        assert!(avdd.iter().all(|a| avdd.iter().all(|b| a.pos == b.pos || a.pos.1 != b.pos.1 || (a.pos.0 - b.pos.0).abs() > 4)), "no symbols side by side");
    }
}
