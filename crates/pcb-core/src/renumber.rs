//! Renumbering the parts by where they sit on the board (R1, R2, ... from the top row to the bottom row, each row from left to right),
//! and following the new numbers through everything on the board that names a part.

use crate::model::*;
use std::collections::BTreeMap;

/// Parts whose centres are less than this far apart (up and down, mm) from the first part of a row are in the same row.
pub const ROW_MM: f64 = 3.0;

/// "R12" -> ("R", 12). Anything else (a copy of a sheet like "R1_CH1", a power symbol) is not renumbered.
fn split(refdes: &str) -> Option<(&str, u32)> {
    let at = refdes.find(|c: char| c.is_ascii_digit())?;
    let (prefix, num) = refdes.split_at(at);
    if prefix.is_empty() || !prefix.chars().all(|c| c.is_ascii_alphabetic()) || !num.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((prefix, num.parse().ok()?))
}

/// The new numbers, old -> new, for the parts whose number changes: within each letter (R, C, U ...) the parts are numbered from 1 in
/// reading order. Parts locked in place keep their numbers (and the others do not take those numbers).
pub fn renumber_plan(board: &Board) -> BTreeMap<String, String> {
    let mut by_prefix: BTreeMap<&str, Vec<&Part>> = BTreeMap::new();
    for p in &board.parts {
        if let Some((prefix, _)) = split(&p.refdes) {
            by_prefix.entry(prefix).or_default().push(p);
        }
    }
    let mut plan = BTreeMap::new();
    for (prefix, mut parts) in by_prefix {
        parts.sort_by(|a, b| a.pos.y.total_cmp(&b.pos.y));
        // rows: a part starts a new row when it is lower than the row's first part by more than ROW_MM
        let mut rows: Vec<Vec<&Part>> = vec![];
        for p in parts {
            match rows.last_mut() {
                Some(row) if p.pos.y - row[0].pos.y <= ROW_MM => row.push(p),
                _ => rows.push(vec![p]),
            }
        }
        let ordered: Vec<&Part> = rows
            .into_iter()
            .flat_map(|mut row| {
                row.sort_by(|a, b| a.pos.x.total_cmp(&b.pos.x));
                row
            })
            .collect();
        let kept: std::collections::BTreeSet<u32> = ordered.iter().filter(|p| board.is_locked(&p.refdes)).filter_map(|p| split(&p.refdes).map(|s| s.1)).collect();
        let mut next = (1u32..).filter(|n| !kept.contains(n));
        for p in ordered.iter().filter(|p| !board.is_locked(&p.refdes)) {
            let new = format!("{prefix}{}", next.next().unwrap_or(1));
            if new != p.refdes {
                plan.insert(p.refdes.clone(), new);
            }
        }
    }
    plan
}

fn rename_keys<V>(m: &mut BTreeMap<String, V>, map: &BTreeMap<String, String>) {
    let old = std::mem::take(m);
    *m = old.into_iter().map(|(k, v)| (map.get(&k).cloned().unwrap_or(k), v)).collect();
}

impl Board {
    /// Give parts new reference designators (`map`: old -> new, all at once). Everything that names a part follows: the pads, holes,
    /// silk, locks, the shop numbers, the pin trades, and the rows of the table of requirements. Returns how many parts changed.
    pub fn rename_parts(&mut self, map: &BTreeMap<String, String>) -> usize {
        let map: BTreeMap<String, String> = map.iter().filter(|(a, b)| a != b).map(|(a, b)| (a.clone(), b.clone())).collect();
        if map.is_empty() {
            return 0;
        }
        let r = |s: &mut String| {
            if let Some(n) = map.get(s.as_str()) {
                *s = n.clone();
            }
        };
        let mut changed = 0;
        for p in &mut self.parts {
            if map.contains_key(&p.refdes) {
                changed += 1;
            }
            r(&mut p.refdes);
        }
        self.pads.iter_mut().for_each(|p| r(&mut p.refdes));
        self.holes.iter_mut().for_each(|h| r(&mut h.refdes));
        self.silk.iter_mut().for_each(|s| r(&mut s.refdes));
        self.texts.iter_mut().for_each(|t| r(&mut t.refdes));
        self.locked_parts.iter_mut().for_each(r);
        self.hidden_labels.iter_mut().for_each(r);
        rename_keys(&mut self.part_shops, &map);
        rename_keys(&mut self.part_sheets, &map);
        rename_keys(&mut self.pin_map, &map);
        rename_keys(&mut self.label_offsets, &map);
        let mut spec = design_spec::DesignSpec {
            groups: std::mem::take(&mut self.place_groups),
            apart: std::mem::take(&mut self.keep_apart),
            bypass: std::mem::take(&mut self.bypass),
            waivers: std::mem::take(&mut self.waivers),
            measures: std::mem::take(&mut self.measures),
            ..Default::default()
        };
        spec.rename_parts(&map);
        (self.place_groups, self.keep_apart, self.bypass, self.waivers, self.measures) = (spec.groups, spec.apart, spec.bypass, spec.waivers, spec.measures);
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(r: &str, x: f64, y: f64) -> Part {
        Part { refdes: r.into(), value: "1k".into(), footprint: "R_0603".into(), pos: Point { x, y }, rot: 0, side: Side::Top }
    }

    fn board() -> Board {
        let mut b = Board::new("t", 50.0, 50.0);
        // R1 is at the lower right, R2 at the upper right, R3 at the upper left; C1 alone
        b.parts = vec![part("R1", 30.0, 20.0), part("R2", 30.0, 5.0), part("R3", 10.0, 6.0), part("C1", 5.0, 5.0), part("R1_CH1", 1.0, 1.0)];
        b
    }

    fn map(p: &[(&str, &str)]) -> BTreeMap<String, String> {
        p.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn numbers_follow_the_reading_order_row_by_row() {
        let plan = renumber_plan(&board());
        // the upper row (y 5 and 6) reads R3 (x 10) then R2 (x 30); the lower row has R1
        assert_eq!(plan, map(&[("R3", "R1"), ("R1", "R3")]), "R2 is already second");
        assert!(!plan.contains_key("C1"), "already C1");
        assert!(!plan.contains_key("R1_CH1"), "a copy of a sheet keeps its name");
    }

    #[test]
    fn a_locked_part_keeps_its_number() {
        let mut b = board();
        b.locked_parts = vec!["R1".into()];
        // R1 stays; in reading order the others (R3, R2) take 2 and 3
        assert_eq!(renumber_plan(&b), map(&[("R3", "R2"), ("R2", "R3")]));
    }

    #[test]
    fn everything_that_names_a_part_follows() {
        let mut b = board();
        let mut pad = Pad::new(Point { x: 0.0, y: 0.0 }, PadShape::Circle { d: 1.0 }, None, Side::Top);
        pad.refdes = "R1".into();
        b.pads.push(pad);
        b.part_shops.insert("R1".into(), Default::default());
        b.pin_map.insert("R2".into(), Default::default());
        b.locked_parts = vec!["R3".into()];
        b.place_groups.push(design_spec::PlaceGroup { name: "g".into(), parts: vec!["R1".into(), "R2".into()], ..Default::default() });
        assert_eq!(b.rename_parts(&map(&[("R1", "R3"), ("R3", "R1")])), 2);
        assert_eq!(b.pads[0].refdes, "R3");
        assert!(b.part_shops.contains_key("R3") && b.pin_map.contains_key("R2"));
        assert_eq!(b.locked_parts, vec!["R1"]);
        assert_eq!(b.place_groups[0].parts, vec!["R3", "R2"]);
        assert_eq!(b.parts.iter().filter(|p| p.refdes == "R3").count(), 1);
    }
}
