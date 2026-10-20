//! Running independent jobs on several threads, in the order they were given.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// `items.iter().map(f).collect()`, with the items shared out over the machine's cores. The results come back in the order of `items`
/// whatever order the threads finish in. The simulation options (a per-thread setting) are copied to every worker. With one core, or
/// fewer than two items, it runs on the calling thread.
pub fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get()).min(items.len());
    if workers < 2 {
        return items.iter().map(f).collect();
    }
    let options = crate::options::options();
    let next = AtomicUsize::new(0);
    let done: Mutex<Vec<(usize, R)>> = Mutex::new(Vec::with_capacity(items.len()));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                crate::options::set_options(options);
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(i) else { break };
                    let r = f(item);
                    done.lock().unwrap().push((i, r));
                }
            });
        }
    });
    let mut done = done.into_inner().unwrap();
    done.sort_by_key(|(i, _)| *i);
    done.into_iter().map(|(_, r)| r).collect()
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
    fn the_options_of_the_caller_reach_the_workers() {
        let saved = crate::options::options();
        crate::options::set_options(crate::options::SimOptions { gmin: 3e-9, ..saved });
        let seen = par_map(&[0u8; 32], |_| crate::options::options().gmin);
        crate::options::set_options(saved);
        assert!(seen.iter().all(|g| *g == 3e-9), "{seen:?}");
    }
}
