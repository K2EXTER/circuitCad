//! Which parts to place and lock first by hand (`lock_first`, `LockWhy`).

use super::*;

/// Why a part is worth placing by hand before the rest is laid out round it (`lock_first`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LockWhy {
    /// A connector, a header or a module: at the board's edge, or where the case has its hole.
    Connector,
    /// A knob or a switch: where the case has its hole.
    Control,
    /// A relay: big and heavy, its coils and contacts kept together away from the small signals.
    Relay,
    /// A big chip (14 leads or more): the parts round it go where it is.
    BigChip,
}

/// The parts worth placing by hand first on a perfboard (then locked, and the rest laid out round them): the connectors and modules,
/// the knobs and switches, the relays and the big chips, in that order (the most leads first within each), not those locked already.
/// By the part's name (J, CN, SW, VR, K …) and its leads.
pub fn lock_first(l: &StripLayout) -> Vec<(String, LockWhy)> {
    let prefix = |r: &str| r.chars().take_while(|c| c.is_ascii_alphabetic()).collect::<String>().to_ascii_uppercase();
    let mut v: Vec<(LockWhy, Reverse<usize>, String)> = l
        .parts
        .iter()
        .filter(|p| !l.locked.contains(&p.refdes))
        .filter_map(|p| {
            let n = p.leads.len();
            let why = match prefix(&p.refdes).as_str() {
                "J" | "P" | "CN" | "CON" => LockWhy::Connector,
                "SW" | "S" | "VR" | "RV" | "POT" | "ENC" => LockWhy::Control,
                "K" | "RL" | "RY" => LockWhy::Relay,
                _ if matches!(p.mount, crate::mount::Mount::Module { .. }) => LockWhy::Connector,
                _ if p.look == Look::Row && n >= LONG_ROW => LockWhy::Connector,
                _ if p.look == Look::Dip && n >= 14 => LockWhy::BigChip,
                _ => return None,
            };
            Some((why, Reverse(n), p.refdes.clone()))
        })
        .collect();
    v.sort();
    v.into_iter().map(|(w, _, r)| (r, w)).collect()
}
