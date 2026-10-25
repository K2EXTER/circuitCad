//! Running independent jobs on several threads, in the order they were given (the pours of a board, the ends of its tracks).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// `items.iter().map(f).collect()`, with the items shared out over the machine's cores. The results come back in the order of `items`
/// whatever order the threads finish in. With one core, or fewer than two items, it runs on the calling thread.
pub fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get()).min(items.len());
    if workers < 2 {
        return items.iter().map(f).collect();
    }
    let next = AtomicUsize::new(0);
    let finished: Mutex<Vec<(usize, R)>> = Mutex::new(Vec::with_capacity(items.len()));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(item) = items.get(i) else { break };
                let r = f(item);
                finished.lock().unwrap().push((i, r));
            });
        }
    });
    let mut out: Vec<Option<R>> = (0..items.len()).map(|_| None).collect();
    for (i, r) in finished.into_inner().unwrap() {
        out[i] = Some(r);
    }
    out.into_iter().map(|r| r.expect("every item was done")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_results_come_back_in_the_order_of_the_items() {
        let v: Vec<u32> = (0..200).collect();
        let r = par_map(&v, |x| {
            if x % 7 == 0 {
                std::thread::sleep(std::time::Duration::from_micros(300));
            }
            x * 2
        });
        assert_eq!(r, v.iter().map(|x| x * 2).collect::<Vec<_>>());
        assert!(par_map(&Vec::<u32>::new(), |x| *x).is_empty());
        assert_eq!(par_map(&[5u32], |x| x + 1), vec![6]);
    }
}
