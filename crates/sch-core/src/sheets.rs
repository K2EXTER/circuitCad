//! Several sheets in one schematic. The sheet being edited lives in the fields of `Schematic` itself
//! (`components`, `wires`, `labels`, `erc_skip`); the others are parked in `sheets` (the entry of the
//! active sheet is empty). Net labels and power symbols are global: the same name is the same net on every sheet.
//! The netlist, ERC and the simulation work on `flat()`, all sheets side by side in one big sheet.

use crate::*;
use std::borrow::Cow;

/// Grid steps between the sheets in `flat()`: far enough that wires of different sheets never touch.
pub const SHEET_STRIDE: i32 = 100_000;

/// A sheet that is not being edited (and, in `Schematic::sheets`, the place of the one that is: empty).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub name: String,
    #[serde(default)]
    pub components: Vec<Component>,
    #[serde(default)]
    pub wires: Vec<Wire>,
    #[serde(default)]
    pub labels: Vec<Label>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub erc_skip: Vec<(Pt, Pt)>,
}

/// The sheet number and the place on that sheet of a point of the flat sheet.
pub fn sheet_of(p: Pt) -> (usize, Pt) {
    let k = (p.0 + SHEET_STRIDE / 2).div_euclid(SHEET_STRIDE).max(0);
    (k as usize, (p.0 - k * SHEET_STRIDE, p.1))
}

impl Schematic {
    /// Number of sheets (1 for a sheet that was never split).
    pub fn sheet_count(&self) -> usize {
        self.sheets.len().max(1)
    }

    pub fn sheet_name(&self, k: usize) -> String {
        self.sheets.get(k).map(|s| s.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| format!("シート{}", k + 1))
    }

    /// All sheets as one: sheet `k` is moved `k * SHEET_STRIDE` to the right. A single sheet is returned as it is.
    pub fn flat(&self) -> Cow<'_, Schematic> {
        if self.sheets.is_empty() {
            return Cow::Borrowed(self);
        }
        let mut out = Schematic { components: vec![], wires: vec![], labels: vec![], erc_skip: vec![], sheets: vec![], active_sheet: 0, ..self.clone() };
        for k in 0..self.sheets.len() {
            let dx = k as i32 * SHEET_STRIDE;
            let mv = |p: Pt| (p.0 + dx, p.1);
            let (comps, wires, labels, skip) = if k == self.active_sheet {
                (&self.components, &self.wires, &self.labels, &self.erc_skip)
            } else {
                let s = &self.sheets[k];
                (&s.components, &s.wires, &s.labels, &s.erc_skip)
            };
            out.components.extend(comps.iter().map(|c| Component { pos: mv(c.pos), ..c.clone() }));
            out.wires.extend(wires.iter().map(|w| Wire { a: mv(w.a), b: mv(w.b) }));
            out.labels.extend(labels.iter().map(|l| Label { pos: mv(l.pos), ..l.clone() }));
            out.erc_skip.extend(skip.iter().map(|&(a, b)| (mv(a), mv(b))));
        }
        Cow::Owned(out)
    }

    /// Puts the editing fields away into the active sheet's place.
    fn park(&mut self) {
        let a = self.active_sheet;
        let s = &mut self.sheets[a];
        s.components = std::mem::take(&mut self.components);
        s.wires = std::mem::take(&mut self.wires);
        s.labels = std::mem::take(&mut self.labels);
        s.erc_skip = std::mem::take(&mut self.erc_skip);
    }

    /// Takes sheet `k` into the editing fields (they must be empty).
    fn unpark(&mut self, k: usize) {
        let s = &mut self.sheets[k];
        self.components = std::mem::take(&mut s.components);
        self.wires = std::mem::take(&mut s.wires);
        self.labels = std::mem::take(&mut s.labels);
        self.erc_skip = std::mem::take(&mut s.erc_skip);
        self.active_sheet = k;
    }

    /// Edit sheet `k` from now on. False when there is no such sheet.
    pub fn switch_sheet(&mut self, k: usize) -> bool {
        if k >= self.sheets.len() {
            return false;
        }
        if k != self.active_sheet {
            self.park();
            self.unpark(k);
        }
        true
    }

    /// A new empty sheet, which becomes the one being edited. Returns its number.
    pub fn add_sheet(&mut self) -> usize {
        if self.sheets.is_empty() {
            self.sheets.push(Sheet { name: "シート1".into(), ..Default::default() });
            self.active_sheet = 0;
        }
        let k = self.sheets.len();
        self.sheets.push(Sheet { name: format!("シート{}", k + 1), ..Default::default() });
        self.switch_sheet(k);
        k
    }

    pub fn rename_sheet(&mut self, k: usize, name: &str) {
        if let Some(s) = self.sheets.get_mut(k) {
            s.name = name.to_string();
        }
    }

    /// Deletes sheet `k` with everything on it. False when there is no such sheet or it is the only one.
    /// With one sheet left the schematic is a plain single sheet again.
    pub fn remove_sheet(&mut self, k: usize) -> bool {
        if self.sheets.len() < 2 || k >= self.sheets.len() {
            return false;
        }
        if k == self.active_sheet {
            self.components.clear();
            self.wires.clear();
            self.labels.clear();
            self.erc_skip.clear();
            self.sheets.remove(k);
            self.unpark(k.min(self.sheets.len() - 1));
        } else {
            self.sheets.remove(k);
            if k < self.active_sheet {
                self.active_sheet -= 1;
            }
        }
        if self.sheets.len() == 1 {
            self.sheets.clear();
            self.active_sheet = 0;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_sheets() -> Schematic {
        let mut s = Schematic::default();
        let r = s.add(Kind::Resistor, (10, 10), 0);
        let pin = s.components[r].pins()[0].1;
        s.labels.push(Label { pos: pin, text: "X".into() });
        s.add_sheet();
        let r2 = s.add(Kind::Resistor, (10, 10), 0);
        let pin2 = s.components[r2].pins()[0].1;
        s.labels.push(Label { pos: pin2, text: "X".into() });
        s
    }

    #[test]
    fn switching_keeps_each_sheet_and_numbers_stay_unique() {
        let mut s = two_sheets();
        assert_eq!(s.sheet_count(), 2);
        assert_eq!(s.active_sheet, 1);
        assert_eq!(s.components[0].refdes, "R2", "the number continues over the sheets");
        s.switch_sheet(0);
        assert_eq!(s.components.len(), 1);
        assert_eq!(s.components[0].refdes, "R1");
        assert_eq!(s.flat().components.len(), 2);
    }

    #[test]
    fn the_same_label_joins_the_nets_of_two_sheets() {
        let s = two_sheets();
        let nets = extract(&s);
        let x = nets.iter().find(|n| n.name == "X").expect("net X");
        assert_eq!(x.pins.len(), 2, "one pin from each sheet: {nets:?}");
    }

    #[test]
    fn findings_come_back_in_the_coordinates_of_their_sheet() {
        let s = two_sheets();
        let ps = erc(&s);
        assert!(ps.iter().any(|p| p.sheet == 0 && p.at.is_some()));
        assert!(ps.iter().any(|p| p.sheet == 1 && p.at.is_some_and(|a| a.0.abs() < 1000)));
    }

    #[test]
    fn deleting_down_to_one_sheet_makes_a_plain_schematic() {
        let mut s = two_sheets();
        assert!(s.remove_sheet(1));
        assert!(s.sheets.is_empty() && s.active_sheet == 0);
        assert_eq!(s.components.len(), 1);
        assert!(!s.remove_sheet(0));
    }

    #[test]
    fn sheets_round_trip_through_json() {
        let mut s = two_sheets();
        s.switch_sheet(0);
        let back: Schematic = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        assert!(!serde_json::to_string(&Schematic::default()).unwrap().contains("sheets"));
    }
}
