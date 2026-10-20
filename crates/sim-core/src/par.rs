//! Running independent jobs on several threads, in the order they were given.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

/// `items.iter().map(f).collect()`, with the items shared out over the machine's cores. The results come back in the order of `items`
/// whatever order the threads finish in. The simulation options (a per-thread setting) are copied to every worker. With one core, or
/// fewer than two items, it runs on the calling thread.
pub fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    par_map_progress(items, f, &AtomicUsize::new(0), &AtomicBool::new(false)).into_iter().map(|r| r.expect("not cancelled")).collect()
}

/// `par_map` that counts the finished items in `done` (a progress bar can read it while the jobs run) and stops handing out items once
/// `cancel` is set. Items that were not started come back as `None`; the ones that were started finish.
pub fn par_map_progress<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync, done: &AtomicUsize, cancel: &AtomicBool) -> Vec<Option<R>> {
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get()).min(items.len());
    if workers < 2 {
        return items
            .iter()
            .map(|item| {
                if cancel.load(Ordering::Relaxed) {
                    return None;
                }
                let r = f(item);
                done.fetch_add(1, Ordering::Relaxed);
                Some(r)
            })
            .collect();
    }
    let options = crate::options::options();
    let next = AtomicUsize::new(0);
    let finished: Mutex<Vec<(usize, R)>> = Mutex::new(Vec::with_capacity(items.len()));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                crate::options::set_options(options);
                while !cancel.load(Ordering::Relaxed) {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(i) else { break };
                    let r = f(item);
                    finished.lock().unwrap().push((i, r));
                    done.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
    });
    let mut out: Vec<Option<R>> = (0..items.len()).map(|_| None).collect();
    for (i, r) in finished.into_inner().unwrap() {
        out[i] = Some(r);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_keep_the_order_of_the_items_and_match_the_sequential_map() {
        let items: Vec<u64> = (0..500).collect();
        // uneven work, so the threads finish out of order
        let f = |x: &u64| (0..(x % 7) * 20_000).fold(*x, |a, k| a.wrapping_mul(31).wrapping_add(k));
        assert_eq!(par_map(&items, f), items.iter().map(f).collect::<Vec<_>>());
        assert_eq!(par_map(&items[..1], f), vec![f(&0)]);
        assert!(par_map(&Vec::<u64>::new(), f).is_empty());
    }

    #[test]
    fn progress_is_counted_and_a_cancel_stops_new_items() {
        let items: Vec<u32> = (0..200).collect();
        let (done, cancel) = (AtomicUsize::new(0), AtomicBool::new(false));
        let all = par_map_progress(&items, |x| x + 1, &done, &cancel);
        assert_eq!(done.load(Ordering::Relaxed), 200);
        assert!(all.iter().all(Option::is_some));
        // cancelled after ten items: the rest are not started, the finished ones keep their values
        let (done, cancel) = (AtomicUsize::new(0), AtomicBool::new(false));
        let some = par_map_progress(
            &items,
            |x| {
                if done.load(Ordering::Relaxed) >= 10 {
                    cancel.store(true, Ordering::Relaxed);
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
                x + 1
            },
            &done,
            &cancel,
        );
        let finished = some.iter().flatten().count();
        assert!((10..200).contains(&finished), "{finished}");
        assert!(some.iter().enumerate().all(|(i, r)| r.is_none_or(|v| v == i as u32 + 1)));
    }

    #[test]
    fn the_options_of_the_caller_reach_the_workers() {
        let saved = crate::options::options();
        crate::options::set_options(crate::options::SimOptions { gmin: 3e-9, ..saved });
        let seen = par_map(&[0u8; 32], |_| crate::options::options().gmin);
        crate::options::set_options(saved);
        assert!(seen.iter().all(|g| *g == 3e-9), "{seen:?}");
    }
}
