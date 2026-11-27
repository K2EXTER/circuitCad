//! Compares the perfboard's wiring ("自動配線", `perf_route_left`) before and after a change, on the same placements: for each sample
//! (or the files given), its automatic layout's parts with the wires taken off, wired again. The placements are saved once
//! (`--save DIR`) and read back after the change (`--load DIR`), so a change to the wiring is measured apart from the placing it would
//! also change. Prints, per board and in all: the nets left apart, the wires on top, the bare wires (pieces and length in holes, 45
//! degrees counting √2) and how much longer the copper is than the shortest tree of each net's leads (the same lengths, along the rows,
//! the columns and at 45 degrees). Run it in release:
//!
//! `cargo run --release -p sch-core --example perf_wire_bench -- --save DIR [FILE.schjson ...]`, then after the change `-- --load DIR`
//! (`--layout`: the automatic layout itself measured instead, placed and wired, nothing saved)

use sch_core::perfboard::{perf_layout_with, perf_route_left, without_wires};
use sch_core::stripboard::{strip_unjoined, StripLayout, StripOptions};
use sch_core::Schematic;
use std::collections::BTreeMap;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut save, mut load, mut files, mut layout) = (None, None, vec![], false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--save" => save = it.next().cloned(),
            "--load" => load = it.next().cloned(),
            "--layout" => layout = true,
            _ => files.push(a.clone()),
        }
    }
    let dir = save.clone().or(load.clone()).unwrap_or_else(|| std::env::temp_dir().display().to_string());
    assert!(layout || save.is_some() || load.is_some(), "--save DIR, --load DIR or --layout");
    std::fs::create_dir_all(&dir).unwrap();
    if files.is_empty() {
        let mut v: Vec<String> = std::fs::read_dir("docs/examples").unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "schjson")).map(|p| p.display().to_string()).collect();
        v.sort();
        files = v;
    }
    let mut total = [0f64; 6];
    for f in &files {
        let Ok(s) = std::fs::read_to_string(f).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<Schematic>(&t).map_err(|e| e.to_string())) else { continue };
        let name = f.rsplit(['/', '\\']).next().unwrap_or(f).trim_end_matches(".schjson").to_string();
        let path = format!("{dir}/{name}.json");
        let placed: StripLayout = if layout {
            StripLayout::default()
        } else if save.is_some() {
            let l = without_wires(&perf_layout_with(&s, StripOptions::default()));
            std::fs::write(&path, serde_json::to_string(&l).unwrap()).unwrap();
            l
        } else {
            match std::fs::read_to_string(&path) {
                Ok(t) => serde_json::from_str(&t).unwrap(),
                Err(_) => continue,
            }
        };
        if placed.parts.is_empty() && !layout {
            continue;
        }
        let t = Instant::now();
        let l = if layout { perf_layout_with(&s, StripOptions::default()) } else { perf_route_left(&s, &placed) };
        if l.parts.is_empty() {
            continue;
        }
        let ms = t.elapsed().as_millis();
        let m = measure(&s, &l);
        for (k, v) in m.iter().enumerate() {
            total[k] += v;
        }
        total[5] += ms as f64;
        println!("{name:<36} apart {:>2}  top {:>3}  bare {:>4} pieces {:>7.1} holes  shortest {:>7.1}  ({:+5.1} %)  {ms} ms", m[0], m[1], m[2], m[3], m[4], 100.0 * (m[3] / m[4].max(1.0) - 1.0));
    }
    println!(
        "ALL apart {}  top {}  bare {} pieces {:.1} holes  shortest {:.1}  ({:+.1} %)  {:.1} s",
        total[0],
        total[1],
        total[2],
        total[3],
        total[4],
        100.0 * (total[3] / total[4].max(1.0) - 1.0),
        total[5] / 1000.0
    );
}

/// The nets left apart, the wires on top, the bare wires' pieces and length, and the shortest trees' length (of each net's leads
/// joined, by its copper).
fn measure(s: &Schematic, l: &StripLayout) -> [f64; 5] {
    let apart = strip_unjoined(s, l).len() as f64;
    let len = |a: (u16, u16), b: (u16, u16)| {
        let (dr, dc) = (a.0.abs_diff(b.0) as f64, a.1.abs_diff(b.1) as f64);
        dr.max(dc) - dr.min(dc) + dr.min(dc) * std::f64::consts::SQRT_2
    };
    let bare: f64 = l.bare.iter().map(|(a, b)| len(*a, *b)).sum::<f64>() + l.links.iter().map(|k| len(k.a, k.b)).sum::<f64>();
    // each piece of copper's leads, and the shortest tree joining them (Prim's)
    let groups = l.hole_groups();
    let mut leads: BTreeMap<usize, Vec<(u16, u16)>> = BTreeMap::new();
    for a in l.parts.iter().flat_map(|p| p.leads.iter().map(|x| x.1)).chain(l.off.iter().flat_map(|(_, v)| v.iter().map(|w| w.2))) {
        if let Some(g) = groups.get(&a) {
            leads.entry(*g).or_default().push(a);
        }
    }
    let mut shortest = 0.0;
    for v in leads.values() {
        let mut inside = vec![false; v.len()];
        let mut d = vec![f64::MAX; v.len()];
        d[0] = 0.0;
        for _ in 0..v.len() {
            let i = (0..v.len()).filter(|i| !inside[*i]).min_by(|a, b| d[*a].total_cmp(&d[*b])).unwrap();
            inside[i] = true;
            shortest += d[i];
            for j in 0..v.len() {
                if !inside[j] {
                    d[j] = d[j].min(len(v[i], v[j]));
                }
            }
        }
    }
    [apart, l.links.len() as f64, l.bare.len() as f64, bare, shortest]
}
