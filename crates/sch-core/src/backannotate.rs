//! Back-annotation: the pin trades made on the board (gates of a 74HC32 swapped, the two inputs of an OR gate) go back into the schematic,
//! so that the drawing and the board say the same thing and the next import has nothing left to remember.
//!
//! The board keeps a trade as `pin_map`: for a part, "the pad of pin P carries the net of schematic pin S". Here the wires and net labels
//! that end on the tip of pin S are moved to the tip of pin P. The wires then cross each other on the drawing (the part is not redrawn),
//! but the nets are what the board has. The result is checked against the nets before: when anything else would join or split, nothing is
//! changed.

use crate::*;
use std::collections::{BTreeMap, BTreeSet};

/// What one call did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BackAnnotation {
    /// The parts whose pins were traded in the schematic.
    pub parts: Vec<String>,
    /// How many wire ends and net labels were moved.
    pub moved: usize,
}

impl Schematic {
    /// Apply the trades of the board (`pin_map`: part -> pad pin -> the schematic pin whose net it carries) to the schematic.
    /// A part that is not in the schematic is left out of the answer. On an error the schematic is not changed.
    pub fn apply_pin_trades(&mut self, trades: &BTreeMap<String, BTreeMap<String, String>>) -> Result<BackAnnotation, String> {
        let mut work = self.clone();
        let mut rep = BackAnnotation::default();
        for (refdes, map) in trades {
            let map: BTreeMap<&String, &String> = map.iter().filter(|(p, s)| p != s).collect();
            if map.is_empty() {
                continue;
            }
            let keys: BTreeSet<&String> = map.keys().copied().collect();
            let values: BTreeSet<&String> = map.values().copied().collect();
            if keys != values {
                return Err(format!("{refdes}: ピンの入れ替えが輪になっていません (入れ替えた先のピンが、別のピンへ移っていません)。基板の「回路図から取り込み」をやり直してから、入れ替えをもう一度してください"));
            }
            let Some(k) = work.sheet_of_part(refdes) else { continue };
            let n = work.trade_on_sheet(k, refdes, &map)?;
            rep.parts.push(refdes.clone());
            rep.moved += n;
            // a "not connected" mark follows its pin
            let marks: Vec<(String, String)> = work.no_connects.iter().filter(|(r, _)| r == refdes).cloned().collect();
            for (r, p) in marks {
                if let Some((newp, _)) = map.iter().find(|(_, s)| ***s == p) {
                    work.no_connects.retain(|(r2, p2)| !(r2 == &r && p2 == &p));
                    work.no_connects.push((r, (*newp).clone()));
                }
            }
        }
        if rep.parts.is_empty() {
            return Ok(rep);
        }
        check_same_nets(&self.flat(), &work.flat(), trades)?;
        *self = work;
        Ok(rep)
    }

    fn sheet_of_part(&self, refdes: &str) -> Option<usize> {
        (0..self.sheet_count()).find(|&k| self.sheet_fields(k).0.iter().any(|c| c.refdes == refdes))
    }

    fn sheet_fields(&self, k: usize) -> (&Vec<Component>, &Vec<Wire>, &Vec<Label>) {
        if self.sheets.is_empty() || k == self.active_sheet {
            (&self.components, &self.wires, &self.labels)
        } else {
            let s = &self.sheets[k];
            (&s.components, &s.wires, &s.labels)
        }
    }

    fn sheet_fields_mut(&mut self, k: usize) -> (&mut Vec<Component>, &mut Vec<Wire>, &mut Vec<Label>) {
        if self.sheets.is_empty() || k == self.active_sheet {
            (&mut self.components, &mut self.wires, &mut self.labels)
        } else {
            let s = &mut self.sheets[k];
            (&mut s.components, &mut s.wires, &mut s.labels)
        }
    }

    /// Move the wire ends and labels on the tip of pin `S` to the tip of pin `P` for every `P -> S` of `map`. Returns how many were moved.
    fn trade_on_sheet(&mut self, k: usize, refdes: &str, map: &BTreeMap<&String, &String>) -> Result<usize, String> {
        let (comps, wires, labels) = self.sheet_fields_mut(k);
        let Some(c) = comps.iter().find(|c| c.refdes == refdes) else { return Ok(0) };
        let tips: BTreeMap<String, Pt> = c.pins().into_iter().collect();
        // from the tip of S to the tip of P
        let mut to: BTreeMap<Pt, Pt> = BTreeMap::new();
        for (p, s) in map {
            let (Some(&tp), Some(&ts)) = (tips.get(*p), tips.get(*s)) else {
                return Err(format!("{refdes}: ピン {p} か {s} が回路図の部品にありません"));
            };
            to.insert(ts, tp);
        }
        let mut moved = 0;
        for w in wires.iter_mut() {
            // read both ends before moving either: a wire between two traded pins swaps its ends
            let (a, b) = (w.a, w.b);
            if let Some(&t) = to.get(&a) {
                w.a = t;
                moved += 1;
            }
            if let Some(&t) = to.get(&b) {
                w.b = t;
                moved += 1;
            }
        }
        for l in labels.iter_mut() {
            if let Some(&t) = to.get(&l.pos) {
                l.pos = t;
                moved += 1;
            }
        }
        Ok(moved)
    }
}

/// The nets after the trades must be those before, with the pins of the traded parts renamed: pin S of a part becomes pin P.
fn check_same_nets(before: &Schematic, after: &Schematic, trades: &BTreeMap<String, BTreeMap<String, String>>) -> Result<(), String> {
    // (part, schematic pin S) -> the pin P that now has its net
    let renamed = |r: &str, pin: &str| -> String {
        trades.get(r).and_then(|m| m.iter().find(|(p, s)| s.as_str() == pin && p.as_str() != pin).map(|(p, _)| p.clone())).unwrap_or_else(|| pin.to_string())
    };
    let sets = |s: &Schematic, rename: bool| -> BTreeSet<BTreeSet<(String, String)>> {
        netlist::extract(s)
            .iter()
            .map(|n| n.pins.iter().map(|p| (p.refdes.clone(), if rename { renamed(&p.refdes, &p.pin) } else { p.pin.clone() })).collect::<BTreeSet<_>>())
            .filter(|set| set.len() > 1)
            .collect()
    };
    if sets(before, true) == sets(after, false) {
        Ok(())
    } else {
        Err("配線の端を動かすと、別のネットがつながったり切れたりします (入れ替えるピンの間を、ほかの配線が通っているためです)。回路図でピンの配線を描き直してから、基板の「回路図から取り込み」をしてください。回路図は変えていません".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire(s: &mut Schematic, a: Pt, b: Pt) {
        s.wires.push(Wire { a, b });
    }

    /// Two resistors R1 (pins at (8,10),(12,10)) and R2 (pins at (8,20),(12,20)), each with a label on its left pin.
    fn two() -> Schematic {
        let mut s = Schematic::default();
        s.add(Kind::Resistor, (10, 10), 0);
        s.add(Kind::Resistor, (10, 20), 0);
        s.labels.push(Label { pos: (8, 10), text: "A".into() });
        s.labels.push(Label { pos: (8, 20), text: "B".into() });
        s
    }

    fn net_of(s: &Schematic, r: &str, pin: &str) -> String {
        netlist::extract(&s.flat()).into_iter().find(|n| n.pins.iter().any(|p| p.refdes == r && p.pin == pin)).map(|n| n.name).unwrap_or_default()
    }

    #[test]
    fn labels_follow_a_traded_pin() {
        let mut s = two();
        // R1's pad 1 carries the net of pin 2 and the other way round: the trade of the two pins of a resistor
        let trades: BTreeMap<String, BTreeMap<String, String>> = [("R1".to_string(), [("1".to_string(), "2".to_string()), ("2".to_string(), "1".to_string())].into())].into();
        s.labels.push(Label { pos: (12, 10), text: "C".into() });
        assert_eq!((net_of(&s, "R1", "1"), net_of(&s, "R1", "2")), ("A".into(), "C".into()));
        let rep = s.apply_pin_trades(&trades).unwrap();
        assert_eq!(rep.parts, ["R1"]);
        assert_eq!(rep.moved, 2);
        assert_eq!((net_of(&s, "R1", "1"), net_of(&s, "R1", "2")), ("C".into(), "A".into()));
    }

    #[test]
    fn wires_follow_a_traded_pin_and_the_other_part_is_left_alone() {
        let mut s = two();
        s.labels.clear();
        // R1.1 -- R2.1 by a wire, R1.2 -- R2.2 by a wire (both pins to the same side, so the wires are straight lines between pin tips)
        wire(&mut s, (8, 10), (8, 20));
        wire(&mut s, (12, 10), (12, 20));
        s.labels.push(Label { pos: (8, 20), text: "X".into() });
        s.labels.push(Label { pos: (12, 20), text: "Y".into() });
        let trades: BTreeMap<String, BTreeMap<String, String>> = [("R1".to_string(), [("1".to_string(), "2".to_string()), ("2".to_string(), "1".to_string())].into())].into();
        let rep = s.apply_pin_trades(&trades).unwrap();
        assert_eq!(rep.moved, 2);
        // R1.1 is now on the wire that went to R2.2 (net Y), R1.2 on the one that went to R2.1 (X)
        assert_eq!((net_of(&s, "R1", "1"), net_of(&s, "R1", "2")), ("Y".into(), "X".into()));
        assert_eq!((net_of(&s, "R2", "1"), net_of(&s, "R2", "2")), ("X".into(), "Y".into()));
    }

    #[test]
    fn a_trade_that_is_not_a_loop_is_refused_and_changes_nothing() {
        let mut s = two();
        s.labels.clear();
        // R1.1 and R2.1 on one wire, and a wire of R1.2 that runs through the tip of R2.2: moving the ends would mix nets
        wire(&mut s, (8, 10), (8, 20));
        s.labels.push(Label { pos: (8, 20), text: "X".into() });
        s.labels.push(Label { pos: (12, 10), text: "Y".into() });
        // a trade that is not a loop (pin 1 takes the net of pin 2, nobody takes pin 1's) is refused
        let trades: BTreeMap<String, BTreeMap<String, String>> = [("R1".to_string(), [("1".to_string(), "2".to_string())].into())].into();
        let before = s.clone();
        assert!(s.apply_pin_trades(&trades).is_err());
        assert_eq!(s, before, "an error leaves the schematic as it was");
    }

    #[test]
    fn a_part_that_is_not_in_the_schematic_is_skipped() {
        let mut s = two();
        let trades: BTreeMap<String, BTreeMap<String, String>> = [("U9".to_string(), [("1".to_string(), "2".to_string()), ("2".to_string(), "1".to_string())].into())].into();
        let before = s.clone();
        assert_eq!(s.apply_pin_trades(&trades).unwrap(), BackAnnotation::default());
        assert_eq!(s, before);
    }

    #[test]
    fn a_not_connected_mark_follows_its_pin() {
        let mut s = two();
        s.labels.clear();
        s.set_no_connect("R1", "2", true);
        s.labels.push(Label { pos: (8, 10), text: "A".into() });
        let trades: BTreeMap<String, BTreeMap<String, String>> = [("R1".to_string(), [("1".to_string(), "2".to_string()), ("2".to_string(), "1".to_string())].into())].into();
        s.apply_pin_trades(&trades).unwrap();
        assert!(s.is_no_connect("R1", "1") && !s.is_no_connect("R1", "2"));
        assert_eq!(net_of(&s, "R1", "2"), "A");
    }
}
