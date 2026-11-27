//! The tests' layouts, each worked out once: many tests lay out the same sample with the same options (a perfboard's layout
//! takes up to 2 s), and a layout depends on nothing else (it stops at an amount of work, not of time), so the first test to ask
//! works it out and the others get a copy. The key is the schematic as written and the options: a test that changes the
//! schematic first gets a layout of its own. A test that checks the layout itself comes out the same each time calls the
//! layout directly.

use crate::stripboard::{StripLayout, StripOptions};
use crate::Schematic;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

type Made = Arc<OnceLock<StripLayout>>;

fn once(kind: &str, s: &Schematic, opt: StripOptions, make: impl FnOnce() -> StripLayout) -> StripLayout {
    static ALL: OnceLock<Mutex<HashMap<String, Made>>> = OnceLock::new();
    let key = format!("{kind} {opt:?} {}", serde_json::to_string(s).unwrap());
    // the entry under the lock, the layout outside it: two tests asking for the same one wait for one working, different ones run side by side
    let made = ALL.get_or_init(Default::default).lock().unwrap().entry(key).or_default().clone();
    made.get_or_init(make).clone()
}

/// `perf_layout_with(s, StripOptions::default())`, once.
pub(crate) fn perf(s: &Schematic) -> StripLayout {
    perf_with(s, StripOptions::default())
}

/// `perf_layout_with(s, opt)`, once.
pub(crate) fn perf_with(s: &Schematic, opt: StripOptions) -> StripLayout {
    once("perf", s, opt, || crate::perfboard::perf_layout_with(s, opt))
}

/// `strip_layout(s)`, once.
pub(crate) fn strip(s: &Schematic) -> StripLayout {
    once("strip", s, StripOptions::default(), || crate::stripboard::strip_layout(s))
}
