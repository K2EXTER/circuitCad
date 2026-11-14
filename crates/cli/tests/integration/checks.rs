//! Writes the measured comparison tables into `crates/part-lib/model_checks.json` (shown next to the part on the screen).

use part_lib::model_checks::{CheckRow, ModelChecks, fingerprint};
use std::collections::BTreeMap;

/// The model text of the bundled part `name`.
pub fn bundled_model(name: &str) -> String {
    part_lib::EXAMPLES
        .iter()
        .filter_map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).ok())
        .find(|d| d.name == name)
        .and_then(|d| d.sim)
        .unwrap_or_else(|| panic!("no bundled model for {name}"))
        .text
}

/// `f` over `items` on all the cores, the answers in the order of the items. Each measurement runs `ccad` on a small circuit of its
/// own, so the rows are independent; one after another they took 20-80 s per file.
pub fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(items.len()).max(1);
    let mut done: Vec<(usize, R)> = std::thread::scope(|s| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let mut mine = vec![];
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(i) else { break mine };
                        mine.push((i, f(item)));
                    }
                })
            })
            .collect();
        workers.into_iter().flat_map(|w| w.join().unwrap_or_else(|e| std::panic::resume_unwind(e))).collect()
    });
    done.sort_by_key(|(i, _)| *i);
    done.into_iter().map(|(_, r)| r).collect()
}

/// Replaces the tables of the given parts, keeping the others'.
pub fn write(tables: Vec<(String, String, Vec<CheckRow>)>, date: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../part-lib/model_checks.json");
    let mut all: BTreeMap<String, ModelChecks> = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    for (name, source, rows) in tables {
        let fingerprint = fingerprint(&bundled_model(&name));
        all.insert(name, ModelChecks { fingerprint, source, date: date.into(), rows });
    }
    std::fs::write(&path, serde_json::to_string_pretty(&all).unwrap() + "\n").unwrap();
}
