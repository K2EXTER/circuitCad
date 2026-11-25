//! Compares the stripboard and perfboard layouts before and after a change: for each schematic (the files given, or every sample in
//! docs/examples), the board's size, its wires on top and the check's problems, with and without a board width. Run it in release
//! (the perfboard layout stops at a time budget, so a debug build tries fewer layouts):
//!
//! `cargo run --release -p sch-core --example strip_bench -- [--cols 59] [--strip] [--back] [FILE.schjson ...]` (`--strip`: the stripboard only; `--back`: the chip resistors and capacitors on the copper side)

use sch_core::stripboard::{strip_check, strip_layout_with, StripOptions};
use sch_core::Schematic;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut cols: Option<u16> = Some(59);
    let mut files: Vec<String> = vec![];
    let mut strip_only = false;
    let mut back = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--strip" => strip_only = true,
            "--back" => back = true,
            "--cols" => cols = it.next().and_then(|c| c.parse().ok()).filter(|c| *c > 0),
            _ => files.push(a.clone()),
        }
    }
    let all = files.is_empty();
    if all {
        let mut v: Vec<String> = std::fs::read_dir("docs/examples").unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "schjson")).map(|p| p.display().to_string()).collect();
        v.sort();
        files = v;
    }
    // totals: holes, wires, problems, per board kind and width
    let mut total = [[0usize; 4]; 4];
    for f in &files {
        let Ok(s) = std::fs::read_to_string(f).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<Schematic>(&t).map_err(|e| e.to_string())) else { continue };
        let mut line = format!("{:<40}", f.rsplit(['/', '\\']).next().unwrap_or(f));
        for (k, (perf, width)) in [(false, None), (false, cols), (true, None), (true, cols)].into_iter().enumerate() {
            if perf && strip_only {
                continue;
            }
            let t = Instant::now();
            let opt = StripOptions { cols: width, chips_back: back, ..Default::default() };
            let l = if perf { sch_core::perfboard::perf_layout_with(&s, opt) } else { strip_layout_with(&s, opt) };
            let ms = t.elapsed().as_millis();
            let p = strip_check(&s, &l).len();
            total[k][0] += l.rows as usize * l.cols as usize;
            total[k][1] += l.links.len();
            total[k][2] += p;
            let backs = l.parts.iter().filter(|x| matches!(x.mount, sch_core::mount::Mount::Back { .. })).count();
            total[k][3] += backs;
            if !all && p > 0 {
                eprintln!("{f} {}{:?}: {:?}", if perf { "perf" } else { "strip" }, width, strip_check(&s, &l).iter().take(5).collect::<Vec<_>>());
            }
            if !all {
                line += &format!(" | {}{}: {}x{} links {} problems {} back {backs} ({ms} ms)", if perf { "perf" } else { "strip" }, width.map(|w| format!(" w{w}")).unwrap_or_default(), l.cols, l.rows, l.links.len(), p);
            }
        }
        if !all {
            println!("{line}");
        }
    }
    for (k, name) in ["strip", "strip w", "perf", "perf w"].iter().enumerate() {
        println!("{name}: holes {} links {} problems {} back {}", total[k][0], total[k][1], total[k][2], total[k][3]);
    }
}
